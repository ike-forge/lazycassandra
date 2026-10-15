use super::*;

fn failure() -> Overlay {
    Overlay::Message(Message {
        title: "Cannot connect".to_owned(),
        message: "no route to host".to_owned(),
        severity: Severity::Danger,
    })
}

fn click_cell(state: &mut State, panel: Panel, row: usize, column: usize) -> Vec<Effect> {
    point(
        state,
        Pointer::Click(Click::Row {
            panel,
            row,
            column: Some(column),
        }),
    )
}

/// `shop.orders` loaded with `count` single-column rows.
fn showing_rows(count: usize) -> State {
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
            columns: vec!["id".to_owned()],
            rows: (0..count)
                .map(|i| Row {
                    values: vec![CqlValue::Int(
                        i64::try_from(i).expect("the fixture is small"),
                    )],
                })
                .collect(),
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state
}

/// `shop.orders` loaded with one row of `columns` columns.
fn showing_columns(columns: usize) -> State {
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
            columns: (0..columns).map(|i| format!("c{i}")).collect(),
            rows: vec![Row {
                values: (0..columns)
                    .map(|i| CqlValue::Int(i64::try_from(i).expect("the fixture is small")))
                    .collect(),
            }],
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state
}

#[test]
fn a_click_on_a_keyspace_folds_and_unfolds_it() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

    click_row(&mut state, Panel::Tables, 0);
    assert_eq!(row_count(&state), 3, "the keyspace unfolded");

    click_row(&mut state, Panel::Tables, 0);
    assert_eq!(row_count(&state), 1, "and folded again");
}

#[test]
fn a_click_on_a_table_reads_it() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    click_row(&mut state, Panel::Tables, 0);

    let effects = click_row(&mut state, Panel::Tables, 1);

    assert!(
        matches!(
            effects.as_slice(),
            [Effect::ExecutePaged { .. }, Effect::RecordHistory(_)]
        ),
        "got {effects:?}"
    );
    assert_eq!(
        state.tables.cursor.index(),
        1,
        "on the row that was clicked"
    );
}

#[test]
fn a_click_on_a_grid_row_moves_the_cursor_and_asks_for_nothing() {
    let mut state = showing_rows(5);

    let effects = click_row(&mut state, Panel::Contents, 3);

    assert!(effects.is_empty(), "a selection runs no query");
    assert_eq!(state.contents.cursor.index(), 3);
}

#[test]
fn a_click_on_a_cell_moves_both_cursors() {
    let mut state = showing_columns(4);

    let effects = click_cell(&mut state, Panel::Contents, 0, 2);

    assert!(effects.is_empty(), "a selection runs no query");
    assert_eq!(state.contents.cursor.index(), 0);
    assert_eq!(state.contents.column.index(), 2);
}

/// A press names whatever cells it can reach; which of them the grid draws
/// a band around is the mode's business, and `m` is the only thing that
/// changes it.
#[test]
fn a_click_on_a_cell_leaves_the_selection_mode_alone() {
    for mode in [
        SelectionMode::Cell,
        SelectionMode::Row,
        SelectionMode::Column,
    ] {
        let mut state = showing_columns(4);
        state.contents.mode = mode;

        click_cell(&mut state, Panel::Contents, 0, 2);

        assert_eq!(state.contents.mode, mode, "from {mode:?}");
        assert_eq!(state.contents.column.index(), 2, "from {mode:?}");
    }
}

#[test]
fn a_click_on_a_heading_moves_the_column_cursor_and_no_row() {
    let mut state = showing_columns(4);
    state.contents.cursor.go_to(0, 1);

    let effects = point(
        &mut state,
        Pointer::Click(Click::Column {
            panel: Panel::Contents,
            column: 3,
        }),
    );

    assert!(effects.is_empty());
    assert_eq!(state.focus(), Panel::Contents);
    assert_eq!(state.contents.column.index(), 3);
    assert_eq!(state.contents.cursor.index(), 0, "a heading names no row");
}

#[test]
fn a_click_on_a_column_that_has_gone_is_dropped_rather_than_clamped() {
    let mut state = showing_columns(2);
    state.contents.column.go_to(1, 2);

    point(
        &mut state,
        Pointer::Click(Click::Column {
            panel: Panel::Contents,
            column: 5,
        }),
    );

    assert_eq!(
        state.contents.column.index(),
        1,
        "a column the page no longer has moves nothing"
    );
}

