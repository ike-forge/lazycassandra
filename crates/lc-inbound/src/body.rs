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

use lc_domain::contents::SelectionMode;
use lc_domain::grid::{BAR, COLUMN_SPACING, available, fitting};
use lc_domain::state::{Range, Span};
use lc_domain::style::Token;
use lc_domain::view::{GridCell, GridColumn, PanelBody, Position, RowKind, TreeRow};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span as TextSpan, Text};
use ratatui::widgets::{Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap};
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use crate::hit::Band;
use crate::theme::Theme;

/// The limb a table hangs off its keyspace by, and the one that closes the
/// run.
const LIMB: &str = "├─ ";
const LAST_LIMB: &str = "└─ ";

const ELLIPSIS: char = '…';

/// Rows of a grid spent on its header rather than on a row.
const HEADER_ROWS: u16 = 1;

/// The mark in the cursor row's gutter.
///
/// A left half-block rather than an arrow: it reads as a bar running down the
/// edge of the row, and it is one column wide in every font we can assume.
const SELECTION_BAR: char = '▌';

/// The mark, reserved as a column of its own on every row so unselected rows
/// never shift sideways as the cursor moves (design §4.2). Lit for the cursor
/// row or, when `v` has extended one, any row a held range covers — the
/// caller decides which (design §6 rule 4).
pub(crate) fn selection_bar(marked: bool, theme: Theme) -> TextSpan<'static> {
    if marked {
        TextSpan::styled(SELECTION_BAR.to_string(), theme.style(Token::SelectionBar))
    } else {
        TextSpan::raw(" ")
    }
}

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
    /// Where each drawn column of a grid landed, naming its page index. Empty
    /// for every body that is not a grid.
    pub(crate) bands: Vec<Band<usize>>,
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
#[derive(Clone)]
pub(crate) enum Unit {
    /// Rows a press can land on, `header` of which the panel spends on its own
    /// chrome above the first one drawn.
    Rows { header: u16 },
    /// Lines, each naming the row it was wrapped from — the shape of the Ddl
    /// body, whose rows stand one to several lines. Empty where the lines
    /// belong to no rows at all, which is the Queries editor.
    Lines { rows: Vec<Option<usize>> },
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
            range,
            flash,
        } => Some(Body {
            extent: tree(
                frame,
                area,
                rows,
                *selected,
                *offset,
                Marked::new(*range, *flash),
                theme,
            ),
            columns: None,
            bands: Vec::new(),
            unit: Unit::Rows { header: 0 },
            selection: Span::default(),
        }),
        PanelBody::Details {
            details,
            selected,
            offset,
            range,
            flash,
        } => {
            let (extent, selection, rows) = crate::details::render(
                frame,
                area,
                details,
                *selected,
                *offset,
                Marked::new(*range, *flash),
                theme,
            );
            Some(Body {
                extent,
                columns: None,
                bands: Vec::new(),
                unit: Unit::Lines { rows },
                selection,
            })
        }
        PanelBody::Editor { lines, cursor } => Some(Body {
            extent: editor(frame, area, lines, *cursor, focused),
            columns: None,
            bands: Vec::new(),
            unit: Unit::Lines { rows: Vec::new() },
            selection: Span::default(),
        }),
        PanelBody::Grid { .. } => {
            let (extent, horizontal, bands) = grid(frame, area, &grid_view(body)?, theme);

            Some(Body {
                extent,
                columns: Some(horizontal),
                bands,
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
    marked: Marked,
    theme: Theme,
) -> Extent {
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let ranged = marked.holds(index);
            item(
                row,
                index == selected || ranged,
                ranged.then_some(marked.band),
                theme,
            )
        })
        .collect();
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
    mode: SelectionMode,
    gutter: u16,
    /// The rows `v` has selected while extending the row axis, as
    /// [`PanelBody::Grid::rows_range`].
    rows_range: Option<Range>,
    /// [`GridView::rows_range`]'s counterpart for the column axis.
    columns_range: Option<Range>,
    /// What the two ranges are painted with — [`Token::Ranged`] for a live
    /// selection, the louder [`Token::Selected`] for a yank's afterglow.
    band: Token,
}

