use super::*;

#[test]
fn the_ddl_panel_scrolls_to_the_lines_below_its_fold() {
    // At 40x20 the Ddl body is 14 by 5, which `orders()` overflows.
    const WIDTH: usize = 14;
    const HEIGHT: usize = 5;

    let state = describing(orders());
    let all = crate::details::lay_out(
        &ddl_details(&state),
        0,
        &Marked::none(),
        WIDTH,
        None,
        Theme::default(),
    )
    .lines;
    assert!(
        all.len() > HEIGHT,
        "the fixture must overflow the panel or this test proves nothing"
    );
    let tail = all.last().expect("the body has lines").to_string();
    assert!(
        !tail.trim().is_empty(),
        "the assertions below need a tail to look for"
    );

    let mut scrolled = describing(orders());
    scrolled.ddl.scroll.to_end(all.len(), HEIGHT);

    let before = draw_state(40, 20, &state);
    let after = draw_state(40, 20, &scrolled);
    // Rows 13..17, columns 1..14: the Ddl panel only.
    let visible = |buffer: &Buffer| -> Vec<String> {
        (13..18)
            .map(|y| {
                (1..15)
                    .filter_map(|x| buffer.cell((x, y)))
                    .map(Cell::symbol)
                    .collect::<String>()
            })
            .collect()
    };

    assert!(
        !visible(&before)
            .iter()
            .any(|line| line.contains(tail.trim())),
        "the tail starts below the fold"
    );
    assert!(
        visible(&after)
            .iter()
            .any(|line| line.contains(tail.trim())),
        "scrolling to the end must bring it into view"
    );
}

/// Column 15 at 40x12, between rows 0 and 6.
fn tables_border(buffer: &Buffer) -> String {
    (1..4)
        .filter_map(|y| buffer.cell((15, y)))
        .map(Cell::symbol)
        .collect()
}

#[test]
fn a_panel_whose_content_overflows_grows_a_bar_on_its_border() {
    let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();

    let buffer = draw_state(40, 12, &browsing(&refs, 0));

    assert!(
        tables_border(&buffer).contains(THUMB),
        "the thumb says how far through twelve keyspaces three rows are"
    );
}

#[test]
fn the_thumb_sits_at_whichever_end_of_the_track_the_viewport_is_on() {
    let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();

    assert!(
        tables_border(&draw_state(40, 12, &browsing(&refs, 0))).starts_with(THUMB),
        "the first keyspace is on screen, so the thumb belongs at the top"
    );
    assert!(
        tables_border(&draw_state(40, 12, &browsing(&refs, 11))).ends_with(THUMB),
        "the last keyspace is on screen, so the thumb belongs at the bottom"
    );
}

#[test]
fn the_track_is_the_border_itself_rather_than_a_second_bar_beside_it() {
    let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();

    // Scrolled to the top; the default set would draw `║`.
    let border = tables_border(&draw_state(40, 12, &browsing(&refs, 0)));
    let track: String = border.chars().skip_while(|c| *c == '▐').collect();

    assert!(!track.is_empty(), "twelve keyspaces do not fit three rows");
    assert!(
        track.chars().all(|c| c == '│'),
        "the track has to be the border glyph, or the panel grows a second \
         border beside the first: {track:?}"
    );
}

#[test]
fn the_bar_takes_the_border_column_rather_than_a_column_of_content() {
    let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();

    let buffer = draw_state(40, 12, &browsing(&refs, 0));

    assert_eq!(
        row(&buffer, 1),
        "│▌\u{f084} ks00       ▐│Press Enter on a table│",
        "the row is as wide as it is without a scrollbar; the scrollbar replaced the border"
    );
}

#[test]
fn what_the_frame_measures_does_not_depend_on_where_it_is_scrolled() {
    // A `Geometry` varying with its own offset would close the render-domain
    // loop.
    let state = describing(orders());
    let mut scrolled = describing(orders());
    scrolled.ddl.scroll.to_end(100, 6);

    assert_eq!(geometry_of(40, 20, &state), geometry_of(40, 20, &scrolled));
}

#[test]
fn a_panel_whose_content_fits_keeps_its_border_intact() {
    let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo"], 0));

    assert_eq!(
        tables_border(&buffer),
        "│││",
        "nothing is hidden, so nothing says otherwise"
    );
}

#[test]
fn the_selection_stays_visible_when_the_list_outgrows_the_panel() {
    // Three content rows; twelve keyspaces force a scroll.
    let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();

    let buffer = draw_state(40, 12, &browsing(&refs, 11));
    let rows: Vec<String> = (1..4).map(|y| row(&buffer, y)).collect();

    assert!(
        rows.iter().any(|line| line.contains("ks11")),
        "the selected row must stay on screen once the list scrolls"
    );
    assert!(
        !rows.iter().any(|line| line.contains("ks00")),
        "a row scrolled out of view must not still be drawn"
    );
}
