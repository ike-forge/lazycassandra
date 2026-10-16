//! Row ranges and the multi-group selection built from them
//! (docs/superpowers/specs/2026-08-14-multi-group-selection-design.md).

/// An inclusive run of rows, which cannot be built the wrong way round.
///
/// `first <= last` by construction, so nothing downstream compares the two
/// ends before drawing between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    first: usize,
    last: usize,
}

impl Range {
    /// The run between the two ends, in either order.
    #[must_use]
    pub const fn new(anchor: usize, cursor: usize) -> Self {
        if anchor <= cursor {
            Self {
                first: anchor,
                last: cursor,
            }
        } else {
            Self {
                first: cursor,
                last: anchor,
            }
        }
    }

    #[must_use]
    pub const fn first(self) -> usize {
        self.first
    }

    #[must_use]
    pub const fn last(self) -> usize {
        self.last
    }

    #[must_use]
    pub const fn contains(self, index: usize) -> bool {
        self.first <= index && index <= self.last
    }

    /// How many rows the run holds. Never zero — which is why this is not
    /// `len`, whose `is_empty` counterpart would answer a question a range
    /// cannot ask.
    #[must_use]
    pub const fn rows(self) -> usize {
        self.last - self.first + 1
    }
}

/// Inclusive runs of rows, sorted, disjoint and never adjacent.
///
/// Normalised on the way in and never afterwards: [`Ranges::insert`] absorbs
/// every group the new one overlaps *or touches*, because two groups meeting
/// at consecutive rows are one band on screen and a set that could hold both
/// spellings would let a renderer draw a seam nothing explains. Sorted, so a
/// yank reads rows in the order they are drawn without sorting them itself
/// (docs/superpowers/specs/2026-08-14-multi-group-selection-design.md §2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ranges(Vec<Range>);

impl Ranges {
    #[must_use]
    pub fn of(range: Range) -> Self {
        Self(vec![range])
    }

    pub fn insert(&mut self, range: Range) {
        let mut first = range.first;
        let mut last = range.last;
        let mut kept: Vec<Range> = Vec::with_capacity(self.0.len() + 1);
        let mut placed = false;

        for held in self.0.drain(..) {
            if held.first <= last.saturating_add(1) && first <= held.last.saturating_add(1) {
                first = first.min(held.first);
                last = last.max(held.last);
            } else if held.last < first {
                kept.push(held);
            } else {
                if !placed {
                    kept.push(Range { first, last });
                    placed = true;
                }

                kept.push(held);
            }
        }

        if !placed {
            kept.push(Range { first, last });
        }

        self.0 = kept;
    }

    /// Drops the group holding `index`, answering with it.
    pub fn remove_holding(&mut self, index: usize) -> Option<Range> {
        let position = self.0.iter().position(|group| group.contains(index))?;

        Some(self.0.remove(position))
    }

    #[must_use]
    pub fn contains(&self, index: usize) -> bool {
        self.0.iter().any(|group| group.contains(index))
    }

    /// How many rows are selected across every group.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.0.iter().map(|group| group.rows()).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn groups(&self) -> impl Iterator<Item = Range> + '_ {
        self.0.iter().copied()
    }

    /// Every selected row, in the order the rows are drawn.
    pub fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().flat_map(|group| group.first..=group.last)
    }

    #[must_use]
    pub fn first(&self) -> Option<Range> {
        self.0.first().copied()
    }

    /// Pulls the groups back over a list that shrank, dropping the ones whose
    /// first row has gone.
    pub fn clamp_to(&mut self, len: usize) {
        let Some(last) = len.checked_sub(1) else {
            self.0.clear();
            return;
        };

        self.0.retain(|group| group.first <= last);

        for group in &mut self.0 {
            if group.last > last {
                group.last = last;
            }
        }
    }

    /// Every group moved `by` rows later, keeping their width and their
    /// order.
    ///
    /// For a projection whose rows do not start where the ranges' own
    /// indices do — the trail's dropped-count row is one such prefix — so
    /// the shift lives in one place a caller cannot apply to only one of a
    /// pair of related values (query log design §6).
    #[must_use]
    pub fn shifted(&self, by: usize) -> Self {
        Self(
            self.0
                .iter()
                .map(|group| Range {
                    first: group.first + by,
                    last: group.last + by,
                })
                .collect(),
        )
    }
}

