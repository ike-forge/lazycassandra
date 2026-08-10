//! The grid's column arithmetic.
//!
//! Separate from [`crate::contents`] because it is arithmetic and not state:
//! how wide a column is, how many fit from a given offset, and where the
//! horizontal viewport has to sit to show a cell. `update` needs all four
//! answers, and they cannot be measured in the renderer — "how many fit from
//! offset N" varies with N, and architecture.md §3 forbids an offset-dependent
//! measurement crossing back
//! (doc/specs/2026-08-10-table-display-design.md §6).

use unicode_width::UnicodeWidthStr as _;

use crate::cell;
use crate::model::Page;

/// How wide a column may grow before its values are ellipsised.
///
/// A uuid is 36 columns and a blob is unbounded. Horizontal scrolling reaches
/// columns, not the tails of values (design §7).
pub const MAX_COLUMN_WIDTH: u16 = 24;

/// One blank column between cells, and between the gutter and the first
/// column.
pub const COLUMN_SPACING: u16 = 1;

/// Each column's natural width: the widest of its heading and its values,
/// capped.
#[must_use]
pub fn widths(page: &Page) -> Vec<u16> {
    page.columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let widest = page
                .rows
                .iter()
                .filter_map(|row| row.values.get(index))
                .map(|value| cell::text(value).width())
                .chain(std::iter::once(name.width()))
                .max()
                .unwrap_or(1);

            u16::try_from(widest)
                .unwrap_or(MAX_COLUMN_WIDTH)
                .clamp(1, MAX_COLUMN_WIDTH)
        })
        .collect()
}

/// The width left for columns once the gutter and its spacing are taken off.
///
/// One home for arithmetic the domain and the renderer must agree on exactly:
/// a disagreement scrolls the cell cursor and the drawn window to different
/// places, and no test can see it (design §6).
#[must_use]
pub const fn available(width: u16, gutter: u16) -> u16 {
    width
        .saturating_sub(gutter)
        .saturating_sub(if gutter == 0 { 0 } else { COLUMN_SPACING })
}

/// How many columns fit in `available`, drawing from `first`.
///
/// At least one whenever `first` names a column: a panel too narrow for even
/// that one still shows what it can of it, which is more use than an empty
/// panel.
#[must_use]
pub fn fitting(widths: &[u16], first: usize, available: u16) -> usize {
    let mut used: u16 = 0;
    let mut count = 0;

    for width in widths.iter().skip(first) {
        let spacing = if count == 0 { 0 } else { COLUMN_SPACING };
        let next = used.saturating_add(spacing).saturating_add(*width);

        if next > available && count > 0 {
            break;
        }

        used = next;
        count += 1;
    }

    count
}

/// The row-number gutter's width: the digits of the last row's number.
#[must_use]
pub fn gutter(rows: usize) -> u16 {
    if rows == 0 {
        return 0;
    }

    let digits = rows.to_string().len();

    u16::try_from(digits).unwrap_or(u16::MAX)
}

/// Which column the grid is drawn from.
///
/// Not [`crate::state::Scroll`]: every method there takes a `height`, because
/// a row viewport shows a fixed number of rows whatever the offset. How many
/// columns fit depends on which one you start from, so the window is a
/// function of the offset rather than a constant beside it (design §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColumnScroll {
    first: usize,
}

impl ColumnScroll {
    /// The leftmost column drawn.
    #[must_use]
    pub const fn first(self) -> usize {
        self.first
    }

