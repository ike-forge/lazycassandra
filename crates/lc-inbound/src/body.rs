//! Drawing what a panel contains. Tables, Contents and Ddl carry offsets as
//! domain state; Queries derives both of its offsets from the cursor each
//! frame. Only Ddl reports a selection span: its rows wrap, which only the
//! renderer can measure.

use std::ops::Range;

use lc_domain::contents::{Order, SelectionMode};
use lc_domain::grid::{BAR, COLUMN_SPACING, MARKS, available, fitting};
use lc_domain::queries::{CqlMode, Highlight};
use lc_domain::search::Search;
use lc_domain::selection::Ranges;
use lc_domain::style::Token;
use lc_domain::style::{Segment, text};
use lc_domain::view::{
    Activity, CompletionView, GridCell, GridColumn, PanelBody, Position, RowKind, TreeRow,
};
use lc_domain::viewport::Span;
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

const LIMB: &str = "├─ ";
const LAST_LIMB: &str = "└─ ";

const ELLIPSIS: char = '…';

/// Names and a blank rule under them, so headings read as chrome.
pub(crate) const HEADER_ROWS: u16 = 2;

/// A half-block: one column wide in every font we can assume.
const SELECTION_BAR: char = '▌';

/// Reserved on every row so rows never shift as the cursor moves.
pub(crate) fn selection_bar(marked: bool, theme: Theme) -> TextSpan<'static> {
    if marked {
        TextSpan::styled(SELECTION_BAR.to_string(), theme.style(Token::SelectionBar))
    } else {
        TextSpan::raw(" ")
    }
}

