//! Key dispatch; an overlay claims a press before the focused panel does.

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
use super::export::{cancel_pending_export, export_chord, export_key, open_export};
use super::health::{health_key, open_health};
use super::history::{history_key, open_history};
use super::insert::open_insert;
use super::inspect::{edit_from_grid, inspect_key, open_inspector};
use super::palette::{open_palette, palette_key};
use super::search::{next_match_key, open_prompt, restore_origin, search_key};
use super::themes::{open_themes, preview_theme, themes_key};
use super::yank::yank;
use super::{
    Effect, Motion, Step, activate, cancel, clear_selection, every_page, focus_by_number, go_left,
    go_right, next_page, quit, run, scroll, stop_extending, toggle_visual, walk_landing, walk_tab,
};

/// Only two of [`Motion`]'s landings: an overlay has no paging arm to fall
/// through to.
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

    // Before modifiers and prefix: the modal has one exit.
    if let Some(Overlay::Fatal(_)) = state.overlay {
        return match press.key {
            Key::Escape | Key::Char('q') => quit(state),
            _ => Vec::new(),
        };
    }

    // alt and unclaimed ctrl chords are refused, not reinterpreted. shift is
    // not checked: a shifted arrow stays a synonym, leaving it free for a
    // future extend-selection binding.
    if press.modifiers.alt {
        return Vec::new();
    }

    if press.modifiers.ctrl {
        return ctrl_key(state, press.key);
    }

    // Above the overlays: `g`/`G` mean the ends of whichever list the frame
    // drives.
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

    // No key is carved out of an open buffer: `WHERE id = 4` must be typeable.
    if state.takes_text() && edit(state, press.key) {
        return Vec::new();
    }

    panel_key(state, press.key)
}

pub(super) fn overlay_key(state: &mut State, key: Key) -> Option<Vec<Effect>> {
    let effects = match state.overlay {
        Some(Overlay::Commands(_)) => palette_key(state, key),
        Some(Overlay::Diagnostics(_)) => diagnostics_key(state, key),
        Some(Overlay::Inspect(_)) => inspect_key(state, key),
        Some(Overlay::Themes { .. }) => themes_key(state, key),
        Some(Overlay::Connections { .. }) => connections_key(state, key),
        Some(Overlay::History { .. }) => history_key(state, key),
        Some(Overlay::Health(_)) => health_key(state, key),
        Some(Overlay::Search(_)) => search_key(state, key),
        Some(Overlay::Export(_)) => export_key(state, key),
        Some(Overlay::Filter(_)) => {
            filter_key(state, key);
            Vec::new()
        }
        Some(Overlay::Drop(_)) => drop_key(state, key),
        Some(Overlay::Commit(_)) => commit_key(state, key),
        // `q` included: the question must be answered, not pressed past.
        Some(Overlay::Discard(_)) => discard_key(state, key),
        Some(Overlay::Message(_) | Overlay::Fatal(_)) | None => return None,
    };

    Some(effects)
}

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
                // A range carries the old mode's axis; keeping it would address
                // cells never chosen.
                state.contents.rows.clear();
                state.contents.columns.clear();
                state.contents.mode = state.contents.mode.next();
            }
        }
        // Column mode only: outside it the cursor names no column.
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
        Key::Char(']') => walk_tab(state, Step::Forward),
        Key::Char('[') => walk_tab(state, Step::Back),
        // Last, so every binding above outranks a panel number.
        Key::Char(key) => focus_by_number(state, key),
        _ => {}
    }

    Vec::new()
}

