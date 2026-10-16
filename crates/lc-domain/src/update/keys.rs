//! Key dispatch: the one place a press is resolved, overlays first.

use crate::connection::ConnectionState;
use crate::contents::{Filter, SelectionMode};
use crate::cql;
use crate::line::Line;
use crate::model::{Key, KeyPress};
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::queries::Refresh;
use crate::search::{Direction, Searched};
use crate::state::State;
use crate::style::Severity;
use crate::theme::BuiltIn;

use super::commit::{commit_extent, commit_inspected_row, commit_key, discard_key, open_commit};
use super::connections::{connections_key, offer_connections, open_connections};
use super::drop::{drop_key, open_drop};
use super::export::{cancel_pending_export, export_key, open_export};
use super::health::{health_key, open_health};
use super::history::{history_key, open_history};
use super::insert::open_insert;
use super::inspect::{edit_from_grid, inspect_key, open_inspector};
use super::palette::{open_palette, palette_key};
use super::search::{next_match_key, open_prompt, restore_origin, search_key};
use super::themes::{open_themes, preview_theme, themes_key};
use super::yank::yank;
use super::{
    Effect, Motion, Step, activate, clear_selection, every_page, focus_by_number, go_left,
    go_right, next_page, quit, run, scroll, stop_extending, toggle_visual, walk_landing, walk_tab,
};

/// Which end of a list `g` and `G` reach.
///
/// Narrower than [`Motion`] on purpose: only two of that enum's six landings
/// can be spelled this way, and an overlay resolving `g` has no paging arm
/// to fall through to.
#[derive(Clone, Copy)]
pub(super) enum Edge {
    Start,
    End,
}

impl Edge {
    const fn motion(self) -> Motion {
        match self {
            Self::Start => Motion::Start,
            Self::End => Motion::End,
        }
    }
}

pub(super) fn handle_key(state: &mut State, press: KeyPress) -> Vec<Effect> {
    state.mark_dirty();

    // Claimed before the prefix, before ctrl, before focus — nothing else
    // about this press is consulted. The modal has no dismissal, only the
    // one exit (query log design §7).
    if let Some(Overlay::Fatal(_)) = state.overlay {
        return match press.key {
            Key::Escape | Key::Char('q') => quit(state),
            _ => Vec::new(),
        };
    }

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

    // `g`/`G` are decided above the overlays, because they mean the same
    // thing in all of them: the list this frame is driving, whichever that is
    // (docs/shortcuts.md §1). Refused wherever a key is text, which is the one
    // reading `goto` cannot resolve.
    if !takes_text(state) {
        match press.key {
            Key::Char('g') => {
                goto(state, Edge::Start);
                return Vec::new();
            }
            Key::Char('G') => {
                goto(state, Edge::End);
                return Vec::new();
            }
            _ => {}
        }
    }

    if let Some(effects) = overlay_key(state, press.key) {
        return effects;
    }

    // An open buffer claims every printable key and the editing keys; what it
    // does not claim keeps the global meaning it has everywhere else (design
    // §3). No key is carved out, its own number included: `WHERE id = 4` has
    // to be typeable (cql edit mode design §2).
    if state.takes_text() && edit(state, press.key) {
        return Vec::new();
    }

    panel_key(state, press.key)
}

/// The overlay's answer to `key`, or `None` when no overlay claims it.
///
/// An overlay that takes text claims every printable key, the
/// panel-that-takes-text rule applied to an overlay (panel search design
/// §1.1). A picker or the palette takes motion until a sift is up, and text
/// while one is (docs/superpowers/specs/2026-08-16-overlay-search-design.md
/// §5).
pub(super) fn overlay_key(state: &mut State, key: Key) -> Option<Vec<Effect>> {
    let effects = match state.overlay {
        Some(Overlay::Commands(_)) => palette_key(state, key),
        // Motion, not text: diagnostics has no sift to raise (design §11).
        Some(Overlay::Diagnostics(_)) => diagnostics_key(state, key),
        // Motion, `y`, and the two ways out; `q` still quits (cell inspector
        // design §4).
        Some(Overlay::Inspect(_)) => inspect_key(state, key),
        Some(Overlay::Themes { .. }) => themes_key(state, key),
        Some(Overlay::Connections { .. }) => connections_key(state, key),
        Some(Overlay::History { .. }) => history_key(state, key),
        // Motion and the tree's fold keys; `q` still quits.
        Some(Overlay::Health(_)) => health_key(state, key),
        Some(Overlay::Search(_)) => search_key(state, key),
        // Text on one of its stops, and most printable keys go nowhere on the
        // others (export design §2).
        Some(Overlay::Export(_)) => export_key(state, key),
        // One text field (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §3).
        Some(Overlay::Filter(_)) => {
            filter_key(state, key);
            Vec::new()
        }
        // Every key it is answered with and, on its typed spelling, every
        // printable key besides (drop design §2.1).
        Some(Overlay::Drop(_)) => drop_key(state, key),
        Some(Overlay::Commit(_)) => commit_key(state, key),
        // `q` included: the question has to be answered rather than pressed
        // past (cell editing design §6).
        Some(Overlay::Discard(_)) => discard_key(state, key),
        // `Fatal` is claimed at the top of `handle_key`, before modifiers.
        Some(Overlay::Message(_) | Overlay::Fatal(_)) | None => return None,
    };

    Some(effects)
}