/// Prepended after the highlight is patched on, or the bar would take the
/// selection's foreground.
pub(crate) fn barred(mut line: Line<'static>, marked: bool, theme: Theme) -> Line<'static> {
    line.spans.insert(0, selection_bar(marked, theme));
    line
}

/// The line style too: ratatui falls back to it past the spans' widths.
pub(crate) fn patched(mut line: Line<'static>, style: Style) -> Line<'static> {
    line.style = line.style.patch(style);
    for span in &mut line.spans {
        span.style = span.style.patch(style);
    }
    line
}

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

pub(crate) struct Body {
    pub(crate) extent: Extent,
    /// Only a grid scrolls sideways.
    pub(crate) columns: Option<Extent>,
    /// Empty for every body but a grid.
    pub(crate) bands: Vec<Band<usize>>,
    /// Empty for every body but a grid, and for a column too narrow for them.
    pub(crate) marks: Vec<Band<MarkAt>>,
    pub(crate) unit: Unit,
    /// Only Ddl reports one: its selection can span several lines.
    pub(crate) selection: Span,
}

/// "These are rows" and "this many are spent above the first" cannot be said
/// apart.
#[derive(Clone)]
pub(crate) enum Unit {
    /// `header` rows are the panel's own chrome above the first row drawn.
    Rows { header: u16 },
    /// Empty for the Queries editor, whose lines belong to no rows.
    Lines { rows: Vec<Option<usize>> },
}

/// One argument for where a body is, rather than three that can disagree.
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
        PanelBody::Editor { .. } => Some(editor_body(frame, area, body, focused, theme)),
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

/// `None` is load-bearing: ratatui walks a supplied selection into view, which
/// would undo the stored scroll.
const fn visible(selected: usize, offset: usize, height: usize) -> Option<usize> {
    if selected >= offset && selected < offset + height {
        Some(selected)
    } else {
        None
    }
}

/// Split out to keep [`render`] under clippy's line budget.
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

/// Split out for [`log`]'s reason.
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

/// Split out for [`log`]'s reason.
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

    // Read back, so the hit map reflects what actually drew.
    Extent {
        position: state.offset(),
        total: rows.len(),
        visible: height,
    }
}

#[derive(Clone, Copy)]
struct GridView<'a> {
    columns: &'a [GridColumn],
    rows: &'a [Vec<GridCell>],
    total: usize,
    selected: usize,
    offset: usize,
    first_column: usize,
    column: usize,
    mode: SelectionMode,
    gutter: u16,
    rows_ranges: &'a Ranges,
    columns_ranges: &'a Ranges,
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
        total,
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

    // Shared by `visible` and the scrollbar: two copies could disagree by a row
    // and scroll the widget away.
    let height = usize::from(area.height).saturating_sub(usize::from(HEADER_ROWS));

    let widths: Vec<u16> = columns.iter().map(|column| column.width).collect();
    let available = available(area.width, gutter);
    let first_column = first_column.min(columns.len());
    let shown = fitting(&widths, first_column, available);
    let drawn = first_column..first_column + shown;
    // `Constraint::Length` is a request: given more than the area, ratatui
    // shrinks every column, gutter included, with no ellipsis.
    let drawn_widths: Vec<u16> = widths[drawn.clone()]
        .iter()
        .map(|width| (*width).min(available))
        .collect();

    let constraints = std::iter::once(Constraint::Length(BAR + gutter))
        .chain(drawn_widths.iter().copied().map(Constraint::Length));

    let columns_at = bands(area, &drawn_widths, first_column, gutter);

    let row = visible(selected, offset, height);
    // The heading wash goes with the crosshair, so it is off when the row
    // cursor scrolls out of view.
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
        .map(|(index, cells)| grid_row(offset + index, cells, &row_layout, theme));

    let table = Table::new(body, constraints)
        .header(header)
        .column_spacing(COLUMN_SPACING);

    // The bar-and-gutter column sits first, so the selected column is one
    // further right.
    let drawn_column = column.saturating_sub(first_column) + 1;
    // `None` off the right edge: ratatui would clamp to the last drawn column
    // and band the wrong one. A resize reaches this for a frame.
    let selected_column = drawn.contains(&column).then_some(drawn_column);

    // A row scrolled out of view is `None`; a column selection is independent
    // of the row viewport.
    let empty = TableState::default();
    let drawn_row = row.map(|row| row - offset);
    let (table, mut state) = match mode {
        SelectionMode::Cell => (
            table.cell_highlight_style(cursor_cell(view, theme)),
            empty.with_selected_cell(drawn_row.map(|row| (row, drawn_column))),
        ),
        SelectionMode::Row => (
            table.row_highlight_style(theme.style(Token::Selected)),
            empty.with_selected(drawn_row),
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
        crosshair(frame, area, drawn_row, at, theme);
    }

    frame.render_stateful_widget(table, area, &mut state);

    heading_rule(frame, chrome);

    let marks = mark_bands(&columns[drawn], &drawn_widths, &columns_at);

    (
        Extent {
            position: offset + state.offset(),
            total,
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

/// `SelectedCell` and `Flash` both invert, and two inversions cancel; the flash
/// un-inverts the cursor's cell and keeps its underline.
fn cursor_cell(view: &GridView, theme: Theme) -> Style {
    let style = theme.style(Token::SelectedCell);

    if matches!(view.band, Token::Flash)
        && ranged(
            view.selected,
            view.column,
            view.rows_ranges,
            view.columns_ranges,
        )
    {
        style.remove_modifier(Modifier::REVERSED)
    } else {
        style
    }
}

/// Washed under the table, not as ratatui highlight styles, which would bury a
/// held range's background. `row` counts from the first drawn row; `column` is
/// in frame columns.
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

/// Page indices. An empty axis constrains nothing; both empty paints nothing.
fn ranged(row: usize, column: usize, rows: &Ranges, columns: &Ranges) -> bool {
    (!rows.is_empty() || !columns.is_empty())
        && (rows.is_empty() || rows.contains(row))
        && (columns.is_empty() || columns.contains(column))
}

/// `None` is unreachable: the caller matched `Grid`.
fn grid_view(body: &PanelBody) -> Option<GridView<'_>> {
    let PanelBody::Grid {
        columns,
        rows,
        total,
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
        total: *total,
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

/// Together, so a flash cannot lose its band and draw as a selection.
#[derive(Clone)]
pub(crate) struct Marked {
    pub(crate) ranges: Ranges,
    pub(crate) band: Token,
}

impl Marked {
    /// Inverted: the three bands are a ramp too close to read as feedback.
    const fn band(flash: bool) -> Token {
        if flash { Token::Flash } else { Token::Ranged }
    }

    pub(crate) fn new(ranges: Ranges, flash: bool) -> Self {
        Self {
            ranges,
            band: Self::band(flash),
        }
    }

    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self::new(Ranges::default(), false)
    }

    pub(crate) fn holds(&self, index: usize) -> bool {
        self.ranges.contains(index)
    }
}

fn grid_row(index: usize, cells: &[GridCell], layout: &GridRow, theme: Theme) -> Row<'static> {
    let held = layout.rows_ranges.contains(index);
    let marked = index == layout.selected || held;

    // A whole-row range paints the row style so the band spans the column gaps.
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
                // The offset is into the drawn window; add `first_column` for
                // the page column.
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

/// Laid out from the `Table`'s own constraints. Each band claims the spacing
/// after it, so a press between cells names one.
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

/// Drawn out to the panel border, as the palette's group rules are.
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

/// The two-headed arrow is drawn when unsorted, so the key is discoverable and
/// clickable.
const SORT_MARKS: [&str; 3] = ["\u{21c5}", "\u{2193}", "\u{2191}"];

/// A Nerd Font codepoint, on the tree icons' terms.
const FILTER_MARK: &str = "\u{f0b0}";

const fn sort_mark(sort: Option<Order>) -> &'static str {
    match sort {
        None => SORT_MARKS[0],
        Some(Order::Descending) => SORT_MARKS[1],
        Some(Order::Ascending) => SORT_MARKS[2],
    }
}

/// A mark in force must read apart from the idle ones.
const fn mark_token(active: bool) -> Token {
    if active { Token::Marker } else { Token::Faint }
}

/// `washed` indexes the drawn columns. Each heading spends [`MARKS`] columns on
/// its marks.
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
                    // A projection's columns carry no token, and the wash adds
                    // none.
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

/// Below [`MARKS`] the name gets the cell: a truncated funnel says nothing.
fn heading_cell(column: &GridColumn, width: u16, style: Style, theme: Theme) -> Cell<'static> {
    if width < MARKS {
        return styled_cell(&column.name, Some(style), width);
    }

    let name = truncated(&column.name, width - MARKS);

    // Marks patch the heading style, so a column-mode wash reads across the
    // whole cell.
    Cell::from(Line::from(vec![
        TextSpan::styled(format!("{name} "), style),
        TextSpan::styled(
            sort_mark(column.sort),
            style.patch(theme.style(mark_token(column.sort.is_some()))),
        ),
        TextSpan::styled(" ", style),
        TextSpan::styled(
            FILTER_MARK,
            style.patch(theme.style(mark_token(column.filtered))),
        ),
    ]))
    .style(style)
}

/// Assembled from what drew, like the column bands.
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
                (sort.saturating_add(2), lc_domain::pointer::Mark::Filter),
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

fn number(index: usize, gutter: u16, theme: Theme) -> TextSpan<'static> {
    let text = format!("{:>width$}", index + 1, width = usize::from(gutter));

    TextSpan::styled(text, theme.style(Token::Muted))
}

/// Composes onto the cell's token. `Match` overwrites `fg`, so a matched null
/// and matched "null" draw alike.
fn cell_style(cell: &GridCell, theme: Theme) -> Option<Style> {
    // Dirty outranks the column colour; a match still composes over it.
    let base = if cell.dirty {
        Some(theme.style(Token::Dirty))
    } else {
        cell.token.map(|token| theme.style(token))
    };

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

/// ratatui clips silently; the ellipsis says the value continues.
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
    // Indentation is the tree's shape; no fold marker.
    let limb = match row.kind {
        RowKind::Keyspace => "",
        RowKind::Table { last: false } => LIMB,
        RowKind::Table { last: true } => LAST_LIMB,
    };

    // Only the icon is bold, so a font without a bold Nerd cut loses nothing.
    // It keeps its kind's colour while a system keyspace's name goes faint.
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

/// No rows, so an empty hit map.
fn editor_body(
    frame: &mut Frame,
    area: Rect,
    body: &PanelBody,
    focused: bool,
    theme: Theme,
) -> Body {
    let PanelBody::Editor {
        lines,
        cursor,
        mode,
        band,
        completion,
    } = body
    else {
        unreachable!("the caller matched on this arm");
    };

    Body {
        extent: editor(
            frame,
            area,
            &EditorView {
                lines,
                cursor: *cursor,
                band: *band,
                completion: completion.as_ref(),
            },
            focused && *mode == CqlMode::Editing,
            theme,
        ),
        columns: None,
        bands: Vec::new(),
        marks: Vec::new(),
        unit: Unit::Lines { rows: Vec::new() },
        selection: Span::default(),
    }
}

pub(crate) struct EditorView<'a> {
    pub(crate) lines: &'a [Vec<Segment>],
    pub(crate) cursor: Position,
    /// Banded as a list bands its cursor row.
    pub(crate) band: Option<Highlight>,
    pub(crate) completion: Option<&'a CompletionView>,
}

/// One buffer line per screen row: long lines scroll sideways, so the domain's
/// line count holds and the cursor stays on screen.
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
            .enumerate()
            .skip(top)
            .take(height)
            .map(|(at, line)| {
                let band = banded(view.band, at, line);
                Line::from(shifted(line, left, band.as_ref(), theme))
            })
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

    // Last: the popup draws over whatever it crosses, panels above included.
    if let Some(completion) = view.completion.filter(|_| focused) {
        popup(frame, cell, completion, theme);
    }

    Extent {
        position: top,
        total: view.lines.len(),
        visible: height,
    }
}

