//! Exporting a result set: scope, destination, and encoding.

use std::fmt::Write as _;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use crate::cell;
use crate::config::ClipboardMb;
use crate::contents::Paging;
use crate::contents::Subject;
use crate::model::{CqlValue, JobId, Key, Row};
use crate::style::Segment;
use crate::update::Source;
use crate::yank;

/// Shared with the drop confirmation's typed name.
pub use crate::line::Line;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Csv,
    Json,
}

impl Format {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }

    /// Exhaustive, so a third format fails to compile here.
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

/// Stateful only to put a comma before each JSON object but the first: the
/// paging loop cannot know whether another page follows.
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

    pub fn start(&mut self, columns: &[String]) -> String {
        match self.format {
            Format::Csv => format!("{}\n", yank::line(columns)),
            Format::Json => "[\n".to_owned(),
        }
    }

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

    /// An empty array closes on the line it opened.
    pub fn finish(&mut self) -> String {
        match self.format {
            Format::Csv => String::new(),
            Format::Json if self.written == 0 => "]\n".to_owned(),
            Format::Json => "\n]\n".to_owned(),
        }
    }
}

/// A short row pads with `null`, so every object has the same shape.
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

/// Exhaustive, so a new variant must decide its JSON type.
fn json_value(value: &CqlValue) -> String {
    match value {
        CqlValue::Null => "null".to_owned(),
        CqlValue::Boolean(flag) => flag.to_string(),
        CqlValue::Int(number) => number.to_string(),
        // JSON cannot spell NaN or infinity.
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
        // No JSON type: travel as the text the grid draws.
        CqlValue::Text(_)
        | CqlValue::Blob(_)
        | CqlValue::Uuid(_)
        | CqlValue::Timestamp(_)
        | CqlValue::List(_)
        | CqlValue::Set(_)
        | CqlValue::Map(_) => json_string(&cell::text(value)),
    }
}

/// Control characters below 0x20 take the `\uXXXX` form.
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// Re-run with no limit of ours.
    #[default]
    Whole,
    Fetched,
    First,
}

impl Scope {
    const ALL: [Self; 3] = [Self::Fetched, Self::Whole, Self::First];

    fn step(self, forwards: bool) -> Self {
        let at = Self::ALL
            .iter()
            .position(|scope| *scope == self)
            .unwrap_or(0);
        let len = Self::ALL.len();
        let to = if forwards { at + 1 } else { at + len - 1 };

        Self::ALL[to % len]
    }
}

const DEFAULT_ROWS: &str = "1000";

/// "Everything, and there is more" is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetched {
    /// Both scopes would write the same file.
    Everything(usize),
    Some(usize),
}

impl Fetched {
    /// A filter makes the scopes differ even when the result ended.
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

/// Where a run sends the encoded result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    File { path: PathBuf, overwrite: bool },
    Clipboard(ClipboardMb),
}

impl Destination {
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::File { path, .. } => path.display().to_string(),
            Self::Clipboard(_) => "the clipboard".to_owned(),
        }
    }
}

/// A copy carries its text back: OSC 52 is written from the main loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exported {
    Written(usize),
    Copied { rows: usize, text: String },
}

impl Exported {
    #[must_use]
    pub const fn rows(&self) -> usize {
        match self {
            Self::Written(rows) | Self::Copied { rows, .. } => *rows,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    File,
    Clipboard,
}

impl Button {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "Export to file",
            Self::Clipboard => "Copy to clipboard",
        }
    }

    /// Behind ctrl: the path stop takes every printable key as text.
    #[must_use]
    pub const fn chord(self) -> char {
        match self {
            Self::File => 's',
            Self::Clipboard => 'y',
        }
    }

