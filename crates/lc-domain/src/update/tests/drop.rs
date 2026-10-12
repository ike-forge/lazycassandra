use super::*;

/// The cursor on the keyspace row of `droppable_tree`.
fn on_keyspace() -> State {
    droppable_tree()
}

/// The cursor on the table row below it.
fn on_table() -> State {
    let mut state = droppable_tree();
    press(&mut state, Key::Char('j'));
    state
}

#[test]
fn d_on_a_keyspace_asks_for_its_name() {
    let mut state = on_keyspace();

    press(&mut state, Key::Char('d'));

    let prompt = dropping(&state).expect("the confirmation is up");
    assert_eq!(prompt.target(), &Target::Keyspace("shop".to_owned()));
    assert!(prompt.takes_text(), "a keyspace is always typed back");
}

#[test]
fn d_on_a_table_asks_yes_or_no() {
    let mut state = on_table();

    press(&mut state, Key::Char('d'));

    let prompt = dropping(&state).expect("the confirmation is up");
    assert_eq!(
        prompt.target(),
        &Target::Table(TableRef::new("shop", "orders"))
    );
    assert!(!prompt.takes_text());
}

#[test]
fn a_protected_connection_types_a_table_name_back() {
    let mut state = on_table();
    let ConnectionState::Connected(connection) = &mut state.connection else {
        panic!("the fixture is connected");
    };
    connection.posture = Posture::Protected;

    press(&mut state, Key::Char('d'));

    assert!(
        dropping(&state)
            .expect("the confirmation is up")
            .takes_text(),
        "a protected connection needs the name typed, whatever the target"
    );
}

#[test]
fn y_drops_the_table() {
    let mut state = on_table();
    press(&mut state, Key::Char('d'));

    let effects = press(&mut state, Key::Char('y'));

    assert!(state.overlay.is_none(), "the question is answered");
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::Execute { stmt, .. } if stmt.cql == "DROP TABLE \"shop\".\"orders\""
        )),
        "{effects:?}"
    );
    assert!(state.dropping.is_some(), "the drop is outstanding");
}

#[test]
fn n_abandons_the_drop() {
    let mut state = on_table();
    press(&mut state, Key::Char('d'));

    let effects = press(&mut state, Key::Char('n'));

    assert!(state.overlay.is_none());
    assert!(effects.is_empty(), "nothing reaches the cluster");
    assert!(state.dropping.is_none());
}

#[test]
fn esc_abandons_the_drop_on_either_spelling() {
    for mut state in [on_table(), on_keyspace()] {
        press(&mut state, Key::Char('d'));
        let effects = press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
        assert!(effects.is_empty());
    }
}

#[test]
fn enter_on_a_mismatched_name_does_nothing() {
    let mut state = on_keyspace();
    press(&mut state, Key::Char('d'));
    type_in(&mut state, "sho");

    let effects = press(&mut state, Key::Enter);

    assert!(effects.is_empty(), "a half-typed name drops nothing");
    assert!(
        dropping(&state).is_some(),
        "and the question is still being asked"
    );
}

#[test]
fn enter_on_the_typed_name_drops_the_keyspace() {
    let mut state = on_keyspace();
    press(&mut state, Key::Char('d'));
    type_in(&mut state, "shop");

    let effects = press(&mut state, Key::Enter);

    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::Execute { stmt, .. } if stmt.cql == "DROP KEYSPACE \"shop\""
        )),
        "{effects:?}"
    );
}

/// `n` cannot be a cancel where a name holding one has to be typeable
/// (design §2.1).
#[test]
fn n_is_text_on_a_typed_confirmation() {
    let mut state = connected(Posture::Writable);
    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(tabled_schema(&[("nation", &["orders"])])),
            took: Duration::ZERO,
        },
    );
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14).with_drop_fits(true);

    press(&mut state, Key::Char('d'));
    type_in(&mut state, "nation");

    let effects = press(&mut state, Key::Enter);

    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::Execute { stmt, .. } if stmt.cql == "DROP KEYSPACE \"nation\""
        )),
        "every `n` typed itself; {effects:?}"
    );
}

