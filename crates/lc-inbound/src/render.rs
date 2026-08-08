//! Drawing a frame.

use lc_domain::style::Token;
use lc_domain::view::ViewModel;
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

        frame.render_widget(block, area);
    }
}

#[cfg(test)]
mod tests {
    use lc_domain::State;
    use lc_domain::view::view_model;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::Modifier;

    use super::*;

    fn draw(width: u16, height: u16) -> Buffer {
        let state = State::default();
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| render(frame, &view_model(&state), Theme::TerminalDefault))
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
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
        assert_eq!(row(&buffer, 1), "│              ││                      │");
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
    fn a_collapsed_panel_is_not_drawn_at_all() {
        // Five rows leaves no room for a second panel in either column.
        let buffer = draw(40, 5);
        let screen: String = (0..5).map(|y| row(&buffer, y)).collect();

        assert!(screen.contains("Tables"));
        assert!(screen.contains("Table contents"));
        assert!(!screen.contains("Table DDL"), "Ddl collapsed to zero rows");
        assert!(
            !screen.contains("Queries"),
            "Queries collapsed to zero rows"
        );
    }
}
