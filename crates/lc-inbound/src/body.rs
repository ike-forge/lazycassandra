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

use lc_domain::contents::{Order, SelectionMode};
use lc_domain::grid::{BAR, COLUMN_SPACING, MARKS, available, fitting};
use lc_domain::search::Search;
use lc_domain::state::{Ranges, Span};
use lc_domain::style::Token;
use lc_domain::style::{Segment, text};
use lc_domain::view::{
    Activity, CompletionView, GridCell, GridColumn, PanelBody, Position, RowKind, TreeRow,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span as TextSpan, Text};
use ratatui::widgets::{
    Block, BorderType, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
    Wrap,
};
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use crate::hit::{Band, MarkAt};
use crate::theme::Theme;

/// The limb a table hangs off its keyspace by, and the one that closes the
/// run.
const LIMB: &str = "├─ ";
const LAST_LIMB: &str = "└─ ";

const ELLIPSIS: char = '…';

/// Rows of a grid spent on its header rather than on a row: the column names
/// and a blank line under them, so the headings read as chrome above the page
/// rather than as its first row.
pub(crate) const HEADER_ROWS: u16 = 2;

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

/// `line` behind the bar column, marked where the line was wrapped from a row
/// the mark belongs to — every one of them, since the highlight covers every
/// one. Shared by Ddl and Logs, the two bodies whose rows stand one to
/// several lines rather than one to a `List`/`Table` row of their own.
///
/// Prepended after the highlight is patched on, or the bar would be repainted
/// in the selection's own foreground and stop reading as a bar.
pub(crate) fn barred(mut line: Line<'static>, marked: bool, theme: Theme) -> Line<'static> {
    line.spans.insert(0, selection_bar(marked, theme));
    line
}

/// `style` patched onto both the line's own style and each of its spans'.
///
/// Every row shape builds its spans through [`crate::highlight::spans`] now,
/// so a matched fragment can carry [`Token::Match`] distinct from the rest of
/// the line — the colour lives on the spans, not on the line as a whole.
/// Patching the line's own style too still matters: ratatui falls back to it
/// past the end of the widths a line's spans account for, which `patched`
/// itself has no way to know.
pub(crate) fn patched(mut line: Line<'static>, style: Style) -> Line<'static> {
    line.style = line.style.patch(style);
    for span in &mut line.spans {
        span.style = span.style.patch(style);
    }
    line
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
    /// Where each drawn heading's two marks landed. Empty for every body that
    /// is not a grid, and for a column too narrow to draw them in
    /// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §5).
    pub(crate) marks: Vec<Band<MarkAt>>,
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

/// A panel's frame, as the bodies that draw into it need it: the rect inside
/// the borders, the bordered rect itself, and the style those borders wear.
/// Only the grid needs the last two — its heading rule meets the frame the
/// same way the palette's group rules meet theirs — but a body takes one
/// argument for where it is rather than three that can disagree.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Chrome {
    pub(crate) inner: Rect,
    pub(crate) block: Rect,
    pub(crate) border: Style,
}

pub(crate) fn render(
    frame: &mut Frame,
    chrome: Chrome,
    body: &PanelBody,
    search: Option<&Search>,
    focused: bool,
    activity: Activity,
    theme: Theme,
) -> Option<Body> {
    let area = chrome.inner;

    if area.is_empty() {
        return None;
    }

    match body {
        PanelBody::Blank => None,
        PanelBody::Log {
            rows,
            selected,
            offset,
            ranges,
            flash,
        } => {
            let marked = Marked::new(ranges.clone(), *flash);
            let view = crate::logs::LogsView {
                rows,
                selected: *selected,
                offset: *offset,
                marked: &marked,
                search,
                activity,
            };
            Some(log(frame, area, &view, theme))
        }
        PanelBody::Notice { text, token } => {
            notice(frame, area, text, *token, theme);
            None
        }
        PanelBody::Tree {
            rows,
            selected,
            offset,
            ranges,
            flash,
        } => {
            let marked = Marked::new(ranges.clone(), *flash);
            let view = TreeView {
                rows,
                selected: *selected,
                offset: *offset,
                marked: &marked,
                search,
            };
            Some(tree_body(frame, area, &view, theme))
        }
        PanelBody::Details {
            details,
            selected,
            offset,
            ranges,
            flash,
        } => {
            let marked = Marked::new(ranges.clone(), *flash);
            let view = crate::details::DetailsView {
                details,
                selected: *selected,
                offset: *offset,
                marked: &marked,
                search,
            };
            Some(details_body(frame, area, &view, theme))
        }
        PanelBody::Editor {
            lines,
            cursor,
            completion,
        } => Some(Body {
            extent: editor(
                frame,
                area,
                &EditorView {
                    lines,
                    cursor: *cursor,
                    completion: completion.as_ref(),
                },
                focused,
                theme,
            ),
            columns: None,
            bands: Vec::new(),
            marks: Vec::new(),
            unit: Unit::Lines { rows: Vec::new() },
            selection: Span::default(),
        }),
        PanelBody::Grid { .. } => {
            let (extent, horizontal, bands, marks) = grid(frame, chrome, &grid_view(body)?, theme);

            Some(Body {
                extent,
                columns: Some(horizontal),
                bands,
                marks,
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

/// The view model's tree, as the renderer reads it.
///
/// `PanelBody::Log`'s arm, split out so [`render`] stays under clippy's line
/// budget — the same reason [`grid`] is its own function rather than inline
/// in the match.
fn log(frame: &mut Frame, area: Rect, view: &crate::logs::LogsView, theme: Theme) -> Body {
    let (extent, selection, rows) = crate::logs::render(frame, area, view, theme);

    Body {
        extent,
        columns: None,
        bands: Vec::new(),
        marks: Vec::new(),
        unit: Unit::Lines { rows },
        selection,
    }
}

/// `PanelBody::Tree`'s arm, split out for [`log`]'s reason.
fn tree_body(frame: &mut Frame, area: Rect, view: &TreeView, theme: Theme) -> Body {
    Body {
        extent: tree(frame, area, view, theme),
        columns: None,
        bands: Vec::new(),
        marks: Vec::new(),
        unit: Unit::Rows { header: 0 },
        selection: Span::default(),
    }
}

/// `PanelBody::Details`'s arm, split out for [`log`]'s reason.
fn details_body(
    frame: &mut Frame,
    area: Rect,
    view: &crate::details::DetailsView,
    theme: Theme,
) -> Body {
    let (extent, selection, rows) = crate::details::render(frame, area, view, theme);

    Body {
        extent,
        columns: None,
        bands: Vec::new(),
        marks: Vec::new(),
        unit: Unit::Lines { rows },
        selection,
    }
}

/// A struct rather than six more parameters: the fields arrive together from
/// one `PanelBody::Tree` and mean nothing apart, the same reason
/// [`GridView`] exists.
#[derive(Clone, Copy)]
struct TreeView<'a> {
    rows: &'a [TreeRow],
    selected: usize,
    offset: usize,
    marked: &'a Marked,
    search: Option<&'a Search>,
}

fn tree(frame: &mut Frame, area: Rect, view: &TreeView, theme: Theme) -> Extent {
    let TreeView {
        rows,
        selected,
        offset,
        marked,
        search,
    } = *view;

    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let ranged = marked.holds(index);
            item(
                row,
                index == selected || ranged,
                ranged.then_some(marked.band),
                search,
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
    /// [`PanelBody::Grid::rows_ranges`].
    rows_ranges: &'a Ranges,
    /// [`GridView::rows_ranges`]'s counterpart for the column axis.
    columns_ranges: &'a Ranges,
    /// What the two ranges are painted with — [`Token::Ranged`] for a live
    /// selection, the louder [`Token::Flash`] for a yank's afterglow.
    band: Token,
}

fn grid(
    frame: &mut Frame,
    chrome: Chrome,
    view: &GridView,
    theme: Theme,
) -> (Extent, Extent, Vec<Band<usize>>, Vec<Band<MarkAt>>) {
    let area = chrome.inner;

    let GridView {
        columns,
        rows,
        selected,
        offset,
        first_column,
        column,
        mode,
        gutter,
        rows_ranges,
        columns_ranges,
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
        rows_ranges,
        columns_ranges,
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
        let at = selected_column
            .map(|_| column - first_column)
            .map(|at| (columns_at[at].start, drawn_widths[at]));
        crosshair(frame, area, row.map(|row| row - offset), at, theme);
    }

    frame.render_stateful_widget(table, area, &mut state);

    heading_rule(frame, chrome);

    let marks = mark_bands(&columns[drawn], &drawn_widths, &columns_at);

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
        marks,
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
/// indices, never the drawn window's — falls inside the rectangle the row
/// groups and the column groups cross.
///
/// An *empty* axis was never extended, so it constrains nothing: row groups
/// with no column groups are whole rows, and column groups with no row groups
/// are whole columns. The two together are painted only when at least one
/// holds something (multi-group selection design §5, §6).
fn ranged(row: usize, column: usize, rows: &Ranges, columns: &Ranges) -> bool {
    (!rows.is_empty() || !columns.is_empty())
        && (rows.is_empty() || rows.contains(row))
        && (columns.is_empty() || columns.contains(column))
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
        rows_ranges,
        columns_ranges,
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
        rows_ranges,
        columns_ranges,
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
    rows_ranges: &'a Ranges,
    columns_ranges: &'a Ranges,
    band: Token,
}

/// A run of rows marked out, and what it is painted with.
///
/// The two travel together because a range that lost its band would be drawn
/// as an ordinary selection — which is exactly what a yank's afterglow must
/// not look like.
#[derive(Clone)]
pub(crate) struct Marked {
    pub(crate) ranges: Ranges,
    pub(crate) band: Token,
}

impl Marked {
    /// A yank's afterglow inverts what it took rather than taking a fourth
    /// colour. The three bands are one ramp apart, so the loudest of them —
    /// `Selected` — read as barely a change against the `Ranged` band standing
    /// there a frame earlier; swapping ground for text cannot be mistaken for
    /// any of the three, in any palette (visual mode design §6.1).
    const fn band(flash: bool) -> Token {
        if flash { Token::Flash } else { Token::Ranged }
    }

    pub(crate) fn new(ranges: Ranges, flash: bool) -> Self {
        Self {
            ranges,
            band: Self::band(flash),
        }
    }

    /// Nothing marked out. The band is the one an ordinary selection takes,
    /// which nothing paints while `ranges` is empty.
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self::new(Ranges::default(), false)
    }

    pub(crate) fn holds(&self, index: usize) -> bool {
        self.ranges.contains(index)
    }
}

/// One row of the grid's body: the bar-and-gutter cell, then the columns
/// `layout.drawn` names, each patched with `Token::Ranged` where [`ranged`]
/// says its page column falls inside the block.
fn grid_row(index: usize, cells: &[GridCell], layout: &GridRow, theme: Theme) -> Row<'static> {
    let held = layout.rows_ranges.contains(index);
    let marked = index == layout.selected || held;

    // A row range unconstrained on the column axis holds the whole row, and
    // the row's own style is the only thing ratatui paints under the column
    // spacing. Left to the cells, the band would break at every gap and read
    // as one stripe per column rather than one selection — the same continuity
    // `row_highlight_style` gives the cursor row.
    let whole_row = held && layout.columns_ranges.is_empty();

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
                        layout.rows_ranges,
                        layout.columns_ranges,
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
    .style(if whole_row {
        theme.style(layout.band)
    } else {
        Style::default()
    })
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

/// The line under the headings.
///
/// The heading is two rows tall and the second was blank, which read as a gap
/// rather than as a line under a heading. It carries the rule instead, drawn
/// the way the palette draws the rules between its groups: out to the panel's
/// own border, in whatever style that border wears.
fn heading_rule(frame: &mut Frame, chrome: Chrome) {
    if chrome.inner.height >= HEADER_ROWS {
        crate::render::rule(
            frame,
            chrome.block,
            chrome.inner.y + HEADER_ROWS - 1,
            chrome.border,
        );
    }
}

/// The mark saying which way a column is sorted. The arrow points the way the
/// column reads, and the two-headed one is the affordance: the mark is drawn
/// whether or not the grid is sorted, so the key is discoverable and the
/// glyph has somewhere to be clicked
/// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §5).
const SORT_MARKS: [&str; 3] = ["\u{21c5}", "\u{2193}", "\u{2191}"];

/// `nf-fa-filter`, the funnel. A Nerd Font codepoint like the tree's icons
/// and drawn on the same terms (`icons.rs`).
const FILTER_MARK: &str = "\u{f0b0}";

const fn sort_mark(sort: Option<Order>) -> &'static str {
    match sort {
        None => SORT_MARKS[0],
        Some(Order::Descending) => SORT_MARKS[1],
        Some(Order::Ascending) => SORT_MARKS[2],
    }
}

/// A mark in force is drawn in [`Token::Marker`] and an idle one faint: the
/// column being sorted or filtered has to read apart from the eleven beside
/// it that are not (design §5).
const fn mark_token(active: bool) -> Token {
    if active { Token::Marker } else { Token::Faint }
}

/// The grid's heading row: a blank cell over the bar and the gutter, then the
/// drawn columns. `washed` names which of them the cursor is in — an index into
/// what is drawn, not into the page — and the style to wash it with; `None`
/// where the mode paints no band, or where the cursor's column is not one of
/// these.
///
/// Each heading is its name, a blank, and its two marks — the [`MARKS`]
/// columns the domain measured the heading with, so the marks never push the
/// name out of its own column (design §5).
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

                    heading_cell(column, *width, style, theme)
                }),
        ),
    )
    .height(HEADER_ROWS)
}

/// One heading: the name in the column's own style, then the two marks in
/// theirs. Below [`MARKS`] columns there is no room for them and the name has
/// the cell to itself — a truncated funnel says nothing.
fn heading_cell(column: &GridColumn, width: u16, style: Style, theme: Theme) -> Cell<'static> {
    if width < MARKS {
        return styled_cell(&column.name, Some(style), width);
    }

    let name = truncated(&column.name, width - MARKS);

    // The marks patch onto the heading's own style rather than replacing it,
    // so a column-mode wash still reads across the whole cell and only the
    // foreground says whether the mark is in force.
    Cell::from(Line::from(vec![
        TextSpan::styled(format!("{name} "), style),
        TextSpan::styled(
            sort_mark(column.sort),
            style.patch(theme.style(mark_token(column.sort.is_some()))),
        ),
        TextSpan::styled(
            FILTER_MARK,
            style.patch(theme.style(mark_token(column.filtered))),
        ),
    ]))
}

