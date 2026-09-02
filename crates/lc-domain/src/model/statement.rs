//! CQL statements.
//!
//! Every action the tool takes is expressible as CQL and is shown before it
//! runs (vision.md §4, principle 3), so a statement is a first-class value
//! that can be previewed, logged, and copied — not a string assembled at the
//! point of execution.

use std::fmt::Write as _;

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

    /// One table, gone. Quoted for [`quoted`]'s reason: a name read back
    /// from the cluster is only safe to interpolate quoted (drop design §3).
    #[must_use]
    pub fn drop_table(keyspace: &str, table: &str) -> Self {
        Self {
            cql: format!("DROP TABLE {}.{}", quoted(keyspace), quoted(table)),
            kind: StatementKind::Destructive,
        }
    }

    /// One keyspace and every table in it.
    #[must_use]
    pub fn drop_keyspace(keyspace: &str) -> Self {
        Self {
            cql: format!("DROP KEYSPACE {}", quoted(keyspace)),
            kind: StatementKind::Destructive,
        }
    }

    /// One row, addressed by its whole primary key.
    ///
    /// `key` is `(column, literal)` in key order — partition columns then
    /// clustering ones — already spelled by [`crate::literal`], which is
    /// what makes this total: a key it could not spell never becomes one of
    /// these (drop design §3.1).
    ///
    /// [`StatementKind::Write`] rather than `Destructive`, and deliberately:
    /// `kind_of` classifies a typed `DELETE` the same way, and the two must
    /// not disagree about one verb. The confirmation this runs behind is
    /// owed to the key that opened it, not to the kind (drop design §2).
    #[must_use]
    pub fn delete_row(keyspace: &str, table: &str, key: &[(String, String)]) -> Self {
        Self {
            cql: format!(
                "DELETE FROM {}.{} WHERE {}",
                quoted(keyspace),
                quoted(table),
                restriction(key)
            ),
            kind: StatementKind::Write,
        }
    }

    /// Several rows, as one batch.
    ///
    /// Logged, which is the expensive one: a confirmation that said "3 rows"
    /// promises three rows, and an unlogged batch that half-applies has told
    /// the user something untrue. The count is bounded by what the grid
    /// holds, so the batchlog write it costs is bounded too.
    ///
    /// A single key produces [`Self::delete_row`] instead — a batch of one
    /// is a batch the user has to read past to see what it does.
    #[must_use]
    pub fn delete_rows(keyspace: &str, table: &str, keys: &[Vec<(String, String)>]) -> Self {
        if let [only] = keys {
            return Self::delete_row(keyspace, table, only);
        }

        let mut cql = "BEGIN BATCH\n".to_owned();
        for key in keys {
            let _ = writeln!(
                cql,
                "  DELETE FROM {}.{} WHERE {};",
                quoted(keyspace),
                quoted(table),
                restriction(key)
            );
        }
        cql.push_str("APPLY BATCH");

        Self {
            cql,
            kind: StatementKind::Write,
        }
    }

    /// A statement typed by hand, classified by the verb that opens it.
    ///
    /// A sniffer rather than a parser, but a scanning one: the verb is read
    /// off [`crate::cql::opening_words`], so a leading comment is skipped and
    /// a `;` inside a string does not split a statement. Every statement in
    /// the buffer is classified and the most restricted kind wins, so a
    /// `SELECT` typed ahead of a `DROP` does not launder it
    /// (docs/superpowers/specs/2026-08-10-query-execution-design.md §4,
    /// posture design §2).
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

