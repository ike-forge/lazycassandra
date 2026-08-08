//! The block drawn over the panels when something needs saying.

use lc_domain::state::Overlay;
use lc_domain::style::Token;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use crate::theme::Theme;

const MAX_W: u16 = 60;
/// Below this the block cannot hold a sentence, and the panel underneath is
/// already saying the same thing — so nothing is drawn rather than a truncated
/// box. Same rule the panels follow in `layout`.
const MIN_W: u16 = 20;
const MIN_H: u16 = 5;
/// Two borders and a column of padding on each side.
const CHROME_W: u16 = 4;

pub(crate) fn render(frame: &mut Frame, overlay: &Overlay, theme: Theme) {
    let area = frame.area();
    let Some(rect) = centred(area, overlay) else {
        return;
    };

    let block = Block::bordered()
        .title(overlay.title.as_str())
        .border_style(theme.style(Token::Severity(overlay.severity)));
    let inner = block.inner(rect);

    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(overlay.message.as_str()).wrap(Wrap { trim: true }),
        inner,
    );
}

fn centred(area: Rect, overlay: &Overlay) -> Option<Rect> {
    let width = area.width.saturating_sub(CHROME_W).min(MAX_W);
    if width < MIN_W || area.height < MIN_H {
        return None;
    }

    let lines = wrapped_line_count(&overlay.message, width.saturating_sub(2));
    let height = lines.saturating_add(2).clamp(MIN_H, area.height);

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

/// Greedy word wrap, matching `Wrap { trim: true }` closely enough for
/// sizing: a word joins the current line if it (plus a separating space)
/// fits, otherwise it starts a new one. A word longer than `width` occupies
/// `ceil(len / width)` lines by itself. Counts characters, not bytes, since a
/// CQL error can carry non-ASCII.
fn wrapped_line_count(message: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let mut lines: usize = 0;
    let mut current: usize = 0;

    for word in message.split_whitespace() {
        let word_len = word.chars().count();

        if word_len > width {
            if current > 0 {
                lines += 1;
                current = 0;
            }
            lines += word_len.div_ceil(width);
            continue;
        }

        let needed = if current == 0 {
            word_len
        } else {
            current + 1 + word_len
        };

        if needed > width {
            lines += 1;
            current = word_len;
        } else {
            current = needed;
        }
    }

    if current > 0 || lines == 0 {
        lines += 1;
    }

    u16::try_from(lines).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use lc_domain::state::{ConnectionState, Overlay};
    use lc_domain::style::Severity;
    use lc_domain::{State, view_model};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::Color;

    use crate::render::render;
    use crate::theme::Theme;

    use super::wrapped_line_count;

    fn draw(width: u16, height: u16, overlay: Option<Overlay>) -> Buffer {
        let state = State {
            connection: ConnectionState::Disconnected,
            overlay,
            ..State::default()
        };
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| render(frame, &view_model(&state), Theme::TerminalDefault))
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    fn failure() -> Overlay {
        Overlay {
            title: "Connection failed".to_owned(),
            message: "could not reach the cluster".to_owned(),
            severity: Severity::Danger,
        }
    }

    fn text(buffer: &Buffer) -> String {
        buffer.content().iter().map(Cell::symbol).collect()
    }

    #[test]
    fn a_failure_is_drawn_over_the_panels() {
        let buffer = draw(60, 20, Some(failure()));

        let drawn = text(&buffer);
        assert!(drawn.contains("Connection failed"));
        assert!(drawn.contains("could not reach the cluster"));
    }

    #[test]
    fn the_overlay_border_carries_the_severity() {
        let buffer = draw(60, 20, Some(failure()));

        let red = (0..buffer.area.height).any(|y| {
            (0..buffer.area.width).any(|x| {
                buffer
                    .cell((x, y))
                    .is_some_and(|cell| cell.fg == Color::Red && cell.symbol() == "─")
            })
        });

        assert!(red, "a danger overlay is bordered in red");
    }

    #[test]
    fn the_overlay_is_centred() {
        // At 60x20 the block is 56 wide (the frame less two columns of margin
        // each side, under the 60-column cap) and 5 tall (its floor, since the
        // message wraps to one line). That centres it at (2, 7).
        let buffer = draw(60, 20, Some(failure()));
        let symbol = |x, y| buffer.cell((x, y)).expect("in bounds").symbol().to_owned();

        assert_eq!(symbol(2, 7), "┌");
        assert_eq!(symbol(57, 7), "┐");
        assert_eq!(symbol(2, 11), "└");
        assert_eq!(symbol(57, 11), "┘");
    }

    #[test]
    fn a_frame_too_small_for_a_legible_overlay_does_not_get_one() {
        let buffer = draw(18, 4, Some(failure()));

        assert!(
            !text(&buffer).contains("Connection failed"),
            "a truncated error block is worse than none; the panel still says it"
        );
    }

    #[test]
    fn no_overlay_leaves_the_panels_alone() {
        let with = draw(60, 20, Some(failure()));
        let without = draw(60, 20, None);

        assert_ne!(text(&with), text(&without));
        assert!(!text(&without).contains("Connection failed"));
    }

    #[test]
    fn a_message_shorter_than_the_width_is_one_line() {
        assert_eq!(wrapped_line_count("could not reach the cluster", 54), 1);
    }

    #[test]
    fn a_message_that_wraps_evenly_is_exactly_two_lines() {
        // "aaaaa bbbbb" is 11 characters, exactly the width; "ccccc" cannot
        // join it and starts a second line.
        assert_eq!(wrapped_line_count("aaaaa bbbbb ccccc", 11), 2);
    }

    #[test]
    fn a_single_word_longer_than_the_width_spans_ceil_of_its_length_over_the_width() {
        // 34 characters at a width of 10 needs ceil(34 / 10) = 4 lines.
        assert_eq!(
            wrapped_line_count("supercalifragilisticexpialidocious", 10),
            4
        );
    }

    #[test]
    fn an_empty_or_whitespace_only_message_is_one_line_never_zero() {
        assert_eq!(wrapped_line_count("", 20), 1);
        assert_eq!(wrapped_line_count("   ", 20), 1);
    }
}
