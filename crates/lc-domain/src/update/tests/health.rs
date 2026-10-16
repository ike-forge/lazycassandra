use super::*;
use crate::health::{Health, HealthPanel, NodeOutcome, NodeReport, Readings, Thresholds};

fn on_table() -> State {
    let mut state = droppable_tree();
    state.geometry = state.geometry.with_health_rows(20);
    press(&mut state, Key::Char('j'));
    state
}

fn block(state: &State) -> Option<&HealthPanel> {
    match state.overlay.as_ref()? {
        Overlay::Health(panel) => Some(panel),
        _ => None,
    }
}

fn request(effects: &[Effect]) -> QueryId {
    match effects {
        [Effect::LoadHealth { request, .. }] => *request,
        other => panic!("expected one health read, got {other:?}"),
    }
}

fn one_node() -> Health {
    Health {
        nodes: vec![NodeReport {
            address: "10.0.0.1".to_owned(),
            datacenter: None,
            version: Some("5.0.8".to_owned()),
            gossip: None,
            outcome: NodeOutcome::Read {
                readings: Readings::default(),
                thresholds: Thresholds::default(),
            },
        }],
    }
}

fn land(state: &mut State, request: QueryId, health: Result<Health, DbError>) {
    update(
        state,
        Msg::HealthLoaded {
            request,
            health,
            took: Duration::ZERO,
        },
    );
}

#[test]
fn shift_h_on_a_table_opens_the_block_and_reads_that_table() {
    let mut state = on_table();
    let conn = current_conn(&state);

    let effects = press(&mut state, Key::Char('H'));

    assert_eq!(
        effects,
        [Effect::LoadHealth {
            conn,
            request: request(&effects),
            table: TableRef::new("shop", "orders"),
        }]
    );
    assert!(block(&state).expect("the block is up").is_loading());
}

#[test]
fn shift_h_on_a_keyspace_does_nothing() {
    let mut state = on_table();
    press(&mut state, Key::Char('k'));

    assert!(press(&mut state, Key::Char('H')).is_empty());
    assert!(state.overlay.is_none());
}

#[test]
fn shift_h_outside_tables_does_nothing() {
    let mut state = on_table();
    state.focus_on(Panel::Ddl);

    assert!(press(&mut state, Key::Char('H')).is_empty());
    assert!(state.overlay.is_none());
}

#[test]
fn shift_h_where_the_block_cannot_be_drawn_says_so() {
    let mut state = on_table();
    state.geometry = state.geometry.with_health_rows(0);

    assert!(press(&mut state, Key::Char('H')).is_empty());
    assert!(state.overlay.is_none());
    assert_eq!(
        toasted(&state),
        Some((Severity::Info, "no room to draw the health"))
    );
}

#[test]
fn the_loader_turns_while_the_read_is_out_and_rests_once_it_lands() {
    let mut state = on_table();
    let request = request(&press(&mut state, Key::Char('H')));

    assert!(state.is_working());

    land(&mut state, request, Ok(one_node()));

    assert!(!block(&state).expect("still up").is_loading());
    assert!(!state.is_reading_health());
}

#[test]
fn an_answer_for_a_closed_block_is_logged_and_dropped() {
    let mut state = on_table();
    let first = request(&press(&mut state, Key::Char('H')));
    press(&mut state, Key::Escape);
    let second = request(&press(&mut state, Key::Char('H')));

    land(&mut state, first, Ok(one_node()));

    let panel = block(&state).expect("the second block is up");
    assert_eq!(panel.request(), second);
    assert!(panel.is_loading(), "the first answer is not this block's");
}

#[test]
fn a_failed_read_is_drawn_in_the_block() {
    let mut state = on_table();
    let request = request(&press(&mut state, Key::Char('H')));

    land(
        &mut state,
        request,
        Err(DbError::Unreachable("no node answered".to_owned())),
    );

    assert!(matches!(
        block(&state).expect("still up").report(),
        crate::health::Report::Failed(_)
    ));
}

#[test]
fn esc_closes_the_block_and_q_quits_from_it() {
    let mut state = on_table();
    press(&mut state, Key::Char('H'));

    press(&mut state, Key::Escape);
    assert!(state.overlay.is_none());

    press(&mut state, Key::Char('H'));
    press(&mut state, Key::Char('q'));
    assert!(state.should_quit);
}

#[test]
fn motion_inside_the_block_moves_its_cursor_not_the_tree() {
    let mut state = on_table();
    let request = request(&press(&mut state, Key::Char('H')));
    land(&mut state, request, Ok(one_node()));
    let tree = state.tables.cursor().index();

    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('G'));

    assert_eq!(state.tables.cursor().index(), tree);
    let panel = block(&state).expect("still up");
    assert_eq!(panel.selected(), panel.rows().len() - 1);
}

fn landed() -> State {
    let mut state = on_table();
    let request = request(&press(&mut state, Key::Char('H')));
    land(&mut state, request, Ok(one_node()));
    state
}

#[test]
fn a_click_on_a_row_of_the_block_selects_it_and_folds_it_as_enter_does() {
    let mut clicked = landed();
    let mut typed = landed();

    let effects = point(&mut clicked, Pointer::Entry(1));
    press(&mut typed, Key::Char('j'));
    press(&mut typed, Key::Enter);

    let (clicked, typed) = (
        block(&clicked).expect("still up"),
        block(&typed).expect("still up"),
    );
    assert!(effects.is_empty());
    assert_eq!(clicked.selected(), 1);
    assert_eq!(clicked.rows(), typed.rows());
    assert_ne!(
        clicked.rows(),
        block(&landed()).expect("up").rows(),
        "the heading folded"
    );
}

#[test]
fn a_click_on_a_row_the_block_no_longer_has_is_dropped() {
    let mut state = landed();
    let len = block(&state).expect("up").rows().len();

    point(&mut state, Pointer::Entry(len));

    assert_eq!(block(&state).expect("still up").selected(), 0);
}

#[test]
fn a_click_inside_the_block_off_its_rows_keeps_it_open() {
    let mut state = landed();

    point(&mut state, Pointer::Modal);

    assert!(block(&state).is_some());
}