    #[must_use]
    pub fn of_chord(key: Key) -> Option<Self> {
        [Self::File, Self::Clipboard]
            .into_iter()
            .find(|button| key == Key::Char(button.chord()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Path { cursor: usize },
    Format(Format),
    Scope(Scope),
    Rows { cursor: usize },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Stop {
    /// The one stop where a printable key is text.
    #[default]
    Path,
    Format,
    /// Skipped while both scopes would write the same file.
    Scope,
    /// Skipped unless that scope is chosen; digits only.
    Rows,
    Button(Button),
}

impl Stop {
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Path => Self::Format,
            Self::Format => Self::Scope,
            Self::Scope => Self::Rows,
            Self::Rows => Self::Button(Button::File),
            Self::Button(Button::File) => Self::Button(Button::Clipboard),
            Self::Button(Button::Clipboard) => Self::Path,
        }
    }

    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Path => Self::Button(Button::Clipboard),
            Self::Format => Self::Path,
            Self::Scope => Self::Format,
            Self::Rows => Self::Scope,
            Self::Button(Button::File) => Self::Rows,
            Self::Button(Button::Clipboard) => Self::Button(Button::File),
        }
    }

    #[must_use]
    pub const fn takes_text(self) -> bool {
        matches!(self, Self::Path | Self::Rows)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    Overwrite,
}

/// Carries the refused attempt's source and `JobId`: the panel may have moved
/// on, and a retry must close the same trail entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Pending {
    Overwrite(Box<Source>, JobId),
}

impl Pending {
    #[must_use]
    pub const fn confirm(&self) -> Confirm {
        match self {
            Self::Overwrite(..) => Confirm::Overwrite,
        }
    }

    #[must_use]
    pub fn source(&self) -> &Source {
        match self {
            Self::Overwrite(source, _) => source,
        }
    }

    #[must_use]
    pub const fn job(&self) -> JobId {
        match self {
            Self::Overwrite(_, job) => *job,
        }
    }
}

const TITLE_LIMIT: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub struct ExportForm {
    subject: Subject,
    fetched: Fetched,
    stop: Stop,
    scope: Scope,
    format: Format,
    path: Line,
    rows: Line,
    pending: Option<Pending>,
}

impl ExportForm {
    /// `cwd` is passed in: the domain reads no environment.
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
            rows: Line::new(DEFAULT_ROWS.to_owned()),
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
    pub const fn rows(&self) -> &Line {
        &self.rows
    }

    pub const fn rows_mut(&mut self) -> &mut Line {
        &mut self.rows
    }

    #[must_use]
    pub fn row_limit(&self) -> Option<NonZeroUsize> {
        self.rows.text().parse().ok()
    }

    #[must_use]
    pub const fn subject(&self) -> &Subject {
        &self.subject
    }

    #[must_use]
    pub const fn fetched(&self) -> Fetched {
        self.fetched
    }

    #[must_use]
    pub fn confirm(&self) -> Option<Confirm> {
        self.pending.as_ref().map(Pending::confirm)
    }

