use super::*;

fn form_of(state: &State) -> &crate::export::ExportForm {
    match state.overlay.as_ref() {
        Some(Overlay::Export(form)) => form,
        _ => panic!("the export form is up"),
    }
}

fn started(state: &mut State) -> Vec<Effect> {
    open_form(state);
    while form_of(state).stop() != crate::export::Stop::Export {
        handle_key(state, KeyPress::new(Key::Tab));
    }
    handle_key(state, KeyPress::new(Key::Char(' ')))
}

fn contents_showing_typed(cql: &str) -> State {
    let mut state = connected(Posture::ReadOnly);
    state.geometry = Geometry::default().with_export_fits(true);
    state.focus_on(Panel::Contents);
    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Typed(cql.to_owned()),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        more_of(PAGE),
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );
    state
}

#[test]
fn every_live_hint_the_export_forms_bar_shows_does_something() {
    for tabs in 0..4 {
        let seated = || {
            let mut state = contents_showing_more(PAGE);
            open_form(&mut state);
            for _ in 0..tabs {
                handle_key(&mut state, KeyPress::new(Key::Tab));
            }
            state
        };
        let bar = crate::view::hints::bar(&seated());
        let stop = form_of(&seated()).stop();

        for hint in bar.panel.iter().chain(&bar.global) {
            assert_eq!(hint.liveness, crate::view::Liveness::Live);

            for key in hint.keys.split('/') {
                let key = match key {
                    "tab" => Key::Tab,
                    "space" => Key::Char(' '),
                    "⏎" => Key::Enter,
                    "esc" => Key::Escape,
                    other => panic!("{stop:?} names {other}, which this sweep cannot press"),
                };
                let mut state = seated();
                let before = crate::view::view_model(&state);

                let effects = press(&mut state, key);

                assert!(
                    crate::view::view_model(&state) != before || !effects.is_empty(),
                    "{stop:?} offers '{key:?} {}' and pressing it changes nothing",
                    hint.action
                );
            }
        }
    }
}

#[test]
fn the_export_forms_bar_follows_the_stop() {
    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    let named = |state: &State| {
        let bar = crate::view::hints::bar(state);
        bar.panel
            .iter()
            .chain(&bar.global)
            .map(|hint| format!("{} {}", hint.keys, hint.action))
            .collect::<Vec<_>>()
    };

    assert_eq!(
        named(&state),
        ["tab next option", "esc close"],
        "space is text on the path"
    );
    press(&mut state, Key::Tab);
    assert_eq!(
        named(&state),
        ["tab next option", "space toggle", "esc close"]
    );
    press(&mut state, Key::Tab);
    assert_eq!(
        named(&state),
        ["tab next option", "space toggle", "esc close"]
    );
    press(&mut state, Key::Tab);
    assert_eq!(
        named(&state),
        ["tab next option", "space/⏎ export", "esc close"]
    );
}

#[test]
fn ctrl_e_over_a_loaded_contents_panel_opens_the_export_form() {
    let mut state = contents_showing(5);

    open_form(&mut state);

    assert_eq!(
        crate::style::text(&form_of(&state).title()),
        "Export table shop.orders"
    );
}

#[test]
fn ctrl_e_does_nothing_over_an_empty_contents_panel() {
    let mut state = contents_showing(0);

    open_form(&mut state);

    assert!(state.overlay.is_none());
    assert_eq!(toasted(&state), Some((Severity::Info, "nothing to export")));
}

#[test]
fn ctrl_e_is_inert_where_the_frame_cannot_draw_the_form() {
    let mut state = contents_showing(5);
    state.geometry = Geometry::default().with_export_fits(false);

    open_form(&mut state);

    assert!(state.overlay.is_none());
    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "no room to draw the export form"))
    );
}

#[test]
fn a_frame_that_shrank_past_the_form_closes_it() {
    let mut state = contents_showing(5);
    open_form(&mut state);
    assert!(matches!(state.overlay, Some(Overlay::Export(_))));

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 14).with_export_fits(false)),
    );

    assert!(
        state.overlay.is_none(),
        "the palette still fits, and the form does not"
    );
}

