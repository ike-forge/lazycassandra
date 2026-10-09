use super::*;

#[test]
fn a_frame_that_makes_room_for_the_palette_repaints_the_bar() {
    let mut state = State::default();
    assert_eq!(
        state.geometry.command_rows(),
        0,
        "the default geometry is where `?` starts out inert"
    );

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 14)),
    );

    assert!(
        state.dirty,
        "command_rows went from zero to non-zero, so the bar's `? commands` \
         hint just appeared and nothing scheduled the repaint that shows it"
    );
}

#[test]
fn j_and_k_walk_the_keyspace_list() {
    let mut state = browsing(&["a", "b", "c"]);

    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    assert_eq!(state.tables.cursor.index(), 2);

    press(&mut state, Key::Char('k'));

    assert_eq!(state.tables.cursor.index(), 1);
}

#[test]
fn the_arrow_keys_do_what_j_and_k_do() {
    let mut state = browsing(&["a", "b"]);

    press(&mut state, Key::Down);

    assert_eq!(state.tables.cursor.index(), 1);

    press(&mut state, Key::Up);

    assert_eq!(state.tables.cursor.index(), 0);
}

#[test]
fn the_selection_cannot_run_off_a_list_it_has_not_loaded() {
    let mut state = connected(Posture::ReadOnly);

    press(&mut state, Key::Char('j'));

    assert_eq!(
        state.tables.cursor.index(),
        0,
        "there are no rows to move onto"
    );
}

#[test]
fn motion_belongs_to_the_focused_panel_only() {
    let mut state = browsing(&["a", "b", "c"]);
    state.focus_on(Panel::Ddl);

    press(&mut state, Key::Char('j'));

    assert_eq!(state.tables.cursor.index(), 0);
}

#[test]
fn escape_dismisses_the_overlay() {
    let mut state = browsing(&["a"]);
    state.overlay = Some(Overlay::Message(Message {
        title: "Schema unavailable".to_owned(),
        message: "boom".to_owned(),
        severity: Severity::Danger,
    }));

    press(&mut state, Key::Escape);

    assert!(state.overlay.is_none());
}

#[test]
fn q_quits() {
    let mut state = browsing(&["a"]);

    let effects = press(&mut state, Key::Char('q'));

    assert!(state.should_quit);
    assert_eq!(effects, vec![Effect::Quit]);
}

#[test]
fn tab_from_queries_cql_lands_on_queries_logs() {
    let mut state = State::default();
    state.focus_on(Panel::Queries);
    assert_eq!(state.queries.tab(), QueriesTab::Cql);

    press(&mut state, Key::Tab);

    assert_eq!(
        state.focus(),
        Panel::Queries,
        "the cycle has not left the panel yet"
    );
    assert_eq!(state.queries.tab(), QueriesTab::Logs);
}

#[test]
fn tab_from_queries_logs_lands_on_tables_and_reverts_to_cql() {
    let mut state = on_the_trail();

    press(&mut state, Key::Tab);

    assert_eq!(state.focus(), Panel::Tables);
    assert_eq!(
        state.queries.tab(),
        QueriesTab::Cql,
        "leaving the panel still reverts its tab"
    );
}

#[test]
fn shift_tab_from_tables_lands_on_queries_logs() {
    let mut state = State::default();

    press(&mut state, Key::BackTab);

    assert_eq!(
        state.focus(),
        Panel::Queries,
        "stepping back from the first stop wraps to the last"
    );
    assert_eq!(
        state.queries.tab(),
        QueriesTab::Logs,
        "arrival sets the tab explicitly, or focus_on's revert of the \
         panel it just left would undo it"
    );
}

#[test]
fn tab_steps_ddls_two_tabs_as_separate_stops() {
    let mut state = browsing_a_table();
    state.focus_on(Panel::Ddl);
    assert_eq!(state.ddl.tab(), DdlTab::Ddl);

    press(&mut state, Key::Tab);

    assert_eq!(
        state.focus(),
        Panel::Ddl,
        "the cycle has not left the panel yet"
    );
    assert_eq!(state.ddl.tab(), DdlTab::Columns);
}

#[test]
fn a_full_forward_cycle_returns_to_where_it_started() {
    let mut state = browsing_a_table();
    state.focus_on(Panel::Tables);
    let stops = Landing::all(&state).len();

    for _ in 0..stops {
        press(&mut state, Key::Tab);
    }

    assert_eq!(state.focus(), Panel::Tables);
    assert_eq!(
        state.queries.tab(),
        QueriesTab::Cql,
        "the cycle passed through Queries and came out the other side"
    );
}