#[test]
fn a_click_on_a_ddl_row_moves_the_cursor() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    click_row(&mut state, Panel::Tables, 0);
    click_row(&mut state, Panel::Tables, 1);
    let rows = crate::view::ddl_rows(&state);
    assert!(rows > 2, "the fixture must describe something, got {rows}");

    let effects = click_row(&mut state, Panel::Ddl, 2);

    assert!(effects.is_empty(), "a selection runs no query");
    assert_eq!(state.focus(), Panel::Ddl);
    assert_eq!(state.ddl.cursor.index(), 2);
}

#[test]
fn a_click_on_a_ddl_row_that_has_gone_is_dropped_rather_than_clamped() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    click_row(&mut state, Panel::Tables, 0);

    // Nothing is selected, so the panel describes nothing and has no rows.
    let effects = click_row(&mut state, Panel::Ddl, 3);

    assert!(effects.is_empty());
    assert_eq!(state.ddl.cursor.index(), 0);
}

#[test]
fn a_click_on_a_tab_focuses_its_panel_and_draws_it() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    click_row(&mut state, Panel::Tables, 0);
    click_row(&mut state, Panel::Tables, 1);
    state.focus_on(Panel::Queries);

    let effects = point(
        &mut state,
        Pointer::Click(Click::Tab {
            panel: Panel::Ddl,
            tab: 1,
        }),
    );

    assert!(effects.is_empty());
    assert_eq!(state.focus(), Panel::Ddl);
    assert_eq!(state.ddl.tab(), DdlTab::Columns);
}

#[test]
fn a_click_on_the_logs_tab_focuses_queries_and_draws_the_trail() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);

    point(
        &mut state,
        Pointer::Click(Click::Tab {
            panel: Panel::Queries,
            tab: 1,
        }),
    );

    assert_eq!(state.focus(), Panel::Queries);
    assert_eq!(state.queries.tab(), QueriesTab::Logs);
}

/// The same reachability rule the key obeys: on a keyspace the strip has
/// one tab, so a press on a strip drawn a frame ago cannot reach a tab
/// this frame has not got.
#[test]
fn a_click_on_a_tab_the_strip_no_longer_draws_only_focuses() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.focus_on(Panel::Queries);

    point(
        &mut state,
        Pointer::Click(Click::Tab {
            panel: Panel::Ddl,
            tab: 1,
        }),
    );

    assert_eq!(state.focus(), Panel::Ddl);
    assert_eq!(state.ddl.tab(), DdlTab::Ddl);
}

#[test]
fn a_click_on_a_single_tab_panels_title_focuses_it() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.focus_on(Panel::Queries);

    point(
        &mut state,
        Pointer::Click(Click::Tab {
            panel: Panel::Tables,
            tab: 0,
        }),
    );

    assert_eq!(state.focus(), Panel::Tables);
}

#[test]
fn a_click_focuses_the_panel_it_landed_in() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.focus_on(Panel::Queries);

    click_row(&mut state, Panel::Tables, 0);

    assert_eq!(state.focus(), Panel::Tables, "one click focuses and acts");
    assert_eq!(row_count(&state), 2, "the keyspace unfolded on that click");
}

#[test]
fn a_click_on_chrome_focuses_and_does_nothing_else() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    let before = state.tables.clone();

    let effects = point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

    assert!(effects.is_empty());
    assert_eq!(
        state.tables, before,
        "chrome moves no selection and folds nothing"
    );
}

#[test]
fn a_click_on_a_row_that_has_gone_is_dropped_rather_than_clamped() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    click_row(&mut state, Panel::Tables, 0);
    click_row(&mut state, Panel::Tables, 0);

    // Row 2 was `users` a frame ago; the tree is one row long now.
    let effects = click_row(&mut state, Panel::Tables, 2);

    assert!(effects.is_empty(), "a stale row reads nothing");
    assert_eq!(state.tables.cursor.index(), 0, "and moves nothing");
}

#[test]
fn a_click_dismisses_the_overlay_and_does_nothing_else() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.overlay = Some(failure());

    let effects = click_row(&mut state, Panel::Tables, 0);

    assert!(state.overlay.is_none());
    assert!(effects.is_empty());
    assert_eq!(
        row_count(&state),
        1,
        "the keyspace the overlay covered did not unfold"
    );
}

