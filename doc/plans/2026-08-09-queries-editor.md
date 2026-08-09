# Queries editor implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Queries panel a writable multi-line CQL buffer, with the text owned by `lc-domain`.

**Architecture:** A hand-written `Editor` (lines + cursor) lives in `lc-domain`, because that is where a `Statement` gets built, posture-gated, and will later meet the `Schema` for completion. `handle_key` stays the one dispatch point and gains one early route: when the focused panel takes text, printable and editing keys go to the buffer and everything else keeps its global meaning. `lc-inbound` draws the buffer and derives both scroll offsets per frame from the cursor, storing none — the rule `body.rs` already follows for lists.

**Tech Stack:** Rust 2024 · `ratatui` 0.30.2 · `crossterm` 0.29 · `unicode-width` 0.2 (`lc-inbound` only) · `cargo nextest`.

**Design:** [doc/specs/2026-08-09-queries-editor-design.md](../specs/2026-08-09-queries-editor-design.md). Section references below (§1–§10) are to that document.

## Global Constraints

- **No new dependencies.** `tui-textarea` 0.7.0 pins `ratatui ^0.29` against this workspace's 0.30.2 and cannot render into our `Frame`; `edtui` and `tui-input` were rejected on architecture (§1). Adding any crate to a `Cargo.toml` in this plan is a mistake.
- **`lc-domain` must not gain `ratatui`, `crossterm`, `tokio`, or `unicode-width`.** Display width is `lc-inbound`'s unit (§2).
- **No `unsafe`, no FFI.** Enforced by `unsafe_code = "forbid"`.
- **Zero warnings.** `mise run ci` must be clean. Any `#[allow(...)]` must be item-scoped with a reason. `clippy::pedantic` is on and `unwrap_used` / `expect_used` warn — outside tests, use `unwrap_or`, `map_or`, or `saturating_*`.
- **No narrating comments** (conventions.md §2). The comments written into this plan are the ones that earn their place — edge cases, and rules the code cannot state. Do not add others.
- **Determinism.** Everything in this plan is fast-tier: no clock, no socket, no filesystem, no `HashMap` iteration order.
- **Public items in `lc-domain` carry `///` docs.**
- **Conventional Commits**, imperative, lowercase, no trailing period.
- **Never `git pull`, never `git push`.**

## Commands

| | |
|---|---|
| Build | `mise run build` |
| Format | `mise run fmt` |
| Lint | `mise run lint` |
| All fast tests | `mise run test` |
| One test | `cargo nextest run -p <crate> -E 'test(<name>)'` |
| Full gate | `mise run ci` |

## File structure

| File | Responsibility |
|---|---|
| `crates/lc-domain/src/editor.rs` | **New.** `Editor` and `Position`: the buffer, its cursor, and every edit and motion on them. Pure, no I/O. |
| `crates/lc-domain/src/lib.rs` | Declare `pub mod editor`. |
| `crates/lc-domain/src/state.rs` | `State.queries: Editor`; `Panel::takes_text`; `QUERIES_HINTS`. |
| `crates/lc-domain/src/update.rs` | The `edit` route in `handle_key`; `escape` gains its second meaning. |
| `crates/lc-domain/src/view/mod.rs` | `PanelBody::Editor`; `body()` projects the buffer; re-export `Position`. |
| `crates/lc-domain/src/view/hints.rs` | `HintGroup` deleted; `q quit` dropped where a panel takes text. |
| `crates/lc-inbound/src/body.rs` | Draws the buffer, derives both offsets, sets the terminal cursor. |
| `crates/lc-inbound/src/render.rs` | Passes `focused` into `body::render`. |
| `crates/lc-inbound/src/hints.rs` | `Segment::Sentence` and `panel_segments` deleted. |
| `doc/shortcuts.md`, `doc/architecture.md`, `TODO.md`, `doc/specs/2026-08-09-hint-bar-design.md` | Documentation (§10). |

Tasks 1–3 build `editor.rs` alone and touch nothing else. Task 4 makes it reachable from the keyboard. Task 5 makes it visible. Task 6 makes its bindings visible. Task 7 is the documentation.

---

### Task 1: The buffer, and putting text into it

**Files:**
- Create: `crates/lc-domain/src/editor.rs`
- Modify: `crates/lc-domain/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Editor::default() -> Editor` · `Editor::lines(&self) -> &[String]` · `Editor::cursor(&self) -> Position` · `Editor::text(&self) -> String` · `Editor::insert(&mut self, char)` · `Editor::insert_newline(&mut self)` · `struct Position { pub line: usize, pub column: usize }`. All later tasks depend on these names.

- [ ] **Step 1: Write the failing tests**

Create `crates/lc-domain/src/editor.rs` containing *only* the test module for now:

```rust
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
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `pub mod editor;` to `crates/lc-domain/src/lib.rs`, in alphabetical position — after `pub mod contents;` and before `pub mod model;`.

Run: `cargo nextest run -p lc-domain -E 'test(editor::)'`
Expected: FAIL — compile errors, `cannot find type Editor in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the test module in `crates/lc-domain/src/editor.rs`:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p lc-domain -E 'test(editor::)'`
Expected: PASS, 7 tests.

Then `mise run lint` — expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src/editor.rs crates/lc-domain/src/lib.rs
git commit -m "feat(domain): add the Queries buffer and typing into it

The editor is written rather than taken from a widget crate: tui-textarea
pins ratatui ^0.29 against this workspace's 0.30.2, and every crate of that
shape owns the buffer inside lc-inbound, which architecture.md §3 and
shortcuts.md §2 both forbid (queries editor design §1).

Columns count chars so the domain needs no unicode-width."
```

