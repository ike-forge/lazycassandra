//! The Contents panel's projection: a page of rows as a drawable grid.
//!
//! Unlike the other two panels this one does not follow the tree cursor, so
//! its title is what says which table is on screen (vision.md §4, principle
//! 8).

use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use crate::bytes::Bytes;
use crate::cell;
use crate::contents::{
    ContentsState, Fetch, GridPage, MEMORY_STEP, Order, Paging, SelectionMode, Stopped, Subject,
    TableRef,
};
use crate::inspect::{Inspected, Inspector};
use crate::model::{CqlValue, Schema};
use crate::search::Search;
use crate::state::{Panel, Ranges, State};
use crate::style::{Segment, Severity, Token};

use super::{InspectBody, InspectField, InspectValue, InspectView, PanelBody, waiting};

/// One column heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridColumn {
    pub name: String,
    /// The column's place in the physical layout —
    /// [`Token::PartitionKey`], [`Token::ClusteringKey`] or
    /// [`Token::RegularColumn`]. `None` when the schema no longer has the
    /// table, so no claim about its shape can be checked.
    pub token: Option<Token>,
    /// Character columns it is drawn in: capped by `grid.max_column_width`,
    /// then widened into room the panel has spare. Computed here rather than in the
    /// renderer so the fitting arithmetic can live beside the offsets it
    /// bounds (docs/superpowers/specs/2026-08-10-table-display-design.md §6).
    pub width: u16,
    /// Which way this column is sorted, `None` when the grid is not sorted on
    /// it. What the mark over the heading says, and the marks are drawn on
    /// every column whether or not one is in force — an affordance a user can
    /// see and click before ever pressing `s`
    /// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §5).
    pub sort: Option<Order>,
    /// Whether a filter is narrowing the grid by this column.
    pub filtered: bool,
}

/// One value, already rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCell {
    pub text: String,
    /// `Some(Token::Faint)` for a null, which is what keeps it apart from a
    /// text column whose value is the word "null".
    pub token: Option<Token>,
    /// Whether the applied search matched `text`.
    ///
    /// A field of its own rather than a second meaning for `token`:
    /// overwriting the null marker on a match would lose that distinction
    /// for exactly the cells the user went looking for (design §4.3).
    pub matched: bool,
}

/// How much of a result Table contents holds, drawn on its bottom border
/// (docs/superpowers/specs/2026-09-17-grid-paging-design.md §5).
///
/// Three parts because the renderer drops them apart: `size` first, then the
/// whole footer, as the border narrows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footer {
    /// Every row fetched, filtered or not: the title already says how much of
    /// that a filter left.
    pub rows: String,
    /// The estimate, from [`SIZE_SHOWN_FROM`] up, in the warning colour from
    /// the first memory stop up.
    pub size: Option<Segment>,
    /// `None` once the result ended.
    pub status: Option<String>,
    /// The key that continues a stopped `P`. Dropped first on a narrow border:
    /// the hint bar names it too.
    pub hint: Option<String>,
}

/// The estimate below which the footer leaves it out.
pub const SIZE_SHOWN_FROM: Bytes = Bytes::MB.times(10);

pub(super) fn footer(state: &State) -> Option<Footer> {
    let ContentsState::Loaded { page, paging, .. } = state.contents.state() else {
        return None;
    };
    let bytes = page.bytes();

    Some(Footer {
        rows: match page.fetched() {
            1 => "1 row".to_owned(),
            count => format!("{} rows", thousands(count)),
        },
        size: (bytes >= SIZE_SHOWN_FROM).then(|| {
            Segment::styled(
                format!("≈ {bytes}"),
                if bytes >= MEMORY_STEP {
                    Token::Severity(Severity::Warning)
                } else {
                    Token::Muted
                },
            )
        }),
        status: match paging {
            Paging::Ended => None,
            Paging::More { fetch, .. } => Some(match fetch {
                Fetch::Idle => "more".to_owned(),
                Fetch::One(_) | Fetch::All { .. } => "loading".to_owned(),
                // A stop is a whole multiple of `MEMORY_STEP`, and the page that
                // crossed it landed whole, so the estimate sits past it.
                Fetch::Stopped(Stopped::Memory(at)) => {
                    format!("paused past {} MB", at.get() / Bytes::MB.get())
                }
                Fetch::Stopped(Stopped::Rows(at)) => {
                    format!("paused past {} rows", thousands(*at))
                }
            }),
        },
        hint: matches!(
            paging,
            Paging::More {
                fetch: Fetch::Stopped(_),
                ..
            }
        )
        .then(|| "P for more".to_owned()),
    })
}

/// `1234567` as `1,234,567`.
fn thousands(count: usize) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);

    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }

    out
}

pub(super) fn title(state: &State) -> Vec<Segment> {
    const BASE: &str = "Table contents";

    match state.contents.state() {
        ContentsState::Empty => vec![Segment::plain(BASE)],
        ContentsState::Loading { subject, .. } | ContentsState::Failed { subject, .. } => {
            named(BASE, subject, None)
        }
        ContentsState::Loaded { subject, page, .. } => named(BASE, subject, Some(count(page))),
    }
}

