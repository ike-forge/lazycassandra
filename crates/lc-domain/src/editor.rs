//! The Queries panel's text buffer.
//!
//! Written here rather than taken from a widget crate because the domain is
//! what builds a `Statement` from this text, gates it on `Posture`, and will
//! match it against `Schema` for completion
//! (doc/specs/2026-08-09-queries-editor-design.md §1).

/// A multi-line text buffer with one cursor.
///
/// Two invariants, held by every mutator rather than checked by callers: the
/// buffer is never empty, and the cursor is always inside it. Both fields are
/// private so neither can be broken from outside this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    lines: Vec<String>,
    cursor: Position,
    desired_column: usize,
}

/// Where the cursor is.
///
/// `column` counts `char`s. Not bytes, which would put a char-boundary
/// invariant on every edit; not display width, which is the renderer's unit
/// and the reason `unicode-width` is not a dependency of this crate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Default for Editor {
    /// One empty line, so there is no "no current line" state for a cursor to
    /// point at.
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            cursor: Position::default(),
            desired_column: 0,
        }
    }
}

impl Editor {
    /// The buffer's lines, one per screen row — the renderer does not wrap.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    #[must_use]
    pub const fn cursor(&self) -> Position {
        self.cursor
    }

    /// The buffer as one string, which is what a statement is built from.
    #[must_use]
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Inserts `character` at the cursor and steps over it.
    pub fn insert(&mut self, character: char) {
        let at = self.byte_offset();
        self.line_mut().insert(at, character);
        self.cursor.column += 1;
        self.settle();
    }

    /// Splits the line at the cursor, which lands at the start of the new one.
    pub fn insert_newline(&mut self) {
        let at = self.byte_offset();
        let tail = self.line_mut().split_off(at);
        self.cursor.line += 1;
        self.cursor.column = 0;
        self.lines.insert(self.cursor.line, tail);
        self.settle();
    }

    /// Deletes the character before the cursor. At the start of a line that is
    /// the line break, so the line joins the one above; at the start of the
    /// buffer there is nothing to delete.
    pub fn backspace(&mut self) {
        if self.cursor.column > 0 {
            self.cursor.column -= 1;
            let at = self.byte_offset();
            self.line_mut().remove(at);
        } else if self.cursor.line > 0 {
            let line = self.lines.remove(self.cursor.line);
            self.cursor.line -= 1;
            self.cursor.column = self.line_len();
            self.line_mut().push_str(&line);
        }

        self.settle();
    }

