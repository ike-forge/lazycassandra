use super::*;

#[test]
fn question_mark_opens_the_palette_and_closes_it() {
    let mut state = measured(&["a"]);

    press(&mut state, Key::Char('?'));
    assert!(palette(&state).is_some());

    press(&mut state, Key::Char('?'));
    assert!(state.overlay.is_none());
}

#[test]
fn escape_closes_the_palette() {
    let mut state = measured(&["a"]);
    press(&mut state, Key::Char('?'));

    press(&mut state, Key::Escape);

    assert!(state.overlay.is_none());
}

#[test]
fn a_letter_typed_into_the_palettes_sift_does_not_quit_or_close() {
    let mut state = measured(&["a"]);
    press(&mut state, Key::Char('?'));

    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('q'));
    press(&mut state, Key::Char('?'));

    assert!(!state.should_quit);
    let panel = palette(&state).expect("the palette is still up");
    assert_eq!(panel.sift().map(Sift::query), Some("q?"));
}

#[test]
fn g_and_shift_g_type_themselves_into_the_palettes_sift() {
    let mut state = measured(&["a"]);
    press(&mut state, Key::Char('?'));

    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('g'));
    press(&mut state, Key::Char('G'));

    let panel = palette(&state).expect("the palette is still up");
    assert_eq!(panel.sift().map(Sift::query), Some("gG"));
    assert!(
        panel.command().is_none(),
        "the term matched nothing, and neither letter jumped the cursor"
    );
}

#[test]
fn enter_closes_the_palettes_sift_and_the_next_one_runs_the_row() {
    let mut state = measured(&["a"]);
    press(&mut state, Key::Char('?'));

    press(&mut state, Key::Char('/'));
    for character in "choose a theme".chars() {
        press(&mut state, Key::Char(character));
    }
    press(&mut state, Key::Enter);
    assert!(palette(&state).expect("still up").sift().is_none());

    press(&mut state, Key::Enter);
    assert!(
        matches!(state.overlay, Some(Overlay::Themes { .. })),
        "the row the search found is the row that ran"
    );
}

#[test]
fn escape_leaves_the_palettes_sift_before_the_palette() {
    let mut state = measured(&["a"]);
    press(&mut state, Key::Char('?'));
    let origin = palette(&state).expect("open").selected();

    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('q'));
    press(&mut state, Key::Escape);

    let panel = palette(&state).expect("the palette survives the first esc");
    assert!(panel.sift().is_none());
    assert_eq!(panel.selected(), origin);

    press(&mut state, Key::Escape);
    assert!(state.overlay.is_none());
}

#[test]
fn opening_the_palette_replaces_the_message_it_covers() {
    let mut state = measured(&["a"]);
    state.overlay = Some(Overlay::Message(Message {
        title: "Schema unavailable".to_owned(),
        message: "boom".to_owned(),
        severity: Severity::Danger,
    }));

    press(&mut state, Key::Char('?'));
    assert!(palette(&state).is_some());

    press(&mut state, Key::Escape);
    assert!(
        state.overlay.is_none(),
        "esc leaves nothing rather than peeling back to the message"
    );
}

#[test]
fn the_palette_does_not_open_where_it_cannot_be_drawn() {
    let mut state = browsing(&["a"]);
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 0);

    press(&mut state, Key::Char('?'));

    assert!(
        state.overlay.is_none(),
        "a mode with nothing on screen saying so is worse than an inert key"
    );
}

#[test]
fn an_open_palette_takes_the_motion_the_panel_would_have() {
    let mut state = measured(&["a", "b", "c"]);
    press(&mut state, Key::Char('?'));

    press(&mut state, Key::Char('j'));

    assert_eq!(
        state.tables.cursor().index(),
        0,
        "the tree did not move behind the palette"
    );
    assert_eq!(palette(&state).expect("still open").selected(), 2);

    press(&mut state, Key::Char('k'));
    assert_eq!(palette(&state).expect("still open").selected(), 1);

    press(&mut state, Key::End);
    assert_eq!(
        palette(&state).expect("still open").selected(),
        crate::commands::row_count(None) - 1
    );

    press(&mut state, Key::Home);
    assert_eq!(palette(&state).expect("still open").selected(), 1);

    press(&mut state, Key::PageDown);
    assert_eq!(
        palette(&state).expect("still open").selected(),
        15,
        "paging is routed at the handle_key layer, same as j/k"
    );

    press(&mut state, Key::PageUp);
    assert_eq!(
        palette(&state).expect("still open").selected(),
        1,
        "a page up from the second group lands inside the first"
    );
}

#[test]
fn q_quits_from_inside_the_palette() {
    let mut state = measured(&["a"]);
    press(&mut state, Key::Char('?'));

    press(&mut state, Key::Char('q'));

    assert!(state.should_quit);
}

#[test]
fn an_open_palette_swallows_the_keys_it_does_not_use() {
    // Each focus is chosen so a leaked key would be observable.
    let cases = [
        (Key::Char('t'), Panel::Contents),
        (Key::Tab, Panel::Contents),
        (Key::Char(' '), Panel::Tables),
    ];

    for (key, focus) in cases {
        let mut state = browsing_tables(&[("a", &["t1"])]);
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        state.focus_on(focus);
        press(&mut state, Key::Char('?'));
        let before = crate::view::view_model(&state);

        let effects = press(&mut state, key);

        assert!(palette(&state).is_some(), "{key:?} dismissed the palette");
        assert_eq!(
            crate::view::view_model(&state),
            before,
            "{key:?} reached past the palette"
        );
        assert!(effects.is_empty(), "{key:?} issued {effects:?}");
    }
}

