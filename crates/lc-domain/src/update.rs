//! The one place state changes.
//!
//! `update` is synchronous, pure, and total. It mutates state and *describes*
//! what should happen next; it cannot open a socket because it has nothing to
//! open one with (architecture.md §3).

use crate::commands::CommandsPanel;
use crate::contents::{ContentsState, SelectionMode, Subject, TableRef};
use crate::ddl::DdlTab;
use crate::export::{ExportForm, Fetched};
use crate::grid::{self, ColumnScroll};
use crate::model::{ConnId, DbError, Key, KeyPress, Page, PagingState, QueryId, Schema, Statement};
use crate::pointer::{Click, Pointer, Wheel};
use crate::posture::Posture;
use crate::search::{Filtered, Prompt};
use crate::state::{
    ConnectionState, Cursor, Geometry, Message, Overlay, Panel, Range, SchemaState, Scroll,
    Selection, State,
};
use crate::style::Severity;
use crate::theme::ThemeChoice;
use crate::tree::Node;

/// How many rows one look at a table brings back.
///
/// A wall rather than a window until paging lands
/// (docs/superpowers/specs/2026-08-09-table-contents-design.md §3).
pub const ROW_LIMIT: usize = 200;

/// Everything that can happen. Async results re-enter here through the same
/// channel as a keystroke.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Input(KeyPress),
    /// A pointer gesture, resolved by `lc-inbound` against the frame it
    /// happened in (docs/superpowers/specs/2026-08-09-mouse-input-design.md §5).
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
    /// The external editor exited cleanly; this is what it saved
    /// (docs/superpowers/specs/2026-08-12-external-editor-design.md §3).
    EditorClosed {
        buffer: String,
    },
    /// It could not be run, or it exited non-zero. The buffer is untouched.
    EditorFailed {
        reason: String,
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
    /// Hand `buffer` to the user's editor and report what comes back. The one
    /// effect `lc-app` runs on the main loop rather than as a task: the child
    /// needs the terminal, so nothing else may draw while it holds it
    /// (design §4).
    OpenEditor {
        buffer: String,
    },
    /// Put the yanked line on the system clipboard. Write-only: OSC 52 gives
    /// no answer, so there is no `Msg` back and nothing to report
    /// (docs/superpowers/specs/2026-08-13-yank-design.md §3, §4).
    Yank(String),
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
    let column_scroll = state.contents.column_scroll;

    let effects = apply(state, msg);

    // The Ddl offset belongs to the node being described, so a new subject
    // opens at its top rather than halfway down the last one. Stated once here
    // rather than at each of the four paths that can move the tree selection.
    if described(state) != subject {
        state.ddl.cursor.to_start();
        state.ddl.scroll.to_start();
        state.ddl.selection.clear();
    }

    clamp_lists(state);
    // Before the viewport follows it, so a page that lost columns cannot
    // strand either.
    state
        .contents
        .column
        .clamp_to(state.contents.column_count());
    follow_moved_cursors(state, cursors);
    follow_grid_columns(state);

    // `Msg::Measured` marks nothing dirty by itself, so a clamp it provoked
    // would otherwise not be drawn until the next unrelated keypress.
    if (state.tables.scroll, state.contents.scroll) != scrolls
        || state.contents.column_scroll != column_scroll
    {
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
            raise(
                state,
                Message {
                    title: "Connection failed".to_owned(),
                    message: error.to_string(),
                    severity: Severity::Danger,
                },
            );
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
                raise(
                    state,
                    Message {
                        title: "Schema unavailable".to_owned(),
                        message: error.to_string(),
                        severity: Severity::Danger,
                    },
                );
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

        Msg::EditorClosed { buffer } => editor_closed(state, &buffer),

        Msg::EditorFailed { reason } => editor_failed(state, reason),

        Msg::Quit => quit(state),
    }
}

fn editor_closed(state: &mut State, buffer: &str) -> Vec<Effect> {
    state.queries.replace(buffer);
    state.mark_dirty();
    Vec::new()
}

fn editor_failed(state: &mut State, reason: String) -> Vec<Effect> {
    raise(
        state,
        Message {
            title: "Editor".to_owned(),
            message: reason,
            severity: Severity::Danger,
        },
    );
    state.mark_dirty();
    Vec::new()
}

fn quit(state: &mut State) -> Vec<Effect> {
    state.should_quit = true;
    let cancels = state.in_flight.keys().copied().map(Effect::Cancel);

    cancels.chain(std::iter::once(Effect::Quit)).collect()
}

/// What the focused panel has under its cursor, as one line — which of the
/// panel's contents that is, and whether it is CSV, is the panel's answer.
/// Nothing under the cursor yanks nothing: there is nothing there to say
/// anything about (yank design §1).
///
/// A payload holding nothing but `,` and `\n` raises no effect either.
/// `Effect::Yank` becomes an OSC 52 sequence, and an empty payload asks the
/// terminal to clear its clipboard, so a projection with nothing but
/// delimiters to say must say nothing rather than wipe what the user had
/// copied (selection-modes design §4). Every projection converges here, so no
/// shape can reach the clipboard empty.
fn yank(state: &State) -> Vec<Effect> {
    let line = match state.focus {
        Panel::Contents => contents(state),
        Panel::Tables => tree_nodes(state),
        Panel::Ddl => detail_rows(state),
        Panel::Queries => None,
    };

    // A payload of nothing but delimiters says nothing, and `Effect::Yank`
    // becomes an OSC 52 sequence: an empty one asks the terminal to clear the
    // clipboard (design §5).
    line.filter(|line| line.chars().any(|c| c != ',' && c != '\n'))
        .map(Effect::Yank)
        .into_iter()
        .collect()
}

/// The range, or the one row the cursor is on when nothing is selected.
fn spanned(selection: Selection, cursor: usize) -> Range {
    selection
        .range(cursor)
        .unwrap_or_else(|| Range::new(cursor, cursor))
}

/// Which of the grid's two cursors `y` means is what the selection mode
/// answers (docs/superpowers/specs/2026-08-13-yank-selection-modes-design.md §1).
fn contents(state: &State) -> Option<String> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };
    let page = page.page();
    let rows = spanned(state.contents.rows, state.contents.cursor.index());
    let columns = spanned(state.contents.columns, state.contents.column.index());

    match state.contents.mode {
        SelectionMode::Row => Some(crate::yank::grid_block(
            &page.rows,
            rows,
            Range::new(0, page.columns.len().saturating_sub(1)),
        )),
        SelectionMode::Cell => {
            if rows.rows() == 1 && columns.rows() == 1 {
                let row = page.rows.get(rows.first())?;
                let value = row.values.get(columns.first())?;

                return crate::yank::grid_cell(value);
            }

            Some(crate::yank::grid_block(&page.rows, rows, columns))
        }
        SelectionMode::Column => Some(crate::yank::grid_columns(&page.rows, columns)),
    }
}

fn tree_nodes(state: &State) -> Option<String> {
    let schema = state.schema()?;
    let rows = state.tables.rows(schema);
    let range = spanned(state.tables.selection, state.tables.cursor.index());

    Some(crate::yank::lines(
        rows.iter()
            .enumerate()
            .filter(|(index, _)| range.contains(*index))
            .map(|(_, node)| crate::yank::node(node)),
    ))
}

fn detail_rows(state: &State) -> Option<String> {
    let details = crate::view::details_of(state)?;
    let range = spanned(state.ddl.selection, state.ddl.cursor.index());

    Some(crate::yank::lines(
        details
            .rows()
            .enumerate()
            .filter(|(index, _)| range.contains(*index))
            .map(|(_, row)| crate::yank::detail_row(row)),
    ))
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

    // alt turns a key into a separate binding and none is bound, so it is
    // refused rather than reinterpreted — including on top of ctrl, where a
    // two-modifier chord is a third layer and no layer is designed. ctrl is
    // refused the same way, past the arms `ctrl_key` claims. shift is not
    // checked: a shifted letter already arrives as a different Key::Char, and
    // a shifted arrow is left as a synonym for the plain one so it stays free
    // for a future "extend selection" binding.
    if press.modifiers.alt {
        return Vec::new();
    }

    if press.modifiers.ctrl {
        return ctrl_key(state, press.key);
    }

    // An open palette takes the keys it uses
    // (docs/superpowers/specs/2026-08-09-command-palette-design.md §7).
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return palette_key(state, press.key);
    }

    // The theme picker takes motion, not text, the same as the palette
    // (design §6).
    if matches!(state.overlay, Some(Overlay::Themes { .. })) {
        return themes_key(state, press.key);
    }

    // A prompt takes text, so it claims every printable key — the
    // panel-that-takes-text rule, applied to an overlay (design §1.1).
    if matches!(state.overlay, Some(Overlay::Search(_))) {
        return search_key(state, press.key);
    }

    // The form takes text on one of its four stops, so it claims every
    // printable key the way a prompt does — and hands most of them nowhere on
    // the other three (export design §2).
    if matches!(state.overlay, Some(Overlay::Export(_))) {
        return export_key(state, press.key);
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
        Key::Enter => return activate(state),
        Key::Char('l') | Key::Right => go_right(state),
        Key::Char('h') | Key::Left => go_left(state),
        Key::Char('/') => open_prompt(state),
        Key::Char('y') => return yank(state),
        Key::Char('m') => {
            if state.focus == Panel::Contents {
                // A range's axis is the mode's axis, so carrying one across
                // would address rows or columns the user never chose — the
                // argument `DdlPanel::show` already makes for the cursor
                // (design §3).
                state.contents.rows.clear();
                state.contents.columns.clear();
                state.contents.mode = state.contents.mode.next();
            }
        }
        Key::Char('v') => toggle_visual(state),
        // Last, so every binding above keeps precedence over a panel key. A
        // DDL tab is a place the same way a panel is, so its key both focuses
        // the panel and draws the tab — one press to arrive, whichever tab was
        // last up (docs/shortcuts.md §4). Checked first because `d` is both
        // the panel's letter and its first tab's.
        Key::Char(key) => show_tab(state, key),
        _ => {}
    }

    Vec::new()
}

/// Rows a wheel notch moves, in whatever unit the panel scrolls in. One is
/// sluggish enough that the pointer feels broken; three is what terminals
/// settled on (design §6).
const WHEEL_ROWS: usize = 3;

/// The one place a pointer gesture becomes behaviour, as `handle_key` is for
/// keys (docs/shortcuts.md §2).
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
/// (docs/superpowers/specs/2026-08-09-list-viewport-design.md §1).
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

    // A press names one row, so it spends the range rather than standing
    // beside it and making the next `y` lie (design §4).
    for selection in selections(state, click.panel()).into_iter().flatten() {
        selection.clear();
    }

    let (panel, row, column) = match click {
        Click::Row { panel, row, column } => (panel, Some(row), column),
        Click::Column { panel, column } => (panel, None, Some(column)),
        // A tab is a place, and its key is how one is reached: this is the
        // `Key::Char` arm of `handle_key`, reached by pointer (design §1).
        Click::Tab { shortcut, .. } => {
            show_tab(state, shortcut);
            return Vec::new();
        }
        Click::Panel(_) => return Vec::new(),
    };

    // Neither cursor changes the selection mode. A press names whatever cells
    // it can, and the mode is what decides which of them the grid draws a band
    // around — `m` is still the only way to change that.
    if let Some(column) = column {
        let columns = state.contents.column_count();
        if panel == Panel::Contents && column < columns {
            state.contents.column.go_to(column, columns);
        }
    }

    let Some(row) = row else {
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
        // The offset the viewport owes this cursor is `Msg::Measured`'s to
        // compute, exactly as it is after `j`: only the renderer knows which
        // lines the row wrapped to.
        Panel::Ddl => {
            state.ddl.cursor.go_to(row, len);
            Vec::new()
        }
        // Queries is an editor: it has no rows for a press to land on.
        Panel::Queries => Vec::new(),
    }
}

/// Focuses the panel a tab's key names, and draws that tab.
///
/// Shared with [`handle_key`]'s `Key::Char` arm so a tab reached by pointer and
/// one reached by key cannot diverge.
fn show_tab(state: &mut State, shortcut: char) {
    if let Some(tab) = DdlTab::from_shortcut(shortcut) {
        state.focus = Panel::Ddl;
        // A tab the strip does not draw is not a place to arrive at: `c` on a
        // keyspace focuses the panel and no more.
        if crate::view::ddl_tabs(state).contains(&tab) {
            state.ddl.show(tab);
        }
    } else if let Some(panel) = Panel::from_shortcut(shortcut) {
        state.focus = panel;
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
        Key::Enter => editor.insert_newline(),
        Key::Escape | Key::Tab | Key::BackTab | Key::Insert | Key::F(_) => return false,
    }

    true
}

/// The way a message overlay is installed. Every site goes through here —
/// `ConnectFailed`, `SchemaFailed`, and `run`'s destructive-statement refusal.
///
/// A direct `state.overlay = Some(Overlay::Message(_))` replaces a live search
/// prompt without restoring its `previous`, stranding the target under
/// whatever the half-typed query had applied and losing the only copy of what
/// was there before (panel search design §3.1). Cancelling the prompt first is
/// the same guarantee `Esc` gives, and it belongs to the installation rather
/// than to each caller's memory of it.
///
/// That no current site can *observe* the difference is incidental: a message
/// arrives on connect, before a prompt can be open, and `run` gates on Queries
/// having focus, which no prompt can coexist with. Design §8 anticipates
/// Queries gaining a filter, at which point a prompt over it is constructible.
///
/// A live theme picker is unwound for the same reason and by the same rule:
/// its cursor writes `State.theme` as it moves, so replacing it outright
/// would strand the frame in a half-previewed theme with nothing left saying
/// which one the user had chosen (themes design §6). [`restore_theme`] is
/// what puts it back, and — unlike the prompt above — a message *can* arrive
/// over an open picker today, since the picker outlives no focus condition.
fn raise(state: &mut State, message: Message) {
    if let Some(Overlay::Search(prompt)) = &state.overlay {
        state.apply_filter(prompt.target(), prompt.previous().cloned());
    }
    restore_theme(state);
    state.overlay = Some(Overlay::Message(message));
}

