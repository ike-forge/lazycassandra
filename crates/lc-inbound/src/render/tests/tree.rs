use super::*;

#[test]
fn keyspaces_are_listed_inside_the_tables_panel() {
    let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 0));

    assert_eq!(
        row(&buffer, 1),
        "│▌\u{f084} shop       ││Press Enter on a table│"
    );
    assert_eq!(
        row(&buffer, 2),
        "│ \u{f084} typezoo    ││                      │"
    );
    assert_eq!(
        row(&buffer, 3),
        "│ \u{f084} system     ││                      │"
    );
}

#[test]
fn a_tree_row_wears_the_icon_of_what_it_names() {
    let mut state = State::default();
    state.connection = ConnectionState::Connected(Connection {
        id: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
        posture: Posture::ReadOnly,
        schema: SchemaState::Loaded(tabled_schema(&[("shop", &["orders"])])),
    });
    let schema = state.schema().cloned().expect("loaded");
    state.tables.open(&schema);

    let buffer = draw_state(40, 12, &state);

    assert!(
        row(&buffer, 1).contains(&format!("{}shop", crate::icons::KEYSPACE)),
        "the keyspace icon precedes the keyspace"
    );
    assert!(
        row(&buffer, 2).contains(&format!("{}orders", crate::icons::TABLE)),
        "the table icon precedes the table"
    );
}

#[test]
fn the_ddl_identity_rows_wear_the_tree_icons_and_each_heading_its_own() {
    // Wide enough for every icon.
    let buffer = draw_state(160, 41, &describing(orders()));
    let rows = ddl_rows(&buffer);

    assert!(
        rows[0].starts_with(&format!("▌  {}Keyspace", crate::icons::KEYSPACE)),
        "the panel names its keyspace as the tree does, got {:?}",
        rows[0]
    );
    assert!(
        rows[1].starts_with(&format!("   {}Table", crate::icons::TABLE)),
        "and its table likewise, got {:?}",
        rows[1]
    );
    assert!(
        rows.iter()
            .any(|line| *line == format!("{}Partition Key", crate::icons::KEY)),
        "got {rows:?}"
    );
    assert!(
        rows.iter()
            .any(|line| *line == format!("{}Options", crate::icons::SETTINGS)),
        "and a settings section the cog, got {rows:?}"
    );
}

#[test]
fn a_name_with_no_room_beside_its_label_wraps_rather_than_abbreviating() {
    // 23 columns, ten spent on the label, so the name wraps.
    let buffer = draw_state(104, 41, &listing(orders()));

    assert_eq!(
        ddl_rows(&buffer)[..3],
        [
            // The cursor opens on this row.
            format!("▌  {}Keyspace       shop", crate::icons::KEYSPACE),
            format!("   {}Table", crate::icons::TABLE),
            "     orders_by_customer".to_owned(),
        ]
    );
}

#[test]
fn an_unfolded_keyspaces_tables_are_indented_beneath_it() {
    let mut state = State::default();
    state.connection = ConnectionState::Connected(Connection {
        id: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
        posture: Posture::ReadOnly,
        schema: SchemaState::Loaded(tabled_schema(&[
            ("shop", &["orders", "users"]),
            ("typezoo", &[]),
        ])),
    });
    let schema = state.schema().cloned().expect("loaded");
    state.tables.open(&schema);

    let buffer = draw_state(40, 12, &state);

    // Four rows in three, so `typezoo` is below the fold.
    assert_eq!(
        row(&buffer, 1),
        "│▌\u{f084} shop       ▐│Press Enter on a table│"
    );
    assert_eq!(
        row(&buffer, 2),
        "│ ├─ \u{f0ce} orders  ▐│                      │"
    );
    assert_eq!(
        row(&buffer, 3),
        "│ └─ \u{f0ce} users   ▐│                      │"
    );
}

#[test]
fn a_match_is_painted_where_it_sits() {
    // A search forces the matching keyspace open.
    let mut state = State::default();
    state.connection = ConnectionState::Connected(Connection {
        id: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
        posture: Posture::ReadOnly,
        schema: SchemaState::Loaded(tabled_schema(&[("shop", &["orders", "users"])])),
    });
    state.apply_search(Searched::Tables, Search::new("der"));

    let buffer = draw_state(40, 12, &state);
    assert_eq!(
        row(&buffer, 2),
        "│ └─ \u{f0ce} orders  ││                      │"
    );

    let table = Theme::default()
        .style(Token::Table)
        .fg
        .expect("a table spends a colour");
    let matched = Theme::default()
        .style(Token::Match)
        .fg
        .expect("a match spends a colour");
    let fg = |x: u16| buffer.cell((x, 2)).expect("inside the panel").fg;

    // "orders" from column 7: "or" unmatched, "der" matched, "s" unmatched.
    assert_eq!(fg(7), table, "'o' is unmatched");
    assert_eq!(fg(8), table, "'r' is unmatched");
    assert_eq!(fg(9), matched, "'d' wears the match colour");
    assert_eq!(fg(10), matched, "'e' wears the match colour");
    assert_eq!(fg(11), matched, "'r' wears the match colour");
    assert_eq!(fg(12), table, "'s' is unmatched");
}

