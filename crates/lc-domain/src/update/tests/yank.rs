use super::*;

#[test]
fn y_yanks_a_grid_row() {
    let mut state = browsing_a_table();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");
    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page {
                columns: vec!["id".to_owned(), "name".to_owned()],
                rows: vec![Row {
                    values: vec![CqlValue::Int(7), CqlValue::Text("ada".to_owned())],
                }],
                next: None,
            }),
            took: Duration::ZERO,
        },
    );

    state.contents.mode = SelectionMode::Row;
    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(effects, vec![Effect::Yank("7,ada".to_owned())]);
}

#[test]
fn a_yanked_row_carries_every_column_of_the_page() {
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
            columns: vec![
                "c0".to_owned(),
                "c1".to_owned(),
                "c2".to_owned(),
                "c3".to_owned(),
            ],
            rows: vec![Row {
                values: vec![
                    CqlValue::Text("v0".to_owned()),
                    CqlValue::Text("v1".to_owned()),
                    CqlValue::Text("v2".to_owned()),
                    CqlValue::Text("v3".to_owned()),
                ],
            }],
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state.focus_on(Panel::Contents);
    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(7)),
    );

    let (widths, available) = grid_columns(&state).expect("a page is loaded");
    assert!(
        grid::fitting(&widths, 0, available) < widths.len(),
        "the viewport must hide at least one column for this test to prove anything"
    );

    state.contents.mode = SelectionMode::Row;
    assert_eq!(
        press(&mut state, Key::Char('y')),
        vec![Effect::Yank("v0,v1,v2,v3".to_owned())]
    );
}

#[test]
fn the_column_follow_draws_by_the_widths_the_grid_is_drawn_at() {
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
            columns: vec!["id".to_owned(), "body".to_owned()],
            rows: vec![Row {
                values: vec![CqlValue::Int(1), CqlValue::Text("x".repeat(500))],
            }],
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state.focus_on(Panel::Contents);
    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
    );

    let (widths, available) = grid_columns(&state).expect("a page is loaded");
    assert_eq!(widths.as_ref(), [6, available - 6 - grid::COLUMN_SPACING]);

    let crate::view::PanelBody::Grid { columns, .. } = crate::view::view_model(&state)
        .panels
        .into_iter()
        .find(|panel| panel.panel == Panel::Contents)
        .expect("a contents panel")
        .body
    else {
        panic!("a grid");
    };
    let drawn: Vec<u16> = columns.iter().map(|column| column.width).collect();
    assert_eq!(drawn, widths.as_ref());

    state.contents.mode = SelectionMode::Row;
    press(&mut state, Key::Char('l'));
    assert_eq!(
        state.contents.column_scroll.first(),
        0,
        "a widened grid still fits, so there is nothing to scroll to"
    );
}

#[test]
fn y_yanks_the_cell_in_cell_mode() {
    let mut state = grid_of_named_cells(3, 2);
    state.contents.mode = SelectionMode::Cell;
    state.contents.cursor.go_to(1, 2);
    state.contents.column.go_to(2, 3);

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(
        effects,
        vec![Effect::Yank("r1c2".to_owned())],
        "cell mode yanks the value the two cursors name, and no delimiter"
    );
}

/// The panel opens in row mode, so this is what `y` does by default
/// (design §1).
#[test]
fn y_yanks_the_row_in_row_mode() {
    let mut state = grid_of_named_cells(3, 2);
    state.contents.cursor.go_to(1, 2);
    state.contents.column.go_to(2, 3);

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(
        effects,
        vec![Effect::Yank("r1c0,r1c1,r1c2".to_owned())],
        "row mode yanks every column of the row the row cursor names"
    );
}

#[test]
fn y_yanks_the_column_in_column_mode() {
    let mut state = grid_of_named_cells(3, 2);
    state.contents.mode = SelectionMode::Column;
    state.contents.cursor.go_to(1, 2);
    state.contents.column.go_to(2, 3);

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(
        effects,
        vec![Effect::Yank("r0c2,r1c2".to_owned())],
        "column mode yanks every row of the column the column cursor names"
    );
}

