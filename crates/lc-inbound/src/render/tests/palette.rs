use super::*;

/// Every cell of `buffer`, with where it sits.
fn cells(buffer: &Buffer) -> impl Iterator<Item = (Position, &Cell)> {
    buffer.content().iter().enumerate().map(|(index, cell)| {
        let index = u16::try_from(index).unwrap_or(u16::MAX);
        (
            Position::new(
                buffer.area.x + index % buffer.area.width,
                buffer.area.y + index / buffer.area.width,
            ),
            cell,
        )
    })
}

/// `text` above already names the `CqlValue` constructor the grid tests
/// use; this is the same buffer-flattening helper `overlay::tests` keeps
/// under that name, renamed to fit beside it.
fn buffer_text(buffer: &Buffer) -> String {
    buffer.content().iter().map(Cell::symbol).collect()
}

/// A frame height at which the whole catalogue fits inside the palette,
/// derived rather than pinned: the block takes `HEIGHT_PERCENT` of the
/// frame, so a catalogue that grows by one command would otherwise rot
/// every test that hard-codes a "tall enough" number.
fn tall_enough_for_the_catalogue() -> u16 {
    let needed = u16::try_from(lc_domain::commands::row_count(None) + 2).expect("small");

    needed.div_ceil(7).saturating_mul(10)
}

/// The palette's first heading, its keys column, and the rule under the
/// first group, read as styled cells rather than as text.
#[test]
fn the_palette_marks_a_group_the_way_a_section_heading_is_marked() {
    let buffer = with_palette(100, 40);
    let (x, y) = top_border(&buffer, "Commands");
    let theme = Theme::default();
    // One column for the border, one for the palette's own padding.
    let first = x + 2;
    let cell = |at: u16| buffer.cell((first, at)).expect("inside the block");

    assert_eq!(
        cell(y + 1).fg,
        theme
            .style(Token::Section)
            .fg
            .expect("a section heading spends a colour"),
        "a heading wears what Options wears"
    );
    let area = block_area(&buffer, "Commands");
    assert!(
        (area.x + 1..area.right() - 1).all(|x| !buffer
            .cell((x, y + 2))
            .expect("inside the block")
            .modifier
            .contains(Modifier::UNDERLINED)),
        "a key in a column carries no underline: the colour is the whole mark"
    );
}

#[test]
fn a_rule_separates_one_group_from_the_next() {
    let buffer = with_palette(100, 40);
    let rows: Vec<String> = (0..26)
        .map(|line| block_row(&buffer, "Commands", line))
        .collect();
    let rule = rows
        .iter()
        .position(|row| row.trim().chars().all(|glyph| glyph == '─'))
        .expect("the catalogue has more than one group");

    assert_eq!(
        rows[rule + 1].trim(),
        "Navigate",
        "the rule sits above the heading it separates from the group before"
    );
    assert_eq!(
        rows[0].trim(),
        "Global",
        "and the first group is not preceded by one"
    );
}

/// A rule joins the block it divides: it reaches both borders and turns
/// them into junctions, rather than stopping at the pad columns and
/// reading as a row of content.
#[test]
fn a_rule_meets_the_block_it_divides() {
    // Tall enough that the whole catalogue fits, so no scrollbar owns the
    // right border where the rule lands.
    let buffer = with_palette(100, tall_enough_for_the_catalogue());
    let area = block_area(&buffer, "Commands");
    let symbol = |x: u16, y: u16| {
        buffer
            .cell((x, y))
            .expect("inside the block")
            .symbol()
            .to_owned()
    };
    let rule = (area.y + 1..area.bottom() - 1)
        .find(|y| symbol(area.x, *y) == symbols::line::VERTICAL_RIGHT)
        .expect("the catalogue has more than one group");

    assert_eq!(symbol(area.right() - 1, rule), symbols::line::VERTICAL_LEFT);
    assert_eq!(symbol(area.x + 1, rule), symbols::line::HORIZONTAL);
    assert_eq!(symbol(area.right() - 2, rule), symbols::line::HORIZONTAL);
}

/// The action leads, the keys follow — the shape a search-actions dialogue
/// has, and the reason `MIN_W`'s two columns swapped places.
#[test]
fn a_command_row_names_what_it_does_before_the_key_it_costs() {
    let buffer = with_palette(100, 40);
    let row = block_row(&buffer, "Commands", 20);

    assert_eq!(row.trim(), "yank                  y");
}

