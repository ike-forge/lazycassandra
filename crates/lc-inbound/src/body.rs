//! Drawing what a panel contains.
//!
//! Scrolling lives here rather than in the domain: the domain has no idea the
//! panel is eight rows tall. A `ListState` built per frame from the selected
//! index computes the offset, so no scroll state is stored anywhere.

use lc_domain::style::Token;
use lc_domain::view::{GridCell, GridColumn, Marker, PanelBody, TreeRow};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::text::Text;
use ratatui::widgets::{Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap};
use unicode_width::UnicodeWidthStr as _;

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
    /// Wrapped lines: scrolled, but nothing a press can address.
    Lines,
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    body: &PanelBody,
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
        PanelBody::Tree { rows, selected } => Some(Body {
            extent: tree(frame, area, rows, *selected, theme),
            unit: Unit::Rows { header: 0 },
        }),
        PanelBody::Details { details, offset } => Some(Body {
            extent: crate::details::render(frame, area, details, *offset, theme),
            unit: Unit::Lines,
        }),
        PanelBody::Grid {
            columns,
            rows,
            selected,
        } => Some(Body {
            extent: grid(frame, area, columns, rows, *selected, theme),
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

fn tree(frame: &mut Frame, area: Rect, rows: &[TreeRow], selected: usize, theme: Theme) -> Extent {
    let items: Vec<ListItem> = rows.iter().map(|row| item(row, theme)).collect();

    let list = List::new(items).highlight_style(theme.style(Token::Selected));
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(list, area, &mut state);

    // Read back rather than recomputed: the widget owns the arithmetic that
    // keeps the selection on screen, and a second copy of it here would be a
    // second thing to keep right.
    Extent {
        position: state.offset(),
        total: rows.len(),
        visible: usize::from(area.height),
    }
}

/// Column widths are computed here rather than in the domain, for the reason
/// scrolling is: only the renderer knows how wide the panel is. Columns whose
/// width runs past the right edge are simply not drawn — horizontal navigation
/// is a later slice.
fn grid(
    frame: &mut Frame,
    area: Rect,
    columns: &[GridColumn],
    rows: &[Vec<GridCell>],
    selected: usize,
    theme: Theme,
) -> Extent {
    // Only the columns that fit are handed to the widget. `Constraint::Length`
    // is a request, not a floor: given more of them than the area can hold,
    // ratatui shrinks every column to fit rather than letting the last one run
    // off the edge — which truncates values with no ellipsis to say so.
    let widths = widths(columns, rows, area.width);
    let shown = fitting(&widths, area.width);
    let widths = &widths[..shown];

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
                .map(|(cell, width)| styled(&cell.text, cell.token, *width, theme)),
        )
    });

    let table = Table::new(body, widths.iter().copied().map(Constraint::Length))
        .header(header)
        .column_spacing(COLUMN_SPACING)
        .row_highlight_style(theme.style(Token::Selected));
    let mut state = TableState::default().with_selected(Some(selected));

    frame.render_stateful_widget(table, area, &mut state);

    Extent {
        position: state.offset(),
        total: rows.len(),
        visible: usize::from(area.height).saturating_sub(usize::from(HEADER_ROWS)),
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

fn styled(text: &str, token: Option<Token>, width: u16, theme: Theme) -> Cell<'static> {
    let cell = Cell::from(Text::raw(truncated(text, width)));

    match token {
        Some(token) => cell.style(theme.style(token)),
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