fn seat_on(state: &mut State, action: &str) {
    for _ in 0..crate::commands::row_count(None) {
        let seated = palette(state)
            .expect("the palette is open")
            .command()
            .is_some_and(|command| command.binding.action == action);

        if seated {
            return;
        }
        press(state, Key::Char('j'));
    }

    panic!("no row is labelled {action:?}");
}

#[test]
fn enter_closes_the_palette_and_focuses_the_panel_the_row_belongs_to() {
    let mut state = measured(&["shop"]);
    state.focus_on(Panel::Tables);

    press(&mut state, Key::Char('?'));
    seat_on(&mut state, "cycle selection mode");
    press(&mut state, Key::Enter);

    assert!(state.overlay.is_none(), "the palette closes first, always");
    assert_eq!(
        state.focus(),
        Panel::Contents,
        "the row is Table contents', so Table contents is where it is pressed"
    );
}

#[test]
fn enter_presses_the_rows_own_key() {
    let mut state = measured(&["shop"]);
    state.focus_on(Panel::Contents);
    let before = state.contents.mode;

    press(&mut state, Key::Char('?'));
    seat_on(&mut state, "cycle selection mode");
    press(&mut state, Key::Enter);

    assert_ne!(
        state.contents.mode, before,
        "running the row does what pressing m does"
    );
}

#[test]
fn enter_on_the_run_row_runs_the_buffer() {
    let mut state = measured(&["shop"]);
    state.focus_on(Panel::Queries);
    state.queries.start_editing();
    type_into(&mut state, "select * from shop.a");
    // Leave Queries so `?` opens the palette through the real key path.
    state.focus_on(Panel::Tables);

    press(&mut state, Key::Char('?'));
    seat_on(&mut state, "run the statement");
    let effects = press(&mut state, Key::Enter);

    assert_eq!(
        state.focus(),
        Panel::Contents,
        "a run is a read, and a read lands where the rows do"
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::ExecutePaged { .. })),
        "{effects:?} carries no statement to run"
    );
}

#[test]
fn a_row_whose_action_cannot_act_still_moves_focus() {
    let mut state = measured(&["shop"]);
    state.focus_on(Panel::Tables);

    press(&mut state, Key::Char('?'));
    seat_on(&mut state, "run the statement");
    let effects = press(&mut state, Key::Enter);

    assert_eq!(state.focus(), Panel::Queries);
    assert!(effects.is_empty(), "an empty buffer runs nothing");
}

#[test]
fn enter_on_an_inert_row_closes_the_palette_and_stops() {
    for group in CATALOGUE {
        for command in group.commands {
            if command.run != Run::Inert {
                continue;
            }

            let mut state = measured(&["shop"]);
            state.focus_on(Panel::Tables);
            let before = crate::view::view_model(&state);

            press(&mut state, Key::Char('?'));
            seat_on(&mut state, command.binding.action);
            press(&mut state, Key::Enter);

            assert!(
                state.overlay.is_none(),
                "{:?} left the palette open",
                command.binding.action
            );
            assert_eq!(
                crate::view::view_model(&state),
                before,
                "{:?} changed more than the overlay",
                command.binding.action
            );
        }
    }
}

#[test]
fn every_row_can_be_run() {
    for group in CATALOGUE {
        for command in group.commands {
            let mut state = measured(&["shop"]);
            press(&mut state, Key::Char('?'));
            seat_on(&mut state, command.binding.action);
            let _ = press(&mut state, Key::Enter);

            match command.run {
                Run::Editing => assert_eq!(
                    state.focus(),
                    Panel::Queries,
                    "{:?} is an edit",
                    command.binding.action
                ),
                Run::In(panel) => assert_eq!(
                    state.focus(),
                    panel,
                    "{:?} is filed under {panel:?}",
                    command.binding.action
                ),
                Run::Here | Run::Inert => {}
            }
        }
    }
}

#[test]
fn a_row_filed_under_a_panel_stays_in_that_panel() {
    for group in CATALOGUE {
        for command in group.commands {
            let panel = match command.run {
                Run::In(panel) => panel,
                Run::Editing => Panel::Queries,
                Run::Here | Run::Inert => continue,
            };
            let key = *command
                .binding
                .keys
                .first()
                .expect("a binding names at least one key");

            let mut state = measured(&["shop"]);
            state.focus_on(panel);
            if command.run == Run::Editing {
                state.queries.start_editing();
            }
            let _ = press_stroke(&mut state, key);

            assert_eq!(
                state.focus(),
                panel,
                "{:?} is filed under {panel:?} and leaves it",
                command.binding.action
            );
        }
    }
}

#[test]
fn a_frame_that_shrinks_below_the_palette_closes_it() {
    let mut state = measured(&["a"]);
    press(&mut state, Key::Char('?'));
    assert!(palette(&state).is_some());

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
    );

    assert!(
        state.overlay.is_none(),
        "a palette nothing can draw must not still capture keys"
    );
}

#[test]
fn a_frame_that_shrinks_below_the_diagnostics_modal_closes_it() {
    let (mut state, _) = init(Config::default(), vec![diagnostic("one")], Stamp::default());
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
    assert!(matches!(state.overlay, Some(Overlay::Diagnostics(_))));

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
    );

    assert!(
        state.overlay.is_none(),
        "a diagnostics modal nothing can draw must not still capture every key"
    );
}
