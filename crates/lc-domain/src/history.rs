//! What each connection has been asked, newest first. Keyed by name, not
//! [`ConnId`], so a reconnect keeps it.
//!
//! [`ConnId`]: crate::model::ConnId

use std::collections::BTreeMap;

use crate::config::{ConnectionName, HistoryLimit};
use crate::cql::fold;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    entries: Vec<String>,
}

impl History {
    /// A case-only variant of the newest replaces it; a blank statement is
    /// ignored. `false` when nothing changed, so an exact repeat costs no
    /// write.
    pub fn record(&mut self, statement: &str, limit: HistoryLimit) -> bool {
        let statement = statement.trim();

        if limit.is_off() || statement.is_empty() {
            return false;
        }

        match self.entries.first_mut() {
            Some(newest) if newest == statement => return false,
            Some(newest) if fold(newest) == fold(statement) => statement.clone_into(newest),
            _ => self.entries.insert(0, statement.to_owned()),
        }

        self.entries
            .truncate(usize::try_from(limit.get()).unwrap_or(usize::MAX));
        true
    }

    #[must_use]
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

/// Ordered, for determinism.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Histories(BTreeMap<ConnectionName, History>);

impl Histories {
    pub fn record(
        &mut self,
        connection: &ConnectionName,
        statement: &str,
        limit: HistoryLimit,
    ) -> bool {
        self.0
            .entry(connection.clone())
            .or_default()
            .record(statement, limit)
    }

    /// Same rules as [`History::record`], so a hand-edited file still yields a
    /// valid list.
    pub fn restore(&mut self, connection: &ConnectionName, stored: &[String], limit: HistoryLimit) {
        let mut history = History::default();

        for statement in stored.iter().rev() {
            history.record(statement, limit);
        }

        self.0.insert(connection.clone(), history);
    }

    #[must_use]
    pub fn of(&self, connection: &ConnectionName) -> &[String] {
        self.0.get(connection).map_or(&[], History::entries)
    }
}

/// One row per statement.
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
    fn a_statement_asked_again_after_another_is_kept_again() {
        let mut history = History::default();
        history.record("SELECT 1;", limit(10));
        history.record("SELECT 2;", limit(10));
        history.record("SELECT 1;", limit(10));

        assert_eq!(history.entries(), ["SELECT 1;", "SELECT 2;", "SELECT 1;"]);
    }

    #[test]
    fn a_repeat_differing_in_keyword_or_name_case_replaces_the_newest() {
        let mut history = History::default();
        history.record("SELECT * FROM Shop.Orders;", limit(10));
        history.record("select * from shop.orders;", limit(10));

        assert_eq!(history.entries(), ["select * from shop.orders;"]);
    }

    #[test]
    fn case_inside_a_literal_or_a_quoted_name_is_a_different_statement() {
        let mut history = History::default();
        history.record("SELECT * FROM t WHERE a = 'X';", limit(10));
        history.record("SELECT * FROM t WHERE a = 'x';", limit(10));
        history.record("SELECT * FROM \"T\";", limit(10));
        history.record("SELECT * FROM \"t\";", limit(10));

        assert_eq!(history.entries().len(), 4);
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
    fn recording_says_whether_anything_changed() {
        let mut history = History::default();

        assert!(history.record("SELECT 1;", limit(10)));
        assert!(
            !history.record("SELECT 1;", limit(10)),
            "already the newest"
        );
        assert!(history.record("SELECT 2;", limit(10)));
        assert!(history.record("SELECT 1;", limit(10)), "not the newest");
        assert!(history.record("select 1;", limit(10)), "case replaced");
        assert!(!history.record("  ", limit(10)));
    }

    #[test]
    fn restoring_keeps_a_stores_order_and_applies_the_limit() {
        let mut histories = Histories::default();
        let stored = ["SELECT 3;", "SELECT 2;", "select 2;", "SELECT 1;"].map(str::to_owned);

        histories.restore(&name("dev"), &stored, limit(2));

        assert_eq!(histories.of(&name("dev")), ["SELECT 3;", "SELECT 2;"]);
    }

    #[test]
    fn a_statement_over_several_lines_is_one_row() {
        assert_eq!(
            one_line("SELECT *\n  FROM shop.orders\n  WHERE id = 1;"),
            "SELECT * FROM shop.orders WHERE id = 1;"
        );
    }
}
