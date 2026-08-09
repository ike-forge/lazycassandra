//! The one place state changes.
//!
//! `update` is synchronous, pure, and total. It mutates state and *describes*
//! what should happen next; it cannot open a socket because it has nothing to
//! open one with (architecture.md §3).

use crate::contents::TableRef;
use crate::model::{ConnId, DbError, Key, KeyPress, Page, PagingState, QueryId, Schema, Statement};
use crate::posture::Posture;
use crate::state::{ConnectionState, Cursor, Geometry, Overlay, Panel, SchemaState, State};
use crate::style::Severity;
use crate::tree::Node;

/// How many rows one look at a table brings back.
///
/// A wall rather than a window until paging lands
/// (doc/specs/2026-08-09-table-contents-design.md §3).
pub const ROW_LIMIT: usize = 200;

/// Everything that can happen. Async results re-enter here through the same
/// channel as a keystroke.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Input(KeyPress),
    /// The terminal was resized. Carries no geometry — the renderer reads the
    /// real size from `frame.area()` — it exists only to trigger a repaint.
    Resized,
    /// What the frame just drawn turned out to be. The renderer's one report
    /// back into the domain (design §3).
    Measured(Geometry),
    Tick,
    Connected {
        conn: ConnId,
        posture: Posture,
    },
    ConnectFailed {
        conn: ConnId,
        error: DbError,
    },
    SchemaFailed {
        conn: ConnId,
        error: DbError,
    },
    SchemaLoaded {
        conn: ConnId,
        schema: Box<Schema>,
    },
    PageReady {
        query: QueryId,
        page: Box<Page>,
    },
    Failed {
        query: QueryId,
        error: DbError,
    },
    Quit,
}

/// What `update` decided should happen. Interpreted by the effect runtime in
/// `lc-app`, never here.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Connect(ConnId),
    LoadSchema(ConnId),
    Execute {
        conn: ConnId,
        query: QueryId,
        stmt: Box<Statement>,
    },
    FetchNextPage {
        conn: ConnId,
        query: QueryId,
        cursor: PagingState,
    },
    Cancel(QueryId),
    RecordHistory(Box<Statement>),
    Quit,
}

/// The state the application starts in, and what it must do first.
///
/// TEA's `init`: the startup connection is *described* here rather than
/// performed behind the domain's back, so the first frame can already say
/// "Connecting…" (architecture.md §3).
#[must_use]
pub fn init(conn: ConnId) -> (State, Vec<Effect>) {
    let state = State {
        connection: ConnectionState::Connecting(conn),
        dirty: true,
        ..State::default()
    };

    (state, vec![Effect::Connect(conn)])
}

/// Applies a message and returns the effects it implies.
pub fn update(state: &mut State, msg: Msg) -> Vec<Effect> {
    let subject = described(state);
    let effects = apply(state, msg);

    // The Ddl offset belongs to the node being described, so a new subject
    // opens at its top rather than halfway down the last one. Stated once here
    // rather than at each of the four paths that can move the tree selection.
    if described(state) != subject {
        state.ddl.to_start();
    }

    effects
}

/// What the Ddl panel is describing, in a form that survives the mutation it
/// is compared across — [`Node`] borrows the schema.
///
/// Unfolding is deliberately not part of the identity: it leaves the keyspace
/// selected and the panel on the same subject, so it must not scroll it.
#[derive(PartialEq, Eq)]
enum Described {
    Keyspace(String),
    Table { keyspace: String, table: String },
}

fn described(state: &State) -> Option<Described> {
    let schema = state.schema()?;

    Some(match state.tables.selected(schema)? {
        Node::Keyspace { keyspace, .. } => Described::Keyspace(keyspace.name.clone()),
        Node::Table { keyspace, table } => Described::Table {
            keyspace: keyspace.name.clone(),
            table: table.name.clone(),
        },
    })
}

fn apply(state: &mut State, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Tick => Vec::new(),

        Msg::Input(press) => handle_key(state, press),

        Msg::Resized => {
            state.mark_dirty();
            Vec::new()
        }

        // No unconditional `mark_dirty`: this message describes the frame
        // that produced it, so redrawing on every one of them would
        // re-measure and redraw forever (design §3).
        Msg::Measured(geometry) => {
            state.geometry = geometry;

            let before = state.ddl;
            state
                .ddl
                .clamp_to(geometry.ddl_lines(), geometry.height(Panel::Ddl));

            if state.ddl != before {
                state.mark_dirty();
            }
            Vec::new()
        }

        Msg::Connected { conn, posture } => {
            if !state.connection.is_current(conn) {
                return Vec::new();
            }
            state.connection = ConnectionState::Connected(crate::state::Connection {
                id: conn,
                posture,
                schema: SchemaState::Loading,
            });
            state.mark_dirty();
            vec![Effect::LoadSchema(conn)]
        }

        Msg::ConnectFailed { conn, error } => {
            if !state.connection.is_current(conn) {
                return Vec::new();
            }
            state.overlay = Some(Overlay {
                title: "Connection failed".to_owned(),
                message: error.to_string(),
                severity: Severity::Danger,
            });
            state.connection = ConnectionState::Failed(error);
            state.mark_dirty();
            Vec::new()
        }

        Msg::SchemaFailed { conn, error } => {
            let applied = match &mut state.connection {
                ConnectionState::Connected(connection) if connection.id == conn => {
                    connection.schema = SchemaState::Failed(error.clone());
                    true
                }
                _ => false,
            };
            if applied {
                state.overlay = Some(Overlay {
                    title: "Schema unavailable".to_owned(),
                    message: error.to_string(),
                    severity: Severity::Danger,
                });
                state.mark_dirty();
            }
            Vec::new()
        }

        Msg::SchemaLoaded { conn, schema } => {
            let applied = match &mut state.connection {
                ConnectionState::Connected(connection) if connection.id == conn => {
                    connection.schema = SchemaState::Loaded(*schema);
                    true
                }
                _ => false,
            };
            if applied {
                let rows = tree_rows(state);
                state.tables.cursor.clamp_to(rows);
                state.mark_dirty();
            }
            Vec::new()
        }

        // A result whose id is no longer in flight belongs to a cancelled
        // query, and applying it would resurrect a view the user left. The
        // panel applies the second gate: whether the result is *its* result.
        Msg::PageReady { query, page } => {
            if state.in_flight.remove(&query).is_some() {
                state.contents.page_ready(query, *page);
                state.mark_dirty();
            }
            Vec::new()
        }

        // No overlay, unlike `SchemaFailed`: a schema failure disables the
        // application, a failed browse disables one panel, and an overlay per
        // mistyped table is the warning noise vision.md §8 flags.
        Msg::Failed { query, error } => {
            if state.in_flight.remove(&query).is_some() {
                state.contents.failed(query, error);
                state.mark_dirty();
            }
            Vec::new()
        }

        Msg::Quit => quit(state),
    }
}

