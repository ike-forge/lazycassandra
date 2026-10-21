//! The commit confirmation and the quit warning. Unlike the drop confirmation
//! this scrolls: a summary of a write is not a preview of it.

use lc_domain::model::Key;
use lc_domain::pointer::Press;
use lc_domain::style::{Severity, Token};
use lc_domain::view::CommitView;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::body::Extent;
use crate::drop::Button;
use crate::hit::Modal;
use crate::theme::Theme;

/// Past this a line is longer than the eye tracks back from.
const MAX_WIDTH: u16 = 100;
/// Two columns each side, two rows above and below.
const MARGIN: u16 = 4;
const MIN_WIDTH: u16 = 44;
/// Borders, a blank, one statement line, a blank, the keys.
const MIN_HEIGHT: u16 = 7;
const PAD: u16 = 1;
/// A blank above the body; a blank and the keys row below.
const CHROME: u16 = 3;

/// Sized from the frame alone, so the height bounding the offset never moves
/// with the statement.
fn rect(area: Rect) -> Option<Rect> {
    let width = area.width.saturating_sub(MARGIN).min(MAX_WIDTH);
    let height = area.height.saturating_sub(MARGIN);

    if width < MIN_WIDTH || height < MIN_HEIGHT {
        return None;
    }

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

fn body(block: Rect) -> Rect {
    let inner = block.inner(Margin {
        horizontal: 1 + PAD,
        vertical: 1,
    });

    Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(CHROME),
        ..inner
    }
}

/// Zero makes `ctrl-s` inert.
pub(crate) fn visible_rows(area: Rect) -> usize {
    rect(area).map_or(0, |block| usize::from(body(block).height))
}

pub(crate) fn render(
    frame: &mut Frame,
    view: &CommitView,
    pressed: Option<Press>,
    theme: Theme,
) -> (usize, Option<Modal>) {
    let Some(area) = rect(frame.area()) else {
        return (0, None);
    };
    let inner = body(area);
    let wrapped = wrapped(view, usize::from(inner.width), theme);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(Token::Severity(Severity::Warning)))
        .title(crate::drop::title(&view.title, theme));

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    let offset = u16::try_from(view.offset).unwrap_or(u16::MAX);
    let lines: Vec<Line<'static>> = wrapped
        .iter()
        .map(|line| crate::wrap::line(line.clone(), None, theme))
        .collect();
    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), inner);

    let keys = Rect {
        y: inner.y + inner.height + 1,
        height: 1,
        ..inner
    };
    let buttons = crate::drop::keys(
        &[
            Button::new(Key::Char('y'), "Yes, write them", true),
            Button::new(Key::Char('n'), "Cancel", true),
        ],
        pressed,
        theme,
    );
    frame.render_widget(Paragraph::new(buttons.line()), keys);
    let modal = Modal::new(area, buttons.at(keys));

    let extent = Extent {
        position: view.offset,
        total: wrapped.len(),
        visible: usize::from(inner.height),
    };
    if extent.overflows() {
        crate::render::scrollbar(frame, area, &extent, theme.focus_border(), theme);
    }

    (wrapped.len(), Some(modal))
}

/// A continuation keeps the formatter's indent plus one step.
fn wrapped(view: &CommitView, width: usize, theme: Theme) -> Vec<Vec<(char, Style)>> {
    view.lines
        .iter()
        .flat_map(|line| {
            let chars = crate::wrap::chars_of(line, theme);
            let laid = chars
                .iter()
                .position(|&(character, _)| character != ' ')
                .unwrap_or(chars.len());
            let indent = (laid + 2).min(width.saturating_sub(1));
            let rows = crate::wrap::wrap(&chars[laid..], width, indent);

            if rows.is_empty() {
                vec![Vec::new()]
            } else {
                rows
            }
        })
        .collect()
}