/// The `WHERE` clause addressing one row: every key column, in key order,
/// joined with `AND`. The names are quoted for [`quoted`]'s reason; the
/// values arrive already spelled.
fn restriction(key: &[(String, String)]) -> String {
    key.iter()
        .map(|(column, literal)| format!("{} = {literal}", quoted(column)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Every verb that opens a statement changing nothing on the cluster.
///
/// The allowlist is the whole gate: a verb absent from it is mutating, so
/// being wrong here costs an error message and being wrong the other way
/// costs a write the posture was set to prevent (design §4, posture design
/// §2). The list is closed and short by design — CQL grows verbs, and a new
/// one arriving is refused on a read-only connection until someone decides
/// it belongs here.
///
/// `USE` changes the session's keyspace rather than the cluster, and
/// `DESCRIBE`/`LIST` read schema and permissions back.
const READING: &[&str] = &["SELECT", "USE", "DESCRIBE", "DESC", "LIST"];

/// Every verb that removes data or a schema object outright. Gated by posture
/// *and* by typed confirmation (vision.md §4, principle 6).
///
/// `DROP` covers every object it can name — keyspace, table, index, type,
/// view, role, function — because the first word is what the gate reads.
const DESTRUCTIVE: &[&str] = &["DROP", "TRUNCATE"];

/// Every verb that changes schema, cluster metadata or permissions rather
/// than rows: `CREATE`/`ALTER` over any object, and the two that move
/// privileges around.
///
/// `ALTER TABLE … ADD` is here rather than under a fourth kind: adding a
/// column is a schema change whichever clause spells it.
const SCHEMA: &[&str] = &["CREATE", "ALTER", "GRANT", "REVOKE"];

/// The kind a verb opens, with an unrecognised one mutating.
fn kind_of_verb(verb: &str) -> StatementKind {
    if READING.contains(&verb) {
        StatementKind::Read
    } else if DESTRUCTIVE.contains(&verb) {
        StatementKind::Destructive
    } else if SCHEMA.contains(&verb) {
        StatementKind::Schema
    } else {
        // `INSERT`, `UPDATE`, `DELETE`, `BEGIN`/`APPLY` around a batch, and
        // every verb this table has not learned.
        StatementKind::Write
    }
}

/// How far a kind is trusted, least to most restricted. A buffer holding
/// several statements takes the highest of theirs: a `SELECT` typed ahead of
/// a `DROP` must not launder it (posture design §2).
const fn severity(kind: StatementKind) -> u8 {
    match kind {
        StatementKind::Read => 0,
        StatementKind::Write => 1,
        StatementKind::Schema => 2,
        StatementKind::Destructive => 3,
    }
}

/// The kind of a buffer: the most restricted kind among the statements in it.
///
/// A buffer with no statement in it at all classifies as [`StatementKind::Write`]
/// — the same conservative direction an unknown verb takes.
fn kind_of(cql: &str) -> StatementKind {
    crate::cql::opening_words(cql)
        .iter()
        .map(|verb| kind_of_verb(verb))
        .max_by_key(|kind| severity(*kind))
        .unwrap_or(StatementKind::Write)
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
        assert!(Statement::typed("REBUILD INDEX shop.orders").mutates());
        assert!(Statement::typed("").mutates());
    }

    #[test]
    fn a_leading_comment_does_not_hide_the_verb() {
        for cql in [
            "-- what this does\nSELECT * FROM shop.orders",
            "// what this does\nSELECT * FROM shop.orders",
            "/* what this does */ SELECT * FROM shop.orders",
        ] {
            assert_eq!(Statement::typed(cql).kind, StatementKind::Read, "{cql}");
        }
    }

    #[test]
    fn reading_the_schema_and_the_session_is_not_a_mutation() {
        for cql in [
            "USE shop",
            "DESCRIBE TABLES",
            "desc keyspace shop",
            "LIST ROLES",
            "LIST ALL PERMISSIONS",
        ] {
            assert!(
                !Statement::typed(cql).mutates(),
                "{cql} changes nothing and a read-only connection may run it"
            );
        }
    }

    #[test]
    fn every_verb_that_writes_rows_is_a_write() {
        for cql in [
            "INSERT INTO shop.orders (id) VALUES (1)",
            "UPDATE shop.orders SET total = 1 WHERE id = 1",
            "DELETE FROM shop.orders WHERE id = 1",
            "BEGIN BATCH INSERT INTO shop.orders (id) VALUES (1); APPLY BATCH",
        ] {
            assert_eq!(Statement::typed(cql).kind, StatementKind::Write, "{cql}");
        }
    }

    #[test]
    fn every_verb_that_changes_schema_or_permissions_is_a_schema_change() {
        for cql in [
            "CREATE TABLE shop.orders (id int PRIMARY KEY)",
            "CREATE KEYSPACE shop WITH replication = {}",
            "CREATE INDEX ON shop.orders (total)",
            "CREATE MATERIALIZED VIEW shop.v AS SELECT * FROM shop.orders",
            "CREATE ROLE reader",
            "ALTER TABLE shop.orders ADD note text",
            "ALTER KEYSPACE shop WITH durable_writes = true",
            "GRANT SELECT ON KEYSPACE shop TO reader",
            "REVOKE MODIFY ON KEYSPACE shop FROM reader",
        ] {
            assert_eq!(Statement::typed(cql).kind, StatementKind::Schema, "{cql}");
        }
    }

    #[test]
    fn every_verb_that_removes_something_is_destructive() {
        for cql in [
            "DROP TABLE shop.orders",
            "DROP KEYSPACE shop",
            "DROP INDEX shop.orders_total",
            "DROP MATERIALIZED VIEW shop.v",
            "DROP TYPE shop.address",
            "DROP ROLE reader",
            "TRUNCATE shop.orders",
            "TRUNCATE TABLE shop.orders",
        ] {
            assert_eq!(
                Statement::typed(cql).kind,
                StatementKind::Destructive,
                "{cql} must not reach the cluster without confirmation"
            );
        }
    }

    // A read typed ahead of a write is the whole reason the buffer is
    // classified statement by statement rather than by its first word.
    #[test]
    fn a_buffer_takes_the_most_restricted_kind_in_it() {
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders; DROP TABLE shop.orders").kind,
            StatementKind::Destructive
        );
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders; UPDATE shop.orders SET a = 1").kind,
            StatementKind::Write
        );
        assert_eq!(
            Statement::typed("CREATE TABLE t (a int PRIMARY KEY); SELECT * FROM t").kind,
            StatementKind::Schema
        );
    }

    #[test]
    fn a_semicolon_inside_a_string_does_not_open_a_statement() {
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders WHERE note = '; DROP TABLE x'").kind,
            StatementKind::Read
        );
    }

    #[test]
    fn a_trailing_semicolon_adds_no_statement() {
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders;\n").kind,
            StatementKind::Read
        );
    }

    #[test]
    fn the_cql_is_kept_verbatim() {
        let cql = "SELECT *\nFROM shop.orders";
        assert_eq!(Statement::typed(cql).cql, cql);
    }

    #[test]
    fn dropping_quotes_the_names_and_is_destructive() {
        let table = Statement::drop_table("MyKs", "Orders");
        assert_eq!(table.cql, "DROP TABLE \"MyKs\".\"Orders\"");
        assert_eq!(table.kind, StatementKind::Destructive);

        let keyspace = Statement::drop_keyspace("MyKs");
        assert_eq!(keyspace.cql, "DROP KEYSPACE \"MyKs\"");
        assert_eq!(keyspace.kind, StatementKind::Destructive);
    }

    /// The builders and the sniffer must not disagree about what a verb is:
    /// the posture guard reads `kind` whichever of the two produced it.
    #[test]
    fn a_built_statement_classifies_as_its_own_cql_would() {
        for built in [
            Statement::drop_table("shop", "orders"),
            Statement::drop_keyspace("shop"),
            Statement::delete_row("shop", "orders", &key()),
        ] {
            assert_eq!(
                built.kind,
                Statement::typed(&built.cql).kind,
                "{} classifies two ways",
                built.cql
            );
        }
    }

    fn key() -> Vec<(String, String)> {
        vec![("id".to_owned(), "8f14e45f".to_owned())]
    }

    #[test]
    fn deleting_a_row_restricts_every_key_column_and_quotes_the_names() {
        assert_eq!(
            Statement::delete_row(
                "shop",
                "orders",
                &[
                    ("id".to_owned(), "8f14e45f".to_owned()),
                    ("placed".to_owned(), "'2026-01-01'".to_owned()),
                ]
            )
            .cql,
            "DELETE FROM \"shop\".\"orders\" WHERE \"id\" = 8f14e45f AND \"placed\" = '2026-01-01'"
        );
    }

    #[test]
    fn a_delete_is_a_write_rather_than_a_destructive_statement() {
        let stmt = Statement::delete_row("shop", "orders", &key());

        assert_eq!(stmt.kind, StatementKind::Write);
        assert!(stmt.mutates(), "a read-only connection still refuses it");
    }

    #[test]
    fn one_key_is_a_statement_and_several_are_a_batch() {
        assert_eq!(
            Statement::delete_rows("shop", "orders", &[key()]).cql,
            Statement::delete_row("shop", "orders", &key()).cql,
            "a batch of one is a batch the user has to read past"
        );

        let two = vec![key(), vec![("id".to_owned(), "c9f0f895".to_owned())]];
        assert_eq!(
            Statement::delete_rows("shop", "orders", &two).cql,
            "BEGIN BATCH\n  \
             DELETE FROM \"shop\".\"orders\" WHERE \"id\" = 8f14e45f;\n  \
             DELETE FROM \"shop\".\"orders\" WHERE \"id\" = c9f0f895;\n\
             APPLY BATCH"
        );
    }

    #[test]
    fn selecting_everything_quotes_both_halves_and_bounds_nothing() {
        assert_eq!(
            Statement::select_everything("shop", "orders").cql,
            "SELECT * FROM \"shop\".\"orders\""
        );
    }
}
