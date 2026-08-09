//! The command palette: every binding, drawn over the panels.

use lc_domain::commands::CommandRow;
use lc_domain::style::Token;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState};

use crate::body::Extent;
use crate::theme::Theme;

const WIDTH_PERCENT: u16 = 50;
const HEIGHT_PERCENT: u16 = 70;
/// The widest row the catalogue can produce, plus both borders and both pad
/// columns — `Margin { horizontal: PAD }` spends one on each side. Derived
/// from the catalogue rather than chosen — `commands::tests` is what stops a
/// longer action being written
/// (doc/specs/2026-08-09-command-palette-design.md §5).
const MIN_W: u16 = 35;
/// Past this every row is trailing space.
const MAX_W: u16 = 60;
/// Two borders, a heading, and enough commands to be worth opening.
const MIN_H: u16 = 8;
/// Between the left border and the keys column.
const PAD: u16 = 1;
/// Between the keys column and the action column.
const GAP: &str = "  ";

/// Where a palette listing `rows` rows goes in `area`, or `None` when it does
/// not fit.
fn rect(area: Rect, rows: usize) -> Option<Rect> {
    let width = percent(area.width, WIDTH_PERCENT).min(MAX_W);
    let wanted = u16::try_from(rows.saturating_add(2)).unwrap_or(u16::MAX);
    let height = percent(area.height, HEIGHT_PERCENT).min(wanted);

    if width < MIN_W || height < MIN_H {
        return None;
    }

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

/// Rows a palette would show in `area`, whether or not one is open. Zero means
/// it does not fit, and is what gates `?` in the domain.
pub(crate) fn visible_rows(area: Rect, rows: usize) -> usize {
    rect(area, rows).map_or(0, |rect| usize::from(rect.height.saturating_sub(2)))
}

/// Widened to `u32` first: at a terminal width near `u16::MAX` the product
/// would overflow, which panics in debug rather than clamping.
fn percent(value: u16, share: u16) -> u16 {
    u16::try_from(u32::from(value) * u32::from(share) / 100).unwrap_or(u16::MAX)
}

pub(crate) fn render(frame: &mut Frame, rows: &[CommandRow], selected: usize, theme: Theme) {
    let Some(area) = rect(frame.area(), rows.len()) else {
        return;
    };

    let block = Block::bordered().title("Commands");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let gutter = rows.iter().map(keys_width).max().unwrap_or(0);
    let items: Vec<ListItem> = rows.iter().map(|row| item(row, gutter, theme)).collect();
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.style(Token::Selected)),
        inner,
        &mut state,
    );

    let extent = Extent {
        position: state.offset(),
        total: rows.len(),
        visible: usize::from(inner.height),
    };

    if extent.overflows() {
        crate::render::scrollbar(frame, area, &extent, theme);
    }
}

/// The palette says in a column what the bar says in a row, so it says it with
/// the bar's tokens — a `CommandKey` token would be a second name for one
/// meaning (architecture.md §6).
fn item(row: &CommandRow, gutter: usize, theme: Theme) -> ListItem<'static> {
    match row {
        CommandRow::Heading(heading) => ListItem::new(Line::from(Span::styled(
            (*heading).to_owned(),
            theme.style(Token::Muted),
        ))),
        CommandRow::Command { keys, action } => ListItem::new(Line::from(vec![
            Span::styled(format!("{keys:<gutter$}"), theme.style(Token::HintKey)),
            Span::styled(format!("{GAP}{action}"), theme.style(Token::HintAction)),
        ])),
    }
}

/// Columns, on the same single-width assumption the hint bar makes of the
/// labels `hints::name` produces.
fn keys_width(row: &CommandRow) -> usize {
    match row {
        CommandRow::Heading(_) => 0,
        CommandRow::Command { keys, .. } => keys.chars().count(),
    }
}
