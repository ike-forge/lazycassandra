//! One line of editable text with a cursor into it.
//!
//! Shared by the two overlays that take a single line: the export form's
//! destination path (export design §2.1) and the drop confirmation's typed
//! name (drop design §2.1).

/// The cursor counts characters, not bytes, so no motion can leave it inside
/// one — the invariant that keeps every slice below safe. Neither
/// [`crate::editor::Editor`] (multi-line, and a panel's) nor
/// [`crate::search::Prompt`] (appends at the end only) fits a field that
/// arrives prefilled and is edited in the middle (export design §2.1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    text: String,
    cursor: usize,
}

impl Line {
    /// Opens with the cursor at the end, where the filename is.
    #[must_use]
    pub fn new(text: String) -> Self {
        let cursor = text.chars().count();

        Self { text, cursor }
    }

    /// The line as typed.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// A character index, which is what a renderer measures a column with.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Types `character` at the cursor, which moves past it.
    pub fn insert(&mut self, character: char) {
        let at = self.byte_offset(self.cursor);
        self.text.insert(at, character);
        self.cursor += 1;
    }

    /// `Backspace`: takes the character behind the cursor, if there is one.
    pub fn delete_back(&mut self) {
        if self.cursor == 0 {
            return;
        }

        self.cursor -= 1;
        let at = self.byte_offset(self.cursor);
        self.text.remove(at);
    }

    /// `Delete`: takes the character under the cursor, if there is one.
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.len() {
            return;
        }

        let at = self.byte_offset(self.cursor);
        self.text.remove(at);
    }

    /// `←`, stopping at the start.
    pub const fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// `→`, stopping one past the last character, where typing appends.
    pub fn right(&mut self) {
        self.cursor = self.cursor.saturating_add(1).min(self.len());
    }

    /// `Home`.
    pub const fn home(&mut self) {
        self.cursor = 0;
    }

    /// `End`.
    pub fn end(&mut self) {
        self.cursor = self.len();
    }

    /// Replaces the trailing extension, if there is one. A path with no
    /// extension gains none: the export form's format radio rewrites what is
    /// there rather than asserting what should be (export design §2.1).
    pub fn set_extension(&mut self, extension: &str) {
        let last_separator = self.text.rfind(['/', '\\']).map_or(0, |at| at + 1);
        let Some(dot) = self.text[last_separator..]
            .rfind('.')
            .filter(|dot| *dot > 0)
        else {
            return;
        };

        self.text.truncate(last_separator + dot + 1);
        self.text.push_str(extension);
        self.cursor = self.len();
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// The byte offset of character `index`, or the end of the string.
    fn byte_offset(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(at, _)| at)
    }
}