const POPUP_CHROME: u16 = 2;

/// Past this the next character narrows the list.
const POPUP_ROWS: u16 = 8;

/// Above the cursor by default: Queries sits at the bottom of the frame.
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

    // The same window arithmetic the buffer scrolls by.
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

/// Used for both axes, recomputed every frame rather than stored.
const fn window(index: usize, span: usize) -> usize {
    index.saturating_sub(span.saturating_sub(1))
}

/// Snapped up so a double-width character straddling the offset does not shift
/// the text and cursor.
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

fn width_of(line: &str, chars: usize) -> usize {
    line.chars()
        .take(chars)
        .map(|character| character.width().unwrap_or(0))
        .sum()
}

/// Counts `char`s, so no display width is measured.
fn banded(band: Option<Highlight>, at: usize, line: &[Segment]) -> Option<Range<usize>> {
    let band = band?;
    if at < band.start.line || at > band.end.line {
        return None;
    }

    let from = if at == band.start.line {
        band.start.column
    } else {
        0
    };
    let to = if at == band.end.line {
        band.end.column
    } else {
        line.iter()
            .map(|segment| segment.text.chars().count())
            .sum()
    };

    (from < to).then_some(from..to)
}

/// Offset applied to the whole line; runs are rebuilt per character and
/// coalesced, since the band cuts across segments.
fn shifted(
    line: &[Segment],
    columns: usize,
    band: Option<&Range<usize>>,
    theme: Theme,
) -> Vec<TextSpan<'static>> {
    // Background only, so it composes with the character's role.
    let ground = theme.style(Token::Selected);
    let mut used = 0;
    let mut at = 0;
    let mut spans: Vec<TextSpan<'static>> = Vec::with_capacity(line.len());

    for segment in line {
        let base = segment
            .token
            .map_or_else(Style::default, |token| theme.style(token));

        for character in segment.text.chars() {
            let index = at;
            at += 1;

            if used < columns {
                used += character.width().unwrap_or(0);
                continue;
            }

            let style = if band.is_some_and(|band| band.contains(&index)) {
                base.patch(ground)
            } else {
                base
            };

            match spans.last_mut() {
                Some(last) if last.style == style => last.content.to_mut().push(character),
                _ => spans.push(TextSpan::styled(character.to_string(), style)),
            }
        }
    }

    spans
}

/// Callers pass values bounded by an area dimension.
fn narrowed(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reserved, not inserted, so unselected rows do not shift.
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
