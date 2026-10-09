use super::*;

#[test]
fn the_bar_describes_the_open_palette_rather_than_the_panel_under_it() {
    // The sample design §7 shows, checked against what the renderer
    // actually draws rather than being correct by construction.
    //
    let buffer = with_palette(80, 24);

    assert_eq!(
        row(&buffer, 23).trim_end(),
        " ⏎ run | ?/esc close | q quit | / search"
    );
}

#[test]
fn the_bar_says_what_the_focused_panel_does() {
    let bars: Vec<String> = Panel::ALL
        .iter()
        .map(|&panel| row(&draw_focused(80, 24, panel), 23).trim_end().to_owned())
        .collect();

    assert!(bars[0].contains("⏎ open"), "Tables opens: {:?}", bars[0]);
    assert!(bars[1].contains("/ search"), "Ddl searches: {:?}", bars[1]);
    assert!(
        bars[2].contains("/ search"),
        "Contents searches: {:?}",
        bars[2]
    );
    assert!(
        bars[3].contains("⏎ edit"),
        "Queries opens its buffer: {:?}",
        bars[3]
    );
}

/// Overlay search design §7: an overlay's prompt draws where a panel's
/// prompt draws — on the hint bar row.
#[test]
fn an_overlay_sift_draws_its_prompt_on_the_hint_bar() {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("a backend");
    let mut state = State::default();
    let mut panel = CommandsPanel::opened();
    panel.open_sift();
    panel.push('q');
    state.overlay = Some(Overlay::Commands(panel));

    terminal
        .draw(|frame| {
            render(frame, &view_model(&state), Theme::default());
        })
        .expect("a frame");

    let bar: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .skip(80 * 23)
        .map(Cell::symbol)
        .collect();

    assert!(bar.contains("/q"), "the prompt is on the bar: {bar}");
}

#[test]
fn a_narrow_frame_drops_hints_rather_than_the_bar() {
    let buffer = draw(40, 12);

    assert_eq!(
        row(&buffer, 11).trim_end(),
        " ⏎ open | / search | tab panel | q quit",
        "the row the loader gave up goes back to the hints"
    );
}
