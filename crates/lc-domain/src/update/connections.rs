//! The connection picker and connecting (connection picker design).

use std::time::Duration;

use crate::config::{Config, ConnectionConfig};
use crate::connection::{Connection, ConnectionState, SchemaState};
use crate::log::{Entry, Outcome, Trace};
use crate::model::{ConnId, DbError, Key, Schema};
use crate::overlay::{Message, Overlay};
use crate::posture::Posture;
use crate::search::Sift;
use crate::state::State;
use crate::style::Severity;
use crate::viewport::Cursor;

use super::{Effect, escape, logged, picker_matches, quit, sentence};

/// Shared with the view, for [`theme_labels`]'s reason.
pub(crate) fn connection_names(config: &Config) -> Vec<String> {
    config
        .connections
        .iter()
        .map(|connection| connection.name.as_str().to_owned())
        .collect()
}

pub(super) fn default_connection_index(config: &Config) -> Option<usize> {
    let name = config.default_connection.as_ref()?;

    config
        .connections
        .iter()
        .position(|connection| connection.name == *name)
}

/// An empty picker opens whatever the room: it explains the empty panels.
pub(super) fn open_connections(state: &mut State) {
    let last = state.config.connections.len();
    let busy = matches!(state.overlay, Some(Overlay::Search(_)));
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

/// Motion previews nothing: opening a connection is a round trip, not a
/// repaint.
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
        // The bar names no `/` on an empty picker.
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
        // `⏎` closes the prompt rather than connecting: the row the cursor
        // holds may not be the one being narrowed to.
        Key::Enter => {
            // A term that matched nothing returns the cursor to its origin, so
            // the next `⏎` cannot open a hidden entry.
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

/// A fresh id lets `is_current` drop a result that arrives after the user moved
/// on.
pub(super) fn connect(state: &mut State, target: ConnectionConfig) -> Vec<Effect> {
    let conn = ConnId::next();
    let mut effects = Vec::new();

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

/// Keyed on `Unchosen`, not "not connected": a dismissed picker leaves
/// `Disconnected` and stays away.
pub(super) fn offer_connections(state: &mut State) {
    if !matches!(state.connection, ConnectionState::Unchosen) || state.overlay.is_some() {
        return;
    }

    open_connections(state);

    if state.overlay.is_some() {
        state.mark_dirty();
    }
}

pub(super) fn connected(
    state: &mut State,
    conn: ConnId,
    posture: Posture,
    took: Duration,
) -> Vec<Effect> {
    // Only `Connecting` holds the name the outstanding attempt began with.
    let ConnectionState::Connecting {
        conn: outstanding,
        name,
    } = &state.connection
    else {
        return Vec::new();
    };
    if *outstanding != conn {
        return Vec::new();
    }
    let name = name.clone();
    state.notify(Severity::Info, format!("connected to {}", name.as_str()));

    state.connection = ConnectionState::Connected(Connection {
        id: conn,
        name,
        posture,
        schema: SchemaState::Loading,
    });
    state.log.complete(
        Trace::Connect(conn),
        Outcome::Succeeded {
            detail: format!("connected, {}", posture_label(posture)),
            took,
        },
    );
    logged(
        state,
        sentence("Reading schema"),
        Outcome::Pending {
            trace: Trace::Schema(conn),
        },
    );
    state.mark_dirty();
    vec![Effect::LoadSchema(conn)]
}

pub(super) fn connect_failed(state: &mut State, conn: ConnId, error: DbError) -> Vec<Effect> {
    if !state.connection.is_current(conn) {
        return Vec::new();
    }
    state.log.complete(
        Trace::Connect(conn),
        Outcome::Failed {
            detail: error.to_string(),
        },
    );
    state.overlay = Some(Overlay::Fatal(Message {
        title: "Connection failed".to_owned(),
        message: error.to_string(),
        severity: Severity::Danger,
    }));
    state.connection = ConnectionState::Failed(error);
    state.mark_dirty();
    Vec::new()
}

pub(super) fn schema_failed(state: &mut State, conn: ConnId, error: &DbError) -> Vec<Effect> {
    let applied = match &mut state.connection {
        ConnectionState::Connected(connection) if connection.id == conn => {
            connection.schema = SchemaState::Failed(error.clone());
            true
        }
        _ => false,
    };
    if applied {
        state.log.complete(
            Trace::Schema(conn),
            Outcome::Failed {
                detail: error.to_string(),
            },
        );
        state.overlay = Some(Overlay::Fatal(Message {
            title: "Schema unavailable".to_owned(),
            message: error.to_string(),
            severity: Severity::Danger,
        }));
        state.mark_dirty();
    }
    Vec::new()
}

pub(super) fn schema_loaded(
    state: &mut State,
    conn: ConnId,
    schema: Schema,
    took: Duration,
) -> Vec<Effect> {
    let detail = format!(
        "{} keyspaces, {} tables",
        schema.keyspaces.len(),
        schema
            .keyspaces
            .values()
            .map(|ks| ks.tables.len())
            .sum::<usize>(),
    );
    let applied = match &mut state.connection {
        ConnectionState::Connected(connection) if connection.id == conn => {
            connection.schema = SchemaState::Loaded(schema);
            true
        }
        _ => false,
    };
    if applied {
        state
            .log
            .complete(Trace::Schema(conn), Outcome::Succeeded { detail, took });
        if let Some((tables, schema)) = state.tables_and_schema() {
            tables.clamp(schema);
        }
        state.mark_dirty();
    }
    Vec::new()
}

fn posture_label(posture: Posture) -> &'static str {
    match posture {
        Posture::ReadOnly => "read-only",
        Posture::Writable => "writable",
        Posture::Protected => "protected",
    }
}
