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

/// Live hints only. An inert one is drawn greyed and promises nothing —
/// `ctrl-r` on an empty buffer is the one there is (query execution
/// design §3) — so pressing it and demanding a change would be asserting
/// the opposite of what the bar says.
///
/// A binding the posture refuses is left off the bar, so the two are
/// paired by action. The bar appends `esc leave` on every panel but
/// Tables; `escape_leaves_every_panel_for_tables` presses it.
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
    // `?` is filtered by `hints::bar` itself once a binding's effect
    // depends on more than focus (design §5.1) — the sweep is over what
    // the bar actually shows for this state, which is what it promises.
    for panel in Panel::ALL {
        let state = populated(panel);
        let shown: Vec<&str> = crate::view::hints::bar(&state)
            .global
            .iter()
            .map(|hint| hint.action)
            .collect();

        // `q` left GLOBAL because the bar stops naming it in a panel that
        // takes text, and `?` is dropped where no palette would fit. Both
        // are conditions on the bar rather than on the binding set, so the
        // sweep asks the bar what it is showing.
        let claimed = crate::view::hints::GLOBAL
            .iter()
            .chain((!state.takes_text()).then_some(&crate::view::hints::QUIT));

        for binding in claimed {
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

/// Every `Key` the domain understands.
///
/// The `match` is dead weight at runtime and load-bearing at compile time:
/// a variant added to `Key` without a line here fails to compile, so the
/// sweep below cannot quietly stop sweeping.
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

/// Every `KeyPress` worth sweeping for the palette's completeness claim.
///
/// Unmodified, plus ctrl held: ctrl is the only modifier `handle_key`
/// dispatches on (`ctrl_key`) rather than refusing outright, so it is the
/// only one that can ever do something the palette fails to list. Alt is
/// refused wholesale before any arm sees it and shift never reaches a
/// letter — sweeping either would only double the cost of this test for
/// no coverage a future binding could fail to have.
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

/// States that between them exercise every binding: one with an overlay to
/// dismiss, an unfolded keyspace to fold, and the tree at its first row;
/// one focused somewhere other than Tables so the panel numbers differ;
/// one with the tree folded and moved off its first row, so `k`, `pgup`
/// and `home` all have somewhere to go; and one with the palette open and
/// one with the theme picker open, the states that capture keys rather
/// than passing them to a panel — a sweep that never opens one cannot see
/// what it captures; and one focused on a loaded grid, without which the
/// `Table contents` group's keys are only ever answered by the tree and
/// `m` would go unswept.
///
/// The unfold in `covered` matters: with every keyspace already folded,
/// `h` and `←` would be a no-op in all four and the sweep would call them
/// unbound. `moved`'s keyspaces are childless, so `l` and `→` never
/// register there either — `covered` is the only fixture that opens a
/// keyspace with something in it, and so the only one where those two
/// keys do anything. `elsewhere`'s Ddl scroll starts off zero for the
/// same reason `k`/`home` need `moved` off row zero: `ctrl-y` would be a
/// no-op at the top of the document.
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
    // Off zero, or `ctrl-y` would find nothing to scroll up over here.
    elsewhere
        .ddl
        .scroll
        .down(geometry.ddl_lines(), geometry.height(Panel::Ddl));

    let mut moved = measured(&["a", "b", "c"]);
    press(&mut moved, Key::Char('j'));
    press(&mut moved, Key::Char('j'));

    let mut palette_open = measured(&["a", "b", "c"]);
    press(&mut palette_open, Key::Char('?'));

    // Off the first row, so `k` and `home` have somewhere to go here too.
    let mut picker_open = measured(&["a", "b", "c"]);
    ctrl(&mut picker_open, Key::Char('t'));
    press(&mut picker_open, Key::Char('j'));

    // A loaded grid, for `y` to have a row under the cursor.
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
    // The one fixture `ctrl-e` opens the form from: a loaded grid on a
    // frame with room for it.
    yankable.geometry = yankable.geometry.with_export_fits(true);

    // Measured, so the columns `m` cycles the highlight over are drawn.
    let grid = grid_of_rows(4, 5, 80);

    // The one fixture the three column keys answer in: a grid in column
    // mode, with a filter already standing on the column the cursor is on
    // so that `c` has one to clear (grid sort and filter design §6).
    let mut columnar = grid_of_rows(4, 5, 80);
    press(&mut columnar, Key::Char('m'));
    assert_eq!(
        columnar.contents.mode,
        SelectionMode::Column,
        "the fixture is the one column mode reaches in a single press"
    );
    columnar.contents.set_filter(0, Search::new("1"));

    // The export form, on its scope radios — one `tab` past the path, where
    // every printable key is text. Every key `export_key`
    // claims there — `esc`, `tab`/`backtab`, `j`/`k`/`↓`/`↑`, `⏎` — is
    // already catalogued elsewhere (`Global`, `Motion`, `Tables`), so
    // this adds no key the negative sweep has to worry about; only
    // `Export`'s own `space` needs a state where it does something, and
    // this is that state (`toggle_scope` only fires with a scope to
    // toggle to, hence `ROW_LIMIT` rather than a smaller page).
    let mut export_open = contents_showing_more(PAGE);
    ctrl(&mut export_open, Key::Char('e'));
    press(&mut export_open, Key::Tab);

    // The one fixture `n`/`N` do something in: a search applied, with
    // more than one match to walk between (design §5.3).
    let mut searched = browsing_tables(&[("shop", &["orders"])]);
    searched.geometry = geometry;
    searched.apply_search(Searched::Tables, Search::new("o"));

    // The two fixtures the `Drop` group needs: one where `d` opens a
    // question, and one with that question open, so the negative sweep
    // sees what the confirmation captures. The typed spelling is left
    // out for the reason the editor is — every printable key is text
    // there, which would tell the negative sweep the palette is missing
    // the alphabet (`the_typed_confirmation_takes_text` covers it).
    let mut droppable = droppable_tree();
    press(&mut droppable, Key::Char('j'));

    let mut dropping = droppable_tree();
    press(&mut dropping, Key::Char('j'));
    press(&mut dropping, Key::Char('d'));

    // The one state `[`/`]` do anything in: the DDL panel focused with a
    // table under the tree cursor, so its strip draws two tabs.
    let mut tabbed = browsing_a_table();
    tabbed.geometry = geometry;
    tabbed.focus_on(Panel::Ddl);

    // The one fixture `p` and `P` answer in: a result with a page left.
    let pageable = contents_showing_more(3);

    // The two the `Edit` group needs: the inspector open on an editable
    // column, so `e` opens a field, and a grid with an edit standing on
    // it, so `ctrl-s` has something to ask about. The open field is left
    // out for the reason the editor and the typed confirmation are —
    // every printable key is text there
    // (`enter_accepts_the_field_and_the_cell_goes_dirty` covers it).
    let mut editing = inspecting_row(3);
    on_email(&mut editing);

    let dirty = committable(1);

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
    ]
}

