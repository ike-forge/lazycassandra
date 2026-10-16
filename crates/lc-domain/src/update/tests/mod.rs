use super::connections::open_connections;
use super::motion::{grid_columns, tree_rows};
use super::navigation::Landing;
use super::pointer::WHEEL_ROWS;
use super::*;
use crate::clock::Stamp;
use crate::commands::{CATALOGUE, CommandsPanel, Run};
use crate::commit::CommitPrompt;
use crate::config::{
    Config, ConnectionName, Diagnostic, Discovery, Icons, Origin, Span as ConfigSpan,
};
use crate::connection::{Connection, SchemaState};
use crate::contents::{ContentsState, Filter, SelectionMode, Subject};
use crate::ddl::DdlTab;
use crate::drop::{DropPrompt, Target};
use crate::export::Confirm;
use crate::feedback::FLASH_TICKS;
use crate::grid;
use crate::inspect::{Inspected, Inspector};
use crate::jobs::ExportJob;
use crate::line::Line;
use crate::model::{CqlValue, Key, Modifiers, Row, StatementKind};
use crate::panel::Panel;
use crate::pointer::Mark;
use crate::pointer::{Click, Pointer, Wheel};
use crate::queries::QueriesTab;
use crate::search::Sift;
use crate::search::{Prompt, Search, Searched};
use crate::selection::{Range, Ranges, Selection};
use crate::theme::{BuiltIn, Ours, Ported};
use crate::tree::tests_support::schema_of as tabled_schema;
use crate::view::OverlayView;
use crate::view::Stroke;
use crate::view::tests_support::tabled;
use crate::viewport::{Cursor, Span};

mod columns;
mod commit;
mod connections;
mod drop;
mod edit;
mod editor;
mod export;
mod filter;
mod health;
mod hints;
mod history;
mod insert;
mod inspect;
mod lifecycle;
mod logs;
mod motion;
mod navigation;
mod paging;
mod palette;
mod pointer;
mod queries;
mod reads;
mod run;
mod search;
mod selection;
mod themes;
mod tree;
mod yank;

fn toasted(state: &State) -> Option<(Severity, &str)> {
    state
        .toast
        .as_ref()
        .map(|toast| (toast.severity(), toast.message()))
}

fn schema_of(names: &[&str]) -> Schema {
    tabled_schema(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
}

fn connected(posture: Posture) -> State {
    let mut state = State::default();
    state.connection = ConnectionState::Connected(Connection {
        id: ConnId::next(),
        name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
        posture,
        schema: SchemaState::Loading,
    });
    state
}

fn current_conn(state: &State) -> ConnId {
    let ConnectionState::Connected(connection) = &state.connection else {
        panic!("the helper builds a connected state");
    };
    connection.id
}

fn statement(kind: StatementKind) -> Statement {
    Statement {
        cql: "SELECT * FROM ks.t".to_owned(),
        kind,
    }
}

fn diagnostic(message: &str) -> Diagnostic {
    Diagnostic {
        origin: Origin::new("./.lazycassandra.toml"),
        span: None,
        severity: Severity::Danger,
        message: message.to_owned(),
    }
}

fn diagnostics_cursor(state: &State) -> usize {
    let Some(Overlay::Diagnostics(panel)) = &state.overlay else {
        panic!("the diagnostics overlay is open");
    };
    panel.selected()
}

fn browsing(keyspaces: &[&str]) -> State {
    let mut state = connected(Posture::ReadOnly);
    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(schema_of(keyspaces)),
            took: Duration::ZERO,
        },
    );
    state
}

fn press(state: &mut State, key: Key) -> Vec<Effect> {
    update(state, Msg::Input(KeyPress::new(key)))
}

fn press_binding(state: &mut State, binding: KeyPress) -> Vec<Effect> {
    update(state, Msg::Input(binding))
}

fn press_stroke(state: &mut State, stroke: Stroke) -> Vec<Effect> {
    press_binding(state, stroke.press())
}

/// `browsing` leaves `command_rows` and `theme_rows` at zero, where no palette
/// or picker opens.
fn measured(keyspaces: &[&str]) -> State {
    let mut state = browsing(keyspaces);
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14).with_theme_rows(20);
    state
}