fn grid(
    frame: &mut Frame,
    area: Rect,
    view: &GridView,
    theme: Theme,
) -> (Extent, Extent, Vec<Band<usize>>) {
    let GridView {
        columns,
        rows,
        selected,
        offset,
        first_column,
        column,
        mode,
        gutter,
        rows_range,
        columns_range,
        band,
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

    let constraints = std::iter::once(Constraint::Length(BAR + gutter))
        .chain(drawn_widths.iter().copied().map(Constraint::Length));

    let columns_at = bands(area, &drawn_widths, first_column, gutter);

    let row = visible(selected, offset, height);
    // The heading's wash belongs to the crosshair, and the crosshair goes with
    // the selection: a row cursor scrolled out of view leaves the grid painting
    // neither band, so a heading lit on its own would be the only mark on an
    // unlit grid (design §4). A column selection does not depend on where the
    // row viewport sits.
    let wash = match mode {
        SelectionMode::Cell => row.is_some().then(|| theme.style(Token::Crosshair)),
        SelectionMode::Column => Some(theme.style(Token::Selected)),
        SelectionMode::Row => None,
    };
    let header = heading(
        &columns[drawn.clone()],
        &drawn_widths,
        wash.zip(column.checked_sub(first_column)),
        gutter,
        theme,
    );

    let row_layout = GridRow {
        drawn: drawn.clone(),
        drawn_widths: &drawn_widths,
        first_column,
        selected,
        gutter,
        rows_range,
        columns_range,
        band,
    };
    let body = rows
        .iter()
        .enumerate()
        .map(|(index, cells)| grid_row(index, cells, &row_layout, theme));

    let table = Table::new(body, constraints)
        .header(header)
        .column_spacing(COLUMN_SPACING);

    // The bar and the row number share one widget column that is not a column
    // of the page, so the selected column is one further right than the
    // domain's index.
    let drawn_column = column.saturating_sub(first_column) + 1;
    // `None` where the cursor is off the right of what fits: ratatui clamps
    // `selected_column` to the last column it drew, so column mode would lay a
    // full-height band over a column the heading gate — which asks the same
    // question of the page's own indices — does not match. `follow_grid_columns`
    // runs against the geometry of the frame before this one, so a resize that
    // narrows the panel reaches this state for a frame. The cell mode's own
    // vertical band is gated on the same answer.
    let selected_column = drawn.contains(&column).then_some(drawn_column);

    // Which of `TableState`'s three selections this frame carries. A row
    // scrolled out of view is `None` in cell and row mode alike; a column
    // selection does not depend on where the row viewport sits (design §4).
    let empty = TableState::default().with_offset(offset);
    let (table, mut state) = match mode {
        SelectionMode::Cell => (
            table.cell_highlight_style(theme.style(Token::SelectedCell)),
            empty.with_selected_cell(row.map(|row| (row, drawn_column))),
        ),
        SelectionMode::Row => (
            table.row_highlight_style(theme.style(Token::Selected)),
            empty.with_selected(row),
        ),
        SelectionMode::Column => (
            table.column_highlight_style(theme.style(Token::Selected)),
            empty.with_selected_column(selected_column),
        ),
    };

    if matches!(mode, SelectionMode::Cell) {
        let at = selected_column.map(|_| column - first_column);
        crosshair(
            frame,
            area,
            row.map(|row| row - offset),
            at.map(|at| (columns_at[at].start, drawn_widths[at])),
            theme,
        );
    }

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
        columns_at,
    )
}

/// The cell mode's two bands, washed onto the buffer *under* the table rather
/// than handed to it as `row_highlight_style` / `column_highlight_style`.
///
/// ratatui paints those over the whole band after the rows are drawn, so they
/// bury whatever background a cell spends of its own — and the band a held
/// range paints is exactly that. Washed underneath, the crosshair survives on
/// every cell that spends no background and yields on the cells the range
/// holds: the crosshair says where the cursor is, the range says what a command
/// would act on, and where both are true the second is what a cell has to say
/// (design §6). The cursor's own cell reads over either, `Token::SelectedCell`
/// being a modifier rather than a colour.
///
/// `row` counts from the first row drawn rather than from the page; `column` is
/// where the cursor's column landed, in absolute frame columns. Either is
/// `None` where that axis' cursor is not one of the cells drawn.
fn crosshair(
    frame: &mut Frame,
    area: Rect,
    row: Option<usize>,
    column: Option<(u16, u16)>,
    theme: Theme,
) {
    let body = Rect {
        y: area.y.saturating_add(HEADER_ROWS),
        height: area.height.saturating_sub(HEADER_ROWS),
        ..area
    };
    let wash = theme.style(Token::Crosshair);
    let bands = [
        row.map(|row| Rect {
            y: body
                .y
                .saturating_add(u16::try_from(row).unwrap_or(u16::MAX)),
            height: 1,
            ..body
        }),
        column.map(|(x, width)| Rect { x, width, ..body }),
    ];

    for band in bands.into_iter().flatten() {
        frame.buffer_mut().set_style(band.intersection(body), wash);
    }
}

