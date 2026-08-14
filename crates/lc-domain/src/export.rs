//! Exporting a result set: what it covers, where it goes, and the bytes it
//! becomes (docs/superpowers/specs/2026-08-14-export-design.md).

use std::fmt::Write as _;

use crate::cell;
use crate::model::{CqlValue, Row};
use crate::yank;

/// What an export is written as.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Csv,
    Json,
}

impl Format {
    /// The extension the destination path carries, and the one the format
    /// radio rewrites when it changes.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }

    /// The other one. Exhaustive rather than a boolean flip, so a third
    /// format fails to compile here first.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Csv => Self::Json,
            Self::Json => Self::Csv,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Json => "JSON",
        }
    }
}

/// Turns pages into the text a file holds, one chunk at a time.
///
/// Stateful for one reason: a JSON array needs a separator between objects and
/// the paging loop cannot know whether another page follows until the cluster
/// has answered. Counting what has been written puts the comma before each
/// object but the first, which needs no lookahead.
#[derive(Debug, Clone)]
pub struct Encoder {
    format: Format,
    written: usize,
}

impl Encoder {
    #[must_use]
    pub const fn new(format: Format) -> Self {
        Self { format, written: 0 }
    }

    /// What precedes the first row: a header row, or the opening bracket.
    pub fn start(&mut self, columns: &[String]) -> String {
        match self.format {
            Format::Csv => format!("{}\n", yank::line(columns)),
            Format::Json => "[\n".to_owned(),
        }
    }

    /// `rows` as text, ready to append to whatever `start` and earlier chunks
    /// produced.
    pub fn chunk(&mut self, columns: &[String], rows: &[Row]) -> String {
        let mut out = String::new();

        for row in rows {
            match self.format {
                Format::Csv => {
                    out.push_str(&yank::grid_row(row));
                    out.push('\n');
                }
                Format::Json => {
                    if self.written > 0 {
                        out.push_str(",\n");
                    }
                    out.push_str(&json_object(columns, row));
                }
            }
            self.written += 1;
        }

        out
    }

    /// What follows the last row. A CSV is finished when its last line is; a
    /// JSON array is not a document until it is closed. An empty array has no
    /// row to put a newline after, so it closes on the same line it opened.
    pub fn finish(&mut self) -> String {
        match self.format {
            Format::Csv => String::new(),
            Format::Json if self.written == 0 => "]\n".to_owned(),
            Format::Json => "\n]\n".to_owned(),
        }
    }
}

/// One row as a JSON object. A row shorter than the column list — which a
/// driver should not produce and a fake can — pads with `null` rather than
/// dropping the key, so every object in the array has the same shape.
fn json_object(columns: &[String], row: &Row) -> String {
    let fields: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let value = row.values.get(index).unwrap_or(&CqlValue::Null);

            format!("{}:{}", json_string(column), json_value(value))
        })
        .collect();

    format!("{{{}}}", fields.join(","))
}

/// A value as JSON. Exhaustive over [`CqlValue`], so a new variant has to
/// decide here whether it has a JSON type of its own.
fn json_value(value: &CqlValue) -> String {
    match value {
        CqlValue::Null => "null".to_owned(),
        CqlValue::Boolean(flag) => flag.to_string(),
        CqlValue::Int(number) => number.to_string(),
        // JSON can spell neither NaN nor an infinity, and a document holding
        // one is not a document.
        CqlValue::Float(number) if !number.is_finite() => "null".to_owned(),
        CqlValue::Float(number) => {
            let rendered = number.to_string();
            // `1f64.to_string()` is "1", which JSON reads back as an integer.
            if rendered.contains(['.', 'e', 'E']) {
                rendered
            } else {
                format!("{rendered}.0")
            }
        }
        // No JSON type of its own: a blob, a uuid, a timestamp and the
        // collections travel as the text the grid already draws.
        CqlValue::Text(_)
        | CqlValue::Blob(_)
        | CqlValue::Uuid(_)
        | CqlValue::Timestamp(_)
        | CqlValue::List(_)
        | CqlValue::Set(_)
        | CqlValue::Map(_) => json_string(&cell::text(value)),
    }
}

/// A JSON string literal, escaped as RFC 8259 requires. Control characters
/// below 0x20 have no literal spelling and take the `\uXXXX` form.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');

    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control < ' ' => {
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            character => out.push(character),
        }
    }

    out.push('"');
    out
}

