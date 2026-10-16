use super::*;

type Route = (&'static str, Box<dyn Fn(&mut State)>);

#[test]
fn switching_panels_clears_the_selection_of_the_panel_left_behind() {
    let routes: Vec<Route> = vec![
        (
            "tab",
            Box::new(|state: &mut State| drop(press(state, Key::Tab))),
        ),
        (
            "shift-tab",
            Box::new(|state: &mut State| drop(press(state, Key::BackTab))),
        ),
        (
            "1",
            Box::new(|state: &mut State| drop(press(state, Key::Char('1')))),
        ),
        (
            "2",
            Box::new(|state: &mut State| drop(press(state, Key::Char('2')))),
        ),
        (
            "3",
            Box::new(|state: &mut State| drop(press(state, Key::Char('3')))),
        ),
        (
            "4",
            Box::new(|state: &mut State| drop(press(state, Key::Char('4')))),
        ),
    ];

    for (name, route) in routes {
        let mut state = tables_state();
        state.focus_on(Panel::Tables);
        state.tables.selection.start(0);
        state.tables.selection.stop(1);

        route(&mut state);

        if state.focus() == Panel::Tables {
            continue;
        }

        assert!(
            !state.tables.selection.is_active(),
            "{name} left Tables holding a selection"
        );
    }
}

#[test]
fn switching_away_from_the_trail_clears_its_selection_by_every_route() {
    let routes: Vec<Route> = vec![
        (
            "tab",
            Box::new(|state: &mut State| drop(press(state, Key::Tab))),
        ),
        (
            "shift-tab",
            Box::new(|state: &mut State| drop(press(state, Key::BackTab))),
        ),
        (
            "1",
            Box::new(|state: &mut State| drop(press(state, Key::Char('1')))),
        ),
        (
            "2",
            Box::new(|state: &mut State| drop(press(state, Key::Char('2')))),
        ),
        (
            "3",
            Box::new(|state: &mut State| drop(press(state, Key::Char('3')))),
        ),
    ];

    for (name, route) in routes {
        let mut state = trail_of(&["SELECT one", "SELECT two"]);
        press(&mut state, Key::Char('v'));

        route(&mut state);

        if state.focus() == Panel::Queries && state.queries.tab() == QueriesTab::Logs {
            continue;
        }

        assert!(
            !state.log.selection.is_active(),
            "{name} left the trail holding a selection"
        );
    }
}

#[test]
fn the_trails_search_does_not_survive_a_switch_to_the_cql_tab() {
    let mut state = trail_of(&["SELECT one", "DELETE two"]);
    search_for(&mut state, "DELETE");
    press(&mut state, Key::Enter);
    assert!(
        state.search(Searched::Queries).is_some(),
        "the search committed"
    );

    press(&mut state, Key::BackTab);

    assert_eq!(
        state.queries.tab(),
        QueriesTab::Cql,
        "landed on the buffer without leaving the panel"
    );
    assert_eq!(
        state.search(Searched::Queries),
        None,
        "a stray search survived onto the Cql tab"
    );
    let bar = crate::view::hints::bar(&state);
    assert!(
        !bar.panel.iter().any(|hint| hint.action == "next match"),
        "the hint bar still offered n while the buffer takes text"
    );
}

#[test]
fn re_focusing_the_focused_panel_keeps_its_selection() {
    let mut state = tables_state();
    state.focus_on(Panel::Tables);
    state.tables.selection.start(0);
    state.tables.selection.stop(1);

    let _ = press(&mut state, Key::Char('1'));

    assert_eq!(
        state.tables.selection.ranges(state.tables.cursor().index()),
        Ranges::of(Range::new(0, 1)),
        "`1` on Tables is not a panel switch"
    );
}

#[test]
fn leaving_contents_clears_both_of_its_axes() {
    let mut state = contents_state();
    state.focus_on(Panel::Contents);
    state.contents.rows.start(0);
    state.contents.rows.stop(1);
    state.contents.columns.start(0);
    state.contents.columns.stop(1);

    state.focus_on(Panel::Tables);

    assert!(!state.contents.rows.is_active(), "the row axis survived");
    assert!(
        !state.contents.columns.is_active(),
        "the column axis survived"
    );
}

#[test]
fn a_click_clears_the_selection_of_the_panel_it_leaves() {
    let mut state = contents_state();
    state.focus_on(Panel::Tables);
    state.tables.selection.start(0);
    state.tables.selection.stop(1);

    click_row(&mut state, Panel::Contents, 0);

    assert!(!state.tables.selection.is_active());
}

#[test]
fn enter_on_a_table_leaves_no_selection_behind_in_tables() {
    let mut state = tables_state();
    state.focus_on(Panel::Tables);
    state.move_tree_cursor(1);
    state.tables.selection.start(1);
    state.tables.selection.stop(1);

    // Row 1 is `orders`: `enter` on a keyspace would move no focus.
    let _ = press(&mut state, Key::Enter);

    assert_eq!(state.focus(), Panel::Contents);
    assert!(!state.tables.selection.is_active());
}

#[test]
fn esc_out_of_queries_clears_what_queries_left_behind() {
    let mut state = tables_state();
    state.focus_on(Panel::Queries);

    let _ = press(&mut state, Key::Escape);

    assert_eq!(state.focus(), Panel::Tables);
}

#[test]
fn v_starts_a_range_at_the_cursor_and_motion_extends_it() {
    let mut state = browsing(&["a", "b", "c", "d"]);
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    assert_eq!(
        state.tables.selection.ranges(state.tables.cursor().index()),
        Ranges::of(Range::new(1, 3))
    );
}

#[test]
fn a_second_v_freezes_the_range_and_the_cursor_walks_out_of_it() {
    let mut state = browsing(&["a", "b", "c", "d"]);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    assert_eq!(state.tables.cursor().index(), 2);
    assert_eq!(
        state.tables.selection.ranges(state.tables.cursor().index()),
        Ranges::of(Range::new(0, 1)),
        "a frozen range does not follow the cursor"
    );
}

#[test]
fn escape_freezes_a_live_range_before_it_clears_one() {
    let mut state = browsing(&["a", "b", "c", "d"]);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Escape);
    assert_eq!(
        state.tables.selection.ranges(state.tables.cursor().index()),
        Ranges::of(Range::new(0, 1)),
        "the first press leaves visual mode and keeps the range"
    );

    press(&mut state, Key::Escape);
    assert!(!state.tables.selection.is_active(), "the second clears it");
}