/// `y` is one gesture: it copies, and then it puts the panel back where
/// the user started it from (visual mode design §5.1).
#[test]
fn y_drops_the_selection_and_sends_the_cursor_back_to_its_anchor() {
    let mut state = grid_of_named_cells(3, 4);
    state.contents.cursor.go_to(1, 4);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('y'));

    assert_eq!(state.contents.rows, Selection::default());
    assert_eq!(
        state.contents.cursor.index(),
        1,
        "the cursor comes home to where `v` was pressed"
    );
}

/// The anchor, not the top of the range: a selection extended upwards
/// started at its *last* row, and that is where the cursor came from.
#[test]
fn a_selection_grown_upwards_sends_the_cursor_back_down() {
    let mut state = grid_of_named_cells(3, 4);
    state.contents.cursor.go_to(3, 4);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('k'));
    press(&mut state, Key::Char('k'));

    press(&mut state, Key::Char('y'));

    assert_eq!(state.contents.cursor.index(), 3);
}

/// Frozen by a second `v`, so the group is no longer extending and the
/// cursor has already left the anchor by the time `y` asks for it.
#[test]
fn a_frozen_range_still_knows_where_to_send_the_cursor() {
    let mut state = grid_of_named_cells(3, 4);
    state.contents.cursor.go_to(1, 4);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('y'));

    assert_eq!(state.contents.rows, Selection::default());
    assert_eq!(state.contents.cursor.index(), 1);
}

#[test]
fn cell_mode_sends_both_cursors_home() {
    let mut state = grid_of_named_cells(3, 4);
    state.contents.mode = SelectionMode::Cell;
    state.contents.cursor.go_to(1, 4);
    state.contents.column.go_to(1, 3);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('l'));

    press(&mut state, Key::Char('y'));

    assert_eq!(state.contents.rows, Selection::default());
    assert_eq!(state.contents.columns, Selection::default());
    assert_eq!(
        (state.contents.cursor.index(), state.contents.column.index()),
        (1, 1),
        "a rectangle has an anchor on each axis and both are homes"
    );
}

#[test]
fn the_tree_collapses_its_selection_on_y_too() {
    let mut state = browsing(&["a", "b", "c"]);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('y'));

    assert_eq!(state.tables.selection, Selection::default());
    assert_eq!(state.tables.cursor().index(), 0);
}

/// Nothing yanked is nothing said, and a `y` that said nothing must not
/// move the cursor or throw the selection away either (visual mode design §5.1).
#[test]
fn a_y_that_yanks_nothing_leaves_the_selection_alone() {
    let mut state = measured(&["a"]);
    state.focus_on(Panel::Queries);
    state.contents.cursor.go_to(0, 1);
    state.tables.selection.start(0);

    let effects = press(&mut state, Key::Char('y'));

    assert!(effects.is_empty());
    let mut extending_from_zero = Selection::default();
    extending_from_zero.start(0);
    assert_eq!(
        state.tables.selection, extending_from_zero,
        "a panel that yanks nothing has nothing to reset"
    );
}

/// The selection stops being drawn on `y`, so without this the copy would
/// leave no mark at all and the user would have nothing but the absence of
/// a range to tell them it happened (visual mode design §6.1).
#[test]
fn y_lights_up_what_it_yanked() {
    let mut state = grid_of_named_cells(3, 4);
    state.contents.mode = SelectionMode::Cell;
    state.contents.cursor.go_to(1, 4);
    state.contents.column.go_to(0, 3);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('l'));

    press(&mut state, Key::Char('y'));

    let flash = state.flash.expect("the yank lit what it copied");
    assert_eq!(flash.panel(), Panel::Contents);
    assert_eq!(flash.rows(), &Ranges::of(Range::new(1, 2)));
    assert_eq!(flash.columns(), &Ranges::of(Range::new(0, 1)));
}