/// Puts back the theme a live picker opened on, if the overlay is that
/// picker. Otherwise a no-op. Three sites unwind a preview: [`escape`], which
/// does it while taking the overlay apart, and [`raise`] and the
/// resize-shrink close in [`measured`], which share this one — so the second
/// of those two cannot copy the check and diverge from the first.
fn restore_theme(state: &mut State) {
    if let Some(Overlay::Themes { restored, .. }) = state.overlay {
        state.theme = restored;
    }
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
/// `Escape` is decided before the prompt's mutable borrow is taken: it defers
/// to [`escape`], which needs `&mut State` whole, and no reborrow of it can
/// coexist with `prompt`. `Enter` stays inside the match because it only needs
/// `&mut state.overlay`, and `prompt` is dead in that arm.
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

/// `Esc` resolves in one order, first hit wins: the overlay; then freezing an
/// extending range; then clearing a frozen range; then the focused panel's
/// filter; then leaving a panel that takes text (Queries hands focus back to
/// Tables). One press does one of the five, never two — the thing most
/// recently put on screen comes off first (panel search design §6).
fn escape(state: &mut State) {
    match state.overlay.take() {
        Some(Overlay::Search(prompt)) => {
            state.apply_filter(prompt.target(), prompt.previous().cloned());
            return;
        }
        Some(Overlay::Themes { restored, .. }) => {
            state.theme = restored;
            return;
        }
        Some(Overlay::Message(_) | Overlay::Commands(_) | Overlay::Export(_)) => return,
        None => {}
    }

    // Above the filter for the reason the overlay is above both: the thing
    // most recently put on screen comes off first (design §1).
    if stop_extending(state) || clear_selection(state) {
        return;
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

/// `ctrl-t` opens the theme picker, seated on the theme in force.
///
/// Refused where the frame cannot draw a list, for the reason [`open_palette`]
/// gives. Gated on the palette's own room rather than on a measurement of its
/// own: the picker is the smaller block, so a frame that holds a palette holds
/// a picker.
///
/// Refused over a live prompt too, which no unmodified key needs to say: the
/// ctrl layer is dispatched above every overlay, so this is the one opener a
/// prompt cannot swallow, and replacing one would strand the panel under a
/// half-typed query with nothing left holding the filter it had.
fn open_themes(state: &mut State) {
    if state.geometry.command_rows() == 0 || matches!(state.overlay, Some(Overlay::Search(_))) {
        return;
    }

    let mut cursor = Cursor::default();
    cursor.go_to(state.theme.index(), ThemeChoice::ALL.len());

    state.overlay = Some(Overlay::Themes {
        cursor,
        restored: state.theme,
    });
}

/// What an open picker responds to.
///
/// Moving the cursor sets the theme, so the frame underneath is the preview —
/// the same live-as-you-type rule the search prompt follows (panel search
/// design §1).
///
/// `q` still quits, for the reason [`palette_key`] gives. Paging is the whole
/// of the picker's long-range motion: the list is every theme there is and
/// always shorter than the block drawing it, so one page is the whole list and
/// a separate `Home`/`End` would be a second key for a landing `PageUp` and
/// `PageDown` already make in one press (docs/shortcuts.md §1).
fn themes_key(state: &mut State, key: Key) -> Vec<Effect> {
    match key {
        Key::Char('q') => return quit(state),
        Key::Enter => {
            state.overlay = None;
            return Vec::new();
        }
        Key::Escape => {
            escape(state);
            return Vec::new();
        }
        _ => {}
    }

    let last = ThemeChoice::ALL.len();
    let height = state.geometry.command_rows();
    let Some(Overlay::Themes { cursor, .. }) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => cursor.down(last),
        Key::Char('k') | Key::Up => cursor.up(),
        Key::PageDown => cursor.page_down(last, height),
        Key::PageUp => cursor.page_up(height),
        _ => {}
    }

    if let Some(Overlay::Themes { cursor, .. }) = &state.overlay
        && let Some(choice) = ThemeChoice::ALL.get(cursor.index())
    {
        state.theme = *choice;
    }

    Vec::new()
}

#[derive(Clone, Copy)]
enum Transition {
    Open,
    Close,
}

/// Applies `motion` to the focused panel's selection.
///
/// The focused panel and no other. A wheel names the panel under the pointer
/// and moves its viewport directly ([`wheel`]), so nothing but a key reaches
/// here.
///
/// In Ddl the height is counted in wrapped lines and the cursor in rows, so a
/// page overshoots on a run of wrapped values. Accepted rather than corrected:
/// the fix needs a line-to-row map from the renderer, and `home`/`end` are
/// exact regardless (design §4).
fn scroll(state: &mut State, motion: Motion) {
    let panel = state.focus;
    let height = state.geometry.height(panel);
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

/// The ctrl layer's whole key table (docs/shortcuts.md §2).
///
/// Dispatched above the palette check, so it honours that rule itself rather
/// than inheriting it: routing a modified key into `palette_key`, which
/// matches on the key alone, would make `ctrl-j` move the palette cursor.
/// Each arm gates on its own panel below.
fn ctrl_key(state: &mut State, key: Key) -> Vec<Effect> {
    // Paging is decided above the refusal below: `ctrl-d`/`ctrl-u` are a
    // second spelling of `PageDown`/`PageUp` rather than members of their own,
    // so they reach whatever those two reach, an open overlay included.
    match key {
        Key::Char('d') => return page(state, Key::PageDown),
        Key::Char('u') => return page(state, Key::PageUp),
        _ => {}
    }

    if matches!(
        state.overlay,
        Some(Overlay::Commands(_) | Overlay::Themes { .. })
    ) {
        return Vec::new();
    }

    match key {
        Key::Char('r') => run(state),
        Key::Char('t') => {
            open_themes(state);
            Vec::new()
        }
        Key::Char('e') => ctrl_e(state),
        _ => Vec::new(),
    }
}

/// `ctrl-e` opens the user's editor on the Queries buffer and the export form
/// over the Contents grid. Two meanings, each gated on its own panel — the
/// shape `ctrl-r` already has (export design §1).
fn ctrl_e(state: &mut State) -> Vec<Effect> {
    match state.focus {
        Panel::Queries => open_editor(state),
        Panel::Contents => {
            open_export(state);
            Vec::new()
        }
        Panel::Tables | Panel::Ddl => Vec::new(),
    }
}

/// `ctrl-e` opens the user's editor on the Queries buffer, and does nothing
/// anywhere else: the panel takes text, so the action has no unmodified key
/// available (external editor design §1).
fn open_editor(state: &State) -> Vec<Effect> {
    if state.focus != Panel::Queries {
        return Vec::new();
    }

    vec![Effect::OpenEditor {
        buffer: state.queries.text(),
    }]
}

/// Refused over a grid with no rows.
fn open_export(state: &mut State) {
    let ContentsState::Loaded { subject, page } = state.contents.state() else {
        return;
    };

    let rows = page.page().rows.len();
    if rows == 0 {
        return;
    }

    let form = ExportForm::opened(subject.clone(), Fetched::of(rows), &state.cwd);
    state.overlay = Some(Overlay::Export(form));
    state.dirty = true;
}

/// What [`export_key`]'s form-only match decided, resolved against `state`
/// once the form's borrow has ended.
enum Outcome {
    Nothing,
    Close,
    Run,
}

/// Every key the export form claims.
///
/// The confirmation route is checked before the form is borrowed mutably, so
/// it never holds one; the rest of the table runs inside a scoped block that
/// borrows `form` and reports an [`Outcome`], leaving `state` free again for
/// [`start_export`] and the overlay assignment below — `palette_key` and
/// `search_key` split the same borrow the same way.
fn export_key(state: &mut State, key: Key) -> Vec<Effect> {
    let confirming =
        matches!(&state.overlay, Some(Overlay::Export(form)) if form.confirm().is_some());
    if confirming {
        return confirm_key(state, key);
    }

    let outcome = {
        let Some(Overlay::Export(form)) = &mut state.overlay else {
            return Vec::new();
        };

        match key {
            Key::Escape => Outcome::Close,
            Key::Tab => {
                form.tab();
                Outcome::Nothing
            }
            Key::BackTab => {
                form.back_tab();
                Outcome::Nothing
            }
            Key::Char(' ') if !form.stop().takes_text() => {
                if form.activate() {
                    Outcome::Run
                } else {
                    Outcome::Nothing
                }
            }
            Key::Enter if !form.stop().takes_text() => Outcome::Run,
            Key::Char('j') | Key::Down if !form.stop().takes_text() => {
                form.down();
                Outcome::Nothing
            }
            Key::Char('k') | Key::Up if !form.stop().takes_text() => {
                form.up();
                Outcome::Nothing
            }
            _ if form.stop().takes_text() => {
                edit_path(form, key);
                Outcome::Nothing
            }
            _ => Outcome::Nothing,
        }
    };

    state.dirty = true;

    match outcome {
        Outcome::Nothing => Vec::new(),
        Outcome::Close => {
            state.overlay = None;
            Vec::new()
        }
        Outcome::Run => start_export(state),
    }
}

/// The path stop's keys. Every printable key is a character there and the
/// editing keys move within the line (design §2).
fn edit_path(form: &mut ExportForm, key: Key) {
    let line = form.path_mut();

    match key {
        Key::Char(character) => line.insert(character),
        Key::Backspace => line.delete_back(),
        Key::Delete => line.delete_forward(),
        Key::Left => line.left(),
        Key::Right => line.right(),
        Key::Home => line.home(),
        Key::End => line.end(),
        _ => {}
    }
}

/// Filled in when `Effect::Export` exists (Task 7).
fn start_export(_state: &mut State) -> Vec<Effect> {
    Vec::new()
}

fn confirm_key(_state: &mut State, _key: Key) -> Vec<Effect> {
    Vec::new()
}

/// `ctrl-d`/`ctrl-u`, dispatched to whatever `key` would reach unmodified.
///
/// Delegating rather than paging here is what keeps the two spellings from
/// drifting: an overlay that learns to page learns it once.
fn page(state: &mut State, key: Key) -> Vec<Effect> {
    match &state.overlay {
        Some(Overlay::Commands(_)) => palette_key(state, key),
        Some(Overlay::Themes { .. }) => themes_key(state, key),
        // The prompt takes text and pages nothing, so neither spelling does;
        // the form has nothing paged either, whichever stop it is on.
        Some(Overlay::Search(_) | Overlay::Export(_)) => Vec::new(),
        Some(Overlay::Message(_)) | None => {
            if state.focus.takes_text() {
                edit(state, key);
            } else if matches!(key, Key::PageDown) {
                scroll(state, Motion::PageDown);
            } else {
                scroll(state, Motion::PageUp);
            }

            Vec::new()
        }
    }
}

/// How many rows `panel`'s *cursor* moves over. Queries never arrives here —
/// the editor claims the motion keys first (queries editor design §3) — so
/// its arm is unreachable in practice and kept for exhaustiveness.
fn row_count(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Tables => tree_rows(state),
        Panel::Ddl => crate::view::ddl_rows(state),
        Panel::Contents => state.contents.row_count(),
        Panel::Queries => 0,
    }
}

/// Queries never arrives here — the editor claims the motion keys before
/// this match is reached (design §3) — so its arm is unreachable in
/// practice and kept for exhaustiveness.
fn panel_cursor(state: &mut State, panel: Panel) -> Option<&mut Cursor> {
    match panel {
        Panel::Tables => Some(&mut state.tables.cursor),
        Panel::Ddl => Some(&mut state.ddl.cursor),
        Panel::Contents => Some(&mut state.contents.cursor),
        Panel::Queries => None,
    }
}

/// The selections `panel` holds. Two for the grid, which selects on both axes
/// (design §3); none for Queries, which is an editor.
fn selections(state: &mut State, panel: Panel) -> [Option<&mut Selection>; 2] {
    match panel {
        Panel::Tables => [Some(&mut state.tables.selection), None],
        Panel::Ddl => [Some(&mut state.ddl.selection), None],
        Panel::Contents => match state.contents.mode {
            SelectionMode::Row => [Some(&mut state.contents.rows), None],
            SelectionMode::Column => [Some(&mut state.contents.columns), None],
            SelectionMode::Cell => [
                Some(&mut state.contents.rows),
                Some(&mut state.contents.columns),
            ],
        },
        Panel::Queries => [None, None],
    }
}

/// `v`: start extending from the cursor, or freeze what is being extended.
fn toggle_visual(state: &mut State) {
    let cursors = axis_cursors(state, state.focus);

    for (selection, cursor) in selections(state, state.focus)
        .into_iter()
        .zip(cursors)
        .filter_map(|(selection, cursor)| Some((selection?, cursor)))
    {
        if selection.is_extending() {
            selection.stop(cursor);
        } else {
            selection.start(cursor);
        }
    }
}

/// Where each of `panel`'s selections has its far end, in the order
/// [`selections`] returns them.
fn axis_cursors(state: &State, panel: Panel) -> [usize; 2] {
    match panel {
        Panel::Contents => match state.contents.mode {
            SelectionMode::Column => [state.contents.column.index(), 0],
            SelectionMode::Row | SelectionMode::Cell => {
                [state.contents.cursor.index(), state.contents.column.index()]
            }
        },
        Panel::Tables | Panel::Ddl | Panel::Queries => [cursor_index(state, panel), 0],
    }
}

/// `esc`'s second rung: leave visual mode, keeping the range. Reports whether
/// there was one to leave.
fn stop_extending(state: &mut State) -> bool {
    let cursors = axis_cursors(state, state.focus);
    let mut stopped = false;

    for (selection, cursor) in selections(state, state.focus)
        .into_iter()
        .zip(cursors)
        .filter_map(|(selection, cursor)| Some((selection?, cursor)))
    {
        if selection.is_extending() {
            selection.stop(cursor);
            stopped = true;
        }
    }

    stopped
}

/// `esc`'s third rung: drop the range. Reports whether there was one.
fn clear_selection(state: &mut State) -> bool {
    let mut cleared = false;

    for selection in selections(state, state.focus).into_iter().flatten() {
        if selection.range(0).is_some() {
            selection.clear();
            cleared = true;
        }
    }

    cleared
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
    // (docs/superpowers/specs/2026-08-09-list-viewport-design.md §5).
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

    // Follows here rather than in `update`'s epilogue because the epilogue's
    // geometry describes the *previous* frame, and so the previously selected
    // row: there is no correct offset for it to compute (design §5.4).
    //
    // Unconditional, and safe from the wheel without a guard: a wheel changes
    // no height, no wrapped-line count and no span, so `lc-app` sends no
    // measurement at all. The same covers ctrl-e and ctrl-y.
    let before = state.ddl.scroll;
    let height = geometry.height(Panel::Ddl);
    state
        .ddl
        .scroll
        .follow_span(geometry.ddl_selection(), height);
    state.ddl.scroll.clamp_to(geometry.ddl_lines(), height);

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
    // (design §5.1). The picker shares the gate it opened under, so it
    // shares this closing too — and restores the theme it opened on, the
    // same as `Esc`, rather than stranding the preview.
    if geometry.command_rows() == 0 {
        restore_theme(state);
        if matches!(
            state.overlay,
            Some(Overlay::Commands(_) | Overlay::Themes { .. })
        ) {
            state.overlay = None;
            state.mark_dirty();
        }
    }

    Vec::new()
}

/// The panels whose viewport `update` follows by index. Ddl keeps its own: its
/// offset counts wrapped lines rather than rows, so it is followed by span
/// in `measured` instead (design §5.1).
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
        Panel::Ddl => state.ddl.cursor.index(),
        Panel::Contents => state.contents.cursor.index(),
        Panel::Queries => 0,
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

    // Not in the loop above: `LISTS` is the panels whose offsets are followed
    // by index, and Ddl's offset counts lines while its cursor counts rows
    // (design §5.1). The clamp is unconditional for the same reason the loop
    // is — a schema reload that shrinks a table changes no geometry, so no
    // measurement arrives to clamp against.
    state.ddl.cursor.clamp_to(crate::view::ddl_rows(state));

    // Beside the cursor clamps and for the same reason: a schema reload or a
    // fold that shrinks a list changes no geometry, so no measurement arrives
    // to clamp against (design §2).
    state.tables.selection.clamp_to(tree_rows(state));
    state.ddl.selection.clamp_to(crate::view::ddl_rows(state));

    state.contents.rows.clamp_to(state.contents.row_count());
    state
        .contents
        .columns
        .clamp_to(state.contents.column_count());
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
    if state.tables.selects_many() {
        return Vec::new();
    }

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

/// Replaces whatever the Contents panel is showing with a read of `table`,
/// and takes focus there.
///
/// Focus moves now rather than when the page lands: it is a consequence of the
/// keypress, and a focus change that waits on the network happens at a time
/// the user cannot predict
/// (docs/superpowers/specs/2026-08-10-table-display-design.md §1).
fn read(state: &mut State, table: TableRef) -> Vec<Effect> {
    state.focus = Panel::Contents;

    let stmt = Statement::select_all(&table.keyspace, &table.table, ROW_LIMIT);
    submit_into_contents(state, Subject::Table(table), stmt)
}

/// `ctrl-r`: the Queries buffer, whole, as one statement (design §3).
fn run(state: &mut State) -> Vec<Effect> {
    if state.focus != Panel::Queries {
        return Vec::new();
    }

    let text = state.queries.text();
    let cql = text.trim();
    if cql.is_empty() {
        return Vec::new();
    }

    let stmt = Statement::typed(cql);

    // Refused rather than executed: vision.md §4 principle 6 makes typed
    // confirmation the condition on these running at all, and the flow that
    // asks for it is unbuilt. This call site becomes the prompt (design §5).
    if stmt.kind.is_destructive() {
        raise(
            state,
            Message {
                title: "Not run".to_owned(),
                message:
                    "Dropping and truncating wait on a typed confirmation, which is not built yet."
                        .to_owned(),
                severity: Severity::Danger,
            },
        );
        return Vec::new();
    }

    submit_into_contents(state, Subject::Typed(cql.to_owned()), stmt)
}

/// Puts `stmt`'s result in the Contents panel, cancelling whatever it was
/// waiting on. One grid, one outstanding query, whichever way it was asked for.
///
/// Focus is deliberately not touched here. Reading from the tree moves it
/// ([`read`]); `ctrl-r` does not, because the user is composing in a panel
/// that takes text and pulling focus out of it would turn their next
/// keystrokes into bindings (docs/superpowers/specs/2026-08-10-table-display-design.md §1).
fn submit_into_contents(state: &mut State, subject: Subject, stmt: Statement) -> Vec<Effect> {
    let mut effects = Vec::new();

    // Dropping the id is what actually stops a late page from being applied;
    // the effect is still inert (architecture.md §3).
    if let Some(superseded) = state.contents.outstanding() {
        state.in_flight.remove(&superseded);
        effects.push(Effect::Cancel(superseded));
    }

    match submit(state, stmt) {
        Ok((query, submitted)) => {
            state.contents.requested(query, subject);
            effects.extend(submitted);
        }
        Err(error) => state.contents.refused(subject, error),
    }

    effects
}

/// `l` and `→`: into the tree, or on to the next column.
///
/// The four horizontal keys mean *move left* and *move right*, and each panel
/// resolves that in its own vocabulary — the shape `j`/`k` already have
/// (docs/shortcuts.md §2, docs/superpowers/specs/2026-08-10-table-display-design.md §2).
fn go_right(state: &mut State) {
    match state.focus {
        Panel::Tables => transition(state, Transition::Open),
        Panel::Contents => {
            let len = state.contents.column_count();
            state.contents.column.down(len);
        }
        Panel::Ddl | Panel::Queries => {}
    }
}

fn go_left(state: &mut State) {
    match state.focus {
        Panel::Tables => transition(state, Transition::Close),
        Panel::Contents => state.contents.column.up(),
        Panel::Ddl | Panel::Queries => {}
    }
}

/// The columns the grid is drawing over, and the width they have.
///
/// `None` when there is no page: nothing to fit, and nothing to follow.
fn grid_columns(state: &State) -> Option<(&[u16], u16)> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };

    let gutter = grid::gutter(page.page().rows.len());
    let width = u16::try_from(state.geometry.grid_width()).unwrap_or(u16::MAX);

    Some((page.widths(), grid::available(width, gutter)))
}

