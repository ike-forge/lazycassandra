//! The one place state changes.
//!
//! `update` is synchronous, pure, and total. It mutates state and *describes*
//! what should happen next; it cannot open a socket because it has nothing to
//! open one with (architecture.md §3).

use crate::clock::Stamp;
use crate::commands::{CommandsPanel, Run};
use crate::config::{Config, ConnectionConfig, Diagnostic};
use crate::contents::{ContentsState, Filter, SelectionMode, Subject, TableRef};
use crate::cql;
use crate::ddl::DdlTab;
use crate::drop::{DropPrompt, Target};
use crate::export::{Confirm, ExportForm, Fetched, Format, Pending, Scope, Stop};
use crate::grid::{self, ColumnScroll};
use crate::line::Line;
use crate::log::{Entry, Outcome, Subject as LogSubject, Trace};
use crate::model::{ConnId, DbError, JobId, Key, KeyPress, Page, QueryId, Row, Schema, Statement};
use crate::pointer::{Click, Mark, Pointer, Wheel};
use crate::ports::ExportError;
use crate::posture::Posture;
use crate::queries::{QueriesTab, Refresh};
use crate::search::{Direction, Origin, Prompt, Search, Searched, Sift};
use crate::state::{
    ConnectionState, Cursor, DiagnosticsPanel, Dropping, ExportJob, Flash, Geometry, Message,
    Overlay, Panel, Range, Ranges, SchemaState, Scroll, Selection, State,
};
use crate::style::Severity;
use crate::theme::{BuiltIn, ThemeChoice};
use crate::tree::Node;
use std::path::PathBuf;
use std::time::Duration;

/// How many rows one look at a table brings back.
///
/// A wall rather than a window until paging lands
/// (docs/superpowers/specs/2026-08-09-table-contents-design.md §3).
pub const ROW_LIMIT: usize = 200;

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
    },
    LoadSchema(ConnId),
    Execute {
        conn: ConnId,
        query: QueryId,
        stmt: Box<Statement>,
    },
    Cancel(QueryId),
    RecordHistory(Box<Statement>),
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
    Whole(Box<Statement>),
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
        Msg::PageReady { query, page, took } => {
            if state.in_flight.remove(&query).is_none() {
                return Vec::new();
            }
            state.log.complete(
                Trace::Query(query),
                Outcome::Succeeded {
                    detail: format!("{} rows", page.rows.len()),
                    took,
                },
            );
            // Answers whichever asked for it. A drop's result reaches the
            // panel too and is dropped there, because the panel is not
            // waiting on that id (drop design §3).
            state
                .contents
                .page_ready(query, *page, state.config.grid.max_column_width);
            state.mark_dirty();
            finish_drop(state, query, None)
        }

        // No overlay, unlike `SchemaFailed`: a schema failure disables the
        // application, a failed browse disables one panel, and an overlay per
        // mistyped table is the warning noise vision.md §8 flags.
        Msg::Failed { query, error, .. } => {
            if state.in_flight.remove(&query).is_none() {
                return Vec::new();
            }
            state.log.complete(
                Trace::Query(query),
                Outcome::Failed {
                    detail: error.to_string(),
                },
            );
            state.contents.failed(query);
            state.mark_dirty();
            finish_drop(state, query, Some(&error))
        }

        Msg::EditorClosed { buffer } => editor_closed(state, &buffer),

        Msg::EditorFailed { reason } => editor_failed(state, reason),

        Msg::ThemeSaveFailed { reason } => theme_save_failed(state, reason),

        Msg::ExportProgress { job, rows } => export_progress(state, job, rows),

        Msg::ExportFinished { job, result, took } => finish_export(state, job, result, took),

        Msg::Quit => quit(state),
    }
}

fn connected(state: &mut State, conn: ConnId, posture: Posture, took: Duration) -> Vec<Effect> {
    if !state.connection.is_current(conn) {
        return Vec::new();
    }
    state.connection = ConnectionState::Connected(crate::state::Connection {
        id: conn,
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

fn quit(state: &mut State) -> Vec<Effect> {
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

/// What the focused panel has under its cursor, as one line — which of the
/// panel's contents that is, and whether it is CSV, is the panel's answer.
/// Nothing under the cursor yanks nothing: there is nothing there to say
/// anything about (yank design §1).
///
/// A payload holding nothing but `,` and `\n` raises no effect either.
/// `Effect::Yank` becomes an OSC 52 sequence, and an empty payload asks the
/// terminal to clear its clipboard, so a projection with nothing but
/// delimiters to say must say nothing rather than wipe what the user had
/// copied (selection-modes design §4). Every projection converges here, so no
/// shape can reach the clipboard empty.
fn yank(state: &mut State) -> Vec<Effect> {
    let line = match state.focus() {
        Panel::Contents => contents(state),
        Panel::Tables => tree_nodes(state),
        Panel::Ddl => detail_rows(state),
        Panel::Queries => log_entries(state),
    };

    // A payload of nothing but delimiters says nothing, and `Effect::Yank`
    // becomes an OSC 52 sequence: an empty one asks the terminal to clear the
    // clipboard (design §5).
    let Some(line) = line.filter(|line| line.chars().any(|c| c != ',' && c != '\n')) else {
        // A `y` that said nothing has nothing to show and nothing to reset:
        // spending the selection here would punish the user for pressing `y`
        // on a null (visual mode design §5.1).
        return Vec::new();
    };

    state.flash = Some(lit(state));
    collapse_selection(state);

    vec![Effect::Yank(line)]
}

/// What `y` just took, ready to be lit once the selection that named it is
/// gone (visual mode design §6.1).
fn lit(state: &State) -> Flash {
    let (rows, columns) = match state.focus() {
        Panel::Contents => grid_extent(state),
        Panel::Tables => (
            spanned(&state.tables.selection, state.tables.cursor.index()),
            Ranges::default(),
        ),
        Panel::Ddl => (
            spanned(&state.ddl.selection, state.ddl.cursor.index()),
            Ranges::default(),
        ),
        // Only reached over the Logs tab: over Cql, `log_entries` yanks
        // nothing and `yank` has already returned by the time this is asked.
        Panel::Queries => (
            spanned(&state.log.selection, state.log.cursor.index()),
            Ranges::default(),
        ),
    };

    Flash::lit(state.focus(), rows, columns)
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

/// The rectangle `y` reads out of the grid, on each of its two axes.
///
/// Empty on an axis is the whole of it — the convention the view model's
/// `rows_ranges` and `columns_ranges` already carry, empty for `None`, and
/// what row mode and column mode take. The one place the mode decides which
/// axes it bounds, so what is copied and what is lit afterwards cannot answer
/// differently.
fn grid_extent(state: &State) -> (Ranges, Ranges) {
    let rows = spanned(&state.contents.rows, state.contents.cursor.index());
    let columns = spanned(&state.contents.columns, state.contents.column.index());

    match state.contents.mode {
        SelectionMode::Row => (rows, Ranges::default()),
        SelectionMode::Column => (Ranges::default(), columns),
        SelectionMode::Cell => (rows, columns),
    }
}

/// Which of the grid's two cursors `y` means is what the selection mode
/// answers (docs/superpowers/specs/2026-08-13-yank-selection-modes-design.md §1).
fn contents(state: &State) -> Option<String> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };
    // The rows as the grid draws them, not as the cluster returned them: a
    // yank copies what is on screen, so a filtered-out row is not in it and
    // a sorted grid copies in the order it reads
    // (grid sort and filter design §4).
    let drawn: Vec<Row> = page.rows().cloned().collect();
    let (rows, columns) = grid_extent(state);
    let whole = |len: usize| Ranges::of(Range::new(0, len.saturating_sub(1)));
    let rows = if rows.is_empty() {
        whole(drawn.len())
    } else {
        rows
    };
    let columns = if columns.is_empty() {
        whole(page.page().columns.len())
    } else {
        columns
    };

    match state.contents.mode {
        SelectionMode::Cell if rows.rows() == 1 && columns.rows() == 1 => {
            let row = drawn.get(rows.first()?.first())?;
            let value = row.values.get(columns.first()?.first())?;

            crate::yank::grid_cell(value)
        }
        SelectionMode::Row | SelectionMode::Cell => {
            Some(crate::yank::grid_block(&drawn, &rows, &columns))
        }
        SelectionMode::Column => Some(crate::yank::grid_columns(&drawn, &columns)),
    }
}

fn tree_nodes(state: &State) -> Option<String> {
    let schema = state.schema()?;
    let rows = state.tables.rows(schema);
    let selected = spanned(&state.tables.selection, state.tables.cursor.index());

    Some(crate::yank::lines(
        selected
            .indices()
            .filter_map(|index| rows.get(index))
            .map(crate::yank::node),
    ))
}

fn detail_rows(state: &State) -> Option<String> {
    let details = crate::view::details_of(state)?;
    let selected = spanned(&state.ddl.selection, state.ddl.cursor.index());
    let rows: Vec<_> = details.rows().collect();

    Some(crate::yank::lines(
        selected
            .indices()
            .filter_map(|index| rows.get(index))
            .map(|row| crate::yank::detail_row(row)),
    ))
}

/// `None` over the Cql tab — the editor claims `y` as text before `yank` is
/// ever called, so this is unreached there, but stated rather than assumed.
/// Over the Logs tab, each selected entry yanks as its two rows joined,
/// which is what makes it paste into a ticket (query log design §5.3).
fn log_entries(state: &State) -> Option<String> {
    if state.queries.tab() != QueriesTab::Logs {
        return None;
    }

    let entries: Vec<&Entry> = state.log.entries().collect();
    let selected = spanned(&state.log.selection, state.log.cursor.index());

    Some(crate::yank::lines(
        selected
            .indices()
            .filter_map(|index| entries.get(index).copied())
            .map(crate::yank::log_entry),
    ))
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

    // An open palette takes motion until a search is up, and text while one is
    // (docs/superpowers/specs/2026-08-16-overlay-search-design.md §5).
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return palette_key(state, press.key);
    }

    // The diagnostics overlay takes motion, not text — the same as the
    // palette only when no sift is up, and diagnostics has none to raise
    // (design §11).
    if matches!(state.overlay, Some(Overlay::Diagnostics(_))) {
        return diagnostics_key(state, press.key);
    }

    // The theme picker takes motion until a search is up, and text while one
    // is, the same as the palette
    // (docs/superpowers/specs/2026-08-16-overlay-search-design.md §5).
    if matches!(state.overlay, Some(Overlay::Themes { .. })) {
        return themes_key(state, press.key);
    }

    // The connection picker, on the same terms as the theme picker above.
    if matches!(state.overlay, Some(Overlay::Connections { .. })) {
        return connections_key(state, press.key);
    }

    // A prompt takes text, so it claims every printable key — the
    // panel-that-takes-text rule, applied to an overlay (design §1.1).
    if matches!(state.overlay, Some(Overlay::Search(_))) {
        return search_key(state, press.key);
    }

    // The form takes text on one of its four stops, so it claims every
    // printable key the way a prompt does — and hands most of them nowhere on
    // the other three (export design §2).
    if matches!(state.overlay, Some(Overlay::Export(_))) {
        return export_key(state, press.key);
    }

    // A modal that is one text field claims every printable key, the same
    // rule the prompt and the form follow
    // (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §3).
    if matches!(state.overlay, Some(Overlay::Filter(_))) {
        filter_key(state, press.key);
        return Vec::new();
    }

    // The confirmation claims every key it is answered with, and — on its
    // typed spelling — every printable key besides, the same rule the form
    // and the prompt follow above (drop design §2.1).
    if matches!(state.overlay, Some(Overlay::Drop(_))) {
        return drop_key(state, press.key);
    }

    // Focus is the mode. A panel that takes text claims every printable key
    // and the editing keys; what it does not claim keeps the global meaning
    // it has everywhere else (design §3). This is the rule shortcuts.md §2
    // said the CQL editor would have to break — no key is carved out of it,
    // its own number included: `WHERE id = 4` has to be typeable from inside
    // the buffer that takes text (docs/shortcuts.md §2).
    if state.takes_text() && edit(state, press.key) {
        return Vec::new();
    }

    match press.key {
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
        Some(Overlay::Themes { sift, .. } | Overlay::Connections { sift, .. }) => sift.is_some(),
        // `Fatal` is unreachable here — `handle_key` claims every press for
        // it before this is consulted — but written down anyway, rather than
        // grouped with `Message`/`None`, because neither it nor `Diagnostics`
        // has text of its own to type into.
        Some(Overlay::Diagnostics(_) | Overlay::Fatal(_)) => false,
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

    match &mut state.overlay {
        Some(Overlay::Commands(panel)) => match edge {
            Edge::Start => panel.to_start(),
            Edge::End => panel.to_end(),
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
        Some(Overlay::Message(_)) | None => scroll(state, edge.motion()),
        // The drop confirmation drives no list either, and `Fatal` never
        // reaches here for `takes_text`'s reason.
        Some(
            Overlay::Fatal(_)
            | Overlay::Search(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_),
        ) => {}
    }
}

/// Rows a wheel notch moves, in whatever unit the panel scrolls in. One is
/// sluggish enough that the pointer feels broken; three is what terminals
/// settled on (design §6).
const WHEEL_ROWS: usize = 3;

/// The one place a pointer gesture becomes behaviour, as `handle_key` is for
/// keys (docs/shortcuts.md §2).
///
/// Every gesture decomposes into a binding that already exists (design §1): a
/// click on a tree row is a selection followed by the `Enter` arm, and a wheel
/// notch is repeated motions.
fn handle_pointer(state: &mut State, pointer: Pointer) -> Vec<Effect> {
    state.mark_dirty();

    // The overlay is modal: it spends the click that dismisses it rather than
    // letting it act on a row it was covering (design §2). Routed through
    // `escape` rather than a bare `state.overlay = None`, so a click on an
    // open prompt restores `previous` instead of dropping it; with any other
    // overlay up, `escape` only dismisses it, so nothing else changes.
    if state.overlay.is_some() {
        if matches!(pointer, Pointer::Click(_)) {
            escape(state);
        }
        return Vec::new();
    }

    match pointer {
        Pointer::Click(click) => click_at(state, click),
        Pointer::Wheel { panel, direction } => {
            wheel(state, panel, direction);
            Vec::new()
        }
    }
}

/// Scrolls `panel`'s viewport without focusing it and without moving its
/// selection: the wheel is a look somewhere else, and a keypress returns you
/// (docs/superpowers/specs/2026-08-09-list-viewport-design.md §1).
fn wheel(state: &mut State, panel: Panel, direction: Wheel) {
    let len = scroll_len(state, panel);
    let height = state.geometry.height(panel);

    let Some(scroll) = panel_scroll(state, panel) else {
        return;
    };

    for _ in 0..WHEEL_ROWS {
        match direction {
            Wheel::Down => scroll.down(len, height),
            Wheel::Up => scroll.up(),
        }
    }
}

fn click_at(state: &mut State, click: Click) -> Vec<Effect> {
    state.focus_on(click.panel());

    // A press names one row, so it spends the range rather than standing
    // beside it and making the next `y` lie (design §4).
    for axis in axes(state, click.panel()).into_iter().flatten() {
        axis.selection.clear();
    }

    let (panel, row, column) = match click {
        Click::Row { panel, row, column } => (panel, Some(row), column),
        Click::Column { panel, column } => (panel, None, Some(column)),
        // A mark is the `s`/`f` binding reached by pointer: it names its own
        // column, so it moves the cursor there and acts, whatever mode the
        // grid is in — a press says which column it means, where a key has
        // only the cursor to go on (design §5).
        Click::Mark {
            panel,
            column,
            mark,
        } => {
            if panel == Panel::Contents && column < state.contents.column_count() {
                state
                    .contents
                    .column
                    .go_to(column, state.contents.column_count());

                match mark {
                    Mark::Sort => state.contents.cycle_sort(column),
                    Mark::Filter => open_filter(state),
                }
            }
            return Vec::new();
        }
        // A tab is a place, and pressing it is how one is reached: this is
        // the `[`/`]` arm of `handle_key`, reached by pointer (design §1).
        Click::Tab { panel, tab } => {
            show_tab(state, panel, tab);
            return Vec::new();
        }
        Click::Panel(_) => return Vec::new(),
    };

    // Neither cursor changes the selection mode. A press names whatever cells
    // it can, and the mode is what decides which of them the grid draws a band
    // around — `m` is still the only way to change that.
    if let Some(column) = column {
        let columns = state.contents.column_count();
        if panel == Panel::Contents && column < columns {
            state.contents.column.go_to(column, columns);
        }
    }

    let Some(row) = row else {
        return Vec::new();
    };
    let len = row_count(state, panel);

    // The map a press was resolved against describes the *previous* frame. A
    // row that has gone since is dropped rather than clamped onto whatever
    // took its place: in Tables, clamping would read a table nobody pointed at
    // (design §6).
    if row >= len {
        return Vec::new();
    }

    match panel {
        Panel::Tables => {
            state.tables.cursor.go_to(row, len);
            activate(state)
        }
        Panel::Contents => {
            state.contents.cursor.go_to(row, len);
            Vec::new()
        }
        // The offset the viewport owes this cursor is `Msg::Measured`'s to
        // compute, exactly as it is after `j`: only the renderer knows which
        // lines the row wrapped to.
        Panel::Ddl => {
            state.ddl.cursor.go_to(row, len);
            Vec::new()
        }
        // Queries is an editor: it has no rows for a press to land on.
        Panel::Queries => Vec::new(),
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
/// press by staying where it is.
fn focus_by_number(state: &mut State, key: char) {
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
/// the DDL panel offers one tab, and walking stays on it. Queries has no
/// strip a key can reach this way: `4` is text once its CQL tab has focus,
/// the same as any other digit, and the Logs tab is reached by walking
/// `Tab`/`BackTab`'s cycle of stops (see [`Landing`]) or by a click.
fn walk_tab(state: &mut State, step: Step) {
    if state.focus() != Panel::Ddl {
        return;
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
/// Every `(panel, tab)` pair a strip can draw is a stop, DDL's two and
/// Queries' two included, so the trail is reachable without spending a
/// panel's own number on it — the number stays text where the CQL buffer is
/// what has focus (docs/shortcuts.md §2).
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

/// Puts back the theme a live picker opened on, if the overlay is that
/// picker. Otherwise a no-op. Three sites unwind a preview: [`escape`], which
/// does it while taking the overlay apart, the resize-shrink close in
/// [`measured`], and [`unwind_overlay`] — which every replacement from a
/// message shares, so no such site can copy the check and diverge from the
/// rest.
fn restore_theme(state: &mut State) {
    if let Some(Overlay::Themes { restored, .. }) = &state.overlay {
        state.theme = restored.clone();
    }
}

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
fn cancel_pending_export(state: &mut State) {
    let job = match &state.overlay {
        Some(Overlay::Export(form)) => form.pending().map(Pending::job),
        _ => None,
    };
    if let Some(job) = job {
        state.log.complete(Trace::Export(job), Outcome::Cancelled);
    }
}

/// Where `target`'s cursor is now.
fn origin_of(state: &State, target: Searched) -> Origin {
    match target {
        Searched::Tables => Origin {
            row: state.tables.cursor.index(),
            column: 0,
        },
        Searched::Ddl => Origin {
            row: state.ddl.cursor.index(),
            column: 0,
        },
        Searched::Contents => Origin {
            row: state.contents.cursor.index(),
            column: state.contents.column.index(),
        },
        Searched::Queries => Origin {
            row: state.log.cursor.index(),
            column: 0,
        },
    }
}

/// Puts `target`'s cursor back where it was.
fn restore_origin(state: &mut State, target: Searched, origin: Origin) {
    match target {
        Searched::Tables => {
            let len = tree_rows(state);
            state.tables.cursor.go_to(origin.row, len);
        }
        Searched::Ddl => {
            let len = crate::view::ddl_rows(state);
            state.ddl.cursor.go_to(origin.row, len);
        }
        Searched::Contents => {
            let rows = state.contents.row_count();
            let columns = state.contents.column_count();
            state.contents.cursor.go_to(origin.row, rows);
            state.contents.column.go_to(origin.column, columns);
        }
        Searched::Queries => {
            let len = state.log.len();
            state.log.cursor.go_to(origin.row, len);
        }
    }
}

/// Moves `target`'s cursor to the next match from `from`, and falls back to
/// `from` itself when nothing matches (design §5.2) — a no-op for `n`/`N`,
/// which always call with `from` already the cursor, and the promised
/// restore to `origin` for a live `/` prompt whose term stops matching.
fn jump_to_match(state: &mut State, target: Searched, from: Origin, direction: Direction) {
    match target {
        Searched::Tables => {
            let row = state
                .schema()
                .and_then(|schema| state.tables.next_match(schema, from.row, direction));
            match row {
                Some(row) => {
                    let len = tree_rows(state);
                    state.tables.cursor.go_to(row, len);
                }
                None => restore_origin(state, target, from),
            }
        }
        Searched::Ddl => match crate::view::ddl_next_match(state, from.row, direction) {
            Some(row) => {
                let len = crate::view::ddl_rows(state);
                state.ddl.cursor.go_to(row, len);
            }
            None => restore_origin(state, target, from),
        },
        Searched::Contents => {
            let hit = state
                .contents
                .next_match((from.row, from.column), direction);
            match hit {
                Some((row, column)) => {
                    let rows = state.contents.row_count();
                    let columns = state.contents.column_count();
                    state.contents.cursor.go_to(row, rows);
                    state.contents.column.go_to(column, columns);
                }
                None => restore_origin(state, target, from),
            }
        }
        Searched::Queries => match state.log.next_match(from.row, direction) {
            Some(row) => {
                let len = state.log.len();
                state.log.cursor.go_to(row, len);
            }
            None => restore_origin(state, target, from),
        },
    }
}

/// `n` and `N` walk the focused panel's matches, wrapping.
///
/// Motions, so they take the path `j` and `k` take and an extending range
/// follows them. Inert where the panel holds no search — a panel that cannot
/// honour a motion swallows it (docs/shortcuts.md §2) — and unreachable from
/// Queries' Cql tab, which takes text and never sees this key at all; its
/// Logs tab holds a search like any other list (design §5.3).
fn next_match_key(state: &mut State, direction: Direction) {
    let Some(target) = Searched::of(state.focus()) else {
        return;
    };

    if state.search(target).is_none() {
        return;
    }

    let from = origin_of(state, target);
    jump_to_match(state, target, from, direction);
}

/// `/` opens a prompt over the focused panel. Unreachable over Queries' Cql
/// tab, since the editor claims `/` as text before this is ever called
/// (panel search design §8); reachable over its Logs tab, which holds a
/// search like any other list (query log design §5.3).
///
/// Clears the target's search immediately rather than leaving `previous`
/// showing until the first keystroke: the buffer opens empty, and live
/// search (design §3.3) means the panel underneath is the preview of
/// whatever the buffer holds, from the first frame the prompt is up.
fn open_prompt(state: &mut State) {
    let Some(target) = Searched::of(state.focus()) else {
        return;
    };

    let previous = state.search(target).cloned();
    let origin = origin_of(state, target);
    state.overlay = Some(Overlay::Search(Prompt::opened(target, previous, origin)));
    state.apply_search(target, None);
}

/// What an open prompt responds to.
///
/// Every printable key is a character, so `q` types a `q` — the opposite of
/// the palette, which leaves `q` quitting. The two differ because the palette
/// takes motion and this takes text (panel search design §1.1, §3.1).
///
/// `Escape` is decided before the prompt's mutable borrow is taken: it defers
/// to [`escape`], which needs `&mut State` whole, and no reborrow of it can
/// coexist with `prompt`. `Enter` stays inside the match because it only needs
/// `&mut state.overlay`, and `prompt` is dead in that arm.
fn search_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let Some(Overlay::Search(prompt)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => prompt.push(character),
        Key::Backspace => prompt.pop(),
        Key::Enter => {
            state.overlay = None;
            return Vec::new();
        }
        _ => return Vec::new(),
    }

    let (target, search, origin) = (prompt.target(), prompt.search(), prompt.origin());
    state.apply_search(target, search);
    jump_to_match(state, target, origin, Direction::Forwards);

    Vec::new()
}

/// `Esc` resolves in one order, first hit wins: a live sift inside an
/// overlay; then the overlay itself; then freezing an extending range; then
/// clearing a frozen range; then the focused panel's search; then the panel
/// itself, which hands focus back to Tables. One press does one of the six,
/// never two — the thing most recently put on screen comes off first (panel
/// search design §6, overlay search design §1).
///
/// The last rung is every panel, not just the one that takes text: a key that
/// backs out of Queries but sits inert in DDL beside it is the same key
/// meaning two things (docs/shortcuts.md §2). On Tables it is already the
/// last rung's destination, so it does nothing there.
fn escape(state: &mut State) {
    // Read before the `take()` below empties it: a click is the one path
    // that reaches `Overlay::Export`'s arm here rather than `export_key`,
    // which claims keyboard `Esc` itself (final review, I4 follow-up).
    cancel_pending_export(state);

    match state.overlay.take() {
        Some(Overlay::Search(prompt)) => {
            state.apply_search(prompt.target(), prompt.previous().cloned());
            restore_origin(state, prompt.target(), prompt.origin());
            return;
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
            return;
        }
        Some(Overlay::Themes { restored, .. }) => {
            state.theme = restored;
            return;
        }
        Some(Overlay::Connections {
            cursor,
            sift: Some(sift),
        }) => {
            let mut cursor = cursor;
            cursor.go_to(sift.origin(), state.config.connections.len());
            state.overlay = Some(Overlay::Connections { cursor, sift: None });
            return;
        }
        // A dismissed picker leaves `Disconnected` rather than `Unchosen`, so
        // `offer_connections` does not put it straight back up: waving the
        // picker away is a choice, and the panels are allowed to stay empty
        // (connection picker design §4).
        Some(Overlay::Connections { .. }) => {
            if matches!(state.connection, ConnectionState::Unchosen) {
                state.connection = ConnectionState::Disconnected;
            }
            return;
        }
        Some(Overlay::Commands(mut panel)) if panel.sift().is_some() => {
            panel.restore_origin();
            state.overlay = Some(Overlay::Commands(panel));
            return;
        }
        // The filter prompt applies nothing on the way out: `esc` leaves the
        // column filtered exactly as it was, and `⏎` is the one key that
        // writes (grid sort and filter design §3).
        Some(
            Overlay::Message(_)
            | Overlay::Commands(_)
            | Overlay::Diagnostics(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_),
        ) => {
            // The diagnostics modal is what claims startup's first frame when
            // the config had anything wrong with it; closing it is where the
            // picker underneath finally gets its turn.
            offer_connections(state);
            return;
        }
        // Not dismissable: `handle_key` claims `Esc` for `Fatal` before this
        // function is reached, so the only caller left is a click routed
        // through here by `handle_pointer`, which must not spend itself on
        // ending the modal (query log design §7).
        Some(fatal @ Overlay::Fatal(_)) => {
            state.overlay = Some(fatal);
            return;
        }
        None => {}
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

/// `?` opens the palette, replacing whatever overlay was up.
///
/// Refused where the frame cannot draw one: opening a palette nothing draws
/// leaves the user in a mode with nothing on screen saying so, wondering why
/// `j` stopped moving the tree (design §5.1).
fn open_palette(state: &mut State) {
    if state.geometry.command_rows() == 0 {
        return;
    }

    state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));
}

/// What an open palette responds to.
///
/// `q` still quits, because a key that means "quit" everywhere except in one
/// overlay is not a key anyone can trust without looking. Everything the
/// palette does not use is inert rather than dismissing, so no letter means
/// "close" in one frame and something else in the next (design §7).
fn palette_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(&state.overlay, Some(Overlay::Commands(panel)) if panel.sift().is_some()) {
        return palette_sift_key(state, key);
    }

    let height = state.geometry.command_rows();

    match key {
        Key::Char('q') => return quit(state),
        Key::Char('?') | Key::Escape => {
            state.overlay = None;
            return Vec::new();
        }
        Key::Enter => return run_selected(state),
        _ => {}
    }

    let Some(Overlay::Commands(panel)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => panel.down(),
        Key::Char('k') | Key::Up => panel.up(),
        Key::PageDown => panel.page_down(height),
        Key::PageUp => panel.page_up(height),
        Key::Home => panel.to_start(),
        Key::End => panel.to_end(),
        Key::Char('/') => panel.open_sift(),
        _ => {}
    }

    Vec::new()
}

/// What a palette with a live sift responds to.
///
/// Text, for the reason [`themes_sift_key`] takes text: an overlay with a
/// prompt up is a panel that takes text (overlay search design §5).
fn palette_sift_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let Some(Overlay::Commands(panel)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => panel.push(character),
        Key::Backspace => panel.pop(),
        Key::Enter => panel.close_sift(),
        _ => {}
    }

    Vec::new()
}

/// `⏎`: close the palette, then perform the row the cursor is on.
///
/// The row's own first key, pressed through [`handle_key`], is what performs
/// it. A table of actions here would be the second dispatch point
/// docs/shortcuts.md §2 forbids, and a row that ran something other than what
/// its key runs is the "bar that lies" one level down
/// (docs/superpowers/specs/2026-08-15-palette-execution-design.md §2).
///
/// The recursion terminates: the overlay is gone before the call, so
/// `handle_key`'s palette arm cannot be re-entered on this pass.
fn run_selected(state: &mut State) -> Vec<Effect> {
    let Some(Overlay::Commands(open)) = &state.overlay else {
        return Vec::new();
    };
    let command = open.command();
    state.overlay = None;

    let Some(command) = command else {
        return Vec::new();
    };

    match command.run {
        Run::Inert => return Vec::new(),
        Run::In(panel) => state.focus_on(panel),
        Run::Here => {}
    }

    let Some(stroke) = command.binding.keys.first().copied() else {
        return Vec::new();
    };

    handle_key(state, stroke.press())
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

/// Where `choice` sits in the picker's list: the built-ins, then the
/// config's own, in config order — the same order [`crate::view`] labels it.
fn theme_picker_index(config: &Config, choice: &ThemeChoice) -> Option<usize> {
    BuiltIn::ALL
        .iter()
        .position(|built_in| ThemeChoice::from(*built_in) == *choice)
        .or_else(|| {
            config
                .themes
                .iter()
                .position(
                    |theme| matches!(choice, ThemeChoice::Custom(name) if *name == theme.name),
                )
                .map(|index| index + BuiltIn::ALL.len())
        })
}

/// The choice the picker's `index`th row names, or `None` past the end of
/// the built-ins and the config's own.
fn theme_at_picker_index(config: &Config, index: usize) -> Option<ThemeChoice> {
    BuiltIn::ALL
        .get(index)
        .map(|built_in| ThemeChoice::from(*built_in))
        .or_else(|| {
            config
                .themes
                .get(index - BuiltIn::ALL.len())
                .map(|theme| ThemeChoice::Custom(theme.name.clone()))
        })
}

/// The picker's rows in picker order: the built-ins, then the config's own.
///
/// The one builder for this order, shared with [`crate::view`]'s
/// `OverlayView::Themes`: the search's whole correctness claim is that
/// `theme_labels[i]` is the label the renderer draws at row `i`, so the two
/// cannot be allowed to drift into separate copies (review, item 2).
pub(crate) fn theme_labels(config: &Config) -> Vec<String> {
    BuiltIn::ALL
        .iter()
        .map(|built_in| built_in.label().to_owned())
        .chain(
            config
                .themes
                .iter()
                .map(|theme| theme.name.as_str().to_owned()),
        )
        .collect()
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

/// `ctrl-t` opens the theme picker, seated on the theme in force.
///
/// Refused where the frame cannot draw the list, for the reason
/// [`open_palette`] gives. Gated on the picker's own room, [`Geometry::theme_rows`],
/// rather than the palette's: the picker used to be the smaller block always,
/// but the list is every built-in plus every theme a config defines, and a
/// long enough custom list no longer fits where a palette would (design §9).
///
/// Refused over a live prompt too, which no unmodified key needs to say: the
/// ctrl layer is dispatched above every overlay, so this is the one opener a
/// prompt cannot swallow, and replacing one would strand the panel under a
/// half-typed query with nothing left holding the search it had.
///
/// A picker's own live sift needs no matching guard here: `ctrl_key` already
/// refuses every ctrl chord over any open overlay, `Overlay::Themes { .. }`
/// included, before this function is ever called, so a sift up inside the
/// picker is covered by that blanket refusal, not this one.
fn open_themes(state: &mut State) {
    let last = BuiltIn::ALL.len() + state.config.themes.len();
    let busy = matches!(state.overlay, Some(Overlay::Search(_)));
    if last > state.geometry.theme_rows() || busy {
        return;
    }

    let mut cursor = Cursor::default();
    let index = theme_picker_index(&state.config, &state.theme).unwrap_or(0);
    cursor.go_to(index, last);

    state.overlay = Some(Overlay::Themes {
        cursor,
        restored: state.theme.clone(),
        sift: None,
    });
}

/// What an open picker responds to.
///
/// Moving the cursor sets the theme, so the frame underneath is the preview —
/// the same live-as-you-type rule the search prompt follows (panel search
/// design §1).
///
/// `q` still quits, for the reason [`palette_key`] gives. The ends are
/// reachable here as they are in every other list, `home`/`end` and `g`/`G`
/// alike: paging does land on either end in one press in a list this short,
/// but a hand that types `G` in the panels and gets nothing in the picker has
/// found the one list that is different, which costs more than the arm
/// (docs/shortcuts.md §1).
fn themes_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(state.overlay, Some(Overlay::Themes { sift: Some(_), .. })) {
        return themes_sift_key(state, key);
    }

    match key {
        Key::Char('q') => return quit(state),
        Key::Enter => {
            let chosen = state.theme.clone();
            let changed = match state.overlay.take() {
                Some(Overlay::Themes { restored, .. }) => restored != chosen,
                _ => false,
            };

            return if changed {
                vec![Effect::SaveTheme(chosen)]
            } else {
                Vec::new()
            };
        }
        Key::Escape => {
            escape(state);
            return Vec::new();
        }
        _ => {}
    }

    let last = BuiltIn::ALL.len() + state.config.themes.len();
    let height = state.geometry.command_rows();
    let Some(Overlay::Themes { cursor, sift, .. }) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => cursor.down(last),
        Key::Char('k') | Key::Up => cursor.up(),
        Key::PageDown => cursor.page_down(last, height),
        Key::PageUp => cursor.page_up(height),
        Key::Home => cursor.to_start(),
        Key::End => cursor.to_end(last),
        Key::Char('/') => *sift = Some(Sift::opened(cursor.index())),
        _ => {}
    }

    preview_theme(state);

    Vec::new()
}

/// The picker's cursor is the preview: the theme it sits on is the theme in
/// force, and every motion that moves it says so (themes design §6).
///
/// The cursor counts in the rows a live sift left, so the row it holds is
/// resolved through those before the config is asked for a theme. A term that
/// matched nothing holds no row, and the preview stays where it was.
fn preview_theme(state: &mut State) {
    let Some(Overlay::Themes { cursor, sift, .. }) = &state.overlay else {
        return;
    };
    let labels = theme_labels(&state.config);
    let search = sift.as_ref().and_then(Sift::search);
    let choice = picker_matches(&labels, search.as_ref())
        .get(cursor.index())
        .copied()
        .and_then(|at| theme_at_picker_index(&state.config, at));

    if let Some(choice) = choice {
        state.theme = choice;
    }
}

/// What a picker with a live sift responds to.
///
/// The overlay takes text, so the letters that move it or quit type
/// themselves — the panel-that-takes-text rule, applied to an overlay
/// (overlay search design §5). `Escape` defers to [`escape`], which needs
/// `&mut State` whole, so it is decided before the overlay is borrowed —
/// [`search_key`]'s reason.
fn themes_sift_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let labels = theme_labels(&state.config);
    let Some(Overlay::Themes { cursor, sift, .. }) = &mut state.overlay else {
        return Vec::new();
    };
    let Some(open) = sift else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => open.push(character),
        Key::Backspace => {
            // An empty query is no search, so the list the cursor is counting
            // in is already the whole one: closing here moves nothing.
            if !open.pop() {
                *sift = None;
                return Vec::new();
            }
        }
        Key::Enter => {
            let at = picker_matches(&labels, open.search().as_ref())
                .get(cursor.index())
                .copied();
            *sift = None;

            if let Some(index) = at {
                cursor.go_to(index, labels.len());
            }
            return Vec::new();
        }
        _ => return Vec::new(),
    }

    // The first surviving row at or beyond the one the prompt opened on, and
    // the last one when the term kept nothing beyond it (overlay filter
    // design §1).
    let origin = open.origin();
    let matches = picker_matches(&labels, open.search().as_ref());
    let seat = matches
        .iter()
        .position(|at| *at >= origin)
        .unwrap_or_else(|| matches.len().saturating_sub(1));
    cursor.go_to(seat, matches.len());

    preview_theme(state);

    Vec::new()
}

/// The picker's rows in picker order, which is the order the merge defined:
/// global entries first, then local-only ones (configuration design §5).
///
/// The one builder for that order, shared with [`crate::view`]'s
/// `OverlayView::Connections`, for the reason [`theme_labels`] is shared —
/// the search's correctness claim is that `connection_names[i]` names the row
/// the renderer draws at `i`.
pub(crate) fn connection_names(config: &Config) -> Vec<String> {
    config
        .connections
        .iter()
        .map(|connection| connection.name.as_str().to_owned())
        .collect()
}

/// The connection `default_connection` names, as an index into
/// [`connection_names`].
fn default_connection_index(config: &Config) -> Option<usize> {
    let name = config.default_connection.as_ref()?;

    config
        .connections
        .iter()
        .position(|connection| connection.name == *name)
}

/// Opens the connection picker, seated on the default connection when there
/// is one and the first entry otherwise.
///
/// Refused where the frame cannot draw the list, for [`open_themes`]'s
/// reason, and over a live prompt for the same reason it is — with one
/// exception: a picker with *nothing* to list is guidance rather than a list,
/// and is opened whatever the row count, because a user with no connections
/// is exactly the user who most needs telling why the panels are empty
/// (connection picker design §3).
fn open_connections(state: &mut State) {
    let last = state.config.connections.len();
    let busy = matches!(state.overlay, Some(Overlay::Search(_)));
    // The guidance an empty list draws is a block the size of a message, so
    // it is gated on the signal every other modal is gated on rather than on
    // rows it does not have (design §3, §7).
    let fits = if last == 0 {
        state.geometry.command_rows() > 0
    } else {
        last <= state.geometry.connection_rows()
    };

    if busy || !fits {
        return;
    }

    let mut cursor = Cursor::default();
    cursor.go_to(default_connection_index(&state.config).unwrap_or(0), last);

    state.overlay = Some(Overlay::Connections { cursor, sift: None });
}

/// What an open picker responds to.
///
/// Unlike [`themes_key`] a motion previews nothing: a connection is reached
/// by opening it, and opening one is a round trip to a cluster rather than a
/// repaint. So `⏎` is the only key that acts, and `esc` leaves the connection
/// as it was (connection picker design §3).
fn connections_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(
        state.overlay,
        Some(Overlay::Connections { sift: Some(_), .. })
    ) {
        return connections_sift_key(state, key);
    }

    match key {
        Key::Char('q') => return quit(state),
        Key::Enter => return choose_connection(state),
        // Nothing to search when there is nothing listed: the bar names no
        // `/` on an empty picker, and the key must agree with it.
        Key::Char('/') if state.config.connections.is_empty() => return Vec::new(),
        Key::Escape => {
            escape(state);
            return Vec::new();
        }
        _ => {}
    }

    let last = state.config.connections.len();
    let height = state.geometry.connection_rows();
    let Some(Overlay::Connections { cursor, sift }) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => cursor.down(last),
        Key::Char('k') | Key::Up => cursor.up(),
        Key::PageDown => cursor.page_down(last, height),
        Key::PageUp => cursor.page_up(height),
        Key::Home => cursor.to_start(),
        Key::End => cursor.to_end(last),
        Key::Char('/') => *sift = Some(Sift::opened(cursor.index())),
        _ => {}
    }

    Vec::new()
}

/// What a picker with a live sift responds to — [`themes_sift_key`]'s
/// contract, over a list that previews nothing.
fn connections_sift_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let names = connection_names(&state.config);
    let Some(Overlay::Connections { cursor, sift }) = &mut state.overlay else {
        return Vec::new();
    };
    let Some(open) = sift else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => open.push(character),
        Key::Backspace => {
            if !open.pop() {
                *sift = None;
                return Vec::new();
            }
        }
        // `⏎` closes the prompt onto the row it left the cursor on rather
        // than connecting: a term typed to find an entry should not also open
        // it, when the entry the cursor happens to hold may not be the one
        // the user was still narrowing towards (connection picker design §5).
        Key::Enter => {
            // A term that matched nothing holds no row, so the cursor goes
            // back where the prompt opened rather than staying on the row it
            // was pushed to — the same landing `Esc` makes. Leaving it would
            // seat the cursor on an entry the filter had hidden, and the next
            // `⏎` would open a connection the user never saw
            // (connection picker design §5).
            let at = picker_matches(&names, open.search().as_ref())
                .get(cursor.index())
                .copied()
                .unwrap_or_else(|| open.origin());
            *sift = None;
            cursor.go_to(at, names.len());
            return Vec::new();
        }
        _ => return Vec::new(),
    }

    let origin = open.origin();
    let matches = picker_matches(&names, open.search().as_ref());
    let seat = matches
        .iter()
        .position(|at| *at >= origin)
        .unwrap_or_else(|| matches.len().saturating_sub(1));
    cursor.go_to(seat, matches.len());

    Vec::new()
}

/// Connects to the row the picker's cursor holds.
///
/// The cursor counts in the rows a live sift left, so it is resolved through
/// those before the config is asked — [`preview_theme`]'s rule, applied to a
/// list where landing on the wrong row costs a connection rather than a
/// repaint. A term that matched nothing holds no row and connects to nothing.
fn choose_connection(state: &mut State) -> Vec<Effect> {
    let names = connection_names(&state.config);
    let Some(Overlay::Connections { cursor, sift }) = &state.overlay else {
        return Vec::new();
    };
    let search = sift.as_ref().and_then(Sift::search);
    let Some(target) = picker_matches(&names, search.as_ref())
        .get(cursor.index())
        .copied()
        .and_then(|at| state.config.connections.get(at))
        .cloned()
    else {
        return Vec::new();
    };

    state.overlay = None;

    connect(state, target)
}

/// Begins a connection to `target`, whatever the state was before.
///
/// Shared by the startup path and the picker: an id is minted here so a
/// result that arrives after the user has moved on can be dropped by
/// `is_current`, which is the whole reason [`ConnId`] is carried
/// (architecture.md §3).
fn connect(state: &mut State, target: ConnectionConfig) -> Vec<Effect> {
    let conn = ConnId::next();

    state.connection = ConnectionState::Connecting(conn);
    state.log.push(Entry::new(
        state.now,
        sentence(format!("Connecting to {}", target.name.as_str())),
        Outcome::Pending {
            trace: Trace::Connect(conn),
        },
    ));

    vec![Effect::Connect {
        conn,
        target: Box::new(target),
    }]
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
                | Overlay::Diagnostics(_)
                | Overlay::Export(_)
                | Overlay::Filter(_)
                | Overlay::Drop(_)
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

/// Refused over a grid with no rows, and while an export is already running:
/// one at a time (design §5).
///
/// Refused too where the frame cannot draw the form, for the reason
/// [`open_palette`] gives: the form claims every key while it is open, so one
/// nothing draws is a mode with nothing on screen saying so and only `Esc` out
/// of it. Gated on the form's own measurement rather than on the palette's,
/// which is the weaker condition — a frame that draws the palette need not
/// draw a 56×11 block.
fn open_export(state: &mut State) {
    if state.export.is_some() || !state.geometry.export_fits() {
        return;
    }

    let ContentsState::Loaded { subject, page } = state.contents.state() else {
        return;
    };

    let rows = page.len();
    if rows == 0 {
        return;
    }

    let form = ExportForm::opened(
        subject.clone(),
        Fetched::of_drawn(page.page().rows.len(), rows),
        &state.cwd,
    );
    state.overlay = Some(Overlay::Export(form));
    state.dirty = true;
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

/// Every key the export form claims.
///
/// A confirmation in front of the form is resolved first, through
/// [`confirm_key`].
fn export_key(state: &mut State, key: Key) -> Vec<Effect> {
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

/// Turns the form into the one effect that does the work.
///
/// A confirmation re-emits the source it is holding rather than reading the
/// panel again: the panel is live for the whole of an export, and by the time
/// `Exists` comes back the user may be standing on another table (final review
/// I3).
fn start_export(state: &mut State) -> Vec<Effect> {
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
            Source::Whole(stmt) => statement_subject(stmt),
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
    }]
}

/// What the chosen scope reads.
///
/// `None` when the panel is no longer showing a page — which the form's own
/// key claim makes impossible today, and which must not become a panic if it
/// ever stops being.
fn source(state: &State, form: &ExportForm) -> Option<Source> {
    let ContentsState::Loaded { subject, page } = state.contents.state() else {
        return None;
    };

    match form.scope() {
        // Ours is the limit that is dropped; a typed statement's own is the
        // user's and is honoured verbatim (design §3).
        Scope::Whole => Some(Source::Whole(Box::new(match subject {
            Subject::Table(table) => Statement::select_everything(&table.keyspace, &table.table),
            Subject::Typed(cql) => Statement::typed(cql),
        }))),
        // What the grid is showing, arrangement and all: a form opened over
        // a filtered grid offering to write the rows behind the filter would
        // be exporting something the user cannot see
        // (grid sort and filter design §4).
        Scope::Fetched => Some(Source::Fetched {
            columns: page.page().columns.clone(),
            rows: page.rows().cloned().collect(),
        }),
    }
}

/// The overwrite confirmation's keys: `Space` or `Enter` confirms, `Esc` goes
/// back to the form (design §7).
fn confirm_key(state: &mut State, key: Key) -> Vec<Effect> {
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
fn export_progress(state: &mut State, job: JobId, rows: usize) -> Vec<Effect> {
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
fn finish_export(
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
            raise(
                state,
                Message {
                    title: "Exported".to_owned(),
                    message: format!("wrote {rows} rows to {}", finished.destination().display()),
                    severity: Severity::Info,
                },
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

/// What `d` would aim at in this frame, or `None` where it would open
/// nothing.
///
/// One derivation, read by the key that opens the confirmation and by the
/// hint bar that greys it (`view::hints::liveness`), so the bar cannot
/// promise a question the key refuses (drop design §1).
///
/// It answers only what the *cursor* names. The refusals that are about the
/// session rather than about the cursor — the posture, the frame, a drop
/// already running — are [`open_drop`]'s, because the bar greys the first two
/// and shows the third as a running job.
pub(crate) fn drop_target(state: &State) -> Option<Target> {
    match state.focus() {
        Panel::Tables => tree_target(state),
        Panel::Contents => rows_target(state),
        Panel::Ddl | Panel::Queries => None,
    }
}

/// The node under the tree cursor. Not the range: `v` selects rows for `y`,
/// and "drop these four" is a different question with a different
/// confirmation (design §1).
fn tree_target(state: &State) -> Option<Target> {
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

/// The grid's selected rows, addressed by their primary key — `None` where
/// that key is not known or not present (design §3.1).
///
/// The range, here, and not only the cursor: the tree's `v` selects rows of a
/// list and a delete of several is a different statement, while the grid's
/// `v` selects the rows a command acts on and `y` already reads it that way
/// (yank selection modes design §1). Whole rows in every selection mode — a
/// column range addresses no row, and a cell is not a thing `DELETE` removes.
fn rows_target(state: &State) -> Option<Target> {
    let ContentsState::Loaded { subject, page } = state.contents.state() else {
        return None;
    };
    // A typed statement names its table in text this program will not trust
    // for a `WHERE` clause it writes itself (design §3.1).
    let Subject::Table(table) = subject else {
        return None;
    };
    let described = state
        .schema()?
        .keyspaces
        .get(&table.keyspace)?
        .tables
        .get(&table.table)?;

    let page = page.page();
    let selected = spanned(&state.contents.rows, state.contents.cursor.index());
    let rows: Vec<&Row> = selected
        .indices()
        .map(|index| page.rows.get(index))
        .collect::<Option<Vec<_>>>()?;

    Target::rows(table, described, &page.columns, rows.into_iter())
}

/// `d` opens the confirmation over whatever [`drop_target`] names.
///
/// Refused, silently, wherever the question it would ask cannot be answered:
/// a frame with no room to draw the block, no schema to name a node from, a
/// drop already in flight, and a range of more than one row — "drop these
/// four" is a different question with a different confirmation (drop design
/// §1). Those are greyed on the hint bar, so the refusal is visible before
/// the key is pressed (`view::hints::liveness`).
///
/// The posture is the one refusal that is *not* silent. It is greyed too, but
/// a key that is grey because of how the connection was opened teaches
/// nothing about which connection that is, and a user who pressed it anyway
/// is owed the reason (posture design §3).
fn open_drop(state: &mut State) {
    if !state.posture().permits_mutation() {
        refuse_read_only(state, "drop and delete");
        return;
    }

    if state.dropping.is_some() || !state.geometry.drop_fits() {
        return;
    }

    let Some(target) = drop_target(state) else {
        return;
    };

    state.overlay = Some(Overlay::Drop(DropPrompt::opened(target, state.posture())));
    state.dirty = true;
}

/// Every key the drop confirmation claims.
///
/// The two spellings answer differently and this is the one place that says
/// so: `y` runs the yes/no question, and `⏎` runs the typed one only once the
/// name matches — a keystroke landing in a field that does not yet say what
/// it was asked to say must not drop anything (drop design §2.1).
fn drop_key(state: &mut State, key: Key) -> Vec<Effect> {
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

/// The yes/no spelling. Everything it does not use is inert rather than
/// dismissing, the rule the palette already follows (command palette design
/// §7).
const fn yes_no_key(key: Key) -> Decision {
    match key {
        Key::Char('y') => Decision::Run,
        Key::Char('n') | Key::Escape => Decision::Close,
        _ => Decision::Nothing,
    }
}

/// The typed spelling. The catch-all is last and unguarded: every printable
/// key is text here, `n` included — a keyspace named `nation` could not be
/// typed otherwise (drop design §2.1).
fn typed_key(prompt: &mut DropPrompt, key: Key) -> Decision {
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

/// Turns the answered confirmation into the statement it was asking about.
///
/// `submit` is where the posture guard lives, so [`open_drop`]'s own gate is
/// convenience rather than enforcement and a posture that changed under a
/// held-open confirmation is still refused here (drop design §3).
fn start_drop(state: &mut State) -> Vec<Effect> {
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
            // Logged before the modal, not instead of it: the modal is gone
            // the moment it is dismissed, and the trail is where the reason
            // survives (query log design §1, design §5).
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

/// A drop ended. `error` is `None` on the success the cluster reported.
///
/// Dropped where `query` is not the drop that is outstanding, which is every
/// ordinary read: this is called from both result arms, and the id is the
/// only thing that separates a drop's answer from a grid's
/// (architecture.md §3).
///
/// The success arm re-issues `Effect::LoadSchema`: the tree named a node that
/// no longer exists the moment the statement succeeded, and `schema_loaded`
/// clamps the cursor to what is left (drop design §4). Nothing is reloaded on
/// a failure — the schema did not change.
fn finish_drop(state: &mut State, query: QueryId, error: Option<&DbError>) -> Vec<Effect> {
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

    raise(
        state,
        Message {
            title: match dropped.target() {
                Target::Rows { .. } => "Deleted".to_owned(),
                Target::Keyspace(_) | Target::Table(_) => "Dropped".to_owned(),
            },
            message: format!("{} is gone", dropped.target().qualified()),
            severity: Severity::Info,
        },
    );

    // What went stale is what gets re-read. A drop changed the shape of the
    // cluster and the tree is describing it; a delete changed the rows of one
    // table and the grid is showing them (design §4).
    match dropped.target() {
        Target::Rows { table, .. } => {
            let table = table.clone();
            let stmt = Statement::select_all(&table.keyspace, &table.table, ROW_LIMIT);

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

/// `ctrl-d`/`ctrl-u`, dispatched to whatever `key` would reach unmodified.
///
/// Delegating rather than paging here is what keeps the two spellings from
/// drifting: an overlay that learns to page learns it once.
fn page(state: &mut State, key: Key) -> Vec<Effect> {
    match &state.overlay {
        Some(Overlay::Commands(_)) => palette_key(state, key),
        Some(Overlay::Themes { .. }) => themes_key(state, key),
        Some(Overlay::Connections { .. }) => connections_key(state, key),
        Some(Overlay::Diagnostics(_)) => diagnostics_key(state, key),
        // The prompt takes text and pages nothing, so neither spelling does;
        // the form and the filter modal have nothing paged either, and the
        // confirmation drives no list. `Fatal` is unreachable here for
        // `takes_text`'s reason.
        Some(
            Overlay::Fatal(_)
            | Overlay::Search(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_),
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
    let previous = state.geometry;
    state.geometry = geometry;

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

    // The bar's `? commands` hint is a function of `command_rows`
    // (view/hints.rs): a resize that flips it between zero and
    // non-zero changes the view model just as surely as the ddl
    // clamp above does, and owes it the same repaint.
    if geometry.command_rows() != previous.command_rows() {
        state.mark_dirty();
    }

    // A palette or a diagnostics list the frame shrank past cannot stay
    // open: nothing would draw it, and it would still be capturing every key
    // (design §5.1).
    if geometry.command_rows() == 0
        && matches!(
            state.overlay,
            Some(Overlay::Commands(_) | Overlay::Diagnostics(_))
        )
    {
        state.overlay = None;
        state.mark_dirty();
    }

    // The picker closes on its own gate, not the palette's, since config
    // themes can make its list not fit where a palette would (design §9) —
    // and restores the theme it opened on, the same as `Esc`, rather than
    // stranding the preview.
    if geometry.theme_rows() == 0 && matches!(state.overlay, Some(Overlay::Themes { .. })) {
        restore_theme(state);
        state.overlay = None;
        state.mark_dirty();
    }

    // The form is a larger block than the palette, so it has a shrink of its
    // own to answer for — between the two thresholds the panels draw, the form
    // does not, and it would still be capturing every key.
    if !geometry.export_fits() && matches!(state.overlay, Some(Overlay::Export(_))) {
        cancel_pending_export(state);
        state.overlay = None;
        state.mark_dirty();
    }

    // The confirmation answers for its own shrink, on its own threshold, for
    // the reason the picker does not borrow the palette's: it is a different
    // block and fits where the form does not (drop design §1).
    if !geometry.drop_fits() && matches!(state.overlay, Some(Overlay::Drop(_))) {
        state.overlay = None;
        state.mark_dirty();
    }

    // The connection picker shrinks out the same way, on its own gate — with
    // the exception `open_connections` makes: an empty list is guidance, not
    // a list, and there are no rows for a frame to be too short for.
    if geometry.connection_rows() == 0
        && !state.config.connections.is_empty()
        && matches!(state.overlay, Some(Overlay::Connections { .. }))
    {
        state.overlay = None;
        state.mark_dirty();
    }

    // Last, so a picker is offered only into a frame the guards above have
    // already emptied. `init` had no frame to size one against, so this
    // measurement is where an unchosen connection gets its offer
    // (connection picker design §4).
    offer_connections(state);

    Vec::new()
}

/// Opens the picker where the frame is free and nothing has been chosen.
///
/// Called from the two places the frame becomes free with the connection
/// still unmade: the first measurement, since `init` runs before there is a
/// frame, and a dismissal, since the diagnostics modal claims startup's first
/// frame whenever the config had anything wrong with it.
///
/// Keyed on [`ConnectionState::Unchosen`] and not on "not connected", which
/// is what makes a picker the user has waved away stay away: dismissing it
/// leaves `Disconnected`, and nothing here re-offers that.
fn offer_connections(state: &mut State) {
    if !matches!(state.connection, ConnectionState::Unchosen) || state.overlay.is_some() {
        return;
    }

    open_connections(state);

    if state.overlay.is_some() {
        state.mark_dirty();
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

    let stmt = Statement::select_all(&table.keyspace, &table.table, ROW_LIMIT);
    submit_into_contents(state, Subject::Table(table), stmt)
}

/// `ctrl-r`: the Queries buffer, whole, as one statement (design §3).
fn run(state: &mut State) -> Vec<Effect> {
    if state.focus() != Panel::Queries {
        return Vec::new();
    }

    let text = state.queries.text();
    let cql = text.trim();
    if cql.is_empty() {
        return Vec::new();
    }

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
    submit_into_contents(state, Subject::Typed(cql.to_owned()), stmt)
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
    match submit(state, stmt) {
        Ok((query, submitted)) => {
            state.contents.requested(query, subject);
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
            state.contents.refused(subject);

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
            let next = first + grid::fitting(widths, first, available);

            (next < widths.len()).then_some(next)
        }
    }
}

/// The columns the grid is drawing over, and the width they have.
///
/// `None` when there is no page: nothing to fit, and nothing to follow.
fn grid_columns(state: &State) -> Option<(&[u16], u16)> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };

    let gutter = grid::gutter(page.len());
    let width = u16::try_from(state.geometry.grid_width()).unwrap_or(u16::MAX);

    Some((page.widths(), grid::available(width, gutter)))
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
    // were measured with rather than be copied per tick.
    let mut columns = state.contents.column_scroll;
    // Clamp first, follow second, mirroring `clamp_lists` before
    // `follow_moved_cursors` on the row axis: the clamp answers "is the
    // offset too far right for this page and this width", which can leave the
    // cursor off screen, and the follow is what puts it back. The other order
    // would let the clamp undo the follow and hide the selected cell.
    columns.clamp_to(widths, available);
    columns.follow(column, widths, available);

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

    Ok((
        query,
        vec![
            Effect::Execute {
                conn,
                query,
                stmt: Box::new(stmt.clone()),
            },
            Effect::RecordHistory(Box::new(stmt)),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Stamp;
    use crate::commands::{CATALOGUE, CommandsPanel, Run};
    use crate::config::{
        Config, ConnectionName, Diagnostic, Discovery, Icons, Origin, Span as ConfigSpan,
    };
    use crate::contents::{ContentsState, SelectionMode};
    use crate::model::{CqlValue, Modifiers, Row, StatementKind};
    use crate::pointer::{Click, Pointer, Wheel};
    use crate::search::{Prompt, Search, Searched};
    use crate::state::{Connection, FLASH_TICKS, Range, Span};
    use crate::theme::{Ours, Ported};
    use crate::tree::tests_support::schema_of as tabled_schema;
    use crate::view::OverlayView;
    use crate::view::Stroke;
    use crate::view::tests_support::tabled;

    fn schema_of(names: &[&str]) -> Schema {
        tabled_schema(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
    }

    /// A config naming one connection and making it the default.
    ///
    /// What `init` needs before it opens anything at all: nothing is reached
    /// unless a file named it, so `Config::default()` now starts in the
    /// picker rather than on a cluster (connection picker design §2).
    fn connecting_config() -> Config {
        let name = ConnectionName::try_new("fixture").expect("a valid name");

        Config {
            connections: vec![ConnectionConfig {
                name: name.clone(),
                hosts: vec!["127.0.0.1:9042".to_owned()],
                posture: Posture::ReadOnly,
                discovery: Discovery::default(),
                datacenter: None,
                credentials: None,
            }],
            default_connection: Some(name),
            ..Config::default()
        }
    }

    /// Starts up on a config with a default connection, and reports the id
    /// `init` minted for it — the id is no longer the caller's to choose.
    fn started_on_default(
        diagnostics: Vec<Diagnostic>,
        now: Stamp,
    ) -> (State, ConnId, Vec<Effect>) {
        let (state, effects) = init(connecting_config(), diagnostics, now);
        let Some(Effect::Connect { conn, .. }) = effects.first() else {
            panic!("a config naming a default connection opens it at startup");
        };

        (state, *conn, effects)
    }

    fn connected(posture: Posture) -> State {
        let mut state = State::default();
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
            posture,
            schema: SchemaState::Loading,
        });
        state
    }

    #[test]
    fn starting_up_logs_the_connection_it_is_opening() {
        let (state, _conn, _effects) = started_on_default(Vec::new(), Stamp::new(14, 2, 9));

        let entry = state.log.entries().next().expect("an entry per effect");
        assert_eq!(entry.at(), Stamp::new(14, 2, 9));
        assert!(
            entry.subject().text().starts_with("Connecting to fixture"),
            "the trail names which connection, now that there is more than one to name"
        );
        assert!(matches!(
            entry.outcome(),
            Outcome::Pending {
                trace: Trace::Connect(_)
            }
        ));
    }

    #[test]
    fn connecting_completes_the_entry_that_was_waiting_on_it() {
        let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());

        let _effects = update(
            &mut state,
            Msg::Connected {
                conn,
                posture: Posture::Writable,
                took: Duration::from_millis(38),
            },
        );

        let entry = state.log.entries().next().expect("the connection entry");
        let Outcome::Succeeded { detail, took } = entry.outcome() else {
            panic!("the connection succeeded");
        };
        assert!(detail.contains("writable"));
        assert_eq!(*took, Duration::from_millis(38));
    }

    #[test]
    fn a_failed_connection_leaves_its_reason_in_the_trail() {
        let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());

        let _effects = update(
            &mut state,
            Msg::ConnectFailed {
                conn,
                error: DbError::Unreachable("no route to host".to_owned()),
                took: Duration::from_millis(12),
            },
        );

        let entry = state.log.entries().next().expect("the connection entry");
        let Outcome::Failed { detail } = entry.outcome() else {
            panic!("the connection failed");
        };
        assert!(detail.contains("no route to host"));
    }

    #[test]
    fn a_failed_startup_connection_raises_a_modal_that_ends_the_process() {
        let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());

        let _effects = update(
            &mut state,
            Msg::ConnectFailed {
                conn,
                error: DbError::Unreachable("no route to host".to_owned()),
                took: Duration::ZERO,
            },
        );

        assert!(matches!(state.overlay, Some(Overlay::Fatal(_))));
    }

    #[test]
    fn the_fatal_modal_quits_rather_than_dismissing() {
        let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());
        let _effects = update(
            &mut state,
            Msg::ConnectFailed {
                conn,
                error: DbError::Timeout,
                took: Duration::ZERO,
            },
        );

        for key in [Key::Escape, Key::Char('q')] {
            let mut state = state.clone();
            let effects = press(&mut state, key);

            assert!(
                effects.contains(&Effect::Quit),
                "{key:?} ends the process rather than revealing an application that never started"
            );
            assert!(
                matches!(state.overlay, Some(Overlay::Fatal(_))),
                "and does not dismiss"
            );
        }
    }

    // The config diagnostics describe a session that is not going to happen.
    #[test]
    fn a_fatal_modal_replaces_the_diagnostics_it_lands_on() {
        let (mut state, conn, _effects) =
            started_on_default(vec![diagnostic("boom")], Stamp::default());
        assert!(matches!(state.overlay, Some(Overlay::Diagnostics(_))));

        let _effects = update(
            &mut state,
            Msg::ConnectFailed {
                conn,
                error: DbError::Timeout,
                took: Duration::ZERO,
            },
        );

        assert!(matches!(state.overlay, Some(Overlay::Fatal(_))));
    }

    #[test]
    fn what_the_config_got_wrong_is_in_the_trail_as_well_as_the_modal() {
        let (state, _effects) = init(
            Config::default(),
            vec![diagnostic("boom")],
            Stamp::default(),
        );

        assert!(
            state.log.entries().any(|entry| matches!(
                entry.outcome(),
                Outcome::Failed { .. } | Outcome::Rejected { .. }
            )),
            "a diagnostic reaches the trail"
        );
        assert!(
            matches!(state.overlay, Some(Overlay::Diagnostics(_))),
            "the modal is unchanged"
        );
        assert!(
            matches!(
                state.log.entries().next().unwrap().outcome(),
                Outcome::Rejected { .. }
            ),
            "the diagnostic precedes the connection entry — the order things happened"
        );
    }

    #[test]
    fn connecting_logs_the_schema_read_it_starts() {
        let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());

        let _effects = update(
            &mut state,
            Msg::Connected {
                conn,
                posture: Posture::Writable,
                took: Duration::ZERO,
            },
        );

        let entry = state.log.entries().nth(1).expect("the schema entry");
        assert!(matches!(
            entry.outcome(),
            Outcome::Pending {
                trace: Trace::Schema(_)
            }
        ));
    }

    #[test]
    fn a_loaded_schema_reports_what_it_found() {
        let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());
        let _effects = update(
            &mut state,
            Msg::Connected {
                conn,
                posture: Posture::Writable,
                took: Duration::ZERO,
            },
        );

        let _effects = update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(&["shop"])),
                took: Duration::from_millis(112),
            },
        );

        let entry = state.log.entries().nth(1).expect("the schema entry");
        let Outcome::Succeeded { detail, .. } = entry.outcome() else {
            panic!("the schema loaded");
        };
        assert!(detail.contains("keyspace"), "{detail}");
    }

    #[test]
    fn a_tick_costs_a_repaint_only_while_something_is_outstanding() {
        let mut idle = State::default();
        idle.dirty = false;

        assert!(!idle.is_working());
        update(&mut idle, Msg::Tick(Stamp::default()));
        assert!(
            !idle.dirty,
            "an idle tick changes nothing, and repainting on it would burn the terminal 25 times a second"
        );

        let mut working = connected(Posture::ReadOnly);
        working.dirty = false;

        assert!(working.is_working(), "the schema is still loading");
        update(&mut working, Msg::Tick(Stamp::default()));
        assert!(working.dirty);
    }

    #[test]
    fn a_tick_refreshes_the_clock() {
        let mut state = State::default();

        let effects = update(&mut state, Msg::Tick(Stamp::new(14, 2, 9)));

        assert_eq!(state.now, Stamp::new(14, 2, 9));
        assert!(effects.is_empty());
    }

    // A tick arrives twenty-five times a second. One that changes nothing must
    // not cost a frame (visual polish design §4.2).
    #[test]
    fn refreshing_the_clock_does_not_cost_a_frame() {
        let mut state = State::default();
        state.dirty = false;

        let _effects = update(&mut state, Msg::Tick(Stamp::new(14, 2, 9)));

        assert!(!state.dirty, "an idle tick repaints nothing");
    }

    #[test]
    fn the_loader_falls_back_to_rest_when_the_work_finishes() {
        let mut state = connected(Posture::ReadOnly);
        for _ in 0..5 {
            update(&mut state, Msg::Tick(Stamp::default()));
        }
        assert_ne!(state.loader.frame(), 0);

        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(&["shop"])),
                took: Duration::ZERO,
            },
        );
        state.dirty = false;
        update(&mut state, Msg::Tick(Stamp::default()));

        assert_eq!(
            state.loader.frame(),
            0,
            "an idle frame is always the same frame, not wherever the animation stopped"
        );
        assert!(state.dirty, "the fall back to rest is itself a repaint");
    }

    #[test]
    fn a_query_in_flight_is_work_the_loader_animates_for() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(&["shop"])),
                took: Duration::ZERO,
            },
        );
        assert!(
            !state.is_working(),
            "the schema arrived and nothing else is out"
        );

        state
            .in_flight
            .insert(QueryId::next(), statement(StatementKind::Read));

        assert!(state.is_working());
    }

    #[test]
    fn each_wait_belongs_to_the_panel_it_will_fill() {
        let mut state = connected(Posture::ReadOnly);

        assert_eq!(
            working_panels(&state),
            vec![Panel::Tables],
            "a schema still loading is the tree filling"
        );

        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(&["shop"])),
                took: Duration::ZERO,
            },
        );
        state
            .in_flight
            .insert(QueryId::next(), statement(StatementKind::Read));

        assert_eq!(
            working_panels(&state),
            vec![Panel::Contents],
            "a statement lands in the grid, whichever panel asked for it"
        );
    }

    fn working_panels(state: &State) -> Vec<Panel> {
        Panel::ALL
            .into_iter()
            .filter(|&panel| state.is_working_in(panel))
            .collect()
    }

    fn current_conn(state: &State) -> ConnId {
        let ConnectionState::Connected(connection) = &state.connection else {
            panic!("the helper builds a connected state");
        };
        connection.id
    }

    fn statement(kind: StatementKind) -> Statement {
        Statement {
            cql: "SELECT * FROM ks.t".to_owned(),
            kind,
        }
    }

    #[test]
    fn read_only_connection_refuses_a_mutating_statement() {
        let mut state = connected(Posture::ReadOnly);

        let result = submit(&mut state, statement(StatementKind::Write));

        assert_eq!(result, Err(DbError::ReadOnly));
        assert!(
            state.in_flight.is_empty(),
            "a refused statement must not be registered as in flight"
        );
    }

    #[test]
    fn a_connection_with_no_posture_is_read_only() {
        let mut state = State::default();

        assert_eq!(state.posture(), Posture::ReadOnly);
        assert_eq!(
            submit(&mut state, statement(StatementKind::Write)),
            Err(DbError::ReadOnly)
        );
    }

    #[test]
    fn reads_are_permitted_on_a_read_only_connection() {
        let mut state = connected(Posture::ReadOnly);

        let (_, effects) =
            submit(&mut state, statement(StatementKind::Read)).expect("read is allowed");

        assert_eq!(effects.len(), 2, "expected Execute and RecordHistory");
        assert_eq!(state.in_flight.len(), 1);
    }

    #[test]
    fn writable_connection_executes_and_records_a_mutating_statement() {
        let mut state = connected(Posture::Writable);
        let conn = current_conn(&state);

        let (query, effects) =
            submit(&mut state, statement(StatementKind::Write)).expect("write is allowed");

        assert_eq!(
            state.in_flight.keys().copied().collect::<Vec<_>>(),
            vec![query],
            "the id handed back is the one registered as in flight"
        );
        assert_eq!(
            effects,
            vec![
                Effect::Execute {
                    conn,
                    query,
                    stmt: Box::new(statement(StatementKind::Write)),
                },
                Effect::RecordHistory(Box::new(statement(StatementKind::Write))),
            ]
        );
    }

    #[test]
    fn a_statement_with_no_connection_to_run_it_on_is_refused() {
        let mut state = State::default();

        assert_eq!(
            submit(&mut state, statement(StatementKind::Read)),
            Err(DbError::Unreachable("not connected".to_owned()))
        );
        assert!(state.in_flight.is_empty());
    }

    #[test]
    fn a_result_for_a_cancelled_query_is_dropped() {
        let mut state = connected(Posture::ReadOnly);
        let cancelled = QueryId::next();

        let effects = update(
            &mut state,
            Msg::Failed {
                query: cancelled,
                error: DbError::Timeout,
                took: Duration::ZERO,
            },
        );

        assert!(effects.is_empty());
        assert!(
            !state.dirty,
            "a late result for a query nobody is waiting on must not force a redraw"
        );
    }

    #[test]
    fn quitting_cancels_everything_still_in_flight() {
        let mut state = connected(Posture::Writable);
        let (query, _) =
            submit(&mut state, statement(StatementKind::Write)).expect("write is allowed");

        let effects = update(&mut state, Msg::Quit);

        assert!(state.should_quit);
        assert_eq!(effects, vec![Effect::Cancel(query), Effect::Quit]);
    }

    #[test]
    fn a_resize_marks_the_state_dirty_and_emits_no_effects() {
        let mut state = State::default();

        let effects = update(&mut state, Msg::Resized);

        assert!(
            effects.is_empty(),
            "a resize is not an effect-producing event"
        );
        assert!(state.dirty, "a resize must force the next frame to repaint");
    }

    #[test]
    fn schema_for_a_stale_connection_is_ignored() {
        let mut state = connected(Posture::ReadOnly);

        let effects = update(
            &mut state,
            Msg::SchemaLoaded {
                conn: ConnId::next(),
                schema: Box::new(Schema::default()),
                took: Duration::ZERO,
            },
        );

        assert!(effects.is_empty());
        let ConnectionState::Connected(connection) = state.connection else {
            panic!("the connection is still present");
        };
        assert!(
            matches!(connection.schema, SchemaState::Loading),
            "schema from a connection we are no longer on must not be applied"
        );
    }

    /// Design §2: a config that names a default connection opens it, and one
    /// that does not opens nothing — the tool reaches no cluster a file did
    /// not name.
    #[test]
    fn a_config_naming_a_default_connection_opens_it_at_startup() {
        let (state, conn, effects) = started_on_default(Vec::new(), Stamp::default());

        assert!(
            matches!(effects.as_slice(), [Effect::Connect { conn: id, target }]
                if *id == conn && target.name.as_str() == "fixture"),
            "the effect carries the connection it opens, not only an id"
        );
        assert!(state.connection.is_current(conn));
        assert!(state.dirty, "the first frame must be painted");
    }

    /// A config naming `count` connections, none of them the default.
    fn config_of(names: &[&str]) -> Config {
        Config {
            connections: names
                .iter()
                .map(|name| ConnectionConfig {
                    name: ConnectionName::try_new(name).expect("a valid name"),
                    hosts: vec![format!("{name}.example:9042")],
                    posture: Posture::ReadOnly,
                    discovery: Discovery::default(),
                    datacenter: None,
                    credentials: None,
                })
                .collect(),
            ..Config::default()
        }
    }

    /// A frame the picker and the palette both fit in.
    const ROOMY: Geometry = Geometry::new([8, 14, 20, 1], 37, 14);

    fn roomy() -> Geometry {
        ROOMY.with_connection_rows(20).with_theme_rows(20)
    }

    /// Startup on `config`, carried to the first measurement — which is where
    /// the picker is offered, `init` having had no frame to size one against.
    fn started_on(config: Config) -> State {
        let (mut state, _effects) = init(config, Vec::new(), Stamp::default());
        update(&mut state, Msg::Measured(roomy()));
        state
    }

    fn connection_picker(state: &State) -> Option<usize> {
        match state.overlay.as_ref()? {
            Overlay::Connections { cursor, .. } => Some(cursor.index()),
            _ => None,
        }
    }

    /// Design §4: a config with connections but no default opens the picker
    /// rather than guessing which one was meant.
    #[test]
    fn connections_with_no_default_open_the_picker() {
        let state = started_on(config_of(&["prod", "staging"]));

        assert_eq!(
            connection_picker(&state),
            Some(0),
            "the picker opens on the first entry when no default names one"
        );
        assert!(matches!(state.connection, ConnectionState::Unchosen));
    }

    /// Design §2: the default is seated under the cursor, so `⏎` on an opened
    /// picker is the same connection a start with no picker would have made.
    #[test]
    fn the_picker_opens_seated_on_the_default_connection() {
        let mut config = config_of(&["prod", "staging", "dev"]);
        config.default_connection = Some(ConnectionName::try_new("dev").expect("a valid name"));
        let mut state = started_on(config);
        // A default connects at startup rather than opening the picker, so it
        // is reopened by hand to see where its cursor lands.
        state.connection = ConnectionState::Disconnected;
        state.overlay = None;
        open_connections(&mut state);

        assert_eq!(connection_picker(&state), Some(2));
    }

    /// Design §3: nothing to list is not a short list — the picker still
    /// opens, because a user with no connections is the one who most needs
    /// telling why the panels are empty.
    #[test]
    fn a_config_with_no_connections_still_opens_the_picker() {
        let state = started_on(Config::default());

        assert_eq!(connection_picker(&state), Some(0));
    }

    #[test]
    fn enter_on_the_picker_connects_to_the_row_under_the_cursor() {
        let mut state = started_on(config_of(&["prod", "staging"]));

        press(&mut state, Key::Char('j'));
        let effects = press(&mut state, Key::Enter);

        assert!(
            matches!(effects.as_slice(), [Effect::Connect { target, .. }]
                if target.name.as_str() == "staging"),
            "the second row is the one opened: {effects:?}"
        );
        assert!(state.overlay.is_none(), "the picker closes behind it");
    }

    /// Design §4: dismissing the picker is a choice, and nothing puts it back
    /// up — otherwise `esc` would be inert and the tool unusable without a
    /// connection.
    #[test]
    fn a_dismissed_picker_is_not_offered_again() {
        let mut state = started_on(config_of(&["prod"]));

        press(&mut state, Key::Escape);
        assert!(state.overlay.is_none());
        assert!(matches!(state.connection, ConnectionState::Disconnected));

        update(&mut state, Msg::Measured(roomy().with_grid_width(40)));

        assert!(
            state.overlay.is_none(),
            "a measurement must not resurrect a picker the user waved away"
        );
    }

    /// Design §5: `ctrl-o` is the way back, so a dismissed picker — or a
    /// connection opened by mistake — is not final.
    #[test]
    fn ctrl_o_reopens_the_picker() {
        let mut state = started_on(config_of(&["prod"]));
        press(&mut state, Key::Escape);

        update(&mut state, Msg::Input(KeyPress::ctrl(Key::Char('o'))));

        assert_eq!(connection_picker(&state), Some(0));
    }

    /// Design §4: the diagnostics modal claims startup's first frame, so the
    /// picker underneath waits for it rather than being lost.
    #[test]
    fn the_picker_waits_behind_the_diagnostics_modal_and_opens_when_it_closes() {
        let (mut state, _effects) = init(
            config_of(&["prod"]),
            vec![diagnostic("boom")],
            Stamp::default(),
        );
        update(&mut state, Msg::Measured(roomy()));

        assert!(
            matches!(state.overlay, Some(Overlay::Diagnostics(_))),
            "what the config got wrong is read before a connection is chosen"
        );

        press(&mut state, Key::Escape);

        assert_eq!(
            connection_picker(&state),
            Some(0),
            "closing the diagnostics gives the picker its turn"
        );
    }

    /// Overlay filter design §1: the term filters the list, and `⏎` after it
    /// opens the row the cursor was left on.
    #[test]
    fn a_term_filters_the_picker_and_enter_connects_to_what_it_left() {
        let mut state = started_on(config_of(&["prod", "staging", "dev"]));

        press(&mut state, Key::Char('/'));
        for character in "sta".chars() {
            press(&mut state, Key::Char(character));
        }
        let effects = press(&mut state, Key::Enter);

        assert!(
            effects.is_empty(),
            "the first enter closes the prompt rather than connecting"
        );

        let effects = press(&mut state, Key::Enter);

        assert!(
            matches!(effects.as_slice(), [Effect::Connect { target, .. }]
                if target.name.as_str() == "staging"),
            "the row the term left is the row opened: {effects:?}"
        );
    }

    /// Design §5: a term that kept nothing holds no row, so closing it opens
    /// nothing and puts the cursor back where the prompt started — never on
    /// an entry the filter had hidden.
    #[test]
    fn a_term_that_matches_nothing_opens_nothing_and_restores_the_cursor() {
        let mut state = started_on(config_of(&["prod", "staging", "dev"]));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('/'));
        for character in "zzz".chars() {
            press(&mut state, Key::Char(character));
        }
        let effects = press(&mut state, Key::Enter);

        assert!(
            effects.is_empty(),
            "a term holding no row opens no connection"
        );
        assert_eq!(
            connection_picker(&state),
            Some(1),
            "the cursor lands back where the prompt opened"
        );

        let effects = press(&mut state, Key::Enter);

        assert!(
            matches!(effects.as_slice(), [Effect::Connect { target, .. }]
                if target.name.as_str() == "staging"),
            "and the row it landed on is the visible one: {effects:?}"
        );
    }

    /// The posture the picker names is the connection's own, which is what
    /// `Posture::default()` being `ReadOnly` protects: opening `writable`
    /// has to be something a file said.
    #[test]
    fn the_connection_opened_carries_its_own_posture() {
        let mut config = config_of(&["prod"]);
        config.connections[0].posture = Posture::Writable;
        let mut state = started_on(config);

        let effects = press(&mut state, Key::Enter);

        assert!(
            matches!(effects.as_slice(), [Effect::Connect { target, .. }]
                if target.posture == Posture::Writable),
        );
    }

    #[test]
    fn a_config_naming_no_default_connects_to_nothing() {
        let (state, effects) = init(Config::default(), Vec::new(), Stamp::default());

        assert!(
            effects.is_empty(),
            "no file named a cluster, so none is reached"
        );
        assert!(matches!(state.connection, ConnectionState::Unchosen));
    }

    // The connection entry is pushed before any tick has arrived, so a defaulted
    // clock would stamp the one entry a user reads when something has gone wrong
    // with a time that never happened (query log design §4.1).
    #[test]
    fn the_startup_state_is_stamped_by_its_caller() {
        let (state, _effects) = init(Config::default(), Vec::new(), Stamp::new(9, 30, 0));

        assert_eq!(state.now, Stamp::new(9, 30, 0));
    }

    #[test]
    fn a_config_with_nothing_wrong_opens_no_overlay() {
        let (state, _) = init(Config::default(), Vec::new(), Stamp::default());

        assert!(state.overlay.is_none());
    }

    #[test]
    fn diagnostics_open_over_the_panels_and_esc_dismisses_them() {
        let diagnostic = Diagnostic {
            origin: Origin::new("./.lazycassandra.toml"),
            span: Some(ConfigSpan {
                line: 14,
                column: 11,
            }),
            severity: Severity::Danger,
            message: "connection `dev`: `posture` unknown value \"read\"".to_owned(),
        };
        let (mut state, _) = init(Config::default(), vec![diagnostic], Stamp::default());

        assert!(matches!(state.overlay, Some(Overlay::Diagnostics(_))));

        let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Escape)));

        assert!(
            state.overlay.is_none(),
            "esc dismisses this overlay like every other"
        );
    }

    fn diagnostic(message: &str) -> Diagnostic {
        Diagnostic {
            origin: Origin::new("./.lazycassandra.toml"),
            span: None,
            severity: Severity::Danger,
            message: message.to_owned(),
        }
    }

    fn diagnostics_cursor(state: &State) -> usize {
        let Some(Overlay::Diagnostics(panel)) = &state.overlay else {
            panic!("the diagnostics overlay is open");
        };
        panel.selected()
    }

    #[test]
    fn a_single_diagnostic_leaves_the_cursor_nowhere_to_go() {
        let (mut state, _) = init(Config::default(), vec![diagnostic("one")], Stamp::default());

        let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Char('j'))));
        let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Char('j'))));

        assert_eq!(diagnostics_cursor(&state), 0);
    }

    #[test]
    fn motion_walks_the_diagnostics_and_stops_on_the_last_row() {
        let diagnostics = vec![diagnostic("one"), diagnostic("two"), diagnostic("three")];
        let (mut state, _) = init(Config::default(), diagnostics, Stamp::default());

        for _ in 0..5 {
            let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Char('j'))));
        }
        assert_eq!(
            diagnostics_cursor(&state),
            2,
            "the cursor stops on the last row rather than running off the list"
        );

        let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Char('k'))));
        assert_eq!(diagnostics_cursor(&state), 1);
    }

    #[test]
    fn the_config_the_tool_started_on_is_kept_rather_than_consumed() {
        let config = Config {
            icons: Icons::Off,
            ..Config::default()
        };
        let (state, _) = init(config.clone(), Vec::new(), Stamp::default());

        assert_eq!(
            state.config, config,
            "the follow-ups in design §11 read this"
        );
    }

    #[test]
    fn the_theme_a_config_names_is_the_theme_the_tool_opens_on() {
        let config = Config {
            theme: ThemeChoice::Ours(Ours::QuillLight),
            ..Config::default()
        };
        let (state, _) = init(config, Vec::new(), Stamp::default());

        assert_eq!(state.theme, ThemeChoice::Ours(Ours::QuillLight));
    }

    #[test]
    fn connecting_successfully_asks_for_the_schema() {
        let (mut state, conn, _) = started_on_default(Vec::new(), Stamp::default());

        let effects = update(
            &mut state,
            Msg::Connected {
                conn,
                posture: Posture::ReadOnly,
                took: Duration::ZERO,
            },
        );

        assert_eq!(effects, vec![Effect::LoadSchema(conn)]);
        assert!(state.schema().is_none(), "the schema is still loading");
    }

    #[test]
    fn a_connected_message_for_a_stale_attempt_is_ignored() {
        let (mut state, conn, _) = started_on_default(Vec::new(), Stamp::default());

        let effects = update(
            &mut state,
            Msg::Connected {
                conn: ConnId::next(),
                posture: Posture::ReadOnly,
                took: Duration::ZERO,
            },
        );

        assert!(effects.is_empty());
        assert!(
            state.connection.is_current(conn),
            "a Connected message for a connection we are no longer waiting on must not be applied"
        );
    }

    #[test]
    fn a_failed_connection_is_recorded_and_raised() {
        let (mut state, conn, _) = started_on_default(Vec::new(), Stamp::default());

        let effects = update(
            &mut state,
            Msg::ConnectFailed {
                conn,
                error: DbError::Unreachable("no route".to_owned()),
                took: Duration::ZERO,
            },
        );

        assert!(effects.is_empty());
        assert!(matches!(state.connection, ConnectionState::Failed(_)));
        let Some(Overlay::Fatal(overlay)) = state.overlay else {
            panic!("a failure is shown");
        };
        assert_eq!(overlay.severity, Severity::Danger);
        assert!(overlay.message.contains("no route"));
    }

    #[test]
    fn a_failed_connection_for_a_stale_attempt_is_dropped() {
        let (mut state, conn, _) = started_on_default(Vec::new(), Stamp::default());

        update(
            &mut state,
            Msg::ConnectFailed {
                conn: ConnId::next(),
                error: DbError::Timeout,
                took: Duration::ZERO,
            },
        );

        assert!(state.overlay.is_none());
        assert!(state.connection.is_current(conn));
    }

    #[test]
    fn a_failed_schema_is_recorded_and_raised() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);

        update(
            &mut state,
            Msg::SchemaFailed {
                conn,
                error: DbError::Invalid("unauthorized".to_owned()),
                took: Duration::ZERO,
            },
        );

        assert!(state.overlay.is_some());
        let ConnectionState::Connected(connection) = &state.connection else {
            panic!("still connected");
        };
        assert!(matches!(connection.schema, SchemaState::Failed(_)));
    }

    #[test]
    fn a_failed_schema_for_a_stale_connection_is_dropped() {
        let mut state = connected(Posture::ReadOnly);

        update(
            &mut state,
            Msg::SchemaFailed {
                conn: ConnId::next(),
                error: DbError::Invalid("unauthorized".to_owned()),
                took: Duration::ZERO,
            },
        );

        assert!(state.overlay.is_none(), "a stale failure must not be shown");
        let ConnectionState::Connected(connection) = &state.connection else {
            panic!("still connected");
        };
        assert!(
            matches!(connection.schema, SchemaState::Loading),
            "schema failure from a connection we are no longer on must not be applied"
        );
    }

    #[test]
    fn a_message_arriving_over_an_open_prompt_restores_the_previous_search() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(&[("shop", &["orders"])])),
                took: Duration::ZERO,
            },
        );
        state.apply_search(Searched::Tables, Search::new("shop"));

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));
        press(&mut state, Key::Char('r'));
        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("or"),
            "the live prompt is previewing its own buffer, not the previous search"
        );

        // Any message-driven overlay unwinds a live search the same way
        // (`unwind_overlay`'s doc comment) — `EditorFailed` stands in
        // because `SchemaFailed` now raises `Overlay::Fatal`, which has
        // nothing to preserve (there is no application left to return to).
        update(
            &mut state,
            Msg::EditorFailed {
                reason: "could not run hx: No such file or directory".to_owned(),
            },
        );

        assert!(
            matches!(state.overlay, Some(Overlay::Message(_))),
            "the failure must still be shown"
        );
        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("shop"),
            "the half-typed query must not strand itself; the prompt's previous search is restored"
        );
    }

    fn browsing(keyspaces: &[&str]) -> State {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(keyspaces)),
                took: Duration::ZERO,
            },
        );
        state
    }

    fn press(state: &mut State, key: Key) -> Vec<Effect> {
        update(state, Msg::Input(KeyPress::new(key)))
    }

    fn press_binding(state: &mut State, binding: KeyPress) -> Vec<Effect> {
        update(state, Msg::Input(binding))
    }

    fn press_stroke(state: &mut State, stroke: Stroke) -> Vec<Effect> {
        press_binding(state, stroke.press())
    }

    /// A frame big enough for a palette and a picker. `browsing` leaves
    /// geometry at its default, whose `command_rows` and `theme_rows` are
    /// zero — which is itself a binding
    /// (`the_palette_does_not_open_where_it_cannot_be_drawn`).
    fn measured(keyspaces: &[&str]) -> State {
        let mut state = browsing(keyspaces);
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14).with_theme_rows(20);
        state
    }

    fn palette(state: &State) -> Option<&CommandsPanel> {
        match state.overlay.as_ref()? {
            Overlay::Commands(panel) => Some(panel),
            Overlay::Message(_)
            | Overlay::Search(_)
            | Overlay::Themes { .. }
            | Overlay::Connections { .. }
            | Overlay::Diagnostics(_)
            | Overlay::Export(_)
            | Overlay::Filter(_)
            | Overlay::Drop(_)
            | Overlay::Fatal(_) => None,
        }
    }

    fn theme_picker(state: &State) -> Option<usize> {
        match state.overlay.as_ref()? {
            Overlay::Themes { cursor, .. } => Some(cursor.index()),
            _ => None,
        }
    }

    fn picker_sift(state: &State) -> Option<&Sift> {
        match state.overlay.as_ref()? {
            Overlay::Themes { sift, .. } => sift.as_ref(),
            _ => None,
        }
    }

    fn label_index(label: &str) -> usize {
        BuiltIn::ALL
            .iter()
            .position(|built_in| built_in.label() == label)
            .expect("a built-in with that label")
    }

    /// The rows the renderer is told to draw, which is what the picker's
    /// cursor counts in once a sift is up.
    fn picker_labels(state: &State) -> Vec<String> {
        let Some(OverlayView::Themes { labels, .. }) = crate::view::view_model(state).overlay
        else {
            panic!("the picker is up");
        };

        labels
    }

    /// Overlay filter design §1 and §4: typing keeps the rows
    /// that match and seats the cursor — and so the preview — on the first of
    /// them at or beyond the row the prompt opened on.
    #[test]
    fn typing_in_the_picker_narrows_the_list_to_the_matches() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

        press(&mut state, Key::Char('/'));
        type_into(&mut state, "quill");

        let labels = picker_labels(&state);
        assert!(
            labels
                .iter()
                .all(|label| label.to_lowercase().contains("quill")),
            "{labels:?} keeps a row the term did not match"
        );
        assert!(labels.len() < BuiltIn::ALL.len(), "nothing was dropped");
        assert_eq!(state.theme.label(), "Quill dark");
    }

    /// The cursor is what sets the previewed theme, so a search previews as
    /// it walks — no second mechanism (design §6).
    #[test]
    fn a_searched_row_previews_its_theme() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

        press(&mut state, Key::Char('/'));
        type_into(&mut state, "quill");

        assert_eq!(state.theme.label(), "Quill dark");
    }

    /// Overlay filter design §1: a term whose matches all sit above the row the
    /// prompt opened on seats on the last of them rather than on nothing.
    #[test]
    fn a_term_matching_only_above_the_origin_seats_on_the_last_match() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::from(BuiltIn::Ported(Ported::CatppuccinMocha));
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
        assert!(
            theme_picker(&state).expect("an open picker") > label_index("Ash dark"),
            "the origin has to sit below every match for this to be the case"
        );

        press(&mut state, Key::Char('/'));
        type_into(&mut state, "ash");

        let labels = picker_labels(&state);
        assert_eq!(
            Some(state.theme.label()),
            labels.last().cloned(),
            "the last row the term kept"
        );
    }

    /// Design §6: every keystroke searches from the origin, so extending a
    /// term does not walk the cursor forward.
    #[test]
    fn extending_a_term_searches_from_the_origin() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

        press(&mut state, Key::Char('/'));
        type_into(&mut state, "catppuccin");
        assert_eq!(state.theme.label(), "Catppuccin Latte");

        type_into(&mut state, " mocha");
        assert_eq!(state.theme.label(), "Catppuccin Mocha");
    }

    /// Overlay filter design §4: a term that matches nothing leaves no row to hold,
    /// so the list is empty and the preview stays where it was. There is no
    /// failure state to render.
    #[test]
    fn a_term_that_matches_nothing_empties_the_picker_and_keeps_the_preview() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
        let previewed = state.theme.clone();

        press(&mut state, Key::Char('/'));
        type_into(&mut state, "zzz");

        assert!(picker_labels(&state).is_empty());
        assert_eq!(state.theme, previewed);
    }

    /// Overlay search design §1: `theme_labels` is the order the search
    /// scans and `view_model`'s `OverlayView::Themes::labels` is what the
    /// renderer draws, so a search must land on the row the renderer reports
    /// at that index — a config theme included, which every other picker
    /// search test above runs without (`Config::default()` has none).
    #[test]
    fn typing_seats_on_a_config_theme_at_the_renderers_own_index() {
        let mut state = measured(&["a"]);
        let theme = custom_theme("midnight");
        state.config.themes = vec![theme.clone()];
        ctrl(&mut state, Key::Char('t'));

        press(&mut state, Key::Char('/'));
        type_into(&mut state, "midnight");

        let Some(OverlayView::Themes { labels, .. }) = crate::view::view_model(&state).overlay
        else {
            panic!("the picker is up");
        };
        let expected = labels
            .iter()
            .position(|label| label == "midnight")
            .expect("the config theme is in the picker's labels");

        assert_eq!(theme_picker(&state), Some(expected));
    }

    /// Design §5: with a sift up the overlay takes text, so the letters that
    /// mean something in it type themselves.
    #[test]
    fn a_letter_typed_into_the_pickers_sift_does_not_quit_or_move() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
        let origin = theme_picker(&state).expect("an open picker");

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('q'));
        press(&mut state, Key::Char('j'));

        assert!(!state.should_quit, "q is text while a sift is up");
        assert_eq!(picker_sift(&state).map(Sift::query), Some("qj"));
        assert!(
            picker_labels(&state).is_empty(),
            "j filtered rather than moving"
        );

        press(&mut state, Key::Backspace);
        press(&mut state, Key::Backspace);
        press(&mut state, Key::Backspace);
        assert_eq!(theme_picker(&state), Some(origin), "and moved nothing");
    }

    /// `g`/`G` are decided above the overlays (docs/shortcuts.md §1), but
    /// with a sift up the picker takes text (overlay search design §5), so
    /// both letters type themselves rather than jumping the cursor.
    #[test]
    fn g_and_shift_g_type_themselves_into_the_pickers_sift() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
        let origin = theme_picker(&state).expect("an open picker");

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('g'));
        press(&mut state, Key::Char('G'));

        assert_eq!(picker_sift(&state).map(Sift::query), Some("gG"));
        assert!(
            picker_labels(&state).is_empty(),
            "neither letter jumped the cursor; both were typed"
        );

        press(&mut state, Key::Backspace);
        press(&mut state, Key::Backspace);
        assert_eq!(theme_picker(&state), Some(origin), "and jumped nothing");
    }

    /// Design §5: `backspace` on an empty query closes the prompt and leaves
    /// the cursor where it is.
    #[test]
    fn backspace_on_an_empty_sift_closes_the_prompt() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('q'));
        press(&mut state, Key::Backspace);
        assert_eq!(picker_sift(&state).map(Sift::query), Some(""));

        press(&mut state, Key::Backspace);
        assert!(picker_sift(&state).is_none(), "the prompt is gone");
        assert!(theme_picker(&state).is_some(), "the picker is not");
    }

    /// Design §1: `⏎` closes the prompt and keeps the cursor; the picker
    /// stays open, so a second `⏎` is what keeps the theme.
    #[test]
    fn enter_closes_the_pickers_sift_and_keeps_the_row() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

        press(&mut state, Key::Char('/'));
        type_into(&mut state, "quill");
        press(&mut state, Key::Enter);

        assert!(picker_sift(&state).is_none());
        assert_eq!(
            picker_labels(&state).len(),
            BuiltIn::ALL.len(),
            "the list widens back out"
        );
        assert_eq!(
            theme_picker(&state),
            Some(label_index("Quill dark")),
            "with the cursor still on the row the filter found"
        );
    }

    /// Design §10: `q` quits the moment a sift closes — the letter goes back
    /// to meaning what it means everywhere else once the prompt that was
    /// swallowing it is gone.
    #[test]
    fn q_quits_once_the_pickers_sift_has_closed() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

        press(&mut state, Key::Char('/'));
        type_into(&mut state, "quill");
        press(&mut state, Key::Enter);
        press(&mut state, Key::Char('q'));

        assert!(state.should_quit);
    }

    /// Design §1: `Esc` puts the cursor back where the prompt opened — and
    /// the preview with it; a second `Esc` closes the picker.
    #[test]
    fn escape_restores_the_row_the_pickers_sift_opened_on() {
        let mut state = measured(&["a"]);
        let opened_on = state.theme.clone();
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
        let origin = theme_picker(&state).expect("an open picker");

        press(&mut state, Key::Char('/'));
        type_into(&mut state, "quill");
        press(&mut state, Key::Escape);

        assert_eq!(theme_picker(&state), Some(origin));
        assert_eq!(state.theme, opened_on, "the preview goes back too");

        press(&mut state, Key::Escape);
        assert!(state.overlay.is_none());
    }

    /// Design §5: `ctrl-t` already refuses to open over any live overlay,
    /// `ctrl_key`'s blanket refusal for `Overlay::Themes { .. }` included, so
    /// a chord over a picker holding a live sift never reaches `open_themes`
    /// at all — the sift and its query are left exactly as they were.
    #[test]
    fn ctrl_t_over_a_live_sift_is_refused_by_ctrl_keys_overlay_guard() {
        let mut state = measured(&["a"]);
        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
        press(&mut state, Key::Char('/'));
        type_into(&mut state, "quill");

        press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

        assert_eq!(picker_sift(&state).map(Sift::query), Some("quill"));
    }

    #[test]
    fn question_mark_opens_the_palette_and_closes_it() {
        let mut state = measured(&["a"]);

        press(&mut state, Key::Char('?'));
        assert!(palette(&state).is_some());

        press(&mut state, Key::Char('?'));
        assert!(state.overlay.is_none());
    }

    #[test]
    fn escape_closes_the_palette() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
    }

    /// Design §5: with a sift up the palette takes text, so `q` and `?` type
    /// themselves rather than quitting or closing.
    #[test]
    fn a_letter_typed_into_the_palettes_sift_does_not_quit_or_close() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('q'));
        press(&mut state, Key::Char('?'));

        assert!(!state.should_quit);
        let panel = palette(&state).expect("the palette is still up");
        assert_eq!(panel.sift().map(Sift::query), Some("q?"));
    }

    /// `g`/`G` are decided above the overlays (docs/shortcuts.md §1), but
    /// with a sift up the palette takes text (overlay search design §5), so
    /// both letters type themselves rather than jumping the cursor.
    #[test]
    fn g_and_shift_g_type_themselves_into_the_palettes_sift() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('g'));
        press(&mut state, Key::Char('G'));

        let panel = palette(&state).expect("the palette is still up");
        assert_eq!(panel.sift().map(Sift::query), Some("gG"));
        assert!(
            panel.command().is_none(),
            "the term matched nothing, and neither letter jumped the cursor"
        );
    }

    /// Design §1: `⏎` closes the prompt and keeps the cursor, so the next
    /// `⏎` runs the row the search landed on.
    #[test]
    fn enter_closes_the_palettes_sift_and_the_next_one_runs_the_row() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('/'));
        for character in "choose a theme".chars() {
            press(&mut state, Key::Char(character));
        }
        press(&mut state, Key::Enter);
        assert!(palette(&state).expect("still up").sift().is_none());

        press(&mut state, Key::Enter);
        assert!(
            matches!(state.overlay, Some(Overlay::Themes { .. })),
            "the row the search found is the row that ran"
        );
    }

    /// Design §1: `Esc` closes the prompt and restores the row; a second one
    /// closes the palette.
    #[test]
    fn escape_leaves_the_palettes_sift_before_the_palette() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));
        let origin = palette(&state).expect("open").selected();

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('q'));
        press(&mut state, Key::Escape);

        let panel = palette(&state).expect("the palette survives the first esc");
        assert!(panel.sift().is_none());
        assert_eq!(panel.selected(), origin);

        press(&mut state, Key::Escape);
        assert!(state.overlay.is_none());
    }

    #[test]
    fn opening_the_palette_replaces_the_message_it_covers() {
        let mut state = measured(&["a"]);
        state.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

        press(&mut state, Key::Char('?'));
        assert!(palette(&state).is_some());

        press(&mut state, Key::Escape);
        assert!(
            state.overlay.is_none(),
            "esc leaves nothing rather than peeling back to the message"
        );
    }

    #[test]
    fn the_palette_does_not_open_where_it_cannot_be_drawn() {
        let mut state = browsing(&["a"]);
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 0);

        press(&mut state, Key::Char('?'));

        assert!(
            state.overlay.is_none(),
            "a mode with nothing on screen saying so is worse than an inert key"
        );
    }

    #[test]
    fn an_open_palette_takes_the_motion_the_panel_would_have() {
        let mut state = measured(&["a", "b", "c"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "the tree did not move behind the palette"
        );
        assert_eq!(palette(&state).expect("still open").selected(), 2);

        press(&mut state, Key::Char('k'));
        assert_eq!(palette(&state).expect("still open").selected(), 1);

        press(&mut state, Key::End);
        assert_eq!(
            palette(&state).expect("still open").selected(),
            crate::commands::row_count(None) - 1
        );

        press(&mut state, Key::Home);
        assert_eq!(palette(&state).expect("still open").selected(), 1);

        press(&mut state, Key::PageDown);
        assert_eq!(
            palette(&state).expect("still open").selected(),
            15,
            "paging is routed at the handle_key layer, same as j/k"
        );

        press(&mut state, Key::PageUp);
        assert_eq!(
            palette(&state).expect("still open").selected(),
            1,
            "a page up from the second group lands inside the first"
        );
    }

    #[test]
    fn q_quits_from_inside_the_palette() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('q'));

        assert!(state.should_quit);
    }

    #[test]
    fn an_open_palette_swallows_the_keys_it_does_not_use() {
        // Each starting focus is chosen so a leaked key would be observable:
        // `t`/`Tab` need a focus their leak would actually move away from,
        // `Space` needs `Tables` on a keyspace with a table, since folding a
        // childless one leaves no visible trace at all. `Enter` is not among
        // these cases: it runs the selected row (`run_selected`'s own tests).
        let cases = [
            (Key::Char('t'), Panel::Contents),
            (Key::Tab, Panel::Contents),
            (Key::Char(' '), Panel::Tables),
        ];

        for (key, focus) in cases {
            let mut state = browsing_tables(&[("a", &["t1"])]);
            state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
            state.focus_on(focus);
            press(&mut state, Key::Char('?'));
            let before = crate::view::view_model(&state);

            let effects = press(&mut state, key);

            assert!(palette(&state).is_some(), "{key:?} dismissed the palette");
            assert_eq!(
                crate::view::view_model(&state),
                before,
                "{key:?} reached past the palette"
            );
            assert!(effects.is_empty(), "{key:?} issued {effects:?}");
        }
    }

    /// Walks the palette cursor down onto `action`'s row. `down` steps command
    /// to command, so this reaches every row a user can reach and no other.
    fn seat_on(state: &mut State, action: &str) {
        for _ in 0..crate::commands::row_count(None) {
            let seated = palette(state)
                .expect("the palette is open")
                .command()
                .is_some_and(|command| command.binding.action == action);

            if seated {
                return;
            }
            press(state, Key::Char('j'));
        }

        panic!("no row is labelled {action:?}");
    }

    #[test]
    fn enter_closes_the_palette_and_focuses_the_panel_the_row_belongs_to() {
        let mut state = measured(&["shop"]);
        state.focus_on(Panel::Tables);

        press(&mut state, Key::Char('?'));
        seat_on(&mut state, "cycle selection mode");
        press(&mut state, Key::Enter);

        assert!(state.overlay.is_none(), "the palette closes first, always");
        assert_eq!(
            state.focus(),
            Panel::Contents,
            "the row is Table contents', so Table contents is where it is pressed"
        );
    }

    #[test]
    fn enter_presses_the_rows_own_key() {
        let mut state = measured(&["shop"]);
        state.focus_on(Panel::Contents);
        let before = state.contents.mode;

        press(&mut state, Key::Char('?'));
        seat_on(&mut state, "cycle selection mode");
        press(&mut state, Key::Enter);

        assert_ne!(
            state.contents.mode, before,
            "running the row does what pressing m does"
        );
    }

    /// The one case that proves an effect survives the dispatch: a row that
    /// returns work rather than only moving state.
    #[test]
    fn enter_on_the_run_row_runs_the_buffer() {
        let mut state = measured(&["shop"]);
        state.focus_on(Panel::Queries);
        type_into(&mut state, "select * from shop.a");
        // The buffer survives a focus change; leaving Queries before `?` is
        // what lets the palette open through the real key path rather than
        // by assigning the overlay directly.
        state.focus_on(Panel::Tables);

        press(&mut state, Key::Char('?'));
        seat_on(&mut state, "run the buffer");
        let effects = press(&mut state, Key::Enter);

        assert_eq!(
            state.focus(),
            Panel::Contents,
            "a run is a read, and a read lands where the rows do"
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Execute { .. })),
            "{effects:?} carries no statement to run"
        );
    }

    /// Focus moves even when the action that follows has nothing to do — the
    /// palette is how you get to the panel as well as to the binding.
    #[test]
    fn a_row_whose_action_cannot_act_still_moves_focus() {
        let mut state = measured(&["shop"]);
        state.focus_on(Panel::Tables);

        press(&mut state, Key::Char('?'));
        seat_on(&mut state, "run the buffer");
        let effects = press(&mut state, Key::Enter);

        assert_eq!(state.focus(), Panel::Queries);
        assert!(effects.is_empty(), "an empty buffer runs nothing");
    }

    /// Design §9: "Every `Run::Inert` row leaves focus, both cursors, and
    /// every selection as they were; only the overlay changed." `State` does
    /// not derive `PartialEq`, so the comparison is by `view_model`, the same
    /// shape `an_open_palette_swallows_the_keys_it_does_not_use` uses.
    #[test]
    fn enter_on_an_inert_row_closes_the_palette_and_stops() {
        for group in CATALOGUE {
            for command in group.commands {
                if command.run != Run::Inert {
                    continue;
                }

                let mut state = measured(&["shop"]);
                state.focus_on(Panel::Tables);
                let before = crate::view::view_model(&state);

                press(&mut state, Key::Char('?'));
                seat_on(&mut state, command.binding.action);
                press(&mut state, Key::Enter);

                assert!(
                    state.overlay.is_none(),
                    "{:?} left the palette open",
                    command.binding.action
                );
                assert_eq!(
                    crate::view::view_model(&state),
                    before,
                    "{:?} changed more than the overlay",
                    command.binding.action
                );
            }
        }
    }

    /// Every row, from a state with a schema and a focused tree: the palette
    /// closes and nothing panics. The sweep is the point — a row added later
    /// is covered without anyone remembering to cover it.
    #[test]
    fn every_row_can_be_run() {
        for group in CATALOGUE {
            for command in group.commands {
                let mut state = measured(&["shop"]);
                press(&mut state, Key::Char('?'));
                seat_on(&mut state, command.binding.action);
                let _ = press(&mut state, Key::Enter);

                match command.run {
                    // "Leave the editor" is the one row whose whole action is
                    // to leave the panel it is filed under, so `⏎` on it
                    // lands back on the default panel rather than on
                    // Queries — a fair report of what Escape does there, not
                    // a bug (`a_row_filed_under_a_panel_stays_in_that_panel`
                    // carries the same exception).
                    Run::In(_) if command.binding.action == "leave the editor" => {}
                    Run::In(panel) => assert_eq!(
                        state.focus(),
                        panel,
                        "{:?} is filed under {panel:?}",
                        command.binding.action
                    ),
                    Run::Here | Run::Inert => {}
                }
            }
        }
    }

    /// A row filed under a panel names a key that panel answers, and a key
    /// that answers *there*: one that moved focus away would be a row whose
    /// heading is a lie, which is how `tab` would read if it were filed under
    /// Export rather than left inert.
    #[test]
    fn a_row_filed_under_a_panel_stays_in_that_panel() {
        for group in CATALOGUE {
            for command in group.commands {
                let Run::In(panel) = command.run else {
                    continue;
                };
                // The one honest exception: Escape's whole action in the
                // editor is to leave it, so "leave the editor" is a row that
                // leaves the panel it is filed under by design, not a row
                // whose heading lies.
                if command.binding.action == "leave the editor" {
                    continue;
                }
                let key = *command
                    .binding
                    .keys
                    .first()
                    .expect("a binding names at least one key");

                let mut state = measured(&["shop"]);
                state.focus_on(panel);
                let _ = press_stroke(&mut state, key);

                assert_eq!(
                    state.focus(),
                    panel,
                    "{:?} is filed under {panel:?} and leaves it",
                    command.binding.action
                );
            }
        }
    }

    #[test]
    fn a_frame_that_shrinks_below_the_palette_closes_it() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));
        assert!(palette(&state).is_some());

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
        );

        assert!(
            state.overlay.is_none(),
            "a palette nothing can draw must not still capture keys"
        );
    }

    /// The modal's own `MIN_W`/`MIN_H` gate is narrower than the shell's, so a
    /// frame between the two draws the panels but not the diagnostics —
    /// `command_rows() == 0` is the one signal `measured` has for "too small
    /// to draw a modal", and it must close this overlay too, not only the
    /// palette's.
    #[test]
    fn a_frame_that_shrinks_below_the_diagnostics_modal_closes_it() {
        let (mut state, _) = init(Config::default(), vec![diagnostic("one")], Stamp::default());
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        assert!(matches!(state.overlay, Some(Overlay::Diagnostics(_))));

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
        );

        assert!(
            state.overlay.is_none(),
            "a diagnostics modal nothing can draw must not still capture every key"
        );
    }

    fn index_of(built_in: BuiltIn) -> usize {
        BuiltIn::ALL
            .iter()
            .position(|b| *b == built_in)
            .expect("listed")
    }

    #[test]
    fn the_picker_opens_on_the_theme_in_force() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinFrappe);

        ctrl(&mut state, Key::Char('t'));

        assert_eq!(
            theme_picker(&state),
            Some(index_of(BuiltIn::Ported(Ported::CatppuccinFrappe)))
        );
    }

    fn custom_theme(name: &str) -> crate::config::CustomTheme {
        crate::config::CustomTheme {
            name: crate::theme::ThemeName::try_new(name).expect("a plain slug is a valid name"),
            extends: BuiltIn::Ours(Ours::DefaultDark),
            colors: std::collections::BTreeMap::new(),
        }
    }

    /// Every picker test above runs on `Config::default()`, so the index is
    /// always `< BuiltIn::ALL.len()` and the custom branch of
    /// `theme_picker_index` — the `index - BuiltIn::ALL.len()` arithmetic and
    /// the round trip through `ThemeName` equality — never executes. This is
    /// the behaviour custom themes exist for.
    #[test]
    fn the_picker_seats_on_a_custom_theme_already_in_force() {
        let mut state = measured(&["a"]);
        let theme = custom_theme("midnight");
        state.config.themes = vec![theme.clone()];
        state.theme = ThemeChoice::Custom(theme.name.clone());

        ctrl(&mut state, Key::Char('t'));

        assert_eq!(
            theme_picker(&state),
            Some(BuiltIn::ALL.len()),
            "a custom theme sits right after the built-ins, in config order"
        );
    }

    #[test]
    fn moving_past_the_built_ins_previews_a_custom_theme() {
        let mut state = measured(&["a"]);
        let theme = custom_theme("midnight");
        state.config.themes = vec![theme.clone()];

        ctrl(&mut state, Key::Char('t'));
        for _ in 0..BuiltIn::ALL.len() {
            press(&mut state, Key::Char('j'));
        }

        assert_eq!(theme_picker(&state), Some(BuiltIn::ALL.len()));
        assert_eq!(
            state.theme,
            ThemeChoice::Custom(theme.name.clone()),
            "the row past the last built-in previews the config's own theme"
        );
    }

    #[test]
    fn moving_the_cursor_previews_the_theme() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.theme,
            ThemeChoice::from(BuiltIn::ALL[1]),
            "the frame repaints as you browse"
        );
    }

    #[test]
    fn enter_keeps_the_previewed_theme() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Enter);

        assert_eq!(state.theme, ThemeChoice::from(BuiltIn::ALL[1]));
        assert!(state.overlay.is_none());
    }

    #[test]
    fn enter_saves_the_theme_it_keeps() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));
        let effects = press(&mut state, Key::Enter);

        assert_eq!(
            effects,
            vec![Effect::SaveTheme(ThemeChoice::from(BuiltIn::ALL[1]))],
            "the theme a restart opens on is the one the picker was left on"
        );
    }

    /// Opening the picker to look at the list and confirming out of it must
    /// not touch the user's config (theme persistence design §2).
    #[test]
    fn enter_on_the_theme_already_in_force_saves_nothing() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        let effects = press(&mut state, Key::Enter);

        assert!(effects.is_empty(), "{effects:?}");
    }

    #[test]
    fn a_previewed_theme_is_not_a_saved_one() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        let effects = press(&mut state, Key::Char('j'));

        assert!(
            effects.is_empty(),
            "moving the cursor previews; it does not save"
        );
    }

    #[test]
    fn escape_saves_nothing() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));
        let effects = press(&mut state, Key::Escape);

        assert!(effects.is_empty(), "{effects:?}");
    }

    #[test]
    fn a_click_that_dismisses_the_picker_saves_nothing() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));
        let effects = update(
            &mut state,
            Msg::Pointer(Pointer::Click(Click::Panel(Panel::Tables))),
        );

        assert!(effects.is_empty(), "a click restores, the same as Esc");
    }

    #[test]
    fn a_frame_that_shrinks_below_the_picker_saves_nothing() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));
        let effects = update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
        );

        assert!(effects.is_empty(), "{effects:?}");
    }

    /// `Enter` inside a live sift ends the search and leaves the picker open,
    /// so the confirming `Enter` is the next one.
    #[test]
    fn ending_a_search_with_enter_saves_nothing_yet() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('/'));
        type_into(&mut state, "quill");
        let ending = press(&mut state, Key::Enter);

        assert!(ending.is_empty(), "the search closed; the picker did not");
        assert!(matches!(state.overlay, Some(Overlay::Themes { .. })));

        let confirming = press(&mut state, Key::Enter);

        assert_eq!(confirming.len(), 1, "and this one is the confirmation");
    }

    #[test]
    fn a_failed_save_warns_and_logs_and_keeps_the_theme() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

        update(
            &mut state,
            Msg::ThemeSaveFailed {
                reason: "/home/x/.config/lazycassandra/config.toml: permission denied".to_owned(),
            },
        );

        let Some(Overlay::Message(message)) = &state.overlay else {
            panic!("a save the user asked for and did not get is worth one line");
        };
        assert_eq!(message.severity, Severity::Warning);
        assert!(message.message.contains("permission denied"));

        let entry = state.log.entries().next().expect("the save entry");
        let Outcome::Failed { detail } = entry.outcome() else {
            panic!("the save failed");
        };
        assert!(detail.contains("permission denied"));
        assert_eq!(
            state.theme,
            ThemeChoice::Ported(Ported::CatppuccinMocha),
            "a save that failed is not a reason to take back the theme the user chose"
        );
    }

    #[test]
    fn escape_restores_the_theme_the_picker_opened_on() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('k'));
        press(&mut state, Key::Escape);

        assert_eq!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));
        assert!(state.overlay.is_none());
    }

    #[test]
    fn the_picker_cursor_cannot_run_off_the_list() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        for _ in 0..BuiltIn::ALL.len() * 2 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(
            state.theme,
            ThemeChoice::from(BuiltIn::ALL[BuiltIn::ALL.len() - 1])
        );
        assert_eq!(
            theme_picker(&state),
            Some(BuiltIn::ALL.len() - 1),
            "the cursor itself must stop at the last row, not merely the theme it read out"
        );

        for _ in 0..BuiltIn::ALL.len() * 2 {
            press(&mut state, Key::Char('k'));
        }
        assert_eq!(state.theme, ThemeChoice::from(BuiltIn::ALL[0]));
        assert_eq!(theme_picker(&state), Some(0));
    }

    #[test]
    fn paging_reaches_the_ends_of_the_theme_list() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::PageDown);

        assert_eq!(
            theme_picker(&state),
            Some(BuiltIn::ALL.len() - 1),
            "the list is shorter than the block drawing it, so one page is all of it"
        );
        assert_eq!(
            state.theme,
            ThemeChoice::from(BuiltIn::ALL[BuiltIn::ALL.len() - 1]),
            "a page is a move, and every move previews"
        );

        press(&mut state, Key::PageUp);

        assert_eq!(theme_picker(&state), Some(0));
        assert_eq!(state.theme, ThemeChoice::from(BuiltIn::ALL[0]));
    }

    #[test]
    fn ctrl_d_and_ctrl_u_page_the_picker_too() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        ctrl(&mut state, Key::Char('d'));

        assert_eq!(theme_picker(&state), Some(BuiltIn::ALL.len() - 1));

        ctrl(&mut state, Key::Char('u'));

        assert_eq!(theme_picker(&state), Some(0));
    }

    #[test]
    fn the_ends_are_reachable_in_the_picker() {
        let last = BuiltIn::ALL.len() - 1;

        for (start, end) in [(Key::Home, Key::End), (Key::Char('g'), Key::Char('G'))] {
            let mut state = measured(&["a"]);
            ctrl(&mut state, Key::Char('t'));
            press(&mut state, Key::Char('j'));

            press(&mut state, end);
            assert_eq!(theme_picker(&state), Some(last), "{end:?} reaches the last");
            assert_eq!(
                state.theme,
                ThemeChoice::from(BuiltIn::ALL[last]),
                "and previews it, as every other motion in the picker does"
            );

            press(&mut state, start);
            assert_eq!(theme_picker(&state), Some(0), "{start:?} reaches the first");
        }
    }

    #[test]
    fn q_still_quits_with_the_picker_open() {
        let mut state = measured(&["a"]);

        ctrl(&mut state, Key::Char('t'));
        let effects = press(&mut state, Key::Char('q'));

        assert!(
            state.should_quit,
            "a key that means quit everywhere but one overlay is not one anyone can trust"
        );
        assert!(!effects.is_empty(), "quitting cancels what is in flight");
    }

    /// The panel guards above sweep states with no overlay, so an overlay's
    /// own bar needs its own sweep — the picker's is the only bar in the
    /// program whose keys none of them reach.
    #[test]
    fn every_hint_the_pickers_bar_shows_does_something() {
        // Mid-list, so every direction and both ends have somewhere to go.
        let seated = || {
            let mut state = measured(&["a"]);
            ctrl(&mut state, Key::Char('t'));
            press(&mut state, Key::Char('j'));
            press(&mut state, Key::Char('j'));
            state
        };

        let shown = crate::view::hints::bar(&seated());
        let claimed = crate::view::hints::THEMES
            .iter()
            .chain(crate::view::hints::THEMES_GLOBAL);

        assert_eq!(
            shown.panel.len() + shown.global.len(),
            crate::view::hints::THEMES.len() + crate::view::hints::THEMES_GLOBAL.len()
        );

        for binding in claimed {
            for &key in binding.keys {
                let mut state = seated();
                let before = crate::view::view_model(&state);

                let effects = press_stroke(&mut state, key);

                assert!(
                    crate::view::view_model(&state) != before
                        || !effects.is_empty()
                        || state.should_quit,
                    "the picker's bar offers '{} {}' and pressing it changes nothing",
                    crate::view::hints::label(binding.keys),
                    binding.action
                );
            }
        }
    }

    #[test]
    fn moving_the_picker_cursor_asks_for_a_repaint() {
        let mut state = measured(&["a"]);
        ctrl(&mut state, Key::Char('t'));
        state.dirty = false;

        press(&mut state, Key::Char('j'));

        assert!(
            state.dirty,
            "the preview is the frame underneath, which nothing else redraws"
        );
    }

    #[test]
    fn a_frame_too_small_for_a_list_refuses_the_picker() {
        let mut state = State::default();

        ctrl(&mut state, Key::Char('t'));

        assert!(
            state.overlay.is_none(),
            "a mode with nothing on screen saying so is worse than no mode"
        );
    }

    #[test]
    fn a_frame_that_shrinks_below_the_picker_closes_it_and_restores_the_theme() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('k'));
        assert_ne!(
            state.theme,
            ThemeChoice::Ported(Ported::CatppuccinMocha),
            "previewing a different theme before the frame shrinks"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
        );

        assert!(
            state.overlay.is_none(),
            "a picker nothing can draw must not still capture keys"
        );
        assert_eq!(
            state.theme,
            ThemeChoice::Ported(Ported::CatppuccinMocha),
            "the preview must not strand itself when its own room disappears"
        );
    }

    #[test]
    fn a_message_arriving_over_an_open_picker_restores_the_theme() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('k'));
        assert_ne!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));

        raise(
            &mut state,
            Message {
                title: "Cannot connect".to_owned(),
                message: "no route to host".to_owned(),
                severity: Severity::Danger,
            },
        );

        assert_eq!(
            state.theme,
            ThemeChoice::Ported(Ported::CatppuccinMocha),
            "the half-previewed theme must not strand itself under the failure"
        );
        assert!(matches!(state.overlay, Some(Overlay::Message(_))));
    }

    #[test]
    fn a_click_dismisses_the_picker_and_restores_the_theme() {
        let mut state = measured(&["a"]);
        state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('k'));
        assert_ne!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));

        point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

        assert!(state.overlay.is_none());
        assert_eq!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));
    }

    #[test]
    fn a_frame_that_makes_room_for_the_palette_repaints_the_bar() {
        let mut state = State::default();
        assert_eq!(
            state.geometry.command_rows(),
            0,
            "the default geometry is where `?` starts out inert"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 14)),
        );

        assert!(
            state.dirty,
            "command_rows went from zero to non-zero, so the bar's `? commands` \
             hint just appeared and nothing scheduled the repaint that shows it"
        );
    }

    #[test]
    fn j_and_k_walk_the_keyspace_list() {
        let mut state = browsing(&["a", "b", "c"]);

        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), 2);

        press(&mut state, Key::Char('k'));

        assert_eq!(state.tables.cursor.index(), 1);
    }

    #[test]
    fn the_arrow_keys_do_what_j_and_k_do() {
        let mut state = browsing(&["a", "b"]);

        press(&mut state, Key::Down);

        assert_eq!(state.tables.cursor.index(), 1);

        press(&mut state, Key::Up);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn the_selection_cannot_run_off_a_list_it_has_not_loaded() {
        let mut state = connected(Posture::ReadOnly);

        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "there are no rows to move onto"
        );
    }

    #[test]
    fn motion_belongs_to_the_focused_panel_only() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus_on(Panel::Ddl);

        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn escape_dismisses_the_overlay() {
        let mut state = browsing(&["a"]);
        state.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
    }

    #[test]
    fn q_quits() {
        let mut state = browsing(&["a"]);

        let effects = press(&mut state, Key::Char('q'));

        assert!(state.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);
    }

    /// Puts the panel straight on the Logs tab without walking a key
    /// sequence to it: what these tests are covering is the Logs tab itself,
    /// not the cycle that reaches it (that is `Tab`'s job, covered
    /// separately).
    fn on_the_trail() -> State {
        let mut state = State::default();
        state.focus_on(Panel::Queries);
        state.queries.show(QueriesTab::Logs);
        state
    }

    // The panel's unfocused height is a function of its buffer (layout.rs), so an
    // unfocused Logs tab would show one thing and be as tall as another
    // (query log design §5.2).
    #[test]
    fn leaving_the_panel_puts_the_buffer_back() {
        let mut state = on_the_trail();

        press(&mut state, Key::Char('1'));

        assert_eq!(state.focus(), Panel::Tables);
        assert_eq!(state.queries.tab(), QueriesTab::Cql);
    }

    #[test]
    fn the_trail_takes_no_text() {
        let mut state = on_the_trail();

        press(&mut state, Key::Char('x'));

        assert_eq!(state.queries.text(), "", "the buffer is untouched");
    }

    #[test]
    fn q_quits_from_the_trail() {
        let mut state = on_the_trail();

        let effects = press(&mut state, Key::Char('q'));

        assert!(effects.contains(&Effect::Quit));
    }

    /// A trail with one `Outcome::Cancelled` entry per statement, focused on
    /// the Logs tab so `/`, `n`, `v` and `y` reach it directly.
    fn trail_of(statements: &[&str]) -> State {
        let mut state = on_the_trail();
        for statement in statements {
            state.log.push(Entry::new(
                Stamp::default(),
                LogSubject::Sentence((*statement).to_owned()),
                Outcome::Cancelled,
            ));
        }
        state
    }

    /// Opens a search over the focused panel and types `term` into it, the
    /// same two steps `n_walks_to_the_next_match_and_wraps` and its
    /// neighbours drive press by press.
    fn search_for(state: &mut State, term: &str) {
        press(state, Key::Char('/'));
        type_into(state, term);
    }

    #[test]
    fn the_trail_is_searchable() {
        let mut state = trail_of(&["SELECT one", "SELECT two", "DELETE three"]);

        search_for(&mut state, "DELETE");

        assert_eq!(state.log.cursor.index(), 2);
    }

    #[test]
    fn n_walks_the_trails_matches_and_wraps() {
        let mut state = trail_of(&["SELECT one", "DELETE two", "DELETE three"]);
        state.apply_search(Searched::Queries, Search::new("DELETE"));

        press(&mut state, Key::Char('n'));
        assert_eq!(state.log.cursor.index(), 1);

        press(&mut state, Key::Char('n'));
        assert_eq!(state.log.cursor.index(), 2);

        press(&mut state, Key::Char('n'));
        assert_eq!(state.log.cursor.index(), 1, "wraps back to the first match");
    }

    #[test]
    fn escape_over_the_trail_clears_its_search() {
        let mut state = trail_of(&["SELECT one"]);
        search_for(&mut state, "one");

        press(&mut state, Key::Escape);

        assert_eq!(state.search(Searched::Queries), None);
    }

    // What lands on the clipboard is the pair, which is what makes it paste
    // into a ticket (query log design §5.3).
    #[test]
    fn yanking_an_entry_copies_its_statement_and_its_outcome() {
        let mut state = trail_of(&["SELECT one"]);

        let effects = press(&mut state, Key::Char('y'));

        let Some(Effect::Yank(copied)) = effects
            .into_iter()
            .find(|effect| matches!(effect, Effect::Yank(_)))
        else {
            panic!("a yank");
        };
        assert!(copied.contains("SELECT one"));
        assert!(copied.contains("cancelled"), "{copied}");
    }

    #[test]
    fn yanking_a_visual_selection_joins_every_selected_entrys_two_rows() {
        let mut state = trail_of(&["SELECT one", "SELECT two", "SELECT three"]);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Char('y'));

        let Some(Effect::Yank(copied)) = effects
            .into_iter()
            .find(|effect| matches!(effect, Effect::Yank(_)))
        else {
            panic!("a yank");
        };
        assert_eq!(
            copied.matches("cancelled").count(),
            2,
            "one outcome row per selected entry: {copied}"
        );
        assert!(copied.contains("SELECT one"));
        assert!(copied.contains("SELECT two"));
        assert!(!copied.contains("SELECT three"), "{copied}");
    }

    #[test]
    fn yanking_the_trail_lights_the_entries_it_took() {
        let mut state = trail_of(&["SELECT one", "SELECT two"]);

        press(&mut state, Key::Char('y'));

        let flash = state
            .flash
            .expect("a yank on the Logs tab lights something");
        assert_eq!(flash.panel(), Panel::Queries);
        assert_eq!(flash.rows(), &Ranges::of(Range::new(0, 0)));
    }

    #[test]
    fn y_on_the_cql_tab_yanks_nothing() {
        let mut state = trail_of(&["SELECT one"]);
        state.queries.show(QueriesTab::Cql);

        assert!(press(&mut state, Key::Char('y')).is_empty());
    }

    /// The regression `Tab`'s cycle exists to prevent: the panel's own
    /// number is not carved out of what the CQL buffer takes as text, or
    /// `WHERE id = 4` could not be typed from inside it.
    #[test]
    fn a_number_typed_into_the_focused_cql_buffer_inserts_the_digit() {
        let mut state = State::default();
        state.focus_on(Panel::Queries);

        press(&mut state, Key::Char('4'));

        assert_eq!(state.focus(), Panel::Queries, "focus is the mode");
        assert_eq!(state.queries.text(), "4");
    }

    #[test]
    fn tab_from_queries_cql_lands_on_queries_logs() {
        let mut state = State::default();
        state.focus_on(Panel::Queries);
        assert_eq!(state.queries.tab(), QueriesTab::Cql);

        press(&mut state, Key::Tab);

        assert_eq!(
            state.focus(),
            Panel::Queries,
            "the cycle has not left the panel yet"
        );
        assert_eq!(state.queries.tab(), QueriesTab::Logs);
    }

    #[test]
    fn tab_from_queries_logs_lands_on_tables_and_reverts_to_cql() {
        let mut state = on_the_trail();

        press(&mut state, Key::Tab);

        assert_eq!(state.focus(), Panel::Tables);
        assert_eq!(
            state.queries.tab(),
            QueriesTab::Cql,
            "leaving the panel still reverts its tab"
        );
    }

    #[test]
    fn shift_tab_from_tables_lands_on_queries_logs() {
        let mut state = State::default();

        press(&mut state, Key::BackTab);

        assert_eq!(
            state.focus(),
            Panel::Queries,
            "stepping back from the first stop wraps to the last"
        );
        assert_eq!(
            state.queries.tab(),
            QueriesTab::Logs,
            "arrival sets the tab explicitly, or focus_on's revert of the \
             panel it just left would undo it"
        );
    }

    #[test]
    fn tab_steps_ddls_two_tabs_as_separate_stops() {
        let mut state = browsing_a_table();
        state.focus_on(Panel::Ddl);
        assert_eq!(state.ddl.tab(), DdlTab::Ddl);

        press(&mut state, Key::Tab);

        assert_eq!(
            state.focus(),
            Panel::Ddl,
            "the cycle has not left the panel yet"
        );
        assert_eq!(state.ddl.tab(), DdlTab::Columns);
    }

    #[test]
    fn a_full_forward_cycle_returns_to_where_it_started() {
        let mut state = browsing_a_table();
        state.focus_on(Panel::Tables);
        let stops = Landing::all(&state).len();

        for _ in 0..stops {
            press(&mut state, Key::Tab);
        }

        assert_eq!(state.focus(), Panel::Tables);
        assert_eq!(
            state.queries.tab(),
            QueriesTab::Cql,
            "the cycle passed through Queries and came out the other side"
        );
    }

    #[test]
    fn shift_tab_undoes_tab_from_every_stop() {
        let state = browsing_a_table();

        for stop in Landing::all(&state) {
            let mut probe = state.clone();
            stop.arrive(&mut probe);

            press(&mut probe, Key::Tab);
            press(&mut probe, Key::BackTab);

            assert_eq!(
                Landing::current(&probe),
                stop,
                "{stop:?} did not return to itself"
            );
        }
    }

    fn ctrl(state: &mut State, key: Key) -> Vec<Effect> {
        update(state, Msg::Input(KeyPress::ctrl(key)))
    }

    #[test]
    fn ctrl_d_and_ctrl_u_page_the_focused_panel() {
        let mut paged = measured(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"]);
        let mut chorded = paged.clone();

        press(&mut paged, Key::PageDown);
        ctrl(&mut chorded, Key::Char('d'));

        assert_eq!(chorded.tables.cursor.index(), paged.tables.cursor.index());
        assert!(chorded.tables.cursor.index() > 0, "a page was available");

        press(&mut paged, Key::PageUp);
        ctrl(&mut chorded, Key::Char('u'));

        assert_eq!(chorded.tables.cursor.index(), paged.tables.cursor.index());
    }

    #[test]
    fn ctrl_d_pages_an_open_palette_rather_than_the_panel_underneath() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        ctrl(&mut state, Key::Char('d'));

        assert_eq!(
            palette(&state).expect("still open").selected(),
            15,
            "ctrl-d is pgdn by another name, and pgdn pages the palette"
        );
        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn ctrl_d_pages_the_editor_the_way_pgdn_does() {
        let mut paged = editing("a\nb\nc\nd\ne\nf");
        let mut chorded = paged.clone();

        press(&mut paged, Key::PageUp);
        ctrl(&mut chorded, Key::Char('u'));

        assert_eq!(chorded.queries.cursor(), paged.queries.cursor());
        assert_eq!(chorded.queries.text(), paged.queries.text(), "not text");
    }

    #[test]
    fn ctrl_d_is_inert_under_a_search_prompt() {
        let mut state = browsing(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        press(&mut state, Key::Char('/'));
        let before = crate::view::view_model(&state);

        ctrl(&mut state, Key::Char('d'));

        assert_eq!(
            crate::view::view_model(&state),
            before,
            "the prompt pages nothing, and the chord is not text either"
        );
    }

    #[test]
    fn ctrl_e_hands_the_queries_buffer_to_an_editor() {
        let mut state = State::default();
        state.focus_on(Panel::Queries);
        state.queries.replace("SELECT * FROM t");

        let effects = ctrl(&mut state, Key::Char('e'));

        assert_eq!(
            effects,
            vec![Effect::OpenEditor {
                buffer: "SELECT * FROM t".to_owned()
            }]
        );
    }

    /// The chord means one thing now: it scrolled the Table DDL viewport as
    /// well until that half was retired (docs/shortcuts.md §4).
    #[test]
    fn ctrl_e_is_inert_outside_queries() {
        for focus in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let mut state = State::default();
            state.focus_on(focus);
            let before = crate::view::view_model(&state);

            assert!(
                ctrl(&mut state, Key::Char('e')).is_empty(),
                "{focus:?} claims no ctrl-e"
            );
            assert_eq!(
                crate::view::view_model(&state),
                before,
                "{focus:?} moved under a chord it does not claim"
            );
        }
    }

    #[test]
    fn ctrl_e_under_an_overlay_does_nothing() {
        let overlays = [
            Overlay::Commands(CommandsPanel::opened()),
            Overlay::Themes {
                cursor: Cursor::default(),
                restored: ThemeChoice::default(),
                sift: None,
            },
        ];

        for overlay in overlays {
            let mut state = State::default();
            state.focus_on(Panel::Queries);
            state.overlay = Some(overlay);

            assert!(ctrl(&mut state, Key::Char('e')).is_empty());
        }
    }

    #[test]
    fn a_closed_editor_replaces_the_buffer() {
        let mut state = State::default();
        state.queries.replace("SELECT");

        let effects = update(
            &mut state,
            Msg::EditorClosed {
                buffer: "SELECT *\nFROM t".to_owned(),
            },
        );

        assert!(effects.is_empty());
        assert_eq!(state.queries.text(), "SELECT *\nFROM t");
        assert!(state.dirty, "the buffer changed, so the frame is stale");
    }

    #[test]
    fn a_failed_editor_says_so_and_keeps_the_buffer() {
        let mut state = State::default();
        state.queries.replace("SELECT");

        let effects = update(
            &mut state,
            Msg::EditorFailed {
                reason: "could not run hx: No such file or directory".to_owned(),
            },
        );

        assert!(effects.is_empty());
        assert_eq!(state.queries.text(), "SELECT");
        assert!(matches!(
            state.overlay,
            Some(Overlay::Message(Message {
                severity: Severity::Danger,
                ..
            }))
        ));
    }

    /// The modal stays — the user is waiting on it right now — and gets a
    /// mirror in the trail alongside it (query log design §8).
    #[test]
    fn an_editor_that_would_not_run_leaves_its_reason_in_the_trail() {
        let mut state = State::default();

        let _effects = update(
            &mut state,
            Msg::EditorFailed {
                reason: "no $EDITOR set".to_owned(),
            },
        );

        assert!(
            matches!(state.overlay, Some(Overlay::Message(_))),
            "the modal stays"
        );
        let Outcome::Failed { detail } = state.log.entries().last().unwrap().outcome() else {
            panic!("the editor failed");
        };
        assert!(detail.contains("no $EDITOR set"), "{detail}");
    }

    /// A state focused on Queries with `text` typed into it, `\n` meaning
    /// Enter — the same convention `editor.rs`'s own helper uses.
    fn editing(text: &str) -> State {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.focus_on(Panel::Queries);
        type_into(&mut state, text);
        state
    }

    /// Types `text` into the focused editor, `\n` meaning Enter — the same
    /// convention `editor.rs`'s own helper uses.
    fn type_into(state: &mut State, text: &str) {
        for character in text.chars() {
            if character == '\n' {
                press(state, Key::Enter);
            } else {
                press(state, Key::Char(character));
            }
        }
    }

    #[test]
    fn a_letter_typed_in_queries_is_text_not_a_shortcut() {
        // Every letter here is bound somewhere: t/a/c/u focus a panel, j/k/h/l
        // move, q quits. In the editor they are all just text.
        let state = editing("tacujkhlq");

        assert_eq!(state.focus(), Panel::Queries, "no letter moved focus");
        assert_eq!(state.queries.text(), "tacujkhlq");
    }

    #[test]
    fn y_is_text_in_the_editor() {
        let mut state = editing("");

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(state.queries.text(), "y");
        assert!(effects.is_empty(), "not reachable from Queries (§1)");
    }

    /// `?` is a CQL bind marker before it is a shortcut, so the editor has to
    /// win it. Neither parent of the merge that brought the palette and the
    /// editor together could test this: each was the only mode in its branch.
    #[test]
    fn a_question_mark_typed_in_queries_is_a_bind_marker_not_the_palette() {
        let mut state = measured(&["a"]);
        state.focus_on(Panel::Queries);

        press(&mut state, Key::Char('?'));

        assert!(
            state.overlay.is_none(),
            "the editor claimed it before the palette could open"
        );
        assert_eq!(state.queries.text(), "?");
    }

    /// The two mode guards in `handle_key`, in the order they are written: an
    /// overlay the user opened deliberately outranks the focused panel's claim
    /// on printable keys (docs/shortcuts.md §2).
    #[test]
    fn an_open_palette_outranks_a_panel_that_takes_text() {
        let mut state = measured(&["a", "b", "c"]);
        press(&mut state, Key::Char('?'));
        state.focus_on(Panel::Queries);

        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.queries.text(),
            "",
            "the palette took the key rather than the editor"
        );
        assert!(state.overlay.is_some(), "and it is still open");
    }

    #[test]
    fn q_does_not_quit_while_the_editor_has_focus() {
        let mut state = browsing(&["a"]);
        state.focus_on(Panel::Queries);

        let effects = press(&mut state, Key::Char('q'));

        assert!(!state.should_quit);
        assert_eq!(effects, Vec::new());
        assert_eq!(state.queries.text(), "q");
    }

    #[test]
    fn escape_leaves_the_editor_for_tables() {
        let mut state = editing("SELECT");

        press(&mut state, Key::Escape);

        assert_eq!(state.focus(), Panel::Tables);
        assert_eq!(state.queries.text(), "SELECT", "leaving keeps the buffer");
    }

    /// The last rung is the panel, not the editor: a key that backs out of
    /// Queries and sits inert in DDL beside it means two things
    /// (docs/shortcuts.md §2).
    #[test]
    fn escape_leaves_every_panel_for_tables() {
        for panel in Panel::ALL {
            let mut state = populated(panel);

            press(&mut state, Key::Escape);

            assert_eq!(state.focus(), Panel::Tables, "esc from {panel:?}");
        }
    }

    #[test]
    fn escape_dismisses_the_overlay_before_it_leaves_the_editor() {
        let mut state = editing("SELECT");
        state.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
        assert_eq!(state.focus(), Panel::Queries, "one esc does one thing");
    }

    #[test]
    fn tab_still_leaves_the_editor() {
        let mut state = editing("SELECT");

        press(&mut state, Key::Tab);

        assert_eq!(
            state.focus(),
            Panel::Queries,
            "Queries' own second stop, not yet the next panel"
        );
        assert_eq!(state.queries.tab(), QueriesTab::Logs, "not typed as text");
        assert_eq!(state.queries.text(), "SELECT");
    }

    #[test]
    fn a_modified_key_is_still_not_text() {
        let mut state = browsing(&["a"]);
        state.focus_on(Panel::Queries);

        update(
            &mut state,
            Msg::Input(KeyPress {
                key: Key::Char('a'),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
            }),
        );

        assert_eq!(
            state.queries.text(),
            "",
            "ctrl-a is a binding that does not exist yet, not a slower a"
        );
    }

    #[test]
    fn typing_outside_queries_still_focuses_a_panel() {
        let mut state = browsing(&["a"]);

        press(&mut state, Key::Char('3'));

        assert_eq!(state.focus(), Panel::Contents);
        assert_eq!(state.queries.text(), "", "nothing leaked into the buffer");
    }

    #[test]
    fn the_editing_keys_reach_the_buffer() {
        let mut state = editing("SELECT *");

        press(&mut state, Key::Backspace);
        assert_eq!(state.queries.text(), "SELECT ");

        press(&mut state, Key::Enter);
        assert_eq!(state.queries.text(), "SELECT \n");

        press(&mut state, Key::Left);
        press(&mut state, Key::Delete);
        assert_eq!(state.queries.text(), "SELECT ", "delete ate the newline");
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_line_in_the_editor() {
        let mut state = editing("SELECT *\nFROM t");

        press(&mut state, Key::Home);
        assert_eq!(
            state.queries.cursor(),
            crate::editor::Position { line: 1, column: 0 }
        );

        press(&mut state, Key::End);
        assert_eq!(
            state.queries.cursor(),
            crate::editor::Position { line: 1, column: 6 },
            "the line's end, not the buffer's"
        );
    }

    #[test]
    fn a_loaded_schema_pulls_the_selection_back_inside_it() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        for _ in 0..5 {
            state.tables.cursor.down(6);
        }

        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(&["shop", "typezoo"])),
                took: Duration::ZERO,
            },
        );

        assert_eq!(
            state.tables.cursor.index(),
            1,
            "a selection past the end of a smaller schema must be pulled in"
        );
    }

    /// `browsing_tables` with the cursor stepped onto a table, which is what
    /// the Columns tab needs in order to exist.
    fn browsing_a_table() -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        state
    }

    fn browsing_tables(keyspaces: &[(&str, &[&str])]) -> State {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(keyspaces)),
                took: Duration::ZERO,
            },
        );
        state
    }

    /// Two keyspaces, one holding `orders`, both folded — so `tree_rows` is
    /// 2 without a search and a search that matches forces the right one
    /// open.
    fn loaded() -> State {
        browsing_tables(&[("shop", &["orders"]), ("billing", &[])])
    }

    fn row_count(state: &State) -> usize {
        state
            .schema()
            .map_or(0, |schema| state.tables.rows(schema).len())
    }

    #[test]
    fn enter_toggles_a_keyspace() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

        let effects = press(&mut state, Key::Enter);
        assert_eq!(row_count(&state), 3, "⏎ did not unfold");
        assert!(
            effects.is_empty(),
            "unfolding is a pure state change and must emit no effect (table-list design §2)"
        );

        let effects = press(&mut state, Key::Enter);
        assert_eq!(row_count(&state), 1, "⏎ did not fold");
        assert!(
            effects.is_empty(),
            "folding is a pure state change and must emit no effect (table-list design §2)"
        );
    }

    #[test]
    fn enter_is_inert_while_more_than_one_row_is_selected() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Enter);

        assert!(
            effects.is_empty(),
            "a range names more than one thing to open"
        );
        assert_eq!(state.focus(), Panel::Tables, "and focus stays put");
    }

    #[test]
    fn enter_still_reads_a_table_with_one_row_selected() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));

        let effects = press(&mut state, Key::Enter);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Execute { .. })),
            "pressing v must not blank the panel before anything has been extended"
        );
    }

    #[test]
    fn a_click_clears_the_range_it_lands_beside() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));

        update(
            &mut state,
            Msg::Pointer(Pointer::Click(Click::Row {
                panel: Panel::Tables,
                row: 2,
                column: None,
            })),
        );

        assert!(
            !state.tables.selection.is_active(),
            "pointing at one row is saying: this one"
        );
        assert_eq!(state.tables.cursor.index(), 2);
    }

    /// One key for one action: `Space` was `Enter`'s synonym and is now free
    /// (docs/shortcuts.md §2, §3).
    #[test]
    fn space_is_not_a_second_name_for_enter() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

        let effects = press(&mut state, Key::Char(' '));

        assert_eq!(row_count(&state), 1, "space unfolded a keyspace");
        assert!(effects.is_empty(), "space read a table");
    }

    #[test]
    fn l_and_right_open_a_keyspace() {
        for key in [Key::Char('l'), Key::Right] {
            let mut state = browsing_tables(&[("shop", &["orders"])]);

            let effects = press(&mut state, key);

            assert_eq!(row_count(&state), 2, "{key:?} did not open");
            assert!(
                effects.is_empty(),
                "opening is a pure state change and must emit no effect (table-list design §2)"
            );
        }
    }

    #[test]
    fn h_and_left_close_a_keyspace() {
        for key in [Key::Char('h'), Key::Left] {
            let mut state = browsing_tables(&[("shop", &["orders"])]);
            press(&mut state, Key::Char('l'));

            let effects = press(&mut state, key);

            assert_eq!(row_count(&state), 1, "{key:?} did not close");
            assert!(
                effects.is_empty(),
                "closing is a pure state change and must emit no effect (table-list design §2)"
            );
        }
    }

    /// `l` on a table row is `⏎`: *move right* out of the tree's last level
    /// is the panel to its right (docs/shortcuts.md §4).
    #[test]
    fn l_and_right_read_the_table_under_the_cursor() {
        for key in [Key::Char('l'), Key::Right] {
            let mut state = browsing_tables(&[("shop", &["orders"])]);
            press(&mut state, key);
            press(&mut state, key);

            let effects = press(&mut state, key);

            assert!(
                effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::Execute { .. })),
                "{key:?} on a table row did not read it"
            );
            assert_eq!(state.focus(), Panel::Contents, "{key:?}");
        }
    }

    #[test]
    fn l_reads_a_table_under_a_search_the_way_enter_does() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.apply_search(Searched::Tables, Search::new("orders"));

        state.tables.cursor.go_to(1, row_count(&state));
        let effects = press(&mut state, Key::Char('l'));

        assert!(
            !effects.is_empty(),
            "only the fold path is swallowed under a search"
        );
    }

    #[test]
    fn l_on_a_table_is_inert_over_a_range_of_many() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Char('l'));

        assert!(
            effects.is_empty(),
            "reading a table needs one table, as it does for enter"
        );
    }

    #[test]
    fn h_from_a_table_steps_out_to_its_keyspace_before_folding() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        assert_eq!(state.tables.cursor.index(), 1);

        press(&mut state, Key::Char('h'));

        assert_eq!(state.tables.cursor.index(), 0);
        assert_eq!(row_count(&state), 3, "stepping out must not fold");
    }

    /// A state focused on Contents, showing a page of `columns` columns each
    /// two characters wide, in a grid `width` characters across.
    fn grid_of(columns: usize, width: usize) -> State {
        grid_of_rows(columns, 1, width)
    }

    fn grid_of_rows(columns: usize, rows: usize, width: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: (0..columns).map(|i| format!("c{i}")).collect(),
                rows: (0..rows)
                    .map(|_| Row {
                        values: (0..columns).map(|_| CqlValue::Int(1)).collect(),
                    })
                    .collect(),
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );
        state.focus_on(Panel::Contents);
        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(width)),
        );
        state
    }

    /// [`grid_of`] in cell mode, where `h`/`l` move the column cursor. Row
    /// mode moves the window under it instead (selection modes design §3).
    fn cell_grid_of(columns: usize, width: usize) -> State {
        let mut state = grid_of(columns, width);
        state.contents.mode = SelectionMode::Cell;
        state
    }

    #[test]
    fn l_and_right_move_the_grid_cursor_one_column() {
        for key in [Key::Char('l'), Key::Right] {
            let mut state = cell_grid_of(4, 80);

            press(&mut state, key);

            assert_eq!(state.contents.column.index(), 1, "{key:?}");
        }
    }

    #[test]
    fn h_and_left_move_the_grid_cursor_back_one_column() {
        for key in [Key::Char('h'), Key::Left] {
            let mut state = cell_grid_of(4, 80);
            press(&mut state, Key::Char('l'));

            press(&mut state, key);

            assert_eq!(state.contents.column.index(), 0, "{key:?}");
        }
    }

    #[test]
    fn the_grid_cursor_cannot_run_off_either_edge_of_a_page() {
        let mut state = cell_grid_of(2, 80);

        for _ in 0..5 {
            press(&mut state, Key::Char('l'));
        }
        assert_eq!(state.contents.column.index(), 1, "stops on the last column");

        for _ in 0..5 {
            press(&mut state, Key::Char('h'));
        }
        assert_eq!(state.contents.column.index(), 0, "stops on the first");
    }

    #[test]
    fn row_mode_scrolls_the_grid_a_column_per_press() {
        // Four columns of width 6 with two blanks between: two fit in a grid
        // 18 wide, as `the_column_viewport_follows_the_cell_cursor` counts it.
        let mut state = grid_of(4, 18);
        assert_eq!(state.contents.mode, SelectionMode::Row);

        for key in [Key::Char('l'), Key::Right] {
            let first = state.contents.column_scroll.first();

            press(&mut state, key);

            assert_eq!(
                state.contents.column_scroll.first(),
                first + 1,
                "{key:?} must reveal a column rather than move a cursor \
                 nothing paints"
            );
        }

        for key in [Key::Char('h'), Key::Left] {
            let first = state.contents.column_scroll.first();

            press(&mut state, key);

            assert_eq!(state.contents.column_scroll.first(), first - 1, "{key:?}");
        }
    }

    #[test]
    fn row_mode_stops_the_grid_at_either_end_of_the_page() {
        let mut state = grid_of(4, 18);

        for _ in 0..5 {
            press(&mut state, Key::Char('l'));
        }
        assert_eq!(
            state.contents.column_scroll.first(),
            2,
            "the window ends on the last column, with no dead space past it"
        );

        for _ in 0..5 {
            press(&mut state, Key::Char('h'));
        }
        assert_eq!(state.contents.column_scroll.first(), 0);
    }

    #[test]
    fn row_mode_moves_nothing_over_a_page_that_fits() {
        let mut state = grid_of(4, 80);

        press(&mut state, Key::Char('l'));

        assert_eq!(state.contents.column_scroll.first(), 0);
        assert_eq!(
            state.contents.column.index(),
            0,
            "with nothing to reveal there is no column to put the cursor on"
        );
    }

    #[test]
    fn the_column_the_grid_scrolled_to_is_the_one_cell_mode_opens_on() {
        let mut state = grid_of(4, 18);
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::Char('m'));

        assert_eq!(state.contents.mode, SelectionMode::Column);
        assert_eq!(
            state.contents.column.index(),
            2,
            "the invisible cursor rode along with the window, so cycling to a \
             mode that paints it lands on a drawn column"
        );
    }

    #[test]
    fn a_grid_with_no_page_swallows_horizontal_motion() {
        let mut state = State::default();
        state.focus_on(Panel::Contents);

        press(&mut state, Key::Char('l'));

        assert_eq!(state.contents.column.index(), 0);
        assert_eq!(state.contents.column_scroll.first(), 0);
    }

    #[test]
    fn horizontal_motion_belongs_to_the_focused_panel_only() {
        let mut state = grid_of(4, 80);
        state.focus_on(Panel::Ddl);

        press(&mut state, Key::Char('l'));

        assert_eq!(state.contents.column.index(), 0);
    }

    #[test]
    fn the_column_viewport_follows_the_cell_cursor() {
        // Four columns of width 6 ("c0" plus the marks its heading carries,
        // which outrun a one-digit value) with two spaces between: 6+2+6 = 14
        // fits two columns in a grid 18 wide once the merged bar+gutter block
        // (bar plus a one-digit gutter) and the spacing ahead of the values
        // are taken off.
        let mut state = cell_grid_of(4, 18);

        press(&mut state, Key::Char('l'));
        assert_eq!(
            state.contents.column_scroll.first(),
            0,
            "column 1 is already drawn"
        );

        press(&mut state, Key::Char('l'));
        assert_eq!(
            state.contents.column_scroll.first(),
            1,
            "the shortest move that draws column 2"
        );
    }

    #[test]
    fn a_narrower_frame_pulls_the_column_viewport_back_to_the_cursor() {
        let mut state = cell_grid_of(4, 80);
        for _ in 0..3 {
            press(&mut state, Key::Char('l'));
        }
        assert_eq!(
            state.contents.column_scroll.first(),
            0,
            "all four fit at 80"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(18)),
        );

        assert_eq!(
            state.contents.column_scroll.first(),
            2,
            "the selected column must stay drawn after a resize"
        );
    }

    #[test]
    fn a_panel_that_grew_pulls_the_column_viewport_back() {
        let mut state = cell_grid_of(4, 18);
        for _ in 0..3 {
            press(&mut state, Key::Char('l'));
        }
        assert_eq!(
            state.contents.column_scroll.first(),
            2,
            "two of four fit at 16"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
        );

        assert_eq!(
            state.contents.column_scroll.first(),
            0,
            "all four fit again, so none may be left hidden behind the offset"
        );
    }

    #[test]
    fn a_frame_too_small_to_draw_does_not_strand_the_column_viewport() {
        let mut state = cell_grid_of(4, 80);
        for _ in 0..3 {
            press(&mut state, Key::Char('l'));
        }

        // What `render` reports when the terminal is below its minimum size:
        // nothing was drawn, so the grid has no width at all.
        update(&mut state, Msg::Measured(Geometry::default()));
        assert_eq!(
            state.contents.column_scroll.first(),
            3,
            "with no room, the cursor's column is the only one worth drawing"
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
        );

        assert_eq!(
            state.contents.column_scroll.first(),
            0,
            "restoring the size undoes it, without the user pressing h"
        );
    }

    #[test]
    fn home_and_end_still_reach_the_ends_of_the_row_list() {
        let mut state = grid_of_rows(4, 12, 80);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::End);

        assert_eq!(
            state.contents.cursor.index(),
            11,
            "End reaches the ends of the document, which in a grid is the rows"
        );
        assert_eq!(
            state.contents.column.index(),
            1,
            "and says nothing about the column axis"
        );

        press(&mut state, Key::Home);

        assert_eq!(state.contents.cursor.index(), 0, "and Home the other end");
        assert_eq!(
            state.contents.column.index(),
            1,
            "still on the column it was on"
        );
    }

    #[test]
    fn enter_on_a_table_leaves_the_tree_where_it_is() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        let before = state.tables.clone();

        press(&mut state, Key::Enter);

        assert_eq!(
            state.tables, before,
            "reading a table must not disturb the tree the cursor is in"
        );
    }

    #[test]
    fn j_walks_into_an_unfolded_keyspaces_tables() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"]), ("typezoo", &[])]);
        press(&mut state, Key::Char('l'));

        for _ in 0..3 {
            press(&mut state, Key::Char('j'));
        }

        assert_eq!(
            state.tables.cursor.index(),
            3,
            "motion counts tree rows, not keyspaces"
        );
    }

    #[test]
    fn a_tree_transition_belongs_to_the_focused_panel_only() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(Panel::Ddl);

        press(&mut state, Key::Char('l'));

        assert_eq!(row_count(&state), 1);
    }

    #[test]
    fn l_and_h_are_swallowed_while_a_search_is_applied() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.apply_search(Searched::Tables, Search::new("shop"));
        let before = row_count(&state);

        press(&mut state, Key::Char('h'));
        press(&mut state, Key::Char('l'));

        assert_eq!(
            row_count(&state),
            before,
            "a search forces its matches open, so folding could only look broken"
        );
    }

    #[test]
    fn folding_under_a_search_does_not_disturb_the_fold_state_it_will_return_to() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        assert_eq!(row_count(&state), 3, "shop starts unfolded");

        state.apply_search(Searched::Tables, Search::new("shop"));
        press(&mut state, Key::Char('h'));
        press(&mut state, Key::Char('l'));
        state.apply_search(Searched::Tables, None);

        assert_eq!(
            row_count(&state),
            3,
            "the fold state from before the search must survive it untouched"
        );
    }

    /// Every entry point that can land on a retained keyspace row — not just
    /// `h`/`l` — must leave the unfold set alone while a search is applied.
    /// `activate` (`Enter`, `Space`, and a click, which routes through it too)
    /// has no guard of its own; it inherits `TablesPanel::toggle`'s. Remove
    /// that guard and the `Key::Enter` case here fails: the search's own
    /// projection reports `shop` as unfolded, `toggle` reads that as "fold
    /// it", and the real unfold set loses "shop" even though nothing moved
    /// on screen.
    #[test]
    fn no_tables_entry_point_writes_the_fold_state_under_a_search() {
        enum Action {
            Key(Key),
            Click,
        }

        for action in [
            Action::Key(Key::Char('h')),
            Action::Key(Key::Char('l')),
            Action::Key(Key::Enter),
            Action::Key(Key::Char(' ')),
            Action::Click,
        ] {
            let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
            press(&mut state, Key::Char('l'));
            assert_eq!(row_count(&state), 3, "shop starts unfolded");

            state.apply_search(Searched::Tables, Search::new("shop"));

            match action {
                Action::Key(key) => {
                    press(&mut state, key);
                }
                Action::Click => {
                    click_row(&mut state, Panel::Tables, 0);
                }
            }

            state.apply_search(Searched::Tables, None);

            assert_eq!(
                row_count(&state),
                3,
                "the fold state from before the search must survive it untouched"
            );
        }
    }

    /// `Enter` still reads a table under a search — the guard stops keyspace
    /// folding, not table activation.
    #[test]
    fn enter_still_reads_a_table_under_a_search() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.apply_search(Searched::Tables, Search::new("orders"));
        assert_eq!(row_count(&state), 3, "a search drops no row");

        state.tables.cursor.go_to(1, row_count(&state));
        let effects = press(&mut state, Key::Enter);

        assert!(
            !effects.is_empty(),
            "Enter on a table row under a search must still submit a read"
        );
    }

    #[test]
    fn l_and_h_still_fold_once_the_search_is_gone() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

        press(&mut state, Key::Char('l'));

        assert_eq!(row_count(&state), 3);
    }

    #[test]
    fn tab_walks_every_stop_in_order_and_wraps() {
        // A keyspace with no tables, so DDL offers its one tab and the
        // stops are exactly the ones docs/shortcuts.md §4 lists: Tables,
        // Ddl, Contents, Queries/Cql, Queries/Logs.
        let mut state = browsing(&["a"]);

        press(&mut state, Key::Tab);
        assert_eq!(state.focus(), Panel::Ddl);

        press(&mut state, Key::Tab);
        assert_eq!(state.focus(), Panel::Contents);

        press(&mut state, Key::Tab);
        assert_eq!(state.focus(), Panel::Queries);
        assert_eq!(state.queries.tab(), QueriesTab::Cql);

        press(&mut state, Key::Tab);
        assert_eq!(state.focus(), Panel::Queries, "Queries' own second stop");
        assert_eq!(state.queries.tab(), QueriesTab::Logs);

        press(&mut state, Key::Tab);
        assert_eq!(state.focus(), Panel::Tables, "the cycle wraps");
    }

    #[test]
    fn shift_tab_walks_the_same_stops_the_other_way() {
        let mut state = browsing(&["a"]);

        press(&mut state, Key::BackTab);
        assert_eq!(
            state.focus(),
            Panel::Queries,
            "stepping back from the first stop wraps to the last"
        );
        assert_eq!(state.queries.tab(), QueriesTab::Logs);

        press(&mut state, Key::BackTab);
        assert_eq!(state.focus(), Panel::Queries);
        assert_eq!(state.queries.tab(), QueriesTab::Cql);

        press(&mut state, Key::BackTab);
        assert_eq!(state.focus(), Panel::Contents);
    }

    /// Whether a freshly-focused `panel` takes text — the panels a press
    /// cannot reach from, now that it depends on the Queries tab rather than
    /// on the panel alone.
    fn does_not_take_text(panel: Panel) -> bool {
        let mut state = State::default();
        state.focus_on(panel);
        !state.takes_text()
    }

    #[test]
    fn a_number_focuses_its_panel_from_any_panel_that_does_not_take_text() {
        for from in Panel::ALL
            .into_iter()
            .filter(|panel| does_not_take_text(*panel))
        {
            for to in Panel::ALL.into_iter().filter(|panel| *panel != from) {
                let mut state = State::default();
                state.focus_on(from);

                press(&mut state, Key::Char(to.number()));

                assert_eq!(
                    state.focus(),
                    to,
                    "'{}' should reach {to:?} from {from:?}",
                    to.number()
                );
            }
        }
    }

    #[test]
    fn a_repeated_number_keeps_focus_and_walks_the_panels_tabs() {
        for panel in Panel::ALL
            .into_iter()
            .filter(|panel| does_not_take_text(*panel))
        {
            // A table has to be selected, or the DDL strip draws one tab and
            // there is nothing to walk to.
            let mut state = browsing_a_table();
            state.focus_on(Panel::Tables);

            press(&mut state, Key::Char(panel.number()));
            press(&mut state, Key::Char(panel.number()));

            assert_eq!(state.focus(), panel, "the second press does not move focus");
            assert_eq!(
                state.ddl.tab(),
                if panel == Panel::Ddl {
                    DdlTab::Columns
                } else {
                    DdlTab::Ddl
                },
                "only the panel that has tabs answers the second press"
            );
        }
    }

    #[test]
    fn pressing_two_once_per_tab_returns_to_the_first_one() {
        let mut state = browsing_a_table();

        // One press to arrive, then one per tab: the walk wraps whatever the
        // strip holds, so the count follows `DdlTab::ALL` rather than a
        // number a third tab would have made wrong.
        for _ in 0..=DdlTab::ALL.len() {
            press(&mut state, Key::Char('2'));
        }

        assert_eq!(state.focus(), Panel::Ddl);
        assert_eq!(state.ddl.tab(), DdlTab::Ddl);
    }

    #[test]
    fn brackets_walk_the_focused_panels_tabs_both_ways() {
        let mut state = browsing_a_table();
        state.focus_on(Panel::Ddl);

        press(&mut state, Key::Char(']'));
        assert_eq!(state.ddl.tab(), DdlTab::Columns);

        press(&mut state, Key::Char('['));
        assert_eq!(state.ddl.tab(), DdlTab::Ddl);
    }

    #[test]
    fn brackets_do_nothing_from_a_panel_with_no_tabs() {
        for panel in [Panel::Tables, Panel::Contents] {
            let mut state = browsing_a_table();
            state.focus_on(panel);

            press(&mut state, Key::Char(']'));

            assert_eq!(state.focus(), panel, "a bracket moves no focus");
            assert_eq!(
                state.ddl.tab(),
                DdlTab::Ddl,
                "and reaches no panel it is not in"
            );
        }
    }

    #[test]
    fn walking_on_a_keyspace_stays_on_the_only_tab_there_is() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::Char('2'));
        press(&mut state, Key::Char('2'));

        assert_eq!(state.focus(), Panel::Ddl);
        assert_eq!(
            state.ddl.tab(),
            DdlTab::Ddl,
            "a keyspace has no columns, so there is no tab to walk to"
        );
    }

    #[test]
    fn a_number_is_text_in_a_panel_that_takes_text() {
        for to in Panel::ALL {
            let mut state = State::default();
            state.focus_on(Panel::Queries);

            press(&mut state, Key::Char(to.number()));

            assert_eq!(state.focus(), Panel::Queries, "focus is the mode");
            assert_eq!(state.queries.text(), to.number().to_string());
        }
    }

    #[test]
    fn a_bracket_is_text_in_a_panel_that_takes_text() {
        let mut state = State::default();
        state.focus_on(Panel::Queries);

        press(&mut state, Key::Char(']'));

        assert_eq!(state.focus(), Panel::Queries, "focus is the mode");
        assert_eq!(state.queries.text(), "]");
        assert_eq!(state.ddl.tab(), DdlTab::Ddl, "and no tab changed under it");
    }

    /// Also the collision test: a number that an earlier arm already answers
    /// would never reach the focus arm, and the focus assertion would fail.
    #[test]
    fn a_number_moves_focus_and_does_nothing_else() {
        for panel in Panel::ALL {
            let mut state = browsing(&["a", "b", "c"]);
            press(&mut state, Key::Char('j'));
            let cursor = state.tables.cursor;

            let effects = press(&mut state, Key::Char(panel.number()));

            assert_eq!(state.focus(), panel);
            assert!(
                effects.is_empty(),
                "'{}' issued {effects:?}",
                panel.number()
            );
            assert!(!state.should_quit, "'{}' quit the app", panel.number());
            assert_eq!(
                state.tables.cursor,
                cursor,
                "'{}' disturbed the tree",
                panel.number()
            );
        }
    }

    #[test]
    fn the_letters_the_numbers_replaced_stay_unbound() {
        for freed in ['a', 'b', 'c', 'd', 'u'] {
            // Focus starts elsewhere, so an unmoved focus is evidence rather
            // than the state it began in.
            let elsewhere = Panel::Ddl;
            let mut state = State::default();
            state.focus_on(elsewhere);

            press(&mut state, Key::Char(freed));

            assert_eq!(
                state.focus(),
                elsewhere,
                "'{freed}' must stay free for a binding of its own"
            );
        }
    }

    /// `shop` unfolded, with `orders` at row 1 — a table, not a keyspace.
    fn tables_state() -> State {
        let mut state = browsing_tables(&[("shop", &["orders", "users", "products"])]);
        press(&mut state, Key::Char('l'));
        state
    }

    /// Contents focused and showing a loaded page.
    fn contents_state() -> State {
        grid_of_rows(2, 3, 80)
    }

    type Route = (&'static str, Box<dyn Fn(&mut State)>);

    #[test]
    fn switching_panels_clears_the_selection_of_the_panel_left_behind() {
        // Every route focus can change by. A selection outliving one of them
        // would let `y` act on a panel the user is no longer looking at
        // (multi-group selection design §4).
        let routes: Vec<Route> = vec![
            (
                "tab",
                Box::new(|state: &mut State| drop(press(state, Key::Tab))),
            ),
            (
                "shift-tab",
                Box::new(|state: &mut State| drop(press(state, Key::BackTab))),
            ),
            (
                "1",
                Box::new(|state: &mut State| drop(press(state, Key::Char('1')))),
            ),
            (
                "2",
                Box::new(|state: &mut State| drop(press(state, Key::Char('2')))),
            ),
            (
                "3",
                Box::new(|state: &mut State| drop(press(state, Key::Char('3')))),
            ),
            (
                "4",
                Box::new(|state: &mut State| drop(press(state, Key::Char('4')))),
            ),
        ];

        for (name, route) in routes {
            let mut state = tables_state();
            state.focus_on(Panel::Tables);
            state.tables.selection.start(0);
            state.tables.selection.stop(1);

            route(&mut state);

            if state.focus() == Panel::Tables {
                continue;
            }

            assert!(
                !state.tables.selection.is_active(),
                "{name} left Tables holding a selection"
            );
        }
    }

    #[test]
    fn switching_away_from_the_trail_clears_its_selection_by_every_route() {
        // Mirrors `switching_panels_clears_the_selection_of_the_panel_left_behind`,
        // which only drives Tables through this matrix — the trail's
        // selection lives on `state.log`, not on a panel struct `focus_on`'s
        // match already walks, so it needs its own coverage (fix round 1,
        // finding 1). `shift-tab` from the Logs tab lands on Cql without
        // ever leaving the Queries panel, which is `show_queries_tab`'s path
        // rather than `focus_on`'s.
        let routes: Vec<Route> = vec![
            (
                "tab",
                Box::new(|state: &mut State| drop(press(state, Key::Tab))),
            ),
            (
                "shift-tab",
                Box::new(|state: &mut State| drop(press(state, Key::BackTab))),
            ),
            (
                "1",
                Box::new(|state: &mut State| drop(press(state, Key::Char('1')))),
            ),
            (
                "2",
                Box::new(|state: &mut State| drop(press(state, Key::Char('2')))),
            ),
            (
                "3",
                Box::new(|state: &mut State| drop(press(state, Key::Char('3')))),
            ),
        ];

        for (name, route) in routes {
            let mut state = trail_of(&["SELECT one", "SELECT two"]);
            press(&mut state, Key::Char('v'));

            route(&mut state);

            if state.focus() == Panel::Queries && state.queries.tab() == QueriesTab::Logs {
                continue;
            }

            assert!(
                !state.log.selection.is_active(),
                "{name} left the trail holding a selection"
            );
        }
    }

    #[test]
    fn the_trails_search_does_not_survive_a_switch_to_the_cql_tab() {
        // Finding 2 (fix round 1): the trail's search must not outlive the
        // Logs tab being drawn, or a stray search left applied would keep
        // the hint bar advertising `n`/`esc clear` while the Cql buffer
        // takes text, and would cost an `Esc` press to something invisible
        // instead of leaving the buffer.
        let mut state = trail_of(&["SELECT one", "DELETE two"]);
        search_for(&mut state, "DELETE");
        press(&mut state, Key::Enter);
        assert!(
            state.search(Searched::Queries).is_some(),
            "the search committed"
        );

        press(&mut state, Key::BackTab);

        assert_eq!(
            state.queries.tab(),
            QueriesTab::Cql,
            "landed on the buffer without leaving the panel"
        );
        assert_eq!(
            state.search(Searched::Queries),
            None,
            "a stray search survived onto the Cql tab"
        );
        let bar = crate::view::hints::bar(&state);
        assert!(
            !bar.panel.iter().any(|hint| hint.action == "next match"),
            "the hint bar still offered n while the buffer takes text"
        );
    }

    #[test]
    fn re_focusing_the_focused_panel_keeps_its_selection() {
        // Arriving where you already are is not navigation and must not cost
        // you what you were reading (design §4).
        let mut state = tables_state();
        state.focus_on(Panel::Tables);
        state.tables.selection.start(0);
        state.tables.selection.stop(1);

        let _ = press(&mut state, Key::Char('1'));

        assert_eq!(
            state.tables.selection.ranges(state.tables.cursor.index()),
            Ranges::of(Range::new(0, 1)),
            "`1` on Tables is not a panel switch"
        );
    }

    #[test]
    fn leaving_contents_clears_both_of_its_axes() {
        let mut state = contents_state();
        state.focus_on(Panel::Contents);
        state.contents.rows.start(0);
        state.contents.rows.stop(1);
        state.contents.columns.start(0);
        state.contents.columns.stop(1);

        state.focus_on(Panel::Tables);

        assert!(!state.contents.rows.is_active(), "the row axis survived");
        assert!(
            !state.contents.columns.is_active(),
            "the column axis survived"
        );
    }

    #[test]
    fn a_click_clears_the_selection_of_the_panel_it_leaves() {
        let mut state = contents_state();
        state.focus_on(Panel::Tables);
        state.tables.selection.start(0);
        state.tables.selection.stop(1);

        click_row(&mut state, Panel::Contents, 0);

        assert!(!state.tables.selection.is_active());
    }

    #[test]
    fn enter_on_a_table_leaves_no_selection_behind_in_tables() {
        // `enter` focuses Contents on the table under the cursor; the tree it
        // leaves must not stay lit behind it.
        let mut state = tables_state();
        state.focus_on(Panel::Tables);
        state.tables.cursor.go_to(1, 4);
        state.tables.selection.start(1);
        state.tables.selection.stop(1);

        // Row 1 is `orders`, a table: `enter` on a keyspace opens it and
        // moves no focus.
        let _ = press(&mut state, Key::Enter);

        assert_eq!(state.focus(), Panel::Contents);
        assert!(!state.tables.selection.is_active());
    }

    #[test]
    fn esc_out_of_queries_clears_what_queries_left_behind() {
        // Queries holds no selection of its own; the panel it hands focus to
        // must not inherit one either.
        let mut state = tables_state();
        state.focus_on(Panel::Queries);

        let _ = press(&mut state, Key::Escape);

        assert_eq!(state.focus(), Panel::Tables);
    }

    fn read_shop_orders() -> State {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        state
    }

    fn page_of(rows: usize) -> Page {
        Page {
            columns: vec!["id".to_owned()],
            rows: vec![
                crate::model::Row {
                    values: vec![crate::model::CqlValue::Null],
                };
                rows
            ],
            next: None,
        }
    }

    #[test]
    fn a_stated_column_width_is_the_grids_cap() {
        let mut state = read_shop_orders();
        state.config.grid.max_column_width =
            crate::config::MaxColumnWidth::try_new(40).expect("in range");
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the read is waiting");

        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["note".to_owned()],
                    rows: vec![crate::model::Row {
                        values: vec![crate::model::CqlValue::Text("x".repeat(100))],
                    }],
                    next: None,
                }),
                took: Duration::ZERO,
            },
        );

        assert_eq!(state.contents.loaded().expect("a page").widths(), [40]);
    }

    fn statement_subject_from(cql: &str) -> LogSubject {
        statement_subject(&Statement::typed(cql))
    }

    fn executed_query(effects: &[Effect]) -> QueryId {
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Execute { query, .. } => Some(*query),
                _ => None,
            })
            .expect("an Execute effect carrying the query")
    }

    /// Cancels `query` via `Msg::Quit`, which completes every in-flight trace
    /// without pushing an entry of its own — unlike superseding it with a
    /// second read, which would leave the newer entry as the log's last one.
    fn cancel(state: &mut State, query: QueryId) {
        assert!(
            state.in_flight.contains_key(&query),
            "the query being cancelled is the one still in flight"
        );
        let _ = update(state, Msg::Quit);
    }

    // The trail records what ran. The table-open statement carries the LIMIT
    // ROW_LIMIT appended, which is the surprise the trail exists to explain
    // (query log design §6).
    #[test]
    fn opening_a_table_logs_the_statement_it_issued_limit_and_all() {
        let mut state = read_shop_orders();

        let _effects = press(&mut state, Key::Enter);

        let entry = state.log.entries().last().expect("an entry per effect");
        let text = entry.subject().text();
        assert!(text.starts_with("SELECT"), "{text}");
        assert!(text.contains(&format!("LIMIT {ROW_LIMIT}")), "{text}");
        assert!(matches!(
            entry.outcome(),
            Outcome::Pending {
                trace: Trace::Query(_)
            }
        ));
    }

    #[test]
    fn a_statement_is_logged_as_one_line_however_it_was_laid_out() {
        let mut state = State::default();

        logged(
            &mut state,
            statement_subject_from("SELECT *\n  FROM ks.t\n  WHERE id = 1"),
            Outcome::Cancelled,
        );

        assert_eq!(
            state.log.entries().next().unwrap().subject().text(),
            "SELECT * FROM ks.t WHERE id = 1"
        );
    }

    #[test]
    fn a_returned_page_completes_the_entry_with_its_row_count() {
        let mut state = read_shop_orders();
        let effects = press(&mut state, Key::Enter);
        let query = executed_query(&effects);

        let _effects = update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(3)),
                took: Duration::from_millis(41),
            },
        );

        let Outcome::Succeeded { detail, took } = state.log.entries().last().unwrap().outcome()
        else {
            panic!("the query succeeded");
        };
        assert!(detail.contains('3'), "{detail}");
        assert_eq!(*took, Duration::from_millis(41));
    }

    #[test]
    fn a_failed_query_leaves_its_reason_in_the_trail() {
        let mut state = read_shop_orders();
        let effects = press(&mut state, Key::Enter);
        let query = executed_query(&effects);

        let _effects = update(
            &mut state,
            Msg::Failed {
                query,
                error: DbError::Invalid("unconfigured column id".to_owned()),
                took: Duration::ZERO,
            },
        );

        let Outcome::Failed { detail } = state.log.entries().last().unwrap().outcome() else {
            panic!("the query failed");
        };
        assert!(detail.contains("unconfigured column id"), "{detail}");
    }

    #[test]
    fn a_cancelled_query_is_closed_out_rather_than_left_hanging() {
        let mut state = read_shop_orders();
        let effects = press(&mut state, Key::Enter);
        let query = executed_query(&effects);

        cancel(&mut state, query);

        assert!(matches!(
            state.log.entries().last().unwrap().outcome(),
            Outcome::Cancelled
        ));
    }

    #[test]
    fn enter_on_a_table_reads_it() {
        let mut state = read_shop_orders();
        let conn = current_conn(&state);

        let effects = press(&mut state, Key::Enter);

        let query = state.contents.outstanding().expect("the panel is waiting");
        assert_eq!(
            effects,
            vec![
                Effect::Execute {
                    conn,
                    query,
                    stmt: Box::new(Statement::select_all("shop", "orders", ROW_LIMIT)),
                },
                Effect::RecordHistory(Box::new(Statement::select_all("shop", "orders", ROW_LIMIT))),
            ]
        );
        assert!(state.in_flight.contains_key(&query));
    }

    #[test]
    fn space_reads_no_table() {
        let mut state = read_shop_orders();

        press(&mut state, Key::Char(' '));

        assert!(
            state.contents.outstanding().is_none(),
            "space is free, not a second name for ⏎"
        );
    }

    #[test]
    fn reading_a_second_table_cancels_the_first() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let first = state.contents.outstanding().expect("the panel is waiting");
        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Enter);

        assert_eq!(
            effects.first(),
            Some(&Effect::Cancel(first)),
            "the superseded query is cancelled before the new one is issued"
        );
        assert!(
            !state.in_flight.contains_key(&first),
            "a superseded query must not still be awaited"
        );
        let second = state.contents.outstanding().expect("waiting on the second");
        assert_ne!(second, first);
    }

    #[test]
    fn reading_a_table_lands_focus_in_contents() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Enter);

        assert_eq!(state.focus(), Panel::Contents);
    }

    #[test]
    fn a_click_on_a_table_lands_focus_in_contents_too() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('l'));

        click_row(&mut state, Panel::Tables, 1);

        assert_eq!(state.focus(), Panel::Contents);
    }

    #[test]
    fn focus_moves_before_the_page_arrives() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Enter);

        assert!(
            matches!(state.contents.state(), ContentsState::Loading { .. }),
            "focus is a consequence of the keypress, not of the network"
        );
        assert_eq!(state.focus(), Panel::Contents);
    }

    #[test]
    fn a_refused_read_still_lands_focus_in_contents() {
        // No connection, so `submit` refuses before a query is made. `read`
        // is called directly because the tree can only select a table when
        // its schema is loaded, which itself requires a connection — the two
        // cannot be pulled apart through key presses.
        let mut state = tabled(&[("shop", &["orders"])]);
        state.connection = ConnectionState::Disconnected;
        state.focus_on(Panel::Tables);

        read(&mut state, TableRef::new("shop", "orders"));

        assert_eq!(state.focus(), Panel::Contents);
    }

    #[test]
    fn unfolding_a_keyspace_leaves_focus_in_tables() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus_on(Panel::Tables);

        press(&mut state, Key::Enter);

        assert_eq!(
            state.focus(),
            Panel::Tables,
            "a fold is not a read and moves nothing"
        );
    }

    #[test]
    fn reading_a_table_leaves_the_tree_selection_where_it_was() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));
        let selected = state.tables.cursor.index();

        press(&mut state, Key::Enter);

        assert_eq!(state.tables.cursor.index(), selected);
    }

    #[test]
    fn a_page_reaches_the_panel_that_asked_for_it() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");

        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(3)),
                took: Duration::ZERO,
            },
        );

        assert_eq!(state.contents.row_count(), 3);
    }

    // The domain reads no clock; a duration is a value the adapter measured
    // around the real call and handed in (query log design §4).
    #[test]
    fn a_result_carries_how_long_it_took() {
        let msg = Msg::PageReady {
            query: QueryId::next(),
            page: Box::new(page_of(0)),
            took: Duration::from_millis(41),
        };

        let Msg::PageReady { took, .. } = msg else {
            panic!("constructed a PageReady");
        };

        assert_eq!(took, Duration::from_millis(41));
    }

    #[test]
    fn y_yanks_a_grid_row() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["id".to_owned(), "name".to_owned()],
                    rows: vec![Row {
                        values: vec![CqlValue::Int(7), CqlValue::Text("ada".to_owned())],
                    }],
                    next: None,
                }),
                took: Duration::ZERO,
            },
        );

        state.contents.mode = SelectionMode::Row;
        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(effects, vec![Effect::Yank("7,ada".to_owned())]);
    }

    #[test]
    fn a_yanked_row_carries_every_column_of_the_page() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: vec![
                    "c0".to_owned(),
                    "c1".to_owned(),
                    "c2".to_owned(),
                    "c3".to_owned(),
                ],
                rows: vec![Row {
                    values: vec![
                        CqlValue::Text("v0".to_owned()),
                        CqlValue::Text("v1".to_owned()),
                        CqlValue::Text("v2".to_owned()),
                        CqlValue::Text("v3".to_owned()),
                    ],
                }],
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );
        state.focus_on(Panel::Contents);
        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(7)),
        );

        let (widths, available) = grid_columns(&state).expect("a page is loaded");
        assert!(
            grid::fitting(widths, 0, available) < widths.len(),
            "the viewport must hide at least one column for this test to prove anything"
        );

        state.contents.mode = SelectionMode::Row;
        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("v0,v1,v2,v3".to_owned())]
        );
    }

    /// A page whose cell at row `r`, column `c` reads `r{r}c{c}`, so that a
    /// yank names which cell, row or column it came from rather than only how
    /// many commas it holds.
    fn grid_of_named_cells(columns: usize, rows: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: (0..columns).map(|c| format!("c{c}")).collect(),
                rows: (0..rows)
                    .map(|r| Row {
                        values: (0..columns)
                            .map(|c| CqlValue::Text(format!("r{r}c{c}")))
                            .collect(),
                    })
                    .collect(),
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );
        state.focus_on(Panel::Contents);
        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
        );
        state
    }

    #[test]
    fn y_yanks_the_cell_in_cell_mode() {
        let mut state = grid_of_named_cells(3, 2);
        state.contents.mode = SelectionMode::Cell;
        state.contents.cursor.go_to(1, 2);
        state.contents.column.go_to(2, 3);

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            effects,
            vec![Effect::Yank("r1c2".to_owned())],
            "cell mode yanks the value the two cursors name, and no delimiter"
        );
    }

    /// The panel opens in row mode, so this is what `y` does by default
    /// (design §1).
    #[test]
    fn y_yanks_the_row_in_row_mode() {
        let mut state = grid_of_named_cells(3, 2);
        state.contents.cursor.go_to(1, 2);
        state.contents.column.go_to(2, 3);

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            effects,
            vec![Effect::Yank("r1c0,r1c1,r1c2".to_owned())],
            "row mode yanks every column of the row the row cursor names"
        );
    }

    #[test]
    fn y_yanks_the_column_in_column_mode() {
        let mut state = grid_of_named_cells(3, 2);
        state.contents.mode = SelectionMode::Column;
        state.contents.cursor.go_to(1, 2);
        state.contents.column.go_to(2, 3);

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            effects,
            vec![Effect::Yank("r0c2,r1c2".to_owned())],
            "column mode yanks every row of the column the column cursor names"
        );
    }

    /// `y` is one gesture: it copies, and then it puts the panel back where
    /// the user started it from (visual mode design §5.1).
    #[test]
    fn y_drops_the_selection_and_sends_the_cursor_back_to_its_anchor() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.cursor.go_to(1, 4);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('y'));

        assert_eq!(state.contents.rows, Selection::default());
        assert_eq!(
            state.contents.cursor.index(),
            1,
            "the cursor comes home to where `v` was pressed"
        );
    }

    /// The anchor, not the top of the range: a selection extended upwards
    /// started at its *last* row, and that is where the cursor came from.
    #[test]
    fn a_selection_grown_upwards_sends_the_cursor_back_down() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.cursor.go_to(3, 4);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('k'));
        press(&mut state, Key::Char('k'));

        press(&mut state, Key::Char('y'));

        assert_eq!(state.contents.cursor.index(), 3);
    }

    /// Frozen by a second `v`, so the group is no longer extending and the
    /// cursor has already left the anchor by the time `y` asks for it.
    #[test]
    fn a_frozen_range_still_knows_where_to_send_the_cursor() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.cursor.go_to(1, 4);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('y'));

        assert_eq!(state.contents.rows, Selection::default());
        assert_eq!(state.contents.cursor.index(), 1);
    }

    #[test]
    fn cell_mode_sends_both_cursors_home() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Cell;
        state.contents.cursor.go_to(1, 4);
        state.contents.column.go_to(1, 3);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::Char('y'));

        assert_eq!(state.contents.rows, Selection::default());
        assert_eq!(state.contents.columns, Selection::default());
        assert_eq!(
            (state.contents.cursor.index(), state.contents.column.index()),
            (1, 1),
            "a rectangle has an anchor on each axis and both are homes"
        );
    }

    #[test]
    fn the_tree_collapses_its_selection_on_y_too() {
        let mut state = browsing(&["a", "b", "c"]);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('y'));

        assert_eq!(state.tables.selection, Selection::default());
        assert_eq!(state.tables.cursor.index(), 0);
    }

    /// Nothing yanked is nothing said, and a `y` that said nothing must not
    /// move the cursor or throw the selection away either (visual mode design §5.1).
    #[test]
    fn a_y_that_yanks_nothing_leaves_the_selection_alone() {
        let mut state = measured(&["a"]);
        state.focus_on(Panel::Queries);
        state.contents.cursor.go_to(0, 1);
        state.tables.selection.start(0);

        let effects = press(&mut state, Key::Char('y'));

        assert!(effects.is_empty());
        let mut extending_from_zero = Selection::default();
        extending_from_zero.start(0);
        assert_eq!(
            state.tables.selection, extending_from_zero,
            "a panel that yanks nothing has nothing to reset"
        );
    }

    /// The selection stops being drawn on `y`, so without this the copy would
    /// leave no mark at all and the user would have nothing but the absence of
    /// a range to tell them it happened (visual mode design §6.1).
    #[test]
    fn y_lights_up_what_it_yanked() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Cell;
        state.contents.cursor.go_to(1, 4);
        state.contents.column.go_to(0, 3);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::Char('y'));

        let flash = state.flash.expect("the yank lit what it copied");
        assert_eq!(flash.panel(), Panel::Contents);
        assert_eq!(flash.rows(), &Ranges::of(Range::new(1, 2)));
        assert_eq!(flash.columns(), &Ranges::of(Range::new(0, 1)));
    }

    /// Empty on an axis is every row or every column of it — the convention
    /// the view model's ranges already carry, and what row mode yanks.
    #[test]
    fn row_mode_lights_the_whole_row_and_column_mode_the_whole_column() {
        let mut rows = grid_of_named_cells(3, 4);
        press(&mut rows, Key::Char('y'));

        let flash = rows.flash.expect("lit");
        assert_eq!(flash.rows(), &Ranges::of(Range::new(0, 0)));
        assert_eq!(
            flash.columns(),
            &Ranges::default(),
            "row mode yanks every column"
        );

        let mut columns = grid_of_named_cells(3, 4);
        columns.contents.mode = SelectionMode::Column;
        press(&mut columns, Key::Char('y'));

        let flash = columns.flash.expect("lit");
        assert_eq!(
            flash.rows(),
            &Ranges::default(),
            "column mode yanks every row"
        );
        assert_eq!(flash.columns(), &Ranges::of(Range::new(0, 0)));
    }

    /// Nothing was selected, so what the flash marks is the one row `y` took —
    /// a bare `y` earns the same feedback as a ranged one.
    #[test]
    fn a_bare_y_lights_the_row_it_took() {
        let mut state = browsing(&["a", "b"]);

        press(&mut state, Key::Char('y'));

        let flash = state.flash.expect("lit");
        assert_eq!(flash.panel(), Panel::Tables);
        assert_eq!(flash.rows(), &Ranges::of(Range::new(0, 0)));
    }

    #[test]
    fn a_y_that_yanks_nothing_lights_nothing() {
        let mut state = measured(&["a"]);
        state.focus_on(Panel::Queries);

        press(&mut state, Key::Char('y'));

        assert_eq!(state.flash, None);
    }

    #[test]
    fn the_flash_fades_and_only_the_tick_that_ends_it_costs_a_repaint() {
        let mut state = browsing(&["a", "b"]);
        press(&mut state, Key::Char('y'));

        for tick in 1..FLASH_TICKS {
            state.dirty = false;
            update(&mut state, Msg::Tick(Stamp::default()));
            assert!(
                state.flash.is_some() && !state.dirty,
                "tick {tick} neither ends the flash nor changes the frame"
            );
        }

        state.dirty = false;
        update(&mut state, Msg::Tick(Stamp::default()));

        assert_eq!(state.flash, None);
        assert!(
            state.dirty,
            "the frame that loses the flash has to be drawn"
        );
    }

    /// The flash is a moment, and the next key is the end of it — otherwise a
    /// fresh `v` would contend with a fading one for the same band.
    #[test]
    fn the_next_key_ends_the_flash() {
        let mut state = browsing(&["a", "b"]);
        press(&mut state, Key::Char('y'));

        press(&mut state, Key::Char('j'));

        assert_eq!(state.flash, None);
    }

    /// An empty OSC 52 payload asks the terminal to clear its clipboard, so a
    /// `y` with nothing to say must say nothing (design §4).
    #[test]
    fn y_on_a_null_cell_raises_no_effect() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["id".to_owned()],
                    rows: vec![Row {
                        values: vec![CqlValue::Null],
                    }],
                    next: None,
                }),
                took: Duration::ZERO,
            },
        );

        let effects = press(&mut state, Key::Char('y'));

        assert!(
            effects.is_empty(),
            "a null has no text, and an empty yank would wipe the clipboard"
        );
    }

    #[test]
    fn y_in_column_mode_on_a_page_with_no_rows_yanks_nothing() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["id".to_owned()],
                    rows: Vec::new(),
                    next: None,
                }),
                took: Duration::ZERO,
            },
        );
        state.contents.mode = SelectionMode::Column;

        let effects = press(&mut state, Key::Char('y'));

        assert!(effects.is_empty(), "an empty column is nothing to yank");
    }

    #[test]
    fn y_in_column_mode_on_a_column_of_one_null_yanks_nothing() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["id".to_owned()],
                    rows: vec![Row {
                        values: vec![CqlValue::Null],
                    }],
                    next: None,
                }),
                took: Duration::ZERO,
            },
        );
        state.contents.mode = SelectionMode::Column;

        let effects = press(&mut state, Key::Char('y'));

        assert!(
            effects.is_empty(),
            "a lone null field is an empty line, which would wipe the clipboard"
        );
    }

    /// An empty text is a value Cassandra holds and a null is not, but neither
    /// has anything to put on the clipboard (design §4).
    #[test]
    fn y_on_a_cell_holding_the_empty_string_raises_no_effect() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["name".to_owned()],
                    rows: vec![Row {
                        values: vec![CqlValue::Text(String::new())],
                    }],
                    next: None,
                }),
                took: Duration::ZERO,
            },
        );

        let effects = press(&mut state, Key::Char('y'));

        assert!(
            effects.is_empty(),
            "an empty payload would ask the terminal to clear its clipboard"
        );
    }

    /// What is drawn is a window; what is yanked is the column (design §6).
    #[test]
    fn a_column_yank_carries_rows_the_viewport_hides() {
        let mut state = grid_of_rows(1, 20, 80);
        assert!(
            state.geometry.height(Panel::Contents) < state.contents.row_count(),
            "the viewport must hide at least one row for this test to prove anything"
        );

        state.contents.mode = SelectionMode::Column;

        let effects = press(&mut state, Key::Char('y'));

        let Some(Effect::Yank(line)) = effects.first() else {
            panic!("a loaded column yanks, got {effects:?}");
        };
        assert_eq!(
            line.matches(',').count(),
            19,
            "twenty rows make nineteen separators, however few are on screen"
        );
    }

    #[test]
    fn y_on_an_empty_panel_yanks_nothing() {
        let mut state = browsing_a_table();
        state.focus_on(Panel::Contents);

        assert!(press(&mut state, Key::Char('y')).is_empty());
    }

    #[test]
    fn y_yanks_nothing_when_the_search_kept_no_row() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.apply_search(Searched::Tables, Search::new("zzz"));

        assert!(
            press(&mut state, Key::Char('y')).is_empty(),
            "the tree is empty, so there is no row under the cursor to yank"
        );
    }

    #[test]
    fn y_yanks_a_ddl_row() {
        let mut state = browsing_a_table();
        state.focus_on(Panel::Ddl);

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("Keyspace,shop".to_owned())]
        );
    }

    #[test]
    fn y_yanks_a_column_row() {
        let mut state = browsing_a_table();
        press(&mut state, Key::Char('2'));
        press(&mut state, Key::Char('2'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("id,uuid,partition".to_owned())]
        );

        press(&mut state, Key::Char('j'));

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("created_at,timestamp,clustering".to_owned())]
        );
    }

    #[test]
    fn y_yanks_a_tree_node() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("shop".to_owned())]
        );

        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            press(&mut state, Key::Char('y')),
            vec![Effect::Yank("shop.orders".to_owned())]
        );
    }

    /// A grid in column mode with the cursor on `column`, over three rows
    /// whose first column is out of lexicographic order — so a sort has
    /// something to do and the order it produces is unambiguous.
    fn columnar_grid(column: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: vec!["name".to_owned(), "city".to_owned()],
                rows: ["Bob", "Ada", "Cleo"]
                    .into_iter()
                    .zip(["Berlin", "Aarhus", "Berlin"])
                    .map(|(name, city)| Row {
                        values: vec![
                            CqlValue::Text(name.to_owned()),
                            CqlValue::Text(city.to_owned()),
                        ],
                    })
                    .collect(),
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );
        state.focus_on(Panel::Contents);
        state.contents.mode = SelectionMode::Column;
        state.contents.column.go_to(column, 2);
        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
        );
        state
    }

    fn first_names(state: &State) -> Vec<String> {
        state
            .contents
            .loaded()
            .expect("a page is loaded")
            .rows()
            .map(|row| crate::cell::text(&row.values[0]))
            .collect()
    }

    fn filter_prompt(state: &State) -> &Filter {
        match state.overlay.as_ref() {
            Some(Overlay::Filter(filter)) => filter,
            _ => panic!("the filter prompt is up"),
        }
    }

    #[test]
    fn s_sorts_the_column_the_cursor_is_on() {
        let mut state = columnar_grid(0);

        press(&mut state, Key::Char('s'));

        assert_eq!(first_names(&state), ["Cleo", "Bob", "Ada"]);
    }

    #[test]
    fn s_is_a_free_letter_outside_column_mode() {
        for mode in [SelectionMode::Row, SelectionMode::Cell] {
            let mut state = columnar_grid(0);
            state.contents.mode = mode;
            let before = crate::view::view_model(&state);

            let effects = press(&mut state, Key::Char('s'));

            assert!(
                effects.is_empty() && crate::view::view_model(&state) == before,
                "{mode:?} has no column for a column key to address"
            );
        }
    }

    #[test]
    fn f_opens_a_prompt_over_the_column_the_cursor_is_on() {
        let mut state = columnar_grid(1);

        press(&mut state, Key::Char('f'));

        assert_eq!(filter_prompt(&state).column(), 1);
        assert_eq!(filter_prompt(&state).name(), "city");
        assert_eq!(filter_prompt(&state).query(), "");
    }

    #[test]
    fn a_filter_is_applied_when_the_prompt_closes_and_not_before() {
        let mut state = columnar_grid(1);
        press(&mut state, Key::Char('f'));

        type_into(&mut state, "berlin");
        assert_eq!(
            first_names(&state),
            ["Bob", "Ada", "Cleo"],
            "a hard filter must not rebuild the grid under the hands typing it"
        );

        press(&mut state, Key::Enter);

        assert!(state.overlay.is_none());
        assert_eq!(first_names(&state), ["Bob", "Cleo"]);
    }

    #[test]
    fn escape_leaves_the_column_filtered_exactly_as_it_was() {
        let mut state = columnar_grid(1);
        state.contents.set_filter(1, Search::new("aarhus"));
        press(&mut state, Key::Char('f'));

        type_into(&mut state, "x");
        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
        assert_eq!(first_names(&state), ["Ada"], "the standing filter stands");
    }

    #[test]
    fn the_prompt_opens_on_the_filter_already_standing_so_deleting_it_clears() {
        let mut state = columnar_grid(1);
        state.contents.set_filter(1, Search::new("berlin"));

        press(&mut state, Key::Char('f'));
        assert_eq!(filter_prompt(&state).query(), "berlin");

        for _ in 0..6 {
            press(&mut state, Key::Backspace);
        }
        press(&mut state, Key::Enter);

        assert_eq!(first_names(&state), ["Bob", "Ada", "Cleo"]);
        assert_eq!(state.contents.filter(1), None);
    }

    #[test]
    fn c_clears_the_filter_on_the_column_the_cursor_is_on() {
        let mut state = columnar_grid(1);
        state.contents.set_filter(0, Search::new("a"));
        state.contents.set_filter(1, Search::new("berlin"));

        press(&mut state, Key::Char('c'));

        assert_eq!(state.contents.filter(1), None);
        assert!(
            state.contents.filter(0).is_some(),
            "and only that column's — `c` names the cursor's column and no other"
        );
    }

    #[test]
    fn the_prompt_claims_every_printable_key_the_panel_would_have() {
        let mut state = columnar_grid(0);
        press(&mut state, Key::Char('f'));

        let effects = press(&mut state, Key::Char('q'));

        assert!(effects.is_empty(), "`q` types itself rather than quitting");
        assert!(!state.should_quit);
        assert_eq!(filter_prompt(&state).query(), "q");
    }

    #[test]
    fn the_ctrl_layer_is_refused_while_the_filter_prompt_is_up() {
        let mut state = columnar_grid(0);
        state.geometry = state.geometry.with_export_fits(true).with_theme_rows(40);
        press(&mut state, Key::Char('f'));

        for key in ['t', 'e', 'r', 'f'] {
            let effects = handle_key(&mut state, KeyPress::ctrl(Key::Char(key)));

            assert!(effects.is_empty(), "ctrl-{key} is refused");
            assert!(
                matches!(state.overlay, Some(Overlay::Filter(_))),
                "and leaves the prompt up rather than opening something over it"
            );
        }
    }

    #[test]
    fn a_filter_that_shrinks_the_grid_pulls_the_row_cursor_back() {
        let mut state = columnar_grid(1);
        state.contents.cursor.go_to(2, 3);

        press(&mut state, Key::Char('f'));
        type_into(&mut state, "aarhus");
        press(&mut state, Key::Enter);

        assert_eq!(
            state.contents.cursor.index(),
            0,
            "one row is left, and the cursor may not point past it"
        );
    }

    #[test]
    fn a_column_yanks_the_rows_the_grid_is_showing() {
        let mut state = columnar_grid(0);
        state.contents.set_filter(1, Search::new("berlin"));
        state.contents.cycle_sort(0);

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            yanked(&effects),
            Some("Cleo,Bob"),
            "filtered and in the order drawn, not the order returned"
        );
    }

    #[test]
    fn exporting_the_fetched_rows_writes_the_ones_the_grid_is_showing() {
        // A full page, so the form has a Fetched scope to toggle to: under
        // the limit the fetched rows *are* the whole table, and the radio has
        // nowhere to go (export design §3).
        let mut state = connected(Posture::ReadOnly);
        state.geometry = Geometry::default().with_export_fits(true);
        state.focus_on(Panel::Contents);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: vec!["kind".to_owned()],
                rows: (0..ROW_LIMIT)
                    .map(|row| Row {
                        values: vec![CqlValue::Text(
                            if row % 2 == 0 { "keep" } else { "drop" }.to_owned(),
                        )],
                    })
                    .collect(),
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );
        state.contents.set_filter(0, Search::new("keep"));

        open_form(&mut state);
        handle_key(&mut state, KeyPress::new(Key::Char(' '))); // Scope: Whole -> Fetched.
        for _ in 0..3 {
            handle_key(&mut state, KeyPress::new(Key::Tab));
        }
        let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

        let [
            Effect::Export {
                source: Source::Fetched { rows, .. },
                ..
            },
        ] = effects.as_slice()
        else {
            panic!("one export effect carrying the rows");
        };
        assert_eq!(
            rows.len(),
            ROW_LIMIT / 2,
            "a form may not write what the grid hides"
        );
    }

    #[test]
    fn clicking_the_sort_mark_sorts_the_column_it_belongs_to() {
        let mut state = columnar_grid(1);

        update(
            &mut state,
            Msg::Pointer(Pointer::Click(Click::Mark {
                panel: Panel::Contents,
                column: 0,
                mark: Mark::Sort,
            })),
        );

        assert_eq!(first_names(&state), ["Cleo", "Bob", "Ada"]);
        assert_eq!(
            state.contents.column.index(),
            0,
            "a press says which column it means, and the cursor follows it there"
        );
    }

    #[test]
    fn clicking_the_filter_mark_opens_the_prompt_over_that_column() {
        let mut state = columnar_grid(0);

        update(
            &mut state,
            Msg::Pointer(Pointer::Click(Click::Mark {
                panel: Panel::Contents,
                column: 1,
                mark: Mark::Filter,
            })),
        );

        assert_eq!(filter_prompt(&state).column(), 1);
    }

    #[test]
    fn a_mark_on_a_column_that_is_gone_is_dropped_rather_than_clamped() {
        let mut state = columnar_grid(0);

        update(
            &mut state,
            Msg::Pointer(Pointer::Click(Click::Mark {
                panel: Panel::Contents,
                column: 9,
                mark: Mark::Sort,
            })),
        );

        assert_eq!(
            state
                .contents
                .loaded()
                .and_then(crate::contents::GridPage::sort),
            None,
            "the map a press was resolved against describes the previous frame"
        );
    }

    #[test]
    fn column_mode_is_the_one_bar_that_names_the_column_keys() {
        let mut state = columnar_grid(0);
        let named = |state: &State| -> Vec<&'static str> {
            crate::view::hints::bar(state)
                .panel
                .iter()
                .map(|hint| hint.action)
                .collect()
        };

        let shown = named(&state);
        assert!(shown.contains(&"sort") && shown.contains(&"filter"));

        state.contents.mode = SelectionMode::Row;
        let shown = named(&state);
        assert!(
            !shown.contains(&"sort") && !shown.contains(&"filter"),
            "a hint has to be true in the frame it is drawn in"
        );
    }

    #[test]
    fn the_clear_filter_hint_is_greyed_until_there_is_one_to_clear() {
        let mut state = columnar_grid(0);
        let liveness = |state: &State| {
            crate::view::hints::bar(state)
                .panel
                .into_iter()
                .find(|hint| hint.action == "clear filter")
                .expect("column mode names it")
                .liveness
        };

        assert_eq!(liveness(&state), crate::view::Liveness::Inert);

        state.contents.set_filter(0, Search::new("bob"));

        assert_eq!(liveness(&state), crate::view::Liveness::Live);
    }

    #[test]
    fn the_bar_under_the_filter_prompt_names_the_two_ways_out_and_nothing_else() {
        let mut state = columnar_grid(0);
        press(&mut state, Key::Char('f'));

        let bar = crate::view::hints::bar(&state);

        assert!(bar.panel.is_empty(), "the panel underneath answers nothing");
        assert_eq!(
            bar.global
                .iter()
                .map(|hint| hint.action)
                .collect::<Vec<_>>(),
            vec!["apply", "cancel"]
        );
    }

    fn yanked(effects: &[Effect]) -> Option<&str> {
        effects.iter().find_map(|effect| match effect {
            Effect::Yank(line) => Some(line.as_str()),
            _ => None,
        })
    }

    #[test]
    fn y_yanks_every_row_of_a_range_one_per_line() {
        let mut state = grid_of_named_cells(2, 3);
        state.contents.mode = SelectionMode::Row;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(yanked(&effects), Some("r0c0,r0c1\nr1c0,r1c1"));
    }

    #[test]
    fn y_in_cell_mode_yanks_the_block_and_nothing_outside_it() {
        let mut state = grid_of_named_cells(3, 3);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('l'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(yanked(&effects), Some("r0c0,r0c1\nr1c0,r1c1"));
    }

    #[test]
    fn y_on_a_single_cell_still_yanks_it_bare() {
        let mut state = grid_of_named_cells(2, 2);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('v'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            yanked(&effects),
            Some("r0c0"),
            "one cell is not a range; adding quotes would put back the delimiter there is none of"
        );
    }

    #[test]
    fn y_in_column_mode_yanks_one_line_per_column() {
        let mut state = grid_of_named_cells(3, 2);
        state.contents.mode = SelectionMode::Column;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('l'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(yanked(&effects), Some("r0c0,r1c0\nr0c1,r1c1"));
    }

    #[test]
    fn y_yanks_every_node_of_a_tree_range() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(yanked(&effects), Some("shop\nshop.orders\nshop.users"));
    }

    #[test]
    fn y_over_two_groups_yanks_their_rows_in_drawn_order() {
        let mut state = tables_state();
        state.focus_on(Panel::Tables);
        state.tables.selection.start(2);
        state.tables.selection.stop(2);
        state.tables.selection.start(0);
        state.tables.selection.stop(0);

        let effects = press(&mut state, Key::Char('y'));

        let Some(Effect::Yank(payload)) = effects.into_iter().next() else {
            panic!("y raised no yank");
        };

        assert_eq!(
            payload, "shop\nshop.products",
            "row order, not insertion order (design §5)"
        );
    }

    #[test]
    fn y_sends_the_cursor_home_to_the_last_v_and_spends_every_group() {
        // The last press is where the user last was, and it is the only one of the
        // three candidate rows still true after a group has been dropped from the
        // middle (design §3).
        let mut state = tables_state();
        state.focus_on(Panel::Tables);
        state.tables.selection.start(0);
        state.tables.selection.stop(1);
        state.tables.selection.start(3);
        state.tables.selection.stop(3);
        state.tables.cursor.go_to(3, 4);

        let _ = press(&mut state, Key::Char('y'));

        assert_eq!(state.tables.cursor.index(), 3, "home is the last v");
        assert!(
            !state.tables.selection.is_active(),
            "a yank spends every group, not just the last"
        );
    }

    #[test]
    fn y_over_a_selection_whose_anchor_was_dropped_moves_no_cursor() {
        let mut state = tables_state();
        state.focus_on(Panel::Tables);
        // Two groups; the second one's row is where `v` was last pressed, and
        // dropping it takes the anchor with it. The first group still stands.
        state.tables.selection.toggle(0);
        state.tables.selection.toggle(1);
        state.tables.selection.toggle(3);
        state.tables.selection.toggle(3);
        state.tables.selection.toggle(3);
        state.tables.cursor.go_to(1, 4);

        assert_eq!(state.tables.selection.anchor(), None);

        let _ = press(&mut state, Key::Char('y'));

        assert_eq!(
            state.tables.cursor.index(),
            1,
            "there is no row the anchor could send it to that the user chose"
        );
        assert!(
            !state.tables.selection.is_active(),
            "a yank spends the selection even when it has no anchor"
        );
    }

    #[test]
    fn a_single_cell_in_one_group_each_way_still_yanks_bare() {
        // "One" is now "one row and one column in one group each"; the bare,
        // unescaped value must survive that rewording (design §5).
        let mut state = contents_state();
        state.focus_on(Panel::Contents);
        state.contents.mode = SelectionMode::Cell;

        let effects = press(&mut state, Key::Char('y'));

        let Some(Effect::Yank(payload)) = effects.into_iter().next() else {
            panic!("y raised no yank");
        };

        assert!(!payload.contains(','), "a bare cell takes no delimiter");
    }

    #[test]
    fn a_payload_of_nothing_but_delimiters_reaches_no_clipboard() {
        // `grid_of_rows` fills its cells with `Int(1)`; this needs nulls, so
        // the page is built here rather than borrowed from it.
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: vec!["a".to_owned(), "b".to_owned()],
                rows: vec![
                    Row {
                        values: vec![CqlValue::Null, CqlValue::Null],
                    };
                    2
                ],
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );
        state.focus_on(Panel::Contents);
        state.contents.mode = SelectionMode::Row;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        let effects = press(&mut state, Key::Char('y'));

        assert!(
            effects.is_empty(),
            "a block of nulls says nothing, and an empty payload clears the clipboard"
        );
    }

    #[test]
    fn a_page_for_a_cancelled_query_never_reaches_the_panel() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let first = state.contents.outstanding().expect("the panel is waiting");
        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Enter);

        update(
            &mut state,
            Msg::PageReady {
                query: first,
                page: Box::new(page_of(3)),
                took: Duration::ZERO,
            },
        );

        assert_eq!(state.contents.row_count(), 0);
        assert!(
            state.contents.outstanding().is_some(),
            "the second request is still outstanding"
        );
    }

    #[test]
    fn a_failed_query_shows_in_the_panel_and_not_over_it() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");

        update(
            &mut state,
            Msg::Failed {
                query,
                error: DbError::Timeout,
                took: Duration::ZERO,
            },
        );

        assert!(matches!(
            state.contents.state(),
            crate::contents::ContentsState::Failed { .. }
        ));
        assert!(
            state.overlay.is_none(),
            "one panel failing must not interrupt the whole application"
        );
    }

    #[test]
    fn j_and_k_move_the_grid_once_contents_has_focus() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(4)),
                took: Duration::ZERO,
            },
        );
        let tree_row = state.tables.cursor.index();

        state.focus_on(Panel::Contents);
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.contents.cursor.index(), 2);
        assert_eq!(
            state.tables.cursor.index(),
            tree_row,
            "the tree must not move while another panel has focus"
        );
    }

    #[test]
    fn the_grid_cursor_cannot_run_off_a_page() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(2)),
                took: Duration::ZERO,
            },
        );

        state.focus_on(Panel::Contents);
        for _ in 0..5 {
            press(&mut state, Key::Char('j'));
        }

        assert_eq!(state.contents.cursor.index(), 1);
    }

    #[test]
    fn motion_in_one_panel_never_reaches_another() {
        let mut state = read_shop_orders();
        let tree_row = state.tables.cursor.index();

        state.focus_on(Panel::Ddl);
        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), tree_row);
        assert_eq!(state.contents.cursor.index(), 0);
    }

    #[test]
    fn motion_in_the_ddl_panel_walks_the_rows_it_describes() {
        let mut state = read_shop_orders();
        state.focus_on(Panel::Ddl);
        let rows = crate::view::ddl_rows(&state);
        assert!(rows > 2, "the fixture describes a key and some options");

        for _ in 0..rows + 3 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(
            state.ddl.cursor.index(),
            rows - 1,
            "j stops on the last row"
        );

        press(&mut state, Key::Home);
        assert_eq!(state.ddl.cursor.index(), 0);

        press(&mut state, Key::End);
        assert_eq!(state.ddl.cursor.index(), rows - 1);
    }

    #[test]
    fn describing_another_node_opens_the_ddl_panel_at_its_first_row() {
        let mut state = read_shop_orders();
        state.focus_on(Panel::Ddl);
        press(&mut state, Key::End);
        assert!(state.ddl.cursor.index() > 0);

        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('k'));

        assert_eq!(state.ddl.cursor.index(), 0);
        assert_eq!(state.ddl.scroll.offset(), 0);
    }

    #[test]
    fn a_measurement_pulls_the_viewport_onto_the_selected_row() {
        let mut state = read_shop_orders();
        state.focus_on(Panel::Ddl);

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 4, 8, 1], 40, 0).with_ddl_selection(Span {
                first: 20,
                lines: 2,
            })),
        );

        assert_eq!(
            state.ddl.scroll.offset(),
            18,
            "the row measured at lines 20..22 has to be on screen in a four-line panel"
        );
    }

    fn log_entry(text: &str) -> Entry {
        Entry::new(
            Stamp::default(),
            LogSubject::Sentence(text.to_owned()),
            Outcome::Succeeded {
                detail: "ok".to_owned(),
                took: Duration::from_millis(1),
            },
        )
    }

    fn geometry_with_log(lines: usize, height: usize, selection: Span) -> Geometry {
        Geometry::new([0, 0, 0, height], 0, 0).with_log(lines, selection)
    }

    // The Logs tab follows Ddl's own rule (design §5.4): `G` moves only the
    // cursor, because the viewport counts wrapped lines the domain cannot
    // map a row onto for itself. The viewport catches up only once the
    // renderer reports back where the newly-selected entry landed.
    #[test]
    fn end_reaches_the_last_entry_and_the_next_measurement_follows_it_into_view() {
        let mut state = on_the_trail();
        for index in 0..20 {
            state.log.push(log_entry(&index.to_string()));
        }
        update(
            &mut state,
            Msg::Measured(geometry_with_log(40, 6, Span { first: 0, lines: 2 })),
        );

        press(&mut state, Key::Char('G'));

        assert_eq!(state.log.cursor.index(), 19, "the last of twenty entries");
        assert_eq!(
            state.log.scroll.offset(),
            0,
            "the viewport has not moved: no measurement has reported where entry 19 landed"
        );

        update(
            &mut state,
            Msg::Measured(geometry_with_log(
                40,
                6,
                Span {
                    first: 38,
                    lines: 2,
                },
            )),
        );

        assert_eq!(
            state.log.scroll.offset(),
            34,
            "the row measured at lines 38..40 has to be on screen in a six-line panel"
        );
    }

    #[test]
    fn enter_outside_the_tree_reads_nothing() {
        let mut state = read_shop_orders();
        state.focus_on(Panel::Contents);

        let effects = press(&mut state, Key::Enter);

        assert!(effects.is_empty());
        assert!(state.contents.outstanding().is_none());
    }

    #[test]
    fn a_reload_that_shrinks_the_tree_pulls_the_selection_inside_it() {
        let mut state = browsing_tables(&[("shop", &["a", "b", "c"])]);
        let conn = current_conn(&state);
        press(&mut state, Key::Char('l'));
        for _ in 0..3 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(state.tables.cursor.index(), 3);

        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(&[("shop", &["a"])])),
                took: Duration::ZERO,
            },
        );

        assert_eq!(
            state.tables.cursor.index(),
            1,
            "the clamp must count tree rows, not keyspaces"
        );
    }

    fn sized(state: &mut State, heights: [usize; 4], ddl_lines: usize) {
        update(state, Msg::Measured(Geometry::new(heights, ddl_lines, 0)));
    }

    #[test]
    fn page_down_moves_the_tree_by_one_panel_height() {
        let mut state = browsing(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        sized(&mut state, [3, 0, 0, 0], 0);

        press(&mut state, Key::PageDown);

        assert_eq!(state.tables.cursor.index(), 3);

        press(&mut state, Key::PageUp);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_tree_in_one_key() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        sized(&mut state, [3, 0, 0, 0], 0);

        press(&mut state, Key::End);

        assert_eq!(state.tables.cursor.index(), 3);

        press(&mut state, Key::Home);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn g_and_shift_g_reach_the_ends_of_the_tree() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        sized(&mut state, [3, 0, 0, 0], 0);

        press(&mut state, Key::Char('G'));
        assert_eq!(state.tables.cursor.index(), 3);

        press(&mut state, Key::Char('g'));
        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn g_and_shift_g_reach_the_ends_of_the_ddl_rows() {
        let mut state = read_shop_orders();
        state.focus_on(Panel::Ddl);
        let rows = crate::view::ddl_rows(&state);

        press(&mut state, Key::Char('G'));
        assert_eq!(state.ddl.cursor.index(), rows - 1);

        press(&mut state, Key::Char('g'));
        assert_eq!(state.ddl.cursor.index(), 0);
    }

    #[test]
    fn g_and_shift_g_reach_the_ends_of_the_grids_rows() {
        let mut state = grid_of_rows(4, 12, 80);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::Char('G'));
        assert_eq!(state.contents.cursor.index(), 11);
        assert_eq!(
            state.contents.column.index(),
            1,
            "and says nothing about the column axis, as End does not"
        );

        press(&mut state, Key::Char('g'));
        assert_eq!(state.contents.cursor.index(), 0);
    }

    #[test]
    fn g_and_shift_g_reach_the_ends_of_an_open_palette() {
        let mut state = measured(&["a", "b", "c"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('G'));
        assert_eq!(
            palette(&state).expect("still open").selected(),
            crate::commands::row_count(None) - 1
        );

        press(&mut state, Key::Char('g'));
        assert_eq!(
            palette(&state).expect("still open").selected(),
            1,
            "the row after the first heading, which is where the palette opens"
        );
    }

    #[test]
    fn g_and_shift_g_reach_the_ends_of_the_diagnostics() {
        let diagnostics = vec![diagnostic("one"), diagnostic("two"), diagnostic("three")];
        let (mut state, _) = init(Config::default(), diagnostics, Stamp::default());

        press(&mut state, Key::Char('G'));
        assert_eq!(diagnostics_cursor(&state), 2);

        press(&mut state, Key::Char('g'));
        assert_eq!(diagnostics_cursor(&state), 0);
    }

    #[test]
    fn g_is_text_in_queries_and_under_a_prompt() {
        let mut editor = editing("SELECT *");
        press(&mut editor, Key::Char('g'));
        press(&mut editor, Key::Char('G'));
        assert_eq!(editor.queries.text(), "SELECT *gG");

        let mut prompted = browsing(&["a", "b", "c"]);
        press(&mut prompted, Key::Char('/'));
        press(&mut prompted, Key::Char('g'));
        press(&mut prompted, Key::Char('G'));
        assert_eq!(
            searching(&prompted).map(Prompt::query),
            Some("gG"),
            "a prompt claims both, as it claims every printable key"
        );
    }

    #[test]
    fn page_down_moves_the_grid_by_one_panel_height() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(30)),
                took: Duration::ZERO,
            },
        );
        state.focus_on(Panel::Contents);
        sized(&mut state, [0, 0, 8, 0], 0);

        press(&mut state, Key::PageDown);

        assert_eq!(state.contents.cursor.index(), 8);
    }

    #[test]
    fn motion_in_the_ddl_panel_moves_its_cursor_not_its_focus() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus_on(Panel::Ddl);
        sized(&mut state, [0, 6, 0, 0], 20);
        let tree_row = state.tables.cursor.index();
        let rows = crate::view::ddl_rows(&state);

        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.ddl.cursor.index(), 2.min(rows - 1));
        assert_eq!(
            state.tables.cursor.index(),
            tree_row,
            "the tree must not move while another panel has focus"
        );

        press(&mut state, Key::End);

        assert_eq!(state.ddl.cursor.index(), rows - 1);
    }

    #[test]
    fn ddl_motion_still_moves_the_cursor_before_the_first_measurement() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus_on(Panel::Ddl);
        let rows = crate::view::ddl_rows(&state);
        assert!(
            rows > 1,
            "a keyspace's replication must have more than one row, or End \
             landing on rows - 1 == 0 would hold even if the cursor never moved"
        );

        press(&mut state, Key::PageDown);
        press(&mut state, Key::End);

        assert_eq!(
            state.ddl.cursor.index(),
            rows - 1,
            "the cursor counts rows, not the still-unmeasured panel height"
        );
    }

    #[test]
    fn the_new_motions_emit_no_effects() {
        for key in [Key::PageDown, Key::PageUp, Key::Home, Key::End] {
            let mut state = browsing(&["a", "b", "c"]);
            sized(&mut state, [3, 6, 6, 1], 20);

            let effects = press(&mut state, key);

            assert!(effects.is_empty(), "{key:?} issued {effects:?}");
        }
    }

    /// The frame this message describes is the frame that produced it.
    /// Marking dirty unconditionally would redraw, re-measure and redraw
    /// forever (design §3).
    #[test]
    fn a_measurement_that_changes_nothing_leaves_the_state_clean() {
        let mut state = browsing(&["a", "b", "c"]);
        let geometry = Geometry::new([3, 6, 6, 1], 20, 0);
        update(&mut state, Msg::Measured(geometry));
        state.dirty = false;

        let effects = update(&mut state, Msg::Measured(geometry));

        assert!(effects.is_empty());
        assert!(
            !state.dirty,
            "a measurement of the frame already drawn must not ask for another"
        );
    }

    /// A table scrolled to its last row and then left must not hand the next
    /// table its cursor position: the reader would open mid-document with
    /// nothing saying so.
    #[test]
    fn moving_the_tree_selection_returns_the_ddl_panel_to_the_top() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        state.focus_on(Panel::Ddl);
        press(&mut state, Key::End);
        assert!(state.ddl.cursor.index() > 0);

        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.ddl.cursor.index(),
            0,
            "users is a document of its own"
        );
    }

    #[test]
    fn a_new_subject_clears_the_ddl_range() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        state.focus_on(Panel::Ddl);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));
        assert!(state.ddl.selection.is_active());

        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('j'));

        assert!(
            !state.ddl.selection.is_active(),
            "the rows the range addressed belong to the node that was on screen"
        );
    }

    #[test]
    fn unfolding_a_keyspace_leaves_the_ddl_panel_where_it_is() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.focus_on(Panel::Ddl);
        press(&mut state, Key::End);
        let end = state.ddl.cursor.index();
        assert!(end > 0);
        state.focus_on(Panel::Tables);

        press(&mut state, Key::Char('l'));

        assert_eq!(
            state.ddl.cursor.index(),
            end,
            "shop is still the selected node, so the panel is still on its description"
        );

        // A second `l` steps onto shop's first table, which is a new subject.
        press(&mut state, Key::Char('l'));

        assert_eq!(state.ddl.cursor.index(), 0);
    }

    #[test]
    fn leaving_ddl_for_tables_clears_its_range_before_unfolding_ever_runs() {
        // Once superseded by `switching_panels_clears_the_selection_of_the_panel_left_behind`:
        // the range used to survive the focus change itself and die only on
        // whatever motion followed. Now `focus_on` is what clears it, so the
        // motion after has nothing left to disturb (design §4).
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.focus_on(Panel::Ddl);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));
        assert!(state.ddl.selection.is_active());

        state.focus_on(Panel::Tables);

        assert!(!state.ddl.selection.is_active());
    }

    #[test]
    fn a_measurement_pulls_a_stale_scroll_offset_back_inside_the_panel() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus_on(Panel::Ddl);
        sized(&mut state, [0, 6, 0, 0], 40);
        state.ddl.scroll.to_end(40, 6);
        assert_eq!(state.ddl.scroll.offset(), 34);
        state.dirty = false;

        // The panel shrank to a wrapping of far fewer lines.
        sized(&mut state, [0, 6, 0, 0], 8);

        assert_eq!(state.ddl.scroll.offset(), 2);
        assert!(
            state.dirty,
            "the offset moved, so the frame must be repainted"
        );
    }

    #[test]
    fn the_ddl_cursor_clamps_when_the_selected_table_shrinks() {
        use crate::model::{Column, Keyspace, PrimaryKey, Replication, TableOptions};

        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let conn = current_conn(&state);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        state.focus_on(Panel::Ddl);
        press(&mut state, Key::End);
        assert!(
            state.ddl.cursor.index() > 0,
            "the fixture table has more than one row"
        );

        // Same keyspace and table name, so `described()` sees no subject
        // change — the clamp below is the only thing that can pull the
        // cursor back in.
        let shrunk = crate::model::Table {
            name: "orders".to_owned(),
            primary_key: PrimaryKey {
                partition: vec![Column {
                    name: "id".to_owned(),
                    cql_type: "uuid".to_owned(),
                }],
                clustering: Vec::new(),
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
            indexes: Vec::new(),
            options: TableOptions::default(),
        };
        let schema = Schema {
            keyspaces: [(
                "shop".to_owned(),
                Keyspace {
                    name: "shop".to_owned(),
                    replication: Replication::SimpleStrategy { factor: 1 },
                    tables: [("orders".to_owned(), shrunk)].into_iter().collect(),
                },
            )]
            .into_iter()
            .collect(),
        };

        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema),
                took: Duration::ZERO,
            },
        );

        let rows = crate::view::ddl_rows(&state);
        assert_eq!(
            rows, 3,
            "the keyspace, the table, one partition column and nothing else"
        );
        assert_eq!(
            state.ddl.cursor.index(),
            rows - 1,
            "the clamp must pull the cursor back even though no measurement arrived"
        );
    }

    /// A state where every motion has somewhere to go in both directions:
    /// two keyspaces with the second unfolded, a page of rows under the
    /// Contents cursor, and a DDL document longer than its panel. A cursor
    /// resting on 0 would make `k`/`h` and `home` inert and fail a correct
    /// hint. The Ddl viewport is likewise nudged off zero, or `ctrl-y` would
    /// find nothing to scroll up over.
    fn populated(focus: Panel) -> State {
        let mut state = browsing_tables(&[("alpha", &["a1"]), ("shop", &["orders", "users"])]);
        state.tables.cursor.go_to(1, 2);
        press(&mut state, Key::Char('l'));

        // Tables keeps its cursor on the keyspace, where `l` steps in and `h`
        // folds. The other panels need a table selected: Ddl draws its
        // details from one, and Contents shows what reading it returned.
        if focus != Panel::Tables {
            press(&mut state, Key::Char('l'));
            press(&mut state, Key::Enter);
            let query = state
                .contents
                .outstanding()
                .expect("pressing enter on a table issues a read");
            // Three columns, not `page_of`'s one: `h`/`l` need a middle
            // column to move away from in both directions for their hint to
            // be exercised.
            update(
                &mut state,
                Msg::PageReady {
                    query,
                    page: Box::new(Page {
                        columns: vec!["id".to_owned(), "name".to_owned(), "email".to_owned()],
                        rows: vec![
                            Row {
                                values: vec![CqlValue::Null, CqlValue::Null, CqlValue::Null],
                            };
                            20
                        ],
                        next: None,
                    }),
                    took: Duration::ZERO,
                },
            );
            state.contents.cursor.go_to(5, 20);
            state.contents.column.go_to(1, 3);
        }

        sized(&mut state, [3, 3, 3, 3], 40);
        // Room for the export form, or Contents' `ctrl-e export` hint is drawn
        // inert and promises nothing to test.
        state.geometry = state.geometry.with_export_fits(true);
        let ddl_len = crate::view::ddl_rows(&state);
        state.ddl.cursor.page_down(ddl_len, 3);
        state.ddl.scroll.down(40, 3);
        state.focus_on(focus);

        state
    }

    #[test]
    fn the_editing_keys_do_nothing_outside_a_panel_that_takes_text() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            for key in [Key::Backspace, Key::Delete] {
                let mut state = populated(panel);
                let before = crate::view::view_model(&state);

                let effects = press(&mut state, key);

                assert!(
                    effects.is_empty() && crate::view::view_model(&state) == before,
                    "{key:?} belongs to the editor and {panel:?} does not take text"
                );
            }
        }
    }

    /// Live hints only. An inert one is drawn greyed and promises nothing —
    /// `ctrl-r` on an empty buffer is the one there is (query execution
    /// design §3) — so pressing it and demanding a change would be asserting
    /// the opposite of what the bar says.
    ///
    /// A binding the posture refuses is left off the bar, so the two are
    /// paired by action. The bar appends `esc leave` on every panel but
    /// Tables; `escape_leaves_every_panel_for_tables` presses it.
    #[test]
    fn every_hint_the_bar_shows_does_something() {
        for panel in Panel::ALL {
            let shown = crate::view::hints::bar(&populated(panel)).panel;

            for binding in panel.hints() {
                let Some(hint) = shown.iter().find(|hint| hint.action == binding.action) else {
                    continue;
                };
                if hint.liveness == crate::view::Liveness::Inert {
                    continue;
                }

                for &key in binding.keys {
                    let mut state = populated(panel);
                    let before = crate::view::view_model(&state);

                    let effects = press_stroke(&mut state, key);

                    assert!(
                        crate::view::view_model(&state) != before || !effects.is_empty(),
                        "{panel:?} offers '{} {}' and pressing it changes nothing",
                        crate::view::hints::label(binding.keys),
                        binding.action
                    );
                }
            }
        }
    }

    #[test]
    fn every_global_hint_does_something_in_every_panel() {
        // `?` is filtered by `hints::bar` itself once a binding's effect
        // depends on more than focus (design §5.1) — the sweep is over what
        // the bar actually shows for this state, which is what it promises.
        for panel in Panel::ALL {
            let state = populated(panel);
            let shown: Vec<&str> = crate::view::hints::bar(&state)
                .global
                .iter()
                .map(|hint| hint.action)
                .collect();

            // `q` left GLOBAL because the bar stops naming it in a panel that
            // takes text, and `?` is dropped where no palette would fit. Both
            // are conditions on the bar rather than on the binding set, so the
            // sweep asks the bar what it is showing.
            let claimed = crate::view::hints::GLOBAL
                .iter()
                .chain((!state.takes_text()).then_some(&crate::view::hints::QUIT));

            for binding in claimed {
                if !shown.contains(&binding.action) {
                    continue;
                }
                for &key in binding.keys {
                    let mut state = populated(panel);
                    let before = crate::view::view_model(&state);

                    let effects = press_stroke(&mut state, key);

                    assert!(
                        crate::view::view_model(&state) != before || !effects.is_empty(),
                        "'{} {}' does nothing with {panel:?} focused",
                        crate::view::hints::label(binding.keys),
                        binding.action
                    );
                }
            }
        }
    }

    fn point(state: &mut State, pointer: Pointer) -> Vec<Effect> {
        update(state, Msg::Pointer(pointer))
    }

    fn click_row(state: &mut State, panel: Panel, row: usize) -> Vec<Effect> {
        point(
            state,
            Pointer::Click(Click::Row {
                panel,
                row,
                column: None,
            }),
        )
    }

    fn click_cell(state: &mut State, panel: Panel, row: usize, column: usize) -> Vec<Effect> {
        point(
            state,
            Pointer::Click(Click::Row {
                panel,
                row,
                column: Some(column),
            }),
        )
    }

    /// `shop.orders` loaded with `count` single-column rows.
    fn showing_rows(count: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: vec!["id".to_owned()],
                rows: (0..count)
                    .map(|i| Row {
                        values: vec![CqlValue::Int(
                            i64::try_from(i).expect("the fixture is small"),
                        )],
                    })
                    .collect(),
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );
        state
    }

    /// `shop.orders` loaded with one row of `columns` columns.
    fn showing_columns(columns: usize) -> State {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: (0..columns).map(|i| format!("c{i}")).collect(),
                rows: vec![Row {
                    values: (0..columns)
                        .map(|i| CqlValue::Int(i64::try_from(i).expect("the fixture is small")))
                        .collect(),
                }],
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );
        state
    }

    fn failure() -> Overlay {
        Overlay::Message(Message {
            title: "Cannot connect".to_owned(),
            message: "no route to host".to_owned(),
            severity: Severity::Danger,
        })
    }

    #[test]
    fn a_click_on_a_keyspace_folds_and_unfolds_it() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

        click_row(&mut state, Panel::Tables, 0);
        assert_eq!(row_count(&state), 3, "the keyspace unfolded");

        click_row(&mut state, Panel::Tables, 0);
        assert_eq!(row_count(&state), 1, "and folded again");
    }

    #[test]
    fn a_click_on_a_table_reads_it() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        click_row(&mut state, Panel::Tables, 0);

        let effects = click_row(&mut state, Panel::Tables, 1);

        assert!(
            matches!(
                effects.as_slice(),
                [Effect::Execute { .. }, Effect::RecordHistory(_)]
            ),
            "got {effects:?}"
        );
        assert_eq!(
            state.tables.cursor.index(),
            1,
            "on the row that was clicked"
        );
    }

    #[test]
    fn a_click_on_a_grid_row_moves_the_cursor_and_asks_for_nothing() {
        let mut state = showing_rows(5);

        let effects = click_row(&mut state, Panel::Contents, 3);

        assert!(effects.is_empty(), "a selection runs no query");
        assert_eq!(state.contents.cursor.index(), 3);
    }

    #[test]
    fn a_click_on_a_cell_moves_both_cursors() {
        let mut state = showing_columns(4);

        let effects = click_cell(&mut state, Panel::Contents, 0, 2);

        assert!(effects.is_empty(), "a selection runs no query");
        assert_eq!(state.contents.cursor.index(), 0);
        assert_eq!(state.contents.column.index(), 2);
    }

    /// A press names whatever cells it can reach; which of them the grid draws
    /// a band around is the mode's business, and `m` is the only thing that
    /// changes it.
    #[test]
    fn a_click_on_a_cell_leaves_the_selection_mode_alone() {
        for mode in [
            SelectionMode::Cell,
            SelectionMode::Row,
            SelectionMode::Column,
        ] {
            let mut state = showing_columns(4);
            state.contents.mode = mode;

            click_cell(&mut state, Panel::Contents, 0, 2);

            assert_eq!(state.contents.mode, mode, "from {mode:?}");
            assert_eq!(state.contents.column.index(), 2, "from {mode:?}");
        }
    }

    #[test]
    fn a_click_on_a_heading_moves_the_column_cursor_and_no_row() {
        let mut state = showing_columns(4);
        state.contents.cursor.go_to(0, 1);

        let effects = point(
            &mut state,
            Pointer::Click(Click::Column {
                panel: Panel::Contents,
                column: 3,
            }),
        );

        assert!(effects.is_empty());
        assert_eq!(state.focus(), Panel::Contents);
        assert_eq!(state.contents.column.index(), 3);
        assert_eq!(state.contents.cursor.index(), 0, "a heading names no row");
    }

    #[test]
    fn a_click_on_a_column_that_has_gone_is_dropped_rather_than_clamped() {
        let mut state = showing_columns(2);
        state.contents.column.go_to(1, 2);

        point(
            &mut state,
            Pointer::Click(Click::Column {
                panel: Panel::Contents,
                column: 5,
            }),
        );

        assert_eq!(
            state.contents.column.index(),
            1,
            "a column the page no longer has moves nothing"
        );
    }

    #[test]
    fn a_click_on_a_ddl_row_moves_the_cursor() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        click_row(&mut state, Panel::Tables, 0);
        click_row(&mut state, Panel::Tables, 1);
        let rows = crate::view::ddl_rows(&state);
        assert!(rows > 2, "the fixture must describe something, got {rows}");

        let effects = click_row(&mut state, Panel::Ddl, 2);

        assert!(effects.is_empty(), "a selection runs no query");
        assert_eq!(state.focus(), Panel::Ddl);
        assert_eq!(state.ddl.cursor.index(), 2);
    }

    #[test]
    fn a_click_on_a_ddl_row_that_has_gone_is_dropped_rather_than_clamped() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        click_row(&mut state, Panel::Tables, 0);

        // Nothing is selected, so the panel describes nothing and has no rows.
        let effects = click_row(&mut state, Panel::Ddl, 3);

        assert!(effects.is_empty());
        assert_eq!(state.ddl.cursor.index(), 0);
    }

    #[test]
    fn a_click_on_a_tab_focuses_its_panel_and_draws_it() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        click_row(&mut state, Panel::Tables, 0);
        click_row(&mut state, Panel::Tables, 1);
        state.focus_on(Panel::Queries);

        let effects = point(
            &mut state,
            Pointer::Click(Click::Tab {
                panel: Panel::Ddl,
                tab: 1,
            }),
        );

        assert!(effects.is_empty());
        assert_eq!(state.focus(), Panel::Ddl);
        assert_eq!(state.ddl.tab(), DdlTab::Columns);
    }

    #[test]
    fn a_click_on_the_logs_tab_focuses_queries_and_draws_the_trail() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);

        point(
            &mut state,
            Pointer::Click(Click::Tab {
                panel: Panel::Queries,
                tab: 1,
            }),
        );

        assert_eq!(state.focus(), Panel::Queries);
        assert_eq!(state.queries.tab(), QueriesTab::Logs);
    }

    /// The same reachability rule the key obeys: on a keyspace the strip has
    /// one tab, so a press on a strip drawn a frame ago cannot reach a tab
    /// this frame has not got.
    #[test]
    fn a_click_on_a_tab_the_strip_no_longer_draws_only_focuses() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(Panel::Queries);

        point(
            &mut state,
            Pointer::Click(Click::Tab {
                panel: Panel::Ddl,
                tab: 1,
            }),
        );

        assert_eq!(state.focus(), Panel::Ddl);
        assert_eq!(state.ddl.tab(), DdlTab::Ddl);
    }

    #[test]
    fn a_click_on_a_single_tab_panels_title_focuses_it() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(Panel::Queries);

        point(
            &mut state,
            Pointer::Click(Click::Tab {
                panel: Panel::Tables,
                tab: 0,
            }),
        );

        assert_eq!(state.focus(), Panel::Tables);
    }

    #[test]
    fn a_click_focuses_the_panel_it_landed_in() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(Panel::Queries);

        click_row(&mut state, Panel::Tables, 0);

        assert_eq!(state.focus(), Panel::Tables, "one click focuses and acts");
        assert_eq!(row_count(&state), 2, "the keyspace unfolded on that click");
    }

    #[test]
    fn a_click_on_chrome_focuses_and_does_nothing_else() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        let before = state.tables.clone();

        let effects = point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

        assert!(effects.is_empty());
        assert_eq!(
            state.tables, before,
            "chrome moves no selection and folds nothing"
        );
    }

    #[test]
    fn a_click_on_a_row_that_has_gone_is_dropped_rather_than_clamped() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        click_row(&mut state, Panel::Tables, 0);
        click_row(&mut state, Panel::Tables, 0);

        // Row 2 was `users` a frame ago; the tree is one row long now.
        let effects = click_row(&mut state, Panel::Tables, 2);

        assert!(effects.is_empty(), "a stale row reads nothing");
        assert_eq!(state.tables.cursor.index(), 0, "and moves nothing");
    }

    #[test]
    fn a_click_dismisses_the_overlay_and_does_nothing_else() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.overlay = Some(failure());

        let effects = click_row(&mut state, Panel::Tables, 0);

        assert!(state.overlay.is_none());
        assert!(effects.is_empty());
        assert_eq!(
            row_count(&state),
            1,
            "the keyspace the overlay covered did not unfold"
        );
    }

    #[test]
    fn a_wheel_scrolls_the_panel_under_it_without_focusing_it() {
        let mut state = showing_rows(20);
        state.geometry = Geometry::new([8, 8, 8, 1], 0, 0);
        state.focus_on(Panel::Tables);

        point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Contents,
                direction: Wheel::Down,
            },
        );

        assert_eq!(state.contents.scroll.offset(), 3, "three rows a notch");
        assert_eq!(
            state.contents.cursor.index(),
            0,
            "the wheel does not move the selection"
        );
        assert_eq!(state.focus(), Panel::Tables, "focus stays where it was");
    }

    #[test]
    fn a_wheel_up_walks_back_the_way_it_came() {
        let mut state = showing_rows(20);
        state.geometry = Geometry::new([8, 8, 8, 1], 0, 0);
        let down = Pointer::Wheel {
            panel: Panel::Contents,
            direction: Wheel::Down,
        };
        point(&mut state, down);
        point(&mut state, down);

        point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Contents,
                direction: Wheel::Up,
            },
        );

        assert_eq!(state.contents.scroll.offset(), 3);
    }

    #[test]
    fn a_wheel_over_the_ddl_panel_scrolls_its_lines() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.geometry = Geometry::new([8, 8, 8, 1], 40, 0);

        point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Ddl,
                direction: Wheel::Down,
            },
        );

        assert_eq!(state.ddl.scroll.offset(), 3);
    }

    #[test]
    fn a_wheel_over_a_panel_with_nothing_to_scroll_is_inert() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(Panel::Queries);
        for character in "one\ntwo\nthree\nfour\nfive\n".chars() {
            press(
                &mut state,
                if character == '\n' {
                    Key::Enter
                } else {
                    Key::Char(character)
                },
            );
        }
        state.focus_on(Panel::Tables);
        let cursor_before = state.queries.cursor();
        let text_before = state.queries.text();

        let effects = point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Queries,
                direction: Wheel::Down,
            },
        );

        assert!(effects.is_empty());
        assert_eq!(state.tables.cursor.index(), 0, "no other panel moved");
        assert_eq!(state.focus(), Panel::Tables);
        assert_eq!(
            state.queries.cursor(),
            cursor_before,
            "the wheel did not move the editor's cursor"
        );
        assert_eq!(
            state.queries.text(),
            text_before,
            "the wheel did not touch the buffer"
        );
    }

    #[test]
    fn a_wheel_over_tables_moves_the_viewport_rather_than_the_selection() {
        // Four keyspaces, so three rows a notch would have landed on the
        // last one under the old cursor-moving wheel. Now nothing about the
        // tree selection changes, so `described()` sees no subject change
        // and `state.ddl.scroll` is left alone (design §7).
        let mut state = browsing(&["a", "b", "c", "d"]);
        state.geometry = Geometry::new([3, 3, 3, 1], 0, 0);
        state.focus_on(Panel::Queries);
        state.ddl.scroll.down(100, 2);
        let ddl_before = state.ddl.scroll;
        assert_ne!(ddl_before.offset(), 0, "the fixture must start scrolled");

        let effects = point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Tables,
                direction: Wheel::Down,
            },
        );

        assert!(effects.is_empty(), "a scroll alone asks for nothing");
        assert_eq!(state.tables.scroll.offset(), 1, "four rows, three visible");
        assert_eq!(state.tables.cursor.index(), 0, "the selection did not move");
        assert_eq!(
            state.focus(),
            Panel::Queries,
            "the wheel does not move focus"
        );
        assert_eq!(
            state.ddl.scroll, ddl_before,
            "no selection changed, so nothing new is described"
        );
    }

    #[test]
    fn a_wheel_does_not_dismiss_the_overlay() {
        let mut state = showing_rows(20);
        state.geometry = Geometry::new([8, 8, 8, 1], 0, 0);
        state.overlay = Some(failure());

        point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Contents,
                direction: Wheel::Down,
            },
        );

        assert!(state.overlay.is_some(), "only a click dismisses");
        assert_eq!(state.contents.cursor.index(), 0, "and nothing scrolled");
    }

    /// A Contents panel showing `count` rows in a five-row viewport.
    fn scrolling_rows(count: usize) -> State {
        let mut state = showing_rows(count);
        state.geometry = Geometry::new([5, 5, 5, 1], 0, 0);
        state
    }

    /// Named apart from the production `wheel`, which has the same parameter
    /// list and would otherwise be shadowed by this inside `use super::*`.
    fn roll(state: &mut State, panel: Panel, direction: Wheel) -> Vec<Effect> {
        point(state, Pointer::Wheel { panel, direction })
    }

    #[test]
    fn a_wheel_scrolls_the_view_and_leaves_the_selection_where_it_was() {
        let mut state = scrolling_rows(40);

        roll(&mut state, Panel::Contents, Wheel::Down);

        assert_eq!(state.contents.scroll.offset(), 3, "three rows a notch");
        assert_eq!(
            state.contents.cursor.index(),
            0,
            "the selection is where the user put it"
        );
    }

    #[test]
    fn a_wheel_up_walks_the_view_back() {
        let mut state = scrolling_rows(40);
        roll(&mut state, Panel::Contents, Wheel::Down);
        roll(&mut state, Panel::Contents, Wheel::Down);

        roll(&mut state, Panel::Contents, Wheel::Up);

        assert_eq!(state.contents.scroll.offset(), 3);
    }

    #[test]
    fn a_wheel_stops_at_both_ends() {
        let mut state = scrolling_rows(8);

        for _ in 0..10 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        assert_eq!(
            state.contents.scroll.offset(),
            3,
            "eight rows in a five-row panel scroll by three and stop"
        );

        for _ in 0..10 {
            roll(&mut state, Panel::Contents, Wheel::Up);
        }
        assert_eq!(state.contents.scroll.offset(), 0);
    }

    #[test]
    fn a_motion_after_a_wheel_brings_the_selection_back_into_view() {
        let mut state = scrolling_rows(40);
        state.focus_on(Panel::Contents);
        for _ in 0..5 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        assert_eq!(state.contents.scroll.offset(), 15, "scrolled well past it");

        press(&mut state, Key::Char('j'));

        assert_eq!(state.contents.cursor.index(), 1);
        assert_eq!(
            state.contents.scroll.offset(),
            1,
            "the view came back to the selection, the shortest distance `follow` allows"
        );
    }

    #[test]
    fn walking_off_the_bottom_still_drags_the_view_with_it() {
        let mut state = scrolling_rows(40);
        state.focus_on(Panel::Contents);

        for _ in 0..6 {
            press(&mut state, Key::Char('j'));
        }

        assert_eq!(state.contents.cursor.index(), 6);
        assert_eq!(
            state.contents.scroll.offset(),
            2,
            "the sixth row is the last of a five-row viewport"
        );
    }

    #[test]
    fn a_click_on_a_scrolled_row_needs_no_correction() {
        let mut state = scrolling_rows(40);
        for _ in 0..4 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        let offset = state.contents.scroll.offset();

        click_row(&mut state, Panel::Contents, offset + 1);

        assert_eq!(state.contents.cursor.index(), offset + 1);
        assert_eq!(
            state.contents.scroll.offset(),
            offset,
            "the row was on screen, so the view had no reason to move"
        );
    }

    #[test]
    fn a_list_that_shrank_pulls_its_offset_back() {
        let mut state = browsing_tables(&[("shop", &["a", "b", "c", "d", "e", "f"])]);
        state.geometry = Geometry::new([3, 3, 3, 1], 0, 0);
        click_row(&mut state, Panel::Tables, 0);
        for _ in 0..3 {
            roll(&mut state, Panel::Tables, Wheel::Down);
        }
        assert_eq!(state.tables.scroll.offset(), 4, "seven rows, three visible");

        // Folding takes the tree back to one row, which no offset can be past.
        click_row(&mut state, Panel::Tables, 0);

        assert_eq!(row_count(&state), 1);
        assert_eq!(state.tables.scroll.offset(), 0);
    }

    #[test]
    fn a_wheel_over_tables_no_longer_scrolls_the_ddl_panel_back() {
        let mut state = browsing_tables(&[("shop", &["a", "b", "c", "d", "e", "f"])]);
        state.geometry = Geometry::new([3, 3, 3, 1], 40, 0);
        click_row(&mut state, Panel::Tables, 0);
        state.ddl.scroll.page_down(40, 3);
        let before = state.ddl.scroll;
        assert_ne!(before.offset(), 0, "the fixture starts scrolled");

        roll(&mut state, Panel::Tables, Wheel::Down);

        assert_eq!(
            state.ddl.scroll, before,
            "the wheel moved no selection, so nothing is newly described"
        );
    }

    #[test]
    fn a_resize_brings_the_selection_back_into_view() {
        let mut state = scrolling_rows(40);
        state.focus_on(Panel::Contents);
        for _ in 0..6 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(state.contents.cursor.index(), 6);
        assert_eq!(state.contents.scroll.offset(), 2, "on the last drawn row");

        update(&mut state, Msg::Measured(Geometry::new([2, 2, 2, 1], 0, 0)));

        assert_eq!(
            state.contents.scroll.offset(),
            5,
            "a panel that lost rows draws its selection again"
        );
    }

    #[test]
    fn a_measurement_that_changes_no_height_leaves_a_wheel_alone() {
        let mut state = scrolling_rows(40);
        for _ in 0..5 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        let scrolled = state.contents.scroll.offset();
        assert_eq!(scrolled, 15);

        update(&mut state, Msg::Measured(Geometry::new([5, 5, 5, 1], 0, 0)));

        assert_eq!(
            state.contents.scroll.offset(),
            scrolled,
            "the selection is off screen and the resize follow must not fetch it"
        );
        assert_eq!(state.contents.cursor.index(), 0);
    }

    #[test]
    fn a_resize_that_moves_a_viewport_asks_for_a_repaint() {
        let mut state = scrolling_rows(8);
        for _ in 0..5 {
            roll(&mut state, Panel::Contents, Wheel::Down);
        }
        assert_eq!(state.contents.scroll.offset(), 3);
        state.dirty = false;

        // Eight rows in an eight-row panel leave nothing to scroll.
        update(&mut state, Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0)));

        assert_eq!(state.contents.scroll.offset(), 0);
        assert!(
            state.dirty,
            "a measurement marks nothing dirty by itself, so an offset it \
             moved would otherwise not be drawn until the next keypress"
        );
    }

    /// Every `Key` the domain understands.
    ///
    /// The `match` is dead weight at runtime and load-bearing at compile time:
    /// a variant added to `Key` without a line here fails to compile, so the
    /// sweep below cannot quietly stop sweeping.
    fn every_key() -> Vec<Key> {
        match Key::Enter {
            Key::Char(_)
            | Key::Enter
            | Key::Escape
            | Key::Backspace
            | Key::Tab
            | Key::BackTab
            | Key::Delete
            | Key::Insert
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
            | Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::F(_) => {}
        }

        [
            Key::Enter,
            Key::Escape,
            Key::Backspace,
            Key::Tab,
            Key::BackTab,
            Key::Delete,
            Key::Insert,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
        ]
        .into_iter()
        .chain((1..=12).map(Key::F))
        .chain((0x20..=0x7e_u8).map(|byte| Key::Char(char::from(byte))))
        .collect()
    }

    /// Every `KeyPress` worth sweeping for the palette's completeness claim.
    ///
    /// Unmodified, plus ctrl held: ctrl is the only modifier `handle_key`
    /// dispatches on (`ctrl_key`) rather than refusing outright, so it is the
    /// only one that can ever do something the palette fails to list. Alt is
    /// refused wholesale before any arm sees it and shift never reaches a
    /// letter — sweeping either would only double the cost of this test for
    /// no coverage a future binding could fail to have.
    fn every_press() -> Vec<KeyPress> {
        every_key()
            .into_iter()
            .flat_map(|key| [KeyPress::new(key), KeyPress::ctrl(key)])
            .collect()
    }

    fn catalogued() -> Vec<KeyPress> {
        CATALOGUE
            .iter()
            .flat_map(|group| group.commands)
            .flat_map(|command| command.binding.keys)
            .copied()
            .map(Stroke::press)
            .collect()
    }

    /// States that between them exercise every binding: one with an overlay to
    /// dismiss, an unfolded keyspace to fold, and the tree at its first row;
    /// one focused somewhere other than Tables so the panel numbers differ;
    /// one with the tree folded and moved off its first row, so `k`, `pgup`
    /// and `home` all have somewhere to go; and one with the palette open and
    /// one with the theme picker open, the states that capture keys rather
    /// than passing them to a panel — a sweep that never opens one cannot see
    /// what it captures; and one focused on a loaded grid, without which the
    /// `Table contents` group's keys are only ever answered by the tree and
    /// `m` would go unswept.
    ///
    /// The unfold in `covered` matters: with every keyspace already folded,
    /// `h` and `←` would be a no-op in all four and the sweep would call them
    /// unbound. `moved`'s keyspaces are childless, so `l` and `→` never
    /// register there either — `covered` is the only fixture that opens a
    /// keyspace with something in it, and so the only one where those two
    /// keys do anything. `elsewhere`'s Ddl scroll starts off zero for the
    /// same reason `k`/`home` need `moved` off row zero: `ctrl-y` would be a
    /// no-op at the top of the document.
    fn sweep_fixtures() -> Vec<State> {
        let geometry = Geometry::new([8, 14, 20, 1], 37, 14);

        let mut covered = browsing_tables(&[("shop", &["orders"])]);
        covered.geometry = geometry;
        press(&mut covered, Key::Char('l'));
        covered.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

        let mut elsewhere = browsing_tables(&[("shop", &["orders"])]);
        elsewhere.geometry = geometry;
        elsewhere.focus_on(Panel::Ddl);
        // Off zero, or `ctrl-y` would find nothing to scroll up over here.
        elsewhere
            .ddl
            .scroll
            .down(geometry.ddl_lines(), geometry.height(Panel::Ddl));

        let mut moved = measured(&["a", "b", "c"]);
        press(&mut moved, Key::Char('j'));
        press(&mut moved, Key::Char('j'));

        let mut palette_open = measured(&["a", "b", "c"]);
        press(&mut palette_open, Key::Char('?'));

        // Off the first row, so `k` and `home` have somewhere to go here too.
        let mut picker_open = measured(&["a", "b", "c"]);
        ctrl(&mut picker_open, Key::Char('t'));
        press(&mut picker_open, Key::Char('j'));

        // A loaded grid, for `y` to have a row under the cursor.
        let mut yankable = read_shop_orders();
        press(&mut yankable, Key::Enter);
        let query = yankable
            .contents
            .outstanding()
            .expect("the panel is waiting");
        update(
            &mut yankable,
            Msg::PageReady {
                query,
                page: Box::new(page_of(3)),
                took: Duration::ZERO,
            },
        );
        // The one fixture `ctrl-e` opens the form from: a loaded grid on a
        // frame with room for it.
        yankable.geometry = yankable.geometry.with_export_fits(true);

        // Measured, so the columns `m` cycles the highlight over are drawn.
        let grid = grid_of_rows(4, 5, 80);

        // The one fixture the three column keys answer in: a grid in column
        // mode, with a filter already standing on the column the cursor is on
        // so that `c` has one to clear (grid sort and filter design §6).
        let mut columnar = grid_of_rows(4, 5, 80);
        press(&mut columnar, Key::Char('m'));
        assert_eq!(
            columnar.contents.mode,
            SelectionMode::Column,
            "the fixture is the one column mode reaches in a single press"
        );
        columnar.contents.set_filter(0, Search::new("1"));

        // The export form, open on its first stop. Every key `export_key`
        // claims there — `esc`, `tab`/`backtab`, `j`/`k`/`↓`/`↑`, `⏎` — is
        // already catalogued elsewhere (`Global`, `Motion`, `Tables`), so
        // this adds no key the negative sweep has to worry about; only
        // `Export`'s own `space` needs a state where it does something, and
        // this is that state (`toggle_scope` only fires with a scope to
        // toggle to, hence `ROW_LIMIT` rather than a smaller page).
        let mut export_open = contents_showing(ROW_LIMIT);
        ctrl(&mut export_open, Key::Char('e'));

        // The one fixture `n`/`N` do something in: a search applied, with
        // more than one match to walk between (design §5.3).
        let mut searched = browsing_tables(&[("shop", &["orders"])]);
        searched.geometry = geometry;
        searched.apply_search(Searched::Tables, Search::new("o"));

        // The two fixtures the `Drop` group needs: one where `d` opens a
        // question, and one with that question open, so the negative sweep
        // sees what the confirmation captures. The typed spelling is left
        // out for the reason the editor is — every printable key is text
        // there, which would tell the negative sweep the palette is missing
        // the alphabet (`the_typed_confirmation_takes_text` covers it).
        let mut droppable = droppable_tree();
        press(&mut droppable, Key::Char('j'));

        let mut dropping = droppable_tree();
        press(&mut dropping, Key::Char('j'));
        press(&mut dropping, Key::Char('d'));

        // The one state `[`/`]` do anything in: the DDL panel focused with a
        // table under the tree cursor, so its strip draws two tabs.
        let mut tabbed = browsing_a_table();
        tabbed.geometry = geometry;
        tabbed.focus_on(Panel::Ddl);

        vec![
            covered,
            elsewhere,
            moved,
            palette_open,
            picker_open,
            yankable,
            grid,
            columnar,
            export_open,
            droppable,
            dropping,
            searched,
            tabbed,
        ]
    }

    /// A writable connection with a table under the tree, on a frame with
    /// room for the confirmation: the state `d` answers in (drop design §1).
    /// The cursor starts on the keyspace row, so `j` reaches the table.
    fn droppable_tree() -> State {
        let mut state = connected(Posture::Writable);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(&[("shop", &["orders"])])),
                took: Duration::ZERO,
            },
        );
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14)
            .with_theme_rows(20)
            .with_drop_fits(true);
        // Unfolded, so the row below the keyspace is its table.
        press(&mut state, Key::Char('l'));
        state
    }

    /// Whether pressing `key` changes anything a user could observe.
    ///
    /// The view model rather than the `State`, which is not `PartialEq` and
    /// whose `dirty` flag `handle_key` sets on every press. It is also the
    /// better question: a key that changes nothing visible and emits no effect
    /// is not a binding, whatever it touched on the way through.
    fn does_something(state: &State, press: KeyPress) -> bool {
        responds(state, |after| press_binding(after, press))
    }

    /// The same question asked of a whole stroke: a chord is judged by what
    /// typing all of it does, since its first press only arms the prefix.
    fn stroke_does_something(state: &State, stroke: Stroke) -> bool {
        responds(state, |after| press_stroke(after, stroke))
    }

    fn responds(state: &State, press: impl FnOnce(&mut State) -> Vec<Effect>) -> bool {
        let mut after = state.clone();
        let effects = press(&mut after);

        !effects.is_empty()
            || after.should_quit != state.should_quit
            || crate::view::view_model(&after) != crate::view::view_model(state)
    }

    #[test]
    fn no_key_does_anything_the_palette_does_not_list() {
        let catalogued = catalogued();

        for state in sweep_fixtures() {
            for press in every_press() {
                if catalogued.contains(&press) {
                    continue;
                }

                assert!(
                    !does_something(&state, press),
                    "{press:?} does something and docs/shortcuts.md's palette does not list it"
                );
            }
        }
    }

    /// An editing state, for the `Queries` group alone. It is kept out of
    /// `sweep_fixtures` because in a panel that takes text every printable key
    /// does something, which would tell
    /// `no_key_does_anything_the_palette_does_not_list` that the palette is
    /// missing the alphabet. What is true there instead is pinned by
    /// `a_letter_typed_in_queries_is_text_not_a_shortcut`.
    fn editor_fixture() -> State {
        let mut editor = editing("SELECT * FROM shop.orders");
        editor.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        // One step off the buffer's end: `Delete` has nothing to remove
        // sitting exactly there (`delete_at_the_end_of_the_buffer_does_nothing`
        // in editor.rs), and `End` from the end is a no-op. Off by one, `Home`,
        // `End`, and `Delete` all have somewhere to move from.
        editor.queries.edit(Key::Left, 1);
        editor
    }

    /// The `Queries` group is checked against the editor fixture *alone*, and
    /// every other group against `sweep_fixtures` alone. Both directions
    /// matter. A Global, Motion or Tables key pressed in the editor types
    /// itself, so it would "do something" whether or not its real binding
    /// still exists. And a Queries key satisfied by some other fixture —
    /// `⏎` folding a keyspace, `esc` dismissing an overlay, `home`/`end`
    /// moving a tree cursor — would let the editor quietly stop claiming it
    /// with the sweep still green. Each group is answered by the state it
    /// describes.
    #[test]
    fn every_key_the_palette_lists_does_something() {
        let plain = sweep_fixtures();
        let editor = vec![editor_fixture()];

        for group in CATALOGUE {
            let fixtures = if group.heading == "Queries" {
                &editor
            } else {
                &plain
            };

            for command in group.commands {
                for &press in command.binding.keys {
                    assert!(
                        fixtures
                            .iter()
                            .any(|state| stroke_does_something(state, press)),
                        "{} lists {press:?} and no state responds to it",
                        group.heading
                    );
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Dropping (docs/superpowers/specs/2026-08-31-drop-design.md)
    // ------------------------------------------------------------------

    fn dropping(state: &State) -> Option<&DropPrompt> {
        match &state.overlay {
            Some(Overlay::Drop(prompt)) => Some(prompt),
            _ => None,
        }
    }

    fn type_in(state: &mut State, text: &str) {
        for character in text.chars() {
            press(state, Key::Char(character));
        }
    }

    /// The cursor on the keyspace row of `droppable_tree`.
    fn on_keyspace() -> State {
        droppable_tree()
    }

    /// The cursor on the table row below it.
    fn on_table() -> State {
        let mut state = droppable_tree();
        press(&mut state, Key::Char('j'));
        state
    }

    #[test]
    fn d_on_a_keyspace_asks_for_its_name() {
        let mut state = on_keyspace();

        press(&mut state, Key::Char('d'));

        let prompt = dropping(&state).expect("the confirmation is up");
        assert_eq!(prompt.target(), &Target::Keyspace("shop".to_owned()));
        assert!(prompt.takes_text(), "a keyspace is always typed back");
    }

    #[test]
    fn d_on_a_table_asks_yes_or_no() {
        let mut state = on_table();

        press(&mut state, Key::Char('d'));

        let prompt = dropping(&state).expect("the confirmation is up");
        assert_eq!(
            prompt.target(),
            &Target::Table(TableRef::new("shop", "orders"))
        );
        assert!(!prompt.takes_text());
    }

    #[test]
    fn a_protected_connection_types_a_table_name_back() {
        let mut state = on_table();
        let ConnectionState::Connected(connection) = &mut state.connection else {
            panic!("the fixture is connected");
        };
        connection.posture = Posture::Protected;

        press(&mut state, Key::Char('d'));

        assert!(
            dropping(&state)
                .expect("the confirmation is up")
                .takes_text(),
            "a protected connection needs the name typed, whatever the target"
        );
    }

    #[test]
    fn y_drops_the_table() {
        let mut state = on_table();
        press(&mut state, Key::Char('d'));

        let effects = press(&mut state, Key::Char('y'));

        assert!(state.overlay.is_none(), "the question is answered");
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::Execute { stmt, .. } if stmt.cql == "DROP TABLE \"shop\".\"orders\""
            )),
            "{effects:?}"
        );
        assert!(state.dropping.is_some(), "the drop is outstanding");
    }

    #[test]
    fn n_abandons_the_drop() {
        let mut state = on_table();
        press(&mut state, Key::Char('d'));

        let effects = press(&mut state, Key::Char('n'));

        assert!(state.overlay.is_none());
        assert!(effects.is_empty(), "nothing reaches the cluster");
        assert!(state.dropping.is_none());
    }

    #[test]
    fn esc_abandons_the_drop_on_either_spelling() {
        for mut state in [on_table(), on_keyspace()] {
            press(&mut state, Key::Char('d'));
            let effects = press(&mut state, Key::Escape);

            assert!(state.overlay.is_none());
            assert!(effects.is_empty());
        }
    }

    #[test]
    fn enter_on_a_mismatched_name_does_nothing() {
        let mut state = on_keyspace();
        press(&mut state, Key::Char('d'));
        type_in(&mut state, "sho");

        let effects = press(&mut state, Key::Enter);

        assert!(effects.is_empty(), "a half-typed name drops nothing");
        assert!(
            dropping(&state).is_some(),
            "and the question is still being asked"
        );
    }

    #[test]
    fn enter_on_the_typed_name_drops_the_keyspace() {
        let mut state = on_keyspace();
        press(&mut state, Key::Char('d'));
        type_in(&mut state, "shop");

        let effects = press(&mut state, Key::Enter);

        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::Execute { stmt, .. } if stmt.cql == "DROP KEYSPACE \"shop\""
            )),
            "{effects:?}"
        );
    }

    /// `n` cannot be a cancel where a name holding one has to be typeable
    /// (design §2.1).
    #[test]
    fn n_is_text_on_a_typed_confirmation() {
        let mut state = connected(Posture::Writable);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(&[("nation", &["orders"])])),
                took: Duration::ZERO,
            },
        );
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14).with_drop_fits(true);

        press(&mut state, Key::Char('d'));
        type_in(&mut state, "nation");

        let effects = press(&mut state, Key::Enter);

        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::Execute { stmt, .. } if stmt.cql == "DROP KEYSPACE \"nation\""
            )),
            "every `n` typed itself; {effects:?}"
        );
    }

    #[test]
    fn a_typed_confirmation_takes_text_rather_than_quitting_or_moving() {
        let mut state = on_keyspace();
        press(&mut state, Key::Char('d'));
        let before = state.tables.cursor.index();

        for key in ['q', 'j', 'G', 'y', '1'] {
            press(&mut state, Key::Char(key));
        }

        assert!(!state.should_quit, "`q` is a character here");
        assert_eq!(state.tables.cursor.index(), before);
        assert_eq!(state.focus(), Panel::Tables);
        assert_eq!(
            dropping(&state).and_then(DropPrompt::typed).map(Line::text),
            Some("qjGy1")
        );
    }

    #[test]
    fn a_yes_no_confirmation_swallows_the_keys_it_does_not_use() {
        let mut state = on_table();
        press(&mut state, Key::Char('d'));
        let before = crate::view::view_model(&state);

        for key in ['q', 'j', '?', 'v', 'm'] {
            press(&mut state, Key::Char(key));
        }

        assert!(!state.should_quit);
        assert_eq!(crate::view::view_model(&state), before);
    }

    #[test]
    fn d_on_a_read_only_connection_asks_nothing_and_says_why() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.geometry = state.geometry.with_drop_fits(true);

        press(&mut state, Key::Char('d'));

        assert!(
            !matches!(state.overlay, Some(Overlay::Drop(_))),
            "a question the statement's own guard would refuse is not asked"
        );
        assert!(
            matches!(&state.overlay, Some(Overlay::Message(message))
                if message.title == "Read-only connection"),
            "the reason the key did nothing is the reason the user is owed"
        );
    }

    /// The key is greyed *and* answered. Greying alone leaves a user who
    /// pressed it with nothing, and the bar cannot say which connection is
    /// the read-only one (posture design §3).
    #[test]
    fn the_read_only_refusal_names_the_way_out_of_it() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.geometry = state.geometry.with_drop_fits(true);

        press(&mut state, Key::Char('d'));

        let Some(Overlay::Message(message)) = &state.overlay else {
            panic!("refused with a modal");
        };
        assert!(message.message.contains("writable"), "{}", message.message);
        assert_eq!(message.severity, Severity::Danger);
    }

    #[test]
    fn d_is_inert_where_the_frame_cannot_draw_the_block() {
        let mut state = on_table();
        state.geometry = state.geometry.with_drop_fits(false);

        press(&mut state, Key::Char('d'));

        assert!(state.overlay.is_none());
    }

    #[test]
    fn d_is_inert_while_a_drop_is_running() {
        let mut state = on_table();
        press(&mut state, Key::Char('d'));
        press(&mut state, Key::Char('y'));
        assert!(state.dropping.is_some(), "one is outstanding");

        press(&mut state, Key::Escape);
        press(&mut state, Key::Char('d'));

        assert!(state.overlay.is_none(), "the second would race the first");
    }

    #[test]
    fn d_is_inert_over_a_multi_row_selection() {
        let mut state = on_keyspace();
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('d'));

        assert!(
            state.overlay.is_none(),
            "'drop these two' is a different question"
        );
    }

    #[test]
    fn d_does_nothing_outside_the_tree() {
        for panel in [Panel::Ddl, Panel::Contents] {
            let mut state = on_table();
            state.focus_on(panel);

            press(&mut state, Key::Char('d'));

            assert!(state.overlay.is_none(), "{panel:?} answers no drop");
        }
    }

    #[test]
    fn a_completed_drop_re_reads_the_schema() {
        let mut state = on_table();
        press(&mut state, Key::Char('d'));
        press(&mut state, Key::Char('y'));
        let query = state
            .dropping
            .as_ref()
            .expect("the drop is outstanding")
            .query();

        let effects = update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page::default()),
                took: Duration::ZERO,
            },
        );

        assert_eq!(effects, vec![Effect::LoadSchema(current_conn(&state))]);
        assert!(state.dropping.is_none(), "the round trip is over");
        assert!(
            matches!(&state.overlay, Some(Overlay::Message(message))
                if message.message.contains("shop.orders")),
            "the modal names what is gone"
        );
    }

    #[test]
    fn a_failed_drop_reloads_nothing_and_says_why() {
        let mut state = on_table();
        press(&mut state, Key::Char('d'));
        press(&mut state, Key::Char('y'));
        let query = state
            .dropping
            .as_ref()
            .expect("the drop is outstanding")
            .query();

        let effects = update(
            &mut state,
            Msg::Failed {
                query,
                error: DbError::Unreachable("no route".to_owned()),
                took: Duration::ZERO,
            },
        );

        assert!(effects.is_empty(), "the schema did not change");
        assert!(state.dropping.is_none());
        assert!(
            matches!(&state.overlay, Some(Overlay::Message(message))
                if message.severity == Severity::Danger && message.message.contains("no route")),
            "the cluster's own reason is what is raised"
        );
    }

    /// The trail is where the reason survives the modal (design §5).
    #[test]
    fn a_drop_the_posture_refuses_is_logged_and_raised() {
        let mut state = on_table();
        press(&mut state, Key::Char('d'));
        // Between the question and the answer the connection turns read-only.
        let ConnectionState::Connected(connection) = &mut state.connection else {
            panic!("the fixture is connected");
        };
        connection.posture = Posture::ReadOnly;

        let effects = press(&mut state, Key::Char('y'));

        assert!(effects.is_empty(), "the posture guard is in `submit`");
        assert!(matches!(&state.overlay, Some(Overlay::Message(_))));
        assert!(
            state
                .log
                .entries()
                .any(|entry| matches!(entry.outcome(), Outcome::Rejected { .. })),
            "the trail keeps the refusal the modal loses"
        );
    }

    /// A result for a drop that is no longer outstanding must not reload the
    /// schema behind a user who has moved on (architecture.md §3).
    #[test]
    fn a_page_for_an_ordinary_read_reloads_nothing() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");

        let effects = update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(3)),
                took: Duration::ZERO,
            },
        );

        assert!(effects.is_empty());
    }

    #[test]
    fn a_frame_that_shrank_past_the_drop_block_closes_it() {
        let mut state = on_table();
        press(&mut state, Key::Char('d'));

        let shrunk = state.geometry.with_drop_fits(false);
        update(&mut state, Msg::Measured(shrunk));

        assert!(
            state.overlay.is_none(),
            "a mode nothing draws still capturing every key is the failure this prevents"
        );
    }

    #[test]
    fn a_second_d_leaves_the_open_confirmation_as_it_was() {
        let mut state = on_keyspace();
        press(&mut state, Key::Char('d'));
        type_in(&mut state, "sho");

        press(&mut state, Key::Char('d'));

        assert_eq!(
            dropping(&state).and_then(DropPrompt::typed).map(Line::text),
            Some("shod"),
            "`d` is a character in the field, not a second opener"
        );
    }

    // ------------------------------------------------------------------
    // Deleting rows (drop design §3.1, §4)
    // ------------------------------------------------------------------

    /// A grid over `shop.orders`, whose fixture primary key is
    /// `(id uuid, created_at timestamp)`, holding `rows` addressable rows.
    fn deletable(rows: usize) -> State {
        grid_over(
            rows,
            &["id", "created_at", "email"],
            |index| {
                vec![
                    CqlValue::Uuid(format!("8f14e45f-000{index}")),
                    CqlValue::Timestamp(
                        1_699_999_999_000 + i64::try_from(index).unwrap_or_default(),
                    ),
                    CqlValue::Text("ada@example.com".to_owned()),
                ]
            },
            Posture::Writable,
        )
    }

    fn grid_over(
        rows: usize,
        columns: &[&str],
        values: impl Fn(usize) -> Vec<CqlValue>,
        posture: Posture,
    ) -> State {
        let mut state = connected(posture);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(&[("shop", &["orders"])])),
                took: Duration::ZERO,
            },
        );
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14).with_drop_fits(true);

        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            Page {
                columns: columns.iter().map(|name| (*name).to_owned()).collect(),
                rows: (0..rows)
                    .map(|index| Row {
                        values: values(index),
                    })
                    .collect(),
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );
        state.focus_on(Panel::Contents);
        state
    }

    fn deleted_cql(effects: &[Effect]) -> Option<String> {
        effects.iter().find_map(|effect| match effect {
            Effect::Execute { stmt, .. } => Some(stmt.cql.clone()),
            _ => None,
        })
    }

    #[test]
    fn d_over_a_grid_asks_about_the_row_under_the_cursor() {
        let mut state = deletable(3);

        press(&mut state, Key::Char('d'));

        let prompt = dropping(&state).expect("the confirmation is up");
        assert_eq!(prompt.target().row_count(), Some(1));
        assert!(
            !prompt.takes_text(),
            "a delete is answered y/n on a writable connection"
        );
    }

    #[test]
    fn d_over_a_grid_asks_about_every_selected_row() {
        let mut state = deletable(3);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('d'));

        assert_eq!(
            dropping(&state)
                .expect("the confirmation is up")
                .target()
                .row_count(),
            Some(3),
            "the range is what a command over the grid acts on"
        );
    }

    #[test]
    fn y_deletes_the_row_by_its_whole_primary_key() {
        let mut state = deletable(3);
        press(&mut state, Key::Char('d'));

        let effects = press(&mut state, Key::Char('y'));

        assert_eq!(
            deleted_cql(&effects).as_deref(),
            Some(
                "DELETE FROM \"shop\".\"orders\" WHERE \"id\" = 8f14e45f-0000 \
                 AND \"created_at\" = 1699999999000"
            )
        );
        assert!(state.dropping.is_some(), "the delete is outstanding");
    }

    #[test]
    fn several_rows_delete_as_one_batch() {
        let mut state = deletable(3);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('d'));
        let effects = press(&mut state, Key::Char('y'));

        let cql = deleted_cql(&effects).expect("a statement");
        assert!(cql.starts_with("BEGIN BATCH\n"), "{cql}");
        assert!(cql.ends_with("APPLY BATCH"), "{cql}");
        assert_eq!(cql.matches("DELETE FROM").count(), 2, "{cql}");
    }

    #[test]
    fn n_abandons_a_delete() {
        let mut state = deletable(3);
        press(&mut state, Key::Char('d'));

        let effects = press(&mut state, Key::Char('n'));

        assert!(state.overlay.is_none());
        assert!(effects.is_empty());
        assert!(state.dropping.is_none());
    }

    /// The "primary key known and present" gate (design §3.1), each half of
    /// it on its own.
    #[test]
    fn d_is_inert_where_the_rows_cannot_be_addressed() {
        let missing_key_column = grid_over(
            2,
            &["email"],
            |_| vec![CqlValue::Text("ada@example.com".to_owned())],
            Posture::Writable,
        );
        let unspellable_value = grid_over(
            2,
            &["id", "created_at"],
            |_| vec![CqlValue::Uuid("8f14e45f".to_owned()), CqlValue::Null],
            Posture::Writable,
        );

        for (mut state, why) in [
            (missing_key_column, "a page without every key column"),
            (unspellable_value, "a key column holding no literal"),
        ] {
            press(&mut state, Key::Char('d'));

            assert!(state.overlay.is_none(), "{why} addresses no row");
        }
    }

    #[test]
    fn d_is_inert_over_a_grid_filled_by_a_typed_statement() {
        let mut state = deletable(2);
        let query = QueryId::next();
        state.contents.requested(
            query,
            Subject::Typed("SELECT * FROM shop.orders".to_owned()),
        );
        state.contents.page_ready(
            query,
            Page {
                columns: vec!["id".to_owned(), "created_at".to_owned()],
                rows: vec![Row {
                    values: vec![
                        CqlValue::Uuid("8f14e45f".to_owned()),
                        CqlValue::Timestamp(0),
                    ],
                }],
                next: None,
            },
            crate::config::MaxColumnWidth::default(),
        );

        press(&mut state, Key::Char('d'));

        assert!(
            state.overlay.is_none(),
            "a typed statement names its table in text this will not trust"
        );
    }

    #[test]
    fn d_is_inert_over_a_grid_with_no_page() {
        let mut state = connected(Posture::Writable);
        state.geometry = Geometry::default().with_drop_fits(true);
        state.focus_on(Panel::Contents);

        press(&mut state, Key::Char('d'));

        assert!(state.overlay.is_none());
    }

    #[test]
    fn a_delete_is_refused_on_a_read_only_connection() {
        let mut state = grid_over(
            2,
            &["id", "created_at"],
            |_| {
                vec![
                    CqlValue::Uuid("8f14e45f".to_owned()),
                    CqlValue::Timestamp(0),
                ]
            },
            Posture::ReadOnly,
        );

        press(&mut state, Key::Char('d'));

        assert!(
            matches!(&state.overlay, Some(Overlay::Message(message))
                if message.title == "Read-only connection"),
            "the grid's `d` is refused by the same posture gate the tree's is"
        );
    }

    /// A `Protected` connection is about *destructive* statements, and a row
    /// delete is a `Write` — so it stays y/n (design §2).
    #[test]
    fn a_protected_connection_still_answers_a_delete_with_y() {
        let mut state = deletable(2);
        let ConnectionState::Connected(connection) = &mut state.connection else {
            panic!("the fixture is connected");
        };
        connection.posture = Posture::Protected;

        press(&mut state, Key::Char('d'));

        assert!(
            !dropping(&state)
                .expect("the confirmation is up")
                .takes_text(),
            "the posture rule names destructive statements, which a delete is not"
        );
    }

    /// What went stale is what gets re-read: a delete changed the rows of one
    /// table, and the grid is showing them (design §4).
    #[test]
    fn a_completed_delete_re_reads_the_grid_rather_than_the_schema() {
        let mut state = deletable(3);
        press(&mut state, Key::Char('d'));
        press(&mut state, Key::Char('y'));
        let query = state
            .dropping
            .as_ref()
            .expect("the delete is outstanding")
            .query();

        let effects = update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page::default()),
                took: Duration::ZERO,
            },
        );

        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadSchema(_))),
            "the schema did not change"
        );
        assert_eq!(
            deleted_cql(&effects).as_deref(),
            Some("SELECT * FROM \"shop\".\"orders\" LIMIT 200"),
            "the grid is re-read so the rows stop being drawn"
        );
        assert!(
            matches!(&state.overlay, Some(Overlay::Message(message))
                if message.message.contains("1 row from shop.orders")),
            "the modal names what is gone"
        );
    }

    fn searching(state: &State) -> Option<&Prompt> {
        match &state.overlay {
            Some(Overlay::Search(prompt)) => Some(prompt),
            _ => None,
        }
    }

    #[test]
    fn slash_opens_the_prompt_in_every_panel_that_can_hold_a_search() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let mut state = browsing_tables(&[("shop", &["orders"])]);
            state.focus_on(panel);

            press(&mut state, Key::Char('/'));

            assert_eq!(
                searching(&state).map(Prompt::target),
                Searched::of(panel),
                "{panel:?} must reach a prompt"
            );
        }
    }

    #[test]
    fn slash_is_text_in_queries_and_opens_nothing() {
        let mut state = State::default();
        state.focus_on(Panel::Queries);

        press(&mut state, Key::Char('/'));

        assert!(state.overlay.is_none(), "the editor claims it first");
        assert_eq!(state.queries.text(), "/");
    }

    #[test]
    fn q_and_the_question_mark_type_themselves_in_the_prompt() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Char('q'));
        press(&mut state, Key::Char('?'));

        assert!(!state.should_quit, "q is text while a prompt is up");
        assert_eq!(searching(&state).map(Prompt::query), Some("q?"));
    }

    #[test]
    fn the_prompt_searches_live_per_keystroke() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Char('u'));

        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("u"),
            "the panel underneath is the preview"
        );
    }

    #[test]
    fn backspace_narrows_the_query_and_re_applies_it() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));
        press(&mut state, Key::Char('z'));

        press(&mut state, Key::Backspace);

        assert_eq!(searching(&state).map(Prompt::query), Some("o"));
        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("o")
        );
    }

    #[test]
    fn backspace_on_an_empty_query_is_inert_rather_than_closing() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Backspace);

        assert_eq!(searching(&state).map(Prompt::query), Some(""));
    }

    #[test]
    fn enter_commits_and_closes_leaving_the_search_applied() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));

        press(&mut state, Key::Enter);

        assert!(state.overlay.is_none());
        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("o")
        );
    }

    #[test]
    fn an_empty_commit_clears_the_search() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.apply_search(Searched::Tables, Search::new("o"));
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Enter);

        assert!(state.overlay.is_none());
        assert_eq!(state.search(Searched::Tables), None);
    }

    /// The design's own mechanism for an empty commit: `Search::new("")` is
    /// `None`, so backspacing a live query back to nothing already clears the
    /// search before `Enter` is ever pressed — unlike
    /// `an_empty_commit_clears_the_search` above, whose buffer is empty
    /// because nothing was ever typed into it.
    #[test]
    fn backspacing_a_query_to_empty_clears_the_search_live() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.apply_search(Searched::Tables, Search::new("o"));
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));

        press(&mut state, Key::Backspace);

        assert_eq!(
            state.search(Searched::Tables),
            None,
            "the buffer is empty, so the live preview already has no search"
        );

        press(&mut state, Key::Enter);

        assert!(state.overlay.is_none());
        assert_eq!(state.search(Searched::Tables), None);
    }

    #[test]
    fn typing_a_term_selects_the_first_match() {
        let mut state = loaded();
        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('/'));

        for character in "orders".chars() {
            press(&mut state, Key::Char(character));
        }

        let schema = state.schema().expect("schema loaded");
        let Some(Node::Table { table, .. }) = state.tables.selected(schema) else {
            panic!("the cursor must be on the matching table");
        };
        assert!(table.name.contains("orders"));
    }

    #[test]
    fn extending_a_term_does_not_walk_the_cursor_forward() {
        let mut state = loaded();
        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Char('o'));
        let after_one = state.tables.cursor.index();
        press(&mut state, Key::Backspace);
        press(&mut state, Key::Char('o'));

        assert_eq!(
            state.tables.cursor.index(),
            after_one,
            "every keystroke searches from where / was pressed, not from the last landing"
        );
    }

    #[test]
    fn a_term_that_matches_nothing_leaves_the_cursor_on_the_empty_tree() {
        let mut state = loaded();
        state.focus_on(Panel::Tables);
        state.tables.cursor.go_to(1, tree_rows(&state));
        press(&mut state, Key::Char('/'));

        for character in "zzz".chars() {
            press(&mut state, Key::Char(character));
        }

        assert_eq!(tree_rows(&state), 0, "no keyspace survives the term");
        assert_eq!(
            state.tables.cursor.index(),
            0,
            "there is no row 1 left to sit on; Esc is what puts the cursor back"
        );
    }

    #[test]
    fn escape_puts_the_cursor_back_on_the_row_a_matchless_term_emptied_out() {
        let mut state = loaded();
        state.focus_on(Panel::Tables);
        state.tables.cursor.go_to(1, tree_rows(&state));
        press(&mut state, Key::Char('/'));
        for character in "zzz".chars() {
            press(&mut state, Key::Char(character));
        }

        press(&mut state, Key::Escape);

        assert_eq!(state.tables.cursor.index(), 1);
    }

    #[test]
    fn a_term_extended_past_its_last_match_restores_the_cursor_to_origin() {
        // Regression for the reviewer's repro: a term that jumps the cursor
        // and then stops matching must snap back to `origin`, not strand the
        // cursor at the last row it landed on (design §5.2).
        let mut state = browsing_tables(&[
            ("aaa", &[]),
            ("bbb", &[]),
            ("ccc", &[]),
            ("ddd", &[]),
            ("orders_ks", &[]),
        ]);
        state.focus_on(Panel::Tables);
        state.tables.cursor.go_to(1, tree_rows(&state));
        press(&mut state, Key::Char('/'));

        for character in "orders".chars() {
            press(&mut state, Key::Char(character));
        }
        assert_eq!(
            state.tables.cursor.index(),
            0,
            "orders_ks is the one keyspace the term keeps, so it is the only row"
        );

        press(&mut state, Key::Char('x'));

        assert_eq!(tree_rows(&state), 0, "ordersx keeps nothing");
        assert_eq!(state.tables.cursor.index(), 0);

        press(&mut state, Key::Escape);

        assert_eq!(
            state.tables.cursor.index(),
            1,
            "the abandoned prompt leaves the panel on the row it opened on"
        );
    }

    #[test]
    fn backspace_to_empty_restores_the_cursor_to_origin() {
        let mut state = browsing_tables(&[
            ("aaa", &[]),
            ("bbb", &[]),
            ("ccc", &[]),
            ("ddd", &[]),
            ("orders_ks", &[]),
        ]);
        state.focus_on(Panel::Tables);
        state.tables.cursor.go_to(1, tree_rows(&state));
        press(&mut state, Key::Char('/'));

        for character in "ordersx".chars() {
            press(&mut state, Key::Char(character));
        }
        assert_eq!(tree_rows(&state), 0, "ordersx matches nothing");

        for _ in 0.."ordersx".chars().count() {
            press(&mut state, Key::Backspace);
        }

        assert_eq!(state.search(Searched::Tables), None);
        assert_eq!(
            state.tables.cursor.index(),
            1,
            "the buffer is empty, but origin is still where the cursor belongs"
        );
    }

    #[test]
    fn escape_restores_the_search_and_the_cursor_together() {
        let mut state = loaded();
        state.focus_on(Panel::Tables);
        state.tables.cursor.go_to(1, tree_rows(&state));
        press(&mut state, Key::Char('/'));
        for character in "orders".chars() {
            press(&mut state, Key::Char(character));
        }

        press(&mut state, Key::Escape);

        assert_eq!(state.tables.cursor.index(), 1, "the cursor goes back too");
        assert_eq!(state.search(Searched::Tables), None);
    }

    #[test]
    fn enter_keeps_where_the_cursor_landed() {
        let mut state = loaded();
        state.focus_on(Panel::Tables);
        press(&mut state, Key::Char('/'));
        for character in "orders".chars() {
            press(&mut state, Key::Char(character));
        }
        let landed = state.tables.cursor.index();

        press(&mut state, Key::Enter);

        assert_eq!(state.tables.cursor.index(), landed);
    }

    #[test]
    fn n_walks_to_the_next_match_and_wraps() {
        let mut state = loaded();
        state.focus_on(Panel::Tables);
        // "o" rather than "orders": "billing" carries neither letter and is
        // dropped, leaving the "shop" keyspace (row 0) and the "orders"
        // table it holds (row 1), both matches. Exact indices, not a
        // before/after comparison, so the second press pins the wrap rather
        // than merely staying in bounds by accident.
        state.apply_search(Searched::Tables, Search::new("o"));

        press(&mut state, Key::Char('n'));
        assert_eq!(
            state.tables.cursor.index(),
            1,
            "\"orders\" is the first match after row 0"
        );

        press(&mut state, Key::Char('n'));
        assert_eq!(
            state.tables.cursor.index(),
            0,
            "the last match wraps back to \"shop\""
        );
    }

    #[test]
    fn shift_n_walks_backwards() {
        // Two matches is not enough to tell `n` from `N` apart: with only
        // one other match in the ring, walking either direction from it
        // lands on that same other match. A third row is what makes forward
        // and backward diverge, so "orders" (one match) and "o" against
        // `loaded()` (two matches, but the ring itself is three rows with
        // only two of them matching) both fail to pin this — `box` is
        // added so every row of a three-row ring matches "o" and the
        // starting row genuinely has a different match on each side of it.
        let mut state = browsing_tables(&[("box", &[]), ("shop", &["orders"])]);
        state.focus_on(Panel::Tables);
        state.apply_search(Searched::Tables, Search::new("o"));
        // Rows, alphabetical: "box" (0), "shop" (1, forced open by its own
        // match), "orders" (2) — every one of the three matches "o".
        state.tables.cursor.go_to(1, tree_rows(&state));

        press(&mut state, Key::Char('n'));
        assert_eq!(
            state.tables.cursor.index(),
            2,
            "n from the middle row is the match after it"
        );

        state.tables.cursor.go_to(1, tree_rows(&state));
        press(&mut state, Key::Char('N'));
        assert_eq!(
            state.tables.cursor.index(),
            0,
            "N from the same row is the match before it, not a synonym for n"
        );
    }

    #[test]
    fn n_is_inert_where_no_search_is_applied() {
        let mut state = loaded();
        state.focus_on(Panel::Tables);
        let before = state.tables.cursor.index();

        press(&mut state, Key::Char('n'));

        assert_eq!(state.tables.cursor.index(), before);
    }

    #[test]
    fn n_types_a_character_in_queries() {
        let mut state = loaded();
        state.focus_on(Panel::Queries);

        press(&mut state, Key::Char('n'));

        assert!(
            state.queries.text().contains('n'),
            "a panel that takes text takes it"
        );
    }

    #[test]
    fn a_live_range_follows_an_n() {
        // Same three-row ring as `shift_n_walks_backwards`: every row
        // matches "o", so `n` from the middle genuinely moves the cursor and
        // an exact-index assertion can tell a real jump from a bare `v`
        // (which the `.ranges().is_empty()` version this replaces could not).
        let mut state = browsing_tables(&[("box", &[]), ("shop", &["orders"])]);
        state.focus_on(Panel::Tables);
        state.apply_search(Searched::Tables, Search::new("o"));
        state.tables.cursor.go_to(1, tree_rows(&state));

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('n'));

        assert_eq!(state.tables.cursor.index(), 2, "n landed on the next match");
        assert_eq!(
            state.tables.selection.ranges(state.tables.cursor.index()),
            Ranges::of(Range::new(1, 2)),
            "the range runs from the anchor n was pressed on to where it landed"
        );
    }

    #[test]
    fn escape_cancels_the_prompt_restoring_the_search_that_was_there() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.apply_search(Searched::Tables, Search::new("orders"));
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('u'));

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("orders"),
            "an abandoned prompt leaves the panel as it was found"
        );
    }

    #[test]
    fn escape_clears_the_focused_panels_search_when_no_overlay_is_up() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(Panel::Contents);
        state.apply_search(Searched::Contents, Search::new("o"));

        press(&mut state, Key::Escape);

        assert_eq!(state.search(Searched::Contents), None);
    }

    #[test]
    fn escape_prefers_the_overlay_to_the_search() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.apply_search(Searched::Tables, Search::new("o"));
        state.overlay = Some(Overlay::Message(Message {
            title: "t".to_owned(),
            message: "m".to_owned(),
            severity: Severity::Info,
        }));

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("o"),
            "one press does one thing"
        );
    }

    #[test]
    fn v_starts_a_range_at_the_cursor_and_motion_extends_it() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.selection.ranges(state.tables.cursor.index()),
            Ranges::of(Range::new(1, 3))
        );
    }

    #[test]
    fn a_second_v_freezes_the_range_and_the_cursor_walks_out_of_it() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), 2);
        assert_eq!(
            state.tables.selection.ranges(state.tables.cursor.index()),
            Ranges::of(Range::new(0, 1)),
            "a frozen range does not follow the cursor"
        );
    }

    #[test]
    fn escape_freezes_a_live_range_before_it_clears_one() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Escape);
        assert_eq!(
            state.tables.selection.ranges(state.tables.cursor.index()),
            Ranges::of(Range::new(0, 1)),
            "the first press leaves visual mode and keeps the range"
        );

        press(&mut state, Key::Escape);
        assert!(!state.tables.selection.is_active(), "the second clears it");
    }

    #[test]
    fn escape_prefers_the_selection_to_the_search() {
        let mut state = browsing(&["alpha", "beta"]);
        state.apply_search(Searched::Tables, Search::new("a"));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Escape);

        press(&mut state, Key::Escape);

        assert!(!state.tables.selection.is_active());
        assert!(
            state.search(Searched::Tables).is_some(),
            "the range came off first; the search is still applied"
        );
    }

    #[test]
    fn v_selects_in_the_focused_panel_only() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(Panel::Ddl);

        press(&mut state, Key::Char('v'));

        assert!(!state.tables.selection.is_active());
        assert!(state.ddl.selection.is_extending());
    }

    #[test]
    fn v_types_a_v_in_queries() {
        let mut state = browsing(&["a"]);
        state.focus_on(Panel::Queries);

        press(&mut state, Key::Char('v'));

        assert_eq!(state.queries.text(), "v");
    }

    #[test]
    fn v_builds_two_groups_and_esc_takes_both() {
        // v j v, walk away, v — the gesture the design opens with (§1).
        let mut state = tables_state();
        state.focus_on(Panel::Tables);

        let _ = press(&mut state, Key::Char('v'));
        let _ = press(&mut state, Key::Char('j'));
        let _ = press(&mut state, Key::Char('v'));
        let _ = press(&mut state, Key::Char('j'));
        let _ = press(&mut state, Key::Char('j'));
        let _ = press(&mut state, Key::Char('v'));
        let _ = press(&mut state, Key::Char('v'));

        let selected = state.tables.selection.ranges(state.tables.cursor.index());
        assert_eq!(selected.groups().count(), 2, "the gap kept them apart");
        assert_eq!(selected.indices().collect::<Vec<_>>(), vec![0, 1, 3]);

        let _ = press(&mut state, Key::Escape);

        assert!(
            !state.tables.selection.is_active(),
            "one esc takes every group"
        );
    }

    #[test]
    fn v_on_a_selected_row_drops_that_group_only() {
        let mut state = tables_state();
        state.focus_on(Panel::Tables);
        state.tables.selection.start(0);
        state.tables.selection.stop(1);
        state.tables.selection.start(3);
        state.tables.selection.stop(3);
        state.tables.cursor.go_to(0, 4);

        let _ = press(&mut state, Key::Char('v'));

        assert_eq!(
            state
                .tables
                .selection
                .ranges(state.tables.cursor.index())
                .indices()
                .collect::<Vec<_>>(),
            vec![3]
        );
    }

    #[test]
    fn a_search_clears_the_range_it_would_renumber() {
        let mut state = browsing(&["alpha", "beta", "gamma"]);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));

        state.apply_search(Searched::Tables, Search::new("a"));

        assert!(!state.tables.selection.is_active());
    }

    #[test]
    fn a_range_over_a_list_that_shrank_is_pulled_back() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::End);
        press(&mut state, Key::Char('v'));
        assert_eq!(
            state.tables.selection.ranges(state.tables.cursor.index()),
            Ranges::of(Range::new(0, 2))
        );

        press(&mut state, Key::Home);
        press(&mut state, Key::Char('h'));

        assert_eq!(
            state.tables.selection.ranges(state.tables.cursor.index()),
            Ranges::of(Range::new(0, 0)),
            "folding the keyspace took the two table rows with it"
        );
    }

    #[test]
    fn escape_leaves_queries_when_nothing_else_claims_it() {
        // Rung 3 alone (§6): no overlay, no search (Queries cannot hold one
        // yet — design §8), so `esc` falls through to leaving the editor.
        // This does not pin rung 2 before rung 3 — that ordering is
        // unreachable, and stays so, until the vim slice gives Queries a
        // search to prefer.
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(Panel::Queries);

        press(&mut state, Key::Escape);

        assert_eq!(state.focus(), Panel::Tables);
    }

    #[test]
    fn tab_is_inert_while_the_prompt_is_up() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));

        press(&mut state, Key::Tab);
        press(&mut state, Key::BackTab);

        assert_eq!(state.focus(), Panel::Tables);
        assert!(searching(&state).is_some(), "and the prompt survives");
    }

    #[test]
    fn a_click_cancels_the_prompt() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        state.apply_search(Searched::Tables, Search::new("orders"));
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('u'));

        point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

        assert!(state.overlay.is_none());
        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("orders"),
            "a click that lands somewhere unexpected must not leave state behind"
        );
    }

    /// The layer is dispatched above every overlay, so a chord that would open
    /// one has to refuse the prompt itself: opening a picker over a live query
    /// would leave the panel searched with nothing holding what it searched
    /// from (docs/shortcuts.md §1).
    #[test]
    fn ctrl_t_is_refused_over_a_live_prompt() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('t'));

        ctrl(&mut state, Key::Char('t'));

        assert_eq!(searching(&state).map(Prompt::query), Some("t"));
    }

    /// The diagnostics overlay is the only place the list it shows lives —
    /// `State.config` does not keep it — so `ctrl-t` opening the theme picker
    /// from under it would destroy those diagnostics for good.
    #[test]
    fn ctrl_t_is_refused_over_diagnostics() {
        let (mut state, _) = init(Config::default(), vec![diagnostic("one")], Stamp::default());

        ctrl(&mut state, Key::Char('t'));

        assert!(
            matches!(state.overlay, Some(Overlay::Diagnostics(_))),
            "the diagnostics overlay must survive an unrelated ctrl chord"
        );
    }

    /// A ctrl chord the layer has no member for is refused as it is anywhere
    /// else, rather than typed into the query.
    #[test]
    fn an_unclaimed_ctrl_chord_is_not_text_in_the_prompt() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));

        ctrl(&mut state, Key::Char('n'));

        assert_eq!(searching(&state).map(Prompt::query), Some(""));
    }

    /// The blank frame design §3.3 accepts, carried to its repair: the offset
    /// a wheel produced from pre-search geometry is out of range for the body
    /// the search left, and the next measurement pulls it back. One frame
    /// wide, and the loop terminates.
    #[test]
    fn the_measurement_after_a_search_pulls_a_stale_ddl_offset_back_inside() {
        let mut state = read_shop_orders();
        state.focus_on(Panel::Ddl);
        sized(&mut state, [8, 4, 8, 1], 40);

        for _ in 0..20 {
            point(
                &mut state,
                Pointer::Wheel {
                    panel: Panel::Ddl,
                    direction: Wheel::Down,
                },
            );
        }
        assert_eq!(
            state.ddl.scroll.offset(),
            36,
            "the wheel bounds itself against the pre-search line count"
        );

        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('t'));

        // A frame of nine wrapped lines in a four-line panel: no offset past
        // five is drawable, whatever the search reported.
        sized(&mut state, [8, 4, 8, 1], 9);

        assert_eq!(state.ddl.scroll.offset(), 5);
    }

    /// The Ddl half of the `esc` chain's second step. Its viewport is followed
    /// by span rather than by index (design §5.1 of the Ddl selection slice),
    /// so clearing a search that moved what the cursor names is repaired on
    /// the measurement that follows, not in `escape` itself.
    #[test]
    fn escape_clearing_a_ddl_search_re_follows_the_selection() {
        let mut state = read_shop_orders();
        state.focus_on(Panel::Ddl);
        sized(&mut state, [8, 4, 8, 1], 40);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('a'));
        press(&mut state, Key::Enter);
        press(&mut state, Key::End);

        press(&mut state, Key::Escape);

        assert!(state.search(Searched::Ddl).is_none());

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 4, 8, 1], 40, 0).with_ddl_selection(Span {
                first: 22,
                lines: 3,
            })),
        );

        assert_eq!(
            state.ddl.scroll.offset(),
            21,
            "lines 21..25 put the whole of the row measured at 22..25 on screen"
        );
    }

    #[test]
    fn a_search_survives_a_focus_change() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('/'));
        press(&mut state, Key::Char('o'));
        press(&mut state, Key::Enter);

        press(&mut state, Key::Tab);
        press(&mut state, Key::Char('t'));

        assert_eq!(
            state.search(Searched::Tables).map(Search::as_str),
            Some("o")
        );
    }

    /// `ctrl-r`, the Queries member of the ctrl layer (docs/shortcuts.md §2).
    fn run_query(state: &mut State) -> Vec<Effect> {
        ctrl(state, Key::Char('r'))
    }

    #[test]
    fn ctrl_r_runs_the_query_buffer() {
        let mut state = editing("SELECT * FROM shop.orders");

        let effects = run_query(&mut state);

        let query = state
            .contents
            .outstanding()
            .expect("the panel is waiting on the query it just ran");
        assert_eq!(
            effects,
            vec![
                Effect::Execute {
                    conn: current_conn(&state),
                    query,
                    stmt: Box::new(Statement::typed("SELECT * FROM shop.orders")),
                },
                Effect::RecordHistory(Box::new(Statement::typed("SELECT * FROM shop.orders"))),
            ]
        );
    }

    // Rejected in update, so no effect was ever issued — the entry is born
    // complete (query log design §3, §3.1).
    #[test]
    fn a_statement_refused_by_the_posture_says_so_in_the_trail() {
        let mut state = editing("DELETE FROM store.orders WHERE id = 7");

        let effects = run_query(&mut state);

        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Execute { .. })),
            "a read-only connection issues nothing"
        );
        let Outcome::Rejected { reason } = state.log.entries().last().unwrap().outcome() else {
            panic!("the statement was rejected");
        };
        assert!(reason.contains("read-only"), "{reason}");
    }

    // The posture guard logs exactly once. Restructuring the logging into
    // `submit_into_contents`'s `Err` arm must not leave `submit` still
    // logging its own posture rejection too (I3).
    #[test]
    fn a_statement_refused_by_the_posture_logs_exactly_one_entry() {
        let mut state = editing("DELETE FROM store.orders WHERE id = 7");
        let before = state.log.len();

        run_query(&mut state);

        assert_eq!(
            state.log.len(),
            before + 1,
            "one refusal, one entry — not one from `submit` and a second from its caller"
        );
    }

    // I3: before `Msg::Connected` lands, a read-only statement is not caught
    // by the mutation guard, so it fell through to `conn_id()` returning
    // `None` — a refusal `submit_into_contents` noticed (`refused` puts
    // "query failed — see [4]-Logs" in Table contents) but never logged. The
    // panel sent the user to a record that did not exist.
    #[test]
    fn a_statement_refused_for_want_of_a_connection_says_so_in_the_trail() {
        let mut state = State::default();
        state.focus_on(Panel::Queries);
        type_into(&mut state, "SELECT * FROM shop.orders");
        let before = state.log.len();

        let effects = run_query(&mut state);

        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::Execute { .. })),
            "there is no connection to run it on"
        );
        assert_eq!(state.log.len(), before + 1, "the refusal is not silent");
        let Outcome::Rejected { reason } = state.log.entries().last().unwrap().outcome() else {
            panic!("the statement was refused for want of a connection");
        };
        assert!(reason.contains("not connected"), "{reason}");
    }

    // M8: the one remaining refusal that left no trace — `run`'s
    // destructive-statement guard raises a modal `esc` destroys on dismissal
    // (query log design §1), and used to log nothing behind it.
    #[test]
    fn a_destructive_statement_refused_by_run_says_so_in_the_trail() {
        let mut state = editing("DROP TABLE shop.orders");
        let ConnectionState::Connected(connection) = &mut state.connection else {
            panic!("the fixture is connected");
        };
        connection.posture = Posture::Writable;

        run_query(&mut state);

        let Outcome::Rejected { reason } = state.log.entries().last().unwrap().outcome() else {
            panic!("the destructive statement was refused");
        };
        assert!(reason.contains("typed confirmation"), "{reason}");
    }

    #[test]
    fn running_the_buffer_lands_focus_on_the_grid_it_filled() {
        let mut state = editing("SELECT * FROM shop.orders");

        run_query(&mut state);

        assert_eq!(
            state.focus(),
            Panel::Contents,
            "a run is a read, and a read lands where the rows do"
        );
    }

    #[test]
    fn a_run_query_titles_the_panel_with_what_was_typed() {
        let mut state = editing("SELECT * FROM shop.orders");
        run_query(&mut state);
        let query = state.contents.outstanding().expect("waiting");

        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(3)),
                took: Duration::ZERO,
            },
        );

        assert_eq!(state.contents.row_count(), 3);
        let ContentsState::Loaded { subject, .. } = state.contents.state() else {
            panic!("the page landed, so the panel is loaded");
        };
        assert_eq!(
            subject.label(),
            "SELECT * FROM shop.orders",
            "the panel says which query it is showing"
        );
    }

    #[test]
    fn ctrl_r_outside_queries_runs_nothing() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let mut state = editing("SELECT * FROM shop.orders");
            state.focus_on(panel);

            assert!(
                run_query(&mut state).is_empty(),
                "the buffer is run from the panel it is typed in, not from {panel:?}"
            );
            assert_eq!(state.contents.outstanding(), None);
        }
    }

    #[test]
    fn an_empty_buffer_runs_nothing() {
        for text in ["", "   \n  \n"] {
            let mut state = editing(text);

            assert!(run_query(&mut state).is_empty());
            assert_eq!(
                state.contents.outstanding(),
                None,
                "an empty buffer is a mistake with no consequence, not an error box"
            );
            assert!(state.overlay.is_none());
        }
    }

    #[test]
    fn running_a_query_cancels_a_pending_table_read() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let read = state.contents.outstanding().expect("the read is in flight");
        state.focus_on(Panel::Queries);
        type_into(&mut state, "SELECT * FROM shop.users");

        let effects = run_query(&mut state);

        assert!(
            effects.contains(&Effect::Cancel(read)),
            "one grid means one outstanding query"
        );
        assert!(!state.in_flight.contains_key(&read));
    }

    #[test]
    fn reading_a_table_cancels_a_pending_query() {
        let mut state = read_shop_orders();
        state.focus_on(Panel::Queries);
        type_into(&mut state, "SELECT * FROM shop.users");
        run_query(&mut state);
        let query = state
            .contents
            .outstanding()
            .expect("the query is in flight");
        state.focus_on(Panel::Tables);

        let effects = press(&mut state, Key::Enter);

        assert!(effects.contains(&Effect::Cancel(query)));
        assert!(!state.in_flight.contains_key(&query));
    }

    #[test]
    fn a_destructive_statement_is_refused_until_confirmation_exists() {
        let mut state = editing("DROP TABLE shop.orders");

        let effects = run_query(&mut state);

        assert!(
            effects.is_empty(),
            "nothing reaches the cluster without the confirmation vision.md §4 principle 6 requires"
        );
        assert_eq!(state.contents.outstanding(), None);
        assert!(
            matches!(
                state.overlay,
                Some(Overlay::Message(Message {
                    severity: Severity::Danger,
                    ..
                }))
            ),
            "the refusal says why"
        );
    }

    #[test]
    fn a_typed_write_is_refused_on_a_read_only_connection() {
        let mut state = editing("INSERT INTO shop.orders (id) VALUES (1)");

        let effects = run_query(&mut state);

        assert!(effects.is_empty());
        assert!(
            matches!(state.contents.state(), ContentsState::Failed { .. }),
            "the posture gate refuses it and the panel says so"
        );
    }

    /// Every mutating verb, run from the buffer against a read-only
    /// connection: nothing reaches the cluster and each one says why. The
    /// list is the classifier's, so a verb added there without a posture
    /// answer fails here (posture design §2).
    #[test]
    fn every_mutating_statement_typed_by_hand_is_refused_and_says_why() {
        for cql in [
            "INSERT INTO shop.orders (id) VALUES (1)",
            "UPDATE shop.orders SET total = 1 WHERE id = 1",
            "DELETE FROM shop.orders WHERE id = 1",
            "BEGIN BATCH INSERT INTO shop.orders (id) VALUES (1); APPLY BATCH",
            "CREATE TABLE shop.new (id int PRIMARY KEY)",
            "CREATE KEYSPACE other WITH replication = {}",
            "ALTER TABLE shop.orders ADD note text",
            "GRANT SELECT ON KEYSPACE shop TO reader",
            "REVOKE MODIFY ON KEYSPACE shop FROM reader",
            "DROP TABLE shop.orders",
            "TRUNCATE shop.orders",
            "-- innocent\nDROP KEYSPACE shop",
            "SELECT * FROM shop.orders; DROP TABLE shop.orders",
        ] {
            let mut state = editing(cql);

            let effects = run_query(&mut state);

            assert!(effects.is_empty(), "{cql} reached the cluster");
            assert!(
                matches!(&state.overlay, Some(Overlay::Message(message))
                    if message.title == "Read-only connection"),
                "{cql} was refused without saying why"
            );
        }
    }

    /// A read is not refused, and a read-only connection is what the tool is
    /// for — the modal must not be in the way of one.
    #[test]
    fn a_read_typed_by_hand_raises_no_refusal() {
        for cql in [
            "SELECT * FROM shop.orders",
            "-- a note\nSELECT * FROM shop.orders",
            "SELECT * FROM shop.orders WHERE note = '; DROP TABLE x'",
        ] {
            let mut state = editing(cql);

            let effects = run_query(&mut state);

            assert!(!effects.is_empty(), "{cql} should have run");
            assert!(state.overlay.is_none(), "{cql} was refused");
        }
    }

    /// The posture names itself rather than deferring to the confirmation
    /// that is not what stands in the way here: a read-only connection has no
    /// `d` to send the user to.
    #[test]
    fn a_destructive_statement_on_a_read_only_connection_blames_the_posture() {
        let mut state = editing("DROP TABLE shop.orders");

        run_query(&mut state);

        let Some(Overlay::Message(message)) = &state.overlay else {
            panic!("refused with a modal");
        };
        assert_eq!(message.title, "Read-only connection");
        assert!(
            !message.message.contains("typed confirmation"),
            "the confirmation is not what refused it: {}",
            message.message
        );
    }

    /// The refusal survives the modal, which `esc` destroys (query log design
    /// §1).
    #[test]
    fn a_statement_the_posture_refuses_says_so_in_the_trail_too() {
        let mut state = editing("DROP TABLE shop.orders");

        run_query(&mut state);

        let Outcome::Rejected { reason } = state.log.entries().last().unwrap().outcome() else {
            panic!("the statement was refused");
        };
        assert!(reason.contains("read-only"), "{reason}");
    }

    #[test]
    fn an_unbound_ctrl_key_does_nothing() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));

        ctrl(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "ctrl-j is a binding of its own that does not exist yet, not a slower j"
        );
    }

    #[test]
    fn an_alt_modified_key_is_not_a_binding() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));

        press_binding(
            &mut state,
            KeyPress {
                key: Key::Char('j'),
                modifiers: Modifiers {
                    alt: true,
                    ..Modifiers::NONE
                },
            },
        );

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "alt-j is refused rather than reinterpreted as a slower j"
        );
    }

    #[test]
    fn ctrl_alt_r_does_not_run_the_buffer() {
        let mut state = editing("SELECT * FROM shop.orders");

        let effects = press_binding(
            &mut state,
            KeyPress {
                key: Key::Char('r'),
                modifiers: Modifiers {
                    ctrl: true,
                    alt: true,
                    shift: false,
                },
            },
        );

        assert!(
            effects.is_empty(),
            "two modifiers are a third layer, and no layer is designed"
        );
        assert_eq!(state.contents.outstanding(), None);
    }

    #[test]
    fn an_open_palette_swallows_ctrl_r() {
        let mut state = editing("SELECT * FROM shop.orders");
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));

        let effects = run_query(&mut state);

        assert!(
            effects.is_empty(),
            "an open palette is what the user is looking at, and it claims every key"
        );
        assert_eq!(state.contents.outstanding(), None);
    }

    #[test]
    fn m_cycles_the_contents_selection_mode() {
        let mut state = populated(Panel::Contents);

        press(&mut state, Key::Char('m'));
        assert_eq!(state.contents.mode, SelectionMode::Column);

        press(&mut state, Key::Char('m'));
        assert_eq!(state.contents.mode, SelectionMode::Cell);

        press(&mut state, Key::Char('m'));
        assert_eq!(state.contents.mode, SelectionMode::Row);
    }

    /// The grid is the only body with two cursors to choose between. `m` is
    /// not a panel number, so in the other panels it must fall through to the
    /// arm that answers those and do nothing.
    #[test]
    fn m_does_nothing_outside_the_grid() {
        for panel in [Panel::Tables, Panel::Ddl] {
            let mut state = populated(panel);
            let before = crate::view::view_model(&state);

            let effects = press(&mut state, Key::Char('m'));

            assert!(
                effects.is_empty() && crate::view::view_model(&state) == before,
                "{panel:?} has no grid for a mode to apply to"
            );
            assert_eq!(state.contents.mode, SelectionMode::Row);
        }
    }

    #[test]
    fn m_types_an_m_in_queries() {
        let mut state = populated(Panel::Queries);
        let before = state.queries.text();

        press(&mut state, Key::Char('m'));

        assert_eq!(state.queries.text(), format!("{before}m"));
        assert_eq!(state.contents.mode, SelectionMode::Row);
    }

    #[test]
    fn v_in_row_mode_extends_rows_and_leaves_the_columns_alone() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Row;

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.contents.rows.ranges(state.contents.cursor.index()),
            Ranges::of(Range::new(0, 1))
        );
        assert!(!state.contents.columns.is_active());
    }

    #[test]
    fn v_in_column_mode_extends_columns_and_leaves_the_rows_alone() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Column;

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('l'));

        assert_eq!(
            state.contents.columns.ranges(state.contents.column.index()),
            Ranges::of(Range::new(0, 1))
        );
        assert!(!state.contents.rows.is_active());
    }

    #[test]
    fn v_in_cell_mode_extends_a_rectangle() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('l'));

        assert_eq!(
            state.contents.rows.ranges(state.contents.cursor.index()),
            Ranges::of(Range::new(0, 1)),
            "the row anchor was set before the column moved off zero"
        );
        assert_eq!(
            state.contents.columns.ranges(state.contents.column.index()),
            Ranges::of(Range::new(1, 2)),
            "the column anchor was set after `l`, not swapped with the row's"
        );
    }

    #[test]
    fn m_clears_the_range_it_would_reinterpret() {
        let mut state = grid_of_named_cells(3, 4);
        state.contents.mode = SelectionMode::Cell;
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('v'));

        press(&mut state, Key::Char('m'));

        assert!(!state.contents.rows.is_active());
        assert!(!state.contents.columns.is_active());
        assert_eq!(
            state.contents.mode,
            SelectionMode::Row,
            "the mode still cycles"
        );
    }

    #[test]
    fn escape_unwinds_both_axes_together() {
        let mut state = grid_of_named_cells(3, 4);
        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Escape);
        assert!(!state.contents.rows.is_extending());
        assert!(state.contents.rows.is_active());

        press(&mut state, Key::Escape);
        assert!(!state.contents.rows.is_active());
        assert!(!state.contents.columns.is_active());
    }

    fn contents_showing(rows: usize) -> State {
        let mut state = connected(Posture::ReadOnly);
        state.geometry = Geometry::default().with_export_fits(true);
        state.focus_on(Panel::Contents);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            page_of(rows),
            crate::config::MaxColumnWidth::default(),
        );
        state
    }

    fn contents_showing_typed(cql: &str) -> State {
        let mut state = connected(Posture::ReadOnly);
        state.geometry = Geometry::default().with_export_fits(true);
        state.focus_on(Panel::Contents);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Typed(cql.to_owned()));
        state.contents.page_ready(
            query,
            page_of(ROW_LIMIT),
            crate::config::MaxColumnWidth::default(),
        );
        state
    }

    fn open_form(state: &mut State) {
        handle_key(state, KeyPress::ctrl(Key::Char('e')));
    }

    fn form_of(state: &State) -> &crate::export::ExportForm {
        match state.overlay.as_ref() {
            Some(Overlay::Export(form)) => form,
            _ => panic!("the export form is up"),
        }
    }

    /// The form's bar changes with the stop, so each stop is swept.
    #[test]
    fn every_live_hint_the_export_forms_bar_shows_does_something() {
        for tabs in 0..4 {
            let seated = || {
                let mut state = contents_showing(ROW_LIMIT);
                open_form(&mut state);
                for _ in 0..tabs {
                    handle_key(&mut state, KeyPress::new(Key::Tab));
                }
                state
            };
            let bar = crate::view::hints::bar(&seated());
            let stop = form_of(&seated()).stop();

            for hint in bar.panel.iter().chain(&bar.global) {
                assert_eq!(hint.liveness, crate::view::Liveness::Live);

                for key in hint.keys.split('/') {
                    let key = match key {
                        "tab" => Key::Tab,
                        "space" => Key::Char(' '),
                        "⏎" => Key::Enter,
                        "esc" => Key::Escape,
                        other => panic!("{stop:?} names {other}, which this sweep cannot press"),
                    };
                    let mut state = seated();
                    let before = crate::view::view_model(&state);

                    let effects = press(&mut state, key);

                    assert!(
                        crate::view::view_model(&state) != before || !effects.is_empty(),
                        "{stop:?} offers '{key:?} {}' and pressing it changes nothing",
                        hint.action
                    );
                }
            }
        }
    }

    #[test]
    fn the_export_forms_bar_follows_the_stop() {
        let mut state = contents_showing(ROW_LIMIT);
        open_form(&mut state);
        let named = |state: &State| {
            let bar = crate::view::hints::bar(state);
            bar.panel
                .iter()
                .chain(&bar.global)
                .map(|hint| format!("{} {}", hint.keys, hint.action))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            named(&state),
            ["tab next option", "space toggle", "esc close"]
        );
        press(&mut state, Key::Tab);
        assert_eq!(
            named(&state),
            ["tab next option", "space toggle", "esc close"]
        );
        press(&mut state, Key::Tab);
        assert_eq!(
            named(&state),
            ["tab next option", "esc close"],
            "space is text on the path"
        );
        press(&mut state, Key::Tab);
        assert_eq!(
            named(&state),
            ["tab next option", "space/⏎ export", "esc close"]
        );
    }

    #[test]
    fn ctrl_e_over_a_loaded_contents_panel_opens_the_export_form() {
        let mut state = contents_showing(5);

        open_form(&mut state);

        assert_eq!(
            crate::style::text(&form_of(&state).title()),
            "Export table shop.orders"
        );
    }

    #[test]
    fn ctrl_e_does_nothing_over_an_empty_contents_panel() {
        let mut state = contents_showing(0);

        open_form(&mut state);

        assert!(state.overlay.is_none());
    }

    /// A form nothing draws would still claim every key: `q` would not quit,
    /// `Tab` would not switch panel, and the path stop would swallow letters
    /// into an invisible field.
    #[test]
    fn ctrl_e_is_inert_where_the_frame_cannot_draw_the_form() {
        let mut state = contents_showing(5);
        state.geometry = Geometry::default().with_export_fits(false);

        open_form(&mut state);

        assert!(state.overlay.is_none());
    }

    /// The band between the program's too-small threshold and the form's own
    /// size is where the panels draw and the form does not — a palette-sized
    /// gate would leave it open there, so the form is measured on its own.
    #[test]
    fn a_frame_that_shrank_past_the_form_closes_it() {
        let mut state = contents_showing(5);
        open_form(&mut state);
        assert!(matches!(state.overlay, Some(Overlay::Export(_))));

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 14).with_export_fits(false)),
        );

        assert!(
            state.overlay.is_none(),
            "the palette still fits, and the form does not"
        );
    }

    /// The chord keeps its Queries meaning; the two are gated on focus, the
    /// way ctrl-r already is (design §1).
    #[test]
    fn ctrl_e_in_queries_still_opens_the_external_editor() {
        let mut state = connected(Posture::ReadOnly);
        state.focus_on(Panel::Queries);

        let effects = handle_key(&mut state, KeyPress::ctrl(Key::Char('e')));

        assert!(matches!(effects.as_slice(), [Effect::OpenEditor { .. }]));
        assert!(state.overlay.is_none());
    }

    #[test]
    fn the_form_records_how_much_of_the_result_is_in_memory() {
        let mut complete = contents_showing(7);
        open_form(&mut complete);
        assert_eq!(
            form_of(&complete).fetched(),
            crate::export::Fetched::Everything(7)
        );

        let mut capped = contents_showing(ROW_LIMIT);
        open_form(&mut capped);
        assert_eq!(
            form_of(&capped).fetched(),
            crate::export::Fetched::Some(ROW_LIMIT)
        );
    }

    #[test]
    fn tab_moves_between_the_forms_stops_rather_than_between_panels() {
        let mut state = contents_showing(5);
        open_form(&mut state);

        handle_key(&mut state, KeyPress::new(Key::Tab));

        assert_eq!(form_of(&state).stop(), crate::export::Stop::Format);
        assert_eq!(state.focus(), Panel::Contents, "focus did not move");
    }

    #[test]
    fn space_toggles_the_focused_radio() {
        let mut state = contents_showing(ROW_LIMIT);
        open_form(&mut state);

        handle_key(&mut state, KeyPress::new(Key::Char(' ')));

        assert_eq!(form_of(&state).scope(), crate::export::Scope::Fetched);
    }

    #[test]
    fn a_printable_key_types_itself_only_on_the_path_stop() {
        let mut state = contents_showing(5);
        open_form(&mut state);
        handle_key(&mut state, KeyPress::new(Key::Char('x')));
        assert_eq!(
            form_of(&state).scope(),
            crate::export::Scope::Whole,
            "an unclaimed key on a radio does nothing"
        );

        handle_key(&mut state, KeyPress::new(Key::Tab));
        handle_key(&mut state, KeyPress::new(Key::Tab));
        handle_key(&mut state, KeyPress::new(Key::Char('x')));

        assert!(form_of(&state).path().text().ends_with('x'));
    }

    #[test]
    fn space_on_the_path_stop_types_a_space() {
        let mut state = contents_showing(5);
        open_form(&mut state);
        handle_key(&mut state, KeyPress::new(Key::Tab));
        handle_key(&mut state, KeyPress::new(Key::Tab));

        handle_key(&mut state, KeyPress::new(Key::Char(' ')));

        assert!(form_of(&state).path().text().ends_with(' '));
    }

    /// `Space` is the form's activation key (design §2). `Enter` reaches the
    /// Export button because a focused button is what it means everywhere
    /// else — but a radio group is not a button, and a keystroke there must
    /// not write a file.
    #[test]
    fn enter_on_a_radio_group_does_not_run_the_export() {
        let mut state = contents_showing(ROW_LIMIT);
        open_form(&mut state);

        let effects = handle_key(&mut state, KeyPress::new(Key::Enter));

        assert!(effects.is_empty());
        assert!(
            matches!(state.overlay, Some(Overlay::Export(_))),
            "the form is still up"
        );
    }

    #[test]
    fn enter_on_the_export_stop_runs_it() {
        let mut state = contents_showing(ROW_LIMIT);
        open_form(&mut state);
        for _ in 0..3 {
            handle_key(&mut state, KeyPress::new(Key::Tab));
        }

        let effects = handle_key(&mut state, KeyPress::new(Key::Enter));

        assert!(matches!(effects.as_slice(), [Effect::Export { .. }]));
    }

    #[test]
    fn q_does_not_quit_while_the_form_is_up() {
        let mut state = contents_showing(5);
        open_form(&mut state);

        handle_key(&mut state, KeyPress::new(Key::Char('q')));

        assert!(
            !state.should_quit,
            "the overlay claims every unmodified key"
        );
    }

    #[test]
    fn esc_closes_the_form_and_writes_nothing() {
        let mut state = contents_showing(5);
        open_form(&mut state);

        let effects = handle_key(&mut state, KeyPress::new(Key::Escape));

        assert!(state.overlay.is_none());
        assert!(effects.is_empty());
    }

    #[test]
    fn the_theme_picker_cannot_replace_an_open_export_form() {
        let mut state = contents_showing(5);
        // Room for the picker, so what refuses the chord is the open form and
        // not the picker's own size gate.
        state.geometry = state.geometry.with_theme_rows(20);
        open_form(&mut state);

        handle_key(&mut state, KeyPress::ctrl(Key::Char('t')));

        assert!(
            matches!(state.overlay, Some(Overlay::Export(_))),
            "a chord must not discard the form and the path typed into it"
        );
    }

    #[test]
    fn a_second_ctrl_e_leaves_the_open_form_as_it_was() {
        let mut state = contents_showing(5);
        open_form(&mut state);
        handle_key(&mut state, KeyPress::new(Key::Tab));
        handle_key(&mut state, KeyPress::new(Key::Tab));
        handle_key(&mut state, KeyPress::new(Key::Char('x')));
        let typed = form_of(&state).path().text().to_owned();

        handle_key(&mut state, KeyPress::ctrl(Key::Char('e')));

        assert_eq!(form_of(&state).path().text(), typed);
        assert_eq!(form_of(&state).stop(), crate::export::Stop::Path);
    }

    fn started(state: &mut State) -> Vec<Effect> {
        open_form(state);
        // Scope, Format, Path, Export.
        for _ in 0..3 {
            handle_key(state, KeyPress::new(Key::Tab));
        }
        handle_key(state, KeyPress::new(Key::Char(' ')))
    }

    /// A typed statement, exported to `destination`, mid-flight.
    fn export_in_flight(cql: &str, destination: &str) -> State {
        let mut state = contents_showing_typed(cql);
        open_form(&mut state);
        if let Some(Overlay::Export(form)) = &mut state.overlay {
            *form.path_mut() = crate::export::Line::new(destination.to_owned());
        }
        // Scope, Format, Path, Export.
        for _ in 0..3 {
            handle_key(&mut state, KeyPress::new(Key::Tab));
        }
        handle_key(&mut state, KeyPress::new(Key::Char(' ')));
        state
    }

    /// The rows already in the grid, exported to `destination`, mid-flight —
    /// `export_in_flight`'s `Source::Fetched` counterpart.
    fn export_fetched_in_flight(destination: &str) -> State {
        let mut state = contents_showing(ROW_LIMIT);
        open_form(&mut state);
        handle_key(&mut state, KeyPress::new(Key::Char(' '))); // Scope: Whole -> Fetched.
        if let Some(Overlay::Export(form)) = &mut state.overlay {
            *form.path_mut() = crate::export::Line::new(destination.to_owned());
        }
        // Format, Path, Export.
        for _ in 0..3 {
            handle_key(&mut state, KeyPress::new(Key::Tab));
        }
        handle_key(&mut state, KeyPress::new(Key::Char(' ')));
        state
    }

    #[test]
    fn exporting_the_whole_table_asks_for_it_without_a_limit() {
        let mut state = contents_showing(ROW_LIMIT);

        let effects = started(&mut state);

        let [
            Effect::Export {
                source,
                format,
                destination,
                overwrite,
                ..
            },
        ] = effects.as_slice()
        else {
            panic!("one export effect");
        };
        assert_eq!(
            *source,
            Source::Whole(Box::new(Statement::typed(
                "SELECT * FROM \"shop\".\"orders\""
            )))
        );
        assert_eq!(*format, crate::export::Format::Csv);
        assert!(destination.ends_with("shop.orders.csv"));
        assert!(!overwrite, "the first attempt never clobbers");
    }

    /// The user's own LIMIT is theirs; only ours is dropped (design §3).
    #[test]
    fn exporting_a_typed_query_re_runs_it_verbatim() {
        let mut state = contents_showing_typed("SELECT * FROM shop.orders LIMIT 10");

        let effects = started(&mut state);

        let [Effect::Export { source, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        assert_eq!(
            *source,
            Source::Whole(Box::new(Statement::typed(
                "SELECT * FROM shop.orders LIMIT 10"
            )))
        );
    }

    #[test]
    fn exporting_the_fetched_rows_asks_the_cluster_nothing() {
        let mut state = contents_showing(ROW_LIMIT);
        open_form(&mut state);
        handle_key(&mut state, KeyPress::new(Key::Char(' ')));
        for _ in 0..3 {
            handle_key(&mut state, KeyPress::new(Key::Tab));
        }

        let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

        let [
            Effect::Export {
                source: Source::Fetched { columns, rows },
                ..
            },
        ] = effects.as_slice()
        else {
            panic!("one export effect carrying the rows");
        };
        assert_eq!(rows.len(), ROW_LIMIT);
        assert_eq!(columns, &["id".to_owned()]);
    }

    #[test]
    fn starting_an_export_closes_the_form_and_records_the_job() {
        let mut state = contents_showing(ROW_LIMIT);

        let effects = started(&mut state);

        assert!(state.overlay.is_none(), "the form is done with");
        let [Effect::Export { job, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        assert_eq!(state.export.as_ref().map(ExportJob::id), Some(*job));
        assert_eq!(state.export.as_ref().map(ExportJob::rows), Some(0));
    }

    #[test]
    fn progress_is_the_running_total() {
        let mut state = contents_showing(ROW_LIMIT);
        let effects = started(&mut state);
        let [Effect::Export { job, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };

        update(
            &mut state,
            Msg::ExportProgress {
                job: *job,
                rows: 5_000,
            },
        );

        assert_eq!(state.export.as_ref().map(ExportJob::rows), Some(5_000));
    }

    #[test]
    fn progress_for_a_job_that_is_not_running_is_dropped() {
        let mut state = contents_showing(ROW_LIMIT);
        started(&mut state);

        update(
            &mut state,
            Msg::ExportProgress {
                job: JobId::next(),
                rows: 9,
            },
        );

        assert_eq!(state.export.as_ref().map(ExportJob::rows), Some(0));
    }

    #[test]
    fn finishing_clears_the_job_and_says_what_was_written() {
        let mut state = contents_showing(ROW_LIMIT);
        let effects = started(&mut state);
        let [Effect::Export { job, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };

        update(
            &mut state,
            Msg::ExportFinished {
                job: *job,
                result: Ok(41_203),
                took: Duration::ZERO,
            },
        );

        assert!(state.export.is_none());
        let Some(Overlay::Message(message)) = &state.overlay else {
            panic!("the result is on screen");
        };
        assert_eq!(message.severity, Severity::Info);
        assert!(message.message.contains("41203"));
        assert!(message.message.contains("shop.orders.csv"));
    }

    #[test]
    fn a_finished_export_records_the_exact_statement_and_where_it_went() {
        let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
        let job = state.export.as_ref().expect("a job").id();

        let _effects = update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Ok(18_420),
                took: Duration::from_millis(1_400),
            },
        );

        let entry = state
            .log
            .entries()
            .find(|entry| entry.subject().text().starts_with("SELECT"))
            .expect("the export entry");
        assert!(
            !entry.subject().text().contains("LIMIT"),
            "an export pages the unbounded statement (export design §6.2)"
        );
        let Outcome::Succeeded { detail, .. } = entry.outcome() else {
            panic!("the export succeeded");
        };
        assert!(detail.contains("18420"), "{detail}");
        assert!(detail.contains("/tmp/orders.csv"), "{detail}");
    }

    /// `ExportError::Exists` puts the form back for a retry rather than
    /// ending the job, so the entry stays pending while the question is up —
    /// and the retry reuses the refused attempt's own `JobId` rather than
    /// minting a fresh one, so it is this same entry the retry's outcome
    /// closes, not a second one left stranded at `⋯ running` (final review
    /// I4).
    #[test]
    fn an_export_that_exists_stays_pending_through_the_retry_and_completes_with_it() {
        let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
        let job = state.export.as_ref().expect("a job").id();

        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        let waiting = state
            .log
            .entries()
            .find(|entry| entry.subject().text().starts_with("SELECT"))
            .expect("the export entry");
        assert!(
            matches!(waiting.outcome(), Outcome::Pending { trace: Trace::Export(id) } if *id == job),
            "still waiting, with the confirmation up"
        );

        let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));
        let [Effect::Export { job: retried, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        assert_eq!(*retried, job, "the retry reuses the refused attempt's id");

        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Ok(18_420),
                took: Duration::from_millis(1_400),
            },
        );

        let matching: Vec<_> = state
            .log
            .entries()
            .filter(|entry| entry.subject().text().starts_with("SELECT"))
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "one entry for the whole export, not one per attempt"
        );
        let Outcome::Succeeded { detail, .. } = matching[0].outcome() else {
            panic!("the retry succeeded");
        };
        assert!(detail.contains("18420"), "{detail}");
    }

    /// Nothing else will ever complete a trace whose question was abandoned:
    /// the source it was asking about is dropped with the confirmation, so
    /// the entry has to be closed right there rather than left `⋯ running`
    /// for a retry that is now never coming (final review I4).
    #[test]
    fn abandoning_the_overwrite_confirmation_cancels_the_pending_entry() {
        let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
        let job = state.export.as_ref().expect("a job").id();
        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        handle_key(&mut state, KeyPress::new(Key::Escape));

        let entry = state
            .log
            .entries()
            .find(|entry| entry.subject().text().starts_with("SELECT"))
            .expect("the export entry");
        assert_eq!(*entry.outcome(), Outcome::Cancelled);
    }

    /// A click is modal-dismissing (`handle_pointer` routes it to `escape`)
    /// and reaches `Overlay::Export`'s arm there, unlike keyboard `Esc`,
    /// which `export_key` claims first — the path re-review found still
    /// dropped the confirmation's `Pending` on the floor (final review, I4
    /// follow-up).
    #[test]
    fn a_click_dismissing_the_overwrite_confirmation_cancels_the_pending_entry() {
        let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
        let job = state.export.as_ref().expect("a job").id();
        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

        assert!(
            state.overlay.is_none(),
            "the click spends itself dismissing it"
        );
        let entry = state
            .log
            .entries()
            .find(|entry| entry.subject().text().starts_with("SELECT"))
            .expect("the export entry");
        assert_eq!(*entry.outcome(), Outcome::Cancelled);
    }

    /// A resize past the form's own threshold closes it outright
    /// (`a_frame_that_shrank_past_the_form_closes_it` covers the plain
    /// form); this is the same close while a confirmation is up, which the
    /// re-review found left the trace pending forever (final review, I4
    /// follow-up).
    #[test]
    fn a_resize_that_closes_the_overwrite_confirmation_cancels_the_pending_entry() {
        let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
        let job = state.export.as_ref().expect("a job").id();
        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 14).with_export_fits(false)),
        );

        assert!(state.overlay.is_none(), "the form still does not fit");
        let entry = state
            .log
            .entries()
            .find(|entry| entry.subject().text().starts_with("SELECT"))
            .expect("the export entry");
        assert_eq!(*entry.outcome(), Outcome::Cancelled);
    }

    /// A `Fetched` export issues no statement, but it is still something the
    /// session ran (design §8's `ExportFinished(Err)` row names no scope
    /// qualifier) — it gets an entry too, a sentence rather than a quoted
    /// query that never ran.
    #[test]
    fn a_fetched_export_lands_a_pending_entry_when_it_starts() {
        let state = export_fetched_in_flight("/tmp/orders.csv");

        let entry = state.log.entries().last().expect("an entry per effect");
        assert!(
            matches!(entry.subject(), LogSubject::Sentence(_)),
            "no statement ran, so the subject is not one"
        );
        assert!(
            matches!(
                entry.outcome(),
                Outcome::Pending {
                    trace: Trace::Export(_)
                }
            ),
            "waiting on the same trace a Whole export waits on"
        );
    }

    #[test]
    fn a_finished_fetched_export_completes_its_entry() {
        let mut state = export_fetched_in_flight("/tmp/orders.csv");
        let job = state.export.as_ref().expect("a job").id();

        let _effects = update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Ok(200),
                took: Duration::from_millis(50),
            },
        );

        let entry = state
            .log
            .entries()
            .find(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
            .expect("the fetched export's entry");
        let Outcome::Succeeded { detail, .. } = entry.outcome() else {
            panic!("the export succeeded");
        };
        assert!(detail.contains("200"), "{detail}");
        assert!(detail.contains("/tmp/orders.csv"), "{detail}");
    }

    #[test]
    fn a_failed_fetched_export_completes_its_entry_as_failed() {
        let mut state = export_fetched_in_flight("/tmp/orders.csv");
        let job = state.export.as_ref().expect("a job").id();

        let _effects = update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Io("disk full".to_owned())),
                took: Duration::ZERO,
            },
        );

        let entry = state
            .log
            .entries()
            .find(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
            .expect("the fetched export's entry");
        let Outcome::Failed { detail } = entry.outcome() else {
            panic!("the export failed");
        };
        assert!(detail.contains("disk full"), "{detail}");
    }

    /// Decision #4 holds for a `Fetched` export the same as it does for a
    /// `Whole` one: `Exists` completes nothing while the question is up, and
    /// the retry — reusing the same `JobId` — is what closes this entry
    /// rather than orphaning it (final review I4).
    #[test]
    fn a_fetched_export_that_exists_stays_pending_through_the_retry_and_completes_with_it() {
        let mut state = export_fetched_in_flight("/tmp/orders.csv");
        let job = state.export.as_ref().expect("a job").id();

        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        let waiting = state
            .log
            .entries()
            .find(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
            .expect("the fetched export's entry");
        assert!(
            matches!(waiting.outcome(), Outcome::Pending { trace: Trace::Export(id) } if *id == job),
            "still waiting, with the confirmation up"
        );

        let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));
        let [Effect::Export { job: retried, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        assert_eq!(*retried, job, "the retry reuses the refused attempt's id");

        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Ok(200),
                took: Duration::from_millis(50),
            },
        );

        let matching: Vec<_> = state
            .log
            .entries()
            .filter(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "one entry for the whole export, not one per attempt"
        );
        let Outcome::Succeeded { detail, .. } = matching[0].outcome() else {
            panic!("the retry succeeded");
        };
        assert!(detail.contains("200"), "{detail}");
    }

    /// The abandon path holds for a `Fetched` export the same as a `Whole`
    /// one (final review I4).
    #[test]
    fn abandoning_a_fetched_exports_overwrite_confirmation_cancels_the_pending_entry() {
        let mut state = export_fetched_in_flight("/tmp/orders.csv");
        let job = state.export.as_ref().expect("a job").id();
        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        handle_key(&mut state, KeyPress::new(Key::Escape));

        let entry = state
            .log
            .entries()
            .find(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
            .expect("the fetched export's entry");
        assert_eq!(*entry.outcome(), Outcome::Cancelled);
    }

    #[test]
    fn a_failure_is_reported_as_a_danger() {
        let mut state = contents_showing(ROW_LIMIT);
        let effects = started(&mut state);
        let [Effect::Export { job, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };

        update(
            &mut state,
            Msg::ExportFinished {
                job: *job,
                result: Err(ExportError::Io("disk full".to_owned())),
                took: Duration::ZERO,
            },
        );

        assert!(state.export.is_none());
        let Some(Overlay::Message(message)) = &state.overlay else {
            panic!("the failure is on screen");
        };
        assert_eq!(message.severity, Severity::Danger);
    }

    /// Not a message: the user is meant to answer it, and the form has to be
    /// there to go back to (design §7).
    #[test]
    fn an_existing_file_reopens_the_form_asking_to_overwrite() {
        let mut state = contents_showing(ROW_LIMIT);
        let effects = started(&mut state);
        let [Effect::Export { job, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };

        update(
            &mut state,
            Msg::ExportFinished {
                job: *job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        assert!(state.export.is_none(), "nothing is running");
        assert_eq!(form_of(&state).confirm(), Some(Confirm::Overwrite));
    }

    /// The overlay is `None` for the whole of a running export, so `ctrl-t` is
    /// not refused and the picker can be up and previewing when the answer
    /// lands. Replacing it outright would leave the previewed theme in force
    /// with `restored` — the only copy of the user's own — dropped with the
    /// picker (final review I2).
    #[test]
    fn an_existing_file_landing_on_an_open_picker_puts_the_theme_back() {
        let mut state = contents_showing(ROW_LIMIT);
        // Room for the picker as well as the form.
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14)
            .with_export_fits(true)
            .with_theme_rows(20);
        let effects = started(&mut state);
        let [Effect::Export { job, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        let job = *job;
        let chosen = state.theme.clone();

        handle_key(&mut state, KeyPress::ctrl(Key::Char('t')));
        handle_key(&mut state, KeyPress::new(Key::Char('j')));
        assert_ne!(state.theme, chosen, "the picker is previewing another");

        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        assert_eq!(state.theme, chosen);
        assert_eq!(form_of(&state).confirm(), Some(Confirm::Overwrite));
    }

    #[test]
    fn confirming_re_emits_the_export_with_overwrite_set() {
        let mut state = contents_showing(ROW_LIMIT);
        let effects = started(&mut state);
        let [Effect::Export { job, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        update(
            &mut state,
            Msg::ExportFinished {
                job: *job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

        let [Effect::Export { overwrite, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        assert!(overwrite);
        assert!(state.overlay.is_none());
    }

    /// The overlay is `None` and the panel is live for the whole of an export,
    /// so the user can step to another table and read it before `Exists` comes
    /// back. What the confirmation re-emits is the export that was attempted,
    /// not whatever the panel holds by then (final review I3).
    #[test]
    fn confirming_re_emits_what_was_attempted_not_what_the_panel_now_holds() {
        let mut state = contents_showing(ROW_LIMIT);
        open_form(&mut state);
        // Scope: Whole -> Fetched, so the rows themselves ride on the effect.
        handle_key(&mut state, KeyPress::new(Key::Char(' ')));
        for _ in 0..3 {
            handle_key(&mut state, KeyPress::new(Key::Tab));
        }
        let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));
        let [Effect::Export { job, source, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        let (job, attempted) = (*job, source.clone());

        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "audit_log")));
        state.contents.page_ready(
            query,
            Page {
                columns: vec!["action".to_owned()],
                ..page_of(3)
            },
            crate::config::MaxColumnWidth::default(),
        );

        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );
        let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

        let [
            Effect::Export {
                source, overwrite, ..
            },
        ] = effects.as_slice()
        else {
            panic!("one export effect");
        };
        assert!(overwrite);
        assert_eq!(*source, attempted, "shop.orders' rows, not audit_log's");
    }

    /// The panel need not be showing a page at all by then — `source` would
    /// hand back `None` and `Space` would be a silent no-op.
    #[test]
    fn confirming_still_exports_when_the_panel_has_moved_on_entirely() {
        let mut state = contents_showing(ROW_LIMIT);
        let effects = started(&mut state);
        let [Effect::Export { job, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        let job = *job;

        state
            .contents
            .requested(QueryId::next(), Subject::Typed("SELECT 1".to_owned()));

        update(
            &mut state,
            Msg::ExportFinished {
                job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );
        let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

        assert!(matches!(effects.as_slice(), [Effect::Export { .. }]));
    }

    #[test]
    fn esc_from_the_confirmation_goes_back_to_the_form_intact() {
        let mut state = contents_showing(ROW_LIMIT);
        let effects = started(&mut state);
        let [Effect::Export { job, .. }] = effects.as_slice() else {
            panic!("one export effect");
        };
        update(
            &mut state,
            Msg::ExportFinished {
                job: *job,
                result: Err(ExportError::Exists),
                took: Duration::ZERO,
            },
        );

        handle_key(&mut state, KeyPress::new(Key::Escape));

        assert_eq!(form_of(&state).confirm(), None, "back to the form");
        assert_eq!(
            form_of(&state).stop(),
            crate::export::Stop::Export,
            "with every field as it was"
        );
    }

    #[test]
    fn ctrl_e_is_inert_while_an_export_is_already_running() {
        let mut state = contents_showing(5);
        started(&mut state);
        state.overlay = None;

        open_form(&mut state);

        assert!(state.overlay.is_none());
    }

    /// The query editor slice: formatting, highlighting and completion
    /// (docs/superpowers/specs/2026-08-16-query-editor-design.md).
    mod editor {
        use super::*;

        fn ctrl(character: char) -> KeyPress {
            KeyPress::ctrl(Key::Char(character))
        }

        fn offered(state: &State) -> Vec<String> {
            state
                .queries
                .completion()
                .map(|completion| {
                    completion
                        .candidates()
                        .iter()
                        .map(|candidate| candidate.text.clone())
                        .collect()
                })
                .unwrap_or_default()
        }

        #[test]
        fn ctrl_f_lays_the_buffer_out() {
            let mut state = editing("select * from shop.orders where id = 7");

            press_binding(&mut state, ctrl('f'));

            assert_eq!(
                state.queries.text(),
                "SELECT *\nFROM shop.orders\nWHERE id = 7"
            );
        }

        #[test]
        fn ctrl_f_does_nothing_outside_queries() {
            let mut state = editing("select * from shop.orders");
            state.focus_on(Panel::Tables);

            press_binding(&mut state, ctrl('f'));

            assert_eq!(
                state.queries.text(),
                "select * from shop.orders",
                "the chord is the Queries member of the layer and inert elsewhere"
            );
        }

        #[test]
        fn typing_a_prefix_offers_the_keyword_it_starts() {
            let mut state = editing("se");

            assert_eq!(offered(&state).first().map(String::as_str), Some("SELECT"));

            press(&mut state, Key::Char('l'));
            assert_eq!(offered(&state).first().map(String::as_str), Some("SELECT"));
        }

        #[test]
        fn a_qualified_prefix_offers_that_keyspaces_tables() {
            let state = editing("SELECT * FROM shop.o");

            assert_eq!(offered(&state), vec!["orders".to_owned()]);
        }

        #[test]
        fn an_unqualified_table_comes_from_the_keyspace_the_tree_is_on() {
            // `browsing_tables` leaves the Tables cursor on `shop`, which is
            // what makes a bare `orders` completable at all (design §5.2).
            // A `FROM` clause wants a table, so the name the tree is
            // standing on outranks a keyword that happens to share its
            // prefix (design §6).
            let state = editing("SELECT * FROM ord");

            assert_eq!(
                offered(&state),
                vec!["orders".to_owned(), "ORDER".to_owned()],
                "a table position wants a name before it wants grammar"
            );
        }

        #[test]
        fn tab_accepts_what_is_offered_rather_than_moving_focus() {
            let mut state = editing("SELECT * FROM shop.o");

            press(&mut state, Key::Tab);

            assert_eq!(state.queries.text(), "SELECT * FROM shop.orders");
            assert_eq!(state.focus(), Panel::Queries, "focus stayed put");
        }

        #[test]
        fn tab_still_walks_the_cycle_when_nothing_is_being_offered() {
            let mut state = editing("SELECT * FROM shop.orders");

            press(&mut state, Key::Tab);

            assert_eq!(
                state.focus(),
                Panel::Queries,
                "Queries' own second stop, not yet the next panel"
            );
            assert_eq!(state.queries.tab(), QueriesTab::Logs);
        }

        #[test]
        fn escape_takes_the_popup_down_before_it_leaves_the_editor() {
            let mut state = editing("SELECT * FROM shop.o");

            press(&mut state, Key::Escape);
            assert!(state.queries.completion().is_none());
            assert_eq!(
                state.focus(),
                Panel::Queries,
                "the panel is still the way in"
            );

            press(&mut state, Key::Escape);
            assert_eq!(
                state.focus(),
                Panel::Tables,
                "and escape is still the way out"
            );
        }

        #[test]
        fn a_motion_takes_the_popup_down() {
            let mut state = editing("SELECT * FROM shop.o");

            press(&mut state, Key::Left);

            assert!(state.queries.completion().is_none());
        }

        #[test]
        fn the_arrows_walk_the_popup_while_one_is_up() {
            let mut state = editing("SELECT * FROM s");

            press(&mut state, Key::Down);
            assert_eq!(
                state.queries.completion().expect("still up").selected(),
                1,
                "the popup takes the key the cursor would otherwise have"
            );

            press(&mut state, Key::Up);
            assert_eq!(state.queries.completion().expect("still up").selected(), 0);

            press(&mut state, Key::Left);
            assert!(
                state.queries.completion().is_none(),
                "a horizontal arrow is still a motion, and a motion still \
                 takes the popup down"
            );
        }

        #[test]
        fn ctrl_n_raises_a_popup_and_then_walks_it() {
            let mut state = editing("SELECT * FROM s");
            // A motion takes the popup down without moving off the word, so
            // `ctrl-n` has something to raise against.
            press(&mut state, Key::End);
            assert!(state.queries.completion().is_none());

            press_binding(&mut state, ctrl('n'));
            let raised = state
                .queries
                .completion()
                .expect("s reaches a keyword and a keyspace")
                .selected();
            assert_eq!(raised, 0);

            press_binding(&mut state, ctrl('n'));
            assert_eq!(
                state.queries.completion().expect("still up").selected(),
                1,
                "the second press walks the list it raised"
            );

            press_binding(&mut state, ctrl('p'));
            assert_eq!(state.queries.completion().expect("still up").selected(), 0);
        }

        #[test]
        fn the_buffer_reaches_the_renderer_painted() {
            let state = editing("SELECT id");

            let crate::view::PanelBody::Editor { lines, .. } =
                crate::view::view_model(&state).panels[3].body.clone()
            else {
                panic!("Queries draws an editor");
            };

            assert_eq!(
                lines[0]
                    .iter()
                    .map(|segment| (segment.token, segment.text.clone()))
                    .collect::<Vec<_>>(),
                vec![
                    (Some(crate::style::Token::Keyword), "SELECT".to_owned()),
                    (None, " ".to_owned()),
                    (Some(crate::style::Token::Reference), "id".to_owned()),
                ]
            );
        }
    }
}
