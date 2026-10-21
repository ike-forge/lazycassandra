//! The one place state changes: synchronous, pure and total. Effects are
//! described here and performed in `lc-app` (architecture.md §3).

use crate::clock::Stamp;
use crate::commit::Discard;
use crate::config::{Config, ConnectionConfig, ConnectionName, Diagnostic, DriverConfig, PageSize};
use crate::connection::ConnectionState;
use crate::contents::TableRef;
use crate::cql;
use crate::export::{Destination, Exported, Format};
use crate::feedback::{Flash, Toast};
use crate::geometry::Geometry;
use crate::health::Health;
use crate::log::{Entry, Outcome, Subject as LogSubject, Trace};
use crate::model::{
    ConnId, DbError, JobId, KeyPress, Page, PagingState, QueryId, Row, Schema, Statement,
};
use crate::overlay::{DiagnosticsPanel, Message, Overlay};
use crate::pointer::Pointer;
use crate::ports::ExportError;
use crate::posture::Posture;
use crate::search::Search;
use crate::state::State;
use crate::style::Severity;
use crate::theme::ThemeChoice;
use crate::tree::Node;
use std::num::NonZeroUsize;
use std::time::Duration;

mod commit;
mod connections;
mod drop;
mod edit;
mod export;
mod health;
mod history;
mod insert;
mod inspect;
mod keys;
mod motion;
mod navigation;
mod palette;
mod pointer;
mod run;
mod search;
mod themes;
mod visual;
mod yank;

use commit::uncommitted;
use connections::{
    connect, connect_failed, connected, default_connection_index, schema_failed, schema_loaded,
};
use edit::{field_key, start_edit};
use export::{cancel_pending_export, export_progress, finish_export};
use health::{health_entry, health_loaded};
use insert::open_insert_commit;
use keys::{Decision, edit_line, escape, handle_key, open_filter, overlay_key};
use motion::{
    Motion, Side, clamp_lists, cursor_indices, follow_grid_columns, follow_moved_cursors, measured,
    panel_scroll, row_count, scroll, scroll_len, step_column,
};
use navigation::{Step, focus_by_number, go_left, go_right, show_tab, walk_landing, walk_tab};
use pointer::handle_pointer;
pub use run::submit;
use run::{
    activate, cancel, every_page, next_page, page_ready, query_failed, read, run,
    submit_into_contents,
};
use search::restore_origin;
use themes::{restore_theme, theme_save_failed};
use visual::{axes, clear_selection, collapse_selection, spanned, stop_extending, toggle_visual};
use yank::copied;

pub(crate) use connections::connection_names;
pub(crate) use drop::drop_target;
pub(crate) use edit::{editable_here, has_uncommitted};
pub(crate) use health::health_target;
pub(crate) use history::history_labels;
pub(crate) use insert::insert_form;
pub(crate) use inspect::inspection;
pub(crate) use themes::theme_labels;

/// Async results re-enter through the same channel as a keystroke.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Input(KeyPress),
    Pointer(Pointer),
    /// Only triggers a repaint; the renderer reads the size from the frame.
    Resized,
    /// The renderer's one report back into the domain.
    Measured(Geometry),
    /// The domain reads no clock; time arrives here.
    Tick(Stamp),
    Connected {
        conn: ConnId,
        posture: Posture,
        /// Measured by the adapter around the call.
        took: Duration,
    },
    ConnectFailed {
        conn: ConnId,
        error: DbError,
        took: Duration,
    },
    SchemaFailed {
        conn: ConnId,
        error: DbError,
        took: Duration,
    },
    SchemaLoaded {
        conn: ConnId,
        schema: Box<Schema>,
        took: Duration,
    },
    PageReady {
        query: QueryId,
        page: Box<Page>,
        took: Duration,
    },
    Failed {
        query: QueryId,
        error: DbError,
        took: Duration,
    },
    EditorClosed {
        buffer: String,
    },
    EditorFailed {
        reason: String,
    },
    ThemeSaveFailed {
        reason: String,
    },
    /// The next save writes the whole list, so a lost one is repaired.
    HistorySaveFailed {
        reason: String,
    },
    HealthLoaded {
        request: QueryId,
        health: Result<Health, DbError>,
        took: Duration,
    },
    /// A running total, not a delta.
    ExportProgress {
        job: JobId,
        rows: usize,
    },
    ExportFinished {
        job: JobId,
        result: Result<Exported, ExportError>,
        took: Duration,
    },
    Quit,
}

