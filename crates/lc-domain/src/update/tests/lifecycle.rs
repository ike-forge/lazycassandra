use super::*;

/// A config naming one connection and making it the default.
///
/// What `init` needs before it opens anything at all: nothing is reached
/// unless a file named it, so `Config::default()` now starts in the
/// picker rather than on a cluster (connection picker design §2).
fn connecting_config() -> Config {
    let name = ConnectionName::try_new("fixture").expect("a valid name");

    Config {
        connections: vec![ConnectionConfig {
            name: name.clone(),
            hosts: vec!["127.0.0.1:9042".to_owned()],
            posture: Posture::ReadOnly,
            discovery: Discovery::default(),
            datacenter: None,
            credentials: None,
        }],
        default_connection: Some(name),
        ..Config::default()
    }
}

/// Starts up on a config with a default connection, and reports the id
/// `init` minted for it — the id is no longer the caller's to choose.
fn started_on_default(diagnostics: Vec<Diagnostic>, now: Stamp) -> (State, ConnId, Vec<Effect>) {
    let (state, effects) = init(connecting_config(), diagnostics, now);
    let Some(Effect::Connect { conn, .. }) = effects.first() else {
        panic!("a config naming a default connection opens it at startup");
    };

    (state, *conn, effects)
}

#[test]
fn starting_up_logs_the_connection_it_is_opening() {
    let (state, _conn, _effects) = started_on_default(Vec::new(), Stamp::new(14, 2, 9));

    let entry = state.log.entries().next().expect("an entry per effect");
    assert_eq!(entry.at(), Stamp::new(14, 2, 9));
    assert!(
        entry.subject().text().starts_with("Connecting to fixture"),
        "the trail names which connection, now that there is more than one to name"
    );
    assert!(matches!(
        entry.outcome(),
        Outcome::Pending {
            trace: Trace::Connect(_)
        }
    ));
}

#[test]
fn connecting_completes_the_entry_that_was_waiting_on_it() {
    let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());

    let _effects = update(
        &mut state,
        Msg::Connected {
            conn,
            posture: Posture::Writable,
            took: Duration::from_millis(38),
        },
    );

    let entry = state.log.entries().next().expect("the connection entry");
    let Outcome::Succeeded { detail, took } = entry.outcome() else {
        panic!("the connection succeeded");
    };
    assert!(detail.contains("writable"));
    assert_eq!(*took, Duration::from_millis(38));
}

#[test]
fn a_failed_connection_leaves_its_reason_in_the_trail() {
    let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());

    let _effects = update(
        &mut state,
        Msg::ConnectFailed {
            conn,
            error: DbError::Unreachable("no route to host".to_owned()),
            took: Duration::from_millis(12),
        },
    );

    let entry = state.log.entries().next().expect("the connection entry");
    let Outcome::Failed { detail } = entry.outcome() else {
        panic!("the connection failed");
    };
    assert!(detail.contains("no route to host"));
}

#[test]
fn a_failed_startup_connection_raises_a_modal_that_ends_the_process() {
    let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());

    let _effects = update(
        &mut state,
        Msg::ConnectFailed {
            conn,
            error: DbError::Unreachable("no route to host".to_owned()),
            took: Duration::ZERO,
        },
    );

    assert!(matches!(state.overlay, Some(Overlay::Fatal(_))));
}

#[test]
fn the_fatal_modal_quits_rather_than_dismissing() {
    let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());
    let _effects = update(
        &mut state,
        Msg::ConnectFailed {
            conn,
            error: DbError::Timeout,
            took: Duration::ZERO,
        },
    );

    for key in [Key::Escape, Key::Char('q')] {
        let mut state = state.clone();
        let effects = press(&mut state, key);

        assert!(
            effects.contains(&Effect::Quit),
            "{key:?} ends the process rather than revealing an application that never started"
        );
        assert!(
            matches!(state.overlay, Some(Overlay::Fatal(_))),
            "and does not dismiss"
        );
    }
}

