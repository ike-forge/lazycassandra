use super::*;

/// `ctrl-r`, the Queries member of the ctrl layer (docs/shortcuts.md §2).
fn run_query(state: &mut State) -> Vec<Effect> {
    ctrl(state, Key::Char('r'))
}

#[test]
fn ctrl_r_runs_the_query_buffer() {
    let mut state = editing("SELECT * FROM shop.orders");

    let effects = run_query(&mut state);

    let query = state
        .contents
        .outstanding()
        .expect("the panel is waiting on the query it just ran");
    assert_eq!(
        effects,
        vec![
            Effect::ExecutePaged {
                conn: current_conn(&state),
                query,
                stmt: Box::new(Statement::typed("SELECT * FROM shop.orders")),
                page_size: crate::config::PageSize::try_new(200).expect("in range"),
                cursor: None,
            },
            Effect::RecordHistory(Box::new(Statement::typed("SELECT * FROM shop.orders"))),
        ]
    );
}

// Rejected in update, so no effect was ever issued — the entry is born
// complete (query log design §3, §3.1).
#[test]
fn a_statement_refused_by_the_posture_says_so_in_the_trail() {
    let mut state = editing("DELETE FROM store.orders WHERE id = 7");

    let effects = run_query(&mut state);

    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::Execute { .. } | Effect::ExecutePaged { .. })),
        "a read-only connection issues nothing"
    );
    let Outcome::Rejected { reason } = state.log.entries().last().unwrap().outcome() else {
        panic!("the statement was rejected");
    };
    assert!(reason.contains("read-only"), "{reason}");
}

// The posture guard logs exactly once. Restructuring the logging into
// `submit_into_contents`'s `Err` arm must not leave `submit` still
// logging its own posture rejection too (I3).
#[test]
fn a_statement_refused_by_the_posture_logs_exactly_one_entry() {
    let mut state = editing("DELETE FROM store.orders WHERE id = 7");
    let before = state.log.len();

    run_query(&mut state);

    assert_eq!(
        state.log.len(),
        before + 1,
        "one refusal, one entry — not one from `submit` and a second from its caller"
    );
}

// I3: before `Msg::Connected` lands, a read-only statement is not caught
// by the mutation guard, so it fell through to `conn_id()` returning
// `None` — a refusal `submit_into_contents` noticed (`refused` shows it
// in Table contents) but never logged, so nothing outlived the panel.
#[test]
fn a_statement_refused_for_want_of_a_connection_says_so_in_the_trail() {
    let mut state = State::default();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();
    type_into(&mut state, "SELECT * FROM shop.orders");
    let before = state.log.len();

    let effects = run_query(&mut state);

    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::ExecutePaged { .. })),
        "there is no connection to run it on"
    );
    assert_eq!(state.log.len(), before + 1, "the refusal is not silent");
    let Outcome::Rejected { reason } = state.log.entries().last().unwrap().outcome() else {
        panic!("the statement was refused for want of a connection");
    };
    assert!(reason.contains("not connected"), "{reason}");
}

// M8: the one remaining refusal that left no trace — `run`'s
// destructive-statement guard raises a modal `esc` destroys on dismissal
// (query log design §1), and used to log nothing behind it.
#[test]
fn a_destructive_statement_refused_by_run_says_so_in_the_trail() {
    let mut state = editing("DROP TABLE shop.orders");
    let ConnectionState::Connected(connection) = &mut state.connection else {
        panic!("the fixture is connected");
    };
    connection.posture = Posture::Writable;

    run_query(&mut state);

    let Outcome::Rejected { reason } = state.log.entries().last().unwrap().outcome() else {
        panic!("the destructive statement was refused");
    };
    assert!(reason.contains("typed confirmation"), "{reason}");
}

#[test]
fn running_the_buffer_lands_focus_on_the_grid_it_filled() {
    let mut state = editing("SELECT * FROM shop.orders");

    run_query(&mut state);

    assert_eq!(
        state.focus(),
        Panel::Contents,
        "a run is a read, and a read lands where the rows do"
    );
}

#[test]
fn a_run_query_titles_the_panel_with_what_was_typed() {
    let mut state = editing("SELECT * FROM shop.orders");
    run_query(&mut state);
    let query = state.contents.outstanding().expect("waiting");

    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(page_of(3)),
            took: Duration::ZERO,
        },
    );

    assert_eq!(state.contents.row_count(), 3);
    let ContentsState::Loaded { subject, .. } = state.contents.state() else {
        panic!("the page landed, so the panel is loaded");
    };
    assert_eq!(
        subject.label(),
        "SELECT * FROM shop.orders",
        "the panel says which query it is showing"
    );
}

/// `editing` leaves the buffer open; the band is browsed with it closed.
fn browsing_cql(text: &str) -> State {
    let mut state = editing(text);
    press(&mut state, Key::Escape);
    state
}

