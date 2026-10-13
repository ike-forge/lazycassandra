use super::*;

const UUID: &str = "8f14e45f-ceea-467a-9575-9c6d7e0d9f3c";

fn form(state: &State) -> Option<&crate::inspect::Inspector> {
    match &state.overlay {
        Some(Overlay::Inspect(inspector)) if inspector.is_inserting() => Some(inspector),
        _ => None,
    }
}

fn prompt(state: &State) -> Option<&CommitPrompt> {
    match &state.overlay {
        Some(Overlay::Commit(prompt)) => Some(prompt),
        _ => None,
    }
}

/// Types `text` into the `field`th column of the open form, which the cursor
/// reaches by `j` because an unset value is one line.
fn fill(state: &mut State, field: usize, text: &str) {
    press(state, Key::Home);
    for _ in 0..field {
        press(state, Key::Char('j'));
    }
    press(state, Key::Char('e'));
    type_in(state, text);
    press(state, Key::Enter);
}

fn filled_key(state: &mut State) {
    fill(state, 0, UUID);
    fill(state, 1, "2026-01-01");
}

fn inserting(rows: usize) -> State {
    let mut state = editable_grid(rows);
    press(&mut state, Key::Char('i'));
    assert!(form(&state).is_some(), "the fixture opens the form");
    state
}

#[test]
fn i_opens_the_inspector_over_a_row_with_nothing_in_it() {
    let state = inserting(3);

    let form = form(&state).expect("the form");
    assert_eq!(form.typed().count(), 0);
    assert!(form.field().is_none());
    assert_eq!(dirty_cells(&state), 0, "nothing reaches the page");
}

#[test]
fn i_opens_over_a_grid_with_no_rows() {
    let state = inserting(0);

    assert!(form(&state).is_some());
}

#[test]
fn a_read_only_connection_is_refused_with_the_reason() {
    let mut state = editable_grid_at(1, Posture::ReadOnly);

    press(&mut state, Key::Char('i'));

    assert!(
        refusal(&state).is_some_and(|message| message.contains("open read-only")),
        "{:?}",
        refusal(&state)
    );
}

#[test]
fn a_typed_statement_has_no_table_to_insert_into() {
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
            rows: Vec::new(),
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );

    press(&mut state, Key::Char('i'));

    assert!(form(&state).is_none());
    assert_eq!(
        toasted(&state),
        Some((
            Severity::Warning,
            "Only a table's own rows can be inserted. This grid came from a typed statement."
        ))
    );
}

#[test]
fn i_is_inert_outside_table_contents() {
    let mut state = editable_grid(1);
    state.focus_on(Panel::Tables);

    press(&mut state, Key::Char('i'));

    assert!(state.overlay.is_none());
}

#[test]
fn a_key_column_can_be_typed_into_on_a_new_row() {
    let mut state = inserting(1);

    press(&mut state, Key::Char('e'));

    assert!(
        form(&state).is_some_and(|form| form.field().is_some()),
        "the cursor opens on `id`, which an update refuses"
    );
}

#[test]
fn an_accepted_field_is_held_by_the_form_and_not_the_page() {
    let mut state = inserting(1);
    fill(&mut state, 2, "ada@example.com");

    let form = form(&state).expect("the form");
    assert_eq!(form.typed().count(), 1);
    assert_eq!(form.edited(), [2]);
    assert_eq!(dirty_cells(&state), 0);
}

#[test]
fn a_text_field_typed_to_empty_is_an_empty_string() {
    let mut state = inserting(1);
    fill(&mut state, 2, "x");

    press(&mut state, Key::Char('e'));
    press(&mut state, Key::Backspace);
    press(&mut state, Key::Enter);

    let form = form(&state).expect("the form");
    assert_eq!(
        form.typed()
            .map(|(_, edited)| edited.literal())
            .collect::<Vec<_>>(),
        ["''"]
    );
}

#[test]
fn opening_a_text_field_and_accepting_it_untouched_leaves_it_unset() {
    let mut state = inserting(1);

    fill(&mut state, 2, "");

    let form = form(&state).expect("the form");
    assert_eq!(form.typed().count(), 0);
    assert!(form.edited().is_empty());
}

#[test]
fn a_field_typed_then_emptied_over_a_column_that_takes_no_empty_value_unsets_it() {
    let mut state = inserting(1);
    fill(&mut state, 1, "x");

    press(&mut state, Key::Char('e'));
    press(&mut state, Key::Backspace);
    press(&mut state, Key::Enter);

    assert_eq!(form(&state).map(|form| form.typed().count()), Some(0));
}

