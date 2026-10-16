use super::*;

fn theme_picker(state: &State) -> Option<usize> {
    match state.overlay.as_ref()? {
        Overlay::Themes { cursor, .. } => Some(cursor.index()),
        _ => None,
    }
}

fn picker_sift(state: &State) -> Option<&Sift> {
    match state.overlay.as_ref()? {
        Overlay::Themes { sift, .. } => sift.as_ref(),
        _ => None,
    }
}

fn label_index(label: &str) -> usize {
    BuiltIn::ALL
        .iter()
        .position(|built_in| built_in.label() == label)
        .expect("a built-in with that label")
}

/// What the cursor counts in once a sift is up.
fn picker_labels(state: &State) -> Vec<String> {
    let Some(OverlayView::Themes { labels, .. }) = crate::view::view_model(state).overlay else {
        panic!("the picker is up");
    };

    labels
}

#[test]
fn typing_in_the_picker_narrows_the_list_to_the_matches() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "quill");

    let labels = picker_labels(&state);
    assert!(
        labels
            .iter()
            .all(|label| label.to_lowercase().contains("quill")),
        "{labels:?} keeps a row the term did not match"
    );
    assert!(labels.len() < BuiltIn::ALL.len(), "nothing was dropped");
    assert_eq!(state.theme.label(), "Quill dark");
}

#[test]
fn a_searched_row_previews_its_theme() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "quill");

    assert_eq!(state.theme.label(), "Quill dark");
}

#[test]
fn a_term_matching_only_above_the_origin_seats_on_the_last_match() {
    let mut state = measured(&["a"]);
    state.theme = ThemeChoice::from(BuiltIn::Ported(Ported::CatppuccinMocha));
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
    assert!(
        theme_picker(&state).expect("an open picker") > label_index("Ash dark"),
        "the origin has to sit below every match for this to be the case"
    );

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "ash");

    let labels = picker_labels(&state);
    assert_eq!(
        Some(state.theme.label()),
        labels.last().cloned(),
        "the last row the term kept"
    );
}

#[test]
fn extending_a_term_searches_from_the_origin() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "catppuccin");
    assert_eq!(state.theme.label(), "Catppuccin Latte");

    type_into(&mut state, " mocha");
    assert_eq!(state.theme.label(), "Catppuccin Mocha");
}

#[test]
fn a_term_that_matches_nothing_empties_the_picker_and_keeps_the_preview() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
    let previewed = state.theme.clone();

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "zzz");

    assert!(picker_labels(&state).is_empty());
    assert_eq!(state.theme, previewed);
}

#[test]
fn typing_seats_on_a_config_theme_at_the_renderers_own_index() {
    let mut state = measured(&["a"]);
    let theme = custom_theme("midnight");
    state.config.themes = vec![theme.clone()];
    ctrl(&mut state, Key::Char('t'));

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "midnight");

    let Some(OverlayView::Themes { labels, .. }) = crate::view::view_model(&state).overlay else {
        panic!("the picker is up");
    };
    let expected = labels
        .iter()
        .position(|label| label == "midnight")
        .expect("the config theme is in the picker's labels");

    assert_eq!(theme_picker(&state), Some(expected));
}

#[test]
fn a_letter_typed_into_the_pickers_sift_does_not_quit_or_move() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
    let origin = theme_picker(&state).expect("an open picker");

    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('q'));
    press(&mut state, Key::Char('j'));

    assert!(!state.should_quit, "q is text while a sift is up");
    assert_eq!(picker_sift(&state).map(Sift::query), Some("qj"));
    assert!(
        picker_labels(&state).is_empty(),
        "j filtered rather than moving"
    );

    press(&mut state, Key::Backspace);
    press(&mut state, Key::Backspace);
    press(&mut state, Key::Backspace);
    assert_eq!(theme_picker(&state), Some(origin), "and moved nothing");
}

#[test]
fn g_and_shift_g_type_themselves_into_the_pickers_sift() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
    let origin = theme_picker(&state).expect("an open picker");

    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('g'));
    press(&mut state, Key::Char('G'));

    assert_eq!(picker_sift(&state).map(Sift::query), Some("gG"));
    assert!(
        picker_labels(&state).is_empty(),
        "neither letter jumped the cursor; both were typed"
    );

    press(&mut state, Key::Backspace);
    press(&mut state, Key::Backspace);
    assert_eq!(theme_picker(&state), Some(origin), "and jumped nothing");
}