/// Empty on an axis is every row or every column of it — the convention
/// the view model's ranges already carry, and what row mode yanks.
#[test]
fn row_mode_lights_the_whole_row_and_column_mode_the_whole_column() {
    let mut rows = grid_of_named_cells(3, 4);
    press(&mut rows, Key::Char('y'));

    let flash = rows.flash.expect("lit");
    assert_eq!(flash.rows(), &Ranges::of(Range::new(0, 0)));
    assert_eq!(
        flash.columns(),
        &Ranges::default(),
        "row mode yanks every column"
    );

    let mut columns = grid_of_named_cells(3, 4);
    columns.contents.mode = SelectionMode::Column;
    press(&mut columns, Key::Char('y'));

    let flash = columns.flash.expect("lit");
    assert_eq!(
        flash.rows(),
        &Ranges::default(),
        "column mode yanks every row"
    );
    assert_eq!(flash.columns(), &Ranges::of(Range::new(0, 0)));
}

/// Nothing was selected, so what the flash marks is the one row `y` took —
/// a bare `y` earns the same feedback as a ranged one.
#[test]
fn a_bare_y_lights_the_row_it_took() {
    let mut state = browsing(&["a", "b"]);

    press(&mut state, Key::Char('y'));

    let flash = state.flash.expect("lit");
    assert_eq!(flash.panel(), Panel::Tables);
    assert_eq!(flash.rows(), &Ranges::of(Range::new(0, 0)));
}

#[test]
fn a_y_that_yanks_nothing_lights_nothing() {
    let mut state = measured(&["a"]);
    state.focus_on(Panel::Queries);

    press(&mut state, Key::Char('y'));

    assert_eq!(state.flash, None);
}

#[test]
fn the_flash_fades_and_only_the_tick_that_ends_it_costs_a_repaint() {
    let mut state = browsing(&["a", "b"]);
    press(&mut state, Key::Char('y'));

    for tick in 1..FLASH_TICKS {
        state.dirty = false;
        update(&mut state, Msg::Tick(Stamp::default()));
        assert!(
            state.flash.is_some() && !state.dirty,
            "tick {tick} neither ends the flash nor changes the frame"
        );
    }

    state.dirty = false;
    update(&mut state, Msg::Tick(Stamp::default()));

    assert_eq!(state.flash, None);
    assert!(
        state.dirty,
        "the frame that loses the flash has to be drawn"
    );
}

/// The flash is a moment, and the next key is the end of it — otherwise a
/// fresh `v` would contend with a fading one for the same band.
#[test]
fn the_next_key_ends_the_flash() {
    let mut state = browsing(&["a", "b"]);
    press(&mut state, Key::Char('y'));

    press(&mut state, Key::Char('j'));

    assert_eq!(state.flash, None);
}

/// An empty OSC 52 payload asks the terminal to clear its clipboard, so a
/// `y` with nothing to say must say nothing (design §4).
#[test]
fn y_on_a_null_cell_raises_no_effect() {
    let mut state = browsing_a_table();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");
    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page {
                columns: vec!["id".to_owned()],
                rows: vec![Row {
                    values: vec![CqlValue::Null],
                }],
                next: None,
            }),
            took: Duration::ZERO,
        },
    );

    let effects = press(&mut state, Key::Char('y'));

    assert!(
        effects.is_empty(),
        "a null has no text, and an empty yank would wipe the clipboard"
    );
}

#[test]
fn y_in_column_mode_on_a_page_with_no_rows_yanks_nothing() {
    let mut state = browsing_a_table();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");
    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page {
                columns: vec!["id".to_owned()],
                rows: Vec::new(),
                next: None,
            }),
            took: Duration::ZERO,
        },
    );
    state.contents.mode = SelectionMode::Column;

    let effects = press(&mut state, Key::Char('y'));

    assert!(effects.is_empty(), "an empty column is nothing to yank");
}

#[test]
fn y_in_column_mode_on_a_column_of_one_null_yanks_nothing() {
    let mut state = browsing_a_table();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");
    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page {
                columns: vec!["id".to_owned()],
                rows: vec![Row {
                    values: vec![CqlValue::Null],
                }],
                next: None,
            }),
            took: Duration::ZERO,
        },
    );
    state.contents.mode = SelectionMode::Column;

    let effects = press(&mut state, Key::Char('y'));

    assert!(
        effects.is_empty(),
        "a lone null field is an empty line, which would wipe the clipboard"
    );
}

