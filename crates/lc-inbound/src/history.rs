//! The query history: the statements a connection has been asked, one line
//! each, drawn over the panels.
//!
//! Unlike the pickers it scrolls: up to `history.limit` rows is far more than
//! a frame holds, so the block is as tall as the frame allows and the list
//! follows the cursor inside it.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState};

use lc_domain::search::Search;
use lc_domain::style::Token;

use crate::theme::Theme;

const MIN_WIDTH: u16 = 30;
const MAX_WIDTH: u16 = 100;
const PAD: u16 = 1;
/// What a row cut short ends in.
const ELLIPSIS: char = '…';

/// Rows the block could show in `area`. Zero means not even its borders fit,
/// which is what gates `ctrl-g` in the domain.
pub(crate) fn visible_rows(area: Rect) -> usize {
    if area.width < MIN_WIDTH || area.height < 3 {
        return 0;
    }

    usize::from(area.height - 2)
}

/// Where a block listing `rows` goes in `area`: as wide as the frame allows up
/// to [`MAX_WIDTH`], as tall as the list wants up to the frame.
fn rect(area: Rect, rows: usize) -> Option<Rect> {
    let visible = visible_rows(area);

    if visible == 0 {
        return None;
    }

    let height = u16::try_from(rows.clamp(1, visible) + 2).unwrap_or(u16::MAX);
    let width = area.width.saturating_sub(4).clamp(MIN_WIDTH, MAX_WIDTH);

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

/// `label` cut to `width` cells, so a long statement shows its start and a
/// wrapped row never takes two lines.
fn clipped(label: &str, width: usize) -> String {
    if label.chars().count() <= width {
        return label.to_owned();
    }

    let mut cut: String = label.chars().take(width.saturating_sub(1)).collect();
    cut.push(ELLIPSIS);
    cut
}

/// `labels` is what a live sift left, newest first; `selected` counts in it.
pub(crate) fn render(
    frame: &mut Frame,
    labels: &[String],
    selected: usize,
    search: Option<&Search>,
    theme: Theme,
) {
    let Some(area) = rect(frame.area(), labels.len()) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.focus_border())
        .title("History");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    if labels.is_empty() {
        frame.render_widget(
            List::new([ListItem::new(Line::from(Span::styled(
                "no statement matches",
                theme.style(Token::Faint),
            )))]),
            inner,
        );
        return;
    }

    let width = usize::from(inner.width);
    let items: Vec<ListItem> = labels
        .iter()
        .map(|label| {
            ListItem::new(Line::from(crate::highlight::spans(
                &clipped(label, width),
                search,
                Style::default(),
                theme,
            )))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.style(Token::Selected)),
        inner,
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use lc_domain::ThemeChoice;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Cell;

    use super::*;

    fn drawn(width: u16, height: u16, labels: &[String], selected: usize) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");
        let theme = Theme::of(&ThemeChoice::default());

        terminal
            .draw(|frame| render(frame, labels, selected, None, theme))
            .expect("a frame");

        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(Cell::symbol)
            .collect()
    }

    fn numbered(count: usize) -> Vec<String> {
        (0..count).map(|n| format!("SELECT {n} FROM t;")).collect()
    }

    #[test]
    fn the_block_lists_the_rows_under_its_title() {
        let frame = drawn(80, 24, &numbered(3), 0);

        assert!(frame.contains("History"));
        for n in 0..3 {
            assert!(frame.contains(&format!("SELECT {n} FROM t;")), "row {n}");
        }
    }

    #[test]
    fn a_list_longer_than_the_frame_scrolls_to_the_cursor() {
        let frame = drawn(80, 12, &numbered(100), 80);

        assert!(frame.contains("SELECT 80 FROM t;"));
        assert!(!frame.contains("SELECT 0 FROM t;"));
    }

    #[test]
    fn a_statement_wider_than_the_block_is_cut_with_an_ellipsis() {
        let long = format!("SELECT {} FROM t;", "column, ".repeat(40));
        let frame = drawn(60, 12, &[long], 0);

        assert!(frame.contains('…'));
        assert!(!frame.contains("FROM t;"));
    }

    #[test]
    fn a_filter_that_matched_nothing_says_so() {
        let frame = drawn(80, 24, &[], 0);

        assert!(frame.contains("no statement matches"));
    }

    #[test]
    fn a_frame_too_small_to_hold_it_draws_nothing() {
        let frame = drawn(MIN_WIDTH - 1, 24, &numbered(3), 0);

        assert!(!frame.contains("History"));
    }

    #[test]
    fn visible_rows_is_the_frame_less_the_borders() {
        assert_eq!(visible_rows(Rect::new(0, 0, 80, 24)), 22);
        assert_eq!(visible_rows(Rect::new(0, 0, 80, 2)), 0);
    }
}