#[test]
fn backspace_on_an_empty_sift_closes_the_prompt() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('q'));
    press(&mut state, Key::Backspace);
    assert_eq!(picker_sift(&state).map(Sift::query), Some(""));

    press(&mut state, Key::Backspace);
    assert!(picker_sift(&state).is_none(), "the prompt is gone");
    assert!(theme_picker(&state).is_some(), "the picker is not");
}

#[test]
fn enter_closes_the_pickers_sift_and_keeps_the_row() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "quill");
    press(&mut state, Key::Enter);

    assert!(picker_sift(&state).is_none());
    assert_eq!(
        picker_labels(&state).len(),
        BuiltIn::ALL.len(),
        "the list widens back out"
    );
    assert_eq!(
        theme_picker(&state),
        Some(label_index("Quill dark")),
        "with the cursor still on the row the filter found"
    );
}

#[test]
fn q_quits_once_the_pickers_sift_has_closed() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "quill");
    press(&mut state, Key::Enter);
    press(&mut state, Key::Char('q'));

    assert!(state.should_quit);
}

#[test]
fn escape_restores_the_row_the_pickers_sift_opened_on() {
    let mut state = measured(&["a"]);
    let opened_on = state.theme.clone();
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
    let origin = theme_picker(&state).expect("an open picker");

    press(&mut state, Key::Char('/'));
    type_into(&mut state, "quill");
    press(&mut state, Key::Escape);

    assert_eq!(theme_picker(&state), Some(origin));
    assert_eq!(state.theme, opened_on, "the preview goes back too");

    press(&mut state, Key::Escape);
    assert!(state.overlay.is_none());
}

#[test]
fn ctrl_t_over_a_live_sift_is_refused_by_ctrl_keys_overlay_guard() {
    let mut state = measured(&["a"]);
    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));
    press(&mut state, Key::Char('/'));
    type_into(&mut state, "quill");

    press_binding(&mut state, KeyPress::ctrl(Key::Char('t')));

    assert_eq!(picker_sift(&state).map(Sift::query), Some("quill"));
}

fn index_of(built_in: BuiltIn) -> usize {
    BuiltIn::ALL
        .iter()
        .position(|b| *b == built_in)
        .expect("listed")
}

#[test]
fn the_picker_opens_on_the_theme_in_force() {
    let mut state = measured(&["a"]);
    state.theme = ThemeChoice::Ported(Ported::CatppuccinFrappe);

    ctrl(&mut state, Key::Char('t'));

    assert_eq!(
        theme_picker(&state),
        Some(index_of(BuiltIn::Ported(Ported::CatppuccinFrappe)))
    );
}

fn custom_theme(name: &str) -> crate::config::CustomTheme {
    crate::config::CustomTheme {
        name: crate::theme::ThemeName::try_new(name).expect("a plain slug is a valid name"),
        extends: BuiltIn::Ours(Ours::DefaultDark),
        colors: std::collections::BTreeMap::new(),
    }
}

/// `Config::default()` has no custom themes, so the other tests never reach
/// that branch.
#[test]
fn the_picker_seats_on_a_custom_theme_already_in_force() {
    let mut state = measured(&["a"]);
    let theme = custom_theme("midnight");
    state.config.themes = vec![theme.clone()];
    state.theme = ThemeChoice::Custom(theme.name.clone());

    ctrl(&mut state, Key::Char('t'));

    assert_eq!(
        theme_picker(&state),
        Some(BuiltIn::ALL.len()),
        "a custom theme sits right after the built-ins, in config order"
    );
}

#[test]
fn moving_past_the_built_ins_previews_a_custom_theme() {
    let mut state = measured(&["a"]);
    let theme = custom_theme("midnight");
    state.config.themes = vec![theme.clone()];

    ctrl(&mut state, Key::Char('t'));
    for _ in 0..BuiltIn::ALL.len() {
        press(&mut state, Key::Char('j'));
    }

    assert_eq!(theme_picker(&state), Some(BuiltIn::ALL.len()));
    assert_eq!(
        state.theme,
        ThemeChoice::Custom(theme.name.clone()),
        "the row past the last built-in previews the config's own theme"
    );
}

#[test]
fn moving_the_cursor_previews_the_theme() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('j'));

    assert_eq!(
        state.theme,
        ThemeChoice::from(BuiltIn::ALL[1]),
        "the frame repaints as you browse"
    );
}

#[test]
fn enter_keeps_the_previewed_theme() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Enter);

    assert_eq!(state.theme, ThemeChoice::from(BuiltIn::ALL[1]));
    assert!(state.overlay.is_none());
}

