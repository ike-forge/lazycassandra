use super::*;

#[test]
fn ctrl_e_hands_the_queries_buffer_to_an_editor() {
    let mut state = State::default();
    state.focus_on(Panel::Queries);
    state.queries.replace("SELECT * FROM t");

    let effects = ctrl(&mut state, Key::Char('e'));

    assert_eq!(
        effects,
        vec![Effect::OpenEditor {
            buffer: "SELECT * FROM t".to_owned()
        }]
    );
}

/// The chord means one thing now: it scrolled the Table DDL viewport as
/// well until that half was retired (docs/shortcuts.md §4).
#[test]
fn ctrl_e_is_inert_outside_queries() {
    for focus in [Panel::Tables, Panel::Ddl, Panel::Contents] {
        let mut state = State::default();
        state.focus_on(focus);
        let before = crate::view::view_model(&state);

        assert!(
            ctrl(&mut state, Key::Char('e')).is_empty(),
            "{focus:?} claims no ctrl-e"
        );
        assert_eq!(
            crate::view::view_model(&state),
            before,
            "{focus:?} moved under a chord it does not claim"
        );
    }
}

#[test]
fn ctrl_e_under_an_overlay_does_nothing() {
    let overlays = [
        Overlay::Commands(CommandsPanel::opened()),
        Overlay::Themes {
            cursor: Cursor::default(),
            restored: ThemeChoice::default(),
            sift: None,
        },
    ];

    for overlay in overlays {
        let mut state = State::default();
        state.focus_on(Panel::Queries);
        state.overlay = Some(overlay);

        assert!(ctrl(&mut state, Key::Char('e')).is_empty());
    }
}

#[test]
fn a_closed_editor_replaces_the_buffer() {
    let mut state = State::default();
    state.queries.replace("SELECT");

    let effects = update(
        &mut state,
        Msg::EditorClosed {
            buffer: "SELECT *\nFROM t".to_owned(),
        },
    );

    assert!(effects.is_empty());
    assert_eq!(state.queries.text(), "SELECT *\nFROM t");
    assert!(state.dirty, "the buffer changed, so the frame is stale");
}

#[test]
fn a_failed_editor_says_so_and_keeps_the_buffer() {
    let mut state = State::default();
    state.queries.replace("SELECT");

    let effects = update(
        &mut state,
        Msg::EditorFailed {
            reason: "could not run hx: No such file or directory".to_owned(),
        },
    );

    assert!(effects.is_empty());
    assert_eq!(state.queries.text(), "SELECT");
    assert!(matches!(
        state.overlay,
        Some(Overlay::Message(Message {
            severity: Severity::Danger,
            ..
        }))
    ));
}

/// The modal stays — the user is waiting on it right now — and gets a
/// mirror in the trail alongside it (query log design §8).
#[test]
fn an_editor_that_would_not_run_leaves_its_reason_in_the_trail() {
    let mut state = State::default();

    let _effects = update(
        &mut state,
        Msg::EditorFailed {
            reason: "no $EDITOR set".to_owned(),
        },
    );

    assert!(
        matches!(state.overlay, Some(Overlay::Message(_))),
        "the modal stays"
    );
    let Outcome::Failed { detail } = state.log.entries().last().unwrap().outcome() else {
        panic!("the editor failed");
    };
    assert!(detail.contains("no $EDITOR set"), "{detail}");
}

#[test]
fn a_letter_typed_in_queries_is_text_not_a_shortcut() {
    // Every letter here is bound somewhere: t/a/c/u focus a panel, j/k/h/l
    // move, q quits. In the editor they are all just text.
    let state = editing("tacujkhlq");

    assert_eq!(state.focus(), Panel::Queries, "no letter moved focus");
    assert_eq!(state.queries.text(), "tacujkhlq");
}

#[test]
fn y_is_text_in_the_editor() {
    let mut state = editing("");

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(state.queries.text(), "y");
    assert!(effects.is_empty(), "not reachable from Queries (§1)");
}

/// `?` is a CQL bind marker before it is a shortcut, so the editor has to
/// win it. Neither parent of the merge that brought the palette and the
/// editor together could test this: each was the only mode in its branch.
#[test]
fn a_question_mark_typed_in_queries_is_a_bind_marker_not_the_palette() {
    let mut state = measured(&["a"]);
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    press(&mut state, Key::Char('?'));

    assert!(
        state.overlay.is_none(),
        "the editor claimed it before the palette could open"
    );
    assert_eq!(state.queries.text(), "?");
}

