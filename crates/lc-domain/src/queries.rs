//! The Queries panel: a buffer, and what the editor is offering to write into
//! it.
//!
//! The wrapper the queries editor design §2 said would earn its name once the
//! panel held more than text. Completion is that second thing: the popup is
//! panel state, not buffer state — [`Editor`] is a text buffer and knows
//! nothing of a schema — and the two have to move together, which is what this
//! type is for (design §5.4).

use crate::cql::Completion;
use crate::editor::{Editor, Position};
use crate::model::Key;

/// What an edit owes the popup that was standing over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// The word under the cursor changed. `update` offers against it again —
    /// only it can, since only it can reach the schema.
    Recompute,
    /// The cursor left the word the offers were made against, so they no
    /// longer describe anything on screen.
    Close,
    /// The press moved the popup and nothing else. The offers stand, and they
    /// still describe the word they were made against.
    Hold,
}

/// Which of the panel's two tabs is drawn.
///
/// An enum rather than a `bool`: the tabs differ in what they hold, not in a
/// flag (conventions.md §1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QueriesTab {
    /// The CQL buffer.
    #[default]
    Cql,
    /// The trail of what the session issued (query log design §5).
    Logs,
}

impl QueriesTab {
    /// The tabs in the order the title strip draws them.
    pub const ALL: [Self; 2] = [Self::Cql, Self::Logs];

    /// The next tab in [`QueriesTab::ALL`], wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Cql => Self::Logs,
            Self::Logs => Self::Cql,
        }
    }

    /// The previous tab in [`QueriesTab::ALL`], wrapping.
    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Cql => Self::Logs,
            Self::Logs => Self::Cql,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cql => "Queries",
            Self::Logs => "Logs",
        }
    }

    /// Both names are short enough that there is nothing left to drop.
    #[must_use]
    pub const fn short_label(self) -> &'static str {
        self.label()
    }
}

/// Whether the Cql tab's buffer is taking text.
///
/// The tab opens on [`CqlMode::Browsing`], where every global key keeps its
/// meaning — `4` included, which is what lets a second `4` reach Logs.
/// `Enter` or `i` opens the buffer, `Esc` closes it (cql edit mode design §2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CqlMode {
    #[default]
    Browsing,
    Editing,
}

/// The drawn tab, carrying the mode only the Cql tab has, so an editing Logs
/// tab is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    Cql(CqlMode),
    Logs,
}