/// The panel's name, then what it is showing, then how much of it.
///
/// The qualified name keeps the schema's own colours ([`TableRef::segments`])
/// and a typed statement the editor's ([`crate::cql::issued`]): this panel
/// does not follow the tree cursor, so its title is the only thing on screen
/// saying what is up (vision.md §4, principle 8), and it says it the way the
/// panel it came from does.
fn named(base: &str, subject: &Subject, count: Option<String>) -> Vec<Segment> {
    let mut segments = vec![Segment::plain(format!("{base} — "))];

    match subject {
        Subject::Table(table) => segments.extend(table.segments()),
        Subject::Typed(cql) => segments.extend(crate::cql::issued(cql)),
    }

    if let Some(count) = count {
        segments.push(Segment::plain(format!(" ({count})")));
    }

    segments
}

/// How much of the page is on screen. Under a filter that is two numbers,
/// not one: a grid drawn as "3 rows" with seventeen more behind a filter says
/// the cluster returned three
/// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §3).
fn count(page: &GridPage) -> String {
    let shown = page.len();

    if page.is_filtered() {
        format!("{shown} of {}", rows(page.page().rows.len()))
    } else {
        rows(shown)
    }
}

fn rows(count: usize) -> String {
    if count == 1 {
        "1 row".to_owned()
    } else {
        format!("{count} rows")
    }
}

pub(super) fn body(state: &State) -> PanelBody {
    match state.contents.state() {
        // The first thing every user sees, so it names the action rather than
        // the absence of one (vision.md §4, principle 1).
        ContentsState::Empty => waiting("Press Enter on a table"),
        ContentsState::Loading { .. } => waiting("Loading rows…"),
        // Quoted, not deferred to [4]-Logs: the trail keeps the history,
        // the panel owns the reason (supersedes query log design §8).
        ContentsState::Failed { error, .. } => PanelBody::Notice {
            text: error.clone(),
            token: Some(Token::Severity(Severity::Danger)),
        },
        ContentsState::Loaded { subject, page, .. } => grid(state, subject, page),
    }
}

fn grid(state: &State, subject: &Subject, grid_page: &GridPage) -> PanelBody {
    let page = grid_page.page();

    if page.rows.is_empty() {
        return waiting("No rows");
    }

    // A filter that matched nothing is not an empty page: the panel says so
    // in the words of the thing that emptied it, so the way back is the key
    // that put it there (design §3).
    if grid_page.is_empty() {
        return waiting("No rows match the filter");
    }

    let flash = super::flash_over(state, Panel::Contents);
    let (rows_ranges, columns_ranges) = match flash {
        Some(flash) => (flash.rows().clone(), flash.columns().clone()),
        None => match state.contents.mode {
            SelectionMode::Row => (
                state.contents.rows.ranges(state.contents.cursor.index()),
                Ranges::default(),
            ),
            SelectionMode::Column => (
                Ranges::default(),
                state.contents.columns.ranges(state.contents.column.index()),
            ),
            SelectionMode::Cell => (
                state.contents.rows.ranges(state.contents.cursor.index()),
                state.contents.columns.ranges(state.contents.column.index()),
            ),
        },
    };

    let offset = state.contents.scroll.offset();
    let table = grid_table(state, subject);
    let tokens: Vec<Option<Token>> = page
        .columns
        .iter()
        .map(|name| column_token(state.schema(), table.as_ref(), name))
        .collect();

    let (widths, _) = grid_page.drawn(state.geometry.grid_width());

    PanelBody::Grid {
        columns: page
            .columns
            .iter()
            .zip(widths.iter())
            .zip(&tokens)
            .enumerate()
            .map(|(index, ((name, width), token))| GridColumn {
                name: name.clone(),
                token: *token,
                width: *width,
                sort: grid_page
                    .sort()
                    .filter(|sort| sort.column == index)
                    .map(|sort| sort.order),
                filtered: grid_page.filter(index).is_some(),
            })
            .collect(),
        // Unmeasured, the panel has no height to window by, and draws from
        // the offset on.
        rows: grid_page
            .rows()
            .skip(offset)
            .take(match state.geometry.height(Panel::Contents) {
                0 => usize::MAX,
                height => height,
            })
            .map(|row| {
                row.values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        grid_cell(
                            value,
                            tokens.get(index).copied().flatten(),
                            state.contents.search(),
                        )
                    })
                    .collect()
            })
            .collect(),
        total: grid_page.len(),
        selected: state.contents.cursor.index(),
        offset,
        first_column: state.contents.column_scroll.first(),
        column: state.contents.column.index(),
        mode: state.contents.mode,
        gutter: crate::grid::gutter(grid_page.len()),
        rows_ranges,
        columns_ranges,
        flash: flash.is_some(),
    }
}

fn grid_cell(value: &CqlValue, column: Option<Token>, search: Option<&Search>) -> GridCell {
    let text = cell::text(value);

    GridCell {
        matched: search.is_some_and(|search| search.matches(&text)),
        token: match value {
            CqlValue::Null => Some(Token::Faint),
            _ => key_token(column),
        },
        text,
    }
}

/// A key column's values carry the colour its heading does, so the primary key
/// can be read off the rows themselves rather than traced back up the grid
/// (vision.md §2). A regular column keeps `None`: body text is what every
/// other value already is, and painting it would leave the keys with nothing
/// to stand out against.
const fn key_token(column: Option<Token>) -> Option<Token> {
    match column {
        Some(Token::PartitionKey) => Some(Token::PartitionKeyValue),
        Some(Token::ClusteringKey) => Some(Token::ClusteringKeyValue),
        _ => None,
    }
}

