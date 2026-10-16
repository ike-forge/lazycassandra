//! The Logs tab's state: a bounded trail of what the session issued and what
//! came back.

use std::collections::VecDeque;
use std::time::Duration;

use crate::clock::Stamp;
use crate::model::{ConnId, JobId, QueryId};
use crate::search::{Direction, Search};
use crate::selection::Selection;
use crate::style::{Segment, Severity};
use crate::viewport::{Cursor, Scroll};

/// A ring: a long scripted session would otherwise grow without bound.
pub const LOG_CAPACITY: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Log {
    entries: VecDeque<Entry>,
    dropped: usize,
    pub cursor: Cursor,
    pub scroll: Scroll,
    pub selection: Selection,
    /// Private: setting it owes [`crate::state::State::apply_search`]'s repair.
    search: Option<Search>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    at: Stamp,
    subject: Subject,
    outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// Highlighted once at push: re-lexing every entry per frame is the wrong
    /// shape.
    Statement(Vec<Segment>),
    Sentence(String),
}

/// [`Outcome::Pending`] carries its trace, so a pending entry nothing can
/// complete is unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pending {
        trace: Trace,
    },
    Succeeded {
        detail: String,
        took: Duration,
    },
    Failed {
        detail: String,
    },
    Cancelled,
    /// Born complete: nothing was issued.
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trace {
    Connect(ConnId),
    Schema(ConnId),
    Query(QueryId),
    Export(JobId),
}

impl Log {
    pub fn push(&mut self, entry: Entry) {
        if self.entries.len() == LOG_CAPACITY {
            self.entries.pop_front();
            self.dropped += 1;
        }
        self.entries.push_back(entry);
    }

    /// A no-op when nothing is pending on it, so a late result cannot resurrect
    /// an entry.
    pub fn complete(&mut self, trace: Trace, outcome: Outcome) {
        let pending = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry.outcome, Outcome::Pending { trace: waiting } if waiting == trace)
        });

        if let Some(entry) = pending {
            entry.outcome = outcome;
        }
    }

    /// A read that skipped an empty page is still one statement.
    pub fn retrace(&mut self, from: Trace, to: Trace) {
        let pending = self.entries.iter_mut().rev().find(
            |entry| matches!(entry.outcome, Outcome::Pending { trace: waiting } if waiting == from),
        );

        if let Some(entry) = pending {
            entry.outcome = Outcome::Pending { trace: to };
        }
    }

    #[must_use]
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &Entry> {
        self.entries.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drawn as a row, so a truncated history never reads as complete.
    #[must_use]
    pub const fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn down(&mut self) {
        self.cursor.down(self.entries.len());
    }

    pub const fn up(&mut self) {
        self.cursor.up();
    }

    pub const fn to_start(&mut self) {
        self.cursor.to_start();
    }

    pub fn to_end(&mut self) {
        self.cursor.to_end(self.entries.len());
    }

    #[must_use]
    pub fn search(&self) -> Option<&Search> {
        self.search.as_ref()
    }

    /// Not public: [`State::apply_search`](crate::state::State::apply_search)
    /// owes the repair.
    pub(crate) fn set_search(&mut self, search: Option<Search>) {
        self.search = search;
    }

    #[must_use]
    pub fn next_match(&self, from: usize, direction: Direction) -> Option<usize> {
        let search = self.search.as_ref()?;
        let len = self.entries.len();
        if len == 0 {
            return None;
        }

        (0..len)
            .scan(from, |at, _| {
                *at = direction.step(*at, len);
                Some(*at)
            })
            .find(|at| search.matches(&self.entries[*at].subject().text()))
    }
}

impl Entry {
    #[must_use]
    pub const fn new(at: Stamp, subject: Subject, outcome: Outcome) -> Self {
        Self {
            at,
            subject,
            outcome,
        }
    }

    #[must_use]
    pub const fn at(&self) -> Stamp {
        self.at
    }

    #[must_use]
    pub const fn subject(&self) -> &Subject {
        &self.subject
    }

    #[must_use]
    pub const fn outcome(&self) -> &Outcome {
        &self.outcome
    }
}

