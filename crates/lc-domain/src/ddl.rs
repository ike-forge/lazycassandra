//! The DDL panel's state: a cursor over the rows it describes, a
//! viewport over the lines they wrapped to, and the search painted over them.
//!
//! A module of its own for the reason [`crate::tree`] and [`crate::contents`]
//! are ones — per-panel state with its own vocabulary rather than another
//! field on `State` (architecture.md §11).

use crate::search::Search;
use crate::state::{Cursor, Scroll, Selection};

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

    /// The next tab in [`DdlTab::ALL`], wrapping.
    ///
    /// A tab has no key of its own: the panel's number reaches the panel and,
    /// pressed again, walks its tabs — so a reader who knows how to arrive
    /// already knows how to look at the rest (docs/shortcuts.md §4).
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Ddl => Self::Columns,
            Self::Columns => Self::Ddl,
        }
    }

    /// The previous tab in [`DdlTab::ALL`], wrapping.
    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Ddl => Self::Columns,
            Self::Columns => Self::Ddl,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ddl => "DDL",
            Self::Columns => "Columns",
        }
    }

    /// The label a border too narrow for the full strip falls back to.
    ///
    /// Both names are already short enough that there is nothing left to drop:
    /// what the tabs describe is a table, and the identity rows beneath the
    /// border say which one
    /// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §4).
    #[must_use]
    pub const fn short_label(self) -> &'static str {
        self.label()
    }
}

/// The DDL panel's own state.
///
/// The cursor and the viewport count in different units — see
/// [`Span`](crate::state::Span) — which is why this panel is not in `update`'s
/// `LISTS` and follows a span rather than an index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DdlPanel {
    pub cursor: Cursor,
    pub scroll: Scroll,
    /// The rows `v` has selected, if any
    /// (docs/superpowers/specs/2026-08-14-visual-mode-design.md §3).
    pub selection: Selection,
    /// Private for the reason the search is: a tab change owes a cursor and a
    /// viewport that address the rows of the tab now drawn, and a public field
    /// would let a caller skip that repair.
    tab: DdlTab,
    /// Private for the reason [`crate::tree::TablesPanel`]'s search is:
    /// setting it is going to owe a repair, and a public field would let a
    /// caller skip it (design §2.1, §2.2).
    search: Option<Search>,
}

impl DdlPanel {
    /// The tab the panel is showing.
    #[must_use]
    pub const fn tab(&self) -> DdlTab {
        self.tab
    }

    /// Draws `tab`, putting the cursor and the viewport back at the top when
    /// that is a change.
    ///
    /// The two tabs describe different rows, so a cursor carried across would
    /// land on a row the user never chose — and on a shorter tab, past the end
    /// until the next `Msg::Measured` clamped it. The same argument holds for
    /// a range: it addresses rows of the tab that drew it, so it does not
    /// survive the tab that follows. Arriving at the tab already drawn is not
    /// a change: `a` is how focus comes back to the panel, and it may not
    /// cost the user the row they were reading or the range they had marked.
    pub fn show(&mut self, tab: DdlTab) {
        if self.tab == tab {
            return;
        }

        self.tab = tab;
        self.cursor.to_start();
        self.scroll.to_start();
        self.selection.clear();
    }

    /// The search painted over the panel's body, if any.
    #[must_use]
    pub fn search(&self) -> Option<&Search> {
        self.search.as_ref()
    }

    /// Sets the search. Not public: [`State::apply_search`](crate::state::State::apply_search)
    /// is the one caller, and the repair a change here owes is its job.
    pub(crate) fn set_search(&mut self, search: Option<Search>) {
        self.search = search;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Range, Ranges};

    #[test]
    fn a_panel_opens_on_the_tab_that_describes_the_table() {
        assert_eq!(DdlPanel::default().tab(), DdlTab::Ddl);
    }

    #[test]
    fn walking_the_tabs_wraps_and_stepping_back_undoes_it() {
        for tab in DdlTab::ALL {
            assert_eq!(tab.next().previous(), tab);
        }

        assert_eq!(
            std::iter::successors(Some(DdlTab::ALL[0]), |tab| Some(tab.next()))
                .take(DdlTab::ALL.len())
                .collect::<Vec<_>>(),
            DdlTab::ALL,
            "one press per tab returns to where it started"
        );
    }

    #[test]
    fn showing_a_tab_draws_it() {
        let mut panel = DdlPanel::default();

        panel.show(DdlTab::Columns);

        assert_eq!(panel.tab(), DdlTab::Columns);
    }

    #[test]
    fn a_tab_change_puts_the_cursor_and_the_viewport_back_at_the_top() {
        let mut panel = DdlPanel::default();
        panel.cursor.go_to(4, 8);
        panel.scroll.down(40, 10);

        panel.show(DdlTab::Columns);

        assert_eq!(panel.cursor.index(), 0);
        assert_eq!(panel.scroll.offset(), 0);
    }

    #[test]
    fn showing_the_tab_already_drawn_leaves_the_cursor_and_the_viewport_alone() {
        // The key that reaches a tab is pressed to arrive, not to start over:
        // from the DDL tab, `d` is how focus returns to the panel.
        let mut panel = DdlPanel::default();
        panel.cursor.go_to(4, 8);
        panel.scroll.down(40, 10);

        panel.show(DdlTab::Ddl);

        assert_eq!(panel.cursor.index(), 4);
        assert_eq!(panel.scroll.offset(), 1);
    }

    #[test]
    fn a_tab_change_clears_the_range() {
        let mut panel = DdlPanel::default();
        panel.selection.start(1);
        panel.selection.stop(3);

        panel.show(DdlTab::Columns);

        assert!(!panel.selection.is_active());
    }

    #[test]
    fn arriving_at_the_tab_already_drawn_leaves_the_range_alone() {
        let mut panel = DdlPanel::default();
        panel.selection.start(1);
        panel.selection.stop(3);

        panel.show(DdlTab::Ddl);

        assert_eq!(panel.selection.ranges(0), Ranges::of(Range::new(1, 3)));
    }

    #[test]
    fn a_tab_change_leaves_the_search_where_it_was() {
        // The search is applied to the panel, not to a tab: it paints
        // whichever rows the panel is drawing, and clearing it is `Esc`'s job.
        let mut panel = DdlPanel::default();
        panel.set_search(Search::new("id"));

        panel.show(DdlTab::Columns);

        assert_eq!(panel.search().map(Search::as_str), Some("id"));
    }
}
