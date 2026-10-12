//! Committing or discarding cell edits
//! (docs/superpowers/specs/2026-09-18-cell-editing-design.md).

use crate::commit::CommitPrompt;
use crate::contents::{ContentsState, Subject, TableRef};
use crate::key;
use crate::key::RowKey;
use crate::log::Outcome;
use crate::model::{DbError, Key, QueryId, Statement, UpdatedRow};
use crate::state::{Committing, Message, Overlay, State};
use crate::style::Severity;

use super::{
    Effect, has_uncommitted, logged, quit_now, raise, refuse_read_only, statement_subject, submit,
    submit_into_contents,
};

/// The commit block's wrapped line count and body height, as the last frame
/// measured them.
pub(super) const fn commit_extent(state: &State) -> (usize, usize) {
    (state.geometry.commit_lines(), state.geometry.commit_rows())
}

/// `ctrl-s`: builds one statement from every uncommitted edit and opens the
/// confirmation over it (design §5).
///
/// Silently inert with nothing dirty and on a frame with no room for the
/// block; spoken on a read-only connection, for [`start_edit`]'s reason.
pub(super) fn open_commit(state: &mut State) -> Vec<Effect> {
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

    let Some((table, statement, rows, cells)) = pending_commit(state) else {
        if has_uncommitted(state) {
            state.notify(
                Severity::Warning,
                "edited rows cannot be addressed by primary key",
            );
        } else {
            state.notify(Severity::Info, "nothing to commit");
        }
        return Vec::new();
    };

    state.overlay = Some(Overlay::Commit(CommitPrompt::opened(
        table, statement, rows, cells,
    )));
    state.mark_dirty();
    Vec::new()
}

/// The statement every uncommitted edit becomes, with the table and the counts
/// the confirmation's border names.
///
/// `None` with nothing dirty, and where the page's rows cannot all be
/// addressed — the same [`RowKey`] gate `e` applies per row, re-applied here
/// because a schema can be reloaded between the two.
pub(super) fn pending_commit(state: &State) -> Option<(TableRef, Statement, usize, usize)> {
    let ContentsState::Loaded { subject, page, .. } = state.contents.state() else {
        return None;
    };
    let Subject::Table(table) = subject else {
        return None;
    };
    if page.edits().is_empty() {
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

    let cells = page.edits().cells();
    let statement = Statement::update_rows(&table.keyspace, &table.table, &rows);

    Some((table.clone(), statement, rows.len(), cells))
}

/// Every key the commit confirmation claims. A body to read, and one question
/// to answer: `y` runs it, `n` and `Esc` abandon it, and `⏎` answers nothing
/// — an `UPDATE` is a `Write`, so there is no typed spelling (design §5).
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

/// Turns the answered confirmation into the statement it was asking about.
///
/// `submit` is where the posture guard lives, so [`open_commit`]'s own gate is
/// convenience and a posture that changed under a held-open confirmation is
/// still refused here — `start_drop`'s arrangement, and its reason.
pub(super) fn start_commit(state: &mut State) -> Vec<Effect> {
    let Some(Overlay::Commit(prompt)) = state.overlay.take() else {
        return Vec::new();
    };

    let table = prompt.table().clone();
    let inserting = prompt.is_insert();
    let stmt = prompt.statement().clone();
    let log_subject = statement_subject(&stmt);

    match submit(state, stmt) {
        Ok((query, effects)) => {
            state.committing = Some(if inserting {
                Committing::inserted(query, table)
            } else {
                Committing::started(query, table)
            });
            effects
        }
        Err(error) => {
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

/// A commit ended. `error` is `None` on the success the cluster reported.
///
/// Dropped where `query` is not the commit outstanding, which is every
/// ordinary read: this is called from both result arms and the id is the only
/// thing separating a commit's answer from a grid's (architecture.md §3).
///
/// The success arm writes the edits into the page rather than re-reading it:
/// a re-read costs the user their place, their paging and their arrangement,
/// and the values now in the page are exactly the ones the cluster was sent
/// (design §5). A failure leaves them dirty — nothing was written, so nothing
/// about the page has changed.
pub(super) fn finish_commit(
    state: &mut State,
    query: QueryId,
    error: Option<&DbError>,
) -> Vec<Effect> {
    if state.committing.as_ref().map(Committing::query) != Some(query) {
        return Vec::new();
    }
    let Some(committed) = state.committing.take() else {
        return Vec::new();
    };
    state.dirty = true;

    if let Some(error) = error {
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

    // A new row has no place in the page to be written into, and where it
    // sorts is the cluster's to say (insert design §5).
    if committed.is_insert() {
        let table = committed.table().clone();
        state.notify(
            Severity::Info,
            format!("inserted a row into {}", table.qualified()),
        );

        let stmt = Statement::select_all(&table.keyspace, &table.table);
        return submit_into_contents(state, Subject::Table(table), stmt);
    }

    let cells = state.contents.loaded_mut().map_or(0, |page| {
        let cells = page.edits().cells();
        page.commit_edits();
        cells
    });

    // The inspector may still be up over the row that was just written, and
    // its copy of the page carries the marks it drew them with.
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

/// Every key the quit warning claims. `y` quits and discards, `n` and `Esc`
/// stay, and everything else — a second `q` included — is inert: the question
/// has to be answered rather than pressed past (design §6).
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

/// What is uncommitted, as `(cells, rows)`. `(0, 0)` with nothing loaded.
pub(super) fn uncommitted(state: &State) -> (usize, usize) {
    match state.contents.state() {
        ContentsState::Loaded { page, .. } => (page.edits().cells(), page.edits().rows()),
        _ => (0, 0),
    }
}
