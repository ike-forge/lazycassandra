//! The Queries panel's text buffer. Hand-written: the domain builds, gates and
//! completes statements from it.

/// Never empty, and the cursor always inside it; every mutator holds both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    lines: Vec<String>,
    cursor: Position,
    desired_column: usize,
}

/// `column` counts `char`s: bytes would add a boundary invariant to every edit,
/// and display width is the renderer's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Default for Editor {
    /// One empty line, so there is always a current line.
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            cursor: Position::default(),
            desired_column: 0,
        }
    }
}

impl Editor {
    /// The renderer does not wrap.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    #[must_use]
    pub const fn cursor(&self) -> Position {
        self.cursor
    }

    #[must_use]
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn insert(&mut self, character: char) {
        let at = self.byte_offset();
        self.line_mut().insert(at, character);
        self.cursor.column += 1;
        self.settle();
    }

    pub fn insert_newline(&mut self) {
        let at = self.byte_offset();
        let tail = self.line_mut().split_off(at);
        self.cursor.line += 1;
        self.cursor.column = 0;
        self.lines.insert(self.cursor.line, tail);
        self.settle();
    }

    pub fn write(&mut self, text: &str) {
        for character in text.chars() {
            if character == '\n' {
                self.insert_newline();
            } else {
                self.insert(character);
            }
        }
    }

    /// The one edit away from the cursor, for a completion's `FROM` clause.
    /// `offset` is at or after the cursor, so the saved cursor stays exact.
    pub fn insert_at(&mut self, offset: usize, text: &str) {
        debug_assert!(
            offset >= self.offset(),
            "an edit before the cursor moves it"
        );

        let cursor = self.cursor;
        self.cursor = self.position_of(offset);
        self.write(text);
        self.cursor = cursor;
        self.settle();
    }

