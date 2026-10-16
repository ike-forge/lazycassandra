use super::*;

/// The first column is out of order, so a sort has something to do.
fn columnar_grid(column: usize) -> State {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: vec!["name".to_owned(), "city".to_owned()],
            rows: ["Bob", "Ada", "Cleo"]
                .into_iter()
                .zip(["Berlin", "Aarhus", "Berlin"])
                .map(|(name, city)| Row {
                    values: vec![
                        CqlValue::Text(name.to_owned()),
                        CqlValue::Text(city.to_owned()),
                    ],
                })
                .collect(),
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state.focus_on(Panel::Contents);
    state.contents.mode = SelectionMode::Column;
    state.contents.column.go_to(column, 2);
    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
    );
    state
}

fn first_names(state: &State) -> Vec<String> {
    state
        .contents
        .loaded()
        .expect("a page is loaded")
        .rows()
        .map(|row| crate::cell::text(&row.values[0]))
        .collect()
}

fn filter_prompt(state: &State) -> &Filter {
    match state.overlay.as_ref() {
        Some(Overlay::Filter(filter)) => filter,
        _ => panic!("the filter prompt is up"),
    }
}

#[test]
fn s_sorts_the_column_the_cursor_is_on() {
    let mut state = columnar_grid(0);

    press(&mut state, Key::Char('s'));

    assert_eq!(first_names(&state), ["Cleo", "Bob", "Ada"]);
}

#[test]
fn s_is_a_free_letter_outside_column_mode() {
    for mode in [SelectionMode::Row, SelectionMode::Cell] {
        let mut state = columnar_grid(0);
        state.contents.mode = mode;
        let before = crate::view::view_model(&state);

        let effects = press(&mut state, Key::Char('s'));

        assert!(
            effects.is_empty() && crate::view::view_model(&state) == before,
            "{mode:?} has no column for a column key to address"
        );
    }
}

#[test]
fn f_opens_a_prompt_over_the_column_the_cursor_is_on() {
    let mut state = columnar_grid(1);

    press(&mut state, Key::Char('f'));

    assert_eq!(filter_prompt(&state).column(), 1);
    assert_eq!(filter_prompt(&state).name(), "city");
    assert_eq!(filter_prompt(&state).query(), "");
}

#[test]
fn a_filter_is_applied_when_the_prompt_closes_and_not_before() {
    let mut state = columnar_grid(1);
    press(&mut state, Key::Char('f'));

    type_into(&mut state, "berlin");
    assert_eq!(
        first_names(&state),
        ["Bob", "Ada", "Cleo"],
        "a hard filter must not rebuild the grid under the hands typing it"
    );

    press(&mut state, Key::Enter);

    assert!(state.overlay.is_none());
    assert_eq!(first_names(&state), ["Bob", "Cleo"]);
}

#[test]
fn escape_leaves_the_column_filtered_exactly_as_it_was() {
    let mut state = columnar_grid(1);
    state.contents.set_filter(1, Search::new("aarhus"));
    press(&mut state, Key::Char('f'));

    type_into(&mut state, "x");
    press(&mut state, Key::Escape);

    assert!(state.overlay.is_none());
    assert_eq!(first_names(&state), ["Ada"], "the standing filter stands");
}

#[test]
fn the_prompt_opens_on_the_filter_already_standing_so_deleting_it_clears() {
    let mut state = columnar_grid(1);
    state.contents.set_filter(1, Search::new("berlin"));

    press(&mut state, Key::Char('f'));
    assert_eq!(filter_prompt(&state).query(), "berlin");

    for _ in 0..6 {
        press(&mut state, Key::Backspace);
    }
    press(&mut state, Key::Enter);

    assert_eq!(first_names(&state), ["Bob", "Ada", "Cleo"]);
    assert_eq!(state.contents.filter(1), None);
}

#[test]
fn c_clears_the_filter_on_the_column_the_cursor_is_on() {
    let mut state = columnar_grid(1);
    state.contents.set_filter(0, Search::new("a"));
    state.contents.set_filter(1, Search::new("berlin"));

    press(&mut state, Key::Char('c'));

    assert_eq!(state.contents.filter(1), None);
    assert!(
        state.contents.filter(0).is_some(),
        "and only that column's — `c` names the cursor's column and no other"
    );
}

#[test]
fn the_prompt_claims_every_printable_key_the_panel_would_have() {
    let mut state = columnar_grid(0);
    press(&mut state, Key::Char('f'));

    let effects = press(&mut state, Key::Char('q'));

    assert!(effects.is_empty(), "`q` types itself rather than quitting");
    assert!(!state.should_quit);
    assert_eq!(filter_prompt(&state).query(), "q");
}

#[test]
fn the_ctrl_layer_is_refused_while_the_filter_prompt_is_up() {
    let mut state = columnar_grid(0);
    state.geometry = state.geometry.with_export_fits(true).with_theme_rows(40);
    press(&mut state, Key::Char('f'));

    for key in ['t', 'e', 'r', 'f'] {
        let effects = handle_key(&mut state, KeyPress::ctrl(Key::Char(key)));

        assert!(effects.is_empty(), "ctrl-{key} is refused");
        assert!(
            matches!(state.overlay, Some(Overlay::Filter(_))),
            "and leaves the prompt up rather than opening something over it"
        );
    }
}

