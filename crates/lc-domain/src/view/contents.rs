//! The Contents panel's projection: a page of rows as a drawable grid.
//!
//! Unlike the other two panels this one does not follow the tree cursor, so
//! its title is what says which table is on screen (vision.md §4, principle
//! 8).

use crate::cell;
use crate::contents::{ContentsState, TableRef};
use crate::model::{CqlValue, Page, Schema};
use crate::state::State;
use crate::style::Token;

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
}

/// One value, already rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCell {
    pub text: String,
    /// `Some(Token::Muted)` for a null, which is what keeps it apart from a
    /// text column whose value is the word "null".
    pub token: Option<Token>,
}

pub(super) fn title(state: &State) -> String {
    const BASE: &str = "Table contents";

    match state.contents.state() {
        ContentsState::Empty => BASE.to_owned(),
        ContentsState::Loading { table, .. } | ContentsState::Failed { table, .. } => {
            format!("{BASE} — {}", table.qualified())
        }
        ContentsState::Loaded { table, page } => {
            format!("{BASE} — {} ({})", table.qualified(), rows(page.rows.len()))
        }
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
        ContentsState::Failed { error, .. } => failure(error),
        ContentsState::Loaded { table, page } => grid(state, table, page),
    }
}

fn grid(state: &State, table: &TableRef, page: &Page) -> PanelBody {
    if page.rows.is_empty() {
        return waiting("No rows");
    }

    PanelBody::Grid {
        columns: page
            .columns
            .iter()
            .map(|name| GridColumn {
                name: name.clone(),
                token: column_token(state.schema(), table, name),
            })
            .collect(),
        rows: page
            .rows
            .iter()
            .map(|row| row.values.iter().map(grid_cell).collect())
            .collect(),
        selected: state.contents.cursor.index(),
        offset: state.contents.scroll.offset(),
    }
}

fn grid_cell(value: &CqlValue) -> GridCell {
    GridCell {
        text: cell::text(value),
        token: match value {
            CqlValue::Null => Some(Token::Muted),
            _ => None,
        },
    }
}

/// Where a column sits in the table's physical layout, which vision.md §2
/// exists to keep visible — here, in the grid being read, rather than a panel
/// away.
fn column_token(schema: Option<&Schema>, table: &TableRef, column: &str) -> Option<Token> {
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
        ClusteringColumn, ClusteringOrder, Column, ConnId, DbError, QueryId, Row, Schema,
    };
    use crate::posture::Posture;
    use crate::state::{Connection, ConnectionState, SchemaState, State};
    use crate::style::Severity;

    use super::super::tests_support::{panel_body, tabled};
    use super::super::view_model;
    use super::*;

    fn contents_body(state: &State) -> PanelBody {
        panel_body(state, 2)
    }

    fn contents_title(state: &State) -> String {
        view_model(state).panels[2].title.text().to_owned()
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
            .requested(query, TableRef::new("shop", "orders"));
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
        state
            .contents
            .requested(QueryId::next(), TableRef::new("shop", "orders"));

        assert_eq!(
            contents_body(&state),
            PanelBody::Notice {
                text: "Loading rows…".to_owned(),
                token: Some(Token::Muted),
            }
        );
        assert_eq!(contents_title(&state), "Table contents — shop.orders");
    }

    #[test]
    fn a_failed_read_is_readable_in_the_panel() {
        let mut state = tabled(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, TableRef::new("shop", "orders"));
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
    fn a_null_is_muted_so_it_cannot_be_read_as_the_word() {
        let state = showing(page_of(
            &["id", "email"],
            &[&[CqlValue::Int(1), CqlValue::Null]],
        ));

        let PanelBody::Grid { rows, .. } = contents_body(&state) else {
            panic!("a loaded page draws a grid");
        };
        assert_eq!(rows[0][0].token, None);
        assert_eq!(rows[0][1].token, Some(Token::Muted));
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
            .requested(query, TableRef::new("shop", "orders"));
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
}
