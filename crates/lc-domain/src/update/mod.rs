//! The one place state changes.
//!
//! `update` is synchronous, pure, and total. It mutates state and *describes*
//! what should happen next; it cannot open a socket because it has nothing to
//! open one with (architecture.md §3).

use crate::clock::Stamp;
use crate::commit::Discard;
use crate::config::{Config, ConnectionConfig, ConnectionName, Diagnostic, DriverConfig, PageSize};
use crate::connection::ConnectionState;
use crate::contents::TableRef;
use crate::cql;
use crate::export::Format;
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
use std::path::PathBuf;
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
    panel_scroll, row_count, scroll, scroll_len, step_column, tree_rows,
};
use navigation::{Step, focus_by_number, go_left, go_right, show_tab, walk_landing, walk_tab};
use pointer::handle_pointer;
pub use run::submit;
use run::{
    activate, every_page, next_page, page_ready, query_failed, read, run, submit_into_contents,
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

/// Everything that can happen. Async results re-enter here through the same
/// channel as a keystroke.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Input(KeyPress),
    /// A pointer gesture, resolved by `lc-inbound` against the frame it
    /// happened in (docs/superpowers/specs/2026-08-09-mouse-input-design.md §5).
    Pointer(Pointer),
    /// The terminal was resized. Carries no geometry — the renderer reads the
    /// real size from `frame.area()` — it exists only to trigger a repaint.
    Resized,
    /// What the frame just drawn turned out to be. The renderer's one report
    /// back into the domain (design §3).
    Measured(Geometry),
    /// Elapsed time, and the wall-clock the domain is not allowed to read
    /// (query log design §4).
    Tick(Stamp),
    Connected {
        conn: ConnId,
        posture: Posture,
        /// How long the call took, measured by the adapter around the real
        /// call. The domain reads no clock (query log design §4).
        took: Duration,
    },
    ConnectFailed {
        conn: ConnId,
        error: DbError,
        /// As [`Msg::Connected::took`].
        took: Duration,
    },
    SchemaFailed {
        conn: ConnId,
        error: DbError,
        /// As [`Msg::Connected::took`].
        took: Duration,
    },
    SchemaLoaded {
        conn: ConnId,
        schema: Box<Schema>,
        /// As [`Msg::Connected::took`].
        took: Duration,
    },
    PageReady {
        query: QueryId,
        page: Box<Page>,
        /// As [`Msg::Connected::took`].
        took: Duration,
    },
    Failed {
        query: QueryId,
        error: DbError,
        /// As [`Msg::Connected::took`].
        took: Duration,
    },
    /// The external editor exited cleanly; this is what it saved
    /// (docs/superpowers/specs/2026-08-12-external-editor-design.md §3).
    EditorClosed {
        buffer: String,
    },
    /// It could not be run, or it exited non-zero. The buffer is untouched.
    EditorFailed {
        reason: String,
    },
    /// The theme was confirmed and could not be written down. Carries the
    /// adapter's own words: which file, and why (theme persistence design §6).
    ThemeSaveFailed {
        reason: String,
    },
    /// A history write failed. The in-memory list is untouched and the next
    /// run retries with the whole of it (query history design §5).
    HistorySaveFailed {
        reason: String,
    },
    /// What the nodes said about the table the health block is open on.
    HealthLoaded {
        request: QueryId,
        health: Result<Health, DbError>,
        /// As [`Msg::Connected::took`].
        took: Duration,
    },
    /// Rows written so far by `job`, a running total.
    ExportProgress {
        job: JobId,
        rows: usize,
    },
    ExportFinished {
        job: JobId,
        result: Result<usize, ExportError>,
        /// As [`Msg::Connected::took`].
        took: Duration,
    },
    Quit,
}

