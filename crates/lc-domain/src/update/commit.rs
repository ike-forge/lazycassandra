//! Committing or discarding cell edits.

use crate::commit::CommitPrompt;
use crate::contents::{ContentsState, Subject, TableRef};
use crate::edit::CommitScope;
use crate::jobs::Committing;
use crate::key;
use crate::key::RowKey;
use crate::log::Outcome;
use crate::model::{DbError, Key, QueryId, Statement, UpdatedRow};
use crate::overlay::{Message, Overlay};
use crate::state::State;
use crate::style::Severity;

use super::{
    Effect, field_key, logged, open_insert_commit, quit_now, raise, refuse_read_only,
    statement_subject, submit, submit_into_contents,
};

pub(super) const fn commit_extent(state: &State) -> (usize, usize) {
    (state.geometry.commit_lines(), state.geometry.commit_rows())
}

pub(super) fn open_commit(state: &mut State) -> Vec<Effect> {
    open_scoped_commit(state, CommitScope::All)
}

/// An open field that does not parse keeps the key from doing anything.
pub(super) fn commit_inspected_row(state: &mut State) -> Vec<Effect> {
    if matches!(&state.overlay, Some(Overlay::Inspect(inspector)) if inspector.field().is_some()) {
        field_key(state, Key::Enter);
    }
    let Some(Overlay::Inspect(inspector)) = &state.overlay else {
        return Vec::new();
    };
    if inspector.field().is_some() {
        return Vec::new();
    }
    // A new row is written whole and has no page row to scope to.
    if inspector.is_inserting() {
        open_insert_commit(state);
        return Vec::new();
    }
    let Some(row) = state
        .contents
        .loaded()
        .and_then(|page| page.page_row(inspector.row()))
    else {
        return Vec::new();
    };

    open_scoped_commit(state, CommitScope::Row(row))
}

fn open_scoped_commit(state: &mut State, scope: CommitScope) -> Vec<Effect> {
    if !state.posture().permits_mutation() {
        refuse_read_only(state, "committing an edit");
        return Vec::new();
    }
    if state.committing.is_some() {
        state.notify(Severity::Info, "a commit is already running");
        return Vec::new();
    }
    if state.geometry.commit_rows() == 0 {
        state.notify(Severity::Info, "no room to draw the confirmation");
        return Vec::new();
    }

    let Some((table, statement, rows, cells)) = pending_commit(state, scope) else {
        if state
            .contents
            .loaded()
            .is_some_and(|page| page.edits().cells_in(scope) > 0)
        {
            state.notify(
                Severity::Warning,
                "edited rows cannot be addressed by primary key",
            );
        } else {
            state.notify(Severity::Info, "nothing to commit");
        }
        return Vec::new();
    };

    state.overlay = Some(Overlay::Commit(
        CommitPrompt::opened(table, statement, rows, cells).scoped(scope),
    ));
    state.mark_dirty();
    Vec::new()
}

/// The [`RowKey`] gate is re-applied: the schema can reload between `e` and
/// here.
pub(super) fn pending_commit(
    state: &State,
    scope: CommitScope,
) -> Option<(TableRef, Statement, usize, usize)> {
    let ContentsState::Loaded { subject, page, .. } = state.contents.state() else {
        return None;
    };
    let Subject::Table(table) = subject else {
        return None;
    };
    let cells = page.edits().cells_in(scope);
    if cells == 0 {
        return None;
    }
    let described = state
        .schema()?
        .keyspaces
        .get(&table.keyspace)?
        .tables
        .get(&table.table)?;
    let key = key::columns(described);
    if key.is_empty() {
        return None;
    }

    let columns = &page.page().columns;
    let rows: Vec<UpdatedRow> = page
        .edits()
        .by_row()
        .filter(|(at, _)| scope.admits(*at))
        .map(|(at, edited)| {
            let sets = edited
                .into_iter()
                .map(|(column, edited)| {
                    Some((columns.get(column)?.clone(), edited.literal().to_owned()))
                })
                .collect::<Option<Vec<_>>>()?;

            Some((
                sets,
                RowKey::of(&key, columns, page.page().rows.get(at)?)?
                    .pairs()
                    .to_vec(),
            ))
        })
        .collect::<Option<Vec<_>>>()?;

    let statement = Statement::update_rows(&table.keyspace, &table.table, &rows);

    Some((table.clone(), statement, rows.len(), cells))
}

