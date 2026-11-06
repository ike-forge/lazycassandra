//! The Trace tab's projection: drawn as the trail is, a summary row over one
//! row per event.

use std::time::Duration;

use crate::style::{Segment, Severity, Token};
use crate::trace::{QueryTrace, TraceEvent, TracePanel, Tracing};
use crate::view::{LogRow, PanelBody};

pub(crate) fn trace_body(panel: &TracePanel) -> PanelBody {
    match panel.tracing() {
        Tracing::Untraced => super::waiting("nothing traced yet: ctrl-t runs the statement traced"),
        Tracing::Waiting { .. } => super::waiting("tracing…"),
        Tracing::Failed { cql, reason } => PanelBody::Log {
            rows: vec![LogRow {
                at: None,
                subject: crate::cql::issued(cql),
                outcome: vec![Segment::styled(
                    format!("✗ {reason}"),
                    Token::Severity(Severity::Danger),
                )],
                severity: Severity::Danger,
            }],
            selected: 0,
            offset: panel.scroll.offset(),
            ranges: crate::selection::Ranges::default(),
            flash: false,
        },
        Tracing::Traced { cql, trace } => PanelBody::Log {
            rows: std::iter::once(summary(cql, trace))
                .chain(trace.events.iter().map(event))
                .collect(),
            selected: panel.cursor.index(),
            offset: panel.scroll.offset(),
            ranges: crate::selection::Ranges::default(),
            flash: false,
        },
    }
}

fn summary(cql: &str, trace: &QueryTrace) -> LogRow {
    let mut facts = Vec::new();
    if let Some(coordinator) = trace.coordinator {
        facts.push(format!("coordinated by {coordinator}"));
    }
    if let Some(duration) = trace.duration {
        facts.push(format!("took {}", elapsed(duration)));
    }
    let nodes = trace.nodes().len();
    facts.push(format!(
        "{nodes} {}, {} {}",
        if nodes == 1 { "node" } else { "nodes" },
        trace.events.len(),
        if trace.events.len() == 1 {
            "event"
        } else {
            "events"
        },
    ));

    LogRow {
        at: None,
        subject: crate::cql::issued(cql),
        outcome: vec![Segment::styled(
            format!("→ {}", facts.join(", ")),
            Token::Severity(Severity::Info),
        )],
        severity: Severity::Info,
    }
}

fn event(event: &TraceEvent) -> LogRow {
    let place: Vec<String> = event
        .source
        .map(|source| source.to_string())
        .into_iter()
        .chain(event.thread.clone())
        .collect();

    LogRow {
        at: event.elapsed.map(|took| format!("+{}", elapsed(took))),
        subject: vec![Segment::plain(event.activity.clone())],
        outcome: vec![Segment::styled(place.join(" · "), Token::Faint)],
        severity: Severity::Info,
    }
}

/// Microseconds are what a trace is read in; fits the trail's stamp column.
fn elapsed(took: Duration) -> String {
    let micros = took.as_micros();
    if micros < 1_000 {
        format!("{micros}µs")
    } else if micros < 1_000_000 {
        format!("{:.1}ms", took.as_secs_f64() * 1_000.0)
    } else {
        format!("{:.2}s", took.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;
    use crate::model::QueryId;

    fn node(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, last))
    }

    fn traced() -> TracePanel {
        let mut panel = TracePanel::default();
        let query = QueryId::next();
        panel.wait(query, "SELECT * FROM ks.t".to_owned());
        panel.answer(
            query,
            Ok(QueryTrace {
                coordinator: Some(node(1)),
                duration: Some(Duration::from_micros(2_345)),
                events: vec![
                    TraceEvent {
                        activity: "Parsing a statement".to_owned(),
                        source: Some(node(1)),
                        elapsed: Some(Duration::from_micros(87)),
                        thread: Some("Native-Transport-Requests-1".to_owned()),
                    },
                    TraceEvent {
                        activity: "Read 1 live rows".to_owned(),
                        source: Some(node(2)),
                        elapsed: Some(Duration::from_micros(1_250)),
                        thread: None,
                    },
                ],
            }),
        );
        panel
    }

    #[test]
    fn a_trace_reads_as_a_summary_over_one_row_per_event() {
        let PanelBody::Log { rows, .. } = trace_body(&traced()) else {
            panic!("a trace draws as the trail does");
        };

        let stamps: Vec<Option<&str>> = rows.iter().map(|row| row.at.as_deref()).collect();
        assert_eq!(stamps, [None, Some("+87µs"), Some("+1.2ms")]);
        assert_eq!(
            rows[0].outcome[0].text,
            "→ coordinated by 10.0.0.1, took 2.3ms, 2 nodes, 2 events"
        );
        assert_eq!(
            rows[1].outcome[0].text,
            "10.0.0.1 · Native-Transport-Requests-1"
        );
        assert_eq!(rows[2].outcome[0].text, "10.0.0.2");
    }

    #[test]
    fn the_body_holds_as_many_rows_as_the_cursor_walks() {
        let panel = traced();
        let PanelBody::Log { rows, .. } = trace_body(&panel) else {
            panic!("a trace draws as the trail does");
        };

        assert_eq!(rows.len(), panel.rows());
    }

    #[test]
    fn a_failed_trace_names_the_statement_and_why() {
        let mut panel = TracePanel::default();
        let query = QueryId::next();
        panel.wait(query, "SELECT * FROM ks.t".to_owned());
        panel.answer(query, Err("timed out".to_owned()));

        let PanelBody::Log { rows, .. } = trace_body(&panel) else {
            panic!("a failure draws as one trail row");
        };
        assert_eq!(rows[0].outcome[0].text, "✗ timed out");
    }

    #[test]
    fn elapsed_time_steps_from_micro_to_milli_to_whole_seconds() {
        assert_eq!(elapsed(Duration::from_micros(999)), "999µs");
        assert_eq!(elapsed(Duration::from_micros(12_340)), "12.3ms");
        assert_eq!(elapsed(Duration::from_millis(3_210)), "3.21s");
    }
}