#[test]
fn a_typed_confirmation_takes_text_rather_than_quitting_or_moving() {
    let mut state = on_keyspace();
    press(&mut state, Key::Char('d'));
    let before = state.tables.cursor.index();

    for key in ['q', 'j', 'G', 'y', '1'] {
        press(&mut state, Key::Char(key));
    }

    assert!(!state.should_quit, "`q` is a character here");
    assert_eq!(state.tables.cursor.index(), before);
    assert_eq!(state.focus(), Panel::Tables);
    assert_eq!(
        dropping(&state).and_then(DropPrompt::typed).map(Line::text),
        Some("qjGy1")
    );
}

#[test]
fn a_yes_no_confirmation_swallows_the_keys_it_does_not_use() {
    let mut state = on_table();
    press(&mut state, Key::Char('d'));
    let before = crate::view::view_model(&state);

    for key in ['q', 'j', '?', 'v', 'm'] {
        press(&mut state, Key::Char(key));
    }

    assert!(!state.should_quit);
    assert_eq!(crate::view::view_model(&state), before);
}

#[test]
fn d_on_a_read_only_connection_asks_nothing_and_says_why() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.geometry = state.geometry.with_drop_fits(true);

    press(&mut state, Key::Char('d'));

    assert!(
        !matches!(state.overlay, Some(Overlay::Drop(_))),
        "a question the statement's own guard would refuse is not asked"
    );
    assert!(
        matches!(&state.overlay, Some(Overlay::Message(message))
            if message.title == "Read-only connection"),
        "the reason the key did nothing is the reason the user is owed"
    );
}

/// The key is greyed *and* answered. Greying alone leaves a user who
/// pressed it with nothing, and the bar cannot say which connection is
/// the read-only one (posture design §3).
#[test]
fn the_read_only_refusal_names_the_way_out_of_it() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    state.geometry = state.geometry.with_drop_fits(true);

    press(&mut state, Key::Char('d'));

    let Some(Overlay::Message(message)) = &state.overlay else {
        panic!("refused with a modal");
    };
    assert!(message.message.contains("writable"), "{}", message.message);
    assert_eq!(message.severity, Severity::Danger);
}

#[test]
fn d_is_inert_where_the_frame_cannot_draw_the_block() {
    let mut state = on_table();
    state.geometry = state.geometry.with_drop_fits(false);

    press(&mut state, Key::Char('d'));

    assert!(state.overlay.is_none());
    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "no room to draw the confirmation"))
    );
}

#[test]
fn d_is_inert_while_a_drop_is_running() {
    let mut state = on_table();
    press(&mut state, Key::Char('d'));
    press(&mut state, Key::Char('y'));
    assert!(state.dropping.is_some(), "one is outstanding");

    press(&mut state, Key::Escape);
    press(&mut state, Key::Char('d'));

    assert!(state.overlay.is_none(), "the second would race the first");
    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "a drop is already running"))
    );
}

#[test]
fn d_is_inert_over_a_multi_row_selection() {
    let mut state = on_keyspace();
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('d'));

    assert!(
        state.overlay.is_none(),
        "'drop these two' is a different question"
    );
    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "drop takes one node, not a selection"))
    );
}

#[test]
fn d_does_nothing_outside_the_tree() {
    for panel in [Panel::Ddl, Panel::Contents] {
        let mut state = on_table();
        state.focus_on(panel);

        press(&mut state, Key::Char('d'));

        assert!(state.overlay.is_none(), "{panel:?} answers no drop");
    }
}

#[test]
fn a_completed_drop_re_reads_the_schema() {
    let mut state = on_table();
    press(&mut state, Key::Char('d'));
    press(&mut state, Key::Char('y'));
    let query = state
        .dropping
        .as_ref()
        .expect("the drop is outstanding")
        .query();

    let effects = update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page::default()),
            took: Duration::ZERO,
        },
    );

    assert_eq!(effects, vec![Effect::LoadSchema(current_conn(&state))]);
    assert!(state.dropping.is_none(), "the round trip is over");
    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "dropped shop.orders"))
    );
    assert!(state.overlay.is_none(), "nothing to dismiss");
}