impl Default for Shown {
    fn default() -> Self {
        Self::Cql(CqlMode::default())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueriesPanel {
    buffer: Editor,
    completion: Option<Completion>,
    shown: Shown,
}

impl QueriesPanel {
    /// The buffer's lines, one per screen row.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        self.buffer.lines()
    }

    /// The tab the panel is showing.
    #[must_use]
    pub const fn tab(&self) -> QueriesTab {
        match self.shown {
            Shown::Cql(_) => QueriesTab::Cql,
            Shown::Logs => QueriesTab::Logs,
        }
    }

    /// The Cql tab's mode, or `None` while Logs is drawn.
    #[must_use]
    pub const fn mode(&self) -> Option<CqlMode> {
        match self.shown {
            Shown::Cql(mode) => Some(mode),
            Shown::Logs => None,
        }
    }

    #[must_use]
    pub const fn is_editing(&self) -> bool {
        matches!(self.shown, Shown::Cql(CqlMode::Editing))
    }

    /// Draws `tab`. Arriving at the tab already drawn is not a change, so an
    /// open buffer stays open.
    pub fn show(&mut self, tab: QueriesTab) {
        if self.tab() == tab {
            return;
        }

        self.stop_editing();
        self.shown = match tab {
            QueriesTab::Cql => Shown::Cql(CqlMode::Browsing),
            QueriesTab::Logs => Shown::Logs,
        };
    }

    /// Opens the buffer for typing. Inert on the Logs tab.
    pub fn start_editing(&mut self) {
        if let Shown::Cql(mode) = &mut self.shown {
            *mode = CqlMode::Editing;
        }
    }

    /// Closes the buffer. The popup goes with it: it offers against a caret
    /// that is no longer drawn.
    pub fn stop_editing(&mut self) {
        if let Shown::Cql(mode) = &mut self.shown {
            *mode = CqlMode::Browsing;
        }
        self.completion = None;
    }

    #[must_use]
    pub fn cursor(&self) -> Position {
        self.buffer.cursor()
    }

    /// The buffer as one string, which is what a statement is built from.
    #[must_use]
    pub fn text(&self) -> String {
        self.buffer.text()
    }

    #[must_use]
    pub const fn buffer(&self) -> &Editor {
        &self.buffer
    }

    #[must_use]
    pub const fn completion(&self) -> Option<&Completion> {
        self.completion.as_ref()
    }

    /// Replaces the whole buffer, cursor at the end of it. Whatever was being
    /// offered described text that is gone.
    pub fn replace(&mut self, text: &str) {
        self.buffer.replace(text);
        self.completion = None;
    }

    /// Lays the buffer out. Answers whether anything changed, so a press that
    /// formats an already-formatted buffer costs no frame.
    pub fn format(&mut self) -> bool {
        let laid_out = crate::cql::format(&self.buffer.text());
        if laid_out == self.buffer.text() {
            return false;
        }

        self.replace(&laid_out);
        true
    }

    /// Offers `candidates`, or takes the popup down.
    pub fn offer(&mut self, completion: Option<Completion>) {
        self.completion = completion;
    }

    /// Steps the popup's selection, answering whether there was one to step.
    pub fn step(&mut self, forwards: bool) -> bool {
        let Some(completion) = self.completion.as_mut() else {
            return false;
        };

        if forwards {
            completion.next();
        } else {
            completion.previous();
        }

        true
    }

    /// Offers `key` to the buffer, answering what it did — `None` where the
    /// key is not the editor's to claim, so it keeps its global meaning, and
    /// for every key while the buffer is not open.
    ///
    /// Exhaustive over [`Key`], so a new key has to decide whether an editor
    /// claims it rather than defaulting into one side.
    pub fn edit(&mut self, key: Key, height: usize) -> Option<Refresh> {
        if !self.is_editing() {
            return None;
        }

        // Above the buffer: with a popup up, these five are the popup's
        // (design §5.5). Without one they are what they are everywhere else —
        // `Tab` moves focus, `Enter` splits the line, `Esc` closes the buffer,
        // `↑`/`↓` move the cursor — so none of them is claimed.
        //
        // `Enter` beside `Tab` because both are what accepting is spelled as
        // in the editors the audience has met, and because a shape's caret
        // lands mid-statement, where the next thing wanted is a name and not
        // a line break (completion ranking design §5).
        //
        // The vertical arrows walk the list for the same reason, and only the
        // vertical ones: `←`/`→` stay motions, which is what takes the popup
        // down.
        if let Some(completion) = self.completion.take() {
            match key {
                Key::Tab | Key::Enter => {
                    completion.accept(&mut self.buffer);
                    return Some(Refresh::Close);
                }
                Key::Escape => return Some(Refresh::Close),
                Key::Up | Key::Down => {
                    self.completion = Some(completion);
                    self.step(key == Key::Down);
                    return Some(Refresh::Hold);
                }
                _ => self.completion = Some(completion),
            }
        }

        let buffer = &mut self.buffer;

        match key {
            Key::Char(character) => buffer.insert(character),
            Key::Backspace => buffer.backspace(),
            Key::Delete => buffer.delete(),
            Key::Enter => buffer.insert_newline(),
            Key::Left => return Some(self.moved(Editor::left)),
            Key::Right => return Some(self.moved(Editor::right)),
            Key::Up => return Some(self.moved(Editor::up)),
            Key::Down => return Some(self.moved(Editor::down)),
            Key::Home => return Some(self.moved(Editor::line_start)),
            Key::End => return Some(self.moved(Editor::line_end)),
            Key::PageUp => return Some(self.paged(height, Editor::page_up)),
            Key::PageDown => return Some(self.paged(height, Editor::page_down)),
            Key::Escape | Key::Tab | Key::BackTab | Key::Insert | Key::F(_) => return None,
        }

        Some(Refresh::Recompute)
    }

    /// A motion leaves the word the offers were made against, so it takes the
    /// popup with it.
    fn moved(&mut self, motion: impl Fn(&mut Editor)) -> Refresh {
        motion(&mut self.buffer);
        self.completion = None;

        Refresh::Close
    }

    fn paged(&mut self, height: usize, motion: impl Fn(&mut Editor, usize)) -> Refresh {
        motion(&mut self.buffer, height);
        self.completion = None;

        Refresh::Close
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cql::suggest;
    use crate::model::Schema;
    use crate::tree::tests_support::schema_of;

    fn schema() -> Schema {
        schema_of(&[("shop", &["orders"])])
    }

    #[test]
    fn a_panel_opens_on_the_buffer_rather_than_the_trail() {
        assert_eq!(QueriesPanel::default().tab(), QueriesTab::Cql);
    }

    #[test]
    fn walking_the_tabs_wraps_and_stepping_back_undoes_it() {
        for tab in QueriesTab::ALL {
            assert_eq!(tab.next().previous(), tab);
        }
    }

    #[test]
    fn showing_a_tab_draws_it() {
        let mut panel = QueriesPanel::default();

        panel.show(QueriesTab::Logs);

        assert_eq!(panel.tab(), QueriesTab::Logs);
    }

    #[test]
    fn a_panel_opens_browsing_and_claims_no_key_until_opened() {
        let mut panel = QueriesPanel::default();

        assert_eq!(panel.mode(), Some(CqlMode::Browsing));
        assert_eq!(panel.edit(Key::Char('4'), 1), None);
        assert_eq!(panel.text(), "");

        panel.start_editing();

        assert_eq!(panel.edit(Key::Char('4'), 1), Some(Refresh::Recompute));
        assert_eq!(panel.text(), "4");
    }

    #[test]
    fn the_logs_tab_cannot_be_opened_for_typing() {
        let mut panel = QueriesPanel::default();
        panel.show(QueriesTab::Logs);

        panel.start_editing();

        assert_eq!(panel.mode(), None);
        assert!(!panel.is_editing());
    }

    #[test]
    fn leaving_the_cql_tab_closes_the_buffer_and_coming_back_does_not_reopen_it() {
        let mut panel = QueriesPanel::default();
        panel.start_editing();

        panel.show(QueriesTab::Logs);
        panel.show(QueriesTab::Cql);

        assert_eq!(panel.mode(), Some(CqlMode::Browsing));
    }

    #[test]
    fn showing_the_cql_tab_again_keeps_the_buffer_open() {
        let mut panel = QueriesPanel::default();
        panel.start_editing();

        panel.show(QueriesTab::Cql);

        assert!(panel.is_editing());
    }

    #[test]
    fn closing_the_buffer_takes_the_popup_down() {
        let mut panel = typed("se");

        panel.stop_editing();

        assert!(panel.completion().is_none());
    }

    fn opened() -> QueriesPanel {
        let mut panel = QueriesPanel::default();
        panel.start_editing();
        panel
    }

    /// Types `text`, offering against the schema after each character — the
    /// loop `update` runs for real.
    fn typed(text: &str) -> QueriesPanel {
        let mut panel = opened();
        let schema = schema();

        for character in text.chars() {
            let refresh = panel.edit(Key::Char(character), 1);
            assert_eq!(refresh, Some(Refresh::Recompute));
            panel.offer(suggest(panel.buffer(), Some(&schema), Some("shop")));
        }

        panel
    }

    #[test]
    fn typing_a_prefix_raises_a_popup_and_typing_past_it_takes_it_down() {
        assert!(typed("se").completion().is_some());
        assert!(
            typed("select").completion().is_none(),
            "the only offer would be the word already typed"
        );
    }

    #[test]
    fn tab_accepts_what_is_offered_and_takes_the_popup_down() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Tab, 1), Some(Refresh::Close));

