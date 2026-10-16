use crate::connection::ConnectionState;
use crate::contents::{ContentsState, Continue, Failure, Landed, Subject, TableRef};
use crate::log::{Outcome, Trace};
use crate::model::{DbError, Page, QueryId, Statement};
use crate::overlay::Message;
use crate::panel::Panel;
use crate::state::State;
use crate::style::Severity;
use crate::tree::Node;
use std::time::Duration;

use super::commit::finish_commit;
use super::drop::finish_drop;
use super::{Effect, logged, raise, refuse_read_only, sentence, statement_subject};

pub(super) fn page_ready(
    state: &mut State,
    query: QueryId,
    page: Page,
    took: Duration,
) -> Vec<Effect> {
    if state.in_flight.remove(&query).is_none() {
        return Vec::new();
    }
    let rows = page.rows.len();
    // A drop's result reaches the panel too and is dropped there: the panel is
    // not waiting on that id.
    let landed = state.contents.page_ready(
        query,
        page,
        state.config.grid.max_column_width,
        QueryId::next,
    );
    state.mark_dirty();

    match landed {
        Landed::Elsewhere | Landed::Read => {
            state.log.complete(
                Trace::Query(query),
                Outcome::Succeeded {
                    detail: format!("{rows} rows"),
                    took,
                },
            );
            let mut effects = finish_drop(state, query, None);
            effects.extend(finish_commit(state, query, None));
            effects
        }
        Landed::Skipped(next) => {
            state
                .log
                .retrace(Trace::Query(query), Trace::Query(next.query));
            follow(state, Some(next))
        }
        Landed::Appended(next) => follow(state, next),
    }
}

pub(super) fn query_failed(state: &mut State, query: QueryId, error: &DbError) -> Vec<Effect> {
    if state.in_flight.remove(&query).is_none() {
        return Vec::new();
    }
    let outcome = Outcome::Failed {
        detail: error.to_string(),
    };
    // A later page has no entry of its own and the loaded rows stay, so the
    // trail is the one place it fails.
    if state.contents.failed(query, error) == Some(Failure::Page) {
        let label = match state.contents.state() {
            ContentsState::Loaded { subject, .. } => subject.label(),
            _ => String::new(),
        };
        logged(
            state,
            sentence(format!("Reading the next page of {label}")),
            outcome,
        );
    } else {
        state.log.complete(Trace::Query(query), outcome);
    }
    state.mark_dirty();
    let mut effects = finish_drop(state, query, Some(error));
    effects.extend(finish_commit(state, query, Some(error)));
    effects
}