fn quit(state: &mut State) -> Vec<Effect> {
    state.should_quit = true;
    let cancels = state.in_flight.keys().copied().map(Effect::Cancel);

    cancels.chain(std::iter::once(Effect::Quit)).collect()
}

#[derive(Clone, Copy)]
enum Motion {
    Up,
    Down,
    PageUp,
    PageDown,
    Start,
    End,
}

fn handle_key(state: &mut State, press: KeyPress) -> Vec<Effect> {
    state.mark_dirty();

    // ctrl and alt turn a key into a separate binding, refused here rather
    // than reinterpreted. shift is not checked: a shifted letter already
    // arrives as a different Key::Char, and a shifted arrow is left as a
    // synonym for the plain one so it stays free for a future "extend
    // selection" binding.
    if press.modifiers.ctrl || press.modifiers.alt {
        return Vec::new();
    }

    // Focus is the mode. A panel that takes text claims every printable key
    // and the editing keys; what it does not claim keeps the global meaning
    // it has everywhere else (design §3). This is the rule shortcuts.md §2
    // said the CQL editor would have to break.
    if state.focus.takes_text() && edit(state, press.key) {
        return Vec::new();
    }

    match press.key {
        Key::Char('q') => return quit(state),
        Key::Escape => escape(state),
        Key::Tab => state.focus = state.focus.next(),
        Key::BackTab => state.focus = state.focus.previous(),
        Key::Char('j') | Key::Down => scroll(state, Motion::Down),
        Key::Char('k') | Key::Up => scroll(state, Motion::Up),
        Key::PageDown => scroll(state, Motion::PageDown),
        Key::PageUp => scroll(state, Motion::PageUp),
        Key::Home => scroll(state, Motion::Start),
        Key::End => scroll(state, Motion::End),
        Key::Enter | Key::Char(' ') => return activate(state),
        Key::Char('l') | Key::Right => transition(state, Transition::Open),
        Key::Char('h') | Key::Left => transition(state, Transition::Close),
        // Last, so every binding above keeps precedence over a panel key.
        Key::Char(key) => {
            if let Some(panel) = Panel::from_shortcut(key) {
                state.focus = panel;
            }
        }
        _ => {}
    }

    Vec::new()
}

/// Offers `key` to the focused panel's buffer, answering whether it was text.
///
/// Exhaustive over [`Key`], so a new key has to decide whether an editor
/// claims it rather than defaulting into one side.
fn edit(state: &mut State, key: Key) -> bool {
    let height = state.geometry.height(Panel::Queries);
    let editor = &mut state.queries;

    match key {
        Key::Char(character) => editor.insert(character),
        Key::Enter => editor.insert_newline(),
        Key::Backspace => editor.backspace(),
        Key::Delete => editor.delete(),
        Key::Left => editor.left(),
        Key::Right => editor.right(),
        Key::Up => editor.up(),
        Key::Down => editor.down(),
        Key::Home => editor.line_start(),
        Key::End => editor.line_end(),
        Key::PageUp => editor.page_up(height),
        Key::PageDown => editor.page_down(height),
        Key::Escape | Key::Tab | Key::BackTab | Key::Insert | Key::F(_) => return false,
    }

    true
}

/// `Esc` dismisses the overlay when there is one, and otherwise leaves a panel
/// that takes text — the one key that restores every letter binding at once
/// (design §3). One press does one of the two, never both.
fn escape(state: &mut State) {
    if state.overlay.take().is_some() {
        return;
    }

    if state.focus.takes_text() {
        state.focus = Panel::default();
    }
}

#[derive(Clone, Copy)]
enum Transition {
    Open,
    Close,
}

/// Applies `motion` to whatever the focused panel moves: a selection in
/// Tables and Contents, a viewport in Ddl.
fn scroll(state: &mut State, motion: Motion) {
    let height = state.geometry.height(state.focus);

    if state.focus == Panel::Ddl {
        let len = state.geometry.ddl_lines();

        match motion {
            Motion::Down => state.ddl.down(len, height),
            Motion::Up => state.ddl.up(),
            Motion::PageDown => state.ddl.page_down(len, height),
            Motion::PageUp => state.ddl.page_up(height),
            Motion::Start => state.ddl.to_start(),
            Motion::End => state.ddl.to_end(len, height),
        }
        return;
    }

    let len = focused_row_count(state);

    let Some(cursor) = focused_cursor(state) else {
        return;
    };

    match motion {
        Motion::Down => cursor.down(len),
        Motion::Up => cursor.up(),
        Motion::PageDown => cursor.page_down(len, height),
        Motion::PageUp => cursor.page_up(height),
        Motion::Start => cursor.to_start(),
        Motion::End => cursor.to_end(len),
    }
}

