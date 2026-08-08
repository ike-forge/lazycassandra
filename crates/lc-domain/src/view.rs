//! The domain's projection of itself for a renderer.
//!
//! architecture.md §3. One tree is rebuilt per frame; §11's question about
//! per-panel view models with dirty tracking stays open until panels carry
//! enough content to measure.

use crate::cell;
use crate::contents::{ContentsState, TableRef};
use crate::model::{CqlValue, DbError, KeyspaceKind, Page, Schema};
use crate::state::{ConnectionState, Overlay, Panel, SchemaState, State};
use crate::style::{Severity, Token};
use crate::tree::{Node, TablesPanel};

/// Everything a renderer needs for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewModel {
    /// In [`Panel::ALL`] order.
    pub panels: [PanelView; 4],
    pub focus: Panel,
    /// Drawn over the panels when something needs saying.
    pub overlay: Option<Overlay>,
}

/// One panel, described without reference to how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub panel: Panel,
    pub title: String,
    pub body: PanelBody,
}

/// What a panel has to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelBody {
    Blank,
    /// A sentence where content would be: waiting, empty, or failed.
    ///
    /// `token` carries the meaning the renderer must draw it with — `None`
    /// styles nothing, `Some(Token::Muted)` for a waiting state,
    /// `Some(Token::Severity(Severity::Danger))` for a failure. Mirrors
    /// [`TreeRow::token`] so the panel never has to decide what a sentence
    /// means; the domain already knows (architecture.md §6).
    Notice {
        text: String,
        token: Option<Token>,
    },
    Tree {
        rows: Vec<TreeRow>,
        selected: usize,
    },
    /// A result set: one header per column, one row per row.
    ///
    /// Carries no widths. Sizing columns needs to know how wide the panel is,
    /// which is the renderer's business for the same reason scrolling is.
    Grid {
        columns: Vec<GridColumn>,
        rows: Vec<Vec<GridCell>>,
        selected: usize,
    },
}

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

/// One row of the Tables tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub label: String,
    /// 0 for a keyspace, 1 for a table.
    pub depth: u8,
    pub marker: Marker,
    /// `Some(Token::Muted)` for the cluster's own keyspaces and their tables.
    /// A user keyspace spends no token: it is the baseline everything else is
    /// read against.
    pub token: Option<Token>,
}

/// Whether a row has children, and whether they are showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Folded,
    Unfolded,
    /// Nothing to unfold: a table, or a keyspace with no tables.
    Leaf,
}

/// Project `state` into the frame a renderer draws.
#[must_use]
pub fn view_model(state: &State) -> ViewModel {
    ViewModel {
        panels: Panel::ALL.map(|panel| PanelView {
            panel,
            title: title(state, panel),
            body: body(state, panel),
        }),
        focus: state.focus,
        overlay: state.overlay.clone(),
    }
}

fn title(state: &State, panel: Panel) -> String {
    match panel {
        Panel::Tables => "Tables".to_owned(),
        Panel::Ddl => "Table DDL".to_owned(),
        Panel::Contents => contents_title(state),
        Panel::Queries => "Queries".to_owned(),
    }
}

