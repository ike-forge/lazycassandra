//! Drawing what a panel contains.
//!
//! Two scroll regimes meet here. Tables, Contents and Ddl carry a scroll
//! offset as domain state that arrives in the [`PanelBody`] — the Contents
//! grid a horizontal one besides — and what lives here is the arithmetic that
//! needs the area: how many of the domain's column widths fit in it, and
//! whether the selection falls inside the rows drawn
//! (docs/superpowers/specs/2026-08-09-list-viewport-design.md §2). Queries stores no
//! offset at all: both its vertical and horizontal offsets are derived from
//! the cursor and the area every frame
//! (docs/superpowers/specs/2026-08-09-queries-editor-design.md §5).
//!
//! [`Body::selection`] is the one thing besides [`Extent`] a body reports
//! back. Every panel but Ddl can locate its own selection from a row index
//! alone, so `Span::default()` is all they carry; Ddl's selection can span
//! several wrapped lines, which only the renderer that wrapped them can
//! measure (docs/superpowers/specs/2026-08-10-ddl-selection-design.md §5.4).

use lc_domain::grid::{COLUMN_SPACING, available, fitting};
use lc_domain::state::Span;
use lc_domain::style::Token;
use lc_domain::view::{GridCell, GridColumn, PanelBody, Position, RowKind, TreeRow};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span as TextSpan, Text};
use ratatui::widgets::{Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap};
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use crate::theme::Theme;

/// The limb a table hangs off its keyspace by, and the one that closes the
/// run.
const LIMB: &str = "├─ ";
const LAST_LIMB: &str = "└─ ";

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
    /// The horizontal extent, for the bodies that scroll sideways. Only a
    /// grid does.
    pub(crate) columns: Option<Extent>,
    pub(crate) unit: Unit,
    /// Where the body drew its selection, in the unit [`Extent`] counts in.
    ///
    /// Only the Ddl body reports one: it is the one panel whose selection
    /// spans more than a line, and so the one whose offset cannot be derived
    /// from a row index.
    pub(crate) selection: Span,
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
            columns: None,
            unit: Unit::Rows { header: 0 },
            selection: Span::default(),
        }),
        PanelBody::Details {
            details,
            selected,
            offset,
        } => {
            let (extent, selection) =
                crate::details::render(frame, area, details, *selected, *offset, theme);
            Some(Body {
                extent,
                columns: None,
                unit: Unit::Lines,
                selection,
            })
        }
        PanelBody::Editor { lines, cursor } => Some(Body {
            extent: editor(frame, area, lines, *cursor, focused),
            columns: None,
            unit: Unit::Lines,
            selection: Span::default(),
        }),
        PanelBody::Grid {
            columns,
            rows,
            selected,
            offset,
            first_column,
            column,
            gutter,
        } => {
            let (extent, horizontal) = grid(
                frame,
                area,
                &GridView {
                    columns,
                    rows,
                    selected: *selected,
                    offset: *offset,
                    first_column: *first_column,
                    column: *column,
                    gutter: *gutter,
                },
                theme,
            );

            Some(Body {
                extent,
                columns: Some(horizontal),
                unit: Unit::Rows {
                    header: HEADER_ROWS,
                },
                selection: Span::default(),
            })
        }
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
/// (docs/superpowers/specs/2026-08-09-list-viewport-design.md §4).
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

/// The view model's grid, as the renderer reads it.
///
/// A struct rather than seven more parameters: the fields arrive together
/// from one `PanelBody::Grid` and mean nothing apart.
#[derive(Clone, Copy)]
struct GridView<'a> {
    columns: &'a [GridColumn],
    rows: &'a [Vec<GridCell>],
    selected: usize,
    offset: usize,
    first_column: usize,
    column: usize,
    gutter: u16,
}