#[test]
fn a_wheel_scrolls_the_panel_under_it_without_focusing_it() {
    let mut state = showing_rows(20);
    state.geometry = Geometry::new([8, 8, 8, 1], 0, 0);
    state.focus_on(Panel::Tables);

    point(
        &mut state,
        Pointer::Wheel {
            panel: Panel::Contents,
            direction: Wheel::Down,
        },
    );

    assert_eq!(state.contents.scroll.offset(), 3, "three rows a notch");
    assert_eq!(
        state.contents.cursor.index(),
        0,
        "the wheel does not move the selection"
    );
    assert_eq!(state.focus(), Panel::Tables, "focus stays where it was");
}

#[test]
fn a_wheel_up_walks_back_the_way_it_came() {
    let mut state = showing_rows(20);
    state.geometry = Geometry::new([8, 8, 8, 1], 0, 0);
    let down = Pointer::Wheel {
        panel: Panel::Contents,
        direction: Wheel::Down,
    };
    point(&mut state, down);
    point(&mut state, down);

    point(
        &mut state,
        Pointer::Wheel {
            panel: Panel::Contents,
            direction: Wheel::Up,
        },
    );

    assert_eq!(state.contents.scroll.offset(), 3);
}

#[test]
fn a_wheel_over_the_ddl_panel_scrolls_its_lines() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.geometry = Geometry::new([8, 8, 8, 1], 40, 0);

    point(
        &mut state,
        Pointer::Wheel {
            panel: Panel::Ddl,
            direction: Wheel::Down,
        },
    );

    assert_eq!(state.ddl.scroll.offset(), 3);
}

#[test]
fn a_wheel_over_a_panel_with_nothing_to_scroll_is_inert() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.focus_on(Panel::Queries);
    for character in "one\ntwo\nthree\nfour\nfive\n".chars() {
        press(
            &mut state,
            if character == '\n' {
                Key::Enter
            } else {
                Key::Char(character)
            },
        );
    }
    state.focus_on(Panel::Tables);
    let cursor_before = state.queries.cursor();
    let text_before = state.queries.text();

    let effects = point(
        &mut state,
        Pointer::Wheel {
            panel: Panel::Queries,
            direction: Wheel::Down,
        },
    );

    assert!(effects.is_empty());
    assert_eq!(state.tables.cursor.index(), 0, "no other panel moved");
    assert_eq!(state.focus(), Panel::Tables);
    assert_eq!(
        state.queries.cursor(),
        cursor_before,
        "the wheel did not move the editor's cursor"
    );
    assert_eq!(
        state.queries.text(),
        text_before,
        "the wheel did not touch the buffer"
    );
}

#[test]
fn a_wheel_over_tables_moves_the_viewport_rather_than_the_selection() {
    // Four keyspaces, so three rows a notch would have landed on the
    // last one under the old cursor-moving wheel. Now nothing about the
    // tree selection changes, so `described()` sees no subject change
    // and `state.ddl.scroll` is left alone (design §7).
    let mut state = browsing(&["a", "b", "c", "d"]);
    state.geometry = Geometry::new([3, 3, 3, 1], 0, 0);
    state.focus_on(Panel::Queries);
    state.ddl.scroll.down(100, 2);
    let ddl_before = state.ddl.scroll;
    assert_ne!(ddl_before.offset(), 0, "the fixture must start scrolled");

    let effects = point(
        &mut state,
        Pointer::Wheel {
            panel: Panel::Tables,
            direction: Wheel::Down,
        },
    );

    assert!(effects.is_empty(), "a scroll alone asks for nothing");
    assert_eq!(state.tables.scroll.offset(), 1, "four rows, three visible");
    assert_eq!(state.tables.cursor.index(), 0, "the selection did not move");
    assert_eq!(
        state.focus(),
        Panel::Queries,
        "the wheel does not move focus"
    );
    assert_eq!(
        state.ddl.scroll, ddl_before,
        "no selection changed, so nothing new is described"
    );
}

#[test]
fn a_wheel_does_not_dismiss_the_overlay() {
    let mut state = showing_rows(20);
    state.geometry = Geometry::new([8, 8, 8, 1], 0, 0);
    state.overlay = Some(failure());

    point(
        &mut state,
        Pointer::Wheel {
            panel: Panel::Contents,
            direction: Wheel::Down,
        },
    );

    assert!(state.overlay.is_some(), "only a click dismisses");
    assert_eq!(state.contents.cursor.index(), 0, "and nothing scrolled");
}

