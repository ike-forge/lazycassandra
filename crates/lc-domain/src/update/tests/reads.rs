use super::*;

/// Cancels `query` via `Msg::Quit`, which completes every in-flight trace
/// without pushing an entry of its own — unlike superseding it with a
/// second read, which would leave the newer entry as the log's last one.
fn cancel(state: &mut State, query: QueryId) {
    assert!(
        state.in_flight.contains_key(&query),
        "the query being cancelled is the one still in flight"
    );
    let _ = update(state, Msg::Quit);
}

#[test]
fn a_stated_column_width_is_the_grids_cap() {
    let mut state = read_shop_orders();
    state.config.grid.max_column_width =
        crate::config::MaxColumnWidth::try_new(40).expect("in range");
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the read is waiting");

    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(Page {
                columns: vec!["note".to_owned()],
                rows: vec![crate::model::Row {
                    values: vec![crate::model::CqlValue::Text("x".repeat(100))],
                }],
                next: None,
            }),
            took: Duration::ZERO,
        },
    );

    assert_eq!(state.contents.loaded().expect("a page").widths(), [40]);
}

fn statement_subject_from(cql: &str) -> LogSubject {
    statement_subject(&Statement::typed(cql))
}

fn executed_query(effects: &[Effect]) -> QueryId {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Execute { query, .. } | Effect::ExecutePaged { query, .. } => Some(*query),
            _ => None,
        })
        .expect("an Execute effect carrying the query")
}

// The trail records what ran (query log design §6).
#[test]
fn opening_a_table_logs_the_statement_it_issued() {
    let mut state = read_shop_orders();

    let _effects = press(&mut state, Key::Enter);

    let entry = state.log.entries().last().expect("an entry per effect");
    let text = entry.subject().text();
    assert!(text.starts_with("SELECT"), "{text}");
    assert!(
        !text.contains("LIMIT"),
        "the grid pages rather than limits: {text}"
    );
    assert!(matches!(
        entry.outcome(),
        Outcome::Pending {
            trace: Trace::Query(_)
        }
    ));
}

#[test]
fn a_statement_is_logged_as_one_line_however_it_was_laid_out() {
    let mut state = State::default();

    logged(
        &mut state,
        statement_subject_from("SELECT *\n  FROM ks.t\n  WHERE id = 1"),
        Outcome::Cancelled,
    );

    assert_eq!(
        state.log.entries().next().unwrap().subject().text(),
        "SELECT * FROM ks.t WHERE id = 1"
    );
}

#[test]
fn a_returned_page_completes_the_entry_with_its_row_count() {
    let mut state = read_shop_orders();
    let effects = press(&mut state, Key::Enter);
    let query = executed_query(&effects);

    let _effects = update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(page_of(3)),
            took: Duration::from_millis(41),
        },
    );

    let Outcome::Succeeded { detail, took } = state.log.entries().last().unwrap().outcome() else {
        panic!("the query succeeded");
    };
    assert!(detail.contains('3'), "{detail}");
    assert_eq!(*took, Duration::from_millis(41));
}

#[test]
fn a_failed_query_leaves_its_reason_in_the_trail() {
    let mut state = read_shop_orders();
    let effects = press(&mut state, Key::Enter);
    let query = executed_query(&effects);

    let _effects = update(
        &mut state,
        Msg::Failed {
            query,
            error: DbError::Invalid("unconfigured column id".to_owned()),
            took: Duration::ZERO,
        },
    );

    let Outcome::Failed { detail } = state.log.entries().last().unwrap().outcome() else {
        panic!("the query failed");
    };
    assert!(detail.contains("unconfigured column id"), "{detail}");
}

#[test]
fn a_cancelled_query_is_closed_out_rather_than_left_hanging() {
    let mut state = read_shop_orders();
    let effects = press(&mut state, Key::Enter);
    let query = executed_query(&effects);

    cancel(&mut state, query);

    assert!(matches!(
        state.log.entries().last().unwrap().outcome(),
        Outcome::Cancelled
    ));
}

#[test]
fn enter_on_a_table_reads_it() {
    let mut state = read_shop_orders();
    let conn = current_conn(&state);

    let effects = press(&mut state, Key::Enter);

    let query = state.contents.outstanding().expect("the panel is waiting");
    assert_eq!(
        effects,
        vec![
            Effect::ExecutePaged {
                conn,
                query,
                stmt: Box::new(Statement::select_all("shop", "orders")),
                page_size: crate::config::PageSize::try_new(200).expect("in range"),
                cursor: None,
            },
            Effect::RecordHistory(Box::new(Statement::select_all("shop", "orders"))),
        ]
    );
    assert!(state.in_flight.contains_key(&query));
}

