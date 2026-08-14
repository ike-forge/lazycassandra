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

    /// Every column of one table, unbounded. The export's whole-table scope
    /// (export design §3); [`Statement::select_all`] is the panel's bounded
    /// read.
    #[must_use]
    pub fn select_everything(keyspace: &str, table: &str) -> Self {
        Self {
            cql: format!("SELECT * FROM {}.{}", quoted(keyspace), quoted(table)),
            kind: StatementKind::Read,
        }
    }

    /// A statement typed by hand, classified by its first keyword.
    ///
    /// A sniffer rather than a parser: a statement opening with a comment
    /// classifies as [`StatementKind::Write`] and is refused on a read-only
    /// connection. That is the conservative direction, and it is where a real
    /// CQL parser lands when one arrives
    /// (docs/superpowers/specs/2026-08-10-query-execution-design.md §4).
    #[must_use]
    pub fn typed(cql: &str) -> Self {
        Self {
            cql: cql.to_owned(),
            kind: kind_of(cql),
        }
    }
}

/// CQL folds an unquoted identifier to lower case, and `system_schema` reports
/// names in their true form — so a name read back from the cluster is only
/// safe to interpolate quoted. An embedded quote is doubled, as CQL requires.
fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// An unrecognised verb is mutating: being wrong that way costs an error
/// message, and being wrong the other way costs a write the posture was set to
/// prevent (design §4).
fn kind_of(cql: &str) -> StatementKind {
    let verb = cql.split_whitespace().next().unwrap_or_default();

    match verb.to_ascii_uppercase().as_str() {
        "SELECT" => StatementKind::Read,
        "DROP" | "TRUNCATE" => StatementKind::Destructive,
        "CREATE" | "ALTER" => StatementKind::Schema,
        _ => StatementKind::Write,
    }
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

    #[test]
    fn selecting_is_a_read() {
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders").kind,
            StatementKind::Read
        );
    }

    #[test]
    fn case_and_leading_space_do_not_change_a_kind() {
        assert_eq!(Statement::typed("  select 1").kind, StatementKind::Read);
        assert_eq!(
            Statement::typed("InSeRt INTO t (a) VALUES (1)").kind,
            StatementKind::Write
        );
    }

    #[test]
    fn dropping_and_truncating_are_destructive() {
        for cql in ["DROP TABLE shop.orders", "truncate shop.orders"] {
            assert_eq!(
                Statement::typed(cql).kind,
                StatementKind::Destructive,
                "{cql} must not reach the cluster without confirmation"
            );
        }
    }

    #[test]
    fn creating_and_altering_are_schema_changes() {
        assert_eq!(
            Statement::typed("CREATE TABLE t (a int PRIMARY KEY)").kind,
            StatementKind::Schema
        );
        assert_eq!(
            Statement::typed("ALTER TABLE t ADD b int").kind,
            StatementKind::Schema
        );
    }

    #[test]
    fn an_unknown_verb_is_treated_as_mutating() {
        // The conservative direction: a verb this table has not learned is
        // refused on a read-only connection rather than waved through it.
        assert!(Statement::typed("LIST ROLES").mutates());
        assert!(Statement::typed("-- a comment first\nSELECT 1").mutates());
    }

    #[test]
    fn the_cql_is_kept_verbatim() {
        let cql = "SELECT *\nFROM shop.orders";
        assert_eq!(Statement::typed(cql).cql, cql);
    }

    #[test]
    fn selecting_everything_quotes_both_halves_and_bounds_nothing() {
        assert_eq!(
            Statement::select_everything("shop", "orders").cql,
            "SELECT * FROM \"shop\".\"orders\""
        );
    }
}