#[test]
fn a_failed_drop_reloads_nothing_and_says_why() {
    let mut state = on_table();
    press(&mut state, Key::Char('d'));
    press(&mut state, Key::Char('y'));
    let query = state
        .dropping
        .as_ref()
        .expect("the drop is outstanding")
        .query();

    let effects = update(
        &mut state,
        Msg::Failed {
            query,
            error: DbError::Unreachable("no route".to_owned()),
            took: Duration::ZERO,
        },
    );

    assert!(effects.is_empty(), "the schema did not change");
    assert!(state.dropping.is_none());
    assert!(
        matches!(&state.overlay, Some(Overlay::Message(message))
            if message.severity == Severity::Danger && message.message.contains("no route")),
        "the cluster's own reason is what is raised"
    );
}

/// The trail is where the reason survives the modal (design §5).
#[test]
fn a_drop_the_posture_refuses_is_logged_and_raised() {
    let mut state = on_table();
    press(&mut state, Key::Char('d'));
    // Between the question and the answer the connection turns read-only.
    let ConnectionState::Connected(connection) = &mut state.connection else {
        panic!("the fixture is connected");
    };
    connection.posture = Posture::ReadOnly;

    let effects = press(&mut state, Key::Char('y'));

    assert!(effects.is_empty(), "the posture guard is in `submit`");
    assert!(matches!(&state.overlay, Some(Overlay::Message(_))));
    assert!(
        state
            .log
            .entries()
            .any(|entry| matches!(entry.outcome(), Outcome::Rejected { .. })),
        "the trail keeps the refusal the modal loses"
    );
}

/// A result for a drop that is no longer outstanding must not reload the
/// schema behind a user who has moved on (architecture.md §3).
#[test]
fn a_page_for_an_ordinary_read_reloads_nothing() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");

    let effects = update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(page_of(3)),
            took: Duration::ZERO,
        },
    );

    assert!(effects.is_empty());
}

#[test]
fn a_frame_that_shrank_past_the_drop_block_closes_it() {
    let mut state = on_table();
    press(&mut state, Key::Char('d'));

    let shrunk = state.geometry.with_drop_fits(false);
    update(&mut state, Msg::Measured(shrunk));

    assert!(
        state.overlay.is_none(),
        "a mode nothing draws still capturing every key is the failure this prevents"
    );
}

#[test]
fn a_second_d_leaves_the_open_confirmation_as_it_was() {
    let mut state = on_keyspace();
    press(&mut state, Key::Char('d'));
    type_in(&mut state, "sho");

    press(&mut state, Key::Char('d'));

    assert_eq!(
        dropping(&state).and_then(DropPrompt::typed).map(Line::text),
        Some("shod"),
        "`d` is a character in the field, not a second opener"
    );
}

#[test]
fn d_over_a_grid_asks_about_the_row_under_the_cursor() {
    let mut state = deletable(3);

    press(&mut state, Key::Char('d'));

    let prompt = dropping(&state).expect("the confirmation is up");
    assert_eq!(prompt.target().row_count(), Some(1));
    assert!(
        !prompt.takes_text(),
        "a delete is answered y/n on a writable connection"
    );
}

#[test]
fn d_over_a_grid_asks_about_every_selected_row() {
    let mut state = deletable(3);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('d'));

    assert_eq!(
        dropping(&state)
            .expect("the confirmation is up")
            .target()
            .row_count(),
        Some(3),
        "the range is what a command over the grid acts on"
    );
}

#[test]
fn y_deletes_the_row_by_its_whole_primary_key() {
    let mut state = deletable(3);
    press(&mut state, Key::Char('d'));

    let effects = press(&mut state, Key::Char('y'));

    assert_eq!(
        deleted_cql(&effects).as_deref(),
        Some(
            "DELETE FROM \"shop\".\"orders\" WHERE \"id\" = 8f14e45f-0000 \
             AND \"created_at\" = 1699999999000"
        )
    );
    assert!(state.dropping.is_some(), "the delete is outstanding");
}

#[test]
fn several_rows_delete_as_one_batch() {
    let mut state = deletable(3);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('d'));
    let effects = press(&mut state, Key::Char('y'));

    let cql = deleted_cql(&effects).expect("a statement");
    assert!(cql.starts_with("BEGIN BATCH\n"), "{cql}");
    assert!(cql.ends_with("APPLY BATCH"), "{cql}");
    assert_eq!(cql.matches("DELETE FROM").count(), 2, "{cql}");
}