#[test]
fn shift_tab_undoes_tab_from_every_stop() {
    let state = browsing_a_table();

    for stop in Landing::all(&state) {
        let mut probe = state.clone();
        stop.arrive(&mut probe);

        press(&mut probe, Key::Tab);
        press(&mut probe, Key::BackTab);

        assert_eq!(
            Landing::current(&probe),
            stop,
            "{stop:?} did not return to itself"
        );
    }
}

#[test]
fn ctrl_d_and_ctrl_u_page_the_focused_panel() {
    let mut paged = measured(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"]);
    let mut chorded = paged.clone();

    press(&mut paged, Key::PageDown);
    ctrl(&mut chorded, Key::Char('d'));

    assert_eq!(chorded.tables.cursor.index(), paged.tables.cursor.index());
    assert!(chorded.tables.cursor.index() > 0, "a page was available");

    press(&mut paged, Key::PageUp);
    ctrl(&mut chorded, Key::Char('u'));

    assert_eq!(chorded.tables.cursor.index(), paged.tables.cursor.index());
}

#[test]
fn ctrl_d_pages_an_open_palette_rather_than_the_panel_underneath() {
    let mut state = measured(&["a"]);
    press(&mut state, Key::Char('?'));

    ctrl(&mut state, Key::Char('d'));

    assert_eq!(
        palette(&state).expect("still open").selected(),
        15,
        "ctrl-d is pgdn by another name, and pgdn pages the palette"
    );
    assert_eq!(state.tables.cursor.index(), 0);
}

#[test]
fn ctrl_d_pages_the_editor_the_way_pgdn_does() {
    let mut paged = editing("a\nb\nc\nd\ne\nf");
    let mut chorded = paged.clone();

    press(&mut paged, Key::PageUp);
    ctrl(&mut chorded, Key::Char('u'));

    assert_eq!(chorded.queries.cursor(), paged.queries.cursor());
    assert_eq!(chorded.queries.text(), paged.queries.text(), "not text");
}

#[test]
fn ctrl_d_is_inert_under_a_search_prompt() {
    let mut state = browsing(&["a", "b", "c", "d", "e", "f", "g", "h"]);
    press(&mut state, Key::Char('/'));
    let before = crate::view::view_model(&state);

    ctrl(&mut state, Key::Char('d'));

    assert_eq!(
        crate::view::view_model(&state),
        before,
        "the prompt pages nothing, and the chord is not text either"
    );
}

#[test]
fn tab_walks_every_stop_in_order_and_wraps() {
    // A keyspace with no tables, so DDL offers its one tab and the
    // stops are exactly the ones docs/shortcuts.md §4 lists: Tables,
    // Ddl, Contents, Queries/Cql, Queries/Logs.
    let mut state = browsing(&["a"]);

    press(&mut state, Key::Tab);
    assert_eq!(state.focus(), Panel::Ddl);

    press(&mut state, Key::Tab);
    assert_eq!(state.focus(), Panel::Contents);

    press(&mut state, Key::Tab);
    assert_eq!(state.focus(), Panel::Queries);
    assert_eq!(state.queries.tab(), QueriesTab::Cql);

    press(&mut state, Key::Tab);
    assert_eq!(state.focus(), Panel::Queries, "Queries' own second stop");
    assert_eq!(state.queries.tab(), QueriesTab::Logs);

    press(&mut state, Key::Tab);
    assert_eq!(state.focus(), Panel::Tables, "the cycle wraps");
}

#[test]
fn shift_tab_walks_the_same_stops_the_other_way() {
    let mut state = browsing(&["a"]);

    press(&mut state, Key::BackTab);
    assert_eq!(
        state.focus(),
        Panel::Queries,
        "stepping back from the first stop wraps to the last"
    );
    assert_eq!(state.queries.tab(), QueriesTab::Logs);

    press(&mut state, Key::BackTab);
    assert_eq!(state.focus(), Panel::Queries);
    assert_eq!(state.queries.tab(), QueriesTab::Cql);

    press(&mut state, Key::BackTab);
    assert_eq!(state.focus(), Panel::Contents);
}

/// Whether a freshly-focused `panel` takes text — the panels a press
/// cannot reach from, now that it depends on the Queries tab rather than
/// on the panel alone.
fn does_not_take_text(panel: Panel) -> bool {
    let mut state = State::default();
    state.focus_on(panel);
    !state.takes_text()
}

#[test]
fn a_number_focuses_its_panel_from_any_panel_that_does_not_take_text() {
    for from in Panel::ALL
        .into_iter()
        .filter(|panel| does_not_take_text(*panel))
    {
        for to in Panel::ALL.into_iter().filter(|panel| *panel != from) {
            let mut state = State::default();
            state.focus_on(from);

            press(&mut state, Key::Char(to.number()));

            assert_eq!(
                state.focus(),
                to,
                "'{}' should reach {to:?} from {from:?}",
                to.number()
            );
        }
    }
}

#[test]
fn a_repeated_number_keeps_focus_and_walks_the_panels_tabs() {
    // Queries numbers each tab instead; see `four_focuses_queries_…`.
    for panel in Panel::ALL
        .into_iter()
        .filter(|panel| does_not_take_text(*panel) && *panel != Panel::Queries)
    {
        // A table has to be selected, or the DDL strip draws one tab and
        // there is nothing to walk to.
        let mut state = browsing_a_table();
        state.focus_on(Panel::Tables);

        press(&mut state, Key::Char(panel.number()));
        press(&mut state, Key::Char(panel.number()));

        assert_eq!(state.focus(), panel, "the second press does not move focus");
        assert_eq!(
            state.ddl.tab(),
            if panel == Panel::Ddl {
                DdlTab::Columns
            } else {
                DdlTab::Ddl
            },
            "only the panel that has tabs answers the second press"
        );
    }
}

#[test]
fn pressing_two_once_per_tab_returns_to_the_first_one() {
    let mut state = browsing_a_table();

    // One press to arrive, then one per tab: the walk wraps whatever the
    // strip holds, so the count follows `DdlTab::ALL` rather than a
    // number a third tab would have made wrong.
    for _ in 0..=DdlTab::ALL.len() {
        press(&mut state, Key::Char('2'));
    }

    assert_eq!(state.focus(), Panel::Ddl);
    assert_eq!(state.ddl.tab(), DdlTab::Ddl);
}

#[test]
fn brackets_walk_the_focused_panels_tabs_both_ways() {
    let mut state = browsing_a_table();
    state.focus_on(Panel::Ddl);

    press(&mut state, Key::Char(']'));
    assert_eq!(state.ddl.tab(), DdlTab::Columns);

    press(&mut state, Key::Char('['));
    assert_eq!(state.ddl.tab(), DdlTab::Ddl);
}

#[test]
fn brackets_do_nothing_from_a_panel_with_no_tabs() {
    for panel in [Panel::Tables, Panel::Contents] {
        let mut state = browsing_a_table();
        state.focus_on(panel);

        press(&mut state, Key::Char(']'));

        assert_eq!(state.focus(), panel, "a bracket moves no focus");
        assert_eq!(
            state.ddl.tab(),
            DdlTab::Ddl,
            "and reaches no panel it is not in"
        );
    }
}

#[test]
fn walking_on_a_keyspace_stays_on_the_only_tab_there_is() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('l'));

    press(&mut state, Key::Char('2'));
    press(&mut state, Key::Char('2'));

    assert_eq!(state.focus(), Panel::Ddl);
    assert_eq!(
        state.ddl.tab(),
        DdlTab::Ddl,
        "a keyspace has no columns, so there is no tab to walk to"
    );
}

