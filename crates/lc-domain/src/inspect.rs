//! The cell inspector: one value, or one row, read whole
//! (docs/superpowers/specs/2026-09-17-cell-inspector-design.md).
//!
//! The grid caps a column and ellipsises what does not fit; this is where a
//! value is laid out for reading instead. Wrapping is the renderer's, because
//! it needs a width, so the domain holds a cursor over the laid-out lines it
//! can count itself, and an offset it bounds by what the frame reports back
//! (design §5).

use std::collections::BTreeMap;

use crate::bytes::Bytes;
use crate::cell;
use crate::contents::Subject;
use crate::edit::{Edited, Field};
use crate::model::CqlValue;
use crate::state::{Scroll, Span};
use crate::style::Segment;

/// What `Enter` opened over, copied out of the page: the grid beneath can be
/// replaced while the overlay is up, and what is being read must not change
/// under it.
#[derive(Debug, Clone, PartialEq)]
pub enum Inspected {
    /// The value under the cell cursor, in cell and column mode.
    Cell {
        /// The column's index in the page, which is what an edit is keyed by
        /// — the name alone addresses nothing
        /// (cell editing design §4).
        at: usize,
        column: String,
        value: CqlValue,
    },
    /// Every column of the row, in row mode.
    Row {
        /// The gutter's number, one-based.
        number: usize,
        columns: Vec<String>,
        values: Vec<CqlValue>,
    },
}

impl Inspected {
    /// Every value the block lays out, in the order it draws them.
    fn values(&self) -> &[CqlValue] {
        match self {
            Self::Cell { value, .. } => std::slice::from_ref(value),
            Self::Row { values, .. } => values,
        }
    }

    /// The page column the `at`th drawn value belongs to. In row mode a field
    /// *is* a column, in column order; in cell mode there is one field and it
    /// is the column the grid cursor was on.
    fn column_of(&self, at: usize) -> Option<(usize, &str)> {
        match self {
            Self::Cell { at: column, .. } if at == 0 => Some((*column, "")),
            Self::Cell { .. } => None,
            Self::Row { columns, .. } => columns.get(at).map(|name| (at, name.as_str())),
        }
    }

    /// Replaces the `at`th drawn value, so the block redraws what was just
    /// typed into it: the inspector holds a copy of the page, and a copy that
    /// did not follow the edit would show the value the user replaced.
    fn set(&mut self, at: usize, value: CqlValue) {
        match self {
            Self::Cell { value: held, .. } if at == 0 => *held = value,
            Self::Cell { .. } => {}
            Self::Row { values, .. } => {
                if let Some(held) = values.get_mut(at) {
                    *held = value;
                }
            }
        }
    }
}

/// An open inspector: what it shows, which line the cursor is on, and how
/// far down it is scrolled.
///
/// The cursor counts laid-out lines, which the domain can count for itself:
/// the layout is width-independent, and only the wrapping under it is not.
/// The offset counts wrapped lines, so it follows the cursor by the span the
/// last frame drew it at, the way the Logs tab's does (design §5).
#[derive(Debug, Clone, PartialEq)]
pub struct Inspector {
    /// The drawn row the grid was standing on, which is what an edit made
    /// here is applied back to (cell editing design §4).
    row: usize,
    subject: Subject,
    inspected: Inspected,
    cursor: usize,
    scroll: Scroll,
    /// The open edit field, while one is.
    ///
    /// A field on the inspector rather than a sibling overlay: a field with
    /// no value under it is not a state, the argument the export form makes
    /// for its overwrite confirmation (export design §7).
    field: Option<Field>,
    /// Which drawn fields have been typed into and not yet written. Indices
    /// into the values the block draws, so the projection can band them
    /// without going back to the page.
    edited: Vec<usize>,
    /// Set while the block is the form for a row that does not exist yet
    /// (docs/superpowers/specs/2026-10-12-row-insert-design.md §2).
    new_row: Option<BTreeMap<usize, Edited>>,
}

impl Inspector {
    #[must_use]
    pub fn opened(row: usize, subject: Subject, inspected: Inspected) -> Self {
        Self {
            row,
            subject,
            inspected,
            cursor: 0,
            scroll: Scroll::default(),
            field: None,
            edited: Vec::new(),
            new_row: None,
        }
    }