/// An empty text is a value Cassandra holds and a null is not, but neither
/// has anything to put on the clipboard (design §4).
#[test]
fn y_on_a_cell_holding_the_empty_string_raises_no_effect() {
    let mut state = browsing_a_table();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");
    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page {
                columns: vec!["name".to_owned()],
                rows: vec![Row {
                    values: vec![CqlValue::Text(String::new())],
                }],
                next: None,
            }),
            took: Duration::ZERO,
        },
    );

    let effects = press(&mut state, Key::Char('y'));

    assert!(
        effects.is_empty(),
        "an empty payload would ask the terminal to clear its clipboard"
    );
}

/// What is drawn is a window; what is yanked is the column (design §6).
#[test]
fn a_column_yank_carries_rows_the_viewport_hides() {
    let mut state = grid_of_rows(1, 20, 80);
    assert!(
        state.geometry.height(Panel::Contents) < state.contents.row_count(),
        "the viewport must hide at least one row for this test to prove anything"
    );

    state.contents.mode = SelectionMode::Column;

    let effects = press(&mut state, Key::Char('y'));

    let Some(Effect::Yank(line)) = effects.first() else {
        panic!("a loaded column yanks, got {effects:?}");
    };
    assert_eq!(
        line.matches(',').count(),
        19,
        "twenty rows make nineteen separators, however few are on screen"
    );
}

#[test]
fn y_on_an_empty_panel_yanks_nothing() {
    let mut state = browsing_a_table();
    state.focus_on(Panel::Contents);

    assert!(press(&mut state, Key::Char('y')).is_empty());
    assert!(press(&mut state, Key::Char('Y')).is_empty());
}

#[test]
fn y_yanks_nothing_when_the_search_kept_no_row() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.apply_search(Searched::Tables, Search::new("zzz"));

    assert!(
        press(&mut state, Key::Char('y')).is_empty(),
        "the tree is empty, so there is no row under the cursor to yank"
    );
}

#[test]
fn y_yanks_a_ddl_row() {
    let mut state = browsing_a_table();
    state.focus_on(Panel::Ddl);

    assert_eq!(
        press(&mut state, Key::Char('y')),
        vec![Effect::Yank("Keyspace,shop".to_owned())]
    );
}

#[test]
fn y_yanks_a_column_row() {
    let mut state = browsing_a_table();
    press(&mut state, Key::Char('2'));
    press(&mut state, Key::Char('2'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    assert_eq!(
        press(&mut state, Key::Char('y')),
        vec![Effect::Yank("id,uuid,partition".to_owned())]
    );

    press(&mut state, Key::Char('j'));

    assert_eq!(
        press(&mut state, Key::Char('y')),
        vec![Effect::Yank("created_at,timestamp,clustering".to_owned())]
    );
}

#[test]
fn y_yanks_a_tree_node() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);

    assert_eq!(
        press(&mut state, Key::Char('y')),
        vec![Effect::Yank("shop".to_owned())]
    );

    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('j'));

    assert_eq!(
        press(&mut state, Key::Char('y')),
        vec![Effect::Yank("shop.orders".to_owned())]
    );
}

#[test]
fn y_yanks_every_row_of_a_range_one_per_line() {
    let mut state = grid_of_named_cells(2, 3);
    state.contents.mode = SelectionMode::Row;
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(yanked(&effects), Some("r0c0,r0c1\nr1c0,r1c1"));
}

#[test]
fn y_in_cell_mode_yanks_the_block_and_nothing_outside_it() {
    let mut state = grid_of_named_cells(3, 3);
    state.contents.mode = SelectionMode::Cell;
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('l'));

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(yanked(&effects), Some("r0c0,r0c1\nr1c0,r1c1"));
}

#[test]
fn y_on_a_single_cell_still_yanks_it_bare() {
    let mut state = grid_of_named_cells(2, 2);
    state.contents.mode = SelectionMode::Cell;
    press(&mut state, Key::Char('v'));

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(
        yanked(&effects),
        Some("r0c0"),
        "one cell is not a range; adding quotes would put back the delimiter there is none of"
    );
}

