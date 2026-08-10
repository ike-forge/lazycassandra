//! Drawing what a panel contains.
//!
//! Two scroll regimes meet here. Tables, Contents and Ddl carry a scroll
//! offset as domain state that arrives in the [`PanelBody`]; what lives here
//! is the arithmetic that needs a width or a height — which columns fit, and
//! whether the selection falls inside the rows drawn
//! (doc/specs/2026-08-09-list-viewport-design.md §2). Queries stores no
//! offset at all: both its vertical and horizontal offsets are derived from
//! the cursor and the area every frame
//! (doc/specs/2026-08-09-queries-editor-design.md §5).

use lc_domain::style::Token;
use lc_domain::view::{GridCell, GridColumn, Marker, PanelBody, Position, TreeRow};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap};
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use crate::theme::Theme;

const FOLDED: &str = "▸ ";
const UNFOLDED: &str = "▾ ";
/// A row with nothing to unfold still spends the marker's width, so names stay
/// aligned down the panel.
const LEAF: &str = "  ";

const INDENT: &str = "  ";

/// How wide a grid column may grow before it is truncated.
///
/// A uuid is 36 columns and a blob is unbounded; letting either take its full
/// width pushes every other column off the panel. Horizontal navigation and
/// column priority are a later slice — this is the cap that keeps the grid
/// readable until then.
const MAX_COLUMN_WIDTH: u16 = 24;

/// Cells in a row are separated by one column. Stated rather than left to
/// ratatui's default because [`fitting`] has to do the same arithmetic.
const COLUMN_SPACING: u16 = 1;

const ELLIPSIS: char = '…';

/// Rows of a grid spent on its header rather than on a row.
const HEADER_ROWS: u16 = 1;

/// What a scrollbar needs, in whatever unit the panel scrolls in.
pub(crate) struct Extent {
    pub(crate) position: usize,
    pub(crate) total: usize,
    pub(crate) visible: usize,
}

impl Extent {
    pub(crate) const fn overflows(&self) -> bool {
        self.total > self.visible
    }
}

/// What a panel's body turned out to be: how far it scrolled, and in what.
pub(crate) struct Body {
    pub(crate) extent: Extent,
    pub(crate) unit: Unit,
}

/// What a body's [`Extent`] counts in.
///
/// An enum rather than a flag beside a header count, so "these are rows" and
/// "this many are spent above the first" cannot be said apart
/// (conventions.md §1).
#[derive(Clone, Copy)]
pub(crate) enum Unit {
    /// Rows a press can land on, `header` of which the panel spends on its own
    /// chrome above the first one drawn.
    Rows { header: u16 },
    /// Lines: scrolled, but nothing a press can address.
    Lines,
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    body: &PanelBody,
    focused: bool,
    theme: Theme,
) -> Option<Body> {
    if area.is_empty() {
        return None;
    }

    match body {
        PanelBody::Blank => None,
        PanelBody::Notice { text, token } => {
            notice(frame, area, text, *token, theme);
            None
        }
        PanelBody::Tree {
            rows,
            selected,
            offset,
        } => Some(Body {
            extent: tree(frame, area, rows, *selected, *offset, theme),
            unit: Unit::Rows { header: 0 },
        }),
        PanelBody::Details { details, offset } => Some(Body {
            extent: crate::details::render(frame, area, details, *offset, theme),
            unit: Unit::Lines,
        }),
        PanelBody::Editor { lines, cursor } => Some(Body {
            extent: editor(frame, area, lines, *cursor, focused),
            unit: Unit::Lines,
        }),
        PanelBody::Grid {
            columns,
            rows,
            selected,
            offset,
        } => Some(Body {
            extent: grid(frame, area, columns, rows, *selected, *offset, theme),
            unit: Unit::Rows {
                header: HEADER_ROWS,
            },
        }),
    }
}

fn notice(frame: &mut Frame, area: Rect, text: &str, token: Option<Token>, theme: Theme) {
    let mut paragraph = Paragraph::new(text).wrap(Wrap { trim: true });
    if let Some(token) = token {
        paragraph = paragraph.style(theme.style(token));
    }
    frame.render_widget(paragraph, area);
}

/// The selection, but only when the rows drawn include it.
///
/// `None` is not merely "draw no highlight". `ratatui` honours a supplied
/// offset only when nothing is selected, and otherwise walks the offset until
/// the selection is in view — so handing it a selection it cannot see would
/// drag the viewport back and undo the scroll
/// (doc/specs/2026-08-09-list-viewport-design.md §4).
const fn visible(selected: usize, offset: usize, height: usize) -> Option<usize> {
    if selected >= offset && selected < offset + height {
        Some(selected)
    } else {
        None
    }
}

