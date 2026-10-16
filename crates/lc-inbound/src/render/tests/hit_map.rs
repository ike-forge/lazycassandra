use super::*;

#[test]
fn the_map_resolves_every_drawn_keyspace_to_the_row_that_drew_it() {
    let names = ["shop", "typezoo", "system"];
    let (buffer, map) = draw_mapped(40, 12, &browsing(&names, 0));

    for (index, name) in names.iter().enumerate() {
        let y = (1..7)
            .find(|&y| row(&buffer, y).contains(name))
            .unwrap_or_else(|| panic!("{name} was not drawn"));

        assert_eq!(
            map.hit(2, y),
            Some(Click::Row {
                panel: Panel::Tables,
                row: index,
                column: None
            }),
            "{name} was drawn on screen row {y}"
        );
    }
}

#[test]
fn the_map_resolves_a_scrolled_tree_to_the_rows_that_were_drawn() {
    // Twelve keyspaces in five rows force a scroll.
    let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();

    let (buffer, map) = draw_mapped(40, 12, &browsing(&refs, 11));
    let mut resolved = 0;

    for (index, name) in names.iter().enumerate() {
        let Some(y) = (1..7).find(|&y| row(&buffer, y).contains(name.as_str())) else {
            continue;
        };

        assert_eq!(
            map.hit(2, y),
            Some(Click::Row {
                panel: Panel::Tables,
                row: index,
                column: None
            }),
            "{name} was drawn on screen row {y}"
        );
        resolved += 1;
    }

    assert!(
        resolved >= 2,
        "the test proves nothing unless rows were on screen, got {resolved}"
    );
}

#[test]
fn the_map_resolves_a_scrolled_grid_to_the_rows_that_were_drawn() {
    // Twenty rows in five force a scroll.
    let labels: Vec<String> = (0..20).map(|i| format!("row{i:02}")).collect();
    let values: Vec<Vec<CqlValue>> = labels.iter().map(|l| vec![text(l)]).collect();
    let rows: Vec<&[CqlValue]> = values.iter().map(Vec::as_slice).collect();
    let mut state = showing(page_of(&["id"], &rows));
    select_contents(&mut state, 19, 5);

    let (buffer, map) = draw_mapped(60, 12, &state);
    let mut resolved = 0;

    for (index, label) in labels.iter().enumerate() {
        let Some(y) = (1..8).find(|&y| row(&buffer, y).contains(label.as_str())) else {
            continue;
        };

        assert_eq!(
            map.hit(20, y),
            Some(Click::Row {
                panel: Panel::Contents,
                row: index,
                column: None
            }),
            "{label} was drawn on screen row {y}"
        );
        resolved += 1;
    }

    assert!(
        resolved >= 2,
        "the test proves nothing unless rows were on screen, got {resolved}"
    );
}

fn column_of(buffer: &Buffer, y: u16, text: &str) -> u16 {
    let line = row(buffer, y);
    let at = line
        .find(text)
        .unwrap_or_else(|| panic!("{text} was not drawn on row {y}: {line:?}"));

    u16::try_from(line[..at].chars().count()).expect("a frame is narrower than u16::MAX")
}

/// Unique values that are not row numbers, so a find hits the cell, not the
/// gutter.
fn grid_state() -> State {
    showing(page_of(
        &["id", "email"],
        &[
            &[text("id-ada"), text("ada@example.com")],
            &[text("id-bob"), text("bob@example.com")],
        ],
    ))
}

#[test]
fn the_rule_under_a_grids_headings_meets_the_panel() {
    let (buffer, _) = draw_mapped(80, 14, &grid_state());
    let area = block_area(&buffer, "Table contents");
    let symbol = |x: u16| {
        buffer
            .cell((x, area.y + 1 + crate::body::HEADER_ROWS - 1))
            .expect("inside the block")
            .symbol()
            .to_owned()
    };

    assert_eq!(symbol(area.x), symbols::line::VERTICAL_RIGHT);
    assert_eq!(symbol(area.right() - 1), symbols::line::VERTICAL_LEFT);
    assert_eq!(symbol(area.x + 1), symbols::line::HORIZONTAL);
}

#[test]
fn a_grids_heading_names_a_column_rather_than_its_first_row() {
    let (buffer, map) = draw_mapped(60, 12, &grid_state());

    assert!(
        row(&buffer, 1).contains("id") && row(&buffer, 1).contains("email"),
        "row 1 must be the header row this test is about"
    );
    assert!(
        block_row(&buffer, "Table contents", 1)
            .chars()
            .all(|glyph| glyph == '─'),
        "row 2 is the rule under the headings"
    );
    assert!(
        row(&buffer, 3).contains("ada@example.com"),
        "row 3 must carry the first row's value, or `row: 0` below proves nothing"
    );

    assert_eq!(
        map.hit(column_of(&buffer, 1, "email"), 1),
        Some(Click::Column {
            panel: Panel::Contents,
            column: 1
        }),
        "a heading is the column it names"
    );
    assert_eq!(
        map.hit(column_of(&buffer, 3, "ada@example.com"), 3),
        Some(Click::Row {
            panel: Panel::Contents,
            row: 0,
            column: Some(1)
        })
    );
    assert_eq!(
        map.hit(20, 5),
        Some(Click::Panel(Panel::Contents)),
        "below the last row is blank space, not a row"
    );
}