    /// The form for a row that does not exist yet: every column of `subject`,
    /// all unset (insert design §2).
    ///
    /// The grid row is `0` and means nothing: nothing typed here is applied
    /// to the page, which is what [`Self::row`] otherwise addresses.
    #[must_use]
    pub fn inserting(subject: Subject, columns: Vec<String>) -> Self {
        let values = vec![CqlValue::Null; columns.len()];
        let mut form = Self::opened(
            0,
            subject,
            Inspected::Row {
                number: 0,
                columns,
                values,
            },
        );
        form.new_row = Some(BTreeMap::new());

        form
    }

    #[must_use]
    pub const fn is_inserting(&self) -> bool {
        self.new_row.is_some()
    }

    /// The fields typed into so far, as `(drawn field, value)` in column
    /// order, which is the order the statement reads in.
    pub fn typed(&self) -> impl Iterator<Item = (usize, &Edited)> {
        self.new_row
            .iter()
            .flat_map(|typed| typed.iter().map(|(at, edited)| (*at, edited)))
    }

    /// Whether `⏎` would accept the open field. An empty field is accepted in
    /// the form, where it unsets the column (insert design §3).
    #[must_use]
    pub fn can_accept(&self) -> bool {
        self.field.as_ref().is_some_and(|field| {
            field.parsed().is_some() || (self.is_inserting() && field.line().text().is_empty())
        })
    }

    /// Why `⏎` is greyed, `None` while it is not.
    #[must_use]
    pub fn field_error(&self) -> Option<String> {
        if self.can_accept() {
            return None;
        }

        self.field.as_ref().and_then(Field::error)
    }

    /// Accepts the open field into the form: a value is held, an empty field
    /// unsets the column. `false` while the text does not parse, which leaves
    /// the field open (cell editing design §3).
    pub fn accept_new(&mut self) -> bool {
        if !self.can_accept() {
            return false;
        }
        let Some(at) = self.at_field().map(|(at, _, _)| at) else {
            return false;
        };
        let Some(field) = self.field.take() else {
            return false;
        };
        let Some(typed) = self.new_row.as_mut() else {
            self.field = Some(field);
            return false;
        };

        if let Some(edited) = field.parsed()
            && !field.line().text().is_empty()
        {
            self.inspected.set(at, edited.value().clone());
            typed.insert(at, edited);
            if !self.edited.contains(&at) {
                self.edited.push(at);
            }
        } else {
            self.inspected.set(at, CqlValue::Null);
            typed.remove(&at);
            self.edited.retain(|marked| *marked != at);
        }

        true
    }

    /// The drawn grid row this was opened over.
    #[must_use]
    pub const fn row(&self) -> usize {
        self.row
    }

    /// Marks the drawn fields already dirty when the block opened, so a
    /// reopened inspector says so too.
    #[must_use]
    pub fn with_edited(self, edited: Vec<usize>) -> Self {
        Self { edited, ..self }
    }

    /// The drawn fields typed into and not yet written.
    #[must_use]
    pub fn edited(&self) -> &[usize] {
        &self.edited
    }

    /// The open field, while one is.
    #[must_use]
    pub const fn field(&self) -> Option<&Field> {
        self.field.as_ref()
    }

    pub const fn field_mut(&mut self) -> Option<&mut Field> {
        self.field.as_mut()
    }

    /// The drawn field the cursor's line belongs to, and the page column and
    /// name it names. `None` where the cursor is on a value that belongs to
    /// no column of the page.
    #[must_use]
    pub fn at_field(&self) -> Option<(usize, usize, &str)> {
        let at = self.field_at_cursor();

        self.inspected
            .column_of(at)
            .map(|(column, name)| (at, column, name))
    }

    /// The value the cursor's line belongs to, as the block draws it.
    #[must_use]
    pub fn value_at_cursor(&self) -> Option<&CqlValue> {
        self.inspected.values().get(self.field_at_cursor())
    }

