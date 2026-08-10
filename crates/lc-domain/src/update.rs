//! The one place state changes.
//!
//! `update` is synchronous, pure, and total. It mutates state and *describes*
//! what should happen next; it cannot open a socket because it has nothing to
//! open one with (architecture.md §3).

use crate::commands::CommandsPanel;
use crate::contents::TableRef;
use crate::model::{ConnId, DbError, Key, KeyPress, Page, PagingState, QueryId, Schema, Statement};
use crate::pointer::{Click, Pointer, Wheel};
use crate::posture::Posture;
use crate::search::{Filtered, Prompt};
use crate::state::{
    ConnectionState, Cursor, Geometry, Message, Overlay, Panel, SchemaState, Scroll, State,
};
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
    /// A pointer gesture, resolved by `lc-inbound` against the frame it
    /// happened in (doc/specs/2026-08-09-mouse-input-design.md §5).
    Pointer(Pointer),
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
    let cursors = cursor_indices(state);
    let scrolls = (state.tables.scroll, state.contents.scroll);

    let effects = apply(state, msg);

    // The Ddl offset belongs to the node being described, so a new subject
    // opens at its top rather than halfway down the last one. Stated once here
    // rather than at each of the four paths that can move the tree selection.
    if described(state) != subject {
        state.ddl.scroll.to_start();
    }

    clamp_lists(state);
    follow_moved_cursors(state, cursors);

    // `Msg::Measured` marks nothing dirty by itself, so a clamp it provoked
    // would otherwise not be drawn until the next unrelated keypress.
    if (state.tables.scroll, state.contents.scroll) != scrolls {
        state.mark_dirty();
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
        Msg::Tick => {
            let working = state.is_working();
            if state.loader.advance(working) {
                state.mark_dirty();
            }
            Vec::new()
        }

        Msg::Input(press) => handle_key(state, press),

        Msg::Pointer(pointer) => handle_pointer(state, pointer),

        Msg::Resized => {
            state.mark_dirty();
            Vec::new()
        }

        Msg::Measured(geometry) => measured(state, geometry),

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
            state.overlay = Some(Overlay::Message(Message {
                title: "Connection failed".to_owned(),
                message: error.to_string(),
                severity: Severity::Danger,
            }));
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
                state.overlay = Some(Overlay::Message(Message {
                    title: "Schema unavailable".to_owned(),
                    message: error.to_string(),
                    severity: Severity::Danger,
                }));
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

    // An open palette takes the keys it uses
    // (doc/specs/2026-08-09-command-palette-design.md §7).
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return palette_key(state, press.key);
    }

    // A prompt takes text, so it claims every printable key — the
    // panel-that-takes-text rule, applied to an overlay (design §1.1).
    if matches!(state.overlay, Some(Overlay::Search(_))) {
        return search_key(state, press.key);
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
        Key::Char('?') => open_palette(state),
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
        Key::Char('/') => open_prompt(state),
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

/// Rows a wheel notch moves, in whatever unit the panel scrolls in. One is
/// sluggish enough that the pointer feels broken; three is what terminals
/// settled on (design §6).
const WHEEL_ROWS: usize = 3;

/// The one place a pointer gesture becomes behaviour, as `handle_key` is for
/// keys (doc/shortcuts.md §2).
///
/// Every gesture decomposes into a binding that already exists (design §1): a
/// click on a tree row is a selection followed by the `Enter` arm, and a wheel
/// notch is repeated motions.
fn handle_pointer(state: &mut State, pointer: Pointer) -> Vec<Effect> {
    state.mark_dirty();

    // The overlay is modal: it spends the click that dismisses it rather than
    // letting it act on a row it was covering (design §2). Routed through
    // `escape` rather than a bare `state.overlay = None`, so a click on an
    // open prompt restores `previous` instead of dropping it; with any other
    // overlay up, `escape` only dismisses it, so nothing else changes.
    if state.overlay.is_some() {
        if matches!(pointer, Pointer::Click(_)) {
            escape(state);
        }
        return Vec::new();
    }

    match pointer {
        Pointer::Click(click) => click_at(state, click),
        Pointer::Wheel { panel, direction } => {
            wheel(state, panel, direction);
            Vec::new()
        }
    }
}

/// Scrolls `panel`'s viewport without focusing it and without moving its
/// selection: the wheel is a look somewhere else, and a keypress returns you
/// (doc/specs/2026-08-09-list-viewport-design.md §1).
fn wheel(state: &mut State, panel: Panel, direction: Wheel) {
    let len = scroll_len(state, panel);
    let height = state.geometry.height(panel);

    let Some(scroll) = panel_scroll(state, panel) else {
        return;
    };

    for _ in 0..WHEEL_ROWS {
        match direction {
            Wheel::Down => scroll.down(len, height),
            Wheel::Up => scroll.up(),
        }
    }
}

fn click_at(state: &mut State, click: Click) -> Vec<Effect> {
    state.focus = click.panel();

    let Click::Row { panel, row } = click else {
        return Vec::new();
    };

    let len = row_count(state, panel);

    // The map a press was resolved against describes the *previous* frame. A
    // row that has gone since is dropped rather than clamped onto whatever
    // took its place: in Tables, clamping would read a table nobody pointed at
    // (design §6).
    if row >= len {
        return Vec::new();
    }

    match panel {
        Panel::Tables => {
            state.tables.cursor.go_to(row, len);
            activate(state)
        }
        Panel::Contents => {
            state.contents.cursor.go_to(row, len);
            Vec::new()
        }
        // Ddl and Queries are both scrolled views with nothing a press can
        // address — lines, not rows (design §4).
        Panel::Ddl | Panel::Queries => Vec::new(),
    }
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

/// `/` opens a prompt over the focused panel. Refused where the panel holds no
/// filter, which is Queries — unreachable, since the editor claims `/` as text
/// first (panel search design §8), and stated here rather than assumed.
///
/// Clears the target's filter immediately rather than leaving `previous`
/// showing until the first keystroke: the buffer opens empty, and live
/// filtering (design §3.3) means the panel underneath is the preview of
/// whatever the buffer holds, from the first frame the prompt is up.
fn open_prompt(state: &mut State) {
    let Some(target) = Filtered::of(state.focus) else {
        return;
    };

    let previous = state.filter(target).cloned();
    state.overlay = Some(Overlay::Search(Prompt::opened(target, previous)));
    state.apply_filter(target, None);
}

/// What an open prompt responds to.
///
/// Every printable key is a character, so `q` types a `q` — the opposite of
/// the palette, which leaves `q` quitting. The two differ because the palette
/// takes motion and this takes text (panel search design §1.1, §3.1).
///
/// `Enter` and `Escape` are decided before the prompt's mutable borrow is
/// taken, rather than inside the match that holds it: both close the overlay,
/// which needs `&mut state.overlay` while `prompt` is still live from the
/// `let Some(...) = &mut state.overlay` above it, so folding the close into
/// that match cannot borrow-check.
fn search_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let Some(Overlay::Search(prompt)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => prompt.push(character),
        Key::Backspace => prompt.pop(),
        Key::Enter => {
            state.overlay = None;
            return Vec::new();
        }
        _ => return Vec::new(),
    }

    let (target, filter) = (prompt.target(), prompt.filter());
    state.apply_filter(target, filter);

    Vec::new()
}

/// `Esc` resolves in one order, first hit wins: the overlay, then the focused
/// panel's filter, then leaving a panel that takes text. One press does one of
/// the three, never two — the thing most recently put on screen comes off
/// first (panel search design §6).
fn escape(state: &mut State) {
    match state.overlay.take() {
        Some(Overlay::Search(prompt)) => {
            state.apply_filter(prompt.target(), prompt.previous().cloned());
            return;
        }
        Some(Overlay::Message(_) | Overlay::Commands(_)) => return,
        None => {}
    }

    if let Some(target) = Filtered::of(state.focus)
        && state.filter(target).is_some()
    {
        state.apply_filter(target, None);
        return;
    }

    if state.focus.takes_text() {
        state.focus = Panel::default();
    }
}

/// `?` opens the palette, replacing whatever overlay was up.
///
/// Refused where the frame cannot draw one: opening a palette nothing draws
/// leaves the user in a mode with nothing on screen saying so, wondering why
/// `j` stopped moving the tree (design §5.1).
fn open_palette(state: &mut State) {
    if state.geometry.command_rows() == 0 {
        return;
    }

    state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));
}