#[test]
fn space_reads_no_table() {
    let mut state = read_shop_orders();

    press(&mut state, Key::Char(' '));

    assert!(
        state.contents.outstanding().is_none(),
        "space is free, not a second name for ⏎"
    );
}

#[test]
fn reading_a_second_table_cancels_the_first() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let first = state.contents.outstanding().expect("the panel is waiting");
    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('j'));

    let effects = press(&mut state, Key::Enter);

    assert_eq!(
        effects.first(),
        Some(&Effect::Cancel(first)),
        "the superseded query is cancelled before the new one is issued"
    );
    assert!(
        !state.in_flight.contains_key(&first),
        "a superseded query must not still be awaited"
    );
    let second = state.contents.outstanding().expect("waiting on the second");
    assert_ne!(second, first);
}

#[test]
fn reading_a_table_lands_focus_in_contents() {
    let mut state = tabled(&[("shop", &["orders"])]);
    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Enter);

    assert_eq!(state.focus(), Panel::Contents);
}

#[test]
fn a_click_on_a_table_lands_focus_in_contents_too() {
    let mut state = tabled(&[("shop", &["orders"])]);
    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('l'));

    click_row(&mut state, Panel::Tables, 1);

    assert_eq!(state.focus(), Panel::Contents);
}

#[test]
fn focus_moves_before_the_page_arrives() {
    let mut state = tabled(&[("shop", &["orders"])]);
    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Enter);

    assert!(
        matches!(state.contents.state(), ContentsState::Loading { .. }),
        "focus is a consequence of the keypress, not of the network"
    );
    assert_eq!(state.focus(), Panel::Contents);
}

#[test]
fn a_refused_read_still_lands_focus_in_contents() {
    // No connection, so `submit` refuses before a query is made. `read`
    // is called directly because the tree can only select a table when
    // its schema is loaded, which itself requires a connection — the two
    // cannot be pulled apart through key presses.
    let mut state = tabled(&[("shop", &["orders"])]);
    state.connection = ConnectionState::Disconnected;
    state.focus_on(Panel::Tables);

    read(&mut state, TableRef::new("shop", "orders"));

    assert_eq!(state.focus(), Panel::Contents);
}

#[test]
fn unfolding_a_keyspace_leaves_focus_in_tables() {
    let mut state = tabled(&[("shop", &["orders"])]);
    state.focus_on(Panel::Tables);

    press(&mut state, Key::Enter);

    assert_eq!(
        state.focus(),
        Panel::Tables,
        "a fold is not a read and moves nothing"
    );
}

#[test]
fn reading_a_table_leaves_the_tree_selection_where_it_was() {
    let mut state = tabled(&[("shop", &["orders"])]);
    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('j'));
    let selected = state.tables.cursor.index();

    press(&mut state, Key::Enter);

    assert_eq!(state.tables.cursor.index(), selected);
}

#[test]
fn a_page_reaches_the_panel_that_asked_for_it() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");

    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(page_of(3)),
            took: Duration::ZERO,
        },
    );

    assert_eq!(state.contents.row_count(), 3);
}

// The domain reads no clock; a duration is a value the adapter measured
// around the real call and handed in (query log design §4).
#[test]
fn a_result_carries_how_long_it_took() {
    let msg = Msg::PageReady {
        query: QueryId::next(),
        page: Box::new(page_of(0)),
        took: Duration::from_millis(41),
    };

    let Msg::PageReady { took, .. } = msg else {
        panic!("constructed a PageReady");
    };

    assert_eq!(took, Duration::from_millis(41));
}

#[test]
fn a_page_for_a_cancelled_query_never_reaches_the_panel() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let first = state.contents.outstanding().expect("the panel is waiting");
    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Enter);

    update(
        &mut state,
        Msg::PageReady {
            query: first,
            page: Box::new(page_of(3)),
            took: Duration::ZERO,
        },
    );

    assert_eq!(state.contents.row_count(), 0);
    assert!(
        state.contents.outstanding().is_some(),
        "the second request is still outstanding"
    );
}

#[test]
fn a_failed_query_shows_in_the_panel_and_not_over_it() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");

    update(
        &mut state,
        Msg::Failed {
            query,
            error: DbError::Timeout,
            took: Duration::ZERO,
        },
    );

    assert!(matches!(
        state.contents.state(),
        crate::contents::ContentsState::Failed { .. }
    ));
    assert!(
        state.overlay.is_none(),
        "one panel failing must not interrupt the whole application"
    );
}