/// What `update` decided should happen. Interpreted by the effect runtime in
/// `lc-app`, never here.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Open a session to `target`. Carries the connection rather than only an
    /// id: the adapter has no table to look one up in, and a picker means the
    /// address is chosen at runtime rather than fixed when the adapter is
    /// built (connection picker design §6).
    ///
    /// Boxed for `clippy::large_enum_variant`: a [`ConnectionConfig`] holds
    /// four strings and a host list.
    Connect {
        conn: ConnId,
        target: Box<ConnectionConfig>,
        driver: DriverConfig,
    },
    LoadSchema(ConnId),
    /// A mutation or DDL, whose result is not paged.
    Execute {
        conn: ConnId,
        query: QueryId,
        stmt: Box<Statement>,
    },
    /// One page of a read into Table contents, from `cursor` when it
    /// continues one (grid paging design §1).
    ExecutePaged {
        conn: ConnId,
        query: QueryId,
        stmt: Box<Statement>,
        page_size: PageSize,
        cursor: Option<PagingState>,
    },
    Cancel(QueryId),
    /// Read every node's view of `table`, answered by
    /// [`Msg::HealthLoaded`] under `request`.
    LoadHealth {
        conn: ConnId,
        request: QueryId,
        table: TableRef,
    },
    RecordHistory(Box<Statement>),
    /// Write `connection`'s history, newest first, to the store. Carries the
    /// whole list rather than the new entry: a save replaces, so a lost one
    /// is repaired by the next (query history design §5).
    SaveHistory {
        connection: ConnectionName,
        entries: Vec<String>,
    },
    /// Hand `buffer` to the user's editor and report what comes back. The one
    /// effect `lc-app` runs on the main loop rather than as a task: the child
    /// needs the terminal, so nothing else may draw while it holds it
    /// (design §4).
    OpenEditor {
        buffer: String,
    },
    /// Put the yanked line on the system clipboard. Write-only: OSC 52 gives
    /// no answer, so there is no `Msg` back and nothing to report
    /// (docs/superpowers/specs/2026-08-13-yank-design.md §3, §4).
    Yank(String),
    /// Write the confirmed theme into the config file it is read from, so the
    /// next launch opens on it (theme persistence design §2). Silent on
    /// success: the theme on screen is the confirmation.
    SaveTheme(ThemeChoice),
    /// Write a result set to a file. The paging loop, if there is one, runs in
    /// `lc-app` (export design §5).
    Export {
        conn: ConnId,
        job: JobId,
        source: Source,
        destination: PathBuf,
        format: Format,
        overwrite: bool,
        /// `driver.page_size`: export streams, so it pages by the driver's
        /// size rather than the grid's (grid paging design §3).
        page_size: PageSize,
    },
    Quit,
}

/// Where an export's rows come from.
///
/// An enum and not a struct with two optional fields: a statement and a row
/// list at once, or neither, are both nonsense and neither can be written
/// down (design §5).
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Run this and page to the end. The columns arrive with the first page.
    ///
    /// `limit` stops it after that many rows, mid-page if need be: a typed
    /// statement's own `LIMIT` is the user's, so ours is counted here rather
    /// than spliced into the CQL.
    Whole {
        statement: Box<Statement>,
        limit: Option<NonZeroUsize>,
    },
    Fetched {
        columns: Vec<String>,
        rows: Vec<Row>,
    },
}

/// The state the application starts in, and what it must do first.
///
/// TEA's `init`: the startup connection is *described* here rather than
/// performed behind the domain's back, so the first frame can already say
/// "Connecting…" (architecture.md §3).
///
/// Nothing is connected to unless a config named it. `default_connection` is
/// the whole of that: it names one entry, so "exactly one connection is the
/// default" is a property of the type rather than a rule the merge has to
/// enforce, and a start with no default ends in the picker rather than at
/// whatever address happened to be compiled in
/// (connection picker design §2).
///
/// `now` is passed in for the reason `State::cwd` is: the domain cannot read
/// a clock and must not learn how (query log design §4.1).
#[must_use]
pub fn init(config: Config, diagnostics: Vec<Diagnostic>, now: Stamp) -> (State, Vec<Effect>) {
    let mut state = State::default();
    state.now = now;
    state.theme = config.theme.clone();

    // Read before the move into the overlay below, so the trail carries these
    // ahead of the connection entry — the order things happened in
    // (query log design §8).
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

    // The picker is not opened here: it is a block sized against a frame, and
    // at `init` there has been none to measure. `offer_connections` opens it
    // at the first measurement instead — and again after the diagnostics
    // modal is dismissed, which is what claims startup's first frame whenever
    // the config had anything wrong with it (connection picker design §4).
    let target = default_connection_index(&state.config)
        .and_then(|at| state.config.connections.get(at))
        .cloned();
    let effects = target.map_or_else(Vec::new, |target| connect(&mut state, target));

    (state, effects)
}

