use super::*;
use crate::commands::CommandsPanel;
use crate::connection::{Connection, ConnectionState, SchemaState};
use crate::geometry::Geometry;
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::posture::Posture;
use crate::search::{Prompt, Search, Searched};
use crate::state::State;
use crate::theme::ThemeChoice;
use crate::tree::tests_support::schema_of;
use crate::viewport::Cursor;

fn fits() -> State {
    let mut state = State::default();
    state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
    state
}

#[test]
fn a_binding_behind_ctrl_is_named_with_a_ctrl_prefix() {
    assert_eq!(
        label(&[Stroke::ctrl(Key::Char('d')), Stroke::ctrl(Key::Char('u')),]),
        "ctrl-d/ctrl-u"
    );
    assert_eq!(label(&[Stroke::key(Key::Char('j'))]), "j");
}

#[test]
fn several_keys_for_one_action_join_with_a_slash() {
    assert_eq!(
        label(&[Stroke::key(Key::Char('j')), Stroke::key(Key::Char('k'))]),
        "j/k"
    );
    assert_eq!(
        label(&[Stroke::key(Key::PageDown), Stroke::key(Key::PageUp)]),
        "pgdn/pgup"
    );
}

#[test]
fn a_key_with_no_printable_form_is_named_rather_than_drawn() {
    assert_eq!(
        label(&[Stroke::key(Key::Home), Stroke::key(Key::End)]),
        "home/end"
    );
    assert_eq!(label(&[Stroke::key(Key::Char(' '))]), "space");
    assert_eq!(label(&[Stroke::key(Key::Tab)]), "tab");
}

#[test]
fn the_bar_carries_the_focused_panels_bindings_and_the_common_ones() {
    let mut state = State::default();
    state.focus_on(Panel::Tables);

    let bar = bar(&state);

    assert_eq!(bar.panel[0].keys, "⏎");
    assert_eq!(bar.panel[0].action, "open");
    assert_eq!(bar.global.last().expect("y copy is last").action, "copy");
}

#[test]
fn every_panel_draws_a_bar() {
    for panel in Panel::ALL {
        let mut state = fits();
        state.focus_on(panel);

        assert!(!bar(&state).global.is_empty(), "{panel:?} draws no hints");
    }
}

#[test]
fn the_logs_tabs_bar_advertises_a_list_not_an_editor() {
    let mut state = State::default();
    state.focus_on(Panel::Queries);
    state.queries.show(crate::queries::QueriesTab::Logs);

    let bar = bar(&state);

    assert!(
        bar.panel.iter().all(|hint| hint.action != "newline"),
        "⏎ types nothing on the Logs tab: {:?}",
        bar.panel
    );
    assert!(
        bar.global.iter().any(|hint| hint.action == "leave"),
        "esc leaves the panel here as it does on Cql: {:?}",
        bar.global
    );
    assert!(
        bar.global.iter().any(|hint| hint.action == "search"),
        "/ search is genuinely bound here: {:?}",
        bar.global
    );
    assert!(
        bar.global.iter().any(|hint| hint.action == "select"),
        "v select is genuinely bound here: {:?}",
        bar.global
    );
}

#[test]
fn a_panel_that_takes_text_does_not_claim_q_quits() {
    let mut state = fits();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    let bar = bar(&state);

    assert_eq!(bar.panel[0].action, "run");
    assert!(
        bar.global.iter().all(|hint| hint.action != "quit"),
        "q types a q in the editor, and a bar that lies is worse than no bar"
    );
    assert!(
        bar.global.iter().all(|hint| hint.action != "commands"),
        "? types a ? in the editor"
    );
    assert!(bar.global.iter().any(|hint| hint.action == "done"));
}

#[test]
fn the_bar_ends_on_the_palette() {
    assert_eq!(
        bar(&fits()).global.last().map(|hint| hint.action),
        Some("commands"),
        "? outlives every other hint"
    );
}

#[test]
fn the_bar_drops_the_palette_hint_where_no_palette_would_fit() {
    assert!(
        bar(&State::default())
            .global
            .iter()
            .all(|hint| hint.action != "commands"),
        "a hint for a key that can do nothing here is a bar that lies"
    );
}

