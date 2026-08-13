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
    /// What the row names, and where it sits under its keyspace. Not derivable
    /// from [`TreeRow::token`]: a system keyspace and its tables are both
    /// [`Token::Faint`].
    pub kind: RowKind,
    /// [`Token::Faint`] for the cluster's own keyspaces and their tables.
    /// A user keyspace and a user table are told apart, which is the
    /// distinction the panel exists for.
    pub token: Token,
}

/// What a tree row names.
///
/// A table carries whether it is the last one drawn under its keyspace, which
/// is what decides the limb the renderer draws it on. Last-ness is over the
/// rows the panel actually shows, so a filter that hides the tail of a
/// keyspace moves the elbow up rather than leaving it on a row nobody sees
/// (docs/superpowers/specs/2026-08-12-nerd-font-icons-design.md §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Keyspace,
    Table { last: bool },
}

impl RowKind {
    #[must_use]
    pub const fn is_table(self) -> bool {
        matches!(self, Self::Table { .. })
    }
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
    let nodes = tables.rows(schema);
    let rows: Vec<TreeRow> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            tree_row(
                node,
                matches!(nodes.get(index + 1), Some(Node::Table { .. })),
            )
        })
        .collect();

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

/// `followed` says whether the next row drawn is another table of the same
/// keyspace — the only thing a table row needs in order to know it is the last
/// of its own, because a keyspace's tables are contiguous and a keyspace row
/// ends the run.
fn tree_row(node: &Node<'_>, followed: bool) -> TreeRow {
    let (keyspace, label, kind) = match node {
        Node::Keyspace { keyspace, .. } => (*keyspace, keyspace.name.clone(), RowKind::Keyspace),
        Node::Table { keyspace, table } => (
            *keyspace,
            table.name.clone(),
            RowKind::Table { last: !followed },
        ),
    };

    let token = match (keyspace.kind(), kind) {
        (KeyspaceKind::System, _) => Token::Faint,
        (KeyspaceKind::User, RowKind::Keyspace) => Token::Keyspace,
        (KeyspaceKind::User, RowKind::Table { .. }) => Token::Table,
    };

    TreeRow { label, kind, token }
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
        assert_eq!(
            rows[0].token,
            Token::Keyspace,
            "a user keyspace is told apart from a user table"
        );
        assert_eq!(rows[2].token, Token::Faint);
        assert_eq!(selected, 0);
    }

    #[test]
    fn a_keyspace_with_no_tables_offers_nothing_to_unfold() {
        let state = loaded(&["shop"]);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(
            rows[0].kind,
            RowKind::Keyspace,
            "a keyspace with nothing under it is still a keyspace"
        );
    }

    #[test]
    fn a_keyspace_with_tables_is_folded_until_it_is_opened() {
        let mut state = tabled(&[("shop", &["orders", "users"])]);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, RowKind::Keyspace);

        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };
        assert_eq!(
            rows.iter()
                .map(|row| (row.kind, row.label.as_str()))
                .collect::<Vec<_>>(),
            [
                (RowKind::Keyspace, "shop"),
                (RowKind::Table { last: false }, "orders"),
                (RowKind::Table { last: true }, "users"),
            ]
        );
    }

    #[test]
    fn a_table_is_faint_exactly_when_its_keyspace_is() {
        let mut state = tabled(&[("system_schema", &["tables"])]);
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(rows[0].token, Token::Faint);
        assert_eq!(
            rows[1].token,
            Token::Faint,
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
    fn a_keyspace_and_a_table_are_told_apart_by_colour() {
        let mut state = tabled(&[("shop", &["orders"])]);
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema draws a tree");
        };

        let keyspace = rows
            .iter()
            .find(|row| row.kind == RowKind::Keyspace)
            .expect("a keyspace");
        let table = rows
            .iter()
            .find(|row| row.kind.is_table())
            .expect("a table");

        assert_eq!(keyspace.token, Token::Keyspace);
        assert_eq!(table.token, Token::Table);
        assert_ne!(
            keyspace.token, table.token,
            "the panel's whole job is the keyspace/table distinction"
        );
    }

    #[test]
    fn a_cluster_keyspace_stays_quiet() {
        let mut state = tabled(&[("system_schema", &["tables"])]);
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema draws a tree");
        };

        assert!(
            rows.iter().all(|row| row.token == Token::Faint),
            "a system keyspace and its tables are the cluster's, not the user's"
        );
    }
}