fn banded(state: &State) -> Option<String> {
    state.queries.statement()
}

#[test]
fn j_and_k_walk_the_bands_over_the_buffers_statements() {
    let mut state = browsing_cql("SELECT 1;\nSELECT 2;\nSELECT 3");
    press(&mut state, Key::Char('g'));
    assert_eq!(banded(&state).as_deref(), Some("SELECT 1;"));

    press(&mut state, Key::Char('j'));
    assert_eq!(banded(&state).as_deref(), Some("SELECT 2;"));

    press(&mut state, Key::Char('k'));
    assert_eq!(banded(&state).as_deref(), Some("SELECT 1;"));

    press(&mut state, Key::Char('G'));
    assert_eq!(banded(&state).as_deref(), Some("SELECT 3"));
}

#[test]
fn j_is_a_character_again_once_the_buffer_is_open() {
    let mut state = browsing_cql("SELECT 1;\nSELECT 2");
    press(&mut state, Key::Char('g'));

    press(&mut state, Key::Enter);
    press(&mut state, Key::Char('j'));

    assert_eq!(
        state.queries.text(),
        "jSELECT 1;\nSELECT 2",
        "an open buffer claims the key before the band is stepped, and the \
         caret is where the band started"
    );
}

#[test]
fn ctrl_r_runs_the_banded_statement_and_not_the_rest_of_the_buffer() {
    let mut state = browsing_cql("SELECT * FROM shop.orders;\nSELECT * FROM shop.users");
    press(&mut state, Key::Char('g'));

    run_query(&mut state);

    let ContentsState::Loading { subject, .. } = state.contents.state() else {
        panic!("the run is outstanding");
    };
    assert_eq!(subject.label(), "SELECT * FROM shop.orders;");
}

#[test]
fn stepping_the_band_changes_what_ctrl_r_runs() {
    let mut state = browsing_cql("SELECT * FROM shop.orders;\nSELECT * FROM shop.users");
    press(&mut state, Key::Char('g'));
    press(&mut state, Key::Char('j'));

    run_query(&mut state);

    let ContentsState::Loading { subject, .. } = state.contents.state() else {
        panic!("the run is outstanding");
    };
    assert_eq!(subject.label(), "SELECT * FROM shop.users");
}

#[test]
fn ctrl_r_outside_queries_runs_nothing() {
    for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
        let mut state = editing("SELECT * FROM shop.orders");
        state.focus_on(panel);

        assert!(
            run_query(&mut state).is_empty(),
            "the buffer is run from the panel it is typed in, not from {panel:?}"
        );
        assert_eq!(state.contents.outstanding(), None);
    }
}

#[test]
fn an_empty_buffer_runs_nothing() {
    for text in ["", "   \n  \n"] {
        let mut state = editing(text);

        assert!(run_query(&mut state).is_empty());
        assert_eq!(
            state.contents.outstanding(),
            None,
            "an empty buffer is a mistake with no consequence, not an error box"
        );
        assert!(state.overlay.is_none());
    }
}

#[test]
fn running_a_query_cancels_a_pending_table_read() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let read = state.contents.outstanding().expect("the read is in flight");
    state.focus_on(Panel::Queries);
    state.queries.start_editing();
    type_into(&mut state, "SELECT * FROM shop.users");

    let effects = run_query(&mut state);

    assert!(
        effects.contains(&Effect::Cancel(read)),
        "one grid means one outstanding query"
    );
    assert!(!state.in_flight.contains_key(&read));
}

#[test]
fn reading_a_table_cancels_a_pending_query() {
    let mut state = read_shop_orders();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();
    type_into(&mut state, "SELECT * FROM shop.users");
    run_query(&mut state);
    let query = state
        .contents
        .outstanding()
        .expect("the query is in flight");
    state.focus_on(Panel::Tables);

    let effects = press(&mut state, Key::Enter);

    assert!(effects.contains(&Effect::Cancel(query)));
    assert!(!state.in_flight.contains_key(&query));
}

#[test]
fn a_destructive_statement_is_refused_until_confirmation_exists() {
    let mut state = editing("DROP TABLE shop.orders");

    let effects = run_query(&mut state);

    assert!(
        effects.is_empty(),
        "nothing reaches the cluster without the confirmation vision.md §4 principle 6 requires"
    );
    assert_eq!(state.contents.outstanding(), None);
    assert!(
        matches!(
            state.overlay,
            Some(Overlay::Message(Message {
                severity: Severity::Danger,
                ..
            }))
        ),
        "the refusal says why"
    );
}

#[test]
fn a_typed_write_is_refused_on_a_read_only_connection() {
    let mut state = editing("INSERT INTO shop.orders (id) VALUES (1)");

    let effects = run_query(&mut state);

    assert!(effects.is_empty());
    assert!(
        matches!(state.contents.state(), ContentsState::Failed { .. }),
        "the posture gate refuses it and the panel says so"
    );
}

