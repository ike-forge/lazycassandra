//! Statements to the cluster: reads, `ctrl-r`, paging, and their results.

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
    // Answers whichever asked for it. A drop's result reaches the
    // panel too and is dropped there, because the panel is not
    // waiting on that id (drop design §3).
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
    // A later page has no entry of its own to complete, and the rows
    // already loaded stay, so the trail is the one place it can fail.
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

/// `Enter` and `Space`: unfold a keyspace, or read a table.
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

/// Replaces whatever the Contents panel is showing with a read of `table`,
/// and takes focus there.
///
/// Focus moves now rather than when the page lands: it is a consequence of the
/// keypress, and a focus change that waits on the network happens at a time
/// the user cannot predict
/// (docs/superpowers/specs/2026-08-10-table-display-design.md §1).
pub(super) fn read(state: &mut State, table: TableRef) -> Vec<Effect> {
    state.focus_on(Panel::Contents);

    let stmt = Statement::select_all(&table.keyspace, &table.table);
    submit_into_contents(state, Subject::Table(table), stmt)
}

/// `ctrl-r`: the Queries buffer, whole, as one statement (design §3).
pub(super) fn run(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Queries {
        return Vec::new();
    }

    // The statement the cursor is in rather than the whole buffer: the
    // protocol carries one statement per request, so a buffer holding several
    // was never runnable whole, and the band on screen says which one this is
    // (cql statement selection design §5).
    let Some(cql) = state.queries.statement() else {
        return Vec::new();
    };
    let cql = cql.as_str();

    let stmt = Statement::typed(cql);

    // Refused rather than executed: vision.md §4 principle 6 makes typed
    // confirmation the condition on these running at all, and the tree's `d`
    // is the only place that confirmation exists. A statement typed by hand
    // names its target in text this program does not parse, so there is
    // nothing to ask the user to type back (drop design's preamble). This
    // call site becomes the prompt once there is (design §5).
    // Gated on the posture permitting mutation at all: on a read-only
    // connection the confirmation is not what stands between the user and
    // this statement, and naming it would send them looking for a `d` that
    // would refuse them too. `submit_into_contents` raises the posture's own
    // refusal below (posture design §3).
    if stmt.kind.is_destructive() && state.posture().permits_mutation() {
        // Logged before the modal, not instead of it: the modal is gone the
        // moment it is dismissed, and the trail is where the reason survives
        // (query log design §1, §8).
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

    // Only what was sent: a statement the posture refused issues nothing, and
    // is no more worth recalling than one that never left the buffer.
    if !effects.is_empty() {
        effects.extend(record_history(state, cql));
    }

    effects
}

/// Remembers a statement sent from the buffer against the connection in use.
/// Only the buffer: reading a table from the tree is not something the user
/// typed (query history design §2).
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

/// Puts `stmt`'s result in the Contents panel, cancelling whatever it was
/// waiting on. One grid, one outstanding query, whichever way it was asked for.
///
/// Focus is deliberately not touched here: both callers move it themselves —
/// reading from the tree ([`read`]) and `ctrl-r` alike land the user on the
/// rows they asked for. The Queries panel collapses to the query it holds
/// when focus leaves it, so the grid gets the room the editor was using
/// (docs/superpowers/specs/2026-08-16-cql-semantics-design.md §8, which
/// supersedes table display design §1).
pub(super) fn submit_into_contents(
    state: &mut State,
    subject: Subject,
    stmt: Statement,
) -> Vec<Effect> {
    let mut effects = Vec::new();

    // Dropping the id is what actually stops a late page from being applied;
    // the effect is still inert (architecture.md §3).
    if let Some(superseded) = state.contents.outstanding() {
        state.in_flight.remove(&superseded);
        state
            .log
            .complete(Trace::Query(superseded), Outcome::Cancelled);
        effects.push(Effect::Cancel(superseded));
    }

    // Logged here rather than inside `submit`, so every refusal the panel
    // notices — the posture guard and the no-connection guard alike — gets
    // an entry by construction: one arm, not one call per refusal path to
    // remember (query log design §8).
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

            // Logged before the modal, not instead of it: the modal is gone
            // the moment it is dismissed, and the trail is where the reason
            // survives (query log design §1, §8). Only the posture raises
            // one — "not connected" is already what the empty panels say.
            if matches!(error, DbError::ReadOnly) {
                refuse_read_only(state, "a statement that changes anything");
            }
        }
    }

    effects
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
/// to run it on. Neither is logged here — `submit_into_contents`, the one
/// caller, logs every refusal it sees in its `Err` arm, so this function
/// stays the single place that decides whether a statement runs and leaves
/// the trail to the caller that owns it.
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

/// `p`: one more page into the grid, whatever stop `P` last reached
/// (grid paging design §2).
pub(super) fn next_page(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Contents {
        return Vec::new();
    }

    let next = state.contents.fetch_one(QueryId::next);
    follow(state, next)
}

/// `P`: pages until the result ends or the next stop (grid paging design §3).
pub(super) fn every_page(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Contents {
        return Vec::new();
    }

    let next = state.contents.fetch_all(QueryId::next);
    follow(state, next)
}

/// Asks for the page the grid wants next, if it wants one. A continuation is
/// not a new statement: no trail entry and no history (grid paging design §2).
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
