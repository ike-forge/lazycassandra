use super::*;

#[test]
fn a_stored_offset_decides_which_rows_are_drawn() {
    let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
    let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
    let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
    let mut state = showing(page_of(&["id"], &rows));
    // The selection stays on row 0; only the viewport moves, which is what
    // no test could express before the offset was state.
    state.contents.scroll.follow(12, 1);

    let buffer = draw_state(60, 12, &state);
    let drawn: Vec<String> = (2..7).map(|y| row(&buffer, y)).collect();

    assert!(
        drawn.iter().any(|line| line.contains("row12")),
        "the stored offset decides the top row, got {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|line| line.contains("row00")),
        "the selected row is scrolled away, got {drawn:?}"
    );
}

#[test]
fn a_selection_scrolled_out_of_view_is_not_highlighted() {
    let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
    let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
    let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
    let mut state = showing(page_of(&["id"], &rows));
    state.contents.mode = SelectionMode::Cell;
    state.contents.scroll.follow(12, 1);

    let highlight = Theme::default()
        .style(Token::Crosshair)
        .bg
        .expect("the cell mode's crosshair spends a background");

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let selected = |y: u16| buffer.cell((17, y)).expect("inside the panel").bg == highlight;
    let drawn = grid_row_y(area, 0)..grid_row_y(area, 4);

    for y in drawn.clone() {
        assert!(
            !selected(y),
            "row 0 is off screen, so nothing drawn is the selection"
        );
    }

    // A control: the same panel, drawn with the selection inside the
    // viewport, does highlight it — so the absence above is the rule at
    // work rather than a fixture that stopped drawing rows.
    let mut on_screen = showing(page_of(&["id"], &rows));
    on_screen.contents.mode = SelectionMode::Cell;
    on_screen.contents.cursor.go_to(12, 20);
    on_screen.contents.scroll.follow(12, 4);
    let buffer = draw_state(60, 12, &on_screen);
    let highlighted = drawn
        .clone()
        .any(|y| buffer.cell((17, y)).expect("inside the panel").bg == highlight);

    assert!(
        highlighted,
        "a selection on screen is drawn as the selection"
    );
}

#[test]
fn a_tree_selection_scrolled_out_of_view_is_not_highlighted() {
    // The tree's half of the rule in `a_selection_scrolled_out_of_view_is_
    // not_highlighted`. Without it, handing ratatui a `Some(selected)` it
    // cannot see would walk the offset back to the selection and the wheel
    // would scroll Tables nowhere, with every other test still green
    // (list viewport design §4).
    let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut state = browsing(&refs, 0);
    state.tables.scroll.follow(11, 1);

    let highlight = Theme::default()
        .style(Token::Selected)
        .bg
        .expect("the cursor row spends a background");
    let (buffer, _map) = draw_mapped(40, 12, &state);
    let drawn: Vec<String> = (1..6).map(|y| row(&buffer, y)).collect();
    let selected = |y: u16| buffer.cell((1, y)).expect("inside the panel").bg == highlight;

    assert!(
        drawn.iter().any(|line| line.contains("ks11")),
        "the stored offset decides the top row, got {drawn:?}"
    );
    for y in 1..6 {
        assert!(
            !selected(y),
            "ks00 is off screen, so nothing drawn is the selection"
        );
    }
}

/// The title is the only thing on screen naming the table this panel is
/// showing, and it names it in the tree's colours.
#[test]
fn a_contents_title_draws_its_table_in_the_schemas_colours() {
    let state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));
    let buffer = draw_state(60, 12, &state);
    let theme = Theme::default();
    let border = row(&buffer, 0);
    // Characters, not bytes: the border is drawn out of box-drawing glyphs
    // and an em dash, and a byte offset into it is not a column.
    let at = |text: &str| {
        let byte = border.find(text).expect("the title names the table");
        let column = border[..byte].chars().count();

        u16::try_from(column).expect("a 60-column frame")
    };
    let fg = |x: u16| Some(buffer.cell((x, 0)).expect("inside the border").fg);

    assert_eq!(fg(at("shop")), theme.style(Token::Keyspace).fg);
    assert_eq!(fg(at("orders")), theme.style(Token::Table).fg);
    assert_eq!(fg(at("shop") + 4), theme.style(Token::Faint).fg, "the dot");
}