/// Every mutating verb, run from the buffer against a read-only
/// connection: nothing reaches the cluster and each one says why. The
/// list is the classifier's, so a verb added there without a posture
/// answer fails here (posture design §2).
#[test]
fn every_mutating_statement_typed_by_hand_is_refused_and_says_why() {
    for cql in [
        "INSERT INTO shop.orders (id) VALUES (1)",
        "UPDATE shop.orders SET total = 1 WHERE id = 1",
        "DELETE FROM shop.orders WHERE id = 1",
        "BEGIN BATCH INSERT INTO shop.orders (id) VALUES (1); APPLY BATCH",
        "CREATE TABLE shop.new (id int PRIMARY KEY)",
        "CREATE KEYSPACE other WITH replication = {}",
        "ALTER TABLE shop.orders ADD note text",
        "GRANT SELECT ON KEYSPACE shop TO reader",
        "REVOKE MODIFY ON KEYSPACE shop FROM reader",
        "DROP TABLE shop.orders",
        "TRUNCATE shop.orders",
        "-- innocent\nDROP KEYSPACE shop",
        "SELECT * FROM shop.orders; DROP TABLE shop.orders",
    ] {
        let mut state = editing(cql);

        let effects = run_query(&mut state);

        assert!(effects.is_empty(), "{cql} reached the cluster");
        assert!(
            matches!(&state.overlay, Some(Overlay::Message(message))
                if message.title == "Read-only connection"),
            "{cql} was refused without saying why"
        );
    }
}

/// A read is not refused, and a read-only connection is what the tool is
/// for — the modal must not be in the way of one.
#[test]
fn a_read_typed_by_hand_raises_no_refusal() {
    for cql in [
        "SELECT * FROM shop.orders",
        "-- a note\nSELECT * FROM shop.orders",
        "SELECT * FROM shop.orders WHERE note = '; DROP TABLE x'",
    ] {
        let mut state = editing(cql);

        let effects = run_query(&mut state);

        assert!(!effects.is_empty(), "{cql} should have run");
        assert!(state.overlay.is_none(), "{cql} was refused");
    }
}

/// The posture names itself rather than deferring to the confirmation
/// that is not what stands in the way here: a read-only connection has no
/// `d` to send the user to.
#[test]
fn a_destructive_statement_on_a_read_only_connection_blames_the_posture() {
    let mut state = editing("DROP TABLE shop.orders");

    run_query(&mut state);

    let Some(Overlay::Message(message)) = &state.overlay else {
        panic!("refused with a modal");
    };
    assert_eq!(message.title, "Read-only connection");
    assert!(
        !message.message.contains("typed confirmation"),
        "the confirmation is not what refused it: {}",
        message.message
    );
}

/// The refusal survives the modal, which `esc` destroys (query log design
/// §1).
#[test]
fn a_statement_the_posture_refuses_says_so_in_the_trail_too() {
    let mut state = editing("DROP TABLE shop.orders");

    run_query(&mut state);

    let Outcome::Rejected { reason } = state.log.entries().last().unwrap().outcome() else {
        panic!("the statement was refused");
    };
    assert!(reason.contains("read-only"), "{reason}");
}

#[test]
fn an_unbound_ctrl_key_does_nothing() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('l'));

    ctrl(&mut state, Key::Char('j'));

    assert_eq!(
        state.tables.cursor.index(),
        0,
        "ctrl-j is a binding of its own that does not exist yet, not a slower j"
    );
}

#[test]
fn an_alt_modified_key_is_not_a_binding() {
    let mut state = browsing_tables(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('l'));

    press_binding(
        &mut state,
        KeyPress {
            key: Key::Char('j'),
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        },
    );

    assert_eq!(
        state.tables.cursor.index(),
        0,
        "alt-j is refused rather than reinterpreted as a slower j"
    );
}

#[test]
fn ctrl_alt_r_does_not_run_the_buffer() {
    let mut state = editing("SELECT * FROM shop.orders");

    let effects = press_binding(
        &mut state,
        KeyPress {
            key: Key::Char('r'),
            modifiers: Modifiers {
                ctrl: true,
                alt: true,
                shift: false,
            },
        },
    );

    assert!(
        effects.is_empty(),
        "two modifiers are a third layer, and no layer is designed"
    );
    assert_eq!(state.contents.outstanding(), None);
}

#[test]
fn an_open_palette_swallows_ctrl_r() {
    let mut state = editing("SELECT * FROM shop.orders");
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
    state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));

    let effects = run_query(&mut state);

    assert!(
        effects.is_empty(),
        "an open palette is what the user is looking at, and it claims every key"
    );
    assert_eq!(state.contents.outstanding(), None);
}