#[test]
fn ctrl_e_in_queries_still_opens_the_external_editor() {
    let mut state = connected(Posture::ReadOnly);
    state.focus_on(Panel::Queries);

    let effects = handle_key(&mut state, KeyPress::ctrl(Key::Char('e')));

    assert!(matches!(effects.as_slice(), [Effect::OpenEditor { .. }]));
    assert!(state.overlay.is_none());
}

#[test]
fn the_form_records_how_much_of_the_result_is_in_memory() {
    let mut complete = contents_showing(7);
    open_form(&mut complete);
    assert_eq!(
        form_of(&complete).fetched(),
        crate::export::Fetched::Everything(7)
    );

    let mut capped = contents_showing_more(PAGE);
    open_form(&mut capped);
    assert_eq!(
        form_of(&capped).fetched(),
        crate::export::Fetched::Some(PAGE)
    );
}

#[test]
fn tab_moves_between_the_forms_stops_rather_than_between_panels() {
    let mut state = contents_showing(5);
    open_form(&mut state);

    handle_key(&mut state, KeyPress::new(Key::Tab));

    assert_eq!(form_of(&state).stop(), crate::export::Stop::Format);
    assert_eq!(state.focus(), Panel::Contents, "focus did not move");
}

#[test]
fn space_toggles_the_focused_radio() {
    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    handle_key(&mut state, KeyPress::new(Key::Tab));
    handle_key(&mut state, KeyPress::new(Key::Tab));

    handle_key(&mut state, KeyPress::new(Key::Char(' ')));

    assert_eq!(form_of(&state).scope(), crate::export::Scope::First);
}

#[test]
fn a_printable_key_types_itself_only_on_the_path_stop() {
    let mut state = contents_showing(5);
    open_form(&mut state);
    handle_key(&mut state, KeyPress::new(Key::Tab));
    handle_key(&mut state, KeyPress::new(Key::Char('x')));
    assert_eq!(
        form_of(&state).scope(),
        crate::export::Scope::Whole,
        "an unclaimed key on a radio does nothing"
    );
    assert!(!form_of(&state).path().text().ends_with('x'));

    handle_key(&mut state, KeyPress::new(Key::BackTab));
    handle_key(&mut state, KeyPress::new(Key::Char('x')));

    assert!(form_of(&state).path().text().ends_with('x'));
}

#[test]
fn space_on_the_path_stop_types_a_space() {
    let mut state = contents_showing(5);
    open_form(&mut state);

    handle_key(&mut state, KeyPress::new(Key::Char(' ')));

    assert!(form_of(&state).path().text().ends_with(' '));
}

#[test]
fn enter_on_a_radio_group_does_not_run_the_export() {
    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);

    let effects = handle_key(&mut state, KeyPress::new(Key::Enter));

    assert!(effects.is_empty());
    assert!(
        matches!(state.overlay, Some(Overlay::Export(_))),
        "the form is still up"
    );
}

#[test]
fn enter_on_the_export_stop_runs_it() {
    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    for _ in 0..3 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }

    let effects = handle_key(&mut state, KeyPress::new(Key::Enter));

    assert!(matches!(effects.as_slice(), [Effect::Export { .. }]));
}

#[test]
fn q_does_not_quit_while_the_form_is_up() {
    let mut state = contents_showing(5);
    open_form(&mut state);

    handle_key(&mut state, KeyPress::new(Key::Char('q')));

    assert!(
        !state.should_quit,
        "the overlay claims every unmodified key"
    );
}

#[test]
fn esc_closes_the_form_and_writes_nothing() {
    let mut state = contents_showing(5);
    open_form(&mut state);

    let effects = handle_key(&mut state, KeyPress::new(Key::Escape));

    assert!(state.overlay.is_none());
    assert!(effects.is_empty());
}

#[test]
fn the_theme_picker_cannot_replace_an_open_export_form() {
    let mut state = contents_showing(5);
    // Room for the picker, so the open form is what refuses the chord.
    state.geometry = state.geometry.with_theme_rows(20);
    open_form(&mut state);

    handle_key(&mut state, KeyPress::ctrl(Key::Char('t')));

    assert!(
        matches!(state.overlay, Some(Overlay::Export(_))),
        "a chord must not discard the form and the path typed into it"
    );
}