/// Where each drawn column's two marks landed, for a press to be resolved
/// against — the same rule the column bands follow: assembled from what
/// actually drew, never computed a second time (mouse input design §4).
fn mark_bands(
    columns: &[GridColumn],
    widths: &[u16],
    columns_at: &[Band<usize>],
) -> Vec<Band<MarkAt>> {
    columns
        .iter()
        .zip(widths)
        .zip(columns_at)
        .filter(|((_, width), _)| **width >= MARKS)
        .flat_map(|((column, width), band)| {
            let name = truncated(&column.name, width - MARKS).width();
            let sort = band
                .start
                .saturating_add(u16::try_from(name).unwrap_or(0) + 1);

            [
                (sort, lc_domain::pointer::Mark::Sort),
                (sort.saturating_add(1), lc_domain::pointer::Mark::Filter),
            ]
            .map(|(start, mark)| Band {
                start,
                end: start.saturating_add(1),
                names: MarkAt {
                    column: band.names,
                    mark,
                },
            })
        })
        .collect()
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

fn item(
    row: &TreeRow,
    marked: bool,
    band: Option<Token>,
    search: Option<&Search>,
    theme: Theme,
) -> ListItem<'static> {
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

    let mut spans = vec![selection_bar(marked, theme), TextSpan::raw(limb), icon];
    spans.extend(crate::highlight::spans(
        &row.label,
        search,
        theme.style(row.token),
        theme,
    ));

    let line = Line::from(spans);

    let item = ListItem::new(line);

    match band {
        Some(band) => item.style(theme.style(band)),
        None => item,
    }
}