/// The focused panel's border is bold, and the title inherits it. A schema
/// name is not emphasis, so it does not keep it.
#[test]
fn a_titles_table_is_not_bold_under_the_focused_borders_own_weight() {
    let state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));
    let buffer = draw_state(60, 12, &state);
    let border = row(&buffer, 0);
    let bold = |text: &str| {
        let byte = border.find(text).expect("the title names the table");
        let column = u16::try_from(border[..byte].chars().count()).expect("a 60-column frame");

        buffer
            .cell((column, 0))
            .expect("inside the border")
            .modifier
            .contains(Modifier::BOLD)
    };

    assert!(!bold("shop"), "the keyspace kept the border's weight");
    assert!(!bold("orders"), "the table kept the border's weight");
    assert!(
        bold("[3]"),
        "the panel's own number still wears the focus the border gave it"
    );
}

#[test]
fn a_page_is_drawn_as_a_header_over_its_rows() {
    let state = showing(page_of(
        &["id", "email"],
        &[
            &[CqlValue::Int(1), text("ada@example.com")],
            &[CqlValue::Int(2), text("bob@example.com")],
        ],
    ));

    let buffer = draw_state(60, 12, &state);

    assert_eq!(
        row(&buffer, 0),
        "╭── [1]-Tables ╮╭ [3]-Table contents — shop.orders (2 rows)╮"
    );
    assert_eq!(
        row(&buffer, 1),
        "│▌\u{f084} shop       ││    id \u{21c5} \u{f0b0}  email \u{21c5} \u{f0b0}                     │",
        "the header leaves the bar+gutter block blank, and each name is \
         followed by the two marks its column always carries"
    );
    assert_eq!(
        row(&buffer, 2),
        "│              │├──────────────────────────────────────────┤",
        "a rule under the headings keeps them from reading as a row"
    );
    // The id column is as wide as its heading and marks, not as its
    // one-digit values, so the email column starts four further right
    // than the values alone would put it.
    assert_eq!(
        row(&buffer, 3),
        "│              ││▌1  1       ada@example.com               │"
    );
    assert_eq!(
        row(&buffer, 4),
        "╰──────────────╯│ 2  2       bob@example.com               │"
    );
}

#[test]
fn a_column_wider_than_the_cap_is_ellipsised_rather_than_clipped() {
    // A uuid is 36 columns; the cap is 30.
    let state = showing(page_of(
        &["id"],
        &[&[text("8f14e45f-ceea-467a-9c2b-8c4d2a1e9f01")]],
    ));

    let buffer = draw_state(60, 12, &state);

    assert_eq!(
        row(&buffer, 3),
        "│              ││▌1  8f14e45f-ceea-467a-9c2b-8c4d2…        │",
        "a silently clipped value looks like a shorter value"
    );
}

#[test]
fn a_table_too_wide_for_the_panel_drops_columns_rather_than_squeezing_them() {
    // Four uuid-wide columns want 4×30 + spacing = 126 in a 66-column
    // panel. `Constraint::Length` is a request, not a floor: handed all
    // four, ratatui shrinks every one to fit and truncates each value with
    // no ellipsis to say it did.
    let wide = "8f14e45f-ceea-467a-9c2b-8c4d2a1e9f01";
    let state = showing(page_of(
        &["a", "b", "c", "d"],
        &[&[text(wide), text(wide), text(wide), text(wide)]],
    ));

    let buffer = draw_state(90, 12, &state);

    assert_eq!(
        row(&buffer, 1),
        "│▌\u{f084} shop             ││    a \u{21c5} \u{f0b0}                           b \u{21c5} \u{f0b0}                         │",
        "the two columns that do not fit are not drawn at all"
    );
    assert_eq!(
        row(&buffer, 3),
        "│                    ││▌1  8f14e45f-ceea-467a-9c2b-8c4d2…  8f14e45f-ceea-467a-9c2b-8c4d2…│",
        "and the two that do keep their full width and their ellipsis"
    );
}