#[test]
fn a_second_ctrl_e_leaves_the_open_form_as_it_was() {
    let mut state = contents_showing(5);
    open_form(&mut state);
    handle_key(&mut state, KeyPress::new(Key::Char('x')));
    let typed = form_of(&state).path().text().to_owned();

    handle_key(&mut state, KeyPress::ctrl(Key::Char('e')));

    assert_eq!(form_of(&state).path().text(), typed);
    assert_eq!(form_of(&state).stop(), crate::export::Stop::Path);
}

fn export_in_flight(cql: &str, destination: &str) -> State {
    let mut state = contents_showing_typed(cql);
    open_form(&mut state);
    if let Some(Overlay::Export(form)) = &mut state.overlay {
        *form.path_mut() = crate::export::Line::new(destination.to_owned());
    }
    while form_of(&state).stop() != crate::export::Stop::Export {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }
    handle_key(&mut state, KeyPress::new(Key::Char(' ')));
    state
}

fn export_fetched_in_flight(destination: &str) -> State {
    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    if let Some(Overlay::Export(form)) = &mut state.overlay {
        *form.path_mut() = crate::export::Line::new(destination.to_owned());
    }
    for _ in 0..2 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }
    handle_key(&mut state, KeyPress::new(Key::Char('k'))); // Scope: Whole -> Fetched.
    // Export.
    for _ in 0..1 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }
    handle_key(&mut state, KeyPress::new(Key::Char(' ')));
    state
}

