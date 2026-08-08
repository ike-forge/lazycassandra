//! CQL statements.
//!
//! Every action the tool takes is expressible as CQL and is shown before it
//! runs (vision.md §4, principle 3), so a statement is a first-class value
//! that can be previewed, logged, and copied — not a string assembled at the
//! point of execution.

/// A statement plus what it is allowed to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub cql: String,
    pub kind: StatementKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    Read,
    Write,
    /// `DROP`, `TRUNCATE`, and friends. Gated by posture *and* by typed
    /// confirmation (vision.md §4, principle 6).
    Destructive,
    Schema,
}

impl StatementKind {
    /// Whether executing this changes anything on the cluster.
    #[must_use]
    pub const fn mutates(self) -> bool {
        matches!(self, Self::Write | Self::Destructive | Self::Schema)
    }

    #[must_use]
    pub const fn is_destructive(self) -> bool {
        matches!(self, Self::Destructive)
    }
}

impl Statement {
    #[must_use]
    pub const fn mutates(&self) -> bool {
        self.kind.mutates()
    }

    /// Every column of one table, bounded by `limit`.
    ///
    /// The bound is in the CQL rather than a page size on the request, so what
    /// reaches the screen is exactly what the statement on display asked for
    /// (vision.md §4, principle 3).
    #[must_use]
    pub fn select_all(keyspace: &str, table: &str, limit: usize) -> Self {
        Self {
            cql: format!(
                "SELECT * FROM {}.{} LIMIT {limit}",
                quoted(keyspace),
                quoted(table)
            ),
            kind: StatementKind::Read,
        }
    }
}

/// CQL folds an unquoted identifier to lower case, and `system_schema` reports
/// names in their true form — so a name read back from the cluster is only
/// safe to interpolate quoted. An embedded quote is doubled, as CQL requires.
fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_a_table_quotes_both_halves_of_its_name() {
        assert_eq!(
            Statement::select_all("shop", "orders_by_customer", 200).cql,
            "SELECT * FROM \"shop\".\"orders_by_customer\" LIMIT 200"
        );
    }

    #[test]
    fn a_name_that_is_not_all_lower_case_survives_the_round_trip() {
        // Unquoted, CQL would fold this to `myks` and the statement would name
        // a keyspace that does not exist.
        assert_eq!(
            Statement::select_all("MyKs", "Orders", 10).cql,
            "SELECT * FROM \"MyKs\".\"Orders\" LIMIT 10"
        );
    }

    #[test]
    fn a_quote_inside_a_name_is_doubled_rather_than_ending_the_identifier() {
        assert_eq!(quoted("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn selecting_is_a_read_and_so_survives_a_read_only_connection() {
        assert!(!Statement::select_all("shop", "orders", 200).mutates());
    }
}
