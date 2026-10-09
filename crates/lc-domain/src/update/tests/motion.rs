use super::*;

#[test]
fn j_and_k_move_the_grid_once_contents_has_focus() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");
    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(page_of(4)),
            took: Duration::ZERO,
        },
    );
    let tree_row = state.tables.cursor.index();

    state.focus_on(Panel::Contents);
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    assert_eq!(state.contents.cursor.index(), 2);
    assert_eq!(
        state.tables.cursor.index(),
        tree_row,
        "the tree must not move while another panel has focus"
    );
}

#[test]
fn the_grid_cursor_cannot_run_off_a_page() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");
    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(page_of(2)),
            took: Duration::ZERO,
        },
    );

    state.focus_on(Panel::Contents);
    for _ in 0..5 {
        press(&mut state, Key::Char('j'));
    }

    assert_eq!(state.contents.cursor.index(), 1);
}

#[test]
fn motion_in_one_panel_never_reaches_another() {
    let mut state = read_shop_orders();
    let tree_row = state.tables.cursor.index();

    state.focus_on(Panel::Ddl);
    press(&mut state, Key::Char('j'));

    assert_eq!(state.tables.cursor.index(), tree_row);
    assert_eq!(state.contents.cursor.index(), 0);
}

#[test]
fn motion_in_the_ddl_panel_walks_the_rows_it_describes() {
    let mut state = read_shop_orders();
    state.focus_on(Panel::Ddl);
    let rows = crate::view::ddl_rows(&state);
    assert!(rows > 2, "the fixture describes a key and some options");

    for _ in 0..rows + 3 {
        press(&mut state, Key::Char('j'));
    }
    assert_eq!(
        state.ddl.cursor.index(),
        rows - 1,
        "j stops on the last row"
    );

    press(&mut state, Key::Home);
    assert_eq!(state.ddl.cursor.index(), 0);

    press(&mut state, Key::End);
    assert_eq!(state.ddl.cursor.index(), rows - 1);
}

#[test]
fn describing_another_node_opens_the_ddl_panel_at_its_first_row() {
    let mut state = read_shop_orders();
    state.focus_on(Panel::Ddl);
    press(&mut state, Key::End);
    assert!(state.ddl.cursor.index() > 0);

    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('k'));

    assert_eq!(state.ddl.cursor.index(), 0);
    assert_eq!(state.ddl.scroll.offset(), 0);
}

#[test]
fn a_measurement_pulls_the_viewport_onto_the_selected_row() {
    let mut state = read_shop_orders();
    state.focus_on(Panel::Ddl);

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 4, 8, 1], 40, 0).with_ddl_selection(Span {
            first: 20,
            lines: 2,
        })),
    );

    assert_eq!(
        state.ddl.scroll.offset(),
        18,
        "the row measured at lines 20..22 has to be on screen in a four-line panel"
    );
}

fn log_entry(text: &str) -> Entry {
    Entry::new(
        Stamp::default(),
        LogSubject::Sentence(text.to_owned()),
        Outcome::Succeeded {
            detail: "ok".to_owned(),
            took: Duration::from_millis(1),
        },
    )
}

fn geometry_with_log(lines: usize, height: usize, selection: Span) -> Geometry {
    Geometry::new([0, 0, 0, height], 0, 0).with_log(lines, selection)
}

// The Logs tab follows Ddl's own rule (design §5.4): `G` moves only the
// cursor, because the viewport counts wrapped lines the domain cannot
// map a row onto for itself. The viewport catches up only once the
// renderer reports back where the newly-selected entry landed.
#[test]
fn end_reaches_the_last_entry_and_the_next_measurement_follows_it_into_view() {
    let mut state = on_the_trail();
    for index in 0..20 {
        state.log.push(log_entry(&index.to_string()));
    }
    update(
        &mut state,
        Msg::Measured(geometry_with_log(40, 6, Span { first: 0, lines: 2 })),
    );

    press(&mut state, Key::Char('G'));

    assert_eq!(state.log.cursor.index(), 19, "the last of twenty entries");
    assert_eq!(
        state.log.scroll.offset(),
        0,
        "the viewport has not moved: no measurement has reported where entry 19 landed"
    );

    update(
        &mut state,
        Msg::Measured(geometry_with_log(
            40,
            6,
            Span {
                first: 38,
                lines: 2,
            },
        )),
    );

    assert_eq!(
        state.log.scroll.offset(),
        34,
        "the row measured at lines 38..40 has to be on screen in a six-line panel"
    );
}