/// Applies a message and returns the effects it implies.
pub fn update(state: &mut State, msg: Msg) -> Vec<Effect> {
    let subject = described(state);
    let cursors = cursor_indices(state);
    let scrolls = (state.tables.scroll, state.contents.scroll);
    let column_scroll = state.contents.column_scroll;

    let effects = apply(state, msg);

    // The Ddl offset belongs to the node being described, so a new subject
    // opens at its top rather than halfway down the last one. Stated once here
    // rather than at each of the four paths that can move the tree selection.
    if described(state) != subject {
        state.ddl.cursor.to_start();
        state.ddl.scroll.to_start();
        state.ddl.selection.clear();
    }

    clamp_lists(state);
    // Before the viewport follows it, so a page that lost columns cannot
    // strand either.
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

/// What the Ddl panel is describing, in a form that survives the mutation it
/// is compared across — [`Node`] borrows the schema.
///
/// Unfolding is deliberately not part of the identity: it leaves the keyspace
/// selected and the panel on the same subject, so it must not scroll it.
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

        // The flash is a moment rather than a state, and the next input is the
        // end of it: left standing, it would contend with a fresh `v` for the
        // same band (visual mode design §6.1).
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

        // A result whose id is no longer in flight belongs to a cancelled
        // query, and applying it would resurrect a view the user left. The
        // panel applies the second gate: whether the result is *its* result.
        Msg::PageReady { query, page, took } => page_ready(state, query, *page, took),

        // No overlay, unlike `SchemaFailed`: a schema failure disables the
        // application, a failed browse disables one panel, and an overlay per
        // mistyped table is the warning noise vision.md §8 flags.
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

/// The two things elapsed time does to the frame: turn the loader, and burn
/// down a yank's afterglow.
///
/// Each repaints only when it changed something. A tick arrives 25 times a
/// second, and one that changes nothing must not cost a frame (visual polish
/// design §4.2).
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

/// `q`, from wherever it was pressed.
///
/// The guard is here rather than at each key that reaches it, so the palette's
/// quit row, the inspector's `q` and `Msg::Quit` are all caught by
/// construction and `Effect::Quit` still leaves from one place
/// (cell editing design §6).
fn quit(state: &mut State) -> Vec<Effect> {
    let (cells, rows) = uncommitted(state);

    if cells > 0 {
        state.overlay = Some(Overlay::Discard(Discard::of(cells, rows)));
        state.mark_dirty();
        return Vec::new();
    }

    quit_now(state)
}

/// Quitting past the guard: what `q` does with nothing outstanding, and what
/// answering the warning with `y` does.
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

/// Pushes an entry stamped from the clock the last tick left behind.
///
/// Every push happens in the arm that returns the effect the entry waits on,
/// or in the arm that handles the result — nothing outside `update` writes to
/// the trail (query log design §3.1).
fn logged(state: &mut State, subject: LogSubject, outcome: Outcome) {
    state.log.push(Entry::new(state.now, subject, outcome));
}

fn sentence(text: impl Into<String>) -> LogSubject {
    LogSubject::Sentence(text.into())
}

/// A statement as the trail draws it: one logical line, highlighted once.
///
/// `cql::issued` works from the lexer's tokens rather than the raw text, so
/// whitespace collapses only between tokens — never inside a string literal
/// — and a comment is dropped rather than swallowing the clause that
/// followed it. What ran is the statement, not its indentation (query log
/// design §3, §6).
fn statement_subject(stmt: &Statement) -> LogSubject {
    LogSubject::Statement(cql::issued(&stmt.cql))
}

/// The way a message overlay is installed. Every site goes through here —
/// `ConnectFailed`, `SchemaFailed`, and `run`'s destructive-statement refusal.
///
/// A direct `state.overlay = Some(Overlay::Message(_))` replaces a live search
/// prompt without restoring its `previous`, stranding the target under
/// whatever the half-typed query had applied and losing the only copy of what
/// was there before (panel search design §3.1). Cancelling the prompt first is
/// the same guarantee `Esc` gives, and it belongs to the installation rather
/// than to each caller's memory of it.
///
/// That no current site can *observe* the difference is incidental: a message
/// arrives on connect, before a prompt can be open, and `run` gates on Queries
/// having focus, which no prompt can coexist with. Design §8 anticipates
/// Queries gaining a search, at which point a prompt over it is constructible.
///
/// A live theme picker is unwound for the same reason and by the same rule:
/// its cursor writes `State.theme` as it moves, so replacing it outright
/// would strand the frame in a half-previewed theme with nothing left saying
/// which one the user had chosen (themes design §6). [`restore_theme`] is
/// what puts it back, and — unlike the prompt above — a message *can* arrive
/// over an open picker today, since the picker outlives no focus condition.
fn raise(state: &mut State, message: Message) {
    unwind_overlay(state);
    state.overlay = Some(Overlay::Message(message));
}

/// The one modal a read-only connection answers a mutating action with.
///
/// `what` names the action in the user's vocabulary — the key they pressed,
/// not the statement it would have built — because the statement is the part
/// they have not seen yet.
///
/// There is deliberately no way past it. A posture is a property of the
/// connection, so the way to run this is to open a connection that permits
/// it; an override key here would make the posture a suggestion, and a
/// suggestion is not what vision.md §6 asks for (posture design §3).
///
/// `state.dirty` is set by [`raise`]'s callers elsewhere; set here because
/// both call sites are refusals that change nothing else in the frame.
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

/// Puts back whatever the overlay on screen was previewing, and completes
/// whatever it was leaving pending, so a replacement can be installed over
/// it without stranding either (final review, I4 follow-up).
///
/// Every message-driven overlay replacement goes through here: [`raise`], and
/// the `Exists` arm of [`finish_export`], which puts the export form back over
/// whatever the user opened while the export was in flight — a picker among
/// them, since the overlay is `None` for the whole of a running export and
/// `ctrl-t` is not refused (final review I2).
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

/// The rows the picker draws under `search`, as indices into the unfiltered
/// list — the numbering [`theme_at_picker_index`] counts in, and the one a
/// cursor is put back into when the prompt closes
/// (docs/superpowers/specs/2026-08-31-overlay-filter-design.md §4).
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
