use super::*;

fn with_query(text: &str, focus: Panel) -> State {
    let mut state = State::default();
    state.focus_on(focus);
    state.queries.start_editing();
    for character in text.chars() {
        let key = if character == '\n' {
            Key::Enter
        } else {
            Key::Char(character)
        };
        state.queries.edit(key, 1);
    }
    if focus != Panel::Queries {
        state.queries.stop_editing();
    }
    state
}

/// At 80x24 focused Queries is `Rect { x: 20, y: 10, w: 60, h: 13 }`: content
/// rows 11 to 21, inner left edge 21.
const CONTENT_ROW: u16 = 11;
const LAST_CONTENT_ROW: u16 = 21;
const INNER_LEFT: u16 = 21;

#[test]
fn the_editor_draws_what_has_been_typed() {
    let (buffer, _) = drawn_with_cursor(80, 24, &with_query("SELECT * FROM t", Panel::Queries));

    assert!(
        row(&buffer, CONTENT_ROW).contains("SELECT * FROM t"),
        "the buffer is on the panel's first content row: {:?}",
        row(&buffer, CONTENT_ROW)
    );
}

#[test]
fn the_cursor_sits_where_the_domain_says_it_is() {
    let state = with_query("SELECT", Panel::Queries);
    let expected = state.queries.cursor();

    let (_, cursor) = drawn_with_cursor(80, 24, &state);

    let cursor = cursor.expect("a focused editor shows a cursor");
    assert_eq!(
        cursor.x,
        INNER_LEFT + u16::try_from(expected.column).expect("a test column fits"),
        "one column past the six typed"
    );
    assert_eq!(cursor.y, CONTENT_ROW);
}

#[test]
fn the_banded_statement_is_the_one_the_cursor_is_in() {
    let mut state = with_query("SELECT 1;\nSELECT 2", Panel::Queries);
    state.queries.edge_statement(false);

    let (buffer, _) = drawn_with_cursor(80, 24, &state);

    let ground = Theme::default().style(Token::Selected).bg;
    assert_eq!(
        buffer[(INNER_LEFT, CONTENT_ROW)].bg,
        ground.expect("the band is a ground colour"),
        "the statement the cursor is in wears the cursor row's ground"
    );
    assert_ne!(
        buffer[(INNER_LEFT, CONTENT_ROW + 1)].bg,
        ground.expect("the band is a ground colour"),
        "the statement below it does not"
    );
}

#[test]
fn a_band_stops_where_its_statement_does() {
    let mut state = with_query("SELECT 1; SELECT 2", Panel::Queries);
    state.queries.edge_statement(false);

    let (buffer, _) = drawn_with_cursor(80, 24, &state);

    let ground = Theme::default()
        .style(Token::Selected)
        .bg
        .expect("the band is a ground colour");
    assert_eq!(buffer[(INNER_LEFT + 8, CONTENT_ROW)].bg, ground, "the `;`");
    assert_ne!(
        buffer[(INNER_LEFT + 10, CONTENT_ROW)].bg,
        ground,
        "the statement after it"
    );
}

#[test]
fn an_empty_buffer_draws_no_band() {
    let (buffer, _) = drawn_with_cursor(80, 24, &with_query("", Panel::Queries));

    assert_ne!(
        buffer[(INNER_LEFT, CONTENT_ROW)].bg,
        Theme::default()
            .style(Token::Selected)
            .bg
            .expect("the band is a ground colour"),
    );
}

fn completing(text: &str) -> State {
    let mut state = with_query(text, Panel::Queries);
    let completion = lc_domain::cql::suggest(state.queries.buffer(), None, None);
    state.queries.offer(completion);
    state
}

#[test]
fn the_buffer_is_painted_by_what_its_words_mean() {
    let (buffer, _) = drawn_with_cursor(80, 24, &with_query("SELECT id", Panel::Queries));

    let keyword = buffer[(INNER_LEFT, CONTENT_ROW)].fg;
    let name = buffer[(INNER_LEFT + 7, CONTENT_ROW)].fg;

    assert_eq!(
        row(&buffer, CONTENT_ROW).trim_end().chars().nth(21),
        Some('S'),
        "the assertion below is reading the two cells it means to"
    );
    assert_ne!(
        keyword, name,
        "a keyword and a column name are two meanings and two colours"
    );
}

#[test]
fn the_completion_popup_hangs_off_the_cursor() {
    let (buffer, _) = drawn_with_cursor(80, 24, &completing("se"));

    assert!(
        offered_row(&buffer) > usize::from(CONTENT_ROW),
        "the cursor is at the top of an open panel, so the popup opens downwards"
    );
}

#[test]
fn the_completion_popup_draws_a_statement_shape_with_its_holes() {
    let (buffer, _) = drawn_with_cursor(80, 24, &completing("sel"));

    let drawn: String = (0..buffer.area.height).map(|y| row(&buffer, y)).collect();

    assert!(
        drawn.contains("SELECT <> FROM <>"),
        "the popup draws the shape rather than the skeleton it types"
    );
}

#[test]
fn the_completion_popup_opens_upwards_off_the_bottom_of_the_frame() {
    // Eleven lines put the cursor on the last content row.
    let state = completing(&format!("{}se", "\n".repeat(11)));

    let (buffer, cursor) = drawn_with_cursor(80, 24, &state);

    assert_eq!(
        cursor.expect("a focused editor shows a cursor").y,
        LAST_CONTENT_ROW
    );
    assert!(
        offered_row(&buffer) < usize::from(LAST_CONTENT_ROW),
        "there is no room beneath the cursor, so the popup opens upwards"
    );
}