#[test]
fn enter_outside_the_tree_reads_nothing() {
    let mut state = read_shop_orders();
    state.focus_on(Panel::Contents);

    let effects = press(&mut state, Key::Enter);

    assert!(effects.is_empty());
    assert!(state.contents.outstanding().is_none());
}

#[test]
fn a_reload_that_shrinks_the_tree_pulls_the_selection_inside_it() {
    let mut state = browsing_tables(&[("shop", &["a", "b", "c"])]);
    let conn = current_conn(&state);
    press(&mut state, Key::Char('l'));
    for _ in 0..3 {
        press(&mut state, Key::Char('j'));
    }
    assert_eq!(state.tables.cursor.index(), 3);

    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(tabled_schema(&[("shop", &["a"])])),
            took: Duration::ZERO,
        },
    );

    assert_eq!(
        state.tables.cursor.index(),
        1,
        "the clamp must count tree rows, not keyspaces"
    );
}

#[test]
fn page_down_moves_the_tree_by_one_panel_height() {
    let mut state = browsing(&["a", "b", "c", "d", "e", "f", "g", "h"]);
    sized(&mut state, [3, 0, 0, 0], 0);

    press(&mut state, Key::PageDown);

    assert_eq!(state.tables.cursor.index(), 3);

    press(&mut state, Key::PageUp);

    assert_eq!(state.tables.cursor.index(), 0);
}

#[test]
fn home_and_end_reach_the_ends_of_the_tree_in_one_key() {
    let mut state = browsing(&["a", "b", "c", "d"]);
    sized(&mut state, [3, 0, 0, 0], 0);

    press(&mut state, Key::End);

    assert_eq!(state.tables.cursor.index(), 3);

    press(&mut state, Key::Home);

    assert_eq!(state.tables.cursor.index(), 0);
}

#[test]
fn g_and_shift_g_reach_the_ends_of_the_tree() {
    let mut state = browsing(&["a", "b", "c", "d"]);
    sized(&mut state, [3, 0, 0, 0], 0);

    press(&mut state, Key::Char('G'));
    assert_eq!(state.tables.cursor.index(), 3);

    press(&mut state, Key::Char('g'));
    assert_eq!(state.tables.cursor.index(), 0);
}

#[test]
fn g_and_shift_g_reach_the_ends_of_the_ddl_rows() {
    let mut state = read_shop_orders();
    state.focus_on(Panel::Ddl);
    let rows = crate::view::ddl_rows(&state);

    press(&mut state, Key::Char('G'));
    assert_eq!(state.ddl.cursor.index(), rows - 1);

    press(&mut state, Key::Char('g'));
    assert_eq!(state.ddl.cursor.index(), 0);
}

#[test]
fn g_and_shift_g_reach_the_ends_of_the_grids_rows() {
    let mut state = grid_of_rows(4, 12, 80);
    state.contents.mode = SelectionMode::Cell;
    press(&mut state, Key::Char('l'));

    press(&mut state, Key::Char('G'));
    assert_eq!(state.contents.cursor.index(), 11);
    assert_eq!(
        state.contents.column.index(),
        1,
        "and says nothing about the column axis, as End does not"
    );

    press(&mut state, Key::Char('g'));
    assert_eq!(state.contents.cursor.index(), 0);
}

#[test]
fn g_and_shift_g_reach_the_ends_of_an_open_palette() {
    let mut state = measured(&["a", "b", "c"]);
    press(&mut state, Key::Char('?'));

    press(&mut state, Key::Char('G'));
    assert_eq!(
        palette(&state).expect("still open").selected(),
        crate::commands::row_count(None) - 1
    );

    press(&mut state, Key::Char('g'));
    assert_eq!(
        palette(&state).expect("still open").selected(),
        1,
        "the row after the first heading, which is where the palette opens"
    );
}

