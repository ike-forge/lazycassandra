use super::*;
use crate::trace::{QueryTrace, TraceEvent, Tracing};

fn trace_query(state: &mut State) -> Vec<Effect> {
    ctrl(state, Key::Char('t'))
}

fn waiting_on(state: &State) -> QueryId {
    state
        .trace
        .waiting()
        .expect("the Trace tab is waiting on the statement it just ran")
}

fn a_trace() -> QueryTrace {
    QueryTrace {
        coordinator: None,
        duration: Some(Duration::from_micros(900)),
        events: vec![TraceEvent {
            activity: "Parsing a statement".to_owned(),
            source: None,
            elapsed: Some(Duration::from_micros(30)),
            thread: None,
        }],
    }
}

fn traced(state: &mut State) -> QueryId {
    trace_query(state);
    let query = waiting_on(state);
    update(
        state,
        Msg::PageReady {
            query,
            page: Box::new(page_of(2)),
            took: Duration::ZERO,
        },
    );
    query
}

#[test]
fn ctrl_t_runs_the_banded_statement_traced() {
    let mut state = editing("SELECT * FROM shop.orders");

    let effects = trace_query(&mut state);

    let query = waiting_on(&state);
    assert_eq!(
        state.contents.outstanding(),
        Some(query),
        "the page still lands in Contents"
    );
    assert_eq!(
        effects.first(),
        Some(&Effect::ExecuteTraced {
            conn: current_conn(&state),
            query,
            stmt: Box::new(Statement::typed("SELECT * FROM shop.orders")),
            page_size: crate::config::PageSize::try_new(200).expect("in range"),
        })
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::SaveHistory { .. })),
        "a traced statement is still one the user typed"
    );
}

#[test]
fn ctrl_t_keeps_focus_on_queries_and_shows_the_trace_tab() {
    let mut state = editing("SELECT * FROM shop.orders");

    trace_query(&mut state);

    assert_eq!(state.focus(), Panel::Queries);
    assert_eq!(state.queries.tab(), QueriesTab::Trace);
    assert!(!state.takes_text(), "the buffer closed with the tab");
}

#[test]
fn ctrl_t_outside_queries_still_opens_the_theme_picker() {
    let mut state = measured(&["shop"]);

    let effects = trace_query(&mut state);

    assert!(effects.is_empty());
    assert!(matches!(state.overlay, Some(Overlay::Themes { .. })));
}

#[test]
fn a_refused_statement_traces_nothing_and_leaves_the_tab_alone() {
    let mut state = editing("DELETE FROM shop.orders WHERE id = 7");

    let effects = trace_query(&mut state);

    assert!(effects.is_empty(), "a read-only connection issues nothing");
    assert_eq!(state.trace.tracing(), &Tracing::Untraced);
    assert_eq!(state.queries.tab(), QueriesTab::Cql);
}

#[test]
fn the_trace_lands_after_its_page() {
    let mut state = editing("SELECT * FROM shop.orders");
    let query = traced(&mut state);
    assert_eq!(state.contents.row_count(), 2);
    assert_eq!(
        state.trace.waiting(),
        Some(query),
        "the page is not the trace"
    );

    update(
        &mut state,
        Msg::TraceReady {
            query,
            trace: Ok(a_trace()),
        },
    );

    assert_eq!(
        state.trace.tracing(),
        &Tracing::Traced {
            cql: "SELECT * FROM shop.orders".to_owned(),
            trace: a_trace(),
        }
    );
}

#[test]
fn a_trace_the_tab_stopped_waiting_on_is_dropped() {
    let mut state = editing("SELECT * FROM shop.orders");
    let first = traced(&mut state);
    state.focus_on(Panel::Queries);
    state.queries.start_editing();
    let second = traced(&mut state);

    update(
        &mut state,
        Msg::TraceReady {
            query: first,
            trace: Ok(a_trace()),
        },
    );

    assert_eq!(state.trace.waiting(), Some(second));
}

#[test]
fn a_failed_request_fails_its_trace() {
    let mut state = editing("SELECT * FROM shop.orders");
    trace_query(&mut state);
    let query = waiting_on(&state);

    update(
        &mut state,
        Msg::Failed {
            query,
            error: DbError::Invalid("no such table".to_owned()),
            took: Duration::ZERO,
        },
    );

    assert!(
        matches!(state.trace.tracing(), Tracing::Failed { reason, .. } if reason.contains("no such table")),
        "{:?}",
        state.trace.tracing()
    );
}

#[test]
fn ctrl_c_cancels_a_trace_still_being_read_after_its_page_landed() {
    let mut state = editing("SELECT * FROM shop.orders");
    let query = traced(&mut state);
    assert!(state.in_flight.is_empty());

    let effects = ctrl(&mut state, Key::Char('c'));

    assert_eq!(effects, vec![Effect::Cancel(query)]);
    assert!(matches!(state.trace.tracing(), Tracing::Failed { .. }));
    assert_eq!(state.contents.row_count(), 2, "the page stays");
}

#[test]
fn six_focuses_the_trace_tab() {
    let mut state = browsing(&["shop"]);

    press(&mut state, Key::Char('6'));

    assert_eq!(state.focus(), Panel::Queries);
    assert_eq!(state.queries.tab(), QueriesTab::Trace);
}

#[test]
fn a_trace_landing_elsewhere_says_where_to_read_it() {
    let mut state = editing("SELECT * FROM shop.orders");
    let query = traced(&mut state);
    state.focus_on(Panel::Contents);

    update(
        &mut state,
        Msg::TraceReady {
            query,
            trace: Ok(a_trace()),
        },
    );

    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "trace ready, 6 shows it"))
    );
}

#[test]
fn search_is_inert_over_the_trace_tab() {
    let mut state = editing("SELECT * FROM shop.orders");
    traced(&mut state);

    press(&mut state, Key::Char('/'));

    assert!(
        searching(&state).is_none(),
        "Queries' search is the trail's"
    );
}

#[test]
fn j_walks_the_trace_rows() {
    let mut state = editing("SELECT * FROM shop.orders");
    let query = traced(&mut state);
    update(
        &mut state,
        Msg::TraceReady {
            query,
            trace: Ok(a_trace()),
        },
    );

    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    assert_eq!(
        state.trace.cursor.index(),
        1,
        "the summary and one event, and no further"
    );
}

#[test]
fn neither_run_chord_fires_under_a_live_logs_prompt() {
    for chord in ['r', 't'] {
        let mut state = editing("SELECT * FROM shop.orders");
        press(&mut state, Key::Escape);
        press(&mut state, Key::Char(']'));
        press(&mut state, Key::Char('/'));

        let effects = ctrl(&mut state, Key::Char(chord));

        assert!(effects.is_empty(), "ctrl-{chord} ran under the prompt");
        assert!(
            searching(&state).is_some(),
            "ctrl-{chord} stranded the prompt"
        );
        assert_eq!(state.queries.tab(), QueriesTab::Logs);
    }
}
