//! Bounded motion over a list: a cursor, a scroll offset, and the span a
//! wrapped row occupies.

/// A selection into a list, which cannot be moved past either end.
///
/// The index is private on purpose: an out-of-range selection cannot be
/// constructed from outside this module, so a renderer never has to defend
/// against one (conventions.md §1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    index: usize,
}

impl Cursor {
    /// The selected row.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    /// Moves to the next row, stopping on the last one.
    pub const fn down(&mut self, len: usize) {
        let last = len.saturating_sub(1);
        if self.index < last {
            self.index += 1;
        } else {
            self.index = last;
        }
    }

    /// Moves to the previous row, stopping on the first one.
    pub const fn up(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    /// Pulls the selection back inside a list that shrank.
    pub const fn clamp_to(&mut self, len: usize) {
        let last = len.saturating_sub(1);
        if self.index > last {
            self.index = last;
        }
    }

    /// Moves to `index`, or to the last row when the list is shorter.
    pub const fn go_to(&mut self, index: usize, len: usize) {
        self.index = index;
        self.clamp_to(len);
    }

    /// Moves a screen down, stopping on the last row.
    pub const fn page_down(&mut self, len: usize, height: usize) {
        self.go_to(self.index.saturating_add(height), len);
    }

    /// Moves a screen up, stopping on the first row.
    pub const fn page_up(&mut self, height: usize) {
        self.index = self.index.saturating_sub(height);
    }

    pub const fn to_start(&mut self) {
        self.index = 0;
    }

    pub const fn to_end(&mut self, len: usize) {
        self.go_to(len.saturating_sub(1), len);
    }
}

/// Where a selected row was drawn, counted in the lines a viewport scrolls
/// over.
///
/// The Table DDL panel is the one place a cursor and an offset count in
/// different units: the cursor walks described rows, the offset walks the
/// wrapped lines they occupy, and only the renderer knows the conversion
/// (docs/superpowers/specs/2026-08-10-ddl-selection-design.md §5.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Span {
    /// The first line the row occupies.
    pub first: usize,
    /// How many it occupies. Zero means no row was drawn.
    pub lines: usize,
}

/// A viewport offset over a document of known length.
///
/// The offset is private for the reason [`Cursor`]'s index is: every motion
/// re-establishes `offset <= len - height` here, so a renderer is never
/// handed one that points past the end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scroll {
    offset: usize,
}

impl Scroll {
    /// The first line drawn.
    #[must_use]
    pub const fn offset(self) -> usize {
        self.offset
    }

    /// Moves down a line, stopping with the last line at the bottom of the
    /// panel rather than dragging it to the top.
    pub const fn down(&mut self, len: usize, height: usize) {
        self.go_to(self.offset.saturating_add(1), len, height);
    }

    /// Moves up a line, stopping at the top.
    pub const fn up(&mut self) {
        self.offset = self.offset.saturating_sub(1);
    }

    pub const fn page_down(&mut self, len: usize, height: usize) {
        self.go_to(self.offset.saturating_add(height), len, height);
    }

    pub const fn page_up(&mut self, height: usize) {
        self.offset = self.offset.saturating_sub(height);
    }

    pub const fn to_start(&mut self) {
        self.offset = 0;
    }

    pub const fn to_end(&mut self, len: usize, height: usize) {
        self.go_to(len, len, height);
    }

    /// Pulls a stale offset back over a document that shrank or a panel that
    /// grew.
    pub const fn clamp_to(&mut self, len: usize, height: usize) {
        self.go_to(self.offset, len, height);
    }

    /// Pulls the viewport the shortest distance that puts `index` inside it.
    ///
    /// What makes a selection stay visible now that the renderer no longer
    /// arranges it — `update` applies this to whichever cursor moved
    /// (docs/superpowers/specs/2026-08-09-list-viewport-design.md §3).
    pub const fn follow(&mut self, index: usize, height: usize) {
        if index < self.offset {
            self.offset = index;
        } else if height > 0 && index >= self.offset + height {
            self.offset = index + 1 - height;
        }
    }

