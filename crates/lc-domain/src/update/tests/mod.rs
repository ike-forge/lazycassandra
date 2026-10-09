use super::pointer::WHEEL_ROWS;
use super::*;
use crate::clock::Stamp;
use crate::commands::{CATALOGUE, CommandsPanel, Run};
use crate::commit::CommitPrompt;
use crate::config::{
    Config, ConnectionName, Diagnostic, Discovery, Icons, Origin, Span as ConfigSpan,
};
use crate::contents::{ContentsState, SelectionMode};
use crate::drop::{DropPrompt, Target};
use crate::export::Confirm;
use crate::inspect::{Inspected, Inspector};
use crate::model::{CqlValue, Modifiers, Row, StatementKind};
use crate::pointer::Mark;
use crate::pointer::{Click, Pointer, Wheel};
use crate::search::Sift;
use crate::search::{Prompt, Search, Searched};
use crate::state::ExportJob;
use crate::state::{Connection, FLASH_TICKS, Range, Span};
use crate::theme::{Ours, Ported};
use crate::tree::tests_support::schema_of as tabled_schema;
use crate::view::OverlayView;
use crate::view::Stroke;
use crate::view::tests_support::tabled;

mod columns;
mod commit;
mod connections;
mod drop;
mod edit;
mod editor;
mod export;
mod filter;
mod hints;
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

/// A frame big enough for a palette and a picker. `browsing` leaves
/// geometry at its default, whose `command_rows` and `theme_rows` are
/// zero — which is itself a binding
/// (`the_palette_does_not_open_where_it_cannot_be_drawn`).
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

/// Puts the panel straight on the Logs tab without walking a key
/// sequence to it: what these tests are covering is the Logs tab itself,
/// not the cycle that reaches it (that is `Tab`'s job, covered
/// separately).
fn on_the_trail() -> State {
    let mut state = State::default();
    state.focus_on(Panel::Queries);
    state.queries.show(QueriesTab::Logs);
    state
}

/// A trail with one `Outcome::Cancelled` entry per statement, focused on
/// the Logs tab so `/`, `n`, `v` and `y` reach it directly.
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

/// Opens a search over the focused panel and types `term` into it, the
/// same two steps `n_walks_to_the_next_match_and_wraps` and its
/// neighbours drive press by press.
fn search_for(state: &mut State, term: &str) {
    press(state, Key::Char('/'));
    type_into(state, term);
}

fn ctrl(state: &mut State, key: Key) -> Vec<Effect> {
    update(state, Msg::Input(KeyPress::ctrl(key)))
}

/// A state focused on Queries with its buffer opened and `text` typed
/// into it, `\n` meaning Enter — the same convention `editor.rs`'s own
/// helper uses.
fn editing(text: &str) -> State {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    state.focus_on(Panel::Queries);
    press(&mut state, Key::Enter);
    type_into(&mut state, text);
    state
}

/// Types `text` into the focused editor, `\n` meaning Enter — the same
/// convention `editor.rs`'s own helper uses.
fn type_into(state: &mut State, text: &str) {
    for character in text.chars() {
        if character == '\n' {
            press(state, Key::Enter);
        } else {
            press(state, Key::Char(character));
        }
    }
}

/// `browsing_tables` with the cursor stepped onto a table, which is what
/// the Columns tab needs in order to exist.
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

/// `shop` unfolded, with `orders` at row 1 — a table, not a keyspace.
fn tables_state() -> State {
    let mut state = browsing_tables(&[("shop", &["orders", "users", "products"])]);
    press(&mut state, Key::Char('l'));
    state
}

/// Contents focused and showing a loaded page.
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

/// `grid.page_rows`' default, the size of a page that has a successor.
const PAGE: usize = 200;

/// [`page_of`], with a page after it.
fn more_of(rows: usize) -> Page {
    Page {
        next: Some(PagingState(vec![1])),
        ..page_of(rows)
    }
}

/// A page whose cell at row `r`, column `c` reads `r{r}c{c}`, so that a
/// yank names which cell, row or column it came from rather than only how
/// many commas it holds.
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

/// A state where every motion has somewhere to go in both directions:
/// two keyspaces with the second unfolded, a page of rows under the
/// Contents cursor, and a DDL document longer than its panel. A cursor
/// resting on 0 would make `k`/`h` and `home` inert and fail a correct
/// hint. The Ddl viewport is likewise nudged off zero, or `ctrl-y` would
/// find nothing to scroll up over.
fn populated(focus: Panel) -> State {
    let mut state = browsing_tables(&[("alpha", &["a1"]), ("shop", &["orders", "users"])]);
    state.tables.cursor.go_to(1, 2);
    press(&mut state, Key::Char('l'));

    // Tables keeps its cursor on the keyspace, where `l` steps in and `h`
    // folds. The other panels need a table selected: Ddl draws its
    // details from one, and Contents shows what reading it returned.
    if focus != Panel::Tables {
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Enter);
        let query = state
            .contents
            .outstanding()
            .expect("pressing enter on a table issues a read");
        // Three columns, not `page_of`'s one: `h`/`l` need a middle
        // column to move away from in both directions for their hint to
        // be exercised.
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
    // Room for the export form, or Contents' `ctrl-e export` hint is drawn
    // inert and promises nothing to test.
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

/// A writable connection with a table under the tree, on a frame with
/// room for the confirmation: the state `d` answers in (drop design §1).
/// The cursor starts on the keyspace row, so `j` reaches the table.
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
    // Unfolded, so the row below the keyspace is its table.
    press(&mut state, Key::Char('l'));
    state
}

// ------------------------------------------------------------------
// Inspecting (docs/superpowers/specs/2026-09-17-cell-inspector-design.md)
// ------------------------------------------------------------------

/// `grid_of_named_cells`, measured into a frame whose inspector shows ten
/// rows of a value forty lines long.
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

// ------------------------------------------------------------------
// Dropping (docs/superpowers/specs/2026-08-31-drop-design.md)
// ------------------------------------------------------------------

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

// ------------------------------------------------------------------
// Deleting rows (drop design §3.1, §4)
// ------------------------------------------------------------------

/// A grid over `shop.orders`, whose fixture primary key is
/// `(id uuid, created_at timestamp)`, holding `rows` addressable rows.
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

/// [`contents_showing`] over a result with pages left, so the form's
/// fetched-rows scope differs from the whole one.
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

// ------------------------------------------------------------------
// Editing cells (docs/superpowers/specs/2026-09-18-cell-editing-design.md)
// ------------------------------------------------------------------

/// The fixture table with `email text` added as a regular column: the
/// shared one is all primary key, and §2 refuses every column of it.
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

/// A grid over `shop.orders` — key `(id uuid, created_at timestamp)`, one
/// editable column — with room for the inspector and the confirmation.
/// The cursor lands on `id`, so a test walks to `email` before `e` opens
/// anything.
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

/// Walks the inspector's cursor onto `email`, the one editable column of
/// the fixture: `id` and `created_at` are its primary key.
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