#[test]
fn exporting_the_whole_table_asks_for_it_without_a_limit() {
    let mut state = contents_showing_more(PAGE);

    let effects = started(&mut state);

    let [
        Effect::Export {
            source,
            format,
            destination,
            overwrite,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("one export effect");
    };
    assert_eq!(
        *source,
        Source::Whole {
            statement: Box::new(Statement::typed("SELECT * FROM \"shop\".\"orders\"")),
            limit: None,
        }
    );
    assert_eq!(*format, crate::export::Format::Csv);
    assert!(destination.ends_with("shop.orders.csv"));
    assert!(!overwrite, "the first attempt never clobbers");
}

#[test]
fn exporting_a_typed_query_re_runs_it_verbatim() {
    let mut state = contents_showing_typed("SELECT * FROM shop.orders LIMIT 10");

    let effects = started(&mut state);

    let [Effect::Export { source, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    assert_eq!(
        *source,
        Source::Whole {
            statement: Box::new(Statement::typed("SELECT * FROM shop.orders LIMIT 10")),
            limit: None,
        }
    );
}

#[test]
fn the_first_scope_carries_the_typed_count_and_takes_only_digits() {
    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    for _ in 0..2 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }
    handle_key(&mut state, KeyPress::new(Key::Char(' '))); // Scope: Whole -> First.
    handle_key(&mut state, KeyPress::new(Key::Tab));
    for _ in 0..4 {
        handle_key(&mut state, KeyPress::new(Key::Backspace));
    }
    for key in ['x', '7', ' ', '5'] {
        handle_key(&mut state, KeyPress::new(Key::Char(key)));
    }
    handle_key(&mut state, KeyPress::new(Key::Tab));

    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

    let [Effect::Export { source, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    let Source::Whole { limit, .. } = source else {
        panic!("the first scope re-runs the statement");
    };
    assert_eq!(limit.map(NonZeroUsize::get), Some(75));
}

#[test]
fn a_first_scope_with_no_count_does_not_start() {
    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    for _ in 0..2 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }
    handle_key(&mut state, KeyPress::new(Key::Char(' ')));
    handle_key(&mut state, KeyPress::new(Key::Tab));
    for _ in 0..4 {
        handle_key(&mut state, KeyPress::new(Key::Backspace));
    }
    handle_key(&mut state, KeyPress::new(Key::Tab));

    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

    assert!(effects.is_empty());
    assert!(matches!(state.overlay, Some(Overlay::Export(_))));
}

#[test]
fn exporting_the_fetched_rows_asks_the_cluster_nothing() {
    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    for _ in 0..2 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }
    handle_key(&mut state, KeyPress::new(Key::Char('k')));
    for _ in 0..1 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }

    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

    let [
        Effect::Export {
            source: Source::Fetched { columns, rows },
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("one export effect carrying the rows");
    };
    assert_eq!(rows.len(), PAGE);
    assert_eq!(columns, &["id".to_owned()]);
}

#[test]
fn starting_an_export_closes_the_form_and_records_the_job() {
    let mut state = contents_showing_more(PAGE);

    let effects = started(&mut state);

    assert!(state.overlay.is_none(), "the form is done with");
    let [Effect::Export { job, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    assert_eq!(state.export.as_ref().map(ExportJob::id), Some(*job));
    assert_eq!(state.export.as_ref().map(ExportJob::rows), Some(0));
    assert!(
        matches!(toasted(&state), Some((Severity::Info, message)) if message.starts_with("exporting to ")),
        "the start is announced"
    );
}

#[test]
fn progress_is_the_running_total() {
    let mut state = contents_showing_more(PAGE);
    let effects = started(&mut state);
    let [Effect::Export { job, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };

    update(
        &mut state,
        Msg::ExportProgress {
            job: *job,
            rows: 5_000,
        },
    );

    assert_eq!(state.export.as_ref().map(ExportJob::rows), Some(5_000));
}

#[test]
fn progress_for_a_job_that_is_not_running_is_dropped() {
    let mut state = contents_showing_more(PAGE);
    started(&mut state);

    update(
        &mut state,
        Msg::ExportProgress {
            job: JobId::next(),
            rows: 9,
        },
    );

    assert_eq!(state.export.as_ref().map(ExportJob::rows), Some(0));
}

#[test]
fn finishing_clears_the_job_and_says_what_was_written() {
    let mut state = contents_showing_more(PAGE);
    let effects = started(&mut state);
    let [Effect::Export { job, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };

    update(
        &mut state,
        Msg::ExportFinished {
            job: *job,
            result: Ok(41_203),
            took: Duration::ZERO,
        },
    );

    assert!(state.export.is_none());
    let Some((severity, message)) = toasted(&state) else {
        panic!("the result is on screen");
    };
    assert_eq!(severity, Severity::Info);
    assert!(message.contains("41203"));
    assert!(message.contains("shop.orders.csv"));
}

#[test]
fn a_finished_export_records_the_exact_statement_and_where_it_went() {
    let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
    let job = state.export.as_ref().expect("a job").id();

    let _effects = update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Ok(18_420),
            took: Duration::from_millis(1_400),
        },
    );

    let entry = state
        .log
        .entries()
        .find(|entry| entry.subject().text().starts_with("SELECT"))
        .expect("the export entry");
    assert!(
        !entry.subject().text().contains("LIMIT"),
        "an export pages the unbounded statement (export design §6.2)"
    );
    let Outcome::Succeeded { detail, .. } = entry.outcome() else {
        panic!("the export succeeded");
    };
    assert!(detail.contains("18420"), "{detail}");
    assert!(detail.contains("/tmp/orders.csv"), "{detail}");
}

#[test]
fn an_export_that_exists_stays_pending_through_the_retry_and_completes_with_it() {
    let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
    let job = state.export.as_ref().expect("a job").id();

    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    let waiting = state
        .log
        .entries()
        .find(|entry| entry.subject().text().starts_with("SELECT"))
        .expect("the export entry");
    assert!(
        matches!(waiting.outcome(), Outcome::Pending { trace: Trace::Export(id) } if *id == job),
        "still waiting, with the confirmation up"
    );

    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));
    let [Effect::Export { job: retried, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    assert_eq!(*retried, job, "the retry reuses the refused attempt's id");

    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Ok(18_420),
            took: Duration::from_millis(1_400),
        },
    );

    let matching: Vec<_> = state
        .log
        .entries()
        .filter(|entry| entry.subject().text().starts_with("SELECT"))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "one entry for the whole export, not one per attempt"
    );
    let Outcome::Succeeded { detail, .. } = matching[0].outcome() else {
        panic!("the retry succeeded");
    };
    assert!(detail.contains("18420"), "{detail}");
}

#[test]
fn abandoning_the_overwrite_confirmation_cancels_the_pending_entry() {
    let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
    let job = state.export.as_ref().expect("a job").id();
    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    handle_key(&mut state, KeyPress::new(Key::Escape));

    let entry = state
        .log
        .entries()
        .find(|entry| entry.subject().text().starts_with("SELECT"))
        .expect("the export entry");
    assert_eq!(*entry.outcome(), Outcome::Cancelled);
}

#[test]
fn a_click_dismissing_the_overwrite_confirmation_cancels_the_pending_entry() {
    let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
    let job = state.export.as_ref().expect("a job").id();
    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    point(&mut state, Pointer::Click(Click::Panel(Panel::Tables)));

    assert!(
        state.overlay.is_none(),
        "the click spends itself dismissing it"
    );
    let entry = state
        .log
        .entries()
        .find(|entry| entry.subject().text().starts_with("SELECT"))
        .expect("the export entry");
    assert_eq!(*entry.outcome(), Outcome::Cancelled);
}

#[test]
fn a_resize_that_closes_the_overwrite_confirmation_cancels_the_pending_entry() {
    let mut state = export_in_flight("SELECT * FROM store.orders", "/tmp/orders.csv");
    let job = state.export.as_ref().expect("a job").id();
    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 14).with_export_fits(false)),
    );

    assert!(state.overlay.is_none(), "the form still does not fit");
    let entry = state
        .log
        .entries()
        .find(|entry| entry.subject().text().starts_with("SELECT"))
        .expect("the export entry");
    assert_eq!(*entry.outcome(), Outcome::Cancelled);
}

