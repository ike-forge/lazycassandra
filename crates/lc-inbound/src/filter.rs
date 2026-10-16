//! The filter prompt: a modal, so it cannot be mistaken for the `/` bar prompt.

use lc_domain::style::Token;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr as _;

use crate::theme::Theme;

/// Narrows rather than refusing: it fits the smallest shell frame, so `f` needs
/// no domain-side gate.
const MAX_W: u16 = 44;
/// Two columns each side.
const MARGIN_W: u16 = 4;
/// The keys are named on the bar, as every other mode's are.
const HEIGHT: u16 = 3;

fn centred(area: Rect, height: u16) -> Option<Rect> {
    let width = area.width.saturating_sub(MARGIN_W).min(MAX_W);

    if width < MARGIN_W || area.height < height {
        return None;
    }

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

pub(crate) fn render(frame: &mut Frame, column: &str, query: &str, theme: Theme) {
    let Some(area) = centred(frame.area(), HEIGHT) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.focus_border())
        .title(TextLine::from(vec![
            Span::raw("Filter "),
            Span::styled(column.to_owned(), theme.style(Token::RegularColumn)),
        ]));
    let inner = block.inner(area);

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    // The tail stays on screen, where the caret is.
    let visible = tail(query, usize::from(inner.width).saturating_sub(1));

    frame.render_widget(
        Paragraph::new(TextLine::from(Span::styled(
            visible.clone(),
            theme.style(Token::Selected),
        ))),
        inner,
    );

    let caret = u16::try_from(visible.width()).unwrap_or(inner.width);
    frame.set_cursor_position((
        (inner.x + caret).min(inner.right().saturating_sub(1)),
        inner.y,
    ));
}

/// Cut at a character boundary.
fn tail(query: &str, width: usize) -> String {
    let mut taken: Vec<char> = Vec::new();
    let mut used = 0;

    for character in query.chars().rev() {
        let next = used + unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if next > width {
            break;
        }
        used = next;
        taken.push(character);
    }

    taken.into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_term_that_fits_is_shown_whole() {
        assert_eq!(tail("orders", 10), "orders");
    }

    #[test]
    fn a_term_longer_than_the_field_keeps_the_end_the_caret_is_on() {
        assert_eq!(tail("abcdefgh", 3), "fgh");
    }

    #[test]
    fn a_wide_character_is_cut_whole_rather_than_halved() {
        assert_eq!(tail("字a", 2), "a", "a two-column character needs two");
        assert_eq!(tail("字a", 3), "字a");
    }
}
