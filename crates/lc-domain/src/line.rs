//! One editable line with a cursor, for the export path and the typed drop
//! name.

/// The cursor counts characters, which keeps every slice on a boundary. Neither
/// `Editor` nor `Prompt` fits a prefilled field edited mid-line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    text: String,
    cursor: usize,
}

impl Line {
    /// Cursor at the end, where the filename is.
    #[must_use]
    pub fn new(text: String) -> Self {
        let cursor = text.chars().count();

        Self { text, cursor }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// A character index.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn insert(&mut self, character: char) {
        let at = self.byte_offset(self.cursor);
        self.text.insert(at, character);
        self.cursor += 1;
    }

    pub fn delete_back(&mut self) {
        if self.cursor == 0 {
            return;
        }

        self.cursor -= 1;
        let at = self.byte_offset(self.cursor);
        self.text.remove(at);
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.len() {
            return;
        }

        let at = self.byte_offset(self.cursor);
        self.text.remove(at);
    }

    /// Clamped to the end.
    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.len());
    }

    pub const fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = self.cursor.saturating_add(1).min(self.len());
    }

    pub const fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.len();
    }

    /// A path with no extension gains none.
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

    fn byte_offset(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(at, _)| at)
    }
}