/// What the focused panel answers, once no overlay and no open buffer has
/// claimed the press. The tail of [`handle_key`]'s one dispatch point, split
/// out only for its length (docs/shortcuts.md §2).
pub(super) fn panel_key(state: &mut State, key: Key) -> Vec<Effect> {
    match key {
        Key::Char('q') => return quit(state),
        Key::Char('?') => open_palette(state),
        Key::Escape => escape(state),
        Key::Tab => walk_landing(state, Step::Forward),
        Key::BackTab => walk_landing(state, Step::Back),
        Key::Char('j') | Key::Down => scroll(state, Motion::Down),
        Key::Char('k') | Key::Up => scroll(state, Motion::Up),
        Key::PageDown => scroll(state, Motion::PageDown),
        Key::PageUp => scroll(state, Motion::PageUp),
        Key::Home => scroll(state, Motion::Start),
        Key::End => scroll(state, Motion::End),
        Key::Enter if state.focus() == Panel::Contents => open_inspector(state),
        Key::Char('e') if state.focus() == Panel::Contents => return edit_from_grid(state),
        Key::Enter | Key::Char('i') if state.focus() == Panel::Queries => {
            state.queries.start_editing();
        }
        Key::Enter => return activate(state),
        Key::Char('l') | Key::Right => return go_right(state),
        Key::Char('h') | Key::Left => go_left(state),
        Key::Char('/') => open_prompt(state),
        Key::Char('y') => return yank(state),
        Key::Char('m') => {
            if state.focus() == Panel::Contents {
                // A range's axis is the mode's axis, so carrying one across
                // would address rows or columns the user never chose — the
                // argument `DdlPanel::show` already makes for the cursor
                // (design §3).
                state.contents.rows.clear();
                state.contents.columns.clear();
                state.contents.mode = state.contents.mode.next();
            }
        }
        // The three column keys. Answered in column mode alone: they address
        // the column the cursor is on, and that cursor is what column mode
        // *is* — outside it `s`, `f` and `c` are free letters the bar does
        // not name (grid sort and filter design §6).
        Key::Char('s') if columnar(state) => state.contents.cycle_sort(column_of(state)),
        Key::Char('f') if columnar(state) => open_filter(state),
        Key::Char('c') if columnar(state) => {
            state.contents.set_filter(column_of(state), None);
        }
        Key::Char('d') => open_drop(state),
        Key::Char('i') => open_insert(state),
        Key::Char('H') => return open_health(state),
        Key::Char('p') => return next_page(state),
        Key::Char('P') => return every_page(state),
        Key::Char('v') => toggle_visual(state),
        Key::Char('n') => next_match_key(state, Direction::Forwards),
        Key::Char('N') => next_match_key(state, Direction::Backwards),
        // Tabs are walked from the focused panel and nowhere else: `]` on
        // Tables would otherwise mean "the DDL panel's next tab", which is a
        // second way to reach a place the number already reaches
        // (docs/shortcuts.md §4).
        Key::Char(']') => walk_tab(state, Step::Forward),
        Key::Char('[') => walk_tab(state, Step::Back),
        // Last, so every binding above keeps precedence over a panel key.
        Key::Char(key) => focus_by_number(state, key),
        _ => {}
    }

    Vec::new()
}

