use super::*;

#[test]
fn a_loaded_schema_pulls_the_selection_back_inside_it() {
    let mut state = connected(Posture::ReadOnly);
    let conn = current_conn(&state);
    for _ in 0..5 {
        state.tables.cursor_mut().down(6);
    }

    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(schema_of(&["shop", "typezoo"])),
            took: Duration::ZERO,
        },
    );

    assert_eq!(
        state.tables.cursor().index(),
        1,
        "a selection past the end of a smaller schema must be pulled in"
    );
}

#[test]
fn enter_toggles_a_keyspace() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

    let effects = press(&mut state, Key::Enter);
    assert_eq!(row_count(&state), 3, "⏎ did not unfold");
    assert!(
        effects.is_empty(),
        "unfolding is a pure state change and must emit no effect"
    );

    let effects = press(&mut state, Key::Enter);
    assert_eq!(row_count(&state), 1, "⏎ did not fold");
    assert!(
        effects.is_empty(),
        "folding is a pure state change and must emit no effect"
    );
}

#[test]
fn enter_is_inert_while_more_than_one_row_is_selected() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    let effects = press(&mut state, Key::Enter);

    assert!(
        effects.is_empty(),
        "a range names more than one thing to open"
    );
    assert_eq!(state.focus(), Panel::Tables, "and focus stays put");
}

#[test]
fn enter_still_reads_a_table_with_one_row_selected() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('v'));

    let effects = press(&mut state, Key::Enter);

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ExecutePaged { .. })),
        "pressing v must not blank the panel before anything has been extended"
    );
}

#[test]
fn a_click_clears_the_range_it_lands_beside() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('v'));

    update(
        &mut state,
        Msg::Pointer(Pointer::Click(Click::Row {
            panel: Panel::Tables,
            row: 2,
            column: None,
        })),
    );

    assert!(
        !state.tables.selection.is_active(),
        "pointing at one row is saying: this one"
    );
    assert_eq!(state.tables.cursor().index(), 2);
}

#[test]
fn space_is_not_a_second_name_for_enter() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

    let effects = press(&mut state, Key::Char(' '));

    assert_eq!(row_count(&state), 1, "space unfolded a keyspace");
    assert!(effects.is_empty(), "space read a table");
}

#[test]
fn l_and_right_open_a_keyspace() {
    for key in [Key::Char('l'), Key::Right] {
        let mut state = browsing_tables(&[("shop", &["orders"])]);

        let effects = press(&mut state, key);

        assert_eq!(row_count(&state), 2, "{key:?} did not open");
        assert!(
            effects.is_empty(),
            "opening is a pure state change and must emit no effect"
        );
    }
}

#[test]
fn h_and_left_close_a_keyspace() {
    for key in [Key::Char('h'), Key::Left] {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));

        let effects = press(&mut state, key);

        assert_eq!(row_count(&state), 1, "{key:?} did not close");
        assert!(
            effects.is_empty(),
            "closing is a pure state change and must emit no effect"
        );
    }
}

#[test]
fn l_and_right_read_the_table_under_the_cursor() {
    for key in [Key::Char('l'), Key::Right] {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, key);
        press(&mut state, key);

        let effects = press(&mut state, key);

        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::ExecutePaged { .. })),
            "{key:?} on a table row did not read it"
        );
        assert_eq!(state.focus(), Panel::Contents, "{key:?}");
    }
}

#[test]
fn l_reads_a_table_under_a_search_the_way_enter_does() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    state.apply_search(Searched::Tables, Search::new("orders"));

    state.move_tree_cursor(1);
    let effects = press(&mut state, Key::Char('l'));

    assert!(
        !effects.is_empty(),
        "only the fold path is swallowed under a search"
    );
}

#[test]
fn l_on_a_table_is_inert_over_a_range_of_many() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    let effects = press(&mut state, Key::Char('l'));

    assert!(
        effects.is_empty(),
        "reading a table needs one table, as it does for enter"
    );
}

#[test]
fn h_from_a_table_steps_out_to_its_keyspace_before_folding() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('l'));
    assert_eq!(state.tables.cursor().index(), 1);

    press(&mut state, Key::Char('h'));

    assert_eq!(state.tables.cursor().index(), 0);
    assert_eq!(row_count(&state), 3, "stepping out must not fold");
}

#[test]
fn enter_on_a_table_leaves_the_tree_where_it_is() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('l'));
    let before = state.tables.clone();

    press(&mut state, Key::Enter);

    assert_eq!(
        state.tables, before,
        "reading a table must not disturb the tree the cursor is in"
    );
}

#[test]
fn j_walks_into_an_unfolded_keyspaces_tables() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"]), ("typezoo", &[])]);
    press(&mut state, Key::Char('l'));

    for _ in 0..3 {
        press(&mut state, Key::Char('j'));
    }

    assert_eq!(
        state.tables.cursor().index(),
        3,
        "motion counts tree rows, not keyspaces"
    );
}

#[test]
fn a_tree_transition_belongs_to_the_focused_panel_only() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.focus_on(Panel::Ddl);

    press(&mut state, Key::Char('l'));

    assert_eq!(row_count(&state), 1);
}

#[test]
fn l_and_h_are_swallowed_while_a_search_is_applied() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    state.apply_search(Searched::Tables, Search::new("shop"));
    let before = row_count(&state);

    press(&mut state, Key::Char('h'));
    press(&mut state, Key::Char('l'));

    assert_eq!(
        row_count(&state),
        before,
        "a search forces its matches open, so folding could only look broken"
    );
}

#[test]
fn folding_under_a_search_does_not_disturb_the_fold_state_it_will_return_to() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    assert_eq!(row_count(&state), 3, "shop starts unfolded");

    state.apply_search(Searched::Tables, Search::new("shop"));
    press(&mut state, Key::Char('h'));
    press(&mut state, Key::Char('l'));
    state.apply_search(Searched::Tables, None);

    assert_eq!(
        row_count(&state),
        3,
        "the fold state from before the search must survive it untouched"
    );
}

/// `activate` has no search guard of its own; it inherits
/// `TablesPanel::toggle`'s. Without it, `Enter` here folds `shop` in the real
/// unfold set.
#[test]
fn no_tables_entry_point_writes_the_fold_state_under_a_search() {
    enum Action {
        Key(Key),
        Click,
    }

    for action in [
        Action::Key(Key::Char('h')),
        Action::Key(Key::Char('l')),
        Action::Key(Key::Enter),
        Action::Key(Key::Char(' ')),
        Action::Click,
    ] {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        assert_eq!(row_count(&state), 3, "shop starts unfolded");

        state.apply_search(Searched::Tables, Search::new("shop"));

        match action {
            Action::Key(key) => {
                press(&mut state, key);
            }
            Action::Click => {
                click_row(&mut state, Panel::Tables, 0);
            }
        }

        state.apply_search(Searched::Tables, None);

        assert_eq!(
            row_count(&state),
            3,
            "the fold state from before the search must survive it untouched"
        );
    }
}

#[test]
fn enter_still_reads_a_table_under_a_search() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    state.apply_search(Searched::Tables, Search::new("orders"));
    assert_eq!(
        row_count(&state),
        2,
        "the keyspace and the one table the term reaches"
    );

    state.move_tree_cursor(1);
    let effects = press(&mut state, Key::Enter);

    assert!(
        !effects.is_empty(),
        "Enter on a table row under a search must still submit a read"
    );
}

#[test]
fn l_and_h_still_fold_once_the_search_is_gone() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

    press(&mut state, Key::Char('l'));

    assert_eq!(row_count(&state), 3);
}