#[test]
fn the_key_columns_are_drawn_in_the_styles_that_carry_the_physical_shape() {
    let mut state = showing(page_of(
        &["id", "email", "note"],
        &[&[CqlValue::Int(1), text("a"), text("b")]],
    ));
    // A third column for the cursor to sit on: in the default cell mode
    // the crosshair washes the heading of the column the cursor is in, and
    // the terminal-default theme flattens the text under its wash — so the
    // two headings under test have to be ones the cursor is not on.
    state.contents.column.go_to(2, 3);

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let header = |x: u16| buffer.cell((x, 1)).expect("inside the panel");
    let partition = Theme::default()
        .style(Token::PartitionKey)
        .fg
        .expect("the partition key spends a colour");

    let id = drawn_column(area, 0);
    assert_eq!(header(id).fg, partition, "id is the partition key");
    assert!(header(id).modifier.contains(Modifier::BOLD));
    assert_eq!(
        header(drawn_column(area, 1)).fg,
        Theme::default()
            .style(Token::RegularColumn)
            .fg
            .expect("a regular column reads as body text"),
        "a regular column reads as body text rather than as a key"
    );
}

#[test]
fn a_matched_cell_reads_apart_from_an_unmatched_one_on_and_off_the_selected_row() {
    // Row 0 (not selected) and row 1 (the cursor's row) each hold one
    // cell the search matches and one it does not — the composed case
    // is row 1, where `Token::Match` is drawn under `Token::Selected`'s
    // background bar and still has to be tellable apart from the id
    // beside it.
    let mut state = showing(page_of(
        &["id", "email"],
        &[
            &[CqlValue::Int(1), text("ali@example.com")],
            &[CqlValue::Int(2), text("alice@example.com")],
        ],
    ));
    state.contents.cursor.go_to(1, 2);
    state.contents.scroll.follow(1, 5);
    state.apply_search(Searched::Contents, Search::new("ali"));

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let style = |x: u16, y: u16| {
        let cell = buffer.cell((x, y)).expect("inside the panel");
        (cell.fg, cell.bg, cell.modifier)
    };
    // `id` is two wide and holds the cursor's cell; `email`, the matched
    // one, is the column after it.
    let unmatched_id = drawn_column(area, 0);
    let matched_email = drawn_column(area, 1);

    assert_ne!(
        style(matched_email, grid_row_y(area, 0)),
        style(unmatched_id, grid_row_y(area, 0)),
        "row 0 is not selected: its matched email must still read apart from its unmatched id"
    );
    assert!(
        buffer
            .cell((matched_email, grid_row_y(area, 1)))
            .expect("inside the panel")
            .modifier
            .contains(Modifier::BOLD)
            && !buffer
                .cell((unmatched_id, grid_row_y(area, 1)))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::BOLD),
        "row 1 is the cursor's row: its highlight text flattens both cells' colours, so \
         `BOLD` is what still tells the matched cell apart from the unmatched one \
         (the unmatched id is also the cell cursor, so it carries its own modifiers \
         instead — never `BOLD`)"
    );
}

#[test]
fn a_null_is_faint_so_it_reads_apart_from_the_word() {
    // Row 1 (not the cursor's row) so the row's own colours are not
    // flattened under `Token::Selected`'s highlight text.
    let mut state = showing(page_of(
        &["id", "email"],
        &[
            &[CqlValue::Int(1), text("a")],
            &[CqlValue::Int(2), CqlValue::Null],
        ],
    ));
    state.contents.cursor.go_to(0, 2);

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let cell = |x: u16| {
        buffer
            .cell((x, grid_row_y(area, 1)))
            .expect("inside the panel")
    };
    let faint = Theme::default()
        .style(Token::Faint)
        .fg
        .expect("faint spends a colour");
    let muted = Theme::default()
        .style(Token::Muted)
        .fg
        .expect("muted spends a colour");

    assert_eq!(cell(drawn_column(area, 1)).fg, faint, "the null is faint");
    assert_eq!(
        cell(area.x + 1 + BAR).fg,
        muted,
        "the gutter digit shares the bar+gutter block's column and is muted, not faint"
    );
    assert_ne!(
        cell(drawn_column(area, 0)).fg,
        faint,
        "the value beside it is not — that column is the id value's own, \
         not the gutter, and is not what this is about"
    );
}

