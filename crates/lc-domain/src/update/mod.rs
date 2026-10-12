//! The one place state changes.
//!
//! `update` is synchronous, pure, and total. It mutates state and *describes*
//! what should happen next; it cannot open a socket because it has nothing to
//! open one with (architecture.md §3).

use crate::clock::Stamp;
use crate::commit::Discard;
use crate::config::{Config, ConnectionConfig, ConnectionName, Diagnostic, DriverConfig, PageSize};
use crate::contents::{
    ContentsState, Continue, Failure, Filter, Landed, SelectionMode, Subject, TableRef,
};
use crate::cql;
use crate::ddl::DdlTab;
use crate::export::Format;
use crate::grid::{self, ColumnScroll};
use crate::line::Line;
use crate::log::{Entry, Outcome, Subject as LogSubject, Trace};
use crate::model::{
    ConnId, DbError, JobId, Key, KeyPress, Page, PagingState, QueryId, Row, Schema, Statement,
};
use crate::pointer::Pointer;
use crate::ports::ExportError;
use crate::posture::Posture;
use crate::queries::{CqlMode, QueriesTab, Refresh};
use crate::search::{Direction, Search, Searched};
use crate::state::{
    ConnectionState, Cursor, DiagnosticsPanel, Flash, Geometry, Message, Overlay, Panel, Range,
    Ranges, SchemaState, Scroll, Selection, State, Toast,
};
use crate::style::Severity;
use crate::theme::{BuiltIn, ThemeChoice};
use crate::tree::Node;
use std::borrow::Cow;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

mod commit;
mod connections;
mod drop;
mod edit;
mod export;
mod history;
mod inspect;
mod palette;
mod pointer;
mod search;
mod themes;
mod yank;

use commit::{commit_extent, commit_key, discard_key, finish_commit, open_commit, uncommitted};
use connections::{
    connect, connections_key, default_connection_index, offer_connections, open_connections,
};
use drop::{drop_key, finish_drop, open_drop};
use edit::{field_key, start_edit};
use export::{cancel_pending_export, export_key, export_progress, finish_export, open_export};
use history::{history_key, open_history};
use inspect::{edit_from_grid, inspect_key, inspector_extent, open_inspector};
use palette::{open_palette, palette_key};
use pointer::handle_pointer;
use search::{next_match_key, open_prompt, restore_origin, search_key};
use themes::{open_themes, preview_theme, restore_theme, themes_key};
use yank::{copied, yank};

pub(crate) use connections::connection_names;
pub(crate) use drop::drop_target;
pub(crate) use edit::{editable_here, has_uncommitted};
pub(crate) use history::history_labels;
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
            Vec::new()
        }

        Msg::ExportProgress { job, rows } => export_progress(state, job, rows),

        Msg::ExportFinished { job, result, took } => finish_export(state, job, result, took),

        Msg::Quit => quit(state),
    }
}