// The config diagnostics describe a session that is not going to happen.
#[test]
fn a_fatal_modal_replaces_the_diagnostics_it_lands_on() {
    let (mut state, conn, _effects) =
        started_on_default(vec![diagnostic("boom")], Stamp::default());
    assert!(matches!(state.overlay, Some(Overlay::Diagnostics(_))));

    let _effects = update(
        &mut state,
        Msg::ConnectFailed {
            conn,
            error: DbError::Timeout,
            took: Duration::ZERO,
        },
    );

    assert!(matches!(state.overlay, Some(Overlay::Fatal(_))));
}

#[test]
fn what_the_config_got_wrong_is_in_the_trail_as_well_as_the_modal() {
    let (state, _effects) = init(
        Config::default(),
        vec![diagnostic("boom")],
        Stamp::default(),
    );

    assert!(
        state.log.entries().any(|entry| matches!(
            entry.outcome(),
            Outcome::Failed { .. } | Outcome::Rejected { .. }
        )),
        "a diagnostic reaches the trail"
    );
    assert!(
        matches!(state.overlay, Some(Overlay::Diagnostics(_))),
        "the modal is unchanged"
    );
    assert!(
        matches!(
            state.log.entries().next().unwrap().outcome(),
            Outcome::Rejected { .. }
        ),
        "the diagnostic precedes the connection entry — the order things happened"
    );
}

#[test]
fn connecting_logs_the_schema_read_it_starts() {
    let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());

    let _effects = update(
        &mut state,
        Msg::Connected {
            conn,
            posture: Posture::Writable,
            took: Duration::ZERO,
        },
    );

    let entry = state.log.entries().nth(1).expect("the schema entry");
    assert!(matches!(
        entry.outcome(),
        Outcome::Pending {
            trace: Trace::Schema(_)
        }
    ));
}

#[test]
fn a_loaded_schema_reports_what_it_found() {
    let (mut state, conn, _effects) = started_on_default(Vec::new(), Stamp::default());
    let _effects = update(
        &mut state,
        Msg::Connected {
            conn,
            posture: Posture::Writable,
            took: Duration::ZERO,
        },
    );

    let _effects = update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(schema_of(&["shop"])),
            took: Duration::from_millis(112),
        },
    );

    let entry = state.log.entries().nth(1).expect("the schema entry");
    let Outcome::Succeeded { detail, .. } = entry.outcome() else {
        panic!("the schema loaded");
    };
    assert!(detail.contains("keyspace"), "{detail}");
}

#[test]
fn a_tick_costs_a_repaint_only_while_something_is_outstanding() {
    let mut idle = State::default();
    idle.dirty = false;

    assert!(!idle.is_working());
    update(&mut idle, Msg::Tick(Stamp::default()));
    assert!(
        !idle.dirty,
        "an idle tick changes nothing, and repainting on it would burn the terminal 25 times a second"
    );

    let mut working = connected(Posture::ReadOnly);
    working.dirty = false;

    assert!(working.is_working(), "the schema is still loading");
    update(&mut working, Msg::Tick(Stamp::default()));
    assert!(working.dirty);
}

#[test]
fn a_tick_refreshes_the_clock() {
    let mut state = State::default();

    let effects = update(&mut state, Msg::Tick(Stamp::new(14, 2, 9)));

    assert_eq!(state.now, Stamp::new(14, 2, 9));
    assert!(effects.is_empty());
}

// A tick arrives twenty-five times a second. One that changes nothing must
// not cost a frame (visual polish design §4.2).
#[test]
fn refreshing_the_clock_does_not_cost_a_frame() {
    let mut state = State::default();
    state.dirty = false;

    let _effects = update(&mut state, Msg::Tick(Stamp::new(14, 2, 9)));

    assert!(!state.dirty, "an idle tick repaints nothing");
}

#[test]
fn the_loader_falls_back_to_rest_when_the_work_finishes() {
    let mut state = connected(Posture::ReadOnly);
    for _ in 0..5 {
        update(&mut state, Msg::Tick(Stamp::default()));
    }
    assert_ne!(state.loader.frame(), 0);

    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(schema_of(&["shop"])),
            took: Duration::ZERO,
        },
    );
    state.dirty = false;
    update(&mut state, Msg::Tick(Stamp::default()));

    assert_eq!(
        state.loader.frame(),
        0,
        "an idle frame is always the same frame, not wherever the animation stopped"
    );
    assert!(state.dirty, "the fall back to rest is itself a repaint");
}