#[test]
fn an_empty_string_is_written_as_one() {
    let mut state = inserting(1);
    filled_key(&mut state);
    fill(&mut state, 2, "x");
    press(&mut state, Key::Char('e'));
    press(&mut state, Key::Backspace);
    press(&mut state, Key::Enter);

    press(&mut state, Key::Enter);

    assert_eq!(
        prompt(&state).map(|prompt| prompt.statement().cql.clone()),
        Some(format!(
            "INSERT INTO \"shop\".\"orders\" (\"id\", \"created_at\", \"email\") \
             VALUES ({UUID}, '2026-01-01', '')"
        ))
    );
}

#[test]
fn x_unsets_the_field_under_the_cursor() {
    let mut state = inserting(1);
    fill(&mut state, 2, "x");

    press(&mut state, Key::Char('x'));

    let form = form(&state).expect("the form");
    assert_eq!(form.typed().count(), 0);
    assert!(form.edited().is_empty());
}

#[test]
fn a_refused_field_keeps_the_form() {
    let mut state = inserting(1);
    press(&mut state, Key::Char('e'));
    type_in(&mut state, "not a uuid");

    press(&mut state, Key::Enter);

    assert!(
        form(&state).is_some_and(|form| form.field().is_some()),
        "the field stays open with what was typed"
    );
}

#[test]
fn enter_without_the_whole_key_names_what_is_missing_and_keeps_the_form() {
    let mut state = inserting(1);
    fill(&mut state, 0, UUID);

    press(&mut state, Key::Enter);

    assert_eq!(
        toasted(&state),
        Some((
            Severity::Warning,
            "created_at must be set: a row is addressed by its whole primary key."
        ))
    );
    assert!(form(&state).is_some());
}

#[test]
fn enter_shows_the_insert_it_would_run_with_only_what_was_filled_in() {
    let mut state = inserting(1);
    filled_key(&mut state);
    fill(&mut state, 2, "o'hara");

    press(&mut state, Key::Enter);

    assert_eq!(
        prompt(&state).map(|prompt| prompt.statement().cql.clone()),
        Some(format!(
            "INSERT INTO \"shop\".\"orders\" (\"id\", \"created_at\", \"email\") \
             VALUES ({UUID}, '2026-01-01', 'o''hara')"
        ))
    );
}

#[test]
fn a_key_alone_is_a_row() {
    let mut state = inserting(1);
    filled_key(&mut state);

    press(&mut state, Key::Enter);

    assert_eq!(
        prompt(&state).map(|prompt| prompt.statement().cql.clone()),
        Some(format!(
            "INSERT INTO \"shop\".\"orders\" (\"id\", \"created_at\") \
             VALUES ({UUID}, '2026-01-01')"
        ))
    );
}

#[test]
fn n_hands_the_filled_form_back() {
    let mut state = inserting(1);
    filled_key(&mut state);
    press(&mut state, Key::Enter);

    press(&mut state, Key::Char('n'));

    assert_eq!(form(&state).map(|form| form.typed().count()), Some(2));
}

#[test]
fn escape_over_the_confirmation_hands_the_form_back_too() {
    let mut state = inserting(1);
    filled_key(&mut state);
    press(&mut state, Key::Enter);

    press(&mut state, Key::Escape);

    assert!(form(&state).is_some());
}

#[test]
fn y_runs_the_insert() {
    let mut state = inserting(1);
    filled_key(&mut state);
    press(&mut state, Key::Enter);

    let effects = press(&mut state, Key::Char('y'));

    assert!(
        deleted_cql(&effects).is_some_and(|cql| cql.starts_with("INSERT INTO")),
        "{effects:?}"
    );
    assert!(
        state
            .committing
            .as_ref()
            .is_some_and(crate::state::Committing::is_insert)
    );
}

#[test]
fn a_read_only_connection_refuses_the_write_at_the_confirmation_too() {
    let mut state = inserting(1);
    filled_key(&mut state);
    state.connection = match std::mem::take(&mut state.connection) {
        ConnectionState::Connected(mut connection) => {
            connection.posture = Posture::ReadOnly;
            ConnectionState::Connected(connection)
        }
        other => other,
    };

    press(&mut state, Key::Enter);

    assert!(
        refusal(&state).is_some_and(|message| message.contains("open read-only")),
        "{:?}",
        refusal(&state)
    );
}

#[test]
fn a_completed_insert_re_reads_the_grid_rather_than_patching_it() {
    let mut state = inserting(1);
    filled_key(&mut state);
    press(&mut state, Key::Enter);
    press(&mut state, Key::Char('y'));
    let query = state.committing.as_ref().expect("in flight").query();

    let effects = update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page {
                columns: Vec::new(),
                rows: Vec::new(),
                next: None,
            }),
            took: Duration::ZERO,
        },
    );

    assert!(state.committing.is_none());
    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "inserted a row into shop.orders"))
    );
    assert!(
        deleted_cql(&effects).is_some_and(|cql| cql.starts_with("SELECT")),
        "{effects:?}"
    );
}