/// Whether the cell at `row, column` — both counted in the page's own
/// indices, never the drawn window's — falls inside the rectangle
/// `rows_range` and `columns_range` describe.
///
/// `None` on an axis means that axis was never extended, so it constrains
/// nothing: a row range with no column range is the whole row, and a column
/// range with no row range is the whole column. The two together are painted
/// only when at least one of them is `Some` (design §6).
fn ranged(
    row: usize,
    column: usize,
    rows_range: Option<Range>,
    columns_range: Option<Range>,
) -> bool {
    (rows_range.is_some() || columns_range.is_some())
        && rows_range.is_none_or(|range| range.contains(row))
        && columns_range.is_none_or(|range| range.contains(column))
}

/// The grid's fields, read off the body that carries them. `None` for every
/// other body, which is unreachable: the caller matched `Grid` to get here.
fn grid_view(body: &PanelBody) -> Option<GridView<'_>> {
    let PanelBody::Grid {
        columns,
        rows,
        selected,
        offset,
        first_column,
        column,
        mode,
        gutter,
        rows_range,
        columns_range,
        flash,
    } = body
    else {
        return None;
    };

    Some(GridView {
        columns,
        rows,
        selected: *selected,
        offset: *offset,
        first_column: *first_column,
        column: *column,
        mode: *mode,
        gutter: *gutter,
        rows_range: *rows_range,
        columns_range: *columns_range,
        band: Marked::band(*flash),
    })
}

/// What one row of the grid's body needs, gathered here rather than passed as
/// seven more parameters of `grid_row` (the reason [`GridView`] itself is a
/// struct).
struct GridRow<'a> {
    drawn: std::ops::Range<usize>,
    drawn_widths: &'a [u16],
    first_column: usize,
    selected: usize,
    gutter: u16,
    rows_range: Option<Range>,
    columns_range: Option<Range>,
    band: Token,
}

/// A run of rows marked out, and what it is painted with.
///
/// The two travel together because a range that lost its band would be drawn
/// as an ordinary selection — which is exactly what a yank's afterglow must
/// not look like.
#[derive(Clone, Copy)]
pub(crate) struct Marked {
    pub(crate) range: Option<Range>,
    pub(crate) band: Token,
}

impl Marked {
    /// A yank's afterglow takes the loudest of the three bands, so a copy
    /// reads as a flash against the range that was standing there a frame ago.
    /// Which token that is holds in every palette rather than by choice of
    /// colour:
    /// `the_range_is_louder_than_the_crosshair_and_quieter_than_the_cursor_row`
    /// pins `Selected` above `Ranged` in each of them, dark and light
    /// (design §8).
    const fn band(flash: bool) -> Token {
        if flash {
            Token::Selected
        } else {
            Token::Ranged
        }
    }

    const fn new(range: Option<Range>, flash: bool) -> Self {
        Self {
            range,
            band: Self::band(flash),
        }
    }

    /// Nothing marked out. The band is the one an ordinary selection takes,
    /// which nothing paints while `range` is `None`.
    #[cfg(test)]
    pub(crate) const fn none() -> Self {
        Self::new(None, false)
    }

    pub(crate) fn holds(self, index: usize) -> bool {
        self.range.is_some_and(|range| range.contains(index))
    }
}

/// One row of the grid's body: the bar-and-gutter cell, then the columns
/// `layout.drawn` names, each patched with `Token::Ranged` where [`ranged`]
/// says its page column falls inside the block.
fn grid_row(index: usize, cells: &[GridCell], layout: &GridRow, theme: Theme) -> Row<'static> {
    let marked = index == layout.selected
        || layout
            .rows_range
            .is_some_and(|rows_range| rows_range.contains(index));

    Row::new(
        std::iter::once(Cell::from(Line::from(vec![
            selection_bar(marked, theme),
            number(index, layout.gutter, theme),
        ])))
        .chain(
            cells
                .get(layout.drawn.clone())
                .unwrap_or_default()
                .iter()
                .zip(layout.drawn_widths)
                .enumerate()
                // `drawn` starts at `first_column`, not at the page's own
                // origin: the offset here names a position in the window
                // drawn, and only adding it back to `first_column` gives the
                // page column `ranged` means.
                .map(|(offset, (cell, width))| {
                    let style = if ranged(
                        index,
                        layout.first_column + offset,
                        layout.rows_range,
                        layout.columns_range,
                    ) {
                        Some(
                            cell_style(cell, theme)
                                .unwrap_or_default()
                                .patch(theme.style(layout.band)),
                        )
                    } else {
                        cell_style(cell, theme)
                    };

                    styled_cell(&cell.text, style, *width)
                }),
        ),
    )
}