#[test]
fn a_query_in_flight_is_work_the_loader_animates_for() {
    let mut state = connected(Posture::ReadOnly);
    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(schema_of(&["shop"])),
            took: Duration::ZERO,
        },
    );
    assert!(
        !state.is_working(),
        "the schema arrived and nothing else is out"
    );

    state
        .in_flight
        .insert(QueryId::next(), statement(StatementKind::Read));

    assert!(state.is_working());
}

#[test]
fn each_wait_belongs_to_the_panel_it_will_fill() {
    let mut state = connected(Posture::ReadOnly);

    assert_eq!(
        working_panels(&state),
        vec![Panel::Tables],
        "a schema still loading is the tree filling"
    );

    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(schema_of(&["shop"])),
            took: Duration::ZERO,
        },
    );
    state
        .in_flight
        .insert(QueryId::next(), statement(StatementKind::Read));

    assert_eq!(
        working_panels(&state),
        vec![Panel::Contents],
        "a statement lands in the grid, whichever panel asked for it"
    );
}

fn working_panels(state: &State) -> Vec<Panel> {
    Panel::ALL
        .into_iter()
        .filter(|&panel| state.is_working_in(panel))
        .collect()
}

#[test]
fn read_only_connection_refuses_a_mutating_statement() {
    let mut state = connected(Posture::ReadOnly);

    let result = submit(&mut state, statement(StatementKind::Write));

    assert_eq!(result, Err(DbError::ReadOnly));
    assert!(
        state.in_flight.is_empty(),
        "a refused statement must not be registered as in flight"
    );
}

#[test]
fn a_connection_with_no_posture_is_read_only() {
    let mut state = State::default();

    assert_eq!(state.posture(), Posture::ReadOnly);
    assert_eq!(
        submit(&mut state, statement(StatementKind::Write)),
        Err(DbError::ReadOnly)
    );
}

#[test]
fn reads_are_permitted_on_a_read_only_connection() {
    let mut state = connected(Posture::ReadOnly);

    let (_, effects) = submit(&mut state, statement(StatementKind::Read)).expect("read is allowed");

    assert_eq!(effects.len(), 2, "expected Execute and RecordHistory");
    assert_eq!(state.in_flight.len(), 1);
}

#[test]
fn writable_connection_executes_and_records_a_mutating_statement() {
    let mut state = connected(Posture::Writable);
    let conn = current_conn(&state);

    let (query, effects) =
        submit(&mut state, statement(StatementKind::Write)).expect("write is allowed");

    assert_eq!(
        state.in_flight.keys().copied().collect::<Vec<_>>(),
        vec![query],
        "the id handed back is the one registered as in flight"
    );
    assert_eq!(
        effects,
        vec![
            Effect::Execute {
                conn,
                query,
                stmt: Box::new(statement(StatementKind::Write)),
            },
            Effect::RecordHistory(Box::new(statement(StatementKind::Write))),
        ]
    );
}

#[test]
fn a_statement_with_no_connection_to_run_it_on_is_refused() {
    let mut state = State::default();

    assert_eq!(
        submit(&mut state, statement(StatementKind::Read)),
        Err(DbError::Unreachable("not connected".to_owned()))
    );
    assert!(state.in_flight.is_empty());
}

#[test]
fn a_result_for_a_cancelled_query_is_dropped() {
    let mut state = connected(Posture::ReadOnly);
    let cancelled = QueryId::next();

    let effects = update(
        &mut state,
        Msg::Failed {
            query: cancelled,
            error: DbError::Timeout,
            took: Duration::ZERO,
        },
    );

    assert!(effects.is_empty());
    assert!(
        !state.dirty,
        "a late result for a query nobody is waiting on must not force a redraw"
    );
}

#[test]
fn quitting_cancels_everything_still_in_flight() {
    let mut state = connected(Posture::Writable);
    let (query, _) = submit(&mut state, statement(StatementKind::Write)).expect("write is allowed");

    let effects = update(&mut state, Msg::Quit);

    assert!(state.should_quit);
    assert_eq!(effects, vec![Effect::Cancel(query), Effect::Quit]);
}

