use super::*;

#[test]
fn a_flavour_paints_the_field_it_is_drawn_on() {
    let mut terminal =
        Terminal::new(TestBackend::new(80, 24)).expect("TestBackend always initialises");
    let state = State::default();

    terminal
        .draw(|frame| {
            render(
                frame,
                &view_model(&state),
                Theme::of(&ThemeChoice::Ported(Ported::CatppuccinMocha)),
            );
        })
        .expect("TestBackend always draws");

    let buffer = terminal.backend().buffer();
    let ground = buffer[(0, 0)].bg;

    assert_eq!(
        ground,
        Color::Rgb(0x1e, 0x1e, 0x2e),
        "a flavour owns the whole frame, corners included"
    );
}

#[test]
fn focus_wins_over_a_flavours_own_panel_border() {
    let theme = Theme::of(&ThemeChoice::Ported(Ported::CatppuccinMocha));
    let mut terminal =
        Terminal::new(TestBackend::new(40, 12)).expect("TestBackend always initialises");
    let mut state = State::default();
    state.focus_on(Panel::Tables);

    terminal
        .draw(|frame| {
            render(frame, &view_model(&state), theme);
        })
        .expect("TestBackend always draws");

    let buffer = terminal.backend().buffer();
    let corner_fg = buffer[(0, 0)].fg;

    assert_eq!(
        corner_fg,
        theme.palette().focus,
        "focus's colour patches on top of panel_border's, exactly as \
         border_style()'s doc comment claims"
    );
}

#[test]
fn a_flavour_paints_the_field_even_when_the_frame_is_too_small_for_a_panel() {
    // Below layout::MIN_W x MIN_H (40x7), `render` takes the `TooSmall`
    // arm and returns before the panel loop — the path the ground paint
    // must cover too, or a flavour leaves a hole exactly where the notice
    // is drawn.
    let mut terminal =
        Terminal::new(TestBackend::new(30, 6)).expect("TestBackend always initialises");
    let state = State::default();

    terminal
        .draw(|frame| {
            render(
                frame,
                &view_model(&state),
                Theme::of(&ThemeChoice::Ported(Ported::CatppuccinMocha)),
            );
        })
        .expect("TestBackend always draws");

    let buffer = terminal.backend().buffer();

    assert!(
        buffer.content().iter().all(|cell| cell.bg != Color::Reset),
        "a cell the flavour missed shows the terminal through it, notice included"
    );
}

#[test]
fn the_default_theme_paints_the_field_too() {
    let mut terminal =
        Terminal::new(TestBackend::new(80, 24)).expect("TestBackend always initialises");
    let state = State::default();

    terminal
        .draw(|frame| {
            render(frame, &view_model(&state), Theme::default());
        })
        .expect("TestBackend always draws");

    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .all(|cell| cell.bg != Color::Reset),
        "the default theme declares its own base, corners included \
         (docs/superpowers/specs/2026-08-13-colour-palettes-design.md §1)"
    );
}

#[test]
fn the_four_panels_are_drawn_where_the_draft_puts_them() {
    // At 40x12 the sidebar sits on its 16-column floor, Tables takes 5
    // rows to Ddl's 6, and Queries — unfocused and empty — is closed to
    // its two borders.
    let buffer = draw(40, 12);

    assert_eq!(row(&buffer, 0), "╭── [1]-Tables ╮╭── [3]-Table contents ╮");
    assert_eq!(row(&buffer, 1), "│Not connected ││Press Enter on a table│");
    assert_eq!(row(&buffer, 4), "╰──────────────╯│                      │");
    // Nothing is selected, so the panel offers no Columns tab and the
    // strip is the one name.
    assert_eq!(row(&buffer, 5), "╭── [2]-DDL ───╮│                      │");
    assert_eq!(row(&buffer, 8), "│              │╰──────────────────────╯");
    // `[5]-Logs` does not fit beside it, so the strip keeps the active tab.
    assert_eq!(row(&buffer, 9), "│              │╭── [4]-Queries ───────╮");
    assert_eq!(row(&buffer, 10), "╰──────────────╯╰──────────────────────╯");
}

#[test]
fn the_hint_bar_sits_on_the_bottom_row() {
    let buffer = draw(80, 24);

    assert_eq!(
        row(&buffer, 23).trim_end(),
        " ⏎ open | / search | H health | v select | tab panel | q quit",
        "the whole row is the bar's: the loader draws on panel borders now"
    );
    assert!(
        !row(&buffer, 22).contains("q quit"),
        "the bar owns one row and no more"
    );
}

/// The bottom border of `panel`'s rect at 80x24, where its loader is
/// drawn.
fn bottom_border(buffer: &Buffer, panel: Panel) -> String {
    let area = match layout(Rect::new(0, 0, 80, 24), QueriesHeight::Closed) {
        Shell::Panels(rects) => rects.get(panel),
        Shell::TooSmall(_) => unreachable!("80x24 holds the panels"),
    };

    row(buffer, area.bottom() - 1)
        .chars()
        .skip(usize::from(area.x))
        .take(usize::from(area.width))
        .collect()
}

