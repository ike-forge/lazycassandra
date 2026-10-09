use super::*;

#[test]
fn a_panel_wears_the_search_applied_to_it() {
    let mut state = browsing(&["shop"], 0);
    state.apply_search(Searched::Tables, Search::new("ord"));

    let buffer = draw_state(80, 24, &state);

    assert!(
        row(&buffer, top_border(&buffer, "Tables").1).contains("/ord"),
        "the term sits on the panel's own top border"
    );
}

#[test]
fn the_marker_is_drawn_muted() {
    // On an unfocused panel: `Block` patches `border_style` under every
    // title span, focused Tables included (its own shortcut span already
    // reads BOLD from that, ambiently). Filtering Ddl instead, with the
    // default focus on Tables, isolates the marker's own style from that
    // ambient one.
    let mut state = browsing(&["shop"], 0);
    state.apply_search(Searched::Ddl, Search::new("ord"));

    let buffer = draw_state(80, 24, &state);
    let (fg, _bg, modifier) = style_of_first(&buffer, "/ord");
    let expected = Theme::default().style(Token::Muted);

    assert_eq!(
        fg,
        expected.fg.expect("muted spends a colour"),
        "no colour override"
    );
    assert_eq!(expected.bg, None, "muted paints no background of its own");
    assert_eq!(modifier, expected.add_modifier, "no modifier override");
}

#[test]
fn the_marker_is_dropped_rather_than_truncated() {
    let mut state = browsing(&["shop"], 0);
    state.apply_search(
        Searched::Tables,
        Search::new("a_very_long_search_term_indeed"),
    );

    let buffer = draw_state(40, 24, &state);
    let border = row(&buffer, top_border(&buffer, "Tables").1);

    assert!(
        !border.contains('/'),
        "a truncated term is a lie about what is applied, got {border:?}"
    );
    assert!(border.contains("Tables"), "and the title survives");
}

#[test]
fn the_marker_fits_flush_against_the_corner_it_is_tuned_for() {
    // At 80 columns Tables gets 20: 1 (`╭`) + 10 (`[1]-Tables`) + 4
    // (`/ord`) + 1 (`╮`) leaves exactly 4 columns, which is what
    // `TITLE_PAD` spends either side of each — so the two sit flush with a
    // gap and no dash between them, nothing is left over for an inset, and
    // one column narrower drops the marker.
    let mut state = browsing(&["shop"], 0);
    state.apply_search(Searched::Tables, Search::new("ord"));

    let buffer = draw_state(80, 24, &state);
    let border = row(&buffer, top_border(&buffer, "Tables").1);

    assert_eq!(
        border, "╭ [1]-Tables  /ord ╮╭── [3]-Table contents ────────────────────────────────────╮",
        "the widest term this width can hold, sitting flush with no gap and no overlap"
    );
}

#[test]
fn an_unfocused_panel_still_wears_its_marker() {
    // The marker's whole point: a search that survives a focus change
    // has to say so from the panel it is applied to, not from the bar,
    // which only ever describes the focused panel (design §5.1).
    let mut state = browsing(&["shop"], 0);
    state.focus_on(Panel::Contents);
    state.apply_search(Searched::Tables, Search::new("ord"));

    let buffer = draw_state(80, 24, &state);

    assert!(
        row(&buffer, top_border(&buffer, "Tables").1).contains("/ord"),
        "Tables is unfocused but its own border still carries the marker"
    );
}

#[test]
fn an_open_prompt_owns_the_bar_row() {
    let mut prompt = Prompt::opened(Searched::Tables, None, lc_domain::search::Origin::default());
    prompt.push('o');
    prompt.push('r');
    let mut state = State::default();
    state.overlay = Some(Overlay::Search(prompt));

    let buffer = draw_state(80, 24, &state);
    let bar = row(&buffer, 23);

    assert!(bar.contains("/or"), "the query is on the bar: {bar:?}");
    assert!(
        bar.contains("cancel"),
        "and so are the prompt's own keys: {bar:?}"
    );
    assert!(
        !bar.contains("j/k move"),
        "the panel's own hints are gone while the prompt is up: {bar:?}"
    );
}

#[test]
fn the_prompts_cursor_sits_after_the_query() {
    let mut prompt = Prompt::opened(Searched::Tables, None, lc_domain::search::Origin::default());
    prompt.push('o');
    prompt.push('r');
    let mut state = State::default();
    state.overlay = Some(Overlay::Search(prompt));

    let (_, cursor) = drawn_with_cursor(80, 24, &state);

    // The bar row is 23 at this size, and the bar owns it from column 0:
    // the query's leading space, its `/` and its two characters put the
    // cursor at column 4.
    assert_eq!(
        cursor,
        Some(ratatui::layout::Position::new(4, 23)),
        "one cell past the last character typed"
    );
}

#[test]
fn the_prompts_cursor_stays_inside_the_frame_when_the_query_fills_the_bar() {
    let mut prompt = Prompt::opened(Searched::Tables, None, lc_domain::search::Origin::default());
    for c in "a query long enough to overrun a narrow terminal's hint bar".chars() {
        prompt.push(c);
    }
    let mut state = State::default();
    state.overlay = Some(Overlay::Search(prompt));

    let (_, cursor) = drawn_with_cursor(40, 24, &state);
    let cursor = cursor.expect("the prompt draws a cursor");

    assert!(
        cursor.x < 40,
        "a query wide enough to fill the bar must not push the cursor \
         past the frame's right edge: {cursor:?}"
    );
}

#[test]
fn the_prompts_query_carries_a_theme_token() {
    let mut prompt = Prompt::opened(Searched::Tables, None, lc_domain::search::Origin::default());
    prompt.push('o');
    let mut state = State::default();
    state.overlay = Some(Overlay::Search(prompt));

    let buffer = draw_state(80, 24, &state);
    // Column 9: one past the loader and `bar_split`'s gap, landing on
    // the query's leading space.
    let cell = buffer.cell((9, 23)).expect("inside the bar row");
    let muted = Theme::default()
        .style(Token::HintAction)
        .fg
        .expect("the hint bar's action spends a colour");

    assert_eq!(
        cell.fg,
        muted,
        "the query must carry the same token the rest of the bar's text \
         does ({:?} was left bare)",
        cell.symbol()
    );
}
