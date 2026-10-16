//! The export form, its confirmation, and the running job
//! (docs/superpowers/specs/2026-08-14-export-design.md).

use crate::connection::ConnectionState;
use crate::contents::{ContentsState, Subject};
use crate::export::{Confirm, ExportForm, Fetched, Pending, Scope, Stop};
use crate::jobs::ExportJob;
use crate::log::{Outcome, Trace};
use crate::model::{JobId, Key, Statement};
use crate::overlay::{Message, Overlay};
use crate::ports::ExportError;
use crate::state::State;
use crate::style::Severity;
use std::path::PathBuf;
use std::time::Duration;

use super::{
    Decision, Effect, Source, edit_line, logged, raise, sentence, statement_subject, unwind_overlay,
};

/// Completes the current overlay's pending export trail entry as
/// `Cancelled`, if it is an `Overlay::Export` confirmation with one.
/// Otherwise a no-op.
///
/// The one place that reads a confirmation's `JobId` before the overlay
/// carrying it is dropped or replaced, so a fourth site cannot silently
/// leave the trace pending the way three already did (final review, I4
/// follow-up). An orphaned pending entry is not merely a stale row any
/// more: `is_working_in(Panel::Queries)` (`state.rs`) is now true for any
/// pending entry, so a trace nothing will ever complete keeps
/// `State::is_working` true forever, and the app never goes idle again
/// (`tick` keeps advancing the loader and marking every frame dirty).
///
/// Called from the same four places dropping or replacing the overlay
/// while it might be this confirmation: [`escape`] (the click-dismiss
/// path — keyboard `Esc` while the form is up is claimed by
/// [`export_key`] first and never reaches `escape`), the resize-shrink
/// close in [`measured`], [`confirm_key`]'s own `Esc` arm, and here in
/// [`unwind_overlay`], which covers every message-driven replacement
/// ([`raise`]'s callers) including the one that reaches it: `editor_failed`,
/// if an `Exists` confirmation lands on the trail while the external
/// editor is suspending the terminal and `Msg::EditorFailed` is what
/// resumes it.
pub(super) fn cancel_pending_export(state: &mut State) {
    let job = match &state.overlay {
        Some(Overlay::Export(form)) => form.pending().map(Pending::job),
        _ => None,
    };
    if let Some(job) = job {
        state.log.complete(Trace::Export(job), Outcome::Cancelled);
    }
}

/// Refused over a grid with no rows, and while an export is already running:
/// one at a time (design §5).
///
/// Refused too where the frame cannot draw the form, for the reason
/// [`open_palette`] gives: the form claims every key while it is open, so one
/// nothing draws is a mode with nothing on screen saying so and only `Esc` out
/// of it. Gated on the form's own measurement rather than on the palette's,
/// which is the weaker condition — a frame that draws the palette need not
/// draw a 56×11 block.
pub(super) fn open_export(state: &mut State) {
    if state.export.is_some() {
        state.notify(Severity::Info, "an export is already running");
        return;
    }
    if !state.geometry.export_fits() {
        state.notify(Severity::Info, "no room to draw the export form");
        return;
    }

    let ContentsState::Loaded {
        subject,
        page,
        paging,
    } = state.contents.state()
    else {
        state.notify(Severity::Info, "nothing to export");
        return;
    };

    let rows = page.len();
    if rows == 0 {
        state.notify(Severity::Info, "nothing to export");
        return;
    }

    let form = ExportForm::opened(
        subject.clone(),
        Fetched::of_drawn(paging, page.fetched(), rows),
        &state.cwd,
    );
    state.overlay = Some(Overlay::Export(form));
    state.dirty = true;
}

