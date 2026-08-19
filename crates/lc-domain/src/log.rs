//! The Logs tab's state: a bounded trail of what the session issued and what
//! came back.
//!
//! A module of its own for the reason [`crate::tree`], [`crate::contents`]
//! and [`crate::ddl`] are ones — per-panel state with a vocabulary of its own
//! rather than another field on `State` (architecture.md §11).
//!
//! Full design: docs/superpowers/specs/2026-08-19-query-log-design.md.

use std::collections::VecDeque;
use std::time::Duration;

use crate::clock::Stamp;
use crate::model::{ConnId, JobId, QueryId};
use crate::state::{Cursor, Scroll, Selection};
use crate::style::{Segment, Severity};

/// Entries the trail retains. Past this the oldest is dropped and counted.
///
/// A ring rather than an unbounded `Vec`: a session left open against a
/// scripted workload has no ceiling otherwise, and "the process grows until it
/// is killed" is not a behaviour worth a longer scrollback (design §3.2).
pub const LOG_CAPACITY: usize = 500;

/// The trail, and the cursor reading it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Log {
    entries: VecDeque<Entry>,
    dropped: usize,
    pub cursor: Cursor,
    pub scroll: Scroll,
    pub selection: Selection,
}

/// One thing the session did, and how it turned out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    at: Stamp,
    subject: Subject,
    outcome: Outcome,
}

/// An entry's first row: what was done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// Highlighted once, at push, and normalised to one logical line —
    /// re-lexing every retained statement per frame is the wrong shape, and
    /// what ran is the statement rather than its indentation (design §3).
    Statement(Vec<Segment>),
    Sentence(String),
}

/// An entry's second row: how it turned out.
///
/// [`Outcome::Pending`] carries what will complete it rather than the entry
/// carrying it, which is what makes a pending entry nothing can complete —
/// and a completed entry holding a dangling trace — unrepresentable
/// (design §2).
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
    /// A statement `update` refused before it could become an `Effect` —
    /// today only [`Posture::ReadOnly`](crate::Posture) (architecture.md §4).
    /// Born complete: nothing was issued, so there is nothing to wait for.
    Rejected {
        reason: String,
    },
}

/// What a pending entry is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trace {
    Connect(ConnId),
    Schema(ConnId),
    Query(QueryId),
    Export(JobId),
}

impl Log {
    /// Appends an entry, dropping the oldest if the ring is full.
    pub fn push(&mut self, entry: Entry) {
        if self.entries.len() == LOG_CAPACITY {
            self.entries.pop_front();
            self.dropped += 1;
        }
        self.entries.push_back(entry);
    }

    /// Rewrites the outcome of the entry pending on `trace`.
    ///
    /// A no-op where no entry is pending on it: the entry may have aged out of
    /// the ring, or the trace may already have been answered — a late result
    /// for a cancelled query must not resurrect it (architecture.md §3).
    pub fn complete(&mut self, trace: Trace, outcome: Outcome) {
        let pending = self.entries.iter_mut().rev().find(|entry| {
            matches!(entry.outcome, Outcome::Pending { trace: waiting } if waiting == trace)
        });

        if let Some(entry) = pending {
            entry.outcome = outcome;
        }
    }

    /// The trail, oldest first.
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

    /// Entries the ring has evicted. Drawn as a row of its own rather than
    /// letting the panel present a truncated history as a complete one
    /// (design §6).
    #[must_use]
    pub const fn dropped(&self) -> usize {
        self.dropped
    }

    /// Moves to the next entry, stopping on the last one.
    pub fn down(&mut self) {
        self.cursor.down(self.entries.len());
    }

    /// Moves to the previous entry, stopping on the first one.
    pub const fn up(&mut self) {
        self.cursor.up();
    }

    pub const fn to_start(&mut self) {
        self.cursor.to_start();
    }

    pub fn to_end(&mut self) {
        self.cursor.to_end(self.entries.len());
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
    /// The subject as one string — what a search is matched against and what a
    /// yank puts on the clipboard.
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Statement(segments) => crate::style::text(segments),
            Self::Sentence(text) => text.clone(),
        }
    }
}

impl Outcome {
    /// How the outcome row is drawn.
    ///
    /// Derived rather than stored beside the outcome: a field would make
    /// "succeeded, in red" representable, and the renderer would then need a
    /// precedence rule the domain never stated (conventions.md §1).
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

    // `push` pops from the front and then always pushes back, so the ring's
    // length never shrinks — a full ring stays full, it does not make room.
    // A cursor already inside it therefore cannot be pushed out of bounds by
    // eviction, which is why there is no clamp in `push` to test.
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

    // A result whose entry has aged out of the ring must not resurrect it. The
    // same rule `update` applies to a result for a cancelled QueryId
    // (architecture.md §3).
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

    // The connection and the schema read are two entries on one ConnId, and
    // they resolve separately (query log design §3).
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
}
