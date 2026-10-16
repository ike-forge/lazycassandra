//! Dropping a keyspace or table, or deleting rows.

use crate::contents::{ContentsState, Subject, TableRef};
use crate::drop::{DropPrompt, Target};
use crate::jobs::Dropping;
use crate::log::{Outcome, Trace};
use crate::model::{DbError, Key, QueryId, Row, Statement};
use crate::overlay::{Message, Overlay};
use crate::panel::Panel;
use crate::state::State;
use crate::style::Severity;
use crate::tree::Node;

use super::{
    Decision, Effect, edit_line, logged, raise, refuse_read_only, sentence, spanned,
    statement_subject, submit, submit_into_contents,
};

/// Shared by `d` and the hint bar, so the bar cannot promise what the key
/// refuses. Session-level refusals are [`open_drop`]'s.
pub(crate) fn drop_target(state: &State) -> Option<Target> {
    match state.focus() {
        Panel::Tables => tree_target(state),
        Panel::Contents => rows_target(state),
        Panel::Ddl | Panel::Queries => None,
    }
}

/// Not the range: "drop these four" is a different question.
pub(super) fn tree_target(state: &State) -> Option<Target> {
    if state.tables.selects_many() {
        return None;
    }

    match state.tables.selected(state.schema()?)? {
        Node::Keyspace { keyspace, .. } => Some(Target::Keyspace(keyspace.name.clone())),
        Node::Table { keyspace, table } => {
            Some(Target::Table(TableRef::new(&keyspace.name, &table.name)))
        }
    }
}

/// The whole range, by primary key, in every selection mode: a column range
/// addresses no row.
pub(super) fn rows_target(state: &State) -> Option<Target> {
    let ContentsState::Loaded { subject, page, .. } = state.contents.state() else {
        return None;
    };
    // A typed statement's table name is not trusted for a `WHERE` we write.
    let Subject::Table(table) = subject else {
        return None;
    };
    let described = state
        .schema()?
        .keyspaces
        .get(&table.keyspace)?
        .tables
        .get(&table.table)?;

    let selected = spanned(&state.contents.rows, state.contents.cursor.index());
    // `GridPage::row`: the cursor counts drawn rows, so indexing `page.rows`
    // deletes the wrong row in a sorted or filtered grid.
    let rows: Vec<&Row> = selected
        .indices()
        .map(|index| page.row(index))
        .collect::<Option<Vec<_>>>()?;

    Target::rows(table, described, &page.page().columns, rows.into_iter())
}

/// Silent refusals are greyed on the bar. The posture refusal is not silent:
/// the user is owed which connection forbids it.
pub(super) fn open_drop(state: &mut State) {
    if !state.posture().permits_mutation() {
        refuse_read_only(state, "drop and delete");
        return;
    }

    if state.dropping.is_some() {
        state.notify(Severity::Info, "a drop is already running");
        return;
    }
    if !state.geometry.drop_fits() {
        state.notify(Severity::Info, "no room to draw the confirmation");
        return;
    }

    let Some(target) = drop_target(state) else {
        match state.focus() {
            Panel::Tables if state.tables.selects_many() => {
                state.notify(Severity::Info, "drop takes one node, not a selection");
            }
            Panel::Contents => state.notify(
                Severity::Info,
                "these rows cannot be addressed by primary key",
            ),
            Panel::Tables | Panel::Ddl | Panel::Queries => {}
        }
        return;
    };

    state.overlay = Some(Overlay::Drop(DropPrompt::opened(target, state.posture())));
    state.dirty = true;
}

/// `⏎` runs the typed spelling only once the name matches.
pub(super) fn drop_key(state: &mut State, key: Key) -> Vec<Effect> {
    let decision = {
        let Some(Overlay::Drop(prompt)) = &mut state.overlay else {
            return Vec::new();
        };

        if prompt.takes_text() {
            typed_key(prompt, key)
        } else {
            yes_no_key(key)
        }
    };

    state.dirty = true;

    match decision {
        Decision::Nothing => Vec::new(),
        Decision::Close => {
            state.overlay = None;
            Vec::new()
        }
        Decision::Run => start_drop(state),
    }
}

pub(super) const fn yes_no_key(key: Key) -> Decision {
    match key {
        Key::Char('y') => Decision::Run,
        Key::Char('n') | Key::Escape => Decision::Close,
        _ => Decision::Nothing,
    }
}

/// Every printable key is text, `n` included: `nation` must be typeable.
pub(super) fn typed_key(prompt: &mut DropPrompt, key: Key) -> Decision {
    match key {
        Key::Escape => Decision::Close,
        Key::Enter if prompt.confirmed() => Decision::Run,
        Key::Enter => Decision::Nothing,
        _ => {
            if let Some(line) = prompt.typed_mut() {
                edit_line(line, key);
            }
            Decision::Nothing
        }
    }
}

/// `submit` re-checks the posture, so one that changed under an open
/// confirmation is still refused.
pub(super) fn start_drop(state: &mut State) -> Vec<Effect> {
    let Some(Overlay::Drop(prompt)) = state.overlay.take() else {
        return Vec::new();
    };

    let target = prompt.target().clone();
    let stmt = prompt.statement();
    let log_subject = statement_subject(&stmt);

    match submit(state, stmt) {
        Ok((query, effects)) => {
            state.dropping = Some(Dropping::started(query, target));
            effects
        }
        Err(error) => {
            // Logged too: the modal is gone once dismissed.
            logged(
                state,
                log_subject,
                Outcome::Rejected {
                    reason: error.to_string(),
                },
            );
            raise(
                state,
                Message {
                    title: "Not dropped".to_owned(),
                    message: error.to_string(),
                    severity: Severity::Danger,
                },
            );
            Vec::new()
        }
    }
}

/// Dropped unless `query` is the outstanding drop: both result arms call this.
/// Success reloads the schema, since the tree names a node that is gone.
pub(super) fn finish_drop(
    state: &mut State,
    query: QueryId,
    error: Option<&DbError>,
) -> Vec<Effect> {
    if state.dropping.as_ref().map(Dropping::query) != Some(query) {
        return Vec::new();
    }
    let Some(dropped) = state.dropping.take() else {
        return Vec::new();
    };
    state.dirty = true;

    if let Some(error) = error {
        raise(
            state,
            Message {
                title: "Not dropped".to_owned(),
                message: error.to_string(),
                severity: Severity::Danger,
            },
        );
        return Vec::new();
    }

    state.notify(
        Severity::Info,
        match dropped.target() {
            Target::Rows { .. } => format!("deleted {}", dropped.target().qualified()),
            Target::Keyspace(_) | Target::Table(_) => {
                format!("dropped {}", dropped.target().qualified())
            }
        },
    );

    match dropped.target() {
        Target::Rows { table, .. } => {
            let table = table.clone();
            let stmt = Statement::select_all(&table.keyspace, &table.table);

            submit_into_contents(state, Subject::Table(table), stmt)
        }
        Target::Keyspace(_) | Target::Table(_) => {
            let Some(conn) = state.conn_id() else {
                return Vec::new();
            };
            logged(
                state,
                sentence("Reading schema"),
                Outcome::Pending {
                    trace: Trace::Schema(conn),
                },
            );

            vec![Effect::LoadSchema(conn)]
        }
    }
}