#[test]
fn a_resize_marks_the_state_dirty_and_emits_no_effects() {
    let mut state = State::default();

    let effects = update(&mut state, Msg::Resized);

    assert!(
        effects.is_empty(),
        "a resize is not an effect-producing event"
    );
    assert!(state.dirty, "a resize must force the next frame to repaint");
}

#[test]
fn schema_for_a_stale_connection_is_ignored() {
    let mut state = connected(Posture::ReadOnly);

    let effects = update(
        &mut state,
        Msg::SchemaLoaded {
            conn: ConnId::next(),
            schema: Box::new(Schema::default()),
            took: Duration::ZERO,
        },
    );

    assert!(effects.is_empty());
    let ConnectionState::Connected(connection) = state.connection else {
        panic!("the connection is still present");
    };
    assert!(
        matches!(connection.schema, SchemaState::Loading),
        "schema from a connection we are no longer on must not be applied"
    );
}

/// Design §2: a config that names a default connection opens it, and one
/// that does not opens nothing — the tool reaches no cluster a file did
/// not name.
#[test]
fn a_config_naming_a_default_connection_opens_it_at_startup() {
    let (state, conn, effects) = started_on_default(Vec::new(), Stamp::default());

    assert!(
        matches!(effects.as_slice(), [Effect::Connect { conn: id, target }]
            if *id == conn && target.name.as_str() == "fixture"),
        "the effect carries the connection it opens, not only an id"
    );
    assert!(state.connection.is_current(conn));
    assert!(state.dirty, "the first frame must be painted");
}

#[test]
fn diagnostics_open_over_the_panels_and_esc_dismisses_them() {
    let diagnostic = Diagnostic {
        origin: Origin::new("./.lazycassandra.toml"),
        span: Some(ConfigSpan {
            line: 14,
            column: 11,
        }),
        severity: Severity::Danger,
        message: "connection `dev`: `posture` unknown value \"read\"".to_owned(),
    };
    let (mut state, _) = init(Config::default(), vec![diagnostic], Stamp::default());

    assert!(matches!(state.overlay, Some(Overlay::Diagnostics(_))));

    let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Escape)));

    assert!(
        state.overlay.is_none(),
        "esc dismisses this overlay like every other"
    );
}

#[test]
fn a_single_diagnostic_leaves_the_cursor_nowhere_to_go() {
    let (mut state, _) = init(Config::default(), vec![diagnostic("one")], Stamp::default());

    let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Char('j'))));
    let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Char('j'))));

    assert_eq!(diagnostics_cursor(&state), 0);
}

#[test]
fn motion_walks_the_diagnostics_and_stops_on_the_last_row() {
    let diagnostics = vec![diagnostic("one"), diagnostic("two"), diagnostic("three")];
    let (mut state, _) = init(Config::default(), diagnostics, Stamp::default());

    for _ in 0..5 {
        let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Char('j'))));
    }
    assert_eq!(
        diagnostics_cursor(&state),
        2,
        "the cursor stops on the last row rather than running off the list"
    );

    let _ = update(&mut state, Msg::Input(KeyPress::new(Key::Char('k'))));
    assert_eq!(diagnostics_cursor(&state), 1);
}

#[test]
fn the_config_the_tool_started_on_is_kept_rather_than_consumed() {
    let config = Config {
        icons: Icons::Off,
        ..Config::default()
    };
    let (state, _) = init(config.clone(), Vec::new(), Stamp::default());

    assert_eq!(
        state.config, config,
        "the follow-ups in design §11 read this"
    );
}

#[test]
fn the_theme_a_config_names_is_the_theme_the_tool_opens_on() {
    let config = Config {
        theme: ThemeChoice::Ours(Ours::QuillLight),
        ..Config::default()
    };
    let (state, _) = init(config, Vec::new(), Stamp::default());

    assert_eq!(state.theme, ThemeChoice::Ours(Ours::QuillLight));
}

#[test]
fn connecting_successfully_asks_for_the_schema() {
    let (mut state, conn, _) = started_on_default(Vec::new(), Stamp::default());

    let effects = update(
        &mut state,
        Msg::Connected {
            conn,
            posture: Posture::ReadOnly,
            took: Duration::ZERO,
        },
    );

    assert_eq!(effects, vec![Effect::LoadSchema(conn)]);
    assert!(state.schema().is_none(), "the schema is still loading");
}

