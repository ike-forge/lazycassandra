//! The Table DDL panel's state: a cursor over the rows it describes, a
//! viewport over the lines they wrapped to, and the filter narrowing them.
//!
//! A module of its own for the reason [`crate::tree`] and [`crate::contents`]
//! are ones — per-panel state with its own vocabulary rather than another
//! field on `State` (architecture.md §11).

use crate::search::Filter;
use crate::state::{Cursor, Scroll};

/// Which of the panel's two tabs is drawn.
///
/// An enum rather than a `bool` on the panel: the tabs differ in what they
/// describe, not in a flag, and a third one (indexes, views) would be an arm
/// here rather than a second boolean (conventions.md §1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DdlTab {
    /// The physical shape and the table's options.
    #[default]
    Ddl,
    /// Every column the table has, whatever part of the key it plays.
    Columns,
}

impl DdlTab {
    /// The tabs in the order the title strip draws them.
    pub const ALL: [Self; 2] = [Self::Ddl, Self::Columns];

    /// The next tab, wrapping — what pressing the panel's own shortcut does
    /// once the panel already has focus.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Ddl => Self::Columns,
            Self::Columns => Self::Ddl,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ddl => "Table DDL",
            Self::Columns => "Table columns",
        }
    }

    /// The label a border too narrow for the full strip falls back to.
    ///
    /// It drops the noun the panel's own name already supplies, which is what
    /// lets both tabs stay on a sidebar that cannot hold both full names
    /// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §4).
    #[must_use]
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Ddl => "Table DDL",
            Self::Columns => "Columns",
        }
    }
}

/// The Table DDL panel's own state.
///
/// The cursor and the viewport count in different units — see
/// [`Span`](crate::state::Span) — which is why this panel is not in `update`'s
/// `LISTS` and follows a span rather than an index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DdlPanel {
    pub cursor: Cursor,
    pub scroll: Scroll,
    /// Private for the reason the filter is: a tab change owes a cursor and a
    /// viewport that address the rows of the tab now drawn, and a public field
    /// would let a caller skip that repair.
    tab: DdlTab,
    /// Private for the reason [`crate::tree::TablesPanel`]'s filter is:
    /// setting it is going to owe a repair, and a public field would let a
    /// caller skip it (design §2.1, §2.2).
    filter: Option<Filter>,
}

impl DdlPanel {
    /// The tab the panel is showing.
    #[must_use]
    pub const fn tab(&self) -> DdlTab {
        self.tab
    }

    /// Moves to the next tab, putting the cursor and the viewport back at the
    /// top.
    ///
    /// The two tabs describe different rows, so a cursor carried across would
    /// land on a row the user never chose — and on a shorter tab, past the end
    /// until the next `Msg::Measured` clamped it.
    pub const fn cycle(&mut self) {
        self.tab = self.tab.next();
        self.cursor.to_start();
        self.scroll.to_start();
    }

    /// The filter narrowing the panel's body, if any.
    #[must_use]
    pub fn filter(&self) -> Option<&Filter> {
        self.filter.as_ref()
    }

    /// Sets the filter. Not public: [`State::apply_filter`](crate::state::State::apply_filter)
    /// is the one caller, and the repair a change here owes is its job.
    pub(crate) fn set_filter(&mut self, filter: Option<Filter>) {
        self.filter = filter;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_opens_on_the_tab_that_describes_the_table() {
        assert_eq!(DdlPanel::default().tab(), DdlTab::Ddl);
    }

    #[test]
    fn cycling_walks_every_tab_and_comes_back() {
        let mut panel = DdlPanel::default();
        let walked: Vec<DdlTab> = DdlTab::ALL
            .iter()
            .map(|_| {
                panel.cycle();
                panel.tab()
            })
            .collect();

        assert_eq!(walked, [DdlTab::Columns, DdlTab::Ddl]);
    }

    #[test]
    fn a_tab_change_puts_the_cursor_and_the_viewport_back_at_the_top() {
        let mut panel = DdlPanel::default();
        panel.cursor.go_to(4, 8);
        panel.scroll.down(40, 10);

        panel.cycle();

        assert_eq!(panel.cursor.index(), 0);
        assert_eq!(panel.scroll.offset(), 0);
    }

    #[test]
    fn a_tab_change_leaves_the_filter_where_it_was() {
        // The filter is applied to the panel, not to a tab: it narrows
        // whichever rows the panel is drawing, and clearing it is `Esc`'s job.
        let mut panel = DdlPanel::default();
        panel.set_filter(Filter::new("id"));

        panel.cycle();

        assert_eq!(panel.filter().map(Filter::as_str), Some("id"));
    }
}
