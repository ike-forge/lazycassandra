//! The domain's projection of itself for a renderer.
//!
//! architecture.md §3. One tree is rebuilt per frame; §11's question about
//! per-panel view models with dirty tracking stays open until panels carry
//! enough content to measure.
//!
//! One module per panel that has content, because each projection brings its
//! own vocabulary — the same reason `TablesPanel` left `state.rs`.

mod contents;
mod details;
pub(crate) mod hints;
mod tables;
mod title;

pub use contents::{GridCell, GridColumn};
pub use details::{ColumnKind, DetailRow, Details, Group, Heading, IdentityKind, Section};
pub use hints::{Binding, Hint, HintBar, Liveness};
pub use tables::{RowKind, TreeRow};
use title::TabSpec;
pub use title::{PanelTitle, TAB_SEPARATOR, Tab};

pub use crate::editor::Position;

use crate::commands::{self, CommandRow};
use crate::config::Diagnostic;
use crate::contents::SelectionMode;
use crate::ddl::DdlTab;
use crate::export::{Confirm, Format, Scope, Stop};
use crate::model::DbError;
use crate::state::{Message, Overlay, Panel, Ranges, State};
use crate::style::{Severity, Token};
use crate::theme::{BuiltIn, ThemeChoice};

/// Everything a renderer needs for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewModel {
    /// In [`Panel::ALL`] order.
    pub panels: [PanelView; 4],
    pub focus: Panel,
    /// Drawn over the panels when something needs saying.
    pub overlay: Option<OverlayView>,
    /// What the focused panel responds to, along the bottom of the frame.
    pub hints: HintBar,
    /// Whether the tool is waiting on the cluster, and how far into the
    /// loader's cycle that wait is.
    pub activity: Activity,
    pub theme: ThemeChoice,
}

/// What the loader has to say.
///
/// An enum rather than a bare frame number the domain promises to zero: which
/// of the two states the tool is in is the decision `update` is making, and
/// the one `lc-app` reads to know whether a tick is worth anything
/// (docs/superpowers/specs/2026-08-09-visual-polish-design.md §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// Nothing outstanding. Drawn at the start of the cycle, always the same
    /// frame.
    Resting,
    Working {
        frame: usize,
    },
}

/// An overlay as a renderer needs it.
///
/// Splits from [`Overlay`] because the palette's state is a cursor and its
/// projection is rows — the same reason `TablesPanel` and [`PanelBody::Tree`]
/// are not one type. `Message` is shared rather than restated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayView {
    Message(Message),
    Commands {
        rows: Vec<CommandRow>,
        selected: usize,
    },
    /// Carries only the query: `Prompt::target` is what the renderer needs to
    /// find the panel it is drawn against, but that panel is already
    /// `view.focus` — `target` is captured focus, and cannot be anything
    /// else while the prompt is up (design §3.2) — so restating it here would
    /// be a second name for the same fact.
    Search {
        query: String,
    },
    /// The theme picker's rows and where its cursor is.
    Themes {
        labels: Vec<String>,
        selected: usize,
    },
    /// The config diagnostics' rows and where its cursor is.
    Diagnostics {
        rows: Vec<DiagnosticRow>,
        selected: usize,
    },
    /// Boxed: `OverlayView`'s other variants are small, and clippy's
    /// `large_enum_variant` is denied.
    Export(Box<ExportView>),
}

/// One diagnostic as a renderer needs it: the location already formatted, so
/// the renderer never has to know how a span is spelled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRow {
    pub location: String,
    pub severity: Severity,
    pub message: String,
}

fn row(diagnostic: &Diagnostic) -> DiagnosticRow {
    let location = diagnostic.span.map_or_else(
        || diagnostic.origin.as_str().to_owned(),
        |span| {
            format!(
                "{}:{}:{}",
                diagnostic.origin.as_str(),
                span.line,
                span.column
            )
        },
    );

    DiagnosticRow {
        location,
        severity: diagnostic.severity,
        message: diagnostic.message.clone(),
    }
}

/// The export form, flattened for a renderer that never sees an
/// [`ExportForm`](crate::export::ExportForm).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportView {
    pub title: String,
    /// Whole first, fetched second — the order they are drawn in.
    pub scope_labels: [String; 2],
    pub scope: Scope,
    pub fetched_enabled: bool,
    pub format: Format,
    pub path: String,
    /// A character offset into `path`, for the drawn caret.
    pub cursor: usize,
    pub stop: Stop,
    pub confirm: Option<Confirm>,
}