/// The two mode guards in `handle_key`, in the order they are written: an
/// overlay the user opened deliberately outranks the focused panel's claim
/// on printable keys (docs/shortcuts.md §2).
#[test]
fn an_open_palette_outranks_a_panel_that_takes_text() {
    let mut state = measured(&["a", "b", "c"]);
    press(&mut state, Key::Char('?'));
    state.focus_on(Panel::Queries);

    press(&mut state, Key::Char('j'));

    assert_eq!(
        state.queries.text(),
        "",
        "the palette took the key rather than the editor"
    );
    assert!(state.overlay.is_some(), "and it is still open");
}

#[test]
fn q_does_not_quit_while_the_editor_has_focus() {
    let mut state = browsing(&["a"]);
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    let effects = press(&mut state, Key::Char('q'));

    assert!(!state.should_quit);
    assert_eq!(effects, Vec::new());
    assert_eq!(state.queries.text(), "q");
}

#[test]
fn escape_closes_the_buffer_and_a_second_leaves_for_tables() {
    let mut state = editing("SELECT");

    press(&mut state, Key::Escape);

    assert_eq!(state.focus(), Panel::Queries, "one esc does one thing");
    assert!(!state.takes_text());

    press(&mut state, Key::Escape);

    assert_eq!(state.focus(), Panel::Tables);
    assert_eq!(state.queries.text(), "SELECT", "leaving keeps the buffer");
}

/// The last rung is the panel, not the editor: a key that backs out of
/// Queries and sits inert in DDL beside it means two things
/// (docs/shortcuts.md §2).
#[test]
fn escape_leaves_every_panel_for_tables() {
    for panel in Panel::ALL {
        let mut state = populated(panel);

        press(&mut state, Key::Escape);

        assert_eq!(state.focus(), Panel::Tables, "esc from {panel:?}");
    }
}

#[test]
fn escape_dismisses_the_overlay_before_it_leaves_the_editor() {
    let mut state = editing("SELECT");
    state.overlay = Some(Overlay::Message(Message {
        title: "Schema unavailable".to_owned(),
        message: "boom".to_owned(),
        severity: Severity::Danger,
    }));

    press(&mut state, Key::Escape);

    assert!(state.overlay.is_none());
    assert_eq!(state.focus(), Panel::Queries, "one esc does one thing");
}

#[test]
fn tab_still_leaves_the_editor() {
    let mut state = editing("SELECT");

    press(&mut state, Key::Tab);

    assert_eq!(
        state.focus(),
        Panel::Queries,
        "Queries' own second stop, not yet the next panel"
    );
    assert_eq!(state.queries.tab(), QueriesTab::Logs, "not typed as text");
    assert_eq!(state.queries.text(), "SELECT");
}

#[test]
fn a_modified_key_is_still_not_text() {
    let mut state = browsing(&["a"]);
    state.focus_on(Panel::Queries);

    update(
        &mut state,
        Msg::Input(KeyPress {
            key: Key::Char('a'),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        }),
    );

    assert_eq!(
        state.queries.text(),
        "",
        "ctrl-a is a binding that does not exist yet, not a slower a"
    );
}

#[test]
fn typing_outside_queries_still_focuses_a_panel() {
    let mut state = browsing(&["a"]);

    press(&mut state, Key::Char('3'));

    assert_eq!(state.focus(), Panel::Contents);
    assert_eq!(state.queries.text(), "", "nothing leaked into the buffer");
}

#[test]
fn the_editing_keys_reach_the_buffer() {
    let mut state = editing("SELECT *");

    press(&mut state, Key::Backspace);
    assert_eq!(state.queries.text(), "SELECT ");

    press(&mut state, Key::Enter);
    assert_eq!(state.queries.text(), "SELECT \n");

    press(&mut state, Key::Left);
    press(&mut state, Key::Delete);
    assert_eq!(state.queries.text(), "SELECT ", "delete ate the newline");
}

#[test]
fn home_and_end_reach_the_ends_of_the_line_in_the_editor() {
    let mut state = editing("SELECT *\nFROM t");

    press(&mut state, Key::Home);
    assert_eq!(
        state.queries.cursor(),
        crate::editor::Position { line: 1, column: 0 }
    );

    press(&mut state, Key::End);
    assert_eq!(
        state.queries.cursor(),
        crate::editor::Position { line: 1, column: 6 },
        "the line's end, not the buffer's"
    );
}