fn page_ready(state: &mut State, query: QueryId, page: Page, took: Duration) -> Vec<Effect> {
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

fn query_failed(state: &mut State, query: QueryId, error: &DbError) -> Vec<Effect> {
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

fn connected(state: &mut State, conn: ConnId, posture: Posture, took: Duration) -> Vec<Effect> {
    // Matched rather than asked through `is_current`: the name the connection
    // arrives under is the one the outstanding attempt was begun with, and
    // only `Connecting` holds it.
    let ConnectionState::Connecting {
        conn: outstanding,
        name,
    } = &state.connection
    else {
        return Vec::new();
    };
    if *outstanding != conn {
        return Vec::new();
    }
    let name = name.clone();

    state.connection = ConnectionState::Connected(crate::state::Connection {
        id: conn,
        name,
        posture,
        schema: SchemaState::Loading,
    });
    state.log.complete(
        Trace::Connect(conn),
        Outcome::Succeeded {
            detail: format!("connected, {}", posture_label(posture)),
            took,
        },
    );
    logged(
        state,
        sentence("Reading schema"),
        Outcome::Pending {
            trace: Trace::Schema(conn),
        },
    );
    state.mark_dirty();
    vec![Effect::LoadSchema(conn)]
}

fn connect_failed(state: &mut State, conn: ConnId, error: DbError) -> Vec<Effect> {
    if !state.connection.is_current(conn) {
        return Vec::new();
    }
    state.log.complete(
        Trace::Connect(conn),
        Outcome::Failed {
            detail: error.to_string(),
        },
    );
    state.overlay = Some(Overlay::Fatal(Message {
        title: "Connection failed".to_owned(),
        message: error.to_string(),
        severity: Severity::Danger,
    }));
    state.connection = ConnectionState::Failed(error);
    state.mark_dirty();
    Vec::new()
}

fn schema_failed(state: &mut State, conn: ConnId, error: &DbError) -> Vec<Effect> {
    let applied = match &mut state.connection {
        ConnectionState::Connected(connection) if connection.id == conn => {
            connection.schema = SchemaState::Failed(error.clone());
            true
        }
        _ => false,
    };
    if applied {
        state.log.complete(
            Trace::Schema(conn),
            Outcome::Failed {
                detail: error.to_string(),
            },
        );
        state.overlay = Some(Overlay::Fatal(Message {
            title: "Schema unavailable".to_owned(),
            message: error.to_string(),
            severity: Severity::Danger,
        }));
        state.mark_dirty();
    }
    Vec::new()
}

fn schema_loaded(state: &mut State, conn: ConnId, schema: Schema, took: Duration) -> Vec<Effect> {
    let detail = format!(
        "{} keyspaces, {} tables",
        schema.keyspaces.len(),
        schema
            .keyspaces
            .values()
            .map(|ks| ks.tables.len())
            .sum::<usize>(),
    );
    let applied = match &mut state.connection {
        ConnectionState::Connected(connection) if connection.id == conn => {
            connection.schema = SchemaState::Loaded(schema);
            true
        }
        _ => false,
    };
    if applied {
        state
            .log
            .complete(Trace::Schema(conn), Outcome::Succeeded { detail, took });
        let rows = tree_rows(state);
        state.tables.cursor.clamp_to(rows);
        state.mark_dirty();
    }
    Vec::new()
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

/// Not silent, unlike the first-run example write: that one is ambient and
/// repeats on every launch, so a warning would fire forever on a machine where
/// nothing is wrong. This one happens because a key was pressed, once per
/// press (theme persistence design §6).
fn theme_save_failed(state: &mut State, reason: String) -> Vec<Effect> {
    logged(
        state,
        sentence("Saving the theme"),
        Outcome::Failed {
            detail: reason.clone(),
        },
    );
    // The theme stays applied. A save that failed is not a reason to take back
    // the theme the user chose.
    raise(
        state,
        Message {
            title: "Theme".to_owned(),
            message: reason,
            severity: Severity::Warning,
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

/// The groups, or the one row the cursor is on when nothing is selected.
fn spanned(selection: &Selection, cursor: usize) -> Ranges {
    let ranges = selection.ranges(cursor);

    if ranges.is_empty() {
        Ranges::of(Range::new(cursor, cursor))
    } else {
        ranges
    }
}

/// Which end of a list `g` and `G` reach.
///
/// Narrower than [`Motion`] on purpose: only two of that enum's six landings
/// can be spelled this way, and an overlay resolving `g` has no paging arm
/// to fall through to.
#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

impl Edge {
    const fn motion(self) -> Motion {
        match self {
            Self::Start => Motion::Start,
            Self::End => Motion::End,
        }
    }
}

#[derive(Clone, Copy)]
enum Motion {
    Up,
    Down,
    PageUp,
    PageDown,
    Start,
    End,
}

fn handle_key(state: &mut State, press: KeyPress) -> Vec<Effect> {
    state.mark_dirty();

    // Claimed before the prefix, before ctrl, before focus — nothing else
    // about this press is consulted. The modal has no dismissal, only the
    // one exit (query log design §7).
    if let Some(Overlay::Fatal(_)) = state.overlay {
        return match press.key {
            Key::Escape | Key::Char('q') => quit(state),
            _ => Vec::new(),
        };
    }

    // alt turns a key into a separate binding and none is bound, so it is
    // refused rather than reinterpreted — including on top of ctrl, where a
    // two-modifier chord is a third layer and no layer is designed. ctrl is
    // refused the same way, past the arms `ctrl_key` claims. shift is not
    // checked: a shifted letter already arrives as a different Key::Char, and
    // a shifted arrow is left as a synonym for the plain one so it stays free
    // for a future "extend selection" binding.
    if press.modifiers.alt {
        return Vec::new();
    }

    if press.modifiers.ctrl {
        return ctrl_key(state, press.key);
    }

    // `g`/`G` are decided above the overlays, because they mean the same
    // thing in all of them: the list this frame is driving, whichever that is
    // (docs/shortcuts.md §1). Refused wherever a key is text, which is the one
    // reading `goto` cannot resolve.
    if !takes_text(state) {
        match press.key {
            Key::Char('g') => {
                goto(state, Edge::Start);
                return Vec::new();
            }
            Key::Char('G') => {
                goto(state, Edge::End);
                return Vec::new();
            }
            _ => {}
        }
    }

    if let Some(effects) = overlay_key(state, press.key) {
        return effects;
    }

    // An open buffer claims every printable key and the editing keys; what it
    // does not claim keeps the global meaning it has everywhere else (design
    // §3). No key is carved out, its own number included: `WHERE id = 4` has
    // to be typeable (cql edit mode design §2).
    if state.takes_text() && edit(state, press.key) {
        return Vec::new();
    }

    panel_key(state, press.key)
}

/// The overlay's answer to `key`, or `None` when no overlay claims it.
///
/// An overlay that takes text claims every printable key, the
/// panel-that-takes-text rule applied to an overlay (panel search design
/// §1.1). A picker or the palette takes motion until a sift is up, and text
/// while one is (docs/superpowers/specs/2026-08-16-overlay-search-design.md
/// §5).
fn overlay_key(state: &mut State, key: Key) -> Option<Vec<Effect>> {
    let effects = match state.overlay {
        Some(Overlay::Commands(_)) => palette_key(state, key),
        // Motion, not text: diagnostics has no sift to raise (design §11).
        Some(Overlay::Diagnostics(_)) => diagnostics_key(state, key),
        // Motion, `y`, and the two ways out; `q` still quits (cell inspector
        // design §4).
        Some(Overlay::Inspect(_)) => inspect_key(state, key),
        Some(Overlay::Themes { .. }) => themes_key(state, key),
        Some(Overlay::Connections { .. }) => connections_key(state, key),
        Some(Overlay::History { .. }) => history_key(state, key),
        Some(Overlay::Search(_)) => search_key(state, key),
        // Text on one of its stops, and most printable keys go nowhere on the
        // others (export design §2).
        Some(Overlay::Export(_)) => export_key(state, key),
        // One text field (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §3).
        Some(Overlay::Filter(_)) => {
            filter_key(state, key);
            Vec::new()
        }
        // Every key it is answered with and, on its typed spelling, every
        // printable key besides (drop design §2.1).
        Some(Overlay::Drop(_)) => drop_key(state, key),
        Some(Overlay::Commit(_)) => commit_key(state, key),
        // `q` included: the question has to be answered rather than pressed
        // past (cell editing design §6).
        Some(Overlay::Discard(_)) => discard_key(state, key),
        // `Fatal` is claimed at the top of `handle_key`, before modifiers.
        Some(Overlay::Message(_) | Overlay::Fatal(_)) | None => return None,
    };

    Some(effects)
}

/// What the focused panel answers, once no overlay and no open buffer has
/// claimed the press. The tail of [`handle_key`]'s one dispatch point, split
/// out only for its length (docs/shortcuts.md §2).
fn panel_key(state: &mut State, key: Key) -> Vec<Effect> {
    match key {
        Key::Char('q') => return quit(state),
        Key::Char('?') => open_palette(state),
        Key::Escape => escape(state),
        Key::Tab => walk_landing(state, Step::Forward),
        Key::BackTab => walk_landing(state, Step::Back),
        Key::Char('j') | Key::Down => scroll(state, Motion::Down),
        Key::Char('k') | Key::Up => scroll(state, Motion::Up),
        Key::PageDown => scroll(state, Motion::PageDown),
        Key::PageUp => scroll(state, Motion::PageUp),
        Key::Home => scroll(state, Motion::Start),
        Key::End => scroll(state, Motion::End),
        Key::Enter if state.focus() == Panel::Contents => open_inspector(state),
        Key::Char('e') if state.focus() == Panel::Contents => return edit_from_grid(state),
        Key::Enter | Key::Char('i') if state.focus() == Panel::Queries => {
            state.queries.start_editing();
        }
        Key::Enter => return activate(state),
        Key::Char('l') | Key::Right => return go_right(state),
        Key::Char('h') | Key::Left => go_left(state),
        Key::Char('/') => open_prompt(state),
        Key::Char('y') => return yank(state),
        Key::Char('m') => {
            if state.focus() == Panel::Contents {
                // A range's axis is the mode's axis, so carrying one across
                // would address rows or columns the user never chose — the
                // argument `DdlPanel::show` already makes for the cursor
                // (design §3).
                state.contents.rows.clear();
                state.contents.columns.clear();
                state.contents.mode = state.contents.mode.next();
            }
        }
        // The three column keys. Answered in column mode alone: they address
        // the column the cursor is on, and that cursor is what column mode
        // *is* — outside it `s`, `f` and `c` are free letters the bar does
        // not name (grid sort and filter design §6).
        Key::Char('s') if columnar(state) => state.contents.cycle_sort(column_of(state)),
        Key::Char('f') if columnar(state) => open_filter(state),
        Key::Char('c') if columnar(state) => {
            state.contents.set_filter(column_of(state), None);
        }
        Key::Char('d') => open_drop(state),
        Key::Char('p') => return next_page(state),
        Key::Char('P') => return every_page(state),
        Key::Char('v') => toggle_visual(state),
        Key::Char('n') => next_match_key(state, Direction::Forwards),
        Key::Char('N') => next_match_key(state, Direction::Backwards),
        // Tabs are walked from the focused panel and nowhere else: `]` on
        // Tables would otherwise mean "the DDL panel's next tab", which is a
        // second way to reach a place the number already reaches
        // (docs/shortcuts.md §4).
        Key::Char(']') => walk_tab(state, Step::Forward),
        Key::Char('[') => walk_tab(state, Step::Back),
        // Last, so every binding above keeps precedence over a panel key.
        Key::Char(key) => focus_by_number(state, key),
        _ => {}
    }

    Vec::new()
}

/// Whether this frame types rather than navigates.
///
/// A prompt and the export form claim every printable key themselves, and so
/// does a panel that takes text with nothing over it — an overlay that takes
/// motion is in front of that panel, and its list is what a motion moves
/// (design §1, docs/shortcuts.md §2).
fn takes_text(state: &State) -> bool {
    match &state.overlay {
        Some(Overlay::Search(_) | Overlay::Export(_) | Overlay::Filter(_)) => true,
        // Only on the spelling that has a name to type into. A `y`/`n`
        // confirmation takes no text, so `g`/`G` are decided above it and
        // find nothing to move (drop design §2.1).
        Some(Overlay::Drop(prompt)) => prompt.takes_text(),
        // A sift up makes the overlay itself a text field (overlay search
        // design §5): the letters that would otherwise move the cursor or
        // close the overlay type themselves instead.
        Some(Overlay::Commands(panel)) => panel.sift().is_some(),
        Some(
            Overlay::Themes { sift, .. }
            | Overlay::Connections { sift, .. }
            | Overlay::History { sift, .. },
        ) => sift.is_some(),
        // `Fatal` is unreachable here — `handle_key` claims every press for
        // it before this is consulted — but written down anyway, rather than
        // grouped with `Message`/`None`, because neither it nor `Diagnostics`
        // has text of its own to type into.
        // The inspector takes text on exactly the frames a field is open,
        // the rule the prompt and the form follow (cell editing design §3).
        Some(Overlay::Inspect(inspector)) => inspector.field().is_some(),
        // Neither confirmation has a field to type into either; the commit
        // block drives a body `g`/`G` move, which `goto` answers below.
        Some(
            Overlay::Diagnostics(_) | Overlay::Fatal(_) | Overlay::Commit(_) | Overlay::Discard(_),
        ) => false,
        Some(Overlay::Message(_)) | None => state.takes_text(),
    }
}

/// `g`/`G`, resolved against whatever list the frame is driving: the overlay
/// that captures keys if one is up, and the focused panel otherwise.
///
/// The two landings `Home`/`End` already make, reached through the same
/// methods rather than through a copy of them — the relation `ctrl-d` has to
/// `PageDown` (docs/shortcuts.md §1).
fn goto(state: &mut State, edge: Edge) {
    let last = BuiltIn::ALL.len() + state.config.themes.len();
    let connections = state.config.connections.len();
    let history = state.history().len();
    let (commit_lines, commit_rows) = commit_extent(state);

    match &mut state.overlay {
        Some(Overlay::Commands(panel)) => match edge {
            Edge::Start => panel.to_start(),
            Edge::End => panel.to_end(),
        },
        Some(Overlay::Inspect(inspector)) => match edge {
            Edge::Start => inspector.to_start(),
            Edge::End => inspector.to_end(),
        },
        Some(Overlay::Commit(prompt)) => match edge {
            Edge::Start => prompt.to_start(),
            Edge::End => prompt.to_end(commit_lines, commit_rows),
        },
        Some(Overlay::Diagnostics(panel)) => match edge {
            Edge::Start => panel.to_start(),
            Edge::End => panel.to_end(),
        },
        Some(Overlay::Themes { cursor, .. }) => {
            match edge {
                Edge::Start => cursor.to_start(),
                Edge::End => cursor.to_end(last),
            }
            preview_theme(state);
        }
        Some(Overlay::Connections { cursor, .. }) => match edge {
            Edge::Start => cursor.to_start(),
            Edge::End => cursor.to_end(connections),
        },
        Some(Overlay::History { cursor, .. }) => match edge {
            Edge::Start => cursor.to_start(),
            Edge::End => cursor.to_end(history),
        },
        Some(Overlay::Message(_)) | None => scroll(state, edge.motion()),
        // The drop confirmation drives no list either, and `Fatal` never
        // reaches here for `takes_text`'s reason.
        Some(
            Overlay::Fatal(_)
            | Overlay::Search(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_)
            | Overlay::Discard(_),
        ) => {}
    }
}

/// Which way [`walk_tab`] steps through a panel's strip, or [`walk_landing`]
/// through the `Tab` cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Forward,
    Back,
}

/// Focuses the panel `key` numbers and, where it already had focus, steps to
/// its next tab.
///
/// One key for both: a panel is the place, and its tabs are what that place
/// can show — so a reader who knows how to arrive already knows how to look at
/// the rest (docs/shortcuts.md §4). A panel with one tab answers the second
/// press by staying where it is. Queries is the exception: each of its tabs
/// has a number, and `4` arrives ready to type.
fn focus_by_number(state: &mut State, key: char) {
    if let Some(tab) = QueriesTab::from_number(key) {
        state.focus_on(Panel::Queries);
        state.show_queries_tab(tab);
        state.queries.start_editing();
        return;
    }

    let Some(panel) = Panel::from_number(key) else {
        return;
    };

    if state.focus() == panel {
        walk_tab(state, Step::Forward);
    } else {
        state.focus_on(panel);
    }
}

/// Steps the focused panel's strip one tab in `step`, wrapping.
///
/// A tab the strip does not draw is not a place to arrive at: on a keyspace
/// the DDL panel offers one tab, and walking stays on it. Queries reaches
/// here only with its buffer closed — an open one has already claimed `]`
/// and `[` as text (cql edit mode design §2).
fn walk_tab(state: &mut State, step: Step) {
    match state.focus() {
        Panel::Ddl => {}
        Panel::Queries => {
            let shown = state.queries.tab();
            state.show_queries_tab(match step {
                Step::Forward => shown.next(),
                Step::Back => shown.previous(),
            });
            return;
        }
        Panel::Tables | Panel::Contents => return,
    }

    let shown = state.ddl.tab();
    let candidate = match step {
        Step::Forward => shown.next(),
        Step::Back => shown.previous(),
    };

    if crate::view::ddl_tabs(state).contains(&candidate) {
        state.ddl.show(candidate);
    }
}

/// One stop in the `Tab`/`BackTab` cycle: a panel, and — where it has more
/// than one — the tab it lands on.
///
/// Every `(panel, tab)` pair a strip can draw is a stop, DDL's tabs and
/// Queries' two included, so `Tab` alone walks everything a number and its
/// repeats reach (docs/shortcuts.md §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Landing {
    Tables,
    Ddl(DdlTab),
    Contents,
    Queries(QueriesTab),
}

impl Landing {
    /// Every stop the cycle can be at right now, in the order it visits them.
    ///
    /// Built fresh rather than cached: DDL's strip depends on what the
    /// Tables cursor is on, so a keyspace's single tab is one stop here and a
    /// table's two are two, the same distinction [`walk_tab`] draws for `2`.
    fn all(state: &State) -> Vec<Self> {
        std::iter::once(Self::Tables)
            .chain(crate::view::ddl_tabs(state).iter().copied().map(Self::Ddl))
            .chain(std::iter::once(Self::Contents))
            .chain(QueriesTab::ALL.into_iter().map(Self::Queries))
            .collect()
    }

    /// The stop the frame is sitting on right now.
    fn current(state: &State) -> Self {
        match state.focus() {
            Panel::Tables => Self::Tables,
            Panel::Ddl => Self::Ddl(state.ddl.tab()),
            Panel::Contents => Self::Contents,
            Panel::Queries => Self::Queries(state.queries.tab()),
        }
    }

    /// Focuses the panel this stop names and draws the tab it names, in one
    /// move.
    ///
    /// Both, even where the panel already has focus: [`State::focus_on`]
    /// reverts the Queries tab to `Cql` the moment focus leaves it, so a
    /// landing on `Queries(Logs)` reached only by focusing and never told
    /// which tab to draw would be undone by its own arrival.
    fn arrive(self, state: &mut State) {
        match self {
            Self::Tables => state.focus_on(Panel::Tables),
            Self::Ddl(tab) => {
                state.focus_on(Panel::Ddl);
                state.ddl.show(tab);
            }
            Self::Contents => state.focus_on(Panel::Contents),
            Self::Queries(tab) => {
                state.focus_on(Panel::Queries);
                state.show_queries_tab(tab);
            }
        }
    }
}

/// Steps `Tab`/`BackTab` to the next or previous [`Landing`], wrapping.
fn walk_landing(state: &mut State, step: Step) {
    let stops = Landing::all(state);
    let current = Landing::current(state);
    let index = stops.iter().position(|stop| *stop == current).unwrap_or(0);

    let next = match step {
        Step::Forward => (index + 1) % stops.len(),
        Step::Back => (index + stops.len() - 1) % stops.len(),
    };

    stops[next].arrive(state);
}

/// Draws the `tab`th tab of the strip `panel` last drew. `click_at` has
/// already focused it.
///
/// Reaches the same `show` the keys do — [`walk_tab`] for DDL, [`Landing`]
/// for Queries — so a tab arrived at by pointer and one arrived at by key
/// cannot diverge.
fn show_tab(state: &mut State, panel: Panel, tab: usize) {
    match panel {
        // The map a press was resolved against describes the previous frame,
        // so a strip that has since lost a tab drops the press rather than
        // showing whichever tab took that position (design §6).
        Panel::Ddl => {
            if let Some(tab) = crate::view::ddl_tabs(state).get(tab).copied() {
                state.ddl.show(tab);
            }
        }
        Panel::Queries => {
            if let Some(tab) = QueriesTab::ALL.get(tab).copied() {
                state.show_queries_tab(tab);
            }
        }
        Panel::Tables | Panel::Contents => {}
    }
}

/// Offers `key` to the focused panel's buffer, answering whether it was text.
///
/// The exhaustive match over [`Key`] lives on [`QueriesPanel`], which is what
/// holds the popup the two-key exceptions turn on; what is left here is the
/// half only `update` can do, which is offering against the schema
/// (query editor design §5.4).
fn edit(state: &mut State, key: Key) -> bool {
    let height = state.geometry.height(Panel::Queries);
    let Some(refresh) = state.queries.edit(key, height) else {
        return false;
    };

    match refresh {
        Refresh::Recompute => complete(state),
        Refresh::Close | Refresh::Hold => {}
    }

    true
}

/// Offers the schema's own vocabulary against the word under the cursor.
///
/// The keyspace is the one the Tables panel is showing, which is what makes an
/// unqualified table name completable at all — the panel beside the editor is
/// already saying which keyspace the user is looking at
/// (query editor design §5.2).
fn complete(state: &mut State) {
    let keyspace = state.visible_keyspace();
    let completion = cql::suggest(state.queries.buffer(), state.schema(), keyspace.as_deref());
    state.queries.offer(completion);
}

/// `ctrl-f` lays the buffer out, and does nothing anywhere else: the panel
/// takes text, so the action has no unmodified key available — the shape
/// `ctrl-r` and `ctrl-e` already have (query editor design §4.3).
fn format_buffer(state: &mut State) {
    if state.focus() != Panel::Queries {
        return;
    }

    state.queries.format();
}

/// `ctrl-n` and `ctrl-p` walk the popup, and raise one where none is up: a
/// user who wants to see what is on offer should not have to type a character
/// they will then delete (query editor design §5.5).
fn walk_completion(state: &mut State, forwards: bool) {
    if state.focus() != Panel::Queries {
        return;
    }

    if !state.queries.step(forwards) {
        complete(state);
    }
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

fn posture_label(posture: Posture) -> &'static str {
    match posture {
        Posture::ReadOnly => "read-only",
        Posture::Writable => "writable",
        Posture::Protected => "protected",
    }
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

/// `Esc` resolves in one order, first hit wins: a live sift inside an
/// overlay; then the overlay itself; then a running `P`; then an open buffer;
/// then freezing an extending range; then clearing a frozen range; then the
/// focused panel's search; then the panel itself, which hands focus back to
/// Tables. One press does one of them, never two — the thing most recently put
/// on screen comes off first (panel search design §6, overlay search design
/// §1).
///
/// The last rung is every panel, not just the one that takes text: a key that
/// backs out of Queries but sits inert in DDL beside it is the same key
/// meaning two things (docs/shortcuts.md §2). On Tables it is already the
/// last rung's destination, so it does nothing there.
fn escape(state: &mut State) {
    if dismiss_overlay(state) {
        return;
    }

    // Directly under the overlays: a running `P` is still putting rows on
    // screen (grid paging design §2).
    if state.focus() == Panel::Contents && state.contents.stop_fetching() {
        return;
    }

    // An open buffer is the innermost thing on screen: `esc` closes it and
    // keeps focus, and the next `esc` leaves the panel (cql edit mode design
    // §2).
    if state.takes_text() {
        state.queries.stop_editing();
        return;
    }

    // Above the search for the reason the overlay is above both: the thing
    // most recently put on screen comes off first (design §1).
    if stop_extending(state) || clear_selection(state) {
        return;
    }

    if let Some(target) = Searched::of(state.focus())
        && state.search(target).is_some()
    {
        state.apply_search(target, None);
        return;
    }

    state.focus_on(Panel::default());
}

/// `Esc`'s first two rungs: a live sift inside an overlay, then the overlay
/// itself. Whether there was an overlay to answer.
fn dismiss_overlay(state: &mut State) -> bool {
    // Read before the `take()` below empties it: a click is the one path
    // that reaches `Overlay::Export`'s arm here rather than `export_key`,
    // which claims keyboard `Esc` itself (final review, I4 follow-up).
    cancel_pending_export(state);

    match state.overlay.take() {
        Some(Overlay::Search(prompt)) => {
            state.apply_search(prompt.target(), prompt.previous().cloned());
            restore_origin(state, prompt.target(), prompt.origin());
            true
        }
        Some(Overlay::Themes {
            cursor,
            restored,
            sift: Some(sift),
        }) => {
            let mut cursor = cursor;
            cursor.go_to(
                sift.origin(),
                BuiltIn::ALL.len() + state.config.themes.len(),
            );
            state.overlay = Some(Overlay::Themes {
                cursor,
                restored,
                sift: None,
            });
            preview_theme(state);
            true
        }
        Some(Overlay::Themes { restored, .. }) => {
            state.theme = restored;
            true
        }
        Some(Overlay::Connections {
            cursor,
            sift: Some(sift),
        }) => {
            let mut cursor = cursor;
            cursor.go_to(sift.origin(), state.config.connections.len());
            state.overlay = Some(Overlay::Connections { cursor, sift: None });
            true
        }
        Some(Overlay::History {
            cursor,
            sift: Some(sift),
        }) => {
            let mut cursor = cursor;
            cursor.go_to(sift.origin(), state.history().len());
            state.overlay = Some(Overlay::History { cursor, sift: None });
            true
        }
        // A dismissed picker leaves `Disconnected` rather than `Unchosen`, so
        // `offer_connections` does not put it straight back up: waving the
        // picker away is a choice, and the panels are allowed to stay empty
        // (connection picker design §4).
        Some(Overlay::Connections { .. }) => {
            if matches!(state.connection, ConnectionState::Unchosen) {
                state.connection = ConnectionState::Disconnected;
            }
            true
        }
        Some(Overlay::Commands(mut panel)) if panel.sift().is_some() => {
            panel.restore_origin();
            state.overlay = Some(Overlay::Commands(panel));
            true
        }
        // The filter prompt applies nothing on the way out: `esc` leaves the
        // column filtered exactly as it was, and `⏎` is the one key that
        // writes (grid sort and filter design §3).
        Some(
            Overlay::Message(_)
            | Overlay::Commands(_)
            | Overlay::History { .. }
            | Overlay::Diagnostics(_)
            | Overlay::Inspect(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_)
            // Both confirmations abandon on `Esc`, leaving the edits standing:
            // dismissing a question is not answering it
            // (cell editing design §5).
            | Overlay::Commit(_)
            | Overlay::Discard(_),
        ) => {
            // The diagnostics modal is what claims startup's first frame when
            // the config had anything wrong with it; closing it is where the
            // picker underneath finally gets its turn.
            offer_connections(state);
            true
        }
        // Not dismissable: `handle_key` claims `Esc` for `Fatal` before this
        // function is reached, so the only caller left is a click routed
        // through here by `handle_pointer`, which must not spend itself on
        // ending the modal (query log design §7).
        Some(fatal @ Overlay::Fatal(_)) => {
            state.overlay = Some(fatal);
            true
        }
        None => false,
    }
}

/// What an open diagnostics overlay responds to.
///
/// `Esc` dismisses it and motion moves the cursor, bounded by the row count —
/// the same treatment [`palette_key`] gives `Overlay::Commands`, minus the
/// paging arithmetic a palette needs to skip a heading, which this list has
/// none of.
fn diagnostics_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        state.overlay = None;
        // What the config got wrong is read before a connection is chosen, so
        // this dismissal is where the picker underneath gets its turn
        // (connection picker design §4).
        offer_connections(state);
        return Vec::new();
    }

    // `command_rows` counts one-line palette rows; a diagnostic renders as
    // two, so a page is half as many rows as it is lines.
    let height = state.geometry.command_rows() / 2;
    let Some(Overlay::Diagnostics(panel)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => panel.down(),
        Key::Char('k') | Key::Up => panel.up(),
        Key::PageDown => panel.page_down(height),
        Key::PageUp => panel.page_up(height),
        Key::Home => panel.to_start(),
        Key::End => panel.to_end(),
        _ => {}
    }

    Vec::new()
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

#[derive(Clone, Copy)]
enum Transition {
    Open,
    Close,
}

/// Applies `motion` to the focused panel's selection.
///
/// The focused panel and no other. A wheel names the panel under the pointer
/// and moves its viewport directly ([`wheel`]), so nothing but a key reaches
/// here.
///
/// In Ddl the height is counted in wrapped lines and the cursor in rows, so a
/// page overshoots on a run of wrapped values. Accepted rather than corrected:
/// the fix needs a line-to-row map from the renderer, and `home`/`end` are
/// exact regardless (design §4).
fn scroll(state: &mut State, motion: Motion) {
    let panel = state.focus();

    // The Cql tab's rows are its statements. Reachable while the buffer is
    // closed alone — an open one claims these keys as motions before
    // `panel_key` is consulted — which is the mode the band is drawn in
    // (cql statement selection design §4).
    if panel == Panel::Queries && state.queries.mode() == Some(CqlMode::Browsing) {
        step_statement(state, motion);
        return;
    }

    let height = state.geometry.height(panel);
    let len = row_count(state, panel);

    let Some(cursor) = panel_cursor(state, panel) else {
        return;
    };

    match motion {
        Motion::Down => cursor.down(len),
        Motion::Up => cursor.up(),
        Motion::PageDown => cursor.page_down(len, height),
        Motion::PageUp => cursor.page_up(height),
        Motion::Start => cursor.to_start(),
        Motion::End => cursor.to_end(len),
    }
}

/// The Cql tab's motions, which walk statements rather than rows: the band
/// names what `ctrl-r` would run, and stepping it is what makes a buffer of
/// several statements navigable (cql statement selection design §4).
///
/// Paging is left inert. The buffer's viewport is derived from the cursor
/// rather than stored (queries editor design §5), so a statement step already
/// brings its own statement on screen and there is no second thing for a page
/// to move.
fn step_statement(state: &mut State, motion: Motion) {
    match motion {
        Motion::Down => state.queries.step_statement(true),
        Motion::Up => state.queries.step_statement(false),
        Motion::Start => state.queries.edge_statement(false),
        Motion::End => state.queries.edge_statement(true),
        Motion::PageDown | Motion::PageUp => {}
    }
}

/// The ctrl layer's whole key table (docs/shortcuts.md §2).
///
/// Dispatched above the palette check, so it honours that rule itself rather
/// than inheriting it: routing a modified key into `palette_key`, which
/// matches on the key alone, would make `ctrl-j` move the palette cursor.
/// Each arm gates on its own panel below.
fn ctrl_key(state: &mut State, key: Key) -> Vec<Effect> {
    // Paging is decided above the refusal below: `ctrl-d`/`ctrl-u` are a
    // second spelling of `PageDown`/`PageUp` rather than members of their own,
    // so they reach whatever those two reach, an open overlay included.
    match key {
        Key::Char('d') => return page(state, Key::PageDown),
        Key::Char('u') => return page(state, Key::PageUp),
        _ => {}
    }

    if matches!(
        state.overlay,
        Some(
            Overlay::Commands(_)
                | Overlay::Themes { .. }
                | Overlay::Connections { .. }
                | Overlay::History { .. }
                | Overlay::Diagnostics(_)
                | Overlay::Inspect(_)
                | Overlay::Export(_)
                | Overlay::Filter(_)
                | Overlay::Drop(_)
                | Overlay::Commit(_)
                | Overlay::Discard(_)
        )
    ) {
        return Vec::new();
    }

    match key {
        Key::Char('r') => run(state),
        Key::Char('t') => {
            open_themes(state);
            Vec::new()
        }
        // The way back to the picker. Without it a dismissed picker is
        // final — `Esc` leaves `Disconnected`, which nothing re-offers — and
        // a user who opened the wrong cluster would have to restart the tool
        // to reach another (connection picker design §5).
        Key::Char('o') => {
            open_connections(state);
            Vec::new()
        }
        Key::Char('e') => ctrl_e(state),
        Key::Char('g') => {
            open_history(state);
            Vec::new()
        }
        Key::Char('s') => open_commit(state),
        Key::Char('f') => {
            format_buffer(state);
            Vec::new()
        }
        Key::Char('n') => {
            walk_completion(state, true);
            Vec::new()
        }
        Key::Char('p') => {
            walk_completion(state, false);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// `ctrl-e` opens the user's editor on the Queries buffer and the export form
/// over the Contents grid. Two meanings, each gated on its own panel — the
/// shape `ctrl-r` already has (export design §1).
fn ctrl_e(state: &mut State) -> Vec<Effect> {
    match state.focus() {
        Panel::Queries => open_editor(state),
        Panel::Contents => {
            open_export(state);
            Vec::new()
        }
        Panel::Tables | Panel::Ddl => Vec::new(),
    }
}

/// `ctrl-e` opens the user's editor on the Queries buffer, and does nothing
/// anywhere else: the panel takes text, so the action has no unmodified key
/// available (external editor design §1).
fn open_editor(state: &State) -> Vec<Effect> {
    if state.focus() != Panel::Queries {
        return Vec::new();
    }

    vec![Effect::OpenEditor {
        buffer: state.queries.text(),
    }]
}

/// Whether the three column keys answer this frame: Table contents focused,
/// in column mode, over a page that has columns to address.
///
/// The mode is the gate rather than the panel, so `s`, `f` and `c` stay free
/// letters everywhere else — including in row and cell mode, where the cursor
/// names a row or a cell and a column key would have to guess
/// (grid sort and filter design §6).
fn columnar(state: &State) -> bool {
    state.focus() == Panel::Contents
        && state.contents.mode == SelectionMode::Column
        && state.contents.column_count() > 0
}

/// The column the grid's cursor is on.
fn column_of(state: &State) -> usize {
    state.contents.column.index()
}

/// Opens the filter prompt over the column the cursor is on, seeded with
/// whatever filter already stands there (design §3).
fn open_filter(state: &mut State) {
    let column = column_of(state);
    let Some(page) = state.contents.loaded() else {
        return;
    };
    let Some(name) = page.page().columns.get(column) else {
        return;
    };

    let filter = Filter::opened(column, name, page.filter(column));
    state.overlay = Some(Overlay::Filter(filter));
}

/// What the filter prompt responds to: text, `⏎` to apply and `esc` to leave
/// it as it was (design §3).
fn filter_key(state: &mut State, key: Key) {
    let Some(Overlay::Filter(filter)) = &mut state.overlay else {
        return;
    };

    match key {
        Key::Char(character) => filter.push(character),
        Key::Backspace => filter.pop(),
        Key::Enter => {
            let (column, search) = (filter.column(), filter.search());
            state.overlay = None;
            state.contents.set_filter(column, search);
        }
        Key::Escape => escape(state),
        _ => {}
    }
}

/// What [`export_key`]'s form-only match decided.
enum Decision {
    Nothing,
    Close,
    Run,
}

/// The keys a single-line field claims: every printable key is a character
/// and the editing keys move within the line. Shared by the export form's
/// path stop (export design §2) and the drop confirmation's typed name
/// (drop design §2.1), which take text on the same terms.
fn edit_line(line: &mut Line, key: Key) {
    match key {
        Key::Char(character) => line.insert(character),
        Key::Backspace => line.delete_back(),
        Key::Delete => line.delete_forward(),
        Key::Left => line.left(),
        Key::Right => line.right(),
        Key::Home => line.home(),
        Key::End => line.end(),
        _ => {}
    }
}

/// `ctrl-d`/`ctrl-u`, dispatched to whatever `key` would reach unmodified.
///
/// Delegating rather than paging here is what keeps the two spellings from
/// drifting: an overlay that learns to page learns it once.
fn page(state: &mut State, key: Key) -> Vec<Effect> {
    match &state.overlay {
        Some(Overlay::Commands(_)) => palette_key(state, key),
        Some(Overlay::Themes { .. }) => themes_key(state, key),
        Some(Overlay::Connections { .. }) => connections_key(state, key),
        Some(Overlay::History { .. }) => history_key(state, key),
        Some(Overlay::Diagnostics(_)) => diagnostics_key(state, key),
        Some(Overlay::Inspect(_)) => inspect_key(state, key),
        // The prompt takes text and pages nothing, so neither spelling does;
        // the form and the filter modal have nothing paged either, and the
        // confirmation drives no list. `Fatal` is unreachable here for
        // `takes_text`'s reason.
        Some(Overlay::Commit(_)) => commit_key(state, key),
        Some(
            Overlay::Fatal(_)
            | Overlay::Search(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_)
            | Overlay::Discard(_),
        ) => Vec::new(),
        Some(Overlay::Message(_)) | None => {
            if state.takes_text() {
                edit(state, key);
            } else if matches!(key, Key::PageDown) {
                scroll(state, Motion::PageDown);
            } else {
                scroll(state, Motion::PageUp);
            }

            Vec::new()
        }
    }
}

/// How many rows `panel`'s *cursor* moves over — entries in the Logs tab. The
/// Cql tab never arrives here: the editor claims the motion keys first
/// (queries editor design §3).
fn row_count(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Tables => tree_rows(state),
        Panel::Ddl => crate::view::ddl_rows(state),
        Panel::Contents => state.contents.row_count(),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => 0,
            QueriesTab::Logs => state.log.len(),
        },
    }
}

/// The Cql tab never arrives here — the editor claims the motion keys before
/// this match is reached (design §3) — so that arm is unreachable in
/// practice and kept for exhaustiveness.
fn panel_cursor(state: &mut State, panel: Panel) -> Option<&mut Cursor> {
    match panel {
        Panel::Tables => Some(&mut state.tables.cursor),
        Panel::Ddl => Some(&mut state.ddl.cursor),
        Panel::Contents => Some(&mut state.contents.cursor),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => None,
            QueriesTab::Logs => Some(&mut state.log.cursor),
        },
    }
}

/// One axis a panel selects on: the run selected, and the cursor that is its
/// far end while it is being extended.
///
/// The two travel together because reading one without the other is always a
/// mistake — a `Selection` only means a range in company with its cursor, and
/// which cursor that is depends on the grid's mode.
struct Axis<'a> {
    selection: &'a mut Selection,
    cursor: &'a mut Cursor,
    /// How many rows the axis has, so a cursor sent along it cannot leave it.
    len: usize,
}

/// The axes `panel` selects on. Two for the grid in cell mode, which selects a
/// rectangle (design §3); none for Queries' Cql tab, which is an editor; one
/// for its Logs tab, a list like Tables and Ddl (query log design §5.3).
///
/// The one place a selection is matched up with its cursor. Column mode puts
/// the *column* cursor on the first axis, and a second match written out
/// elsewhere would be one edit away from disagreeing about that — silently,
/// since both are `usize`.
fn axes(state: &mut State, panel: Panel) -> [Option<Axis<'_>>; 2] {
    let rows = row_count(state, panel);
    let columns = state.contents.column_count();
    let contents = &mut state.contents;

    match panel {
        Panel::Tables => [
            Some(Axis {
                selection: &mut state.tables.selection,
                cursor: &mut state.tables.cursor,
                len: rows,
            }),
            None,
        ],
        Panel::Ddl => [
            Some(Axis {
                selection: &mut state.ddl.selection,
                cursor: &mut state.ddl.cursor,
                len: rows,
            }),
            None,
        ],
        Panel::Contents => match contents.mode {
            SelectionMode::Row => [
                Some(Axis {
                    selection: &mut contents.rows,
                    cursor: &mut contents.cursor,
                    len: rows,
                }),
                None,
            ],
            SelectionMode::Column => [
                Some(Axis {
                    selection: &mut contents.columns,
                    cursor: &mut contents.column,
                    len: columns,
                }),
                None,
            ],
            SelectionMode::Cell => [
                Some(Axis {
                    selection: &mut contents.rows,
                    cursor: &mut contents.cursor,
                    len: rows,
                }),
                Some(Axis {
                    selection: &mut contents.columns,
                    cursor: &mut contents.column,
                    len: columns,
                }),
            ],
        },
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => [None, None],
            QueriesTab::Logs => [
                Some(Axis {
                    selection: &mut state.log.selection,
                    cursor: &mut state.log.cursor,
                    len: rows,
                }),
                None,
            ],
        },
    }
}

/// `v`: freeze, drop or open a group at the cursor (multi-group selection
/// design §1).
fn toggle_visual(state: &mut State) {
    for axis in axes(state, state.focus()).into_iter().flatten() {
        axis.selection.toggle(axis.cursor.index());
    }
}

/// `esc`'s second rung: leave visual mode, keeping the range. Reports whether
/// there was one to leave.
fn stop_extending(state: &mut State) -> bool {
    let mut stopped = false;

    for axis in axes(state, state.focus()).into_iter().flatten() {
        if axis.selection.is_extending() {
            axis.selection.stop(axis.cursor.index());
            stopped = true;
        }
    }

    stopped
}

/// `esc`'s third rung: drop the range. Reports whether there was one.
fn clear_selection(state: &mut State) -> bool {
    let mut cleared = false;

    for axis in axes(state, state.focus()).into_iter().flatten() {
        if axis.selection.is_active() {
            axis.selection.clear();
            cleared = true;
        }
    }

    cleared
}

/// `y`'s second half: spend the selection and send each cursor back to the end
/// it grew from (visual mode design §5.1).
///
/// A yank is one gesture rather than a mode the user is left standing in — the
/// range has been said, so it stops being drawn, and the cursor goes back to
/// where `v` was pressed rather than sitting wherever the range happened to
/// end. The anchor and not the range's first row: a selection grown upwards
/// started at its last one.
fn collapse_selection(state: &mut State) {
    for axis in axes(state, state.focus()).into_iter().flatten() {
        // The selection is spent unconditionally (visual-mode design §5.1);
        // only the cursor's move home is conditional on an anchor surviving
        // (multi-group design §3).
        if let Some(anchor) = axis.selection.anchor() {
            axis.cursor.go_to(anchor, axis.len);
        }
        axis.selection.clear();
    }
}

/// Records what the last frame turned out to be, and repairs whatever that
/// invalidates.
///
/// No unconditional `mark_dirty`: this message describes the frame that
/// produced it, so redrawing on every one of them would re-measure and redraw
/// forever (panel scrolling design §3).
fn measured(state: &mut State, geometry: Geometry) -> Vec<Effect> {
    let previous = std::mem::replace(&mut state.geometry, geometry);

    follow_viewports(state, previous, geometry);

    // The grid projects only as many rows as its panel is tall.
    if geometry.height(Panel::Contents) != previous.height(Panel::Contents) {
        state.mark_dirty();
    }

    // The bar's `? commands` hint is a function of `command_rows`
    // (view/hints.rs): a resize that flips it between zero and non-zero
    // changes the view model as surely as a viewport clamp does.
    if geometry.command_rows() != previous.command_rows() {
        state.mark_dirty();
    }

    close_unfitting_overlay(state, geometry);
    refit_overlay(state, geometry);

    // Last, so a picker is offered only into a frame the guards above have
    // already emptied. `init` had no frame to size one against, so this
    // measurement is where an unchosen connection gets its offer
    // (connection picker design §4).
    offer_connections(state);

    Vec::new()
}

/// Keeps every panel's viewport on its selection across a change of frame.
fn follow_viewports(state: &mut State, previous: Geometry, geometry: Geometry) {
    // A panel that changed height must draw its selection again: the renderer
    // stopped arranging that, and `update`'s epilogue follows only a cursor
    // that *moved* — a resize moves none
    // (docs/superpowers/specs/2026-08-09-list-viewport-design.md §5).
    //
    // Keyed on the height rather than on the message, so a wheel cannot be
    // undone here: a wheel changes no height, and `lc-app` reports no
    // measurement for a frame whose geometry did not change.
    for panel in LISTS {
        let height = geometry.height(panel);

        if height == previous.height(panel) {
            continue;
        }

        let index = cursor_index(state, panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.follow(index, height);
        }
    }

    // Follows here rather than in `update`'s epilogue because the epilogue's
    // geometry describes the *previous* frame, and so the previously selected
    // row: there is no correct offset for it to compute (design §5.4).
    //
    // Unconditional, and safe from the wheel without a guard: a wheel changes
    // no height, no wrapped-line count and no span, so `lc-app` sends no
    // measurement at all. The same covers ctrl-e and ctrl-y.
    let before = state.ddl.scroll;
    let height = geometry.height(Panel::Ddl);
    state
        .ddl
        .scroll
        .follow_span(geometry.ddl_selection(), height);
    state.ddl.scroll.clamp_to(geometry.ddl_lines(), height);

    if state.ddl.scroll != before {
        state.mark_dirty();
    }

    // The Logs tab's viewport, for the same reason as the Ddl block above:
    // its offset counts wrapped lines rather than entries, so it is followed
    // by span here rather than by index in the `LISTS` loop (design §5.4).
    let log_before = state.log.scroll;
    let log_height = geometry.height(Panel::Queries);
    state
        .log
        .scroll
        .follow_span(geometry.log_selection(), log_height);
    state.log.scroll.clamp_to(geometry.log_lines(), log_height);

    if state.log.scroll != log_before {
        state.mark_dirty();
    }
}

/// Closes an overlay the frame shrank past: nothing would draw it, and it
/// would still be capturing every key (design §5.1). Leaves the way `Esc`
/// would, so the theme picker restores the theme it opened on rather than
/// stranding the preview, and an export confirmation completes its pending
/// trail entry.
fn close_unfitting_overlay(state: &mut State, geometry: Geometry) {
    let Some(overlay) = &state.overlay else {
        return;
    };

    if fits(overlay, geometry, &state.config) {
        return;
    }

    restore_theme(state);
    cancel_pending_export(state);
    state.overlay = None;
    state.mark_dirty();
}

/// Whether `geometry` leaves room to draw `overlay`. Each block answers on its
/// own gate: they are different sizes, and one that fits where another does
/// not must not borrow the other's threshold.
fn fits(overlay: &Overlay, geometry: Geometry, config: &Config) -> bool {
    match overlay {
        Overlay::Commands(_) | Overlay::Diagnostics(_) => geometry.command_rows() > 0,
        // Config themes can make the picker's list not fit where a palette
        // would (design §9).
        Overlay::Themes { .. } => geometry.theme_rows() > 0,
        // An empty list is guidance, not a list: there are no rows for a frame
        // to be too short for.
        Overlay::Connections { .. } => {
            config.connections.is_empty() || geometry.connection_rows() > 0
        }
        Overlay::History { .. } => geometry.history_rows() > 0,
        Overlay::Inspect(_) => geometry.inspector_rows() > 0,
        Overlay::Commit(_) => geometry.commit_rows() > 0,
        Overlay::Export(_) => geometry.export_fits(),
        // A different block from the export form, and fits where it does not
        // (drop design §1).
        Overlay::Drop(_) => geometry.drop_fits(),
        Overlay::Message(_)
        | Overlay::Search(_)
        | Overlay::Filter(_)
        | Overlay::Discard(_)
        | Overlay::Fatal(_) => true,
    }
}

/// Pulls a scrolling overlay's offset back inside its body: a narrower frame
/// can rewrap the body shorter than the offset it was left at. The commit
/// confirmation has no cursor, so only the inspector has a line to follow
/// (cell editing design §5).
fn refit_overlay(state: &mut State, geometry: Geometry) {
    let (lines, height) = inspector_extent(state);
    if let Some(Overlay::Inspect(inspector)) = &mut state.overlay {
        let before = inspector.offset();
        inspector.follow(geometry.inspector_selection(), height);
        inspector.clamp_to(lines, height);

        if inspector.offset() != before {
            state.dirty = true;
        }
    }

    let (lines, height) = commit_extent(state);
    if let Some(Overlay::Commit(prompt)) = &mut state.overlay {
        let before = prompt.offset();
        prompt.clamp_to(lines, height);

        if prompt.offset() != before {
            state.dirty = true;
        }
    }
}

/// The panels whose viewport `update` follows by index. Ddl keeps its own: its
/// offset counts wrapped lines rather than rows, so it is followed by span
/// in `measured` instead (design §5.1).
const LISTS: [Panel; 2] = [Panel::Tables, Panel::Contents];

/// The viewport `panel` draws through, for the panels that have one.
fn panel_scroll(state: &mut State, panel: Panel) -> Option<&mut Scroll> {
    match panel {
        Panel::Tables => Some(&mut state.tables.scroll),
        Panel::Contents => Some(&mut state.contents.scroll),
        Panel::Ddl => Some(&mut state.ddl.scroll),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => None,
            QueriesTab::Logs => Some(&mut state.log.scroll),
        },
    }
}

/// How long the document `panel` scrolls over is — wrapped lines in Ddl and in
/// the Logs tab, rows elsewhere. The Cql tab has no document to scroll: the
/// editor claims its motion keys first (queries editor design §3).
fn scroll_len(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Ddl => state.geometry.ddl_lines(),
        Panel::Tables | Panel::Contents => row_count(state, panel),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => 0,
            QueriesTab::Logs => state.geometry.log_lines(),
        },
    }
}