#[test]
fn g_and_shift_g_reach_the_ends_of_the_diagnostics() {
    let diagnostics = vec![diagnostic("one"), diagnostic("two"), diagnostic("three")];
    let (mut state, _) = init(Config::default(), diagnostics, Stamp::default());

    press(&mut state, Key::Char('G'));
    assert_eq!(diagnostics_cursor(&state), 2);

    press(&mut state, Key::Char('g'));
    assert_eq!(diagnostics_cursor(&state), 0);
}

#[test]
fn g_is_text_in_queries_and_under_a_prompt() {
    let mut editor = editing("SELECT *");
    press(&mut editor, Key::Char('g'));
    press(&mut editor, Key::Char('G'));
    assert_eq!(editor.queries.text(), "SELECT *gG");

    let mut prompted = browsing(&["a", "b", "c"]);
    press(&mut prompted, Key::Char('/'));
    press(&mut prompted, Key::Char('g'));
    press(&mut prompted, Key::Char('G'));
    assert_eq!(
        searching(&prompted).map(Prompt::query),
        Some("gG"),
        "a prompt claims both, as it claims every printable key"
    );
}

#[test]
fn page_down_moves_the_grid_by_one_panel_height() {
    let mut state = read_shop_orders();
    press(&mut state, Key::Enter);
    let query = state.contents.outstanding().expect("the panel is waiting");
    update(
        &mut state,
        Msg::PageReady {
            query,
            page: Box::new(page_of(30)),
            took: Duration::ZERO,
        },
    );
    state.focus_on(Panel::Contents);
    sized(&mut state, [0, 0, 8, 0], 0);

    press(&mut state, Key::PageDown);

    assert_eq!(state.contents.cursor.index(), 8);
}

#[test]
fn motion_in_the_ddl_panel_moves_its_cursor_not_its_focus() {
    let mut state = browsing(&["a", "b", "c"]);
    state.focus_on(Panel::Ddl);
    sized(&mut state, [0, 6, 0, 0], 20);
    let tree_row = state.tables.cursor.index();
    let rows = crate::view::ddl_rows(&state);

    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    assert_eq!(state.ddl.cursor.index(), 2.min(rows - 1));
    assert_eq!(
        state.tables.cursor.index(),
        tree_row,
        "the tree must not move while another panel has focus"
    );

    press(&mut state, Key::End);

    assert_eq!(state.ddl.cursor.index(), rows - 1);
}

#[test]
fn ddl_motion_still_moves_the_cursor_before_the_first_measurement() {
    let mut state = browsing(&["a", "b", "c"]);
    state.focus_on(Panel::Ddl);
    let rows = crate::view::ddl_rows(&state);
    assert!(
        rows > 1,
        "a keyspace's replication must have more than one row, or End \
         landing on rows - 1 == 0 would hold even if the cursor never moved"
    );

    press(&mut state, Key::PageDown);
    press(&mut state, Key::End);

    assert_eq!(
        state.ddl.cursor.index(),
        rows - 1,
        "the cursor counts rows, not the still-unmeasured panel height"
    );
}

#[test]
fn the_new_motions_emit_no_effects() {
    for key in [Key::PageDown, Key::PageUp, Key::Home, Key::End] {
        let mut state = browsing(&["a", "b", "c"]);
        sized(&mut state, [3, 6, 6, 1], 20);

        let effects = press(&mut state, key);

        assert!(effects.is_empty(), "{key:?} issued {effects:?}");
    }
}

/// The frame this message describes is the frame that produced it.
/// Marking dirty unconditionally would redraw, re-measure and redraw
/// forever (design §3).
#[test]
fn a_measurement_that_changes_nothing_leaves_the_state_clean() {
    let mut state = browsing(&["a", "b", "c"]);
    let geometry = Geometry::new([3, 6, 6, 1], 20, 0);
    update(&mut state, Msg::Measured(geometry));
    state.dirty = false;

    let effects = update(&mut state, Msg::Measured(geometry));

    assert!(effects.is_empty());
    assert!(
        !state.dirty,
        "a measurement of the frame already drawn must not ask for another"
    );
}

/// A table scrolled to its last row and then left must not hand the next
/// table its cursor position: the reader would open mid-document with
/// nothing saying so.
#[test]
fn moving_the_tree_selection_returns_the_ddl_panel_to_the_top() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('l'));
    state.focus_on(Panel::Ddl);
    press(&mut state, Key::End);
    assert!(state.ddl.cursor.index() > 0);

    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('j'));

    assert_eq!(
        state.ddl.cursor.index(),
        0,
        "users is a document of its own"
    );
}

