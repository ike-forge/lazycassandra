//! What each connection has been asked, newest first.
//!
//! Held for the session and keyed by connection name rather than [`ConnId`]:
//! an id is minted per connect, so reconnecting to the same cluster would
//! otherwise start from nothing.
//!
//! [`ConnId`]: crate::model::ConnId

use std::collections::BTreeMap;

use crate::config::{ConnectionName, HistoryLimit};

/// One connection's statements, most recent first, each present once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    entries: Vec<String>,
}

impl History {
    /// Remembers `statement` as the newest entry.
    ///
    /// An entry already held moves to the front instead of repeating, and a
    /// blank statement is not an entry. The oldest entries past `limit` fall
    /// off.
    pub fn record(&mut self, statement: &str, limit: HistoryLimit) {
        let statement = statement.trim();

        if statement.is_empty() {
            return;
        }

        self.entries.retain(|held| held != statement);
        self.entries.insert(0, statement.to_owned());
        self.entries
            .truncate(usize::try_from(limit.get()).unwrap_or(usize::MAX));
    }

    #[must_use]
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

/// Every connection's [`History`]. A `BTreeMap` so nothing depends on hash
/// order (conventions.md §4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Histories(BTreeMap<ConnectionName, History>);

impl Histories {
    pub fn record(&mut self, connection: &ConnectionName, statement: &str, limit: HistoryLimit) {
        self.0
            .entry(connection.clone())
            .or_default()
            .record(statement, limit);
    }

    /// `connection`'s entries, newest first; none for one never asked anything.
    #[must_use]
    pub fn of(&self, connection: &ConnectionName) -> &[String] {
        self.0.get(connection).map_or(&[], History::entries)
    }
}

/// An entry as one line: a pick list has a row per statement, and a statement
/// laid out over several lines would take several.
#[must_use]
pub fn one_line(statement: &str) -> String {
    statement.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(rows: u32) -> HistoryLimit {
        HistoryLimit::try_new(rows).expect("in range")
    }

    fn name(name: &str) -> ConnectionName {
        ConnectionName::try_new(name).expect("a valid name")
    }

    #[test]
    fn the_newest_statement_comes_first() {
        let mut history = History::default();
        history.record("SELECT 1;", limit(10));
        history.record("SELECT 2;", limit(10));

        assert_eq!(history.entries(), ["SELECT 2;", "SELECT 1;"]);
    }

    #[test]
    fn a_repeated_statement_moves_to_the_front_instead_of_repeating() {
        let mut history = History::default();
        history.record("SELECT 1;", limit(10));
        history.record("SELECT 2;", limit(10));
        history.record("SELECT 1;", limit(10));

        assert_eq!(history.entries(), ["SELECT 1;", "SELECT 2;"]);
    }

    #[test]
    fn surrounding_whitespace_does_not_make_a_statement_new() {
        let mut history = History::default();
        history.record("SELECT 1;", limit(10));
        history.record("  SELECT 1;\n", limit(10));

        assert_eq!(history.entries(), ["SELECT 1;"]);
    }

    #[test]
    fn a_blank_statement_is_not_an_entry() {
        let mut history = History::default();
        history.record(" \n\t", limit(10));

        assert!(history.entries().is_empty());
    }

    #[test]
    fn the_oldest_entries_fall_off_past_the_limit() {
        let mut history = History::default();

        for n in 1..=4 {
            history.record(&format!("SELECT {n};"), limit(3));
        }

        assert_eq!(history.entries(), ["SELECT 4;", "SELECT 3;", "SELECT 2;"]);
    }

    #[test]
    fn lowering_the_limit_trims_on_the_next_record() {
        let mut history = History::default();

        for n in 1..=4 {
            history.record(&format!("SELECT {n};"), limit(10));
        }
        history.record("SELECT 5;", limit(2));

        assert_eq!(history.entries(), ["SELECT 5;", "SELECT 4;"]);
    }

    #[test]
    fn each_connection_keeps_its_own() {
        let mut histories = Histories::default();
        histories.record(&name("dev"), "SELECT 1;", limit(10));
        histories.record(&name("prod"), "SELECT 2;", limit(10));

        assert_eq!(histories.of(&name("dev")), ["SELECT 1;"]);
        assert_eq!(histories.of(&name("prod")), ["SELECT 2;"]);
        assert!(histories.of(&name("stage")).is_empty());
    }

    #[test]
    fn a_statement_over_several_lines_is_one_row() {
        assert_eq!(
            one_line("SELECT *\n  FROM shop.orders\n  WHERE id = 1;"),
            "SELECT * FROM shop.orders WHERE id = 1;"
        );
    }
}
