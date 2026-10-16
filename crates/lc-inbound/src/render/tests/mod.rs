use crate::body::Marked;
use lc_domain::commands::CommandsPanel;
use lc_domain::connection::{Connection, ConnectionState, SchemaState};
use lc_domain::contents::{SelectionMode, Subject, TableRef};
use lc_domain::ddl::DdlTab;
use lc_domain::feedback::Flash;
use lc_domain::geometry::Geometry;
use lc_domain::grid::{BAR, COLUMN_SPACING};
use lc_domain::model::{
    ClusteringColumn, ClusteringOrder, Column, ConnId, CqlValue, Index, IndexKind, IndexTarget,
    Key, Keyspace, Page, PrimaryKey, QueryId, Replication, Schema, Statement, Table, TableOptions,
};
use lc_domain::overlay::{Message, Overlay};
use lc_domain::panel::Panel;
use lc_domain::pointer::{Click, Mark};
use lc_domain::posture::Posture;
use lc_domain::search::{Prompt, Search, Searched};
use lc_domain::selection::{Range, Ranges, Selection};
use lc_domain::style::Severity;
use lc_domain::view::{Footer, PanelBody};
use lc_domain::{Ported, ThemeChoice};
use lc_domain::{State, view_model};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Position;
use ratatui::style::{Color, Modifier};
use std::collections::BTreeMap;

use super::*;
use crate::layout::QueriesHeight;

mod bar;
mod ddl;
mod focus;
mod footer;
mod frame;
mod grid;
mod grid_scroll;
mod hit_map;
mod measure;
mod palette;
mod queries;
mod scroll;
mod search;
mod tree;

fn held(anchor: usize, cursor: usize) -> Selection {
    let mut selection = Selection::default();
    selection.start(anchor);
    selection.stop(cursor);
    selection
}

fn draw_focused(width: u16, height: u16, focus: Panel) -> Buffer {
    let mut state = State::default();
    state.focus_on(focus);
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

    terminal
        .draw(|frame| {
            render(frame, &view_model(&state), Theme::default());
        })
        .expect("TestBackend always draws");

    terminal.backend().buffer().clone()
}

fn draw(width: u16, height: u16) -> Buffer {
    draw_focused(width, height, Panel::default())
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .filter_map(|x| buffer.cell((x, y)))
        .map(Cell::symbol)
        .collect()
}

fn table(name: &str) -> Table {
    Table {
        name: name.to_owned(),
        primary_key: PrimaryKey {
            partition: vec![column("id", "uuid")],
            clustering: Vec::new(),
        },
        static_columns: Vec::new(),
        regular_columns: Vec::new(),
        indexes: Vec::new(),
        options: TableOptions::default(),
    }
}

fn column(name: &str, cql_type: &str) -> Column {
    Column {
        name: name.to_owned(),
        cql_type: cql_type.to_owned(),
    }
}

fn schema_of(names: &[&str]) -> Schema {
    tabled_schema(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
}

fn tabled_schema(keyspaces: &[(&str, &[&str])]) -> Schema {
    Schema {
        keyspaces: keyspaces
            .iter()
            .map(|(name, tables)| {
                (
                    (*name).to_owned(),
                    Keyspace {
                        name: (*name).to_owned(),
                        replication: Replication::SimpleStrategy { factor: 1 },
                        tables: tables
                            .iter()
                            .map(|t| ((*t).to_owned(), table(t)))
                            .collect::<BTreeMap<_, _>>(),
                    },
                )
            })
            .collect(),
    }
}

/// Rows of body the Tables panel has at 40x12, where these tests draw it.
const TABLES_ROWS: usize = 3;

fn browsing(names: &[&str], selected: usize) -> State {
    let mut state = State::default();
    state.connection = ConnectionState::Connected(Connection {
        id: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
        posture: Posture::ReadOnly,
        schema: SchemaState::Loaded(schema_of(names)),
    });
    state.move_tree_cursor(selected);
    // Set with the cursor for the reason `select_contents` sets it: half a
    // state is not a state (list viewport design §6).
    state.tables.scroll.follow(selected, TABLES_ROWS);
    state
}

fn draw_state(width: u16, height: u16, state: &State) -> Buffer {
    draw_state_with(width, height, state, Theme::default())
}

fn draw_state_with(width: u16, height: u16, state: &State, theme: Theme) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

    terminal
        .draw(|frame| {
            render(frame, &view_model(state), theme);
        })
        .expect("TestBackend always draws");

    terminal.backend().buffer().clone()
}