#[test]
fn the_palette_wears_the_focus_the_panels_gave_up() {
    let lit = draw(100, 40);
    let covered = with_palette(100, 40);
    let focus = Theme::default().palette().focus;
    let modal = block_area(&covered, "Commands");
    let outside = |buffer: &Buffer, block: Option<Rect>| {
        cells(buffer)
            .filter(|(at, cell)| {
                cell.fg == focus && !block.is_some_and(|block| block.contains(*at))
            })
            .count()
    };

    assert!(outside(&lit, None) > 0, "a focused panel is drawn in focus");
    assert_eq!(
        outside(&covered, Some(modal)),
        0,
        "and nothing beneath a modal claims the focus the modal has"
    );
    assert!(
        cells(&covered).any(|(at, cell)| modal.contains(at) && cell.fg == focus),
        "which the modal itself is now wearing"
    );
}

/// Not focused, and not fainter than the panels that were never focused
/// either: the title reads as any unfocused title does.
#[test]
fn the_panel_the_palette_covers_reads_as_an_unfocused_one() {
    let buffer = with_palette(100, 40);
    let theme = Theme::default();
    let (x, y) = top_border(&buffer, "Tables");
    // Past the corner, the two border dashes and the pad, onto the title.
    let title = buffer.cell((x + 4, y)).expect("the title is drawn");

    assert_eq!(
        title.fg,
        theme
            .style(Token::Muted)
            .fg
            .expect("an unfocused title spends a colour"),
        "the panel focus left reads like the ones it never had"
    );
    assert_ne!(
        title.fg,
        theme.palette().border,
        "and not like its own border"
    );
}

#[test]
fn the_row_under_the_palette_is_not_highlighted() {
    let buffer = with_palette(100, 40);
    let selection = Theme::default()
        .style(Token::Selected)
        .bg
        .expect("the cursor row spends a background");
    let inside = block_area(&buffer, "Commands");

    for (index, cell) in buffer.content().iter().enumerate() {
        let x = u16::try_from(index).unwrap_or(u16::MAX) % buffer.area.width;
        let y = u16::try_from(index).unwrap_or(u16::MAX) / buffer.area.width;

        assert!(
            cell.bg != selection || inside.contains((x, y).into()),
            "the tree's cursor row is still drawn at {x},{y}"
        );
    }
}

#[test]
fn the_bar_goes_quiet_under_the_palette() {
    let theme = Theme::default();
    let lit = draw(100, 40);
    let covered = with_palette(100, 40);
    let bar = |buffer: &Buffer| {
        let y = buffer.area.height - 1;
        (0..buffer.area.width)
            .filter_map(|x| buffer.cell((x, y)))
            .filter(|cell| cell.symbol() != " ")
            .map(|cell| cell.fg)
            .collect::<Vec<_>>()
    };

    let key = theme.style(Token::HintKey).fg.expect("a key is coloured");
    let faint = theme.style(Token::Faint).fg.expect("faint is a colour");

    assert!(
        bar(&lit).contains(&key),
        "the bar names live keys when nothing covers it"
    );
    assert!(
        !bar(&covered).contains(&key),
        "and none of them stays lit while the palette is up"
    );
    assert!(
        bar(&covered).contains(&faint),
        "they recede rather than disappear"
    );
}

#[test]
fn the_palette_lists_the_catalogue() {
    let drawn = buffer_text(&with_palette(100, 40));

    assert!(drawn.contains("Commands"), "the block is titled");
    // Strings only the palette can produce: the hint bar already draws
    // "q quit" along the bottom row, so asserting on "quit" would pass
    // with no palette at all.
    assert!(drawn.contains("Global"));
    assert!(drawn.contains("back out one step"));
    assert!(drawn.contains("focus Table contents"));
    assert!(drawn.contains("down one row"));
}

#[test]
fn the_selected_command_is_highlighted() {
    let buffer = with_palette(100, 40);
    let (x, y) = top_border(&buffer, "Commands");

    // Row 0 of the list is the `Global` heading, so the opened cursor sits
    // on row 1: one down for the border, one for the heading.
    let selected = y + 2;
    let highlight = Theme::default()
        .style(Token::Selected)
        .bg
        .expect("the cursor row spends a background");
    let styled = |at: u16| buffer.cell((x + 2, at)).expect("inside the block").bg == highlight;

    assert!(styled(selected), "the cursor's row is marked");
    assert!(!styled(selected + 1), "and only the cursor's row is");
}