fn tree(
    frame: &mut Frame,
    area: Rect,
    rows: &[TreeRow],
    selected: usize,
    offset: usize,
    theme: Theme,
) -> Extent {
    let items: Vec<ListItem> = rows.iter().map(|row| item(row, theme)).collect();
    let height = usize::from(area.height);

    let list = List::new(items).highlight_style(theme.style(Token::Selected));
    let mut state = ListState::default()
        .with_offset(offset)
        .with_selected(visible(selected, offset, height));

    frame.render_stateful_widget(list, area, &mut state);

    // Read back rather than assumed: the offset handed in is what the widget
    // draws from, and reading it out keeps the hit map assembled from what
    // actually drew (mouse input design §4).
    Extent {
        position: state.offset(),
        total: rows.len(),
        visible: height,
    }
}

/// Column widths are computed here rather than in the domain because only the
/// renderer knows how wide the panel is. Columns whose width runs past the
/// right edge are simply not drawn — horizontal navigation is a later slice.
fn grid(
    frame: &mut Frame,
    area: Rect,
    columns: &[GridColumn],
    rows: &[Vec<GridCell>],
    selected: usize,
    offset: usize,
    theme: Theme,
) -> Extent {
    // Only the columns that fit are handed to the widget. `Constraint::Length`
    // is a request, not a floor: given more of them than the area can hold,
    // ratatui shrinks every column to fit rather than letting the last one run
    // off the edge — which truncates values with no ellipsis to say so.
    let widths = widths(columns, rows, area.width);
    let shown = fitting(&widths, area.width);
    let widths = &widths[..shown];
    // Both the window `visible` judges against and the extent the scrollbar is
    // drawn from. Two copies of this subtraction can disagree by a row, and a
    // selection wrongly called visible makes the widget scroll away from the
    // stored offset.
    let height = usize::from(area.height).saturating_sub(usize::from(HEADER_ROWS));

    let header = Row::new(
        columns
            .iter()
            .zip(widths)
            .map(|(column, width)| styled(&column.name, column.token, *width, theme)),
    );

    let body = rows.iter().map(|cells| {
        Row::new(
            cells
                .iter()
                .zip(widths)
                .map(|(cell, width)| styled_cell(&cell.text, cell_style(cell, theme), *width)),
        )
    });

    let table = Table::new(body, widths.iter().copied().map(Constraint::Length))
        .header(header)
        .column_spacing(COLUMN_SPACING)
        .row_highlight_style(theme.style(Token::Selected));
    let mut state = TableState::default()
        .with_offset(offset)
        .with_selected(visible(selected, offset, height));

    frame.render_stateful_widget(table, area, &mut state);

    Extent {
        position: state.offset(),
        total: rows.len(),
        visible: height,
    }
}

/// How many of `widths` fit before the right edge.
///
/// Always at least one: a panel too narrow for even the first column still
/// shows what it can of it, which is more use than an empty panel.
fn fitting(widths: &[u16], available: u16) -> usize {
    let mut used = 0;
    let mut count = 0;

    for (index, width) in widths.iter().enumerate() {
        let spacing = if index == 0 { 0 } else { COLUMN_SPACING };
        let next = used + spacing + width;

        if next > available && count > 0 {
            break;
        }

        used = next;
        count += 1;
    }

    count
}

/// Each column takes the widest thing in it, capped — and never wider than
/// the panel, so the first column is ellipsised rather than squeezed when even
/// it does not fit.
fn widths(columns: &[GridColumn], rows: &[Vec<GridCell>], available: u16) -> Vec<u16> {
    let cap = MAX_COLUMN_WIDTH.min(available.max(1));

    columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let widest = rows
                .iter()
                .filter_map(|cells| cells.get(index))
                .map(|cell| cell.text.width())
                .chain(std::iter::once(column.name.width()))
                .max()
                .unwrap_or(1);

            u16::try_from(widest).unwrap_or(cap).clamp(1, cap)
        })
        .collect()
}