/// Where a column sits in the table's physical layout, which vision.md §2
/// exists to keep visible — here, in the grid being read, rather than a panel
/// away.
///
/// A typed statement gets the same treatment as one from the tree when the
/// resolver can name exactly one table for it and the schema has that table.
/// A column the table does not have — an alias, an aggregate, `writetime(x)`
/// — stays plain, because colouring it would claim it was a column of that
/// table (cql semantics design §7).
fn column_token(schema: Option<&Schema>, table: Option<&TableRef>, column: &str) -> Option<Token> {
    let table = table?;
    let found = schema
        .and_then(|schema| schema.keyspaces.get(&table.keyspace))
        .and_then(|keyspace| keyspace.tables.get(&table.table))?;

    let key = &found.primary_key;
    if key.partition.iter().any(|c| c.name == column) {
        Some(Token::PartitionKey)
    } else if key.clustering.iter().any(|c| c.column.name == column) {
        Some(Token::ClusteringKey)
    } else if found.static_columns.iter().any(|c| c.name == column)
        || found.regular_columns.iter().any(|c| c.name == column)
    {
        Some(Token::RegularColumn)
    } else {
        // Not a column of the table the statement read: an alias, an
        // aggregate, or a projection. `RegularColumn` here would be a claim
        // the schema does not support.
        None
    }
}

/// The inspector's projection. Tokens are the grid's own, so a key column
/// reads as one in both (cell inspector design §3).
pub(super) fn inspect(state: &State, inspector: &Inspector) -> InspectView {
    let table = grid_table(state, inspector.subject());
    let column = |name: &str| column_token(state.schema(), table.as_ref(), name);
    let value = |name: &str, value: &CqlValue| InspectValue {
        lines: crate::inspect::lines(value),
        token: match value {
            CqlValue::Null => Some(Token::Faint),
            _ => key_token(column(name)),
        },
    };

    let body = match inspector.inspected() {
        Inspected::Cell {
            column: name,
            value: inspected,
        } => InspectBody::Cell(value(name, inspected)),
        Inspected::Row {
            columns, values, ..
        } => {
            let cap = usize::from(state.config.grid.max_column_width.get());
            let names = columns
                .iter()
                .map(|name| name.width())
                .max()
                .unwrap_or(0)
                .min(cap);

            InspectBody::Row {
                names: u16::try_from(names).unwrap_or(u16::MAX),
                fields: columns
                    .iter()
                    .zip(values)
                    .map(|(name, inspected)| InspectField {
                        name: fitted(name, names),
                        token: column(name),
                        value: value(name, inspected),
                    })
                    .collect(),
            }
        }
    };

    InspectView {
        title: inspector.title(),
        size: inspector.size(),
        body,
        cursor: inspector.cursor(),
        offset: inspector.offset(),
    }
}

/// `name` cut to `width` cells with an ellipsis, the way the grid cuts a
/// heading.
fn fitted(name: &str, width: usize) -> String {
    if name.width() <= width {
        return name.to_owned();
    }

    let mut out = String::new();
    let mut used = 0;
    for character in name.chars() {
        used += character.width().unwrap_or(0);
        if used >= width {
            break;
        }
        out.push(character);
    }
    out.push('…');

    out
}

