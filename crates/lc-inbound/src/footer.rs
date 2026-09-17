//! Table contents' bottom border: how much of a result is loaded
//! (docs/superpowers/specs/2026-09-17-grid-paging-design.md §5).

use lc_domain::style::Token;
use lc_domain::view::Footer;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::theme::Theme;

const SEPARATOR: &str = " · ";

/// Draws `footer` from the left corner of `block`'s bottom border, short of
/// `limit`, and answers with the first column it left free. The estimate is
/// dropped first and the whole footer next where the border cannot hold them.
pub(crate) fn render(
    frame: &mut Frame,
    block: Rect,
    limit: u16,
    footer: &Footer,
    theme: Theme,
) -> u16 {
    let start = block.x.saturating_add(1 + crate::loader::INSET);
    let room = usize::from(limit.saturating_sub(start));
    let fitting = [line(footer, true, theme), line(footer, false, theme)]
        .into_iter()
        .find(|line| line.width() <= room);

    let Some(line) = fitting else {
        return block.x.saturating_add(1);
    };
    let width = u16::try_from(line.width()).unwrap_or(u16::MAX);
    frame.render_widget(
        line,
        Rect {
            x: start,
            y: block.bottom().saturating_sub(1),
            width,
            height: 1,
        },
    );

    start.saturating_add(width)
}

fn line(footer: &Footer, sized: bool, theme: Theme) -> Line<'static> {
    let muted = theme.style(Token::Muted);
    let size = footer.size.as_ref().filter(|_| sized).map(|size| {
        Span::styled(
            size.text.clone(),
            size.token.map_or(muted, |token| theme.style(token)),
        )
    });
    let parts = std::iter::once(Span::styled(footer.rows.clone(), muted))
        .chain(size)
        .chain(
            footer
                .status
                .clone()
                .map(|status| Span::styled(status, muted)),
        );

    let mut spans = vec![Span::styled(" ", muted)];
    for (at, part) in parts.enumerate() {
        if at > 0 {
            spans.push(Span::styled(SEPARATOR, muted));
        }
        spans.push(part);
    }
    spans.push(Span::styled(" ", muted));

    Line::from(spans)
}
