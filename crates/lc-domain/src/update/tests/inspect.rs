use super::*;

fn inspecting(state: &State) -> Option<&Inspector> {
    match &state.overlay {
        Some(Overlay::Inspect(inspector)) => Some(inspector),
        _ => None,
    }
}

/// Forty lines, what `inspectable` wraps to, so cursor and offset share a unit.
fn opened_long() -> State {
    let mut state = opened(SelectionMode::Cell);
    let Some(Overlay::Inspect(inspector)) = &state.overlay else {
        panic!("the inspector is up");
    };

    state.overlay = Some(Overlay::Inspect(Inspector::opened(
        0,
        inspector.subject().clone(),
        Inspected::Cell {
            at: 0,
            column: "c1".to_owned(),
            value: CqlValue::List((0..38).map(CqlValue::Int).collect()),
        },
    )));
    state
}

#[test]
fn enter_in_row_mode_inspects_the_whole_row() {
    let state = opened(SelectionMode::Row);

    assert_eq!(
        inspecting(&state).map(Inspector::inspected),
        Some(&Inspected::Row {
            number: 3,
            columns: vec!["c0".to_owned(), "c1".to_owned(), "c2".to_owned()],
            values: ["r2c0", "r2c1", "r2c2"]
                .map(|text| CqlValue::Text(text.to_owned()))
                .to_vec(),
        })
    );
}

#[test]
fn enter_in_cell_mode_inspects_the_cell_under_the_cursor() {
    let state = opened(SelectionMode::Cell);

    assert_eq!(
        inspecting(&state).map(Inspector::inspected),
        Some(&Inspected::Cell {
            at: 1,
            column: "c1".to_owned(),
            value: CqlValue::Text("r2c1".to_owned()),
        })
    );
}

#[test]
fn enter_in_column_mode_is_inert() {
    assert_eq!(opened(SelectionMode::Column).overlay, None);
}

#[test]
fn enter_inspects_the_row_as_the_grid_draws_it() {
    let mut state = inspectable(2, 3);
    state.contents.cycle_sort(0);

    press(&mut state, Key::Enter);

    let Some(Inspected::Row { number, values, .. }) = inspecting(&state).map(Inspector::inspected)
    else {
        panic!("the row is up");
    };
    assert_eq!(*number, 1, "the gutter's number, not the cluster's order");
    assert_eq!(values[0], CqlValue::Text("r2c0".to_owned()));
}

#[test]
fn enter_is_inert_with_nothing_loaded() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.focus_on(Panel::Contents);
    state.geometry = state.geometry.with_inspector(10, 0, Span::default());

    press(&mut state, Key::Enter);

    assert_eq!(state.overlay, None);
}

#[test]
fn enter_is_inert_on_an_empty_grid() {
    let mut state = inspectable(2, 0);

    press(&mut state, Key::Enter);

    assert_eq!(state.overlay, None);
}

#[test]
fn enter_is_inert_while_a_range_is_held() {
    for mode in [
        SelectionMode::Row,
        SelectionMode::Cell,
        SelectionMode::Column,
    ] {
        let mut state = inspectable(3, 4);
        state.contents.mode = mode;
        press(&mut state, Key::Char('v'));

        press(&mut state, Key::Enter);
        assert_eq!(state.overlay, None, "extending, {mode:?}");

        press(&mut state, Key::Char('v'));
        press(&mut state, Key::Enter);
        assert_eq!(state.overlay, None, "frozen, {mode:?}");
    }
}

#[test]
fn enter_is_inert_on_a_frame_with_no_room_for_the_block() {
    let mut state = grid_of_named_cells(3, 4);

    press(&mut state, Key::Enter);

    assert_eq!(state.overlay, None);
}

#[test]
fn j_k_and_the_arrows_move_a_line() {
    let mut state = opened_long();

    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Down);
    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(2));

    press(&mut state, Key::Char('k'));
    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(1));
    press(&mut state, Key::Up);
    press(&mut state, Key::Up);
    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(0));
}

