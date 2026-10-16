//! The command palette: every binding, drawn over the panels.

use lc_domain::commands::CommandRow;
use lc_domain::search::Search;
use lc_domain::style::Token;
use lc_domain::view::Liveness;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState};

use crate::body::Extent;
use crate::theme::Theme;

const WIDTH_PERCENT: u16 = 50;
const HEIGHT_PERCENT: u16 = 70;
/// The widest catalogue row plus borders and pads; `commands::tests` stops a
/// longer action.
const MIN_W: u16 = 39;
/// Past this every row is trailing space.
const MAX_W: u16 = 60;
/// Two borders, a heading, and enough commands to be worth opening.
const MIN_H: u16 = 8;
const PAD: u16 = 1;
const GAP: &str = "  ";

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

/// Zero gates `?` in the domain.
pub(crate) fn visible_rows(area: Rect, rows: usize) -> usize {
    rect(area, rows).map_or(0, |rect| usize::from(rect.height.saturating_sub(2)))
}

/// Through `u32`: the product overflows near `u16::MAX`.
fn percent(value: u16, share: u16) -> u16 {
    u16::try_from(u32::from(value) * u32::from(share) / 100).unwrap_or(u16::MAX)
}

/// Sized from the unfiltered `total`, so a filter moves no border.
pub(crate) fn render(
    frame: &mut Frame,
    rows: &[CommandRow],
    total: usize,
    selected: usize,
    search: Option<&Search>,
    theme: Theme,
) {
    let Some(area) = rect(frame.area(), total) else {
        return;
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.focus_border())
        .title("Commands");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, theme.ground());
    frame.render_widget(block, area);

    let gutter = rows.iter().map(action_width).max().unwrap_or(0);
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| item(row, gutter, index == selected, search, theme))
        .collect();
    // The empty line a failed term leaves is not a runnable row.
    let landed = matches!(rows.get(selected), Some(CommandRow::Command { .. }));
    let mut state = ListState::default().with_selected(landed.then_some(selected));

    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.style(Token::Selected)),
        inner,
        &mut state,
    );

    rules(frame, area, inner, rows, state.offset(), theme);

    let extent = Extent {
        position: state.offset(),
        total: rows.len(),
        visible: usize::from(inner.height),
    };

    if extent.overflows() {
        crate::render::scrollbar(frame, area, &extent, theme.focus_border(), theme);
    }
}

fn rules(
    frame: &mut Frame,
    area: Rect,
    inner: Rect,
    rows: &[CommandRow],
    offset: usize,
    theme: Theme,
) {
    for line in 0..inner.height {
        let index = offset.saturating_add(usize::from(line));

        if matches!(rows.get(index), Some(CommandRow::Separator)) {
            crate::render::rule(frame, area, inner.y + line, theme.focus_border());
        }
    }
}

/// The bar's tokens, not a new one. Action first: a reader is looking for a
/// thing to do.
fn item(
    row: &CommandRow,
    gutter: usize,
    selected: bool,
    search: Option<&Search>,
    theme: Theme,
) -> ListItem<'static> {
    match row {
        // `rules` paints the line across the borders.
        CommandRow::Separator => ListItem::new(Line::default()),
        CommandRow::Empty => ListItem::new(Line::from(Span::styled(
            "no command matches".to_owned(),
            theme.style(Token::Faint),
        ))),
        CommandRow::Heading(heading) => ListItem::new(Line::from(Span::styled(
            (*heading).to_owned(),
            theme.style(Token::Section),
        ))),
        // No underline: on the overlay's own ground the colour is the signal.
        // An inert row recedes rather than vanishing.
        CommandRow::Command {
            keys,
            action,
            liveness,
            refused,
        } => {
            // An inert row under the cursor still reads at full contrast.
            let (key, text) = match (liveness, selected) {
                (Liveness::Live, _) | (Liveness::Inert, true) => {
                    (Token::HintKey, Token::HintAction)
                }
                (Liveness::Inert, false) => (Token::Faint, Token::Faint),
            };

            let mut spans = crate::highlight::spans(
                &format!("{action:<gutter$}"),
                search,
                theme.style(text),
                theme,
            );
            spans.push(Span::styled(
                format!("{GAP}{keys}"),
                theme.style(key).remove_modifier(Modifier::UNDERLINED),
            ));
            if let Some(posture) = refused {
                spans.push(Span::styled(
                    format!(" (disabled, {})", posture.label()),
                    theme.style(Token::Muted),
                ));
            }

            ListItem::new(Line::from(spans))
        }
    }
}