/// Whether this frame types rather than navigates.
///
/// A prompt and the export form claim every printable key themselves, and so
/// does a panel that takes text with nothing over it — an overlay that takes
/// motion is in front of that panel, and its list is what a motion moves
/// (design §1, docs/shortcuts.md §2).
pub(super) fn takes_text(state: &State) -> bool {
    match &state.overlay {
        Some(Overlay::Search(_) | Overlay::Export(_) | Overlay::Filter(_)) => true,
        // Only on the spelling that has a name to type into. A `y`/`n`
        // confirmation takes no text, so `g`/`G` are decided above it and
        // find nothing to move (drop design §2.1).
        Some(Overlay::Drop(prompt)) => prompt.takes_text(),
        // A sift up makes the overlay itself a text field (overlay search
        // design §5): the letters that would otherwise move the cursor or
        // close the overlay type themselves instead.
        Some(Overlay::Commands(panel)) => panel.sift().is_some(),
        Some(
            Overlay::Themes { sift, .. }
            | Overlay::Connections { sift, .. }
            | Overlay::History { sift, .. },
        ) => sift.is_some(),
        // The inspector takes text on exactly the frames a field is open,
        // the rule the prompt and the form follow (cell editing design §3).
        Some(Overlay::Inspect(inspector)) => inspector.field().is_some(),
        // None of these has a field to type into. `Fatal` is unreachable —
        // `handle_key` claims every press for it first. The commit block
        // drives a body `g`/`G` move, which `goto` answers below.
        Some(
            Overlay::Diagnostics(_)
            | Overlay::Health(_)
            | Overlay::Fatal(_)
            | Overlay::Commit(_)
            | Overlay::Discard(_),
        ) => false,
        Some(Overlay::Message(_)) | None => state.takes_text(),
    }
}

/// `g`/`G`, resolved against whatever list the frame is driving: the overlay
/// that captures keys if one is up, and the focused panel otherwise.
///
/// The two landings `Home`/`End` already make, reached through the same
/// methods rather than through a copy of them — the relation `ctrl-d` has to
/// `PageDown` (docs/shortcuts.md §1).
pub(super) fn goto(state: &mut State, edge: Edge) {
    let last = BuiltIn::ALL.len() + state.config.themes.len();
    let connections = state.config.connections.len();
    let history = state.history().len();
    let (commit_lines, commit_rows) = commit_extent(state);

    match &mut state.overlay {
        Some(Overlay::Commands(panel)) => match edge {
            Edge::Start => panel.to_start(),
            Edge::End => panel.to_end(),
        },
        Some(Overlay::Inspect(inspector)) => match edge {
            Edge::Start => inspector.to_start(),
            Edge::End => inspector.to_end(),
        },
        Some(Overlay::Commit(prompt)) => match edge {
            Edge::Start => prompt.to_start(),
            Edge::End => prompt.to_end(commit_lines, commit_rows),
        },
        Some(Overlay::Diagnostics(panel)) => match edge {
            Edge::Start => panel.to_start(),
            Edge::End => panel.to_end(),
        },
        Some(Overlay::Themes { cursor, .. }) => {
            match edge {
                Edge::Start => cursor.to_start(),
                Edge::End => cursor.to_end(last),
            }
            preview_theme(state);
        }
        Some(Overlay::Connections { cursor, .. }) => match edge {
            Edge::Start => cursor.to_start(),
            Edge::End => cursor.to_end(connections),
        },
        Some(Overlay::History { cursor, .. }) => match edge {
            Edge::Start => cursor.to_start(),
            Edge::End => cursor.to_end(history),
        },
        Some(Overlay::Health(panel)) => match edge {
            Edge::Start => panel.to_start(),
            Edge::End => panel.to_end(),
        },
        Some(Overlay::Message(_)) | None => scroll(state, edge.motion()),
        // The drop confirmation drives no list either, and `Fatal` never
        // reaches here for `takes_text`'s reason.
        Some(
            Overlay::Fatal(_)
            | Overlay::Search(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_)
            | Overlay::Discard(_),
        ) => {}
    }
}

/// Offers `key` to the focused panel's buffer, answering whether it was text.
///
/// The exhaustive match over [`Key`] lives on [`QueriesPanel`], which is what
/// holds the popup the two-key exceptions turn on; what is left here is the
/// half only `update` can do, which is offering against the schema
/// (query editor design §5.4).
pub(super) fn edit(state: &mut State, key: Key) -> bool {
    let height = state.geometry.height(Panel::Queries);
    let Some(refresh) = state.queries.edit(key, height) else {
        return false;
    };

    match refresh {
        Refresh::Recompute => complete(state),
        Refresh::Close | Refresh::Hold => {}
    }

    true
}

