use super::*;

#[test]
fn the_editing_keys_do_nothing_outside_a_panel_that_takes_text() {
    for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
        for key in [Key::Backspace, Key::Delete] {
            let mut state = populated(panel);
            let before = crate::view::view_model(&state);

            let effects = press(&mut state, key);

            assert!(
                effects.is_empty() && crate::view::view_model(&state) == before,
                "{key:?} belongs to the editor and {panel:?} does not take text"
            );
        }
    }
}

/// Live hints only: an inert one promises nothing. Posture-refused bindings are
/// off the bar, so the two are paired by action.
#[test]
fn every_hint_the_bar_shows_does_something() {
    for panel in Panel::ALL {
        let shown = crate::view::hints::bar(&populated(panel)).panel;

        for binding in panel.hints() {
            let Some(hint) = shown.iter().find(|hint| hint.action == binding.action) else {
                continue;
            };
            if hint.liveness == crate::view::Liveness::Inert {
                continue;
            }

            for &key in binding.keys {
                let mut state = populated(panel);
                let before = crate::view::view_model(&state);

                let effects = press_stroke(&mut state, key);

                assert!(
                    crate::view::view_model(&state) != before || !effects.is_empty(),
                    "{panel:?} offers '{} {}' and pressing it changes nothing",
                    crate::view::hints::label(binding.keys),
                    binding.action
                );
            }
        }
    }
}

#[test]
fn every_global_hint_does_something_in_every_panel() {
    for panel in Panel::ALL {
        let state = populated(panel);
        let shown: Vec<&str> = crate::view::hints::bar(&state)
            .global
            .iter()
            .map(|hint| hint.action)
            .collect();

        // `?` is conditional on the bar, not the binding set.
        for binding in crate::view::hints::GLOBAL {
            if !shown.contains(&binding.action) {
                continue;
            }
            for &key in binding.keys {
                let mut state = populated(panel);
                let before = crate::view::view_model(&state);

                let effects = press_stroke(&mut state, key);

                assert!(
                    crate::view::view_model(&state) != before || !effects.is_empty(),
                    "'{} {}' does nothing with {panel:?} focused",
                    crate::view::hints::label(binding.keys),
                    binding.action
                );
            }
        }
    }
}

/// The match fails to compile when `Key` gains a variant, so the sweep cannot
/// quietly shrink.
fn every_key() -> Vec<Key> {
    match Key::Enter {
        Key::Char(_)
        | Key::Enter
        | Key::Escape
        | Key::Backspace
        | Key::Tab
        | Key::BackTab
        | Key::Delete
        | Key::Insert
        | Key::Home
        | Key::End
        | Key::PageUp
        | Key::PageDown
        | Key::Up
        | Key::Down
        | Key::Left
        | Key::Right
        | Key::F(_) => {}
    }

    [
        Key::Enter,
        Key::Escape,
        Key::Backspace,
        Key::Tab,
        Key::BackTab,
        Key::Delete,
        Key::Insert,
        Key::Home,
        Key::End,
        Key::PageUp,
        Key::PageDown,
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
    ]
    .into_iter()
    .chain((1..=12).map(Key::F))
    .chain((0x20..=0x7e_u8).map(|byte| Key::Char(char::from(byte))))
    .collect()
}

/// ctrl is the only modifier `handle_key` dispatches on; alt is refused and
/// shift never reaches a letter.
fn every_press() -> Vec<KeyPress> {
    every_key()
        .into_iter()
        .flat_map(|key| [KeyPress::new(key), KeyPress::ctrl(key)])
        .collect()
}

fn catalogued() -> Vec<KeyPress> {
    CATALOGUE
        .iter()
        .flat_map(|group| group.commands)
        .flat_map(|command| command.binding.keys)
        .copied()
        .map(Stroke::press)
        .collect()
}

