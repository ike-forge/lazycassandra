use super::*;

/// Two keyspaces, one holding `orders`, both folded — so `tree_rows` is
/// 2 without a search and a search that matches forces the right one
/// open.
fn loaded() -> State {
    browsing_tables(&[("shop", &["orders"]), ("billing", &[])])
}

#[test]
fn slash_opens_the_prompt_in_every_panel_that_can_hold_a_search() {
    for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus_on(panel);

        press(&mut state, Key::Char('/'));

        assert_eq!(
            searching(&state).map(Prompt::target),
            Searched::of(panel),
            "{panel:?} must reach a prompt"
        );
    }
}

#[test]
fn slash_is_text_in_queries_and_opens_nothing() {
    let mut state = State::default();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    press(&mut state, Key::Char('/'));

    assert!(state.overlay.is_none(), "the editor claims it first");
    assert_eq!(state.queries.text(), "/");
}

#[test]
fn q_and_the_question_mark_type_themselves_in_the_prompt() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Char('q'));
    press(&mut state, Key::Char('?'));

    assert!(!state.should_quit, "q is text while a prompt is up");
    assert_eq!(searching(&state).map(Prompt::query), Some("q?"));
}

#[test]
fn the_prompt_searches_live_per_keystroke() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Char('u'));

    assert_eq!(
        state.search(Searched::Tables).map(Search::as_str),
        Some("u"),
        "the panel underneath is the preview"
    );
}

#[test]
fn backspace_narrows_the_query_and_re_applies_it() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('o'));
    press(&mut state, Key::Char('z'));

    press(&mut state, Key::Backspace);

    assert_eq!(searching(&state).map(Prompt::query), Some("o"));
    assert_eq!(
        state.search(Searched::Tables).map(Search::as_str),
        Some("o")
    );
}

#[test]
fn backspace_on_an_empty_query_is_inert_rather_than_closing() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Backspace);

    assert_eq!(searching(&state).map(Prompt::query), Some(""));
}

#[test]
fn enter_commits_and_closes_leaving_the_search_applied() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('o'));

    press(&mut state, Key::Enter);

    assert!(state.overlay.is_none());
    assert_eq!(
        state.search(Searched::Tables).map(Search::as_str),
        Some("o")
    );
}

#[test]
fn an_empty_commit_clears_the_search() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.apply_search(Searched::Tables, Search::new("o"));
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Enter);

    assert!(state.overlay.is_none());
    assert_eq!(state.search(Searched::Tables), None);
}

/// The design's own mechanism for an empty commit: `Search::new("")` is
/// `None`, so backspacing a live query back to nothing already clears the
/// search before `Enter` is ever pressed — unlike
/// `an_empty_commit_clears_the_search` above, whose buffer is empty
/// because nothing was ever typed into it.
#[test]
fn backspacing_a_query_to_empty_clears_the_search_live() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.apply_search(Searched::Tables, Search::new("o"));
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('o'));

    press(&mut state, Key::Backspace);

    assert_eq!(
        state.search(Searched::Tables),
        None,
        "the buffer is empty, so the live preview already has no search"
    );

    press(&mut state, Key::Enter);

    assert!(state.overlay.is_none());
    assert_eq!(state.search(Searched::Tables), None);
}

#[test]
fn typing_a_term_selects_the_first_match() {
    let mut state = loaded();
    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('/'));

    for character in "orders".chars() {
        press(&mut state, Key::Char(character));
    }

    let schema = state.schema().expect("schema loaded");
    let Some(Node::Table { table, .. }) = state.tables.selected(schema) else {
        panic!("the cursor must be on the matching table");
    };
    assert!(table.name.contains("orders"));
}

#[test]
fn extending_a_term_does_not_walk_the_cursor_forward() {
    let mut state = loaded();
    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Char('o'));
    let after_one = state.tables.cursor.index();
    press(&mut state, Key::Backspace);
    press(&mut state, Key::Char('o'));

    assert_eq!(
        state.tables.cursor.index(),
        after_one,
        "every keystroke searches from where / was pressed, not from the last landing"
    );
}