impl Subject {
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Statement(segments) => crate::style::text(segments),
            Self::Sentence(text) => text.clone(),
        }
    }
}

impl Outcome {
    /// Derived, so "succeeded, in red" is unrepresentable.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        match self {
            Self::Pending { .. } | Self::Succeeded { .. } => Severity::Info,
            Self::Failed { .. } => Severity::Danger,
            Self::Cancelled | Self::Rejected { .. } => Severity::Warning,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentence(text: &str) -> Subject {
        Subject::Sentence(text.to_owned())
    }

    fn pending(text: &str, trace: Trace) -> Entry {
        Entry::new(Stamp::default(), sentence(text), Outcome::Pending { trace })
    }

    fn done(text: &str) -> Entry {
        Entry::new(
            Stamp::default(),
            sentence(text),
            Outcome::Succeeded {
                detail: "ok".to_owned(),
                took: Duration::from_millis(1),
            },
        )
    }

    #[test]
    fn a_fresh_log_is_empty_and_has_lost_nothing() {
        let log = Log::default();

        assert!(log.is_empty());
        assert_eq!(log.dropped(), 0);
    }

    #[test]
    fn entries_are_kept_in_the_order_they_were_pushed() {
        let mut log = Log::default();
        log.push(done("first"));
        log.push(done("second"));

        let texts: Vec<_> = log.entries().map(|entry| entry.subject().text()).collect();

        assert_eq!(texts, vec!["first", "second"]);
    }

    #[test]
    fn the_ring_drops_the_oldest_entry_and_counts_what_it_lost() {
        let mut log = Log::default();
        for index in 0..LOG_CAPACITY + 2 {
            log.push(done(&index.to_string()));
        }

        assert_eq!(log.len(), LOG_CAPACITY);
        assert_eq!(log.dropped(), 2);
        assert_eq!(
            log.entries().next().map(|entry| entry.subject().text()),
            Some("2".to_owned()),
            "the two oldest left, and the third is now the first"
        );
    }

    #[test]
    fn the_cursor_stops_on_the_last_entry() {
        let mut log = Log::default();
        log.push(done("first"));
        log.push(done("second"));

        log.down();
        log.down();
        log.down();

        assert_eq!(log.cursor.index(), 1);
    }

    #[test]
    fn the_cursor_stops_on_the_first_entry() {
        let mut log = Log::default();
        log.push(done("first"));
        log.push(done("second"));
        log.down();

        log.up();
        log.up();

        assert_eq!(log.cursor.index(), 0);
    }

    // A full ring stays full, so eviction cannot push the cursor out of bounds.
    #[test]
    fn eviction_leaves_the_cursor_addressable_and_the_ring_at_capacity() {
        let mut log = Log::default();
        for index in 0..LOG_CAPACITY {
            log.push(done(&index.to_string()));
        }
        log.to_end();

        log.push(done("one more"));

        assert!(log.cursor.index() < log.len());
        assert_eq!(log.len(), LOG_CAPACITY);
    }

    #[test]
    fn a_completion_rewrites_the_outcome_of_the_entry_waiting_on_it() {
        let query = QueryId::next();
        let mut log = Log::default();
        log.push(pending("SELECT", Trace::Query(query)));

        log.complete(
            Trace::Query(query),
            Outcome::Succeeded {
                detail: "200 rows".to_owned(),
                took: Duration::from_millis(41),
            },
        );

        assert!(matches!(
            log.entries().next().map(Entry::outcome),
            Some(Outcome::Succeeded { .. })
        ));
    }

    #[test]
    fn a_completion_finds_its_entry_past_later_ones() {
        let query = QueryId::next();
        let mut log = Log::default();
        log.push(pending("SELECT", Trace::Query(query)));
        log.push(done("something else"));
        log.push(done("and another"));

        log.complete(Trace::Query(query), Outcome::Cancelled);

        assert!(matches!(
            log.entries().next().map(Entry::outcome),
            Some(Outcome::Cancelled)
        ));
        assert_eq!(log.len(), 3, "completion rewrites, it does not append");
    }