    /// Opens `field` over the value the cursor is on.
    pub fn start_edit(&mut self, field: Field) {
        self.field = Some(field);
    }

    pub fn cancel_edit(&mut self) {
        self.field = None;
    }

    /// Accepts the open field: closes it, redraws the value it typed, and
    /// answers with the drawn field and what was accepted — which is what
    /// `update` carries back to the page.
    ///
    /// `None` while the text does not parse, which leaves the field open:
    /// a keystroke that loses what was typed is worse than no validation
    /// (design §3).
    pub fn accept_edit(&mut self) -> Option<(usize, usize, Edited)> {
        let field = self.field.as_ref()?;
        let edited = field.parsed()?;
        let (at, column, _) = self.at_field()?;

        self.inspected.set(at, edited.value().clone());
        self.field = None;
        if !self.edited.contains(&at) {
            self.edited.push(at);
        }

        Some((at, column, edited))
    }

    /// Unmarks the `at`th drawn field, for an edit that restored the value it
    /// started from (design §1).
    pub fn unmark(&mut self, at: usize) {
        self.edited.retain(|marked| *marked != at);
    }

    /// Which drawn value the cursor's line falls in.
    fn field_at_cursor(&self) -> usize {
        let mut at = self.cursor;

        for (index, value) in self.inspected.values().iter().enumerate() {
            let laid = lines(value).len();

            if at < laid {
                return index;
            }
            at -= laid;
        }

        self.inspected.values().len().saturating_sub(1)
    }

    #[must_use]
    pub const fn subject(&self) -> &Subject {
        &self.subject
    }

    #[must_use]
    pub const fn inspected(&self) -> &Inspected {
        &self.inspected
    }

    /// The first wrapped line drawn.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.scroll.offset()
    }

    /// The laid-out line the cursor is on.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Laid-out lines the block holds: one value's, or every field's.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.inspected
            .values()
            .iter()
            .map(|value| lines(value).len())
            .sum()
    }

    pub fn down(&mut self) {
        self.go_to(self.cursor.saturating_add(1));
    }

    pub const fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn page_down(&mut self, height: usize) {
        self.go_to(self.cursor.saturating_add(height));
    }

    pub const fn page_up(&mut self, height: usize) {
        self.cursor = self.cursor.saturating_sub(height);
    }

    pub const fn to_start(&mut self) {
        self.cursor = 0;
    }

    pub fn to_end(&mut self) {
        self.go_to(usize::MAX);
    }

    fn go_to(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.line_count().saturating_sub(1));
    }

    /// Pulls the viewport the shortest distance that puts the cursor's line
    /// inside it, `span` being where the last frame wrapped that line to.
    pub const fn follow(&mut self, span: Span, height: usize) {
        self.scroll.follow_span(span, height);
    }

    /// Pulls the offset back over a body that rewrapped shorter.
    pub const fn clamp_to(&mut self, lines: usize, height: usize) {
        self.scroll.clamp_to(lines, height);
    }

    /// What the grid's `y` yanks in the mode this was opened from: the bare
    /// value, or the row as one CSV line (design §4).
    #[must_use]
    pub fn yank(&self) -> Option<String> {
        match &self.inspected {
            Inspected::Cell { value, .. } => crate::yank::grid_cell(value),
            Inspected::Row { values, .. } => Some(crate::yank::grid_row(&crate::model::Row {
                values: values.clone(),
            })),
        }
    }

    /// The laid-out line under the cursor, its indent dropped — and, in row
    /// mode, without the column name beside it, which is a heading rather
    /// than a value. `None` over a null, as [`Self::yank`] is.
    #[must_use]
    pub fn line(&self) -> Option<String> {
        let (value, line) = self.at_cursor()?;

        match value {
            CqlValue::Null => None,
            _ => Some(line.trim_start().to_owned()),
        }
    }

    /// The value the cursor's line belongs to, and the line.
    fn at_cursor(&self) -> Option<(&CqlValue, String)> {
        let mut at = self.cursor;

        for value in self.inspected.values() {
            let laid = lines(value);

            if at < laid.len() {
                return laid.into_iter().nth(at).map(|line| (value, line));
            }
            at -= laid.len();
        }

        None
    }

    /// `<subject> · <column>`, or `<subject> · row <n>`.
    #[must_use]
    pub fn title(&self) -> Vec<Segment> {
        let mut segments = match &self.subject {
            Subject::Table(table) => table.segments(),
            Subject::Typed(_) => vec![Segment::plain(self.subject.label())],
        };

        segments.push(Segment::plain(match &self.inspected {
            Inspected::Cell { column, .. } => format!(" · {column}"),
            Inspected::Row { .. } if self.is_inserting() => " · new row".to_owned(),
            Inspected::Row { number, .. } => format!(" · row {number}"),
        }));

        segments
    }

    /// The cell's size for the bottom border. `None` over a row, and over a
    /// null, which has no bytes to count.
    #[must_use]
    pub fn size(&self) -> Option<String> {
        let Inspected::Cell { value, .. } = &self.inspected else {
            return None;
        };

        let bytes = match value {
            CqlValue::Null => return None,
            CqlValue::Blob(bytes) => bytes.len(),
            CqlValue::Text(text) | CqlValue::Uuid(text) => text.len(),
            other => cell::text(other).len(),
        };

        Some(Bytes::new(bytes as u64).to_string())
    }
}