#[test]
fn a_matched_null_still_carries_the_match_colour() {
    // The ramp carries the tier now, not a modifier (design §4.1), so a
    // null's own token (`Token::Faint`) has no modifier for
    // `Style::patch` to carry through `Token::Match` overwriting `fg`. A
    // matched null and a matched word both end up drawn as a plain
    // match, indistinguishable from each other; telling a matched null
    // apart from a matched word again is future work, not this token's
    // job.
    let mut state = showing(page_of(
        &["id", "value", "note"],
        &[&[CqlValue::Int(1), CqlValue::Null, text("null")]],
    ));
    state.apply_search(Searched::Contents, Search::new("null"));

    let buffer = draw_state(70, 12, &state);
    // Shifted right by the bar, the row-number gutter and their spacing:
    // one digit for a one-row page, plus one column each (table display
    // design §4, design §4.2).
    let area = block_area(&buffer, "Table contents");
    let cell = |x: u16| {
        buffer
            .cell((x, grid_row_y(area, 0)))
            .expect("inside the panel")
    };
    // `id` is five columns (its name and marks outrun a one-digit value),
    // `value` eight (`value` plus marks), each followed by the spacing —
    // so the null draws seven columns in and the word seventeen.
    let null_cell = cell(first_cell(area, 1) + 7);
    let word_cell = cell(first_cell(area, 1) + 17);

    assert_eq!(
        null_cell.fg, word_cell.fg,
        "both are matches, so they carry the same match colour"
    );
    assert!(
        !null_cell.modifier.contains(Modifier::DIM) && !word_cell.modifier.contains(Modifier::DIM),
        "no token recedes with a modifier now that the ramp carries the tier"
    );
}

#[test]
fn the_selected_row_of_the_grid_is_the_one_the_cursor_is_on() {
    let mut state = showing(page_of(
        &["id"],
        &[&[CqlValue::Int(1)], &[CqlValue::Int(2)]],
    ));
    state.contents.mode = SelectionMode::Cell;
    state.contents.cursor.down(2);

    let highlight = Theme::default()
        .style(Token::Crosshair)
        .bg
        .expect("the cell mode's crosshair spends a background");
    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let selected = |y: u16| buffer.cell((17, y)).expect("inside the panel").bg == highlight;

    assert!(!selected(1), "the header is never the selection");
    assert!(!selected(grid_row_y(area, 0)), "nor is the first row");
    assert!(selected(grid_row_y(area, 1)), "the second row is");
}

#[test]
fn row_mode_lights_the_whole_row_and_no_column() {
    let mut state = showing(page_of(
        &["id", "name"],
        &[
            &[CqlValue::Int(1), text("alice")],
            &[CqlValue::Int(2), text("bob")],
        ],
    ));
    state.contents.mode = SelectionMode::Row;
    state.contents.cursor.down(2);
    // The column cursor is still somewhere, so "no column" is a claim about
    // what row mode draws rather than about where the cursor happens to be.
    state.contents.column.go_to(1, 2);

    let theme = Theme::default();
    let selected = theme
        .style(Token::Selected)
        .bg
        .expect("the cursor row spends a background");
    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let bg = |x: u16, y: u16| buffer.cell((x, y)).expect("inside the panel").bg;
    let name = area.x + 1 + 6;

    assert_eq!(
        bg(17, grid_row_y(area, 1)),
        selected,
        "the row the cursor is on is lit"
    );
    assert_ne!(
        bg(name, grid_row_y(area, 0)),
        selected,
        "and the cursor's column off that row is not — row mode lights no column"
    );
    assert_ne!(
        bg(17, grid_row_y(area, 0)),
        selected,
        "and the row above it is not — row mode lights one row"
    );
    assert_ne!(
        bg(17, 1),
        selected,
        "the header is never part of the selection in row mode"
    );
}