    #[must_use]
    pub const fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }

    pub fn ask(&mut self, pending: Pending) {
        self.pending = Some(pending);
    }

    /// The held export goes with the question.
    pub fn dismiss(&mut self) {
        self.pending = None;
    }

    #[must_use]
    pub const fn fetched_enabled(&self) -> bool {
        !self.fetched.complete()
    }

    pub fn tab(&mut self) {
        loop {
            self.stop = self.stop.next();
            if self.reachable(self.stop) {
                return;
            }
        }
    }

    pub fn back_tab(&mut self) {
        loop {
            self.stop = self.stop.previous();
            if self.reachable(self.stop) {
                return;
            }
        }
    }

    fn reachable(&self, stop: Stop) -> bool {
        match stop {
            Stop::Scope => self.fetched_enabled(),
            Stop::Rows => self.fetched_enabled() && self.scope == Scope::First,
            Stop::Path | Stop::Format | Stop::Button(_) => true,
        }
    }

    /// `Some` only from a [`Stop::Button`].
    pub fn activate(&mut self) -> Option<Button> {
        match self.stop {
            Stop::Scope => {
                self.step_scope(true);
                None
            }
            Stop::Format => {
                self.toggle_format();
                None
            }
            Stop::Path | Stop::Rows => None,
            Stop::Button(button) => Some(button),
        }
    }

    /// A scope the form does not offer cannot be pressed.
    pub fn press(&mut self, field: Field) {
        match field {
            Field::Path { cursor } => {
                self.stop = Stop::Path;
                self.path.set_cursor(cursor);
            }
            Field::Format(format) => {
                self.stop = Stop::Format;
                if format != self.format {
                    self.toggle_format();
                }
            }
            Field::Scope(scope) if self.fetched_enabled() => {
                self.stop = Stop::Scope;
                self.scope = scope;
            }
            Field::Rows { cursor } if self.fetched_enabled() => {
                self.stop = Stop::Rows;
                self.scope = Scope::First;
                self.rows.set_cursor(cursor);
            }
            Field::Scope(_) | Field::Rows { .. } => {}
        }
    }

    pub const fn focus_button(&mut self, button: Button) {
        self.stop = Stop::Button(button);
    }

    /// Two options per group, so a move equals a toggle.
    pub fn down(&mut self) {
        self.move_within_group(true);
    }

    pub fn up(&mut self) {
        self.move_within_group(false);
    }

    /// Where the result ended, the first is the only one drawn, as a sentence.
    #[must_use]
    pub fn scope_labels(&self) -> [String; 3] {
        let what = if self.subject.table().is_some() {
            "table"
        } else {
            "query result"
        };
        let all = match self.fetched {
            Fetched::Everything(rows) => format!("Export all {rows} rows"),
            Fetched::Some(rows) => format!("Export the {rows} visible rows"),
        };

        [
            all,
            format!("Export the whole {what}"),
            "Export the first".to_owned(),
        ]
    }

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

    fn move_within_group(&mut self, forwards: bool) {
        match self.stop {
            Stop::Scope => self.step_scope(forwards),
            Stop::Format => self.toggle_format(),
            Stop::Path | Stop::Rows | Stop::Button(_) => {}
        }
    }

    fn step_scope(&mut self, forwards: bool) {
        if self.fetched_enabled() {
            self.scope = self.scope.step(forwards);
        }
    }

    fn toggle_format(&mut self) {
        self.format = self.format.next();
        self.path.set_extension(self.format.extension());
    }
}

fn stem(subject: &Subject) -> String {
    match subject {
        Subject::Table(table) => table.qualified(),
        Subject::Typed(_) => "query".to_owned(),
    }
}

