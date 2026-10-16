//! The grid's column arithmetic. Here rather than in the renderer: "how many
//! fit from offset N" depends on N, and no offset-dependent measurement may
//! cross back (architecture.md §3).

use std::borrow::Cow;

use unicode_width::UnicodeWidthStr as _;

use crate::cell;
use crate::config::MaxColumnWidth;
use crate::model::Page;

/// Two: an ellipsised value with one blank reads as one word with the next
/// column.
pub const COLUMN_SPACING: u16 = 2;

/// Shares the gutter's widget column. Drawn on every row, so [`available`]
/// subtracts it unconditionally.
pub const BAR: u16 = 1;

/// The blank before the marks and the one between them included. Domain-owned
/// so a heading never outgrows its column.
pub const MARKS: u16 = 4;

/// Headings count with the marks they always carry.
#[must_use]
pub fn natural_widths(page: &Page) -> Vec<u16> {
    page.columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let widest = page
                .rows
                .iter()
                .filter_map(|row| row.values.get(index))
                .map(|value| cell::text(value).width())
                .chain(std::iter::once(name.width() + usize::from(MARKS)))
                .max()
                .unwrap_or(1);

            u16::try_from(widest).unwrap_or(u16::MAX).max(1)
        })
        .collect()
}

/// Past `cap` a value is ellipsised; `Enter` reads it whole.
#[must_use]
pub fn capped(natural: &[u16], cap: MaxColumnWidth) -> Vec<u16> {
    natural
        .iter()
        .map(|width| (*width).min(cap.get()))
        .collect()
}

#[must_use]
pub fn widths(page: &Page, cap: MaxColumnWidth) -> Vec<u16> {
    capped(&natural_widths(page), cap)
}

/// Leftover room is shared equally among truncated columns, none past natural
/// width. Columns that do not all fit stay as they are.
#[must_use]
pub fn spread<'a>(widths: &'a [u16], natural: &[u16], available: u16) -> Cow<'a, [u16]> {
    let spacing = u16::try_from(widths.len().saturating_sub(1))
        .unwrap_or(u16::MAX)
        .saturating_mul(COLUMN_SPACING);
    let used = widths
        .iter()
        .fold(spacing, |used, width| used.saturating_add(*width));
    if used >= available {
        return Cow::Borrowed(widths);
    }
    let mut leftover = available - used;

    let mut truncated: Vec<usize> = (0..widths.len())
        .filter(|at| {
            natural
                .get(*at)
                .is_some_and(|natural| *natural > widths[*at])
        })
        .collect();
    if truncated.is_empty() {
        return Cow::Borrowed(widths);
    }

    let mut spread = widths.to_vec();
    while leftover > 0 && !truncated.is_empty() {
        let count = u16::try_from(truncated.len()).unwrap_or(u16::MAX);
        let share = (leftover / count).max(1);

        for at in &truncated {
            let grown = share.min(natural[*at] - spread[*at]).min(leftover);
            spread[*at] += grown;
            leftover -= grown;
        }
        truncated.retain(|at| spread[*at] < natural[*at]);
    }

    Cow::Owned(spread)
}

/// The domain and the renderer must agree exactly: a mismatch scrolls cursor
/// and window apart invisibly.
#[must_use]
pub const fn available(width: u16, gutter: u16) -> u16 {
    width
        .saturating_sub(BAR)
        .saturating_sub(gutter)
        .saturating_sub(COLUMN_SPACING)
}

/// At least one: a panel too narrow for a column still shows part of it.
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

#[must_use]
pub fn gutter(rows: usize) -> u16 {
    if rows == 0 {
        return 0;
    }

    let digits = rows.to_string().len();

    u16::try_from(digits).unwrap_or(u16::MAX)
}

/// Not [`crate::viewport::Scroll`]: how many columns fit depends on the offset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColumnScroll {
    first: usize,
}

impl ColumnScroll {
    #[must_use]
    pub const fn first(self) -> usize {
        self.first
    }

    /// [`Self::follow`] only moves toward the cursor, so without this a panel
    /// that shrank and grew draws dead space. Runs before `follow`.
    pub fn clamp_to(&mut self, widths: &[u16], available: u16) {
        while self.first > 0
            && (self.first - 1) + fitting(widths, self.first - 1, available) >= widths.len()
        {
            self.first -= 1;
        }
    }

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

