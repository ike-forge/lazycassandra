//! The query editor slice: formatting, highlighting and completion
//! (docs/superpowers/specs/2026-08-16-query-editor-design.md).

use super::*;

fn ctrl(character: char) -> KeyPress {
    KeyPress::ctrl(Key::Char(character))
}

fn offered(state: &State) -> Vec<String> {
    state
        .queries
        .completion()
        .map(|completion| {
            completion
                .candidates()
                .iter()
                .map(|candidate| candidate.text.clone())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn ctrl_f_lays_the_buffer_out() {
    let mut state = editing("select * from shop.orders where id = 7");

    press_binding(&mut state, ctrl('f'));

    assert_eq!(
        state.queries.text(),
        "SELECT *\nFROM shop.orders\nWHERE id = 7"
    );
}

#[test]
fn ctrl_f_does_nothing_outside_queries() {
    let mut state = editing("select * from shop.orders");
    state.focus_on(Panel::Tables);

    press_binding(&mut state, ctrl('f'));

    assert_eq!(
        state.queries.text(),
        "select * from shop.orders",
        "the chord is the Queries member of the layer and inert elsewhere"
    );
}

#[test]
fn typing_a_prefix_offers_the_keyword_it_starts() {
    let mut state = editing("se");

    assert_eq!(offered(&state).first().map(String::as_str), Some("SELECT"));

    press(&mut state, Key::Char('l'));
    assert_eq!(offered(&state).first().map(String::as_str), Some("SELECT"));
}

#[test]
fn a_qualified_prefix_offers_that_keyspaces_tables() {
    let state = editing("SELECT * FROM shop.o");

    assert_eq!(offered(&state), vec!["orders".to_owned()]);
}

#[test]
fn an_unqualified_table_comes_from_the_keyspace_the_tree_is_on() {
    // `browsing_tables` leaves the Tables cursor on `shop`, which is
    // what makes a bare `orders` completable at all (design §5.2).
    // A `FROM` clause wants a table, so the name the tree is
    // standing on outranks a keyword that happens to share its
    // prefix (design §6).
    let state = editing("SELECT * FROM ord");

    assert_eq!(
        offered(&state),
        vec!["orders".to_owned(), "ORDER".to_owned()],
        "a table position wants a name before it wants grammar"
    );
}

#[test]
fn tab_accepts_what_is_offered_rather_than_moving_focus() {
    let mut state = editing("SELECT * FROM shop.o");

    press(&mut state, Key::Tab);

    assert_eq!(state.queries.text(), "SELECT * FROM shop.orders");
    assert_eq!(state.focus(), Panel::Queries, "focus stayed put");
}

#[test]
fn tab_still_walks_the_cycle_when_nothing_is_being_offered() {
    let mut state = editing("SELECT * FROM shop.orders");

    press(&mut state, Key::Tab);

    assert_eq!(
        state.focus(),
        Panel::Queries,
        "Queries' own second stop, not yet the next panel"
    );
    assert_eq!(state.queries.tab(), QueriesTab::Logs);
}

#[test]
fn escape_takes_the_popup_down_before_it_leaves_the_editor() {
    let mut state = editing("SELECT * FROM shop.o");

    press(&mut state, Key::Escape);
    assert!(state.queries.completion().is_none());
    assert_eq!(
        state.focus(),
        Panel::Queries,
        "the panel is still the way in"
    );

    press(&mut state, Key::Escape);
    assert!(!state.takes_text(), "the next escape closes the buffer");

    press(&mut state, Key::Escape);
    assert_eq!(
        state.focus(),
        Panel::Tables,
        "and escape is still the way out"
    );
}

#[test]
fn a_motion_takes_the_popup_down() {
    let mut state = editing("SELECT * FROM shop.o");

    press(&mut state, Key::Left);

    assert!(state.queries.completion().is_none());
}

#[test]
fn the_arrows_walk_the_popup_while_one_is_up() {
    let mut state = editing("SELECT * FROM s");

    press(&mut state, Key::Down);
    assert_eq!(
        state.queries.completion().expect("still up").selected(),
        1,
        "the popup takes the key the cursor would otherwise have"
    );

    press(&mut state, Key::Up);
    assert_eq!(state.queries.completion().expect("still up").selected(), 0);

    press(&mut state, Key::Left);
    assert!(
        state.queries.completion().is_none(),
        "a horizontal arrow is still a motion, and a motion still \
         takes the popup down"
    );
}

#[test]
fn ctrl_n_raises_a_popup_and_then_walks_it() {
    let mut state = editing("SELECT * FROM s");
    // A motion takes the popup down without moving off the word, so
    // `ctrl-n` has something to raise against.
    press(&mut state, Key::End);
    assert!(state.queries.completion().is_none());

    press_binding(&mut state, ctrl('n'));
    let raised = state
        .queries
        .completion()
        .expect("s reaches a keyword and a keyspace")
        .selected();
    assert_eq!(raised, 0);

    press_binding(&mut state, ctrl('n'));
    assert_eq!(
        state.queries.completion().expect("still up").selected(),
        1,
        "the second press walks the list it raised"
    );

    press_binding(&mut state, ctrl('p'));
    assert_eq!(state.queries.completion().expect("still up").selected(), 0);
}

#[test]
fn the_buffer_reaches_the_renderer_painted() {
    let state = editing("SELECT id");

    let crate::view::PanelBody::Editor { lines, .. } =
        crate::view::view_model(&state).panels[3].body.clone()
    else {
        panic!("Queries draws an editor");
    };

    assert_eq!(
        lines[0]
            .iter()
            .map(|segment| (segment.token, segment.text.clone()))
            .collect::<Vec<_>>(),
        vec![
            (Some(crate::style::Token::Keyword), "SELECT".to_owned()),
            (None, " ".to_owned()),
            (Some(crate::style::Token::Reference), "id".to_owned()),
        ]
    );
}