/// Performed by `lc-app`'s effect runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Carries the connection: the adapter has no table to look an id up in.
    Connect {
        conn: ConnId,
        target: Box<ConnectionConfig>,
        driver: DriverConfig,
    },
    LoadSchema(ConnId),
    Execute {
        conn: ConnId,
        query: QueryId,
        stmt: Box<Statement>,
    },
    ExecutePaged {
        conn: ConnId,
        query: QueryId,
        stmt: Box<Statement>,
        page_size: PageSize,
        cursor: Option<PagingState>,
    },
    Cancel(QueryId),
    LoadHealth {
        conn: ConnId,
        request: QueryId,
        table: TableRef,
    },
    RecordHistory(Box<Statement>),
    /// The whole list: a save replaces, so a lost one is repaired by the next.
    SaveHistory {
        connection: ConnectionName,
        entries: Vec<String>,
    },
    /// Run on the main loop rather than as a task: the editor needs the
    /// terminal.
    OpenEditor {
        buffer: String,
    },
    /// Write-only: OSC 52 gives no answer.
    Yank(String),
    /// Silent on success: the theme on screen is the confirmation.
    SaveTheme(ThemeChoice),
    /// The paging loop runs in `lc-app`.
    Export {
        conn: ConnId,
        job: JobId,
        source: Source,
        destination: Destination,
        format: Format,
        /// Export pages by the driver's size, not the grid's.
        page_size: PageSize,
    },
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// `limit` is counted here rather than spliced into the CQL: a statement's
    /// own `LIMIT` is the user's.
    Whole {
        statement: Box<Statement>,
        limit: Option<NonZeroUsize>,
    },
    Fetched {
        columns: Vec<String>,
        rows: Vec<Row>,
    },
}

/// Nothing connects unless the config names a default. `now` is passed in
/// because the domain reads no clock.
#[must_use]
pub fn init(config: Config, diagnostics: Vec<Diagnostic>, now: Stamp) -> (State, Vec<Effect>) {
    let mut state = State::default();
    state.now = now;
    state.theme = config.theme.clone();

    // Before the move below, so the trail keeps the order things happened in.
    for diagnostic in &diagnostics {
        state.log.push(Entry::new(
            now,
            sentence(format!(
                "{}: {}",
                crate::view::location(diagnostic),
                diagnostic.message
            )),
            Outcome::Rejected {
                reason: "dropped from the configuration".to_owned(),
            },
        ));
    }

    state.overlay =
        (!diagnostics.is_empty()).then(|| Overlay::Diagnostics(DiagnosticsPanel::new(diagnostics)));
    state.config = config;
    state.connection = ConnectionState::Unchosen;
    state.dirty = true;

    // No picker yet: it is sized against a frame and none has been measured.
    // `offer_connections` opens it on the first measurement.
    let target = default_connection_index(&state.config)
        .and_then(|at| state.config.connections.get(at))
        .cloned();
    let effects = target.map_or_else(Vec::new, |target| connect(&mut state, target));

    (state, effects)
}

pub fn update(state: &mut State, msg: Msg) -> Vec<Effect> {
    let subject = described(state);
    let cursors = cursor_indices(state);
    let scrolls = (state.tables.scroll, state.contents.scroll);
    let column_scroll = state.contents.column_scroll;

    let effects = apply(state, msg);

    // A new subject opens the Ddl panel at its top, whichever path moved the
    // tree selection.
    if described(state) != subject {
        state.ddl.cursor.to_start();
        state.ddl.scroll.to_start();
        state.ddl.selection.clear();
    }

    clamp_lists(state);
    // Before the viewport follows, so a page that lost columns strands neither.
    state
        .contents
        .column
        .clamp_to(state.contents.column_count());
    follow_moved_cursors(state, cursors);
    follow_grid_columns(state);

    // `Msg::Measured` marks nothing dirty by itself, so a clamp it provoked
    // would otherwise not be drawn until the next unrelated keypress.
    if (state.tables.scroll, state.contents.scroll) != scrolls
        || state.contents.column_scroll != column_scroll
    {
        state.mark_dirty();
    }

    effects
}

/// Owned, so it survives the mutation it is compared across. Unfolding is
/// excluded: it must not scroll the panel.
#[derive(PartialEq, Eq)]
enum Described {
    Keyspace(String),
    Table { keyspace: String, table: String },
}

fn described(state: &State) -> Option<Described> {
    let schema = state.schema()?;

    Some(match state.tables.selected(schema)? {
        Node::Keyspace { keyspace, .. } => Described::Keyspace(keyspace.name.clone()),
        Node::Table { keyspace, table } => Described::Table {
            keyspace: keyspace.name.clone(),
            table: table.name.clone(),
        },
    })
}