/// A Contents panel showing `count` rows in a five-row viewport.
fn scrolling_rows(count: usize) -> State {
    let mut state = showing_rows(count);
    state.geometry = Geometry::new([5, 5, 5, 1], 0, 0);
    state
}

/// Named apart from the production `wheel`, which has the same parameter
/// list and would otherwise be shadowed by this inside `use super::*`.
fn roll(state: &mut State, panel: Panel, direction: Wheel) -> Vec<Effect> {
    point(state, Pointer::Wheel { panel, direction })
}

#[test]
fn a_wheel_scrolls_the_view_and_leaves_the_selection_where_it_was() {
    let mut state = scrolling_rows(40);

    roll(&mut state, Panel::Contents, Wheel::Down);

    assert_eq!(state.contents.scroll.offset(), 3, "three rows a notch");
    assert_eq!(
        state.contents.cursor.index(),
        0,
        "the selection is where the user put it"
    );
}

#[test]
fn a_wheel_up_walks_the_view_back() {
    let mut state = scrolling_rows(40);
    roll(&mut state, Panel::Contents, Wheel::Down);
    roll(&mut state, Panel::Contents, Wheel::Down);

    roll(&mut state, Panel::Contents, Wheel::Up);

    assert_eq!(state.contents.scroll.offset(), 3);
}

#[test]
fn a_wheel_stops_at_both_ends() {
    let mut state = scrolling_rows(8);

    for _ in 0..10 {
        roll(&mut state, Panel::Contents, Wheel::Down);
    }
    assert_eq!(
        state.contents.scroll.offset(),
        3,
        "eight rows in a five-row panel scroll by three and stop"
    );

    for _ in 0..10 {
        roll(&mut state, Panel::Contents, Wheel::Up);
    }
    assert_eq!(state.contents.scroll.offset(), 0);
}

#[test]
fn a_motion_after_a_wheel_brings_the_selection_back_into_view() {
    let mut state = scrolling_rows(40);
    state.focus_on(Panel::Contents);
    for _ in 0..5 {
        roll(&mut state, Panel::Contents, Wheel::Down);
    }
    assert_eq!(state.contents.scroll.offset(), 15, "scrolled well past it");

    press(&mut state, Key::Char('j'));

    assert_eq!(state.contents.cursor.index(), 1);
    assert_eq!(
        state.contents.scroll.offset(),
        1,
        "the view came back to the selection, the shortest distance `follow` allows"
    );
}

#[test]
fn walking_off_the_bottom_still_drags_the_view_with_it() {
    let mut state = scrolling_rows(40);
    state.focus_on(Panel::Contents);

    for _ in 0..6 {
        press(&mut state, Key::Char('j'));
    }

    assert_eq!(state.contents.cursor.index(), 6);
    assert_eq!(
        state.contents.scroll.offset(),
        2,
        "the sixth row is the last of a five-row viewport"
    );
}

#[test]
fn a_click_on_a_scrolled_row_needs_no_correction() {
    let mut state = scrolling_rows(40);
    for _ in 0..4 {
        roll(&mut state, Panel::Contents, Wheel::Down);
    }
    let offset = state.contents.scroll.offset();

    click_row(&mut state, Panel::Contents, offset + 1);

    assert_eq!(state.contents.cursor.index(), offset + 1);
    assert_eq!(
        state.contents.scroll.offset(),
        offset,
        "the row was on screen, so the view had no reason to move"
    );
}

#[test]
fn a_list_that_shrank_pulls_its_offset_back() {
    let mut state = browsing_tables(&[("shop", &["a", "b", "c", "d", "e", "f"])]);
    state.geometry = Geometry::new([3, 3, 3, 1], 0, 0);
    click_row(&mut state, Panel::Tables, 0);
    for _ in 0..3 {
        roll(&mut state, Panel::Tables, Wheel::Down);
    }
    assert_eq!(state.tables.scroll.offset(), 4, "seven rows, three visible");

    // Folding takes the tree back to one row, which no offset can be past.
    click_row(&mut state, Panel::Tables, 0);

    assert_eq!(row_count(&state), 1);
    assert_eq!(state.tables.scroll.offset(), 0);
}

