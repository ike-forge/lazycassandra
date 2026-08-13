//! One row as a CSV line
//! (docs/superpowers/specs/2026-08-13-yank-design.md §2).
//!
//! Pure projections from a row to a line: no `State`, no panel, no clipboard.
//! The panels differ in what their fields are, not in how a field is escaped,
//! and one encoder is what keeps that true.

use crate::cell;
use crate::model::{ClusteringOrder, CqlValue, Row};
use crate::tree::Node;
use crate::view::{ColumnKind, DetailRow};

/// `fields` as one CSV line: RFC 4180 as far as a single line goes, and
/// without the trailing newline that would make a paste into a shell submit
/// itself (design §2).
#[must_use]
pub fn line(fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| escaped(field.as_str()))
        .collect::<Vec<_>>()
        .join(",")
}

/// A grid row, every column of it — including the ones the horizontal
/// viewport has scrolled off-screen, and untruncated: what is drawn is a
/// window, what is yanked is the row (design §2.1).
#[must_use]
pub fn grid_row(row: &Row) -> String {
    let fields: Vec<String> = row.values.iter().map(value).collect();

    line(&fields)
}

#[must_use]
pub fn detail_row(row: &DetailRow) -> String {
    let fields = match row {
        DetailRow::Identity { kind, name } => vec![kind.label().to_owned(), name.clone()],
        DetailRow::KeyColumn {
            name,
            order,
            cql_type,
        } => {
            let mut fields = vec![name.clone(), cql_type.clone()];
            // Two fields or three, decided by the variant's own `Option`: a
            // partition column has no ordering, and padding it to a fixed
            // arity would put back the field the type removed (design §2.3).
            if let Some(order) = order {
                fields.push(ordering(*order).to_owned());
            }
            fields
        }
        DetailRow::Column {
            name,
            cql_type,
            kind,
        } => vec![name.clone(), cql_type.clone(), part(*kind).to_owned()],
        DetailRow::Property { name, value } => vec![name.clone(), value.clone()],
    };

    line(&fields)
}

/// The tree node, in the form every CQL statement takes it in (design §2.4).
#[must_use]
pub fn node(node: &Node<'_>) -> String {
    let fields = match node {
        Node::Keyspace { keyspace, .. } => vec![keyspace.name.clone()],
        Node::Table { keyspace, table } => {
            vec![format!("{}.{}", keyspace.name, table.name)]
        }
    };

    line(&fields)
}

fn escaped(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// A null yanks as nothing rather than as the `null` the grid draws: the grid
/// can tell the two apart with a muted token and a CSV line cannot, so a text
/// column holding the word would otherwise yank identically (design §2.1).
fn value(value: &CqlValue) -> String {
    match value {
        CqlValue::Null => String::new(),
        value => cell::text(value),
    }
}

const fn ordering(order: ClusteringOrder) -> &'static str {
    match order {
        ClusteringOrder::Ascending => "asc",
        ClusteringOrder::Descending => "desc",
    }
}

const fn part(kind: ColumnKind) -> &'static str {
    match kind {
        ColumnKind::Partition => "partition",
        ColumnKind::Clustering => "clustering",
        ColumnKind::Static => "static",
        ColumnKind::Regular => "regular",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ClusteringOrder, CqlValue, Keyspace, PrimaryKey, Replication, Row, Table, TableOptions,
    };
    use crate::tree::Node;
    use crate::view::{ColumnKind, DetailRow, IdentityKind};
    use std::collections::BTreeMap;

    fn table(name: &str) -> Table {
        Table {
            name: name.to_owned(),
            primary_key: PrimaryKey {
                partition: Vec::new(),
                clustering: Vec::new(),
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
            options: TableOptions::default(),
        }
    }

    #[test]
    fn a_field_is_quoted_only_when_it_has_to_be() {
        assert_eq!(
            line(&["plain".to_owned(), "has,comma".to_owned()]),
            "plain,\"has,comma\""
        );
        assert_eq!(line(&["has\nnewline".to_owned()]), "\"has\nnewline\"");
    }

    #[test]
    fn an_embedded_quote_is_doubled() {
        assert_eq!(line(&["say \"hi\"".to_owned()]), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn a_line_ends_without_a_newline() {
        assert!(!line(&["a".to_owned(), "b".to_owned()]).ends_with('\n'));
    }

    #[test]
    fn a_null_yanks_as_an_empty_field() {
        let row = Row {
            values: vec![CqlValue::Int(1), CqlValue::Null],
        };

        assert_eq!(grid_row(&row), "1,");
    }

    #[test]
    fn a_text_null_yanks_as_the_word() {
        let row = Row {
            values: vec![CqlValue::Text("null".to_owned())],
        };

        assert_eq!(grid_row(&row), "null");
    }

    #[test]
    fn a_column_row_carries_the_part_it_plays() {
        let row = DetailRow::Column {
            name: "created_at".to_owned(),
            cql_type: "timestamp".to_owned(),
            kind: ColumnKind::Clustering,
        };

        assert_eq!(detail_row(&row), "created_at,timestamp,clustering");
    }

    #[test]
    fn a_partition_column_yanks_without_an_order() {
        let partition = DetailRow::KeyColumn {
            name: "id".to_owned(),
            order: None,
            cql_type: "uuid".to_owned(),
        };
        let clustering = DetailRow::KeyColumn {
            name: "created_at".to_owned(),
            order: Some(ClusteringOrder::Descending),
            cql_type: "timestamp".to_owned(),
        };

        assert_eq!(detail_row(&partition), "id,uuid");
        assert_eq!(detail_row(&clustering), "created_at,timestamp,desc");
    }

    #[test]
    fn a_property_and_an_identity_row_yank_their_two_fields() {
        let property = DetailRow::Property {
            name: "compaction".to_owned(),
            value: "SizeTieredCompactionStrategy".to_owned(),
        };
        let identity = DetailRow::Identity {
            kind: IdentityKind::Table,
            name: "users".to_owned(),
        };

        assert_eq!(
            detail_row(&property),
            "compaction,SizeTieredCompactionStrategy"
        );
        assert_eq!(detail_row(&identity), "Table,users");
    }

    #[test]
    fn a_keyspace_yanks_its_name_and_a_table_its_qualified_one() {
        let keyspace = Keyspace {
            name: "demo".to_owned(),
            replication: Replication::SimpleStrategy { factor: 1 },
            tables: BTreeMap::new(),
        };
        let users = table("users");

        assert_eq!(
            node(&Node::Keyspace {
                keyspace: &keyspace,
                unfolded: false
            }),
            "demo"
        );
        assert_eq!(
            node(&Node::Table {
                keyspace: &keyspace,
                table: &users
            }),
            "demo.users"
        );
    }
}
