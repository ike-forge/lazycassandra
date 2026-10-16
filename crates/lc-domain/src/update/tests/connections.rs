use super::*;

fn config_of(names: &[&str]) -> Config {
    Config {
        connections: names
            .iter()
            .map(|name| ConnectionConfig {
                name: ConnectionName::try_new(name).expect("a valid name"),
                hosts: vec![format!("{name}.example:9042")],
                posture: Posture::ReadOnly,
                discovery: Discovery::default(),
                datacenter: None,
                credentials: None,
                tls: crate::config::Tls::Off,
            })
            .collect(),
        ..Config::default()
    }
}

const ROOMY: Geometry = Geometry::new([8, 14, 20, 1], 37, 14);

fn roomy() -> Geometry {
    ROOMY.with_connection_rows(20).with_theme_rows(20)
}

/// The picker is offered at the first measurement.
fn started_on(config: Config) -> State {
    let (mut state, _effects) = init(config, Vec::new(), Stamp::default());
    update(&mut state, Msg::Measured(roomy()));
    state
}

fn connection_picker(state: &State) -> Option<usize> {
    match state.overlay.as_ref()? {
        Overlay::Connections { cursor, .. } => Some(cursor.index()),
        _ => None,
    }
}

#[test]
fn connections_with_no_default_open_the_picker() {
    let state = started_on(config_of(&["prod", "staging"]));

    assert_eq!(
        connection_picker(&state),
        Some(0),
        "the picker opens on the first entry when no default names one"
    );
    assert!(matches!(state.connection, ConnectionState::Unchosen));
}

#[test]
fn the_picker_opens_seated_on_the_default_connection() {
    let mut config = config_of(&["prod", "staging", "dev"]);
    config.default_connection = Some(ConnectionName::try_new("dev").expect("a valid name"));
    let mut state = started_on(config);
    // A default connects instead of opening the picker, so reopen it by hand.
    state.connection = ConnectionState::Disconnected;
    state.overlay = None;
    open_connections(&mut state);

    assert_eq!(connection_picker(&state), Some(2));
}

#[test]
fn a_config_with_no_connections_still_opens_the_picker() {
    let state = started_on(Config::default());

    assert_eq!(connection_picker(&state), Some(0));
}

#[test]
fn enter_on_the_picker_connects_to_the_row_under_the_cursor() {
    let mut state = started_on(config_of(&["prod", "staging"]));

    press(&mut state, Key::Char('j'));
    let effects = press(&mut state, Key::Enter);

    assert!(
        matches!(effects.as_slice(), [Effect::Connect { target, .. }]
            if target.name.as_str() == "staging"),
        "the second row is the one opened: {effects:?}"
    );
    assert!(state.overlay.is_none(), "the picker closes behind it");
}

#[test]
fn a_dismissed_picker_is_not_offered_again() {
    let mut state = started_on(config_of(&["prod"]));

    press(&mut state, Key::Escape);
    assert!(state.overlay.is_none());
    assert!(matches!(state.connection, ConnectionState::Disconnected));

    update(&mut state, Msg::Measured(roomy().with_grid_width(40)));

    assert!(
        state.overlay.is_none(),
        "a measurement must not resurrect a picker the user waved away"
    );
}

#[test]
fn ctrl_o_reopens_the_picker() {
    let mut state = started_on(config_of(&["prod"]));
    press(&mut state, Key::Escape);

    update(&mut state, Msg::Input(KeyPress::ctrl(Key::Char('o'))));

    assert_eq!(connection_picker(&state), Some(0));
}

#[test]
fn the_picker_waits_behind_the_diagnostics_modal_and_opens_when_it_closes() {
    let (mut state, _effects) = init(
        config_of(&["prod"]),
        vec![diagnostic("boom")],
        Stamp::default(),
    );
    update(&mut state, Msg::Measured(roomy()));

    assert!(
        matches!(state.overlay, Some(Overlay::Diagnostics(_))),
        "what the config got wrong is read before a connection is chosen"
    );

    press(&mut state, Key::Escape);

    assert_eq!(
        connection_picker(&state),
        Some(0),
        "closing the diagnostics gives the picker its turn"
    );
}

#[test]
fn a_term_filters_the_picker_and_enter_connects_to_what_it_left() {
    let mut state = started_on(config_of(&["prod", "staging", "dev"]));

    press(&mut state, Key::Char('/'));
    for character in "sta".chars() {
        press(&mut state, Key::Char(character));
    }
    let effects = press(&mut state, Key::Enter);

    assert!(
        effects.is_empty(),
        "the first enter closes the prompt rather than connecting"
    );

    let effects = press(&mut state, Key::Enter);

    assert!(
        matches!(effects.as_slice(), [Effect::Connect { target, .. }]
            if target.name.as_str() == "staging"),
        "the row the term left is the row opened: {effects:?}"
    );
}

#[test]
fn a_term_that_matches_nothing_opens_nothing_and_restores_the_cursor() {
    let mut state = started_on(config_of(&["prod", "staging", "dev"]));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('/'));
    for character in "zzz".chars() {
        press(&mut state, Key::Char(character));
    }
    let effects = press(&mut state, Key::Enter);

    assert!(
        effects.is_empty(),
        "a term holding no row opens no connection"
    );
    assert_eq!(
        connection_picker(&state),
        Some(1),
        "the cursor lands back where the prompt opened"
    );

    let effects = press(&mut state, Key::Enter);

    assert!(
        matches!(effects.as_slice(), [Effect::Connect { target, .. }]
            if target.name.as_str() == "staging"),
        "and the row it landed on is the visible one: {effects:?}"
    );
}

#[test]
fn the_connection_opened_carries_its_own_posture() {
    let mut config = config_of(&["prod"]);
    config.connections[0].posture = Posture::Writable;
    let mut state = started_on(config);

    let effects = press(&mut state, Key::Enter);

    assert!(
        matches!(effects.as_slice(), [Effect::Connect { target, .. }]
            if target.posture == Posture::Writable),
    );
}

#[test]
fn a_config_naming_no_default_connects_to_nothing() {
    let (state, effects) = init(Config::default(), Vec::new(), Stamp::default());

    assert!(
        effects.is_empty(),
        "no file named a cluster, so none is reached"
    );
    assert!(matches!(state.connection, ConnectionState::Unchosen));
}

#[test]
fn the_startup_state_is_stamped_by_its_caller() {
    let (state, _effects) = init(Config::default(), Vec::new(), Stamp::new(9, 30, 0));

    assert_eq!(state.now, Stamp::new(9, 30, 0));
}

#[test]
fn a_config_with_nothing_wrong_opens_no_overlay() {
    let (state, _) = init(Config::default(), Vec::new(), Stamp::default());

    assert!(state.overlay.is_none());
}