#[test]
fn the_bar_names_what_the_palette_responds_to() {
    let mut state = State::default();
    state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));

    let bar = bar(&state);

    assert!(
        bar.panel.is_empty(),
        "the palette answers to nothing a terminal list does not: {:?}",
        bar.panel
    );
    assert_eq!(bar.global[0].keys, "⏎");
    assert_eq!(bar.global[0].action, "run");
    assert_eq!(bar.global[1].keys, "?/esc");
    assert_eq!(bar.global[1].action, "close");
    assert_eq!(bar.global[2].keys, "q");
    assert_eq!(bar.global[2].action, "quit");
    assert_eq!(bar.global.last().expect("/ is last").action, "search");
}

#[test]
fn the_overlay_bars_name_the_search_key() {
    let mut state = State::default();
    state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));

    let bar = bar(&state);

    assert!(
        bar.global.iter().any(|hint| hint.action == "search"),
        "the palette's bar names /"
    );
}

#[test]
fn a_live_sift_shows_the_prompts_bar() {
    let mut state = State::default();
    let mut panel = CommandsPanel::opened();
    panel.open_sift();
    state.overlay = Some(Overlay::Commands(panel));

    let bar = bar(&state);
    let actions: Vec<&str> = bar.global.iter().map(|hint| hint.action).collect();

    assert_eq!(actions, ["apply", "cancel"]);
}

#[test]
fn a_live_sift_over_the_picker_shows_the_prompts_bar() {
    let mut state = State::default();
    state.overlay = Some(Overlay::Themes {
        cursor: Cursor::default(),
        restored: ThemeChoice::default(),
        sift: Some(crate::search::Sift::opened(0)),
    });

    let bar = bar(&state);
    let actions: Vec<&str> = bar.global.iter().map(|hint| hint.action).collect();

    assert_eq!(actions, ["apply", "cancel"]);
}

#[test]
fn the_bar_names_what_the_picker_responds_to() {
    let mut state = State::default();
    state.overlay = Some(Overlay::Themes {
        cursor: Cursor::default(),
        restored: ThemeChoice::default(),
        sift: None,
    });

    let bar = bar(&state);

    assert_eq!(
        bar.panel
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .collect::<Vec<_>>(),
        [("j/k", "preview"), ("/", "search")],
        "the focused panel responds to nothing while the picker is up"
    );
    assert_eq!(
        bar.global
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .collect::<Vec<_>>(),
        [("⏎", "keep"), ("esc", "cancel"), ("q", "quit")]
    );
}

#[test]
fn the_connection_pickers_bar_names_connect_and_teaches_no_motion() {
    let mut state = State::default();
    state.config.connections = vec![connection()];
    state.overlay = Some(Overlay::Connections {
        cursor: Cursor::default(),
        sift: None,
    });

    let bar = bar(&state);

    assert_eq!(
        bar.panel
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .collect::<Vec<_>>(),
        [("/", "search")],
        "a motion that previews nothing is not worth the bar's width"
    );
    assert_eq!(
        bar.global
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .collect::<Vec<_>>(),
        [("⏎", "connect"), ("esc", "close"), ("q", "quit")]
    );
}

fn connection() -> crate::config::ConnectionConfig {
    crate::config::ConnectionConfig {
        name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
        hosts: vec!["127.0.0.1:9042".to_owned()],
        posture: crate::posture::Posture::ReadOnly,
        discovery: crate::config::Discovery::default(),
        datacenter: None,
        credentials: None,
        tls: crate::config::Tls::Off,
    }
}

#[test]
fn an_empty_connection_picker_names_neither_search_nor_connect() {
    let mut state = State::default();
    state.overlay = Some(Overlay::Connections {
        cursor: Cursor::default(),
        sift: None,
    });

    let bar = bar(&state);

    assert!(bar.panel.is_empty(), "there is nothing to search");
    assert_eq!(
        bar.global
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .collect::<Vec<_>>(),
        [("esc", "close"), ("q", "quit")],
        "the two ways out are all that is true of an empty picker"
    );
}

#[test]
fn a_live_sift_in_the_connection_picker_shows_the_prompts_bar() {
    let mut state = State::default();
    state.config.connections = vec![connection()];
    state.overlay = Some(Overlay::Connections {
        cursor: Cursor::default(),
        sift: Some(crate::search::Sift::opened(0)),
    });

    let bar = bar(&state);

    assert!(bar.panel.is_empty());
    assert_eq!(
        bar.global
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .collect::<Vec<_>>(),
        [("⏎", "apply"), ("esc", "cancel")]
    );
}

