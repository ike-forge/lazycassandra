//! The connection picker and connecting
//! (docs/superpowers/specs/2026-08-31-connection-picker-design.md).

use crate::config::{Config, ConnectionConfig};
use crate::log::{Entry, Outcome, Trace};
use crate::model::{ConnId, Key};
use crate::search::Sift;
use crate::state::{ConnectionState, Cursor, Overlay, State};

use super::{Effect, escape, picker_matches, quit, sentence};

/// The picker's rows in picker order, which is the order the merge defined:
/// global entries first, then local-only ones (configuration design §5).
///
/// The one builder for that order, shared with [`crate::view`]'s
/// `OverlayView::Connections`, for the reason [`theme_labels`] is shared —
/// the search's correctness claim is that `connection_names[i]` names the row
/// the renderer draws at `i`.
pub(crate) fn connection_names(config: &Config) -> Vec<String> {
    config
        .connections
        .iter()
        .map(|connection| connection.name.as_str().to_owned())
        .collect()
}

/// The connection `default_connection` names, as an index into
/// [`connection_names`].
pub(super) fn default_connection_index(config: &Config) -> Option<usize> {
    let name = config.default_connection.as_ref()?;

    config
        .connections
        .iter()
        .position(|connection| connection.name == *name)
}

/// Opens the connection picker, seated on the default connection when there
/// is one and the first entry otherwise.
///
/// Refused where the frame cannot draw the list, for [`open_themes`]'s
/// reason, and over a live prompt for the same reason it is — with one
/// exception: a picker with *nothing* to list is guidance rather than a list,
/// and is opened whatever the row count, because a user with no connections
/// is exactly the user who most needs telling why the panels are empty
/// (connection picker design §3).
pub(super) fn open_connections(state: &mut State) {
    let last = state.config.connections.len();
    let busy = matches!(state.overlay, Some(Overlay::Search(_)));
    // The guidance an empty list draws is a block the size of a message, so
    // it is gated on the signal every other modal is gated on rather than on
    // rows it does not have (design §3, §7).
    let fits = if last == 0 {
        state.geometry.command_rows() > 0
    } else {
        last <= state.geometry.connection_rows()
    };

    if busy || !fits {
        return;
    }

    let mut cursor = Cursor::default();
    cursor.go_to(default_connection_index(&state.config).unwrap_or(0), last);

    state.overlay = Some(Overlay::Connections { cursor, sift: None });
}

/// What an open picker responds to.
///
/// Unlike [`themes_key`] a motion previews nothing: a connection is reached
/// by opening it, and opening one is a round trip to a cluster rather than a
/// repaint. So `⏎` is the only key that acts, and `esc` leaves the connection
/// as it was (connection picker design §3).
pub(super) fn connections_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(
        state.overlay,
        Some(Overlay::Connections { sift: Some(_), .. })
    ) {
        return connections_sift_key(state, key);
    }

    match key {
        Key::Char('q') => return quit(state),
        Key::Enter => return choose_connection(state),
        // Nothing to search when there is nothing listed: the bar names no
        // `/` on an empty picker, and the key must agree with it.
        Key::Char('/') if state.config.connections.is_empty() => return Vec::new(),
        Key::Escape => {
            escape(state);
            return Vec::new();
        }
        _ => {}
    }

    let last = state.config.connections.len();
    let height = state.geometry.connection_rows();
    let Some(Overlay::Connections { cursor, sift }) = &mut state.overlay else {
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

/// What a picker with a live sift responds to — [`themes_sift_key`]'s
/// contract, over a list that previews nothing.
fn connections_sift_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let names = connection_names(&state.config);
    let Some(Overlay::Connections { cursor, sift }) = &mut state.overlay else {
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
        // `⏎` closes the prompt onto the row it left the cursor on rather
        // than connecting: a term typed to find an entry should not also open
        // it, when the entry the cursor happens to hold may not be the one
        // the user was still narrowing towards (connection picker design §5).
        Key::Enter => {
            // A term that matched nothing holds no row, so the cursor goes
            // back where the prompt opened rather than staying on the row it
            // was pushed to — the same landing `Esc` makes. Leaving it would
            // seat the cursor on an entry the filter had hidden, and the next
            // `⏎` would open a connection the user never saw
            // (connection picker design §5).
            let at = picker_matches(&names, open.search().as_ref())
                .get(cursor.index())
                .copied()
                .unwrap_or_else(|| open.origin());
            *sift = None;
            cursor.go_to(at, names.len());
            return Vec::new();
        }
        _ => return Vec::new(),
    }

    let origin = open.origin();
    let matches = picker_matches(&names, open.search().as_ref());
    let seat = matches
        .iter()
        .position(|at| *at >= origin)
        .unwrap_or_else(|| matches.len().saturating_sub(1));
    cursor.go_to(seat, matches.len());

    Vec::new()
}

/// Connects to the row the picker's cursor holds.
///
/// The cursor counts in the rows a live sift left, so it is resolved through
/// those before the config is asked — [`preview_theme`]'s rule, applied to a
/// list where landing on the wrong row costs a connection rather than a
/// repaint. A term that matched nothing holds no row and connects to nothing.
fn choose_connection(state: &mut State) -> Vec<Effect> {
    let names = connection_names(&state.config);
    let Some(Overlay::Connections { cursor, sift }) = &state.overlay else {
        return Vec::new();
    };
    let search = sift.as_ref().and_then(Sift::search);
    let Some(target) = picker_matches(&names, search.as_ref())
        .get(cursor.index())
        .copied()
        .and_then(|at| state.config.connections.get(at))
        .cloned()
    else {
        return Vec::new();
    };

    state.overlay = None;

    connect(state, target)
}

/// Begins a connection to `target`, whatever the state was before.
///
/// Shared by the startup path and the picker: an id is minted here so a
/// result that arrives after the user has moved on can be dropped by
/// `is_current`, which is the whole reason [`ConnId`] is carried
/// (architecture.md §3).
pub(super) fn connect(state: &mut State, target: ConnectionConfig) -> Vec<Effect> {
    let conn = ConnId::next();
    let mut effects = Vec::new();

    // The grid's cursor belongs to the session being left (grid paging design
    // §2).
    if let Some(query) = state.contents.end_paging() {
        state.in_flight.remove(&query);
        effects.push(Effect::Cancel(query));
    }

    state.connection = ConnectionState::Connecting {
        conn,
        name: target.name.clone(),
    };
    state.log.push(Entry::new(
        state.now,
        sentence(format!("Connecting to {}", target.name.as_str())),
        Outcome::Pending {
            trace: Trace::Connect(conn),
        },
    ));

    effects.push(Effect::Connect {
        conn,
        target: Box::new(target),
        driver: state.config.driver,
    });
    effects
}

/// Opens the picker where the frame is free and nothing has been chosen.
///
/// Called from the two places the frame becomes free with the connection
/// still unmade: the first measurement, since `init` runs before there is a
/// frame, and a dismissal, since the diagnostics modal claims startup's first
/// frame whenever the config had anything wrong with it.
///
/// Keyed on [`ConnectionState::Unchosen`] and not on "not connected", which
/// is what makes a picker the user has waved away stay away: dismissing it
/// leaves `Disconnected`, and nothing here re-offers that.
pub(super) fn offer_connections(state: &mut State) {
    if !matches!(state.connection, ConnectionState::Unchosen) || state.overlay.is_some() {
        return;
    }

    open_connections(state);

    if state.overlay.is_some() {
        state.mark_dirty();
    }
}