#[test]
fn the_loader_rests_until_something_is_outstanding() {
    // `State::default` is disconnected with nothing in flight.
    let resting = draw(80, 24);
    let mut state = State::default();
    state.connection = ConnectionState::Connecting {
        conn: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
    };
    state.loader.advance(true);

    let working = draw_state(80, 24, &state);

    assert!(
        bottom_border(&resting, Panel::Tables)
            .chars()
            .all(|glyph| "╰─╯".contains(glyph)),
        "an idle panel's border is border and nothing else"
    );
    assert_ne!(
        bottom_border(&resting, Panel::Tables),
        bottom_border(&working, Panel::Tables),
        "a connection attempt is the tree filling, and shows on its border"
    );
    assert_eq!(
        row(&resting, 23),
        row(&working, 23),
        "and it is off the hint bar entirely"
    );
}

#[test]
fn the_loader_marks_the_panel_the_wait_is_for() {
    let mut state = State::default();
    state.connection = ConnectionState::Connecting {
        conn: ConnId::next(),
        name: lc_domain::config::ConnectionName::try_new("local").expect("a valid name"),
    };
    state.loader.advance(true);

    let buffer = draw_state(80, 24, &state);

    for panel in Panel::ALL {
        let border = bottom_border(&buffer, panel);
        let quiet = border.chars().all(|glyph| "╰─╯".contains(glyph));

        assert_eq!(
            quiet,
            panel != Panel::Tables,
            "{panel:?} drew {border:?} while the schema was on its way"
        );
    }
}

#[test]
fn a_query_in_flight_shows_on_the_grid_it_will_fill() {
    let mut state = State::default();
    state
        .in_flight
        .insert(QueryId::next(), Statement::typed("select * from k.t"));
    state.loader.advance(true);

    let buffer = draw_state(80, 24, &state);

    assert!(
        !bottom_border(&buffer, Panel::Contents)
            .chars()
            .all(|glyph| "╰─╯".contains(glyph)),
        "the grid is where a statement lands, whichever panel asked for it"
    );
}

#[test]
fn a_frame_below_the_minimum_draws_the_notice_and_nothing_else() {
    for (width, height) in [(39, 6), (40, 6), (40, 5), (39, 5)] {
        let buffer = draw(width, height);
        let text: String = (0..height).map(|y| row(&buffer, y)).collect();

        assert!(
            text.contains("need 40×7"),
            "{width}x{height} must say what it needs, got {text:?}"
        );
        assert!(
            !text.contains('╭'),
            "{width}x{height} must draw no panel, got {text:?}"
        );
    }
}

#[test]
fn the_notice_names_the_frame_it_was_given() {
    let buffer = draw(24, 5);
    let text: String = (0..5).map(|y| row(&buffer, y)).collect();

    assert!(text.contains("have 24×5"), "got {text:?}");
}

#[test]
fn the_notice_drops_its_least_useful_line_first() {
    let lines = |height| {
        let buffer = draw(30, height);
        (0..height)
            .map(|y| row(&buffer, y).trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
    };

    assert_eq!(lines(3), ["Too small", "need 40×7", "have 30×3"]);
    assert_eq!(lines(2), ["need 40×7", "have 30×2"]);
    assert_eq!(lines(1), ["need 40×7"]);
}

#[test]
fn nothing_is_drawn_over_the_notice() {
    let mut state = State::default();
    state.overlay = Some(Overlay::Message(Message {
        title: "Cannot connect".to_owned(),
        message: "no route to host".to_owned(),
        severity: Severity::Danger,
    }));
    let mut terminal =
        Terminal::new(TestBackend::new(39, 6)).expect("TestBackend always initialises");

    terminal
        .draw(|frame| {
            render(frame, &view_model(&state), Theme::default());
        })
        .expect("TestBackend always draws");

    let buffer = terminal.backend().buffer().clone();
    let text: String = (0..6).map(|y| row(&buffer, y)).collect();

    assert!(
        !text.contains("Cannot connect"),
        "an overlay has nothing to sit over, got {text:?}"
    );
}

/// `OverlayView::Fatal` and `OverlayView::Message` share the same draw
/// arm (query log design §7): the two differ in what dismisses them, not
/// in how they look.
#[test]
fn a_fatal_failure_is_drawn_like_any_other_message() {
    let mut state = State::default();
    state.overlay = Some(Overlay::Fatal(Message {
        title: "Connection failed".to_owned(),
        message: "could not reach the cluster: no route to host".to_owned(),
        severity: Severity::Danger,
    }));

    let buffer = draw_state(60, 20, &state);
    let text: String = (0..20).map(|y| row(&buffer, y)).collect();

    assert!(text.contains("Connection failed"), "{text}");
    assert!(text.contains("no route to host"), "{text}");
}

#[test]
fn a_zero_row_frame_draws_nothing_and_does_not_panic() {
    let buffer = draw(30, 0);

    assert_eq!(buffer.area.height, 0);
}

#[test]
fn a_frame_below_the_minimum_measures_nothing() {
    let state = State::default();
    let mut terminal =
        Terminal::new(TestBackend::new(39, 6)).expect("TestBackend always initialises");
    let mut geometry = Geometry::default();

    terminal
        .draw(|frame| {
            geometry = render(frame, &view_model(&state), Theme::default()).geometry;
        })
        .expect("TestBackend always draws");

    assert_eq!(geometry, Geometry::default());
}