#[test]
fn a_number_is_text_in_a_panel_that_takes_text() {
    for to in Panel::ALL {
        let mut state = State::default();
        state.focus_on(Panel::Queries);
        state.queries.start_editing();

        press(&mut state, Key::Char(to.number()));

        assert_eq!(state.focus(), Panel::Queries, "focus is the mode");
        assert_eq!(state.queries.text(), to.number().to_string());
    }
}

#[test]
fn a_bracket_is_text_in_a_panel_that_takes_text() {
    let mut state = State::default();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    press(&mut state, Key::Char(']'));

    assert_eq!(state.focus(), Panel::Queries, "focus is the mode");
    assert_eq!(state.queries.text(), "]");
    assert_eq!(state.ddl.tab(), DdlTab::Ddl, "and no tab changed under it");
}

#[test]
fn four_focuses_queries_with_the_buffer_open() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);

    press(&mut state, Key::Char('4'));
    assert_eq!(state.focus(), Panel::Queries);
    assert!(state.takes_text(), "arriving is ready to type");

    press(&mut state, Key::Char('4'));
    assert_eq!(state.queries.text(), "4", "a second `4` is text");
}

#[test]
fn five_focuses_the_logs_tab_from_anywhere() {
    for from in Panel::ALL {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(from);

        press(&mut state, Key::Char('5'));

        assert_eq!(state.focus(), Panel::Queries, "from {from:?}");
        assert_eq!(state.queries.tab(), QueriesTab::Logs, "from {from:?}");
        assert!(!state.takes_text(), "from {from:?}");
    }
}