#[test]
fn the_paging_keys_move_a_body_height() {
    let mut state = opened_long();

    press(&mut state, Key::PageDown);
    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(10));
    press_binding(&mut state, KeyPress::ctrl(Key::Char('d')));
    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(20));
    press_binding(&mut state, KeyPress::ctrl(Key::Char('u')));
    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(10));
    press(&mut state, Key::PageUp);
    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(0));
}

#[test]
fn g_home_g_and_end_reach_the_top_and_the_bottom() {
    for (bottom, top) in [(Key::Char('G'), Key::Char('g')), (Key::End, Key::Home)] {
        let mut state = opened_long();

        press(&mut state, bottom);
        assert_eq!(
            inspecting(&state).map(Inspector::cursor),
            Some(39),
            "the last laid-out line"
        );

        press(&mut state, top);
        assert_eq!(inspecting(&state).map(Inspector::cursor), Some(0));
    }
}

#[test]
fn the_cursor_stops_at_the_last_line() {
    let mut state = opened_long();

    for _ in 0..50 {
        press(&mut state, Key::Char('j'));
    }

    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(39));
}

#[test]
fn a_value_of_one_line_has_nowhere_to_move() {
    let mut state = opened(SelectionMode::Cell);

    press(&mut state, Key::Char('j'));
    press(&mut state, Key::End);

    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(0));
}

#[test]
fn the_offset_follows_the_span_the_frame_drew_the_cursor_at() {
    let mut state = opened_long();
    press(&mut state, Key::End);

    update(
        &mut state,
        Msg::Measured(
            Geometry::new([8, 8, 8, 1], 0, 0)
                .with_grid_width(80)
                .with_inspector(
                    10,
                    40,
                    Span {
                        first: 39,
                        lines: 1,
                    },
                ),
        ),
    );

    assert_eq!(
        inspecting(&state).map(Inspector::offset),
        Some(30),
        "the cursor's line at the bottom of the body"
    );
}

#[test]
fn enter_and_escape_close_it() {
    for key in [Key::Enter, Key::Escape] {
        let mut state = opened(SelectionMode::Cell);

        let effects = press(&mut state, key);

        assert_eq!(state.overlay, None, "{key:?}");
        assert!(effects.is_empty());
    }
}

#[test]
fn q_quits_from_the_inspector() {
    let mut state = opened(SelectionMode::Cell);

    let effects = press(&mut state, Key::Char('q'));

    assert!(effects.contains(&Effect::Quit));
}

#[test]
fn every_other_key_is_inert() {
    for key in [
        Key::Char('m'),
        Key::Char('v'),
        Key::Char('/'),
        Key::Char('d'),
        Key::Char('l'),
        Key::Char('?'),
        Key::Char('1'),
        Key::Tab,
    ] {
        let mut state = opened(SelectionMode::Cell);
        let before = state.clone();

        let effects = press(&mut state, key);

        assert!(effects.is_empty(), "{key:?}");
        assert_eq!(state.overlay, before.overlay, "{key:?}");
        assert_eq!(state.focus(), Panel::Contents, "{key:?}");
        assert_eq!(state.contents, before.contents, "{key:?}");
    }

    let mut state = opened(SelectionMode::Cell);
    for chord in ['t', 'o', 'e'] {
        press_binding(&mut state, KeyPress::ctrl(Key::Char(chord)));
        assert!(inspecting(&state).is_some(), "ctrl-{chord}");
    }
}

#[test]
fn y_yanks_the_bare_value_and_leaves_the_inspector_open() {
    let mut state = opened(SelectionMode::Cell);

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(effects, vec![Effect::Yank("r2c1".to_owned())]);
    assert!(inspecting(&state).is_some());
}

#[test]
fn y_yanks_the_row_as_one_csv_line() {
    let mut state = opened(SelectionMode::Row);

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(effects, vec![Effect::Yank("r2c0,r2c1,r2c2".to_owned())]);
    assert!(inspecting(&state).is_some());
}