fn palette(state: &State) -> Option<&CommandsPanel> {
    match state.overlay.as_ref()? {
        Overlay::Commands(panel) => Some(panel),
        Overlay::Message(_)
        | Overlay::Search(_)
        | Overlay::Themes { .. }
        | Overlay::Connections { .. }
        | Overlay::History { .. }
        | Overlay::Health(_)
        | Overlay::Diagnostics(_)
        | Overlay::Export(_)
        | Overlay::Filter(_)
        | Overlay::Drop(_)
        | Overlay::Inspect(_)
        | Overlay::Commit(_)
        | Overlay::Discard(_)
        | Overlay::Fatal(_) => None,
    }
}

fn on_the_trail() -> State {
    let mut state = State::default();
    state.focus_on(Panel::Queries);
    state.queries.show(QueriesTab::Logs);
    state
}

fn trail_of(statements: &[&str]) -> State {
    let mut state = on_the_trail();
    for statement in statements {
        state.log.push(Entry::new(
            Stamp::default(),
            LogSubject::Sentence((*statement).to_owned()),
            Outcome::Cancelled,
        ));
    }
    state
}

fn search_for(state: &mut State, term: &str) {
    press(state, Key::Char('/'));
    type_into(state, term);
}

fn ctrl(state: &mut State, key: Key) -> Vec<Effect> {
    update(state, Msg::Input(KeyPress::ctrl(key)))
}

/// `\n` means Enter.
fn editing(text: &str) -> State {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    state.focus_on(Panel::Queries);
    press(&mut state, Key::Enter);
    type_into(&mut state, text);
    state
}

/// `\n` means Enter.
fn type_into(state: &mut State, text: &str) {
    for character in text.chars() {
        if character == '\n' {
            press(state, Key::Enter);
        } else {
            press(state, Key::Char(character));
        }
    }
}

/// The Columns tab exists only over a table.
fn browsing_a_table() -> State {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('l'));
    state
}

fn browsing_tables(keyspaces: &[(&str, &[&str])]) -> State {
    let mut state = connected(Posture::ReadOnly);
    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(tabled_schema(keyspaces)),
            took: Duration::ZERO,
        },
    );
    state
}

fn row_count(state: &State) -> usize {
    state
        .schema()
        .map_or(0, |schema| state.tables.rows(schema).len())
}

fn grid_of_rows(columns: usize, rows: usize, width: usize) -> State {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: (0..columns).map(|i| format!("c{i}")).collect(),
            rows: (0..rows)
                .map(|_| Row {
                    values: (0..columns).map(|_| CqlValue::Int(1)).collect(),
                })
                .collect(),
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state.focus_on(Panel::Contents);
    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(width)),
    );
    state
}

fn tables_state() -> State {
    let mut state = browsing_tables(&[("shop", &["orders", "users", "products"])]);
    press(&mut state, Key::Char('l'));
    state
}

fn contents_state() -> State {
    grid_of_rows(2, 3, 80)
}

fn read_shop_orders() -> State {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('l'));
    state
}

fn page_of(rows: usize) -> Page {
    Page {
        columns: vec!["id".to_owned()],
        rows: vec![
            crate::model::Row {
                values: vec![crate::model::CqlValue::Null],
            };
            rows
        ],
        next: None,
    }
}

/// `grid.page_rows`' default.
const PAGE: usize = 200;

fn more_of(rows: usize) -> Page {
    Page {
        next: Some(PagingState(vec![1])),
        ..page_of(rows)
    }
}

/// Cells read `r{r}c{c}`, so a yank says where it came from.
fn grid_of_named_cells(columns: usize, rows: usize) -> State {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: (0..columns).map(|c| format!("c{c}")).collect(),
            rows: (0..rows)
                .map(|r| Row {
                    values: (0..columns)
                        .map(|c| CqlValue::Text(format!("r{r}c{c}")))
                        .collect(),
                })
                .collect(),
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state.focus_on(Panel::Contents);
    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
    );
    state
}

