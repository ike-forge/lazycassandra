use super::*;

fn land_page(state: &mut State, page: Page) -> Vec<Effect> {
    let query = state.contents.outstanding().expect("a page is outstanding");
    update(
        state,
        Msg::PageReady {
            query,
            page: Box::new(page),
            took: Duration::ZERO,
        },
    )
}

fn fetched_from(effects: &[Effect]) -> Vec<Option<PagingState>> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::ExecutePaged { cursor, .. } => Some(cursor.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn p_fetches_the_next_page_from_where_the_last_one_left_off() {
    let mut state = contents_showing_more(3);
    let conn = current_conn(&state);

    let effects = press(&mut state, Key::Char('p'));

    let query = state.contents.outstanding().expect("the page is in flight");
    assert_eq!(
        effects,
        [Effect::ExecutePaged {
            conn,
            query,
            stmt: Box::new(Statement::select_all("shop", "orders")),
            page_size: crate::config::PageSize::try_new(200).expect("in range"),
            cursor: Some(PagingState(vec![1])),
        }],
        "no history: a continuation is not a new statement"
    );
    assert!(state.in_flight.contains_key(&query));

    assert!(land_page(&mut state, page_of(2)).is_empty());
    assert_eq!(state.contents.row_count(), 5);
    assert!(!state.contents.has_more(), "the last page ends the result");
}

#[test]
fn a_page_asked_for_is_sized_by_grid_page_rows() {
    let mut state = contents_showing_more(3);
    let size = crate::config::PageRows::try_new(50).expect("in range");
    state.config.grid.page_rows = size;

    let effects = press(&mut state, Key::Char('p'));

    assert!(matches!(
        effects.as_slice(),
        [Effect::ExecutePaged { page_size, .. }] if *page_size == size.into()
    ));
}

#[test]
fn paging_is_inert_without_more_elsewhere_and_while_a_page_is_in_flight() {
    for key in [Key::Char('p'), Key::Char('P')] {
        let mut ended = contents_showing(3);
        assert!(
            press(&mut ended, key).is_empty(),
            "{key:?} on an ended result"
        );

        let mut elsewhere = contents_showing_more(3);
        elsewhere.focus_on(Panel::Tables);
        assert!(
            press(&mut elsewhere, key).is_empty(),
            "{key:?} outside the grid"
        );

        let mut flying = contents_showing_more(3);
        press(&mut flying, Key::Char('p'));
        assert!(
            press(&mut flying, key).is_empty(),
            "{key:?} with a page in flight"
        );
    }
}

#[test]
fn shift_p_fetches_pages_until_the_result_ends() {
    let mut state = contents_showing_more(3);

    assert!(!fetched_from(&press(&mut state, Key::Char('P'))).is_empty());
    assert!(!fetched_from(&land_page(&mut state, more_of(3))).is_empty());
    assert!(!fetched_from(&land_page(&mut state, more_of(3))).is_empty());
    assert!(land_page(&mut state, page_of(3)).is_empty());

    assert_eq!(state.contents.row_count(), 12);
    assert!(state.in_flight.is_empty());
}

#[test]
fn continuation_pages_leave_the_trail_as_it_was() {
    let mut state = contents_showing_more(3);
    let before = state.log.len();

    press(&mut state, Key::Char('P'));
    land_page(&mut state, more_of(3));
    land_page(&mut state, page_of(3));

    assert_eq!(state.log.len(), before);
}

#[test]
fn an_empty_first_page_is_followed_under_the_reads_one_entry() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let entries = state.log.len();

    let effects = land_page(&mut state, more_of(0));

    assert_eq!(fetched_from(&effects), [Some(PagingState(vec![1]))]);
    assert!(matches!(
        state.log.entries().last().map(Entry::outcome),
        Some(Outcome::Pending { .. })
    ));

    land_page(&mut state, page_of(2));

    assert_eq!(state.log.len(), entries);
    assert!(matches!(
        state.log.entries().last().map(Entry::outcome),
        Some(Outcome::Succeeded { detail, .. }) if detail == "2 rows"
    ));
    assert_eq!(state.contents.row_count(), 2);
}

#[test]
fn escape_stops_shift_p_and_the_page_in_flight_still_lands() {
    let mut state = contents_showing_more(3);
    press(&mut state, Key::Char('P'));

    let effects = press(&mut state, Key::Escape);

    assert!(effects.is_empty());
    assert_eq!(
        state.focus(),
        Panel::Contents,
        "stopping is the rung that fired"
    );
    assert!(state.contents.outstanding().is_some());
    assert!(land_page(&mut state, more_of(3)).is_empty());
    assert_eq!(state.contents.row_count(), 6);
    assert!(state.contents.can_fetch());
}