    #[test]
    fn a_completion_for_an_evicted_entry_does_nothing() {
        let query = QueryId::next();
        let mut log = Log::default();
        log.push(pending("SELECT", Trace::Query(query)));
        for index in 0..LOG_CAPACITY {
            log.push(done(&index.to_string()));
        }

        log.complete(Trace::Query(query), Outcome::Cancelled);

        assert_eq!(log.len(), LOG_CAPACITY);
        assert!(
            log.entries()
                .all(|entry| !matches!(entry.outcome(), Outcome::Cancelled)),
            "nothing was appended and nothing else was rewritten"
        );
    }

    #[test]
    fn a_second_completion_for_the_same_trace_does_nothing() {
        let query = QueryId::next();
        let mut log = Log::default();
        log.push(pending("SELECT", Trace::Query(query)));
        log.complete(Trace::Query(query), Outcome::Cancelled);

        log.complete(
            Trace::Query(query),
            Outcome::Failed {
                detail: "too late".to_owned(),
            },
        );

        assert!(
            matches!(
                log.entries().next().map(Entry::outcome),
                Some(Outcome::Cancelled)
            ),
            "the first answer stands"
        );
    }

    // The connection and the schema read are two entries on one ConnId.
    #[test]
    fn two_pending_entries_on_one_connection_complete_independently() {
        let conn = ConnId::next();
        let mut log = Log::default();
        log.push(pending("Connecting", Trace::Connect(conn)));
        log.push(pending("Reading schema", Trace::Schema(conn)));

        log.complete(
            Trace::Connect(conn),
            Outcome::Succeeded {
                detail: "connected".to_owned(),
                took: Duration::from_millis(38),
            },
        );

        let outcomes: Vec<_> = log.entries().map(Entry::outcome).collect();
        assert!(matches!(outcomes[0], Outcome::Succeeded { .. }));
        assert!(matches!(outcomes[1], Outcome::Pending { .. }));
    }

    #[test]
    fn an_outcome_carries_the_severity_it_should_be_drawn_with() {
        assert_eq!(
            Outcome::Pending {
                trace: Trace::Query(QueryId::next())
            }
            .severity(),
            Severity::Info
        );
        assert_eq!(
            Outcome::Succeeded {
                detail: String::new(),
                took: Duration::ZERO
            }
            .severity(),
            Severity::Info
        );
        assert_eq!(
            Outcome::Failed {
                detail: String::new()
            }
            .severity(),
            Severity::Danger
        );
        assert_eq!(Outcome::Cancelled.severity(), Severity::Warning);
        assert_eq!(
            Outcome::Rejected {
                reason: String::new()
            }
            .severity(),
            Severity::Warning
        );
    }

    #[test]
    fn a_fresh_log_holds_no_search() {
        assert_eq!(Log::default().search(), None);
    }

    #[test]
    fn setting_a_search_is_read_back() {
        let mut log = Log::default();

        log.set_search(Search::new("delete"));

        assert_eq!(log.search().map(Search::as_str), Some("delete"));
    }

    #[test]
    fn the_entry_n_reaches_is_matched_against_its_subject() {
        let mut log = Log::default();
        log.push(done("SELECT one"));
        log.push(done("DELETE two"));
        log.push(done("SELECT three"));
        log.set_search(Search::new("delete"));

        let hit = log.next_match(0, Direction::Forwards);

        assert_eq!(hit, Some(1));
    }

    #[test]
    fn n_wraps_past_the_last_entry_back_to_the_first_match() {
        let mut log = Log::default();
        log.push(done("SELECT one"));
        log.push(done("DELETE two"));
        log.set_search(Search::new("delete"));

        assert_eq!(
            log.next_match(1, Direction::Forwards),
            Some(1),
            "the only match wraps to itself"
        );
    }

    #[test]
    fn no_search_finds_no_match() {
        let mut log = Log::default();
        log.push(done("SELECT one"));

        assert_eq!(log.next_match(0, Direction::Forwards), None);
    }

    #[test]
    fn a_search_over_an_empty_trail_finds_nothing() {
        let mut log = Log::default();
        log.set_search(Search::new("delete"));

        assert_eq!(log.next_match(0, Direction::Forwards), None);
    }
}