fn offered_row(buffer: &Buffer) -> usize {
    (0..usize::from(buffer.area.height))
        .find(|&y| {
            row(
                buffer,
                u16::try_from(y).expect("a drawn row's index fits u16"),
            )
            .contains("SELECT")
        })
        .expect("the popup names what it is offering")
}

#[test]
fn an_unfocused_editor_draws_no_popup() {
    let mut state = completing("se");
    state.focus_on(Panel::Tables);

    let (buffer, _) = drawn_with_cursor(80, 24, &state);

    assert!(
        (0..24).all(|y| !row(&buffer, y).contains("SELECT")),
        "a popup over a panel that is not taking keys would be offering into nothing"
    );
}

#[test]
fn an_unfocused_editor_sets_no_cursor() {
    let (_, cursor) = drawn_with_cursor(80, 24, &with_query("SELECT", Panel::Tables));

    assert_eq!(
        cursor, None,
        "a cursor in an unfocused panel would say it takes the keys"
    );
}

#[test]
fn a_long_line_scrolls_sideways_and_keeps_the_cursor_on_screen() {
    let long = "SELECT * FROM keyspace_with_a_long_name.table_with_a_long_name WHERE id = 7";
    let state = with_query(long, Panel::Queries);

    let (buffer, cursor) = drawn_with_cursor(80, 24, &state);
    let drawn = row(&buffer, CONTENT_ROW);

    assert!(
        drawn.contains("WHERE id = 7"),
        "the cursor's end of the line is what is shown: {drawn:?}"
    );
    assert!(
        !drawn.contains("SELECT *"),
        "the start has scrolled off to the left: {drawn:?}"
    );
    let cursor = cursor.expect("a focused editor shows a cursor");
    assert!(
        cursor.x < 80,
        "the cursor stays inside the frame at {}",
        cursor.x
    );
}

#[test]
fn a_tall_buffer_scrolls_and_says_so() {
    // Fifteen lines into eleven rows.
    let lines: Vec<String> = (0..15).map(|line| format!("line{line}")).collect();
    let state = with_query(&lines.join("\n"), Panel::Queries);

    let (buffer, cursor) = drawn_with_cursor(80, 24, &state);
    let drawn = row(&buffer, LAST_CONTENT_ROW);

    assert!(
        drawn.contains("line14"),
        "the cursor's line is the last one drawn: {drawn:?}"
    );
    assert_eq!(
        cursor.expect("a focused editor shows a cursor").y,
        LAST_CONTENT_ROW,
        "the cursor's own row is on screen"
    );
    assert!(
        (CONTENT_ROW..=LAST_CONTENT_ROW).any(|y| row(&buffer, y).ends_with(THUMB)),
        "a buffer past the panel's rows grows a scrollbar"
    );
}

#[test]
fn a_double_width_character_does_not_desync_the_cursor_from_the_scroll() {
    // Thirty double-width characters put the scroll offset mid-character.
    let long = format!("{}SELECT", "永".repeat(30));
    let state = with_query(&long, Panel::Queries);

    let (buffer, cursor) = drawn_with_cursor(80, 24, &state);
    let drawn = row(&buffer, CONTENT_ROW);

    assert!(
        drawn.contains("SELECT"),
        "the cursor's end of the line is what is shown: {drawn:?}"
    );
    assert_eq!(
        buffer.cell((INNER_LEFT, CONTENT_ROW)).map(Cell::symbol),
        Some("永"),
        "the drawn row begins on a whole character the offset names, not a half-dropped one: {drawn:?}"
    );

    let cursor = cursor.expect("a focused editor shows a cursor");
    assert_eq!(cursor.y, CONTENT_ROW);
    assert_eq!(
        buffer.cell((cursor.x - 1, CONTENT_ROW)).map(Cell::symbol),
        Some("T"),
        "the cursor sits one cell past the last character typed, not inside or short of it"
    );
}

#[test]
fn a_closed_buffer_draws_no_cursor_though_its_panel_is_focused() {
    let mut state = with_query("SELECT", Panel::Queries);
    state.queries.stop_editing();

    let (_, cursor) = drawn_with_cursor(80, 24, &state);

    assert_eq!(cursor, None, "the caret is what says the buffer takes text");
}

#[test]
fn an_overlay_hides_the_cursor_even_when_the_editor_is_focused() {
    let mut state = with_query("SELECT", Panel::Queries);
    state.overlay = Some(Overlay::Message(Message {
        title: "Query failed".to_owned(),
        message: "boom".to_owned(),
        severity: Severity::Danger,
    }));

    let (_, cursor) = drawn_with_cursor(80, 24, &state);

    assert_eq!(
        cursor, None,
        "an overlay owns the frame; a cursor beneath it would blink over the modal at a position that means nothing there"
    );
}

#[test]
fn an_open_prompts_own_cursor_replaces_the_editors() {
    // The prompt has its own cursor on the bar row; the editor's stays hidden.
    let mut state = with_query("SELECT", Panel::Queries);
    state.overlay = Some(Overlay::Search(Prompt::opened(
        Searched::Tables,
        None,
        lc_domain::search::Origin::default(),
    )));

    let (_, cursor) = drawn_with_cursor(80, 24, &state);

    // Row 23 is the bar; column 2 is after `" /"`.
    assert_eq!(
        cursor,
        Some(ratatui::layout::Position::new(2, 23)),
        "a prompt outranks focus exactly as a message or the palette does; \
         the cursor shown is the prompt's own, not the editor's"
    );
}