fn yanked(effects: &[Effect]) -> Option<&str> {
    effects.iter().find_map(|effect| match effect {
        Effect::Yank(line) => Some(line.as_str()),
        _ => None,
    })
}

fn sized(state: &mut State, heights: [usize; 4], ddl_lines: usize) {
    update(state, Msg::Measured(Geometry::new(heights, ddl_lines, 0)));
}

/// Every cursor and offset sits off its ends, so motion hints have somewhere to
/// go both ways.
fn populated(focus: Panel) -> State {
    let mut state = browsing_tables(&[("alpha", &["a1"]), ("shop", &["orders", "users"])]);
    state.move_tree_cursor(1);
    press(&mut state, Key::Char('l'));

    if focus != Panel::Tables {
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Enter);
        let query = state
            .contents
            .outstanding()
            .expect("pressing enter on a table issues a read");
        // Three columns, so `h`/`l` can move both ways.
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(Page {
                    columns: vec!["id".to_owned(), "name".to_owned(), "email".to_owned()],
                    rows: vec![
                        Row {
                            values: vec![CqlValue::Null, CqlValue::Null, CqlValue::Null],
                        };
                        20
                    ],
                    next: None,
                }),
                took: Duration::ZERO,
            },
        );
        state.contents.cursor.go_to(5, 20);
        state.contents.column.go_to(1, 3);
    }

    sized(&mut state, [3, 3, 3, 3], 40);
    // Room for the export form, or its hint is inert.
    state.geometry = state.geometry.with_export_fits(true);
    let ddl_len = crate::view::ddl_rows(&state);
    state.ddl.cursor.page_down(ddl_len, 3);
    state.ddl.scroll.down(40, 3);
    state.focus_on(focus);

    state
}

fn point(state: &mut State, pointer: Pointer) -> Vec<Effect> {
    update(state, Msg::Pointer(pointer))
}

fn click_row(state: &mut State, panel: Panel, row: usize) -> Vec<Effect> {
    point(
        state,
        Pointer::Click(Click::Row {
            panel,
            row,
            column: None,
        }),
    )
}

fn droppable_tree() -> State {
    let mut state = connected(Posture::Writable);
    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(tabled_schema(&[("shop", &["orders"])])),
            took: Duration::ZERO,
        },
    );
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14)
        .with_theme_rows(20)
        .with_drop_fits(true);
    press(&mut state, Key::Char('l'));
    state
}

fn inspectable(columns: usize, rows: usize) -> State {
    let mut state = grid_of_named_cells(columns, rows);
    update(
        &mut state,
        Msg::Measured(
            Geometry::new([8, 8, 8, 1], 0, 0)
                .with_grid_width(80)
                .with_inspector(10, 40, Span::default()),
        ),
    );
    state
}

fn opened(mode: SelectionMode) -> State {
    let mut state = inspectable(3, 4);
    state.contents.mode = mode;
    state.contents.cursor.go_to(2, 4);
    state.contents.column.go_to(1, 3);
    press(&mut state, Key::Enter);
    state
}

fn dropping(state: &State) -> Option<&DropPrompt> {
    match &state.overlay {
        Some(Overlay::Drop(prompt)) => Some(prompt),
        _ => None,
    }
}

fn type_in(state: &mut State, text: &str) {
    for character in text.chars() {
        press(state, Key::Char(character));
    }
}

/// Fixture primary key: `(id uuid, created_at timestamp)`.
fn deletable(rows: usize) -> State {
    grid_over(
        rows,
        &["id", "created_at", "email"],
        |index| {
            vec![
                CqlValue::Uuid(format!("8f14e45f-000{index}")),
                CqlValue::Timestamp(1_699_999_999_000 + i64::try_from(index).unwrap_or_default()),
                CqlValue::Text("ada@example.com".to_owned()),
            ]
        },
        Posture::Writable,
    )
}

fn grid_over(
    rows: usize,
    columns: &[&str],
    values: impl Fn(usize) -> Vec<CqlValue>,
    posture: Posture,
) -> State {
    let mut state = connected(posture);
    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(tabled_schema(&[("shop", &["orders"])])),
            took: Duration::ZERO,
        },
    );
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14).with_drop_fits(true);

    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: columns.iter().map(|name| (*name).to_owned()).collect(),
            rows: (0..rows)
                .map(|index| Row {
                    values: values(index),
                })
                .collect(),
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state.focus_on(Panel::Contents);
    state
}