#[test]
fn column_mode_lights_the_column_and_its_heading() {
    let mut state = showing(page_of(
        &["id", "name"],
        &[
            &[CqlValue::Int(1), text("alice")],
            &[CqlValue::Int(2), text("bob")],
        ],
    ));
    state.contents.mode = SelectionMode::Column;
    state.contents.column.go_to(1, 2);

    let theme = Theme::default();
    let selected = theme
        .style(Token::Selected)
        .bg
        .expect("the cursor column spends a background");
    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let bg = |x: u16, y: u16| buffer.cell((x, y)).expect("inside the panel").bg;
    let name = drawn_column(area, 1);

    assert_eq!(
        bg(name, grid_row_y(area, 0)),
        selected,
        "every row of the column is lit"
    );
    assert_eq!(
        bg(name, grid_row_y(area, 1)),
        selected,
        "including the last one"
    );
    assert_eq!(
        bg(name, 1),
        selected,
        "and its heading, which ratatui leaves to us (design §4)"
    );
    assert_ne!(
        bg(drawn_column(area, 0), grid_row_y(area, 0)),
        selected,
        "the row the cursor is on is not — column mode lights one column"
    );
}

/// A yank spends the selection that named the rows, so the flash is the
/// only thing left saying what was copied — and the three bands are one
/// ramp apart, so no colour among them reads as a change against the band
/// standing there a frame ago. The afterglow inverts instead (visual mode
/// design §6.1).
#[test]
fn a_yanks_afterglow_inverts_the_rows_it_took() {
    let mut ranged = showing(sized_page(2, 4));
    ranged.contents.mode = SelectionMode::Row;
    ranged.contents.cursor.go_to(3, 4);
    ranged.contents.rows = held(0, 1);

    let mut flashed = showing(sized_page(2, 4));
    flashed.contents.mode = SelectionMode::Row;
    flashed.contents.cursor.go_to(3, 4);
    flashed.flash = Some(Flash::lit(
        Panel::Contents,
        Ranges::of(Range::new(0, 1)),
        Ranges::default(),
    ));

    let cell = |state: &State, row: u16| {
        let buffer = draw_state(60, 12, state);
        let area = block_area(&buffer, "Table contents");
        buffer
            .cell((area.x + 4, grid_row_y(area, row)))
            .expect("inside the panel")
            .clone()
    };
    let reversed = |state: &State, row: u16| cell(state, row).modifier.contains(Modifier::REVERSED);

    assert_eq!(cell(&ranged, 0).bg, Theme::default().palette().ranged);
    assert!(
        !reversed(&ranged, 0),
        "a held range is banded, not inverted"
    );
    assert!(
        reversed(&flashed, 0),
        "and the afterglow inverts what it took"
    );
    assert!(
        !reversed(&flashed, 2),
        "leaving the rows the yank did not take alone"
    );
    assert_eq!(
        cell(&flashed, 2).bg,
        cell(&ranged, 2).bg,
        "in colour as well as in modifier"
    );
}

/// §5.1's inversion is a projection of every group a multi-group `y` took,
/// not just the one range the single-group test above pins
/// (multi-group selection design §8).
#[test]
fn a_multi_group_yanks_afterglow_inverts_every_group_it_took() {
    let mut ranges = Ranges::default();
    ranges.insert(Range::new(0, 0));
    ranges.insert(Range::new(2, 2));

    let mut flashed = showing(sized_page(2, 4));
    flashed.contents.mode = SelectionMode::Row;
    flashed.contents.cursor.go_to(3, 4);
    flashed.flash = Some(Flash::lit(Panel::Contents, ranges, Ranges::default()));

    let cell = |state: &State, row: u16| {
        let buffer = draw_state(60, 12, state);
        let area = block_area(&buffer, "Table contents");
        buffer
            .cell((area.x + 4, grid_row_y(area, row)))
            .expect("inside the panel")
            .clone()
    };
    let reversed = |state: &State, row: u16| cell(state, row).modifier.contains(Modifier::REVERSED);

    assert!(reversed(&flashed, 0), "the first group inverts");
    assert!(
        !reversed(&flashed, 1),
        "the gap between groups stays unpainted"
    );
    assert!(reversed(&flashed, 2), "the second group inverts too");
}