/// The Contents panel does not follow the tree cursor, so the title is what
/// says which table is on screen (vision.md §4, principle 8).
fn contents_title(state: &State) -> String {
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

fn body(state: &State, panel: Panel) -> PanelBody {
    match panel {
        Panel::Tables => tables_body(state),
        Panel::Contents => contents_body(state),
        Panel::Ddl | Panel::Queries => PanelBody::Blank,
    }
}

fn waiting(text: &str) -> PanelBody {
    PanelBody::Notice {
        text: text.to_owned(),
        token: Some(Token::Muted),
    }
}

fn failure(error: &DbError) -> PanelBody {
    PanelBody::Notice {
        text: error.to_string(),
        token: Some(Token::Severity(Severity::Danger)),
    }
}

fn tables_body(state: &State) -> PanelBody {
    match &state.connection {
        ConnectionState::Disconnected => waiting("Not connected"),
        ConnectionState::Connecting(_) => waiting("Connecting…"),
        ConnectionState::Failed(error) => failure(error),
        ConnectionState::Connected(connection) => match &connection.schema {
            SchemaState::Loading => waiting("Loading schema…"),
            SchemaState::Failed(error) => failure(error),
            SchemaState::Loaded(schema) => tree(&state.tables, schema),
        },
    }
}

fn tree(tables: &TablesPanel, schema: &Schema) -> PanelBody {
    let rows: Vec<TreeRow> = tables.rows(schema).iter().map(tree_row).collect();

    if rows.is_empty() {
        return waiting("No keyspaces");
    }

    PanelBody::Tree {
        rows,
        selected: tables.cursor.index(),
    }
}

fn contents_body(state: &State) -> PanelBody {
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

fn tree_row(node: &Node<'_>) -> TreeRow {
    let (keyspace, label, depth, marker) = match node {
        Node::Keyspace { keyspace, unfolded } => {
            let marker = if keyspace.tables.is_empty() {
                Marker::Leaf
            } else if *unfolded {
                Marker::Unfolded
            } else {
                Marker::Folded
            };
            (*keyspace, keyspace.name.clone(), 0, marker)
        }
        Node::Table { keyspace, table } => (*keyspace, table.name.clone(), 1, Marker::Leaf),
    };

    TreeRow {
        label,
        depth,
        marker,
        token: match keyspace.kind() {
            KeyspaceKind::User => None,
            KeyspaceKind::System => Some(Token::Muted),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnId, DbError, Schema};
    use crate::posture::Posture;
    use crate::state::{Connection, ConnectionState, Overlay, SchemaState};
    use crate::style::{Severity, Token};
    use crate::tree::tests_support::schema_of as tabled_schema;

    #[test]
    fn every_panel_is_projected_in_reading_order() {
        let view = view_model(&State::default());

        assert_eq!(view.panels.map(|panel| panel.panel), Panel::ALL);
    }

    #[test]
    fn panels_carry_the_titles_the_draft_names() {
        let view = view_model(&State::default());

        assert_eq!(
            view.panels.map(|panel| panel.title),
            ["Tables", "Table DDL", "Table contents", "Queries"].map(str::to_owned)
        );
    }

    #[test]
    fn focus_mirrors_the_state() {
        let state = State {
            focus: Panel::Contents,
            ..State::default()
        };

        assert_eq!(view_model(&state).focus, Panel::Contents);
    }

    fn schema_of(names: &[&str]) -> Schema {
        tabled_schema(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
    }

    fn state_with(connection: ConnectionState) -> State {
        State {
            connection,
            ..State::default()
        }
    }

    fn loaded(names: &[&str]) -> State {
        state_with(ConnectionState::Connected(Connection {
            id: ConnId::next(),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema_of(names)),
        }))
    }

    fn tabled(keyspaces: &[(&str, &[&str])]) -> State {
        state_with(ConnectionState::Connected(Connection {
            id: ConnId::next(),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(tabled_schema(keyspaces)),
        }))
    }

    fn tables_body(state: &State) -> PanelBody {
        view_model(state).panels[0].body.clone()
    }

    #[test]
    fn the_tables_panel_says_what_it_is_waiting_for() {
        assert_eq!(
            tables_body(&State::default()),
            PanelBody::Notice {
                text: "Not connected".to_owned(),
                token: Some(Token::Muted),
            }
        );
        assert_eq!(
            tables_body(&state_with(ConnectionState::Connecting(ConnId::next()))),
            PanelBody::Notice {
                text: "Connecting…".to_owned(),
                token: Some(Token::Muted),
            }
        );
        assert_eq!(
            tables_body(&state_with(ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loading,
            }))),
            PanelBody::Notice {
                text: "Loading schema…".to_owned(),
                token: Some(Token::Muted),
            }
        );
    }

    #[test]
    fn a_failure_is_readable_in_the_panel_and_not_only_in_the_overlay() {
        let body = tables_body(&state_with(ConnectionState::Failed(DbError::Timeout)));

        let PanelBody::Notice { text, token } = body else {
            panic!("a failed connection has nothing to list");
        };
        assert!(text.contains("timed out"));
        assert_eq!(
            token,
            Some(Token::Severity(Severity::Danger)),
            "a failure must carry the danger token, not the muted one waiting states use"
        );
    }

    #[test]
    fn keyspaces_are_listed_with_the_clusters_own_dimmed_and_last() {
        let state = loaded(&["system_schema", "typezoo", "shop"]);

        let PanelBody::Tree { rows, selected } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(
            rows.iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            ["shop", "typezoo", "system_schema"]
        );
        assert_eq!(rows[0].token, None, "a user keyspace spends no token");
        assert_eq!(rows[2].token, Some(Token::Muted));
        assert_eq!(selected, 0);
    }

    #[test]
    fn a_keyspace_with_no_tables_offers_nothing_to_unfold() {
        let state = loaded(&["shop"]);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(
            rows[0].marker,
            Marker::Leaf,
            "an unfoldable-looking row that unfolds to nothing is worse than none"
        );
    }

    #[test]
    fn a_keyspace_with_tables_is_folded_until_it_is_opened() {
        let mut state = tabled(&[("shop", &["orders", "users"])]);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].marker, Marker::Folded);
        assert_eq!(rows[0].depth, 0);

        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };
        assert_eq!(
            rows.iter()
                .map(|row| (row.depth, row.marker, row.label.as_str()))
                .collect::<Vec<_>>(),
            [
                (0, Marker::Unfolded, "shop"),
                (1, Marker::Leaf, "orders"),
                (1, Marker::Leaf, "users"),
            ]
        );
    }

    #[test]
    fn a_table_is_muted_exactly_when_its_keyspace_is() {
        let mut state = tabled(&[("system_schema", &["tables"])]);
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(rows[0].token, Some(Token::Muted));
        assert_eq!(
            rows[1].token,
            Some(Token::Muted),
            "one rule per row, not two rules that can drift"
        );
    }

    #[test]
    fn the_selection_is_projected_from_the_cursor() {
        let mut state = loaded(&["shop", "typezoo"]);
        state.tables.cursor.down(2);

        let PanelBody::Tree { selected, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(selected, 1);
    }

    #[test]
    fn a_cluster_with_no_keyspaces_says_so_rather_than_listing_nothing() {
        assert_eq!(
            tables_body(&loaded(&[])),
            PanelBody::Notice {
                text: "No keyspaces".to_owned(),
                token: Some(Token::Muted),
            }
        );
    }

    #[test]
    fn a_schema_that_failed_to_load_carries_the_danger_token() {
        let state = state_with(ConnectionState::Connected(Connection {
            id: ConnId::next(),
            posture: Posture::ReadOnly,
            schema: SchemaState::Failed(DbError::Invalid("unauthorized".to_owned())),
        }));

        let PanelBody::Notice { text, token } = tables_body(&state) else {
            panic!("a failed schema has nothing to list");
        };
        assert!(text.contains("unauthorized"));
        assert_eq!(token, Some(Token::Severity(Severity::Danger)));
    }

    #[test]
    fn the_ddl_and_queries_panels_are_still_blank() {
        let view = view_model(&loaded(&["shop"]));

        for panel in view
            .panels
            .iter()
            .filter(|panel| matches!(panel.panel, Panel::Ddl | Panel::Queries))
        {
            assert_eq!(
                panel.body,
                PanelBody::Blank,
                "{:?} has no content yet",
                panel.panel
            );
        }
    }

    fn contents_body(state: &State) -> PanelBody {
        view_model(state).panels[2].body.clone()
    }

    fn contents_title(state: &State) -> String {
        view_model(state).panels[2].title.clone()
    }

    fn page_of(columns: &[&str], rows: &[&[CqlValue]]) -> Page {
        Page {
            columns: columns.iter().map(|name| (*name).to_owned()).collect(),
            rows: rows
                .iter()
                .map(|values| crate::model::Row {
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
        let query = crate::model::QueryId::next();
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
        state.contents.requested(
            crate::model::QueryId::next(),
            TableRef::new("shop", "orders"),
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

    #[test]
    fn a_failed_read_is_readable_in_the_panel() {
        let mut state = tabled(&[("shop", &["orders"])]);
        let query = crate::model::QueryId::next();
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
        let schema = state.schema().cloned().expect("loaded");
        let mut schema = schema;
        let table = schema
            .keyspaces
            .get_mut("shop")
            .and_then(|keyspace| keyspace.tables.get_mut("orders"))
            .expect("the builder made it");
        table.primary_key.clustering = vec![crate::model::ClusteringColumn {
            column: crate::model::Column {
                name: "at".to_owned(),
                cql_type: "timestamp".to_owned(),
            },
            order: crate::model::ClusteringOrder::Ascending,
        }];
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema),
        });
        let query = crate::model::QueryId::next();
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

    #[test]
    fn the_overlay_is_carried_to_the_renderer() {
        let state = State {
            overlay: Some(Overlay {
                title: "Connection failed".to_owned(),
                message: "no route".to_owned(),
                severity: Severity::Danger,
            }),
            ..State::default()
        };

        assert_eq!(view_model(&state).overlay, state.overlay);
    }
}
