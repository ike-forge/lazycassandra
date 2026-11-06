use super::*;
use crate::config::Consistency;

fn roomy(mut state: State) -> State {
    state.geometry = state.geometry.with_consistency_rows(20);
    state
}

fn picker_cursor(state: &State) -> Option<Consistency> {
    match &state.overlay {
        Some(Overlay::Consistency { cursor }) => Consistency::ALL.get(cursor.index()).copied(),
        _ => None,
    }
}

/// From the picker's top, so the test names the level rather than a count of
/// presses from wherever the cursor started.
fn choose(state: &mut State, level: Consistency) {
    ctrl(state, Key::Char('l'));
    press(state, Key::Home);
    while picker_cursor(state) != Some(level) {
        press(state, Key::Char('j'));
    }
    press(state, Key::Enter);
}

fn statement_of(effects: &[Effect]) -> Option<&Statement> {
    effects.iter().find_map(|effect| match effect {
        Effect::Execute { stmt, .. }
        | Effect::ExecutePaged { stmt, .. }
        | Effect::ExecuteTraced { stmt, .. } => Some(stmt.as_ref()),
        _ => None,
    })
}

fn on_shop_orders() -> State {
    let mut state = roomy(browsing_tables(&[("shop", &["orders"])]));
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('l'));
    state
}

#[test]
fn a_connection_starts_at_the_configured_level() {
    let mut state = connected(Posture::ReadOnly);
    state.config.driver.consistency = Consistency::LocalOne;
    let conn = current_conn(&state);
    state.connection = ConnectionState::Connecting {
        conn,
        name: ConnectionName::try_new("local").expect("a valid name"),
    };

    update(
        &mut state,
        Msg::Connected {
            conn,
            posture: Posture::ReadOnly,
            took: Duration::ZERO,
        },
    );

    assert_eq!(state.consistency(), Consistency::LocalOne);
}

#[test]
fn ctrl_l_opens_the_picker_on_the_level_in_force() {
    let mut state = roomy(connected(Posture::ReadOnly));

    ctrl(&mut state, Key::Char('l'));

    assert_eq!(picker_cursor(&state), Some(Consistency::LocalQuorum));
}

#[test]
fn enter_sets_the_level_and_esc_leaves_it() {
    let mut state = roomy(connected(Posture::ReadOnly));

    choose(&mut state, Consistency::One);
    assert_eq!(state.consistency(), Consistency::One);
    assert!(state.overlay.is_none());

    ctrl(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Escape);

    assert_eq!(state.consistency(), Consistency::One);
    assert!(state.overlay.is_none());
}

#[test]
fn setting_a_level_leaves_a_line_in_the_trail() {
    let mut state = roomy(connected(Posture::ReadOnly));

    choose(&mut state, Consistency::Serial);

    assert!(matches!(
        state.log.entries().last().map(crate::log::Entry::subject),
        Some(crate::log::Subject::Sentence(text)) if text == "Consistency level set to SERIAL"
    ));
}

#[test]
fn without_a_connection_there_is_no_level_to_set() {
    let mut state = State::default();
    state.geometry = state.geometry.with_consistency_rows(20);

    ctrl(&mut state, Key::Char('l'));

    assert!(state.overlay.is_none());
    assert!(toasted(&state).is_some(), "the press is answered");
}

#[test]
fn a_frame_too_short_for_every_level_opens_no_picker() {
    let mut state = connected(Posture::ReadOnly);
    state.geometry = state
        .geometry
        .with_consistency_rows(Consistency::ALL.len() - 1);

    ctrl(&mut state, Key::Char('l'));

    assert!(state.overlay.is_none());
}

#[test]
fn a_read_runs_at_the_level_the_session_is_at() {
    let mut state = on_shop_orders();
    choose(&mut state, Consistency::LocalOne);

    let effects = press(&mut state, Key::Enter);

    assert_eq!(
        statement_of(&effects).map(|stmt| stmt.consistency),
        Some(Consistency::LocalOne)
    );
}

#[test]
fn a_page_that_follows_runs_at_the_level_the_read_began_at() {
    let mut state = on_shop_orders();
    choose(&mut state, Consistency::One);
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the read is out");
    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(more_of(2)),
            took: Duration::ZERO,
        },
    );

    choose(&mut state, Consistency::All);
    state.focus_on(Panel::Contents);
    let effects = press(&mut state, Key::Char('p'));

    assert_eq!(
        statement_of(&effects).map(|stmt| stmt.consistency),
        Some(Consistency::One),
        "rows read at two levels would be one grid claiming one"
    );
}

#[test]
fn a_write_runs_at_the_session_level_too() {
    let mut state = roomy(connected(Posture::Writable));
    choose(&mut state, Consistency::Quorum);

    let (_, effects) = submit(&mut state, statement(StatementKind::Write)).expect("permitted");

    assert_eq!(
        statement_of(&effects).map(|stmt| stmt.consistency),
        Some(Consistency::Quorum)
    );
}
