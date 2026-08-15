//! The Contents panel's projection: a page of rows as a drawable grid.
//!
//! Unlike the other two panels this one does not follow the tree cursor, so
//! its title is what says which table is on screen (vision.md §4, principle
//! 8).

use crate::cell;
use crate::contents::{ContentsState, GridPage, SelectionMode, Subject, TableRef};
use crate::model::{CqlValue, Schema};
use crate::search::Filter;
use crate::state::{Panel, Ranges, State};
use crate::style::{Segment, Token};

use super::{PanelBody, failure, waiting};

/// One column heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridColumn {
    pub name: String,
    /// The column's place in the physical layout —
    /// [`Token::PartitionKey`], [`Token::ClusteringKey`] or
    /// [`Token::RegularColumn`]. `None` when the schema no longer has the
    /// table, so no claim about its shape can be checked.
    pub token: Option<Token>,
    /// Character columns it is drawn in, capped by
    /// [`crate::grid::MAX_COLUMN_WIDTH`]. Computed here rather than in the
    /// renderer so the fitting arithmetic can live beside the offsets it
    /// bounds (docs/superpowers/specs/2026-08-10-table-display-design.md §6).
    pub width: u16,
}

/// One value, already rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCell {
    pub text: String,
    /// `Some(Token::Faint)` for a null, which is what keeps it apart from a
    /// text column whose value is the word "null".
    pub token: Option<Token>,
    /// Whether the applied filter matched `text`.
    ///
    /// A field of its own rather than a second meaning for `token`:
    /// overwriting the null marker on a match would lose that distinction
    /// for exactly the cells the user went looking for (design §4.3).
    pub matched: bool,
}

pub(super) fn title(state: &State) -> Vec<Segment> {
    const BASE: &str = "Table contents";

    match state.contents.state() {
        ContentsState::Empty => vec![Segment::plain(BASE)],
        ContentsState::Loading { subject, .. } | ContentsState::Failed { subject, .. } => {
            named(BASE, subject, None)
        }
        ContentsState::Loaded { subject, page } => {
            named(BASE, subject, Some(rows(page.page().rows.len())))
        }
    }
}

/// The panel's name, then what it is showing, then how much of it.
///
/// The qualified name keeps the schema's own colours ([`TableRef::segments`]):
/// this panel does not follow the tree cursor, so its title is the only thing
/// on screen saying which table is up (vision.md §4, principle 8), and it says
/// it the way the tree does.
fn named(base: &str, subject: &Subject, count: Option<String>) -> Vec<Segment> {
    let mut segments = vec![Segment::plain(format!("{base} — "))];

    match subject {
        Subject::Table(table) => segments.extend(table.segments()),
        // A typed statement names no one table, so there is nothing to colour.
        Subject::Typed(_) => segments.push(Segment::plain(subject.label())),
    }

    if let Some(count) = count {
        segments.push(Segment::plain(format!(" ({count})")));
    }

    segments
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
        ContentsState::Failed { error, .. } => failure(error),
        ContentsState::Loaded { subject, page } => grid(state, subject, page),
    }
}

fn grid(state: &State, subject: &Subject, grid_page: &GridPage) -> PanelBody {
    let page = grid_page.page();

    if page.rows.is_empty() {
        return waiting("No rows");
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

    let tokens: Vec<Option<Token>> = page
        .columns
        .iter()
        .map(|name| column_token(state.schema(), subject.table(), name))
        .collect();

    PanelBody::Grid {
        columns: page
            .columns
            .iter()
            .zip(grid_page.widths())
            .zip(&tokens)
            .map(|((name, width), token)| GridColumn {
                name: name.clone(),
                token: *token,
                width: *width,
            })
            .collect(),
        rows: page
            .rows
            .iter()
            .map(|row| {
                row.values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        grid_cell(
                            value,
                            tokens.get(index).copied().flatten(),
                            state.contents.filter(),
                        )
                    })
                    .collect()
            })
            .collect(),
        selected: state.contents.cursor.index(),
        offset: state.contents.scroll.offset(),
        first_column: state.contents.column_scroll.first(),
        column: state.contents.column.index(),
        mode: state.contents.mode,
        gutter: crate::grid::gutter(page.rows.len()),
        rows_ranges,
        columns_ranges,
        flash: flash.is_some(),
    }
}

fn grid_cell(value: &CqlValue, column: Option<Token>, filter: Option<&Filter>) -> GridCell {
    let text = cell::text(value);

    GridCell {
        matched: filter.is_some_and(|filter| filter.matches(&text)),
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
/// `None` for a typed statement: its columns can be a projection, an
/// aggregate, or a join of nothing, and claiming a partition key among them
/// would be a decoration the schema does not support.
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
    } else {
        Some(Token::RegularColumn)
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{
        ClusteringColumn, ClusteringOrder, Column, ConnId, DbError, Page, QueryId, Row, Schema,
    };
    use crate::posture::Posture;
    use crate::search::{Filter, Filtered};
    use crate::state::{Connection, ConnectionState, SchemaState, State};
    use crate::style::Severity;

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
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(query, page);
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

    /// A statement names no one table, so the title has nothing to colour —
    /// and still reads exactly as it did.
    #[test]
    fn a_typed_subjects_title_carries_no_colour() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.contents.requested(
            QueryId::next(),
            Subject::Typed("SELECT * FROM shop.orders".to_owned()),
        );

        assert!(
            title_segments(&state)
                .iter()
                .all(|segment| segment.token.is_none())
        );
        assert_eq!(
            contents_title(&state),
            "Table contents — SELECT * FROM shop.orders"
        );
    }

    #[test]
    fn a_failed_read_is_readable_in_the_panel() {
        let mut state = tabled(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.failed(query, DbError::Timeout);

        let PanelBody::Notice { text, token } = contents_body(&state) else {
            panic!("a failed read has no rows to list");
        };
        assert!(text.contains("timed out"));
        assert_eq!(token, Some(Token::Severity(Severity::Danger)));
        assert_eq!(contents_title(&state), "Table contents — shop.orders");
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
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema),
        });
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            page_of(
                &["id", "at", "total"],
                &[&[CqlValue::Int(1), CqlValue::Int(2), CqlValue::Int(3)]],
            ),
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

    #[test]
    fn a_page_whose_table_is_gone_claims_nothing_about_its_shape() {
        let mut state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
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
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema),
        });
        let query = QueryId::next();
        state
            .contents
            .requested(query, Subject::Table(TableRef::new("shop", "orders")));
        state.contents.page_ready(
            query,
            page_of(
                &["id", "at", "total"],
                &[&[CqlValue::Int(1), CqlValue::Int(2), CqlValue::Int(3)]],
            ),
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
    fn a_filter_flags_the_cells_that_match_and_keeps_every_row() {
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

        state.apply_filter(Filtered::Contents, Filter::new("ali"));

        let PanelBody::Grid { rows, .. } = contents_body(&state) else {
            panic!("a filter must not turn the grid into a notice");
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

        state.apply_filter(Filtered::Contents, Filter::new("null"));

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
            "a null renders as the word null, so it answers this filter"
        );
        assert_eq!(
            null.token,
            Some(Token::Faint),
            "and must stay distinguishable from a text column holding \"null\""
        );
    }

    #[test]
    fn no_filter_flags_nothing() {
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
            vec![2, 5]
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
}