/// Where each drawn column of the grid landed, in absolute frame columns.
///
/// Laid out from the same constraints handed to the `Table`, in the same
/// order: the bar and the gutter share one widget column, one spacing column
/// follows it, and one follows every value column but the last. Each band
/// claims the spacing that follows it, so a press on the blank between two
/// cells names the one it sits behind rather than nothing at all; the blank
/// past the last column belongs to no column, and is where the grid runs out.
///
/// `lc_domain::grid::fitting` is what keeps the sum inside `area`, so no band
/// runs past the panel and `ratatui` never shrinks a column out from under one
/// (design §6).
fn bands(area: Rect, widths: &[u16], first_column: usize, gutter: u16) -> Vec<Band<usize>> {
    let last = widths.len().saturating_sub(1);
    let mut start = area.x + BAR + gutter + COLUMN_SPACING;

    widths
        .iter()
        .enumerate()
        .map(|(offset, width)| {
            let spacing = if offset == last { 0 } else { COLUMN_SPACING };
            let band = Band {
                start,
                end: start.saturating_add(*width).saturating_add(spacing),
                names: first_column + offset,
            };
            start = band.end;

            band
        })
        .collect()
}

/// The grid's heading row: a blank cell over the bar and the gutter, then the
/// drawn columns. `washed` names which of them the cursor is in — an index into
/// what is drawn, not into the page — and the style to wash it with; `None`
/// where the mode paints no band, or where the cursor's column is not one of
/// these.
fn heading(
    columns: &[GridColumn],
    widths: &[u16],
    washed: Option<(Style, usize)>,
    gutter: u16,
    theme: Theme,
) -> Row<'static> {
    Row::new(
        std::iter::once(Cell::from(Text::raw(" ".repeat(usize::from(BAR + gutter))))).chain(
            columns
                .iter()
                .zip(widths)
                .enumerate()
                .map(|(index, (column, width))| {
                    // A projection's columns carry no token at all, and claiming
                    // one for them under the wash would be a decoration the
                    // schema does not support (`view::contents`): the wash
                    // composes onto whatever the heading already draws, which
                    // for those is nothing.
                    let base = column
                        .token
                        .map_or_else(Style::new, |token| theme.style(token));

                    let style = match washed.filter(|(_, washed)| *washed == index) {
                        Some((wash, _)) => base.patch(wash),
                        None => base,
                    };

                    styled_cell(&column.name, Some(style), *width)
                }),
        ),
    )
}

/// A row's number, right-aligned in the gutter.
fn number(index: usize, gutter: u16, theme: Theme) -> TextSpan<'static> {
    let text = format!("{:>width$}", index + 1, width = usize::from(gutter));

    TextSpan::styled(text, theme.style(Token::Muted))
}

/// A matched cell composes `Token::Match` onto whatever `token` already
/// draws, rather than replacing it, the same composition idiom the selected
/// row already relies on via `row_highlight_style`. `Token::Match` overwrites
/// `fg`, so a matched null and a matched cell holding the literal word
/// `"null"` draw alike; the tokens carry no modifier to tell them apart by
/// (design §4.1).
fn cell_style(cell: &GridCell, theme: Theme) -> Option<Style> {
    let base = cell.token.map(|token| theme.style(token));

    if !cell.matched {
        return base;
    }

    let matched = theme.style(Token::Match);
    Some(base.map_or(matched, |base| base.patch(matched)))
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

fn item(row: &TreeRow, marked: bool, band: Option<Token>, theme: Theme) -> ListItem<'static> {
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
    // system keyspace's glyph keeps its colour while its name stays faint: the
    // de-emphasis says "this is not yours to browse", which is a claim about
    // the name, not about what kind of thing the row is (design §8).
    let icon = TextSpan::styled(
        crate::icons::tree_row(row.kind),
        theme
            .style(crate::icons::token(row.kind))
            .add_modifier(Modifier::BOLD),
    );

    let line = Line::from(vec![
        selection_bar(marked, theme),
        TextSpan::raw(limb),
        icon,
        TextSpan::raw(row.label.clone()),
    ])
    .style(theme.style(row.token));

    let item = ListItem::new(line);

    match band {
        Some(band) => item.style(theme.style(band)),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Reserved rather than inserted: an unselected row pays the same column, or
    /// every row would shift sideways as the cursor moved (design §4.2).
    #[test]
    fn the_gutter_is_one_column_on_every_row_and_marked_on_one() {
        let selected = selection_bar(true, Theme::default());
        let unselected = selection_bar(false, Theme::default());

        assert_eq!(selected.content.chars().count(), 1);
        assert_eq!(unselected.content.chars().count(), 1);
        assert_ne!(selected.content, unselected.content);
        assert_eq!(
            selected.style.fg,
            Some(Theme::default().palette().selection_bar)
        );
    }
}