#[test]
fn a_fetched_export_lands_a_pending_entry_when_it_starts() {
    let state = export_fetched_in_flight("/tmp/orders.csv");

    let entry = state.log.entries().last().expect("an entry per effect");
    assert!(
        matches!(entry.subject(), LogSubject::Sentence(_)),
        "no statement ran, so the subject is not one"
    );
    assert!(
        matches!(
            entry.outcome(),
            Outcome::Pending {
                trace: Trace::Export(_)
            }
        ),
        "waiting on the same trace a Whole export waits on"
    );
}

#[test]
fn a_finished_fetched_export_completes_its_entry() {
    let mut state = export_fetched_in_flight("/tmp/orders.csv");
    let job = state.export.as_ref().expect("a job").id();

    let _effects = update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Ok(200),
            took: Duration::from_millis(50),
        },
    );

    let entry = state
        .log
        .entries()
        .find(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
        .expect("the fetched export's entry");
    let Outcome::Succeeded { detail, .. } = entry.outcome() else {
        panic!("the export succeeded");
    };
    assert!(detail.contains("200"), "{detail}");
    assert!(detail.contains("/tmp/orders.csv"), "{detail}");
}

#[test]
fn a_failed_fetched_export_completes_its_entry_as_failed() {
    let mut state = export_fetched_in_flight("/tmp/orders.csv");
    let job = state.export.as_ref().expect("a job").id();

    let _effects = update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Io("disk full".to_owned())),
            took: Duration::ZERO,
        },
    );

    let entry = state
        .log
        .entries()
        .find(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
        .expect("the fetched export's entry");
    let Outcome::Failed { detail } = entry.outcome() else {
        panic!("the export failed");
    };
    assert!(detail.contains("disk full"), "{detail}");
}

#[test]
fn a_fetched_export_that_exists_stays_pending_through_the_retry_and_completes_with_it() {
    let mut state = export_fetched_in_flight("/tmp/orders.csv");
    let job = state.export.as_ref().expect("a job").id();

    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    let waiting = state
        .log
        .entries()
        .find(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
        .expect("the fetched export's entry");
    assert!(
        matches!(waiting.outcome(), Outcome::Pending { trace: Trace::Export(id) } if *id == job),
        "still waiting, with the confirmation up"
    );

    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));
    let [Effect::Export { job: retried, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    assert_eq!(*retried, job, "the retry reuses the refused attempt's id");

    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Ok(200),
            took: Duration::from_millis(50),
        },
    );

    let matching: Vec<_> = state
        .log
        .entries()
        .filter(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "one entry for the whole export, not one per attempt"
    );
    let Outcome::Succeeded { detail, .. } = matching[0].outcome() else {
        panic!("the retry succeeded");
    };
    assert!(detail.contains("200"), "{detail}");
}

#[test]
fn abandoning_a_fetched_exports_overwrite_confirmation_cancels_the_pending_entry() {
    let mut state = export_fetched_in_flight("/tmp/orders.csv");
    let job = state.export.as_ref().expect("a job").id();
    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    handle_key(&mut state, KeyPress::new(Key::Escape));

    let entry = state
        .log
        .entries()
        .find(|entry| matches!(entry.subject(), LogSubject::Sentence(_)))
        .expect("the fetched export's entry");
    assert_eq!(*entry.outcome(), Outcome::Cancelled);
}

