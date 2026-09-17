//! Exporting a result set: what it covers, where it goes, and the bytes it
//! becomes (docs/superpowers/specs/2026-08-14-export-design.md).

use std::fmt::Write as _;
use std::path::Path;

use crate::cell;
use crate::contents::Paging;
use crate::contents::Subject;
use crate::model::{CqlValue, JobId, Row};
use crate::style::Segment;
use crate::update::Source;
use crate::yank;

/// The form's path field. Re-exported rather than moved wholesale, so a
/// reader of the form finds it where the form is: the type is shared with the
/// drop confirmation's typed name (drop design §2.1), which is why it lives
/// in [`crate::line`].
pub use crate::line::Line;

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

/// How much of the result an export covers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// Re-run the statement with no limit of ours and page to the end.
    #[default]
    Whole,
    /// The rows the panel already holds.
    Fetched,
}

/// The rows in memory, and whether the statement has more behind them.
///
/// An enum rather than a count and a flag: "everything, and there is more"
/// cannot be written down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetched {
    /// The result ended in what was fetched, so both scopes would write the
    /// same file.
    Everything(usize),
    Some(usize),
}

impl Fetched {
    /// What the panel holds: `fetched` is what the cluster returned and
    /// `drawn` what the filters left of it.
    ///
    /// A filter makes the two scopes differ even where the result ended: the
    /// whole table and the rows on screen are different files as soon as one
    /// row is hidden, so a filtered grid always offers the Fetched scope
    /// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §4).
    #[must_use]
    pub const fn of_drawn(paging: &Paging, fetched: usize, drawn: usize) -> Self {
        if matches!(paging, Paging::Ended) && drawn == fetched {
            Self::Everything(drawn)
        } else {
            Self::Some(drawn)
        }
    }

    #[must_use]
    pub const fn count(self) -> usize {
        match self {
            Self::Everything(rows) | Self::Some(rows) => rows,
        }
    }

    #[must_use]
    const fn complete(self) -> bool {
        matches!(self, Self::Everything(_))
    }
}

/// Where `Tab` is in the form.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Stop {
    #[default]
    Scope,
    Format,
    /// The one stop where a printable key is text (design §2).
    Path,
    Export,
}

impl Stop {
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Scope => Self::Format,
            Self::Format => Self::Path,
            Self::Path => Self::Export,
            Self::Export => Self::Scope,
        }
    }

    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Scope => Self::Export,
            Self::Format => Self::Scope,
            Self::Path => Self::Format,
            Self::Export => Self::Path,
        }
    }

    /// Whether this stop claims printable keys as text.
    #[must_use]
    pub const fn takes_text(self) -> bool {
        matches!(self, Self::Path)
    }
}

/// What the form is asking before it will act, as the form draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    /// The destination exists. `Esc` goes back to the form (design §7).
    Overwrite,
}

/// A question the form is asking, and the export answering it runs.
///
/// [`Confirm`] is this without its payload — what a renderer needs and all it
/// needs. The payload is the point: the panel underneath is live for the whole
/// of an export, so the rows and the statement the refused attempt was made
/// with are carried here rather than read again from a Contents panel the user
/// is free to move meanwhile (final review I3).
///
/// Carries the [`JobId`] the refused attempt logged its pending trail entry
/// under, alongside the source: a retry reuses it rather than minting a
/// fresh one, so the trail keeps one entry for one export rather than
/// stranding the first as permanently `⋯ running` (final review I4).
#[derive(Debug, Clone, PartialEq)]
pub enum Pending {
    Overwrite(Box<Source>, JobId),
}

impl Pending {
    /// The question, without what answering it would write.
    #[must_use]
    pub const fn confirm(&self) -> Confirm {
        match self {
            Self::Overwrite(..) => Confirm::Overwrite,
        }
    }

    /// What answering it writes.
    #[must_use]
    pub fn source(&self) -> &Source {
        match self {
            Self::Overwrite(source, _) => source,
        }
    }

    /// The trail entry the refused attempt is still pending on.
    #[must_use]
    pub const fn job(&self) -> JobId {
        match self {
            Self::Overwrite(_, job) => *job,
        }
    }
}

/// How many characters of a typed statement the title shows.
const TITLE_LIMIT: usize = 100;

