use super::*;

fn grid_of(columns: usize, width: usize) -> State {
    grid_of_rows(columns, 1, width)
}

fn cell_grid_of(columns: usize, width: usize) -> State {
    let mut state = grid_of(columns, width);
    state.contents.mode = SelectionMode::Cell;
    state
}

#[test]
fn l_and_right_move_the_grid_cursor_one_column() {
    for key in [Key::Char('l'), Key::Right] {
        let mut state = cell_grid_of(4, 80);

        press(&mut state, key);

        assert_eq!(state.contents.column.index(), 1, "{key:?}");
    }
}

#[test]
fn h_and_left_move_the_grid_cursor_back_one_column() {
    for key in [Key::Char('h'), Key::Left] {
        let mut state = cell_grid_of(4, 80);
        press(&mut state, Key::Char('l'));

        press(&mut state, key);

        assert_eq!(state.contents.column.index(), 0, "{key:?}");
    }
}

#[test]
fn the_grid_cursor_cannot_run_off_either_edge_of_a_page() {
    let mut state = cell_grid_of(2, 80);

    for _ in 0..5 {
        press(&mut state, Key::Char('l'));
    }
    assert_eq!(state.contents.column.index(), 1, "stops on the last column");

    for _ in 0..5 {
        press(&mut state, Key::Char('h'));
    }
    assert_eq!(state.contents.column.index(), 0, "stops on the first");
}

#[test]
fn row_mode_scrolls_the_grid_a_column_per_press() {
    // Four 6-wide columns with two blanks between: two fit in 18.
    let mut state = grid_of(4, 18);
    assert_eq!(state.contents.mode, SelectionMode::Row);

    for key in [Key::Char('l'), Key::Right] {
        let first = state.contents.column_scroll.first();

        press(&mut state, key);

        assert_eq!(
            state.contents.column_scroll.first(),
            first + 1,
            "{key:?} must reveal a column rather than move a cursor \
             nothing paints"
        );
    }

    for key in [Key::Char('h'), Key::Left] {
        let first = state.contents.column_scroll.first();

        press(&mut state, key);

        assert_eq!(state.contents.column_scroll.first(), first - 1, "{key:?}");
    }
}

#[test]
fn row_mode_stops_the_grid_at_either_end_of_the_page() {
    let mut state = grid_of(4, 18);

    for _ in 0..5 {
        press(&mut state, Key::Char('l'));
    }
    assert_eq!(
        state.contents.column_scroll.first(),
        2,
        "the window ends on the last column, with no dead space past it"
    );

    for _ in 0..5 {
        press(&mut state, Key::Char('h'));
    }
    assert_eq!(state.contents.column_scroll.first(), 0);
}

#[test]
fn row_mode_moves_nothing_over_a_page_that_fits() {
    let mut state = grid_of(4, 80);

    press(&mut state, Key::Char('l'));

    assert_eq!(state.contents.column_scroll.first(), 0);
    assert_eq!(
        state.contents.column.index(),
        0,
        "with nothing to reveal there is no column to put the cursor on"
    );
}

#[test]
fn the_column_the_grid_scrolled_to_is_the_one_cell_mode_opens_on() {
    let mut state = grid_of(4, 18);
    press(&mut state, Key::Char('l'));

    press(&mut state, Key::Char('m'));

    assert_eq!(state.contents.mode, SelectionMode::Column);
    assert_eq!(
        state.contents.column.index(),
        2,
        "the invisible cursor rode along with the window, so cycling to a \
         mode that paints it lands on a drawn column"
    );
}

#[test]
fn a_grid_with_no_page_swallows_horizontal_motion() {
    let mut state = State::default();
    state.focus_on(Panel::Contents);

    press(&mut state, Key::Char('l'));

    assert_eq!(state.contents.column.index(), 0);
    assert_eq!(state.contents.column_scroll.first(), 0);
}

#[test]
fn horizontal_motion_belongs_to_the_focused_panel_only() {
    let mut state = grid_of(4, 80);
    state.focus_on(Panel::Ddl);

    press(&mut state, Key::Char('l'));

    assert_eq!(state.contents.column.index(), 0);
}

#[test]
fn the_column_viewport_follows_the_cell_cursor() {
    // Four 6-wide columns (heading marks outrun the value) with two blanks
    // between: two fit in 18 once the bar and gutter are taken off.
    let mut state = cell_grid_of(4, 18);

    press(&mut state, Key::Char('l'));
    assert_eq!(
        state.contents.column_scroll.first(),
        0,
        "column 1 is already drawn"
    );

    press(&mut state, Key::Char('l'));
    assert_eq!(
        state.contents.column_scroll.first(),
        1,
        "the shortest move that draws column 2"
    );
}

#[test]
fn a_narrower_frame_pulls_the_column_viewport_back_to_the_cursor() {
    let mut state = cell_grid_of(4, 80);
    for _ in 0..3 {
        press(&mut state, Key::Char('l'));
    }
    assert_eq!(
        state.contents.column_scroll.first(),
        0,
        "all four fit at 80"
    );

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(18)),
    );

    assert_eq!(
        state.contents.column_scroll.first(),
        2,
        "the selected column must stay drawn after a resize"
    );
}

#[test]
fn a_panel_that_grew_pulls_the_column_viewport_back() {
    let mut state = cell_grid_of(4, 18);
    for _ in 0..3 {
        press(&mut state, Key::Char('l'));
    }
    assert_eq!(
        state.contents.column_scroll.first(),
        2,
        "two of four fit at 16"
    );

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
    );

    assert_eq!(
        state.contents.column_scroll.first(),
        0,
        "all four fit again, so none may be left hidden behind the offset"
    );
}

#[test]
fn a_frame_too_small_to_draw_does_not_strand_the_column_viewport() {
    let mut state = cell_grid_of(4, 80);
    for _ in 0..3 {
        press(&mut state, Key::Char('l'));
    }

    // A frame below the minimum size draws nothing.
    update(&mut state, Msg::Measured(Geometry::default()));
    assert_eq!(
        state.contents.column_scroll.first(),
        3,
        "with no room, the cursor's column is the only one worth drawing"
    );

    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0).with_grid_width(80)),
    );

    assert_eq!(
        state.contents.column_scroll.first(),
        0,
        "restoring the size undoes it, without the user pressing h"
    );
}

#[test]
fn home_and_end_still_reach_the_ends_of_the_row_list() {
    let mut state = grid_of_rows(4, 12, 80);
    state.contents.mode = SelectionMode::Cell;
    press(&mut state, Key::Char('l'));

    press(&mut state, Key::End);

    assert_eq!(
        state.contents.cursor.index(),
        11,
        "End reaches the ends of the document, which in a grid is the rows"
    );
    assert_eq!(
        state.contents.column.index(),
        1,
        "and says nothing about the column axis"
    );

    press(&mut state, Key::Home);

    assert_eq!(state.contents.cursor.index(), 0, "and Home the other end");
    assert_eq!(
        state.contents.column.index(),
        1,
        "still on the column it was on"
    );
}