/// Cell mode's cursor cell is inverted at rest, so an afterglow adding the
/// same inversion left the cell a `y` had just taken looking untouched.
/// The flash un-inverts it instead (visual mode design §6.1).
#[test]
fn a_cell_mode_yanks_afterglow_reads_on_the_cursors_own_cell() {
    let at = |flash: Option<Flash>| {
        let mut state = showing(sized_page(2, 4));
        state.contents.mode = SelectionMode::Cell;
        state.contents.cursor.go_to(1, 4);
        state.contents.column.go_to(1, 2);
        state.flash = flash;

        let buffer = draw_state(60, 12, &state);
        let area = block_area(&buffer, "Table contents");
        buffer
            .cell((drawn_column(area, 1), grid_row_y(area, 1)))
            .expect("inside the panel")
            .clone()
    };

    let resting = at(None);
    let flashed = at(Some(Flash::lit(
        Panel::Contents,
        Ranges::of(Range::new(1, 1)),
        Ranges::of(Range::new(1, 1)),
    )));

    assert!(
        resting.modifier.contains(Modifier::REVERSED),
        "the cursor cell is inverted at rest"
    );
    assert!(
        !flashed.modifier.contains(Modifier::REVERSED),
        "and the afterglow swaps it back, so the yank reads"
    );
    assert!(
        flashed.modifier.contains(Modifier::UNDERLINED),
        "while the cursor is still findable under the flash"
    );
}

/// The flash reaches the panel whose `y` lit it and no other.
#[test]
fn a_yank_in_the_tree_lights_the_tree_alone() {
    let mut state = showing(sized_page(2, 4));
    state.flash = Some(Flash::lit(
        Panel::Tables,
        Ranges::of(Range::new(0, 0)),
        Ranges::default(),
    ));

    let buffer = draw_state(60, 12, &state);
    let tree = block_area(&buffer, "Tables");
    let grid = block_area(&buffer, "Table contents");
    let inverted = |x: u16, y: u16| {
        buffer
            .cell((x, y))
            .expect("inside the panel")
            .modifier
            .contains(Modifier::REVERSED)
    };

    assert!(inverted(tree.x + 2, tree.y + 1));
    // Row 2, not the grid's own row 0: cell mode inverts the cursor's own
    // cell whether or not anything flashed, and row 0 would pass this
    // assertion for the wrong reason.
    assert!(
        !inverted(grid.x + 4, grid.y + 4),
        "a flash lit over the tree reaches no other panel"
    );
}

#[test]
fn every_row_of_a_range_is_banded_and_the_cursor_row_reads_apart_from_them() {
    let mut state = showing(sized_page(2, 4));
    state.contents.mode = SelectionMode::Row;
    state.contents.cursor.down(4);
    state.contents.rows = held(0, 2);

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let bg = |row: u16, column: u16| {
        buffer
            .cell((drawn_column(area, column), grid_row_y(area, row)))
            .expect("inside the panel")
            .bg
    };

    let banded = bg(0, 0);
    let cursor = bg(1, 0);
    let plain = bg(3, 0);

    assert_ne!(banded, plain, "a row in the range is banded");
    assert_ne!(
        banded, cursor,
        "and the cursor row still reads apart from it"
    );
    assert_ne!(
        bg(0, 1),
        bg(3, 1),
        "a second column on the banded row is banded too, not just the first"
    );
}

/// A band drawn cell by cell breaks at every column spacing, so a held
/// range reads as one stripe per column instead of one selection.
#[test]
fn a_row_range_bands_the_spacing_between_its_columns_too() {
    let mut state = showing(sized_page(2, 4));
    state.contents.mode = SelectionMode::Row;
    state.contents.cursor.down(4);
    state.contents.rows = held(0, 2);

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let bg = |row: u16, x: u16| {
        buffer
            .cell((x, grid_row_y(area, row)))
            .expect("inside the panel")
            .bg
    };

    // The row number in the gutter and the blank ahead of the second
    // column: neither is a cell of the row, and both are inside it.
    for x in [area.x + 1 + BAR, drawn_column(area, 1) - 1] {
        assert_eq!(
            bg(0, x),
            bg(0, drawn_column(area, 0)),
            "the band runs the width of the row, gutter and spacing included"
        );
        assert_ne!(bg(3, x), bg(0, x), "and stops at the range's last row");
    }
}