#[test]
fn four_from_the_logs_tab_opens_the_buffer() {
    let mut state = on_the_trail();

    press(&mut state, Key::Char('4'));

    assert_eq!(state.queries.tab(), QueriesTab::Cql);
    assert!(state.takes_text());
}

#[test]
fn brackets_walk_queries_tabs_while_the_buffer_is_closed() {
    let mut state = State::default();
    state.focus_on(Panel::Queries);

    press(&mut state, Key::Char(']'));
    assert_eq!(state.queries.tab(), QueriesTab::Logs);

    press(&mut state, Key::Char('['));
    assert_eq!(state.queries.tab(), QueriesTab::Cql);
}

#[test]
fn enter_and_i_open_the_buffer_and_type_nothing() {
    for key in [Key::Enter, Key::Char('i')] {
        let mut state = State::default();
        state.focus_on(Panel::Queries);

        press(&mut state, key);

        assert!(state.takes_text(), "{key:?} opens the buffer");
        assert_eq!(state.queries.text(), "", "{key:?} is not typed");
    }
}

#[test]
fn enter_and_i_open_nothing_on_the_logs_tab() {
    for key in [Key::Enter, Key::Char('i')] {
        let mut state = on_the_trail();

        press(&mut state, key);

        assert_eq!(state.queries.tab(), QueriesTab::Logs, "{key:?}");
        assert!(!state.takes_text(), "{key:?}");
    }
}

#[test]
fn a_closed_buffer_leaves_the_global_keys_their_meaning() {
    let mut state = State::default();
    state.focus_on(Panel::Queries);

    press(&mut state, Key::Char('1'));

    assert_eq!(state.focus(), Panel::Tables);
    assert_eq!(state.queries.text(), "");
}

#[test]
fn leaving_the_panel_closes_the_buffer() {
    let mut state = editing("SELECT");

    press(&mut state, Key::Tab);
    press(&mut state, Key::BackTab);

    assert_eq!(state.focus(), Panel::Queries);
    assert!(!state.takes_text(), "arriving back does not reopen it");

    press(&mut state, Key::Enter);
    state.focus_on(Panel::Tables);
    state.focus_on(Panel::Queries);

    assert!(!state.takes_text());
}

#[test]
fn a_click_on_the_buffer_opens_it() {
    let mut state = State::default();

    point(&mut state, Pointer::Click(Click::Panel(Panel::Queries)));

    assert_eq!(state.focus(), Panel::Queries);
    assert!(state.takes_text());
}

/// Also the collision test: a number that an earlier arm already answers
/// would never reach the focus arm, and the focus assertion would fail.
#[test]
fn a_number_moves_focus_and_does_nothing_else() {
    for panel in Panel::ALL {
        let mut state = browsing(&["a", "b", "c"]);
        press(&mut state, Key::Char('j'));
        let cursor = state.tables.cursor;

        let effects = press(&mut state, Key::Char(panel.number()));

        assert_eq!(state.focus(), panel);
        assert!(
            effects.is_empty(),
            "'{}' issued {effects:?}",
            panel.number()
        );
        assert!(!state.should_quit, "'{}' quit the app", panel.number());
        assert_eq!(
            state.tables.cursor,
            cursor,
            "'{}' disturbed the tree",
            panel.number()
        );
    }
}

#[test]
fn the_letters_the_numbers_replaced_stay_unbound() {
    for freed in ['a', 'b', 'c', 'd', 'u'] {
        // Focus starts elsewhere, so an unmoved focus is evidence rather
        // than the state it began in.
        let elsewhere = Panel::Ddl;
        let mut state = State::default();
        state.focus_on(elsewhere);

        press(&mut state, Key::Char(freed));

        assert_eq!(
            state.focus(),
            elsewhere,
            "'{freed}' must stay free for a binding of its own"
        );
    }
}