/// How many rows the focused panel's *cursor* moves over. Ddl scrolls a
/// viewport rather than a selection and never reaches here; Queries never
/// arrives here either — the editor claims the motion keys first (design
/// §3), so this arm is unreachable in practice and kept for exhaustiveness.
fn focused_row_count(state: &State) -> usize {
    match state.focus {
        Panel::Tables => tree_rows(state),
        Panel::Contents => state.contents.row_count(),
        Panel::Ddl | Panel::Queries => 0,
    }
}

/// Queries never arrives here — the editor claims the motion keys before
/// this match is reached (design §3) — so its arm is unreachable in
/// practice and kept for exhaustiveness.
fn focused_cursor(state: &mut State) -> Option<&mut Cursor> {
    match state.focus {
        Panel::Tables => Some(&mut state.tables.cursor),
        Panel::Contents => Some(&mut state.contents.cursor),
        Panel::Ddl | Panel::Queries => None,
    }
}

/// `Enter` and `Space`: unfold a keyspace, or read a table.
fn activate(state: &mut State) -> Vec<Effect> {
    if state.focus != Panel::Tables {
        return Vec::new();
    }

    let table = {
        let Some((tables, schema)) = state.tables_and_schema() else {
            return Vec::new();
        };

        match tables.selected(schema) {
            Some(Node::Keyspace { .. }) => {
                tables.toggle(schema);
                return Vec::new();
            }
            Some(Node::Table { keyspace, table }) => TableRef::new(&keyspace.name, &table.name),
            None => return Vec::new(),
        }
    };

    read(state, table)
}

/// Replaces whatever the Contents panel is showing with a read of `table`.
fn read(state: &mut State, table: TableRef) -> Vec<Effect> {
    let mut effects = Vec::new();

    // Dropping the id is what actually stops a late page from being applied;
    // the effect is still inert (architecture.md §3).
    if let Some(superseded) = state.contents.outstanding() {
        state.in_flight.remove(&superseded);
        effects.push(Effect::Cancel(superseded));
    }

    let stmt = Statement::select_all(&table.keyspace, &table.table, ROW_LIMIT);
    match submit(state, stmt) {
        Ok((query, submitted)) => {
            state.contents.requested(query, table);
            effects.extend(submitted);
        }
        Err(error) => state.contents.refused(table, error),
    }

    effects
}

fn transition(state: &mut State, transition: Transition) {
    if state.focus != Panel::Tables {
        return;
    }

    let Some((tables, schema)) = state.tables_and_schema() else {
        return;
    };

    match transition {
        Transition::Open => tables.open(schema),
        Transition::Close => tables.close(schema),
    }
}

fn tree_rows(state: &State) -> usize {
    state
        .schema()
        .map_or(0, |schema| state.tables.rows(schema).len())
}