#[test]
fn a_cell_block_bands_the_rectangle_and_nothing_outside_it() {
    let mut state = showing(sized_page(3, 3));
    // The grid opens in row mode, and only cell mode has a rectangle: the
    // projection gives a column range on no other (visual mode design §3).
    state.contents.mode = SelectionMode::Cell;
    state.contents.rows = held(0, 1);
    state.contents.columns = held(0, 1);

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let bg = |row: u16, column: u16| {
        buffer
            .cell((drawn_column(area, column), grid_row_y(area, row)))
            .expect("inside the panel")
            .bg
    };

    assert_ne!(
        bg(1, 1),
        bg(1, 2),
        "the block ends at the column range's edge"
    );
    // Columns 1 and 2 rather than 0 and 2: the cursor's own column is 0,
    // and cell mode washes it down every row regardless of the range —
    // reading it here would prove nothing about the range.
    assert_eq!(
        bg(2, 1),
        bg(2, 2),
        "outside the row range, neither cell is banded"
    );
}

#[test]
fn a_column_range_bands_its_columns_and_lights_no_gutter_bar() {
    let mut state = showing(sized_page(3, 3));
    state.contents.mode = SelectionMode::Column;
    state.contents.columns = held(0, 1);

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let bar = buffer
        .cell((area.x + 1, grid_row_y(area, 1)))
        .expect("inside the panel");

    assert_eq!(
        bar.symbol(),
        " ",
        "the range is columns; a bar down every row would claim the rows were selected"
    );

    // Row 2 (index 1), off the cursor row: column 0 is inside the range,
    // column 2 is not.
    let bg = |column: u16| {
        buffer
            .cell((drawn_column(area, column), grid_row_y(area, 1)))
            .expect("inside the panel")
            .bg
    };
    assert_ne!(
        bg(0),
        bg(2),
        "a ranged column is banded and an unranged one is not, off the cursor row"
    );
}