/// Whether pressing `key` changes anything a user could observe.
///
/// The view model rather than the `State`, which is not `PartialEq` and
/// whose `dirty` flag `handle_key` sets on every press. It is also the
/// better question: a key that changes nothing visible and emits no effect
/// is not a binding, whatever it touched on the way through.
fn does_something(state: &State, press: KeyPress) -> bool {
    responds(state, |after| press_binding(after, press))
}

/// The same question asked of a whole stroke: a chord is judged by what
/// typing all of it does, since its first press only arms the prefix.
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

/// An editing state, for the `Queries` group alone. It is kept out of
/// `sweep_fixtures` because in a panel that takes text every printable key
/// does something, which would tell
/// `no_key_does_anything_the_palette_does_not_list` that the palette is
/// missing the alphabet. What is true there instead is pinned by
/// `a_letter_typed_in_queries_is_text_not_a_shortcut`.
fn editor_fixture() -> State {
    let mut editor = editing("SELECT * FROM shop.orders");
    editor.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
    // One step off the buffer's end: `Delete` has nothing to remove
    // sitting exactly there (`delete_at_the_end_of_the_buffer_does_nothing`
    // in editor.rs), and `End` from the end is a no-op. Off by one, `Home`,
    // `End`, and `Delete` all have somewhere to move from.
    editor.queries.edit(Key::Left, 1);
    editor
}

/// The `Queries` group is checked against the editor fixture *alone*, and
/// every other group against `sweep_fixtures` alone. Both directions
/// matter. A Global, Motion or Tables key pressed in the editor types
/// itself, so it would "do something" whether or not its real binding
/// still exists. And a Queries key satisfied by some other fixture —
/// `⏎` folding a keyspace, `esc` dismissing an overlay, `home`/`end`
/// moving a tree cursor — would let the editor quietly stop claiming it
/// with the sweep still green. Each group is answered by the state it
/// describes.
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