    /// Deletes the character at the cursor. At the end of a line that is the
    /// line break, so the next line comes up; at the end of the buffer there
    /// is nothing to delete.
    pub fn delete(&mut self) {
        if self.cursor.column < self.line_len() {
            let at = self.byte_offset();
            self.line_mut().remove(at);
        } else if self.cursor.line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor.line + 1);
            self.line_mut().push_str(&next);
        }

        self.settle();
    }

    /// Characters on the cursor's line, which is also the largest column the
    /// cursor may hold on it.
    fn line_len(&self) -> usize {
        self.lines[self.cursor.line].chars().count()
    }

    /// The cursor's column as a byte index into its own line.
    ///
    /// Past the last character it is the line's length: `char_indices` has no
    /// entry for the position after the end, which is where the cursor sits
    /// whenever it is at end-of-line.
    fn byte_offset(&self) -> usize {
        let line = &self.lines[self.cursor.line];

        line.char_indices()
            .nth(self.cursor.column)
            .map_or(line.len(), |(at, _)| at)
    }

    fn line_mut(&mut self) -> &mut String {
        &mut self.lines[self.cursor.line]
    }

    /// Adopts the cursor's column as the one vertical motion should aim for.
    /// Everything but a vertical motion calls this.
    fn settle(&mut self) {
        self.desired_column = self.cursor.column;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Types `text` from a fresh buffer, `\n` meaning Enter.
    fn typed(text: &str) -> Editor {
        let mut editor = Editor::default();
        for character in text.chars() {
            if character == '\n' {
                editor.insert_newline();
            } else {
                editor.insert(character);
            }
        }
        editor
    }

    /// Places the cursor directly, so these tests do not depend on motions.
    fn at(editor: &mut Editor, line: usize, column: usize) {
        editor.cursor = Position { line, column };
        editor.desired_column = column;
    }

    #[test]
    fn a_new_buffer_is_one_empty_line() {
        let editor = Editor::default();

        assert_eq!(editor.lines(), [String::new()]);
        assert_eq!(editor.cursor(), Position { line: 0, column: 0 });
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn typing_advances_the_cursor_one_column_per_character() {
        let editor = typed("SELECT");

        assert_eq!(editor.text(), "SELECT");
        assert_eq!(editor.cursor(), Position { line: 0, column: 6 });
    }

    #[test]
    fn typing_inside_a_line_pushes_the_rest_along() {
        let mut editor = typed("SELECT t");
        at(&mut editor, 0, 7);

        editor.insert('*');

        assert_eq!(editor.text(), "SELECT *t");
        assert_eq!(editor.cursor().column, 8);
    }

    #[test]
    fn enter_splits_the_line_at_the_cursor() {
        let mut editor = typed("SELECT * FROM t");
        at(&mut editor, 0, 9);

        editor.insert_newline();

        assert_eq!(editor.lines(), ["SELECT * ".to_owned(), "FROM t".to_owned()]);
        assert_eq!(editor.cursor(), Position { line: 1, column: 0 });
    }

    #[test]
    fn enter_at_the_end_of_a_line_opens_an_empty_one() {
        let editor = typed("SELECT\n");

        assert_eq!(editor.lines(), ["SELECT".to_owned(), String::new()]);
        assert_eq!(editor.cursor(), Position { line: 1, column: 0 });
    }

    #[test]
    fn text_joins_the_lines_with_newlines() {
        assert_eq!(typed("SELECT *\nFROM t").text(), "SELECT *\nFROM t");
    }

    #[test]
    fn a_multibyte_character_is_one_column() {
        let editor = typed("é日");

        assert_eq!(
            editor.cursor().column,
            2,
            "columns count chars, not bytes or terminal cells"
        );
        assert_eq!(editor.text(), "é日");
    }

    #[test]
    fn backspace_removes_the_character_before_the_cursor() {
        let mut editor = typed("SELECTX");

        editor.backspace();

        assert_eq!(editor.text(), "SELECT");
        assert_eq!(editor.cursor().column, 6);
    }

    #[test]
    fn backspace_at_the_start_of_a_line_joins_it_to_the_one_above() {
        let mut editor = typed("SELECT *\nFROM t");
        at(&mut editor, 1, 0);

        editor.backspace();

        assert_eq!(editor.text(), "SELECT *FROM t");
        assert_eq!(
            editor.cursor(),
            Position { line: 0, column: 8 },
            "the cursor lands on the seam"
        );
    }

    #[test]
    fn backspace_at_the_start_of_the_buffer_does_nothing() {
        let mut editor = typed("SELECT");
        at(&mut editor, 0, 0);

        editor.backspace();

        assert_eq!(editor.text(), "SELECT");
        assert_eq!(editor.cursor(), Position { line: 0, column: 0 });
    }

    #[test]
    fn delete_removes_the_character_at_the_cursor() {
        let mut editor = typed("SXELECT");
        at(&mut editor, 0, 1);

        editor.delete();

        assert_eq!(editor.text(), "SELECT");
        assert_eq!(editor.cursor().column, 1, "the cursor does not move");
    }

    #[test]
    fn delete_at_the_end_of_a_line_pulls_the_next_one_up() {
        let mut editor = typed("SELECT *\nFROM t");
        at(&mut editor, 0, 8);

        editor.delete();

        assert_eq!(editor.text(), "SELECT *FROM t");
        assert_eq!(editor.lines().len(), 1);
    }

    #[test]
    fn delete_at_the_end_of_the_buffer_does_nothing() {
        let mut editor = typed("SELECT");

        editor.delete();

        assert_eq!(editor.text(), "SELECT");
        assert_eq!(editor.cursor().column, 6);
    }

    #[test]
    fn a_multibyte_character_deletes_whole() {
        let mut editor = typed("aé日");

        editor.backspace();

        assert_eq!(editor.text(), "aé", "a char is removed, not a byte");
        assert_eq!(editor.cursor().column, 2);
    }
}
