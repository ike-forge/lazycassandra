//! The one place state changes.
//!
//! `update` is synchronous, pure, and total. It mutates state and *describes*
//! what should happen next; it cannot open a socket because it has nothing to
//! open one with (architecture.md §3).

use crate::model::{ConnId, DbError, Key, KeyPress, Page, PagingState, QueryId, Schema, Statement};
use crate::posture::Posture;
use crate::state::{ConnectionState, Overlay, Panel, SchemaState, State};
use crate::style::Severity;

/// Everything that can happen. Async results re-enter here through the same
/// channel as a keystroke.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Input(KeyPress),
    /// The terminal was resized. Carries no geometry — the renderer reads the
    /// real size from `frame.area()` — it exists only to trigger a repaint.
    Resized,
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
        query: QueryId,
        stmt: Box<Statement>,
    },
    FetchNextPage {
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
    match msg {
        Msg::Tick => Vec::new(),

        Msg::Input(press) => handle_key(state, press),

        Msg::Resized => {
            state.mark_dirty();
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

        Msg::PageReady { query, page: _ } | Msg::Failed { query, error: _ } => {
            // A result whose id is no longer in flight belongs to a cancelled
            // query, and applying it would resurrect a view the user left.
            if state.in_flight.remove(&query).is_some() {
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

    match press.key {
        Key::Char('q') => return quit(state),
        Key::Escape => state.overlay = None,
        Key::Char('j') | Key::Down => move_selection(state, Motion::Down),
        Key::Char('k') | Key::Up => move_selection(state, Motion::Up),
        Key::Enter | Key::Char(' ') => transition(state, Transition::Toggle),
        Key::Char('l') | Key::Right => transition(state, Transition::Open),
        Key::Char('h') | Key::Left => transition(state, Transition::Close),
        _ => {}
    }

    Vec::new()
}

#[derive(Clone, Copy)]
enum Transition {
    Toggle,
    Open,
    Close,
}

fn move_selection(state: &mut State, motion: Motion) {
    if state.focus != Panel::Tables {
        return;
    }

    let rows = tree_rows(state);
    match motion {
        Motion::Down => state.tables.cursor.down(rows),
        Motion::Up => state.tables.cursor.up(),
    }
}

fn transition(state: &mut State, transition: Transition) {
    if state.focus != Panel::Tables {
        return;
    }

    let Some((tables, schema)) = state.tables_and_schema() else {
        return;
    };

    match transition {
        Transition::Toggle => tables.toggle(schema),
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
/// # Errors
///
/// [`DbError::ReadOnly`] if the statement mutates and the connection's posture
/// does not permit it. Nothing is recorded and no effect is emitted.
pub fn submit(state: &mut State, stmt: Statement) -> Result<Vec<Effect>, DbError> {
    if stmt.mutates() && !state.posture().permits_mutation() {
        return Err(DbError::ReadOnly);
    }

    let query = QueryId::next();
    state.in_flight.insert(query, stmt.clone());
    state.mark_dirty();

    Ok(vec![
        Effect::Execute {
            query,
            stmt: Box::new(stmt.clone()),
        },
        Effect::RecordHistory(Box::new(stmt)),
    ])
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

        let effects = submit(&mut state, statement(StatementKind::Read)).expect("read is allowed");

        assert_eq!(effects.len(), 2, "expected Execute and RecordHistory");
        assert_eq!(state.in_flight.len(), 1);
    }

    #[test]
    fn writable_connection_executes_and_records_a_mutating_statement() {
        let mut state = connected(Posture::Writable);

        let effects =
            submit(&mut state, statement(StatementKind::Write)).expect("write is allowed");

        let query = *state.in_flight.keys().next().expect("query is in flight");
        assert_eq!(
            effects,
            vec![
                Effect::Execute {
                    query,
                    stmt: Box::new(statement(StatementKind::Write)),
                },
                Effect::RecordHistory(Box::new(statement(StatementKind::Write))),
            ]
        );
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
        submit(&mut state, statement(StatementKind::Write)).expect("write is allowed");
        let query = *state.in_flight.keys().next().expect("query is in flight");

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
        state.focus = Panel::Queries;

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
    fn enter_on_a_table_does_nothing_yet() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        let before = state.tables.clone();

        press(&mut state, Key::Enter);

        assert_eq!(
            state.tables, before,
            "Enter on a table is reserved for acting on it"
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
        state.focus = Panel::Queries;

        press(&mut state, Key::Char('l'));

        assert_eq!(row_count(&state), 1);
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
}