#[test]
fn the_bar_names_what_diagnostics_responds_to() {
    use crate::config::Diagnostic;
    use crate::overlay::DiagnosticsPanel;

    let mut state = State::default();
    state.overlay = Some(Overlay::Diagnostics(DiagnosticsPanel::new(vec![
        Diagnostic {
            origin: crate::config::Origin::new("./.lazycassandra.toml"),
            span: None,
            severity: crate::style::Severity::Warning,
            message: "unknown key `k0` — ignored".to_owned(),
        },
    ])));

    let bar = bar(&state);

    assert!(
        bar.panel.is_empty(),
        "the overlay answers to nothing the focused panel does: {:?}",
        bar.panel
    );
    assert_eq!(
        bar.global
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .collect::<Vec<_>>(),
        [("esc", "close")],
        "esc is the only way out, and the one binding worth naming"
    );
}

#[test]
fn the_bar_names_search_in_every_panel_that_can_hold_a_search() {
    for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
        let mut state = fits();
        state.focus_on(panel);

        assert!(
            bar(&state).global.iter().any(|hint| hint.keys == "/"),
            "{panel:?} responds to / and must say so"
        );
    }
}

#[test]
fn the_bar_does_not_name_search_in_queries() {
    let mut state = fits();
    state.focus_on(Panel::Queries);

    assert!(bar(&state).global.iter().all(|hint| hint.keys != "/"));
}

#[test]
fn the_bar_names_esc_clear_only_where_a_search_is_applied() {
    let mut state = fits();
    state.focus_on(Panel::Tables);
    assert!(bar(&state).global.iter().all(|hint| hint.action != "clear"));

    state.apply_search(Searched::Tables, Search::new("o"));

    assert!(
        bar(&state)
            .global
            .iter()
            .any(|hint| hint.keys == "esc" && hint.action == "clear"),
        "in this frame pressing esc unconditionally does something"
    );
}

#[test]
fn the_bar_names_n_only_where_a_search_is_applied() {
    let mut state = fits();
    state.focus_on(Panel::Tables);
    assert!(
        bar(&state)
            .global
            .iter()
            .all(|hint| hint.action != "next match")
    );

    state.apply_search(Searched::Tables, Search::new("o"));

    assert!(
        bar(&state)
            .global
            .iter()
            .any(|hint| hint.keys == "n" && hint.action == "next match")
    );
}

fn next_match_hint(bar: &HintBar) -> &Hint {
    bar.global
        .iter()
        .find(|hint| hint.action == "next match")
        .expect("a search is applied, so the hint is named")
}

#[test]
fn the_next_match_hint_is_inert_when_the_term_matches_nothing() {
    let mut state = crate::view::tests_support::tabled(&[("shop", &["orders"])]);
    state.focus_on(Panel::Tables);
    state.apply_search(Searched::Tables, Search::new("zzz"));

    assert_eq!(
        next_match_hint(&bar(&state)).liveness,
        Liveness::Inert,
        "n does nothing when nothing matches, and a bar that lies is worse than no bar"
    );
}

#[test]
fn the_next_match_hint_is_live_when_something_matches() {
    let mut state = crate::view::tests_support::tabled(&[("shop", &["orders"])]);
    state.focus_on(Panel::Tables);
    state.apply_search(Searched::Tables, Search::new("shop"));

    assert_eq!(next_match_hint(&bar(&state)).liveness, Liveness::Live);
}

#[test]
fn the_next_match_hint_is_inert_in_contents_with_no_page_loaded() {
    let mut state = fits();
    state.focus_on(Panel::Contents);
    state.apply_search(Searched::Contents, Search::new("id"));

    assert_eq!(
        next_match_hint(&bar(&state)).liveness,
        Liveness::Inert,
        "n does nothing over an empty grid"
    );
}

#[test]
fn the_bar_names_what_the_prompt_responds_to() {
    let mut state = fits();
    state.overlay = Some(Overlay::Search(Prompt::opened(
        Searched::Tables,
        None,
        crate::search::Origin::default(),
    )));

    let bar = bar(&state);

    assert!(
        bar.panel.is_empty(),
        "the panel underneath responds to nothing"
    );
    assert_eq!(
        bar.global
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .collect::<Vec<_>>(),
        [("⏎", "apply"), ("esc", "cancel")]
    );
}

#[test]
fn the_queries_bar_names_the_run_binding() {
    let mut state = fits();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    let panel = bar(&state).panel;

    assert_eq!(
        panel.first().map(|hint| (hint.keys.as_str(), hint.action)),
        Some(("ctrl-r", "run")),
        "the panel exists to run queries, so the bar says that first"
    );
}