#[test]
fn a_wheel_over_tables_no_longer_scrolls_the_ddl_panel_back() {
    let mut state = browsing_tables(&[("shop", &["a", "b", "c", "d", "e", "f"])]);
    state.geometry = Geometry::new([3, 3, 3, 1], 40, 0);
    click_row(&mut state, Panel::Tables, 0);
    state.ddl.scroll.page_down(40, 3);
    let before = state.ddl.scroll;
    assert_ne!(before.offset(), 0, "the fixture starts scrolled");

    roll(&mut state, Panel::Tables, Wheel::Down);

    assert_eq!(
        state.ddl.scroll, before,
        "the wheel moved no selection, so nothing is newly described"
    );
}

#[test]
fn a_resize_brings_the_selection_back_into_view() {
    let mut state = scrolling_rows(40);
    state.focus_on(Panel::Contents);
    for _ in 0..6 {
        press(&mut state, Key::Char('j'));
    }
    assert_eq!(state.contents.cursor.index(), 6);
    assert_eq!(state.contents.scroll.offset(), 2, "on the last drawn row");

    update(&mut state, Msg::Measured(Geometry::new([2, 2, 2, 1], 0, 0)));

    assert_eq!(
        state.contents.scroll.offset(),
        5,
        "a panel that lost rows draws its selection again"
    );
}

#[test]
fn a_measurement_that_changes_no_height_leaves_a_wheel_alone() {
    let mut state = scrolling_rows(40);
    for _ in 0..5 {
        roll(&mut state, Panel::Contents, Wheel::Down);
    }
    let scrolled = state.contents.scroll.offset();
    assert_eq!(scrolled, 15);

    update(&mut state, Msg::Measured(Geometry::new([5, 5, 5, 1], 0, 0)));

    assert_eq!(
        state.contents.scroll.offset(),
        scrolled,
        "the selection is off screen and the resize follow must not fetch it"
    );
    assert_eq!(state.contents.cursor.index(), 0);
}

#[test]
fn a_resize_that_moves_a_viewport_asks_for_a_repaint() {
    let mut state = scrolling_rows(8);
    for _ in 0..5 {
        roll(&mut state, Panel::Contents, Wheel::Down);
    }
    assert_eq!(state.contents.scroll.offset(), 3);
    state.dirty = false;

    // Eight rows in an eight-row panel leave nothing to scroll.
    update(&mut state, Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0)));

    assert_eq!(state.contents.scroll.offset(), 0);
    assert!(
        state.dirty,
        "a measurement marks nothing dirty by itself, so an offset it \
         moved would otherwise not be drawn until the next keypress"
    );
}

#[test]
fn a_wheel_over_the_palette_walks_its_cursor_without_closing_it() {
    let mut state = measured(&["shop"]);
    press(&mut state, Key::Char('?'));
    let wheel = |direction| Pointer::Wheel {
        panel: Panel::Contents,
        direction,
    };

    point(&mut state, wheel(Wheel::Down));
    let down = palette(&state)
        .expect("the wheel does not dismiss")
        .selected();
    point(&mut state, wheel(Wheel::Up));
    let back = palette(&state).expect("nor does the way back").selected();

    assert!(down > back, "down moved the cursor and up undid it");
}

fn asked_to_drop_a_table() -> State {
    let mut state = droppable_tree();
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('d'));
    assert!(dropping(&state).is_some(), "the confirmation is up");
    state
}

#[test]
fn a_pressed_button_answers_as_its_key_does() {
    let mut pressed = asked_to_drop_a_table();
    let mut typed = asked_to_drop_a_table();

    let by_pointer = point(&mut pressed, Pointer::Button(Key::Char('y')));
    let by_key = press(&mut typed, Key::Char('y'));

    let drops = |effects: &[Effect]| {
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::Execute { .. }))
            .count()
    };
    assert_eq!(drops(&by_pointer), 1, "the drop is issued");
    assert_eq!(drops(&by_pointer), drops(&by_key));
    assert_eq!(pressed.overlay, typed.overlay);
}

#[test]
fn a_click_on_the_modal_off_its_buttons_answers_nothing() {
    let mut state = asked_to_drop_a_table();

    let effects = point(&mut state, Pointer::Modal);

    assert!(effects.is_empty());
    assert!(dropping(&state).is_some(), "the question is still open");
}

#[test]
fn a_click_outside_the_modal_closes_it() {
    let mut state = asked_to_drop_a_table();

    let effects = click_row(&mut state, Panel::Tables, 0);

    assert!(effects.is_empty());
    assert!(dropping(&state).is_none());
}
