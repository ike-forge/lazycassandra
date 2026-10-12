use super::*;
use crate::config::{HistoryConfig, HistoryLimit};
use crate::view::hints::bar;

/// `ctrl-g`, Queries' history chord (docs/shortcuts.md §1).
fn open_history(state: &mut State) -> Vec<Effect> {
    ctrl(state, Key::Char('g'))
}

/// A frame the history fits in.
fn roomy() -> Geometry {
    Geometry::new([8, 14, 20, 1], 37, 14).with_history_rows(10)
}

/// A connection that has run `statements` from the buffer, oldest first, and
/// is back on the Queries panel with an empty buffer.
fn having_run(statements: &[&str]) -> State {
    let mut state = editing("");
    state.geometry = roomy();

    for statement in statements {
        state.focus_on(Panel::Queries);
        state.queries.replace(statement);
        run_query(&mut state);
    }

    state.focus_on(Panel::Queries);
    state.queries.replace("");
    state
}

fn run_query(state: &mut State) -> Vec<Effect> {
    ctrl(state, Key::Char('r'))
}

fn buffer(state: &State) -> String {
    state.queries.text()
}

#[test]
fn a_statement_run_from_the_buffer_is_remembered() {
    let state = having_run(&["SELECT 1;", "SELECT 2;"]);

    assert_eq!(state.history(), ["SELECT 2;", "SELECT 1;"]);
}

#[test]
fn a_table_read_from_the_tree_is_not_remembered() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Enter);

    assert!(state.history().is_empty());
}

#[test]
fn the_configured_limit_bounds_what_is_kept() {
    let mut state = editing("");
    state.config.history = HistoryConfig {
        limit: HistoryLimit::try_new(2).expect("in range"),
    };

    for statement in ["SELECT 1;", "SELECT 2;", "SELECT 3;"] {
        state.focus_on(Panel::Queries);
        state.queries.replace(statement);
        run_query(&mut state);
    }

    assert_eq!(state.history(), ["SELECT 3;", "SELECT 2;"]);
}

fn saves(effects: &[Effect]) -> Vec<&Vec<String>> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::SaveHistory { entries, .. } => Some(entries),
            _ => None,
        })
        .collect()
}

#[test]
fn a_run_saves_the_whole_list_for_its_connection() {
    let mut state = having_run(&["SELECT 1;"]);
    state.focus_on(Panel::Queries);
    state.queries.replace("SELECT 2;");

    let effects = run_query(&mut state);

    assert_eq!(
        saves(&effects),
        [&vec!["SELECT 2;".to_owned(), "SELECT 1;".to_owned()]]
    );
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::SaveHistory { connection, .. } if connection.as_str() == "local"
    )));
}

#[test]
fn running_the_newest_statement_again_saves_nothing() {
    let mut state = having_run(&["SELECT 1;"]);
    state.focus_on(Panel::Queries);
    state.queries.replace("SELECT 1;");

    let effects = run_query(&mut state);

    assert!(saves(&effects).is_empty());
}

#[test]
fn a_statement_the_posture_refused_is_neither_remembered_nor_saved() {
    let mut state = having_run(&[]);
    state
        .queries
        .replace("DELETE FROM shop.orders WHERE id = 7;");

    let effects = run_query(&mut state);

    assert!(saves(&effects).is_empty());
    assert!(state.history().is_empty());
}

#[test]
fn a_failed_save_lands_in_the_trail_and_not_in_a_modal() {
    let mut state = having_run(&[]);
    let before = state.log.len();

    update(
        &mut state,
        Msg::HistorySaveFailed {
            reason: "/x/history.toml: permission denied".to_owned(),
        },
    );

    assert_eq!(state.log.len(), before + 1);
    assert!(state.overlay.is_none());
}

#[test]
fn ctrl_g_opens_the_history_on_the_newest_entry() {
    let mut state = having_run(&["SELECT 1;", "SELECT 2;"]);

    open_history(&mut state);

    let Some(Overlay::History { cursor, .. }) = &state.overlay else {
        panic!("the history is open");
    };
    assert_eq!(cursor.index(), 0);
}

#[test]
fn ctrl_g_opens_nothing_with_no_history() {
    let mut state = having_run(&[]);

    open_history(&mut state);

    assert!(state.overlay.is_none());
}

#[test]
fn ctrl_g_opens_nothing_where_the_frame_cannot_draw_it() {
    let mut state = having_run(&["SELECT 1;"]);
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);

    open_history(&mut state);

    assert!(state.overlay.is_none());
}

#[test]
fn ctrl_g_belongs_to_queries() {
    let mut state = having_run(&["SELECT 1;"]);
    state.focus_on(Panel::Tables);

    open_history(&mut state);

    assert!(state.overlay.is_none());
}

#[test]
fn enter_loads_the_entry_into_the_buffer_and_runs_nothing() {
    let mut state = having_run(&["SELECT 1;", "SELECT 2;"]);
    open_history(&mut state);

    let effects = press(&mut state, Key::Enter);

    assert!(effects.is_empty(), "loading is not running");
    assert!(state.overlay.is_none());
    assert_eq!(buffer(&state), "SELECT 2;");
}