fn overlay(state: &State) -> Option<OverlayView> {
    match state.overlay.as_ref()? {
        Overlay::Message(message) => Some(OverlayView::Message(message.clone())),
        Overlay::Commands(panel) => Some(OverlayView::Commands {
            rows: commands::rows(),
            selected: panel.selected(),
        }),
        Overlay::Diagnostics(panel) => Some(OverlayView::Diagnostics {
            rows: panel.rows().iter().map(row).collect(),
            selected: panel.selected(),
        }),
        // `Some`, not drawn through the modal path the other two overlays
        // use — the renderer draws it on the hint bar row instead. Still
        // `Some` rather than `None`: `view.overlay.is_none()` is what
        // `lc-inbound::render` reads to decide whether a panel may show its
        // own cursor, and a prompt outranks focus exactly as a message or
        // the palette does (design §1).
        Overlay::Search(prompt) => Some(OverlayView::Search {
            query: prompt.query().to_owned(),
        }),
        Overlay::Themes { cursor, .. } => Some(OverlayView::Themes {
            labels: BuiltIn::ALL
                .iter()
                .map(|built_in| built_in.label().to_owned())
                .chain(
                    state
                        .config
                        .themes
                        .iter()
                        .map(|theme| theme.name.as_str().to_owned()),
                )
                .collect(),
            selected: cursor.index(),
        }),
        Overlay::Export(form) => Some(OverlayView::Export(Box::new(ExportView {
            title: form.title(),
            scope_labels: form.scope_labels(),
            scope: form.scope(),
            fetched_enabled: form.fetched_enabled(),
            format: form.format(),
            path: form.path().text().to_owned(),
            cursor: form.path().cursor(),
            stop: form.stop(),
            confirm: form.confirm(),
        }))),
    }
}

/// One panel, described without reference to how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub panel: Panel,
    pub title: PanelTitle,
    pub body: PanelBody,
    /// The applied filter's term, as typed. Drawn on the panel's own top
    /// border, focused or not: a filter that survives a focus change has to
    /// say so from the panel it is applied to, not from the bar, which only
    /// ever describes the focused panel (panel search design §5.1).
    pub filter: Option<String>,
}

/// What a panel has to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelBody {
    Blank,
    /// A sentence where content would be: waiting, empty, or failed.
    ///
    /// `token` carries the meaning the renderer must draw it with — `None`
    /// styles nothing, `Some(Token::Muted)` for a waiting state,
    /// `Some(Token::Severity(Severity::Danger))` for a failure. Mirrors
    /// [`TreeRow::token`] so the panel never has to decide what a sentence
    /// means; the domain already knows (architecture.md §6).
    Notice {
        text: String,
        token: Option<Token>,
    },
    Tree {
        rows: Vec<TreeRow>,
        selected: usize,
        /// The first row drawn. State rather than a value the renderer
        /// derives, so a wheel can move it without moving `selected`
        /// (docs/superpowers/specs/2026-08-09-list-viewport-design.md §2).
        offset: usize,
        /// The rows `v` has selected, as groups. Empty means no selection is
        /// active (multi-group selection design §6).
        ranges: Ranges,
        /// Set while `ranges` is a yank's afterglow rather than a live
        /// selection, which the renderer paints louder (visual mode design §6.1).
        flash: bool,
    },
    /// A heading and grouped rows: what is selected, described.
    ///
    /// `offset` is the first line drawn, counted in wrapped lines. Only the
    /// renderer can produce those; the domain scrolls by them on the strength
    /// of the count `Msg::Measured` brings back (design §2).
    Details {
        details: Details,
        /// The row the Ddl cursor is on, in [`Details::rows`] order — the
        /// same meaning [`PanelBody::Tree::selected`] and
        /// [`PanelBody::Grid::selected`] carry.
        selected: usize,
        offset: usize,
        /// The rows `v` has selected, as [`PanelBody::Tree::ranges`].
        ranges: Ranges,
        /// As [`PanelBody::Tree::flash`].
        flash: bool,
    },
    /// A text buffer and where its cursor is.
    ///
    /// Carries no offsets. The renderer derives both from the cursor and the
    /// area it has, the way it already does for a list's selection — so an
    /// editor scrolls without any state being stored for it (design §4).
    Editor {
        lines: Vec<String>,
        cursor: Position,
    },
    /// A result set: one header per column, one row per row.
    Grid {
        columns: Vec<GridColumn>,
        rows: Vec<Vec<GridCell>>,
        selected: usize,
        /// The first row drawn, as [`PanelBody::Tree::offset`].
        offset: usize,
        /// The leftmost column drawn.
        first_column: usize,
        /// The selected column, an index into `columns`.
        column: usize,
        /// Which of `selected` and `column` is drawn, and how
        /// (docs/superpowers/specs/2026-08-13-selection-modes-design.md §4).
        mode: SelectionMode,
        /// Character columns the row-number gutter takes, or zero for a grid
        /// with no rows.
        gutter: u16,
        /// The rows `v` has selected while extending the row axis. Empty
        /// means that axis was never extended, which `body.rs`'s `ranged`
        /// reads as "every row" when painting a rectangle and as "no gutter
        /// bar" when marking rows.
        rows_ranges: Ranges,
        /// The columns `v` has selected while extending the column axis.
        /// Empty means that axis was never extended, which `body.rs`'s
        /// `ranged` reads as "every column" when painting a rectangle.
        columns_ranges: Ranges,
        /// Set while the two ranges are a yank's afterglow rather than a live
        /// selection, as [`PanelBody::Tree::flash`].
        flash: bool,
    },
}