#[test]
fn a_failed_insert_is_raised_and_nothing_is_re_read() {
    let mut state = inserting(1);
    filled_key(&mut state);
    press(&mut state, Key::Enter);
    press(&mut state, Key::Char('y'));
    let query = state.committing.as_ref().expect("in flight").query();

    let effects = update(
        &mut state,
        Msg::Failed {
            query,
            error: DbError::Unreachable("boom".to_owned()),
            took: Duration::ZERO,
        },
    );

    assert!(effects.is_empty());
    assert!(refusal(&state).is_some_and(|message| message.contains("boom")));
}

#[test]
fn enter_still_closes_an_inspector_over_an_existing_row() {
    let mut state = inspecting_row(1);

    press(&mut state, Key::Enter);

    assert!(state.overlay.is_none());
}

fn insert_hint(state: &State) -> Option<crate::view::Liveness> {
    crate::view::hints::bar(state)
        .panel
        .iter()
        .find(|hint| hint.action == "insert")
        .map(|hint| hint.liveness)
}

#[test]
fn the_contents_bar_names_insert_and_lights_it_where_a_form_would_open() {
    let state = editable_grid(1);

    assert_eq!(insert_hint(&state), Some(crate::view::Liveness::Live));
}

#[test]
fn the_contents_bar_drops_insert_on_a_read_only_connection() {
    let state = editable_grid_at(1, Posture::ReadOnly);

    assert_eq!(insert_hint(&state), None);
}

#[test]
fn the_contents_bar_greys_insert_where_the_frame_has_no_room() {
    let mut state = editable_grid(1);
    state.geometry = Geometry::default();

    assert_eq!(insert_hint(&state), Some(crate::view::Liveness::Inert));
}

#[test]
fn the_form_names_enter_as_insert_and_an_empty_field_may_be_saved() {
    let mut state = inserting(1);
    assert_eq!(insert_hint(&state), Some(crate::view::Liveness::Live));

    press(&mut state, Key::Char('e'));
    let save = crate::view::hints::bar(&state)
        .global
        .iter()
        .find(|hint| hint.action == "save")
        .map(|hint| hint.liveness);

    assert_eq!(
        save,
        Some(crate::view::Liveness::Live),
        "an empty field unsets the column"
    );
}

fn refused_insert(state: &mut State) {
    press(state, Key::Enter);
    press(state, Key::Char('y'));
    let query = state.committing.as_ref().expect("in flight").query();

    update(
        state,
        Msg::Failed {
            query,
            error: DbError::Unreachable("boom".to_owned()),
            took: Duration::ZERO,
        },
    );
    press(state, Key::Escape);
}

#[test]
fn i_gives_back_what_a_refused_insert_held() {
    let mut state = inserting(1);
    filled_key(&mut state);
    fill(&mut state, 2, "ada");
    refused_insert(&mut state);
    assert!(form(&state).is_none(), "the refusal closed the form");

    press(&mut state, Key::Char('i'));

    assert_eq!(form(&state).map(|form| form.typed().count()), Some(3));
}

#[test]
fn a_draft_is_handed_over_once() {
    let mut state = inserting(1);
    filled_key(&mut state);
    refused_insert(&mut state);
    press(&mut state, Key::Char('i'));
    press(&mut state, Key::Escape);

    press(&mut state, Key::Char('i'));

    assert_eq!(form(&state).map(|form| form.typed().count()), Some(0));
}

#[test]
fn a_new_read_drops_the_draft() {
    let mut state = inserting(1);
    filled_key(&mut state);
    refused_insert(&mut state);

    state.contents.requested(
        QueryId::next(),
        Subject::Table(TableRef::new("shop", "customers")),
        crate::model::Statement::select_all("shop", "customers"),
    );

    assert_eq!(state.contents.take_draft(), None);
}

#[test]
fn a_written_insert_leaves_no_draft() {
    let mut state = inserting(1);
    filled_key(&mut state);
    press(&mut state, Key::Enter);
    press(&mut state, Key::Char('y'));
    let query = state.committing.as_ref().expect("in flight").query();

    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page {
                columns: Vec::new(),
                rows: Vec::new(),
                next: None,
            }),
            took: Duration::ZERO,
        },
    );

    assert_eq!(state.contents.take_draft(), None);
}

#[test]
fn ctrl_s_over_the_form_asks_to_insert_rather_than_commit_a_row() {
    let mut state = inserting(1);
    filled_key(&mut state);

    ctrl(&mut state, Key::Char('s'));

    assert!(
        prompt(&state).is_some_and(|prompt| prompt.statement().cql.starts_with("INSERT INTO")),
        "{:?}",
        state.overlay
    );
}
