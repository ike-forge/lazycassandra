//! Inserting a row
//! (docs/superpowers/specs/2026-10-12-row-insert-design.md).

use crate::commit::CommitPrompt;
use crate::contents::{ContentsState, Subject};
use crate::inspect::{Inspected, Inspector};
use crate::key;
use crate::model::Statement;
use crate::state::{Overlay, Panel, State};
use crate::style::Severity;

use super::refuse_read_only;

/// The form `i` would open, or the sentence saying why it opens none —
/// empty where the refusal is not worth saying, as for a panel that is not
/// Contents.
///
/// One derivation, read by the key and by the hint bar, so the bar cannot
/// promise a form the key refuses (insert design §1).
pub(crate) fn insert_form(state: &State) -> Result<Inspector, String> {
    if state.focus() != Panel::Contents {
        return Err(String::new());
    }
    let ContentsState::Loaded { subject, .. } = state.contents.state() else {
        return Err(String::new());
    };
    let Subject::Table(table) = subject else {
        return Err(
            "Only a table's own rows can be inserted. This grid came from a typed statement."
                .to_owned(),
        );
    };
    let described = state
        .schema()
        .and_then(|schema| schema.keyspaces.get(&table.keyspace))
        .and_then(|keyspace| keyspace.tables.get(&table.table))
        .ok_or_else(|| format!("The loaded schema no longer has {}.", table.qualified()))?;

    if key::columns(described).is_empty() {
        return Err(format!(
            "{} has no primary key to insert by.",
            table.qualified()
        ));
    }

    Ok(Inspector::inserting(
        subject.clone(),
        described
            .columns()
            .map(|column| column.name.clone())
            .collect(),
    ))
}

/// `i` over Table contents: opens the inspector over a row with nothing in it.
///
/// The posture is the one refusal that is spoken as a modal, for
/// [`super::edit::start_edit`]'s reason; the rest are toasts, which leave the
/// grid where it is.
pub(super) fn open_insert(state: &mut State) {
    if state.focus() != Panel::Contents {
        return;
    }
    if !state.posture().permits_mutation() {
        refuse_read_only(state, "inserting a row");
        return;
    }
    if state.geometry.inspector_rows() == 0 {
        state.notify(Severity::Info, "no room to draw the form");
        return;
    }

    match insert_form(state) {
        Ok(blank) => {
            // What the cluster refused last time, for the same table
            // (insert design §5).
            let form = state
                .contents
                .take_draft()
                .filter(|draft| draft.subject() == blank.subject())
                .unwrap_or(blank);

            state.overlay = Some(Overlay::Inspect(form));
            state.mark_dirty();
        }
        Err(refusal) if refusal.is_empty() => {}
        Err(refusal) => state.notify(Severity::Warning, refusal),
    }
}

/// `⏎` over the form: builds the `INSERT` and opens the confirmation over it,
/// keeping the form for `n` to hand back (insert design §4).
///
/// Refused with a toast rather than a modal wherever the form can be fixed
/// and tried again, which is everywhere but the posture.
pub(super) fn open_insert_commit(state: &mut State) {
    if !state.posture().permits_mutation() {
        refuse_read_only(state, "inserting a row");
        return;
    }
    if state.committing.is_some() {
        state.notify(Severity::Info, "a commit is already running");
        return;
    }
    if state.geometry.commit_rows() == 0 {
        state.notify(Severity::Info, "no room to draw the confirmation");
        return;
    }

    let Some(Overlay::Inspect(form)) = &state.overlay else {
        return;
    };
    let statement = match insert_statement(state, form) {
        Ok(statement) => statement,
        Err(refusal) => {
            state.notify(Severity::Warning, refusal);
            return;
        }
    };
    let Subject::Table(table) = form.subject().clone() else {
        return;
    };

    let Some(Overlay::Inspect(form)) = state.overlay.take() else {
        return;
    };
    state.overlay = Some(Overlay::Commit(CommitPrompt::inserting(
        table, statement, form,
    )));
    state.mark_dirty();
}

/// The `INSERT` the form holds, or the sentence saying what it lacks.
///
/// A row is addressed by its whole primary key, so a form missing any key
/// column writes nothing — and a form with only key columns is still a row
/// (insert design §3).
fn insert_statement(state: &State, form: &Inspector) -> Result<Statement, String> {
    let (Subject::Table(table), Inspected::Row { columns, .. }) =
        (form.subject(), form.inspected())
    else {
        return Err("This is not a form for a row.".to_owned());
    };
    let described = state
        .schema()
        .and_then(|schema| schema.keyspaces.get(&table.keyspace))
        .and_then(|keyspace| keyspace.tables.get(&table.table))
        .ok_or_else(|| format!("The loaded schema no longer has {}.", table.qualified()))?;

    let values: Vec<(String, String)> = form
        .typed()
        .filter_map(|(at, edited)| Some((columns.get(at)?.clone(), edited.literal().to_owned())))
        .collect();

    let missing: Vec<&str> = key::columns(described)
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| values.iter().all(|(column, _)| column != name))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "{} must be set: a row is addressed by its whole primary key.",
            missing.join(", ")
        ));
    }

    Ok(Statement::insert_row(
        &table.keyspace,
        &table.table,
        &values,
    ))
}
