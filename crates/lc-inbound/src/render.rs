//! Drawing a frame.

use lc_domain::ViewModel;
use lc_domain::style::Token;
use ratatui::Frame;
use ratatui::widgets::Block;

use crate::layout::layout;
use crate::theme::Theme;

/// Draw one titled block per panel, skipping any the terminal is too small to
/// hold.
pub fn render(frame: &mut Frame, view: &ViewModel, theme: Theme) {
    let rects = layout(frame.area());

    for panel in &view.panels {
        let area = rects.get(panel.panel);
        if area.is_empty() {
            continue;
        }

        let mut block = Block::bordered().title(panel.title.as_str());
        if panel.panel == view.focus {
            block = block.border_style(theme.style(Token::Focused));
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);
        crate::body::render(frame, inner, &panel.body, theme);
    }

    if let Some(overlay) = &view.overlay {
        crate::overlay::render(frame, overlay, theme);
    }
}

#[cfg(test)]
mod tests {
    use lc_domain::model::{ConnId, Keyspace, Replication, Schema};
    use lc_domain::posture::Posture;
    use lc_domain::state::{Connection, ConnectionState, Panel, SchemaState};
    use lc_domain::{State, view_model};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::Modifier;
    use std::collections::BTreeMap;

    use super::*;

    fn draw_focused(width: u16, height: u16, focus: Panel) -> Buffer {
        let state = State {
            focus,
            ..State::default()
        };
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| render(frame, &view_model(&state), Theme::TerminalDefault))
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    fn draw(width: u16, height: u16) -> Buffer {
        draw_focused(width, height, Panel::default())
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .filter_map(|x| buffer.cell((x, y)))
            .map(Cell::symbol)
            .collect()
    }

    #[test]
    fn the_four_panels_are_drawn_where_the_draft_puts_them() {
        // At 40x12 the sidebar sits on its 16-column floor, Tables takes 8
        // rows to Ddl's 4, and Queries sits on its 3-row floor.
        let buffer = draw(40, 12);

        assert_eq!(row(&buffer, 0), "┌Tables────────┐┌Table contents────────┐");
        assert_eq!(row(&buffer, 1), "│Not connected ││                      │");
        assert_eq!(row(&buffer, 7), "└──────────────┘│                      │");
        assert_eq!(row(&buffer, 8), "┌Table DDL─────┐└──────────────────────┘");
        assert_eq!(row(&buffer, 9), "│              │┌Queries───────────────┐");
        assert_eq!(row(&buffer, 11), "└──────────────┘└──────────────────────┘");
    }

    #[test]
    fn only_the_focused_panel_is_bold() {
        let buffer = draw(40, 12);
        let bold = |x, y| {
            buffer
                .cell((x, y))
                .expect("corner is in bounds")
                .modifier
                .contains(Modifier::BOLD)
        };

        // Top-left corner of each panel; Tables is focused by default.
        assert!(bold(0, 0), "Tables is focused");
        assert!(!bold(16, 0), "Contents is not");
        assert!(!bold(0, 8), "Ddl is not");
        assert!(!bold(16, 9), "Queries is not");
    }

    #[test]
    fn focus_follows_the_state_rather_than_the_draw_order() {
        // Contents is drawn third, so bolding it cannot be positional luck.
        let buffer = draw_focused(40, 12, Panel::Contents);
        let bold = |x, y| {
            buffer
                .cell((x, y))
                .expect("corner is in bounds")
                .modifier
                .contains(Modifier::BOLD)
        };

        assert!(bold(16, 0), "Contents is focused");
        assert!(!bold(0, 0), "Tables is not");
        assert!(!bold(0, 8), "Ddl is not");
        assert!(!bold(16, 9), "Queries is not");
    }

    #[test]
    fn a_collapsed_panel_is_not_drawn_at_all() {
        // Five rows leaves no room for a second panel in either column.
        let buffer = draw(40, 5);
        let contains = |needle: &str| (0..5).any(|y| row(&buffer, y).contains(needle));

        assert!(contains("Tables"));
        assert!(contains("Table contents"));
        assert!(!contains("Table DDL"), "Ddl collapsed to zero rows");
        assert!(!contains("Queries"), "Queries collapsed to zero rows");
    }

    fn schema_of(names: &[&str]) -> Schema {
        Schema {
            keyspaces: names
                .iter()
                .map(|name| {
                    (
                        (*name).to_owned(),
                        Keyspace {
                            name: (*name).to_owned(),
                            replication: Replication::SimpleStrategy { factor: 1 },
                            tables: BTreeMap::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn browsing(names: &[&str], selected: usize) -> State {
        let mut state = State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loaded(schema_of(names)),
            }),
            ..State::default()
        };
        for _ in 0..selected {
            state.tables.cursor.down(names.len());
        }
        state
    }

    fn draw_state(width: u16, height: u16, state: &State) -> Buffer {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| render(frame, &view_model(state), Theme::TerminalDefault))
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    #[test]
    fn keyspaces_are_listed_inside_the_tables_panel() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 0));

        assert_eq!(row(&buffer, 1), "│▸ shop        ││                      │");
        assert_eq!(row(&buffer, 2), "│▸ typezoo     ││                      │");
        assert_eq!(row(&buffer, 3), "│▸ system      ││                      │");
    }

    #[test]
    fn the_clusters_own_keyspaces_are_dimmed() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "system"], 0));
        let dim = |y: u16| {
            buffer
                .cell((1, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::DIM)
        };

        assert!(!dim(1), "shop is the user's");
        assert!(dim(2), "system is the cluster's");
    }

    #[test]
    fn the_selected_row_is_the_one_the_cursor_is_on() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 1));
        let reversed = |y: u16| {
            buffer
                .cell((1, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::REVERSED)
        };

        assert!(!reversed(1));
        assert!(reversed(2), "typezoo is selected");
        assert!(!reversed(3));
    }

    #[test]
    fn a_panel_with_nothing_to_list_says_what_it_is_waiting_for() {
        let state = State {
            connection: ConnectionState::Connecting(ConnId::next()),
            ..State::default()
        };

        let buffer = draw_state(40, 12, &state);

        assert_eq!(row(&buffer, 1), "│Connecting…   ││                      │");
    }
}