/// On a character boundary: a CQL literal can hold anything.
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

    #[test]
    fn a_float_json_cannot_write_becomes_null() {
        assert_eq!(json_value(&CqlValue::Float(f64::NAN)), "null");
        assert_eq!(json_value(&CqlValue::Float(f64::INFINITY)), "null");
    }

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

    #[test]
    fn setting_the_extension_leaves_a_path_that_has_none_alone() {
        let mut line = Line::new("/dev/stdout".to_owned());
        line.set_extension("json");

        assert_eq!(line.text(), "/dev/stdout");
    }

    #[test]
    fn a_dot_before_the_last_separator_is_not_an_extension() {
        let mut line = Line::new("/home/v1.2/orders".to_owned());
        line.set_extension("json");

        assert_eq!(line.text(), "/home/v1.2/orders");
    }

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
    fn a_form_opens_on_the_path_as_csv_over_the_whole_scope_with_the_path_prefilled() {
        let form = form(Fetched::Some(200));

        assert_eq!(form.stop(), Stop::Path);
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
    fn tab_walks_the_stops_and_both_buttons_and_wraps() {
        let mut form = form(Fetched::Some(200));

        for expected in [
            Stop::Format,
            Stop::Scope,
            Stop::Button(Button::File),
            Stop::Button(Button::Clipboard),
            Stop::Path,
        ] {
            form.tab();
            assert_eq!(form.stop(), expected);
        }
    }

    #[test]
    fn shift_tab_walks_them_backwards() {
        let mut form = form(Fetched::Some(200));

        for expected in [
            Stop::Button(Button::Clipboard),
            Stop::Button(Button::File),
            Stop::Scope,
            Stop::Format,
            Stop::Path,
        ] {
            form.back_tab();
            assert_eq!(form.stop(), expected);
        }
    }

    #[test]
    fn space_on_a_button_reports_which_one_ran() {
        let mut form = form(Fetched::Some(200));
        form.back_tab();

        assert_eq!(form.activate(), Some(Button::Clipboard));
        form.back_tab();
        assert_eq!(form.activate(), Some(Button::File));
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
        form.tab();
        form.tab();
        form.up();
        assert_eq!(form.scope(), Scope::Fetched);
        form.down();
        assert_eq!(form.scope(), Scope::Whole);
    }

    #[test]
    fn tab_skips_the_scope_when_both_scopes_write_the_same_file() {
        let mut form = form(Fetched::Everything(6));

        for expected in [
            Stop::Format,
            Stop::Button(Button::File),
            Stop::Button(Button::Clipboard),
            Stop::Path,
        ] {
            form.tab();
            assert_eq!(form.stop(), expected);
        }
        for expected in [
            Stop::Button(Button::Clipboard),
            Stop::Button(Button::File),
            Stop::Format,
            Stop::Path,
        ] {
            form.back_tab();
            assert_eq!(form.stop(), expected);
        }
    }

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
    fn a_complete_result_is_one_sentence_naming_its_count() {
        let form = form(Fetched::Everything(137));

        assert_eq!(form.scope_labels()[0], "Export all 137 rows");
    }

    #[test]
    fn an_incomplete_result_names_its_three_scopes() {
        let form = form(Fetched::Some(200));

        assert_eq!(
            form.scope_labels(),
            [
                "Export the 200 visible rows".to_owned(),
                "Export the whole table".to_owned(),
                "Export the first".to_owned(),
            ]
        );
    }

    #[test]
    fn a_typed_subject_names_its_whole_scope_a_query_result() {
        let form = ExportForm::opened(
            Subject::Typed("SELECT * FROM shop.orders".to_owned()),
            Fetched::Some(200),
            Path::new(""),
        );

        assert_eq!(form.scope_labels()[1], "Export the whole query result");
    }

    #[test]
    fn space_walks_the_three_scopes_and_wraps() {
        let mut form = form(Fetched::Some(200));
        form.tab();
        form.tab();

        for expected in [Scope::First, Scope::Fetched, Scope::Whole] {
            form.activate();
            assert_eq!(form.scope(), expected);
        }
    }

    #[test]
    fn the_rows_stop_is_reachable_only_under_the_first_scope() {
        let mut form = form(Fetched::Some(200));
        form.tab();
        form.tab();
        form.tab();
        assert_eq!(
            form.stop(),
            Stop::Button(Button::File),
            "Whole has no count to type"
        );

        form.back_tab();
        form.activate();
        assert_eq!(form.scope(), Scope::First);
        form.tab();
        assert_eq!(form.stop(), Stop::Rows);
        form.tab();
        assert_eq!(form.stop(), Stop::Button(Button::File));
    }

    #[test]
    fn the_row_limit_is_a_positive_number_or_nothing() {
        let mut form = form(Fetched::Some(200));
        assert_eq!(form.row_limit().map(NonZeroUsize::get), Some(1000));

        form.rows_mut().delete_back();
        form.rows_mut().delete_back();
        form.rows_mut().delete_back();
        form.rows_mut().delete_back();
        assert_eq!(form.row_limit(), None, "empty is no number");

        form.rows_mut().insert('0');
        assert_eq!(form.row_limit(), None, "zero rows is not an export");
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