#[test]
fn escape_prefers_the_selection_to_the_search() {
    let mut state = browsing(&["alpha", "beta"]);
    state.apply_search(Searched::Tables, Search::new("a"));
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Escape);

    press(&mut state, Key::Escape);

    assert!(!state.tables.selection.is_active());
    assert!(
        state.search(Searched::Tables).is_some(),
        "the range came off first; the search is still applied"
    );
}

#[test]
fn v_selects_in_the_focused_panel_only() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.focus_on(Panel::Ddl);

    press(&mut state, Key::Char('v'));

    assert!(!state.tables.selection.is_active());
    assert!(state.ddl.selection.is_extending());
}

#[test]
fn v_types_a_v_in_queries() {
    let mut state = browsing(&["a"]);
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    press(&mut state, Key::Char('v'));

    assert_eq!(state.queries.text(), "v");
}

#[test]
fn v_builds_two_groups_and_esc_takes_both() {
    let mut state = tables_state();
    state.focus_on(Panel::Tables);

    let _ = press(&mut state, Key::Char('v'));
    let _ = press(&mut state, Key::Char('j'));
    let _ = press(&mut state, Key::Char('v'));
    let _ = press(&mut state, Key::Char('j'));
    let _ = press(&mut state, Key::Char('j'));
    let _ = press(&mut state, Key::Char('v'));
    let _ = press(&mut state, Key::Char('v'));

    let selected = state.tables.selection.ranges(state.tables.cursor().index());
    assert_eq!(selected.groups().count(), 2, "the gap kept them apart");
    assert_eq!(selected.indices().collect::<Vec<_>>(), vec![0, 1, 3]);

    let _ = press(&mut state, Key::Escape);

    assert!(
        !state.tables.selection.is_active(),
        "one esc takes every group"
    );
}

#[test]
fn v_on_a_selected_row_drops_that_group_only() {
    let mut state = tables_state();
    state.focus_on(Panel::Tables);
    state.tables.selection.start(0);
    state.tables.selection.stop(1);
    state.tables.selection.start(3);
    state.tables.selection.stop(3);
    state.move_tree_cursor(0);

    let _ = press(&mut state, Key::Char('v'));

    assert_eq!(
        state
            .tables
            .selection
            .ranges(state.tables.cursor().index())
            .indices()
            .collect::<Vec<_>>(),
        vec![3]
    );
}