    /// Pulls the viewport the shortest distance that puts `span` inside it.
    ///
    /// [`Self::follow`]'s counterpart for a selection that occupies more than
    /// one line. A span taller than the panel cannot be made to fit, so its
    /// first line goes to the top: a property drawn tail-first has its name
    /// scrolled off, which is the half that says what the tail is.
    pub const fn follow_span(&mut self, span: Span, height: usize) {
        if span.lines == 0 || height == 0 {
            return;
        }

        if span.first < self.offset || span.lines >= height {
            self.offset = span.first;
            return;
        }

        let last = span.first + span.lines - 1;

        if last >= self.offset + height {
            self.offset = last + 1 - height;
        }
    }

    const fn go_to(&mut self, offset: usize, len: usize, height: usize) {
        let last = len.saturating_sub(height);
        // `Ord::min` is not const.
        self.offset = if offset > last { last } else { offset };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_starts_on_the_first_row() {
        assert_eq!(Cursor::default().index(), 0);
    }

    #[test]
    fn moving_down_stops_on_the_last_row() {
        let mut cursor = Cursor::default();

        for _ in 0..5 {
            cursor.down(3);
        }

        assert_eq!(cursor.index(), 2, "a cursor must not run off the end");
    }

    #[test]
    fn moving_up_stops_on_the_first_row() {
        let mut cursor = Cursor::default();
        cursor.down(3);

        cursor.up();
        cursor.up();

        assert_eq!(cursor.index(), 0, "a cursor must not run off the start");
    }

    #[test]
    fn an_empty_list_leaves_the_cursor_on_the_first_row() {
        let mut cursor = Cursor::default();

        cursor.down(0);

        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn clamping_pulls_the_cursor_into_a_list_that_shrank() {
        let mut cursor = Cursor::default();
        for _ in 0..9 {
            cursor.down(10);
        }

        cursor.clamp_to(4);

        assert_eq!(cursor.index(), 3);
    }

    #[test]
    fn clamping_leaves_a_cursor_that_is_already_inside_alone() {
        let mut cursor = Cursor::default();
        cursor.down(10);

        cursor.clamp_to(10);

        assert_eq!(cursor.index(), 1);
    }

    #[test]
    fn a_scroll_starts_at_the_top() {
        assert_eq!(Scroll::default().offset(), 0);
    }

    #[test]
    fn scrolling_down_stops_with_the_last_line_at_the_bottom() {
        let mut scroll = Scroll::default();

        for _ in 0..50 {
            scroll.down(20, 6);
        }

        assert_eq!(
            scroll.offset(),
            14,
            "the last line belongs at the bottom of the panel, not dragged to the top"
        );
    }

    #[test]
    fn scrolling_up_stops_at_the_top() {
        let mut scroll = Scroll::default();
        scroll.down(20, 6);

        scroll.up();
        scroll.up();

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn a_document_that_fits_never_scrolls() {
        let mut scroll = Scroll::default();

        scroll.down(6, 20);
        scroll.page_down(6, 20);
        scroll.to_end(6, 20);

        assert_eq!(scroll.offset(), 0, "there is nothing below the fold");
    }

    #[test]
    fn a_page_moves_exactly_one_panel_height() {
        let mut scroll = Scroll::default();

        scroll.page_down(100, 12);

        assert_eq!(scroll.offset(), 12);

        scroll.page_up(12);

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn the_last_page_is_short_and_lands_on_the_end() {
        let mut scroll = Scroll::default();
        scroll.page_down(20, 6);
        scroll.page_down(20, 6);
        assert_eq!(scroll.offset(), 12);

        scroll.page_down(20, 6);

        assert_eq!(scroll.offset(), 14);
    }

    #[test]
    fn a_scroll_reaches_both_ends_in_one_key() {
        let mut scroll = Scroll::default();

        scroll.to_end(20, 6);
        assert_eq!(scroll.offset(), 14);

        scroll.to_start();
        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn clamping_pulls_a_stale_offset_over_a_document_that_shrank() {
        let mut scroll = Scroll::default();
        scroll.to_end(60, 6);
        assert_eq!(scroll.offset(), 54);

        scroll.clamp_to(10, 6);

        assert_eq!(scroll.offset(), 4);
    }

    #[test]
    fn clamping_pulls_a_stale_offset_back_when_the_panel_grows() {
        let mut scroll = Scroll::default();
        scroll.to_end(20, 6);

        scroll.clamp_to(20, 30);

        assert_eq!(
            scroll.offset(),
            0,
            "a panel taller than its document shows all of it"
        );
    }

    #[test]
    fn paging_the_cursor_moves_a_screen_and_stops_on_the_last_row() {
        let mut cursor = Cursor::default();

        cursor.page_down(20, 6);
        assert_eq!(cursor.index(), 6);

        for _ in 0..5 {
            cursor.page_down(20, 6);
        }
        assert_eq!(cursor.index(), 19, "the cursor stops on the last row");

        cursor.page_up(6);
        assert_eq!(cursor.index(), 13);
    }

    #[test]
    fn the_cursor_reaches_both_ends_in_one_key() {
        let mut cursor = Cursor::default();

        cursor.to_end(20);
        assert_eq!(cursor.index(), 19);

        cursor.to_start();
        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn an_empty_list_has_no_end_to_jump_to() {
        let mut cursor = Cursor::default();

        cursor.to_end(0);

        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn following_a_selection_below_the_viewport_puts_it_on_the_last_row() {
        let mut scroll = Scroll::default();

        scroll.follow(9, 5);

        assert_eq!(scroll.offset(), 5, "rows 5..=9 are drawn");
    }

    #[test]
    fn following_a_selection_above_the_viewport_puts_it_on_the_first_row() {
        let mut scroll = Scroll::default();
        scroll.follow(9, 5);

        scroll.follow(2, 5);

        assert_eq!(scroll.offset(), 2);
    }

    #[test]
    fn following_a_selection_already_on_screen_moves_nothing() {
        let mut scroll = Scroll::default();
        scroll.follow(9, 5);

        for index in 5..=9 {
            scroll.follow(index, 5);
            assert_eq!(scroll.offset(), 5, "row {index} was already drawn");
        }
    }

    #[test]
    fn following_into_a_panel_with_no_room_does_not_underflow() {
        let mut scroll = Scroll::default();

        scroll.follow(9, 0);

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn following_a_span_already_in_view_moves_nothing() {
        let mut scroll = Scroll::default();
        scroll.follow_span(Span { first: 2, lines: 3 }, 10);

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn following_a_span_below_the_viewport_pulls_its_last_line_to_the_bottom() {
        let mut scroll = Scroll::default();
        scroll.follow_span(
            Span {
                first: 12,
                lines: 3,
            },
            10,
        );

        assert_eq!(
            scroll.offset(),
            5,
            "lines 5..15 are drawn, ending on the span"
        );
    }

    #[test]
    fn following_a_span_above_the_viewport_pulls_its_first_line_to_the_top() {
        let mut scroll = Scroll::default();
        scroll.to_end(100, 10);
        scroll.follow_span(Span { first: 4, lines: 2 }, 10);

        assert_eq!(scroll.offset(), 4);
    }

    #[test]
    fn a_span_taller_than_the_panel_shows_its_head_rather_than_its_tail() {
        let mut scroll = Scroll::default();
        scroll.follow_span(
            Span {
                first: 20,
                lines: 30,
            },
            10,
        );

        assert_eq!(
            scroll.offset(),
            20,
            "a property's name is the half worth reading; its tail without it is not"
        );
    }

    #[test]
    fn a_span_of_no_lines_is_nothing_to_follow() {
        let mut scroll = Scroll::default();
        scroll.to_end(100, 10);
        let before = scroll.offset();

        scroll.follow_span(Span::default(), 10);

        assert_eq!(scroll.offset(), before);
    }
}