---

### Task 2: Taking text back out

**Files:**
- Modify: `crates/lc-domain/src/editor.rs`

**Interfaces:**
- Consumes: `Editor`, `Position`, `byte_offset`, `line_mut`, `settle` from Task 1.
- Produces: `Editor::backspace(&mut self)` · `Editor::delete(&mut self)` · `Editor::line_len(&self) -> usize` (private, used by Task 3's motions).

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` block in `crates/lc-domain/src/editor.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p lc-domain -E 'test(editor::)'`
Expected: FAIL — `no method named backspace found for struct Editor`.

- [ ] **Step 3: Write the implementation**

Add to the `impl Editor` block in `crates/lc-domain/src/editor.rs`, after `insert_newline`:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p lc-domain -E 'test(editor::)'`
Expected: PASS, 14 tests.

Then `mise run lint` — expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src/editor.rs
git commit -m "feat(domain): delete from the Queries buffer

backspace at column 0 and delete at end-of-line act on the line break, so
the two of them are the only way to unmake a newline. Both are no-ops at the
ends of the buffer rather than errors."
```

---

### Task 3: Moving the cursor

**Files:**
- Modify: `crates/lc-domain/src/editor.rs`

**Interfaces:**
- Consumes: `Editor`, `Position`, `line_len`, `settle` from Tasks 1–2.
- Produces: `Editor::left` · `right` · `up` · `down` · `page_up(usize)` · `page_down(usize)` · `line_start` · `line_end`, all `&mut self`. Task 4 calls exactly these.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` block:

```rust
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
            Position { line: 1, column: 12 },
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p lc-domain -E 'test(editor::)'`
Expected: FAIL — `no method named left found for struct Editor`.

- [ ] **Step 3: Write the implementation**

Add to the `impl Editor` block, after `delete`:

```rust
    /// Steps back one character, onto the end of the line above at column 0.
    pub fn left(&mut self) {
        if self.cursor.column > 0 {
            self.cursor.column -= 1;
        } else if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.cursor.column = self.line_len();
        }

        self.settle();
    }

    /// Steps on one character, onto the start of the line below at end-of-line.
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
        self.to_line(self.cursor.line.saturating_sub(1));
    }

    pub fn down(&mut self) {
        self.to_line(self.cursor.line.saturating_add(1));
    }

    pub fn page_up(&mut self, height: usize) {
        self.to_line(self.cursor.line.saturating_sub(height));
    }

    pub fn page_down(&mut self, height: usize) {
        self.to_line(self.cursor.line.saturating_add(height));
    }

    pub fn line_start(&mut self) {
        self.cursor.column = 0;
        self.settle();
    }

    /// The end of the cursor's line, not of the buffer — what `End` does in a
    /// text editor, and a deliberate divergence from the other three panels
    /// (design §3).
    pub fn line_end(&mut self) {
        self.cursor.column = self.line_len();
        self.settle();
    }

    /// Every vertical motion. `desired_column` is read and left alone, so a
    /// pass over a short line does not cost the column for good.
    fn to_line(&mut self, line: usize) {
        // `lines` is never empty, so this cannot underflow.
        self.cursor.line = line.min(self.lines.len() - 1);
        self.cursor.column = self.desired_column.min(self.line_len());
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run -p lc-domain -E 'test(editor::)'`
Expected: PASS, 22 tests.

Then `mise run test` and `mise run lint` — expected: clean, nothing else affected.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src/editor.rs
git commit -m "feat(domain): move the cursor around the Queries buffer

desired_column is what makes a pass over a short line reversible: the
vertical motions read it and leave it alone, everything else adopts the
column it lands on."
```

---

### Task 4: Focus is the mode

**Files:**
- Modify: `crates/lc-domain/src/state.rs`
- Modify: `crates/lc-domain/src/update.rs`

**Interfaces:**
- Consumes: the whole `Editor` API from Tasks 1–3.
- Produces: `State.queries: Editor` (public field) · `Panel::takes_text(self) -> bool` (`const`). Tasks 5 and 6 both use these.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `crates/lc-domain/src/update.rs`, after `a_modified_key_is_not_a_binding`:

```rust
    /// A state focused on Queries with `text` typed into it.
    fn editing(text: &str) -> State {
        let mut state = browsing(&["a"]);
        state.focus = Panel::Queries;
        for character in text.chars() {
            press(&mut state, Key::Char(character));
        }
        state
    }

    #[test]
    fn a_letter_typed_in_queries_is_text_not_a_shortcut() {
        // Every letter here is bound somewhere: t/a/c/u focus a panel, j/k/h/l
        // move, q quits. In the editor they are all just text.
        let state = editing("tacujkhlq");

        assert_eq!(state.focus, Panel::Queries, "no letter moved focus");
        assert_eq!(state.queries.text(), "tacujkhlq");
    }

    #[test]
    fn q_does_not_quit_while_the_editor_has_focus() {
        let mut state = browsing(&["a"]);
        state.focus = Panel::Queries;

        let effects = press(&mut state, Key::Char('q'));

        assert!(!state.should_quit);
        assert_eq!(effects, Vec::new());
        assert_eq!(state.queries.text(), "q");
    }

    #[test]
    fn escape_leaves_the_editor_for_tables() {
        let mut state = editing("SELECT");

        press(&mut state, Key::Escape);

        assert_eq!(state.focus, Panel::Tables);
        assert_eq!(state.queries.text(), "SELECT", "leaving keeps the buffer");
    }

    #[test]
    fn escape_dismisses_the_overlay_before_it_leaves_the_editor() {
        let mut state = editing("SELECT");
        state.overlay = Some(Overlay {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        });

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
        assert_eq!(state.focus, Panel::Queries, "one esc does one thing");
    }

    #[test]
    fn tab_still_leaves_the_editor() {
        let mut state = editing("SELECT");

        press(&mut state, Key::Tab);

        assert_eq!(state.focus, Panel::Tables, "Queries wraps to Tables");
        assert_eq!(state.queries.text(), "SELECT");
    }

    #[test]
    fn a_modified_key_is_still_not_text() {
        let mut state = browsing(&["a"]);
        state.focus = Panel::Queries;

        update(
            &mut state,
            Msg::Input(KeyPress {
                key: Key::Char('a'),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
            }),
        );

        assert_eq!(
            state.queries.text(),
            "",
            "ctrl-a is a binding that does not exist yet, not a slower a"
        );
    }

    #[test]
    fn typing_outside_queries_still_focuses_a_panel() {
        let mut state = browsing(&["a"]);

        press(&mut state, Key::Char('c'));

        assert_eq!(state.focus, Panel::Contents);
        assert_eq!(state.queries.text(), "", "nothing leaked into the buffer");
    }

    #[test]
    fn the_editing_keys_reach_the_buffer() {
        let mut state = editing("SELECT *");

        press(&mut state, Key::Backspace);
        assert_eq!(state.queries.text(), "SELECT ");

        press(&mut state, Key::Enter);
        assert_eq!(state.queries.text(), "SELECT \n");

        press(&mut state, Key::Left);
        press(&mut state, Key::Delete);
        assert_eq!(state.queries.text(), "SELECT ", "delete ate the newline");
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_line_in_the_editor() {
        let mut state = editing("SELECT *\nFROM t");

        press(&mut state, Key::Home);
        assert_eq!(
            state.queries.cursor(),
            crate::editor::Position { line: 1, column: 0 }
        );

        press(&mut state, Key::End);
        assert_eq!(
            state.queries.cursor(),
            crate::editor::Position { line: 1, column: 6 },
            "the line's end, not the buffer's"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p lc-domain -E 'test(update::)'`
Expected: FAIL — `no field queries on type State`.

- [ ] **Step 3: Add the buffer and the predicate to `state.rs`**

In `crates/lc-domain/src/state.rs`, add to the imports:

```rust
use crate::editor::Editor;
```

Add the field to `State`, after `pub ddl: Scroll,`:

```rust
    /// The Queries panel's buffer. Not a `QueriesPanel`: the panel is the
    /// editor until the session log arrives.
    pub queries: Editor,
```

Add to `impl Panel`, after `from_shortcut`:

```rust
    /// Whether letters typed at this panel are text rather than bindings.
    ///
    /// Focus is the mode (design §3). Exhaustive for the reason [`Panel::hints`]
    /// is: a fifth panel cannot compile without saying which it is.
    #[must_use]
    pub const fn takes_text(self) -> bool {
        match self {
            Self::Tables | Self::Ddl | Self::Contents => false,
            Self::Queries => true,
        }
    }
```

- [ ] **Step 4: Route the keys in `update.rs`**

In `crates/lc-domain/src/update.rs`, inside `handle_key`, insert between the ctrl/alt guard and the `match press.key` block:

```rust
    // Focus is the mode. A panel that takes text claims every printable key
    // and the editing keys; what it does not claim keeps the global meaning
    // it has everywhere else (design §3). This is the rule shortcuts.md §2
    // said the CQL editor would have to break.
    if state.focus.takes_text() && edit(state, press.key) {
        return Vec::new();
    }
```

Change the `Escape` arm of that match from `Key::Escape => state.overlay = None,` to:

```rust
        Key::Escape => escape(state),
```

Add these two functions after `handle_key`:

```rust
/// Offers `key` to the focused panel's buffer, answering whether it was text.
///
/// Exhaustive over [`Key`], so a new key has to decide whether an editor
/// claims it rather than defaulting into one side.
fn edit(state: &mut State, key: Key) -> bool {
    let height = state.geometry.height(Panel::Queries);
    let editor = &mut state.queries;

    match key {
        Key::Char(character) => editor.insert(character),
        Key::Enter => editor.insert_newline(),
        Key::Backspace => editor.backspace(),
        Key::Delete => editor.delete(),
        Key::Left => editor.left(),
        Key::Right => editor.right(),
        Key::Up => editor.up(),
        Key::Down => editor.down(),
        Key::Home => editor.line_start(),
        Key::End => editor.line_end(),
        Key::PageUp => editor.page_up(height),
        Key::PageDown => editor.page_down(height),
        Key::Escape | Key::Tab | Key::BackTab | Key::Insert | Key::F(_) => return false,
    }

    true
}

/// `Esc` dismisses the overlay when there is one, and otherwise leaves a panel
/// that takes text — the one key that restores every letter binding at once
/// (design §3). One press does one of the two, never both.
fn escape(state: &mut State) {
    if state.overlay.take().is_some() {
        return;
    }

    if state.focus.takes_text() {
        state.focus = Panel::default();
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo nextest run -p lc-domain`
Expected: PASS, everything.

Two existing tests — `motion_belongs_to_the_focused_panel_only` and `a_tree_transition_belongs_to_the_focused_panel_only` — press `j` and `l` with Queries focused and assert the tree did not move. They still pass, but for a new reason: the key is now text rather than a swallowed motion. Add a line to each so the reason is on the record, immediately before its `press(...)` call:

```rust
        // Queries now takes the key as text; either way the tree must not move.
```

Run: `mise run lint` — expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-domain/src/state.rs crates/lc-domain/src/update.rs
git commit -m "feat(domain): let the Queries panel take text

Settles what shortcuts.md §4 left open. handle_key stays the one dispatch
point and gains one route: a panel that takes text claims the printable and
editing keys, and everything it does not claim keeps its global meaning.

q no longer quits from the editor and Esc gains a second meaning, leaving
for Tables when no overlay is up. Both are deliberate; the hint bar follows."
```

---

### Task 5: Drawing it

**Files:**
- Modify: `crates/lc-domain/src/view/mod.rs`
- Modify: `crates/lc-inbound/src/body.rs`
- Modify: `crates/lc-inbound/src/render.rs`

**Interfaces:**
- Consumes: `State.queries`, `Editor::lines`, `Editor::cursor`, `Position` from Tasks 1–4.
- Produces: `PanelBody::Editor { lines: Vec<String>, cursor: Position }` · `lc_domain::view::Position` (re-export) · `body::render(frame, area, body, focused: bool, theme)` — the new `focused` parameter sits **before** `theme`.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `crates/lc-inbound/src/render.rs`:

```rust
    /// A state focused on `focus`, with `text` in the Queries buffer.
    fn with_query(text: &str, focus: Panel) -> State {
        let mut state = State {
            focus,
            ..State::default()
        };
        for character in text.chars() {
            if character == '\n' {
                state.queries.insert_newline();
            } else {
                state.queries.insert(character);
            }
        }
        state
    }

    /// The drawn frame and where the terminal cursor ended up, if it is shown.
    fn drawn_with_cursor(
        width: u16,
        height: u16,
        state: &State,
    ) -> (Buffer, Option<ratatui::layout::Position>) {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(frame, &view_model(state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");

        let backend = terminal.backend();
        let cursor = backend
            .cursor_visible()
            .then(|| backend.cursor_position());

        (backend.buffer().clone(), cursor)
    }

    /// The Queries panel at 80x24 is `Rect { x: 20, y: 20, w: 60, h: 3 }`:
    /// borders on rows 20 and 22, its one content row on 21, and its inner
    /// left edge at column 21. The hint bar owns row 23.
    const CONTENT_ROW: u16 = 21;
    const INNER_LEFT: u16 = 21;

    #[test]
    fn the_editor_draws_what_has_been_typed() {
        let (buffer, _) = drawn_with_cursor(80, 24, &with_query("SELECT * FROM t", Panel::Queries));

        assert!(
            row(&buffer, CONTENT_ROW).contains("SELECT * FROM t"),
            "the buffer is on the panel's one content row: {:?}",
            row(&buffer, CONTENT_ROW)
        );
    }

    #[test]
    fn the_cursor_sits_where_the_domain_says_it_is() {
        let state = with_query("SELECT", Panel::Queries);
        let expected = state.queries.cursor();

        let (_, cursor) = drawn_with_cursor(80, 24, &state);

        let cursor = cursor.expect("a focused editor shows a cursor");
        assert_eq!(
            cursor.x,
            INNER_LEFT + u16::try_from(expected.column).expect("a test column fits"),
            "one column past the six typed"
        );
        assert_eq!(cursor.y, CONTENT_ROW);
    }

    #[test]
    fn an_unfocused_editor_sets_no_cursor() {
        let (_, cursor) = drawn_with_cursor(80, 24, &with_query("SELECT", Panel::Tables));

        assert_eq!(
            cursor, None,
            "a cursor in an unfocused panel would say it takes the keys"
        );
    }

    #[test]
    fn a_long_line_scrolls_sideways_and_keeps_the_cursor_on_screen() {
        let long = "SELECT * FROM keyspace_with_a_long_name.table_with_a_long_name WHERE id = 7";
        let state = with_query(long, Panel::Queries);

        let (buffer, cursor) = drawn_with_cursor(80, 24, &state);
        let drawn = row(&buffer, CONTENT_ROW);

        assert!(
            drawn.contains("WHERE id = 7"),
            "the cursor's end of the line is what is shown: {drawn:?}"
        );
        assert!(
            !drawn.contains("SELECT *"),
            "the start has scrolled off to the left: {drawn:?}"
        );
        let cursor = cursor.expect("a focused editor shows a cursor");
        assert!(
            cursor.x < 80,
            "the cursor stays inside the frame at {}",
            cursor.x
        );
    }

    #[test]
    fn a_tall_buffer_scrolls_and_says_so() {
        let state = with_query("one\ntwo\nthree\nfour", Panel::Queries);

        let (buffer, cursor) = drawn_with_cursor(80, 24, &state);
        let drawn = row(&buffer, CONTENT_ROW);

        assert!(
            drawn.contains("four"),
            "the cursor's line is the one drawn: {drawn:?}"
        );
        assert_eq!(
            cursor.expect("a focused editor shows a cursor").y,
            CONTENT_ROW,
            "the cursor's own row is on screen"
        );
        assert!(
            drawn.ends_with('█'),
            "four lines in a one-row panel grows the scrollbar: {drawn:?}"
        );
    }
```

Replace the existing `the_queries_panel_is_the_last_one_with_no_content` test in `crates/lc-domain/src/view/mod.rs` (it asserts `PanelBody::Blank`) with:

```rust
    #[test]
    fn the_queries_panel_shows_its_buffer() {
        let view = view_model(&loaded(&["shop"]));

        assert_eq!(
            view.panels[3].body,
            PanelBody::Editor {
                lines: vec![String::new()],
                cursor: Position::default(),
            },
            "an untouched buffer is one empty line with the cursor on it"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p lc-domain -p lc-inbound`
Expected: FAIL — `no variant Editor found for enum PanelBody`.

- [ ] **Step 3: Add the variant and the projection**

In `crates/lc-domain/src/view/mod.rs`, add to the re-exports beside `pub use tables::{Marker, TreeRow};`:

```rust
pub use crate::editor::Position;
```

Add the variant to `PanelBody`, after `Details`:

```rust
    /// A text buffer and where its cursor is.
    ///
    /// Carries no offsets. The renderer derives both from the cursor and the
    /// area it has, the way it already does for a list's selection — so an
    /// editor scrolls without any state being stored for it (design §4).
    Editor {
        lines: Vec<String>,
        cursor: Position,
    },
```

Change the `Panel::Queries` arm of `fn body` from `PanelBody::Blank` to:

```rust
        Panel::Queries => PanelBody::Editor {
            lines: state.queries.lines().to_vec(),
            cursor: state.queries.cursor(),
        },
```

- [ ] **Step 4: Draw it**

In `crates/lc-inbound/src/body.rs`, extend the imports:

```rust
use lc_domain::view::{GridCell, GridColumn, Marker, PanelBody, Position, TreeRow};
use ratatui::text::{Line, Text};
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};
```

Change `render`'s signature and add the arm:

```rust
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    body: &PanelBody,
    focused: bool,
    theme: Theme,
) -> Option<Extent> {
```

```rust
        PanelBody::Editor { lines, cursor } => {
            Some(editor(frame, area, lines, *cursor, focused))
        }
```

Add these functions at the end of the file:

```rust
/// One buffer line is one screen row: long lines scroll sideways rather than
/// wrap, so the domain keeps counting its own lines and the cursor's own row
/// is always on screen (design §5).
fn editor(
    frame: &mut Frame,
    area: Rect,
    lines: &[String],
    cursor: Position,
    focused: bool,
) -> Extent {
    let height = usize::from(area.height);
    let width = usize::from(area.width);

    let current = lines.get(cursor.line).map_or("", String::as_str);
    let column = width_of(current, cursor.column);

    let top = window(cursor.line, height);
    let left = window(column, width);

    let text = Text::from(
        lines
            .iter()
            .skip(top)
            .take(height)
            .map(|line| Line::raw(shifted(line, left)))
            .collect::<Vec<_>>(),
    );

    frame.render_widget(Paragraph::new(text), area);

    if focused {
        frame.set_cursor_position((
            area.x + narrowed(column - left),
            area.y + narrowed(cursor.line - top),
        ));
    }

    Extent {
        position: top,
        total: lines.len(),
        visible: height,
    }
}

/// The smallest offset that brings `index` into a window of `span` rows or
/// columns, starting from the top.
///
/// The same arithmetic `ListState` does for a selection, and stateless for the
/// same reason (module doc): rebuilt from the cursor every frame, so nothing
/// has to be stored or invalidated.
const fn window(index: usize, span: usize) -> usize {
    index.saturating_sub(span.saturating_sub(1))
}

/// Terminal columns the first `chars` characters of `line` occupy.
fn width_of(line: &str, chars: usize) -> usize {
    line.chars()
        .take(chars)
        .map(|character| character.width().unwrap_or(0))
        .sum()
}

/// `line` with its first `columns` terminal columns scrolled off to the left.
fn shifted(line: &str, columns: usize) -> String {
    let mut used = 0;
    let mut out = String::new();

    for character in line.chars() {
        if used >= columns {
            out.push(character);
        } else {
            used += character.width().unwrap_or(0);
        }
    }

    out
}

/// Both callers pass a value already bounded by an area dimension, so the
/// fallback is unreachable rather than a silent clamp.
fn narrowed(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(0)
}
```

- [ ] **Step 5: Pass `focused` from `render.rs`**

In `crates/lc-inbound/src/render.rs`, change the one call site:

```rust
        match crate::body::render(frame, inner, &panel.body, panel.panel == view.focus, theme) {
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo nextest run -p lc-domain -p lc-inbound`
Expected: PASS.

`CONTENT_ROW` and `INNER_LEFT` are measured, not guessed: at 80×24 `layout` puts Queries at `Rect { x: 20, y: 20, width: 60, height: 3 }`. If a row assertion fails, the layout changed — which is out of scope here (§7). Fix the cause, do not move the constant.

Run: `mise run lint` — expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/lc-domain/src/view/mod.rs crates/lc-inbound/src/body.rs crates/lc-inbound/src/render.rs
git commit -m "feat(inbound): draw the Queries buffer and its cursor

PanelBody::Editor carries no offsets: both are derived from the cursor and
the area every frame, the way a list's already are, so an editor scrolls
with no state stored for it.

The cursor is the terminal's own — ratatui shows or hides it from
Frame::set_cursor_position, so an unfocused editor leaves none behind."
```

---

### Task 6: Saying what it responds to

**Files:**
- Modify: `crates/lc-domain/src/state.rs`
- Modify: `crates/lc-domain/src/view/hints.rs`
- Modify: `crates/lc-inbound/src/hints.rs`
- Modify: `crates/lc-inbound/src/render.rs`

**Interfaces:**
- Consumes: `Panel::takes_text` from Task 4.
- Produces: `HintBar { panel: Vec<Hint>, global: Vec<Hint> }` — `HintGroup` is **gone**, not deprecated.

- [ ] **Step 1: Write the failing tests**

In `crates/lc-domain/src/view/hints.rs`, replace `a_panel_with_no_bindings_says_so_rather_than_showing_an_empty_group` with:

```rust
    #[test]
    fn a_panel_that_takes_text_does_not_claim_q_quits() {
        let bar = bar(Panel::Queries);

        assert_eq!(bar.panel[0].action, "newline");
        assert!(
            bar.global.iter().all(|hint| hint.action != "quit"),
            "q types a q in the editor, and a bar that lies is worse than no bar"
        );
        assert_eq!(
            bar.global.last().expect("tab panel survives").action,
            "panel",
            "tab is still the way out"
        );
    }
```

and rewrite `the_bar_carries_the_focused_panels_bindings_and_the_global_ones` for the flattened type:

```rust
    #[test]
    fn the_bar_carries_the_focused_panels_bindings_and_the_global_ones() {
        let bar = bar(Panel::Tables);

        assert_eq!(bar.panel[0].keys, "j/k");
        assert_eq!(bar.panel[0].action, "move");
        assert_eq!(bar.global.last().expect("q quit is last").action, "quit");
    }
```

In `crates/lc-domain/src/state.rs`, replace `only_the_queries_panel_has_nothing_to_offer` with:

```rust
    #[test]
    fn every_panel_says_what_it_responds_to() {
        for panel in Panel::ALL {
            assert!(
                !panel.hints().is_empty(),
                "{panel:?} responds to nothing, which no panel does any more"
            );
        }
    }
```

In `crates/lc-inbound/src/hints.rs`, replace `a_panel_with_no_bindings_shows_its_sentence_and_the_global_tail` with:

```rust
    #[test]
    fn the_editor_names_its_own_keys_and_drops_the_quit_it_no_longer_has() {
        assert_eq!(drawn(Panel::Queries, 80), " ⏎ newline  esc leave │ tab panel");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p lc-domain -p lc-inbound`
Expected: FAIL — the Queries hint list is empty, so `bar.panel[0]` panics.

- [ ] **Step 3: Give Queries its bindings**

In `crates/lc-domain/src/state.rs`, add after `CONTENTS_HINTS`:

```rust
const QUERIES_HINTS: &[Binding] = &[
    Binding {
        keys: &[Key::Enter],
        action: "newline",
    },
    // Named here and nowhere else: `Esc` only leaves a panel that takes text,
    // so this is the one bar where saying so is true (hint bar design §3).
    Binding {
        keys: &[Key::Escape],
        action: "leave",
    },
];
```

and change the `Self::Queries` arm of `Panel::hints` from `&[]` to `QUERIES_HINTS`. Delete the "Queries has none yet and says so" sentence from that method's doc comment.

- [ ] **Step 4: Flatten `HintBar` and drop the quit**

In `crates/lc-domain/src/view/hints.rs`: delete the `HintGroup` enum entirely, change `HintBar.panel` to `Vec<Hint>`, split `GLOBAL`, and rewrite `bar`:

```rust
/// Bindings every panel answers to. Repeated in each panel's bar rather than
/// learned once: a user who never presses `tab` has no way to find the other
/// three panels (design §3).
pub(crate) const GLOBAL: &[Binding] = &[Binding {
    keys: &[Key::Tab],
    action: "panel",
}];

/// Global everywhere a letter is a binding — which is every panel but one.
/// In a panel that takes text `q` types a `q`, and naming it would be a lie
/// (queries editor design §6).
///
/// `pub(crate)` so `update`'s "every global hint does something" test can
/// still reach it now that it has left [`GLOBAL`].
pub(crate) const QUIT: Binding = Binding {
    keys: &[Key::Char('q')],
    action: "quit",
};

pub(crate) fn bar(focus: Panel) -> HintBar {
    let mut global: Vec<Hint> = GLOBAL.iter().map(rendered).collect();
    if !focus.takes_text() {
        global.push(rendered(&QUIT));
    }

    HintBar {
        panel: focus.hints().iter().map(rendered).collect(),
        global,
    }
}
```

Update `HintBar`'s doc comment so it no longer refers to a group.

- [ ] **Step 5: Drop the sentence from the renderer**

In `crates/lc-inbound/src/hints.rs`: change the import to `use lc_domain::view::{Hint, HintBar};`, delete the `Segment::Sentence` variant along with its arms in `width` and `spans`, delete `panel_segments`, and change the first line of `line` to:

```rust
    let panel: Vec<Segment> = bar.panel.iter().map(Segment::hint).collect();
```

`Segment` now has one variant. Leave it an enum — replacing it with a struct is a separate change and this task is already touching four files.

In that file's test module, `a_global_tail_longer_than_two_entries_still_drops_down_to_the_last_one` builds `panel: HintGroup::Keys(vec![])`; change it to `panel: vec![]`.

In `crates/lc-inbound/src/render.rs`, `the_bar_says_what_the_focused_panel_does` asserts `bars[3].contains("nothing here yet")`; change that assertion to:

```rust
        assert!(
            bars[3].contains("⏎ newline"),
            "Queries takes text: {:?}",
            bars[3]
        );
```

- [ ] **Step 6: Keep `q quit` under test**

`every_global_hint_does_something_in_every_panel` in `update.rs` iterates `GLOBAL` and presses every key in it against every panel. Once `QUIT` leaves `GLOBAL`, that loop stops covering `q` entirely — the test would still pass while testing less. Restore the coverage by folding `QUIT` back in for the panels that claim it:

```rust
    #[test]
    fn every_global_hint_does_something_in_every_panel() {
        for panel in Panel::ALL {
            // `q` left GLOBAL because the bar stops naming it in a panel that
            // takes text. It is still a binding in the other three, and this
            // is still where that is checked.
            let claimed = crate::view::hints::GLOBAL
                .iter()
                .chain((!panel.takes_text()).then_some(&crate::view::hints::QUIT));

            for binding in claimed {
                for &key in binding.keys {
                    let mut state = populated(panel);
                    let before = crate::view::view_model(&state);

                    let effects = press(&mut state, key);

                    assert!(
                        crate::view::view_model(&state) != before || !effects.is_empty(),
                        "'{key:?} {}' does nothing with {panel:?} focused",
                        binding.action
                    );
                }
            }
        }
    }
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `mise run test`
Expected: PASS, all of it.

Two existing tests are worth understanding rather than just watching go green:

- `every_hint_the_bar_shows_does_something` now presses `Enter` and `Esc` with Queries focused. `populated` sets `state.focus` only after its setup presses, so the setup is not eaten by the editor; `Enter` then changes the buffer and `Esc` changes the focus, so both hints do something.
- `the_widest_bar_still_fits_the_narrowest_legal_frame` pins the Ddl bar at 40 columns and is untouched. The new Queries bar is `" ⏎ newline  esc leave │ tab panel"` — 33 columns, so it clears the same frame.

Run: `mise run lint` — expected: clean. Confirm no `HintGroup` survives:

```bash
! grep -rn "HintGroup" crates/ && echo "gone"
```

- [ ] **Step 8: Commit**

```bash
git add crates/lc-domain/src/state.rs crates/lc-domain/src/update.rs crates/lc-domain/src/view/hints.rs crates/lc-inbound/src/hints.rs crates/lc-inbound/src/render.rs
git commit -m "feat(inbound): let the hint bar name the editor's keys

Queries names its own two and loses q quit, because q types a q there and a
bar that lies is worse than no bar (hint bar design §2).

HintGroup::Idle went with it. It existed only for a Queries panel with no
bindings, so nothing can construct it any more, and an arm nothing
constructs is a claim the model does not honour."
```

---

### Task 7: The documentation

**Files:**
- Modify: `doc/shortcuts.md`
- Modify: `doc/architecture.md`
- Modify: `doc/specs/2026-08-09-hint-bar-design.md`
- Modify: `doc/specs/2026-08-09-queries-editor-design.md`
- Modify: `TODO.md`

**Interfaces:**
- Consumes: everything above. No code changes.

- [ ] **Step 1: Rewrite the affected rules in `doc/shortcuts.md`**

**§1** — add a "Queries panel" subsection after the Tables one:

```markdown
### Queries panel

The panel takes text, so while it has focus every printable key is a
character and not a binding. What it does not claim keeps its global meaning.

| Key | Action |
|---|---|
| any printable key | Insert it. `q`, `t`, `j` and the rest are text here. |
| `Enter` | Split the line at the cursor. |
| `Backspace` / `Delete` | Delete before / at the cursor; at a line's edge, the line break. |
| `←` `→` `↑` `↓` | Move the cursor. `←`/`→` wrap between lines. |
| `Home` / `End` | To the start / end of the **line**. |
| `PageUp` / `PageDown` | Up / down one panel height. |
| `Esc` | Leave the editor for Tables. |

`Tab` and `Shift-Tab` still move focus. Nothing else global survives inside
the editor — including `q`, which types a `q`.
See [queries editor design](specs/2026-08-09-queries-editor-design.md).
```

**§2** — three rules change. Replace "No modes" wholesale with:

```markdown
**Focus is the mode.** A panel either takes text or it does not, and
`Panel::takes_text` is the one exhaustive answer. In a panel that takes text
every printable key is a character; everywhere else every printable key is a
binding. There is no state within a panel that changes what a key means, so
the regime is always the focused border — which is on screen and already
means something. This replaces the older "no modes" rule, which held only
until the Queries panel took text.
```

Amend "Bindings are global; panels refuse" by appending:

```markdown
This now has one exception, and only one: a panel that takes text claims the
printable and editing keys before the table is consulted. Everything it does
not claim is still dispatched globally and still refused by panels that have
nothing to do with it.
```

Amend "The same key means the same thing in every panel" by appending:

```markdown
`Home` and `End` are the exception. Everywhere else they reach the ends of
the document; in the editor they reach the ends of the line, because that is
what they do in every text editor a user has met. The document's ends stay
reachable with `PageUp` and `PageDown`.
```

**§3** — no letter changes hands, but add after the free list:

> Every printable key is also text in a panel that takes text. That does not
> spend it: `d` is still free to be bound for the other three panels.

**§4** — delete the "Text entry" row. It is built.

**§5** — add these rows:

```markdown
| Letters are text in the editor | `a_letter_typed_in_queries_is_text_not_a_shortcut` |
| Quitting is not reachable from the editor | `q_does_not_quit_while_the_editor_has_focus` |
| `Esc` prefers the overlay, then leaves | `escape_dismisses_the_overlay_before_it_leaves_the_editor`, `escape_leaves_the_editor_for_tables` |
| `Tab` still leaves | `tab_still_leaves_the_editor` |
| Ctrl is still not text | `a_modified_key_is_still_not_text` |
| The editing keys reach the buffer | `the_editing_keys_reach_the_buffer` |
| `Home`/`End` take the line's ends | `home_and_end_reach_the_ends_of_the_line_in_the_editor` |
| Other panels keep their letters | `typing_outside_queries_still_focuses_a_panel` |
| The buffer's own behaviour | the `editor.rs` tests |
```

- [ ] **Step 2: Update `doc/architecture.md`**

In §11, the "Does `State` stay one struct?" bullet says `TablesPanel` and `ContentsPanel` each took a module and that "Two of four is enough of a pattern". Amend it: `editor.rs` is a third module, but it is a *buffer* rather than a panel — `State.queries` is an `Editor`, not a `QueriesPanel` — so the question of what routes messages to per-panel state is still open and still answered by `update` dispatching by hand.

In §1's status line and §4's module list, add `editor.rs` — "the Queries panel's text buffer".

- [ ] **Step 3: Amend the hint bar design**

At the end of `doc/specs/2026-08-09-hint-bar-design.md`, append:

```markdown
---

## Amendment — 2026-08-09, queries editor

§3's "Queries says *nothing here yet*" and the `HintGroup::Idle` variant of §4
are both retired. The Queries panel takes text and names its own bindings, so
no panel has an empty hint list and nothing can construct `Idle`.
`HintBar.panel` is a `Vec<Hint>`.

`GLOBAL` also stopped being unconditional. `q quit` is dropped for a panel that
takes text, where `q` types a `q` — §2's rule that a bar which lies is worse
than no bar, applied to the bar's own tail. `tab panel` is unaffected and is
still the way out.

See [queries editor design](2026-08-09-queries-editor-design.md) §6.
```

- [ ] **Step 4: Update the status line and `TODO.md`**

In `doc/specs/2026-08-09-queries-editor-design.md`, change `**Status:** designed` to `**Status:** implemented`.

Append to `TODO.md`:

```markdown
From the queries editor slice (doc/specs/2026-08-09-queries-editor-design.md §9):

- Size the Queries panel by what is in it. It gets one content row at 80x24
  and 120x40 — the 3-row floor beats `QUERIES_PERCENT` until the terminal is
  about forty rows tall — so multi-line CQL is legible only by scrolling.
  The panel is meant to grow with its content and its focus, which is a
  layout change with its own spec, not a tweak to the percentage.
- Execute the buffer. Nothing turns it into a `Statement` or an
  `Effect::Execute` yet, and doing so has to answer where the result lands:
  `ContentsPanel` models exactly one outstanding read keyed to a `TableRef`,
  which a typed query has no business claiming.
- The session log. architecture.md §4 calls this panel "statement entry and
  session log"; only the first half exists. When it lands, `State.queries`
  becomes a `QueriesPanel` holding both.
- Undo and redo, selection, clipboard, and word motions. The buffer has none.
- Schema-aware completion and syntax highlighting (vision.md §5.1). The
  `Token` path of architecture.md §6 is already the right shape for the
  second.
```

- [ ] **Step 5: Verify the docs match the code**

The rules in `shortcuts.md` §5 name tests; every one must exist.

```bash
for t in a_letter_typed_in_queries_is_text_not_a_shortcut \
         q_does_not_quit_while_the_editor_has_focus \
         escape_dismisses_the_overlay_before_it_leaves_the_editor \
         escape_leaves_the_editor_for_tables \
         tab_still_leaves_the_editor \
         a_modified_key_is_still_not_text \
         the_editing_keys_reach_the_buffer \
         home_and_end_reach_the_ends_of_the_line_in_the_editor \
         typing_outside_queries_still_focuses_a_panel; do
  grep -rq "fn $t" crates/ || echo "MISSING: $t"
done
echo "checked"
```

Expected: `checked`, with no `MISSING` lines.

Then run the full gate: `mise run ci`
Expected: clean — build, fmt, clippy, `cargo deny`, and all fast tiers.

- [ ] **Step 6: Commit**

```bash
git add doc TODO.md
git commit -m "docs: record that the Queries panel takes text

shortcuts.md §2 loses three rules as written: 'no modes' becomes 'focus is
the mode', 'bindings are global' gains its one exception, and Home/End stop
meaning the same thing in every panel. §4's text-entry row is built.

The hint bar design is amended rather than left describing HintGroup::Idle,
which no longer exists."
```

---

## Verification

After Task 7, from a clean tree:

```bash
mise run ci
```

Then drive it by hand — the panel is one row tall at most terminal sizes, which is worth seeing:

```bash
mise run db:up && mise run db:seed && mise run run
```

Press `u`, type `SELECT * FROM ks.t`, watch the line scroll sideways past the panel's width, press `Esc`, then `q`.

## Notes for the implementer

**Where the surprises are.**

- **Task 4's `edit` is exhaustive over `Key` on purpose.** Do not add a `_ => return false` arm. The point is that a new key cannot be added without someone deciding whether an editor claims it.
- **Task 5's offsets look like they lose information, and do not.** `window` pins the cursor to the last row or column once the buffer outgrows the panel, which reads as "the viewport does not scroll back until the cursor does". That is exactly what `ListState` already does for Tables and Contents, because it too is rebuilt from the selection each frame with offset 0. Matching it is the point; do not add a stored offset to "improve" it.
- **Task 6 deletes a public enum.** `HintGroup` is `pub` in `lc-domain::view`, but `lc-inbound` is its only consumer and nothing outside this repo depends on it (architecture.md §9). Delete it rather than deprecating it.
- **The panel is one content row at 80×24.** Tests that assert on a drawn row must account for that; a test that needs three visible lines needs a taller `TestBackend`, not a layout change. Changing `QUERIES_PERCENT` or `PANEL_MIN_H` is out of scope for every task in this plan (§7).
