//! The Tables panel's projection: the keyspace/table tree as drawable rows.

use crate::model::{KeyspaceKind, Schema};
use crate::state::{ConnectionState, SchemaState, State};
use crate::style::Token;
use crate::tree::{Node, TablesPanel};

use super::{PanelBody, failure, waiting};

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

pub(super) fn body(state: &State) -> PanelBody {
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
        return match tables.filter() {
            Some(filter) => waiting(&format!(
                "No keyspace or table matches \"{}\"",
                filter.as_str()
            )),
            None => waiting("No keyspaces"),
        };
    }

    PanelBody::Tree {
        rows,
        selected: tables.cursor.index(),
        offset: tables.scroll.offset(),
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
    use crate::model::{ConnId, DbError};
    use crate::posture::Posture;
    use crate::state::{Connection, ConnectionState, SchemaState, State};
    use crate::style::{Severity, Token};

    use crate::search::{Filter, Filtered};

    use super::super::tests_support::{loaded, panel_body, state_with, tabled};
    use super::*;

    fn tables_body(state: &State) -> PanelBody {
        panel_body(state, 0)
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

        let PanelBody::Tree { rows, selected, .. } = tables_body(&state) else {
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
    fn a_filter_that_matches_nothing_says_so() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.apply_filter(Filtered::Tables, Filter::new("zzz"));

        assert_eq!(
            tables_body(&state),
            PanelBody::Notice {
                text: "No keyspace or table matches \"zzz\"".to_owned(),
                token: Some(Token::Muted),
            }
        );
    }

    #[test]
    fn an_unfiltered_empty_schema_still_says_no_keyspaces() {
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
}
