//! One row's primary key, spelled as the `WHERE` clause that addresses it
//! (docs/superpowers/specs/2026-08-31-drop-design.md §3.1).
//!
//! Shared by the two statements that address a row they did not write: `d`'s
//! `DELETE` and `ctrl-s`'s `UPDATE`
//! (docs/superpowers/specs/2026-09-18-cell-editing-design.md §2).

use crate::model::{Row, Table};

/// Every key column, in key order, paired with the CQL literal that addresses
/// it.
///
/// Private field and one fallible constructor, which is the whole point: a
/// key holding a value [`crate::literal`] could not spell cannot be built,
/// so [`crate::model::Statement::delete_row`] is total and no `WHERE` clause
/// is ever written from a guess (drop design §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowKey(Vec<(String, String)>);

impl RowKey {
    /// `row`'s key, or `None` where any key column is missing from the page
    /// or holds a value with no literal.
    ///
    /// `key` is `(column name, declared CQL type)` in key order.
    #[must_use]
    pub fn of(key: &[(&str, &str)], columns: &[String], row: &Row) -> Option<Self> {
        key.iter()
            .map(|(name, cql_type)| {
                let at = columns.iter().position(|column| column == name)?;
                let literal = crate::literal::of(cql_type, row.values.get(at)?)?;

                Some(((*name).to_owned(), literal))
            })
            .collect::<Option<Vec<_>>>()
            .map(Self)
    }

    /// `(column, literal)` in key order.
    #[must_use]
    pub fn pairs(&self) -> &[(String, String)] {
        &self.0
    }
}

/// `table`'s primary key as [`RowKey::of`] takes it: partition columns, then
/// clustering ones, each with its declared CQL type.
///
/// Empty only for a table the schema reports without one, which
/// [`RowKey::of`] would then accept for every row — so both callers refuse an
/// empty key rather than address a row by nothing.
#[must_use]
pub fn columns(table: &Table) -> Vec<(&str, &str)> {
    table
        .primary_key
        .partition
        .iter()
        .map(|column| (column.name.as_str(), column.cql_type.as_str()))
        .chain(table.primary_key.clustering.iter().map(|clustering| {
            (
                clustering.column.name.as_str(),
                clustering.column.cql_type.as_str(),
            )
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CqlValue;

    fn row(values: Vec<CqlValue>) -> Row {
        Row { values }
    }

    #[test]
    fn a_key_is_spelled_in_key_order_whatever_order_the_page_is_in() {
        let key = RowKey::of(
            &[("id", "int"), ("placed", "date")],
            &["placed".to_owned(), "note".to_owned(), "id".to_owned()],
            &row(vec![
                CqlValue::Text("2026-01-01".to_owned()),
                CqlValue::Text("x".to_owned()),
                CqlValue::Int(7),
            ]),
        );

        assert_eq!(
            key.as_ref().map(RowKey::pairs),
            Some(
                [
                    ("id".to_owned(), "7".to_owned()),
                    ("placed".to_owned(), "'2026-01-01'".to_owned()),
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn a_key_column_the_page_lacks_spells_nothing() {
        assert_eq!(
            RowKey::of(
                &[("id", "int")],
                &["note".to_owned()],
                &row(vec![CqlValue::Text("x".to_owned())])
            ),
            None
        );
    }

    #[test]
    fn a_key_column_holding_a_value_with_no_literal_spells_nothing() {
        assert_eq!(
            RowKey::of(
                &[("total", "double")],
                &["total".to_owned()],
                &row(vec![CqlValue::Float(0.5)])
            ),
            None,
            "a re-spelled float is a row this program might not find"
        );
    }
}