pub(super) fn takes_text(state: &State) -> bool {
    match &state.overlay {
        Some(Overlay::Search(_) | Overlay::Export(_) | Overlay::Filter(_)) => true,
        Some(Overlay::Drop(prompt)) => prompt.takes_text(),
        Some(Overlay::Commands(panel)) => panel.sift().is_some(),
        Some(
            Overlay::Themes { sift, .. }
            | Overlay::Connections { sift, .. }
            | Overlay::History { sift, .. },
        ) => sift.is_some(),
        Some(Overlay::Inspect(inspector)) => inspector.field().is_some(),
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

/// Unqualified table names complete against the keyspace Tables is showing.
pub(super) fn complete(state: &mut State) {
    let keyspace = state.visible_keyspace();
    let completion = cql::suggest(state.queries.buffer(), state.schema(), keyspace.as_deref());
    state.queries.offer(completion);
}

pub(super) fn format_buffer(state: &mut State) {
    if state.focus() != Panel::Queries {
        return;
    }

    state.queries.format();
}

pub(super) fn walk_completion(state: &mut State, forwards: bool) {
    if state.focus() != Panel::Queries {
        return;
    }

    if !state.queries.step(forwards) {
        complete(state);
    }
}

/// One rung per press: the thing most recently put on screen comes off first.
pub(super) fn escape(state: &mut State) {
    if dismiss_overlay(state) {
        return;
    }

    if state.focus() == Panel::Contents && state.contents.stop_fetching() {
        state.notify(Severity::Info, "stopped fetching pages");
        return;
    }

    if state.takes_text() {
        state.queries.stop_editing();
        return;
    }

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

pub(super) fn dismiss_overlay(state: &mut State) -> bool {
    // Before `take()`: a click reaches Export here, while keyboard `Esc` goes
    // through `export_key`.
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
        // `Disconnected`, not `Unchosen`, so `offer_connections` does not
        // reopen it.
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
        // Applies nothing: only `⏎` writes a filter.
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
            // Dismissing is not answering: the edits stay.
            | Overlay::Commit(_)
            | Overlay::Discard(_),
        ) => {
            // Startup diagnostics come before the picker.
            offer_connections(state);
            true
        }
        // A click routed here by `handle_pointer` must not end the modal.
        Some(fatal @ Overlay::Fatal(_)) => {
            state.overlay = Some(fatal);
            true
        }
        None => false,
    }
}

pub(super) fn diagnostics_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        state.overlay = None;
        // Startup diagnostics come before the picker.
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

/// Above the palette: `palette_key` matches the key alone, so `ctrl-j` would
/// move its cursor.
pub(super) fn ctrl_key(state: &mut State, key: Key) -> Vec<Effect> {
    // `ctrl-d`/`ctrl-u` are `PageDown`/`PageUp`, so they reach an open overlay
    // too.
    match key {
        Key::Char('d') => return page(state, Key::PageDown),
        Key::Char('u') => return page(state, Key::PageUp),
        _ => {}
    }

    if key == Key::Char('s') && matches!(state.overlay, Some(Overlay::Inspect(_))) {
        return commit_inspected_row(state);
    }
    if let Some(effects) = export_chord(state, key) {
        return effects;
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
        Key::Char('c') => cancel(state),
        Key::Char('t') => {
            open_themes(state);
            Vec::new()
        }
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

pub(super) fn open_editor(state: &State) -> Vec<Effect> {
    if state.focus() != Panel::Queries {
        return Vec::new();
    }

    vec![Effect::OpenEditor {
        buffer: state.queries.text(),
    }]
}

pub(super) fn columnar(state: &State) -> bool {
    state.focus() == Panel::Contents
        && state.contents.mode == SelectionMode::Column
        && state.contents.column_count() > 0
}

pub(super) fn column_of(state: &State) -> usize {
    state.contents.column.index()
}

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

pub(super) enum Decision {
    Nothing,
    Close,
    Run,
}

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

/// Delegates so the two spellings cannot drift.
pub(super) fn page(state: &mut State, key: Key) -> Vec<Effect> {
    match &state.overlay {
        Some(Overlay::Commands(_)) => palette_key(state, key),
        Some(Overlay::Themes { .. }) => themes_key(state, key),
        Some(Overlay::Connections { .. }) => connections_key(state, key),
        Some(Overlay::History { .. }) => history_key(state, key),
        Some(Overlay::Health(_)) => health_key(state, key),
        Some(Overlay::Diagnostics(_)) => diagnostics_key(state, key),
        Some(Overlay::Inspect(_)) => inspect_key(state, key),
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
