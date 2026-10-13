use super::*;

fn field_text(state: &State) -> Option<String> {
    match &state.overlay {
        Some(Overlay::Inspect(inspector)) => Some(inspector.field()?.line().text().to_owned()),
        _ => None,
    }
}

#[test]
fn e_opens_a_field_seeded_with_what_the_cell_says() {
    let mut state = inspecting_row(3);
    on_email(&mut state);

    press(&mut state, Key::Char('e'));

    assert_eq!(field_text(&state).as_deref(), Some("ada@example.com"));
}

#[test]
fn e_on_the_grid_opens_the_inspector_on_a_field() {
    let mut state = editable_grid(1);
    for key in [
        Key::Char('m'),
        Key::Char('m'),
        Key::Char('l'),
        Key::Char('l'),
    ] {
        press(&mut state, key);
    }

    press(&mut state, Key::Char('e'));

    assert_eq!(field_text(&state).as_deref(), Some("ada@example.com"));
}

#[test]
fn a_cell_inspector_edits_the_column_the_cursor_was_on() {
    let mut state = editable_grid(1);
    for key in [
        Key::Char('m'),
        Key::Char('m'),
        Key::Char('l'),
        Key::Char('l'),
        Key::Enter,
        Key::Char('e'),
    ] {
        press(&mut state, key);
    }

    assert_eq!(field_text(&state).as_deref(), Some("ada@example.com"));
}

#[test]
fn e_on_the_grid_is_refused_as_enter_then_e_is() {
    let mut state = editable_grid(1);
    press(&mut state, Key::Char('e'));

    assert_eq!(
        refusal(&state).as_deref(),
        Some("id is part of the primary key. A key column cannot be updated.")
    );
}

#[test]
fn e_on_a_grid_that_inspects_nothing_does_nothing() {
    let mut state = editable_grid(1);
    press(&mut state, Key::Char('m'));

    press(&mut state, Key::Char('e'));

    assert!(state.overlay.is_none());
}

#[test]
fn a_key_column_is_refused_with_the_reason_rather_than_silently() {
    let mut state = inspecting_row(3);

    press(&mut state, Key::Char('e'));

    assert_eq!(
        refusal(&state).as_deref(),
        Some("id is part of the primary key. A key column cannot be updated.")
    );
}

#[test]
fn a_read_only_connection_is_refused_before_anything_else_is_asked() {
    let mut state = editable_grid_at(1, Posture::ReadOnly);
    press(&mut state, Key::Enter);
    on_email(&mut state);

    press(&mut state, Key::Char('e'));

    assert!(
        refusal(&state).is_some_and(|message| message.contains("open read-only")),
        "{:?}",
        refusal(&state)
    );
}

#[test]
fn a_typed_statement_has_no_table_to_update() {
    let mut state = editable_grid(1);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Typed("SELECT * FROM shop.orders".to_owned()),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: vec!["email".to_owned()],
            rows: vec![Row {
                values: vec![CqlValue::Text("ada@example.com".to_owned())],
            }],
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    press(&mut state, Key::Enter);

    press(&mut state, Key::Char('e'));

    assert_eq!(
        refusal(&state).as_deref(),
        Some("Only a table's own rows can be edited. This grid came from a typed statement.")
    );
}

#[test]
fn enter_accepts_the_field_and_the_cell_goes_dirty() {
    let mut state = inspecting_row(3);
    on_email(&mut state);
    press(&mut state, Key::Char('e'));
    type_in(&mut state, "!");

    press(&mut state, Key::Enter);

    assert_eq!(field_text(&state), None, "the field closed");
    assert_eq!(dirty_cells(&state), 1);
    assert_eq!(
        toasted(&state),
        Some((
            Severity::Info,
            "edit staged, close the inspector and ctrl-s to commit"
        ))
    );
    assert_eq!(
        state
            .contents
            .loaded()
            .and_then(|page| page.value(0, 2))
            .map(crate::cell::text)
            .as_deref(),
        Some("ada@example.com!")
    );
}

#[test]
fn an_edit_that_restores_the_value_it_started_from_leaves_no_mark() {
    let mut state = inspecting_row(3);
    on_email(&mut state);
    press(&mut state, Key::Char('e'));
    type_in(&mut state, "!");
    press(&mut state, Key::Enter);
    assert_eq!(dirty_cells(&state), 1);

    press(&mut state, Key::Char('e'));
    press(&mut state, Key::Backspace);
    press(&mut state, Key::Enter);

    assert_eq!(dirty_cells(&state), 0);
}

#[test]
fn escape_abandons_the_field_and_changes_nothing() {
    let mut state = inspecting_row(3);
    on_email(&mut state);
    press(&mut state, Key::Char('e'));
    type_in(&mut state, "!");

    press(&mut state, Key::Escape);

    assert_eq!(field_text(&state), None);
    assert_eq!(dirty_cells(&state), 0);
    assert!(
        matches!(state.overlay, Some(Overlay::Inspect(_))),
        "`esc` closes the field, not the block under it"
    );
}

/// The edits are keyed by page row, so a sort that re-orders the grid
/// carries each one with the row it was made on (design §4).
#[test]
fn an_edit_survives_a_sort_that_moves_its_row() {
    let mut state = inspecting_row(3);
    on_email(&mut state);
    press(&mut state, Key::Char('e'));
    type_in(&mut state, "!");
    press(&mut state, Key::Enter);
    press(&mut state, Key::Escape);

    state.contents.cycle_sort(0);
    state.contents.cycle_sort(0);

    let page = state.contents.loaded().expect("a page");
    assert_eq!(page.edits().cells(), 1);
    assert!(
        (0..3).any(|at| page.is_edited(at, 2)),
        "the mark followed its row"
    );
}

#[test]
fn a_re_read_discards_the_uncommitted_edits() {
    let mut state = inspecting_row(3);
    on_email(&mut state);
    press(&mut state, Key::Char('e'));
    type_in(&mut state, "!");
    press(&mut state, Key::Enter);
    press(&mut state, Key::Escape);

    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "orders")),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: vec!["id".to_owned(), "created_at".to_owned(), "email".to_owned()],
            rows: Vec::new(),
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );

    assert_eq!(dirty_cells(&state), 0);
}