/// One line of editable text with a cursor into it.
///
/// The cursor counts characters, not bytes, so no motion can leave it inside
/// one — the invariant that keeps every slice below safe. Neither
/// [`crate::editor::Editor`] (multi-line, and a panel's) nor
/// [`crate::search::Prompt`] (appends at the end only) fits a path that
/// arrives prefilled and is edited in the middle (design §2.1).
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

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// A character index, which is what a renderer measures a column with.
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

    /// Replaces the trailing extension, if there is one. A path with no
    /// extension gains none: the format radio rewrites what is there rather
    /// than asserting what should be (design §2.1).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CqlValue, Row};

    fn columns() -> Vec<String> {
        vec!["id".to_owned(), "name".to_owned()]
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                values: vec![CqlValue::Int(1), CqlValue::Text("ada".to_owned())],
            },
            Row {
                values: vec![CqlValue::Int(2), CqlValue::Null],
            },
        ]
    }

    #[test]
    fn csv_opens_with_a_header_row() {
        let mut encoder = Encoder::new(Format::Csv);

        assert_eq!(encoder.start(&columns()), "id,name\n");
    }

    #[test]
    fn csv_writes_one_line_per_row_and_ends_every_one() {
        let mut encoder = Encoder::new(Format::Csv);
        encoder.start(&columns());

        assert_eq!(encoder.chunk(&columns(), &rows()), "1,ada\n2,\n");
        assert_eq!(
            encoder.finish(),
            "",
            "a csv is finished when its last line is"
        );
    }

    #[test]
    fn a_csv_field_holding_a_comma_is_quoted_by_the_yank_encoder() {
        let mut encoder = Encoder::new(Format::Csv);
        let row = vec![Row {
            values: vec![CqlValue::Int(1), CqlValue::Text("last, first".to_owned())],
        }];

        assert_eq!(encoder.chunk(&columns(), &row), "1,\"last, first\"\n");
    }

    #[test]
    fn json_is_one_array_of_objects() {
        let mut encoder = Encoder::new(Format::Json);
        let opened = encoder.start(&columns());
        let body = encoder.chunk(&columns(), &rows());
        let closed = encoder.finish();

        assert_eq!(
            format!("{opened}{body}{closed}"),
            "[\n{\"id\":1,\"name\":\"ada\"},\n{\"id\":2,\"name\":null}\n]\n"
        );
    }

    /// The comma goes *before* each object but the first, so a chunk never has
    /// to know whether another one follows it — which the paging loop cannot
    /// tell it until the cluster has already answered.
    #[test]
    fn json_separates_objects_across_chunk_boundaries() {
        let mut encoder = Encoder::new(Format::Json);
        let opened = encoder.start(&columns());
        let first = encoder.chunk(&columns(), &rows()[..1]);
        let second = encoder.chunk(&columns(), &rows()[1..]);
        let closed = encoder.finish();

        assert_eq!(
            format!("{opened}{first}{second}{closed}"),
            "[\n{\"id\":1,\"name\":\"ada\"},\n{\"id\":2,\"name\":null}\n]\n"
        );
    }

    #[test]
    fn an_empty_json_export_is_still_a_document() {
        let mut encoder = Encoder::new(Format::Json);
        let opened = encoder.start(&columns());

        assert_eq!(format!("{opened}{}", encoder.finish()), "[\n]\n");
    }

    #[test]
    fn json_strings_escape_what_json_requires() {
        assert_eq!(
            json_value(&CqlValue::Text("a\"b\\c\nd\te\u{1}f".to_owned())),
            "\"a\\\"b\\\\c\\nd\\te\\u0001f\""
        );
    }

    #[test]
    fn json_renders_the_scalar_types_unquoted() {
        assert_eq!(json_value(&CqlValue::Int(-7)), "-7");
        assert_eq!(json_value(&CqlValue::Float(1.5)), "1.5");
        assert_eq!(json_value(&CqlValue::Boolean(true)), "true");
        assert_eq!(json_value(&CqlValue::Null), "null");
    }

    /// JSON has no spelling for either, and a document holding `NaN` is not a
    /// document. `null` is the only honest answer.
    #[test]
    fn a_float_json_cannot_write_becomes_null() {
        assert_eq!(json_value(&CqlValue::Float(f64::NAN)), "null");
        assert_eq!(json_value(&CqlValue::Float(f64::INFINITY)), "null");
    }

    /// A blob, a uuid, a timestamp and a collection have no JSON type of their
    /// own, so they travel as the text the grid already draws.
    #[test]
    fn everything_without_a_json_type_travels_as_the_text_the_grid_draws() {
        let blob = CqlValue::Blob(vec![0xde, 0xad]);

        assert_eq!(
            json_value(&blob),
            format!("\"{}\"", crate::cell::text(&blob))
        );
    }

    #[test]
    fn a_row_shorter_than_the_column_list_fills_the_gap_with_null() {
        let mut encoder = Encoder::new(Format::Json);
        let short = vec![Row {
            values: vec![CqlValue::Int(1)],
        }];
        encoder.start(&columns());

        assert_eq!(
            encoder.chunk(&columns(), &short),
            "{\"id\":1,\"name\":null}"
        );
    }

    #[test]
    fn a_whole_float_keeps_its_point_so_json_reads_it_back_as_a_float() {
        assert_eq!(json_value(&CqlValue::Float(3.0)), "3.0");
    }

    #[test]
    fn a_format_names_its_extension_and_cycles_to_the_other() {
        assert_eq!(Format::Csv.extension(), "csv");
        assert_eq!(Format::Json.extension(), "json");
        assert_eq!(Format::Csv.next(), Format::Json);
        assert_eq!(Format::Json.next(), Format::Csv);
    }

    #[test]
    fn a_line_opens_with_its_cursor_at_the_end_where_a_filename_is_edited() {
        let line = Line::new("shop.orders.csv".to_owned());

        assert_eq!(line.text(), "shop.orders.csv");
        assert_eq!(line.cursor(), 15);
    }

    #[test]
    fn typing_inserts_at_the_cursor() {
        let mut line = Line::new("ab".to_owned());
        line.left();
        line.insert('X');

        assert_eq!(line.text(), "aXb");
        assert_eq!(line.cursor(), 2);
    }

    #[test]
    fn backspace_takes_the_character_behind_the_cursor_and_delete_the_one_under_it() {
        let mut line = Line::new("abc".to_owned());
        line.left();
        line.delete_back();

        assert_eq!(line.text(), "ac");
        assert_eq!(line.cursor(), 1);

        line.delete_forward();
        assert_eq!(line.text(), "a");
        assert_eq!(line.cursor(), 1);
    }

    #[test]
    fn neither_delete_runs_off_its_end() {
        let mut line = Line::new(String::new());
        line.delete_back();
        line.delete_forward();

        assert_eq!(line.text(), "");
        assert_eq!(line.cursor(), 0);
    }

    #[test]
    fn the_cursor_stops_at_both_ends() {
        let mut line = Line::new("ab".to_owned());
        line.right();
        line.right();
        assert_eq!(line.cursor(), 2);

        line.home();
        line.left();
        assert_eq!(line.cursor(), 0);

        line.end();
        assert_eq!(line.cursor(), 2);
    }

    /// The cursor counts characters, not bytes: a path can hold anything a
    /// filesystem accepts, and slicing one mid-character panics.
    #[test]
    fn a_multi_byte_character_is_one_step_and_one_delete() {
        let mut line = Line::new("aéb".to_owned());
        line.home();
        line.right();
        line.delete_forward();

        assert_eq!(line.text(), "ab");
        assert_eq!(line.cursor(), 1);
    }

    #[test]
    fn setting_the_extension_replaces_the_one_that_is_there() {
        let mut line = Line::new("/home/ike/shop.orders.csv".to_owned());
        line.set_extension("json");

        assert_eq!(line.text(), "/home/ike/shop.orders.json");
    }

    /// A path the user has retyped to end in nothing keeps ending in nothing:
    /// the rewrite replaces an extension, it does not append one (design §2.1).
    #[test]
    fn setting_the_extension_leaves_a_path_that_has_none_alone() {
        let mut line = Line::new("/dev/stdout".to_owned());
        line.set_extension("json");

        assert_eq!(line.text(), "/dev/stdout");
    }

    /// A dot in a directory name is not an extension.
    #[test]
    fn a_dot_before_the_last_separator_is_not_an_extension() {
        let mut line = Line::new("/home/v1.2/orders".to_owned());
        line.set_extension("json");

        assert_eq!(line.text(), "/home/v1.2/orders");
    }

    /// A leading dot is part of the name, not an extension separator: a
    /// destination field that rewrote `.bashrc` to `.json` would destroy the
    /// filename the user typed.
    #[test]
    fn a_dotfile_has_no_extension_to_replace() {
        let mut line = Line::new("/home/ike/.bashrc".to_owned());
        line.set_extension("json");

        assert_eq!(line.text(), "/home/ike/.bashrc");
    }

    #[test]
    fn a_dotfile_with_a_real_extension_keeps_its_leading_dot() {
        let mut line = Line::new("/home/ike/.config.csv".to_owned());
        line.set_extension("json");

        assert_eq!(line.text(), "/home/ike/.config.json");
    }
}