#[test]
fn the_queries_bar_names_the_editor_binding_after_run() {
    let mut state = fits();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    let panel = bar(&state).panel;

    assert_eq!(
        panel
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .take(2)
            .collect::<Vec<_>>(),
        [("ctrl-r", "run"), ("ctrl-e", "$EDITOR")],
        "the panel exists to run queries; editing one is the next thing it does"
    );
}

#[test]
fn a_closed_buffer_names_the_way_in_first_and_leaves_with_esc() {
    let mut state = fits();
    state.focus_on(Panel::Queries);

    let bar = bar(&state);

    assert_eq!(
        bar.panel
            .first()
            .map(|hint| (hint.keys.as_str(), hint.action)),
        Some(("⏎", "edit"))
    );
    assert!(bar.global.iter().any(|hint| hint.action == "leave"));
}

#[test]
fn an_open_buffer_names_esc_as_done_rather_than_leave() {
    let mut state = fits();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();

    let panel = bar(&state).global;

    assert!(
        panel
            .iter()
            .any(|hint| (hint.keys.as_str(), hint.action) == ("esc", "done"))
    );
    assert!(panel.iter().all(|hint| hint.action != "leave"));
}

#[test]
fn no_other_bar_names_the_editor_binding() {
    for focus in [Panel::Tables, Panel::Ddl, Panel::Contents] {
        let mut state = fits();
        state.focus_on(focus);

        assert!(
            bar(&state)
                .panel
                .iter()
                .all(|hint| hint.action != "$EDITOR"),
            "{focus:?} does not open an editor"
        );
    }
}

/// `\n` means Enter.
fn queries_bar(text: &str) -> HintBar {
    let mut state = fits();
    state.focus_on(Panel::Queries);
    state.queries.start_editing();
    for character in text.chars() {
        let key = if character == '\n' {
            Key::Enter
        } else {
            Key::Char(character)
        };
        state.queries.edit(key, 1);
    }
    bar(&state)
}

fn run_hint(bar: &HintBar) -> &Hint {
    bar.panel
        .iter()
        .find(|hint| hint.action == "run")
        .expect("the queries bar names the run binding")
}

#[test]
fn the_run_hint_is_inert_while_there_is_nothing_to_run() {
    for buffer in ["", "   ", " \n\t "] {
        let bar = queries_bar(buffer);

        assert_eq!(
            run_hint(&bar).liveness,
            Liveness::Inert,
            "ctrl-r on {buffer:?} submits nothing, and a bar that lies is worse than no bar"
        );
    }
}

#[test]
fn the_run_hint_is_live_once_there_is_a_statement() {
    assert_eq!(
        run_hint(&queries_bar("SELECT * FROM shop.orders")).liveness,
        Liveness::Live
    );
}

#[test]
fn the_run_hint_greys_against_a_buffer_the_posture_would_refuse() {
    for (posture, cql, expected) in [
        (
            Posture::ReadOnly,
            "SELECT * FROM shop.orders",
            Liveness::Live,
        ),
        (
            Posture::ReadOnly,
            "DELETE FROM shop.orders WHERE id = 1",
            Liveness::Inert,
        ),
        (Posture::ReadOnly, "DROP TABLE shop.orders", Liveness::Inert),
        (
            Posture::Writable,
            "DELETE FROM shop.orders WHERE id = 1",
            Liveness::Live,
        ),
    ] {
        let mut state = connected(posture, SchemaState::Loading);
        state.focus_on(Panel::Queries);
        state.queries.start_editing();
        for character in cql.chars() {
            state.queries.edit(Key::Char(character), 1);
        }

        assert_eq!(
            run_hint(&bar(&state)).liveness,
            expected,
            "{cql} under a {} connection",
            posture.label()
        );
    }
}

#[test]
fn the_bar_names_esc_while_a_range_stands() {
    let mut state = fits();
    state.focus_on(Panel::Tables);
    state.tables.selection.start(0);

    let bar = bar(&state);

    assert!(
        bar.global.iter().any(|hint| hint.keys == "esc"),
        "under a live range, pressing esc unconditionally does something"
    );
}

#[test]
fn the_bar_leaves_esc_off_a_panel_with_neither_a_range_nor_a_search() {
    let mut state = fits();
    state.focus_on(Panel::Tables);

    assert!(!bar(&state).global.iter().any(|hint| hint.keys == "esc"));
}