fn draw_view(width: u16, height: u16, view: &ViewModel) -> Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

    terminal
        .draw(|frame| {
            render(frame, view, Theme::default());
        })
        .expect("TestBackend always draws");

    terminal.backend().buffer().clone()
}

fn banded(buffer: &Buffer, row: u16) -> bool {
    let band = Theme::default()
        .style(Token::Ranged)
        .bg
        .expect("a ranged row spends a background");

    buffer.cell((1, 1 + row)).expect("inside the panel").bg == band
}

/// A state showing `page` for `shop.orders`, whose partition key is `id`.
fn showing(page: Page) -> State {
    let mut state = State::default();
    state.connection = ConnectionState::Connected(Connection {
        id: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
        posture: Posture::ReadOnly,
        schema: SchemaState::Loaded(tabled_schema(&[("shop", &["orders"])])),
    });
    state.focus_on(Panel::Contents);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        lc_domain::model::Statement::select_all("shop", "orders"),
    );
    state.contents.page_ready(
        query,
        page,
        lc_domain::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state
}

fn page_of(columns: &[&str], rows: &[&[CqlValue]]) -> Page {
    Page {
        columns: columns.iter().map(|name| (*name).to_owned()).collect(),
        rows: rows
            .iter()
            .map(|values| lc_domain::model::Row {
                values: values.to_vec(),
            })
            .collect(),
        next: None,
    }
}

fn text(value: &str) -> CqlValue {
    CqlValue::Text(value.to_owned())
}

/// Places the cursor and the viewport that belongs with it, as `update`
/// does — a fixture that sets one without the other describes a state the
/// application cannot be in
/// (docs/superpowers/specs/2026-08-09-list-viewport-design.md §6).
fn select_contents(state: &mut State, index: usize, height: usize) {
    let len = state.contents.row_count();
    state.contents.cursor.go_to(index, len);
    state.contents.scroll.follow(index, height);
}

/// A page of `columns` columns and `rows` rows, every value as wide as its
/// heading.
fn sized_page(columns: usize, rows: usize) -> Page {
    Page {
        columns: (0..columns).map(|index| format!("c{index}")).collect(),
        rows: (0..rows)
            .map(|row| lc_domain::model::Row {
                values: (0..columns).map(|_| text(&format!("v{row}"))).collect(),
            })
            .collect(),
        next: None,
    }
}

/// A twelve-column page whose column cursor visited `path` in order, with
/// the horizontal viewport following it the way `update` does.
///
/// Twelve two-wide columns in the 22-column body a 40x12 frame gives the
/// Contents panel, less the merged bar+gutter block and its one spacing
/// column: six fit, so anything past the sixth has to be reached by
/// scrolling.
fn showing_column_path(path: &[usize]) -> State {
    walked(22, path)
}

/// The state the cursor reaches by visiting `path` in a Contents panel
/// `inner` columns wide. The width is the fixture's own, because the
/// viewport it walks has to be the one the frame it is drawn in draws.
fn walked(inner: u16, path: &[usize]) -> State {
    let page = sized_page(12, 3);
    let widths = lc_domain::grid::widths(&page, lc_domain::config::MaxColumnWidth::default());
    let mut state = showing(page);
    state.contents.mode = SelectionMode::Cell;
    let available = lc_domain::grid::available(inner, 1);

    for column in path {
        state.contents.column.go_to(*column, widths.len());
        state
            .contents
            .column_scroll
            .follow(*column, &widths, available);
    }

    state
}

/// The y the `row`th row of a grid's body lands on: the panel's own
/// border, then the heading and the blank line under it.
const fn grid_row_y(area: Rect, row: u16) -> u16 {
    area.y + 1 + crate::body::HEADER_ROWS + row
}

/// The x of the first drawn column's first cell: the panel's own border,
/// the merged bar+gutter block, and the spacing that follows it.
/// `gutter` is what `lc_domain::grid::gutter` gives the page — the digits
/// of its last row number.
const fn first_cell(area: Rect, gutter: u16) -> u16 {
    area.x + 1 + BAR + gutter + COLUMN_SPACING
}