/// Pulls the column viewport over the selected cell, and back inside a page
/// that narrowed or a panel that grew.
///
/// Unconditional, for [`clamp_lists`]'s reason: a new page changes no
/// geometry, so no [`Msg::Measured`] arrives to clamp against.
fn follow_grid_columns(state: &mut State) {
    let column = state.contents.column.index();

    let Some((widths, available)) = grid_columns(state) else {
        state.contents.column_scroll = ColumnScroll::default();
        return;
    };

    // Moved out and back so the widths can stay borrowed from the page they
    // were measured with rather than be copied per tick.
    let mut columns = state.contents.column_scroll;
    // Clamp first, follow second, mirroring `clamp_lists` before
    // `follow_moved_cursors` on the row axis: the clamp answers "is the
    // offset too far right for this page and this width", which can leave the
    // cursor off screen, and the follow is what puts it back. The other order
    // would let the clamp undo the follow and hide the selected cell.
    columns.clamp_to(widths, available);
    columns.follow(column, widths, available);

    state.contents.column_scroll = columns;
}

fn transition(state: &mut State, transition: Transition) {
    if state.focus != Panel::Tables {
        return;
    }

    // The filter guard lives in TablesPanel::open/close themselves (design
    // §4.1), so every entry point — this one, and activate's click/Enter/Space
    // path — inherits it from one place rather than two that must agree.
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
    use crate::contents::{ContentsState, SelectionMode};
    use crate::ddl::DdlTab;
    use crate::model::{CqlValue, Modifiers, Row, StatementKind};
    use crate::pointer::{Click, Pointer, Wheel};
    use crate::search::{Filter, Filtered, Prompt};
    use crate::state::{Connection, Range, Span};
    use crate::theme::Ported;
    use crate::tree::tests_support::schema_of as tabled_schema;
    use crate::view::tests_support::tabled;

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

    #[test]
    fn a_message_arriving_over_an_open_prompt_restores_the_previous_filter() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(&[("shop", &["orders"])])),
            },
        );
        state.apply_filter(Filtered::Tables, Filter::new("shop"));

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));
        press(&mut state, Key::Char('r'));
        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("or"),
            "the live prompt is previewing its own buffer, not the previous filter"
        );

        update(
            &mut state,
            Msg::SchemaFailed {
                conn,
                error: DbError::Invalid("unauthorized".to_owned()),
            },
        );

        assert!(
            matches!(state.overlay, Some(Overlay::Message(_))),
            "the failure must still be shown"
        );
        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("shop"),
            "the half-typed query must not strand itself; the prompt's previous filter is restored"
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

    fn press_binding(state: &mut State, binding: KeyPress) -> Vec<Effect> {
        update(state, Msg::Input(binding))
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
            Overlay::Message(_)
            | Overlay::Search(_)
            | Overlay::Themes { .. }
            | Overlay::Export(_) => None,
        }
    }

    fn theme_picker(state: &State) -> Option<usize> {
        match state.overlay.as_ref()? {
            Overlay::Themes { cursor, .. } => Some(cursor.index()),
            _ => None,
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
    fn the_picker_opens_on_the_theme_in_force() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinFrappe);

        ctrl(&mut state, Key::Char('t'));

        assert_eq!(
            theme_picker(&state),
            Some(ThemeChoice::Ported(Ported::CatppuccinFrappe).index())
        );
    }

    #[test]
    fn moving_the_cursor_previews_the_theme() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.theme,
            ThemeChoice::ALL[1],
            "the frame repaints as you browse"
        );
    }

    #[test]
    fn enter_keeps_the_previewed_theme() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Enter);

        assert_eq!(state.theme, ThemeChoice::ALL[1]);
        assert!(state.overlay.is_none());
    }

    #[test]
    fn escape_restores_the_theme_the_picker_opened_on() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('k'));
        press(&mut state, Key::Escape);

        assert_eq!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));
        assert!(state.overlay.is_none());
    }

    #[test]
    fn the_picker_cursor_cannot_run_off_the_list() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        for _ in 0..ThemeChoice::ALL.len() * 2 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(state.theme, ThemeChoice::ALL[ThemeChoice::ALL.len() - 1]);
        assert_eq!(
            theme_picker(&state),
            Some(ThemeChoice::ALL.len() - 1),
            "the cursor itself must stop at the last row, not merely the theme it read out"
        );

        for _ in 0..ThemeChoice::ALL.len() * 2 {
            press(&mut state, Key::Char('k'));
        }
        assert_eq!(state.theme, ThemeChoice::ALL[0]);
        assert_eq!(theme_picker(&state), Some(0));
    }

    #[test]
    fn paging_reaches_the_ends_of_the_theme_list() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::PageDown);

        assert_eq!(
            theme_picker(&state),
            Some(ThemeChoice::ALL.len() - 1),
            "the list is shorter than the block drawing it, so one page is all of it"
        );
        assert_eq!(
            state.theme,
            ThemeChoice::ALL[ThemeChoice::ALL.len() - 1],
            "a page is a move, and every move previews"
        );

        press(&mut state, Key::PageUp);

        assert_eq!(theme_picker(&state), Some(0));
        assert_eq!(state.theme, ThemeChoice::ALL[0]);
    }

    #[test]
    fn ctrl_d_and_ctrl_u_page_the_picker_too() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        ctrl(&mut state, Key::Char('d'));

        assert_eq!(theme_picker(&state), Some(ThemeChoice::ALL.len() - 1));

        ctrl(&mut state, Key::Char('u'));

        assert_eq!(theme_picker(&state), Some(0));
    }

    #[test]
    fn the_ends_are_inert_in_the_picker() {
        for key in [Key::Home, Key::End] {
            let mut state = measured(&["a"]);
            ctrl(&mut state, Key::Char('t'));
            press(&mut state, Key::Char('j'));
            let before = crate::view::view_model(&state);

            press(&mut state, key);

            assert_eq!(
                crate::view::view_model(&state),
                before,
                "{key:?} would be a second name for the page the picker already takes"
            );
        }
    }

    #[test]
    fn q_still_quits_with_the_picker_open() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        let effects = press(&mut state, Key::Char('q'));

        assert!(
            state.should_quit,
            "a key that means quit everywhere but one overlay is not one anyone can trust"
        );
        assert!(!effects.is_empty(), "quitting cancels what is in flight");
    }

    /// The panel guards above sweep states with no overlay, so an overlay's
    /// own bar needs its own sweep — the picker's is the only bar in the
    /// program whose keys none of them reach.
    #[test]
    fn every_hint_the_pickers_bar_shows_does_something() {
        // Mid-list, so every direction and both ends have somewhere to go.
        let seated = || {
            let mut state = measured(&["a"]);
            ctrl(&mut state, Key::Char('t'));
            press(&mut state, Key::Char('j'));
            press(&mut state, Key::Char('j'));
            state
        };

        let shown = crate::view::hints::bar(&seated());
        let claimed = crate::view::hints::THEMES
            .iter()
            .chain(crate::view::hints::THEMES_GLOBAL);

        assert_eq!(
            shown.panel.len() + shown.global.len(),
            crate::view::hints::THEMES.len() + crate::view::hints::THEMES_GLOBAL.len()
        );

        for binding in claimed {
            for &key in binding.keys {
                let mut state = seated();
                let before = crate::view::view_model(&state);

                let effects = press_binding(&mut state, key);

                assert!(
                    crate::view::view_model(&state) != before
                        || !effects.is_empty()
                        || state.should_quit,
                    "the picker's bar offers '{} {}' and pressing it changes nothing",
                    crate::view::hints::label(binding.keys),
                    binding.action
                );
            }
        }
    }

    #[test]
    fn moving_the_picker_cursor_asks_for_a_repaint() {
        let mut state = measured(&["a"]);
        ctrl(&mut state, Key::Char('t'));
        state.dirty = false;

        press(&mut state, Key::Char('j'));

        assert!(
            state.dirty,
            "the preview is the frame underneath, which nothing else redraws"
        );
    }

    #[test]
    fn a_frame_too_small_for_a_list_refuses_the_picker() {
        let mut state = State::default();

        ctrl(&mut state, Key::Char('t'));

        assert!(
            state.overlay.is_none(),
            "a mode with nothing on screen saying so is worse than no mode"
        );
    }

    #[test]
    fn a_frame_that_shrinks_below_the_picker_closes_it_and_restores_the_theme() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('k'));
        assert_ne!(
            state.theme,
            ThemeChoice::Ported(Ported::CatppuccinMocha),
            "previewing a different theme before the frame shrinks"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
        );

        assert!(
            state.overlay.is_none(),
            "a picker nothing can draw must not still capture keys"
        );
        assert_eq!(
            state.theme,
            ThemeChoice::Ported(Ported::CatppuccinMocha),
            "the preview must not strand itself when its own room disappears"
        );
    }

    #[test]
    fn a_message_arriving_over_an_open_picker_restores_the_theme() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('k'));
        assert_ne!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));

        raise(
            &mut state,
            Message {
                title: "Cannot connect".to_owned(),
                message: "no route to host".to_owned(),
                severity: Severity::Danger,
            },
        );

        assert_eq!(
            state.theme,
            ThemeChoice::Ported(Ported::CatppuccinMocha),
            "the half-previewed theme must not strand itself under the failure"
        );
        assert!(matches!(state.overlay, Some(Overlay::Message(_))));
    }

    #[test]
    fn a_click_dismisses_the_picker_and_restores_the_theme() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('k'));
        assert_ne!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));

        point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

        assert!(state.overlay.is_none());
        assert_eq!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));
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

    fn ctrl(state: &mut State, key: Key) -> Vec<Effect> {
        update(state, Msg::Input(KeyPress::ctrl(key)))
    }

    #[test]
    fn ctrl_d_and_ctrl_u_page_the_focused_panel() {
        let mut paged = measured(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"]);
        let mut chorded = paged.clone();

        press(&mut paged, Key::PageDown);
        ctrl(&mut chorded, Key::Char('d'));

        assert_eq!(chorded.tables.cursor.index(), paged.tables.cursor.index());
        assert!(chorded.tables.cursor.index() > 0, "a page was available");

        press(&mut paged, Key::PageUp);
        ctrl(&mut chorded, Key::Char('u'));

        assert_eq!(chorded.tables.cursor.index(), paged.tables.cursor.index());
    }

    #[test]
    fn ctrl_d_pages_an_open_palette_rather_than_the_panel_underneath() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        ctrl(&mut state, Key::Char('d'));

        assert_eq!(
            palette(&state).expect("still open").selected(),
            15,
            "ctrl-d is pgdn by another name, and pgdn pages the palette"
        );
        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn ctrl_d_pages_the_editor_the_way_pgdn_does() {
        let mut paged = editing("a\nb\nc\nd\ne\nf");
        let mut chorded = paged.clone();

        press(&mut paged, Key::PageUp);
        ctrl(&mut chorded, Key::Char('u'));

        assert_eq!(chorded.queries.cursor(), paged.queries.cursor());
        assert_eq!(chorded.queries.text(), paged.queries.text(), "not text");
    }

    #[test]
    fn ctrl_d_is_inert_under_a_search_prompt() {
        let mut state = browsing(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        press(&mut state, Key::Char('/'));
        let before = crate::view::view_model(&state);

        ctrl(&mut state, Key::Char('d'));

        assert_eq!(
            crate::view::view_model(&state),
            before,
            "the prompt pages nothing, and the chord is not text either"
        );
    }

    #[test]
    fn ctrl_e_hands_the_queries_buffer_to_an_editor() {
        let mut state = State {
            focus: Panel::Queries,
            ..State::default()
        };
        state.queries.replace("SELECT * FROM t");

        let effects = ctrl(&mut state, Key::Char('e'));

        assert_eq!(
            effects,
            vec![Effect::OpenEditor {
                buffer: "SELECT * FROM t".to_owned()
            }]
        );
    }

    /// The chord means one thing now: it scrolled the Table DDL viewport as
    /// well until that half was retired (docs/shortcuts.md §4).
    #[test]
    fn ctrl_e_is_inert_outside_queries() {
        for focus in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let mut state = State {
                focus,
                ..State::default()
            };
            let before = crate::view::view_model(&state);

            assert!(
                ctrl(&mut state, Key::Char('e')).is_empty(),
                "{focus:?} claims no ctrl-e"
            );
            assert_eq!(
                crate::view::view_model(&state),
                before,
                "{focus:?} moved under a chord it does not claim"
            );
        }
    }

    #[test]
    fn ctrl_e_under_an_overlay_does_nothing() {
        let overlays = [
            Overlay::Commands(CommandsPanel::opened()),
            Overlay::Themes {
                cursor: Cursor::default(),
                restored: ThemeChoice::default(),
            },
        ];

        for overlay in overlays {
            let mut state = State {
                focus: Panel::Queries,
                overlay: Some(overlay),
                ..State::default()
            };

            assert!(ctrl(&mut state, Key::Char('e')).is_empty());
        }
    }

    #[test]
    fn a_closed_editor_replaces_the_buffer() {
        let mut state = State::default();
        state.queries.replace("SELECT");

        let effects = update(
            &mut state,
            Msg::EditorClosed {
                buffer: "SELECT *\nFROM t".to_owned(),
            },
        );

        assert!(effects.is_empty());
        assert_eq!(state.queries.text(), "SELECT *\nFROM t");
        assert!(state.dirty, "the buffer changed, so the frame is stale");
    }

    #[test]
    fn a_failed_editor_says_so_and_keeps_the_buffer() {
        let mut state = State::default();
        state.queries.replace("SELECT");

        let effects = update(
            &mut state,
            Msg::EditorFailed {
                reason: "could not run hx: No such file or directory".to_owned(),
            },
        );

        assert!(effects.is_empty());
        assert_eq!(state.queries.text(), "SELECT");
        assert!(matches!(
            state.overlay,
            Some(Overlay::Message(Message {
                severity: Severity::Danger,
                ..
            }))
        ));
    }

    /// A state focused on Queries with `text` typed into it, `\n` meaning
    /// Enter — the same convention `editor.rs`'s own helper uses.
    fn editing(text: &str) -> State {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.focus = Panel::Queries;
        type_into(&mut state, text);
        state
    }

    /// Types `text` into the focused editor, `\n` meaning Enter — the same
    /// convention `editor.rs`'s own helper uses.
    fn type_into(state: &mut State, text: &str) {
        for character in text.chars() {
            if character == '\n' {
                press(state, Key::Enter);
            } else {
                press(state, Key::Char(character));
            }
        }
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
    fn y_is_text_in_the_editor() {
        let mut state = editing("");

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(state.queries.text(), "y");
        assert!(effects.is_empty(), "not reachable from Queries (§1)");
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
    /// on printable keys (docs/shortcuts.md §2).
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

        press(&mut state, Key::Char('b'));

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

    /// `browsing_tables` with the cursor stepped onto a table, which is what
    /// the Columns tab needs in order to exist.
    fn browsing_a_table() -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        state
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
    fn enter_toggles_a_keyspace() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

        let effects = press(&mut state, Key::Enter);
        assert_eq!(row_count(&state), 3, "⏎ did not unfold");
        assert!(
            effects.is_empty(),
            "unfolding is a pure state change and must emit no effect (table-list design §2)"
        );

        let effects = press(&mut state, Key::Enter);
        assert_eq!(row_count(&state), 1, "⏎ did not fold");
        assert!(
            effects.is_empty(),
            "folding is a pure state change and must emit no effect (table-list design §2)"
        );
    }

    #[test]
    fn enter_is_inert_while_more_than_one_row_is_selected() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Enter);

        assert!(
            effects.is_empty(),
            "a range names more than one thing to open"
        );
        assert_eq!(state.focus, Panel::Tables, "and focus stays put");
    }

    #[test]
    fn enter_still_reads_a_table_with_one_row_selected() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));

        let effects = press(&mut state, Key::Enter);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Execute { .. })),
            "pressing v must not blank the panel before anything has been extended"
        );
    }

    #[test]
    fn a_click_clears_the_range_it_lands_beside() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));

        update(
            &mut state,
            Msg::Pointer(Pointer::Click(Click::Row {
                panel: Panel::Tables,
                row: 2,
                column: None,
            })),
        );

        assert_eq!(
            state.tables.selection.range(state.tables.cursor.index()),
            None,
            "pointing at one row is saying: this one"
        );
        assert_eq!(state.tables.cursor.index(), 2);
    }

    /// One key for one action: `Space` was `Enter`'s synonym and is now free
    /// (docs/shortcuts.md §2, §3).
    #[test]
    fn space_is_not_a_second_name_for_enter() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

        let effects = press(&mut state, Key::Char(' '));

        assert_eq!(row_count(&state), 1, "space unfolded a keyspace");
        assert!(effects.is_empty(), "space read a table");
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

    /// A state focused on Contents, showing a page of `columns` columns each
    /// two characters wide, in a grid `width` characters across.
    fn grid_of(columns: usize, width: usize) -> State {
        grid_of_rows(columns, 1, width)
    }

    fn grid_of_rows(columns: usize, rows: usize, width: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: (0..columns).map(|i| format!("c{i}")).collect(),
                rows: (0..rows)
                    .map(|_| Row {
                        values: (0..columns).map(|_| CqlValue::Int(1)).collect(),
                    })
                    .collect(),
                next: None,
            },
        );
        state.focus = Panel::Contents;
        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(width)),
        );
        state
    }

    #[test]
    fn l_and_right_move_the_grid_cursor_one_column() {
        for key in [Key::Char('l'), Key::Right] {
            let mut state = grid_of(4, 80);

            press(&mut state, key);

            assert_eq!(state.contents.column.index(), 1, "{key:?}");
        }
    }

    #[test]
    fn h_and_left_move_the_grid_cursor_back_one_column() {
        for key in [Key::Char('h'), Key::Left] {
            let mut state = grid_of(4, 80);
            press(&mut state, Key::Char('l'));

            press(&mut state, key);

            assert_eq!(state.contents.column.index(), 0, "{key:?}");
        }
    }

    #[test]
    fn the_grid_cursor_cannot_run_off_either_edge_of_a_page() {
        let mut state = grid_of(2, 80);

        for _ in 0..5 {
            press(&mut state, Key::Char('l'));
        }
        assert_eq!(state.contents.column.index(), 1, "stops on the last column");

        for _ in 0..5 {
            press(&mut state, Key::Char('h'));
        }
        assert_eq!(state.contents.column.index(), 0, "stops on the first");
    }

    #[test]
    fn a_grid_with_no_page_swallows_horizontal_motion() {
        let mut state = State {
            focus: Panel::Contents,
            ..State::default()
        };

        press(&mut state, Key::Char('l'));

        assert_eq!(state.contents.column.index(), 0);
        assert_eq!(state.contents.column_scroll.first(), 0);
    }

    #[test]
    fn horizontal_motion_belongs_to_the_focused_panel_only() {
        let mut state = grid_of(4, 80);
        state.focus = Panel::Ddl;

        press(&mut state, Key::Char('l'));

        assert_eq!(state.contents.column.index(), 0);
    }

    #[test]
    fn the_column_viewport_follows_the_cell_cursor() {
        // Four columns of width 2 ("c0" and a one-digit value) with one space
        // between: 2+1+2 = 5 fits two columns in a grid 8 wide once the
        // merged bar+gutter block (bar plus a one-digit gutter) and the one
        // spacing column ahead of the values are taken off.
        let mut state = grid_of(4, 8);

        press(&mut state, Key::Char('l'));
        assert_eq!(
            state.contents.column_scroll.first(),
            0,
            "column 1 is already drawn"
        );

        press(&mut state, Key::Char('l'));
        assert_eq!(
            state.contents.column_scroll.first(),
            1,
            "the shortest move that draws column 2"
        );
    }

    #[test]
    fn a_narrower_frame_pulls_the_column_viewport_back_to_the_cursor() {
        let mut state = grid_of(4, 80);
        for _ in 0..3 {
            press(&mut state, Key::Char('l'));
        }
        assert_eq!(
            state.contents.column_scroll.first(),
            0,
            "all four fit at 80"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(8)),
        );

        assert_eq!(
            state.contents.column_scroll.first(),
            2,
            "the selected column must stay drawn after a resize"
        );
    }

    #[test]
    fn a_panel_that_grew_pulls_the_column_viewport_back() {
        let mut state = grid_of(4, 8);
        for _ in 0..3 {
            press(&mut state, Key::Char('l'));
        }
        assert_eq!(
            state.contents.column_scroll.first(),
            2,
            "two of four fit at 8"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
        );

        assert_eq!(
            state.contents.column_scroll.first(),
            0,
            "all four fit again, so none may be left hidden behind the offset"
        );
    }

    #[test]
    fn a_frame_too_small_to_draw_does_not_strand_the_column_viewport() {
        let mut state = grid_of(4, 80);
        for _ in 0..3 {
            press(&mut state, Key::Char('l'));
        }

        // What `render` reports when the terminal is below its minimum size:
        // nothing was drawn, so the grid has no width at all.
        update(&mut state, Msg::Measured(Geometry::default()));
        assert_eq!(
            state.contents.column_scroll.first(),
            3,
            "with no room, the cursor's column is the only one worth drawing"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
        );

        assert_eq!(
            state.contents.column_scroll.first(),
            0,
            "restoring the size undoes it, without the user pressing h"
        );
    }

    #[test]
    fn home_and_end_still_reach_the_ends_of_the_row_list() {
        let mut state = grid_of_rows(4, 12, 80);
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::End);

        assert_eq!(
            state.contents.cursor.index(),
            11,
            "End reaches the ends of the document, which in a grid is the rows"
        );
        assert_eq!(
            state.contents.column.index(),
            1,
            "and says nothing about the column axis"
        );

        press(&mut state, Key::Home);

        assert_eq!(state.contents.cursor.index(), 0, "and Home the other end");
        assert_eq!(
            state.contents.column.index(),
            1,
            "still on the column it was on"
        );
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

    /// Every entry point that can land on a retained keyspace row — not just
    /// `h`/`l` — must leave the unfold set alone while a filter is applied.
    /// `activate` (`Enter`, `Space`, and a click, which routes through it too)
    /// has no guard of its own; it inherits `TablesPanel::toggle`'s. Remove
    /// that guard and the `Key::Enter` case here fails: the filter's own
    /// projection reports `shop` as unfolded, `toggle` reads that as "fold
    /// it", and the real unfold set loses "shop" even though nothing moved
    /// on screen.
    #[test]
    fn no_tables_entry_point_writes_the_fold_state_under_a_filter() {
        enum Action {
            Key(Key),
            Click,
        }

        for action in [
            Action::Key(Key::Char('h')),
            Action::Key(Key::Char('l')),
            Action::Key(Key::Enter),
            Action::Key(Key::Char(' ')),
            Action::Click,
        ] {
            let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
            press(&mut state, Key::Char('l'));
            assert_eq!(row_count(&state), 3, "shop starts unfolded");

            state.apply_filter(Filtered::Tables, Filter::new("shop"));

            match action {
                Action::Key(key) => {
                    press(&mut state, key);
                }
                Action::Click => {
                    click_row(&mut state, Panel::Tables, 0);
                }
            }

            state.apply_filter(Filtered::Tables, None);

            assert_eq!(
                row_count(&state),
                3,
                "the fold state from before the filter must survive it untouched"
            );
        }
    }

    /// `Enter` still reads a table under a filter — the guard stops keyspace
    /// folding, not table activation.
    #[test]
    fn enter_still_reads_a_table_under_a_filter() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.apply_filter(Filtered::Tables, Filter::new("orders"));
        assert_eq!(row_count(&state), 2, "shop and orders both survive");

        state.tables.cursor.go_to(1, row_count(&state));
        let effects = press(&mut state, Key::Enter);

        assert!(
            !effects.is_empty(),
            "Enter on a table row under a filter must still submit a read"
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
    fn a_repeated_shortcut_leaves_a_panel_alone() {
        for panel in Panel::ALL.into_iter().filter(|panel| !panel.takes_text()) {
            let mut state = State::default();

            press(&mut state, Key::Char(panel.shortcut()));
            press(&mut state, Key::Char(panel.shortcut()));

            assert_eq!(state.focus, panel);
            assert_eq!(
                state.ddl.tab(),
                DdlTab::Ddl,
                "a focus letter moves focus and does nothing else, however often it is pressed"
            );
        }
    }

    #[test]
    fn c_reaches_the_columns_tab_from_wherever_focus_was() {
        for from in Panel::ALL.into_iter().filter(|panel| !panel.takes_text()) {
            // A table has to be selected, or the strip draws no Columns tab
            // for the key to arrive at.
            let mut state = browsing_a_table();
            state.focus = from;

            press(&mut state, Key::Char('c'));

            assert_eq!(state.focus, Panel::Ddl, "from {from:?}");
            assert_eq!(state.ddl.tab(), DdlTab::Columns, "from {from:?}");
        }
    }

    #[test]
    fn c_on_a_keyspace_focuses_the_panel_and_draws_no_columns_tab() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::Char('c'));

        assert_eq!(state.focus, Panel::Ddl);
        assert_eq!(
            state.ddl.tab(),
            DdlTab::Ddl,
            "a keyspace has no columns, so there is no tab to store"
        );
    }

    #[test]
    fn d_returns_the_panel_to_its_ddl_tab() {
        let mut state = browsing_a_table();

        press(&mut state, Key::Char('c'));
        press(&mut state, Key::Char('d'));

        assert_eq!(state.focus, Panel::Ddl);
        assert_eq!(state.ddl.tab(), DdlTab::Ddl);
    }

    #[test]
    fn c_is_text_in_a_panel_that_takes_text() {
        let mut state = State {
            focus: Panel::Queries,
            ..State::default()
        };

        press(&mut state, Key::Char('b'));

        assert_eq!(state.focus, Panel::Queries, "focus is the mode");
        assert_eq!(state.queries.text(), "b");
        assert_eq!(state.ddl.tab(), DdlTab::Ddl, "and no tab changed under it");
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
    fn space_reads_no_table() {
        let mut state = read_shop_orders();

        press(&mut state, Key::Char(' '));

        assert!(
            state.contents.outstanding().is_none(),
            "space is free, not a second name for ⏎"
        );
    }

    #[test]
    fn reading_a_second_table_cancels_the_first() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let first = state.contents.outstanding().expect("the panel is waiting");
        state.focus = Panel::Tables;
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
    fn reading_a_table_lands_focus_in_contents() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Enter);

        assert_eq!(state.focus, Panel::Contents);
    }

    #[test]
    fn a_click_on_a_table_lands_focus_in_contents_too() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;
        press(&mut state, Key::Char('l'));

        click_row(&mut state, Panel::Tables, 1);

        assert_eq!(state.focus, Panel::Contents);
    }

    #[test]
    fn focus_moves_before_the_page_arrives() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Enter);

        assert!(
            matches!(state.contents.state(), ContentsState::Loading { .. }),
            "focus is a consequence of the keypress, not of the network"
        );
        assert_eq!(state.focus, Panel::Contents);
    }

    #[test]
    fn a_refused_read_still_lands_focus_in_contents() {
        // No connection, so `submit` refuses before a query is made. `read`
        // is called directly because the tree can only select a table when
        // its schema is loaded, which itself requires a connection — the two
        // cannot be pulled apart through key presses.
        let mut state = tabled(&[("shop", &["orders"])]);
        state.connection = ConnectionState::Disconnected;
        state.focus = Panel::Tables;

        read(&mut state, TableRef::new("shop", "orders"));

        assert_eq!(state.focus, Panel::Contents);
    }

    #[test]
    fn unfolding_a_keyspace_leaves_focus_in_tables() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;

        press(&mut state, Key::Enter);

        assert_eq!(
            state.focus,
            Panel::Tables,
            "a fold is not a read and moves nothing"
        );
    }

    #[test]
    fn reading_a_table_leaves_the_tree_selection_where_it_was() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));
        let selected = state.tables.cursor.index();

        press(&mut state, Key::Enter);

        assert_eq!(state.tables.cursor.index(), selected);
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
    fn y_yanks_a_grid_row() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["id".to_owned(), "name".to_owned()],
                    rows: vec![Row {
                        values: vec![CqlValue::Int(7), CqlValue::Text("ada".to_owned())],
                    }],
                    next: None,
                }),
            },
        );

        state.contents.mode = SelectionMode::Row;
        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(effects, vec![Effect::Yank("7,ada".to_owned())]);
    }

    #[test]
    fn a_yanked_row_carries_every_column_of_the_page() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: vec![
                    "c0".to_owned(),
                    "c1".to_owned(),
                    "c2".to_owned(),
                    "c3".to_owned(),
                ],
                rows: vec![Row {
                    values: vec![
                        CqlValue::Text("v0".to_owned()),
                        CqlValue::Text("v1".to_owned()),
                        CqlValue::Text("v2".to_owned()),
                        CqlValue::Text("v3".to_owned()),
                    ],
                }],
                next: None,
            },
        );
        state.focus = Panel::Contents;
        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(7)),
        );

        let (widths, available) = grid_columns(&state).expect("a page is loaded");
        assert!(
            grid::fitting(widths, 0, available) < widths.len(),
            "the viewport must hide at least one column for this test to prove anything"
        );

        state.contents.mode = SelectionMode::Row;
        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("v0,v1,v2,v3".to_owned())]
        );
    }

    /// A page whose cell at row `r`, column `c` reads `r{r}c{c}`, so that a
    /// yank names which cell, row or column it came from rather than only how
    /// many commas it holds.
    fn grid_of_named_cells(columns: usize, rows: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: (0..columns).map(|c| format!("c{c}")).collect(),
                rows: (0..rows)
                    .map(|r| Row {
                        values: (0..columns)
                            .map(|c| CqlValue::Text(format!("r{r}c{c}")))
                            .collect(),
                    })
                    .collect(),
                next: None,
            },
        );
        state.focus = Panel::Contents;
        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
        );
        state
    }

    #[test]
    fn y_yanks_the_cell_in_cell_mode() {
        let mut state = grid_of_named_cells(3, 2);
        state.contents.mode = SelectionMode::Cell;
        state.contents.cursor.go_to(1, 2);
        state.contents.column.go_to(2, 3);

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            effects,
            vec![Effect::Yank("r1c2".to_owned())],
            "cell mode yanks the value the two cursors name, and no delimiter"
        );
    }

    /// The panel opens in row mode, so this is what `y` does by default
    /// (design §1).
    #[test]
    fn y_yanks_the_row_in_row_mode() {
        let mut state = grid_of_named_cells(3, 2);
        state.contents.cursor.go_to(1, 2);
        state.contents.column.go_to(2, 3);

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            effects,
            vec![Effect::Yank("r1c0,r1c1,r1c2".to_owned())],
            "row mode yanks every column of the row the row cursor names"
        );
    }

    #[test]
    fn y_yanks_the_column_in_column_mode() {
        let mut state = grid_of_named_cells(3, 2);
        state.contents.mode = SelectionMode::Column;
        state.contents.cursor.go_to(1, 2);
        state.contents.column.go_to(2, 3);

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            effects,
            vec![Effect::Yank("r0c2,r1c2".to_owned())],
            "column mode yanks every row of the column the column cursor names"
        );
    }

    /// An empty OSC 52 payload asks the terminal to clear its clipboard, so a
    /// `y` with nothing to say must say nothing (design §4).
    #[test]
    fn y_on_a_null_cell_raises_no_effect() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["id".to_owned()],
                    rows: vec![Row {
                        values: vec![CqlValue::Null],
                    }],
                    next: None,
                }),
            },
        );

        let effects = press(&mut state, Key::Char('y'));

        assert!(
            effects.is_empty(),
            "a null has no text, and an empty yank would wipe the clipboard"
        );
    }

    #[test]
    fn y_in_column_mode_on_a_page_with_no_rows_yanks_nothing() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["id".to_owned()],
                    rows: Vec::new(),
                    next: None,
                }),
            },
        );
        state.contents.mode = SelectionMode::Column;

        let effects = press(&mut state, Key::Char('y'));

        assert!(effects.is_empty(), "an empty column is nothing to yank");
    }

    #[test]
    fn y_in_column_mode_on_a_column_of_one_null_yanks_nothing() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["id".to_owned()],
                    rows: vec![Row {
                        values: vec![CqlValue::Null],
                    }],
                    next: None,
                }),
            },
        );
        state.contents.mode = SelectionMode::Column;

        let effects = press(&mut state, Key::Char('y'));

        assert!(
            effects.is_empty(),
            "a lone null field is an empty line, which would wipe the clipboard"
        );
    }

    /// An empty text is a value Cassandra holds and a null is not, but neither
    /// has anything to put on the clipboard (design §4).
    #[test]
    fn y_on_a_cell_holding_the_empty_string_raises_no_effect() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["name".to_owned()],
                    rows: vec![Row {
                        values: vec![CqlValue::Text(String::new())],
                    }],
                    next: None,
                }),
            },
        );

        let effects = press(&mut state, Key::Char('y'));

        assert!(
            effects.is_empty(),
            "an empty payload would ask the terminal to clear its clipboard"
        );
    }

    /// What is drawn is a window; what is yanked is the column (design §6).
    #[test]
    fn a_column_yank_carries_rows_the_viewport_hides() {
        let mut state = grid_of_rows(1, 20, 80);
        assert!(
            state.geometry.height(Panel::Contents) < state.contents.row_count(),
            "the viewport must hide at least one row for this test to prove anything"
        );

        state.contents.mode = SelectionMode::Column;

        let effects = press(&mut state, Key::Char('y'));

        let Some(Effect::Yank(line)) = effects.first() else {
            panic!("a loaded column yanks, got {effects:?}");
        };
        assert_eq!(
            line.matches(',').count(),
            19,
            "twenty rows make nineteen separators, however few are on screen"
        );
    }

    #[test]
    fn y_on_an_empty_panel_yanks_nothing() {
        let mut state = browsing_a_table();
        state.focus = Panel::Contents;

        assert!(press(&mut state, Key::Char('y')).is_empty());
    }

    #[test]
    fn y_on_a_filter_that_matched_nothing_yanks_nothing() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.apply_filter(Filtered::Tables, Filter::new("zzz"));

        assert!(press(&mut state, Key::Char('y')).is_empty());
    }

    #[test]
    fn y_yanks_a_ddl_row() {
        let mut state = browsing_a_table();
        state.focus = Panel::Ddl;

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("Keyspace,shop".to_owned())]
        );
    }

    #[test]
    fn y_yanks_a_column_row() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Char('c'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("id,uuid,partition".to_owned())]
        );

        press(&mut state, Key::Char('j'));

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("created_at,timestamp,clustering".to_owned())]
        );
    }

    #[test]
    fn y_yanks_a_tree_node() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("shop".to_owned())]
        );

        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("shop.orders".to_owned())]
        );
    }

    fn yanked(effects: &[Effect]) -> Option<&str> {
        effects.iter().find_map(|effect| match effect {
            Effect::Yank(line) => Some(line.as_str()),
            _ => None,
        })
    }

    #[test]
    fn y_yanks_every_row_of_a_range_one_per_line() {
        let mut state = grid_of_named_cells(2, 3);
        state.contents.mode = SelectionMode::Row;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(yanked(&effects), Some("r0c0,r0c1\nr1c0,r1c1"));
    }

    #[test]
    fn y_in_cell_mode_yanks_the_block_and_nothing_outside_it() {
        let mut state = grid_of_named_cells(3, 3);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('l'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(yanked(&effects), Some("r0c0,r0c1\nr1c0,r1c1"));
    }

    #[test]
    fn y_on_a_single_cell_still_yanks_it_bare() {
        let mut state = grid_of_named_cells(2, 2);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('v'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            yanked(&effects),
            Some("r0c0"),
            "one cell is not a range; adding quotes would put back the delimiter there is none of"
        );
    }

    #[test]
    fn y_in_column_mode_yanks_one_line_per_column() {
        let mut state = grid_of_named_cells(3, 2);
        state.contents.mode = SelectionMode::Column;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('l'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(yanked(&effects), Some("r0c0,r1c0\nr0c1,r1c1"));
    }

    #[test]
    fn y_yanks_every_node_of_a_tree_range() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(yanked(&effects), Some("shop\nshop.orders\nshop.users"));
    }

    #[test]
    fn a_payload_of_nothing_but_delimiters_reaches_no_clipboard() {
        // `grid_of_rows` fills its cells with `Int(1)`; this needs nulls, so
        // the page is built here rather than borrowed from it.
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: vec!["a".to_owned(), "b".to_owned()],
                rows: vec![
                    Row {
                        values: vec![CqlValue::Null, CqlValue::Null],
                    };
                    2
                ],
                next: None,
            },
        );
        state.focus = Panel::Contents;
        state.contents.mode = SelectionMode::Row;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Char('y'));

        assert!(
            effects.is_empty(),
            "a block of nulls says nothing, and an empty payload clears the clipboard"
        );
    }

    #[test]
    fn a_page_for_a_cancelled_query_never_reaches_the_panel() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let first = state.contents.outstanding().expect("the panel is waiting");
        state.focus = Panel::Tables;
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
    fn motion_in_one_panel_never_reaches_another() {
        let mut state = read_shop_orders();
        let tree_row = state.tables.cursor.index();

        state.focus = Panel::Ddl;
        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), tree_row);
        assert_eq!(state.contents.cursor.index(), 0);
    }

    #[test]
    fn motion_in_the_ddl_panel_walks_the_rows_it_describes() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        let rows = crate::view::ddl_rows(&state);
        assert!(rows > 2, "the fixture describes a key and some options");

        for _ in 0..rows + 3 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(
            state.ddl.cursor.index(),
            rows - 1,
            "j stops on the last row"
        );

        press(&mut state, Key::Home);
        assert_eq!(state.ddl.cursor.index(), 0);

        press(&mut state, Key::End);
        assert_eq!(state.ddl.cursor.index(), rows - 1);
    }

    #[test]
    fn describing_another_node_opens_the_ddl_panel_at_its_first_row() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        press(&mut state, Key::End);
        assert!(state.ddl.cursor.index() > 0);

        state.focus = Panel::Tables;
        press(&mut state, Key::Char('k'));

        assert_eq!(state.ddl.cursor.index(), 0);
        assert_eq!(state.ddl.scroll.offset(), 0);
    }

    #[test]
    fn a_measurement_pulls_the_viewport_onto_the_selected_row() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 4, 8, 1], 40, 0).with_ddl_selection(Span {
                first: 20,
                lines: 2,
            })),
        );

        assert_eq!(
            state.ddl.scroll.offset(),
            18,
            "the row measured at lines 20..22 has to be on screen in a four-line panel"
        );
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
    fn motion_in_the_ddl_panel_moves_its_cursor_not_its_focus() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;
        sized(&mut state, [0, 6, 0, 0], 20);
        let tree_row = state.tables.cursor.index();
        let rows = crate::view::ddl_rows(&state);

        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.ddl.cursor.index(), 2.min(rows - 1));
        assert_eq!(
            state.tables.cursor.index(),
            tree_row,
            "the tree must not move while another panel has focus"
        );

        press(&mut state, Key::End);

        assert_eq!(state.ddl.cursor.index(), rows - 1);
    }

    #[test]
    fn ddl_motion_still_moves_the_cursor_before_the_first_measurement() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;
        let rows = crate::view::ddl_rows(&state);
        assert!(
            rows > 1,
            "a keyspace's replication must have more than one row, or End \
             landing on rows - 1 == 0 would hold even if the cursor never moved"
        );

        press(&mut state, Key::PageDown);
        press(&mut state, Key::End);

        assert_eq!(
            state.ddl.cursor.index(),
            rows - 1,
            "the cursor counts rows, not the still-unmeasured panel height"
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

    /// A table scrolled to its last row and then left must not hand the next
    /// table its cursor position: the reader would open mid-document with
    /// nothing saying so.
    #[test]
    fn moving_the_tree_selection_returns_the_ddl_panel_to_the_top() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        state.focus = Panel::Ddl;
        press(&mut state, Key::End);
        assert!(state.ddl.cursor.index() > 0);

        state.focus = Panel::Tables;
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.ddl.cursor.index(),
            0,
            "users is a document of its own"
        );
    }

    #[test]
    fn a_new_subject_clears_the_ddl_range() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        state.focus = Panel::Ddl;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));
        assert!(state.ddl.selection.range(0).is_some());

        state.focus = Panel::Tables;
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.ddl.selection.range(state.ddl.cursor.index()),
            None,
            "the rows the range addressed belong to the node that was on screen"
        );
    }

    #[test]
    fn unfolding_a_keyspace_leaves_the_ddl_panel_where_it_is() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.focus = Panel::Ddl;
        press(&mut state, Key::End);
        let end = state.ddl.cursor.index();
        assert!(end > 0);
        state.focus = Panel::Tables;

        press(&mut state, Key::Char('l'));

        assert_eq!(
            state.ddl.cursor.index(),
            end,
            "shop is still the selected node, so the panel is still on its description"
        );

        // A second `l` steps onto shop's first table, which is a new subject.
        press(&mut state, Key::Char('l'));

        assert_eq!(state.ddl.cursor.index(), 0);
    }

    #[test]
    fn unfolding_a_keyspace_leaves_the_ddl_range_alone() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.focus = Panel::Ddl;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));
        let range = state.ddl.selection.range(state.ddl.cursor.index());
        assert!(range.is_some());
        state.focus = Panel::Tables;

        press(&mut state, Key::Char('l'));

        assert_eq!(
            state.ddl.selection.range(state.ddl.cursor.index()),
            range,
            "unfolding leaves the panel on the same subject, so it is not a change the range should die on"
        );
    }

    #[test]
    fn a_measurement_pulls_a_stale_scroll_offset_back_inside_the_panel() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;
        sized(&mut state, [0, 6, 0, 0], 40);
        state.ddl.scroll.to_end(40, 6);
        assert_eq!(state.ddl.scroll.offset(), 34);
        state.dirty = false;

        // The panel shrank to a wrapping of far fewer lines.
        sized(&mut state, [0, 6, 0, 0], 8);

        assert_eq!(state.ddl.scroll.offset(), 2);
        assert!(
            state.dirty,
            "the offset moved, so the frame must be repainted"
        );
    }

    #[test]
    fn the_ddl_cursor_clamps_when_the_selected_table_shrinks() {
        use crate::model::{Column, Keyspace, PrimaryKey, Replication, TableOptions};

        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let conn = current_conn(&state);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        state.focus = Panel::Ddl;
        press(&mut state, Key::End);
        assert!(
            state.ddl.cursor.index() > 0,
            "the fixture table has more than one row"
        );

        // Same keyspace and table name, so `described()` sees no subject
        // change — the clamp below is the only thing that can pull the
        // cursor back in.
        let shrunk = crate::model::Table {
            name: "orders".to_owned(),
            primary_key: PrimaryKey {
                partition: vec![Column {
                    name: "id".to_owned(),
                    cql_type: "uuid".to_owned(),
                }],
                clustering: Vec::new(),
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
            options: TableOptions::default(),
        };
        let schema = Schema {
            keyspaces: [(
                "shop".to_owned(),
                Keyspace {
                    name: "shop".to_owned(),
                    replication: Replication::SimpleStrategy { factor: 1 },
                    tables: [("orders".to_owned(), shrunk)].into_iter().collect(),
                },
            )]
            .into_iter()
            .collect(),
        };

        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema),
            },
        );

        let rows = crate::view::ddl_rows(&state);
        assert_eq!(
            rows, 3,
            "the keyspace, the table, one partition column and nothing else"
        );
        assert_eq!(
            state.ddl.cursor.index(),
            rows - 1,
            "the clamp must pull the cursor back even though no measurement arrived"
        );
    }

    /// A state where every motion has somewhere to go in both directions:
    /// two keyspaces with the second unfolded, a page of rows under the
    /// Contents cursor, and a DDL document longer than its panel. A cursor
    /// resting on 0 would make `k`/`h` and `home` inert and fail a correct
    /// hint. The Ddl viewport is likewise nudged off zero, or `ctrl-y` would
    /// find nothing to scroll up over.
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
            // Three columns, not `page_of`'s one: `h`/`l` need a middle
            // column to move away from in both directions for their hint to
            // be exercised.
            update(
                &mut state,
                Msg::PageReady {
                    query,
                    page: Box::new(Page {
                        columns: vec!["id".to_owned(), "name".to_owned(), "email".to_owned()],
                        rows: vec![
                            Row {
                                values: vec![CqlValue::Null, CqlValue::Null, CqlValue::Null],
                            };
                            20
                        ],
                        next: None,
                    }),
                },
            );
            state.contents.cursor.go_to(5, 20);
            state.contents.column.go_to(1, 3);
        }

        sized(&mut state, [3, 3, 3, 3], 40);
        let ddl_len = crate::view::ddl_rows(&state);
        state.ddl.cursor.page_down(ddl_len, 3);
        state.ddl.scroll.down(40, 3);
        state.focus = focus;

        state
    }

    #[test]
    fn the_editing_keys_do_nothing_outside_a_panel_that_takes_text() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            for key in [Key::Backspace, Key::Delete] {
                let mut state = populated(panel);
                let before = crate::view::view_model(&state);

                let effects = press(&mut state, key);

                assert!(
                    effects.is_empty() && crate::view::view_model(&state) == before,
                    "{key:?} belongs to the editor and {panel:?} does not take text"
                );
            }
        }
    }

    /// Live hints only. An inert one is drawn greyed and promises nothing —
    /// `ctrl-r` on an empty buffer is the one there is (query execution
    /// design §3) — so pressing it and demanding a change would be asserting
    /// the opposite of what the bar says.
    ///
    /// `hints::bar` renders the panel's bindings in order, which is what lets
    /// the two be zipped.
    #[test]
    fn every_hint_the_bar_shows_does_something() {
        for panel in Panel::ALL {
            let shown = crate::view::hints::bar(&populated(panel)).panel;
            assert_eq!(shown.len(), panel.hints().len());

            for (binding, hint) in panel.hints().iter().zip(&shown) {
                if hint.liveness == crate::view::Liveness::Inert {
                    continue;
                }

                for &key in binding.keys {
                    let mut state = populated(panel);
                    let before = crate::view::view_model(&state);

                    let effects = press_binding(&mut state, key);

                    assert!(
                        crate::view::view_model(&state) != before || !effects.is_empty(),
                        "{panel:?} offers '{} {}' and pressing it changes nothing",
                        crate::view::hints::label(binding.keys),
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

                    let effects = press_binding(&mut state, key);

                    assert!(
                        crate::view::view_model(&state) != before || !effects.is_empty(),
                        "'{} {}' does nothing with {panel:?} focused",
                        crate::view::hints::label(binding.keys),
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
        point(
            state,
            Pointer::Click(Click::Row {
                panel,
                row,
                column: None,
            }),
        )
    }

    fn click_cell(state: &mut State, panel: Panel, row: usize, column: usize) -> Vec<Effect> {
        point(
            state,
            Pointer::Click(Click::Row {
                panel,
                row,
                column: Some(column),
            }),
        )
    }

    /// `shop.orders` loaded with `count` single-column rows.
    fn showing_rows(count: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
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

    /// `shop.orders` loaded with one row of `columns` columns.
    fn showing_columns(columns: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: (0..columns).map(|i| format!("c{i}")).collect(),
                rows: vec![Row {
                    values: (0..columns)
                        .map(|i| CqlValue::Int(i64::try_from(i).expect("the fixture is small")))
                        .collect(),
                }],
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
    fn a_click_on_a_cell_moves_both_cursors() {
        let mut state = showing_columns(4);

        let effects = click_cell(&mut state, Panel::Contents, 0, 2);

        assert!(effects.is_empty(), "a selection runs no query");
        assert_eq!(state.contents.cursor.index(), 0);
        assert_eq!(state.contents.column.index(), 2);
    }

    /// A press names whatever cells it can reach; which of them the grid draws
    /// a band around is the mode's business, and `m` is the only thing that
    /// changes it.
    #[test]
    fn a_click_on_a_cell_leaves_the_selection_mode_alone() {
        for mode in [
            SelectionMode::Cell,
            SelectionMode::Row,
            SelectionMode::Column,
        ] {
            let mut state = showing_columns(4);
            state.contents.mode = mode;

            click_cell(&mut state, Panel::Contents, 0, 2);

            assert_eq!(state.contents.mode, mode, "from {mode:?}");
            assert_eq!(state.contents.column.index(), 2, "from {mode:?}");
        }
    }

    #[test]
    fn a_click_on_a_heading_moves_the_column_cursor_and_no_row() {
        let mut state = showing_columns(4);
        state.contents.cursor.go_to(0, 1);

        let effects = point(
            &mut state,
            Pointer::Click(Click::Column {
                panel: Panel::Contents,
                column: 3,
            }),
        );

        assert!(effects.is_empty());
        assert_eq!(state.focus, Panel::Contents);
        assert_eq!(state.contents.column.index(), 3);
        assert_eq!(state.contents.cursor.index(), 0, "a heading names no row");
    }

    #[test]
    fn a_click_on_a_column_that_has_gone_is_dropped_rather_than_clamped() {
        let mut state = showing_columns(2);
        state.contents.column.go_to(1, 2);

        point(
            &mut state,
            Pointer::Click(Click::Column {
                panel: Panel::Contents,
                column: 5,
            }),
        );

        assert_eq!(
            state.contents.column.index(),
            1,
            "a column the page no longer has moves nothing"
        );
    }

    #[test]
    fn a_click_on_a_ddl_row_moves_the_cursor() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        click_row(&mut state, Panel::Tables, 0);
        click_row(&mut state, Panel::Tables, 1);
        let rows = crate::view::ddl_rows(&state);
        assert!(rows > 2, "the fixture must describe something, got {rows}");

        let effects = click_row(&mut state, Panel::Ddl, 2);

        assert!(effects.is_empty(), "a selection runs no query");
        assert_eq!(state.focus, Panel::Ddl);
        assert_eq!(state.ddl.cursor.index(), 2);
    }

    #[test]
    fn a_click_on_a_ddl_row_that_has_gone_is_dropped_rather_than_clamped() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        click_row(&mut state, Panel::Tables, 0);

        // Nothing is selected, so the panel describes nothing and has no rows.
        let effects = click_row(&mut state, Panel::Ddl, 3);

        assert!(effects.is_empty());
        assert_eq!(state.ddl.cursor.index(), 0);
    }

    #[test]
    fn a_click_on_a_tab_focuses_its_panel_and_draws_it() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        click_row(&mut state, Panel::Tables, 0);
        click_row(&mut state, Panel::Tables, 1);
        state.focus = Panel::Queries;

        let effects = point(
            &mut state,
            Pointer::Click(Click::Tab {
                panel: Panel::Ddl,
                shortcut: 'c',
            }),
        );

        assert!(effects.is_empty());
        assert_eq!(state.focus, Panel::Ddl);
        assert_eq!(state.ddl.tab(), DdlTab::Columns);
    }

    /// The same reachability rule the key obeys: `c` on a keyspace focuses the
    /// panel and no more, so a press on a strip drawn a frame ago cannot reach
    /// a tab this frame has not got.
    #[test]
    fn a_click_on_a_tab_the_strip_no_longer_draws_only_focuses() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus = Panel::Queries;

        point(
            &mut state,
            Pointer::Click(Click::Tab {
                panel: Panel::Ddl,
                shortcut: 'c',
            }),
        );

        assert_eq!(state.focus, Panel::Ddl);
        assert_eq!(state.ddl.tab(), DdlTab::Ddl);
    }

    #[test]
    fn a_click_on_a_single_tab_panels_title_focuses_it() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus = Panel::Queries;

        point(
            &mut state,
            Pointer::Click(Click::Tab {
                panel: Panel::Tables,
                shortcut: Panel::Tables.shortcut(),
            }),
        );

        assert_eq!(state.focus, Panel::Tables);
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

    /// Every `KeyPress` worth sweeping for the palette's completeness claim.
    ///
    /// Unmodified, plus ctrl held: ctrl is the only modifier `handle_key`
    /// dispatches on (`ctrl_key`) rather than refusing outright, so it is the
    /// only one that can ever do something the palette fails to list. Alt is
    /// refused wholesale before any arm sees it and shift never reaches a
    /// letter — sweeping either would only double the cost of this test for
    /// no coverage a future binding could fail to have.
    fn every_press() -> Vec<KeyPress> {
        every_key()
            .into_iter()
            .flat_map(|key| [KeyPress::new(key), KeyPress::ctrl(key)])
            .collect()
    }

    fn catalogued() -> Vec<KeyPress> {
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
    /// and `home` all have somewhere to go; and one with the palette open and
    /// one with the theme picker open, the states that capture keys rather
    /// than passing them to a panel — a sweep that never opens one cannot see
    /// what it captures; and one focused on a loaded grid, without which the
    /// `Table contents` group's keys are only ever answered by the tree and
    /// `m` would go unswept.
    ///
    /// The unfold in `covered` matters: with every keyspace already folded,
    /// `h` and `←` would be a no-op in all four and the sweep would call them
    /// unbound. `moved`'s keyspaces are childless, so `l` and `→` never
    /// register there either — `covered` is the only fixture that opens a
    /// keyspace with something in it, and so the only one where those two
    /// keys do anything. `elsewhere`'s Ddl scroll starts off zero for the
    /// same reason `k`/`home` need `moved` off row zero: `ctrl-y` would be a
    /// no-op at the top of the document.
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
        // Off zero, or `ctrl-y` would find nothing to scroll up over here.
        elsewhere
            .ddl
            .scroll
            .down(geometry.ddl_lines(), geometry.height(Panel::Ddl));

        let mut moved = measured(&["a", "b", "c"]);
        press(&mut moved, Key::Char('j'));
        press(&mut moved, Key::Char('j'));

        let mut palette_open = measured(&["a", "b", "c"]);
        press(&mut palette_open, Key::Char('?'));

        // Off the first row, so `k` and `home` have somewhere to go here too.
        let mut picker_open = measured(&["a", "b", "c"]);
        ctrl(&mut picker_open, Key::Char('t'));
        press(&mut picker_open, Key::Char('j'));

        // A loaded grid, for `y` to have a row under the cursor.
        let mut yankable = read_shop_orders();
        press(&mut yankable, Key::Enter);
        let query = yankable
            .contents
            .outstanding()
            .expect("the panel is waiting");
        update(
            &mut yankable,
            Msg::PageReady {
                query,
                page: Box::new(page_of(3)),
            },
        );

        // Measured, so the columns `m` cycles the highlight over are drawn.
        let grid = grid_of_rows(4, 5, 80);

        vec![
            covered,
            elsewhere,
            moved,
            palette_open,
            picker_open,
            yankable,
            grid,
        ]
    }

    /// Whether pressing `key` changes anything a user could observe.
    ///
    /// The view model rather than the `State`, which is not `PartialEq` and
    /// whose `dirty` flag `handle_key` sets on every press. It is also the
    /// better question: a key that changes nothing visible and emits no effect
    /// is not a binding, whatever it touched on the way through.
    fn does_something(state: &State, press: KeyPress) -> bool {
        let mut after = state.clone();
        let effects = press_binding(&mut after, press);

        !effects.is_empty()
            || after.should_quit != state.should_quit
            || crate::view::view_model(&after) != crate::view::view_model(state)
    }

    #[test]
    fn no_key_does_anything_the_palette_does_not_list() {
        let catalogued = catalogued();

        for state in sweep_fixtures() {
            for press in every_press() {
                if catalogued.contains(&press) {
                    continue;
                }

                assert!(
                    !does_something(&state, press),
                    "{press:?} does something and docs/shortcuts.md's palette does not list it"
                );
            }
        }
    }

    /// An editing state, for the `Queries` group alone. It is kept out of
    /// `sweep_fixtures` because in a panel that takes text every printable key
    /// does something, which would tell
    /// `no_key_does_anything_the_palette_does_not_list` that the palette is
    /// missing the alphabet. What is true there instead is pinned by
    /// `a_letter_typed_in_queries_is_text_not_a_shortcut`.
    fn editor_fixture() -> State {
        let mut editor = editing("SELECT * FROM shop.orders");
        editor.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        // One step off the buffer's end: `Delete` has nothing to remove
        // sitting exactly there (`delete_at_the_end_of_the_buffer_does_nothing`
        // in editor.rs), and `End` from the end is a no-op. Off by one, `Home`,
        // `End`, and `Delete` all have somewhere to move from.
        editor.queries.left();
        editor
    }

    /// The `Queries` group is checked against the editor fixture *alone*, and
    /// every other group against `sweep_fixtures` alone. Both directions
    /// matter. A Global, Motion or Tables key pressed in the editor types
    /// itself, so it would "do something" whether or not its real binding
    /// still exists. And a Queries key satisfied by some other fixture —
    /// `⏎` folding a keyspace, `esc` dismissing an overlay, `home`/`end`
    /// moving a tree cursor — would let the editor quietly stop claiming it
    /// with the sweep still green. Each group is answered by the state it
    /// describes.
    #[test]
    fn every_key_the_palette_lists_does_something() {
        let plain = sweep_fixtures();
        let editor = vec![editor_fixture()];

        for group in CATALOGUE {
            let fixtures = if group.heading == "Queries" {
                &editor
            } else {
                &plain
            };

            for binding in group.bindings {
                for &press in binding.keys {
                    assert!(
                        fixtures.iter().any(|state| does_something(state, press)),
                        "{} lists {press:?} and no state responds to it",
                        group.heading
                    );
                }
            }
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

    /// The design's own mechanism for an empty commit: `Filter::new("")` is
    /// `None`, so backspacing a live query back to nothing already clears the
    /// filter before `Enter` is ever pressed — unlike
    /// `an_empty_commit_clears_the_filter` above, whose buffer is empty
    /// because nothing was ever typed into it.
    #[test]
    fn backspacing_a_query_to_empty_clears_the_filter_live() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.apply_filter(Filtered::Tables, Filter::new("o"));
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));

        press(&mut state, Key::Backspace);

        assert_eq!(
            state.filter(Filtered::Tables),
            None,
            "the buffer is empty, so the live preview already has no filter"
        );

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
    fn v_starts_a_range_at_the_cursor_and_motion_extends_it() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.selection.range(state.tables.cursor.index()),
            Some(Range::new(1, 3))
        );
    }

    #[test]
    fn a_second_v_freezes_the_range_and_the_cursor_walks_out_of_it() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), 2);
        assert_eq!(
            state.tables.selection.range(state.tables.cursor.index()),
            Some(Range::new(0, 1)),
            "a frozen range does not follow the cursor"
        );
    }

    #[test]
    fn escape_freezes_a_live_range_before_it_clears_one() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Escape);
        assert_eq!(
            state.tables.selection.range(state.tables.cursor.index()),
            Some(Range::new(0, 1)),
            "the first press leaves visual mode and keeps the range"
        );

        press(&mut state, Key::Escape);
        assert_eq!(
            state.tables.selection.range(state.tables.cursor.index()),
            None,
            "the second clears it"
        );
    }

    #[test]
    fn escape_prefers_the_selection_to_the_filter() {
        let mut state = browsing(&["alpha", "beta"]);
        state.apply_filter(Filtered::Tables, Filter::new("a"));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Escape);

        press(&mut state, Key::Escape);

        assert_eq!(state.tables.selection.range(0), None);
        assert!(
            state.filter(Filtered::Tables).is_some(),
            "the range came off first; the filter is still applied"
        );
    }

    #[test]
    fn v_selects_in_the_focused_panel_only() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus = Panel::Ddl;

        press(&mut state, Key::Char('v'));

        assert_eq!(state.tables.selection.range(0), None);
        assert!(state.ddl.selection.is_extending());
    }

    #[test]
    fn v_types_a_v_in_queries() {
        let mut state = browsing(&["a"]);
        state.focus = Panel::Queries;

        press(&mut state, Key::Char('v'));

        assert_eq!(state.queries.text(), "v");
    }

    #[test]
    fn a_filter_clears_the_range_it_would_renumber() {
        let mut state = browsing(&["alpha", "beta", "gamma"]);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));

        state.apply_filter(Filtered::Tables, Filter::new("a"));

        assert_eq!(state.tables.selection.range(0), None);
    }

    #[test]
    fn a_range_over_a_list_that_shrank_is_pulled_back() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::End);
        press(&mut state, Key::Char('v'));
        assert_eq!(
            state.tables.selection.range(state.tables.cursor.index()),
            Some(Range::new(0, 2))
        );

        press(&mut state, Key::Home);
        press(&mut state, Key::Char('h'));

        assert_eq!(
            state.tables.selection.range(state.tables.cursor.index()),
            Some(Range::new(0, 0)),
            "folding the keyspace took the two table rows with it"
        );
    }

    #[test]
    fn escape_leaves_queries_when_nothing_else_claims_it() {
        // Rung 3 alone (§6): no overlay, no filter (Queries cannot hold one
        // yet — design §8), so `esc` falls through to leaving the editor.
        // This does not pin rung 2 before rung 3 — that ordering is
        // unreachable, and stays so, until the vim slice gives Queries a
        // filter to prefer.
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

    /// The layer is dispatched above every overlay, so a chord that would open
    /// one has to refuse the prompt itself: opening a picker over a live query
    /// would leave the panel filtered with nothing holding what it filtered
    /// from (docs/shortcuts.md §1).
    #[test]
    fn ctrl_t_is_refused_over_a_live_prompt() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('t'));

        ctrl(&mut state, Key::Char('t'));

        assert_eq!(searching(&state).map(Prompt::query), Some("t"));
    }

    /// A ctrl chord the layer has no member for is refused as it is anywhere
    /// else, rather than typed into the query.
    #[test]
    fn an_unclaimed_ctrl_chord_is_not_text_in_the_prompt() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));

        ctrl(&mut state, Key::Char('n'));

        assert_eq!(searching(&state).map(Prompt::query), Some(""));
    }

    /// `ddl_rows` counts the rows the panel draws, so a filter that empties a
    /// section pulls the cursor back with it — the clamp in `clamp_lists`
    /// needs no special case for the filter (panel search design §3.3).
    #[test]
    fn a_ddl_filter_that_shrinks_the_panel_pulls_its_cursor_back() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        sized(&mut state, [8, 4, 8, 1], 40);
        press(&mut state, Key::End);
        let unfiltered = crate::view::ddl_rows(&state);
        assert!(
            state.ddl.cursor.index() > 0,
            "the fixture starts at the end"
        );

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('i'));
        press(&mut state, Key::Char('d'));
        press(&mut state, Key::Enter);

        let filtered = crate::view::ddl_rows(&state);
        assert!(
            filtered > 0 && filtered < unfiltered,
            "the fixture's filter must narrow the panel without emptying it"
        );
        assert!(
            state.ddl.cursor.index() < filtered,
            "the cursor cannot sit on a row the filter removed"
        );
    }

    /// The empty end of the same clamp: a panel with no rows left has nothing
    /// for its cursor to be on, and motion over it is the swallow every other
    /// empty list gets rather than an index that drifts past zero.
    #[test]
    fn a_filter_that_empties_table_ddl_leaves_its_cursor_at_rest() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        sized(&mut state, [8, 4, 8, 1], 40);
        press(&mut state, Key::End);

        state.apply_filter(Filtered::Ddl, Filter::new("no such property"));
        press(&mut state, Key::Char('j'));

        assert_eq!(crate::view::ddl_rows(&state), 0);
        assert_eq!(state.ddl.cursor.index(), 0);

        press(&mut state, Key::End);

        assert_eq!(state.ddl.cursor.index(), 0, "End over nothing goes nowhere");
    }

    /// Clearing keeps the index rather than mapping it back onto the full
    /// list. There is no row identity to carry across — the rows that survived
    /// are a different sequence — and a clamp is owed only where the list
    /// shrinks under the cursor (panel search design §3.3).
    #[test]
    fn clearing_a_ddl_filter_leaves_the_cursor_on_the_index_it_held() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        sized(&mut state, [8, 4, 8, 1], 40);

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('a'));
        press(&mut state, Key::Enter);
        press(&mut state, Key::End);
        let deep = state.ddl.cursor.index();
        assert!(
            deep > 0,
            "the filter must leave more than one row for the cursor to be deep in"
        );

        press(&mut state, Key::Escape);

        assert!(state.filter(Filtered::Ddl).is_none());
        assert_eq!(state.ddl.cursor.index(), deep);
        assert!(
            deep + 1 < crate::view::ddl_rows(&state),
            "and it now names a row of the full list that it was never on"
        );
    }

    /// The blank frame design §3.3 accepts, carried to its repair: the offset
    /// a wheel produced from pre-filter geometry is out of range for the body
    /// the filter left, and the next measurement pulls it back. One frame
    /// wide, and the loop terminates.
    #[test]
    fn the_measurement_after_a_filter_pulls_a_stale_ddl_offset_back_inside() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        sized(&mut state, [8, 4, 8, 1], 40);

        for _ in 0..20 {
            point(
                &mut state,
                Pointer::Wheel {
                    panel: Panel::Ddl,
                    direction: Wheel::Down,
                },
            );
        }
        assert_eq!(
            state.ddl.scroll.offset(),
            36,
            "the wheel bounds itself against the pre-filter line count"
        );

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('t'));

        // The frame the filter produced: nine wrapped lines in a four-line
        // panel, so no offset past five is drawable.
        sized(&mut state, [8, 4, 8, 1], 9);

        assert_eq!(state.ddl.scroll.offset(), 5);
    }

    /// The Ddl half of the `esc` chain's second step. Its viewport is followed
    /// by span rather than by index (design §5.1 of the Ddl selection slice),
    /// so clearing a filter that moved what the cursor names is repaired on
    /// the measurement that follows, not in `escape` itself.
    #[test]
    fn escape_clearing_a_ddl_filter_re_follows_the_selection() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        sized(&mut state, [8, 4, 8, 1], 40);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('a'));
        press(&mut state, Key::Enter);
        press(&mut state, Key::End);

        press(&mut state, Key::Escape);

        assert!(state.filter(Filtered::Ddl).is_none());

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 4, 8, 1], 40, 0).with_ddl_selection(Span {
                first: 22,
                lines: 3,
            })),
        );

        assert_eq!(
            state.ddl.scroll.offset(),
            21,
            "lines 21..25 put the whole of the row measured at 22..25 on screen"
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

    /// `ctrl-r`, the Queries member of the ctrl layer (docs/shortcuts.md §2).
    fn run_query(state: &mut State) -> Vec<Effect> {
        ctrl(state, Key::Char('r'))
    }

    #[test]
    fn ctrl_r_runs_the_query_buffer() {
        let mut state = editing("SELECT * FROM shop.orders");

        let effects = run_query(&mut state);

        let query = state
            .contents
            .outstanding()
            .expect("the panel is waiting on the query it just ran");
        assert_eq!(
            effects,
            vec![
                Effect::Execute {
                    conn: current_conn(&state),
                    query,
                    stmt: Box::new(Statement::typed("SELECT * FROM shop.orders")),
                },
                Effect::RecordHistory(Box::new(Statement::typed("SELECT * FROM shop.orders"))),
            ]
        );
    }

    #[test]
    fn a_run_query_titles_the_panel_with_what_was_typed() {
        let mut state = editing("SELECT * FROM shop.orders");
        run_query(&mut state);
        let query = state.contents.outstanding().expect("waiting");

        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(3)),
            },
        );

        assert_eq!(state.contents.row_count(), 3);
        let ContentsState::Loaded { subject, .. } = state.contents.state() else {
            panic!("the page landed, so the panel is loaded");
        };
        assert_eq!(
            subject.label(),
            "SELECT * FROM shop.orders",
            "the panel says which query it is showing"
        );
    }

    #[test]
    fn ctrl_r_outside_queries_runs_nothing() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let mut state = editing("SELECT * FROM shop.orders");
            state.focus = panel;

            assert!(
                run_query(&mut state).is_empty(),
                "the buffer is run from the panel it is typed in, not from {panel:?}"
            );
            assert_eq!(state.contents.outstanding(), None);
        }
    }

    #[test]
    fn an_empty_buffer_runs_nothing() {
        for text in ["", "   \n  \n"] {
            let mut state = editing(text);

            assert!(run_query(&mut state).is_empty());
            assert_eq!(
                state.contents.outstanding(),
                None,
                "an empty buffer is a mistake with no consequence, not an error box"
            );
            assert!(state.overlay.is_none());
        }
    }

    #[test]
    fn running_a_query_cancels_a_pending_table_read() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let read = state.contents.outstanding().expect("the read is in flight");
        state.focus = Panel::Queries;
        type_into(&mut state, "SELECT * FROM shop.users");

        let effects = run_query(&mut state);

        assert!(
            effects.contains(&Effect::Cancel(read)),
            "one grid means one outstanding query"
        );
        assert!(!state.in_flight.contains_key(&read));
    }

    #[test]
    fn reading_a_table_cancels_a_pending_query() {
        let mut state = read_shop_orders();
        state.focus = Panel::Queries;
        type_into(&mut state, "SELECT * FROM shop.users");
        run_query(&mut state);
        let query = state
            .contents
            .outstanding()
            .expect("the query is in flight");
        state.focus = Panel::Tables;

        let effects = press(&mut state, Key::Enter);

        assert!(effects.contains(&Effect::Cancel(query)));
        assert!(!state.in_flight.contains_key(&query));
    }

    #[test]
    fn a_destructive_statement_is_refused_until_confirmation_exists() {
        let mut state = editing("DROP TABLE shop.orders");

        let effects = run_query(&mut state);

        assert!(
            effects.is_empty(),
            "nothing reaches the cluster without the confirmation vision.md §4 principle 6 requires"
        );
        assert_eq!(state.contents.outstanding(), None);
        assert!(
            matches!(
                state.overlay,
                Some(Overlay::Message(Message {
                    severity: Severity::Danger,
                    ..
                }))
            ),
            "the refusal says why"
        );
    }

    #[test]
    fn a_typed_write_is_refused_on_a_read_only_connection() {
        let mut state = editing("INSERT INTO shop.orders (id) VALUES (1)");

        let effects = run_query(&mut state);

        assert!(effects.is_empty());
        assert!(
            matches!(
                state.contents.state(),
                ContentsState::Failed {
                    error: DbError::ReadOnly,
                    ..
                }
            ),
            "the posture gate refuses it and the panel says so"
        );
    }

    #[test]
    fn an_unbound_ctrl_key_does_nothing() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));

        ctrl(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "ctrl-j is a binding of its own that does not exist yet, not a slower j"
        );
    }

    #[test]
    fn an_alt_modified_key_is_not_a_binding() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));

        press_binding(
            &mut state,
            KeyPress {
                key: Key::Char('j'),
                modifiers: Modifiers {
                    alt: true,
                    ..Modifiers::NONE
                },
            },
        );

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "alt-j is refused rather than reinterpreted as a slower j"
        );
    }

    #[test]
    fn ctrl_alt_r_does_not_run_the_buffer() {
        let mut state = editing("SELECT * FROM shop.orders");

        let effects = press_binding(
            &mut state,
            KeyPress {
                key: Key::Char('r'),
                modifiers: Modifiers {
                    ctrl: true,
                    alt: true,
                    shift: false,
                },
            },
        );

        assert!(
            effects.is_empty(),
            "two modifiers are a third layer, and no layer is designed"
        );
        assert_eq!(state.contents.outstanding(), None);
    }

    #[test]
    fn an_open_palette_swallows_ctrl_r() {
        let mut state = editing("SELECT * FROM shop.orders");
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));

        let effects = run_query(&mut state);

        assert!(
            effects.is_empty(),
            "an open palette is what the user is looking at, and it claims every key"
        );
        assert_eq!(state.contents.outstanding(), None);
    }

    #[test]
    fn m_cycles_the_contents_selection_mode() {
        let mut state = populated(Panel::Contents);

        press(&mut state, Key::Char('m'));
        assert_eq!(state.contents.mode, SelectionMode::Column);

        press(&mut state, Key::Char('m'));
        assert_eq!(state.contents.mode, SelectionMode::Cell);

        press(&mut state, Key::Char('m'));
        assert_eq!(state.contents.mode, SelectionMode::Row);
    }

    /// The grid is the only body with two cursors to choose between. `m` is
    /// not a focus letter, so in the other panels it must fall through to the
    /// arm that answers those and do nothing.
    #[test]
    fn m_does_nothing_outside_the_grid() {
        for panel in [Panel::Tables, Panel::Ddl] {
            let mut state = populated(panel);
            let before = crate::view::view_model(&state);

            let effects = press(&mut state, Key::Char('m'));

            assert!(
                effects.is_empty() && crate::view::view_model(&state) == before,
                "{panel:?} has no grid for a mode to apply to"
            );
            assert_eq!(state.contents.mode, SelectionMode::Row);
        }
    }

    #[test]
    fn m_types_an_m_in_queries() {
        let mut state = populated(Panel::Queries);
        let before = state.queries.text();

        press(&mut state, Key::Char('m'));

        assert_eq!(state.queries.text(), format!("{before}m"));
        assert_eq!(state.contents.mode, SelectionMode::Row);
    }

    #[test]
    fn v_in_row_mode_extends_rows_and_leaves_the_columns_alone() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Row;

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.contents.rows.range(state.contents.cursor.index()),
            Some(Range::new(0, 1))
        );
        assert_eq!(state.contents.columns.range(0), None);
    }

    #[test]
    fn v_in_column_mode_extends_columns_and_leaves_the_rows_alone() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Column;

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('l'));

        assert_eq!(
            state.contents.columns.range(state.contents.column.index()),
            Some(Range::new(0, 1))
        );
        assert_eq!(state.contents.rows.range(0), None);
    }

    #[test]
    fn v_in_cell_mode_extends_a_rectangle() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('l'));

        assert_eq!(
            state.contents.rows.range(state.contents.cursor.index()),
            Some(Range::new(0, 1)),
            "the row anchor was set before the column moved off zero"
        );
        assert_eq!(
            state.contents.columns.range(state.contents.column.index()),
            Some(Range::new(1, 2)),
            "the column anchor was set after `l`, not swapped with the row's"
        );
    }

    #[test]
    fn m_clears_the_range_it_would_reinterpret() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));

        press(&mut state, Key::Char('m'));

        assert_eq!(state.contents.rows.range(0), None);
        assert_eq!(state.contents.columns.range(0), None);
        assert_eq!(
            state.contents.mode,
            SelectionMode::Row,
            "the mode still cycles"
        );
    }

    #[test]
    fn escape_unwinds_both_axes_together() {
        let mut state = grid_of_named_cells(3, 4);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Escape);
        assert!(!state.contents.rows.is_extending());
        assert!(state.contents.rows.range(0).is_some());

        press(&mut state, Key::Escape);
        assert_eq!(state.contents.rows.range(0), None);
        assert_eq!(state.contents.columns.range(0), None);
    }

    fn contents_showing(rows: usize) -> State {
        let mut state = State {
            focus: Panel::Contents,
            ..connected(Posture::ReadOnly)
        };
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(query, page_of(rows));
        state
    }

    fn open_form(state: &mut State) {
        handle_key(state, KeyPress::ctrl(Key::Char('e')));
    }

    fn form_of(state: &State) -> &crate::export::ExportForm {
        match state.overlay.as_ref() {
            Some(Overlay::Export(form)) => form,
            _ => panic!("the export form is up"),
        }
    }

    #[test]
    fn ctrl_e_over_a_loaded_contents_panel_opens_the_export_form() {
        let mut state = contents_showing(5);

        open_form(&mut state);

        assert_eq!(form_of(&state).title(), "Export table shop.orders");
    }

    #[test]
    fn ctrl_e_does_nothing_over_an_empty_contents_panel() {
        let mut state = contents_showing(0);

        open_form(&mut state);

        assert!(state.overlay.is_none());
    }

    /// The chord keeps its Queries meaning; the two are gated on focus, the
    /// way ctrl-r already is (design §1).
    #[test]
    fn ctrl_e_in_queries_still_opens_the_external_editor() {
        let mut state = State {
            focus: Panel::Queries,
            ..connected(Posture::ReadOnly)
        };

        let effects = handle_key(&mut state, KeyPress::ctrl(Key::Char('e')));

        assert!(matches!(effects.as_slice(), [Effect::OpenEditor { .. }]));
        assert!(state.overlay.is_none());
    }

    #[test]
    fn the_form_records_how_much_of_the_result_is_in_memory() {
        let mut complete = contents_showing(7);
        open_form(&mut complete);
        assert_eq!(
            form_of(&complete).fetched(),
            crate::export::Fetched::Everything(7)
        );

        let mut capped = contents_showing(ROW_LIMIT);
        open_form(&mut capped);
        assert_eq!(
            form_of(&capped).fetched(),
            crate::export::Fetched::Some(ROW_LIMIT)
        );
    }

    #[test]
    fn tab_moves_between_the_forms_stops_rather_than_between_panels() {
        let mut state = contents_showing(5);
        open_form(&mut state);

        handle_key(&mut state, KeyPress::new(Key::Tab));

        assert_eq!(form_of(&state).stop(), crate::export::Stop::Format);
        assert_eq!(state.focus, Panel::Contents, "focus did not move");
    }

    #[test]
    fn space_toggles_the_focused_radio() {
        let mut state = contents_showing(ROW_LIMIT);
        open_form(&mut state);

        handle_key(&mut state, KeyPress::new(Key::Char(' ')));

        assert_eq!(form_of(&state).scope(), crate::export::Scope::Fetched);
    }

    #[test]
    fn a_printable_key_types_itself_only_on_the_path_stop() {
        let mut state = contents_showing(5);
        open_form(&mut state);
        handle_key(&mut state, KeyPress::new(Key::Char('x')));
        assert_eq!(
            form_of(&state).scope(),
            crate::export::Scope::Whole,
            "an unclaimed key on a radio does nothing"
        );

        handle_key(&mut state, KeyPress::new(Key::Tab));
        handle_key(&mut state, KeyPress::new(Key::Tab));
        handle_key(&mut state, KeyPress::new(Key::Char('x')));

        assert!(form_of(&state).path().text().ends_with('x'));
    }

    #[test]
    fn space_on_the_path_stop_types_a_space() {
        let mut state = contents_showing(5);
        open_form(&mut state);
        handle_key(&mut state, KeyPress::new(Key::Tab));
        handle_key(&mut state, KeyPress::new(Key::Tab));

        handle_key(&mut state, KeyPress::new(Key::Char(' ')));

        assert!(form_of(&state).path().text().ends_with(' '));
    }

    #[test]
    fn q_does_not_quit_while_the_form_is_up() {
        let mut state = contents_showing(5);
        open_form(&mut state);

        handle_key(&mut state, KeyPress::new(Key::Char('q')));

        assert!(
            !state.should_quit,
            "the overlay claims every unmodified key"
        );
    }

    #[test]
    fn esc_closes_the_form_and_writes_nothing() {
        let mut state = contents_showing(5);
        open_form(&mut state);

        let effects = handle_key(&mut state, KeyPress::new(Key::Escape));

        assert!(state.overlay.is_none());
        assert!(effects.is_empty());
    }
}
