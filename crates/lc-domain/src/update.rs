//! The one place state changes.
//!
//! `update` is synchronous, pure, and total. It mutates state and *describes*
//! what should happen next; it cannot open a socket because it has nothing to
//! open one with (architecture.md §3).

use crate::model::{ConnId, DbError, KeyPress, Page, PagingState, QueryId, Schema, Statement};
use crate::posture::Posture;
use crate::state::{ConnectionState, Overlay, SchemaState, State};
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

        Msg::Input(_) | Msg::Resized => {
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
            let rows = schema.keyspaces.len();
            let applied = match &mut state.connection {
                ConnectionState::Connected(connection) if connection.id == conn => {
                    connection.schema = SchemaState::Loaded(*schema);
                    true
                }
                _ => false,
            };
            if applied {
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

        Msg::Quit => {
            state.should_quit = true;
            let cancels = state.in_flight.keys().copied().map(Effect::Cancel);
            cancels.chain(std::iter::once(Effect::Quit)).collect()
        }
    }
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
    use crate::model::{Keyspace, Replication, StatementKind};
    use crate::state::Connection;
    use std::collections::BTreeMap;

    fn schema_of(names: &[&str]) -> Schema {
        Schema {
            keyspaces: names
                .iter()
                .map(|name| {
                    (
                        (*name).to_owned(),
                        Keyspace {
                            name: (*name).to_owned(),
                            replication: Replication::SimpleStrategy { factor: 1 },
                            tables: BTreeMap::new(),
                        },
                    )
                })
                .collect(),
        }
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
}
