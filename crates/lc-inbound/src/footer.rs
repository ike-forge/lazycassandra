//! Table contents' bottom border: how much of a result is loaded
//! (docs/superpowers/specs/2026-09-17-grid-paging-design.md §5).

use lc_domain::style::{Severity, Token};
use lc_domain::view::Footer;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use crate::theme::Theme;

const SEPARATOR: &str = " · ";

/// Draws `footer` from the left corner of `block`'s bottom border, short of
/// `limit`, and answers with the first column it left free. Where the border
/// cannot hold it all, the hint goes first, the estimate next, then the footer.
pub(crate) fn render(
    frame: &mut Frame,
    block: Rect,
    limit: u16,
    footer: &Footer,
    theme: Theme,
) -> u16 {
    let start = block.x.saturating_add(1 + crate::loader::INSET);
    let room = usize::from(limit.saturating_sub(start));
    let fitting = [
        line(footer, true, true, theme),
        line(footer, true, false, theme),
        line(footer, false, false, theme),
    ]
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

fn line(footer: &Footer, sized: bool, hinted: bool, theme: Theme) -> Line<'static> {
    let muted = theme.style(Token::Muted);
    let size = footer.size.as_ref().filter(|_| sized).map(|size| {
        Span::styled(
            size.text.clone(),
            size.token.map_or(muted, |token| theme.style(token)),
        )
    });
    // The warning colour the dirty cells are grounded in, so the border and
    // the grid say the same thing about them. As text, not as a ground: a
    // count in the border names cells elsewhere rather than being one
    // (cell editing design §7).
    let dirty = footer
        .dirty
        .clone()
        .map(|dirty| Span::styled(dirty, theme.style(Token::Severity(Severity::Warning))));
    let parts = std::iter::once(Span::styled(footer.rows.clone(), muted))
        .chain(size)
        .chain(dirty)
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
    if let Some(hint) = footer.hint.as_ref().filter(|_| hinted) {
        spans.push(Span::styled(format!(" — {hint}"), muted));
    }
    spans.push(Span::styled(" ", muted));

    Line::from(spans)
}
