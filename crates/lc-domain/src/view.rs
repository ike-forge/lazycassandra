//! The domain's projection of itself for a renderer.
//!
//! architecture.md §3. One tree is rebuilt per frame; §11's question about
//! per-panel view models with dirty tracking stays open until panels carry
//! enough content to measure.

use crate::state::{Panel, State};

/// Everything a renderer needs for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewModel {
    /// In [`Panel::ALL`] order.
    pub panels: [PanelView; 4],
    pub focus: Panel,
}

/// One panel, described without reference to how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub panel: Panel,
    pub title: String,
}

/// Project `state` into the frame a renderer draws.
#[must_use]
pub fn view_model(state: &State) -> ViewModel {
    ViewModel {
        panels: Panel::ALL.map(|panel| PanelView {
            panel,
            title: title(panel).to_owned(),
        }),
        focus: state.focus,
    }
}

const fn title(panel: Panel) -> &'static str {
    match panel {
        Panel::Tables => "Tables",
        Panel::Ddl => "Table DDL",
        Panel::Contents => "Table contents",
        Panel::Queries => "Queries",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_panel_is_projected_in_reading_order() {
        let view = view_model(&State::default());

        assert_eq!(view.panels.map(|panel| panel.panel), Panel::ALL);
    }

    #[test]
    fn panels_carry_the_titles_the_draft_names() {
        let view = view_model(&State::default());

        assert_eq!(
            view.panels.map(|panel| panel.title),
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
}