/// Offers the schema's own vocabulary against the word under the cursor.
///
/// The keyspace is the one the Tables panel is showing, which is what makes an
/// unqualified table name completable at all — the panel beside the editor is
/// already saying which keyspace the user is looking at
/// (query editor design §5.2).
pub(super) fn complete(state: &mut State) {
    let keyspace = state.visible_keyspace();
    let completion = cql::suggest(state.queries.buffer(), state.schema(), keyspace.as_deref());
    state.queries.offer(completion);
}

/// `ctrl-f` lays the buffer out, and does nothing anywhere else: the panel
/// takes text, so the action has no unmodified key available — the shape
/// `ctrl-r` and `ctrl-e` already have (query editor design §4.3).
pub(super) fn format_buffer(state: &mut State) {
    if state.focus() != Panel::Queries {
        return;
    }

    state.queries.format();
}

/// `ctrl-n` and `ctrl-p` walk the popup, and raise one where none is up: a
/// user who wants to see what is on offer should not have to type a character
/// they will then delete (query editor design §5.5).
pub(super) fn walk_completion(state: &mut State, forwards: bool) {
    if state.focus() != Panel::Queries {
        return;
    }

    if !state.queries.step(forwards) {
        complete(state);
    }
}

/// `Esc` resolves in one order, first hit wins: a live sift inside an
/// overlay; then the overlay itself; then a running `P`; then an open buffer;
/// then freezing an extending range; then clearing a frozen range; then the
/// focused panel's search; then the panel itself, which hands focus back to
/// Tables. One press does one of them, never two — the thing most recently put
/// on screen comes off first (panel search design §6, overlay search design
/// §1).
///
/// The last rung is every panel, not just the one that takes text: a key that
/// backs out of Queries but sits inert in DDL beside it is the same key
/// meaning two things (docs/shortcuts.md §2). On Tables it is already the
/// last rung's destination, so it does nothing there.
pub(super) fn escape(state: &mut State) {
    if dismiss_overlay(state) {
        return;
    }

    // Directly under the overlays: a running `P` is still putting rows on
    // screen (grid paging design §2).
    if state.focus() == Panel::Contents && state.contents.stop_fetching() {
        state.notify(Severity::Info, "stopped fetching pages");
        return;
    }

    // An open buffer is the innermost thing on screen: `esc` closes it and
    // keeps focus, and the next `esc` leaves the panel (cql edit mode design
    // §2).
    if state.takes_text() {
        state.queries.stop_editing();
        return;
    }

    // Above the search for the reason the overlay is above both: the thing
    // most recently put on screen comes off first (design §1).
    if stop_extending(state) || clear_selection(state) {
        return;
    }

    if let Some(target) = Searched::of(state.focus())
        && state.search(target).is_some()
    {
        state.apply_search(target, None);
        return;
    }

    state.focus_on(Panel::default());
}

