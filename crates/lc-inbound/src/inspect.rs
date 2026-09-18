//! The cell inspector, drawn over the panels
//! (docs/superpowers/specs/2026-09-17-cell-inspector-design.md §3, §5).
//!
//! Wrapped here, because wrapping needs a width; the line count goes back to
//! the domain through `Geometry`, the way the Logs tab's does.

use lc_domain::state::Span;
use lc_domain::style::Token;
use lc_domain::view::{InspectBody, InspectValue, InspectView};
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span as TextSpan};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr as _;

use crate::body::{Extent, patched};
use crate::theme::Theme;

/// A wide block reads worse than a tall one: past this a line of prose is
/// longer than an eye tracks back from.
const MAX_WIDTH: u16 = 100;
/// Two columns of frame each side, and two rows above and below.
const MARGIN: u16 = 4;
const MIN_WIDTH: u16 = 20;
/// The border and three rows of body.
const MIN_HEIGHT: u16 = 5;
/// Between each border and the text.
const PAD: u16 = 1;
/// Between a row's name column and its values.
const GAP: usize = 2;

/// The block's size is a function of the frame's alone, so the height the
/// domain bounds an offset by never moves with the value (design §5).
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
    block.inner(Margin {
        horizontal: 1 + PAD,
        vertical: 1,
    })
}

/// Rows of body the block would show in `area`; zero where it does not fit,
/// which is what makes `Enter` inert there.
pub(crate) fn visible_rows(area: Rect) -> usize {
    rect(area).map_or(0, |block| usize::from(body(block).height))
}

/// Draws the block and answers with the lines its value wrapped to, and the
/// span the cursor's line landed at.
pub(crate) fn render(frame: &mut Frame, view: &InspectView, theme: Theme) -> (usize, Span) {
    let Some(area) = rect(frame.area()) else {
        return (0, Span::default());
    };
    let inner = body(area);
    let (lines, selection) = lay_out(&view.body, view.cursor, usize::from(inner.width), theme);
    let total = lines.len();

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.focus_border())
        .title(crate::drop::title(&view.title, theme));
    if let Some(size) = &view.size {
        block = block.title_bottom(
            Line::from(TextSpan::styled(
                format!(" {size} "),
                theme.style(Token::Muted),
            ))
            .right_aligned(),
        );
    }

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    let offset = u16::try_from(view.offset).unwrap_or(u16::MAX);
    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), inner);

    let extent = Extent {
        position: view.offset,
        total,
        visible: usize::from(inner.height),
    };
    if extent.overflows() {
        crate::render::scrollbar(frame, area, &extent, theme.focus_border(), theme);
    }

    (total, selection)
}

fn lay_out(
    body: &InspectBody,
    cursor: usize,
    width: usize,
    theme: Theme,
) -> (Vec<Line<'static>>, Span) {
    let mut laid = Laid::default();

    match body {
        InspectBody::Cell(value) => {
            let style = value_style(value, theme);

            for (at, rows) in wrapped(value, width, 0).into_iter().enumerate() {
                let rows = rows
                    .into_iter()
                    .map(|row| Line::from(TextSpan::styled(row, style)))
                    .collect();

                laid.push(rows, at == cursor, theme);
            }
        }
        InspectBody::Row { names, fields } => {
            let names = usize::from(*names);
            let hang = names + GAP;
            let mut at = 0;

            for field in fields {
                let style = value_style(&field.value, theme);
                let name = field
                    .token
                    .map_or_else(|| theme.style(Token::Muted), |token| theme.style(token));

                for (line, rows) in wrapped(&field.value, width, hang).into_iter().enumerate() {
                    let rows = rows
                        .into_iter()
                        .enumerate()
                        .map(|(row, text)| {
                            let rest: String = text.chars().skip(hang).collect();
                            let label = if (line, row) == (0, 0) {
                                let pad = names.saturating_sub(field.name.width());
                                format!("{}{}", field.name, " ".repeat(pad + GAP))
                            } else {
                                " ".repeat(hang)
                            };

                            Line::from(vec![
                                TextSpan::styled(label, name),
                                TextSpan::styled(rest, style),
                            ])
                        })
                        .collect();

                    laid.push(rows, at + line == cursor, theme);
                }

                at += field.value.lines.len();
            }
        }
    }

    (laid.lines, laid.selection)
}

