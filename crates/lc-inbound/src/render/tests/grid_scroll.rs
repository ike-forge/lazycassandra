use super::*;

/// A `width`-wide frame showing a page of `columns` columns and `rows`
/// rows, with the cursor on its first cell.
fn frame_showing_grid(columns: usize, width: u16, rows: usize) -> Buffer {
    draw_state(width, 12, &showing(sized_page(columns, rows)))
}

/// The same, one column wide, scrolled so that `offset` is the top row.
fn frame_showing_scrolled_grid(rows: usize, offset: usize) -> Buffer {
    let mut state = showing(sized_page(1, rows));
    state.contents.scroll.follow(offset, 1);

    draw_state(60, 12, &state)
}

#[test]
fn a_grid_wider_than_its_panel_gets_a_horizontal_scrollbar() {
    let buffer = frame_showing_grid(12, 40, 20);

    assert!(has_horizontal_scrollbar(&buffer, "Table contents"));
}

#[test]
fn a_grid_that_fits_gets_no_horizontal_scrollbar() {
    let buffer = frame_showing_grid(2, 200, 20);

    assert!(
        !has_horizontal_scrollbar(&buffer, "Table contents"),
        "its absence is what says nothing is hidden"
    );
}

#[test]
fn the_selected_cell_is_marked_apart_from_the_rest_of_its_row() {
    let mut state = showing(sized_page(4, 3));
    state.contents.mode = SelectionMode::Cell;
    let buffer = draw_state(40, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let underlined = |x: u16, y: u16| {
        buffer
            .cell((x, y))
            .expect("inside the panel")
            .modifier
            .contains(Modifier::UNDERLINED)
    };
    // The first row — the one the cursor is on, at its first column.
    let first = drawn_column(area, 0);
    let selected = grid_row_y(area, 0);

    assert!(underlined(first, selected), "the cell the cursor is on");
    assert!(underlined(first + 1, selected), "for its whole width");
    assert!(
        !underlined(drawn_column(area, 1), selected),
        "the next column of the same row is not what a command would act on"
    );
    assert!(
        !underlined(first, selected + 1),
        "nor is the same column of the next row"
    );
}

/// The crosshair is a locator and the range is a selection, so where the
/// two cross the range is what the cell has to say (design §6).
#[test]
fn a_ranged_cell_keeps_its_band_where_the_crosshair_crosses_it() {
    let mut state = showing(sized_page(4, 3));
    state.contents.mode = SelectionMode::Cell;
    state.contents.rows = held(0, 1);
    state.contents.columns = held(0, 1);

    let buffer = draw_state(40, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let bg = |column: u16, row: u16| {
        buffer
            .cell((drawn_column(area, column), grid_row_y(area, row)))
            .expect("inside the panel")
            .bg
    };
    let theme = Theme::default();

    assert_eq!(
        bg(1, 0),
        theme.palette().ranged,
        "the cursor's own row crosses the range, and the range wins"
    );
    assert_eq!(
        bg(0, 1),
        theme.palette().ranged,
        "so does the cursor's own column"
    );
    assert_eq!(
        bg(2, 0),
        theme.palette().crosshair,
        "the crosshair still runs past the range it crosses"
    );
}

#[test]
fn the_row_and_the_cell_are_marked_in_an_unfocused_panel_too() {
    // `body::grid` ignores the `focused` flag, which is design §3's
    // deliberate decision rather than an oversight: every other selection
    // in the program is drawn unfocused, and the border says which panel
    // is live. True by construction today, so only a fixture drawn with
    // focus elsewhere would notice a future `if focused`.
    let mut state = showing(sized_page(4, 3));
    state.contents.mode = SelectionMode::Cell;
    state.focus_on(Panel::Tables);

    let buffer = draw_state(40, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let cell = |x: u16| {
        buffer
            .cell((x, grid_row_y(area, 0)))
            .expect("inside the panel")
    };
    let first = drawn_column(area, 0);
    let highlight = Theme::default()
        .style(Token::Crosshair)
        .bg
        .expect("the cell mode's crosshair spends a background");

    assert!(
        cell(first).modifier.contains(Modifier::UNDERLINED),
        "the selected cell keeps its highlight without focus"
    );
    assert!(
        cell(drawn_column(area, 1)).bg == highlight
            && !cell(drawn_column(area, 1))
                .modifier
                .contains(Modifier::UNDERLINED),
        "and the rest of its row keeps the row tint alone"
    );
}

#[test]
fn the_gutter_is_muted_and_the_values_beside_it_are_not() {
    let buffer = frame_showing_grid(4, 40, 3);
    let area = block_area(&buffer, "Table contents");
    let fg = |x: u16| {
        buffer
            .cell((x, grid_row_y(area, 1)))
            .expect("inside the panel")
            .fg
    };
    let muted = Theme::default()
        .style(Token::Muted)
        .fg
        .expect("muted spends a colour");

    assert_eq!(
        fg(area.x + 2),
        muted,
        "the row number is chrome, not a value"
    );
    assert_ne!(
        fg(area.x + 4),
        muted,
        "and the value beside it reads at full weight"
    );
}

/// The same over the panel a 60-wide frame gives Contents, which is five
/// columns rather than two once each heading carries its marks — the
/// window the scrolling tests need to tell a drawn position from a page
/// index.
fn walked_wide(path: &[usize]) -> State {
    walked(42, path)
}

#[test]
fn the_grid_is_drawn_from_the_column_the_viewport_names() {
    // Out to c8, which forces the scroll, and back to c5 — which the
    // viewport already shows, so it stays where it is. The cursor then
    // sits in the middle of the drawn window rather than against its
    // right edge, where an index that ignored the offset would land on
    // the same cell and be indistinguishable.
    let state = walked_wide(&[8, 5]);
    assert!(
        state.contents.column_scroll.first() > 0,
        "the fixture must have scrolled sideways or this test proves nothing"
    );

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let heading = block_row(&buffer, "Table contents", 0);
    let first = block_row(&buffer, "Table contents", 2);
    let underlined = |x: u16| {
        buffer
            .cell((x, grid_row_y(area, 0)))
            .expect("inside the panel")
            .modifier
            .contains(Modifier::UNDERLINED)
    };

    assert!(
        heading.trim_start().starts_with("c4"),
        "the drawn window starts at the offset, got {heading:?}"
    );
    assert!(
        first.starts_with('▌'),
        "the bar is pinned and never scrolls off, got {first:?}"
    );
    // c4 is drawn first, so the cursor's c5 is the second column drawn.
    assert!(
        underlined(drawn_column(area, 1)) && underlined(drawn_column(area, 1) + 1),
        "the cell cursor is on c5, counted from the drawn window rather \
         than from the page: {heading:?}"
    );
    assert!(
        !underlined(drawn_column(area, 2)),
        "and not on the column beside it"
    );
    assert!(
        !underlined(drawn_column(area, 4)),
        "nor on the last column drawn, where an index blind to the offset \
         would land"
    );
}

/// A grid whose second column is sorted descending and whose first is
/// filtered, so every state a mark can be in is on screen at once.
fn marked_grid() -> State {
    let mut state = showing(page_of(
        &["id", "name"],
        &[
            &[CqlValue::Int(1), text("ada")],
            &[CqlValue::Int(2), text("bob")],
        ],
    ));
    state.contents.cycle_sort(1);
    state.contents.set_filter(0, Search::new("1"));
    state
}

#[test]
fn every_heading_carries_its_two_marks() {
    let buffer = draw_state(60, 12, &showing(sized_page(2, 1)));

    assert_eq!(
        block_row(&buffer, "Table contents", 0).trim_end(),
        "    c0 \u{21c5} \u{f0b0}  c1 \u{21c5} \u{f0b0}",
        "an unsorted, unfiltered grid still says where the keys apply"
    );
}

#[test]
fn a_sorted_column_wears_the_arrow_of_the_way_it_reads() {
    let mut state = showing(sized_page(2, 1));
    let arrow = |state: &State| {
        let buffer = draw_state(60, 12, state);
        block_row(&buffer, "Table contents", 0)
            .chars()
            .nth(7)
            .expect("the first heading's sort mark")
    };

    assert_eq!(arrow(&state), '\u{21c5}', "unsorted, and an affordance");

    state.contents.cycle_sort(0);
    assert_eq!(arrow(&state), '\u{2193}', "descending first");

    state.contents.cycle_sort(0);
    assert_eq!(arrow(&state), '\u{2191}');
}

#[test]
fn a_mark_in_force_reads_apart_from_the_ones_that_are_not() {
    let state = marked_grid();
    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let fg = |x: u16| buffer.cell((x, area.y + 1)).expect("inside the heading").fg;
    let marker = Theme::default()
        .style(Token::Marker)
        .fg
        .expect("a marker spends a colour");
    let faint = Theme::default()
        .style(Token::Faint)
        .fg
        .expect("faint spends a colour");

    // `id` is six columns wide — two for the name, a blank, the sort
    // mark, a blank, the funnel — so its sort mark sits third and its
    // filter mark fifth.
    let id = first_cell(area, 1);
    assert_eq!(fg(id + 3), faint, "id is not sorted");
    assert_eq!(fg(id + 5), marker, "and it is filtered");

    let name = id + 6 + COLUMN_SPACING;
    assert_eq!(fg(name + 5), marker, "name is sorted");
    assert_eq!(fg(name + 7), faint, "and not filtered");
}

#[test]
fn a_filter_that_hides_rows_draws_only_the_ones_it_left() {
    let buffer = draw_state(60, 12, &marked_grid());

    assert!(
        block_row(&buffer, "Table contents", 2).contains("ada"),
        "the row the filter kept"
    );
    assert!(
        !block_row(&buffer, "Table contents", 3).contains("bob"),
        "and not the one it dropped"
    );
}

#[test]
fn the_title_says_how_much_of_the_page_a_filter_left() {
    // `top_border` panics unless some block's title says so, which is the
    // assertion: a grid titled "1 row" would claim the cluster returned
    // one.
    // `top_border` panics unless some block's title says so, which is the
    // assertion: a grid titled "1 row" would claim the cluster returned
    // one. Widened past 60, where the title is cut before the count.
    top_border(&draw_state(80, 12, &marked_grid()), "(1 of 2 rows)");
}

/// The prompt is a modal of its own, so a search prompt on the bar row and
/// a filter cannot be mistaken for each other (grid sort and filter
/// design §3).
#[test]
fn the_filter_prompt_draws_over_the_panels_naming_its_column() {
    let mut state = marked_grid();
    state.contents.mode = SelectionMode::Column;
    state.contents.column.go_to(1, 2);
    for key in ['f', 'a'] {
        lc_domain::update(
            &mut state,
            lc_domain::update::Msg::Input(lc_domain::model::KeyPress::new(
                lc_domain::model::Key::Char(key),
            )),
        );
    }

    let buffer = draw_state(60, 12, &state);

    // The modal says which column it will narrow, and `top_border` finds
    // no block titled this unless it does.
    top_border(&buffer, "Filter name");
    // And it draws in the smallest frame the shell draws panels in, which
    // is why `f` needs no measurement to gate it (design §3).
    top_border(
        &draw_state(crate::layout::MIN_W, crate::layout::MIN_H, &state),
        "Filter",
    );
    assert!(
        block_row(&buffer, "Filter name", 0).starts_with('a'),
        "and holds the term typed so far"
    );
}

/// The trap `body.rs::grid` documents around `selected_column`: a range
/// held over the *page*'s column indices must be tested against the page
/// column a drawn cell names, not the position it happens to draw at.
#[test]
fn a_scrolled_grids_column_range_bands_the_page_column_not_the_drawn_position() {
    let mut state = walked_wide(&[8, 5]);
    assert_eq!(
        state.contents.column_scroll.first(),
        4,
        "the fixture must scroll to c4 or this test proves nothing about \
         page indices"
    );
    // Page column 6 — drawn third in this window (c4, c5, c6, …). A check
    // keyed on the drawn position rather than the page column would either
    // band the wrong cell or, since no drawn position is 6, band nothing
    // at all in a window this size.
    state.contents.columns = held(6, 6);

    let buffer = draw_state(60, 12, &state);
    let area = block_area(&buffer, "Table contents");
    // Row 1, not the cursor's row 0: the cursor's own row carries the
    // crosshair under the band, and a row that is washed either way proves
    // nothing about the range here.
    let bg = |index: u16| {
        buffer
            .cell((drawn_column(area, index), grid_row_y(area, 1)))
            .expect("inside the panel")
            .bg
    };

    assert_ne!(
        bg(2),
        bg(0),
        "page column 6 draws third in this scrolled window and must be \
         banded there, keyed on its page index rather than its position"
    );
}

/// The horizontal track: past the footer, short of the loader's cells.
fn contents_track(buffer: &Buffer) -> String {
    let area = block_area(buffer, "Table contents");
    let limit = crate::loader::placement(area).expect("room for a loader").x - crate::loader::INSET;
    let border: String = (area.x + 1..limit)
        .filter_map(|x| buffer.cell((x, area.bottom() - 1)))
        .map(Cell::symbol)
        .collect();

    border
        .rsplit_once(' ')
        .map_or(border.clone(), |(_, track)| track.to_owned())
}

#[test]
fn the_horizontal_thumb_sits_at_whichever_end_of_the_track_the_viewport_is_on() {
    let left = contents_track(&draw_state(40, 12, &showing_column_path(&[0])));
    let right = contents_track(&draw_state(40, 12, &showing_column_path(&[11])));

    assert!(
        left.starts_with(symbols::line::THICK_HORIZONTAL),
        "the first column is on screen, so the thumb belongs at the left: {left:?}"
    );
    assert!(
        right.ends_with(symbols::line::THICK_HORIZONTAL),
        "the last column is on screen, so the thumb belongs at the right: {right:?}"
    );
    assert!(
        !left.ends_with(symbols::line::THICK_HORIZONTAL)
            && !right.starts_with(symbols::line::THICK_HORIZONTAL),
        "a thumb spanning the whole track would say nothing about where the \
         viewport is: {left:?} {right:?}"
    );
}

#[test]
fn rows_are_numbered_from_one() {
    let buffer = frame_showing_grid(2, 200, 20);
    let text = block_row(&buffer, "Table contents", 2);

    assert!(
        text.trim_start_matches(['▌', ' ']).starts_with('1'),
        "got {text:?}"
    );
}

#[test]
fn the_row_number_is_the_pages_own_index_after_a_scroll() {
    // A twelve-row page scrolled to put its fourth row at the top.
    let buffer = frame_showing_scrolled_grid(12, 3);
    let text = block_row(&buffer, "Table contents", 2);

    assert!(
        text.trim_start().starts_with('4'),
        "the fourth row is still row 4, got {text:?}"
    );
}

#[test]
fn a_column_wider_than_the_panel_is_ellipsised_rather_than_squeezed() {
    // `lc_domain::grid::widths` caps a column at 30 and not at the panel,
    // so the natural width outruns the 19 columns of body left once the
    // merged bar+gutter block and its spacing column are taken. `drawn_widths`
    // caps each width against what is left, so ratatui is never
    // overcommitted and the bar keeps its own cells; the value is cut by
    // `styled_cell`, which is where the ellipsis below comes from.
    let state = showing(page_of(
        &["id"],
        &[&[text("8f14e45f-ceea-467a-9c2b-8c4d2a1e9f01")]],
    ));

    let buffer = draw_state(40, 12, &state);
    let drawn = block_row(&buffer, "Table contents", 2);

    assert!(
        drawn.starts_with("▌1 "),
        "the bar and the gutter keep their own width: {drawn:?}"
    );
    assert!(
        drawn.trim_end().ends_with('…'),
        "and the value says it continues: {drawn:?}"
    );
}

#[test]
fn a_truncated_column_widens_to_fill_the_panel() {
    let body = "Row after row of the same plain prose. ".repeat(40);
    let mut state = showing(page_of(
        &["id", "body"],
        &[
            &[CqlValue::Int(1), text(&body)],
            &[CqlValue::Int(2), text(&body)],
        ],
    ));
    state.geometry = geometry_of(80, 12, &state);
    state.contents.mode = SelectionMode::Column;
    state.contents.column.go_to(1, 2);

    let buffer = draw_state(80, 12, &state);
    let area = block_area(&buffer, "Table contents");
    let drawn = block_row(&buffer, "Table contents", 2);
    let last = area.right() - 2;
    let bg = |y: u16| buffer.cell((last, y)).expect("inside the panel").bg;
    let selected = Theme::default().style(Token::Selected).bg;

    assert!(
        drawn.ends_with('…'),
        "the body runs to the border: {drawn:?}"
    );
    assert_eq!(
        Some(bg(grid_row_y(area, 1))),
        selected,
        "column mode lights the column to its new edge"
    );
    assert_eq!(Some(bg(1)), selected, "and its heading");
}

/// Whether the block titled `title` has a horizontal scrollbar drawn over
/// its bottom border. The corners stay `╰` and `╯` whether or not one is
/// drawn, so the cells between them are what to look at: an untouched
/// bottom border is all `─`.
fn has_horizontal_scrollbar(buffer: &Buffer, title: &str) -> bool {
    let area = block_area(buffer, title);
    let y = area.bottom() - 1;

    (area.x + 1..area.right() - 1).any(|x| {
        buffer
            .cell((x, y))
            .is_some_and(|cell| cell.symbol() == symbols::line::THICK_HORIZONTAL)
    })
}