#[test]
fn a_failure_is_reported_as_a_danger() {
    let mut state = contents_showing_more(PAGE);
    let effects = started(&mut state);
    let [Effect::Export { job, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };

    update(
        &mut state,
        Msg::ExportFinished {
            job: *job,
            result: Err(ExportError::Io("disk full".to_owned())),
            took: Duration::ZERO,
        },
    );

    assert!(state.export.is_none());
    let Some(Overlay::Message(message)) = &state.overlay else {
        panic!("the failure is on screen");
    };
    assert_eq!(message.severity, Severity::Danger);
}

#[test]
fn an_existing_file_reopens_the_form_asking_to_overwrite() {
    let mut state = contents_showing_more(PAGE);
    let effects = started(&mut state);
    let [Effect::Export { job, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };

    update(
        &mut state,
        Msg::ExportFinished {
            job: *job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    assert!(state.export.is_none(), "nothing is running");
    assert_eq!(form_of(&state).confirm(), Some(Confirm::Overwrite));
}

#[test]
fn an_existing_file_landing_on_an_open_picker_puts_the_theme_back() {
    let mut state = contents_showing_more(PAGE);
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14)
        .with_export_fits(true)
        .with_theme_rows(20);
    let effects = started(&mut state);
    let [Effect::Export { job, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    let job = *job;
    let chosen = state.theme.clone();

    handle_key(&mut state, KeyPress::ctrl(Key::Char('t')));
    handle_key(&mut state, KeyPress::new(Key::Char('j')));
    assert_ne!(state.theme, chosen, "the picker is previewing another");

    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    assert_eq!(state.theme, chosen);
    assert_eq!(form_of(&state).confirm(), Some(Confirm::Overwrite));
}

#[test]
fn confirming_re_emits_the_export_with_overwrite_set() {
    let mut state = contents_showing_more(PAGE);
    let effects = started(&mut state);
    let [Effect::Export { job, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    update(
        &mut state,
        Msg::ExportFinished {
            job: *job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

    let [Effect::Export { overwrite, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    assert!(overwrite);
    assert!(state.overlay.is_none());
}

#[test]
fn confirming_re_emits_what_was_attempted_not_what_the_panel_now_holds() {
    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    // Scope: Whole -> Fetched, so the rows ride on the effect.
    handle_key(&mut state, KeyPress::new(Key::Char(' ')));
    for _ in 0..3 {
        handle_key(&mut state, KeyPress::new(Key::Tab));
    }
    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));
    let [Effect::Export { job, source, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    let (job, attempted) = (*job, source.clone());

    let query = QueryId::next();
    state.contents.requested(
        query,
        Subject::Table(TableRef::new("shop", "audit_log")),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );
    state.contents.page_ready(
        query,
        Page {
            columns: vec!["action".to_owned()],
            ..page_of(3)
        },
        crate::config::MaxColumnWidth::default(),
        QueryId::next,
    );

    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );
    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

    let [
        Effect::Export {
            source, overwrite, ..
        },
    ] = effects.as_slice()
    else {
        panic!("one export effect");
    };
    assert!(overwrite);
    assert_eq!(*source, attempted, "shop.orders' rows, not audit_log's");
}

#[test]
fn confirming_still_exports_when_the_panel_has_moved_on_entirely() {
    let mut state = contents_showing_more(PAGE);
    let effects = started(&mut state);
    let [Effect::Export { job, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    let job = *job;

    state.contents.requested(
        QueryId::next(),
        Subject::Typed("SELECT 1".to_owned()),
        crate::model::Statement::typed("SELECT * FROM shop.orders"),
    );

    update(
        &mut state,
        Msg::ExportFinished {
            job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );
    let effects = handle_key(&mut state, KeyPress::new(Key::Char(' ')));

    assert!(matches!(effects.as_slice(), [Effect::Export { .. }]));
}

#[test]
fn esc_from_the_confirmation_goes_back_to_the_form_intact() {
    let mut state = contents_showing_more(PAGE);
    let effects = started(&mut state);
    let [Effect::Export { job, .. }] = effects.as_slice() else {
        panic!("one export effect");
    };
    update(
        &mut state,
        Msg::ExportFinished {
            job: *job,
            result: Err(ExportError::Exists),
            took: Duration::ZERO,
        },
    );

    handle_key(&mut state, KeyPress::new(Key::Escape));

    assert_eq!(form_of(&state).confirm(), None, "back to the form");
    assert_eq!(
        form_of(&state).stop(),
        crate::export::Stop::Export,
        "with every field as it was"
    );
}

#[test]
fn ctrl_e_is_inert_while_an_export_is_already_running() {
    let mut state = contents_showing(5);
    started(&mut state);
    state.overlay = None;

    open_form(&mut state);

    assert!(state.overlay.is_none());
    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "an export is already running"))
    );
}

#[test]
fn the_export_button_runs_on_release_from_any_stop() {
    use crate::export::Stop;
    use crate::pointer::Pointer;

    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    assert_eq!(form_of(&state).stop(), Stop::Path);

    let down = update(&mut state, Msg::Pointer(Pointer::Hold(Key::Enter)));
    assert!(down.is_empty(), "going down runs nothing");
    assert_eq!(crate::view::view_model(&state).pressed, Some(Key::Enter));

    let up = update(&mut state, Msg::Pointer(Pointer::Release(Some(Key::Enter))));

    assert!(matches!(up.as_slice(), [Effect::Export { .. }]));
}

#[test]
fn releasing_off_the_export_button_runs_nothing() {
    use crate::pointer::Pointer;

    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);

    update(&mut state, Msg::Pointer(Pointer::Hold(Key::Enter)));
    let up = update(&mut state, Msg::Pointer(Pointer::Release(None)));

    assert!(up.is_empty());
    assert!(matches!(state.overlay, Some(Overlay::Export(_))));
}

#[test]
fn pressing_a_format_radio_chooses_it_focuses_the_group_and_rewrites_the_extension() {
    use crate::export::{Field, Format, Stop};
    use crate::pointer::Pointer;

    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);

    update(
        &mut state,
        Msg::Pointer(Pointer::Field(Field::Format(Format::Json))),
    );

    assert_eq!(form_of(&state).format(), Format::Json);
    assert_eq!(form_of(&state).stop(), Stop::Format);
    assert_eq!(
        std::path::Path::new(form_of(&state).path().text()).extension(),
        Some(std::ffi::OsStr::new("json"))
    );

    update(
        &mut state,
        Msg::Pointer(Pointer::Field(Field::Format(Format::Json))),
    );
    assert_eq!(
        form_of(&state).format(),
        Format::Json,
        "a radio is not a toggle"
    );
}

#[test]
fn pressing_a_scope_chooses_it_and_the_count_field_chooses_the_first_scope() {
    use crate::export::{Field, Scope, Stop};
    use crate::pointer::Pointer;

    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);

    update(
        &mut state,
        Msg::Pointer(Pointer::Field(Field::Scope(Scope::Fetched))),
    );
    assert_eq!(form_of(&state).scope(), Scope::Fetched);
    assert_eq!(form_of(&state).stop(), Stop::Scope);

    update(
        &mut state,
        Msg::Pointer(Pointer::Field(Field::Rows { cursor: 1 })),
    );
    assert_eq!(form_of(&state).scope(), Scope::First);
    assert_eq!(form_of(&state).stop(), Stop::Rows);
    assert_eq!(form_of(&state).rows().cursor(), 1);
}

#[test]
fn pressing_the_path_places_the_caret_and_focuses_it() {
    use crate::export::{Field, Stop};
    use crate::pointer::Pointer;

    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);
    handle_key(&mut state, KeyPress::new(Key::Tab));

    update(
        &mut state,
        Msg::Pointer(Pointer::Field(Field::Path { cursor: 3 })),
    );

    assert_eq!(form_of(&state).stop(), Stop::Path);
    assert_eq!(form_of(&state).path().cursor(), 3);
}

#[test]
fn a_press_on_the_form_that_hits_no_field_leaves_it_open() {
    use crate::pointer::Pointer;

    let mut state = contents_showing_more(PAGE);
    open_form(&mut state);

    let effects = update(&mut state, Msg::Pointer(Pointer::Modal));

    assert!(effects.is_empty());
    assert!(matches!(state.overlay, Some(Overlay::Export(_))));
}