#[test]
fn escape_ranks_below_an_overlay_and_above_a_range() {
    let mut state = contents_showing_more(3);
    press(&mut state, Key::Char('P'));
    state.contents.rows.start(0);
    state.overlay = Some(Overlay::Message(Message {
        title: "t".to_owned(),
        message: "m".to_owned(),
        severity: Severity::Info,
    }));

    press(&mut state, Key::Escape);
    assert!(state.overlay.is_none());
    assert!(state.contents.fetching_all(), "the overlay came off first");

    press(&mut state, Key::Escape);
    assert!(!state.contents.fetching_all());
    assert!(
        state.contents.rows.is_active(),
        "the range is the next rung"
    );
}

#[test]
fn reading_another_table_supersedes_a_running_shift_p() {
    let mut state = contents_showing_more(3);
    press(&mut state, Key::Char('P'));
    let running = state.contents.outstanding().expect("a page in flight");

    let effects = read(&mut state, TableRef::new("shop", "users"));

    assert!(effects.contains(&Effect::Cancel(running)));
    assert!(!state.in_flight.contains_key(&running));
    let late = update(
        &mut state,
        Msg::PageReady {
            query: running,
            page: Box::new(more_of(3)),
            took: Duration::ZERO,
        },
    );
    assert!(late.is_empty(), "a superseded page asks for nothing more");
    assert!(matches!(
        state.contents.state(),
        ContentsState::Loading { .. }
    ));
}

#[test]
fn a_failed_page_keeps_the_grid_and_says_so_in_the_trail() {
    let mut state = contents_showing_more(3);
    press(&mut state, Key::Char('p'));
    let query = state.contents.outstanding().expect("a page in flight");

    update(
        &mut state,
        Msg::Failed {
            query,
            error: DbError::Invalid("timed out".to_owned()),
            took: Duration::ZERO,
        },
    );

    assert_eq!(state.contents.row_count(), 3);
    assert!(state.contents.can_fetch());
    let last = state.log.entries().last().expect("an entry");
    assert_eq!(
        last.subject().text(),
        "Reading the next page of shop.orders"
    );
    assert!(matches!(last.outcome(), Outcome::Failed { .. }));
}

#[test]
fn the_bar_names_paging_only_while_there_is_more() {
    let named = |state: &State| {
        crate::view::hints::bar(state)
            .panel
            .into_iter()
            .map(|hint| (format!("{} {}", hint.keys, hint.action), hint.liveness))
            .collect::<Vec<_>>()
    };
    let live = crate::view::Liveness::Live;
    let inert = crate::view::Liveness::Inert;

    let mut state = contents_showing_more(3);
    assert!(named(&state).contains(&("p next page".to_owned(), live)));
    assert!(named(&state).contains(&("P all".to_owned(), live)));

    press(&mut state, Key::Char('P'));
    assert!(named(&state).contains(&("p next page".to_owned(), inert)));
    assert!(named(&state).contains(&("esc stop".to_owned(), live)));

    let ended = contents_showing(3);
    assert!(
        !named(&ended)
            .iter()
            .any(|(hint, _)| hint.contains("page") || hint == "P all")
    );
}

#[test]
fn the_palette_greys_paging_once_the_result_has_ended() {
    let liveness = |state: &State, action: &str| {
        crate::commands::rows(None, state.posture(), &state.contents)
            .into_iter()
            .find_map(|row| match row {
                crate::commands::CommandRow::Command {
                    action: named,
                    liveness,
                    ..
                } if named == action => Some(liveness),
                _ => None,
            })
            .expect("the row is listed")
    };

    for action in ["fetch the next page", "fetch every page"] {
        assert_eq!(
            liveness(&contents_showing_more(3), action),
            crate::view::Liveness::Live
        );
        assert_eq!(
            liveness(&contents_showing(3), action),
            crate::view::Liveness::Inert
        );
    }
}

#[test]
fn a_taller_grid_panel_is_drawn_again() {
    let mut state = contents_showing(30);
    state.dirty = false;

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 20, 1], 0, 0)),
    );

    assert!(
        state.dirty,
        "the projection holds only as many rows as fitted"
    );
}