pub(super) fn activate(state: &mut State) -> Vec<Effect> {
    if state.tables.selects_many() {
        return Vec::new();
    }

    if state.focus() != Panel::Tables {
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

/// Focus moves now rather than when the page lands: a focus change that waits
/// on the network is unpredictable.
pub(super) fn read(state: &mut State, table: TableRef) -> Vec<Effect> {
    state.focus_on(Panel::Contents);

    let stmt = Statement::select_all(&table.keyspace, &table.table);
    submit_into_contents(state, Subject::Table(table), stmt)
}

pub(super) fn run(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Queries {
        return Vec::new();
    }

    // One statement per request: the protocol carries no more.
    let Some(cql) = state.queries.statement() else {
        return Vec::new();
    };
    let cql = cql.as_str();

    let stmt = Statement::typed(cql);

    // Refused: a typed statement names its target in text nothing parses, so
    // there is nothing to confirm against (vision.md §4 principle 6). On a
    // read-only connection the posture's refusal is raised instead.
    if stmt.kind.is_destructive() && state.posture().permits_mutation() {
        // Logged too: the modal is gone once dismissed, and the trail keeps the
        // reason.
        logged(
            state,
            statement_subject(&stmt),
            Outcome::Rejected {
                reason: "dropping and truncating from the buffer wait on a typed \
                         confirmation this statement gives nothing to type"
                    .to_owned(),
            },
        );
        raise(
            state,
            Message {
                title: "Not run".to_owned(),
                message: "Dropping and truncating from the buffer wait on a typed \
                          confirmation. Drop a keyspace or a table with `d` on the Tables \
                          panel, which asks for one."
                    .to_owned(),
                severity: Severity::Danger,
            },
        );
        return Vec::new();
    }

    state.focus_on(Panel::Contents);

    let mut effects = submit_into_contents(state, Subject::Typed(cql.to_owned()), stmt);

    // A refused statement sent nothing and is not worth recalling.
    if !effects.is_empty() {
        effects.extend(record_history(state, cql));
    }

    effects
}

/// Only the buffer: reading a table from the tree is not something the user
/// typed.
pub(super) fn record_history(state: &mut State, cql: &str) -> Option<Effect> {
    let ConnectionState::Connected(connection) = &state.connection else {
        return None;
    };
    let name = connection.name.clone();

    state
        .histories
        .record(&name, cql, state.config.history.limit)
        .then(|| Effect::SaveHistory {
            entries: state.histories.of(&name).to_vec(),
            connection: name,
        })
}

/// Focus is left to the callers.
pub(super) fn submit_into_contents(
    state: &mut State,
    subject: Subject,
    stmt: Statement,
) -> Vec<Effect> {
    let mut effects = Vec::new();

    // Dropping the id is what stops a late page; the effect is inert.
    if let Some(superseded) = state.contents.outstanding() {
        state.in_flight.remove(&superseded);
        state
            .log
            .complete(Trace::Query(superseded), Outcome::Cancelled);
        effects.push(Effect::Cancel(superseded));
    }

    // Logged here so every refusal `submit` returns gets an entry.
    let log_subject = statement_subject(&stmt);
    match submit(state, stmt.clone()) {
        Ok((query, submitted)) => {
            state.contents.requested(query, subject, stmt);
            effects.extend(submitted);
        }
        Err(error) => {
            logged(
                state,
                log_subject,
                Outcome::Rejected {
                    reason: error.to_string(),
                },
            );
            state.contents.refused(subject, &error);

            // Logged too: the modal is gone once dismissed. Only the posture
            // raises one; "not connected" shows in the empty panels.
            if matches!(error, DbError::ReadOnly) {
                refuse_read_only(state, "a statement that changes anything");
            }
        }
    }

    effects
}

/// The posture gate lives here because the TUI constructs no effects.
///
/// # Errors
///
/// [`DbError::ReadOnly`] if the statement mutates and the posture forbids it;
/// [`DbError::Unreachable`] with no connection.
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

    logged(
        state,
        statement_subject(&stmt),
        Outcome::Pending {
            trace: Trace::Query(query),
        },
    );

    let execute = if stmt.mutates() {
        Effect::Execute {
            conn,
            query,
            stmt: Box::new(stmt.clone()),
        }
    } else {
        Effect::ExecutePaged {
            conn,
            query,
            stmt: Box::new(stmt.clone()),
            page_size: state.config.grid.page_rows.into(),
            cursor: None,
        }
    };

    Ok((query, vec![execute, Effect::RecordHistory(Box::new(stmt))]))
}

pub(super) fn next_page(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Contents {
        return Vec::new();
    }

    let next = state.contents.fetch_one(QueryId::next);
    follow(state, next)
}

pub(super) fn every_page(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Contents {
        return Vec::new();
    }

    let next = state.contents.fetch_all(QueryId::next);
    follow(state, next)
}

/// A continuation is not a new statement: no trail entry and no history.
pub(super) fn follow(state: &mut State, next: Option<Continue>) -> Vec<Effect> {
    let Some(Continue {
        query,
        stmt,
        cursor,
    }) = next
    else {
        return Vec::new();
    };
    let Some(conn) = state.conn_id() else {
        state
            .contents
            .failed(query, &DbError::Unreachable("not connected".to_owned()));
        return Vec::new();
    };

    state.in_flight.insert(query, stmt.clone());
    state.mark_dirty();

    vec![Effect::ExecutePaged {
        conn,
        query,
        stmt: Box::new(stmt),
        page_size: state.config.grid.page_rows.into(),
        cursor: Some(cursor),
    }]
}