/// What an open palette responds to.
///
/// `q` still quits, because a key that means "quit" everywhere except in one
/// overlay is not a key anyone can trust without looking. Everything the
/// palette does not use is inert rather than dismissing, so no letter means
/// "close" in one frame and something else in the next (design §7).
fn palette_key(state: &mut State, key: Key) -> Vec<Effect> {
    let height = state.geometry.command_rows();

    match key {
        Key::Char('q') => return quit(state),
        Key::Char('?') | Key::Escape => {
            state.overlay = None;
            return Vec::new();
        }
        _ => {}
    }

    let Some(Overlay::Commands(panel)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => panel.down(),
        Key::Char('k') | Key::Up => panel.up(),
        Key::PageDown => panel.page_down(height),
        Key::PageUp => panel.page_up(height),
        Key::Home => panel.to_start(),
        Key::End => panel.to_end(),
        _ => {}
    }

    Vec::new()
}

#[derive(Clone, Copy)]
enum Transition {
    Open,
    Close,
}

/// Applies `motion` to whatever the focused panel moves: a viewport in Ddl, a
/// selection elsewhere.
///
/// The focused panel and no other. A wheel names the panel under the pointer
/// and moves its viewport directly ([`wheel`]), so nothing but a key reaches
/// here.
fn scroll(state: &mut State, motion: Motion) {
    let panel = state.focus;
    let height = state.geometry.height(panel);

    if panel == Panel::Ddl {
        let len = state.geometry.ddl_lines();

        match motion {
            Motion::Down => state.ddl.scroll.down(len, height),
            Motion::Up => state.ddl.scroll.up(),
            Motion::PageDown => state.ddl.scroll.page_down(len, height),
            Motion::PageUp => state.ddl.scroll.page_up(height),
            Motion::Start => state.ddl.scroll.to_start(),
            Motion::End => state.ddl.scroll.to_end(len, height),
        }
        return;
    }

    let len = row_count(state, panel);

    let Some(cursor) = panel_cursor(state, panel) else {
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

/// How many rows `panel`'s *cursor* moves over. Ddl scrolls a viewport rather
/// than a selection and never reaches here; Queries never arrives here
/// either — the editor claims the motion keys first (design §3), so this arm
/// is unreachable in practice and kept for exhaustiveness.
fn row_count(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Tables => tree_rows(state),
        Panel::Contents => state.contents.row_count(),
        Panel::Ddl | Panel::Queries => 0,
    }
}

/// Queries never arrives here — the editor claims the motion keys before
/// this match is reached (design §3) — so its arm is unreachable in
/// practice and kept for exhaustiveness.
fn panel_cursor(state: &mut State, panel: Panel) -> Option<&mut Cursor> {
    match panel {
        Panel::Tables => Some(&mut state.tables.cursor),
        Panel::Contents => Some(&mut state.contents.cursor),
        Panel::Ddl | Panel::Queries => None,
    }
}

/// Records what the last frame turned out to be, and repairs whatever that
/// invalidates.
///
/// No unconditional `mark_dirty`: this message describes the frame that
/// produced it, so redrawing on every one of them would re-measure and redraw
/// forever (panel scrolling design §3).
fn measured(state: &mut State, geometry: Geometry) -> Vec<Effect> {
    let previous = state.geometry;
    state.geometry = geometry;

    // A panel that changed height must draw its selection again: the renderer
    // stopped arranging that, and `update`'s epilogue follows only a cursor
    // that *moved* — a resize moves none
    // (doc/specs/2026-08-09-list-viewport-design.md §5).
    //
    // Keyed on the height rather than on the message, so a wheel cannot be
    // undone here: a wheel changes no height, and `lc-app` reports no
    // measurement for a frame whose geometry did not change.
    for panel in LISTS {
        let height = geometry.height(panel);

        if height == previous.height(panel) {
            continue;
        }

        let index = cursor_index(state, panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.follow(index, height);
        }
    }

    let before = state.ddl.scroll;
    state
        .ddl
        .scroll
        .clamp_to(geometry.ddl_lines(), geometry.height(Panel::Ddl));

    if state.ddl.scroll != before {
        state.mark_dirty();
    }

    // The bar's `? commands` hint is a function of `command_rows`
    // (view/hints.rs): a resize that flips it between zero and
    // non-zero changes the view model just as surely as the ddl
    // clamp above does, and owes it the same repaint.
    if geometry.command_rows() != previous.command_rows() {
        state.mark_dirty();
    }

    // A palette the frame shrank past cannot stay open: nothing would
    // draw it, and it would still be capturing every key
    // (design §5.1).
    if geometry.command_rows() == 0 && matches!(state.overlay, Some(Overlay::Commands(_))) {
        state.overlay = None;
        state.mark_dirty();
    }

    Vec::new()
}

/// The panels whose viewport `update` maintains. Ddl keeps its own, moved
/// directly by the motions that scroll it.
const LISTS: [Panel; 2] = [Panel::Tables, Panel::Contents];

/// The viewport `panel` draws through, for the panels that have one.
fn panel_scroll(state: &mut State, panel: Panel) -> Option<&mut Scroll> {
    match panel {
        Panel::Tables => Some(&mut state.tables.scroll),
        Panel::Contents => Some(&mut state.contents.scroll),
        Panel::Ddl => Some(&mut state.ddl.scroll),
        Panel::Queries => None,
    }
}

/// How long the document `panel` scrolls over is — wrapped lines in Ddl, rows
/// elsewhere.
fn scroll_len(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Ddl => state.geometry.ddl_lines(),
        Panel::Tables | Panel::Contents => row_count(state, panel),
        Panel::Queries => 0,
    }
}