/// The open export form.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportForm {
    subject: Subject,
    fetched: Fetched,
    stop: Stop,
    scope: Scope,
    format: Format,
    path: Line,
    pending: Option<Pending>,
}

impl ExportForm {
    /// Opens over `subject`, prefilling the path from `cwd`.
    ///
    /// `cwd` is passed rather than read: the domain cannot reach the process's
    /// working directory and must not learn how (design §2.1).
    #[must_use]
    pub fn opened(subject: Subject, fetched: Fetched, cwd: &Path) -> Self {
        let format = Format::default();
        let name = format!("{}.{}", stem(&subject), format.extension());
        let path = cwd.join(&name).to_string_lossy().into_owned();

        Self {
            subject,
            fetched,
            stop: Stop::default(),
            scope: Scope::default(),
            format,
            path: Line::new(path),
            pending: None,
        }
    }

    #[must_use]
    pub const fn stop(&self) -> Stop {
        self.stop
    }

    #[must_use]
    pub const fn scope(&self) -> Scope {
        self.scope
    }

    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    #[must_use]
    pub const fn path(&self) -> &Line {
        &self.path
    }

    pub const fn path_mut(&mut self) -> &mut Line {
        &mut self.path
    }

    #[must_use]
    pub const fn subject(&self) -> &Subject {
        &self.subject
    }

    #[must_use]
    pub const fn fetched(&self) -> Fetched {
        self.fetched
    }

    /// The question up in front of the form, if there is one.
    #[must_use]
    pub fn confirm(&self) -> Option<Confirm> {
        self.pending.as_ref().map(Pending::confirm)
    }

    /// What answering that question would export.
    #[must_use]
    pub const fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }

    /// Puts a question in front of the form, holding what answering it runs.
    pub fn ask(&mut self, pending: Pending) {
        self.pending = Some(pending);
    }

    /// `Esc` from the confirmation: the question goes, and the export it was
    /// holding goes with it — a source with nothing asking about it is what
    /// this type exists to prevent (design §7).
    pub fn dismiss(&mut self) {
        self.pending = None;
    }

    /// Whether the fetched-rows scope can be chosen at all (design §3).
    #[must_use]
    pub const fn fetched_enabled(&self) -> bool {
        !self.fetched.complete()
    }

    pub const fn tab(&mut self) {
        self.stop = self.stop.next();
    }

    pub const fn back_tab(&mut self) {
        self.stop = self.stop.previous();
    }

    /// `Space`. Reports whether the export should run — true only from
    /// [`Stop::Export`].
    pub fn activate(&mut self) -> bool {
        match self.stop {
            Stop::Scope => {
                self.toggle_scope();
                false
            }
            Stop::Format => {
                self.toggle_format();
                false
            }
            Stop::Path => false,
            Stop::Export => true,
        }
    }

    /// `j`/`↓` inside a radio group. The groups have two options each, so a
    /// move in either direction reaches the same place a toggle does.
    pub fn down(&mut self) {
        self.move_within_group();
    }

    pub fn up(&mut self) {
        self.move_within_group();
    }

    /// The two scope options, in order, as the form draws them.
    #[must_use]
    pub fn scope_labels(&self) -> [String; 2] {
        let whole = match (self.subject.table(), self.fetched) {
            (None, Fetched::Everything(rows)) => {
                format!("Export the whole query result ({rows})")
            }
            (None, Fetched::Some(_)) => "Export the whole query result".to_owned(),
            (Some(_), Fetched::Everything(rows)) => format!("Export the whole table ({rows})"),
            (Some(_), Fetched::Some(_)) => "Export the whole table".to_owned(),
        };

        [
            whole,
            format!("Export the {} fetched rows", self.fetched.count()),
        ]
    }

    /// What the form's border says it is about (design §1).
    ///
    /// The table's name keeps the schema's colours the tree and the Table
    /// contents title give it ([`TableRef::segments`]).
    #[must_use]
    pub fn title(&self) -> Vec<Segment> {
        match &self.subject {
            Subject::Table(table) => {
                let mut segments = vec![Segment::plain("Export table ")];
                segments.extend(table.segments());
                segments
            }
            Subject::Typed(_) => vec![Segment::plain(format!(
                "Export query {}",
                cut(&self.subject.label())
            ))],
        }
    }

    fn move_within_group(&mut self) {
        match self.stop {
            Stop::Scope => self.toggle_scope(),
            Stop::Format => self.toggle_format(),
            Stop::Path | Stop::Export => {}
        }
    }

    fn toggle_scope(&mut self) {
        if !self.fetched_enabled() {
            return;
        }

        self.scope = match self.scope {
            Scope::Whole => Scope::Fetched,
            Scope::Fetched => Scope::Whole,
        };
    }

    fn toggle_format(&mut self) {
        self.format = self.format.next();
        self.path.set_extension(self.format.extension());
    }
}