fn grid(frame: &mut Frame, area: Rect, view: &GridView, theme: Theme) -> (Extent, Extent) {
    let GridView {
        columns,
        rows,
        selected,
        offset,
        first_column,
        column,
        gutter,
    } = *view;

    // Both the window `visible` judges against and the extent the scrollbar is
    // drawn from. Two copies of this subtraction can disagree by a row, and a
    // selection wrongly called visible makes the widget scroll away from the
    // stored offset.
    let height = usize::from(area.height).saturating_sub(usize::from(HEADER_ROWS));

    let widths: Vec<u16> = columns.iter().map(|column| column.width).collect();
    let available = available(area.width, gutter);
    let first_column = first_column.min(columns.len());
    let shown = fitting(&widths, first_column, available);
    let drawn = first_column..first_column + shown;
    // `lc_domain::grid::widths` caps a column at its own maximum and not at
    // this panel, so one column can be wider than the room there is.
    // `Constraint::Length` is a request rather than a floor: handed more than
    // the area can hold, ratatui shrinks every column — the gutter included —
    // and truncates each value with no ellipsis to say it did.
    let drawn_widths: Vec<u16> = widths[drawn.clone()]
        .iter()
        .map(|width| (*width).min(available))
        .collect();

    let constraints = std::iter::once(Constraint::Length(gutter))
        .chain(drawn_widths.iter().copied().map(Constraint::Length));

    let header = Row::new(
        std::iter::once(Cell::from(Text::raw(" ".repeat(usize::from(gutter))))).chain(
            columns[drawn.clone()]
                .iter()
                .zip(&drawn_widths)
                .map(|(column, width)| styled(&column.name, column.token, *width, theme)),
        ),
    );

    let body = rows.iter().enumerate().map(|(index, cells)| {
        Row::new(
            std::iter::once(number(index, gutter, theme)).chain(
                cells
                    .get(drawn.clone())
                    .unwrap_or_default()
                    .iter()
                    .zip(&drawn_widths)
                    .map(|(cell, width)| styled_cell(&cell.text, cell_style(cell, theme), *width)),
            ),
        )
    });

    let table = Table::new(body, constraints)
        .header(header)
        .column_spacing(COLUMN_SPACING)
        .row_highlight_style(theme.style(Token::Selected))
        .cell_highlight_style(theme.style(Token::SelectedCell));
    // The gutter is a column of the widget but not of the page, so the
    // selected column is one further right than the domain's index.
    let cell =
        visible(selected, offset, height).map(|row| (row, column.saturating_sub(first_column) + 1));
    let mut state = TableState::default()
        .with_offset(offset)
        .with_selected_cell(cell);

    frame.render_stateful_widget(table, area, &mut state);

    (
        Extent {
            position: state.offset(),
            total: rows.len(),
            visible: height,
        },
        Extent {
            position: first_column,
            total: columns.len(),
            visible: shown,
        },
    )
}

/// A row's number, right-aligned in the gutter.
fn number(index: usize, gutter: u16, theme: Theme) -> Cell<'static> {
    let text = format!("{:>width$}", index + 1, width = usize::from(gutter));

    Cell::from(Text::raw(text)).style(theme.style(Token::Muted))
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
    // A keyspace opens at the margin and its tables hang off it, so the shape
    // of the tree is the indentation and no separate fold marker is spent on
    // saying what the drawn rows already say
    // (docs/superpowers/specs/2026-08-12-nerd-font-icons-design.md §9).
    let limb = match row.kind {
        RowKind::Keyspace => "",
        RowKind::Table { last: false } => LIMB,
        RowKind::Table { last: true } => LAST_LIMB,
    };

    // The icon carries BOLD and the rest of the row does not: a terminal that
    // has no bold cut of the Nerd Font draws the same glyph and loses nothing,
    // and one that has draws the only part of the row that has to be read as a
    // shape rather than as text
    // (docs/superpowers/specs/2026-08-12-nerd-font-icons-design.md §7).
    // The icon takes the colour of what it names rather than the row's, so a
    // system keyspace's glyph keeps its colour while its name stays muted:
    // the mute says "this is not yours to browse", which is a claim about the
    // name, not about what kind of thing the row is (design §8).
    let icon = TextSpan::styled(
        crate::icons::tree_row(row.kind),
        theme
            .style(crate::icons::token(row.kind))
            .add_modifier(Modifier::BOLD),
    );

    ListItem::new(Line::from(vec![
        TextSpan::raw(limb),
        icon,
        TextSpan::raw(row.label.clone()),
    ]))
    .style(theme.style(row.token))
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
