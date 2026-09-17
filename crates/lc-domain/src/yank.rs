//! One row as a CSV line
//! (docs/superpowers/specs/2026-08-13-yank-design.md §2).
//!
//! Pure projections from a row to a line: no `State`, no panel, no clipboard.
//! The panels differ in what their fields are, not in how a field is escaped,
//! and one encoder is what keeps that true.

use crate::cell;
use crate::log::Entry;
use crate::model::{ClusteringOrder, CqlValue, Row};
use crate::state::Ranges;
use crate::tree::Node;
use crate::view::logs::outcome_text;
use crate::view::{ColumnKind, DetailRow};

/// `fields` as one CSV line: RFC 4180 as far as a single line goes, and
/// without the trailing newline that would make a paste into a shell submit
/// itself (design §2).
#[must_use]
pub(crate) fn line(fields: &[String]) -> String {
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

/// One cell, raw: a lone value has no delimiter to protect, so the encoder
/// would only add characters the user then deletes (design §3).
///
/// `None` for a null, so "a null is inert" lives in the type rather than in a
/// check at the call site (design §4).
#[must_use]
pub fn grid_cell(value: &CqlValue) -> Option<String> {
    match value {
        CqlValue::Null => None,
        value => Some(cell::text(value)),
    }
}

/// One column of a page, top to bottom, as one CSV line — no heading, because
/// a heading is not a value (design §1).
#[must_use]
pub fn grid_column(rows: &[Row], column: usize) -> String {
    let fields: Vec<String> = rows
        .iter()
        .map(|row| row.values.get(column).map_or_else(String::new, value))
        .collect();

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
        // The map as the cluster reported it, not the summary drawn over it:
        // what a yank is for is pasting back into CQL.
        DetailRow::ClassProperty { name, raw, .. } => vec![name.clone(), raw.clone()],
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

/// An entry's two rows joined — the statement and its outcome — which is
/// what makes it paste into a ticket (query log design §5.3). `outcome_text`
/// is the string the projection draws the outcome row with, not a second
/// spelling of it.
#[must_use]
pub fn log_entry(entry: &Entry) -> String {
    format!(
        "{} {}",
        entry.subject().text(),
        outcome_text(entry.outcome())
    )
}

/// Several lines as one payload: no trailing newline, for the reason a single
/// line has none — a paste into a shell must not submit itself
/// (docs/superpowers/specs/2026-08-14-visual-mode-design.md §5).
#[must_use]
pub fn lines(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join("\n")
}

/// A block of the page: one line per selected row, holding the selected
/// columns and no others (multi-group selection design §5).
#[must_use]
pub fn grid_block(rows: &[Row], selected: &Ranges, columns: &Ranges) -> String {
    lines(selected.indices().filter_map(|index| {
        let row = rows.get(index)?;
        let fields: Vec<String> = columns
            .indices()
            .filter_map(|column| row.values.get(column))
            .map(value)
            .collect();

        Some(line(&fields))
    }))
}

/// One line per selected column, each being that column's values top to
/// bottom.
///
/// Not the block read row-wise: a column already yanks as its values in order
/// (yank design §1), and a set of columns is that, once per column, left to
/// right (multi-group selection design §5).
#[must_use]
pub fn grid_columns(rows: &[Row], columns: &Ranges) -> String {
    lines(columns.indices().map(|column| grid_column(rows, column)))
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
    use crate::clock::Stamp;
    use crate::log::{Outcome, Subject};
    use crate::model::{
        ClusteringOrder, CqlValue, Keyspace, Page, PrimaryKey, Replication, Row, Setting, Table,
        TableOptions,
    };
    use crate::state::Range;
    use crate::tree::Node;
    use crate::view::{ColumnKind, DetailRow, IdentityKind};
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn sample_rows(rows: usize, columns: usize) -> Vec<Row> {
        (0..rows)
            .map(|row| Row {
                values: (0..columns)
                    .map(|column| CqlValue::Text(format!("r{row}c{column}")))
                    .collect(),
            })
            .collect()
    }

    fn table(name: &str) -> Table {
        Table {
            name: name.to_owned(),
            primary_key: PrimaryKey {
                partition: Vec::new(),
                clustering: Vec::new(),
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
            indexes: Vec::new(),
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
    fn a_yanked_value_is_not_ellipsised() {
        let value = "x".repeat(usize::from(crate::config::MaxColumnWidth::default().get()) + 10);
        let page = Page {
            columns: vec!["blob".to_owned()],
            rows: vec![Row {
                values: vec![CqlValue::Text(value.clone())],
            }],
            next: None,
        };

        let drawn_width = crate::grid::widths(&page, crate::config::MaxColumnWidth::default())[0];
        assert_eq!(
            drawn_width,
            crate::config::MaxColumnWidth::default().get(),
            "the grid draws this column capped, not at the value's full length"
        );

        assert_eq!(
            grid_row(&page.rows[0]),
            value,
            "the yanked field keeps what the capped column the grid draws would have to ellipsise"
        );
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
    fn a_class_property_yanks_the_map_rather_than_the_summary_drawn_over_it() {
        let row = DetailRow::ClassProperty {
            name: "compression".to_owned(),
            class: "LZ4Compressor".to_owned(),
            settings: vec![Setting {
                label: "chunk",
                value: "16kb".to_owned(),
            }],
            raw: "{chunk_length_in_kb: 16, class: org.apache.cassandra.io.compress.LZ4Compressor}"
                .to_owned(),
        };

        assert_eq!(
            detail_row(&row),
            "compression,\"{chunk_length_in_kb: 16, class: org.apache.cassandra.io.compress.LZ4Compressor}\"",
            "a paste back into CQL needs the map, not the abbreviation"
        );
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

    #[test]
    fn a_log_entry_yanks_its_subject_and_its_outcome_joined() {
        let entry = Entry::new(
            Stamp::default(),
            Subject::Sentence("SELECT one".to_owned()),
            Outcome::Cancelled,
        );

        assert_eq!(log_entry(&entry), "SELECT one ⊘ cancelled");
    }

    #[test]
    fn a_log_entrys_outcome_is_the_same_text_the_projection_draws() {
        let entry = Entry::new(
            Stamp::default(),
            Subject::Sentence("SELECT one".to_owned()),
            Outcome::Succeeded {
                detail: "1 row".to_owned(),
                took: Duration::from_millis(4),
            },
        );

        assert_eq!(log_entry(&entry), "SELECT one → 1 row, 4ms");
    }

    #[test]
    fn a_cell_yanks_without_quoting() {
        assert_eq!(
            grid_cell(&CqlValue::Text("Smith, John".to_owned())),
            Some("Smith, John".to_owned()),
            "a lone value has no delimiter to protect"
        );
        assert_eq!(
            grid_cell(&CqlValue::Text("he said \"hi\"".to_owned())),
            Some("he said \"hi\"".to_owned()),
            "nor any quote to double"
        );
    }

    #[test]
    fn a_cell_yank_is_not_ellipsised() {
        let long = "x".repeat(usize::from(crate::config::MaxColumnWidth::default().get()) * 2);

        assert_eq!(
            grid_cell(&CqlValue::Text(long.clone())),
            Some(long),
            "what is drawn is a window, what is yanked is the value"
        );
    }

    #[test]
    fn a_null_cell_yanks_nothing() {
        assert_eq!(
            grid_cell(&CqlValue::Null),
            None,
            "an empty payload would ask the terminal to clear its clipboard"
        );
    }

    #[test]
    fn a_column_yanks_every_row_in_order_and_no_heading() {
        let rows = vec![
            Row {
                values: vec![CqlValue::Int(1), CqlValue::Text("alice".to_owned())],
            },
            Row {
                values: vec![CqlValue::Int(2), CqlValue::Text("bob".to_owned())],
            },
            Row {
                values: vec![CqlValue::Int(3), CqlValue::Text("carol".to_owned())],
            },
        ];

        assert_eq!(
            grid_column(&rows, 1),
            "alice,bob,carol",
            "the values, top to bottom, and nothing in front of them"
        );
        assert_eq!(grid_column(&rows, 0), "1,2,3");
    }

    #[test]
    fn a_null_in_a_column_yanks_as_an_empty_field() {
        let rows = vec![
            Row {
                values: vec![CqlValue::Text("alice".to_owned())],
            },
            Row {
                values: vec![CqlValue::Null],
            },
            Row {
                values: vec![CqlValue::Text("carol".to_owned())],
            },
        ];

        assert_eq!(
            grid_column(&rows, 0),
            "alice,,carol",
            "between delimiters, empty means absent"
        );
    }

    #[test]
    fn a_column_field_is_escaped_the_way_a_row_field_is() {
        let rows = vec![Row {
            values: vec![CqlValue::Text("Smith, John".to_owned())],
        }];

        assert_eq!(
            grid_column(&rows, 0),
            "\"Smith, John\"",
            "here the comma is structural, so the field has to say it is not"
        );
    }

    /// Unreachable through the driver — every row of a `Page` carries one
    /// value per column — but `grid_column` takes a slice and cannot check
    /// that. Dropping the row would shorten the line with nothing to account
    /// for it (design §5).
    #[test]
    fn a_row_too_short_contributes_an_empty_field() {
        let rows = vec![
            Row {
                values: vec![CqlValue::Text("alice".to_owned())],
            },
            Row { values: Vec::new() },
        ];

        assert_eq!(grid_column(&rows, 0), "alice,");
    }

    #[test]
    fn lines_join_without_a_trailing_newline() {
        assert_eq!(
            lines(["a".to_owned(), "b".to_owned()].into_iter()),
            "a\nb",
            "a trailing newline would make a paste into a shell submit itself"
        );
    }

    #[test]
    fn a_block_yanks_one_line_per_row_and_only_the_columns_inside_it() {
        let rows = vec![
            Row {
                values: vec![
                    CqlValue::Int(1),
                    CqlValue::Text("alice".to_owned()),
                    CqlValue::Text("x".to_owned()),
                ],
            },
            Row {
                values: vec![
                    CqlValue::Int(2),
                    CqlValue::Text("bob".to_owned()),
                    CqlValue::Text("y".to_owned()),
                ],
            },
            Row {
                values: vec![
                    CqlValue::Int(3),
                    CqlValue::Text("carol".to_owned()),
                    CqlValue::Text("z".to_owned()),
                ],
            },
        ];

        assert_eq!(
            grid_block(
                &rows,
                &Ranges::of(Range::new(0, 1)),
                &Ranges::of(Range::new(1, 2))
            ),
            "alice,x\nbob,y"
        );
    }

    #[test]
    fn a_block_spanning_every_column_is_the_rows_themselves() {
        let rows = vec![
            Row {
                values: vec![CqlValue::Int(1), CqlValue::Null],
            },
            Row {
                values: vec![CqlValue::Int(2), CqlValue::Text("bob".to_owned())],
            },
        ];

        assert_eq!(
            grid_block(
                &rows,
                &Ranges::of(Range::new(0, 1)),
                &Ranges::of(Range::new(0, 1))
            ),
            "1,\n2,bob"
        );
    }

    #[test]
    fn a_column_range_yanks_one_line_per_column() {
        let rows = vec![
            Row {
                values: vec![CqlValue::Int(1), CqlValue::Text("alice".to_owned())],
            },
            Row {
                values: vec![CqlValue::Int(2), CqlValue::Text("bob".to_owned())],
            },
        ];

        assert_eq!(
            grid_columns(&rows, &Ranges::of(Range::new(0, 1))),
            "1,2\nalice,bob",
            "a column is its values top to bottom, and a range of columns is that once per column"
        );
    }

    #[test]
    fn a_block_ending_past_the_page_yanks_what_is_there() {
        let rows = vec![Row {
            values: vec![CqlValue::Int(1)],
        }];

        assert_eq!(
            grid_block(
                &rows,
                &Ranges::of(Range::new(0, 4)),
                &Ranges::of(Range::new(0, 4))
            ),
            "1"
        );
    }

    #[test]
    fn a_block_of_two_groups_yanks_one_line_per_row_with_no_separator() {
        // The groups leave no trace in the output: a separator would be this
        // program describing its own selection to a spreadsheet that did not
        // ask (multi-group selection design §5).
        let rows = sample_rows(4, 2);
        let mut selected = Ranges::of(Range::new(0, 0));
        selected.insert(Range::new(3, 3));

        assert_eq!(
            grid_block(&rows, &selected, &Ranges::of(Range::new(0, 1))),
            format!("{}\n{}", grid_row(&rows[0]), grid_row(&rows[3]))
        );
    }

    #[test]
    fn a_block_takes_only_the_columns_the_groups_hold() {
        let rows = sample_rows(2, 4);
        let mut columns = Ranges::of(Range::new(0, 0));
        columns.insert(Range::new(3, 3));

        let yanked = grid_block(&rows, &Ranges::of(Range::new(0, 0)), &columns);

        assert_eq!(
            yanked, "r0c0,r0c3",
            "the disjoint columns, not merely two of them"
        );
    }

    #[test]
    fn disjoint_columns_yank_one_line_each_left_to_right() {
        let rows = sample_rows(3, 4);
        let mut columns = Ranges::of(Range::new(0, 0));
        columns.insert(Range::new(2, 2));

        assert_eq!(
            grid_columns(&rows, &columns),
            format!("{}\n{}", grid_column(&rows, 0), grid_column(&rows, 2))
        );
    }
}
