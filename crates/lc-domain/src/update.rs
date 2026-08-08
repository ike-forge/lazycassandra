//! The one place state changes.
//!
//! `update` is synchronous, pure, and total. It mutates state and *describes*
//! what should happen next; it cannot open a socket because it has nothing to
//! open one with (architecture.md §3).

use crate::model::{ConnId, DbError, KeyPress, Page, PagingState, QueryId, Schema, Statement};
use crate::posture::Posture;
use crate::state::State;

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

/// Applies a message and returns the effects it implies.
pub fn update(state: &mut State, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Tick => Vec::new(),

        Msg::Input(_) | Msg::Resized => {
            state.mark_dirty();
            Vec::new()
        }

        Msg::Connected { conn, posture } => {
            state.connection = Some(crate::state::Connection {
                id: conn,
                posture,
                schema: None,
            });
            state.mark_dirty();
            vec![Effect::LoadSchema(conn)]
        }

        Msg::SchemaLoaded { conn, schema } => {
            if let Some(connection) = state.connection.as_mut()
                && connection.id == conn
            {
                connection.schema = Some(*schema);
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
    use crate::model::StatementKind;
    use crate::state::Connection;

    fn connected(posture: Posture) -> State {
        State {
            connection: Some(Connection {
                id: ConnId::next(),
                posture,
                schema: None,
            }),
            ..State::default()
        }
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
        let connection = state.connection.expect("connection is still present");
        assert!(
            connection.schema.is_none(),
            "schema from a connection we are no longer on must not be applied"
        );
    }
}