#[test]
fn enter_saves_the_theme_it_keeps() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('j'));
    let effects = press(&mut state, Key::Enter);

    assert_eq!(
        effects,
        vec![Effect::SaveTheme(ThemeChoice::from(BuiltIn::ALL[1]))],
        "the theme a restart opens on is the one the picker was left on"
    );
}

#[test]
fn enter_on_the_theme_already_in_force_saves_nothing() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    let effects = press(&mut state, Key::Enter);

    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn a_previewed_theme_is_not_a_saved_one() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    let effects = press(&mut state, Key::Char('j'));

    assert!(
        effects.is_empty(),
        "moving the cursor previews; it does not save"
    );
}

#[test]
fn escape_saves_nothing() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('j'));
    let effects = press(&mut state, Key::Escape);

    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn a_click_that_dismisses_the_picker_saves_nothing() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('j'));
    let effects = update(
        &mut state,
        Msg::Pointer(Pointer::Click(Click::Panel(Panel::Tables))),
    );

    assert!(effects.is_empty(), "a click restores, the same as Esc");
}

#[test]
fn a_frame_that_shrinks_below_the_picker_saves_nothing() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('j'));
    let effects = update(
        &mut state,
        Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
    );

    assert!(effects.is_empty(), "{effects:?}");
}

#[test]
fn ending_a_search_with_enter_saves_nothing_yet() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('/'));
    type_into(&mut state, "quill");
    let ending = press(&mut state, Key::Enter);

    assert!(ending.is_empty(), "the search closed; the picker did not");
    assert!(matches!(state.overlay, Some(Overlay::Themes { .. })));

    let confirming = press(&mut state, Key::Enter);

    assert_eq!(confirming.len(), 1, "and this one is the confirmation");
}

#[test]
fn a_failed_save_warns_and_logs_and_keeps_the_theme() {
    let mut state = measured(&["a"]);
    state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

    update(
        &mut state,
        Msg::ThemeSaveFailed {
            reason: "/home/x/.config/lazycassandra/config.toml: permission denied".to_owned(),
        },
    );

    let Some(Overlay::Message(message)) = &state.overlay else {
        panic!("a save the user asked for and did not get is worth one line");
    };
    assert_eq!(message.severity, Severity::Warning);
    assert!(message.message.contains("permission denied"));

    let entry = state.log.entries().next().expect("the save entry");
    let Outcome::Failed { detail } = entry.outcome() else {
        panic!("the save failed");
    };
    assert!(detail.contains("permission denied"));
    assert_eq!(
        state.theme,
        ThemeChoice::Ported(Ported::CatppuccinMocha),
        "a save that failed is not a reason to take back the theme the user chose"
    );
}

#[test]
fn escape_restores_the_theme_the_picker_opened_on() {
    let mut state = measured(&["a"]);
    state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('k'));
    press(&mut state, Key::Escape);

    assert_eq!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));
    assert!(state.overlay.is_none());
}

#[test]
fn the_picker_cursor_cannot_run_off_the_list() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    for _ in 0..BuiltIn::ALL.len() * 2 {
        press(&mut state, Key::Char('j'));
    }
    assert_eq!(
        state.theme,
        ThemeChoice::from(BuiltIn::ALL[BuiltIn::ALL.len() - 1])
    );
    assert_eq!(
        theme_picker(&state),
        Some(BuiltIn::ALL.len() - 1),
        "the cursor itself must stop at the last row, not merely the theme it read out"
    );

    for _ in 0..BuiltIn::ALL.len() * 2 {
        press(&mut state, Key::Char('k'));
    }
    assert_eq!(state.theme, ThemeChoice::from(BuiltIn::ALL[0]));
    assert_eq!(theme_picker(&state), Some(0));
}

#[test]
fn paging_reaches_the_ends_of_the_theme_list() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::PageDown);

    assert_eq!(
        theme_picker(&state),
        Some(BuiltIn::ALL.len() - 1),
        "the list is shorter than the block drawing it, so one page is all of it"
    );
    assert_eq!(
        state.theme,
        ThemeChoice::from(BuiltIn::ALL[BuiltIn::ALL.len() - 1]),
        "a page is a move, and every move previews"
    );

    press(&mut state, Key::PageUp);

    assert_eq!(theme_picker(&state), Some(0));
    assert_eq!(state.theme, ThemeChoice::from(BuiltIn::ALL[0]));
}

#[test]
fn ctrl_d_and_ctrl_u_page_the_picker_too() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    ctrl(&mut state, Key::Char('d'));

    assert_eq!(theme_picker(&state), Some(BuiltIn::ALL.len() - 1));

    ctrl(&mut state, Key::Char('u'));

    assert_eq!(theme_picker(&state), Some(0));
}