/// `follow_grid_columns` pulls the viewport over the column cursor against
/// the geometry of the frame *before* this one, so a resize that narrows
/// the panel draws one frame whose cursor is off the right of what fits.
/// ratatui clamps `selected_column` to the last column it drew, which would
/// band a column the heading gate does not agree with.
#[test]
fn column_mode_lights_nothing_when_its_cursor_is_off_the_columns_drawn() {
    let mut state = showing(sized_page(12, 3));
    state.contents.mode = SelectionMode::Column;
    state.contents.column.go_to(11, 12);

    let buffer = draw_state(40, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let selected = Theme::default()
        .style(Token::Selected)
        .bg
        .expect("the cursor column spends a background");

    assert_eq!(
        state.contents.column_scroll.first(),
        0,
        "the viewport has not followed the cursor, which is the state under test"
    );
    for y in area.y + 1..area.y + area.height - 1 {
        for x in area.x + 1..area.x + area.width - 1 {
            assert_ne!(
                buffer.cell((x, y)).expect("inside the panel").bg,
                selected,
                "({x}, {y}) is lit for a column the grid is not drawing"
            );
        }
    }
}

/// The heading's wash is the crosshair's, and a row cursor scrolled out of
/// view leaves the widget painting no crosshair at all — so the heading
/// cannot stay lit on its own (design §4).
#[test]
fn a_heading_is_unwashed_once_the_crosshair_scrolls_out_of_view() {
    let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
    let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
    let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
    let mut state = showing(page_of(&["id"], &rows));
    state.contents.mode = SelectionMode::Cell;

    let crosshair = Theme::default()
        .style(Token::Crosshair)
        .bg
        .expect("the cell mode's crosshair spends a background");
    let heading = |buffer: &Buffer| {
        let area = block_area(buffer, "Table contents");
        buffer
            .cell((first_cell(area, 2), 1))
            .expect("inside the panel")
            .bg
    };

    let on_screen = draw_state(60, 12, &state);
    assert_eq!(
        heading(&on_screen),
        crosshair,
        "with the cursor in view the heading is washed with its column"
    );

    state.contents.scroll.follow(12, 1);
    let scrolled_away = draw_state(60, 12, &state);

    assert_ne!(
        heading(&scrolled_away),
        crosshair,
        "and once the selection is off screen nothing is left to wash it"
    );
}

/// The wash is composed onto the heading's own token rather than replacing
/// it, so a key column still says which key it is under the crosshair
/// wherever the theme keeps its colours (design §4).
#[test]
fn a_key_columns_heading_keeps_its_colour_under_the_wash() {
    let theme = Theme::of(&ThemeChoice::Ported(Ported::CatppuccinMocha));
    let mut state = showing(page_of(&["id", "email"], &[&[CqlValue::Int(1), text("a")]]));
    state.contents.mode = SelectionMode::Cell;

    let buffer = draw_state_with(60, 12, &state, theme);
    let area = block_area(&buffer, "Table contents");
    let heading = buffer
        .cell((first_cell(area, 1), 1))
        .expect("inside the panel");

    assert_eq!(
        heading.bg,
        theme.palette().crosshair,
        "the cursor's column is washed, heading and all"
    );
    assert_eq!(
        heading.fg,
        theme
            .style(Token::PartitionKey)
            .fg
            .expect("the partition key spends a colour"),
        "and `id` still reads as the partition key under it"
    );
}

/// The wash is a background alone wherever the theme keeps its colours, so
/// a null under the crosshair is still drawn as a null (design §4).
///
/// The null sits off the cursor row on purpose: on it, the row arm of the
/// crosshair would paint the same background, and dropping the column arm
/// entirely would go unnoticed.
#[test]
fn the_crosshair_leaves_a_flavours_column_colours_alone() {
    let theme = Theme::of(&ThemeChoice::Ported(Ported::CatppuccinMocha));
    let mut state = showing(page_of(
        &["id", "value"],
        &[
            &[CqlValue::Int(1), CqlValue::Int(2)],
            &[CqlValue::Int(3), CqlValue::Null],
        ],
    ));
    state.contents.mode = SelectionMode::Cell;
    state.contents.column.go_to(1, 2);

    let buffer = draw_state_with(60, 12, &state, theme);
    let area = block_area(&buffer, "Table contents");
    let null = buffer
        .cell((drawn_column(area, 1), grid_row_y(area, 1)))
        .expect("inside the panel");

    assert_ne!(
        buffer
            .cell((area.x + 1, grid_row_y(area, 1)))
            .expect("inside the panel")
            .bg,
        theme.palette().crosshair,
        "the second row is not the cursor row, so only the column arm can wash it"
    );
    assert_eq!(
        null.bg,
        theme.palette().crosshair,
        "the selected column is washed down its whole height"
    );
    assert_eq!(
        null.fg,
        theme.style(Token::Faint).fg.expect("a null is faint"),
        "and the null under it keeps its own colour"
    );
}

#[test]
fn the_grid_scrolls_to_keep_the_selection_on_screen() {
    // The Contents panel holds a two-row header and four rows at this
    // size, so a twenty-row page forces the scroll a plain `Table` would
    // not perform.
    let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
    let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
    let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
    let mut state = showing(page_of(&["id"], &rows));
    select_contents(&mut state, 19, 4);

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let drawn: Vec<String> = (grid_row_y(area, 0)..grid_row_y(area, 4))
        .map(|y| row(&buffer, y))
        .collect();

    assert!(
        drawn.iter().any(|line| line.contains("row19")),
        "the selected row must stay on screen once the page scrolls"
    );
    assert!(
        !drawn.iter().any(|line| line.contains("row00")),
        "a row scrolled out of view must not still be drawn"
    );
}

#[test]
fn a_table_with_no_rows_says_so_where_the_grid_would_be() {
    let state = showing(page_of(&["id"], &[]));

    let buffer = draw_state(60, 12, &state);

    assert_eq!(
        row(&buffer, 1),
        "│▌\u{f084} shop       ││No rows                                   │"
    );
}
