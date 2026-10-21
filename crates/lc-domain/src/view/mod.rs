//! The domain's projection of itself for a renderer, rebuilt every frame
//! (architecture.md §3).

mod contents;
mod details;
pub(crate) mod hints;
pub(crate) mod logs;
mod tables;
mod title;

pub use crate::style::Segment;
pub use contents::{Footer, GridCell, GridColumn, SIZE_SHOWN_FROM};
pub use details::{ColumnKind, DetailRow, Details, Group, Heading, IdentityKind, Section};
pub use hints::{Binding, Hint, HintBar, Liveness, Stroke};
pub use logs::{LogRow, RUNNING};
pub use tables::{RowKind, TreeRow};
use title::TabSpec;
pub use title::{PanelTitle, TAB_SEPARATOR, Tab};

pub use crate::editor::Position;

use crate::commands::{self, CommandRow};
use crate::config::{Config, ConnectionConfig, Diagnostic};
use crate::contents::SelectionMode;
use crate::cql::{CandidateKind, Completion};
use crate::ddl::DdlTab;
use crate::drop::Answer;
use crate::export::{Confirm, Format, Scope, Stop};
use crate::overlay::{Message, Overlay};
use crate::panel::Panel;
use crate::posture::Posture;
use crate::queries::{CqlMode, Highlight, QueriesTab};
use crate::search::{Direction, Search, Sift};
use crate::selection::Ranges;
use crate::state::State;
use crate::style::{Severity, Token};
use crate::theme::ThemeChoice;
use crate::viewport::Cursor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewModel {
    /// In [`Panel::ALL`] order.
    pub panels: [PanelView; 4],
    pub focus: Panel,
    pub overlay: Option<OverlayView>,
    pub hints: HintBar,
    pub theme: ThemeChoice,
    pub toast: Option<ToastLine>,
    /// The modal button held down with the pointer still over it.
    pub pressed: Option<crate::pointer::Press>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastLine {
    pub severity: Severity,
    pub message: String,
}

/// Per panel: a wait is drawn on the border of the content it will fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Resting,
    Working { frame: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayView {
    Message(Message),
    Commands {
        rows: Vec<CommandRow>,
        selected: usize,
        sift: Option<SiftView>,
    },
    /// No target: a prompt's target is always `view.focus`.
    Search {
        query: String,
    },
    Themes {
        /// The rows a live sift left.
        labels: Vec<String>,
        /// The block is sized from this, so a filter moves no border.
        total: usize,
        selected: usize,
        sift: Option<SiftView>,
    },
    /// Carries every row, unlike [`Self::Themes`]: the widest row sets the
    /// block's width, and sizing from the filtered rows would move the border
    /// while typing.
    Connections {
        rows: Vec<ConnectionRow>,
        /// Indices into `rows` that a live sift left.
        matched: Vec<usize>,
        /// Counted in `matched`.
        selected: usize,
        sift: Option<SiftView>,
        /// Where a connection would be written, for the empty-list guidance.
        /// `None` without `$XDG_CONFIG_HOME` or `$HOME`.
        config_home: Option<String>,
    },
    /// Only the rows a sift left: the block is sized from the frame, not its
    /// rows.
    History {
        labels: Vec<String>,
        selected: usize,
        sift: Option<SiftView>,
    },
    Diagnostics {
        rows: Vec<DiagnosticRow>,
        selected: usize,
    },
    Filter {
        column: String,
        query: String,
    },
    /// Boxed for `clippy::large_enum_variant`.
    Export(Box<ExportView>),
    Drop(Box<DropView>),
    Inspect(Box<InspectView>),
    Health(Box<HealthView>),
    Commit(Box<CommitView>),
    Discard {
        message: String,
    },
    Fatal(Message),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthView {
    pub table: crate::contents::TableRef,
    pub body: HealthBody,
    /// `answered of known nodes`: every total in the block is over the first.
    pub coverage: Option<String>,
    pub activity: Activity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthBody {
    Loading,
    Failed(String),
    Rows {
        rows: Vec<crate::health::Row>,
        selected: usize,
    },
}

/// `hosts` arrives joined: punctuation is the renderer's call, made once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRow {
    pub name: String,
    pub hosts: String,
    pub posture: Posture,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRow {
    pub location: String,
    pub severity: Severity,
    pub message: String,
}

#[must_use]
pub fn location(diagnostic: &Diagnostic) -> String {
    diagnostic.span.map_or_else(
        || diagnostic.origin.as_str().to_owned(),
        |span| {
            format!(
                "{}:{}:{}",
                diagnostic.origin.as_str(),
                span.line,
                span.column
            )
        },
    )
}

fn row(diagnostic: &Diagnostic) -> DiagnosticRow {
    DiagnosticRow {
        location: location(diagnostic),
        severity: diagnostic.severity,
        message: diagnostic.message.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportView {
    pub title: Vec<Segment>,
    pub scope_labels: [String; 3],
    pub scope: Scope,
    pub fetched_enabled: bool,
    pub format: Format,
    pub path: String,
    pub cursor: usize,
    pub rows: String,
    pub rows_cursor: usize,
    pub stop: Stop,
    pub confirm: Option<Confirm>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropView {
    pub title: Vec<Segment>,
    /// Pre-split into lines: the renderer draws what it is given rather than
    /// deciding where a statement breaks.
    pub cql: Vec<String>,
    /// `None` for a drop.
    pub rows: Option<usize>,
    /// `None` on the yes/no spelling.
    pub typed: Option<TypedName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectView {
    pub title: Vec<Segment>,
    /// `None` over a row or a null.
    pub size: Option<String>,
    pub body: InspectBody,
    /// Counted across every field.
    pub cursor: usize,
    pub offset: usize,
    pub editing: Option<EditView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditView {
    /// Counted as [`InspectView::cursor`] counts.
    pub field: usize,
    pub text: String,
    /// A character offset, for the caret.
    pub cursor: usize,
    /// Why `⏎` is greyed.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectBody {
    Cell(InspectValue),
    Row {
        names: u16,
        fields: Vec<InspectField>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectValue {
    pub lines: Vec<String>,
    pub token: Option<Token>,
    /// Separate from `token`: overwriting a key column's colour would hide the
    /// key exactly where a write lands.
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectField {
    pub name: String,
    pub token: Option<Token>,
    pub value: InspectValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitView {
    pub title: Vec<Segment>,
    /// Unwrapped: the block wraps and reports the count back.
    pub lines: Vec<Vec<Segment>>,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedName {
    pub name: String,
    pub text: String,
    /// A character offset, for the caret.
    pub cursor: usize,
    pub confirmed: bool,
}

/// The term as typed, including one too short to match, which leaves `search`
/// `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiftView {
    pub query: String,
    pub search: Option<Search>,
}

fn sift_view(sift: &Sift) -> SiftView {
    SiftView {
        query: sift.query().to_owned(),
        search: sift.search(),
    }
}

fn connection_row(config: &Config, connection: &ConnectionConfig) -> ConnectionRow {
    ConnectionRow {
        name: connection.name.as_str().to_owned(),
        hosts: connection.hosts.join(", "),
        posture: connection.posture,
        is_default: config
            .default_connection
            .as_ref()
            .is_some_and(|name| *name == connection.name),
    }
}

fn history_view(state: &State, cursor: Cursor, sift: Option<&Sift>) -> OverlayView {
    let labels = crate::update::history_labels(state);
    let search = sift.and_then(Sift::search);

    OverlayView::History {
        labels: crate::update::picker_matches(&labels, search.as_ref())
            .into_iter()
            .map(|at| labels[at].clone())
            .collect(),
        selected: cursor.index(),
        sift: sift.map(sift_view),
    }
}

fn health_view(state: &State, panel: &crate::health::HealthPanel) -> HealthView {
    use crate::health::Report;

    HealthView {
        table: panel.table().clone(),
        body: match panel.report() {
            Report::Loading => HealthBody::Loading,
            Report::Failed(error) => HealthBody::Failed(error.to_string()),
            Report::Loaded(_) => HealthBody::Rows {
                rows: panel.rows(),
                selected: panel.selected(),
            },
        },
        coverage: match panel.report() {
            Report::Loaded(health) => {
                let (answered, known) = health.coverage();
                Some(format!("{answered} of {known} nodes answered"))
            }
            Report::Loading | Report::Failed(_) => None,
        },
        activity: if panel.is_loading() {
            Activity::Working {
                frame: state.loader.frame(),
            }
        } else {
            Activity::Resting
        },
    }
}

fn overlay(state: &State) -> Option<OverlayView> {
    match state.overlay.as_ref()? {
        Overlay::Message(message) => Some(OverlayView::Message(message.clone())),
        Overlay::Commands(panel) => Some(OverlayView::Commands {
            rows: commands::rows(panel.search().as_ref(), state.posture(), &state.contents),
            selected: panel.selected(),
            sift: panel.sift().map(sift_view),
        }),
        Overlay::Diagnostics(panel) => Some(OverlayView::Diagnostics {
            rows: panel.rows().iter().map(row).collect(),
            selected: panel.selected(),
        }),
        // Drawn on the hint bar row, but `Some`: an overlay is what stops a
        // panel drawing its own cursor.
        Overlay::Search(prompt) => Some(OverlayView::Search {
            query: prompt.query().to_owned(),
        }),
        Overlay::Themes { cursor, sift, .. } => {
            let labels = crate::update::theme_labels(&state.config);
            let search = sift.as_ref().and_then(Sift::search);

            Some(OverlayView::Themes {
                total: labels.len(),
                labels: crate::update::picker_matches(&labels, search.as_ref())
                    .into_iter()
                    .map(|at| labels[at].clone())
                    .collect(),
                selected: cursor.index(),
                sift: sift.as_ref().map(sift_view),
            })
        }
        Overlay::History { cursor, sift } => Some(history_view(state, *cursor, sift.as_ref())),
        Overlay::Health(panel) => Some(OverlayView::Health(Box::new(health_view(state, panel)))),
        Overlay::Connections { cursor, sift } => {
            let names = crate::update::connection_names(&state.config);
            let search = sift.as_ref().and_then(Sift::search);

            Some(OverlayView::Connections {
                rows: state
                    .config
                    .connections
                    .iter()
                    .map(|connection| connection_row(&state.config, connection))
                    .collect(),
                matched: crate::update::picker_matches(&names, search.as_ref()),
                selected: cursor.index(),
                sift: sift.as_ref().map(sift_view),
                config_home: state
                    .config_home
                    .as_ref()
                    .map(|path| path.display().to_string()),
            })
        }
        Overlay::Export(form) => Some(OverlayView::Export(Box::new(ExportView {
            title: form.title(),
            scope_labels: form.scope_labels(),
            scope: form.scope(),
            fetched_enabled: form.fetched_enabled(),
            format: form.format(),
            path: form.path().text().to_owned(),
            cursor: form.path().cursor(),
            rows: form.rows().text().to_owned(),
            rows_cursor: form.rows().cursor(),
            stop: form.stop(),
            confirm: form.confirm(),
        }))),
        Overlay::Filter(filter) => Some(OverlayView::Filter {
            column: filter.name().to_owned(),
            query: filter.query().to_owned(),
        }),
        Overlay::Drop(prompt) => Some(OverlayView::Drop(Box::new(DropView {
            title: prompt.title(),
            cql: prompt.statement().cql.lines().map(str::to_owned).collect(),
            rows: prompt.target().row_count(),
            typed: match prompt.answer() {
                Answer::YesNo => None,
                Answer::Typed(line) => Some(TypedName {
                    name: prompt.target().name().to_owned(),
                    text: line.text().to_owned(),
                    cursor: line.cursor(),
                    confirmed: prompt.confirmed(),
                }),
            },
        }))),
        Overlay::Inspect(inspector) => Some(OverlayView::Inspect(Box::new(contents::inspect(
            state, inspector,
        )))),
        Overlay::Commit(prompt) => Some(OverlayView::Commit(Box::new(CommitView {
            title: prompt.title(),
            lines: prompt.lines(),
            offset: prompt.offset(),
        }))),
        Overlay::Discard(discard) => Some(OverlayView::Discard {
            message: discard.message(),
        }),
        Overlay::Fatal(message) => Some(OverlayView::Fatal(message.clone())),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionView {
    pub rows: Vec<CompletionRow>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRow {
    pub text: String,
    pub token: Token,
}

fn completion(completion: &Completion) -> CompletionView {
    CompletionView {
        rows: completion
            .candidates()
            .iter()
            .map(|candidate| CompletionRow {
                text: candidate.label.clone(),
                token: match candidate.kind {
                    CandidateKind::Keyspace => Token::Keyspace,
                    CandidateKind::Table => Token::Table,
                    // A shape is grammar with holes; a `WITH` option is a name
                    // the schema holds.
                    CandidateKind::Keyword | CandidateKind::Template => Token::Keyword,
                    CandidateKind::Column | CandidateKind::Property => Token::Reference,
                },
            })
            .collect(),
        selected: completion.selected(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub panel: Panel,
    pub title: PanelTitle,
    pub body: PanelBody,
    /// Drawn on the panel's own border, focused or not: the bar only describes
    /// the focused panel.
    pub search: Option<crate::search::Search>,
    pub activity: Activity,
    /// Table contents alone has one.
    pub footer: Option<Footer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelBody {
    Blank,
    Notice {
        text: String,
        token: Option<Token>,
    },
    Tree {
        rows: Vec<TreeRow>,
        selected: usize,
        /// State, not derived, so a wheel can move it without moving
        /// `selected`.
        offset: usize,
        /// Empty means no selection.
        ranges: Ranges,
        /// A yank's afterglow, painted louder.
        flash: bool,
    },
    /// `offset` counts wrapped lines, which only the renderer can produce.
    Details {
        details: Details,
        selected: usize,
        offset: usize,
        ranges: Ranges,
        flash: bool,
    },
    /// No offsets: the renderer derives them from the cursor and its area.
    Editor {
        lines: Vec<Vec<Segment>>,
        cursor: Position,
        /// The caret is drawn while editing only.
        mode: CqlMode,
        /// Drawn in both modes: it says what `ctrl-r` would run.
        band: Option<Highlight>,
        completion: Option<CompletionView>,
    },
    Grid {
        columns: Vec<GridColumn>,
        /// Only the rows from `offset` the panel can show: a million-row grid
        /// cannot be projected whole every frame.
        rows: Vec<Vec<GridCell>>,
        /// Every row the arrangement leaves, drawn or not.
        total: usize,
        selected: usize,
        offset: usize,
        first_column: usize,
        column: usize,
        mode: SelectionMode,
        /// Zero for a grid with no rows.
        gutter: u16,
        /// Empty means the axis was never extended: every row for a rectangle,
        /// no gutter bar for rows.
        rows_ranges: Ranges,
        /// Empty means the axis was never extended: every column for a
        /// rectangle.
        columns_ranges: Ranges,
        flash: bool,
    },
    Log {
        rows: Vec<LogRow>,
        selected: usize,
        offset: usize,
        ranges: Ranges,
        flash: bool,
    },
}

/// It cannot contend with a selection: the yank spent it, and the next input
/// ends the flash.
fn flash_over(state: &State, panel: Panel) -> Option<&crate::feedback::Flash> {
    state.flash.as_ref().filter(|flash| flash.panel() == panel)
}

#[must_use]
pub fn view_model(state: &State) -> ViewModel {
    ViewModel {
        panels: Panel::ALL.map(|panel| PanelView {
            panel,
            title: title(state, panel),
            body: body(state, panel),
            search: crate::search::Searched::of(panel)
                .and_then(|target| state.search(target))
                .cloned(),
            activity: if state.is_working_in(panel) {
                Activity::Working {
                    frame: state.loader.frame(),
                }
            } else {
                Activity::Resting
            },
            footer: match panel {
                Panel::Contents => contents::footer(state),
                Panel::Tables | Panel::Ddl | Panel::Queries => None,
            },
        }),
        focus: state.focus(),
        hints: hints::bar(state),
        overlay: overlay(state),
        theme: state.theme.clone(),
        pressed: state.held.filter(|held| held.over).map(|held| held.press),
        toast: state.toast.as_ref().map(|toast| ToastLine {
            severity: toast.severity(),
            message: toast.message().to_owned(),
        }),
    }
}

/// Counted from the same projection the panel draws, so the two cannot
/// disagree.
#[must_use]
pub fn ddl_rows(state: &State) -> usize {
    details::of(state).map_or(0, |details| details.row_count())
}

#[must_use]
pub fn details_of(state: &State) -> Option<Details> {
    details::of(state)
}

#[must_use]
pub fn ddl_next_match(state: &State, from: usize, direction: Direction) -> Option<usize> {
    let search = state.ddl.search()?;
    let details = details::of(state)?;

    details::next_match(&details, search, from, direction)
}

/// Public so walking the strip never lands on a tab it does not draw.
#[must_use]
pub fn ddl_tabs(state: &State) -> &'static [DdlTab] {
    details::tabs_for(state)
}

fn title(state: &State, panel: Panel) -> PanelTitle {
    let number = panel.number();

    match panel {
        Panel::Tables => tables::title(state, number),
        Panel::Ddl => {
            let tab = details::shown_tab(state);
            let specs: Vec<TabSpec> = details::tabs_for(state)
                .iter()
                .map(|candidate| TabSpec {
                    label: candidate.label(),
                    short: candidate.short_label(),
                    active: *candidate == tab,
                    number: None,
                })
                .collect();

            PanelTitle::tabbed(number, &specs)
        }
        Panel::Contents => PanelTitle::styled(number, contents::title(state)),
        Panel::Queries => {
            let tab = state.queries.tab();
            let specs: Vec<TabSpec> = QueriesTab::ALL
                .iter()
                .map(|candidate| TabSpec {
                    label: candidate.label(),
                    short: candidate.short_label(),
                    active: *candidate == tab,
                    // The panel's own prefix already names the first tab.
                    number: (candidate.number() != number).then_some(candidate.number()),
                })
                .collect();

            PanelTitle::tabbed(number, &specs)
        }
    }
}

fn body(state: &State, panel: Panel) -> PanelBody {
    match panel {
        Panel::Tables => tables::body(state),
        Panel::Ddl => details::body(state),
        Panel::Contents => contents::body(state),
        Panel::Queries => match state.queries.tab() {
            QueriesTab::Cql => PanelBody::Editor {
                lines: crate::cql::highlight(&state.queries.text()),
                cursor: state.queries.cursor(),
                mode: state.queries.mode().unwrap_or_default(),
                band: state.queries.band(),
                completion: state.queries.completion().map(completion),
            },
            QueriesTab::Logs => logs::log_body(state),
        },
    }
}

pub(crate) fn waiting(text: &str) -> PanelBody {
    PanelBody::Notice {
        text: text.to_owned(),
        token: Some(Token::Muted),
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use crate::connection::{Connection, ConnectionState, SchemaState};
    use crate::model::{ConnId, Schema};
    use crate::posture::Posture;
    use crate::state::State;
    use crate::tree::tests_support::schema_of as tabled_schema;

    use super::{PanelBody, view_model};

    pub(crate) fn state_with(connection: ConnectionState) -> State {
        let mut state = State::default();
        state.connection = connection;
        state
    }

    pub(crate) fn connected(schema: Schema) -> State {
        state_with(ConnectionState::Connected(Connection {
            id: ConnId::next(),
            name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema),
        }))
    }

    pub(crate) fn loaded(names: &[&str]) -> State {
        connected(tabled_schema(
            &names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>(),
        ))
    }

    pub(crate) fn tabled(keyspaces: &[(&str, &[&str])]) -> State {
        connected(tabled_schema(keyspaces))
    }

    pub(crate) fn panel_body(state: &State, index: usize) -> PanelBody {
        view_model(state).panels[index].body.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use super::tests_support::{loaded, tabled};
    use super::*;
    use crate::clock::Stamp;
    use crate::commands::CommandsPanel;
    use crate::config::{Config, CustomTheme};
    use crate::contents::{Subject, TableRef};
    use crate::export::{ExportForm, Fetched, Format, Scope, Stop};
    use crate::jobs::ExportJob;
    use crate::model::{JobId, Key, Page, QueryId};
    use crate::overlay::{Message, Overlay};
    use crate::theme::{BuiltIn, Ours, Ported, ThemeName};
    use crate::update::init;
    use crate::viewport::Cursor;

    #[test]
    fn every_panel_is_projected_in_reading_order() {
        let view = view_model(&State::default());

        assert_eq!(view.panels.map(|panel| panel.panel), Panel::ALL);
    }

    #[test]
    fn panels_carry_the_titles_the_draft_names() {
        let view = view_model(&State::default());

        assert_eq!(
            view.panels.map(|panel| panel.title.text()),
            [
                // Nothing selected, so no Columns tab.
                "[1]-Tables",
                "[2]-DDL",
                "[3]-Table contents",
                "[4]-Queries - [5]-Logs"
            ]
            .map(str::to_owned)
        );
    }

    #[test]
    fn focus_mirrors_the_state() {
        let mut state = State::default();
        state.focus_on(Panel::Contents);

        assert_eq!(view_model(&state).focus, Panel::Contents);
    }

    #[test]
    fn the_queries_panel_shows_its_buffer() {
        let view = view_model(&loaded(&["shop"]));

        assert_eq!(
            view.panels[3].body,
            PanelBody::Editor {
                lines: vec![Vec::new()],
                cursor: Position::default(),
                mode: CqlMode::Browsing,
                band: None,
                completion: None,
            },
            "an untouched buffer is one empty line with the cursor on it"
        );
    }

    #[test]
    fn the_queries_panel_animates_while_the_trail_holds_a_pending_entry() {
        let mut state = loaded(&["shop"]);
        state.log.push(crate::log::Entry::new(
            Stamp::default(),
            crate::log::Subject::Sentence("SELECT".to_owned()),
            crate::log::Outcome::Pending {
                trace: crate::log::Trace::Query(QueryId::next()),
            },
        ));

        let view = view_model(&state);

        assert_eq!(
            view.panels[3].activity,
            Activity::Working {
                frame: state.loader.frame()
            },
            "a pending entry is exactly what the ⋯ running animation exists to distinguish \
             from a fast one"
        );
    }

    #[test]
    fn the_queries_panel_rests_again_once_its_last_pending_entry_completes() {
        let mut state = loaded(&["shop"]);
        let query = QueryId::next();
        state.log.push(crate::log::Entry::new(
            Stamp::default(),
            crate::log::Subject::Sentence("SELECT".to_owned()),
            crate::log::Outcome::Pending {
                trace: crate::log::Trace::Query(query),
            },
        ));
        assert_eq!(
            view_model(&state).panels[3].activity,
            Activity::Working {
                frame: state.loader.frame()
            },
            "sanity: it started out working"
        );

        state.log.complete(
            crate::log::Trace::Query(query),
            crate::log::Outcome::Succeeded {
                detail: "1 row".to_owned(),
                took: std::time::Duration::from_millis(1),
            },
        );

        assert_eq!(
            view_model(&state).panels[3].activity,
            Activity::Resting,
            "nothing is pending any more, so nothing should still be animating"
        );
    }

    #[test]
    fn a_column_offered_with_no_table_in_scope_draws_its_label_but_types_its_bare_name() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.queries.start_editing();
        state.queries.replace("SELECT i");
        let completion = crate::cql::suggest(state.queries.buffer(), state.schema(), None);
        state.queries.offer(completion);

        let PanelBody::Editor { completion, .. } = &body(&state, Panel::Queries) else {
            panic!("the Queries panel is always an editor");
        };
        let at = completion
            .as_ref()
            .expect("offers")
            .rows
            .iter()
            .position(|row| row.text == "shop.orders.id")
            .expect("the popup row draws the label, not the bare column name");

        for _ in 0..at {
            state.queries.step(true);
        }
        state.queries.edit(Key::Tab, 0);

        assert_eq!(
            state.queries.text(),
            "SELECT id FROM shop.orders",
            "accepting types the bare column name, and the clause that puts it in scope"
        );
    }

    #[test]
    fn the_overlay_is_carried_to_the_renderer() {
        let message = Message {
            title: "Connection failed".to_owned(),
            message: "no route".to_owned(),
            severity: Severity::Danger,
        };
        let mut state = State::default();
        state.overlay = Some(Overlay::Message(message.clone()));

        assert_eq!(
            view_model(&state).overlay,
            Some(OverlayView::Message(message))
        );
    }

    #[test]
    fn a_panel_carries_the_search_applied_to_it() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.apply_search(
            crate::search::Searched::Contents,
            crate::search::Search::new("ORD"),
        );

        let view = view_model(&state);

        assert_eq!(
            view.panels[2]
                .search
                .as_ref()
                .map(crate::search::Search::as_str),
            Some("ORD"),
            "as typed"
        );
        assert_eq!(view.panels[0].search, None);
        assert_eq!(view.panels[3].search, None, "Queries holds none");
    }

    #[test]
    fn ddl_next_match_walks_the_selected_tables_rows() {
        let mut state = tabled(&[("shop", &["orders_by_customer"])]);
        let schema = state.schema().expect("connected above").clone();
        state.tables.open(&schema);
        state.tables.open(&schema);
        state.apply_search(
            crate::search::Searched::Ddl,
            crate::search::Search::new("order"),
        );

        assert!(
            ddl_next_match(&state, 0, Direction::Forwards).is_some(),
            "the search reaches the selected table's own rows"
        );
    }

    #[test]
    fn ddl_next_match_is_none_without_a_search() {
        let mut state = tabled(&[("shop", &["orders_by_customer"])]);
        let schema = state.schema().expect("connected above").clone();
        state.tables.open(&schema);
        state.tables.open(&schema);

        assert_eq!(ddl_next_match(&state, 0, Direction::Forwards), None);
    }

    #[test]
    fn an_open_prompt_is_projected_as_its_query() {
        let mut prompt = crate::search::Prompt::opened(
            crate::search::Searched::Tables,
            None,
            crate::search::Origin::default(),
        );
        prompt.push('o');
        prompt.push('r');
        let mut state = State::default();
        state.overlay = Some(Overlay::Search(prompt));

        assert_eq!(
            view_model(&state).overlay,
            Some(OverlayView::Search {
                query: "or".to_owned()
            })
        );
    }

    #[test]
    fn an_open_palette_is_projected_as_rows_and_a_selection() {
        let mut state = State::default();
        state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));

        assert_eq!(
            view_model(&state).overlay,
            Some(OverlayView::Commands {
                rows: crate::commands::rows(None, state.posture(), &state.contents),
                selected: 1,
                sift: None,
            })
        );
    }

    #[test]
    fn an_overlay_carries_its_sift_to_the_renderer() {
        let mut state = State::default();
        let mut panel = CommandsPanel::opened();
        panel.open_sift();
        panel.push('q');
        state.overlay = Some(Overlay::Commands(panel));

        let Some(OverlayView::Commands { sift, .. }) = view_model(&state).overlay else {
            panic!("an open palette");
        };
        let sift = sift.expect("a live sift");

        assert_eq!(sift.query, "q");
        assert!(sift.search.expect("a term").matches("quit"));
    }

    #[test]
    fn an_overlay_with_no_sift_carries_none() {
        let mut state = State::default();
        state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));

        let Some(OverlayView::Commands { sift, .. }) = view_model(&state).overlay else {
            panic!("an open palette");
        };

        assert!(sift.is_none());
    }

    #[test]
    fn a_sift_open_on_an_empty_query_carries_no_search() {
        let mut state = State::default();
        let mut panel = CommandsPanel::opened();
        panel.open_sift();
        state.overlay = Some(Overlay::Commands(panel));

        let Some(OverlayView::Commands { sift, .. }) = view_model(&state).overlay else {
            panic!("an open palette");
        };
        let sift = sift.expect("a live sift, even with nothing typed");

        assert_eq!(sift.query, "");
        assert!(sift.search.is_none());
    }

    #[test]
    fn an_open_picker_is_projected_as_every_theme_and_the_cursors_position() {
        let built_in = BuiltIn::Ported(Ported::CatppuccinMocha);
        let index = BuiltIn::ALL
            .iter()
            .position(|b| *b == built_in)
            .expect("listed");
        let mut cursor = crate::viewport::Cursor::default();
        cursor.go_to(index, BuiltIn::ALL.len());
        let mut state = State::default();
        state.overlay = Some(Overlay::Themes {
            cursor,
            restored: ThemeChoice::default(),
            sift: None,
        });

        assert_eq!(
            view_model(&state).overlay,
            Some(OverlayView::Themes {
                labels: BuiltIn::ALL
                    .iter()
                    .map(|built_in| built_in.label().to_owned())
                    .collect(),
                total: BuiltIn::ALL.len(),
                selected: index,
                sift: None,
            })
        );
    }

    #[test]
    fn the_picker_lists_the_built_ins_and_then_the_config_s_own() {
        let midnight = ThemeName::try_new("midnight").expect("valid");
        let config = Config {
            themes: vec![CustomTheme {
                name: midnight.clone(),
                extends: BuiltIn::Ours(Ours::AshDark),
                colors: BTreeMap::new(),
            }],
            ..Config::default()
        };
        let (mut state, _) = init(config, Vec::new(), Stamp::default());
        state.overlay = Some(Overlay::Themes {
            cursor: Cursor::default(),
            restored: ThemeChoice::default(),
            sift: None,
        });

        let Some(OverlayView::Themes { labels, .. }) = view_model(&state).overlay else {
            panic!("the picker is up");
        };

        assert_eq!(labels.len(), BuiltIn::ALL.len() + 1);
        assert_eq!(labels.last().map(String::as_str), Some("midnight"));
    }

    fn every_contents_state() -> Vec<State> {
        let query = QueryId::next();
        let mut loading = tabled(&[("shop", &["orders"])]);
        loading.contents.requested(
            query,
            Subject::Table(TableRef::new("shop", "orders")),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );

        let mut loaded = loading.clone();
        loaded.contents.page_ready(
            query,
            Page::default(),
            crate::config::MaxColumnWidth::default(),
            QueryId::next,
        );

        let mut failed = loading.clone();
        failed.contents.failed(
            query,
            &crate::model::DbError::Invalid("undefined column name total".to_owned()),
        );

        vec![State::default(), loading, loaded, failed]
    }

    #[test]
    fn the_export_form_projects_everything_the_renderer_draws() {
        let form = ExportForm::opened(
            Subject::Table(TableRef::new("shop", "orders")),
            Fetched::Some(200),
            Path::new("/home/ike"),
        );
        let mut state = State::default();
        state.overlay = Some(Overlay::Export(form));

        let Some(OverlayView::Export(view)) = view_model(&state).overlay else {
            panic!("the form is projected");
        };
        assert_eq!(crate::style::text(&view.title), "Export table shop.orders");
        assert_eq!(view.scope, Scope::Whole);
        assert!(view.fetched_enabled);
        assert_eq!(view.format, Format::Csv);
        assert_eq!(view.path, "/home/ike/shop.orders.csv");
        assert_eq!(view.cursor, 25);
        assert_eq!(view.stop, Stop::Path);
        assert_eq!(view.confirm, None);
    }

    #[test]
    fn a_running_export_puts_its_count_on_the_bar() {
        let mut state = State::default();
        state.focus_on(Panel::Contents);
        state.export = Some(ExportJob::started(
            JobId::next(),
            crate::export::Destination::File {
                path: PathBuf::from("/home/ike/shop.orders.csv"),
                overwrite: false,
            },
            ExportForm::opened(
                Subject::Table(TableRef::new("shop", "orders")),
                Fetched::Some(200),
                Path::new(""),
            ),
            crate::Source::Fetched {
                columns: Vec::new(),
                rows: Vec::new(),
            },
        ));
        if let Some(job) = &mut state.export {
            let id = job.id();
            job.progressed(id, 41_203);
        }

        assert_eq!(
            view_model(&state).hints.export,
            Some("exporting 41203 rows…".to_owned())
        );
    }

    #[test]
    fn no_export_leaves_the_bar_alone() {
        assert_eq!(view_model(&State::default()).hints.export, None);
    }

    #[test]
    fn every_panel_title_opens_with_the_key_that_focuses_it() {
        for state in every_contents_state() {
            for panel in view_model(&state).panels {
                let expected = format!("[{}]-", panel.panel.number());

                assert!(
                    panel.title.text().starts_with(&expected),
                    "{:?} titled {:?} does not open with {expected:?}",
                    panel.panel,
                    panel.title.text(),
                );
            }
        }
    }
}