#[test]
fn a_term_that_matches_nothing_leaves_the_cursor_on_the_empty_tree() {
    let mut state = loaded();
    state.focus_on(Panel::Tables);
    state.tables.cursor.go_to(1, tree_rows(&state));
    press(&mut state, Key::Char('/'));

    for character in "zzz".chars() {
        press(&mut state, Key::Char(character));
    }

    assert_eq!(tree_rows(&state), 0, "no keyspace survives the term");
    assert_eq!(
        state.tables.cursor.index(),
        0,
        "there is no row 1 left to sit on; Esc is what puts the cursor back"
    );
}

#[test]
fn escape_puts_the_cursor_back_on_the_row_a_matchless_term_emptied_out() {
    let mut state = loaded();
    state.focus_on(Panel::Tables);
    state.tables.cursor.go_to(1, tree_rows(&state));
    press(&mut state, Key::Char('/'));
    for character in "zzz".chars() {
        press(&mut state, Key::Char(character));
    }

    press(&mut state, Key::Escape);

    assert_eq!(state.tables.cursor.index(), 1);
}

#[test]
fn a_term_extended_past_its_last_match_restores_the_cursor_to_origin() {
    // Regression for the reviewer's repro: a term that jumps the cursor
    // and then stops matching must snap back to `origin`, not strand the
    // cursor at the last row it landed on (design §5.2).
    let mut state = browsing_tables(&[
        ("aaa", &[]),
        ("bbb", &[]),
        ("ccc", &[]),
        ("ddd", &[]),
        ("orders_ks", &[]),
    ]);
    state.focus_on(Panel::Tables);
    state.tables.cursor.go_to(1, tree_rows(&state));
    press(&mut state, Key::Char('/'));

    for character in "orders".chars() {
        press(&mut state, Key::Char(character));
    }
    assert_eq!(
        state.tables.cursor.index(),
        0,
        "orders_ks is the one keyspace the term keeps, so it is the only row"
    );

    press(&mut state, Key::Char('x'));

    assert_eq!(tree_rows(&state), 0, "ordersx keeps nothing");
    assert_eq!(state.tables.cursor.index(), 0);

    press(&mut state, Key::Escape);

    assert_eq!(
        state.tables.cursor.index(),
        1,
        "the abandoned prompt leaves the panel on the row it opened on"
    );
}

#[test]
fn backspace_to_empty_restores_the_cursor_to_origin() {
    let mut state = browsing_tables(&[
        ("aaa", &[]),
        ("bbb", &[]),
        ("ccc", &[]),
        ("ddd", &[]),
        ("orders_ks", &[]),
    ]);
    state.focus_on(Panel::Tables);
    state.tables.cursor.go_to(1, tree_rows(&state));
    press(&mut state, Key::Char('/'));

    for character in "ordersx".chars() {
        press(&mut state, Key::Char(character));
    }
    assert_eq!(tree_rows(&state), 0, "ordersx matches nothing");

    for _ in 0.."ordersx".chars().count() {
        press(&mut state, Key::Backspace);
    }

    assert_eq!(state.search(Searched::Tables), None);
    assert_eq!(
        state.tables.cursor.index(),
        1,
        "the buffer is empty, but origin is still where the cursor belongs"
    );
}

#[test]
fn escape_restores_the_search_and_the_cursor_together() {
    let mut state = loaded();
    state.focus_on(Panel::Tables);
    state.tables.cursor.go_to(1, tree_rows(&state));
    press(&mut state, Key::Char('/'));
    for character in "orders".chars() {
        press(&mut state, Key::Char(character));
    }

    press(&mut state, Key::Escape);

    assert_eq!(state.tables.cursor.index(), 1, "the cursor goes back too");
    assert_eq!(state.search(Searched::Tables), None);
}

#[test]
fn enter_keeps_where_the_cursor_landed() {
    let mut state = loaded();
    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('/'));
    for character in "orders".chars() {
        press(&mut state, Key::Char(character));
    }
    let landed = state.tables.cursor.index();

    press(&mut state, Key::Enter);

    assert_eq!(state.tables.cursor.index(), landed);
}

#[test]
fn n_walks_to_the_next_match_and_wraps() {
    let mut state = loaded();
    state.focus_on(Panel::Tables);
    // "o" rather than "orders": "billing" carries neither letter and is
    // dropped, leaving the "shop" keyspace (row 0) and the "orders"
    // table it holds (row 1), both matches. Exact indices, not a
    // before/after comparison, so the second press pins the wrap rather
    // than merely staying in bounds by accident.
    state.apply_search(Searched::Tables, Search::new("o"));

    press(&mut state, Key::Char('n'));
    assert_eq!(
        state.tables.cursor.index(),
        1,
        "\"orders\" is the first match after row 0"
    );

    press(&mut state, Key::Char('n'));
    assert_eq!(
        state.tables.cursor.index(),
        0,
        "the last match wraps back to \"shop\""
    );
}