/// The block's body as it accumulates, one laid-out line at a time.
#[derive(Default)]
struct Laid {
    lines: Vec<Line<'static>>,
    selection: Span,
}

impl Laid {
    /// One laid-out line's wrapped rows, banded across the body where the
    /// cursor is on it — every row of it, the way a wrapped Logs entry is
    /// banded whole.
    fn push(&mut self, rows: Vec<Line<'static>>, selected: bool, theme: Theme) {
        if selected {
            self.selection = Span {
                first: self.lines.len(),
                lines: rows.len(),
            };
            let band = theme.style(Token::Selected);
            self.lines
                .extend(rows.into_iter().map(|row| patched(row, band)));
        } else {
            self.lines.extend(rows);
        }
    }
}

fn value_style(value: &InspectValue, theme: Theme) -> Style {
    value
        .token
        .map_or_else(Style::default, |token| theme.style(token))
}

/// Every line of `value` wrapped to `width`, grouped by the line it came
/// from — what the cursor counts in — each opening with `hang` blanks and
/// then its own indent, which a continuation keeps.
///
/// A blank line still takes a row: an empty text value is a field, and a
/// field with no row would lose its name.
fn wrapped(value: &InspectValue, width: usize, hang: usize) -> Vec<Vec<String>> {
    value
        .lines
        .iter()
        .map(|line| {
            let text = line.trim_start_matches(' ');
            let indent = (hang + line.len() - text.len()).min(width.saturating_sub(1));
            let rows = crate::wrap::plain(text, width, indent);

            if rows.is_empty() {
                vec![" ".repeat(hang.min(width))]
            } else {
                rows
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use lc_domain::contents::{Subject, TableRef};
    use lc_domain::inspect::{Inspected, Inspector};
    use lc_domain::model::CqlValue;
    use lc_domain::state::{Overlay, State};
    use lc_domain::view::{OverlayView, view_model};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};

    use super::*;

    fn drawn(width: u16, height: u16, inspected: Inspected) -> (Buffer, usize) {
        let (buffer, lines, _) = drawn_at(width, height, inspected, 0);

        (buffer, lines)
    }

    /// The block as it draws with the cursor `cursor` lines down.
    fn drawn_at(
        width: u16,
        height: u16,
        inspected: Inspected,
        cursor: usize,
    ) -> (Buffer, usize, Span) {
        let mut state = State::default();
        let mut inspector =
            Inspector::opened(Subject::Table(TableRef::new("shop", "orders")), inspected);
        for _ in 0..cursor {
            inspector.down();
        }
        state.overlay = Some(Overlay::Inspect(inspector));
        let Some(OverlayView::Inspect(view)) = view_model(&state).overlay else {
            panic!("the inspector projects");
        };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a backend");
        let mut drawn = (0, Span::default());

        terminal
            .draw(|frame| drawn = render(frame, &view, Theme::default()))
            .expect("TestBackend always draws");

        (terminal.backend().buffer().clone(), drawn.0, drawn.1)
    }

    /// The rows of `buffer` the selection band was patched onto.
    fn banded(buffer: &Buffer) -> Vec<u16> {
        let band = Theme::default().style(Token::Selected);

        (0..buffer.area.height)
            .filter(|&y| {
                buffer
                    .cell((buffer.area.width / 2, y))
                    .is_some_and(|cell| cell.style().bg == band.bg)
            })
            .collect()
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

    fn cell(value: CqlValue) -> Inspected {
        Inspected::Cell {
            column: "spec".to_owned(),
            value,
        }
    }

    #[test]
    fn a_cell_is_drawn_laid_out_under_its_title() {
        let (buffer, lines) = drawn(
            40,
            12,
            cell(CqlValue::Text(r#"{"a":1,"b":[true]}"#.to_owned())),
        );

        assert_eq!(
            rows(&buffer)[2..10],
            [
                "  ╭shop.orders · spec────────────────╮  ",
                "  │ {                                │  ",
                "  │   \"a\": 1,                        │  ",
                "  │   \"b\": [                         │  ",
                "  │     true                         │  ",
                "  │   ]                              │  ",
                "  │ }                                │  ",
                "  ╰──────────────────────────── 18 B ╯  ",
            ]
            .map(str::to_owned)
        );
        assert_eq!(lines, 6);
    }

    #[test]
    fn a_row_is_drawn_one_field_per_column() {
        let (buffer, _) = drawn(
            40,
            12,
            Inspected::Row {
                number: 4,
                columns: vec!["id".to_owned(), "tags".to_owned()],
                values: vec![
                    CqlValue::Int(7),
                    CqlValue::List(vec![CqlValue::Text("x".to_owned())]),
                ],
            },
        );

        assert_eq!(
            rows(&buffer)[2..9],
            [
                "  ╭shop.orders · row 4───────────────╮  ",
                "  │ id    7                          │  ",
                "  │ tags  [                          │  ",
                "  │         x                        │  ",
                "  │       ]                          │  ",
                "  │                                  │  ",
                "  │                                  │  ",
            ]
            .map(str::to_owned)
        );
    }

    #[test]
    fn a_long_value_wraps_under_its_own_indent() {
        let (buffer, lines) = drawn(
            30,
            12,
            Inspected::Row {
                number: 1,
                columns: vec!["note".to_owned()],
                values: vec![CqlValue::List(vec![CqlValue::Text(
                    "one two three four five six".to_owned(),
                )])],
            },
        );

        assert_eq!(
            rows(&buffer)[3..8],
            [
                "  │ note  [                │  ",
                "  │         one two three  │  ",
                "  │         four five six  │  ",
                "  │       ]                │  ",
                "  │                        │  ",
            ]
            .map(str::to_owned)
        );
        assert_eq!(lines, 4);
    }

    #[test]
    fn a_value_longer_than_the_body_gets_a_scrollbar() {
        let value = CqlValue::List((0..40).map(CqlValue::Int).collect());

        let (buffer, lines) = drawn(40, 12, cell(value));

        assert_eq!(lines, 42);
        assert!(
            rows(&buffer).iter().any(|row| row.contains('▐')),
            "{:#?}",
            rows(&buffer)
        );
    }

    #[test]
    fn the_frame_reports_the_body_height_and_the_wrapped_lines() {
        let mut state = State::default();
        state.overlay = Some(Overlay::Inspect(Inspector::opened(
            Subject::Table(TableRef::new("shop", "orders")),
            cell(CqlValue::List(vec![CqlValue::Int(1), CqlValue::Int(2)])),
        )));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("a backend");
        let mut geometry = None;

        terminal
            .draw(|frame| {
                geometry = Some(
                    crate::render::render(frame, &view_model(&state), Theme::default()).geometry,
                );
            })
            .expect("TestBackend always draws");

        let geometry = geometry.expect("drawn");
        assert_eq!(geometry.inspector_rows(), 18);
        assert_eq!(geometry.inspector_lines(), 4);
    }

    #[test]
    fn the_cursor_line_is_banded_across_every_row_it_wrapped_to() {
        let (buffer, _, selection) = drawn_at(
            30,
            12,
            Inspected::Row {
                number: 1,
                columns: vec!["note".to_owned()],
                values: vec![CqlValue::List(vec![CqlValue::Text(
                    "one two three four five six".to_owned(),
                )])],
            },
            1,
        );

        assert_eq!(banded(&buffer), [4, 5], "the element, both of its rows");
        assert_eq!(selection, Span { first: 1, lines: 2 });
    }

    #[test]
    fn a_frame_too_small_for_the_block_draws_none_and_reports_no_room() {
        let area = Rect::new(0, 0, 23, 8);

        assert_eq!(rect(area), None);
        assert_eq!(visible_rows(area), 0);
        assert_eq!(visible_rows(Rect::new(0, 0, 24, 9)), 3);
    }

    #[test]
    fn the_block_is_sized_by_the_frame_and_capped_in_width() {
        assert_eq!(rect(Rect::new(0, 0, 80, 24)), Some(Rect::new(2, 2, 76, 20)));
        assert_eq!(
            rect(Rect::new(0, 0, 200, 50)),
            Some(Rect::new(50, 2, 100, 46))
        );
    }
}