fn hint(state: &State, action: &str) -> Liveness {
    bar(state)
        .panel
        .into_iter()
        .find(|hint| hint.action == action)
        .unwrap_or_else(|| panic!("the panel names {action}"))
        .liveness
}

fn connected(posture: Posture, schema: SchemaState) -> State {
    let mut state = fits();
    state.connection = ConnectionState::Connected(Connection {
        id: crate::model::ConnId::next(),
        name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
        posture,
        schema,
    });
    state
}

#[test]
fn the_drop_hint_is_live_only_where_the_key_would_open_something() {
    let loading = SchemaState::Loading;
    let loaded = SchemaState::Loaded(schema_of(&[("shop", &["orders"])]));

    let mut state = connected(Posture::Writable, loaded.clone());
    state.focus_on(Panel::Tables);
    assert_eq!(
        hint(&state, "drop"),
        Liveness::Inert,
        "a frame with no room for the block draws no question"
    );

    let mut state = connected(Posture::Writable, loading);
    state.focus_on(Panel::Tables);
    state.geometry = state.geometry.with_drop_fits(true);
    assert_eq!(
        hint(&state, "drop"),
        Liveness::Inert,
        "an unread schema names no node to aim at"
    );

    let mut state = connected(Posture::Writable, loaded);
    state.focus_on(Panel::Tables);
    state.geometry = state.geometry.with_drop_fits(true);
    assert_eq!(hint(&state, "drop"), Liveness::Live);
}

#[test]
fn the_health_hint_is_live_on_a_table_with_room_for_the_block() {
    let loaded = SchemaState::Loaded(schema_of(&[("shop", &["orders"])]));
    let mut state = connected(Posture::ReadOnly, loaded);
    state.focus_on(Panel::Tables);
    let schema = state.schema().expect("loaded").clone();
    state.tables.open(&schema);
    state.tables.open(&schema);
    state.geometry = state.geometry.with_health_rows(0);

    assert_eq!(hint(&state, "health"), Liveness::Inert, "no room");

    state.geometry = state.geometry.with_health_rows(10);
    assert_eq!(hint(&state, "health"), Liveness::Live);

    state.tables.cursor_mut().up();
    assert_eq!(
        hint(&state, "health"),
        Liveness::Inert,
        "a keyspace has no health of its own"
    );
}

#[test]
fn a_read_only_connection_leaves_d_off_the_bar() {
    let loaded = SchemaState::Loaded(schema_of(&[("shop", &["orders"])]));

    for panel in [Panel::Tables, Panel::Contents] {
        let mut state = connected(Posture::ReadOnly, loaded.clone());
        state.focus_on(panel);
        state.geometry = state.geometry.with_drop_fits(true);

        assert!(
            bar(&state).panel.iter().all(|hint| hint.keys != "d"),
            "{panel:?} names a key the posture refuses"
        );
    }
}

#[test]
fn the_delete_hint_is_inert_over_a_grid_holding_no_addressable_row() {
    let mut state = connected(
        Posture::Writable,
        SchemaState::Loaded(schema_of(&[("shop", &["orders"])])),
    );
    state.focus_on(Panel::Contents);
    state.geometry = state.geometry.with_drop_fits(true);

    assert_eq!(hint(&state, "delete"), Liveness::Inert);
}

#[test]
fn nothing_but_a_hint_that_would_do_nothing_is_ever_inert() {
    for panel in Panel::ALL {
        let mut state = fits();
        state.focus_on(panel);
        let bar = bar(&state);

        for hint in bar.panel.iter().chain(&bar.global) {
            assert!(
                hint.liveness == Liveness::Live
                    || hint.action == "run"
                    || hint.action == "format"
                    || hint.action == "export"
                    || hint.action == "drop"
                    || hint.action == "health"
                    || hint.action == "delete",
                "{panel:?}'s '{} {}' is inert and nothing says why",
                hint.keys,
                hint.action
            );
        }
    }
}

#[test]
fn every_panel_says_what_it_responds_to() {
    for panel in Panel::ALL {
        assert!(
            !panel.hints().is_empty(),
            "{panel:?} responds to nothing, which no panel does any more"
        );
    }
}

#[test]
fn no_binding_claims_a_key_with_no_action_to_go_with_it() {
    for panel in Panel::ALL {
        for binding in panel.hints() {
            assert!(!binding.keys.is_empty(), "{panel:?} has a keyless hint");
            assert!(!binding.action.is_empty(), "{panel:?} has a nameless hint");
        }
    }
}
