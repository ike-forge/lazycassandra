//! The Logs tab's projection: one row per entry, two lines each.

use std::time::Duration;

use crate::log::{Entry, Outcome, Subject};
use crate::panel::Panel;
use crate::state::State;
use crate::style::{Segment, Severity, Token};
use crate::view::PanelBody;

/// The sentinel `lc-inbound` animates; public so the renderer never keeps a
/// second copy.
pub const RUNNING: &str = "⋯ running";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRow {
    /// `None` on the dropped-count row.
    pub at: Option<String>,
    pub subject: Vec<Segment>,
    pub outcome: Vec<Segment>,
    pub severity: Severity,
}

pub(crate) fn log_body(state: &State) -> PanelBody {
    let log = &state.log;

    if log.is_empty() {
        return PanelBody::Notice {
            text: "nothing has run yet".to_owned(),
            token: Some(Token::Muted),
        };
    }

    let dropped = (log.dropped() > 0).then(|| LogRow {
        at: None,
        subject: vec![Segment::styled(
            format!("… {} earlier entries dropped", log.dropped()),
            Token::Faint,
        )],
        outcome: Vec::new(),
        severity: Severity::Info,
    });
    let leading = usize::from(dropped.is_some());

    let rows = dropped.into_iter().chain(log.entries().map(row)).collect();
    let flash = super::flash_over(state, Panel::Queries);

    // A dropped-count row is prepended once the ring evicts, so every entry
    // index shifts past it together.
    PanelBody::Log {
        rows,
        selected: log.cursor.index() + leading,
        offset: log.scroll.offset(),
        ranges: flash
            .map_or_else(
                || log.selection.ranges(log.cursor.index()),
                |flash| flash.rows().clone(),
            )
            .shifted(leading),
        flash: flash.is_some(),
    }
}

fn row(entry: &Entry) -> LogRow {
    LogRow {
        at: Some(entry.at().to_string()),
        subject: subject(entry.subject()),
        outcome: outcome(entry.outcome()),
        severity: entry.outcome().severity(),
    }
}

fn subject(subject: &Subject) -> Vec<Segment> {
    match subject {
        Subject::Statement(segments) => segments.clone(),
        Subject::Sentence(text) => vec![Segment::plain(text.clone())],
    }
}

fn outcome(outcome: &Outcome) -> Vec<Segment> {
    let token = Token::Severity(outcome.severity());

    vec![Segment::styled(outcome_text(outcome), token)]
}

/// Shared with [`crate::yank::log_entry`], so the two cannot spell an outcome
/// differently.
pub(crate) fn outcome_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Pending { .. } => RUNNING.to_owned(),
        Outcome::Succeeded { detail, took } => format!("→ {detail}, {}", elapsed(*took)),
        Outcome::Failed { detail } => format!("✗ {detail}"),
        Outcome::Cancelled => "⊘ cancelled".to_owned(),
        Outcome::Rejected { reason } => format!("⊘ {reason}"),
    }
}