#[test]
fn a_press_on_the_arrow_the_frame_drew_is_the_sort_key() {
    let (buffer, map) = draw_mapped(60, 12, &grid_state());
    let sort = column_of(&buffer, 1, "\u{21c5}");

    assert_eq!(
        map.hit(sort, 1),
        Some(Click::Mark {
            panel: Panel::Contents,
            column: 0,
            mark: Mark::Sort
        })
    );
    assert_eq!(
        map.hit(sort + 2, 1),
        Some(Click::Mark {
            panel: Panel::Contents,
            column: 0,
            mark: Mark::Filter
        }),
        "and the funnel a blank past it is the filter key"
    );
}

#[test]
fn the_map_resolves_every_cell_to_the_column_that_drew_it() {
    let (buffer, map) = draw_mapped(60, 12, &grid_state());

    for (column, value) in ["id-ada", "ada@example.com"].into_iter().enumerate() {
        assert_eq!(
            map.hit(column_of(&buffer, 3, value), 3),
            Some(Click::Row {
                panel: Panel::Contents,
                row: 0,
                column: Some(column)
            }),
            "{value} was drawn in column {column}"
        );
    }
}

#[test]
fn a_press_on_the_chrome_of_a_panel_names_that_panel() {
    let (_buffer, map) = draw_mapped(40, 12, &browsing(&["shop"], 0));

    assert_eq!(map.hit(0, 0), Some(Click::Panel(Panel::Tables)), "corner");
    assert_eq!(
        map.hit(0, 8),
        Some(Click::Panel(Panel::Ddl)),
        "the Ddl panel's left border"
    );
    assert_eq!(
        map.hit(20, 10),
        Some(Click::Panel(Panel::Queries)),
        "the Queries editor takes no press on its lines"
    );
}

#[test]
fn the_map_resolves_every_drawn_tab_to_its_place_in_the_strip() {
    let (buffer, map) = draw_mapped(104, 41, &listing(orders()));
    let strip = row(&buffer, DDL_BORDER);

    assert!(
        strip.contains("DDL") && strip.contains("Cols") && strip.contains("Idx"),
        "row {DDL_BORDER} must be the strip this test is about: {strip:?}"
    );

    for (label, tab) in [("DDL", 0), ("Cols", 1), ("Idx", 2)] {
        assert_eq!(
            map.hit(column_of(&buffer, DDL_BORDER, label), DDL_BORDER),
            Some(Click::Tab {
                panel: Panel::Ddl,
                tab
            }),
            "{label} is the {tab}th tab, whichever name it is drawn under"
        );
    }
}

#[test]
fn a_single_tab_panel_resolves_its_title_to_its_one_tab() {
    let (buffer, map) = draw_mapped(40, 12, &browsing(&["shop"], 0));

    assert_eq!(
        map.hit(column_of(&buffer, 0, "Tables"), 0),
        Some(Click::Tab {
            panel: Panel::Tables,
            tab: 0
        }),
        "a panel with one tab is its own tab"
    );
}

#[test]
fn the_map_resolves_every_line_of_a_wrapped_ddl_row_to_that_row() {
    // The identity row wraps at 23 columns; a blank and a heading precede the
    // first column.
    let (buffer, map) = draw_mapped(104, 41, &listing(orders()));

    assert_eq!(
        ddl_rows(&buffer)[..6],
        [
            // The cursor opens on this row.
            "▌  \u{f084} Keyspace       shop",
            "   \u{f0ce} Table",
            "     orders_by_customer",
            "",
            "3 columns",
            " \u{f030b} customer_id      uuid",
        ],
        "the lines this test resolves against"
    );

    let expected = [Some(0), Some(1), Some(1), None, None, Some(2)];

    for (line, row) in expected.into_iter().enumerate() {
        let y = DDL_BORDER + 1 + u16::try_from(line).expect("six lines");
        let click = match row {
            Some(row) => Click::Row {
                panel: Panel::Ddl,
                row,
                column: None,
            },
            None => Click::Panel(Panel::Ddl),
        };

        assert_eq!(map.hit(3, y), Some(click), "line {line}");
    }
}

#[test]
fn the_hint_bar_and_a_frame_below_the_minimum_are_not_clickable() {
    let (_buffer, map) = draw_mapped(40, 12, &browsing(&["shop"], 0));
    assert_eq!(map.hit(5, 11), None, "the bar owns the bottom row");

    let (_buffer, tiny) = draw_mapped(39, 6, &State::default());
    assert_eq!(tiny.hit(0, 0), None, "nothing is drawn, so nothing is hit");
    assert_eq!(tiny.hit(20, 3), None);
}