/// Fixed height: nothing to read past.
pub(crate) fn render_discard(
    frame: &mut Frame,
    message: &str,
    pressed: Option<Press>,
    theme: Theme,
) -> Option<Modal> {
    let width = MIN_WIDTH.min(frame.area().width);
    let height = MIN_HEIGHT;
    if frame.area().width < width || frame.area().height < height {
        return None;
    }

    let area = Rect::new(
        (frame.area().width - width) / 2,
        (frame.area().height - height) / 2,
        width,
        height,
    );
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.style(Token::Severity(Severity::Warning)))
        .title("Uncommitted changes");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    let mut lines = vec![Line::default()];
    lines.extend(
        crate::wrap::plain(message, usize::from(inner.width), 0)
            .into_iter()
            .map(Line::from),
    );
    lines.push(Line::default());
    let buttons = crate::drop::keys(
        &[
            Button::new(Key::Char('y'), "Quit and discard", true),
            Button::new(Key::Char('n'), "Stay", true),
        ],
        pressed,
        theme,
    );
    let row = Rect::new(
        inner.x,
        inner.y + u16::try_from(lines.len()).unwrap_or(u16::MAX),
        inner.width,
        1,
    );
    lines.push(buttons.line());

    frame.render_widget(Paragraph::new(lines), inner);

    Some(Modal::new(area, buttons.at(row)))
}

#[cfg(test)]
mod tests {
    use lc_domain::commit::CommitPrompt;
    use lc_domain::contents::TableRef;
    use lc_domain::model::{Statement, UpdatedRow};
    use lc_domain::overlay::Overlay;
    use lc_domain::state::State;
    use lc_domain::view::{OverlayView, view_model};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};

    use super::*;

    fn row(id: &str) -> UpdatedRow {
        (
            vec![("note".to_owned(), "'x'".to_owned())],
            vec![("id".to_owned(), id.to_owned())],
        )
    }

    fn prompt(rows: usize) -> CommitPrompt {
        let rows: Vec<_> = (0..rows).map(|at| row(&at.to_string())).collect();

        CommitPrompt::opened(
            TableRef::new("shop", "orders"),
            Statement::update_rows("shop", "orders", &rows),
            rows.len(),
            rows.len(),
        )
    }

    fn drawn(width: u16, height: u16, prompt: CommitPrompt) -> (Buffer, usize) {
        let mut state = State::default();
        state.overlay = Some(Overlay::Commit(prompt));
        let Some(OverlayView::Commit(view)) = view_model(&state).overlay else {
            panic!("the confirmation projects");
        };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");
        let mut lines = 0;

        terminal
            .draw(|frame| lines = render(frame, &view, None, Theme::default()).0)
            .expect("TestBackend always draws");

        (terminal.backend().buffer().clone(), lines)
    }

    fn rows(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .filter_map(|x| buffer.cell((x, y)))
                    .map(Cell::symbol)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn one_row_draws_the_statement_and_the_two_keys() {
        let (buffer, lines) = drawn(80, 12, prompt(1));
        let drawn = rows(&buffer);

        assert_eq!(lines, 3, "a clause a line, none of them wrapped");
        assert!(
            drawn
                .iter()
                .any(|row| row.contains("Write 1 cell in 1 row of shop.orders")),
            "{drawn:#?}"
        );
        for clause in [
            r#"UPDATE "shop"."orders""#,
            r#"SET "note" = 'x'"#,
            r#"WHERE "id" = 0"#,
        ] {
            assert!(
                drawn.iter().any(|row| row.contains(clause)),
                "{clause}: {drawn:#?}"
            );
        }
        assert!(
            drawn.iter().any(|row| row.contains("Yes, write them")),
            "{drawn:#?}"
        );
    }

    #[test]
    fn a_batch_longer_than_the_body_scrolls_rather_than_eliding() {
        // Every statement line is readable, unlike a drop's elided batch.
        let (buffer, lines) = drawn(80, 14, prompt(30));

        assert_eq!(lines, 92, "three clauses a verb, and both brackets");
        assert!(
            rows(&buffer).iter().any(|row| row.contains('▐')),
            "{:#?}",
            rows(&buffer)
        );
    }

    #[test]
    fn a_frame_too_small_for_the_block_draws_none_and_reports_no_room() {
        assert_eq!(rect(Rect::new(0, 0, 47, 10)), None);
        assert_eq!(visible_rows(Rect::new(0, 0, 47, 10)), 0);
        assert!(visible_rows(Rect::new(0, 0, 48, 11)) > 0);
    }
}