/// A yank's afterglow over `panel`, if that is where the last one was lit.
///
/// It outranks the selection wherever it is lit, and cannot contend with one:
/// the yank spent the selection that named it, and the next input puts the
/// flash out before it can reach a new one (visual mode design §6.1).
fn flash_over(state: &State, panel: Panel) -> Option<&crate::state::Flash> {
    state.flash.as_ref().filter(|flash| flash.panel() == panel)
}

/// Project `state` into the frame a renderer draws.
#[must_use]
pub fn view_model(state: &State) -> ViewModel {
    ViewModel {
        panels: Panel::ALL.map(|panel| PanelView {
            panel,
            title: title(state, panel),
            body: body(state, panel),
            filter: crate::search::Filtered::of(panel)
                .and_then(|target| state.filter(target))
                .map(|filter| filter.as_str().to_owned()),
        }),
        focus: state.focus(),
        hints: hints::bar(state),
        overlay: overlay(state),
        activity: if state.is_working() {
            Activity::Working {
                frame: state.loader.frame(),
            }
        } else {
            Activity::Resting
        },
        theme: state.theme.clone(),
    }
}

/// How many rows the Table DDL panel describes.
///
/// Built by the same projection the panel draws rather than counted a second
/// way: two row counts that can disagree is exactly the drift `tree.rs`'s
/// single flattening exists to prevent.
#[must_use]
pub fn ddl_rows(state: &State) -> usize {
    details::of(state).map_or(0, |details| details.row_count())
}

/// The whole Table DDL projection a range reads.
#[must_use]
pub fn details_of(state: &State) -> Option<Details> {
    details::of(state)
}

/// The tabs the Ddl panel offers for what the Tables cursor is on.
///
/// Public because walking the strip must not land on a tab it does not draw:
/// on a keyspace there is one tab, and pressing `2` again stays on it.
#[must_use]
pub fn ddl_tabs(state: &State) -> &'static [DdlTab] {
    details::tabs_for(state)
}

/// A panel's top border: one name, or the strip of tabs it can show.
fn title(state: &State, panel: Panel) -> PanelTitle {
    let number = panel.number();
    let single = |text: String| PanelTitle::single(number, text);

    match panel {
        Panel::Tables => single("Tables".to_owned()),
        Panel::Ddl => {
            let tab = details::shown_tab(state);
            let specs: Vec<TabSpec> = details::tabs_for(state)
                .iter()
                .map(|candidate| TabSpec {
                    label: candidate.label(),
                    short: candidate.short_label(),
                    active: *candidate == tab,
                })
                .collect();

            PanelTitle::tabbed(number, &specs)
        }
        Panel::Contents => single(contents::title(state)),
        Panel::Queries => single("Queries".to_owned()),
    }
}

fn body(state: &State, panel: Panel) -> PanelBody {
    match panel {
        Panel::Tables => tables::body(state),
        Panel::Ddl => details::body(state),
        Panel::Contents => contents::body(state),
        Panel::Queries => PanelBody::Editor {
            lines: state.queries.lines().to_vec(),
            cursor: state.queries.cursor(),
        },
    }
}

pub(crate) fn waiting(text: &str) -> PanelBody {
    PanelBody::Notice {
        text: text.to_owned(),
        token: Some(Token::Muted),
    }
}

