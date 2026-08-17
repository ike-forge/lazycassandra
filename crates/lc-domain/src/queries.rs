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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueriesPanel {
    buffer: Editor,
    completion: Option<Completion>,
}

impl QueriesPanel {
    /// The buffer's lines, one per screen row.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        self.buffer.lines()
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
    /// key is not the editor's to claim, so it keeps its global meaning.
    ///
    /// Exhaustive over [`Key`], so a new key has to decide whether an editor
    /// claims it rather than defaulting into one side.
    pub fn edit(&mut self, key: Key, height: usize) -> Option<Refresh> {
        // Above the buffer: with a popup up, these three are the popup's
        // (design §5.5). Without one they are what they are everywhere else —
        // `Tab` moves focus, `Enter` splits the line, `Esc` leaves the panel —
        // so none of them is claimed.
        //
        // `Enter` beside `Tab` because both are what accepting is spelled as
        // in the editors the audience has met, and because a shape's caret
        // lands mid-statement, where the next thing wanted is a name and not
        // a line break (completion ranking design §5).
        if let Some(completion) = self.completion.take() {
            match key {
                Key::Tab | Key::Enter => {
                    completion.accept(&mut self.buffer);
                    return Some(Refresh::Close);
                }
                Key::Escape => return Some(Refresh::Close),
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

    /// Types `text`, offering against the schema after each character — the
    /// loop `update` runs for real.
    fn typed(text: &str) -> QueriesPanel {
        let mut panel = QueriesPanel::default();
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

        assert_eq!(panel.text(), "SELECT");
        assert!(panel.completion().is_none());
    }

    #[test]
    fn enter_accepts_what_is_offered_and_splits_the_line_without_a_popup() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Enter, 1), Some(Refresh::Close));
        assert_eq!(panel.text(), "SELECT");
        assert!(panel.completion().is_none());

        assert_eq!(panel.edit(Key::Enter, 1), Some(Refresh::Recompute));
        assert_eq!(
            panel.text(),
            "SELECT\n",
            "with the popup down, enter is a line break again"
        );
    }

    #[test]
    fn tab_with_no_popup_up_is_not_the_editors_to_claim() {
        let mut panel = QueriesPanel::default();

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
            "with the popup down, escape is the way out of the panel again"
        );
    }

    #[test]
    fn a_motion_takes_the_popup_down() {
        let mut panel = typed("se");

        assert_eq!(panel.edit(Key::Left, 1), Some(Refresh::Close));

        assert!(panel.completion().is_none());
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