#[test]
fn a_search_clears_the_range_it_would_renumber() {
    let mut state = browsing(&["alpha", "beta", "gamma"]);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('v'));

    state.apply_search(Searched::Tables, Search::new("a"));

    assert!(!state.tables.selection.is_active());
}

#[test]
fn a_range_over_a_list_that_shrank_is_pulled_back() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::End);
    press(&mut state, Key::Char('v'));
    assert_eq!(
        state.tables.selection.ranges(state.tables.cursor().index()),
        Ranges::of(Range::new(0, 2))
    );

    press(&mut state, Key::Home);
    press(&mut state, Key::Char('h'));

    assert_eq!(
        state.tables.selection.ranges(state.tables.cursor().index()),
        Ranges::of(Range::new(0, 0)),
        "folding the keyspace took the two table rows with it"
    );
}

#[test]
fn escape_leaves_queries_when_nothing_else_claims_it() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.focus_on(Panel::Queries);

    press(&mut state, Key::Escape);

    assert_eq!(state.focus(), Panel::Tables);
}

#[test]
fn m_cycles_the_contents_selection_mode() {
    let mut state = populated(Panel::Contents);

    press(&mut state, Key::Char('m'));
    assert_eq!(state.contents.mode, SelectionMode::Column);

    press(&mut state, Key::Char('m'));
    assert_eq!(state.contents.mode, SelectionMode::Cell);

    press(&mut state, Key::Char('m'));
    assert_eq!(state.contents.mode, SelectionMode::Row);
}

#[test]
fn m_does_nothing_outside_the_grid() {
    for panel in [Panel::Tables, Panel::Ddl] {
        let mut state = populated(panel);
        let before = crate::view::view_model(&state);

        let effects = press(&mut state, Key::Char('m'));

        assert!(
            effects.is_empty() && crate::view::view_model(&state) == before,
            "{panel:?} has no grid for a mode to apply to"
        );
        assert_eq!(state.contents.mode, SelectionMode::Row);
    }
}

#[test]
fn m_types_an_m_in_queries() {
    let mut state = populated(Panel::Queries);
    state.queries.start_editing();
    let before = state.queries.text();

    press(&mut state, Key::Char('m'));

    assert_eq!(state.queries.text(), format!("{before}m"));
    assert_eq!(state.contents.mode, SelectionMode::Row);
}

#[test]
fn v_in_row_mode_extends_rows_and_leaves_the_columns_alone() {
    let mut state = grid_of_named_cells(3, 4);
    state.contents.mode = SelectionMode::Row;

    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    assert_eq!(
        state.contents.rows.ranges(state.contents.cursor.index()),
        Ranges::of(Range::new(0, 1))
    );
    assert!(!state.contents.columns.is_active());
}

#[test]
fn v_in_column_mode_extends_columns_and_leaves_the_rows_alone() {
    let mut state = grid_of_named_cells(3, 4);
    state.contents.mode = SelectionMode::Column;

    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('l'));

    assert_eq!(
        state.contents.columns.ranges(state.contents.column.index()),
        Ranges::of(Range::new(0, 1))
    );
    assert!(!state.contents.rows.is_active());
}

#[test]
fn v_in_cell_mode_extends_a_rectangle() {
    let mut state = grid_of_named_cells(3, 4);
    state.contents.mode = SelectionMode::Cell;
    press(&mut state, Key::Char('l'));

    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('l'));

    assert_eq!(
        state.contents.rows.ranges(state.contents.cursor.index()),
        Ranges::of(Range::new(0, 1)),
        "the row anchor was set before the column moved off zero"
    );
    assert_eq!(
        state.contents.columns.ranges(state.contents.column.index()),
        Ranges::of(Range::new(1, 2)),
        "the column anchor was set after `l`, not swapped with the row's"
    );
}

#[test]
fn m_clears_the_range_it_would_reinterpret() {
    let mut state = grid_of_named_cells(3, 4);
    state.contents.mode = SelectionMode::Cell;
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('v'));

    press(&mut state, Key::Char('m'));

    assert!(!state.contents.rows.is_active());
    assert!(!state.contents.columns.is_active());
    assert_eq!(
        state.contents.mode,
        SelectionMode::Row,
        "the mode still cycles"
    );
}

#[test]
fn escape_unwinds_both_axes_together() {
    let mut state = grid_of_named_cells(3, 4);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Escape);
    assert!(!state.contents.rows.is_extending());
    assert!(state.contents.rows.is_active());

    press(&mut state, Key::Escape);
    assert!(!state.contents.rows.is_active());
    assert!(!state.contents.columns.is_active());
}
