use super::*;

// The panel's unfocused height is a function of its buffer (layout.rs), so an
// unfocused Logs tab would show one thing and be as tall as another
// (query log design §5.2).
#[test]
fn leaving_the_panel_puts_the_buffer_back() {
    let mut state = on_the_trail();

    press(&mut state, Key::Char('1'));

    assert_eq!(state.focus(), Panel::Tables);
    assert_eq!(state.queries.tab(), QueriesTab::Cql);
}

#[test]
fn the_trail_takes_no_text() {
    let mut state = on_the_trail();

    press(&mut state, Key::Char('x'));

    assert_eq!(state.queries.text(), "", "the buffer is untouched");
}

#[test]
fn q_quits_from_the_trail() {
    let mut state = on_the_trail();

    let effects = press(&mut state, Key::Char('q'));

    assert!(effects.contains(&Effect::Quit));
}

#[test]
fn the_trail_is_searchable() {
    let mut state = trail_of(&["SELECT one", "SELECT two", "DELETE three"]);

    search_for(&mut state, "DELETE");

    assert_eq!(state.log.cursor.index(), 2);
}

#[test]
fn n_walks_the_trails_matches_and_wraps() {
    let mut state = trail_of(&["SELECT one", "DELETE two", "DELETE three"]);
    state.apply_search(Searched::Queries, Search::new("DELETE"));

    press(&mut state, Key::Char('n'));
    assert_eq!(state.log.cursor.index(), 1);

    press(&mut state, Key::Char('n'));
    assert_eq!(state.log.cursor.index(), 2);

    press(&mut state, Key::Char('n'));
    assert_eq!(state.log.cursor.index(), 1, "wraps back to the first match");
}

#[test]
fn escape_over_the_trail_clears_its_search() {
    let mut state = trail_of(&["SELECT one"]);
    search_for(&mut state, "one");

    press(&mut state, Key::Escape);

    assert_eq!(state.search(Searched::Queries), None);
}

// What lands on the clipboard is the pair, which is what makes it paste
// into a ticket (query log design §5.3).
#[test]
fn yanking_an_entry_copies_its_statement_and_its_outcome() {
    let mut state = trail_of(&["SELECT one"]);

    let effects = press(&mut state, Key::Char('y'));

    let Some(Effect::Yank(copied)) = effects
        .into_iter()
        .find(|effect| matches!(effect, Effect::Yank(_)))
    else {
        panic!("a yank");
    };
    assert!(copied.contains("SELECT one"));
    assert!(copied.contains("cancelled"), "{copied}");
}

#[test]
fn yanking_a_visual_selection_joins_every_selected_entrys_two_rows() {
    let mut state = trail_of(&["SELECT one", "SELECT two", "SELECT three"]);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    let effects = press(&mut state, Key::Char('y'));

    let Some(Effect::Yank(copied)) = effects
        .into_iter()
        .find(|effect| matches!(effect, Effect::Yank(_)))
    else {
        panic!("a yank");
    };
    assert_eq!(
        copied.matches("cancelled").count(),
        2,
        "one outcome row per selected entry: {copied}"
    );
    assert!(copied.contains("SELECT one"));
    assert!(copied.contains("SELECT two"));
    assert!(!copied.contains("SELECT three"), "{copied}");
}

#[test]
fn yanking_the_trail_lights_the_entries_it_took() {
    let mut state = trail_of(&["SELECT one", "SELECT two"]);

    press(&mut state, Key::Char('y'));

    let flash = state
        .flash
        .expect("a yank on the Logs tab lights something");
    assert_eq!(flash.panel(), Panel::Queries);
    assert_eq!(flash.rows(), &Ranges::of(Range::new(0, 0)));
}

#[test]
fn y_on_the_cql_tab_yanks_nothing() {
    let mut state = trail_of(&["SELECT one"]);
    state.queries.show(QueriesTab::Cql);

    assert!(press(&mut state, Key::Char('y')).is_empty());
    assert!(press(&mut state, Key::Char('Y')).is_empty());
}

/// The panel's own number is not carved out of what an open buffer takes
/// as text, or `WHERE id = 4` could not be typed.
#[test]
fn a_number_typed_into_the_focused_cql_buffer_inserts_the_digit() {
    let mut state = State::default();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    press(&mut state, Key::Char('4'));

    assert_eq!(state.focus(), Panel::Queries, "focus is the mode");
    assert_eq!(state.queries.text(), "4");
}