/// The rows a panel has selected, as groups, and whether the cursor is still
/// extending one of them
/// (docs/superpowers/specs/2026-08-14-multi-group-selection-design.md §2).
///
/// While extending, the far end of the live group is the panel's own cursor
/// rather than a second index stored here: a group that is a function of the
/// cursor cannot fall out of step with it. Freezing is the one moment the
/// derived group is written down, and it is written down exactly because the
/// cursor is about to stop meaning it.
///
/// The anchor is kept beside the groups and outlives the extension. The two
/// are not the same question — the groups say which rows are selected, the
/// anchor says which row the user last pressed `v` on — and `y` sends the
/// cursor home to the anchor (visual mode design §5.1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    groups: Ranges,
    anchor: Option<usize>,
    extending: bool,
}

impl Selection {
    /// Opens a group at `cursor` and starts extending it, keeping what is
    /// already held.
    pub fn start(&mut self, cursor: usize) {
        self.anchor = Some(cursor);
        self.extending = true;
    }

    /// Freezes the group the cursor is making, merging it into what is held.
    pub fn stop(&mut self, cursor: usize) {
        if !self.extending {
            return;
        }

        if let Some(anchor) = self.anchor {
            self.groups.insert(Range::new(anchor, cursor));
        }

        self.extending = false;
    }

    /// `v`: freeze what is being extended, drop the group under the cursor, or
    /// open a new group there.
    ///
    /// The third arm pays for the first. Accumulating means `v` no longer
    /// discards, so without a way to drop a group the only way back from a
    /// mis-added one would be to clear everything — and the row to press it on
    /// is the row the user is already looking at
    /// (multi-group selection design §1).
    pub fn toggle(&mut self, cursor: usize) {
        if self.extending {
            self.stop(cursor);
            return;
        }

        if let Some(dropped) = self.groups.remove_holding(cursor) {
            if self.anchor.is_some_and(|anchor| dropped.contains(anchor)) {
                self.anchor = None;
            }

            return;
        }

        self.start(cursor);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    #[must_use]
    pub const fn is_extending(&self) -> bool {
        self.extending
    }

    /// Whether anything is selected at all.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.extending || !self.groups.is_empty()
    }

    /// The rows selected, given where the cursor is now. The one way out, so
    /// no caller can read the groups and forget the cursor, and none is handed
    /// a seam between a group and the extension running into it.
    #[must_use]
    pub fn ranges(&self, cursor: usize) -> Ranges {
        let mut ranges = self.groups.clone();

        if let (true, Some(anchor)) = (self.extending, self.anchor) {
            ranges.insert(Range::new(anchor, cursor));
        }

        ranges
    }