#[test]
fn shift_n_walks_backwards() {
    // Two matches is not enough to tell `n` from `N` apart: with only
    // one other match in the ring, walking either direction from it
    // lands on that same other match. A third row is what makes forward
    // and backward diverge, so "orders" (one match) and "o" against
    // `loaded()` (two matches, but the ring itself is three rows with
    // only two of them matching) both fail to pin this — `box` is
    // added so every row of a three-row ring matches "o" and the
    // starting row genuinely has a different match on each side of it.
    let mut state = browsing_tables(&[("box", &[]), ("shop", &["orders"])]);
    state.focus_on(Panel::Tables);
    state.apply_search(Searched::Tables, Search::new("o"));
    // Rows, alphabetical: "box" (0), "shop" (1, forced open by its own
    // match), "orders" (2) — every one of the three matches "o".
    state.tables.cursor.go_to(1, tree_rows(&state));

    press(&mut state, Key::Char('n'));
    assert_eq!(
        state.tables.cursor.index(),
        2,
        "n from the middle row is the match after it"
    );

    state.tables.cursor.go_to(1, tree_rows(&state));
    press(&mut state, Key::Char('N'));
    assert_eq!(
        state.tables.cursor.index(),
        0,
        "N from the same row is the match before it, not a synonym for n"
    );
}

#[test]
fn n_is_inert_where_no_search_is_applied() {
    let mut state = loaded();
    state.focus_on(Panel::Tables);
    let before = state.tables.cursor.index();

    press(&mut state, Key::Char('n'));

    assert_eq!(state.tables.cursor.index(), before);
}

#[test]
fn n_types_a_character_in_queries() {
    let mut state = loaded();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    press(&mut state, Key::Char('n'));

    assert!(
        state.queries.text().contains('n'),
        "a panel that takes text takes it"
    );
}

#[test]
fn a_live_range_follows_an_n() {
    // Same three-row ring as `shift_n_walks_backwards`: every row
    // matches "o", so `n` from the middle genuinely moves the cursor and
    // an exact-index assertion can tell a real jump from a bare `v`
    // (which the `.ranges().is_empty()` version this replaces could not).
    let mut state = browsing_tables(&[("box", &[]), ("shop", &["orders"])]);
    state.focus_on(Panel::Tables);
    state.apply_search(Searched::Tables, Search::new("o"));
    state.tables.cursor.go_to(1, tree_rows(&state));

    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('n'));

    assert_eq!(state.tables.cursor.index(), 2, "n landed on the next match");
    assert_eq!(
        state.tables.selection.ranges(state.tables.cursor.index()),
        Ranges::of(Range::new(1, 2)),
        "the range runs from the anchor n was pressed on to where it landed"
    );
}

#[test]
fn escape_cancels_the_prompt_restoring_the_search_that_was_there() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    state.apply_search(Searched::Tables, Search::new("orders"));
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('u'));

    press(&mut state, Key::Escape);

    assert!(state.overlay.is_none());
    assert_eq!(
        state.search(Searched::Tables).map(Search::as_str),
        Some("orders"),
        "an abandoned prompt leaves the panel as it was found"
    );
}

#[test]
fn escape_clears_the_focused_panels_search_when_no_overlay_is_up() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.focus_on(Panel::Contents);
    state.apply_search(Searched::Contents, Search::new("o"));

    press(&mut state, Key::Escape);

    assert_eq!(state.search(Searched::Contents), None);
}

#[test]
fn escape_prefers_the_overlay_to_the_search() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.apply_search(Searched::Tables, Search::new("o"));
    state.overlay = Some(Overlay::Message(Message {
        title: "t".to_owned(),
        message: "m".to_owned(),
        severity: Severity::Info,
    }));

    press(&mut state, Key::Escape);

    assert!(state.overlay.is_none());
    assert_eq!(
        state.search(Searched::Tables).map(Search::as_str),
        Some("o"),
        "one press does one thing"
    );
}