/// `panel`'s selected row, or zero for a panel with no selection.
fn cursor_index(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Tables => state.tables.cursor.index(),
        Panel::Ddl => state.ddl.cursor.index(),
        Panel::Contents => state.contents.cursor.index(),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => 0,
            QueriesTab::Logs => state.log.cursor.index(),
        },
    }
}

/// Each list panel's selected index, for the comparison in [`update`].
///
/// Built from `LISTS` rather than written out, so the array this returns and
/// the one [`follow_moved_cursors`] walks cannot fall out of order — a
/// mismatch there would follow each panel against the other's index, silently.
fn cursor_indices(state: &State) -> [usize; 2] {
    LISTS.map(|panel| cursor_index(state, panel))
}

/// Pulls every list offset back inside its document.
///
/// Unconditional because a shrinking list does not change the geometry —
/// folding a keyspace leaves every panel the same height, so no
/// [`Msg::Measured`] arrives to clamp against (design §3). `clamp_to` only
/// ever reduces an offset, so this cannot fight a wheel bounded by the same
/// arithmetic.
fn clamp_lists(state: &mut State) {
    for panel in LISTS {
        let len = row_count(state, panel);
        let height = state.geometry.height(panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.clamp_to(len, height);
        }
    }

    // Not in the loop above: `LISTS` is the panels whose offsets are followed
    // by index, and Ddl's offset counts lines while its cursor counts rows
    // (design §5.1). The clamp is unconditional for the same reason the loop
    // is — a schema reload that shrinks a table changes no geometry, so no
    // measurement arrives to clamp against.
    state.ddl.cursor.clamp_to(crate::view::ddl_rows(state));

    // Beside the cursor clamps and for the same reason: a schema reload or a
    // fold that shrinks a list changes no geometry, so no measurement arrives
    // to clamp against (design §2).
    state.tables.selection.clamp_to(tree_rows(state));
    state.ddl.selection.clamp_to(crate::view::ddl_rows(state));

    // The row cursor as well as the ranges: a filter shrinks the grid under
    // a cursor no page change reset (grid sort and filter design §4).
    state.contents.cursor.clamp_to(state.contents.row_count());
    state.contents.rows.clamp_to(state.contents.row_count());
    state
        .contents
        .columns
        .clamp_to(state.contents.column_count());
}

