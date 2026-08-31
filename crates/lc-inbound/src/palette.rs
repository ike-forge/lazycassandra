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
/// The widest row the catalogue can produce, plus both borders and both pad
/// columns — `Margin { horizontal: PAD }` spends one on each side. Derived
/// from the catalogue rather than chosen — `commands::tests` is what stops a
/// longer action being written
/// (docs/superpowers/specs/2026-08-09-command-palette-design.md §5).
const MIN_W: u16 = 39;
/// Past this every row is trailing space.
const MAX_W: u16 = 60;
/// Two borders, a heading, and enough commands to be worth opening.
const MIN_H: u16 = 8;
/// Between the left border and the keys column.
const PAD: u16 = 1;
/// Between the action column and the keys column.
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

/// `total` is the unfiltered catalogue's row count, and what the block is
/// sized from: a filter narrows the list inside a block that does not move
/// (docs/superpowers/specs/2026-08-31-palette-filter-design.md §2).
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
    frame.buffer_mut().set_style(area, theme.overlay_ground());
    frame.render_widget(block, area);

    let gutter = rows.iter().map(action_width).max().unwrap_or(0);
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| item(row, gutter, index == selected, search, theme))
        .collect();
    // A cursor on nothing is not a cursor: the empty line a term that matched
    // nothing leaves is not a row `⏎` can run, and does not wear the ground
    // that says it is (palette filter design §3).
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

/// Draws every visible group rule, `crate::render::rule` doing the reaching.
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

/// The palette says in a column what the bar says in a row, so it says it with
/// the bar's tokens — a `CommandKey` token would be a second name for one
/// meaning (architecture.md §6).
///
/// The action leads and the keys follow, the order a search-actions dialogue
/// uses: a reader of the open palette is looking for a thing to do and wants
/// the key it costs, not scanning a column of keys for one they already know.
fn item(
    row: &CommandRow,
    gutter: usize,
    selected: bool,
    search: Option<&Search>,
    theme: Theme,
) -> ListItem<'static> {
    match row {
        // Blank: `rules` paints the line itself, across the borders a
        // `ListItem` cannot reach.
        CommandRow::Separator => ListItem::new(Line::default()),
        CommandRow::Empty => ListItem::new(Line::from(Span::styled(
            "no command matches".to_owned(),
            theme.style(Token::Faint),
        ))),
        CommandRow::Heading(heading) => ListItem::new(Line::from(Span::styled(
            (*heading).to_owned(),
            theme.style(Token::Section),
        ))),
        // The bar's key colour without the bar's underline: that modifier is
        // there because a hint sits on a row whose colour changes with focus
        // (theme.rs, `Token::HintKey`). A palette row sits on the overlay's
        // own ground, where the colour is the whole signal and an underline
        // under every row is a texture rather than a mark.
        //
        // A row that cannot be run recedes the way a dead hint does
        // (`inbound/hints.rs`), rather than being dropped: the binding is
        // still real, and a list that is complete does not hide a row for
        // being unreachable this frame.
        CommandRow::Command {
            keys,
            action,
            liveness,
        } => {
            // The cursor still lands on an inert row, and it still reads at
            // full contrast there — a row you cannot run is still a row you
            // are reading
            // (docs/superpowers/specs/2026-08-15-palette-execution-design.md
            // §8).
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

            ListItem::new(Line::from(spans))
        }
    }
}

/// Columns, on the same single-width assumption the hint bar makes of the
/// labels `hints::name` produces.
fn action_width(row: &CommandRow) -> usize {
    match row {
        CommandRow::Heading(_) | CommandRow::Separator | CommandRow::Empty => 0,
        CommandRow::Command { action, .. } => action.chars().count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// Overlay search design §7: the matched span is painted where it sits,
    /// the rest of the row untouched.
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

    /// The row is still readable and still lands under the cursor; what it
    /// stops claiming is that `⏎` would do something
    /// (docs/superpowers/specs/2026-08-15-palette-execution-design.md §8).
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
        // MIN_H demands enough rows for a palette to open at all
        // (commands::tests::the_catalogue_is_taller_than_the_palettes_floor
        // holds the same floor on the real catalogue); pad past a bare
        // heading-plus-two so `rect` does not refuse the frame.
        let area = rect(Rect::new(0, 0, WIDTH, HEIGHT), rows.len()).expect("fits this frame");
        let buffer = drawn(&rows, 0);
        let theme = Theme::default();
        // One column for the border, one for the palette's own PAD; one row
        // for the border, one for the heading.
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

    /// Palette filter design §2: the block is sized from the whole catalogue,
    /// so narrowing the list moves no border.
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

    /// Palette filter design §3: a term that matched nothing says so, on a row
    /// that wears neither the selection ground nor a promise that `⏎` runs it.
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

    /// Design §8: "it still reads at full contrast when selected — a row you
    /// cannot run is still a row you are reading."
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
        // Index 1 in `rows` is the inert row; the cursor seats on it.
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