/// The same for the `index`th drawn column, where every column before it
/// is `width` wide.
const fn nth_cell(area: Rect, gutter: u16, width: u16, index: u16) -> u16 {
    first_cell(area, gutter) + index * (width + COLUMN_SPACING)
}

/// [`nth_cell`] for the fixtures `sized_page` builds: two-character names
/// and two-character values over a page of fewer than ten rows, so the
/// heading's own marks are what set the width
/// (`lc_domain::grid::MARKS`).
const fn drawn_column(area: Rect, index: u16) -> u16 {
    nth_cell(area, 1, 2 + lc_domain::grid::MARKS, index)
}

/// `shop.orders_by_customer` as the fixture declares it.
fn orders() -> Table {
    Table {
        name: "orders_by_customer".to_owned(),
        primary_key: PrimaryKey {
            partition: vec![column("customer_id", "uuid")],
            clustering: vec![
                ClusteringColumn {
                    column: column("order_date", "date"),
                    order: ClusteringOrder::Descending,
                },
                ClusteringColumn {
                    column: column("order_id", "timeuuid"),
                    order: ClusteringOrder::Ascending,
                },
            ],
        },
        static_columns: Vec::new(),
        regular_columns: Vec::new(),
        indexes: Vec::new(),
        options: TableOptions::new(
            [
                ("comment", "Order list."),
                ("gc_grace_seconds", "864000"),
                ("bloom_filter_fp_chance", "0.01"),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
        ),
    }
}

/// A state with `shop` unfolded and the cursor on its one table.
fn describing(table: Table) -> State {
    let schema = Schema {
        keyspaces: BTreeMap::from([(
            "shop".to_owned(),
            Keyspace {
                name: "shop".to_owned(),
                replication: Replication::SimpleStrategy { factor: 1 },
                tables: BTreeMap::from([(table.name.clone(), table)]),
            },
        )]),
    };
    let mut state = State::default();
    state.connection = ConnectionState::Connected(Connection {
        id: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
        posture: Posture::ReadOnly,
        schema: SchemaState::Loaded(schema.clone()),
    });
    state.tables.open(&schema);
    state.tables.open(&schema);
    state
}

/// The Ddl panel's top border at 104x41; its body opens on the row below.
const DDL_BORDER: u16 = 16;
const DDL_BODY: u16 = DDL_BORDER + 1;
/// One past the body's last row: the panel's bottom border.
const DDL_END: u16 = 39;

/// Inside the Ddl panel's border at 104x41: twenty-two rows of 23 columns.
/// The body's lines, the selection bar's column included — so a row opens
/// with the blank it reserves for the bar and a heading, which is no cursor
/// stop, opens at the margin.
fn ddl_rows(buffer: &Buffer) -> Vec<String> {
    (DDL_BODY..DDL_END)
        .map(|y| {
            (1..25)
                .filter_map(|x| buffer.cell((x, y)))
                .map(Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// `describing`, moved to the panel's second tab.
fn listing(table: Table) -> State {
    let mut state = describing(table);
    state.ddl.show(DdlTab::Columns);
    state
}

/// The `Details` the Ddl panel would draw for `state`.
fn ddl_details(state: &State) -> lc_domain::view::Details {
    let PanelBody::Details { details, .. } = view_model(state).panels[1].body.clone() else {
        panic!("a selected row has something to describe");
    };
    details
}

fn geometry_of(width: u16, height: u16, state: &State) -> Geometry {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");
    let mut geometry = Geometry::default();

    terminal
        .draw(|frame| {
            geometry = render(frame, &view_model(state), Theme::default()).geometry;
        })
        .expect("TestBackend always draws");

    geometry
}

fn draw_mapped(width: u16, height: u16, state: &State) -> (Buffer, FrameMap) {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");
    let mut map = FrameMap::default();

    terminal
        .draw(|frame| {
            map = render(frame, &view_model(state), Theme::default()).map;
        })
        .expect("TestBackend always draws");

    (terminal.backend().buffer().clone(), map)
}

/// The drawn frame and where the terminal cursor ended up, if it is shown.
fn drawn_with_cursor(
    width: u16,
    height: u16,
    state: &State,
) -> (Buffer, Option<ratatui::layout::Position>) {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

    terminal
        .draw(|frame| {
            render(frame, &view_model(state), Theme::default());
        })
        .expect("TestBackend always draws");

    let backend = terminal.backend();
    let cursor = backend.cursor_visible().then(|| backend.cursor_position());

    (backend.buffer().clone(), cursor)
}

/// The colour, background and modifier of the first cell whose row
/// contains `needle` — compared field by field rather than as a whole
/// `Style`, because a `Cell` has no way to say a colour was never set
/// versus set to the terminal's default, so a `Style` rebuilt from one
/// cannot be asserted equal to the `Style` a `Theme` hands out.
fn style_of_first(buffer: &Buffer, needle: &str) -> (Color, Color, Modifier) {
    for y in 0..buffer.area.height {
        let text = row(buffer, y);
        if let Some(byte_at) = text.find(needle) {
            // `str::find` returns a byte offset; the border glyphs before
            // it (`╭`, `─`, …) are multi-byte in UTF-8 but one column
            // each, so the column is the count of `char`s before it, not
            // the byte offset itself.
            let column = text[..byte_at].chars().count();
            let x = u16::try_from(column).expect("a drawn row's width fits u16");
            let cell = buffer.cell((x, y)).expect("a found position is in bounds");
            return (cell.fg, cell.bg, cell.modifier);
        }
    }
    panic!("{needle:?} was not drawn");
}

fn with_palette(width: u16, height: u16) -> Buffer {
    let mut state = State::default();
    state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

    terminal
        .draw(|frame| {
            render(frame, &view_model(&state), Theme::default());
        })
        .expect("TestBackend always draws");

    terminal.backend().buffer().clone()
}

/// The row a bordered block's top edge sits on, found by its left corner.
///
/// The scan stops at the block's own right corner: panels sit side by
/// side, so a row read to the frame's edge carries the titles of every
/// block to the right of this one as well.
fn top_border(buffer: &Buffer, expect: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if buffer.cell((x, y)).is_some_and(|cell| cell.symbol() == "╭") {
                let mut row = String::new();
                for at in x..buffer.area.width {
                    let Some(cell) = buffer.cell((at, y)) else {
                        break;
                    };
                    row.push_str(cell.symbol());
                    if cell.symbol() == "╮" {
                        break;
                    }
                }
                if row.contains(expect) {
                    return (x, y);
                }
            }
        }
    }
    panic!("no block titled {expect:?} was drawn");
}

fn block_width(buffer: &Buffer, expect: &str) -> u16 {
    let (x, y) = top_border(buffer, expect);
    (x + 1..buffer.area.width)
        .find(|at| {
            buffer
                .cell((*at, y))
                .is_some_and(|cell| cell.symbol() == "╮")
        })
        .expect("a bordered block has a right corner")
        - x
        + 1
}

/// The whole block titled `title`, borders included.
fn block_area(buffer: &Buffer, title: &str) -> Rect {
    let (x, y) = top_border(buffer, title);
    let bottom = (y + 1..buffer.area.height)
        .find(|at| {
            buffer
                .cell((x, *at))
                .is_some_and(|cell| cell.symbol() == "╰")
        })
        .expect("a bordered block has a bottom edge");

    Rect::new(x, y, block_width(buffer, title), bottom - y + 1)
}

/// The `line`th row of the block titled `title`, inside its borders.
///
/// Distinct from [`row_text`], which reads an absolute buffer row at the
/// Ddl panel's fixed coordinates.
fn block_row(buffer: &Buffer, title: &str, line: u16) -> String {
    let area = block_area(buffer, title);

    (area.x + 1..area.right() - 1)
        .filter_map(|x| buffer.cell((x, area.y + 1 + line)))
        .map(Cell::symbol)
        .collect()
}

// ------------------------------------------------------------------
// The footer (docs/superpowers/specs/2026-09-17-grid-paging-design.md §5)
// ------------------------------------------------------------------

fn footer(rows: &str, size: Option<(&str, Token)>, status: Option<&str>) -> Footer {
    let (status, hint) = status.map_or((None, None), |status| match status.split_once(" — ") {
        Some((status, hint)) => (Some(status.to_owned()), Some(hint.to_owned())),
        None => (Some(status.to_owned()), None),
    });

    Footer {
        rows: rows.to_owned(),
        size: size.map(|(text, token)| lc_domain::style::Segment::styled(text, token)),
        dirty: None,
        status,
        hint,
    }
}