#[test]
fn a_new_subject_clears_the_ddl_range() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('l'));
    state.focus_on(Panel::Ddl);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('v'));
    assert!(state.ddl.selection.is_active());

    state.focus_on(Panel::Tables);
    press(&mut state, Key::Char('j'));

    assert!(
        !state.ddl.selection.is_active(),
        "the rows the range addressed belong to the node that was on screen"
    );
}

#[test]
fn unfolding_a_keyspace_leaves_the_ddl_panel_where_it_is() {
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    state.focus_on(Panel::Ddl);
    press(&mut state, Key::End);
    let end = state.ddl.cursor.index();
    assert!(end > 0);
    state.focus_on(Panel::Tables);

    press(&mut state, Key::Char('l'));

    assert_eq!(
        state.ddl.cursor.index(),
        end,
        "shop is still the selected node, so the panel is still on its description"
    );

    // A second `l` steps onto shop's first table, which is a new subject.
    press(&mut state, Key::Char('l'));

    assert_eq!(state.ddl.cursor.index(), 0);
}

#[test]
fn leaving_ddl_for_tables_clears_its_range_before_unfolding_ever_runs() {
    // Once superseded by `switching_panels_clears_the_selection_of_the_panel_left_behind`:
    // the range used to survive the focus change itself and die only on
    // whatever motion followed. Now `focus_on` is what clears it, so the
    // motion after has nothing left to disturb (design §4).
    let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
    state.focus_on(Panel::Ddl);
    press(&mut state, Key::Char('v'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('v'));
    assert!(state.ddl.selection.is_active());

    state.focus_on(Panel::Tables);

    assert!(!state.ddl.selection.is_active());
}

#[test]
fn a_measurement_pulls_a_stale_scroll_offset_back_inside_the_panel() {
    let mut state = browsing(&["a", "b", "c"]);
    state.focus_on(Panel::Ddl);
    sized(&mut state, [0, 6, 0, 0], 40);
    state.ddl.scroll.to_end(40, 6);
    assert_eq!(state.ddl.scroll.offset(), 34);
    state.dirty = false;

    // The panel shrank to a wrapping of far fewer lines.
    sized(&mut state, [0, 6, 0, 0], 8);

    assert_eq!(state.ddl.scroll.offset(), 2);
    assert!(
        state.dirty,
        "the offset moved, so the frame must be repainted"
    );
}

#[test]
fn the_ddl_cursor_clamps_when_the_selected_table_shrinks() {
    use crate::model::{Column, Keyspace, PrimaryKey, Replication, TableOptions};

    let mut state = browsing_tables(&[("shop", &["orders"])]);
    let conn = current_conn(&state);
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('l'));
    state.focus_on(Panel::Ddl);
    press(&mut state, Key::End);
    assert!(
        state.ddl.cursor.index() > 0,
        "the fixture table has more than one row"
    );

    // Same keyspace and table name, so `described()` sees no subject
    // change — the clamp below is the only thing that can pull the
    // cursor back in.
    let shrunk = crate::model::Table {
        name: "orders".to_owned(),
        primary_key: PrimaryKey {
            partition: vec![Column {
                name: "id".to_owned(),
                cql_type: "uuid".to_owned(),
            }],
            clustering: Vec::new(),
        },
        static_columns: Vec::new(),
        regular_columns: Vec::new(),
        indexes: Vec::new(),
        options: TableOptions::default(),
    };
    let schema = Schema {
        keyspaces: [(
            "shop".to_owned(),
            Keyspace {
                name: "shop".to_owned(),
                replication: Replication::SimpleStrategy { factor: 1 },
                tables: [("orders".to_owned(), shrunk)].into_iter().collect(),
            },
        )]
        .into_iter()
        .collect(),
    };

    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(schema),
            took: Duration::ZERO,
        },
    );

    let rows = crate::view::ddl_rows(&state);
    assert_eq!(
        rows, 3,
        "the keyspace, the table, one partition column and nothing else"
    );
    assert_eq!(
        state.ddl.cursor.index(),
        rows - 1,
        "the clamp must pull the cursor back even though no measurement arrived"
    );
}