/// `value` laid out for reading, one string per line, leading spaces its
/// indent (design §3).
///
/// Control characters are dropped, which is what the grid's cells come to:
/// ratatui draws no grapheme holding one. Dropped here rather than left to
/// the renderer because wrapping reads `\n` and `\t` as whitespace, and a
/// break the grid never draws would appear in the inspector.
#[must_use]
pub fn lines(value: &CqlValue) -> Vec<String> {
    let laid = match value {
        CqlValue::Text(text) => json(text).unwrap_or_else(|| vec![text.clone()]),
        other => nested(other),
    };

    laid.into_iter()
        .map(|line| line.chars().filter(|c| !c.is_control()).collect())
        .collect()
}

/// A collection one element to a line inside its brackets, nested ones a level
/// deeper; any other value as the grid draws it.
fn nested(value: &CqlValue) -> Vec<String> {
    match value {
        CqlValue::List(values) => bracketed(values.iter().map(|value| (None, value)), '[', ']'),
        CqlValue::Set(values) => bracketed(values.iter().map(|value| (None, value)), '{', '}'),
        CqlValue::Map(entries) => bracketed(keyed(entries), '{', '}'),
        other => vec![cell::text(other)],
    }
}

fn keyed(
    entries: &BTreeMap<String, CqlValue>,
) -> impl ExactSizeIterator<Item = (Option<&str>, &CqlValue)> {
    entries
        .iter()
        .map(|(key, value)| (Some(key.as_str()), value))
}

fn bracketed<'a>(
    entries: impl ExactSizeIterator<Item = (Option<&'a str>, &'a CqlValue)>,
    open: char,
    close: char,
) -> Vec<String> {
    let count = entries.len();
    if count == 0 {
        return vec![format!("{open}{close}")];
    }

    let mut lines = vec![open.to_string()];

    for (at, (key, value)) in entries.enumerate() {
        let mut element = nested(value);

        if let (Some(key), Some(first)) = (key, element.first_mut()) {
            *first = format!("{key}: {first}");
        }
        if at + 1 < count
            && let Some(last) = element.last_mut()
        {
            last.push(',');
        }

        lines.extend(element.into_iter().map(|line| format!("  {line}")));
    }

    lines.push(close.to_string());
    lines
}