/// A matched cell composes `Token::Match` onto whatever `token` already
/// draws, rather than replacing it: `token` is what keeps a null apart from a
/// text cell holding the word `"null"` (design §4.3), and overwriting it
/// would lose that distinction for exactly the cells the user just went
/// looking for. `Style::patch` is the same composition idiom the selected
/// row already relies on, via `row_highlight_style`.
fn cell_style(cell: &GridCell, theme: Theme) -> Option<Style> {
    let base = cell.token.map(|token| theme.style(token));

    if !cell.matched {
        return base;
    }

    let matched = theme.style(Token::Match);
    Some(base.map_or(matched, |base| base.patch(matched)))
}

fn styled(text: &str, token: Option<Token>, width: u16, theme: Theme) -> Cell<'static> {
    styled_cell(text, token.map(|token| theme.style(token)), width)
}

fn styled_cell(text: &str, style: Option<Style>, width: u16) -> Cell<'static> {
    let cell = Cell::from(Text::raw(truncated(text, width)));

    match style {
        Some(style) => cell.style(style),
        None => cell,
    }
}

/// ratatui clips an over-long cell silently; an ellipsis is what says the
/// value continues.
fn truncated(text: &str, width: u16) -> String {
    let width = usize::from(width);
    if text.width() <= width {
        return text.to_owned();
    }

    let mut out = String::new();
    let mut used = 0;
    for character in text.chars() {
        let next = used + unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if next > width.saturating_sub(1) {
            break;
        }
        out.push(character);
        used = next;
    }
    out.push(ELLIPSIS);

    out
}

fn item(row: &TreeRow, theme: Theme) -> ListItem<'static> {
    let indent = INDENT.repeat(usize::from(row.depth));
    let marker = match row.marker {
        Marker::Folded => FOLDED,
        Marker::Unfolded => UNFOLDED,
        Marker::Leaf => LEAF,
    };

    let item = ListItem::new(format!("{indent}{marker}{}", row.label));
    match row.token {
        Some(token) => item.style(theme.style(token)),
        None => item,
    }
}

/// One buffer line is one screen row: long lines scroll sideways rather than
/// wrap, so the domain keeps counting its own lines and the cursor's own row
/// is always on screen (design §5).
fn editor(
    frame: &mut Frame,
    area: Rect,
    lines: &[String],
    cursor: Position,
    focused: bool,
) -> Extent {
    let height = usize::from(area.height);
    let width = usize::from(area.width);

    let current = lines.get(cursor.line).map_or("", String::as_str);
    let column = width_of(current, cursor.column);

    let top = window(cursor.line, height);
    let left = left_edge(current, column, width);

    let text = Text::from(
        lines
            .iter()
            .skip(top)
            .take(height)
            .map(|line| Line::raw(shifted(line, left)))
            .collect::<Vec<_>>(),
    );

    frame.render_widget(Paragraph::new(text), area);

    if focused {
        frame.set_cursor_position((
            area.x + narrowed(column - left),
            area.y + narrowed(cursor.line - top),
        ));
    }

    Extent {
        position: top,
        total: lines.len(),
        visible: height,
    }
}

/// The smallest offset that brings `index` into a window of `span` rows or
/// columns, starting from the top.
///
/// `editor` calls this for both axes — `cursor.line` against the height, and,
/// through [`left_edge`], `cursor.column` against the width — so the same
/// arithmetic serves the vertical and horizontal offset alike, recomputed
/// every frame rather than stored (queries-editor design §5).
const fn window(index: usize, span: usize) -> usize {
    index.saturating_sub(span.saturating_sub(1))
}

/// The horizontal offset, snapped up to a character boundary.
///
/// A double-width character straddling the offset would otherwise be dropped
/// whole by [`shifted`], putting the drawn text further right than the offset
/// claims and the cursor a cell or more off it.
fn left_edge(line: &str, column: usize, width: usize) -> usize {
    let target = window(column, width);
    let mut at = 0;

    for character in line.chars() {
        if at >= target {
            break;
        }
        at += character.width().unwrap_or(0);
    }

    at
}

/// Terminal columns the first `chars` characters of `line` occupy.
fn width_of(line: &str, chars: usize) -> usize {
    line.chars()
        .take(chars)
        .map(|character| character.width().unwrap_or(0))
        .sum()
}

/// `line` with its first `columns` terminal columns scrolled off to the left.
fn shifted(line: &str, columns: usize) -> String {
    let mut used = 0;
    let mut out = String::new();

    for character in line.chars() {
        if used >= columns {
            out.push(character);
        } else {
            used += character.width().unwrap_or(0);
        }
    }

    out
}

/// Both callers pass a value already bounded by an area dimension, so the
/// fallback is unreachable rather than a silent clamp.
fn narrowed(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(0)
}