/// `Esc`'s first two rungs: a live sift inside an overlay, then the overlay
/// itself. Whether there was an overlay to answer.
pub(super) fn dismiss_overlay(state: &mut State) -> bool {
    // Read before the `take()` below empties it: a click is the one path
    // that reaches `Overlay::Export`'s arm here rather than `export_key`,
    // which claims keyboard `Esc` itself (final review, I4 follow-up).
    cancel_pending_export(state);

    match state.overlay.take() {
        Some(Overlay::Search(prompt)) => {
            state.apply_search(prompt.target(), prompt.previous().cloned());
            restore_origin(state, prompt.target(), prompt.origin());
            true
        }
        Some(Overlay::Themes {
            cursor,
            restored,
            sift: Some(sift),
        }) => {
            let mut cursor = cursor;
            cursor.go_to(
                sift.origin(),
                BuiltIn::ALL.len() + state.config.themes.len(),
            );
            state.overlay = Some(Overlay::Themes {
                cursor,
                restored,
                sift: None,
            });
            preview_theme(state);
            true
        }
        Some(Overlay::Themes { restored, .. }) => {
            state.theme = restored;
            true
        }
        Some(Overlay::Connections {
            cursor,
            sift: Some(sift),
        }) => {
            let mut cursor = cursor;
            cursor.go_to(sift.origin(), state.config.connections.len());
            state.overlay = Some(Overlay::Connections { cursor, sift: None });
            true
        }
        Some(Overlay::History {
            cursor,
            sift: Some(sift),
        }) => {
            let mut cursor = cursor;
            cursor.go_to(sift.origin(), state.history().len());
            state.overlay = Some(Overlay::History { cursor, sift: None });
            true
        }
        // A dismissed picker leaves `Disconnected` rather than `Unchosen`, so
        // `offer_connections` does not put it straight back up: waving the
        // picker away is a choice, and the panels are allowed to stay empty
        // (connection picker design §4).
        Some(Overlay::Connections { .. }) => {
            if matches!(state.connection, ConnectionState::Unchosen) {
                state.connection = ConnectionState::Disconnected;
            }
            true
        }
        Some(Overlay::Commands(mut panel)) if panel.sift().is_some() => {
            panel.restore_origin();
            state.overlay = Some(Overlay::Commands(panel));
            true
        }
        // The filter prompt applies nothing on the way out: `esc` leaves the
        // column filtered exactly as it was, and `⏎` is the one key that
        // writes (grid sort and filter design §3).
        Some(
            Overlay::Message(_)
            | Overlay::Commands(_)
            | Overlay::History { .. }
            | Overlay::Health(_)
            | Overlay::Diagnostics(_)
            | Overlay::Inspect(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_)
            // Both confirmations abandon on `Esc`, leaving the edits standing:
            // dismissing a question is not answering it
            // (cell editing design §5).
            | Overlay::Commit(_)
            | Overlay::Discard(_),
        ) => {
            // The diagnostics modal is what claims startup's first frame when
            // the config had anything wrong with it; closing it is where the
            // picker underneath finally gets its turn.
            offer_connections(state);
            true
        }
        // Not dismissable: `handle_key` claims `Esc` for `Fatal` before this
        // function is reached, so the only caller left is a click routed
        // through here by `handle_pointer`, which must not spend itself on
        // ending the modal (query log design §7).
        Some(fatal @ Overlay::Fatal(_)) => {
            state.overlay = Some(fatal);
            true
        }
        None => false,
    }
}