/// `text` re-indented as JSON, or `None` when it is not JSON enough to
/// re-indent safely: it must open with `{` or `[`, balance its brackets,
/// terminate every string, end where its outermost bracket closes, and hold
/// no whitespace between two bare tokens — `[not json]` would otherwise come
/// out as `notjson`.
///
/// Hand-rolled rather than parsed into a map: a map reorders keys and
/// reformats numbers, and every token here is kept byte for byte (design §3).
fn json(text: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = text.trim().chars().collect();
    if !matches!(chars.first(), Some('{' | '[')) {
        return None;
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut open: Vec<char> = Vec::new();
    let mut at = 0;

    while let Some(&character) = chars.get(at) {
        if at > 0 && open.is_empty() {
            return None;
        }

        match character {
            '"' => at = string(&chars, at, &mut line)?,
            '{' | '[' => {
                let close = if character == '{' { '}' } else { ']' };
                let next = significant(&chars, at + 1);

                line.push(character);
                if chars.get(next) == Some(&close) {
                    line.push(close);
                    at = next;
                } else {
                    open.push(close);
                    broken(&mut lines, &mut line, open.len());
                }
            }
            '}' | ']' => {
                if open.pop() != Some(character) {
                    return None;
                }
                broken(&mut lines, &mut line, open.len());
                line.push(character);
            }
            ',' => {
                line.push(character);
                broken(&mut lines, &mut line, open.len());
            }
            ':' => line.push_str(": "),
            space if space.is_whitespace() => {
                let before = line.chars().last().is_some_and(bare);
                let after = chars.get(significant(&chars, at)).is_some_and(|c| bare(*c));

                if before && after {
                    return None;
                }
            }
            other => line.push(other),
        }

        at += 1;
    }

    if !open.is_empty() {
        return None;
    }

    lines.push(line);
    Some(lines)
}

/// Copies the string opening at `at` into `line`, and answers with the index
/// of its closing quote. `None` for a string that never closes.
fn string(chars: &[char], at: usize, line: &mut String) -> Option<usize> {
    line.push('"');
    let mut escaped = false;

    for (offset, &character) in chars.iter().enumerate().skip(at + 1) {
        line.push(character);

        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some(offset);
        }
    }

    None
}

/// The index of the first non-whitespace character at or after `from`.
fn significant(chars: &[char], from: usize) -> usize {
    chars
        .iter()
        .skip(from)
        .position(|character| !character.is_whitespace())
        .map_or(chars.len(), |offset| from + offset)
}

/// A character of a number, a literal or a string's quote: what two tokens
/// separated only by whitespace would run together as.
const fn bare(character: char) -> bool {
    !matches!(character, '{' | '}' | '[' | ']' | ',' | ':' | ' ')
}