#[test]
fn the_cursor_walks_back_through_older_entries() {
    let mut state = having_run(&["SELECT 1;", "SELECT 2;", "SELECT 3;"]);
    open_history(&mut state);
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Enter);

    assert_eq!(buffer(&state), "SELECT 1;");
}

#[test]
fn g_and_capital_g_reach_the_ends() {
    let mut state = having_run(&["SELECT 1;", "SELECT 2;", "SELECT 3;"]);
    open_history(&mut state);

    press(&mut state, Key::Char('G'));
    press(&mut state, Key::Enter);

    assert_eq!(buffer(&state), "SELECT 1;");
}

#[test]
fn a_loaded_entry_can_be_run_straight_away() {
    let mut state = having_run(&["SELECT 1;"]);
    open_history(&mut state);
    press(&mut state, Key::Enter);
    state.focus_on(Panel::Queries);

    let effects = run_query(&mut state);

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ExecutePaged { .. }))
    );
}

#[test]
fn esc_closes_it_and_leaves_the_buffer_alone() {
    let mut state = having_run(&["SELECT 1;"]);
    state.queries.replace("SELECT half");
    open_history(&mut state);

    press(&mut state, Key::Escape);

    assert!(state.overlay.is_none());
    assert_eq!(buffer(&state), "SELECT half");
}

#[test]
fn a_multi_line_entry_is_loaded_whole() {
    let mut state = having_run(&["SELECT *\nFROM shop.orders;"]);
    open_history(&mut state);

    press(&mut state, Key::Enter);

    assert_eq!(buffer(&state), "SELECT *\nFROM shop.orders;");
}

#[test]
fn a_filter_narrows_the_list_and_enter_closes_it_onto_the_row_before_loading() {
    let mut state = having_run(&["SELECT 1 FROM users;", "SELECT 2 FROM orders;"]);
    open_history(&mut state);

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "users");
    press(&mut state, Key::Enter);

    assert!(
        matches!(state.overlay, Some(Overlay::History { sift: None, .. })),
        "the first ⏎ closes the prompt and loads nothing"
    );
    assert_eq!(buffer(&state), "");

    press(&mut state, Key::Enter);

    assert_eq!(buffer(&state), "SELECT 1 FROM users;");
}

#[test]
fn a_filter_that_matches_nothing_closes_onto_the_row_the_prompt_opened_on() {
    let mut state = having_run(&["SELECT 1;", "SELECT 2;"]);
    open_history(&mut state);
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('/'));
    type_into(&mut state, "zzz");

    press(&mut state, Key::Enter);

    let Some(Overlay::History { cursor, sift }) = &state.overlay else {
        panic!("the history stays open");
    };
    assert!(sift.is_none());
    assert_eq!(cursor.index(), 1, "not a row the filter had hidden");
    assert_eq!(buffer(&state), "");
}

#[test]
fn esc_in_a_filter_puts_the_cursor_back() {
    let mut state = having_run(&["SELECT 1 FROM users;", "SELECT 2 FROM orders;"]);
    open_history(&mut state);
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('/'));
    type_into(&mut state, "orders");

    press(&mut state, Key::Escape);

    let Some(Overlay::History { cursor, sift }) = &state.overlay else {
        panic!("the history stays open");
    };
    assert!(sift.is_none());
    assert_eq!(cursor.index(), 1);
}

#[test]
fn ctrl_g_does_nothing_while_the_history_is_open() {
    let mut state = having_run(&["SELECT 1;"]);
    open_history(&mut state);
    press(&mut state, Key::Char('j'));

    open_history(&mut state);

    assert!(matches!(state.overlay, Some(Overlay::History { .. })));
}

#[test]
fn the_bar_names_ctrl_g_only_once_there_is_something_to_open() {
    let empty = having_run(&[]);
    let kept = having_run(&["SELECT 1;"]);

    let named = |state: &State| bar(state).panel.iter().any(|hint| hint.action == "history");

    assert!(!named(&empty));
    assert!(named(&kept));
}

fn disabled() -> State {
    let mut state = having_run(&[]);
    state.config.history = HistoryConfig {
        limit: HistoryLimit::try_new(0).expect("in range"),
    };
    state
}

#[test]
fn a_zero_limit_records_nothing_and_saves_nothing() {
    let mut state = disabled();
    state.queries.replace("SELECT 1;");

    let effects = run_query(&mut state);

    assert!(state.history().is_empty());
    assert!(saves(&effects).is_empty());
}

#[test]
fn ctrl_g_with_the_history_disabled_toasts_instead_of_opening() {
    let mut state = disabled();

    open_history(&mut state);

    assert!(state.overlay.is_none());
    assert_eq!(
        state.toast.as_ref().map(crate::state::Toast::message),
        Some("history is disabled")
    );
}

#[test]
fn the_toast_leaves_after_its_ticks() {
    let mut state = disabled();
    open_history(&mut state);

    for _ in 0..crate::state::TOAST_TICKS {
        assert!(state.toast.is_some());
        update(&mut state, Msg::Tick(crate::clock::Stamp::default()));
    }

    assert!(state.toast.is_none());
}

#[test]
fn ctrl_g_outside_queries_toasts_nothing() {
    let mut state = disabled();
    state.focus_on(Panel::Tables);

    open_history(&mut state);

    assert!(state.toast.is_none());
}
