use super::*;

#[test]
fn the_frame_reports_what_each_panel_can_show() {
    // At 40x20: a 16-column sidebar, Tables 8 rows tall and Ddl 11, so
    // six and nine rows of content inside their borders.
    let geometry = geometry_of(40, 20, &describing(orders()));

    assert_eq!(geometry.height(Panel::Tables), 6);
    assert_eq!(geometry.height(Panel::Ddl), 9);
    assert_eq!(
        geometry.ddl_lines(),
        crate::details::lay_out(
            &ddl_details(&describing(orders())),
            0,
            &Marked::none(),
            14,
            None,
            Theme::default()
        )
        .lines
        .len()
    );
}

#[test]
fn a_grid_reports_the_rows_it_can_show_rather_than_its_height() {
    let state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));

    let geometry = geometry_of(60, 12, &state);

    // The Contents panel is 9 rows tall at this size — Queries is closed
    // below it — so 7 inside its border, two of which the header takes:
    // the names and the blank line under them.
    assert_eq!(
        geometry.height(Panel::Contents),
        5,
        "a page motion must not count the header as a row it can land on"
    );
}

#[test]
fn the_frame_reports_how_wide_the_contents_panel_is() {
    // The Contents block is 44 columns of the 60 at this size, so 42
    // inside its borders.
    let state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));

    assert_eq!(geometry_of(60, 12, &state).grid_width(), 42);
    assert_eq!(
        geometry_of(60, 12, &State::default()).grid_width(),
        42,
        "the width is the panel's, and the domain needs it before a page arrives"
    );
}

#[test]
fn a_collapsed_panel_reports_no_room_at_all() {
    let geometry = geometry_of(40, 5, &describing(orders()));

    assert_eq!(geometry.height(Panel::Ddl), 0);
    assert_eq!(geometry.height(Panel::Queries), 0);
}