        assert_eq!(
            widths(&page, MaxColumnWidth::default()),
            vec![2 + MARKS, 11],
            "the heading counts, and so do the marks it carries"
        );
    }

    #[test]
    fn a_heading_wider_than_every_value_sets_the_width() {
        let page = page(&["description"], &[&[CqlValue::Text("hi".to_owned())]]);

        assert_eq!(widths(&page, MaxColumnWidth::default()), vec![11 + MARKS]);
    }

    #[test]
    fn a_column_is_capped() {
        let page = page(&["blob"], &[&[CqlValue::Text("x".repeat(100))]]);

        assert_eq!(widths(&page, MaxColumnWidth::default()), vec![30]);
    }

    #[test]
    fn the_cap_is_the_configured_one() {
        let page = page(&["blob"], &[&[CqlValue::Text("x".repeat(100))]]);
        let cap = MaxColumnWidth::try_new(48).expect("in range");

        assert_eq!(widths(&page, cap), vec![48]);
    }

    #[test]
    fn a_column_with_no_rows_is_still_as_wide_as_its_heading() {
        let page = page(&["id"], &[]);

        assert_eq!(widths(&page, MaxColumnWidth::default()), vec![2 + MARKS]);
    }

    #[test]
    fn a_page_that_fits_gives_its_leftover_to_the_truncated_column() {
        assert_eq!(spread(&[6, 30], &[6, 100], 60).as_ref(), [6, 52]);
    }

    #[test]
    fn columns_the_cap_did_not_truncate_keep_their_widths() {
        assert_eq!(spread(&[6, 12], &[6, 12], 60).as_ref(), [6, 12]);
    }

    #[test]
    fn truncated_columns_share_the_leftover_equally() {
        assert_eq!(
            spread(&[30, 6, 30], &[100, 6, 100], 81).as_ref(),
            [36, 6, 35],
            "the odd column goes to the leftmost"
        );
    }

    #[test]
    fn a_column_that_reaches_its_natural_width_passes_the_rest_on() {
        assert_eq!(
            spread(&[30, 30, 30], &[33, 100, 100], 110).as_ref(),
            [33, 37, 36]
        );
    }

    #[test]
    fn columns_too_wide_for_the_panel_are_left_to_scroll() {
        assert_eq!(spread(&[30, 30], &[100, 100], 61).as_ref(), [30, 30]);
        assert_eq!(spread(&[30, 30], &[100, 100], 62).as_ref(), [30, 30]);
    }

    #[test]
    fn natural_widths_are_uncapped() {
        let page = page(&["blob"], &[&[CqlValue::Text("x".repeat(100))]]);

        assert_eq!(natural_widths(&page), vec![100]);
    }

    #[test]
    fn the_bar_gutter_block_costs_its_own_width_and_one_spacing_column() {
        assert_eq!(available(22, 1), 22 - BAR - 1 - COLUMN_SPACING);
        assert_eq!(available(22, 4), 22 - BAR - 4 - COLUMN_SPACING);
        assert_eq!(
            available(22, 0),
            22 - BAR - COLUMN_SPACING,
            "the bar is unconditional, so it and its spacing still cost a column \
             even with no gutter beside it"
        );
        assert_eq!(available(1, 4), 0, "a body narrower than its own gutter");
    }

    #[test]
    fn only_the_columns_that_fit_are_counted() {
        // 5 + 2 + 5 + 2 + 5 = 19 for three, 12 for two.
        assert_eq!(fitting(&[5, 5, 5], 0, 19), 3);
        assert_eq!(fitting(&[5, 5, 5], 0, 18), 2);
        assert_eq!(fitting(&[5, 5, 5], 0, 12), 2);
        assert_eq!(fitting(&[5, 5, 5], 0, 11), 1);
    }

    #[test]
    fn fitting_counts_from_the_offset() {
        assert_eq!(
            fitting(&[20, 5, 5], 1, 12),
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

        scroll.follow(2, &[5, 5, 5], 12);

        assert_eq!(
            scroll.first(),
            1,
            "columns 1 and 2 fit in 12; the offset must not overshoot to 2"
        );
    }

    #[test]
    fn the_viewport_follows_a_column_to_the_left() {
        let mut scroll = ColumnScroll::default();
        scroll.follow(2, &[5, 5, 5], 12);

        scroll.follow(0, &[5, 5, 5], 12);

        assert_eq!(scroll.first(), 0);
    }

    #[test]
    fn a_column_too_wide_for_the_panel_is_shown_from_its_own_start() {
        let mut scroll = ColumnScroll::default();

        scroll.follow(1, &[5, 40], 10);

        assert_eq!(scroll.first(), 1);
    }

    /// Three fit in 19, one in 5, eleven in 75.
    const TWELVE: [u16; 12] = [5; 12];

    #[test]
    fn a_panel_that_grew_pulls_the_column_viewport_back() {
        let mut scroll = ColumnScroll::default();
        scroll.follow(11, &TWELVE, 5);
        assert_eq!(scroll.first(), 11, "only the last column fits in 5");

        scroll.clamp_to(&TWELVE, 75);

        assert_eq!(
            scroll.first(),
            1,
            "eleven columns fit in 75, so the window ends on the last one \
             rather than beside a screen of dead space"
        );
    }

    #[test]
    fn a_viewport_already_filling_the_panel_is_left_where_it_is() {
        let mut scroll = ColumnScroll::default();
        scroll.follow(11, &TWELVE, 19);
        assert_eq!(scroll.first(), 9);

        scroll.clamp_to(&TWELVE, 19);

        assert_eq!(scroll.first(), 9, "there is no dead space to reclaim");
    }

    #[test]
    fn clamping_over_a_page_that_lost_columns_returns_to_the_start() {
        let mut scroll = ColumnScroll::default();
        scroll.follow(11, &TWELVE, 5);

        scroll.clamp_to(&[5, 5], 17);

        assert_eq!(scroll.first(), 0);
    }

    #[test]
    fn clamping_over_a_page_with_no_columns_returns_to_the_start() {
        let mut scroll = ColumnScroll::default();
        scroll.follow(11, &TWELVE, 5);

        scroll.clamp_to(&[], 17);

        assert_eq!(scroll.first(), 0);
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
