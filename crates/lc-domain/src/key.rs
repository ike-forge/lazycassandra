//! One row's primary key as the `WHERE` that addresses it, for `d`'s `DELETE`
//! and `ctrl-s`'s `UPDATE`.

use crate::model::{Row, Table};

/// The one fallible constructor means no `WHERE` is ever written from a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowKey(Vec<(String, String)>);

impl RowKey {
    /// `None` where any key column is missing or unspellable. `key` is `(name,
    /// declared type)` in key order.
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

    #[must_use]
    pub fn pairs(&self) -> &[(String, String)] {
        &self.0
    }
}

/// Empty only for a schema reported without a key, which callers refuse.
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