/// What an open diagnostics overlay responds to.
///
/// `Esc` dismisses it and motion moves the cursor, bounded by the row count —
/// the same treatment [`palette_key`] gives `Overlay::Commands`, minus the
/// paging arithmetic a palette needs to skip a heading, which this list has
/// none of.
pub(super) fn diagnostics_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        state.overlay = None;
        // What the config got wrong is read before a connection is chosen, so
        // this dismissal is where the picker underneath gets its turn
        // (connection picker design §4).
        offer_connections(state);
        return Vec::new();
    }

    // `command_rows` counts one-line palette rows; a diagnostic renders as
    // two, so a page is half as many rows as it is lines.
    let height = state.geometry.command_rows() / 2;
    let Some(Overlay::Diagnostics(panel)) = &mut state.overlay else {
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

/// The ctrl layer's whole key table (docs/shortcuts.md §2).
///
/// Dispatched above the palette check, so it honours that rule itself rather
/// than inheriting it: routing a modified key into `palette_key`, which
/// matches on the key alone, would make `ctrl-j` move the palette cursor.
/// Each arm gates on its own panel below.
pub(super) fn ctrl_key(state: &mut State, key: Key) -> Vec<Effect> {
    // Paging is decided above the refusal below: `ctrl-d`/`ctrl-u` are a
    // second spelling of `PageDown`/`PageUp` rather than members of their own,
    // so they reach whatever those two reach, an open overlay included.
    match key {
        Key::Char('d') => return page(state, Key::PageDown),
        Key::Char('u') => return page(state, Key::PageUp),
        _ => {}
    }

    if key == Key::Char('s') && matches!(state.overlay, Some(Overlay::Inspect(_))) {
        return commit_inspected_row(state);
    }

    if matches!(
        state.overlay,
        Some(
            Overlay::Commands(_)
                | Overlay::Themes { .. }
                | Overlay::Connections { .. }
                | Overlay::History { .. }
                | Overlay::Diagnostics(_)
                | Overlay::Inspect(_)
                | Overlay::Export(_)
                | Overlay::Filter(_)
                | Overlay::Drop(_)
                | Overlay::Commit(_)
                | Overlay::Discard(_)
        )
    ) {
        return Vec::new();
    }

    match key {
        Key::Char('r') => run(state),
        Key::Char('t') => {
            open_themes(state);
            Vec::new()
        }
        // The way back to the picker. Without it a dismissed picker is
        // final — `Esc` leaves `Disconnected`, which nothing re-offers — and
        // a user who opened the wrong cluster would have to restart the tool
        // to reach another (connection picker design §5).
        Key::Char('o') => {
            open_connections(state);
            Vec::new()
        }
        Key::Char('e') => ctrl_e(state),
        Key::Char('g') => {
            open_history(state);
            Vec::new()
        }
        Key::Char('s') => open_commit(state),
        Key::Char('f') => {
            format_buffer(state);
            Vec::new()
        }
        Key::Char('n') => {
            walk_completion(state, true);
            Vec::new()
        }
        Key::Char('p') => {
            walk_completion(state, false);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// `ctrl-e` opens the user's editor on the Queries buffer and the export form
/// over the Contents grid. Two meanings, each gated on its own panel — the
/// shape `ctrl-r` already has (export design §1).
pub(super) fn ctrl_e(state: &mut State) -> Vec<Effect> {
    match state.focus() {
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
pub(super) fn open_editor(state: &State) -> Vec<Effect> {
    if state.focus() != Panel::Queries {
        return Vec::new();
    }

    vec![Effect::OpenEditor {
        buffer: state.queries.text(),
    }]
}

/// Whether the three column keys answer this frame: Table contents focused,
/// in column mode, over a page that has columns to address.
///
/// The mode is the gate rather than the panel, so `s`, `f` and `c` stay free
/// letters everywhere else — including in row and cell mode, where the cursor
/// names a row or a cell and a column key would have to guess
/// (grid sort and filter design §6).
pub(super) fn columnar(state: &State) -> bool {
    state.focus() == Panel::Contents
        && state.contents.mode == SelectionMode::Column
        && state.contents.column_count() > 0
}

/// The column the grid's cursor is on.
pub(super) fn column_of(state: &State) -> usize {
    state.contents.column.index()
}

/// Opens the filter prompt over the column the cursor is on, seeded with
/// whatever filter already stands there (design §3).
pub(super) fn open_filter(state: &mut State) {
    let column = column_of(state);
    let Some(page) = state.contents.loaded() else {
        return;
    };
    let Some(name) = page.page().columns.get(column) else {
        return;
    };

    let filter = Filter::opened(column, name, page.filter(column));
    state.overlay = Some(Overlay::Filter(filter));
}

/// What the filter prompt responds to: text, `⏎` to apply and `esc` to leave
/// it as it was (design §3).
pub(super) fn filter_key(state: &mut State, key: Key) {
    let Some(Overlay::Filter(filter)) = &mut state.overlay else {
        return;
    };

    match key {
        Key::Char(character) => filter.push(character),
        Key::Backspace => filter.pop(),
        Key::Enter => {
            let (column, search) = (filter.column(), filter.search());
            state.overlay = None;
            state.contents.set_filter(column, search);
        }
        Key::Escape => escape(state),
        _ => {}
    }
}

/// What [`export_key`]'s form-only match decided.
pub(super) enum Decision {
    Nothing,
    Close,
    Run,
}

/// The keys a single-line field claims: every printable key is a character
/// and the editing keys move within the line. Shared by the export form's
/// path stop (export design §2) and the drop confirmation's typed name
/// (drop design §2.1), which take text on the same terms.
pub(super) fn edit_line(line: &mut Line, key: Key) {
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

/// `ctrl-d`/`ctrl-u`, dispatched to whatever `key` would reach unmodified.
///
/// Delegating rather than paging here is what keeps the two spellings from
/// drifting: an overlay that learns to page learns it once.
pub(super) fn page(state: &mut State, key: Key) -> Vec<Effect> {
    match &state.overlay {
        Some(Overlay::Commands(_)) => palette_key(state, key),
        Some(Overlay::Themes { .. }) => themes_key(state, key),
        Some(Overlay::Connections { .. }) => connections_key(state, key),
        Some(Overlay::History { .. }) => history_key(state, key),
        Some(Overlay::Health(_)) => health_key(state, key),
        Some(Overlay::Diagnostics(_)) => diagnostics_key(state, key),
        Some(Overlay::Inspect(_)) => inspect_key(state, key),
        // The prompt takes text and pages nothing, so neither spelling does;
        // the form and the filter modal have nothing paged either, and the
        // confirmation drives no list. `Fatal` is unreachable here for
        // `takes_text`'s reason.
        Some(Overlay::Commit(_)) => commit_key(state, key),
        Some(
            Overlay::Fatal(_)
            | Overlay::Search(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_)
            | Overlay::Discard(_),
        ) => Vec::new(),
        Some(Overlay::Message(_)) | None => {
            if state.takes_text() {
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