/// Every key the export form claims.
///
/// A confirmation in front of the form is resolved first, through
/// [`confirm_key`].
pub(super) fn export_key(state: &mut State, key: Key) -> Vec<Effect> {
    let confirming =
        matches!(&state.overlay, Some(Overlay::Export(form)) if form.confirm().is_some());
    if confirming {
        return confirm_key(state, key);
    }

    let decision = {
        let Some(Overlay::Export(form)) = &mut state.overlay else {
            return Vec::new();
        };

        match key {
            Key::Escape => Decision::Close,
            Key::Tab => {
                form.tab();
                Decision::Nothing
            }
            Key::BackTab => {
                form.back_tab();
                Decision::Nothing
            }
            Key::Char(' ') if !form.stop().takes_text() => {
                if form.activate() {
                    Decision::Run
                } else {
                    Decision::Nothing
                }
            }
            Key::Enter if form.stop() == Stop::Export => Decision::Run,
            Key::Char('j') | Key::Down if !form.stop().takes_text() => {
                form.down();
                Decision::Nothing
            }
            Key::Char('k') | Key::Up if !form.stop().takes_text() => {
                form.up();
                Decision::Nothing
            }
            // The count is a number: anything else typed there is dropped.
            Key::Char(character) if form.stop() == Stop::Rows && !character.is_ascii_digit() => {
                Decision::Nothing
            }
            _ if form.stop() == Stop::Rows => {
                edit_line(form.rows_mut(), key);
                Decision::Nothing
            }
            _ if form.stop().takes_text() => {
                edit_line(form.path_mut(), key);
                Decision::Nothing
            }
            _ => Decision::Nothing,
        }
    };

    state.dirty = true;

    match decision {
        Decision::Nothing => Vec::new(),
        Decision::Close => {
            state.overlay = None;
            Vec::new()
        }
        Decision::Run => start_export(state),
    }
}

/// Turns the form into the one effect that does the work.
///
/// A confirmation re-emits the source it is holding rather than reading the
/// panel again: the panel is live for the whole of an export, and by the time
/// `Exists` comes back the user may be standing on another table (final review
/// I3).
pub(super) fn start_export(state: &mut State) -> Vec<Effect> {
    let Some(Overlay::Export(form)) = state.overlay.take() else {
        return Vec::new();
    };
    let ConnectionState::Connected(connection) = &state.connection else {
        state.overlay = Some(Overlay::Export(form));
        return Vec::new();
    };

    let overwrite = form.confirm() == Some(Confirm::Overwrite);
    // A retry reuses the refused attempt's own `JobId` rather than minting a
    // fresh one: it is the same export continuing past the overwrite
    // question, not a second one, so it reuses that attempt's still-pending
    // trail entry instead of orphaning it (final review I4).
    let retry = form.pending().map(Pending::job);
    let attempt = match form.pending() {
        Some(pending) => Some(pending.source().clone()),
        None => source(state, &form),
    };
    let Some(source) = attempt else {
        state.overlay = Some(Overlay::Export(form));
        return Vec::new();
    };
    let destination = PathBuf::from(form.path().text());
    let (conn, job, format) = (
        connection.id,
        retry.unwrap_or_else(JobId::next),
        form.format(),
    );

    // Only a fresh attempt logs an entry. A retry's is already pending under
    // the id just reused, and logging a second one for it would give one
    // `Trace` two pending entries — `Log::complete` would rewrite whichever
    // it found first and strand the other exactly as before.
    if retry.is_none() {
        // Both scopes are something the session ran and both get an entry
        // (design §8's `ExportFinished(Err)` row names no scope qualifier). A
        // `Whole` export logs the statement it paged; a `Fetched` export writes
        // rows already in memory and issues no statement (export design §3), so
        // its subject records the act rather than quoting a query that never ran.
        let subject = match &source {
            Source::Whole { statement, .. } => statement_subject(statement),
            Source::Fetched { .. } => sentence("Exporting the fetched rows"),
        };
        logged(
            state,
            subject,
            Outcome::Pending {
                trace: Trace::Export(job),
            },
        );
    }

    // Cloned rather than borrowed: the effect carries it out of the domain and
    // the job holds it against an `Exists` that has not come back yet.
    state.notify(
        Severity::Info,
        format!("exporting to {}", destination.display()),
    );
    state.export = Some(ExportJob::started(
        job,
        destination.clone(),
        form,
        source.clone(),
    ));
    state.dirty = true;

    vec![Effect::Export {
        conn,
        job,
        source,
        destination,
        format,
        overwrite,
        page_size: state.config.driver.page_size,
    }]
}

