//! The DDL panel's state: a cursor over the rows it describes, a
//! viewport over the lines they wrapped to, and the filter narrowing them.
//!
//! A module of its own for the reason [`crate::tree`] and [`crate::contents`]
//! are ones — per-panel state with its own vocabulary rather than another
//! field on `State` (architecture.md §11).

use crate::search::Filter;
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

    /// The key that reaches this tab, from any panel that does not take text.
    ///
    /// A tab is a place, so it gets a key of its own rather than a second
    /// meaning for the panel's — which is why `Ddl` answers to the panel's own
    /// letter and `Columns` to its own leading one (docs/shortcuts.md §4).
    #[must_use]
    pub const fn shortcut(self) -> char {
        match self {
            Self::Ddl => 'd',
            Self::Columns => 'c',
        }
    }

    #[must_use]
    pub fn from_shortcut(key: char) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.shortcut() == key)
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
    use crate::state::Range;

    #[test]
    fn a_panel_opens_on_the_tab_that_describes_the_table() {
        assert_eq!(DdlPanel::default().tab(), DdlTab::Ddl);
    }

    #[test]
    fn each_tab_answers_to_the_key_the_shortcuts_doc_names() {
        assert_eq!(
            DdlTab::ALL.map(DdlTab::shortcut),
            ['d', 'c'],
            "the keys are a promise to the user's fingers, not an implementation detail"
        );
    }

    #[test]
    fn a_tab_key_leads_back_to_the_tab_that_claims_it() {
        for tab in DdlTab::ALL {
            assert_eq!(DdlTab::from_shortcut(tab.shortcut()), Some(tab));
        }

        assert_eq!(DdlTab::from_shortcut('z'), None);
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

        assert_eq!(panel.selection.range(0), None);
    }

    #[test]
    fn arriving_at_the_tab_already_drawn_leaves_the_range_alone() {
        let mut panel = DdlPanel::default();
        panel.selection.start(1);
        panel.selection.stop(3);

        panel.show(DdlTab::Ddl);

        assert_eq!(panel.selection.range(0), Some(Range::new(1, 3)));
    }

    #[test]
    fn a_tab_change_leaves_the_filter_where_it_was() {
        // The filter is applied to the panel, not to a tab: it narrows
        // whichever rows the panel is drawing, and clearing it is `Esc`'s job.
        let mut panel = DdlPanel::default();
        panel.set_filter(Filter::new("id"));

        panel.show(DdlTab::Columns);

        assert_eq!(panel.filter().map(Filter::as_str), Some("id"));
    }
}