#[test]
fn a_connected_message_for_a_stale_attempt_is_ignored() {
    let (mut state, conn, _) = started_on_default(Vec::new(), Stamp::default());

    let effects = update(
        &mut state,
        Msg::Connected {
            conn: ConnId::next(),
            posture: Posture::ReadOnly,
            took: Duration::ZERO,
        },
    );

    assert!(effects.is_empty());
    assert!(
        state.connection.is_current(conn),
        "a Connected message for a connection we are no longer waiting on must not be applied"
    );
}

#[test]
fn a_failed_connection_is_recorded_and_raised() {
    let (mut state, conn, _) = started_on_default(Vec::new(), Stamp::default());

    let effects = update(
        &mut state,
        Msg::ConnectFailed {
            conn,
            error: DbError::Unreachable("no route".to_owned()),
            took: Duration::ZERO,
        },
    );

    assert!(effects.is_empty());
    assert!(matches!(state.connection, ConnectionState::Failed(_)));
    let Some(Overlay::Fatal(overlay)) = state.overlay else {
        panic!("a failure is shown");
    };
    assert_eq!(overlay.severity, Severity::Danger);
    assert!(overlay.message.contains("no route"));
}

#[test]
fn a_failed_connection_for_a_stale_attempt_is_dropped() {
    let (mut state, conn, _) = started_on_default(Vec::new(), Stamp::default());

    update(
        &mut state,
        Msg::ConnectFailed {
            conn: ConnId::next(),
            error: DbError::Timeout,
            took: Duration::ZERO,
        },
    );

    assert!(state.overlay.is_none());
    assert!(state.connection.is_current(conn));
}

#[test]
fn a_failed_schema_is_recorded_and_raised() {
    let mut state = connected(Posture::ReadOnly);
    let conn = current_conn(&state);

    update(
        &mut state,
        Msg::SchemaFailed {
            conn,
            error: DbError::Invalid("unauthorized".to_owned()),
            took: Duration::ZERO,
        },
    );

    assert!(state.overlay.is_some());
    let ConnectionState::Connected(connection) = &state.connection else {
        panic!("still connected");
    };
    assert!(matches!(connection.schema, SchemaState::Failed(_)));
}

#[test]
fn a_failed_schema_for_a_stale_connection_is_dropped() {
    let mut state = connected(Posture::ReadOnly);

    update(
        &mut state,
        Msg::SchemaFailed {
            conn: ConnId::next(),
            error: DbError::Invalid("unauthorized".to_owned()),
            took: Duration::ZERO,
        },
    );

    assert!(state.overlay.is_none(), "a stale failure must not be shown");
    let ConnectionState::Connected(connection) = &state.connection else {
        panic!("still connected");
    };
    assert!(
        matches!(connection.schema, SchemaState::Loading),
        "schema failure from a connection we are no longer on must not be applied"
    );
}

#[test]
fn a_message_arriving_over_an_open_prompt_restores_the_previous_search() {
    let mut state = connected(Posture::ReadOnly);
    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(tabled_schema(&[("shop", &["orders"])])),
            took: Duration::ZERO,
        },
    );
    state.apply_search(Searched::Tables, Search::new("shop"));

    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('o'));
    press(&mut state, Key::Char('r'));
    assert_eq!(
        state.search(Searched::Tables).map(Search::as_str),
        Some("or"),
        "the live prompt is previewing its own buffer, not the previous search"
    );

    // Any message-driven overlay unwinds a live search the same way
    // (`unwind_overlay`'s doc comment) — `EditorFailed` stands in
    // because `SchemaFailed` now raises `Overlay::Fatal`, which has
    // nothing to preserve (there is no application left to return to).
    update(
        &mut state,
        Msg::EditorFailed {
            reason: "could not run hx: No such file or directory".to_owned(),
        },
    );

    assert!(
        matches!(state.overlay, Some(Overlay::Message(_))),
        "the failure must still be shown"
    );
    assert_eq!(
        state.search(Searched::Tables).map(Search::as_str),
        Some("shop"),
        "the half-typed query must not strand itself; the prompt's previous search is restored"
    );
}