/// `panel`'s selected row, or zero for a panel with no selection.
fn cursor_index(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Tables => state.tables.cursor.index(),
        Panel::Contents => state.contents.cursor.index(),
        Panel::Ddl | Panel::Queries => 0,
    }
}

/// Each list panel's selected index, for the comparison in [`update`].
///
/// Built from `LISTS` rather than written out, so the array this returns and
/// the one [`follow_moved_cursors`] walks cannot fall out of order — a
/// mismatch there would follow each panel against the other's index, silently.
fn cursor_indices(state: &State) -> [usize; 2] {
    LISTS.map(|panel| cursor_index(state, panel))
}

/// Pulls every list offset back inside its document.
///
/// Unconditional because a shrinking list does not change the geometry —
/// folding a keyspace leaves every panel the same height, so no
/// [`Msg::Measured`] arrives to clamp against (design §3). `clamp_to` only
/// ever reduces an offset, so this cannot fight a wheel bounded by the same
/// arithmetic.
fn clamp_lists(state: &mut State) {
    for panel in LISTS {
        let len = row_count(state, panel);
        let height = state.geometry.height(panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.clamp_to(len, height);
        }
    }
}

/// Pulls the offset of each list whose selection moved, so the selection is
/// drawn.
///
/// Only the ones that moved: a wheel leaves both indices alone, and following
/// it would undo the scroll it just performed (design §3).
fn follow_moved_cursors(state: &mut State, before: [usize; 2]) {
    for (panel, before) in LISTS.into_iter().zip(before) {
        let index = cursor_index(state, panel);

        if index == before {
            continue;
        }

        let height = state.geometry.height(panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.follow(index, height);
        }
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

    // A filter forces every retained keyspace open (design §4.1), so folding
    // would change nothing on screen. Swallowed rather than half-honoured:
    // doc/shortcuts.md §2, a panel that cannot honour a motion swallows it.
    if state.tables.filter().is_some() {
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
    use crate::commands::{CATALOGUE, CommandsPanel};
    use crate::model::{CqlValue, Modifiers, Row, StatementKind};
    use crate::pointer::{Click, Pointer, Wheel};
    use crate::search::{Filter, Filtered, Prompt};
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

    #[test]
    fn a_tick_costs_a_repaint_only_while_something_is_outstanding() {
        let mut idle = State {
            dirty: false,
            ..State::default()
        };

        assert!(!idle.is_working());
        update(&mut idle, Msg::Tick);
        assert!(
            !idle.dirty,
            "an idle tick changes nothing, and repainting on it would burn the terminal 25 times a second"
        );

        let mut working = connected(Posture::ReadOnly);
        working.dirty = false;

        assert!(working.is_working(), "the schema is still loading");
        update(&mut working, Msg::Tick);
        assert!(working.dirty);
    }

    #[test]
    fn the_loader_falls_back_to_rest_when_the_work_finishes() {
        let mut state = connected(Posture::ReadOnly);
        for _ in 0..5 {
            update(&mut state, Msg::Tick);
        }
        assert_ne!(state.loader.frame(), 0);

        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(&["shop"])),
            },
        );
        state.dirty = false;
        update(&mut state, Msg::Tick);

        assert_eq!(
            state.loader.frame(),
            0,
            "an idle frame is always the same frame, not wherever the animation stopped"
        );
        assert!(state.dirty, "the fall back to rest is itself a repaint");
    }

    #[test]
    fn a_query_in_flight_is_work_the_loader_animates_for() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(&["shop"])),
            },
        );
        assert!(
            !state.is_working(),
            "the schema arrived and nothing else is out"
        );

        state
            .in_flight
            .insert(QueryId::next(), statement(StatementKind::Read));

        assert!(state.is_working());
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
        let Some(Overlay::Message(overlay)) = state.overlay else {
            panic!("a failure is shown");
        };
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

    /// A frame big enough for a palette. `browsing` leaves geometry at its
    /// default, whose `command_rows` is zero — which is itself a binding
    /// (`the_palette_does_not_open_where_it_cannot_be_drawn`).
    fn measured(keyspaces: &[&str]) -> State {
        let mut state = browsing(keyspaces);
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        state
    }

    fn palette(state: &State) -> Option<&CommandsPanel> {
        match state.overlay.as_ref()? {
            Overlay::Commands(panel) => Some(panel),
            Overlay::Message(_) | Overlay::Search(_) => None,
        }
    }

    #[test]
    fn question_mark_opens_the_palette_and_closes_it() {
        let mut state = measured(&["a"]);

        press(&mut state, Key::Char('?'));
        assert!(palette(&state).is_some());

        press(&mut state, Key::Char('?'));
        assert!(state.overlay.is_none());
    }

    #[test]
    fn escape_closes_the_palette() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
    }

    #[test]
    fn opening_the_palette_replaces_the_message_it_covers() {
        let mut state = measured(&["a"]);
        state.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

        press(&mut state, Key::Char('?'));
        assert!(palette(&state).is_some());

        press(&mut state, Key::Escape);
        assert!(
            state.overlay.is_none(),
            "esc leaves nothing rather than peeling back to the message"
        );
    }

    #[test]
    fn the_palette_does_not_open_where_it_cannot_be_drawn() {
        let mut state = browsing(&["a"]);
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 0);

        press(&mut state, Key::Char('?'));

        assert!(
            state.overlay.is_none(),
            "a mode with nothing on screen saying so is worse than an inert key"
        );
    }

    #[test]
    fn an_open_palette_takes_the_motion_the_panel_would_have() {
        let mut state = measured(&["a", "b", "c"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "the tree did not move behind the palette"
        );
        assert_eq!(palette(&state).expect("still open").selected(), 2);

        press(&mut state, Key::Char('k'));
        assert_eq!(palette(&state).expect("still open").selected(), 1);

        press(&mut state, Key::End);
        assert_eq!(
            palette(&state).expect("still open").selected(),
            crate::commands::row_count() - 1
        );

        press(&mut state, Key::Home);
        assert_eq!(palette(&state).expect("still open").selected(), 1);

        press(&mut state, Key::PageDown);
        assert_eq!(
            palette(&state).expect("still open").selected(),
            15,
            "paging is routed at the handle_key layer, same as j/k"
        );

        press(&mut state, Key::PageUp);
        assert_eq!(palette(&state).expect("still open").selected(), 1);
    }

    #[test]
    fn q_quits_from_inside_the_palette() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('q'));

        assert!(state.should_quit);
    }

    #[test]
    fn an_open_palette_swallows_the_keys_it_does_not_use() {
        // Each starting focus is chosen so a leaked key would be observable:
        // `t`/`Tab` need a focus their leak would actually move away from,
        // `Enter`/`Space` need `Tables` on a keyspace with a table, since
        // folding a childless one leaves no visible trace at all.
        let cases = [
            (Key::Char('t'), Panel::Contents),
            (Key::Tab, Panel::Contents),
            (Key::Enter, Panel::Tables),
            (Key::Char(' '), Panel::Tables),
        ];

        for (key, focus) in cases {
            let mut state = browsing_tables(&[("a", &["t1"])]);
            state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
            state.focus = focus;
            press(&mut state, Key::Char('?'));
            let before = crate::view::view_model(&state);

            let effects = press(&mut state, key);

            assert!(palette(&state).is_some(), "{key:?} dismissed the palette");
            assert_eq!(
                crate::view::view_model(&state),
                before,
                "{key:?} reached past the palette"
            );
            assert!(effects.is_empty(), "{key:?} issued {effects:?}");
        }
    }

    #[test]
    fn a_frame_that_shrinks_below_the_palette_closes_it() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));
        assert!(palette(&state).is_some());

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
        );

        assert!(
            state.overlay.is_none(),
            "a palette nothing can draw must not still capture keys"
        );
    }

    #[test]
    fn a_frame_that_makes_room_for_the_palette_repaints_the_bar() {
        let mut state = State::default();
        assert_eq!(
            state.geometry.command_rows(),
            0,
            "the default geometry is where `?` starts out inert"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 14)),
        );

        assert!(
            state.dirty,
            "command_rows went from zero to non-zero, so the bar's `? commands` \
             hint just appeared and nothing scheduled the repaint that shows it"
        );
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
        state.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

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

    /// `?` is a CQL bind marker before it is a shortcut, so the editor has to
    /// win it. Neither parent of the merge that brought the palette and the
    /// editor together could test this: each was the only mode in its branch.
    #[test]
    fn a_question_mark_typed_in_queries_is_a_bind_marker_not_the_palette() {
        let mut state = measured(&["a"]);
        state.focus = Panel::Queries;

        press(&mut state, Key::Char('?'));

        assert!(
            state.overlay.is_none(),
            "the editor claimed it before the palette could open"
        );
        assert_eq!(state.queries.text(), "?");
    }

    /// The two mode guards in `handle_key`, in the order they are written: an
    /// overlay the user opened deliberately outranks the focused panel's claim
    /// on printable keys (doc/shortcuts.md §2).
    #[test]
    fn an_open_palette_outranks_a_panel_that_takes_text() {
        let mut state = measured(&["a", "b", "c"]);
        press(&mut state, Key::Char('?'));
        state.focus = Panel::Queries;

        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.queries.text(),
            "",
            "the palette took the key rather than the editor"
        );
        assert!(state.overlay.is_some(), "and it is still open");
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
        state.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

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
    fn l_and_h_are_swallowed_while_a_filter_is_applied() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.apply_filter(Filtered::Tables, Filter::new("shop"));
        let before = row_count(&state);

        press(&mut state, Key::Char('h'));
        press(&mut state, Key::Char('l'));

        assert_eq!(
            row_count(&state),
            before,
            "a filter forces its matches open, so folding could only look broken"
        );
    }

    #[test]
    fn folding_under_a_filter_does_not_disturb_the_fold_state_it_will_return_to() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        assert_eq!(row_count(&state), 3, "shop starts unfolded");

        state.apply_filter(Filtered::Tables, Filter::new("shop"));
        press(&mut state, Key::Char('h'));
        press(&mut state, Key::Char('l'));
        state.apply_filter(Filtered::Tables, None);

        assert_eq!(
            row_count(&state),
            3,
            "the fold state from before the filter must survive it untouched"
        );
    }

    #[test]
    fn l_and_h_still_fold_once_the_filter_is_gone() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

        press(&mut state, Key::Char('l'));

        assert_eq!(row_count(&state), 3);
    }

    #[test]
    fn the_cursor_clamps_as_a_filter_shrinks_the_tree() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));
        assert_eq!(state.tables.cursor.index(), 2, "on users");

        state.apply_filter(Filtered::Tables, Filter::new("orders"));

        assert_eq!(
            state.tables.cursor.index(),
            1,
            "two rows survive, so the last one is index 1"
        );
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

    fn sized(state: &mut State, heights: [usize; 4], ddl_lines: usize) {
        update(state, Msg::Measured(Geometry::new(heights, ddl_lines, 0)));
    }

    #[test]
    fn page_down_moves_the_tree_by_one_panel_height() {
        let mut state = browsing(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        sized(&mut state, [3, 0, 0, 0], 0);

        press(&mut state, Key::PageDown);

        assert_eq!(state.tables.cursor.index(), 3);

        press(&mut state, Key::PageUp);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_tree_in_one_key() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        sized(&mut state, [3, 0, 0, 0], 0);

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
        sized(&mut state, [0, 0, 8, 0], 0);

        press(&mut state, Key::PageDown);

        assert_eq!(state.contents.cursor.index(), 8);
    }

    #[test]
    fn the_ddl_panel_scrolls_a_viewport_rather_than_a_selection() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;
        sized(&mut state, [0, 6, 0, 0], 20);
        let tree_row = state.tables.cursor.index();

        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.ddl.scroll.offset(), 2);
        assert_eq!(
            state.tables.cursor.index(),
            tree_row,
            "the tree must not move while another panel has focus"
        );

        press(&mut state, Key::End);

        assert_eq!(
            state.ddl.scroll.offset(),
            14,
            "End leaves the last line at the bottom of the panel"
        );
    }

    #[test]
    fn a_motion_in_the_tree_leaves_the_ddl_viewport_alone() {
        let mut state = browsing(&["a", "b", "c"]);
        sized(&mut state, [3, 6, 0, 0], 20);

        press(&mut state, Key::PageDown);
        press(&mut state, Key::End);

        assert_eq!(state.ddl.scroll.offset(), 0);
    }

    #[test]
    fn a_motion_before_the_first_measurement_moves_nothing() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;

        press(&mut state, Key::PageDown);
        press(&mut state, Key::End);

        assert_eq!(
            state.ddl.scroll.offset(),
            0,
            "a panel of unknown height has no page and no end"
        );
    }

    #[test]
    fn the_new_motions_emit_no_effects() {
        for key in [Key::PageDown, Key::PageUp, Key::Home, Key::End] {
            let mut state = browsing(&["a", "b", "c"]);
            sized(&mut state, [3, 6, 6, 1], 20);

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
        let geometry = Geometry::new([3, 6, 6, 1], 20, 0);
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
        sized(&mut state, [3, 6, 0, 0], 40);
        press(&mut state, Key::End);
        assert_eq!(state.ddl.scroll.offset(), 34);

        state.focus = Panel::Tables;
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.ddl.scroll.offset(),
            0,
            "users is a document of its own"
        );
    }

    #[test]
    fn unfolding_a_keyspace_leaves_the_ddl_panel_where_it_is() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.focus = Panel::Ddl;
        sized(&mut state, [3, 6, 0, 0], 40);
        press(&mut state, Key::End);
        assert_eq!(state.ddl.scroll.offset(), 34);
        state.focus = Panel::Tables;

        press(&mut state, Key::Char('l'));

        assert_eq!(
            state.ddl.scroll.offset(),
            34,
            "shop is still the selected node, so the panel is still on its description"
        );

        // A second `l` steps onto shop's first table, which is a new subject.
        press(&mut state, Key::Char('l'));

        assert_eq!(state.ddl.scroll.offset(), 0);
    }

    #[test]
    fn a_measurement_pulls_a_stale_offset_back_inside_the_panel() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;
        sized(&mut state, [0, 6, 0, 0], 40);
        press(&mut state, Key::End);
        assert_eq!(state.ddl.scroll.offset(), 34);
        state.dirty = false;

        // The tree cursor moved to a table with far fewer options.
        sized(&mut state, [0, 6, 0, 0], 8);

        assert_eq!(state.ddl.scroll.offset(), 2);
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

        sized(&mut state, [3, 3, 3, 3], 40);
        state.ddl.scroll.page_down(40, 3);
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
        // `?` is filtered by `hints::bar` itself once a binding's effect
        // depends on more than focus (design §5.1) — the sweep is over what
        // the bar actually shows for this state, which is what it promises.
        for panel in Panel::ALL {
            let shown: Vec<&str> = crate::view::hints::bar(&populated(panel))
                .global
                .iter()
                .map(|hint| hint.action)
                .collect();

            // `q` left GLOBAL because the bar stops naming it in a panel that
            // takes text, and `?` is dropped where no palette would fit. Both
            // are conditions on the bar rather than on the binding set, so the
            // sweep asks the bar what it is showing.
            let claimed = crate::view::hints::GLOBAL
                .iter()
                .chain((!panel.takes_text()).then_some(&crate::view::hints::QUIT));

            for binding in claimed {
                if !shown.contains(&binding.action) {
                    continue;
                }
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

    fn point(state: &mut State, pointer: Pointer) -> Vec<Effect> {
        update(state, Msg::Pointer(pointer))
    }

    fn click_row(state: &mut State, panel: Panel, row: usize) -> Vec<Effect> {
        point(state, Pointer::Click(Click::Row { panel, row }))
    }

    /// `shop.orders` loaded with `count` single-column rows.
    fn showing_rows(count: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, TableRef::new("shop", "orders"));
        state.contents.page_ready(
            query,
            Page {
                columns: vec!["id".to_owned()],
                rows: (0..count)
                    .map(|i| Row {
                        values: vec![CqlValue::Int(
                            i64::try_from(i).expect("the fixture is small"),
                        )],
                    })
                    .collect(),
                next: None,
            },
        );
        state
    }

    fn failure() -> Overlay {
        Overlay::Message(Message {
            title: "Cannot connect".to_owned(),
            message: "no route to host".to_owned(),
            severity: Severity::Danger,
        })
    }

    #[test]
    fn a_click_on_a_keyspace_folds_and_unfolds_it() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

        click_row(&mut state, Panel::Tables, 0);
        assert_eq!(row_count(&state), 3, "the keyspace unfolded");

        click_row(&mut state, Panel::Tables, 0);
        assert_eq!(row_count(&state), 1, "and folded again");
    }

    #[test]
    fn a_click_on_a_table_reads_it() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        click_row(&mut state, Panel::Tables, 0);

        let effects = click_row(&mut state, Panel::Tables, 1);

        assert!(
            matches!(
                effects.as_slice(),
                [Effect::Execute { .. }, Effect::RecordHistory(_)]
            ),
            "got {effects:?}"
        );
        assert_eq!(
            state.tables.cursor.index(),
            1,
            "on the row that was clicked"
        );
    }

    #[test]
    fn a_click_on_a_grid_row_moves_the_cursor_and_asks_for_nothing() {
        let mut state = showing_rows(5);

        let effects = click_row(&mut state, Panel::Contents, 3);

        assert!(effects.is_empty(), "a selection runs no query");
        assert_eq!(state.contents.cursor.index(), 3);
    }

    #[test]
    fn a_click_focuses_the_panel_it_landed_in() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus = Panel::Queries;

        click_row(&mut state, Panel::Tables, 0);

        assert_eq!(state.focus, Panel::Tables, "one click focuses and acts");
        assert_eq!(row_count(&state), 2, "the keyspace unfolded on that click");
    }

    #[test]
    fn a_click_on_chrome_focuses_and_does_nothing_else() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let before = state.tables.clone();

        let effects = point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

        assert!(effects.is_empty());
        assert_eq!(
            state.tables, before,
            "chrome moves no selection and folds nothing"
        );
    }

    #[test]
    fn a_click_on_a_row_that_has_gone_is_dropped_rather_than_clamped() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        click_row(&mut state, Panel::Tables, 0);
        click_row(&mut state, Panel::Tables, 0);

        // Row 2 was `users` a frame ago; the tree is one row long now.
        let effects = click_row(&mut state, Panel::Tables, 2);

        assert!(effects.is_empty(), "a stale row reads nothing");
        assert_eq!(state.tables.cursor.index(), 0, "and moves nothing");
    }

    #[test]
    fn a_click_dismisses_the_overlay_and_does_nothing_else() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.overlay = Some(failure());

        let effects = click_row(&mut state, Panel::Tables, 0);

        assert!(state.overlay.is_none());
        assert!(effects.is_empty());
        assert_eq!(
            row_count(&state),
            1,
            "the keyspace the overlay covered did not unfold"
        );
    }

    #[test]
    fn a_wheel_scrolls_the_panel_under_it_without_focusing_it() {
        let mut state = showing_rows(20);
        state.geometry = Geometry::new([8, 8, 8, 1], 0, 0);
        state.focus = Panel::Tables;

        point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Contents,
                direction: Wheel::Down,
            },
        );

        assert_eq!(state.contents.scroll.offset(), 3, "three rows a notch");
        assert_eq!(
            state.contents.cursor.index(),
            0,
            "the wheel does not move the selection"
        );
        assert_eq!(state.focus, Panel::Tables, "focus stays where it was");
    }

    #[test]
    fn a_wheel_up_walks_back_the_way_it_came() {
        let mut state = showing_rows(20);
        state.geometry = Geometry::new([8, 8, 8, 1], 0, 0);
        let down = Pointer::Wheel {
            panel: Panel::Contents,
            direction: Wheel::Down,
        };
        point(&mut state, down);
        point(&mut state, down);

        point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Contents,
                direction: Wheel::Up,
            },
        );

        assert_eq!(state.contents.scroll.offset(), 3);
    }

    #[test]
    fn a_wheel_over_the_ddl_panel_scrolls_its_lines() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.geometry = Geometry::new([8, 8, 8, 1], 40, 0);

        point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Ddl,
                direction: Wheel::Down,
            },
        );

        assert_eq!(state.ddl.scroll.offset(), 3);
    }

    #[test]
    fn a_wheel_over_a_panel_with_nothing_to_scroll_is_inert() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus = Panel::Queries;
        for character in "one\ntwo\nthree\nfour\nfive\n".chars() {
            press(
                &mut state,
                if character == '\n' {
                    Key::Enter
                } else {
                    Key::Char(character)
                },
            );
        }
        state.focus = Panel::Tables;
        let cursor_before = state.queries.cursor();
        let text_before = state.queries.text();

        let effects = point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Queries,
                direction: Wheel::Down,
            },
        );

        assert!(effects.is_empty());
        assert_eq!(state.tables.cursor.index(), 0, "no other panel moved");
        assert_eq!(state.focus, Panel::Tables);
        assert_eq!(
            state.queries.cursor(),
            cursor_before,
            "the wheel did not move the editor's cursor"
        );
        assert_eq!(
            state.queries.text(),
            text_before,
            "the wheel did not touch the buffer"
        );
    }

    #[test]
    fn a_wheel_over_tables_moves_the_viewport_rather_than_the_selection() {
        // Four keyspaces, so three rows a notch would have landed on the
        // last one under the old cursor-moving wheel. Now nothing about the
        // tree selection changes, so `described()` sees no subject change
        // and `state.ddl.scroll` is left alone (design §7).
        let mut state = browsing(&["a", "b", "c", "d"]);
        state.geometry = Geometry::new([3, 3, 3, 1], 0, 0);
        state.focus = Panel::Queries;
        state.ddl.scroll.down(100, 2);
        let ddl_before = state.ddl.scroll;
        assert_ne!(ddl_before.offset(), 0, "the fixture must start scrolled");

        let effects = point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Tables,
                direction: Wheel::Down,
            },
        );

        assert!(effects.is_empty(), "a scroll alone asks for nothing");
        assert_eq!(state.tables.scroll.offset(), 1, "four rows, three visible");
        assert_eq!(state.tables.cursor.index(), 0, "the selection did not move");
        assert_eq!(state.focus, Panel::Queries, "the wheel does not move focus");
        assert_eq!(
            state.ddl.scroll, ddl_before,
            "no selection changed, so nothing new is described"
        );
    }

    #[test]
    fn a_wheel_does_not_dismiss_the_overlay() {
        let mut state = showing_rows(20);
        state.geometry = Geometry::new([8, 8, 8, 1], 0, 0);
        state.overlay = Some(failure());

        point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Contents,
                direction: Wheel::Down,
            },
        );

        assert!(state.overlay.is_some(), "only a click dismisses");
        assert_eq!(state.contents.cursor.index(), 0, "and nothing scrolled");
    }

    /// A Contents panel showing `count` rows in a five-row viewport.
    fn scrolling_rows(count: usize) -> State {
        let mut state = showing_rows(count);
        state.geometry = Geometry::new([5, 5, 5, 1], 0, 0);
        state
    }

    /// Named apart from the production `wheel`, which has the same parameter
    /// list and would otherwise be shadowed by this inside `use super::*`.
    fn roll(state: &mut State, panel: Panel, direction: Wheel) -> Vec<Effect> {
        point(state, Pointer::Wheel { panel, direction })
    }

    #[test]
    fn a_wheel_scrolls_the_view_and_leaves_the_selection_where_it_was() {
        let mut state = scrolling_rows(40);

        roll(&mut state, Panel::Contents, Wheel::Down);

        assert_eq!(state.contents.scroll.offset(), 3, "three rows a notch");
        assert_eq!(
            state.contents.cursor.index(),
            0,
            "the selection is where the user put it"
        );
    }

    #[test]
    fn a_wheel_up_walks_the_view_back() {
        let mut state = scrolling_rows(40);
        roll(&mut state, Panel::Contents, Wheel::Down);
        roll(&mut state, Panel::Contents, Wheel::Down);

        roll(&mut state, Panel::Contents, Wheel::Up);

        assert_eq!(state.contents.scroll.offset(), 3);
    }

    #[test]
    fn a_wheel_stops_at_both_ends() {
        let mut state = scrolling_rows(8);

        for _ in 0..10 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        assert_eq!(
            state.contents.scroll.offset(),
            3,
            "eight rows in a five-row panel scroll by three and stop"
        );

        for _ in 0..10 {
            roll(&mut state, Panel::Contents, Wheel::Up);
        }
        assert_eq!(state.contents.scroll.offset(), 0);
    }

    #[test]
    fn a_motion_after_a_wheel_brings_the_selection_back_into_view() {
        let mut state = scrolling_rows(40);
        state.focus = Panel::Contents;
        for _ in 0..5 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        assert_eq!(state.contents.scroll.offset(), 15, "scrolled well past it");

        press(&mut state, Key::Char('j'));

        assert_eq!(state.contents.cursor.index(), 1);
        assert_eq!(
            state.contents.scroll.offset(),
            1,
            "the view came back to the selection, the shortest distance `follow` allows"
        );
    }

    #[test]
    fn walking_off_the_bottom_still_drags_the_view_with_it() {
        let mut state = scrolling_rows(40);
        state.focus = Panel::Contents;

        for _ in 0..6 {
            press(&mut state, Key::Char('j'));
        }

        assert_eq!(state.contents.cursor.index(), 6);
        assert_eq!(
            state.contents.scroll.offset(),
            2,
            "the sixth row is the last of a five-row viewport"
        );
    }

    #[test]
    fn a_click_on_a_scrolled_row_needs_no_correction() {
        let mut state = scrolling_rows(40);
        for _ in 0..4 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        let offset = state.contents.scroll.offset();

        click_row(&mut state, Panel::Contents, offset + 1);

        assert_eq!(state.contents.cursor.index(), offset + 1);
        assert_eq!(
            state.contents.scroll.offset(),
            offset,
            "the row was on screen, so the view had no reason to move"
        );
    }

    #[test]
    fn a_list_that_shrank_pulls_its_offset_back() {
        let mut state = browsing_tables(&[("shop", &["a", "b", "c", "d", "e", "f"])]);
        state.geometry = Geometry::new([3, 3, 3, 1], 0, 0);
        click_row(&mut state, Panel::Tables, 0);
        for _ in 0..3 {
            roll(&mut state, Panel::Tables, Wheel::Down);
        }
        assert_eq!(state.tables.scroll.offset(), 4, "seven rows, three visible");

        // Folding takes the tree back to one row, which no offset can be past.
        click_row(&mut state, Panel::Tables, 0);

        assert_eq!(row_count(&state), 1);
        assert_eq!(state.tables.scroll.offset(), 0);
    }

    #[test]
    fn a_wheel_over_tables_no_longer_scrolls_the_ddl_panel_back() {
        let mut state = browsing_tables(&[("shop", &["a", "b", "c", "d", "e", "f"])]);
        state.geometry = Geometry::new([3, 3, 3, 1], 40, 0);
        click_row(&mut state, Panel::Tables, 0);
        state.ddl.scroll.page_down(40, 3);
        let before = state.ddl.scroll;
        assert_ne!(before.offset(), 0, "the fixture starts scrolled");

        roll(&mut state, Panel::Tables, Wheel::Down);

        assert_eq!(
            state.ddl.scroll, before,
            "the wheel moved no selection, so nothing is newly described"
        );
    }

    #[test]
    fn a_resize_brings_the_selection_back_into_view() {
        let mut state = scrolling_rows(40);
        state.focus = Panel::Contents;
        for _ in 0..6 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(state.contents.cursor.index(), 6);
        assert_eq!(state.contents.scroll.offset(), 2, "on the last drawn row");

        update(&mut state, Msg::Measured(Geometry::new([2, 2, 2, 1], 0, 0)));

        assert_eq!(
            state.contents.scroll.offset(),
            5,
            "a panel that lost rows draws its selection again"
        );
    }

    #[test]
    fn a_measurement_that_changes_no_height_leaves_a_wheel_alone() {
        let mut state = scrolling_rows(40);
        for _ in 0..5 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        let scrolled = state.contents.scroll.offset();
        assert_eq!(scrolled, 15);

        update(&mut state, Msg::Measured(Geometry::new([5, 5, 5, 1], 0, 0)));

        assert_eq!(
            state.contents.scroll.offset(),
            scrolled,
            "the selection is off screen and the resize follow must not fetch it"
        );
        assert_eq!(state.contents.cursor.index(), 0);
    }

    #[test]
    fn a_resize_that_moves_a_viewport_asks_for_a_repaint() {
        let mut state = scrolling_rows(8);
        for _ in 0..5 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        assert_eq!(state.contents.scroll.offset(), 3);
        state.dirty = false;

        // Eight rows in an eight-row panel leave nothing to scroll.
        update(&mut state, Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0)));

        assert_eq!(state.contents.scroll.offset(), 0);
        assert!(
            state.dirty,
            "a measurement marks nothing dirty by itself, so an offset it \
             moved would otherwise not be drawn until the next keypress"
        );
    }

    /// Every `Key` the domain understands.
    ///
    /// The `match` is dead weight at runtime and load-bearing at compile time:
    /// a variant added to `Key` without a line here fails to compile, so the
    /// sweep below cannot quietly stop sweeping.
    fn every_key() -> Vec<Key> {
        match Key::Enter {
            Key::Char(_)
            | Key::Enter
            | Key::Escape
            | Key::Backspace
            | Key::Tab
            | Key::BackTab
            | Key::Delete
            | Key::Insert
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
            | Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::F(_) => {}
        }

        [
            Key::Enter,
            Key::Escape,
            Key::Backspace,
            Key::Tab,
            Key::BackTab,
            Key::Delete,
            Key::Insert,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
        ]
        .into_iter()
        .chain((1..=12).map(Key::F))
        .chain((0x20..=0x7e_u8).map(|byte| Key::Char(char::from(byte))))
        .collect()
    }

    fn catalogued() -> Vec<Key> {
        CATALOGUE
            .iter()
            .flat_map(|group| group.bindings)
            .flat_map(|binding| binding.keys)
            .copied()
            .collect()
    }

    /// States that between them exercise every binding: one with an overlay to
    /// dismiss, an unfolded keyspace to fold, and the tree at its first row;
    /// one focused somewhere other than Tables so the focus letters differ;
    /// one with the tree folded and moved off its first row, so `k`, `pgup`
    /// and `home` all have somewhere to go; and one with the palette open,
    /// the one state that captures keys rather than passing them to a panel
    /// — a sweep that never opens it cannot see what it captures.
    ///
    /// The unfold in `covered` matters: with every keyspace already folded,
    /// `h` and `←` would be a no-op in all four and the sweep would call them
    /// unbound. `moved`'s keyspaces are childless, so `l` and `→` never
    /// register there either — `covered` is the only fixture that opens a
    /// keyspace with something in it, and so the only one where those two
    /// keys do anything.
    fn sweep_fixtures() -> Vec<State> {
        let geometry = Geometry::new([8, 14, 20, 1], 37, 14);

        let mut covered = browsing_tables(&[("shop", &["orders"])]);
        covered.geometry = geometry;
        press(&mut covered, Key::Char('l'));
        covered.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

        let mut elsewhere = browsing_tables(&[("shop", &["orders"])]);
        elsewhere.geometry = geometry;
        elsewhere.focus = Panel::Ddl;

        let mut moved = measured(&["a", "b", "c"]);
        press(&mut moved, Key::Char('j'));
        press(&mut moved, Key::Char('j'));

        let mut palette_open = measured(&["a", "b", "c"]);
        press(&mut palette_open, Key::Char('?'));

        vec![covered, elsewhere, moved, palette_open]
    }

    /// Whether pressing `key` changes anything a user could observe.
    ///
    /// The view model rather than the `State`, which is not `PartialEq` and
    /// whose `dirty` flag `handle_key` sets on every press. It is also the
    /// better question: a key that changes nothing visible and emits no effect
    /// is not a binding, whatever it touched on the way through.
    fn does_something(state: &State, key: Key) -> bool {
        let mut after = state.clone();
        let effects = press(&mut after, key);

        !effects.is_empty()
            || after.should_quit != state.should_quit
            || crate::view::view_model(&after) != crate::view::view_model(state)
    }

    #[test]
    fn no_key_does_anything_the_palette_does_not_list() {
        let catalogued = catalogued();

        for state in sweep_fixtures() {
            for key in every_key() {
                if catalogued.contains(&key) {
                    continue;
                }

                assert!(
                    !does_something(&state, key),
                    "{key:?} does something and doc/shortcuts.md's palette does not list it"
                );
            }
        }
    }

    #[test]
    fn every_key_the_palette_lists_does_something() {
        let fixtures = sweep_fixtures();

        for key in catalogued() {
            assert!(
                fixtures.iter().any(|state| does_something(state, key)),
                "the palette lists {key:?} and no state responds to it"
            );
        }
    }

    fn searching(state: &State) -> Option<&Prompt> {
        match &state.overlay {
            Some(Overlay::Search(prompt)) => Some(prompt),
            _ => None,
        }
    }

    #[test]
    fn slash_opens_the_prompt_in_every_panel_that_can_hold_a_filter() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let mut state = browsing_tables(&[("shop", &["orders"])]);
            state.focus = panel;

            press(&mut state, Key::Char('/'));

            assert_eq!(
                searching(&state).map(Prompt::target),
                Filtered::of(panel),
                "{panel:?} must reach a prompt"
            );
        }
    }

    #[test]
    fn slash_is_text_in_queries_and_opens_nothing() {
        let mut state = State {
            focus: Panel::Queries,
            ..State::default()
        };

        press(&mut state, Key::Char('/'));

        assert!(state.overlay.is_none(), "the editor claims it first");
        assert_eq!(state.queries.text(), "/");
    }

    #[test]
    fn q_and_the_question_mark_type_themselves_in_the_prompt() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Char('q'));
        press(&mut state, Key::Char('?'));

        assert!(!state.should_quit, "q is text while a prompt is up");
        assert_eq!(searching(&state).map(Prompt::query), Some("q?"));
    }

    #[test]
    fn the_prompt_filters_live_per_keystroke() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Char('u'));

        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("u"),
            "the panel underneath is the preview"
        );
    }

    #[test]
    fn backspace_narrows_the_query_and_re_applies_it() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));
        press(&mut state, Key::Char('z'));

        press(&mut state, Key::Backspace);

        assert_eq!(searching(&state).map(Prompt::query), Some("o"));
        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("o")
        );
    }

    #[test]
    fn backspace_on_an_empty_query_is_inert_rather_than_closing() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Backspace);

        assert_eq!(searching(&state).map(Prompt::query), Some(""));
    }

    #[test]
    fn enter_commits_and_closes_leaving_the_filter_applied() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));

        press(&mut state, Key::Enter);

        assert!(state.overlay.is_none());
        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("o")
        );
    }

    #[test]
    fn an_empty_commit_clears_the_filter() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.apply_filter(Filtered::Tables, Filter::new("o"));
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Enter);

        assert!(state.overlay.is_none());
        assert_eq!(state.filter(Filtered::Tables), None);
    }

    #[test]
    fn escape_cancels_the_prompt_restoring_the_filter_that_was_there() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.apply_filter(Filtered::Tables, Filter::new("orders"));
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('u'));

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("orders"),
            "an abandoned prompt leaves the panel as it was found"
        );
    }

    #[test]
    fn escape_clears_the_focused_panels_filter_when_no_overlay_is_up() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus = Panel::Contents;
        state.apply_filter(Filtered::Contents, Filter::new("o"));

        press(&mut state, Key::Escape);

        assert_eq!(state.filter(Filtered::Contents), None);
    }

    #[test]
    fn escape_prefers_the_overlay_to_the_filter() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.apply_filter(Filtered::Tables, Filter::new("o"));
        state.overlay = Some(Overlay::Message(Message {
            title: "t".to_owned(),
            message: "m".to_owned(),
            severity: Severity::Info,
        }));

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("o"),
            "one press does one thing"
        );
    }

    #[test]
    fn escape_prefers_the_filter_to_leaving_the_editor() {
        // Unreachable today — Queries holds no filter (design §8) — so this
        // pins the ordering for the vim slice by checking the two lower
        // rungs still resolve in order.
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus = Panel::Queries;

        press(&mut state, Key::Escape);

        assert_eq!(state.focus, Panel::Tables);
    }

    #[test]
    fn tab_is_inert_while_the_prompt_is_up() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Tab);
        press(&mut state, Key::BackTab);

        assert_eq!(state.focus, Panel::Tables);
        assert!(searching(&state).is_some(), "and the prompt survives");
    }

    #[test]
    fn a_click_cancels_the_prompt() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.apply_filter(Filtered::Tables, Filter::new("orders"));
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('u'));

        point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

        assert!(state.overlay.is_none());
        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("orders"),
            "a click that lands somewhere unexpected must not leave state behind"
        );
    }

    #[test]
    fn a_filter_survives_a_focus_change() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));
        press(&mut state, Key::Enter);

        press(&mut state, Key::Tab);
        press(&mut state, Key::Char('t'));

        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("o")
        );
    }
}