/// The table a grid's rows came from: the one the tree asked for, or the one
/// a typed statement names, qualified by the keyspace on screen where the
/// statement did not qualify it.
fn grid_table(state: &State, subject: &Subject) -> Option<TableRef> {
    match subject {
        Subject::Table(table) => Some(table.clone()),
        Subject::Typed(cql) => {
            let name = crate::cql::sole_table(cql)?;

            Some(TableRef {
                keyspace: name.keyspace.clone().or_else(|| state.visible_keyspace())?,
                table: name.table.clone(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{
        ClusteringColumn, ClusteringOrder, Column, ConnId, Page, QueryId, Row, Schema,
    };
    use crate::posture::Posture;
    use crate::search::{Search, Searched};
    use crate::state::{Connection, ConnectionState, SchemaState, State};

    use super::super::tests_support::{panel_body, tabled};
    use super::super::view_model;
    use super::*;

    fn contents_body(state: &State) -> PanelBody {
        panel_body(state, 2)
    }

    /// The strip alone: the `[3]-` every title opens with is
    /// `view::tests`' to pin, not this module's.
    fn contents_title(state: &State) -> String {
        view_model(state).panels[2].title.strip()
    }

    fn page_of(columns: &[&str], rows: &[&[CqlValue]]) -> Page {
        Page {
            columns: columns.iter().map(|name| (*name).to_owned()).collect(),
            rows: rows
                .iter()
                .map(|values| Row {
                    values: values.to_vec(),
                })
                .collect(),
            next: None,
        }
    }

    /// A state showing `page` for `shop.orders`, whose schema entry has
    /// `id` as its partition key.
    fn showing(page: Page) -> State {
        let mut state = tabled(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state.contents.requested(
            query,
            Subject::Table(TableRef::new("shop", "orders")),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );
        state.contents.page_ready(
            query,
            page,
            crate::config::MaxColumnWidth::default(),
            QueryId::next,
        );
        state
    }

    /// A state showing `page` for a hand-typed `cql`, against a schema whose
    /// sole keyspace `shop` has `orders` (`id` partition key, `total`
    /// regular column) and `customers` (`id` partition key). The tree
    /// cursor's default index selects that one keyspace, standing in for
    /// whichever one is on screen when the statement does not qualify its
    /// table.
    fn showing_typed(cql: &str, page: Page) -> State {
        let mut state = tabled(&[("shop", &["orders", "customers"])]);
        let mut schema = state.schema().cloned().expect("loaded");
        let orders = schema
            .keyspaces
            .get_mut("shop")
            .and_then(|keyspace| keyspace.tables.get_mut("orders"))
            .expect("the builder made it");
        orders.regular_columns = vec![Column {
            name: "total".to_owned(),
            cql_type: "int".to_owned(),
        }];
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
            name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema),
        });
        let query = QueryId::next();
        state.contents.requested(
            query,
            Subject::Typed(cql.to_owned()),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );
        state.contents.page_ready(
            query,
            page,
            crate::config::MaxColumnWidth::default(),
            QueryId::next,
        );
        state
    }

    /// [`showing_typed`], but against a schema whose sole keyspace is
    /// `keyspace` — the tree cursor's default index parks it on that
    /// keyspace, which is what an unqualified table name in `cql` resolves
    /// against.
    fn showing_typed_in(keyspace: &str, cql: &str, page: Page) -> State {
        let mut state = tabled(&[(keyspace, &["orders"])]);
        let query = QueryId::next();
        state.contents.requested(
            query,
            Subject::Typed(cql.to_owned()),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );
        state.contents.page_ready(
            query,
            page,
            crate::config::MaxColumnWidth::default(),
            QueryId::next,
        );
        state
    }

    #[test]
    fn an_untouched_contents_panel_says_how_to_fill_it() {
        assert_eq!(
            contents_body(&State::default()),
            PanelBody::Notice {
                text: "Press Enter on a table".to_owned(),
                token: Some(Token::Muted),
            }
        );
        assert_eq!(contents_title(&State::default()), "Table contents");
    }

    #[test]
    fn a_panel_waiting_on_rows_names_the_table_it_is_waiting_for() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.contents.requested(
            QueryId::next(),
            Subject::Table(TableRef::new("shop", "orders")),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );

        assert_eq!(
            contents_body(&state),
            PanelBody::Notice {
                text: "Loading rows…".to_owned(),
                token: Some(Token::Muted),
            }
        );
        assert_eq!(contents_title(&state), "Table contents — shop.orders");
    }

    fn title_segments(state: &State) -> Vec<Segment> {
        view_model(state).panels[2].title.tabs()[0]
            .segments()
            .to_vec()
    }

    fn showing_table(keyspace: &str, table: &str) -> State {
        let mut state = tabled(&[(keyspace, &[table])]);
        state.contents.requested(
            QueryId::next(),
            Subject::Table(TableRef::new(keyspace, table)),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );
        state
    }

    /// The panel that does not follow the tree cursor still names its table
    /// the way the tree does, colours included (vision.md §2).
    #[test]
    fn a_titles_table_name_wears_the_schemas_own_colours() {
        let state = showing_table("shop", "orders");

        assert_eq!(
            title_segments(&state),
            vec![
                Segment::plain("Table contents — "),
                Segment::styled("shop", Token::Keyspace),
                Segment::styled(".", Token::Faint),
                Segment::styled("orders", Token::Table),
            ]
        );
    }

    #[test]
    fn a_system_tables_title_is_faint_on_both_halves_as_its_tree_row_is() {
        let state = showing_table("system_schema", "columns");

        assert_eq!(
            title_segments(&state)[1..],
            [
                Segment::styled("system_schema", Token::Faint),
                Segment::styled(".", Token::Faint),
                Segment::styled("columns", Token::Faint),
            ]
        );
    }

    /// A typed statement is coloured as the editor and the trail colour it.
    #[test]
    fn a_typed_subjects_title_is_highlighted_as_cql() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.contents.requested(
            QueryId::next(),
            Subject::Typed("SELECT *\n  FROM shop.orders".to_owned()),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );

        let mut expected = vec![Segment::plain("Table contents — ")];
        expected.extend(crate::cql::issued("SELECT * FROM shop.orders"));
        assert_eq!(title_segments(&state), expected);
        assert!(
            expected
                .iter()
                .any(|segment| segment.token == Some(Token::Keyword))
        );
        assert_eq!(
            contents_title(&state),
            "Table contents — SELECT * FROM shop.orders"
        );
    }

    fn failed_state() -> State {
        let mut state = tabled(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state.contents.requested(
            query,
            Subject::Table(TableRef::new("shop", "orders")),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );
        state.contents.failed(
            query,
            &crate::model::DbError::Invalid("undefined column name total".to_owned()),
        );
        state
    }

    #[test]
    fn a_failed_read_quotes_the_error_rather_than_pointing_at_the_trail() {
        let PanelBody::Notice { text, token } = contents_body(&failed_state()) else {
            panic!("a notice");
        };
        assert_eq!(
            text,
            "cluster rejected the statement: undefined column name total"
        );
        assert_eq!(token, Some(Token::Severity(Severity::Danger)));
    }

    #[test]
    fn a_table_with_no_rows_says_so_rather_than_drawing_an_empty_grid() {
        let state = showing(page_of(&["id"], &[]));

        assert_eq!(
            contents_body(&state),
            PanelBody::Notice {
                text: "No rows".to_owned(),
                token: Some(Token::Muted),
            }
        );
    }

    #[test]
    fn a_page_becomes_a_header_and_one_row_per_row() {
        let state = showing(page_of(
            &["id", "email"],
            &[
                &[
                    CqlValue::Int(1),
                    CqlValue::Text("ada@example.com".to_owned()),
                ],
                &[CqlValue::Int(2), CqlValue::Null],
            ],
        ));

        let PanelBody::Grid {
            columns,
            rows,
            selected,
            ..
        } = contents_body(&state)
        else {
            panic!("a loaded page draws a grid");
        };
        assert_eq!(
            columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["id", "email"]
        );
        assert_eq!(
            rows.iter()
                .map(|row| row.iter().map(|c| c.text.as_str()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            [["1", "ada@example.com"], ["2", "null"]]
        );
        assert_eq!(selected, 0);
    }

    #[test]
    fn a_null_is_faint_so_it_cannot_be_read_as_the_word() {
        let state = showing(page_of(
            &["id", "email"],
            &[&[CqlValue::Int(1), CqlValue::Null]],
        ));

        let PanelBody::Grid { rows, .. } = contents_body(&state) else {
            panic!("a loaded page draws a grid");
        };
        assert_eq!(rows[0][0].token, Some(Token::PartitionKeyValue));
        assert_eq!(rows[0][1].token, Some(Token::Faint));
    }

    #[test]
    fn the_header_carries_the_tables_physical_shape() {
        let mut state = tabled(&[("shop", &["orders"])]);
        // The shared builder gives every table a single `id` partition key;
        // this one also clusters on `at`.
        let mut schema = state.schema().cloned().expect("loaded");
        let table = schema
            .keyspaces
            .get_mut("shop")
            .and_then(|keyspace| keyspace.tables.get_mut("orders"))
            .expect("the builder made it");
        table.primary_key.clustering = vec![ClusteringColumn {
            column: Column {
                name: "at".to_owned(),
                cql_type: "timestamp".to_owned(),
            },
            order: ClusteringOrder::Ascending,
        }];
        table.regular_columns = vec![Column {
            name: "total".to_owned(),
            cql_type: "int".to_owned(),
        }];
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
            name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema),
        });
        let query = QueryId::next();
        state.contents.requested(
            query,
            Subject::Table(TableRef::new("shop", "orders")),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );
        state.contents.page_ready(
            query,
            page_of(
                &["id", "at", "total"],
                &[&[CqlValue::Int(1), CqlValue::Int(2), CqlValue::Int(3)]],
            ),
            crate::config::MaxColumnWidth::default(),
            QueryId::next,
        );

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("a loaded page draws a grid");
        };
        assert_eq!(
            columns.iter().map(|c| c.token).collect::<Vec<_>>(),
            [
                Some(Token::PartitionKey),
                Some(Token::ClusteringKey),
                Some(Token::RegularColumn),
            ]
        );
    }

    /// The resolver names the one table a typed statement read, so its grid
    /// gets the same key colours a tree-driven one does (cql semantics
    /// design §7).
    #[test]
    fn a_typed_select_over_one_table_colours_its_key_columns() {
        let state = showing_typed(
            "select * from shop.orders",
            page_of(&["id", "total"], &[&[CqlValue::Int(1), CqlValue::Int(2)]]),
        );

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(columns[0].token, Some(Token::PartitionKey));
        assert_eq!(columns[1].token, Some(Token::RegularColumn));
    }

    /// An ordinary SQL habit — a trailing `;` — must not silently turn the
    /// colouring off: `sole_table` treats the placeholder scope the `;`
    /// opens as no statement of its own.
    #[test]
    fn a_trailing_semicolon_still_colours_the_grid() {
        let state = showing_typed(
            "select * from shop.orders;",
            page_of(&["id", "total"], &[&[CqlValue::Int(1), CqlValue::Int(2)]]),
        );

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(columns[0].token, Some(Token::PartitionKey));
        assert_eq!(columns[1].token, Some(Token::RegularColumn));
    }

    /// An alias, an aggregate, or `writetime(x)` matches no column of the
    /// table the statement read — colouring it would claim it was that
    /// column, which the resolver alone cannot know (design §7).
    #[test]
    fn an_aliased_column_claims_nothing_about_the_table() {
        let state = showing_typed(
            "select id as number from shop.orders",
            page_of(&["number"], &[&[CqlValue::Int(1)]]),
        );

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(
            columns[0].token, None,
            "`number` is not `id`, and colouring it would claim it was"
        );
    }

    /// A batch splits into one scope per `;`; its first scope alone names
    /// one table, but the buffer as a whole names two, so nothing is
    /// coloured (design §7; `sole_table`'s one-statement guard).
    #[test]
    fn a_statement_naming_two_tables_colours_nothing() {
        let state = showing_typed(
            "begin batch insert into shop.orders (id) values (1); \
             insert into shop.customers (id) values (2); apply batch",
            page_of(&["id"], &[&[CqlValue::Int(1)]]),
        );

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(columns[0].token, None);
    }

    /// An unqualified table name in a typed statement is read against the
    /// keyspace the tree cursor sits on, the way completion already does
    /// (`update.rs::complete`).
    #[test]
    fn an_unqualified_typed_select_takes_the_keyspace_on_screen() {
        let state = showing_typed_in(
            "shop",
            "select * from orders",
            page_of(&["id"], &[&[CqlValue::Int(1)]]),
        );

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(columns[0].token, Some(Token::PartitionKey));
    }

    /// A statement naming no table at all — as opposed to one the resolver
    /// could not pin down to exactly one — still claims nothing.
    #[test]
    fn a_typed_statement_naming_no_table_colours_nothing() {
        let state = showing_typed("select 1", page_of(&["1"], &[&[CqlValue::Int(1)]]));

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(columns[0].token, None);
    }

    #[test]
    fn a_page_whose_table_is_gone_claims_nothing_about_its_shape() {
        let mut state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
            name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(Schema::default()),
        });

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("the page is still showing");
        };
        assert_eq!(
            columns[0].token, None,
            "a key structure that cannot be checked must not be asserted"
        );
    }

    #[test]
    fn a_key_columns_values_carry_the_colour_its_heading_does() {
        let mut state = tabled(&[("shop", &["orders"])]);
        let mut schema = state.schema().cloned().expect("loaded");
        let table = schema
            .keyspaces
            .get_mut("shop")
            .and_then(|keyspace| keyspace.tables.get_mut("orders"))
            .expect("the builder made it");
        table.primary_key.clustering = vec![ClusteringColumn {
            column: Column {
                name: "at".to_owned(),
                cql_type: "timestamp".to_owned(),
            },
            order: ClusteringOrder::Ascending,
        }];
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
            name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema),
        });
        let query = QueryId::next();
        state.contents.requested(
            query,
            Subject::Table(TableRef::new("shop", "orders")),
            crate::model::Statement::typed("SELECT * FROM shop.orders"),
        );
        state.contents.page_ready(
            query,
            page_of(
                &["id", "at", "total"],
                &[&[CqlValue::Int(1), CqlValue::Int(2), CqlValue::Int(3)]],
            ),
            crate::config::MaxColumnWidth::default(),
            QueryId::next,
        );

        let PanelBody::Grid { rows, .. } = contents_body(&state) else {
            panic!("a loaded page draws a grid");
        };
        assert_eq!(
            rows[0].iter().map(|cell| cell.token).collect::<Vec<_>>(),
            [
                Some(Token::PartitionKeyValue),
                Some(Token::ClusteringKeyValue),
                None
            ],
            "a regular column's values stay body text, which is what the keys \
             stand out against"
        );
    }

    #[test]
    fn a_page_whose_table_is_gone_paints_no_key_colour_on_its_values() {
        let mut state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
            name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(Schema::default()),
        });

        let PanelBody::Grid { rows, .. } = contents_body(&state) else {
            panic!("the page is still showing");
        };
        assert_eq!(rows[0][0].token, None);
    }

    #[test]
    fn the_title_counts_the_rows_and_gets_the_singular_right() {
        let one = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));
        let two = showing(page_of(
            &["id"],
            &[&[CqlValue::Int(1)], &[CqlValue::Int(2)]],
        ));

        assert_eq!(contents_title(&one), "Table contents — shop.orders (1 row)");
        assert_eq!(
            contents_title(&two),
            "Table contents — shop.orders (2 rows)"
        );
    }

    #[test]
    fn a_search_flags_the_cells_that_match_and_keeps_every_row() {
        let mut state = showing(page_of(
            &["id", "email"],
            &[
                &[
                    CqlValue::Int(1),
                    CqlValue::Text("ali@example.com".to_owned()),
                ],
                &[
                    CqlValue::Int(2),
                    CqlValue::Text("bob@example.com".to_owned()),
                ],
            ],
        ));
        let before = match contents_body(&state) {
            PanelBody::Grid { rows, .. } => rows.len(),
            other => panic!("expected a grid, got {other:?}"),
        };

        state.apply_search(Searched::Contents, Search::new("ali"));

        let PanelBody::Grid { rows, .. } = contents_body(&state) else {
            panic!("a search must not turn the grid into a notice");
        };
        assert_eq!(
            rows.len(),
            before,
            "contents highlights; it does not filter"
        );
        let matched: Vec<Vec<bool>> = rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.matched).collect())
            .collect();
        assert_eq!(
            matched,
            [[false, true], [false, false]],
            "only the cell whose text holds \"ali\" is flagged — a \
             case-sensitive `contains` would flag the same cells here, but \
             nothing else in this fixture would catch it slipping in"
        );
    }

    #[test]
    fn a_matched_null_is_still_a_null() {
        let mut state = showing(page_of(
            &["id", "email"],
            &[&[CqlValue::Int(1), CqlValue::Null]],
        ));

        state.apply_search(Searched::Contents, Search::new("null"));

        let PanelBody::Grid { rows, .. } = contents_body(&state) else {
            panic!("expected a grid");
        };
        let null = rows
            .iter()
            .flatten()
            .find(|cell| cell.token == Some(Token::Faint))
            .expect("the fixture has a null");

        assert!(
            null.matched,
            "a null renders as the word null, so it answers this search"
        );
        assert_eq!(
            null.token,
            Some(Token::Faint),
            "and must stay distinguishable from a text column holding \"null\""
        );
    }

    #[test]
    fn no_search_flags_nothing() {
        let state = showing(page_of(
            &["id", "email"],
            &[&[
                CqlValue::Int(1),
                CqlValue::Text("ali@example.com".to_owned()),
            ]],
        ));

        let PanelBody::Grid { rows, .. } = contents_body(&state) else {
            panic!("expected a grid");
        };

        assert!(rows.iter().flatten().all(|cell| !cell.matched));
    }

    #[test]
    fn the_grid_selection_is_projected_from_the_panels_cursor() {
        let mut state = showing(page_of(
            &["id"],
            &[&[CqlValue::Int(1)], &[CqlValue::Int(2)]],
        ));
        state.contents.cursor.down(2);

        let PanelBody::Grid { selected, .. } = contents_body(&state) else {
            panic!("a loaded page draws a grid");
        };
        assert_eq!(selected, 1);
    }

    #[test]
    fn a_grid_carries_the_width_of_every_column() {
        let state = showing(page_of(
            &["id", "name"],
            &[&[CqlValue::Int(1), CqlValue::Text("alice".to_owned())]],
        ));

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(
            columns.iter().map(|c| c.width).collect::<Vec<_>>(),
            vec![2 + crate::grid::MARKS, 4 + crate::grid::MARKS],
            "each heading is measured with the marks it carries, which is \
             what widens `name` past the five columns \"alice\" needs"
        );
    }

    #[test]
    fn a_grid_names_the_selected_cell_and_the_first_column_drawn() {
        let mut state = showing(page_of(
            &["id", "name"],
            &[&[CqlValue::Int(1), CqlValue::Text("alice".to_owned())]],
        ));
        state.contents.column.go_to(1, 2);

        let PanelBody::Grid {
            column,
            first_column,
            selected,
            ..
        } = contents_body(&state)
        else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(column, 1);
        assert_eq!(first_column, 0);
        assert_eq!(selected, 0);
    }

    #[test]
    fn the_gutter_is_as_wide_as_the_last_row_number() {
        let rows: Vec<Vec<CqlValue>> = (0..12).map(|i| vec![CqlValue::Int(i)]).collect();
        let borrowed: Vec<&[CqlValue]> = rows.iter().map(Vec::as_slice).collect();
        let state = showing(page_of(&["id"], &borrowed));

        let PanelBody::Grid { gutter, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(gutter, 2, "twelve rows need two digits");
    }

    #[test]
    fn a_grid_names_the_range_on_each_axis() {
        let mut state = showing(page_of(
            &["id"],
            &[
                &[CqlValue::Int(1)],
                &[CqlValue::Int(2)],
                &[CqlValue::Int(3)],
            ],
        ));
        state.contents.rows.start(0);
        state.contents.rows.stop(2);

        let PanelBody::Grid {
            rows_ranges,
            columns_ranges,
            ..
        } = contents_body(&state)
        else {
            panic!("a loaded page draws a grid");
        };

        assert_eq!(rows_ranges, Ranges::of(crate::state::Range::new(0, 2)));
        assert_eq!(
            columns_ranges,
            Ranges::default(),
            "column mode never extended"
        );
    }

    #[test]
    fn row_mode_projects_no_columns_range_even_when_one_is_held() {
        let mut state = showing(page_of(
            &["id", "name"],
            &[&[CqlValue::Int(1), CqlValue::Int(2)]],
        ));
        state.contents.mode = SelectionMode::Row;
        state.contents.rows.start(0);
        state.contents.rows.stop(0);
        state.contents.columns.start(0);
        state.contents.columns.stop(1);

        let PanelBody::Grid { columns_ranges, .. } = contents_body(&state) else {
            panic!("a loaded page draws a grid");
        };

        assert_eq!(
            columns_ranges,
            Ranges::default(),
            "row mode has no column axis, so a held columns range must not be projected"
        );
    }

    #[test]
    fn column_mode_projects_no_rows_range_even_when_one_is_held() {
        let mut state = showing(page_of(
            &["id", "name"],
            &[
                &[CqlValue::Int(1), CqlValue::Int(2)],
                &[CqlValue::Int(3), CqlValue::Int(4)],
            ],
        ));
        state.contents.mode = SelectionMode::Column;
        state.contents.columns.start(0);
        state.contents.columns.stop(0);
        state.contents.rows.start(0);
        state.contents.rows.stop(1);

        let PanelBody::Grid { rows_ranges, .. } = contents_body(&state) else {
            panic!("a loaded page draws a grid");
        };

        assert_eq!(
            rows_ranges,
            Ranges::default(),
            "column mode has no row axis, so a held rows range must not be projected"
        );
    }

    #[test]
    fn a_row_projects_one_field_per_column_under_a_capped_name_column() {
        let long = "a_column_name_longer_than_the_thirty_cell_cap";
        let state = showing(page_of(
            &["id", long],
            &[&[CqlValue::Int(7), CqlValue::Null]],
        ));
        let inspector = Inspector::opened(
            Subject::Table(TableRef::new("shop", "orders")),
            Inspected::Row {
                number: 1,
                columns: vec!["id".to_owned(), long.to_owned()],
                values: vec![CqlValue::Int(7), CqlValue::Null],
            },
        );

        let view = inspect(&state, &inspector);

        assert_eq!(crate::style::text(&view.title), "shop.orders · row 1");
        assert_eq!(view.size, None);
        assert_eq!(
            view.body,
            InspectBody::Row {
                names: 30,
                fields: vec![
                    InspectField {
                        name: "id".to_owned(),
                        token: Some(Token::PartitionKey),
                        value: InspectValue {
                            lines: vec!["7".to_owned()],
                            token: Some(Token::PartitionKeyValue),
                        },
                    },
                    InspectField {
                        name: "a_column_name_longer_than_the…".to_owned(),
                        token: None,
                        value: InspectValue {
                            lines: vec!["null".to_owned()],
                            token: Some(Token::Faint),
                        },
                    },
                ],
            }
        );
    }

    #[test]
    fn a_cell_projects_its_value_laid_out_and_its_size() {
        let state = showing(page_of(&["note"], &[&[CqlValue::Text("[1,2]".to_owned())]]));
        let inspector = Inspector::opened(
            Subject::Table(TableRef::new("shop", "orders")),
            Inspected::Cell {
                column: "note".to_owned(),
                value: CqlValue::Text("[1,2]".to_owned()),
            },
        );

        let view = inspect(&state, &inspector);

        assert_eq!(view.size.as_deref(), Some("5 B"));
        assert_eq!(
            view.body,
            InspectBody::Cell(InspectValue {
                lines: ["[", "  1,", "  2", "]"].map(str::to_owned).to_vec(),
                token: None,
            })
        );
    }

    fn ids(count: usize) -> Page {
        page_of(&["id"], &vec![&[CqlValue::Int(1)][..]; count])
    }

    fn footer_of(state: &State) -> Option<Footer> {
        view_model(state).panels[2].footer.clone()
    }

    fn said(footer: &Footer) -> String {
        [
            Some(footer.rows.clone()),
            footer.size.as_ref().map(|size| size.text.clone()),
            footer.status.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
            + &footer
                .hint
                .as_ref()
                .map_or_else(String::new, |hint| format!(" — {hint}"))
    }

    fn with_more(page: Page) -> Page {
        Page {
            next: Some(crate::model::PagingState(vec![1])),
            ..page
        }
    }

    #[test]
    fn a_result_that_ended_counts_its_rows_and_says_nothing_more() {
        let footer = footer_of(&showing(ids(1234))).expect("a footer");

        assert_eq!(said(&footer), "1,234 rows");
    }

    #[test]
    fn a_result_with_more_says_so() {
        let footer = footer_of(&showing(with_more(ids(200)))).expect("a footer");

        assert_eq!(said(&footer), "200 rows · more");
    }

    #[test]
    fn a_page_in_flight_is_loading() {
        let mut state = showing(with_more(ids(3400)));
        state.contents.fetch_all(QueryId::next);

        assert_eq!(
            said(&footer_of(&state).expect("a footer")),
            "3,400 rows · loading"
        );
    }

    #[test]
    fn a_memory_stop_names_the_estimate_in_the_warning_colour() {
        let mut state = showing(with_more(ids(812)));
        state.contents.pretend(
            Bytes::new(109_470_105),
            Fetch::Stopped(Stopped::Memory(MEMORY_STEP)),
        );

        let footer = footer_of(&state).expect("a footer");
        assert_eq!(
            said(&footer),
            "812 rows · ≈ 104.4 MB · paused past 100 MB — P for more"
        );
        assert_eq!(
            footer.size.and_then(|size| size.token),
            Some(Token::Severity(Severity::Warning))
        );
    }

    #[test]
    fn a_row_stop_names_the_rows_it_stopped_at() {
        let mut state = showing(with_more(ids(3)));
        state.contents.pretend(
            Bytes::MB.times(61),
            Fetch::Stopped(Stopped::Rows(1_000_000)),
        );

        let footer = footer_of(&state).expect("a footer");
        assert_eq!(
            said(&footer),
            "3 rows · ≈ 61.0 MB · paused past 1,000,000 rows — P for more"
        );
        assert_eq!(
            footer.size.and_then(|size| size.token),
            Some(Token::Muted),
            "under the first memory stop, the estimate is not a warning"
        );
    }

    #[test]
    fn an_estimate_under_ten_megabytes_is_left_out() {
        let mut state = showing(with_more(ids(3)));
        state
            .contents
            .pretend(Bytes::new(SIZE_SHOWN_FROM.get() - 1), Fetch::Idle);

        assert_eq!(footer_of(&state).and_then(|footer| footer.size), None);
    }

    #[test]
    fn a_filter_leaves_the_footer_counting_every_row_fetched() {
        let mut state = showing(page_of(
            &["name"],
            &[
                &[CqlValue::Text("a".to_owned())],
                &[CqlValue::Text("b".to_owned())],
            ],
        ));
        state.contents.set_filter(0, Search::new("a"));

        assert_eq!(footer_of(&state).expect("a footer").rows, "2 rows");
    }

    #[test]
    fn only_a_loaded_grid_has_a_footer() {
        let state = tabled(&[("shop", &["orders"])]);

        assert!(
            view_model(&state)
                .panels
                .iter()
                .all(|panel| panel.footer.is_none())
        );
    }

    #[test]
    fn thousands_are_separated() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn only_the_rows_the_panel_can_show_are_projected() {
        let mut state = showing(ids(20));
        state.geometry = crate::state::Geometry::new([8, 8, 5, 1], 0, 0);
        state.contents.cursor.go_to(7, 20);
        state.contents.scroll.follow(7, 5);

        let PanelBody::Grid {
            rows,
            total,
            offset,
            ..
        } = contents_body(&state)
        else {
            panic!("a grid");
        };

        assert_eq!((rows.len(), total, offset), (5, 20, 3));
    }
}