    /// The row `v` was last pressed on, which `y` sends the cursor back to.
    #[must_use]
    pub const fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// Pulls a selection back over a list that shrank.
    pub fn clamp_to(&mut self, len: usize) {
        // Found before `self.groups` is clamped: afterwards every surviving
        // group is a candidate to "contain" the pulled-back anchor, even one
        // that never held it (multi-group design §2).
        let held = self
            .anchor
            .and_then(|anchor| self.groups.groups().find(|group| group.contains(anchor)));

        self.groups.clamp_to(len);

        let Some(last) = len.checked_sub(1) else {
            self.clear();
            return;
        };

        self.anchor = match self.anchor {
            Some(anchor) if self.extending => Some(anchor.min(last)),
            // Frozen, the anchor is pulled back the same way its own group
            // is; only that group dropping outright takes the anchor with it
            // (design §2).
            Some(anchor) => held.and_then(|group| {
                (group.first() <= last).then(|| anchor.min(group.last().min(last)))
            }),
            None => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_the_two_ends_in_order_whichever_way_it_was_made() {
        let down = Range::new(2, 5);
        let up = Range::new(5, 2);

        assert_eq!((down.first(), down.last()), (2, 5));
        assert_eq!(
            (up.first(), up.last()),
            (2, 5),
            "extending upwards is the same range"
        );
        assert_eq!(down.rows(), 4);
        assert!(down.contains(2) && down.contains(5) && !down.contains(6));
    }

    #[test]
    fn freezing_writes_down_the_range_the_cursor_made() {
        let mut selection = Selection::default();
        selection.start(5);

        assert_eq!(
            selection.ranges(2),
            Ranges::of(Range::new(2, 5)),
            "while extending, the far end is the cursor"
        );

        selection.stop(2);

        assert_eq!(
            selection.ranges(9),
            Ranges::of(Range::new(2, 5)),
            "frozen, the cursor walks free"
        );
        assert_eq!(selection.anchor(), Some(5), "the anchor is where v was");
    }

    #[test]
    fn an_extension_running_into_a_group_shows_no_seam() {
        let mut selection = Selection::default();
        selection.start(0);
        selection.stop(2);
        selection.start(4);

        assert_eq!(
            selection.ranges(3),
            Ranges::of(Range::new(0, 4)),
            "the extension touched the frozen group, so the two are one"
        );
    }

    #[test]
    fn nothing_selected_is_an_empty_set_and_no_anchor() {
        let selection = Selection::default();

        assert!(selection.ranges(4).is_empty());
        assert!(!selection.is_active());
        assert_eq!(selection.anchor(), None);
    }

    #[test]
    fn clamping_pulls_an_extending_anchor_back_into_the_list() {
        let mut selection = Selection::default();
        selection.start(9);

        selection.clamp_to(5);

        assert_eq!(selection.anchor(), Some(4));
        assert_eq!(selection.ranges(0), Ranges::of(Range::new(0, 4)));
    }

    #[test]
    fn clamping_away_the_group_holding_the_anchor_takes_the_anchor() {
        // `y` sends the cursor home to the anchor. An anchor whose rows are
        // gone would send it somewhere the user never was
        // (multi-group selection design §3).
        let mut selection = Selection::default();
        selection.start(8);
        selection.stop(9);

        selection.clamp_to(4);

        assert!(selection.ranges(0).is_empty());
        assert_eq!(selection.anchor(), None);
    }

    #[test]
    fn clamping_a_frozen_group_pulls_its_surviving_anchor_back_too() {
        // The group and the anchor are pulled back the same way; only a
        // group that is dropped outright takes its anchor with it
        // (multi-group selection design §2).
        let mut selection = Selection::default();
        selection.start(9);
        selection.stop(2);

        selection.clamp_to(5);

        assert_eq!(selection.ranges(0), Ranges::of(Range::new(2, 4)));
        assert_eq!(selection.anchor(), Some(4));
    }

    #[test]
    fn clamping_away_the_anchors_group_does_not_hand_it_to_a_survivor() {
        // Two frozen groups; the anchor's own group is dropped by the clamp,
        // but a surviving group happens to contain the pulled-back index too.
        // The anchor must not be adopted by a group that never held it
        // (multi-group selection design §2, §3).
        let mut selection = Selection::default();
        selection.start(0);
        selection.stop(4);
        selection.start(7);
        selection.stop(8);

        selection.clamp_to(5);

        assert_eq!(selection.ranges(0), Ranges::of(Range::new(0, 4)));
        assert_eq!(selection.anchor(), None);
    }

    #[test]
    fn v_freezes_then_opens_a_second_group_then_drops_one() {
        let mut selection = Selection::default();
        selection.toggle(0);
        selection.toggle(1);

        assert_eq!(selection.ranges(1), Ranges::of(Range::new(0, 1)));

        selection.toggle(3);
        selection.toggle(3);

        let mut both = Ranges::of(Range::new(0, 1));
        both.insert(Range::new(3, 3));
        assert_eq!(selection.ranges(3), both, "the first group survived");

        selection.toggle(0);

        assert_eq!(
            selection.ranges(0),
            Ranges::of(Range::new(3, 3)),
            "v on a held row drops the group holding it"
        );
    }

    #[test]
    fn dropping_the_group_holding_the_anchor_takes_the_anchor() {
        // Homing `y` to a group the user just dropped would send the cursor
        // where they never were (design §3).
        let mut selection = Selection::default();
        selection.toggle(2);
        selection.toggle(4);

        assert_eq!(selection.anchor(), Some(2));

        selection.toggle(3);

        assert_eq!(selection.anchor(), None);
        assert!(!selection.is_active());
    }

    #[test]
    fn dropping_another_group_keeps_the_anchor() {
        let mut selection = Selection::default();
        selection.toggle(0);
        selection.toggle(0);
        selection.toggle(5);
        selection.toggle(5);

        selection.toggle(0);

        assert_eq!(
            selection.anchor(),
            Some(5),
            "the anchor's group still stands"
        );
    }

    #[test]
    fn groups_that_overlap_or_merely_touch_become_one() {
        // Two groups meeting at consecutive rows are one band on screen, and a
        // set holding both spellings would let a renderer draw a seam the user
        // cannot explain (multi-group selection design §2).
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 2));
        ranges.insert(Range::new(2, 4));

        assert_eq!(ranges.groups().collect::<Vec<_>>(), vec![Range::new(0, 4)]);

        let mut touching = Ranges::default();
        touching.insert(Range::new(0, 2));
        touching.insert(Range::new(3, 5));

        assert_eq!(
            touching.groups().collect::<Vec<_>>(),
            vec![Range::new(0, 5)],
            "3 follows 2, so the two are one band"
        );
    }

