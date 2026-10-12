use super::*;

fn committing(state: &State) -> Option<&CommitPrompt> {
    match &state.overlay {
        Some(Overlay::Commit(prompt)) => Some(prompt),
        _ => None,
    }
}

#[test]
fn ctrl_s_shows_the_update_it_would_run() {
    let mut state = committable(1);

    ctrl(&mut state, Key::Char('s'));

    let prompt = committing(&state).expect("the confirmation is up");
    assert_eq!(
        prompt.statement().cql,
        "UPDATE \"shop\".\"orders\" SET \"email\" = 'ada@example.com!' \
         WHERE \"id\" = 8f14e45f-0000 AND \"created_at\" = 1699999999000"
    );
}

#[test]
fn several_edited_rows_become_one_batch() {
    let mut state = committable(2);

    ctrl(&mut state, Key::Char('s'));

    let prompt = committing(&state).expect("the confirmation is up");
    assert!(
        prompt.statement().cql.starts_with("BEGIN BATCH"),
        "{}",
        prompt.statement().cql
    );
    let drawn: Vec<String> = prompt
        .lines()
        .iter()
        .map(|line| crate::style::text(line))
        .collect();
    assert_eq!(drawn.first().map(String::as_str), Some("BEGIN BATCH"));
    assert_eq!(drawn.last().map(String::as_str), Some("APPLY BATCH"));
    assert_eq!(
        drawn
            .iter()
            .filter(|line| line.starts_with("  UPDATE "))
            .count(),
        2,
        "one verb per edited row, indented under the brackets"
    );
}

#[test]
fn ctrl_s_with_nothing_dirty_is_inert() {
    let mut state = editable_grid(3);

    ctrl(&mut state, Key::Char('s'));

    assert!(committing(&state).is_none());
    assert_eq!(toasted(&state), Some((Severity::Info, "nothing to commit")));
}

#[test]
fn y_runs_the_statement_and_n_leaves_the_edits_standing() {
    let mut state = committable(1);
    ctrl(&mut state, Key::Char('s'));

    press(&mut state, Key::Char('n'));
    assert!(committing(&state).is_none());
    assert_eq!(dirty_cells(&state), 1, "cancelling writes nothing back");

    ctrl(&mut state, Key::Char('s'));
    let effects = press(&mut state, Key::Char('y'));

    assert!(
        deleted_cql(&effects).is_some_and(|cql| cql.starts_with("UPDATE")),
        "{effects:?}"
    );
    assert!(state.committing.is_some());
}

#[test]
fn a_committed_write_lands_in_the_page_and_clears_the_marks() {
    let mut state = committable(1);
    ctrl(&mut state, Key::Char('s'));
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

    assert_eq!(dirty_cells(&state), 0);
    assert!(state.committing.is_none());
    assert!(
        matches!(toasted(&state), Some((Severity::Info, message)) if message.starts_with("committed 1 cell to ")),
    );
    assert_eq!(
        state
            .contents
            .loaded()
            .and_then(|page| page.page().rows.first())
            .and_then(|row| row.values.get(2))
            .map(crate::cell::text)
            .as_deref(),
        Some("ada@example.com!"),
        "the page holds what the cluster was sent"
    );
}

#[test]
fn a_failed_commit_leaves_the_edits_dirty() {
    let mut state = committable(1);
    ctrl(&mut state, Key::Char('s'));
    press(&mut state, Key::Char('y'));
    let query = state.committing.as_ref().expect("in flight").query();

    update(
        &mut state,
        Msg::Failed {
            query,
            error: DbError::Unreachable("boom".to_owned()),
            took: Duration::ZERO,
        },
    );

    assert_eq!(dirty_cells(&state), 1);
    assert!(state.committing.is_none());
    assert!(
        refusal(&state).is_some_and(|message| message.contains("boom")),
        "{:?}",
        refusal(&state)
    );
}