/// What the editor draws: its styled lines, its cursor, and whatever is being
/// offered into it.
pub(crate) struct EditorView<'a> {
    pub(crate) lines: &'a [Vec<Segment>],
    pub(crate) cursor: Position,
    pub(crate) completion: Option<&'a CompletionView>,
}

/// One buffer line is one screen row: long lines scroll sideways rather than
/// wrap, so the domain keeps counting its own lines and the cursor's own row
/// is always on screen (queries editor design §5).
fn editor(frame: &mut Frame, area: Rect, view: &EditorView, focused: bool, theme: Theme) -> Extent {
    let height = usize::from(area.height);
    let width = usize::from(area.width);

    let current = view.lines.get(view.cursor.line).map(|line| text(line));
    let current = current.as_deref().unwrap_or_default();
    let column = width_of(current, view.cursor.column);

    let top = window(view.cursor.line, height);
    let left = left_edge(current, column, width);

    let drawn = Text::from(
        view.lines
            .iter()
            .skip(top)
            .take(height)
            .map(|line| Line::from(shifted(line, left, theme)))
            .collect::<Vec<_>>(),
    );

    frame.render_widget(Paragraph::new(drawn), area);

    let cell = (
        area.x + narrowed(column - left),
        area.y + narrowed(view.cursor.line - top),
    );

    if focused {
        frame.set_cursor_position(cell);
    }

    // After the buffer and after the cursor: the popup is drawn over whatever
    // it hangs across, which is the panels above it as often as the editor's
    // own rows (query editor design §5.6).
    if let Some(completion) = view.completion.filter(|_| focused) {
        popup(frame, cell, completion, theme);
    }

    Extent {
        position: top,
        total: view.lines.len(),
        visible: height,
    }
}

