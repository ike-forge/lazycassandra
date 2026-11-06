//! The consistency picker: the level the connection's next statements run at.

use std::time::Duration;

use crate::config::Consistency;
use crate::connection::ConnectionState;
use crate::log::Outcome;
use crate::model::Key;
use crate::overlay::Overlay;
use crate::state::State;
use crate::style::Severity;
use crate::viewport::Cursor;

use super::{Effect, escape, logged, quit, sentence};

/// A level belongs to a connection, so there is nothing to set before one.
/// Refused over a live prompt, which a ctrl chord would otherwise replace.
pub(super) fn open_consistency(state: &mut State) {
    if !matches!(state.connection, ConnectionState::Connected(_)) {
        state.notify(Severity::Info, "connect first: a level is per connection");
        return;
    }
    let busy = matches!(state.overlay, Some(Overlay::Search(_)));
    if busy || Consistency::ALL.len() > state.geometry.consistency_rows() {
        return;
    }

    let mut cursor = Cursor::default();
    let current = Consistency::ALL
        .iter()
        .position(|level| *level == state.consistency())
        .unwrap_or(0);
    cursor.go_to(current, Consistency::ALL.len());

    state.overlay = Some(Overlay::Consistency { cursor });
}

/// Motion previews nothing: the level is only worth seeing once it is used.
pub(super) fn consistency_key(state: &mut State, key: Key) -> Vec<Effect> {
    match key {
        Key::Char('q') => return quit(state),
        Key::Enter => {
            choose(state);
            return Vec::new();
        }
        Key::Escape => {
            escape(state);
            return Vec::new();
        }
        _ => {}
    }

    let last = Consistency::ALL.len();
    let Some(Overlay::Consistency { cursor }) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => cursor.down(last),
        Key::Char('k') | Key::Up => cursor.up(),
        Key::PageDown | Key::End => cursor.to_end(last),
        Key::PageUp | Key::Home => cursor.to_start(),
        _ => {}
    }

    Vec::new()
}

fn choose(state: &mut State) {
    let Some(Overlay::Consistency { cursor }) = &state.overlay else {
        return;
    };
    let Some(level) = Consistency::ALL.get(cursor.index()).copied() else {
        return;
    };
    state.overlay = None;

    let ConnectionState::Connected(connection) = &mut state.connection else {
        return;
    };
    connection.consistency = level;

    // In the trail as cqlsh words it, so a reader of the Logs tab sees where
    // the rows after it changed level.
    logged(
        state,
        sentence(format!("Consistency level set to {}", level.label())),
        Outcome::Succeeded {
            detail: level.meaning().to_owned(),
            took: Duration::ZERO,
        },
    );
    state.mark_dirty();
}