fn deleted_cql(effects: &[Effect]) -> Option<String> {
    effects.iter().find_map(|effect| match effect {
        Effect::Execute { stmt, .. } | Effect::ExecutePaged { stmt, .. } => Some(stmt.cql.clone()),
        _ => None,
    })
}

fn searching(state: &State) -> Option<&Prompt> {
    match &state.overlay {
        Some(Overlay::Search(prompt)) => Some(prompt),
        _ => None,
    }
}

fn contents_showing(rows: usize) -> State {
    let mut state = connected(Posture::ReadOnly);
    state.geometry = Geometry::default().with_export_fits(true);
    state.focus_on(Panel::Contents);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        page_of(rows),
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state
}

/// Pages left, so the fetched-rows scope differs from the whole one.
fn contents_showing_more(rows: usize) -> State {
    let mut state = connected(Posture::ReadOnly);
    state.geometry = Geometry::default().with_export_fits(true);
    state.focus_on(Panel::Contents);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        Statement::select_all("shop", "orders"),
    );
    state.contents.page_ready(
        query,
        more_of(rows),
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state
}

fn open_form(state: &mut State) {
    handle_key(state, KeyPress::ctrl(Key::Char('e')));
}

/// The shared fixture is all primary key, which `e` refuses.
fn schema_with_email() -> Schema {
    let mut schema = tabled_schema(&[("shop", &["orders"])]);
    if let Some(table) = schema
        .keyspaces
        .get_mut("shop")
        .and_then(|keyspace| keyspace.tables.get_mut("orders"))
    {
        table.regular_columns.push(crate::model::Column {
            name: "email".to_owned(),
            cql_type: "text".to_owned(),
        });
    }
    schema
}

/// The cursor lands on `id`; walk to `email` before `e`.
fn editable_grid(rows: usize) -> State {
    editable_grid_at(rows, Posture::Writable)
}

fn editable_grid_at(rows: usize, posture: Posture) -> State {
    let mut state = connected(posture);
    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(schema_with_email()),
            took: Duration::ZERO,
        },
    );

    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: vec!["id".to_owned(), "created_at".to_owned(), "email".to_owned()],
            rows: (0..rows)
                .map(|index| Row {
                    values: vec![
                        CqlValue::Uuid(format!("8f14e45f-000{index}")),
                        CqlValue::Timestamp(
                            1_699_999_999_000 + i64::try_from(index).unwrap_or_default(),
                        ),
                        CqlValue::Text("ada@example.com".to_owned()),
                    ],
                })
                .collect(),
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state.focus_on(Panel::Contents);
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14)
        .with_inspector(10, 40, Span::default())
        .with_commit(10, 4);
    state
}

fn inspecting_row(rows: usize) -> State {
    let mut state = editable_grid(rows);
    press(&mut state, Key::Enter);
    assert!(
        matches!(state.overlay, Some(Overlay::Inspect(_))),
        "the fixture opens the inspector"
    );
    state
}

fn on_email(state: &mut State) {
    for _ in 0..2 {
        press(state, Key::Char('j'));
    }
}

fn refusal(state: &State) -> Option<String> {
    match &state.overlay {
        Some(Overlay::Message(message)) => Some(message.message.clone()),
        _ => None,
    }
}

fn dirty_cells(state: &State) -> usize {
    state
        .contents
        .loaded()
        .map_or(0, |page| page.edits().cells())
}

fn committable(rows: usize) -> State {
    let mut state = inspecting_row(rows);
    for row in 0..rows {
        if row > 0 {
            press(&mut state, Key::Escape);
            for _ in 0..row {
                press(&mut state, Key::Char('j'));
            }
            press(&mut state, Key::Enter);
        }
        on_email(&mut state);
        press(&mut state, Key::Char('e'));
        type_in(&mut state, "!");
        press(&mut state, Key::Enter);
    }
    press(&mut state, Key::Escape);
    state
}
