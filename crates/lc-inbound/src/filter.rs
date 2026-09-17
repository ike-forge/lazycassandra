//! The filter prompt, drawn over the panels
//! (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §3).
//!
//! A modal rather than a bar-row prompt, which is what tells it apart from
//! `/`: a search marks cells and keeps every row, and this drops the rows it
//! does not match. Two things that differ that much may not read alike.

use lc_domain::style::Token;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr as _;

use crate::theme::Theme;

/// As wide as the box wants to be, and no wider than the frame it sits in.
///
/// Narrowing rather than refusing, unlike the palette and the export form:
/// those two are gated on a measurement that crosses back into the domain
/// because a form nothing draws would be a mode with nothing on screen saying
/// so. This box needs no such gate — it fits inside the smallest frame the
/// shell draws panels in at all (`layout::MIN_W` × `MIN_H`), so `f` can never
/// open a prompt the frame cannot show
/// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §3).
const MAX_W: u16 = 44;
/// Two columns of margin on each side, between the frame edge and the block.
const MARGIN_W: u16 = 4;
/// Two borders and one field. Nothing else belongs in a box that collects one
/// word — the two keys that leave it are named on the bar, where every other
/// mode's keys are named too.
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

/// The block, titled with the column it will narrow, holding the term typed
/// so far and a caret after it.
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

    // The tail rather than the head: the caret sits after the last character
    // typed, and a term longer than the field has to keep that end on screen
    // — the window the export form's path field maintains, for one line that
    // never scrolls back (export design §2).
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

/// The last `width` columns of `query`, cut at a character boundary.
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