/// Columns the popup spends on chrome: a border down each side.
const POPUP_CHROME: u16 = 2;

/// The tallest popup, in rows of offers. Past this the list is what the next
/// character narrows, not what a taller box shows (query editor design §5.3).
const POPUP_ROWS: u16 = 8;

/// The offers, in a box hanging off the cursor.
///
/// Below the cursor where the frame has room and above it where it has not —
/// the Queries panel sits on the bottom edge of the frame, so above is the
/// usual answer and below is the exception (design §5.6).
fn popup(frame: &mut Frame, cell: (u16, u16), completion: &CompletionView, theme: Theme) {
    let frame_area = frame.area();
    let widest = completion
        .rows
        .iter()
        .map(|row| row.text.width())
        .max()
        .unwrap_or_default();

    let rows = narrowed(completion.rows.len()).min(POPUP_ROWS);
    let width = narrowed(widest)
        .saturating_add(POPUP_CHROME)
        .min(frame_area.width);
    let height = rows.saturating_add(POPUP_CHROME);
    if rows == 0 || width <= POPUP_CHROME || height > frame_area.height {
        return;
    }

    let (cursor_x, cursor_y) = cell;
    let below = cursor_y.saturating_add(1);
    let y = if below.saturating_add(height) <= frame_area.bottom() {
        below
    } else {
        cursor_y.saturating_sub(height)
    };
    let x = cursor_x.min(frame_area.right().saturating_sub(width));
    let area = Rect::new(x, y, width, height);

    // The first row on screen is the selected one once the selection has
    // walked past what fits: the same window arithmetic the buffer itself
    // scrolls by.
    let first = window(completion.selected, usize::from(rows));
    let items: Vec<ListItem> = completion
        .rows
        .iter()
        .enumerate()
        .skip(first)
        .take(usize::from(rows))
        .map(|(index, row)| {
            let style = if index == completion.selected {
                theme.style(Token::Selected)
            } else {
                theme.style(row.token)
            };

            ListItem::new(Line::from(TextSpan::styled(row.text.clone(), style)))
        })
        .collect();

    frame.render_widget(Clear, area);
    frame.render_widget(
        List::new(items).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(theme.style(Token::Faint)),
        ),
        area,
    );
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

/// `line` with its first `columns` terminal columns scrolled off to the left,
/// as the spans the meanings in it are drawn with.
///
/// The offset is applied to the whole line rather than per segment, so a
/// segment scrolled off entirely contributes nothing and the one straddling
/// the offset contributes its tail.
fn shifted(line: &[Segment], columns: usize, theme: Theme) -> Vec<TextSpan<'static>> {
    let mut used = 0;
    let mut spans = Vec::with_capacity(line.len());

    for segment in line {
        let mut out = String::new();

        for character in segment.text.chars() {
            if used >= columns {
                out.push(character);
            } else {
                used += character.width().unwrap_or(0);
            }
        }

        if out.is_empty() {
            continue;
        }

        let style = segment
            .token
            .map_or_else(Style::default, |token| theme.style(token));
        spans.push(TextSpan::styled(out, style));
    }

    spans
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