fn broken(lines: &mut Vec<String>, line: &mut String, depth: usize) {
    lines.push(std::mem::take(line));
    *line = "  ".repeat(depth);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contents::TableRef;

    fn text(value: &str) -> CqlValue {
        CqlValue::Text(value.to_owned())
    }

    #[test]
    fn json_is_indented_two_spaces_a_level_one_member_to_a_line() {
        assert_eq!(
            lines(&text(r#"{"a":1,"b":[true,null],"c":{}}"#)),
            [
                "{",
                r#"  "a": 1,"#,
                r#"  "b": ["#,
                "    true,",
                "    null",
                "  ],",
                r#"  "c": {}"#,
                "}",
            ]
        );
    }

    #[test]
    fn json_keeps_its_key_order_and_its_number_spelling() {
        assert_eq!(
            lines(&text(r#"{"z": 1.50e3, "a": -0}"#)),
            ["{", r#"  "z": 1.50e3,"#, r#"  "a": -0"#, "}"]
        );
    }

    #[test]
    fn braces_and_escaped_quotes_inside_a_string_are_text() {
        assert_eq!(
            lines(&text(r#"{"k":"a {b} [c], \"d\": e"}"#)),
            ["{", r#"  "k": "a {b} [c], \"d\": e""#, "}"]
        );
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_before_detection() {
        assert_eq!(lines(&text("  \n[1, 2]\n ")), ["[", "  1,", "  2", "]"]);
    }

    #[test]
    fn text_that_is_not_json_is_drawn_as_it_is() {
        for raw in [
            r#"{"a": 1"#,
            r#"{"a": "unterminated}"#,
            r#"{"a": 1]"#,
            "[1] trailing",
            "[not json]",
            "plain prose",
        ] {
            assert_eq!(lines(&text(raw)), [raw], "{raw:?}");
        }
    }

    #[test]
    fn a_collection_is_one_element_to_a_line() {
        let value = CqlValue::Map(BTreeMap::from([
            (
                "a".to_owned(),
                CqlValue::List(vec![CqlValue::Int(1), CqlValue::Int(2)]),
            ),
            ("b".to_owned(), CqlValue::Set(Vec::new())),
        ]));

        assert_eq!(
            lines(&value),
            ["{", "  a: [", "    1,", "    2", "  ],", "  b: {}", "}"]
        );
    }

    #[test]
    fn an_empty_collection_stays_on_one_line() {
        assert_eq!(lines(&CqlValue::List(Vec::new())), ["[]"]);
    }

    #[test]
    fn a_blob_and_a_scalar_are_drawn_as_the_grid_draws_them() {
        assert_eq!(lines(&CqlValue::Blob(vec![0xca, 0xfe])), ["0xcafe"]);
        assert_eq!(lines(&CqlValue::Int(7)), ["7"]);
        assert_eq!(lines(&CqlValue::Null), ["null"]);
    }

    #[test]
    fn control_characters_are_dropped_as_the_grid_drops_them() {
        assert_eq!(
            lines(&text("sgr[\u{1b}[31mRED\u{1b}[0m] tab[\t] lf[\n]")),
            ["sgr[[31mRED[0m] tab[] lf[]"]
        );
    }

    fn cell(value: CqlValue) -> Inspector {
        Inspector::opened(
            0,
            Subject::Table(TableRef::new("shop", "orders")),
            Inspected::Cell {
                at: 0,
                column: "note".to_owned(),
                value,
            },
        )
    }

    #[test]
    fn a_cell_is_sized_in_the_bytes_it_holds() {
        assert_eq!(cell(text("héllo")).size().as_deref(), Some("6 B"));
        assert_eq!(
            cell(CqlValue::Blob(vec![0; 2048])).size().as_deref(),
            Some("2.0 KB")
        );
        assert_eq!(cell(CqlValue::Null).size(), None);
    }

    #[test]
    fn a_rows_lines_run_on_from_one_field_to_the_next() {
        let mut row = Inspector::opened(
            0,
            Subject::Table(TableRef::new("shop", "orders")),
            Inspected::Row {
                number: 1,
                columns: vec!["id".to_owned(), "tags".to_owned(), "note".to_owned()],
                values: vec![
                    CqlValue::Int(7),
                    CqlValue::List(vec![text("a"), text("b")]),
                    CqlValue::Null,
                ],
            },
        );

        assert_eq!(row.line_count(), 6, "one, four bracketed, one");
        assert_eq!(row.line().as_deref(), Some("7"));

        row.down();
        row.down();
        assert_eq!(row.line().as_deref(), Some("a,"), "inside the second field");

        row.to_end();
        assert_eq!(row.line(), None, "the null field");
    }

    #[test]
    fn the_cursor_stops_at_the_last_line_of_what_is_open() {
        let mut cell = cell(text("plain prose"));

        cell.page_down(40);

        assert_eq!(cell.cursor(), 0);
    }

    #[test]
    fn the_title_names_the_subject_and_the_column_or_the_row() {
        assert_eq!(
            crate::style::text(&cell(text("x")).title()),
            "shop.orders · note"
        );

        let row = Inspector::opened(
            0,
            Subject::Typed("SELECT *\n  FROM t".to_owned()),
            Inspected::Row {
                number: 3,
                columns: vec!["id".to_owned()],
                values: vec![CqlValue::Int(1)],
            },
        );
        assert_eq!(crate::style::text(&row.title()), "SELECT * FROM t · row 3");
    }

    #[test]
    fn a_cell_yanks_bare_and_a_row_as_one_csv_line() {
        assert_eq!(cell(text("a,b")).yank().as_deref(), Some("a,b"));
        assert_eq!(cell(CqlValue::Null).yank(), None);

        let row = Inspector::opened(
            0,
            Subject::Table(TableRef::new("shop", "orders")),
            Inspected::Row {
                number: 1,
                columns: vec!["id".to_owned(), "note".to_owned()],
                values: vec![CqlValue::Int(1), text("a,b")],
            },
        );
        assert_eq!(row.yank().as_deref(), Some(r#"1,"a,b""#));
    }
}
