//! Words CQL reserves, plus those a reader still reads as grammar. One flat
//! set: CQL's reserved split is about unquoted identifiers, which neither the
//! highlighter nor the completer asks.

/// Upper case and sorted, for bisection and a stable completion order.
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

/// A subset of [`KEYWORDS`], so types stay grammar everywhere; this is what
/// `Expect::Type` offers.
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

/// The words that continue or end a column list, and nothing else. A union
/// across clauses: splitting per clause would need the resolver to carry which
/// clause it is in.
pub(crate) const COLUMN_WORDS: &[&str] = &[
    "ADD",
    "ALLOW",
    "AND",
    "AS",
    "ASC",
    "BY",
    "CLUSTERING",
    "CONTAINS",
    "DESC",
    "DISTINCT",
    "DROP",
    "ENTRIES",
    "EXISTS",
    "FILTERING",
    "FROM",
    "GROUP",
    "IF",
    "IN",
    "IS",
    "JSON",
    "KEY",
    "KEYS",
    "LIMIT",
    "NOT",
    "ORDER",
    "PARTITION",
    "PER",
    "PRIMARY",
    "RENAME",
    "SET",
    "STATIC",
    "TIMESTAMP",
    "TO",
    "USING",
    "VALUES",
    "WHERE",
    "WITH",
];

/// Not keywords: `comment` and `caching` are ordinary identifiers outside
/// `WITH`.
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

/// Keywords that mean values, so drawn as literals.
pub(crate) const VALUE_WORDS: &[&str] = &["FALSE", "NAN", "NULL", "TRUE"];

pub(crate) fn is_keyword(word: &str) -> bool {
    lookup(KEYWORDS, word)
}

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
        for table in [KEYWORDS, COLUMN_WORDS, VALUE_WORDS, TYPES, PROPERTIES] {
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
    fn every_word_a_column_position_takes_is_grammar_everywhere() {
        for word in COLUMN_WORDS {
            assert!(
                is_keyword(word),
                "{word} is offered in a column list and must still highlight as grammar"
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