/// The default filename, without an extension.
fn stem(subject: &Subject) -> String {
    match subject {
        Subject::Table(table) => table.qualified(),
        Subject::Typed(_) => "query".to_owned(),
    }
}

/// `label` cut to [`TITLE_LIMIT`] characters, ellipsised. Cutting on a
/// character boundary rather than a byte one: a CQL literal can hold anything.
fn cut(label: &str) -> String {
    if label.chars().count() <= TITLE_LIMIT {
        return label.to_owned();
    }

    let head: String = label.chars().take(TITLE_LIMIT).collect();

    format!("{head}…")
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

    use crate::contents::{Subject, TableRef};
    use std::path::Path;

    fn form(fetched: Fetched) -> ExportForm {
        ExportForm::opened(
            Subject::Table(TableRef::new("shop", "orders")),
            fetched,
            Path::new("/home/ike"),
        )
    }

    #[test]
    fn a_form_opens_on_the_whole_scope_as_csv_with_the_path_prefilled() {
        let form = form(Fetched::Some(200));

        assert_eq!(form.stop(), Stop::Scope);
        assert_eq!(form.scope(), Scope::Whole);
        assert_eq!(form.format(), Format::Csv);
        assert_eq!(form.path().text(), "/home/ike/shop.orders.csv");
    }

    #[test]
    fn a_typed_subject_is_prefilled_as_query() {
        let form = ExportForm::opened(
            Subject::Typed("SELECT * FROM shop.orders".to_owned()),
            Fetched::Some(200),
            Path::new("/home/ike"),
        );

        assert_eq!(form.path().text(), "/home/ike/query.csv");
    }

    #[test]
    fn no_cwd_prefills_a_bare_relative_name() {
        let form = ExportForm::opened(
            Subject::Table(TableRef::new("shop", "orders")),
            Fetched::Some(200),
            Path::new(""),
        );

        assert_eq!(form.path().text(), "shop.orders.csv");
    }

    #[test]
    fn tab_walks_the_four_stops_and_wraps() {
        let mut form = form(Fetched::Some(200));

        for expected in [Stop::Format, Stop::Path, Stop::Export, Stop::Scope] {
            form.tab();
            assert_eq!(form.stop(), expected);
        }
    }

    #[test]
    fn shift_tab_walks_them_backwards() {
        let mut form = form(Fetched::Some(200));

        for expected in [Stop::Export, Stop::Path, Stop::Format, Stop::Scope] {
            form.back_tab();
            assert_eq!(form.stop(), expected);
        }
    }

    #[test]
    fn space_on_the_scope_stop_moves_to_the_other_option_and_back() {
        let mut form = form(Fetched::Some(200));

        assert!(!form.activate(), "a radio does not run the export");
        assert_eq!(form.scope(), Scope::Fetched);
        form.activate();
        assert_eq!(form.scope(), Scope::Whole);
    }

    #[test]
    fn space_on_the_export_stop_reports_that_it_ran() {
        let mut form = form(Fetched::Some(200));
        form.tab();
        form.tab();
        form.tab();

        assert_eq!(form.stop(), Stop::Export);
        assert!(form.activate());
    }

    #[test]
    fn space_on_the_format_stop_rewrites_the_paths_extension() {
        let mut form = form(Fetched::Some(200));
        form.tab();
        form.activate();

        assert_eq!(form.format(), Format::Json);
        assert_eq!(form.path().text(), "/home/ike/shop.orders.json");
    }

    #[test]
    fn j_and_k_move_within_a_radio_group_too() {
        let mut form = form(Fetched::Some(200));
        form.down();
        assert_eq!(form.scope(), Scope::Fetched);
        form.up();
        assert_eq!(form.scope(), Scope::Whole);
    }

    /// Everything already fetched means the two scopes would write the same
    /// file, and one of the two is the honest name for it (design §3).
    #[test]
    fn a_result_that_is_already_complete_disables_the_fetched_scope() {
        let mut form = form(Fetched::Everything(137));

        assert!(!form.fetched_enabled());
        form.activate();
        assert_eq!(
            form.scope(),
            Scope::Whole,
            "the disabled option is unreachable"
        );
        form.down();
        assert_eq!(form.scope(), Scope::Whole);
    }

    #[test]
    fn a_disabled_fetched_scope_moves_its_count_onto_the_other_label() {
        let form = form(Fetched::Everything(137));

        assert_eq!(
            form.scope_labels(),
            [
                "Export the whole table (137)".to_owned(),
                "Export the 137 fetched rows".to_owned()
            ]
        );
    }

    #[test]
    fn an_incomplete_result_names_both_scopes_plainly() {
        let form = form(Fetched::Some(200));

        assert_eq!(
            form.scope_labels(),
            [
                "Export the whole table".to_owned(),
                "Export the 200 fetched rows".to_owned()
            ]
        );
    }

    /// A typed statement may carry its own LIMIT, so "the whole table" would
    /// be a claim about something that is not a table (design §3).
    #[test]
    fn a_typed_subject_names_its_whole_scope_a_query_result() {
        let form = ExportForm::opened(
            Subject::Typed("SELECT * FROM shop.orders".to_owned()),
            Fetched::Some(200),
            Path::new(""),
        );

        assert_eq!(form.scope_labels()[0], "Export the whole query result");
    }

    /// The count belongs on whichever label survives, and a typed subject's
    /// label survives the same way a table's does (design §3).
    #[test]
    fn a_complete_typed_result_carries_its_count_too() {
        let form = ExportForm::opened(
            Subject::Typed("SELECT * FROM shop.orders".to_owned()),
            Fetched::Everything(137),
            Path::new(""),
        );

        assert_eq!(
            form.scope_labels(),
            [
                "Export the whole query result (137)".to_owned(),
                "Export the 137 fetched rows".to_owned()
            ]
        );
    }

    #[test]
    fn a_table_form_is_titled_by_its_table() {
        assert_eq!(
            crate::style::text(&form(Fetched::Some(200)).title()),
            "Export table shop.orders"
        );
    }

    #[test]
    fn a_long_query_title_is_cut_to_a_hundred_characters_with_an_ellipsis() {
        let cql = format!(
            "SELECT * FROM shop.orders WHERE note = '{}'",
            "x".repeat(200)
        );
        let form = ExportForm::opened(Subject::Typed(cql), Fetched::Some(200), Path::new(""));
        let title = crate::style::text(&form.title());

        assert!(title.starts_with("Export query SELECT * FROM shop.orders"));
        assert!(title.ends_with('…'));
        assert_eq!(
            title.trim_start_matches("Export query ").chars().count(),
            101,
            "a hundred characters and the ellipsis"
        );
    }

    /// Cutting on a byte boundary would panic; cutting on a character one
    /// cannot.
    #[test]
    fn a_query_title_cuts_on_a_character_boundary() {
        let cql = format!("SELECT '{}'", "é".repeat(200));
        let form = ExportForm::opened(Subject::Typed(cql), Fetched::Some(200), Path::new(""));

        assert!(crate::style::text(&form.title()).ends_with('…'));
    }

    #[test]
    fn a_fresh_form_is_not_confirming_anything() {
        assert_eq!(form(Fetched::Some(200)).confirm(), None);
    }

    #[test]
    fn a_filtered_grid_offers_the_fetched_scope_even_where_the_result_ended() {
        let more = Paging::More {
            stmt: crate::model::Statement::select_all("shop", "orders"),
            cursor: crate::model::PagingState(vec![1]),
            fetch: crate::contents::Fetch::Idle,
        };

        assert_eq!(
            Fetched::of_drawn(&Paging::Ended, 12, 12),
            Fetched::Everything(12),
            "a short page drawn whole is both scopes at once"
        );
        assert_eq!(
            Fetched::of_drawn(&Paging::Ended, 12, 5),
            Fetched::Some(5),
            "a filter makes the rows on screen a different file from the table"
        );
        assert_eq!(
            Fetched::of_drawn(&more, 200, 200),
            Fetched::Some(200),
            "and a result with pages left still is"
        );
    }
}