        assert_eq!(panel.text(), "SELECT ");
        assert!(panel.completion().is_none());
    }

    #[test]
    fn enter_accepts_what_is_offered_and_splits_the_line_without_a_popup() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Enter, 1), Some(Refresh::Close));
        assert_eq!(panel.text(), "SELECT ");
        assert!(panel.completion().is_none());

        assert_eq!(panel.edit(Key::Enter, 1), Some(Refresh::Recompute));
        assert_eq!(
            panel.text(),
            "SELECT \n",
            "with the popup down, enter is a line break again"
        );
    }

    #[test]
    fn tab_with_no_popup_up_is_not_the_editors_to_claim() {
        let mut panel = opened();

        assert_eq!(panel.edit(Key::Tab, 1), None, "tab still moves focus");
    }

    #[test]
    fn escape_takes_the_popup_down_before_it_could_leave_the_panel() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Escape, 1), Some(Refresh::Close));
        assert!(panel.completion().is_none());

        assert_eq!(
            panel.edit(Key::Escape, 1),
            None,
            "with the popup down, escape closes the buffer again"
        );
    }

    #[test]
    fn a_motion_takes_the_popup_down() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Left, 1), Some(Refresh::Close));

        assert!(panel.completion().is_none());
    }

    #[test]
    fn the_arrows_walk_the_popup_rather_than_the_buffer() {
        let mut panel = typed("s");

        assert_eq!(panel.edit(Key::Down, 1), Some(Refresh::Hold));
        assert_eq!(panel.completion().expect("still up").selected(), 1);

        assert_eq!(panel.edit(Key::Up, 1), Some(Refresh::Hold));
        assert_eq!(panel.completion().expect("still up").selected(), 0);
    }

    #[test]
    fn walking_the_popup_leaves_the_cursor_where_it_is() {
        let mut panel = opened();
        panel.replace("SELECT *\nFROM shop.");
        assert_eq!(panel.edit(Key::Char('o'), 1), Some(Refresh::Recompute));
        panel.offer(suggest(panel.buffer(), Some(&schema()), Some("shop")));
        let cursor = panel.cursor();

        assert_eq!(panel.edit(Key::Up, 1), Some(Refresh::Hold));

        assert_eq!(panel.cursor(), cursor, "the buffer never saw the key");
        assert!(panel.completion().is_some());
    }

    #[test]
    fn an_arrow_with_no_popup_up_is_a_motion_again() {
        let mut panel = opened();
        panel.replace("SELECT *\nFROM shop.orders");

        assert_eq!(panel.edit(Key::Up, 1), Some(Refresh::Close));

        assert_eq!(panel.cursor().line, 0);
    }

    #[test]
    fn stepping_without_a_popup_does_nothing() {
        let mut panel = QueriesPanel::default();

        assert!(!panel.step(true));
    }

    #[test]
    fn formatting_an_already_formatted_buffer_changes_nothing() {
        let mut panel = QueriesPanel::default();
        panel.replace("SELECT *\nFROM t");

        assert!(
            !panel.format(),
            "no frame is owed for a press that did nothing"
        );
    }

    #[test]
    fn formatting_lays_the_buffer_out_and_leaves_the_cursor_in_it() {
        let mut panel = QueriesPanel::default();
        panel.replace("select * from shop.orders where id = 7");

        assert!(panel.format());

        assert_eq!(panel.text(), "SELECT *\nFROM shop.orders\nWHERE id = 7");
        assert_eq!(
            panel.cursor().line,
            2,
            "the cursor is inside what was written"
        );
    }
}