#[test]
fn tab_is_inert_while_the_prompt_is_up() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Tab);
    press(&mut state, Key::BackTab);

    assert_eq!(state.focus(), Panel::Tables);
    assert!(searching(&state).is_some(), "and the prompt survives");
}

#[test]
fn a_click_cancels_the_prompt() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    state.apply_search(Searched::Tables, Search::new("orders"));
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('u'));

    point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

    assert!(state.overlay.is_none());
    assert_eq!(
        state.search(Searched::Tables).map(Search::as_str),
        Some("orders"),
        "a click that lands somewhere unexpected must not leave state behind"
    );
}

/// The layer is dispatched above every overlay, so a chord that would open
/// one has to refuse the prompt itself: opening a picker over a live query
/// would leave the panel searched with nothing holding what it searched
/// from (docs/shortcuts.md §1).
#[test]
fn ctrl_t_is_refused_over_a_live_prompt() {
    let mut state = measured(&["a"]);
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('t'));

    ctrl(&mut state, Key::Char('t'));

    assert_eq!(searching(&state).map(Prompt::query), Some("t"));
}

/// The diagnostics overlay is the only place the list it shows lives —
/// `State.config` does not keep it — so `ctrl-t` opening the theme picker
/// from under it would destroy those diagnostics for good.
#[test]
fn ctrl_t_is_refused_over_diagnostics() {
    let (mut state, _) = init(Config::default(), vec![diagnostic("one")], Stamp::default());

    ctrl(&mut state, Key::Char('t'));

    assert!(
        matches!(state.overlay, Some(Overlay::Diagnostics(_))),
        "the diagnostics overlay must survive an unrelated ctrl chord"
    );
}

/// A ctrl chord the layer has no member for is refused as it is anywhere
/// else, rather than typed into the query.
#[test]
fn an_unclaimed_ctrl_chord_is_not_text_in_the_prompt() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));

    ctrl(&mut state, Key::Char('n'));

    assert_eq!(searching(&state).map(Prompt::query), Some(""));
}

/// The blank frame design §3.3 accepts, carried to its repair: the offset
/// a wheel produced from pre-search geometry is out of range for the body
/// the search left, and the next measurement pulls it back. One frame
/// wide, and the loop terminates.
#[test]
fn the_measurement_after_a_search_pulls_a_stale_ddl_offset_back_inside() {
    let mut state = read_shop_orders();
    state.focus_on(Panel::Ddl);
    sized(&mut state, [8, 4, 8, 1], 40);

    for _ in 0..20 {
        point(
            &mut state,
            Pointer::Wheel {
                panel: Panel::Ddl,
                direction: Wheel::Down,
            },
        );
    }
    assert_eq!(
        state.ddl.scroll.offset(),
        36,
        "the wheel bounds itself against the pre-search line count"
    );

    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('t'));

    // A frame of nine wrapped lines in a four-line panel: no offset past
    // five is drawable, whatever the search reported.
    sized(&mut state, [8, 4, 8, 1], 9);

    assert_eq!(state.ddl.scroll.offset(), 5);
}

/// The Ddl half of the `esc` chain's second step. Its viewport is followed
/// by span rather than by index (design §5.1 of the Ddl selection slice),
/// so clearing a search that moved what the cursor names is repaired on
/// the measurement that follows, not in `escape` itself.
#[test]
fn escape_clearing_a_ddl_search_re_follows_the_selection() {
    let mut state = read_shop_orders();
    state.focus_on(Panel::Ddl);
    sized(&mut state, [8, 4, 8, 1], 40);
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('a'));
    press(&mut state, Key::Enter);
    press(&mut state, Key::End);

    press(&mut state, Key::Escape);

    assert!(state.search(Searched::Ddl).is_none());

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 4, 8, 1], 40, 0).with_ddl_selection(Span {
            first: 22,
            lines: 3,
        })),
    );

    assert_eq!(
        state.ddl.scroll.offset(),
        21,
        "lines 21..25 put the whole of the row measured at 22..25 on screen"
    );
}

#[test]
fn a_search_survives_a_focus_change() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('o'));
    press(&mut state, Key::Enter);

    press(&mut state, Key::Tab);
    press(&mut state, Key::Char('t'));

    assert_eq!(
        state.search(Searched::Tables).map(Search::as_str),
        Some("o")
    );
}