#[test]
fn q_warns_before_it_discards_and_y_quits_through_the_warning() {
    let mut state = committable(1);

    let effects = press(&mut state, Key::Char('q'));

    assert!(effects.is_empty(), "nothing quits behind the question");
    assert!(!state.should_quit);
    assert!(matches!(state.overlay, Some(Overlay::Discard(_))));

    // A second `q` is inert: the question has to be answered (design §6).
    let effects = press(&mut state, Key::Char('q'));
    assert!(effects.is_empty());
    assert!(!state.should_quit);

    let effects = press(&mut state, Key::Char('y'));
    assert!(state.should_quit);
    assert!(effects.contains(&Effect::Quit));
}

#[test]
fn n_leaves_the_warning_and_the_edits_where_they_were() {
    let mut state = committable(1);
    press(&mut state, Key::Char('q'));

    press(&mut state, Key::Char('n'));

    assert!(state.overlay.is_none());
    assert!(!state.should_quit);
    assert_eq!(dirty_cells(&state), 1);
}

#[test]
fn q_with_nothing_dirty_still_quits_outright() {
    let mut state = editable_grid(1);

    let effects = press(&mut state, Key::Char('q'));

    assert!(state.should_quit);
    assert!(effects.contains(&Effect::Quit));
}

#[test]
fn the_bar_greys_e_where_the_column_cannot_be_edited_and_lights_it_where_it_can() {
    let named = |state: &State| {
        crate::view::hints::bar(state)
            .panel
            .iter()
            .find(|hint| hint.action == "edit")
            .map(|hint| hint.liveness)
    };

    // The cursor opens on `id`, which is a key column.
    let mut state = inspecting_row(3);
    assert_eq!(named(&state), Some(crate::view::Liveness::Inert));

    on_email(&mut state);
    assert_eq!(named(&state), Some(crate::view::Liveness::Live));
}

#[test]
fn an_open_field_names_only_the_two_keys_it_answers() {
    let mut state = inspecting_row(3);
    on_email(&mut state);
    press(&mut state, Key::Char('e'));

    let bar = crate::view::hints::bar(&state);

    assert_eq!(
        bar.panel
            .iter()
            .chain(&bar.global)
            .map(|hint| format!("{} {}", hint.keys, hint.action))
            .collect::<Vec<_>>(),
        ["⏎ save", "esc cancel"]
    );
}

#[test]
fn the_save_hint_greys_while_the_text_does_not_parse() {
    let mut state = editable_grid(1);
    // `created_at` is a key column, so the one non-key typed column is
    // `email`; a type that refuses text needs a column of its own.
    press(&mut state, Key::Enter);
    on_email(&mut state);
    press(&mut state, Key::Char('e'));

    let live = |state: &State| {
        crate::view::hints::bar(state)
            .global
            .iter()
            .find(|hint| hint.action == "save")
            .map(|hint| hint.liveness)
    };

    assert_eq!(
        live(&state),
        Some(crate::view::Liveness::Live),
        "text takes anything"
    );
}

#[test]
fn the_contents_bar_names_commit_only_while_something_is_uncommitted() {
    let named = |state: &State| {
        crate::view::hints::bar(state)
            .panel
            .iter()
            .any(|hint| hint.action == "commit")
    };

    assert!(!named(&editable_grid(3)));
    assert!(named(&committable(1)));
}

/// The cursor counts drawn rows and `GridPage::order` is what resolves one,
/// so `d` has to ask the page rather than index its unarranged rows — a
/// sorted grid otherwise confirms one row and deletes another.
#[test]
fn d_over_a_sorted_grid_asks_about_the_row_the_cursor_is_actually_on() {
    let mut state = deletable(3);
    // Descending on `id`, which reverses the three fixture rows.
    state.contents.cycle_sort(0);

    press(&mut state, Key::Char('d'));

    let prompt = dropping(&state).expect("the confirmation is up");
    assert_eq!(
        prompt.statement().cql,
        "DELETE FROM \"shop\".\"orders\" WHERE \"id\" = 8f14e45f-0002 \
         AND \"created_at\" = 1699999999002",
        "the cursor is on the first drawn row, which sorting made the last fetched one"
    );
}