#[test]
fn n_abandons_a_delete() {
    let mut state = deletable(3);
    press(&mut state, Key::Char('d'));

    let effects = press(&mut state, Key::Char('n'));

    assert!(state.overlay.is_none());
    assert!(effects.is_empty());
    assert!(state.dropping.is_none());
}

/// The "primary key known and present" gate (design §3.1), each half of
/// it on its own.
#[test]
fn d_is_inert_where_the_rows_cannot_be_addressed() {
    let missing_key_column = grid_over(
        2,
        &["email"],
        |_| vec![CqlValue::Text("ada@example.com".to_owned())],
        Posture::Writable,
    );
    let unspellable_value = grid_over(
        2,
        &["id", "created_at"],
        |_| vec![CqlValue::Uuid("8f14e45f".to_owned()), CqlValue::Null],
        Posture::Writable,
    );

    for (mut state, why) in [
        (missing_key_column, "a page without every key column"),
        (unspellable_value, "a key column holding no literal"),
    ] {
        press(&mut state, Key::Char('d'));

        assert!(state.overlay.is_none(), "{why} addresses no row");
    }
}

#[test]
fn d_is_inert_over_a_grid_filled_by_a_typed_statement() {
    let mut state = deletable(2);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Typed("SELECT * FROM shop.orders".to_owned()),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: vec!["id".to_owned(), "created_at".to_owned()],
            rows: vec![Row {
                values: vec![
                    CqlValue::Uuid("8f14e45f".to_owned()),
                    CqlValue::Timestamp(0),
                ],
            }],
            next: None,
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );

    press(&mut state, Key::Char('d'));

    assert!(
        state.overlay.is_none(),
        "a typed statement names its table in text this will not trust"
    );
}

#[test]
fn d_is_inert_over_a_grid_with_no_page() {
    let mut state = connected(Posture::Writable);
    state.geometry = Geometry::default().with_drop_fits(true);
    state.focus_on(Panel::Contents);

    press(&mut state, Key::Char('d'));

    assert!(state.overlay.is_none());
}

#[test]
fn a_delete_is_refused_on_a_read_only_connection() {
    let mut state = grid_over(
        2,
        &["id", "created_at"],
        |_| {
            vec![
                CqlValue::Uuid("8f14e45f".to_owned()),
                CqlValue::Timestamp(0),
            ]
        },
        Posture::ReadOnly,
    );

    press(&mut state, Key::Char('d'));

    assert!(
        matches!(&state.overlay, Some(Overlay::Message(message))
            if message.title == "Read-only connection"),
        "the grid's `d` is refused by the same posture gate the tree's is"
    );
}

/// A `Protected` connection is about *destructive* statements, and a row
/// delete is a `Write` — so it stays y/n (design §2).
#[test]
fn a_protected_connection_still_answers_a_delete_with_y() {
    let mut state = deletable(2);
    let ConnectionState::Connected(connection) = &mut state.connection else {
        panic!("the fixture is connected");
    };
    connection.posture = Posture::Protected;

    press(&mut state, Key::Char('d'));

    assert!(
        !dropping(&state)
            .expect("the confirmation is up")
            .takes_text(),
        "the posture rule names destructive statements, which a delete is not"
    );
}

/// What went stale is what gets re-read: a delete changed the rows of one
/// table, and the grid is showing them (design §4).
#[test]
fn a_completed_delete_re_reads_the_grid_rather_than_the_schema() {
    let mut state = deletable(3);
    press(&mut state, Key::Char('d'));
    press(&mut state, Key::Char('y'));
    let query = state
        .dropping
        .as_ref()
        .expect("the delete is outstanding")
        .query();

    let effects = update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page::default()),
            took: Duration::ZERO,
        },
    );

    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadSchema(_))),
        "the schema did not change"
    );
    assert_eq!(
        deleted_cql(&effects).as_deref(),
        Some("SELECT * FROM \"shop\".\"orders\""),
        "the grid is re-read so the rows stop being drawn"
    );
    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "deleted 1 row from shop.orders"))
    );
}