#[test]
fn y_in_column_mode_yanks_one_line_per_column() {
    let mut state = grid_of_named_cells(3, 2);
    state.contents.mode = SelectionMode::Column;
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('l'));

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(yanked(&effects), Some("r0c0,r1c0\nr0c1,r1c1"));
}

#[test]
fn y_yanks_every_node_of_a_tree_range() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(yanked(&effects), Some("shop\nshop.orders\nshop.users"));
}

#[test]
fn y_over_two_groups_yanks_their_rows_in_drawn_order() {
    let mut state = tables_state();
    state.focus_on(Panel::Tables);
    state.tables.selection.start(2);
    state.tables.selection.stop(2);
    state.tables.selection.start(0);
    state.tables.selection.stop(0);

    let effects = press(&mut state, Key::Char('y'));

    let Some(Effect::Yank(payload)) = effects.into_iter().next() else {
        panic!("y raised no yank");
    };

    assert_eq!(
        payload, "shop\nshop.products",
        "row order, not insertion order (design §5)"
    );
}

#[test]
fn y_sends_the_cursor_home_to_the_last_v_and_spends_every_group() {
    // The last press is where the user last was, and it is the only one of the
    // three candidate rows still true after a group has been dropped from the
    // middle (design §3).
    let mut state = tables_state();
    state.focus_on(Panel::Tables);
    state.tables.selection.start(0);
    state.tables.selection.stop(1);
    state.tables.selection.start(3);
    state.tables.selection.stop(3);
    state.move_tree_cursor(3);

    let _ = press(&mut state, Key::Char('y'));

    assert_eq!(state.tables.cursor().index(), 3, "home is the last v");
    assert!(
        !state.tables.selection.is_active(),
        "a yank spends every group, not just the last"
    );
}

#[test]
fn y_over_a_selection_whose_anchor_was_dropped_moves_no_cursor() {
    let mut state = tables_state();
    state.focus_on(Panel::Tables);
    // Two groups; the second one's row is where `v` was last pressed, and
    // dropping it takes the anchor with it. The first group still stands.
    state.tables.selection.toggle(0);
    state.tables.selection.toggle(1);
    state.tables.selection.toggle(3);
    state.tables.selection.toggle(3);
    state.tables.selection.toggle(3);
    state.move_tree_cursor(1);

    assert_eq!(state.tables.selection.anchor(), None);

    let _ = press(&mut state, Key::Char('y'));

    assert_eq!(
        state.tables.cursor().index(),
        1,
        "there is no row the anchor could send it to that the user chose"
    );
    assert!(
        !state.tables.selection.is_active(),
        "a yank spends the selection even when it has no anchor"
    );
}

#[test]
fn a_single_cell_in_one_group_each_way_still_yanks_bare() {
    // "One" is now "one row and one column in one group each"; the bare,
    // unescaped value must survive that rewording (design §5).
    let mut state = contents_state();
    state.focus_on(Panel::Contents);
    state.contents.mode = SelectionMode::Cell;

    let effects = press(&mut state, Key::Char('y'));

    let Some(Effect::Yank(payload)) = effects.into_iter().next() else {
        panic!("y raised no yank");
    };

    assert!(!payload.contains(','), "a bare cell takes no delimiter");
}

#[test]
fn a_payload_of_nothing_but_delimiters_reaches_no_clipboard() {
    // `grid_of_rows` fills its cells with `Int(1)`; this needs nulls, so
    // the page is built here rather than borrowed from it.
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
            columns: vec!["a".to_owned(), "b".to_owned()],
            rows: vec![
                Row {
                    values: vec![CqlValue::Null, CqlValue::Null],
                };
                2
            ],
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state.focus_on(Panel::Contents);
    state.contents.mode = SelectionMode::Row;
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    let effects = press(&mut state, Key::Char('y'));

    assert!(
        effects.is_empty(),
        "a block of nulls says nothing, and an empty payload clears the clipboard"
    );
}