/// Between them every binding has somewhere to act: an overlay open, a keyspace
/// unfolded, cursors and offsets off their ends, a loaded grid.
fn sweep_fixtures() -> Vec<State> {
    let geometry = Geometry::new([8, 14, 20, 1], 37, 14);

    let mut covered = browsing_tables(&[("shop", &["orders"])]);
    covered.geometry = geometry;
    press(&mut covered, Key::Char('l'));
    covered.overlay = Some(Overlay::Message(Message {
        title: "Schema unavailable".to_owned(),
        message: "boom".to_owned(),
        severity: Severity::Danger,
    }));

    let mut elsewhere = browsing_tables(&[("shop", &["orders"])]);
    elsewhere.geometry = geometry;
    elsewhere.focus_on(Panel::Ddl);
    elsewhere
        .ddl
        .scroll
        .down(geometry.ddl_lines(), geometry.height(Panel::Ddl));

    let mut moved = measured(&["a", "b", "c"]);
    press(&mut moved, Key::Char('j'));
    press(&mut moved, Key::Char('j'));

    let mut palette_open = measured(&["a", "b", "c"]);
    press(&mut palette_open, Key::Char('?'));

    let mut picker_open = measured(&["a", "b", "c"]);
    ctrl(&mut picker_open, Key::Char('t'));
    press(&mut picker_open, Key::Char('j'));

    let mut yankable = read_shop_orders();
    press(&mut yankable, Key::Enter);
    let query = yankable
        .contents
        .outstanding()
        .expect("the panel is waiting");
    update(
        &mut yankable,
        Msg::PageReady {
            query,
            page: Box::new(page_of(3)),
            took: Duration::ZERO,
        },
    );
    yankable.geometry = yankable.geometry.with_export_fits(true);

    let grid = grid_of_rows(4, 5, 80);

    // A filter already stands, so `c` has one to clear.
    let mut columnar = grid_of_rows(4, 5, 80);
    press(&mut columnar, Key::Char('m'));
    assert_eq!(
        columnar.contents.mode,
        SelectionMode::Column,
        "the fixture is the one column mode reaches in a single press"
    );
    columnar.contents.set_filter(0, Search::new("1"));

    // On the scope radios, where `space` toggles; `ROW_LIMIT` so there is a
    // scope to toggle to.
    let mut export_open = contents_showing_more(PAGE);
    ctrl(&mut export_open, Key::Char('e'));
    press(&mut export_open, Key::Tab);

    let mut searched = browsing_tables(&[("shop", &["orders"])]);
    searched.geometry = geometry;
    searched.apply_search(Searched::Tables, Search::new("o"));

    // The typed spelling is left out: every printable key is text there.
    let mut droppable = droppable_tree();
    press(&mut droppable, Key::Char('j'));

    let mut dropping = droppable_tree();
    press(&mut dropping, Key::Char('j'));
    press(&mut dropping, Key::Char('d'));

    // A table under the tree, so the DDL strip draws two tabs.
    let mut tabbed = browsing_a_table();
    tabbed.geometry = geometry;
    tabbed.focus_on(Panel::Ddl);

    let pageable = contents_showing_more(3);

    // The open field is left out: every printable key is text there.
    let mut editing = inspecting_row(3);
    on_email(&mut editing);

    let dirty = committable(1);

    let mut form = editable_grid(1);
    press(&mut form, Key::Char('i'));
    press(&mut form, Key::Char('j'));
    press(&mut form, Key::Char('j'));
    press(&mut form, Key::Char('e'));
    type_in(&mut form, "x");
    press(&mut form, Key::Enter);

    vec![
        covered,
        elsewhere,
        moved,
        palette_open,
        picker_open,
        yankable,
        grid,
        columnar,
        export_open,
        droppable,
        dropping,
        searched,
        tabbed,
        pageable,
        editing,
        dirty,
        form,
    ]
}

/// Compares view models: `State` is not `PartialEq` and every press sets
/// `dirty`.
fn does_something(state: &State, press: KeyPress) -> bool {
    responds(state, |after| press_binding(after, press))
}

/// A chord's first press only arms the prefix.
fn stroke_does_something(state: &State, stroke: Stroke) -> bool {
    responds(state, |after| press_stroke(after, stroke))
}

fn responds(state: &State, press: impl FnOnce(&mut State) -> Vec<Effect>) -> bool {
    let mut after = state.clone();
    let effects = press(&mut after);

    !effects.is_empty()
        || after.should_quit != state.should_quit
        || crate::view::view_model(&after) != crate::view::view_model(state)
}

#[test]
fn no_key_does_anything_the_palette_does_not_list() {
    let catalogued = catalogued();

    for state in sweep_fixtures() {
        for press in every_press() {
            if catalogued.contains(&press) {
                continue;
            }

            assert!(
                !does_something(&state, press),
                "{press:?} does something and docs/shortcuts.md's palette does not list it"
            );
        }
    }
}

/// Kept out of `sweep_fixtures`: every printable key does something in an
/// editor.
fn editor_fixture() -> State {
    let mut editor = editing("SELECT * FROM shop.orders");
    editor.geometry = Geometry::new([8, 14, 20, 1], 37, 14).with_history_rows(10);
    // `ctrl-g` needs a history to open.
    editor.histories.record(
        &crate::config::ConnectionName::try_new("local").expect("a valid name"),
        "SELECT 1;",
        editor.config.history.limit,
    );
    // Off the end by one, so `Home`, `End` and `Delete` have somewhere to move
    // from.
    editor.queries.edit(Key::Left, 1);
    editor
}

/// Each group is checked only against the state it describes: in the editor
/// every key types itself, and other fixtures would satisfy Queries keys.
#[test]
fn every_key_the_palette_lists_does_something() {
    let plain = sweep_fixtures();
    let editor = vec![editor_fixture()];

    for group in CATALOGUE {
        let fixtures = if group.heading == "Queries" {
            &editor
        } else {
            &plain
        };

        for command in group.commands {
            for &press in command.binding.keys {
                assert!(
                    fixtures
                        .iter()
                        .any(|state| stroke_does_something(state, press)),
                    "{} lists {press:?} and no state responds to it",
                    group.heading
                );
            }
        }
    }
}