/// Pulls the offset of each list whose selection moved, so the selection is
/// drawn.
///
/// Only the ones that moved: a wheel leaves both indices alone, and following
/// it would undo the scroll it just performed (design §3).
fn follow_moved_cursors(state: &mut State, before: [usize; 2]) {
    for (panel, before) in LISTS.into_iter().zip(before) {
        let index = cursor_index(state, panel);

        if index == before {
            continue;
        }

        let height = state.geometry.height(panel);

        if let Some(scroll) = panel_scroll(state, panel) {
            scroll.follow(index, height);
        }
    }
}

/// `Enter` and `Space`: unfold a keyspace, or read a table.
fn activate(state: &mut State) -> Vec<Effect> {
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
fn read(state: &mut State, table: TableRef) -> Vec<Effect> {
    state.focus_on(Panel::Contents);

    let stmt = Statement::select_all(&table.keyspace, &table.table);
    submit_into_contents(state, Subject::Table(table), stmt)
}

/// `ctrl-r`: the Queries buffer, whole, as one statement (design §3).
fn run(state: &mut State) -> Vec<Effect> {
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
fn record_history(state: &mut State, cql: &str) -> Option<Effect> {
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
fn submit_into_contents(state: &mut State, subject: Subject, stmt: Statement) -> Vec<Effect> {
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

/// `l` and `→`: into the tree, or on to the next column.
///
/// The four horizontal keys mean *move left* and *move right*, and each panel
/// resolves that in its own vocabulary — the shape `j`/`k` already have
/// (docs/shortcuts.md §2, docs/superpowers/specs/2026-08-10-table-display-design.md §2).
fn go_right(state: &mut State) -> Vec<Effect> {
    match state.focus() {
        Panel::Tables => return step_in(state),
        Panel::Contents => step_column(state, Side::Right),
        Panel::Ddl | Panel::Queries => {}
    }

    Vec::new()
}

/// `l` and `→` in Tables: a keyspace opens, a table is read.
///
/// *Move right* out of a table row is the panel to its right — the tree has no
/// further level, and Contents is what a table's contents are drawn in — so
/// the key lands where `⏎` already does rather than doing nothing
/// (docs/shortcuts.md §4). The keyspace arm keeps [`TablesPanel::open`]'s own
/// vocabulary, which is what makes it not merely `⏎`'s synonym: `⏎` folds an
/// open keyspace, `l` steps into it.
fn step_in(state: &mut State) -> Vec<Effect> {
    let table = {
        let Some((tables, schema)) = state.tables_and_schema() else {
            return Vec::new();
        };

        match tables.selected(schema) {
            // Reading a table needs one table, the argument `activate` makes
            // for `⏎` (docs/shortcuts.md §4). Folding is left alone: it
            // addresses the cursor's row, not the selection.
            Some(Node::Table { .. }) if tables.selects_many() => return Vec::new(),
            Some(Node::Table { keyspace, table }) => TableRef::new(&keyspace.name, &table.name),
            _ => {
                transition(state, Transition::Open);
                return Vec::new();
            }
        }
    };

    read(state, table)
}

fn go_left(state: &mut State) {
    match state.focus() {
        Panel::Tables => transition(state, Transition::Close),
        Panel::Contents => step_column(state, Side::Left),
        Panel::Ddl | Panel::Queries => {}
    }
}

/// Which edge of the drawn column window a press works from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// `h`/`l` on the grid: the column cursor by one, or — in row mode — the
/// window by one.
///
/// The cursor is what moves in both cases, because it is what
/// [`follow_grid_columns`] carries the window with (selection modes design
/// §3). The modes differ only in where it is put: in cell and column mode the
/// cursor is on screen, so moving it one column is a move the user can see,
/// and the window comes along once it reaches an edge. Row mode paints no
/// column, so those same presses are invisible until the cursor has crossed a
/// whole screen of columns — the grid stands still under a key documented to
/// move it, and a wide table reads as one that cannot be scrolled at all.
/// There the cursor lands on the column just outside the window instead, so
/// every press reveals a column, `csvlens`-style.
fn step_column(state: &mut State, side: Side) {
    let column = match state.contents.mode {
        SelectionMode::Row => match beyond(state, side) {
            Some(column) => column,
            // The window is against that edge of the page. Left as it is
            // rather than clamped to the edge column: in row mode that would
            // be a cursor move nothing draws, and the modes the user can cycle
            // to would then open somewhere the presses never showed them.
            None => return,
        },
        SelectionMode::Cell | SelectionMode::Column => match side {
            Side::Left => return state.contents.column.up(),
            Side::Right => return state.contents.column.down(state.contents.column_count()),
        },
    };

    state
        .contents
        .column
        .go_to(column, state.contents.column_count());
}

/// The column just outside the drawn window on `side`, or `None` when the
/// window already ends there — including when the whole page fits, where
/// there is nothing to scroll in either direction.
fn beyond(state: &State, side: Side) -> Option<usize> {
    let (widths, available) = grid_columns(state)?;
    let first = state.contents.column_scroll.first();

    match side {
        Side::Left => first.checked_sub(1),
        Side::Right => {
            let next = first + grid::fitting(&widths, first, available);

            (next < widths.len()).then_some(next)
        }
    }
}

/// The columns the grid is drawing over, and the width they have.
///
/// `None` when there is no page: nothing to fit, and nothing to follow.
fn grid_columns(state: &State) -> Option<(Cow<'_, [u16]>, u16)> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };

    Some(page.drawn(state.geometry.grid_width()))
}

/// Pulls the column viewport over the selected cell, and back inside a page
/// that narrowed or a panel that grew.
///
/// Unconditional, for [`clamp_lists`]'s reason: a new page changes no
/// geometry, so no [`Msg::Measured`] arrives to clamp against.
fn follow_grid_columns(state: &mut State) {
    let column = state.contents.column.index();

    let Some((widths, available)) = grid_columns(state) else {
        state.contents.column_scroll = ColumnScroll::default();
        return;
    };

    // Moved out and back so the widths can stay borrowed from the page they
    // were measured with.
    let mut columns = state.contents.column_scroll;
    // Clamp first, follow second, mirroring `clamp_lists` before
    // `follow_moved_cursors` on the row axis: the clamp answers "is the
    // offset too far right for this page and this width", which can leave the
    // cursor off screen, and the follow is what puts it back. The other order
    // would let the clamp undo the follow and hide the selected cell.
    columns.clamp_to(&widths, available);
    columns.follow(column, &widths, available);

    state.contents.column_scroll = columns;
}

fn transition(state: &mut State, transition: Transition) {
    if state.focus() != Panel::Tables {
        return;
    }

    // The search guard lives in TablesPanel::open/close themselves (design
    // §4.1), so every entry point — this one, and activate's click/Enter/Space
    // path — inherits it from one place rather than two that must agree.
    let Some((tables, schema)) = state.tables_and_schema() else {
        return;
    };

    match transition {
        Transition::Open => tables.open(schema),
        Transition::Close => tables.close(schema),
    }
}

fn tree_rows(state: &State) -> usize {
    state
        .schema()
        .map_or(0, |schema| state.tables.rows(schema).len())
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
fn next_page(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Contents {
        return Vec::new();
    }

    let next = state.contents.fetch_one(QueryId::next);
    follow(state, next)
}

/// `P`: pages until the result ends or the next stop (grid paging design §3).
fn every_page(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Contents {
        return Vec::new();
    }

    let next = state.contents.fetch_all(QueryId::next);
    follow(state, next)
}

/// Asks for the page the grid wants next, if it wants one. A continuation is
/// not a new statement: no trail entry and no history (grid paging design §2).
fn follow(state: &mut State, next: Option<Continue>) -> Vec<Effect> {
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

#[cfg(test)]
mod tests;
