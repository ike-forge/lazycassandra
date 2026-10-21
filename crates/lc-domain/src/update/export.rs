//! The export form, its confirmation, and the running job.

use crate::connection::ConnectionState;
use crate::contents::{ContentsState, Subject};
use crate::export::{
    Button, Confirm, Destination, ExportForm, Exported, Fetched, Pending, Scope, Stop,
};
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
    Effect, Source, copied, edit_line, logged, raise, sentence, statement_subject, unwind_overlay,
};

enum Answer {
    Nothing,
    Close,
    Run(Button),
}

/// Every path that drops or replaces an export confirmation must call this: an
/// orphaned pending entry keeps `State::is_working` true and the app never
/// idles.
pub(super) fn cancel_pending_export(state: &mut State) {
    let job = match &state.overlay {
        Some(Overlay::Export(form)) => form.pending().map(Pending::job),
        _ => None,
    };
    if let Some(job) = job {
        state.log.complete(Trace::Export(job), Outcome::Cancelled);
    }
}

/// Refused where the frame cannot draw the form: an undrawn form would still
/// claim every key.
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

pub(super) fn export_key(state: &mut State, key: Key) -> Vec<Effect> {
    let confirming =
        matches!(&state.overlay, Some(Overlay::Export(form)) if form.confirm().is_some());
    if confirming {
        return confirm_key(state, key);
    }

    let answer = {
        let Some(Overlay::Export(form)) = &mut state.overlay else {
            return Vec::new();
        };

        match key {
            Key::Escape => Answer::Close,
            Key::Tab => {
                form.tab();
                Answer::Nothing
            }
            Key::BackTab => {
                form.back_tab();
                Answer::Nothing
            }
            Key::Char(' ') if !form.stop().takes_text() => {
                form.activate().map_or(Answer::Nothing, Answer::Run)
            }
            Key::Enter => match form.stop() {
                Stop::Button(button) => Answer::Run(button),
                Stop::Path | Stop::Format | Stop::Scope | Stop::Rows => Answer::Nothing,
            },
            Key::Char('j') | Key::Down if !form.stop().takes_text() => {
                form.down();
                Answer::Nothing
            }
            Key::Char('k') | Key::Up if !form.stop().takes_text() => {
                form.up();
                Answer::Nothing
            }
            Key::Char(character) if form.stop() == Stop::Rows && !character.is_ascii_digit() => {
                Answer::Nothing
            }
            _ if form.stop() == Stop::Rows => {
                edit_line(form.rows_mut(), key);
                Answer::Nothing
            }
            _ if form.stop().takes_text() => {
                edit_line(form.path_mut(), key);
                Answer::Nothing
            }
            _ => Answer::Nothing,
        }
    };

    state.dirty = true;

    match answer {
        Answer::Nothing => Vec::new(),
        Answer::Close => {
            state.overlay = None;
            Vec::new()
        }
        Answer::Run(button) => start_export(state, button),
    }
}

/// Re-emits the held source rather than re-reading the panel, which may have
/// moved on by the time `Exists` returns.
pub(super) fn start_export(state: &mut State, button: Button) -> Vec<Effect> {
    let Some(Overlay::Export(form)) = state.overlay.take() else {
        return Vec::new();
    };
    let ConnectionState::Connected(connection) = &state.connection else {
        state.overlay = Some(Overlay::Export(form));
        return Vec::new();
    };

    let overwrite = form.confirm() == Some(Confirm::Overwrite);
    // A retry reuses the refused attempt's `JobId`, and so its pending trail
    // entry.
    let retry = form.pending().map(Pending::job);
    let attempt = match form.pending() {
        Some(pending) => Some(pending.source().clone()),
        None => source(state, &form),
    };
    let Some(source) = attempt else {
        state.overlay = Some(Overlay::Export(form));
        return Vec::new();
    };
    let destination = match button {
        Button::File => Destination::File {
            path: PathBuf::from(form.path().text()),
            overwrite,
        },
        Button::Clipboard => Destination::Clipboard(state.config.export.clipboard_mb),
    };
    let (conn, job, format) = (
        connection.id,
        retry.unwrap_or_else(JobId::next),
        form.format(),
    );

    // A retry's entry is already pending; a second would strand one.
    if retry.is_none() {
        // A `Fetched` export issued no statement, so its subject records the
        // act.
        let subject = match &source {
            Source::Whole { statement, .. } => statement_subject(statement),
            Source::Fetched { .. } => sentence(match destination {
                Destination::File { .. } => "Exporting the fetched rows",
                Destination::Clipboard(_) => "Copying the fetched rows",
            }),
        };
        logged(
            state,
            subject,
            Outcome::Pending {
                trace: Trace::Export(job),
            },
        );
    }

    // Cloned: the job holds it against an `Exists` that has not come back yet.
    state.notify(
        Severity::Info,
        match &destination {
            Destination::File { path, .. } => format!("exporting to {}", path.display()),
            Destination::Clipboard(_) => "copying to the clipboard".to_owned(),
        },
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
        page_size: state.config.driver.page_size,
    }]
}

/// `None` is unreachable today but must not panic if that changes.
pub(super) fn source(state: &State, form: &ExportForm) -> Option<Source> {
    let ContentsState::Loaded { subject, page, .. } = state.contents.state() else {
        return None;
    };

    match form.scope() {
        // Only our limit is dropped; a typed statement's own is the user's.
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
        // What the grid shows, filter and sort included.
        Scope::Fetched => Some(Source::Fetched {
            columns: page.page().columns.clone(),
            rows: page.drawn_rows(),
        }),
    }
}

pub(super) fn confirm_key(state: &mut State, key: Key) -> Vec<Effect> {
    match key {
        Key::Char(' ') | Key::Enter => start_export(state, Button::File),
        Key::Escape => {
            // Before `dismiss()` drops the source: nothing else would complete
            // this trace.
            cancel_pending_export(state);
            if let Some(Overlay::Export(form)) = &mut state.overlay {
                form.dismiss();
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Dropped for a job no longer tracked.
pub(super) fn export_progress(state: &mut State, job: JobId, rows: usize) -> Vec<Effect> {
    if let Some(running) = &mut state.export {
        running.progressed(job, rows);
        state.mark_dirty();
    }
    Vec::new()
}

/// `Exists` reopens the form with its confirmation; the trail entry stays
/// pending until a retry or `Esc` closes it.
pub(super) fn finish_export(
    state: &mut State,
    job: JobId,
    result: Result<Exported, ExportError>,
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
        Ok(exported) => {
            let (rows, to) = (exported.rows(), finished.destination().describe());
            state.log.complete(
                Trace::Export(job),
                Outcome::Succeeded {
                    detail: format!("{rows} rows to {to}"),
                    took,
                },
            );

            return match exported {
                Exported::Written(_) => {
                    state.notify(Severity::Info, format!("exported {rows} rows to {to}"));
                    Vec::new()
                }
                Exported::Copied { text, .. } => {
                    state.notify(Severity::Info, format!("copied {rows} rows to {to}"));
                    copied(Some(text))
                }
            };
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
                    title: match finished.destination() {
                        Destination::File { .. } => "Export failed",
                        Destination::Clipboard(_) => "Copy failed",
                    }
                    .to_owned(),
                    message: error.to_string(),
                    severity: Severity::Danger,
                },
            );
        }
    }

    Vec::new()
}