/// Whether the block titled `title` has a scrollbar drawn over its right
/// border. The track scan stops above the bottom border row: that row's
/// corner is `┘`, which is `!= "│"` whether or not a scrollbar was drawn,
/// and including it would make the scan true unconditionally. The palette's
/// group rules meet the same border and are skipped for the same reason.
fn has_scrollbar(buffer: &Buffer, title: &str) -> bool {
    let area = block_area(buffer, title);
    let x = area.right() - 1;

    (area.y + 1..area.bottom() - 1).any(|y| {
        buffer.cell((x, y)).is_some_and(|cell| {
            !matches!(
                cell.symbol(),
                symbols::line::VERTICAL | symbols::line::VERTICAL_LEFT
            )
        })
    })
}

#[test]
fn a_palette_that_overflows_says_so() {
    // 70% of 24 rows leaves 14 for a catalogue in the low twenties.
    assert!(
        has_scrollbar(&with_palette(100, 24), "Commands"),
        "a catalogue taller than the box gets a scrollbar"
    );

    // Given a frame that holds the whole catalogue (design §10) there is
    // no scrollbar — and its absence is what says nothing is hidden.
    assert!(
        !has_scrollbar(
            &with_palette(100, tall_enough_for_the_catalogue()),
            "Commands"
        ),
        "a frame tall enough to hold the whole catalogue must draw no scrollbar"
    );
}

#[test]
fn the_palette_is_centred() {
    let buffer = with_palette(100, 40);
    let (x, _) = top_border(&buffer, "Commands");
    let width = block_width(&buffer, "Commands");

    assert_eq!(x, (100 - width) / 2, "left margin equals right margin");
}

#[test]
fn a_wide_frame_does_not_get_a_wide_palette() {
    assert_eq!(block_width(&with_palette(200, 40), "Commands"), 60);
    assert_eq!(block_width(&with_palette(100, 40), "Commands"), 50);
}

#[test]
fn a_tall_frame_does_not_get_a_palette_full_of_blank_rows() {
    let buffer = with_palette(100, tall_enough_for_the_catalogue());
    let (x, y) = top_border(&buffer, "Commands");

    let bottom = (y + 1..buffer.area.height)
        .find(|at| {
            buffer
                .cell((x, *at))
                .is_some_and(|cell| cell.symbol() == "╰")
        })
        .expect("a bordered block has a bottom edge");

    assert_eq!(
        usize::from(bottom - y) + 1,
        lc_domain::commands::row_count(None) + 2,
        "the block ends where the catalogue does, not at 70% of 120 rows"
    );
}

#[test]
fn a_frame_too_small_for_a_palette_does_not_get_one() {
    assert!(
        !buffer_text(&with_palette(60, 40)).contains("Commands"),
        "half of 60 columns is under the palette's floor"
    );
}

#[test]
fn the_narrowest_palette_still_fits_its_widest_row() {
    // 50% of 78 columns lands exactly on MIN_W, the floor a wider action
    // would clip against if the margin were counted on one side only.
    let drawn = buffer_text(&with_palette(78, 40));

    assert!(
        drawn.contains("focus Table contents"),
        "the widest action must appear whole, not clipped to 19 characters"
    );
}

#[test]
fn a_frame_that_would_clip_the_widest_row_gets_no_palette() {
    // 50% of 76 columns is 38, one under MIN_W. A gate that let this
    // through would draw a palette whose widest row is clipped by one
    // character rather than refusing it outright.
    let drawn = buffer_text(&with_palette(76, 40));

    assert!(!drawn.contains("Commands"), "the block must not be titled");
    assert!(
        !drawn.contains("Global"),
        "nor must the catalogue be listed"
    );
}

#[test]
fn the_frame_reports_how_many_rows_a_palette_would_show() {
    let state = State::default();
    let mut terminal =
        Terminal::new(TestBackend::new(100, 24)).expect("TestBackend always initialises");
    let mut geometry = Geometry::default();

    terminal
        .draw(|frame| {
            geometry = render(frame, &view_model(&state), Theme::default()).geometry;
        })
        .expect("TestBackend always draws");

    // 70% of 24 rows is 16, less two borders — reported with no palette
    // open, because it is what gates `?` (design §5.1).
    assert_eq!(geometry.command_rows(), 14);
}

#[test]
fn a_frame_too_small_for_a_palette_reports_no_rows() {
    let state = State::default();
    let mut terminal =
        Terminal::new(TestBackend::new(60, 24)).expect("TestBackend always initialises");
    let mut geometry = Geometry::default();

    terminal
        .draw(|frame| {
            geometry = render(frame, &view_model(&state), Theme::default()).geometry;
        })
        .expect("TestBackend always draws");

    assert_eq!(geometry.command_rows(), 0);
}
