//! A line of feedback in the top-right corner, over the panels.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use lc_domain::style::{Severity, Token};

use crate::theme::Theme;

/// Cells a toast keeps clear of the frame's edge.
const MARGIN: u16 = 1;

fn rect(area: Rect, message: &str) -> Option<Rect> {
    let width = u16::try_from(message.chars().count() + 4).ok()?;
    let width = width.min(area.width.saturating_sub(2 * MARGIN));

    if width < 5 || area.height < 3 + MARGIN {
        return None;
    }

    Some(Rect::new(
        area.right().saturating_sub(width + MARGIN),
        area.y + MARGIN,
        width,
        3,
    ))
}

pub(crate) fn render(frame: &mut Frame, severity: Severity, message: &str, theme: Theme) {
    let Some(area) = rect(frame.area(), message) else {
        return;
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(Token::Severity(severity)));
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(Line::raw(message)), inner);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use lc_domain::theme::ThemeChoice;

    use super::*;

    fn drawn(width: u16, height: u16, message: &str) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
        terminal
            .draw(|frame| {
                render(
                    frame,
                    Severity::Info,
                    message,
                    Theme::of(&ThemeChoice::default()),
                );
            })
            .expect("draw");

        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[test]
    fn the_message_is_drawn() {
        assert!(drawn(40, 8, "history is disabled").contains("history is disabled"));
    }

    #[test]
    fn a_frame_too_small_to_hold_it_draws_nothing() {
        assert!(drawn(5, 8, "history is disabled").trim().is_empty());
        assert!(drawn(40, 3, "history is disabled").trim().is_empty());
    }
}