#[test]
fn the_ends_are_reachable_in_the_picker() {
    let last = BuiltIn::ALL.len() - 1;

    for (start, end) in [(Key::Home, Key::End), (Key::Char('g'), Key::Char('G'))] {
        let mut state = measured(&["a"]);
        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));

        press(&mut state, end);
        assert_eq!(theme_picker(&state), Some(last), "{end:?} reaches the last");
        assert_eq!(
            state.theme,
            ThemeChoice::from(BuiltIn::ALL[last]),
            "and previews it, as every other motion in the picker does"
        );

        press(&mut state, start);
        assert_eq!(theme_picker(&state), Some(0), "{start:?} reaches the first");
    }
}

#[test]
fn q_still_quits_with_the_picker_open() {
    let mut state = measured(&["a"]);

    ctrl(&mut state, Key::Char('t'));
    let effects = press(&mut state, Key::Char('q'));

    assert!(
        state.should_quit,
        "a key that means quit everywhere but one overlay is not one anyone can trust"
    );
    assert!(!effects.is_empty(), "quitting cancels what is in flight");
}

#[test]
fn every_hint_the_pickers_bar_shows_does_something() {
    // Mid-list, so every direction and both ends have somewhere to go.
    let seated = || {
        let mut state = measured(&["a"]);
        ctrl(&mut state, Key::Char('t'));
        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));
        state
    };

    let shown = crate::view::hints::bar(&seated());
    let claimed = crate::view::hints::THEMES
        .iter()
        .chain(crate::view::hints::THEMES_GLOBAL);

    assert_eq!(
        shown.panel.len() + shown.global.len(),
        crate::view::hints::THEMES.len() + crate::view::hints::THEMES_GLOBAL.len()
    );

    for binding in claimed {
        for &key in binding.keys {
            let mut state = seated();
            let before = crate::view::view_model(&state);

            let effects = press_stroke(&mut state, key);

            assert!(
                crate::view::view_model(&state) != before
                    || !effects.is_empty()
                    || state.should_quit,
                "the picker's bar offers '{} {}' and pressing it changes nothing",
                crate::view::hints::label(binding.keys),
                binding.action
            );
        }
    }
}

#[test]
fn moving_the_picker_cursor_asks_for_a_repaint() {
    let mut state = measured(&["a"]);
    ctrl(&mut state, Key::Char('t'));
    state.dirty = false;

    press(&mut state, Key::Char('j'));

    assert!(
        state.dirty,
        "the preview is the frame underneath, which nothing else redraws"
    );
}

#[test]
fn a_frame_too_small_for_a_list_refuses_the_picker() {
    let mut state = State::default();

    ctrl(&mut state, Key::Char('t'));

    assert!(
        state.overlay.is_none(),
        "a mode with nothing on screen saying so is worse than no mode"
    );
}

#[test]
fn a_frame_that_shrinks_below_the_picker_closes_it_and_restores_the_theme() {
    let mut state = measured(&["a"]);
    state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('k'));
    assert_ne!(
        state.theme,
        ThemeChoice::Ported(Ported::CatppuccinMocha),
        "previewing a different theme before the frame shrinks"
    );

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 0)),
    );

    assert!(
        state.overlay.is_none(),
        "a picker nothing can draw must not still capture keys"
    );
    assert_eq!(
        state.theme,
        ThemeChoice::Ported(Ported::CatppuccinMocha),
        "the preview must not strand itself when its own room disappears"
    );
}

#[test]
fn a_message_arriving_over_an_open_picker_restores_the_theme() {
    let mut state = measured(&["a"]);
    state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('k'));
    assert_ne!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));

    raise(
        &mut state,
        Message {
            title: "Cannot connect".to_owned(),
            message: "no route to host".to_owned(),
            severity: Severity::Danger,
        },
    );

    assert_eq!(
        state.theme,
        ThemeChoice::Ported(Ported::CatppuccinMocha),
        "the half-previewed theme must not strand itself under the failure"
    );
    assert!(matches!(state.overlay, Some(Overlay::Message(_))));
}

#[test]
fn a_click_dismisses_the_picker_and_restores_the_theme() {
    let mut state = measured(&["a"]);
    state.theme = ThemeChoice::Ported(Ported::CatppuccinMocha);

    ctrl(&mut state, Key::Char('t'));
    press(&mut state, Key::Char('k'));
    assert_ne!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));

    point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

    assert!(state.overlay.is_none());
    assert_eq!(state.theme, ThemeChoice::Ported(Ported::CatppuccinMocha));
}