    #[test]
    fn a_gap_of_one_row_keeps_two_groups() {
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 1));
        ranges.insert(Range::new(3, 4));

        assert_eq!(
            ranges.groups().collect::<Vec<_>>(),
            vec![Range::new(0, 1), Range::new(3, 4)]
        );
        assert_eq!(ranges.rows(), 4);
        assert!(!ranges.contains(2), "the gap is not selected");
        assert!(ranges.contains(0) && ranges.contains(4));
    }

    #[test]
    fn the_order_groups_arrived_in_does_not_show() {
        let mut forwards = Ranges::default();
        forwards.insert(Range::new(0, 1));
        forwards.insert(Range::new(5, 6));
        forwards.insert(Range::new(3, 3));

        let mut backwards = Ranges::default();
        backwards.insert(Range::new(3, 3));
        backwards.insert(Range::new(5, 6));
        backwards.insert(Range::new(0, 1));

        assert_eq!(forwards, backwards);
        assert_eq!(
            forwards.indices().collect::<Vec<_>>(),
            vec![0, 1, 3, 5, 6],
            "indices come out in the order the rows are drawn"
        );
    }

    #[test]
    fn a_group_bridging_two_others_swallows_both() {
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 1));
        ranges.insert(Range::new(5, 6));
        ranges.insert(Range::new(1, 5));

        assert_eq!(ranges.groups().collect::<Vec<_>>(), vec![Range::new(0, 6)]);
    }

    #[test]
    fn removing_answers_with_the_group_that_held_the_row() {
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 1));
        ranges.insert(Range::new(4, 6));

        assert_eq!(ranges.remove_holding(5), Some(Range::new(4, 6)));
        assert_eq!(ranges.groups().collect::<Vec<_>>(), vec![Range::new(0, 1)]);
        assert_eq!(ranges.remove_holding(3), None, "no group holds row 3");
    }

    #[test]
    fn clamping_drops_only_the_groups_whose_head_is_gone() {
        // A group whose first row has gone no longer addresses anything the
        // user pointed at; one whose tail was truncated still does (visual
        // mode design §2, applied per group).
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 1));
        ranges.insert(Range::new(4, 8));
        ranges.insert(Range::new(10, 12));

        ranges.clamp_to(6);

        assert_eq!(
            ranges.groups().collect::<Vec<_>>(),
            vec![Range::new(0, 1), Range::new(4, 5)]
        );

        ranges.clamp_to(0);

        assert!(ranges.is_empty(), "an empty list selects nothing");
    }

    #[test]
    fn shifting_moves_every_group_by_the_same_amount() {
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 1));
        ranges.insert(Range::new(4, 6));

        assert_eq!(
            ranges.shifted(1).groups().collect::<Vec<_>>(),
            vec![Range::new(1, 2), Range::new(5, 7)]
        );
        assert_eq!(
            ranges.shifted(0),
            ranges,
            "no shift leaves the groups exactly as they were"
        );
    }
}
