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
pub use details::{DetailRow, Details, Group, Section};
pub use hints::{Binding, Hint, HintBar, Liveness};
pub use tables::{Marker, TreeRow};
pub use title::PanelTitle;

pub use crate::editor::Position;

use crate::commands::{self, CommandRow};
use crate::model::DbError;
use crate::state::{Message, Overlay, Panel, State};
use crate::style::{Severity, Token};

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
}

/// What the loader has to say.
///
/// An enum rather than a bare frame number the domain promises to zero: which
/// of the two states the tool is in is the decision `update` is making, and
/// the one `lc-app` reads to know whether a tick is worth anything
/// (doc/specs/2026-08-09-visual-polish-design.md §4.2).
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
}

fn overlay(state: &State) -> Option<OverlayView> {
    Some(match state.overlay.as_ref()? {
        Overlay::Message(message) => OverlayView::Message(message.clone()),
        Overlay::Commands(panel) => OverlayView::Commands {
            rows: commands::rows(),
            selected: panel.selected(),
        },
    })
}

/// One panel, described without reference to how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub panel: Panel,
    pub title: PanelTitle,
    pub body: PanelBody,
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
        /// (doc/specs/2026-08-09-list-viewport-design.md §2).
        offset: usize,
    },
    /// A heading and grouped rows: what is selected, described.
    ///
    /// `offset` is the first line drawn, counted in wrapped lines. Only the
    /// renderer can produce those; the domain scrolls by them on the strength
    /// of the count `Msg::Measured` brings back (design §2).
    Details {
        details: Details,
        offset: usize,
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
    ///
    /// Carries no widths. Sizing columns needs to know how wide the panel is,
    /// which is the renderer's business for the same reason scrolling is.
    Grid {
        columns: Vec<GridColumn>,
        rows: Vec<Vec<GridCell>>,
        selected: usize,
        /// The first row drawn, as [`PanelBody::Tree::offset`].
        offset: usize,
    },
}

/// Project `state` into the frame a renderer draws.
#[must_use]
pub fn view_model(state: &State) -> ViewModel {
    ViewModel {
        panels: Panel::ALL.map(|panel| PanelView {
            panel,
            title: PanelTitle::marking(title(state, panel), panel.shortcut()),
            body: body(state, panel),
        }),
        focus: state.focus,
        hints: hints::bar(state),
        overlay: overlay(state),
        activity: if state.is_working() {
            Activity::Working {
                frame: state.loader.frame(),
            }
        } else {
            Activity::Resting
        },
    }
}

fn title(state: &State, panel: Panel) -> String {
    match panel {
        Panel::Tables => "Tables".to_owned(),
        Panel::Ddl => "Table DDL".to_owned(),
        Panel::Contents => contents::title(state),
        Panel::Queries => "Queries".to_owned(),
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
        State {
            connection,
            ..State::default()
        }
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
    use super::tests_support::{loaded, tabled};
    use super::*;
    use crate::commands::CommandsPanel;
    use crate::contents::{Subject, TableRef};
    use crate::model::{DbError, Page, QueryId};
    use crate::state::{Message, Overlay};

    #[test]
    fn every_panel_is_projected_in_reading_order() {
        let view = view_model(&State::default());

        assert_eq!(view.panels.map(|panel| panel.panel), Panel::ALL);
    }

    #[test]
    fn panels_carry_the_titles_the_draft_names() {
        let view = view_model(&State::default());

        assert_eq!(
            view.panels.map(|panel| panel.title.text().to_owned()),
            ["Tables", "Table DDL", "Table contents", "Queries"].map(str::to_owned)
        );
    }

    #[test]
    fn focus_mirrors_the_state() {
        let state = State {
            focus: Panel::Contents,
            ..State::default()
        };

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
        let state = State {
            overlay: Some(Overlay::Message(message.clone())),
            ..State::default()
        };

        assert_eq!(
            view_model(&state).overlay,
            Some(OverlayView::Message(message))
        );
    }

    #[test]
    fn an_open_palette_is_projected_as_rows_and_a_selection() {
        let state = State {
            overlay: Some(Overlay::Commands(CommandsPanel::opened())),
            ..State::default()
        };

        assert_eq!(
            view_model(&state).overlay,
            Some(OverlayView::Commands {
                rows: crate::commands::rows(),
                selected: 1,
            })
        );
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
    fn every_panel_title_marks_the_key_that_focuses_it() {
        for state in every_contents_state() {
            for panel in view_model(&state).panels {
                let (_, mark, _) = panel.title.split();

                assert!(
                    mark.eq_ignore_ascii_case(&panel.panel.shortcut().to_string()),
                    "{:?} titled {:?} does not mark '{}'",
                    panel.panel,
                    panel.title.text(),
                    panel.panel.shortcut()
                );
            }
        }
    }
}