/// No typed spelling: an `UPDATE` is a `Write`.
pub(super) fn commit_key(state: &mut State, key: Key) -> Vec<Effect> {
    let (lines, height) = commit_extent(state);
    state.mark_dirty();

    let Some(Overlay::Commit(prompt)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => prompt.down(lines, height),
        Key::Char('k') | Key::Up => prompt.up(),
        Key::PageDown => prompt.page_down(lines, height),
        Key::PageUp => prompt.page_up(height),
        Key::Home => prompt.to_start(),
        Key::End => prompt.to_end(lines, height),
        Key::Char('y') => return start_commit(state),
        Key::Char('n') | Key::Escape => {
            let form = prompt.take_form();
            state.overlay = form.map(Overlay::Inspect);
        }
        _ => {}
    }

    Vec::new()
}

/// `submit` re-checks the posture, as in `start_drop`.
pub(super) fn start_commit(state: &mut State) -> Vec<Effect> {
    let Some(Overlay::Commit(mut prompt)) = state.overlay.take() else {
        return Vec::new();
    };

    let table = prompt.table().clone();
    let form = prompt.take_form();
    let scope = prompt.scope();
    let stmt = prompt.statement().clone();
    let log_subject = statement_subject(&stmt);

    match submit(state, stmt) {
        Ok((query, effects)) => {
            state.committing = Some(match form {
                Some(form) => Committing::inserted(query, table, form),
                None => Committing::started(query, table, scope),
            });
            effects
        }
        Err(error) => {
            if let Some(form) = form {
                state.contents.keep_draft(form);
            }
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
                    title: "Not committed".to_owned(),
                    message: error.to_string(),
                    severity: Severity::Danger,
                },
            );
            Vec::new()
        }
    }
}

/// Success writes the edits into the page rather than re-reading it, which
/// would lose the user's place, paging and arrangement.
pub(super) fn finish_commit(
    state: &mut State,
    query: QueryId,
    error: Option<&DbError>,
) -> Vec<Effect> {
    if state.committing.as_ref().map(Committing::query) != Some(query) {
        return Vec::new();
    }
    let Some(mut committed) = state.committing.take() else {
        return Vec::new();
    };
    state.dirty = true;

    if let Some(error) = error {
        if let Some(form) = committed.take_form() {
            state.contents.keep_draft(form);
        }
        raise(
            state,
            Message {
                title: "Not committed".to_owned(),
                message: error.to_string(),
                severity: Severity::Danger,
            },
        );
        return Vec::new();
    }

    // Where a new row sorts is the cluster's to say.
    if committed.is_insert() {
        let table = committed.table().clone();
        state.notify(
            Severity::Info,
            format!("inserted a row into {}", table.qualified()),
        );

        let stmt = Statement::select_all(&table.keyspace, &table.table);
        return submit_into_contents(state, Subject::Table(table), stmt);
    }

    let scope = committed.scope();
    let cells = state.contents.loaded_mut().map_or(0, |page| {
        let cells = page.edits().cells_in(scope);
        page.commit_edits(scope);
        cells
    });

    // The inspector's copy of the page carries its own edit marks.
    if let Some(Overlay::Inspect(inspector)) = &mut state.overlay {
        for at in inspector.edited().to_vec() {
            inspector.unmark(at);
        }
    }

    state.notify(
        Severity::Info,
        format!(
            "committed {cells} {} to {}",
            if cells == 1 { "cell" } else { "cells" },
            committed.table().qualified()
        ),
    );

    Vec::new()
}

/// A second `q` is inert: the question must be answered.
pub(super) fn discard_key(state: &mut State, key: Key) -> Vec<Effect> {
    state.mark_dirty();

    match key {
        Key::Char('y') => {
            state.overlay = None;
            quit_now(state)
        }
        Key::Char('n') | Key::Escape => {
            state.overlay = None;
            Vec::new()
        }
        _ => Vec::new(),
    }
}

pub(super) fn uncommitted(state: &State) -> (usize, usize) {
    match state.contents.state() {
        ContentsState::Loaded { page, .. } => (page.edits().cells(), page.edits().rows()),
        _ => (0, 0),
    }
}