#[test]
fn a_filter_that_shrinks_the_grid_pulls_the_row_cursor_back() {
    let mut state = columnar_grid(1);
    state.contents.cursor.go_to(2, 3);

    press(&mut state, Key::Char('f'));
    type_into(&mut state, "aarhus");
    press(&mut state, Key::Enter);

    assert_eq!(
        state.contents.cursor.index(),
        0,
        "one row is left, and the cursor may not point past it"
    );
}

#[test]
fn a_column_yanks_the_rows_the_grid_is_showing() {
    let mut state = columnar_grid(0);
    state.contents.set_filter(1, Search::new("berlin"));
    state.contents.cycle_sort(0);

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(
        yanked(&effects),
        Some("Cleo,Bob"),
        "filtered and in the order drawn, not the order returned"
    );
}

#[test]
fn exporting_the_fetched_rows_writes_the_ones_the_grid_is_showing() {
    // A full page, so the form has a Fetched scope to toggle to.
    let mut state = connected(Posture::ReadOnly);
    state.geometry = Geometry::default().with_export_fits(true);
    state.focus_on(Panel::Contents);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: vec!["kind".to_owned()],
            rows: (0..PAGE)
                .map(|row| Row {
                    values: vec![CqlValue::Text(
                        if row % 2 == 0 { "keep" } else { "drop" }.to_owned(),
                    )],
                })
                .collect(),
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state.contents.set_filter(0, Search::new("keep"));

    open_form(&mut state);
    for _ in 0..2 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }
    handle_key(&mut state, KeyPress::new(Key::Char('k'))); // Scope: Whole -> Fetched.
    for _ in 0..1 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }
    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

    let [
        Effect::Export {
            source: Source::Fetched { rows, .. },
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("one export effect carrying the rows");
    };
    assert_eq!(
        rows.len(),
        PAGE / 2,
        "a form may not write what the grid hides"
    );
}

#[test]
fn clicking_the_sort_mark_sorts_the_column_it_belongs_to() {
    let mut state = columnar_grid(1);

    update(
        &mut state,
        Msg::Pointer(Pointer::Click(Click::Mark {
            panel: Panel::Contents,
            column: 0,
            mark: Mark::Sort,
        })),
    );

    assert_eq!(first_names(&state), ["Cleo", "Bob", "Ada"]);
    assert_eq!(
        state.contents.column.index(),
        0,
        "a press says which column it means, and the cursor follows it there"
    );
}

#[test]
fn clicking_the_filter_mark_opens_the_prompt_over_that_column() {
    let mut state = columnar_grid(0);

    update(
        &mut state,
        Msg::Pointer(Pointer::Click(Click::Mark {
            panel: Panel::Contents,
            column: 1,
            mark: Mark::Filter,
        })),
    );

    assert_eq!(filter_prompt(&state).column(), 1);
}

#[test]
fn a_mark_on_a_column_that_is_gone_is_dropped_rather_than_clamped() {
    let mut state = columnar_grid(0);

    update(
        &mut state,
        Msg::Pointer(Pointer::Click(Click::Mark {
            panel: Panel::Contents,
            column: 9,
            mark: Mark::Sort,
        })),
    );

    assert_eq!(
        state
            .contents
            .loaded()
            .and_then(crate::contents::GridPage::sort),
        None,
        "the map a press was resolved against describes the previous frame"
    );
}

#[test]
fn column_mode_is_the_one_bar_that_names_the_column_keys() {
    let mut state = columnar_grid(0);
    let named = |state: &State| -> Vec<&'static str> {
        crate::view::hints::bar(state)
            .panel
            .iter()
            .map(|hint| hint.action)
            .collect()
    };

    let shown = named(&state);
    assert!(shown.contains(&"sort") && shown.contains(&"filter"));

    state.contents.mode = SelectionMode::Row;
    let shown = named(&state);
    assert!(
        !shown.contains(&"sort") && !shown.contains(&"filter"),
        "a hint has to be true in the frame it is drawn in"
    );
}

#[test]
fn the_clear_filter_hint_is_greyed_until_there_is_one_to_clear() {
    let mut state = columnar_grid(0);
    let liveness = |state: &State| {
        crate::view::hints::bar(state)
            .panel
            .into_iter()
            .find(|hint| hint.action == "clear filter")
            .expect("column mode names it")
            .liveness
    };

    assert_eq!(liveness(&state), crate::view::Liveness::Inert);

    state.contents.set_filter(0, Search::new("bob"));

    assert_eq!(liveness(&state), crate::view::Liveness::Live);
}

#[test]
fn the_bar_under_the_filter_prompt_names_the_two_ways_out_and_nothing_else() {
    let mut state = columnar_grid(0);
    press(&mut state, Key::Char('f'));

    let bar = crate::view::hints::bar(&state);

    assert!(bar.panel.is_empty(), "the panel underneath answers nothing");
    assert_eq!(
        bar.global
            .iter()
            .map(|hint| hint.action)
            .collect::<Vec<_>>(),
        vec!["apply", "cancel"]
    );
}