pub(crate) fn failure(error: &DbError) -> PanelBody {
    PanelBody::Notice {
        text: error.to_string(),
        token: Some(Token::Severity(Severity::Danger)),
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use crate::model::{ConnId, Schema};
    use crate::posture::Posture;
    use crate::state::{Connection, ConnectionState, SchemaState, State};
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
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema),
        }))
    }

    /// A loaded state whose keyspaces have no tables.
    pub(crate) fn loaded(names: &[&str]) -> State {
        connected(tabled_schema(
            &names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>(),
        ))
    }

    /// A loaded state built from `(keyspace name, its table names)`.
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
    use crate::commands::CommandsPanel;
    use crate::config::{Config, CustomTheme};
    use crate::contents::{Subject, TableRef};
    use crate::export::{ExportForm, Fetched, Format, Scope, Stop};
    use crate::model::{ConnId, DbError, JobId, Page, QueryId};
    use crate::state::{Cursor, ExportJob, Message, Overlay};
    use crate::theme::{Ours, Ported, ThemeName};
    use crate::update::init;

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
                // Nothing is selected, so the panel offers no Columns tab to
                // describe a table with.
                "[1]-Tables",
                "[2]-DDL",
                "[3]-Table contents",
                "[4]-Queries"
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
                lines: vec![String::new()],
                cursor: Position::default(),
            },
            "an untouched buffer is one empty line with the cursor on it"
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
    fn a_panel_carries_the_filter_applied_to_it() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.apply_filter(
            crate::search::Filtered::Contents,
            crate::search::Filter::new("ORD"),
        );

        let view = view_model(&state);

        assert_eq!(view.panels[2].filter.as_deref(), Some("ORD"), "as typed");
        assert_eq!(view.panels[0].filter, None);
        assert_eq!(view.panels[3].filter, None, "Queries holds none");
    }

    #[test]
    fn an_open_prompt_is_projected_as_its_query() {
        let mut prompt = crate::search::Prompt::opened(crate::search::Filtered::Tables, None);
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
                rows: crate::commands::rows(),
                selected: 1,
            })
        );
    }

    #[test]
    fn an_open_picker_is_projected_as_every_theme_and_the_cursors_position() {
        let built_in = BuiltIn::Ported(Ported::CatppuccinMocha);
        let index = BuiltIn::ALL
            .iter()
            .position(|b| *b == built_in)
            .expect("listed");
        let mut cursor = crate::state::Cursor::default();
        cursor.go_to(index, BuiltIn::ALL.len());
        let mut state = State::default();
        state.overlay = Some(Overlay::Themes {
            cursor,
            restored: ThemeChoice::default(),
        });

        assert_eq!(
            view_model(&state).overlay,
            Some(OverlayView::Themes {
                labels: BuiltIn::ALL
                    .iter()
                    .map(|built_in| built_in.label().to_owned())
                    .collect(),
                selected: index,
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
        let (mut state, _) = init(ConnId::next(), config, Vec::new());
        state.overlay = Some(Overlay::Themes {
            cursor: Cursor::default(),
            restored: ThemeChoice::default(),
        });

        let Some(OverlayView::Themes { labels, .. }) = view_model(&state).overlay else {
            panic!("the picker is up");
        };

        assert_eq!(labels.len(), BuiltIn::ALL.len() + 1);
        assert_eq!(labels.last().map(String::as_str), Some("midnight"));
    }

    /// Every `ContentsState`, because that panel's title is built per frame
    /// and is the one that could drift out of the invariant below.
    fn every_contents_state() -> Vec<State> {
        let query = QueryId::next();
        let mut loading = tabled(&[("shop", &["orders"])]);
        loading
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));

        let mut loaded = loading.clone();
        loaded.contents.page_ready(query, Page::default());

        let mut failed = loading.clone();
        failed.contents.failed(query, DbError::Timeout);

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
        assert_eq!(view.title, "Export table shop.orders");
        assert_eq!(view.scope, Scope::Whole);
        assert!(view.fetched_enabled);
        assert_eq!(view.format, Format::Csv);
        assert_eq!(view.path, "/home/ike/shop.orders.csv");
        assert_eq!(view.cursor, 25);
        assert_eq!(view.stop, Stop::Scope);
        assert_eq!(view.confirm, None);
    }

    #[test]
    fn a_running_export_puts_its_count_on_the_bar() {
        let mut state = State::default();
        state.focus_on(Panel::Contents);
        state.export = Some(ExportJob::started(
            JobId::next(),
            PathBuf::from("/home/ike/shop.orders.csv"),
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