    /// Moves the shortest distance that puts `column` on screen.
    pub fn follow(&mut self, column: usize, widths: &[u16], available: u16) {
        self.first = self.first.min(column);

        while self.first < column && column >= self.first + fitting(widths, self.first, available) {
            self.first += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CqlValue, Row};

    fn page(columns: &[&str], rows: &[&[CqlValue]]) -> Page {
        Page {
            columns: columns.iter().map(|name| (*name).to_owned()).collect(),
            rows: rows
                .iter()
                .map(|values| Row {
                    values: values.to_vec(),
                })
                .collect(),
            next: None,
        }
    }

    #[test]
    fn a_column_is_as_wide_as_the_widest_thing_in_it() {
        let page = page(
            &["id", "name"],
            &[
                &[CqlValue::Int(1), CqlValue::Text("alice".to_owned())],
                &[CqlValue::Int(2), CqlValue::Text("bartholomew".to_owned())],
            ],
        );

        assert_eq!(widths(&page), vec![2, 11], "the heading counts too");
    }

    #[test]
    fn a_heading_wider_than_every_value_sets_the_width() {
        let page = page(&["description"], &[&[CqlValue::Text("hi".to_owned())]]);

        assert_eq!(widths(&page), vec![11]);
    }

    #[test]
    fn a_column_is_capped() {
        let page = page(&["blob"], &[&[CqlValue::Text("x".repeat(100))]]);

        assert_eq!(widths(&page), vec![MAX_COLUMN_WIDTH]);
    }

    #[test]
    fn a_column_with_no_rows_is_still_as_wide_as_its_heading() {
        let page = page(&["id"], &[]);

        assert_eq!(widths(&page), vec![2]);
    }

    #[test]
    fn the_gutter_costs_its_own_width_and_one_spacing_column() {
        assert_eq!(available(22, 1), 22 - 1 - COLUMN_SPACING);
        assert_eq!(available(22, 4), 22 - 4 - COLUMN_SPACING);
        assert_eq!(available(22, 0), 22, "no gutter, no spacing beside it");
        assert_eq!(available(1, 4), 0, "a body narrower than its own gutter");
    }

    #[test]
    fn only_the_columns_that_fit_are_counted() {
        // 5 + 1 + 5 + 1 + 5 = 17 for three, 11 for two.
        assert_eq!(fitting(&[5, 5, 5], 0, 17), 3);
        assert_eq!(fitting(&[5, 5, 5], 0, 16), 2);
        assert_eq!(fitting(&[5, 5, 5], 0, 11), 2);
        assert_eq!(fitting(&[5, 5, 5], 0, 10), 1);
    }

    #[test]
    fn fitting_counts_from_the_offset() {
        assert_eq!(
            fitting(&[20, 5, 5], 1, 11),
            2,
            "the wide first column is not drawn"
        );
    }

    #[test]
    fn a_panel_too_narrow_for_one_column_still_draws_it() {
        assert_eq!(
            fitting(&[20], 0, 3),
            1,
            "an ellipsised first column is more use than an empty panel"
        );
    }

    #[test]
    fn fitting_past_the_last_column_is_nothing() {
        assert_eq!(fitting(&[5, 5], 2, 40), 0);
    }

    #[test]
    fn the_gutter_is_as_wide_as_the_last_row_number() {
        assert_eq!(gutter(0), 0, "no rows, no gutter");
        assert_eq!(gutter(9), 1);
        assert_eq!(gutter(10), 2);
        assert_eq!(gutter(1000), 4);
    }

    #[test]
    fn a_visible_column_moves_the_viewport_nowhere() {
        let mut scroll = ColumnScroll::default();

        scroll.follow(1, &[5, 5, 5], 17);

        assert_eq!(scroll.first(), 0);
    }

    #[test]
    fn the_viewport_moves_the_shortest_distance_that_shows_a_column_to_the_right() {
        let mut scroll = ColumnScroll::default();

        scroll.follow(2, &[5, 5, 5], 11);

        assert_eq!(
            scroll.first(),
            1,
            "columns 1 and 2 fit in 11; the offset must not overshoot to 2"
        );
    }

    #[test]
    fn the_viewport_follows_a_column_to_the_left() {
        let mut scroll = ColumnScroll::default();
        scroll.follow(2, &[5, 5, 5], 11);

        scroll.follow(0, &[5, 5, 5], 11);

        assert_eq!(scroll.first(), 0);
    }

    #[test]
    fn a_column_too_wide_for_the_panel_is_shown_from_its_own_start() {
        let mut scroll = ColumnScroll::default();

        scroll.follow(1, &[5, 40], 10);

        assert_eq!(scroll.first(), 1);
    }

    #[test]
    fn following_a_column_that_is_gone_clamps_the_viewport() {
        let mut scroll = ColumnScroll::default();
        scroll.follow(5, &[5, 5, 5, 5, 5, 5], 11);

        scroll.follow(0, &[5], 11);

        assert_eq!(
            scroll.first(),
            0,
            "a narrower page must not strand the offset"
        );
    }
}
