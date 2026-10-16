//! The query history picker.

use crate::history::one_line;
use crate::model::Key;
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::search::Sift;
use crate::state::State;
use crate::style::Severity;
use crate::viewport::Cursor;

use super::{Effect, escape, picker_matches, quit};

/// Shared with the view, for [`theme_labels`]'s reason.
pub(crate) fn history_labels(state: &State) -> Vec<String> {
    state
        .history()
        .iter()
        .map(|entry| one_line(entry))
        .collect()
}

/// Refused with nothing to list, so the bar greys it instead.
pub(super) fn open_history(state: &mut State) {
    let busy = matches!(state.overlay, Some(Overlay::Search(_)));

    if state.focus() == Panel::Queries && !busy && state.config.history.limit.is_off() {
        state.notify(Severity::Info, "history is disabled");
        return;
    }

    if state.focus() != Panel::Queries || busy {
        return;
    }
    if state.history().is_empty() {
        state.notify(Severity::Info, "nothing in the history yet");
        return;
    }
    if state.geometry.history_rows() == 0 {
        state.notify(Severity::Info, "no room to draw the history");
        return;
    }

    state.overlay = Some(Overlay::History {
        cursor: Cursor::default(),
        sift: None,
    });
}

pub(super) fn history_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(state.overlay, Some(Overlay::History { sift: Some(_), .. })) {
        return history_sift_key(state, key);
    }

    match key {
        Key::Char('q') => return quit(state),
        Key::Enter => {
            append(state);
            return Vec::new();
        }
        Key::Escape => {
            escape(state);
            return Vec::new();
        }
        _ => {}
    }

    let last = state.history().len();
    let height = state.geometry.history_rows();
    let Some(Overlay::History { cursor, sift }) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => cursor.down(last),
        Key::Char('k') | Key::Up => cursor.up(),
        Key::PageDown => cursor.page_down(last, height),
        Key::PageUp => cursor.page_up(height),
        Key::Home => cursor.to_start(),
        Key::End => cursor.to_end(last),
        Key::Char('/') => *sift = Some(Sift::opened(cursor.index())),
        _ => {}
    }

    Vec::new()
}

/// `⏎` closes the prompt and does not load: the cursor's row may not be the one
/// being narrowed to.
fn history_sift_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let labels = history_labels(state);
    let Some(Overlay::History { cursor, sift }) = &mut state.overlay else {
        return Vec::new();
    };
    let Some(open) = sift else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => open.push(character),
        Key::Backspace => {
            if !open.pop() {
                *sift = None;
                return Vec::new();
            }
        }
        Key::Enter => {
            let at = picker_matches(&labels, open.search().as_ref())
                .get(cursor.index())
                .copied()
                .unwrap_or_else(|| open.origin());
            *sift = None;
            cursor.go_to(at, labels.len());
            return Vec::new();
        }
        _ => return Vec::new(),
    }

    let origin = open.origin();
    let matches = picker_matches(&labels, open.search().as_ref());
    let seat = matches
        .iter()
        .position(|at| *at >= origin)
        .unwrap_or_else(|| matches.len().saturating_sub(1));
    cursor.go_to(seat, matches.len());

    Vec::new()
}

/// Appends, not runs: `ctrl-r` is the one key that sends.
fn append(state: &mut State) {
    let labels = history_labels(state);
    let Some(Overlay::History { cursor, sift }) = &state.overlay else {
        return;
    };
    let search = sift.as_ref().and_then(Sift::search);
    let Some(entry) = picker_matches(&labels, search.as_ref())
        .get(cursor.index())
        .copied()
        .and_then(|at| state.history().get(at))
        .cloned()
    else {
        return;
    };

    state.overlay = None;
    state.queries.append(&entry);
}
