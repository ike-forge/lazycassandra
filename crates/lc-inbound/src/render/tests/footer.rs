use super::*;

fn contents_bottom(buffer: &Buffer) -> String {
    let area = block_area(buffer, "Table contents");

    (area.x + 1..area.right() - 1)
        .filter_map(|x| buffer.cell((x, area.bottom() - 1)))
        .map(Cell::symbol)
        .collect()
}

fn drawn_with_footer(width: u16, state: &State, footer: Footer) -> Buffer {
    let mut view = view_model(state);
    view.panels[2].footer = Some(footer);
    draw_view(width, 12, &view)
}

#[test]
fn every_footer_the_design_lists_is_drawn_from_the_left_corner() {
    let state = showing(sized_page(1, 3));
    let warning = Token::Severity(Severity::Warning);

    for (footer, said) in [
        (footer("1,234 rows", None, None), "─ 1,234 rows ─"),
        (
            footer("200 rows", None, Some("more")),
            "─ 200 rows · more ─",
        ),
        (
            footer("3,400 rows", None, Some("loading")),
            "─ 3,400 rows · loading ─",
        ),
        (
            footer(
                "812,000 rows",
                Some(("≈ 104.4 MB", warning)),
                Some("paused past 100 MB — P for more"),
            ),
            "─ 812,000 rows · ≈ 104.4 MB · paused past 100 MB — P for more ─",
        ),
        (
            footer(
                "1,000,000 rows",
                Some(("≈ 61.0 MB", Token::Muted)),
                Some("paused past 1,000,000 rows — P for more"),
            ),
            "─ 1,000,000 rows · ≈ 61.0 MB · paused past 1,000,000 rows — P for more ─",
        ),
    ] {
        let bottom = contents_bottom(&drawn_with_footer(160, &state, footer));

        assert!(bottom.starts_with(said), "{bottom:?}");
    }
}

#[test]
fn an_estimate_past_the_first_stop_is_drawn_in_the_warning_colour() {
    let state = showing(sized_page(1, 3));
    let buffer = drawn_with_footer(
        160,
        &state,
        footer(
            "3 rows",
            Some(("≈ 104.4 MB", Token::Severity(Severity::Warning))),
            Some("more"),
        ),
    );
    let area = block_area(&buffer, "Table contents");
    let at = contents_bottom(&buffer)
        .chars()
        .position(|c| c == '≈')
        .expect("drawn");
    let cell = buffer
        .cell((
            area.x + 1 + u16::try_from(at).expect("fits"),
            area.bottom() - 1,
        ))
        .expect("on the border");

    assert_eq!(
        Some(cell.fg),
        Theme::default()
            .style(Token::Severity(Severity::Warning))
            .fg
    );
}

#[test]
fn a_loaded_grid_draws_its_own_footer() {
    let bottom = contents_bottom(&draw_state(60, 12, &showing(sized_page(1, 3))));

    assert!(bottom.starts_with("─ 3 rows ─"), "{bottom:?}");
}

#[test]
fn the_horizontal_track_starts_where_the_footer_ends() {
    let buffer = draw_state(40, 12, &showing_column_path(&[0]));
    let bottom = contents_bottom(&buffer);

    assert!(
        bottom.starts_with(&format!("─ 3 rows {}", symbols::line::THICK_HORIZONTAL)),
        "the thumb sits against the footer, not under it: {bottom:?}"
    );
}

#[test]
fn a_narrow_border_drops_the_hint_then_the_estimate_then_the_footer() {
    let state = showing(sized_page(1, 3));
    let stopped = || {
        footer(
            "812,000 rows",
            Some(("≈ 104.4 MB", Token::Muted)),
            Some("paused past 100 MB — P for more"),
        )
    };

    let wide = contents_bottom(&drawn_with_footer(160, &state, stopped()));
    let roomy = contents_bottom(&drawn_with_footer(95, &state, stopped()));
    let middling = contents_bottom(&drawn_with_footer(70, &state, stopped()));
    let narrow = contents_bottom(&drawn_with_footer(40, &state, stopped()));

    assert!(
        wide.contains("≈ 104.4 MB · paused past 100 MB — P for more"),
        "{wide:?}"
    );
    assert!(
        roomy.starts_with("─ 812,000 rows · ≈ 104.4 MB · paused past 100 MB ─"),
        "{roomy:?}"
    );
    assert!(
        middling.starts_with("─ 812,000 rows · paused past 100 MB ─"),
        "{middling:?}"
    );
    assert!(!narrow.contains("rows"), "{narrow:?}");
}

#[test]
fn the_footer_counts_the_uncommitted_cells_in_the_warning_colour() {
    let state = showing(sized_page(1, 3));
    let buffer = drawn_with_footer(
        60,
        &state,
        Footer {
            dirty: Some("2 uncommitted".to_owned()),
            ..footer("3 rows", None, None)
        },
    );

    let border = (0..buffer.area.height)
        .map(|y| row(&buffer, y))
        .find(|line| line.contains("2 uncommitted"))
        .expect("the count is on the border");
    assert!(border.contains("3 rows · 2 uncommitted"), "{border:?}");
}

/// On the cursor row, whose highlight would bury a background.
#[test]
fn a_dirty_cell_is_drawn_in_the_uncommitted_colour() {
    let mut view = view_model(&showing(sized_page(1, 3)));
    let lc_domain::view::PanelBody::Grid { rows, .. } = &mut view.panels[2].body else {
        panic!("the Contents panel draws a grid");
    };
    let cell = rows
        .first_mut()
        .and_then(|row| row.first_mut())
        .expect("a cell");
    cell.dirty = true;
    let want = cell.text.clone();

    let buffer = draw_view(60, 12, &view);
    let dirty = Theme::default().style(Token::Dirty);

    assert!(
        (0..buffer.area.height).any(|y| (0..buffer.area.width).any(|x| {
            buffer.cell((x, y)).is_some_and(|cell| {
                want.starts_with(cell.symbol())
                    && cell.style().fg == dirty.fg
                    && cell.style().add_modifier.contains(Modifier::REVERSED)
            })
        })),
        "no cell wears Token::Dirty"
    );
}
