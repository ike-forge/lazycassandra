//! The words CQL reserves, and the ones it does not but a reader still reads
//! as grammar.
//!
//! One flat set rather than a reserved/non-reserved split: the highlighter and
//! the completer both want "is this grammar or a name", and CQL's own split is
//! about what may be used unquoted as an identifier — a question neither of
//! them asks (docs/superpowers/specs/2026-08-16-query-editor-design.md §2).

/// Every keyword, upper case and sorted, so [`is_keyword`] can bisect and
/// completion can walk it in one order on every machine (conventions.md §4).
pub(crate) const KEYWORDS: &[&str] = &[
    "ADD",
    "AGGREGATE",
    "ALL",
    "ALLOW",
    "ALTER",
    "AND",
    "ANY",
    "APPLY",
    "AS",
    "ASC",
    "ASCII",
    "AUTHORIZE",
    "BATCH",
    "BEGIN",
    "BIGINT",
    "BLOB",
    "BOOLEAN",
    "BY",
    "CLUSTERING",
    "COLUMNFAMILY",
    "COMPACT",
    "CONTAINS",
    "COUNTER",
    "CREATE",
    "CUSTOM",
    "DATE",
    "DECIMAL",
    "DELETE",
    "DESC",
    "DESCRIBE",
    "DISTINCT",
    "DOUBLE",
    "DROP",
    "DURATION",
    "ENTRIES",
    "EXISTS",
    "FILTERING",
    "FINALFUNC",
    "FLOAT",
    "FROM",
    "FROZEN",
    "FULL",
    "FUNCTION",
    "GRANT",
    "GROUP",
    "IF",
    "IN",
    "INDEX",
    "INET",
    "INFINITY",
    "INITCOND",
    "INSERT",
    "INT",
    "INTO",
    "IS",
    "JSON",
    "KEY",
    "KEYS",
    "KEYSPACE",
    "LANGUAGE",
    "LIMIT",
    "LIST",
    "LOGIN",
    "MAP",
    "MATERIALIZED",
    "MBEAN",
    "MBEANS",
    "MODIFY",
    "NOLOGIN",
    "NORECURSIVE",
    "NOSUPERUSER",
    "NOT",
    "OF",
    "ON",
    "OPTIONS",
    "OR",
    "ORDER",
    "PARTITION",
    "PASSWORD",
    "PER",
    "PERMISSION",
    "PERMISSIONS",
    "PRIMARY",
    "RENAME",
    "REPLACE",
    "RETURNS",
    "REVOKE",
    "ROLE",
    "ROLES",
    "SCHEMA",
    "SELECT",
    "SET",
    "SFUNC",
    "SMALLINT",
    "STATIC",
    "STORAGE",
    "STYPE",
    "SUPERUSER",
    "TABLE",
    "TEXT",
    "TIME",
    "TIMESTAMP",
    "TIMEUUID",
    "TINYINT",
    "TO",
    "TRUNCATE",
    "TUPLE",
    "TYPE",
    "UNLOGGED",
    "UPDATE",
    "USE",
    "USER",
    "USERS",
    "USING",
    "UUID",
    "VALUES",
    "VARCHAR",
    "VARINT",
    "VIEW",
    "WHERE",
    "WITH",
];

/// The subset of [`KEYWORDS`] that names a column's type, upper case and
/// sorted for the same reason.
///
/// A subset rather than a table of its own: a type is grammar wherever it
/// appears, so the highlighter and [`is_keyword`] must keep reading it as
/// such. What this table adds is the completer's answer to `Expect::Type`,
/// where `INSERT` and `INTO` are noise and `INT` is not
/// (docs/superpowers/specs/2026-08-16-query-editor-design.md §6).
pub(crate) const TYPES: &[&str] = &[
    "ASCII",
    "BIGINT",
    "BLOB",
    "BOOLEAN",
    "COUNTER",
    "DATE",
    "DECIMAL",
    "DOUBLE",
    "DURATION",
    "FLOAT",
    "FROZEN",
    "INET",
    "INT",
    "LIST",
    "MAP",
    "SET",
    "SMALLINT",
    "TEXT",
    "TIME",
    "TIMESTAMP",
    "TIMEUUID",
    "TINYINT",
    "TUPLE",
    "UUID",
    "VARCHAR",
    "VARINT",
];

/// The options a `WITH` clause takes, sorted, in the lower case Cassandra
/// spells them and stores them in `system_schema`.
///
/// Not keywords: `comment` and `caching` are ordinary identifiers everywhere
/// else, and a table may hold a column called either. They are grammar only
/// in the one position `Expect::Property` marks.
pub(crate) const PROPERTIES: &[&str] = &[
    "additional_write_policy",
    "bloom_filter_fp_chance",
    "caching",
    "cdc",
    "comment",
    "compaction",
    "compression",
    "crc_check_chance",
    "default_time_to_live",
    "durable_writes",
    "extensions",
    "gc_grace_seconds",
    "max_index_interval",
    "memtable_flush_period_in_ms",
    "min_index_interval",
    "read_repair",
    "replication",
    "speculative_retry",
];

/// The three words CQL spells as keywords but means as values. They are
/// [`crate::style::Token::Literal`] rather than
/// [`crate::style::Token::Keyword`]: `null` is what is in the column, not what
/// the statement is doing (design §2.1).
pub(crate) const VALUE_WORDS: &[&str] = &["FALSE", "NAN", "NULL", "TRUE"];

/// Whether `word`, in any case, is grammar.
pub(crate) fn is_keyword(word: &str) -> bool {
    lookup(KEYWORDS, word)
}

/// Whether `word`, in any case, is one of the three value spellings.
pub(crate) fn is_value_word(word: &str) -> bool {
    lookup(VALUE_WORDS, word)
}

fn lookup(table: &[&str], word: &str) -> bool {
    let upper = word.to_ascii_uppercase();

    table.binary_search(&upper.as_str()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_sorted_so_a_lookup_can_bisect() {
        for table in [KEYWORDS, VALUE_WORDS, TYPES, PROPERTIES] {
            let mut sorted = table.to_vec();
            sorted.sort_unstable();
            assert_eq!(table, sorted, "the table is walked and bisected in order");
        }
    }

    #[test]
    fn a_keyword_is_recognised_in_any_case() {
        assert!(is_keyword("select"));
        assert!(is_keyword("SELECT"));
        assert!(is_keyword("SeLeCt"));
    }

    #[test]
    fn a_name_is_not_a_keyword() {
        assert!(!is_keyword("orders"));
        assert!(!is_keyword(""));
    }

    #[test]
    fn every_type_is_also_grammar() {
        for word in TYPES {
            assert!(
                is_keyword(word),
                "{word} is offered as a type and must still highlight as one"
            );
        }
    }

    #[test]
    fn a_property_is_a_name_everywhere_but_its_own_clause() {
        for word in PROPERTIES {
            assert!(
                !is_keyword(word),
                "{word} may be a column name, and colouring it as grammar would lie"
            );
        }
    }

    #[test]
    fn the_value_words_are_not_in_the_keyword_table() {
        for word in VALUE_WORDS {
            assert!(
                !is_keyword(word),
                "{word} is a literal, and one word cannot be both"
            );
            assert!(is_value_word(word));
        }
    }
}