#[test]
fn shift_y_yanks_the_line_the_cursor_is_on() {
    let mut state = opened_long();
    press(&mut state, Key::Char('j'));

    let effects = press(&mut state, Key::Char('Y'));

    assert_eq!(
        effects,
        vec![Effect::Yank("0,".to_owned())],
        "the first element, its indent dropped"
    );
    assert!(inspecting(&state).is_some());
}

#[test]
fn shift_y_yanks_a_row_field_without_its_column_name() {
    let mut state = opened(SelectionMode::Row);
    press(&mut state, Key::Char('j'));

    let effects = press(&mut state, Key::Char('Y'));

    assert_eq!(effects, vec![Effect::Yank("r2c1".to_owned())]);
}

#[test]
fn y_over_a_null_yanks_nothing() {
    let mut state = populated(Panel::Contents);
    state.contents.mode = SelectionMode::Cell;
    state.geometry = state.geometry.with_inspector(10, 1, Span::default());
    press(&mut state, Key::Enter);
    assert!(inspecting(&state).is_some());

    assert!(press(&mut state, Key::Char('y')).is_empty());
    assert!(press(&mut state, Key::Char('Y')).is_empty());
}

#[test]
fn the_wheel_moves_the_inspector_and_a_click_closes_it() {
    let mut state = opened_long();

    update(
        &mut state,
        Msg::Pointer(Pointer::Wheel {
            panel: Panel::Tables,
            direction: Wheel::Down,
        }),
    );
    assert_eq!(inspecting(&state).map(Inspector::cursor), Some(WHEEL_ROWS));

    update(
        &mut state,
        Msg::Pointer(Pointer::Click(Click::Panel(Panel::Tables))),
    );
    assert_eq!(state.overlay, None);
    assert_eq!(state.focus(), Panel::Contents, "the click spent itself");
}

#[test]
fn a_frame_too_small_for_the_block_closes_it() {
    let mut state = opened(SelectionMode::Cell);

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
    );

    assert_eq!(state.overlay, None);
}

#[test]
fn a_value_that_rewraps_shorter_pulls_the_offset_back() {
    let mut state = opened_long();
    press(&mut state, Key::End);
    let span = Span {
        first: 39,
        lines: 1,
    };
    let measured = |lines| {
        Msg::Measured(
            Geometry::new([8, 8, 8, 1], 0, 0)
                .with_grid_width(80)
                .with_inspector(10, lines, span),
        )
    };
    update(&mut state, measured(40));

    update(&mut state, measured(15));

    assert_eq!(inspecting(&state).map(Inspector::offset), Some(5));
    assert!(state.dirty);
}

#[test]
fn the_bar_names_move_copy_and_close_over_the_inspector() {
    let state = opened(SelectionMode::Cell);

    let bar = crate::view::hints::bar(&state);

    assert_eq!(
        bar.panel
            .iter()
            .chain(&bar.global)
            .map(|hint| format!("{} {}", hint.keys, hint.action))
            .collect::<Vec<_>>(),
        ["j/k move", "y copy", "Y copy line", "e edit", "esc close"]
    );
}

#[test]
fn the_contents_bar_names_inspect_only_where_enter_opens_it() {
    let named = |state: &State| {
        crate::view::hints::bar(state)
            .panel
            .iter()
            .any(|hint| hint.action == "inspect")
    };

    let mut column = inspectable(3, 4);
    column.contents.mode = SelectionMode::Column;
    assert!(!named(&column), "column mode");

    for mode in [SelectionMode::Row, SelectionMode::Cell] {
        let mut state = inspectable(3, 4);
        state.contents.mode = mode;
        assert!(named(&state), "{mode:?}");

        press(&mut state, Key::Char('v'));
        assert!(!named(&state), "a range is held, {mode:?}");
    }

    assert!(!named(&inspectable(3, 0)), "an empty grid");
    assert!(!named(&grid_of_named_cells(3, 4)), "no room for the block");
}