fn apply(state: &mut State, msg: Msg) -> Vec<Effect> {
    match msg {
        Msg::Tick(now) => tick(state, now),

        // Left standing, the flash would contend with a fresh `v` for the same
        // band.
        Msg::Input(press) => {
            state.flash = None;
            state.held = None;
            handle_key(state, press)
        }

        Msg::Pointer(pointer) => {
            state.flash = None;
            handle_pointer(state, pointer)
        }

        Msg::Resized => {
            state.mark_dirty();
            Vec::new()
        }

        Msg::Measured(geometry) => measured(state, geometry),

        Msg::Connected {
            conn,
            posture,
            took,
        } => connected(state, conn, posture, took),

        Msg::ConnectFailed { conn, error, .. } => connect_failed(state, conn, error),

        Msg::SchemaFailed { conn, error, .. } => schema_failed(state, conn, &error),

        Msg::SchemaLoaded { conn, schema, took } => schema_loaded(state, conn, *schema, took),

        // A result no longer in flight belongs to a cancelled query.
        Msg::PageReady { query, page, took } => page_ready(state, query, *page, took),

        // No overlay: a failed browse disables one panel, not the application.
        Msg::Failed { query, error, .. } => query_failed(state, query, &error),

        Msg::EditorClosed { buffer } => editor_closed(state, &buffer),

        Msg::EditorFailed { reason } => editor_failed(state, reason),

        Msg::ThemeSaveFailed { reason } => theme_save_failed(state, reason),

        Msg::HistorySaveFailed { reason } => {
            logged(
                state,
                sentence("Saving the history"),
                Outcome::Failed { detail: reason },
            );
            state.notify(Severity::Warning, "history not saved, see the logs");
            Vec::new()
        }

        Msg::HealthLoaded {
            request,
            health,
            took,
        } => health_loaded(state, request, health, took),

        Msg::ExportProgress { job, rows } => export_progress(state, job, rows),

        Msg::ExportFinished { job, result, took } => finish_export(state, job, result, took),

        Msg::Quit => quit(state),
    }
}

/// Repaints only on change: a tick arrives 25 times a second.
fn tick(state: &mut State, now: Stamp) -> Vec<Effect> {
    state.now = now;

    let working = state.is_working();
    if state.loader.advance(working) {
        state.mark_dirty();
    }

    if state.flash.as_mut().is_some_and(Flash::spend) {
        state.flash = None;
        state.mark_dirty();
    }

    if state.toast.as_mut().is_some_and(Toast::spend) {
        state.toast = None;
        state.mark_dirty();
    }

    Vec::new()
}

fn editor_closed(state: &mut State, buffer: &str) -> Vec<Effect> {
    state.queries.replace(buffer);
    state.mark_dirty();
    Vec::new()
}

fn editor_failed(state: &mut State, reason: String) -> Vec<Effect> {
    logged(
        state,
        sentence("Opening the external editor"),
        Outcome::Failed {
            detail: reason.clone(),
        },
    );
    raise(
        state,
        Message {
            title: "Editor".to_owned(),
            message: reason,
            severity: Severity::Danger,
        },
    );
    state.mark_dirty();
    Vec::new()
}

/// The guard lives here so every route to quit passes it.
fn quit(state: &mut State) -> Vec<Effect> {
    let (cells, rows) = uncommitted(state);

    if cells > 0 {
        state.overlay = Some(Overlay::Discard(Discard::of(cells, rows)));
        state.mark_dirty();
        return Vec::new();
    }

    quit_now(state)
}

fn quit_now(state: &mut State) -> Vec<Effect> {
    state.should_quit = true;
    let in_flight: Vec<QueryId> = state.in_flight.keys().copied().collect();
    for query in &in_flight {
        state.log.complete(Trace::Query(*query), Outcome::Cancelled);
    }

    in_flight
        .into_iter()
        .map(Effect::Cancel)
        .chain(std::iter::once(Effect::Quit))
        .collect()
}

fn logged(state: &mut State, subject: LogSubject, outcome: Outcome) {
    state.log.push(Entry::new(state.now, subject, outcome));
}

fn sentence(text: impl Into<String>) -> LogSubject {
    LogSubject::Sentence(text.into())
}

/// Token-based: whitespace collapses only between tokens, and comments drop.
fn statement_subject(stmt: &Statement) -> LogSubject {
    LogSubject::Statement(cql::issued(&stmt.cql))
}

/// Unwinds the overlay first: replacing a live prompt or theme picker outright
/// strands what it was previewing.
fn raise(state: &mut State, message: Message) {
    unwind_overlay(state);
    state.overlay = Some(Overlay::Message(message));
}

/// No way past it: the posture belongs to the connection.
fn refuse_read_only(state: &mut State, what: &str) {
    raise(
        state,
        Message {
            title: "Read-only connection".to_owned(),
            message: format!(
                "This connection is open read-only, so {what} is refused. \
                 Give it `posture = \"writable\"` in your config and reconnect \
                 to run it."
            ),
            severity: Severity::Danger,
        },
    );
    state.mark_dirty();
}

/// Restores what the overlay was previewing and completes what it left pending,
/// so a replacement strands neither.
fn unwind_overlay(state: &mut State) {
    if let Some(Overlay::Search(prompt)) = &state.overlay {
        let (target, previous, origin) =
            (prompt.target(), prompt.previous().cloned(), prompt.origin());
        state.apply_search(target, previous);
        restore_origin(state, target, origin);
    }
    cancel_pending_export(state);
    restore_theme(state);
}

/// Indices into the unfiltered list.
pub(crate) fn picker_matches(labels: &[String], search: Option<&Search>) -> Vec<usize> {
    labels
        .iter()
        .enumerate()
        .filter(|(_, label)| search.is_none_or(|search| search.matches(label)))
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests;