/// Assumes single-width labels, as the hint bar does.
fn action_width(row: &CommandRow) -> usize {
    match row {
        CommandRow::Heading(_) | CommandRow::Separator | CommandRow::Empty => 0,
        CommandRow::Command { action, .. } => action.chars().count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lc_domain::Posture;
    use lc_domain::view::Liveness;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    const WIDTH: u16 = 80;
    const HEIGHT: u16 = 12;

    fn drawn(rows: &[CommandRow], selected: usize) -> Buffer {
        let mut terminal =
            Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| render(frame, rows, rows.len(), selected, None, Theme::default()))
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    fn row(action: &'static str, liveness: Liveness) -> CommandRow {
        CommandRow::Command {
            keys: "tab".to_owned(),
            action,
            liveness,
            refused: None,
        }
    }

    fn drawn_with(rows: &[CommandRow], selected: usize, term: &str) -> Buffer {
        let mut terminal =
            Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("TestBackend always initialises");
        let search = Search::new(term);

        terminal
            .draw(|frame| {
                render(
                    frame,
                    rows,
                    rows.len(),
                    selected,
                    search.as_ref(),
                    Theme::default(),
                );
            })
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    #[test]
    fn a_matched_span_is_marked_in_the_action() {
        let rows = [
            CommandRow::Heading("Export"),
            row("export these rows", Liveness::Live),
            row("close the form", Liveness::Live),
            row("next field", Liveness::Live),
            row("previous field", Liveness::Live),
            row("select, or export", Liveness::Live),
        ];
        let area = rect(Rect::new(0, 0, WIDTH, HEIGHT), rows.len()).expect("fits this frame");
        let buffer = drawn_with(&rows, 1, "export");
        let theme = Theme::default();
        let x = area.x + 1 + PAD;
        let y = area.y + 2;
        let matched = theme
            .style(Token::Match)
            .fg
            .expect("a mark spends a colour");

        assert_eq!(
            buffer.cell((x, y)).expect("inside the block").fg,
            matched,
            "the term's first character wears the mark"
        );
        assert_ne!(
            buffer.cell((x + 7, y)).expect("inside the block").fg,
            matched,
            "the rest of the action does not"
        );
    }

    #[test]
    fn a_row_that_cannot_be_run_recedes() {
        let rows = [
            CommandRow::Heading("Export"),
            row("next field", Liveness::Inert),
            row("export these rows", Liveness::Live),
            row("select, or export", Liveness::Live),
            row("close the form", Liveness::Live),
            row("previous field", Liveness::Live),
        ];
        // MIN_H needs enough rows to open.
        let area = rect(Rect::new(0, 0, WIDTH, HEIGHT), rows.len()).expect("fits this frame");
        let buffer = drawn(&rows, 0);
        let theme = Theme::default();
        // Border and PAD across; border and heading down.
        let x = area.x + 1 + PAD;
        let y0 = area.y + 2;
        let at = |line: u16| buffer.cell((x, y0 + line)).expect("inside the block").fg;

        assert_eq!(
            at(0),
            theme.style(Token::Faint).fg.expect("faint spends a colour")
        );
        assert_eq!(
            at(1),
            theme
                .style(Token::HintAction)
                .fg
                .expect("a live action spends a colour")
        );
    }

    #[test]
    fn a_refused_row_names_the_posture_after_its_keys_in_muted() {
        let refused = CommandRow::Command {
            keys: "d".to_owned(),
            action: "drop this",
            liveness: Liveness::Live,
            refused: Some(Posture::ReadOnly),
        };
        let rows = [
            CommandRow::Heading("Drop and delete"),
            refused,
            row("export these rows", Liveness::Live),
            row("select, or export", Liveness::Live),
            row("close the form", Liveness::Live),
            row("previous field", Liveness::Live),
        ];
        // Wide enough to reach MAX_W.
        let wide = 2 * MAX_W;
        let area = rect(Rect::new(0, 0, wide, HEIGHT), rows.len()).expect("fits this frame");
        let mut terminal =
            Terminal::new(TestBackend::new(wide, HEIGHT)).expect("TestBackend always initialises");
        terminal
            .draw(|frame| render(frame, &rows, rows.len(), 0, None, Theme::default()))
            .expect("TestBackend always draws");
        let buffer = terminal.backend().buffer().clone();
        let theme = Theme::default();
        let x = area.x + 1 + PAD;
        let y = area.y + 2;
        let line: String = (x..area.right() - 1)
            .map(|column| buffer.cell((column, y)).expect("inside the block").symbol())
            .collect();
        let suffix = " (disabled, read-only)";
        let at = line.find(suffix).expect("the refusal is named");
        let column = |offset: usize| x + u16::try_from(offset).expect("within the frame");

        assert!(line[..at].ends_with('d'), "right after the keys: {line:?}");
        assert_eq!(
            buffer
                .cell((column(at + 1), y))
                .expect("inside the block")
                .fg,
            theme.style(Token::Muted).fg.expect("muted spends a colour")
        );
        assert_eq!(
            buffer
                .cell((column(at - 1), y))
                .expect("inside the block")
                .fg,
            theme
                .style(Token::HintKey)
                .fg
                .expect("a key spends a colour"),
            "the key itself reads as a key"
        );
    }

    #[test]
    fn a_filtered_palette_keeps_the_block_the_catalogue_gives_it() {
        let total = 40;
        let rows = [
            CommandRow::Heading("Export"),
            row("export these rows", Liveness::Live),
        ];
        let whole = rect(Rect::new(0, 0, WIDTH, HEIGHT), total).expect("fits this frame");
        let mut terminal =
            Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| render(frame, &rows, total, 1, None, Theme::default()))
            .expect("TestBackend always draws");

        assert_eq!(
            rect(Rect::new(0, 0, WIDTH, HEIGHT), rows.len()),
            None,
            "two rows on their own would not open a palette at all"
        );
        let buffer = terminal.backend().buffer().clone();
        assert_eq!(
            buffer
                .cell((whole.x, whole.y))
                .expect("the block's own corner")
                .symbol(),
            "╭",
            "the border sits where the unfiltered catalogue puts it"
        );
    }

    #[test]
    fn a_term_that_matches_nothing_draws_a_line_saying_so() {
        let total = 40;
        let area = rect(Rect::new(0, 0, WIDTH, HEIGHT), total).expect("fits this frame");
        let mut terminal =
            Terminal::new(TestBackend::new(WIDTH, HEIGHT)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(
                    frame,
                    &[CommandRow::Empty],
                    total,
                    0,
                    None,
                    Theme::default(),
                );
            })
            .expect("TestBackend always draws");

        let buffer = terminal.backend().buffer().clone();
        let theme = Theme::default();
        let x = area.x + 1 + PAD;
        let y = area.y + 1;
        let drawn: String = (0..18)
            .map(|offset| {
                buffer
                    .cell((x + offset, y))
                    .expect("inside the block")
                    .symbol()
                    .to_owned()
            })
            .collect();

        assert_eq!(drawn, "no command matches");
        assert_eq!(
            buffer.cell((x, y)).expect("inside the block").fg,
            theme.style(Token::Faint).fg.expect("faint spends a colour")
        );
        assert_ne!(
            buffer.cell((x, y)).expect("inside the block").bg,
            theme
                .style(Token::Selected)
                .bg
                .expect("the selection spends a ground"),
            "a cursor on nothing is not a cursor"
        );
    }

    #[test]
    fn a_selected_inert_row_reads_at_full_contrast() {
        let rows = [
            CommandRow::Heading("Export"),
            row("next field", Liveness::Inert),
            row("export these rows", Liveness::Live),
            row("select, or export", Liveness::Live),
            row("close the form", Liveness::Live),
            row("previous field", Liveness::Live),
        ];
        let area = rect(Rect::new(0, 0, WIDTH, HEIGHT), rows.len()).expect("fits this frame");
        // Index 1 is the inert row.
        let buffer = drawn(&rows, 1);
        let theme = Theme::default();
        let x = area.x + 1 + PAD;
        let y0 = area.y + 2;
        let at_inert_row = buffer.cell((x, y0)).expect("inside the block").fg;

        assert_eq!(
            at_inert_row,
            theme
                .style(Token::HintAction)
                .fg
                .expect("a live action spends a colour"),
            "a selected inert row must still read at full contrast"
        );
    }
}