/// A trail is read, not measured against.
fn elapsed(took: Duration) -> String {
    if took < Duration::from_secs(1) {
        format!("{}ms", took.as_millis())
    } else {
        format!("{:.1}s", took.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Stamp;
    use crate::feedback::Flash;
    use crate::log::{LOG_CAPACITY, Log, Trace};
    use crate::model::QueryId;
    use crate::queries::QueriesTab;
    use crate::selection::{Range, Ranges};

    fn state_with(log: Log) -> State {
        let mut state = State::default();
        state.log = log;
        state.queries.show(QueriesTab::Logs);
        state
    }

    fn rows_of(state: &State) -> Vec<LogRow> {
        let PanelBody::Log { rows, .. } = log_body(state) else {
            panic!("a log body");
        };
        rows
    }

    #[test]
    fn an_empty_trail_says_so_rather_than_drawing_nothing() {
        let body = log_body(&state_with(Log::default()));

        assert!(matches!(body, PanelBody::Notice { .. }));
    }

    #[test]
    fn an_entry_is_its_time_its_subject_and_its_outcome() {
        let mut log = Log::default();
        log.push(Entry::new(
            Stamp::new(14, 2, 14),
            Subject::Sentence("Connecting".to_owned()),
            Outcome::Succeeded {
                detail: "connected".to_owned(),
                took: Duration::from_millis(38),
            },
        ));

        let rows = rows_of(&state_with(log));

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].at.as_deref(), Some("14:02:14"));
        assert_eq!(crate::style::text(&rows[0].subject), "Connecting");
        assert!(crate::style::text(&rows[0].outcome).contains("38ms"));
        assert_eq!(rows[0].severity, Severity::Info);
    }

    #[test]
    fn a_pending_entry_says_it_is_still_running() {
        let mut log = Log::default();
        log.push(Entry::new(
            Stamp::default(),
            Subject::Sentence("Reading schema".to_owned()),
            Outcome::Pending {
                trace: Trace::Query(QueryId::next()),
            },
        ));

        let rows = rows_of(&state_with(log));

        assert!(crate::style::text(&rows[0].outcome).contains("running"));
    }

    #[test]
    fn an_evicting_trail_opens_with_what_it_dropped() {
        let mut log = Log::default();
        for index in 0..LOG_CAPACITY + 3 {
            log.push(Entry::new(
                Stamp::default(),
                Subject::Sentence(index.to_string()),
                Outcome::Cancelled,
            ));
        }

        let rows = rows_of(&state_with(log));

        assert_eq!(rows.len(), LOG_CAPACITY + 1);
        assert!(rows[0].at.is_none());
        assert!(crate::style::text(&rows[0].subject).contains('3'));
    }

    #[test]
    fn a_duration_past_a_second_is_read_in_seconds() {
        let mut log = Log::default();
        log.push(Entry::new(
            Stamp::default(),
            Subject::Sentence("Exporting".to_owned()),
            Outcome::Succeeded {
                detail: "18420 rows".to_owned(),
                took: Duration::from_millis(1_400),
            },
        ));

        let rows = rows_of(&state_with(log));

        assert!(crate::style::text(&rows[0].outcome).contains("1.4s"));
    }

    #[test]
    fn a_selected_row_accounts_for_the_row_the_drop_count_added() {
        let mut log = Log::default();
        for index in 0..LOG_CAPACITY + 3 {
            log.push(Entry::new(
                Stamp::default(),
                Subject::Sentence(index.to_string()),
                Outcome::Cancelled,
            ));
        }
        log.cursor.go_to(0, log.len());

        let PanelBody::Log { rows, selected, .. } = log_body(&state_with(log)) else {
            panic!("a log body");
        };

        assert_eq!(
            crate::style::text(&rows[selected].subject),
            "3",
            "the cursor names the oldest surviving entry, one row past the \
             dropped-count row"
        );
    }

    #[test]
    fn a_held_range_accounts_for_the_row_the_drop_count_added() {
        let mut log = Log::default();
        for index in 0..LOG_CAPACITY + 3 {
            log.push(Entry::new(
                Stamp::default(),
                Subject::Sentence(index.to_string()),
                Outcome::Cancelled,
            ));
        }
        // Entries 0..=2, the oldest three survivors, hold "3","4","5".
        log.selection.start(0);
        log.selection.stop(2);

        let PanelBody::Log { ranges, .. } = log_body(&state_with(log)) else {
            panic!("a log body");
        };

        assert_eq!(
            ranges,
            Ranges::of(Range::new(1, 3)),
            "the held entries sit one row past the dropped-count row, same as \
             the cursor does"
        );
    }

    #[test]
    fn a_flash_over_the_trail_lights_the_rows_it_was_lit_with() {
        let mut log = Log::default();
        for index in 0..LOG_CAPACITY + 3 {
            log.push(Entry::new(
                Stamp::default(),
                Subject::Sentence(index.to_string()),
                Outcome::Cancelled,
            ));
        }
        let mut state = state_with(log);
        state.flash = Some(Flash::lit(
            Panel::Queries,
            Ranges::of(Range::new(0, 1)),
            Ranges::default(),
        ));

        let PanelBody::Log { flash, ranges, .. } = log_body(&state) else {
            panic!("a log body");
        };

        assert!(flash);
        assert_eq!(
            ranges,
            Ranges::of(Range::new(1, 2)),
            "the frozen span shifts past the dropped-count row same as a live \
             selection does"
        );
    }
}