/// What the chosen scope reads.
///
/// `None` when the panel is no longer showing a page — which the form's own
/// key claim makes impossible today, and which must not become a panic if it
/// ever stops being.
pub(super) fn source(state: &State, form: &ExportForm) -> Option<Source> {
    let ContentsState::Loaded { subject, page, .. } = state.contents.state() else {
        return None;
    };

    match form.scope() {
        // Ours is the limit that is dropped; a typed statement's own is the
        // user's and is honoured verbatim (design §3).
        Scope::Whole | Scope::First => {
            let statement = Box::new(match subject {
                Subject::Table(table) => Statement::select_all(&table.keyspace, &table.table),
                Subject::Typed(cql) => Statement::typed(cql),
            });
            let limit = match form.scope() {
                Scope::First => Some(form.row_limit()?),
                _ => None,
            };

            Some(Source::Whole { statement, limit })
        }
        // What the grid is showing, arrangement and all: a form opened over
        // a filtered grid offering to write the rows behind the filter would
        // be exporting something the user cannot see
        // (grid sort and filter design §4).
        Scope::Fetched => Some(Source::Fetched {
            columns: page.page().columns.clone(),
            rows: page.drawn_rows(),
        }),
    }
}

/// The overwrite confirmation's keys: `Space` or `Enter` confirms, `Esc` goes
/// back to the form (design §7).
pub(super) fn confirm_key(state: &mut State, key: Key) -> Vec<Effect> {
    match key {
        Key::Char(' ') | Key::Enter => start_export(state),
        Key::Escape => {
            // Read before `dismiss()` drops the source it was asking about:
            // nothing else will ever complete this trace once the question
            // is gone (final review I4, and its I4-follow-up choke point,
            // `cancel_pending_export`).
            cancel_pending_export(state);
            if let Some(Overlay::Export(form)) = &mut state.overlay {
                form.dismiss();
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// A running total for `job`, dropped if it belongs to a job no longer
/// tracked — cancelled, or already finished.
pub(super) fn export_progress(state: &mut State, job: JobId, rows: usize) -> Vec<Effect> {
    if let Some(running) = &mut state.export {
        running.progressed(job, rows);
        state.mark_dirty();
    }
    Vec::new()
}

/// An export ended. `Exists` is the one outcome that is a question rather than
/// a result: it reopens the form with its confirmation up (design §7), and
/// the trail's entry stays pending — closed by `start_export`'s retry, which
/// reuses this `job` rather than minting a fresh one, or by `confirm_key`'s
/// `Esc` arm if the question is abandoned instead (final review I4).
pub(super) fn finish_export(
    state: &mut State,
    job: JobId,
    result: Result<usize, ExportError>,
    took: Duration,
) -> Vec<Effect> {
    if state.export.as_ref().map(ExportJob::id) != Some(job) {
        return Vec::new();
    }

    let Some(finished) = state.export.take() else {
        return Vec::new();
    };
    state.dirty = true;

    match result {
        Ok(rows) => {
            state.log.complete(
                Trace::Export(job),
                Outcome::Succeeded {
                    detail: format!("{rows} rows to {}", finished.destination().display()),
                    took,
                },
            );
            state.notify(
                Severity::Info,
                format!(
                    "exported {rows} rows to {}",
                    finished.destination().display()
                ),
            );
        }
        Err(ExportError::Exists) => {
            let (mut form, source) = finished.into_parts();
            form.ask(Pending::Overwrite(Box::new(source), job));
            unwind_overlay(state);
            state.overlay = Some(Overlay::Export(form));
        }
        Err(error) => {
            state.log.complete(
                Trace::Export(job),
                Outcome::Failed {
                    detail: error.to_string(),
                },
            );
            raise(
                state,
                Message {
                    title: "Export failed".to_owned(),
                    message: error.to_string(),
                    severity: Severity::Danger,
                },
            );
        }
    }

    Vec::new()
}