    /// Characters, not bytes.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.lines
            .iter()
            .take(self.cursor.line)
            // Each line's newline counts.
            .map(|line| line.chars().count() + 1)
            .sum::<usize>()
            + self.cursor.column
    }

    /// Clamped to the end.
    pub fn go_to_offset(&mut self, offset: usize) {
        self.cursor = self.position_of(offset);
        self.settle();
    }

    /// Cursor at the end: the old offset means nothing against text an external
    /// editor rewrote. One trailing `\n` or `\r\n` is stripped, or every editor
    /// round trip would append a blank line.
    pub fn replace(&mut self, text: &str) {
        let text = text
            .strip_suffix("\r\n")
            .or_else(|| text.strip_suffix('\n'))
            .unwrap_or(text);
        self.lines = text.split('\n').map(str::to_owned).collect();
        self.cursor = Position {
            line: self.lines.len() - 1,
            column: 0,
        };
        self.cursor.column = self.line_len();
        self.settle();
    }

    /// At a line start it joins the line above.
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

    /// At a line end it joins the next line.
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

    pub fn left(&mut self) {
        if self.cursor.column > 0 {
            self.cursor.column -= 1;
        } else if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.cursor.column = self.line_len();
        }

        self.settle();
    }

    pub fn right(&mut self) {
        if self.cursor.column < self.line_len() {
            self.cursor.column += 1;
        } else if self.cursor.line + 1 < self.lines.len() {
            self.cursor.line += 1;
            self.cursor.column = 0;
        }

        self.settle();
    }

    pub fn up(&mut self) {
        self.go_to_line(self.cursor.line.saturating_sub(1));
    }

    pub fn down(&mut self) {
        self.go_to_line(self.cursor.line.saturating_add(1));
    }

    pub fn page_up(&mut self, height: usize) {
        self.go_to_line(self.cursor.line.saturating_sub(height));
    }

    pub fn page_down(&mut self, height: usize) {
        self.go_to_line(self.cursor.line.saturating_add(height));
    }

    pub fn line_start(&mut self) {
        self.cursor.column = 0;
        self.settle();
    }

    /// Line end, not buffer end, unlike the other panels: what `End` does in
    /// every editor.
    pub fn line_end(&mut self) {
        self.cursor.column = self.line_len();
        self.settle();
    }

    /// Leaves `desired_column` alone, so a short line does not cost the column.
    fn go_to_line(&mut self, line: usize) {
        // `lines` is never empty.
        self.cursor.line = line.min(self.lines.len() - 1);
        self.cursor.column = self.desired_column.min(self.line_len());
    }

    /// Clamped so an offset past the end still satisfies the cursor invariant.
    #[must_use]
    pub fn position_of(&self, offset: usize) -> Position {
        let mut left = offset;

        for (line, text) in self.lines.iter().enumerate() {
            let len = text.chars().count();
            if left <= len {
                return Position { line, column: left };
            }
            left -= len + 1;
        }

        // `lines` is never empty.
        Position {
            line: self.lines.len() - 1,
            column: self.lines[self.lines.len() - 1].chars().count(),
        }
    }

    fn line_len(&self) -> usize {
        self.lines[self.cursor.line].chars().count()
    }

    /// At end-of-line `char_indices` has no entry, so it is the line's length.
    fn byte_offset(&self) -> usize {
        let line = &self.lines[self.cursor.line];

        line.char_indices()
            .nth(self.cursor.column)
            .map_or(line.len(), |(at, _)| at)
    }

    fn line_mut(&mut self) -> &mut String {
        &mut self.lines[self.cursor.line]
    }

    /// Everything but vertical motion calls this.
    fn settle(&mut self) {
        self.desired_column = self.cursor.column;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `\n` means Enter.
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

    #[test]
    fn the_cursors_offset_counts_the_line_breaks_it_is_past() {
        let mut editor = typed("one\ntwo");

        assert_eq!(editor.offset(), "one\ntwo".chars().count());

        editor.line_start();
        assert_eq!(editor.offset(), "one\n".chars().count());
    }

    #[test]
    fn an_offset_counts_characters_rather_than_bytes() {
        let editor = typed("café");

        assert_eq!(editor.offset(), 4, "four characters, five bytes");
        assert_eq!(editor.position_of(4), Position { line: 0, column: 4 });
    }

    #[test]
    fn writing_at_an_offset_leaves_the_cursor_where_it_was() {
        let mut editor = typed("SELECT id");
        let offset = editor.offset();

        editor.insert_at(offset, " FROM shop.orders");

        assert_eq!(editor.text(), "SELECT id FROM shop.orders");
        assert_eq!(editor.cursor(), Position { line: 0, column: 9 });
    }

    #[test]
    fn writing_at_an_offset_past_the_cursor_leaves_what_follows_it_alone() {
        let mut editor = typed("SELECT id; TRUNCATE t");
        at(&mut editor, 0, 9);

        editor.insert_at(9, " FROM shop.orders");

        assert_eq!(editor.text(), "SELECT id FROM shop.orders; TRUNCATE t");
        assert_eq!(editor.cursor(), Position { line: 0, column: 9 });
    }

    #[test]
    fn writing_at_an_offset_past_the_end_of_the_buffer_lands_at_its_end() {
        let mut editor = typed("SELECT");

        editor.insert_at(99, "!");

        assert_eq!(editor.text(), "SELECT!");
    }

    #[test]
    fn writing_a_string_splits_the_line_on_every_break_in_it() {
        let mut editor = Editor::default();

        editor.write("BEGIN BATCH\n\nAPPLY BATCH;");

        assert_eq!(editor.lines(), ["BEGIN BATCH", "", "APPLY BATCH;"]);
        assert_eq!(
            editor.cursor(),
            Position {
                line: 2,
                column: 12
            }
        );
    }

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

        assert_eq!(
            editor.lines(),
            ["SELECT * ".to_owned(), "FROM t".to_owned()]
        );
        assert_eq!(editor.cursor(), Position { line: 1, column: 0 });
    }

    #[test]
    fn enter_at_the_end_of_a_line_opens_an_empty_one() {
        let editor = typed("SELECT\n");

        assert_eq!(editor.lines(), ["SELECT".to_owned(), String::new()]);
        assert_eq!(editor.cursor(), Position { line: 1, column: 0 });
    }

    #[test]
    fn enter_at_the_start_of_a_line_pushes_an_empty_one_above() {
        let mut editor = typed("SELECT");
        at(&mut editor, 0, 0);

        editor.insert_newline();

        assert_eq!(editor.lines(), [String::new(), "SELECT".to_owned()]);
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

    #[test]
    fn the_cursor_walks_left_and_right_within_a_line() {
        let mut editor = typed("SELECT");

        editor.left();
        editor.left();
        assert_eq!(editor.cursor().column, 4);

        editor.right();
        assert_eq!(editor.cursor().column, 5);
    }

    #[test]
    fn left_at_the_start_of_a_line_wraps_to_the_end_of_the_one_above() {
        let mut editor = typed("SELECT *\nFROM t");
        at(&mut editor, 1, 0);

        editor.left();

        assert_eq!(editor.cursor(), Position { line: 0, column: 8 });
    }

    #[test]
    fn right_at_the_end_of_a_line_wraps_to_the_start_of_the_one_below() {
        let mut editor = typed("SELECT *\nFROM t");
        at(&mut editor, 0, 8);

        editor.right();

        assert_eq!(editor.cursor(), Position { line: 1, column: 0 });
    }

    #[test]
    fn the_cursor_cannot_walk_off_either_end_of_the_buffer() {
        let mut editor = typed("SELECT");

        at(&mut editor, 0, 0);
        editor.left();
        editor.up();
        assert_eq!(editor.cursor(), Position { line: 0, column: 0 });

        at(&mut editor, 0, 6);
        editor.right();
        editor.down();
        assert_eq!(editor.cursor(), Position { line: 0, column: 6 });
    }

    #[test]
    fn the_column_survives_a_pass_over_a_short_line() {
        let mut editor = typed("SELECT * FROM t\nx\nWHERE id = 7");
        at(&mut editor, 0, 12);

        editor.down();
        assert_eq!(editor.cursor().column, 1, "the short line has no column 12");

        editor.down();
        assert_eq!(
            editor.cursor().column,
            12,
            "the column the user chose is restored, not the one the short line allowed"
        );
    }

    #[test]
    fn an_edit_gives_up_the_column_the_cursor_was_aiming_for() {
        let mut editor = typed("SELECT * FROM t\nx\nWHERE id = 7");
        at(&mut editor, 0, 12);
        editor.down();

        editor.insert('y');
        editor.down();

        assert_eq!(
            editor.cursor().column,
            2,
            "typing on the short line is where the cursor now wants to be"
        );
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_line() {
        let mut editor = typed("SELECT * FROM t\nWHERE id = 7");

        editor.line_start();
        assert_eq!(editor.cursor(), Position { line: 1, column: 0 });

        editor.line_end();
        assert_eq!(
            editor.cursor(),
            Position {
                line: 1,
                column: 12
            },
            "the line's end, not the buffer's"
        );
    }

    #[test]
    fn a_page_moves_one_panel_height_and_stops_at_the_ends() {
        let mut editor = typed("a\nb\nc\nd\ne\nf");
        at(&mut editor, 0, 0);

        editor.page_down(3);
        assert_eq!(editor.cursor().line, 3);

        editor.page_down(3);
        assert_eq!(editor.cursor().line, 5, "the last line, not past it");

        editor.page_up(3);
        assert_eq!(editor.cursor().line, 2);

        editor.page_up(9);
        assert_eq!(editor.cursor().line, 0);
    }

    #[test]
    fn replacing_the_buffer_lands_the_cursor_at_its_end() {
        let mut editor = typed("SELECT");

        editor.replace("SELECT *\nFROM t");

        assert_eq!(editor.text(), "SELECT *\nFROM t");
        assert_eq!(editor.cursor(), Position { line: 1, column: 6 });
    }

    #[test]
    fn replacing_with_nothing_leaves_the_buffer_as_default() {
        let mut editor = typed("SELECT *\nFROM t");

        editor.replace("");

        assert_eq!(editor, Editor::default());
    }

    #[test]
    fn replacing_strips_one_trailing_newline() {
        let mut editor = typed("SELECT");

        editor.replace("SELECT * FROM t\n");

        assert_eq!(editor.lines(), ["SELECT * FROM t".to_owned()]);
        assert_eq!(
            editor.cursor(),
            Position {
                line: 0,
                column: 15
            }
        );
    }

    #[test]
    fn replacing_strips_one_trailing_crlf() {
        let mut editor = typed("SELECT");

        editor.replace("SELECT * FROM t\r\n");

        assert_eq!(editor.lines(), ["SELECT * FROM t".to_owned()]);
        assert_eq!(
            editor.cursor(),
            Position {
                line: 0,
                column: 15
            }
        );
    }

    #[test]
    fn a_replaced_buffer_takes_edits_at_the_cursor() {
        let mut editor = Editor::default();

        editor.replace("FROM t");
        editor.insert('!');

        assert_eq!(editor.text(), "FROM t!");
    }
}