#[test]
fn a_search_drops_the_tables_it_does_not_reach() {
    let mut state = State::default();
    state.connection = ConnectionState::Connected(Connection {
        id: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
        posture: Posture::ReadOnly,
        schema: SchemaState::Loaded(tabled_schema(&[("shop", &["orders", "users"])])),
    });
    state.apply_search(Searched::Tables, Search::new("orders"));

    let buffer = draw_state(40, 12, &state);

    assert_eq!(
        row(&buffer, 1),
        "│▌\u{f084} shop       ││Press Enter on a table│",
        "the keyspace is still drawn"
    );
    assert_eq!(
        row(&buffer, 2),
        "│ └─ \u{f0ce} orders  ││                      │",
        "the matching row is drawn, and is now the last of its keyspace"
    );
    assert_eq!(
        row(&buffer, 3),
        "│              ││                      │",
        "`users`, which the term does not reach, is not a row"
    );
}

#[test]
fn the_clusters_own_keyspaces_are_dimmed() {
    let buffer = draw_state(40, 12, &browsing(&["shop", "system"], 0));
    // Column 4, past the border, bar and icon.
    let fg = |y: u16| buffer.cell((4, y)).expect("inside the panel").fg;
    let faint = Theme::default()
        .style(Token::Faint)
        .fg
        .expect("faint spends a colour");

    assert_ne!(fg(1), faint, "shop is the user's");
    assert_eq!(fg(2), faint, "system is the cluster's");
}

#[test]
fn a_dimmed_rows_icon_keeps_the_colour_of_what_it_names() {
    let buffer = draw_state(40, 12, &browsing(&["shop", "system"], 0));
    // Column 2, past the border and bar.
    let icon = |y: u16| buffer.cell((2, y)).expect("inside the panel").fg;
    let theme = Theme::default();
    let faint = theme.style(Token::Faint).fg.expect("faint spends a colour");
    let keyspace = theme
        .style(Token::Keyspace)
        .fg
        .expect("a keyspace spends a colour");

    assert_eq!(icon(2), keyspace, "the icon says what the row is");
    assert_eq!(
        buffer.cell((4, 2)).expect("inside the panel").fg,
        faint,
        "the name it precedes stays the cluster's"
    );
}

#[test]
fn the_selected_row_is_the_one_the_cursor_is_on() {
    let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 1));
    let highlight = Theme::default()
        .style(Token::Selected)
        .bg
        .expect("the cursor row spends a background");
    let selected = |y: u16| buffer.cell((1, y)).expect("inside the panel").bg == highlight;

    assert!(!selected(1));
    assert!(selected(2), "typezoo is selected");
    assert!(!selected(3));
}

#[test]
fn a_ranged_tree_row_carries_the_bar_and_the_band() {
    let mut state = browsing(&["shop", "typezoo", "system"], 0);
    state.tables.selection = held(0, 1);

    let buffer = draw_state(40, 12, &state);

    assert_eq!(
        row(&buffer, 2).chars().nth(1),
        Some('▌'),
        "row 1 carries the bar though the cursor is on row 0"
    );
    assert_ne!(
        buffer.cell((1, 2)).expect("inside the panel").bg,
        buffer.cell((1, 3)).expect("inside the panel").bg,
        "row 1 is inside the range and banded, row 2 is not"
    );
    assert_ne!(
        buffer.cell((1, 1)).expect("inside the panel").bg,
        buffer.cell((1, 2)).expect("inside the panel").bg,
        "row 0 is the cursor row, also inside the range, and reads apart from row 1"
    );
}

/// The cursor sits on a fourth row so its highlight cannot stand in for a band.
/// Offset forced to 0.
fn render_tree_with(ranges: Ranges) -> Buffer {
    let state = browsing(&["a", "b", "c", "d"], 3);
    let mut view = view_model(&state);
    let PanelBody::Tree {
        rows,
        selected,
        flash,
        ..
    } = view.panels[0].body.clone()
    else {
        panic!("a loaded schema draws a tree");
    };
    view.panels[0].body = PanelBody::Tree {
        rows,
        selected,
        offset: 0,
        ranges,
        flash,
    };

    draw_view(40, 14, &view)
}

#[test]
fn two_groups_band_separately_with_the_gap_left_unpainted() {
    // A gap stays unbanded.
    let mut ranges = Ranges::default();
    ranges.insert(Range::new(0, 0));
    ranges.insert(Range::new(2, 2));

    let buffer = render_tree_with(ranges);

    assert!(banded(&buffer, 0), "the first group lost its band");
    assert!(!banded(&buffer, 1), "the gap took a band");
    assert!(banded(&buffer, 2), "the second group lost its band");
}

#[test]
fn a_panel_with_nothing_to_list_says_what_it_is_waiting_for() {
    let mut state = State::default();
    state.connection = ConnectionState::Connecting {
        conn: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
    };

    let buffer = draw_state(40, 12, &state);

    assert_eq!(row(&buffer, 1), "│Connecting…   ││Press Enter on a table│");
}

#[test]
fn a_failure_notice_is_drawn_in_the_danger_style_rather_than_dimmed() {
    let mut state = State::default();
    // Startup failures are `Fatal`; a refused read is what still shows a danger
    // notice.
    state.contents.refused(
        Subject::Table(TableRef::new("shop", "orders")),
        &lc_domain::model::DbError::Invalid("undefined column name total".to_owned()),
    );

    let buffer = draw_state(40, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let cell = buffer
        .cell((area.x + 1, area.y + 1))
        .expect("inside the panel");
    let danger = Theme::default()
        .style(Token::Severity(Severity::Danger))
        .fg
        .expect("danger spends a colour");

    assert_eq!(
        cell.fg, danger,
        "a failure notice carries the danger token, not the muted one waiting states use"
    );
    assert!(
        !cell.modifier.contains(Modifier::DIM),
        "a failure must read as urgent, not as background noise"
    );
}