/// Turns a statement into an effect, or refuses.
///
/// The posture gate lives here rather than in the TUI because the TUI does not
/// construct effects — there is no path around this one (architecture.md §4).
///
/// Returns the [`QueryId`] alongside the effects, so a panel can record which
/// result is the one it is waiting for.
///
/// # Errors
///
/// [`DbError::ReadOnly`] if the statement mutates and the connection's posture
/// does not permit it, and [`DbError::Unreachable`] if there is no connection
/// to run it on. Nothing is recorded and no effect is emitted.
pub fn submit(state: &mut State, stmt: Statement) -> Result<(QueryId, Vec<Effect>), DbError> {
    if stmt.mutates() && !state.posture().permits_mutation() {
        return Err(DbError::ReadOnly);
    }

    let Some(conn) = state.conn_id() else {
        return Err(DbError::Unreachable("not connected".to_owned()));
    };

    let query = QueryId::next();
    state.in_flight.insert(query, stmt.clone());
    state.mark_dirty();

    Ok((
        query,
        vec![
            Effect::Execute {
                conn,
                query,
                stmt: Box::new(stmt.clone()),
            },
            Effect::RecordHistory(Box::new(stmt)),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Modifiers, StatementKind};
    use crate::state::Connection;
    use crate::tree::tests_support::schema_of as tabled_schema;

    fn schema_of(names: &[&str]) -> Schema {
        tabled_schema(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
    }

    fn connected(posture: Posture) -> State {
        State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture,
                schema: SchemaState::Loading,
            }),
            ..State::default()
        }
    }

    fn current_conn(state: &State) -> ConnId {
        let ConnectionState::Connected(connection) = &state.connection else {
            panic!("the helper builds a connected state");
        };
        connection.id
    }

    fn statement(kind: StatementKind) -> Statement {
        Statement {
            cql: "SELECT * FROM ks.t".to_owned(),
            kind,
        }
    }

    #[test]
    fn read_only_connection_refuses_a_mutating_statement() {
        let mut state = connected(Posture::ReadOnly);

        let result = submit(&mut state, statement(StatementKind::Write));

        assert_eq!(result, Err(DbError::ReadOnly));
        assert!(
            state.in_flight.is_empty(),
            "a refused statement must not be registered as in flight"
        );
    }

    #[test]
    fn a_connection_with_no_posture_is_read_only() {
        let mut state = State::default();

        assert_eq!(state.posture(), Posture::ReadOnly);
        assert_eq!(
            submit(&mut state, statement(StatementKind::Write)),
            Err(DbError::ReadOnly)
        );
    }

    #[test]
    fn reads_are_permitted_on_a_read_only_connection() {
        let mut state = connected(Posture::ReadOnly);

        let (_, effects) =
            submit(&mut state, statement(StatementKind::Read)).expect("read is allowed");

        assert_eq!(effects.len(), 2, "expected Execute and RecordHistory");
        assert_eq!(state.in_flight.len(), 1);
    }

    #[test]
    fn writable_connection_executes_and_records_a_mutating_statement() {
        let mut state = connected(Posture::Writable);
        let conn = current_conn(&state);

        let (query, effects) =
            submit(&mut state, statement(StatementKind::Write)).expect("write is allowed");

        assert_eq!(
            state.in_flight.keys().copied().collect::<Vec<_>>(),
            vec![query],
            "the id handed back is the one registered as in flight"
        );
        assert_eq!(
            effects,
            vec![
                Effect::Execute {
                    conn,
                    query,
                    stmt: Box::new(statement(StatementKind::Write)),
                },
                Effect::RecordHistory(Box::new(statement(StatementKind::Write))),
            ]
        );
    }

    #[test]
    fn a_statement_with_no_connection_to_run_it_on_is_refused() {
        let mut state = State::default();

        assert_eq!(
            submit(&mut state, statement(StatementKind::Read)),
            Err(DbError::Unreachable("not connected".to_owned()))
        );
        assert!(state.in_flight.is_empty());
    }

    #[test]
    fn a_result_for_a_cancelled_query_is_dropped() {
        let mut state = connected(Posture::ReadOnly);
        let cancelled = QueryId::next();

        let effects = update(
            &mut state,
            Msg::Failed {
                query: cancelled,
                error: DbError::Timeout,
            },
        );

        assert!(effects.is_empty());
        assert!(
            !state.dirty,
            "a late result for a query nobody is waiting on must not force a redraw"
        );
    }

    #[test]
    fn quitting_cancels_everything_still_in_flight() {
        let mut state = connected(Posture::Writable);
        let (query, _) =
            submit(&mut state, statement(StatementKind::Write)).expect("write is allowed");

        let effects = update(&mut state, Msg::Quit);

        assert!(state.should_quit);
        assert_eq!(effects, vec![Effect::Cancel(query), Effect::Quit]);
    }

    #[test]
    fn a_resize_marks_the_state_dirty_and_emits_no_effects() {
        let mut state = State::default();

        let effects = update(&mut state, Msg::Resized);

        assert!(
            effects.is_empty(),
            "a resize is not an effect-producing event"
        );
        assert!(state.dirty, "a resize must force the next frame to repaint");
    }

    #[test]
    fn schema_for_a_stale_connection_is_ignored() {
        let mut state = connected(Posture::ReadOnly);

        let effects = update(
            &mut state,
            Msg::SchemaLoaded {
                conn: ConnId::next(),
                schema: Box::new(Schema::default()),
            },
        );

        assert!(effects.is_empty());
        let ConnectionState::Connected(connection) = state.connection else {
            panic!("the connection is still present");
        };
        assert!(
            matches!(connection.schema, SchemaState::Loading),
            "schema from a connection we are no longer on must not be applied"
        );
    }

    #[test]
    fn the_application_starts_by_connecting() {
        let conn = ConnId::next();

        let (state, effects) = init(conn);

        assert_eq!(effects, vec![Effect::Connect(conn)]);
        assert!(state.connection.is_current(conn));
        assert!(state.dirty, "the first frame must be painted");
    }

    #[test]
    fn connecting_successfully_asks_for_the_schema() {
        let conn = ConnId::next();
        let (mut state, _) = init(conn);

        let effects = update(
            &mut state,
            Msg::Connected {
                conn,
                posture: Posture::ReadOnly,
            },
        );

        assert_eq!(effects, vec![Effect::LoadSchema(conn)]);
        assert!(state.schema().is_none(), "the schema is still loading");
    }

    #[test]
    fn a_connected_message_for_a_stale_attempt_is_ignored() {
        let (mut state, _) = init(ConnId::next());

        let effects = update(
            &mut state,
            Msg::Connected {
                conn: ConnId::next(),
                posture: Posture::ReadOnly,
            },
        );

        assert!(effects.is_empty());
        assert!(
            matches!(state.connection, ConnectionState::Connecting(_)),
            "a Connected message for a connection we are no longer waiting on must not be applied"
        );
    }

    #[test]
    fn a_failed_connection_is_recorded_and_raised() {
        let conn = ConnId::next();
        let (mut state, _) = init(conn);

        let effects = update(
            &mut state,
            Msg::ConnectFailed {
                conn,
                error: DbError::Unreachable("no route".to_owned()),
            },
        );

        assert!(effects.is_empty());
        assert!(matches!(state.connection, ConnectionState::Failed(_)));
        let overlay = state.overlay.expect("a failure is shown");
        assert_eq!(overlay.severity, Severity::Danger);
        assert!(overlay.message.contains("no route"));
    }

    #[test]
    fn a_failed_connection_for_a_stale_attempt_is_dropped() {
        let (mut state, _) = init(ConnId::next());

        update(
            &mut state,
            Msg::ConnectFailed {
                conn: ConnId::next(),
                error: DbError::Timeout,
            },
        );

        assert!(state.overlay.is_none());
        assert!(matches!(state.connection, ConnectionState::Connecting(_)));
    }

    #[test]
    fn a_failed_schema_is_recorded_and_raised() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);

        update(
            &mut state,
            Msg::SchemaFailed {
                conn,
                error: DbError::Invalid("unauthorized".to_owned()),
            },
        );

        assert!(state.overlay.is_some());
        let ConnectionState::Connected(connection) = &state.connection else {
            panic!("still connected");
        };
        assert!(matches!(connection.schema, SchemaState::Failed(_)));
    }

    #[test]
    fn a_failed_schema_for_a_stale_connection_is_dropped() {
        let mut state = connected(Posture::ReadOnly);

        update(
            &mut state,
            Msg::SchemaFailed {
                conn: ConnId::next(),
                error: DbError::Invalid("unauthorized".to_owned()),
            },
        );

        assert!(state.overlay.is_none(), "a stale failure must not be shown");
        let ConnectionState::Connected(connection) = &state.connection else {
            panic!("still connected");
        };
        assert!(
            matches!(connection.schema, SchemaState::Loading),
            "schema failure from a connection we are no longer on must not be applied"
        );
    }

    fn browsing(keyspaces: &[&str]) -> State {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(keyspaces)),
            },
        );
        state
    }

    fn press(state: &mut State, key: Key) -> Vec<Effect> {
        update(state, Msg::Input(KeyPress::new(key)))
    }

    #[test]
    fn j_and_k_walk_the_keyspace_list() {
        let mut state = browsing(&["a", "b", "c"]);

        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), 2);

        press(&mut state, Key::Char('k'));

        assert_eq!(state.tables.cursor.index(), 1);
    }

    #[test]
    fn the_arrow_keys_do_what_j_and_k_do() {
        let mut state = browsing(&["a", "b"]);

        press(&mut state, Key::Down);

        assert_eq!(state.tables.cursor.index(), 1);

        press(&mut state, Key::Up);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn the_selection_cannot_run_off_a_list_it_has_not_loaded() {
        let mut state = connected(Posture::ReadOnly);

        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "there are no rows to move onto"
        );
    }

    #[test]
    fn motion_belongs_to_the_focused_panel_only() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;

        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn escape_dismisses_the_overlay() {
        let mut state = browsing(&["a"]);
        state.overlay = Some(Overlay {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        });

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
    }

    #[test]
    fn q_quits() {
        let mut state = browsing(&["a"]);

        let effects = press(&mut state, Key::Char('q'));

        assert!(state.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);
    }

    #[test]
    fn a_modified_key_is_not_a_binding() {
        let mut state = browsing(&["a", "b"]);

        update(
            &mut state,
            Msg::Input(KeyPress {
                key: Key::Char('j'),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
            }),
        );

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "ctrl-j is a different binding, not a slower j"
        );
    }

    /// A state focused on Queries with `text` typed into it, `\n` meaning
    /// Enter — the same convention `editor.rs`'s own helper uses.
    fn editing(text: &str) -> State {
        let mut state = browsing(&["a"]);
        state.focus = Panel::Queries;
        for character in text.chars() {
            press(
                &mut state,
                if character == '\n' {
                    Key::Enter
                } else {
                    Key::Char(character)
                },
            );
        }
        state
    }

    #[test]
    fn a_letter_typed_in_queries_is_text_not_a_shortcut() {
        // Every letter here is bound somewhere: t/a/c/u focus a panel, j/k/h/l
        // move, q quits. In the editor they are all just text.
        let state = editing("tacujkhlq");

        assert_eq!(state.focus, Panel::Queries, "no letter moved focus");
        assert_eq!(state.queries.text(), "tacujkhlq");
    }

    #[test]
    fn q_does_not_quit_while_the_editor_has_focus() {
        let mut state = browsing(&["a"]);
        state.focus = Panel::Queries;

        let effects = press(&mut state, Key::Char('q'));

        assert!(!state.should_quit);
        assert_eq!(effects, Vec::new());
        assert_eq!(state.queries.text(), "q");
    }

    #[test]
    fn escape_leaves_the_editor_for_tables() {
        let mut state = editing("SELECT");

        press(&mut state, Key::Escape);

        assert_eq!(state.focus, Panel::Tables);
        assert_eq!(state.queries.text(), "SELECT", "leaving keeps the buffer");
    }

    #[test]
    fn escape_dismisses_the_overlay_before_it_leaves_the_editor() {
        let mut state = editing("SELECT");
        state.overlay = Some(Overlay {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        });

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
        assert_eq!(state.focus, Panel::Queries, "one esc does one thing");
    }

    #[test]
    fn tab_still_leaves_the_editor() {
        let mut state = editing("SELECT");

        press(&mut state, Key::Tab);

        assert_eq!(state.focus, Panel::Tables, "Queries wraps to Tables");
        assert_eq!(state.queries.text(), "SELECT");
    }

    #[test]
    fn a_modified_key_is_still_not_text() {
        let mut state = browsing(&["a"]);
        state.focus = Panel::Queries;

        update(
            &mut state,
            Msg::Input(KeyPress {
                key: Key::Char('a'),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
            }),
        );

        assert_eq!(
            state.queries.text(),
            "",
            "ctrl-a is a binding that does not exist yet, not a slower a"
        );
    }

    #[test]
    fn typing_outside_queries_still_focuses_a_panel() {
        let mut state = browsing(&["a"]);

        press(&mut state, Key::Char('c'));

        assert_eq!(state.focus, Panel::Contents);
        assert_eq!(state.queries.text(), "", "nothing leaked into the buffer");
    }

    #[test]
    fn the_editing_keys_reach_the_buffer() {
        let mut state = editing("SELECT *");

        press(&mut state, Key::Backspace);
        assert_eq!(state.queries.text(), "SELECT ");

        press(&mut state, Key::Enter);
        assert_eq!(state.queries.text(), "SELECT \n");

        press(&mut state, Key::Left);
        press(&mut state, Key::Delete);
        assert_eq!(state.queries.text(), "SELECT ", "delete ate the newline");
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_line_in_the_editor() {
        let mut state = editing("SELECT *\nFROM t");

        press(&mut state, Key::Home);
        assert_eq!(
            state.queries.cursor(),
            crate::editor::Position { line: 1, column: 0 }
        );

        press(&mut state, Key::End);
        assert_eq!(
            state.queries.cursor(),
            crate::editor::Position { line: 1, column: 6 },
            "the line's end, not the buffer's"
        );
    }

    #[test]
    fn a_loaded_schema_pulls_the_selection_back_inside_it() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        for _ in 0..5 {
            state.tables.cursor.down(6);
        }

        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(&["shop", "typezoo"])),
            },
        );

        assert_eq!(
            state.tables.cursor.index(),
            1,
            "a selection past the end of a smaller schema must be pulled in"
        );
    }

    fn browsing_tables(keyspaces: &[(&str, &[&str])]) -> State {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(keyspaces)),
            },
        );
        state
    }

    fn row_count(state: &State) -> usize {
        state
            .schema()
            .map_or(0, |schema| state.tables.rows(schema).len())
    }

    #[test]
    fn enter_and_space_both_toggle_a_keyspace() {
        for key in [Key::Enter, Key::Char(' ')] {
            let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

            let effects = press(&mut state, key);
            assert_eq!(row_count(&state), 3, "{key:?} did not unfold");
            assert!(
                effects.is_empty(),
                "unfolding is a pure state change and must emit no effect (table-list design §2)"
            );

            let effects = press(&mut state, key);
            assert_eq!(row_count(&state), 1, "{key:?} did not fold");
            assert!(
                effects.is_empty(),
                "folding is a pure state change and must emit no effect (table-list design §2)"
            );
        }
    }

    #[test]
    fn l_and_right_open_a_keyspace() {
        for key in [Key::Char('l'), Key::Right] {
            let mut state = browsing_tables(&[("shop", &["orders"])]);

            let effects = press(&mut state, key);

            assert_eq!(row_count(&state), 2, "{key:?} did not open");
            assert!(
                effects.is_empty(),
                "opening is a pure state change and must emit no effect (table-list design §2)"
            );
        }
    }

    #[test]
    fn h_and_left_close_a_keyspace() {
        for key in [Key::Char('h'), Key::Left] {
            let mut state = browsing_tables(&[("shop", &["orders"])]);
            press(&mut state, Key::Char('l'));

            let effects = press(&mut state, key);

            assert_eq!(row_count(&state), 1, "{key:?} did not close");
            assert!(
                effects.is_empty(),
                "closing is a pure state change and must emit no effect (table-list design §2)"
            );
        }
    }

    #[test]
    fn h_from_a_table_steps_out_to_its_keyspace_before_folding() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        assert_eq!(state.tables.cursor.index(), 1);

        press(&mut state, Key::Char('h'));

        assert_eq!(state.tables.cursor.index(), 0);
        assert_eq!(row_count(&state), 3, "stepping out must not fold");
    }

    #[test]
    fn enter_on_a_table_leaves_the_tree_where_it_is() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        let before = state.tables.clone();

        press(&mut state, Key::Enter);

        assert_eq!(
            state.tables, before,
            "reading a table must not disturb the tree the cursor is in"
        );
    }

    #[test]
    fn j_walks_into_an_unfolded_keyspaces_tables() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"]), ("typezoo", &[])]);
        press(&mut state, Key::Char('l'));

        for _ in 0..3 {
            press(&mut state, Key::Char('j'));
        }

        assert_eq!(
            state.tables.cursor.index(),
            3,
            "motion counts tree rows, not keyspaces"
        );
    }

    #[test]
    fn a_tree_transition_belongs_to_the_focused_panel_only() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus = Panel::Ddl;

        press(&mut state, Key::Char('l'));

        assert_eq!(row_count(&state), 1);
    }

    #[test]
    fn tab_walks_the_panels_in_reading_order_and_wraps() {
        let mut state = browsing(&["a"]);

        for expected in Panel::ALL.into_iter().skip(1).chain([Panel::ALL[0]]) {
            press(&mut state, Key::Tab);
            assert_eq!(state.focus, expected);
        }
    }

    #[test]
    fn shift_tab_walks_them_the_other_way() {
        let mut state = browsing(&["a"]);

        press(&mut state, Key::BackTab);

        assert_eq!(
            state.focus,
            Panel::Queries,
            "stepping back from the first wraps"
        );

        press(&mut state, Key::BackTab);

        assert_eq!(state.focus, Panel::Contents);
    }

    #[test]
    fn a_shortcut_focuses_its_panel_from_any_panel_that_does_not_take_text() {
        for from in Panel::ALL.into_iter().filter(|panel| !panel.takes_text()) {
            for to in Panel::ALL {
                let mut state = State {
                    focus: from,
                    ..State::default()
                };

                press(&mut state, Key::Char(to.shortcut()));

                assert_eq!(
                    state.focus,
                    to,
                    "'{}' should reach {to:?} from {from:?}",
                    to.shortcut()
                );
            }
        }
    }

    #[test]
    fn a_shortcut_is_text_in_a_panel_that_takes_text() {
        for to in Panel::ALL {
            let mut state = State {
                focus: Panel::Queries,
                ..State::default()
            };

            press(&mut state, Key::Char(to.shortcut()));

            assert_eq!(state.focus, Panel::Queries, "focus is the mode");
            assert_eq!(state.queries.text(), to.shortcut().to_string());
        }
    }

    /// Also the collision test: a shortcut that an earlier arm already answers
    /// would never reach the focus arm, and the focus assertion would fail.
    #[test]
    fn a_shortcut_moves_focus_and_does_nothing_else() {
        for panel in Panel::ALL {
            let mut state = browsing(&["a", "b", "c"]);
            press(&mut state, Key::Char('j'));
            let cursor = state.tables.cursor;

            let effects = press(&mut state, Key::Char(panel.shortcut()));

            assert_eq!(state.focus, panel);
            assert!(
                effects.is_empty(),
                "'{}' issued {effects:?}",
                panel.shortcut()
            );
            assert!(!state.should_quit, "'{}' quit the app", panel.shortcut());
            assert_eq!(
                state.tables.cursor,
                cursor,
                "'{}' disturbed the tree",
                panel.shortcut()
            );
        }
    }

    #[test]
    fn a_shifted_shortcut_stays_unbound() {
        for panel in Panel::ALL {
            // Focus starts elsewhere, so an unmoved focus is evidence rather
            // than the state it began in.
            let elsewhere = panel.next();
            let mut state = State {
                focus: elsewhere,
                ..State::default()
            };
            let shifted = panel.shortcut().to_ascii_uppercase();

            press(&mut state, Key::Char(shifted));

            assert_eq!(
                state.focus, elsewhere,
                "'{shifted}' must stay free for a binding of its own"
            );
        }
    }

    fn read_shop_orders() -> State {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        state
    }

    fn page_of(rows: usize) -> Page {
        Page {
            columns: vec!["id".to_owned()],
            rows: vec![
                crate::model::Row {
                    values: vec![crate::model::CqlValue::Null],
                };
                rows
            ],
            next: None,
        }
    }

    #[test]
    fn enter_on_a_table_reads_it() {
        let mut state = read_shop_orders();
        let conn = current_conn(&state);

        let effects = press(&mut state, Key::Enter);

        let query = state.contents.outstanding().expect("the panel is waiting");
        assert_eq!(
            effects,
            vec![
                Effect::Execute {
                    conn,
                    query,
                    stmt: Box::new(Statement::select_all("shop", "orders", ROW_LIMIT)),
                },
                Effect::RecordHistory(Box::new(Statement::select_all("shop", "orders", ROW_LIMIT))),
            ]
        );
        assert!(state.in_flight.contains_key(&query));
    }

    #[test]
    fn space_reads_a_table_the_same_way_enter_does() {
        let mut state = read_shop_orders();

        press(&mut state, Key::Char(' '));

        assert!(state.contents.outstanding().is_some());
    }

    #[test]
    fn reading_a_second_table_cancels_the_first() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let first = state.contents.outstanding().expect("the panel is waiting");
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Enter);

        assert_eq!(
            effects.first(),
            Some(&Effect::Cancel(first)),
            "the superseded query is cancelled before the new one is issued"
        );
        assert!(
            !state.in_flight.contains_key(&first),
            "a superseded query must not still be awaited"
        );
        let second = state.contents.outstanding().expect("waiting on the second");
        assert_ne!(second, first);
    }

    #[test]
    fn a_page_reaches_the_panel_that_asked_for_it() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");

        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(3)),
            },
        );

        assert_eq!(state.contents.row_count(), 3);
    }

    #[test]
    fn a_page_for_a_cancelled_query_never_reaches_the_panel() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let first = state.contents.outstanding().expect("the panel is waiting");
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Enter);

        update(
            &mut state,
            Msg::PageReady {
                query: first,
                page: Box::new(page_of(3)),
            },
        );

        assert_eq!(state.contents.row_count(), 0);
        assert!(
            state.contents.outstanding().is_some(),
            "the second request is still outstanding"
        );
    }

    #[test]
    fn a_failed_query_shows_in_the_panel_and_not_over_it() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");

        update(
            &mut state,
            Msg::Failed {
                query,
                error: DbError::Timeout,
            },
        );

        assert!(matches!(
            state.contents.state(),
            crate::contents::ContentsState::Failed { .. }
        ));
        assert!(
            state.overlay.is_none(),
            "one panel failing must not interrupt the whole application"
        );
    }

    #[test]
    fn j_and_k_move_the_grid_once_contents_has_focus() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(4)),
            },
        );
        let tree_row = state.tables.cursor.index();

        state.focus = Panel::Contents;
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.contents.cursor.index(), 2);
        assert_eq!(
            state.tables.cursor.index(),
            tree_row,
            "the tree must not move while another panel has focus"
        );
    }

    #[test]
    fn the_grid_cursor_cannot_run_off_a_page() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(2)),
            },
        );

        state.focus = Panel::Contents;
        for _ in 0..5 {
            press(&mut state, Key::Char('j'));
        }

        assert_eq!(state.contents.cursor.index(), 1);
    }

    #[test]
    fn a_panel_with_no_rows_swallows_motion_rather_than_passing_it_on() {
        let mut state = read_shop_orders();
        let tree_row = state.tables.cursor.index();

        state.focus = Panel::Ddl;
        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), tree_row);
        assert_eq!(state.contents.cursor.index(), 0);
    }

    #[test]
    fn enter_outside_the_tree_reads_nothing() {
        let mut state = read_shop_orders();
        state.focus = Panel::Contents;

        let effects = press(&mut state, Key::Enter);

        assert!(effects.is_empty());
        assert!(state.contents.outstanding().is_none());
    }

    #[test]
    fn a_reload_that_shrinks_the_tree_pulls_the_selection_inside_it() {
        let mut state = browsing_tables(&[("shop", &["a", "b", "c"])]);
        let conn = current_conn(&state);
        press(&mut state, Key::Char('l'));
        for _ in 0..3 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(state.tables.cursor.index(), 3);

        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(&[("shop", &["a"])])),
            },
        );

        assert_eq!(
            state.tables.cursor.index(),
            1,
            "the clamp must count tree rows, not keyspaces"
        );
    }

    fn measured(state: &mut State, heights: [usize; 4], ddl_lines: usize) {
        update(state, Msg::Measured(Geometry::new(heights, ddl_lines)));
    }

    #[test]
    fn page_down_moves_the_tree_by_one_panel_height() {
        let mut state = browsing(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        measured(&mut state, [3, 0, 0, 0], 0);

        press(&mut state, Key::PageDown);

        assert_eq!(state.tables.cursor.index(), 3);

        press(&mut state, Key::PageUp);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_tree_in_one_key() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        measured(&mut state, [3, 0, 0, 0], 0);

        press(&mut state, Key::End);

        assert_eq!(state.tables.cursor.index(), 3);

        press(&mut state, Key::Home);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn page_down_moves_the_grid_by_one_panel_height() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(30)),
            },
        );
        state.focus = Panel::Contents;
        measured(&mut state, [0, 0, 8, 0], 0);

        press(&mut state, Key::PageDown);

        assert_eq!(state.contents.cursor.index(), 8);
    }

    #[test]
    fn the_ddl_panel_scrolls_a_viewport_rather_than_a_selection() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;
        measured(&mut state, [0, 6, 0, 0], 20);
        let tree_row = state.tables.cursor.index();

        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.ddl.offset(), 2);
        assert_eq!(
            state.tables.cursor.index(),
            tree_row,
            "the tree must not move while another panel has focus"
        );

        press(&mut state, Key::End);

        assert_eq!(
            state.ddl.offset(),
            14,
            "End leaves the last line at the bottom of the panel"
        );
    }

    #[test]
    fn a_motion_in_the_tree_leaves_the_ddl_viewport_alone() {
        let mut state = browsing(&["a", "b", "c"]);
        measured(&mut state, [3, 6, 0, 0], 20);

        press(&mut state, Key::PageDown);
        press(&mut state, Key::End);

        assert_eq!(state.ddl.offset(), 0);
    }

    #[test]
    fn a_motion_before_the_first_measurement_moves_nothing() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;

        press(&mut state, Key::PageDown);
        press(&mut state, Key::End);

        assert_eq!(
            state.ddl.offset(),
            0,
            "a panel of unknown height has no page and no end"
        );
    }

    #[test]
    fn the_new_motions_emit_no_effects() {
        for key in [Key::PageDown, Key::PageUp, Key::Home, Key::End] {
            let mut state = browsing(&["a", "b", "c"]);
            measured(&mut state, [3, 6, 6, 1], 20);

            let effects = press(&mut state, key);

            assert!(effects.is_empty(), "{key:?} issued {effects:?}");
        }
    }

    /// The frame this message describes is the frame that produced it.
    /// Marking dirty unconditionally would redraw, re-measure and redraw
    /// forever (design §3).
    #[test]
    fn a_measurement_that_changes_nothing_leaves_the_state_clean() {
        let mut state = browsing(&["a", "b", "c"]);
        let geometry = Geometry::new([3, 6, 6, 1], 20);
        update(&mut state, Msg::Measured(geometry));
        state.dirty = false;

        let effects = update(&mut state, Msg::Measured(geometry));

        assert!(effects.is_empty());
        assert!(
            !state.dirty,
            "a measurement of the frame already drawn must not ask for another"
        );
    }

    /// A table scrolled to line 34 and then left must not hand the next table
    /// its offset: the reader would open mid-document with nothing saying so.
    #[test]
    fn moving_the_tree_selection_returns_the_ddl_panel_to_the_top() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        state.focus = Panel::Ddl;
        measured(&mut state, [3, 6, 0, 0], 40);
        press(&mut state, Key::End);
        assert_eq!(state.ddl.offset(), 34);

        state.focus = Panel::Tables;
        press(&mut state, Key::Char('j'));

        assert_eq!(state.ddl.offset(), 0, "users is a document of its own");
    }

    #[test]
    fn unfolding_a_keyspace_leaves_the_ddl_panel_where_it_is() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.focus = Panel::Ddl;
        measured(&mut state, [3, 6, 0, 0], 40);
        press(&mut state, Key::End);
        assert_eq!(state.ddl.offset(), 34);
        state.focus = Panel::Tables;

        press(&mut state, Key::Char('l'));

        assert_eq!(
            state.ddl.offset(),
            34,
            "shop is still the selected node, so the panel is still on its description"
        );

        // A second `l` steps onto shop's first table, which is a new subject.
        press(&mut state, Key::Char('l'));

        assert_eq!(state.ddl.offset(), 0);
    }

    #[test]
    fn a_measurement_pulls_a_stale_offset_back_inside_the_panel() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;
        measured(&mut state, [0, 6, 0, 0], 40);
        press(&mut state, Key::End);
        assert_eq!(state.ddl.offset(), 34);
        state.dirty = false;

        // The tree cursor moved to a table with far fewer options.
        measured(&mut state, [0, 6, 0, 0], 8);

        assert_eq!(state.ddl.offset(), 2);
        assert!(
            state.dirty,
            "the offset moved, so the frame must be repainted"
        );
    }

    /// A state where every motion has somewhere to go in both directions:
    /// two keyspaces with the second unfolded, a page of rows under the
    /// Contents cursor, and a DDL document longer than its panel. A cursor
    /// resting on 0 would make `k` and `home` inert and fail a correct hint.
    fn populated(focus: Panel) -> State {
        let mut state = browsing_tables(&[("alpha", &["a1"]), ("shop", &["orders", "users"])]);
        state.tables.cursor.go_to(1, 2);
        press(&mut state, Key::Char('l'));

        // Tables keeps its cursor on the keyspace, where `l` steps in and `h`
        // folds. The other panels need a table selected: Ddl draws its
        // details from one, and Contents shows what reading it returned.
        if focus != Panel::Tables {
            press(&mut state, Key::Char('l'));
            press(&mut state, Key::Enter);
            let query = state
                .contents
                .outstanding()
                .expect("pressing enter on a table issues a read");
            update(
                &mut state,
                Msg::PageReady {
                    query,
                    page: Box::new(page_of(20)),
                },
            );
            state.contents.cursor.go_to(5, 20);
        }

        measured(&mut state, [3, 3, 3, 3], 40);
        state.ddl.page_down(40, 3);
        state.focus = focus;
        state
    }

    #[test]
    fn every_hint_the_bar_shows_does_something() {
        for panel in Panel::ALL {
            for binding in panel.hints() {
                for &key in binding.keys {
                    let mut state = populated(panel);
                    let before = crate::view::view_model(&state);

                    let effects = press(&mut state, key);

                    assert!(
                        crate::view::view_model(&state) != before || !effects.is_empty(),
                        "{panel:?} offers '{key:?} {}' and pressing it changes nothing",
                        binding.action
                    );
                }
            }
        }
    }

    #[test]
    fn every_global_hint_does_something_in_every_panel() {
        for panel in Panel::ALL {
            // `q` left GLOBAL because the bar stops naming it in a panel that
            // takes text. It is still a binding in the other three, and this
            // is still where that is checked.
            let claimed = crate::view::hints::GLOBAL
                .iter()
                .chain((!panel.takes_text()).then_some(&crate::view::hints::QUIT));

            for binding in claimed {
                for &key in binding.keys {
                    let mut state = populated(panel);
                    let before = crate::view::view_model(&state);

                    let effects = press(&mut state, key);

                    assert!(
                        crate::view::view_model(&state) != before || !effects.is_empty(),
                        "'{key:?} {}' does nothing with {panel:?} focused",
                        binding.action
                    );
                }
            }
        }
    }
}
