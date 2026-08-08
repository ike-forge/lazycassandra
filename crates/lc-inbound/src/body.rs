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

const ELLIPSIS: char = '…';

pub(crate) fn render(frame: &mut Frame, area: Rect, body: &PanelBody, theme: Theme) {
    if area.is_empty() {
        return;
    }

    match body {
        PanelBody::Blank => {}
        PanelBody::Notice { text, token } => notice(frame, area, text, *token, theme),
        PanelBody::Tree { rows, selected } => tree(frame, area, rows, *selected, theme),
        PanelBody::Grid {
            columns,
            rows,
            selected,
        } => grid(frame, area, columns, rows, *selected, theme),
    }
}

fn notice(frame: &mut Frame, area: Rect, text: &str, token: Option<Token>, theme: Theme) {
    let mut paragraph = Paragraph::new(text).wrap(Wrap { trim: true });
    if let Some(token) = token {
        paragraph = paragraph.style(theme.style(token));
    }
    frame.render_widget(paragraph, area);
}

fn tree(frame: &mut Frame, area: Rect, rows: &[TreeRow], selected: usize, theme: Theme) {
    let items: Vec<ListItem> = rows.iter().map(|row| item(row, theme)).collect();

    let list = List::new(items).highlight_style(theme.style(Token::Selected));
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(list, area, &mut state);
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
) {
    let widths = widths(columns, rows);

    let header = Row::new(
        columns
            .iter()
            .zip(&widths)
            .map(|(column, width)| styled(&column.name, column.token, *width, theme)),
    );

    let body = rows.iter().map(|cells| {
        Row::new(
            cells
                .iter()
                .zip(&widths)
                .map(|(cell, width)| styled(&cell.text, cell.token, *width, theme)),
        )
    });

    let table = Table::new(body, widths.iter().copied().map(Constraint::Length))
        .header(header)
        .row_highlight_style(theme.style(Token::Selected));
    let mut state = TableState::default().with_selected(Some(selected));

    frame.render_stateful_widget(table, area, &mut state);
}

/// Each column takes the widest thing in it, capped.
fn widths(columns: &[GridColumn], rows: &[Vec<GridCell>]) -> Vec<u16> {
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

            u16::try_from(widest)
                .unwrap_or(MAX_COLUMN_WIDTH)
                .clamp(1, MAX_COLUMN_WIDTH)
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
