//! Typing a value back, and the uncommitted set holding it until `ctrl-s`.
//! Unlike [`crate::literal`], `float`, `double`, `varint` and `decimal` are
//! accepted: the text is the user's own, not a re-spelling of the driver's.

use std::collections::BTreeMap;

use crate::line::Line;
use crate::literal;
use crate::model::CqlValue;

/// [`parse`] is the only constructor, so the drawn value and the literal cannot
/// drift.
#[derive(Debug, Clone, PartialEq)]
pub struct Edited {
    value: CqlValue,
    literal: String,
}

impl Edited {
    /// Also what a clean cell is compared against.
    #[must_use]
    pub const fn value(&self) -> &CqlValue {
        &self.value
    }

    #[must_use]
    pub fn literal(&self) -> &str {
        &self.literal
    }
}

/// Asked before a field opens.
#[must_use]
pub fn editable(cql_type: &str) -> bool {
    matches!(
        literal::normalised(cql_type).as_str(),
        "text"
            | "varchar"
            | "ascii"
            | "inet"
            | "date"
            | "time"
            | "timestamp"
            | "uuid"
            | "timeuuid"
            | "tinyint"
            | "smallint"
            | "int"
            | "bigint"
            | "varint"
            | "decimal"
            | "float"
            | "double"
            | "boolean"
            | "blob"
    )
}

/// Only the free-text types; an empty `date` is the cluster's to reject.
#[must_use]
pub fn takes_empty(cql_type: &str) -> bool {
    matches!(
        literal::normalised(cql_type).as_str(),
        "text" | "varchar" | "ascii"
    )
}

/// [`editable`] has already been asked, so `None` always means "not a
/// `<type>`".
#[must_use]
pub fn parse(cql_type: &str, text: &str) -> Option<Edited> {
    let (value, literal) = match literal::normalised(cql_type).as_str() {
        // Not validated beyond being text: the cluster owns those formats.
        "text" | "varchar" | "ascii" | "inet" | "date" | "time" | "timestamp" => {
            (CqlValue::Text(text.to_owned()), literal::quoted(text))
        }
        "uuid" | "timeuuid" => {
            let uuid = text.trim().to_ascii_lowercase();
            is_uuid(&uuid).then(|| (CqlValue::Uuid(uuid.clone()), uuid))?
        }
        "tinyint" | "smallint" | "int" | "bigint" => {
            let number: i64 = text.trim().parse().ok()?;
            (CqlValue::Int(number), number.to_string())
        }
        // Verbatim: an `f64` round trip would rewrite `1.50` and `1e3`, and
        // lose wide `varint` and `decimal`.
        "varint" | "decimal" | "float" | "double" => {
            let number = text.trim();
            let parsed: f64 = number.parse().ok()?;
            if !parsed.is_finite() {
                return None;
            }
            (CqlValue::Float(parsed), number.to_owned())
        }
        "boolean" => {
            let flag = match text.trim().to_ascii_lowercase().as_str() {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            (CqlValue::Boolean(flag), flag.to_string())
        }
        "blob" => {
            let bytes = hex(text.trim())?;
            let literal = crate::cell::text(&CqlValue::Blob(bytes.clone()));
            (CqlValue::Blob(bytes), literal)
        }
        _ => return None,
    };

    Some(Edited { value, literal })
}

/// Lower-case 8-4-4-4-12, the form CQL reads bare.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();

    groups.len() == 5
        && groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|group| group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `0x` alone is the empty blob.
fn hex(text: &str) -> Option<Vec<u8>> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;

    if digits.len() % 2 != 0 {
        return None;
    }

    digits
        .as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommitScope {
    #[default]
    All,
    Row(usize),
}

impl CommitScope {
    #[must_use]
    pub const fn admits(self, row: usize) -> bool {
        match self {
            Self::All => true,
            Self::Row(only) => only == row,
        }
    }
}

/// Keyed by page row, not drawn position, so a sort or filter cannot move an
/// edit to another row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Edits(BTreeMap<(usize, usize), Edited>);

impl Edits {
    /// `None` clears, so an edit back to the original leaves no mark.
    pub fn set(&mut self, row: usize, column: usize, edited: Option<Edited>) {
        match edited {
            Some(edited) => {
                self.0.insert((row, column), edited);
            }
            None => {
                self.0.remove(&(row, column));
            }
        }
    }

    #[must_use]
    pub fn get(&self, row: usize, column: usize) -> Option<&Edited> {
        self.0.get(&(row, column))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn cells(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn rows(&self) -> usize {
        self.by_row().count()
    }

    /// `(row, column)` key order already groups by row in column order.
    pub fn by_row(&self) -> impl Iterator<Item = (usize, Vec<(usize, &Edited)>)> {
        let mut grouped: Vec<(usize, Vec<(usize, &Edited)>)> = Vec::new();

        for ((row, column), edited) in &self.0 {
            match grouped.last_mut() {
                Some((at, columns)) if at == row => columns.push((*column, edited)),
                _ => grouped.push((*row, vec![(*column, edited)])),
            }
        }

        grouped.into_iter()
    }

    #[must_use]
    pub fn cells_in(&self, scope: CommitScope) -> usize {
        self.0.keys().filter(|(row, _)| scope.admits(*row)).count()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// The rest stay dirty.
    pub fn drain_in(
        &mut self,
        scope: CommitScope,
    ) -> impl Iterator<Item = ((usize, usize), Edited)> {
        let (taken, kept): (BTreeMap<_, _>, BTreeMap<_, _>) = std::mem::take(&mut self.0)
            .into_iter()
            .partition(|((row, _), _)| scope.admits(*row));
        self.0 = kept;
        taken.into_iter()
    }
}

/// The type is captured: the overlay claims every key, so the schema cannot
/// change underneath.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    column: usize,
    name: String,
    cql_type: String,
    line: Line,
    /// Tells a field typed to empty from one never touched.
    touched: bool,
}

impl Field {
    /// Seeded with the cell's text, so an immediate `⏎` is a no-op.
    #[must_use]
    pub fn opened(column: usize, name: &str, cql_type: &str, text: String) -> Self {
        Self {
            column,
            name: name.to_owned(),
            cql_type: cql_type.to_owned(),
            line: Line::new(text),
            touched: false,
        }
    }

    #[must_use]
    pub const fn touched(&self) -> bool {
        self.touched
    }

    pub const fn touch(&mut self) {
        self.touched = true;
    }

    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn cql_type(&self) -> &str {
        &self.cql_type
    }

    #[must_use]
    pub const fn line(&self) -> &Line {
        &self.line
    }

    pub const fn line_mut(&mut self) -> &mut Line {
        &mut self.line
    }

    #[must_use]
    pub fn parsed(&self) -> Option<Edited> {
        parse(&self.cql_type, self.line.text())
    }

    #[must_use]
    pub fn error(&self) -> Option<String> {
        self.parsed()
            .is_none()
            .then(|| format!("Not a {}.", literal::normalised(&self.cql_type)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(cql_type: &str, text: &str) -> Option<(CqlValue, String)> {
        parse(cql_type, text).map(|edited| (edited.value, edited.literal))
    }

    #[test]
    fn text_is_quoted_and_an_embedded_quote_doubled() {
        assert_eq!(
            parsed("text", "o'hara"),
            Some((CqlValue::Text("o'hara".to_owned()), "'o''hara'".to_owned()))
        );
    }

    #[test]
    fn an_empty_text_is_a_value_rather_than_a_refusal() {
        assert_eq!(
            parsed("varchar", ""),
            Some((CqlValue::Text(String::new()), "''".to_owned()))
        );
    }

    #[test]
    fn the_types_that_arrive_as_text_are_quoted_and_not_second_guessed() {
        // The cluster owns date formats.
        assert_eq!(
            parsed("date", "2026-01-01"),
            Some((
                CqlValue::Text("2026-01-01".to_owned()),
                "'2026-01-01'".to_owned()
            ))
        );
    }

    #[test]
    fn an_integer_is_bare_and_a_non_integer_is_refused() {
        assert_eq!(
            parsed("int", " -7 "),
            Some((CqlValue::Int(-7), "-7".to_owned()))
        );
        assert_eq!(parsed("bigint", "7.5"), None);
        assert_eq!(parsed("int", "seven"), None);
        assert_eq!(parsed("int", ""), None);
    }

    #[test]
    fn a_number_this_program_did_not_render_is_spelled_verbatim() {
        // Unlike `literal::of`: the text is the user's, so no `f64` round trip.
        assert_eq!(
            parsed("decimal", "1.50"),
            Some((CqlValue::Float(1.5), "1.50".to_owned()))
        );
        assert_eq!(
            parsed("double", "1e3"),
            Some((CqlValue::Float(1000.0), "1e3".to_owned()))
        );
        assert_eq!(parsed("float", "nan"), None);
        assert_eq!(parsed("varint", "inf"), None);
    }

    #[test]
    fn a_uuid_is_accepted_only_in_the_shape_the_driver_renders() {
        assert_eq!(
            parsed("uuid", "8F14E45F-CEEA-467A-9575-9C6D7E0D9F3C"),
            Some((
                CqlValue::Uuid("8f14e45f-ceea-467a-9575-9c6d7e0d9f3c".to_owned()),
                "8f14e45f-ceea-467a-9575-9c6d7e0d9f3c".to_owned()
            ))
        );
        assert_eq!(parsed("timeuuid", "8f14e45f"), None);
        assert_eq!(parsed("uuid", "8f14e45f-ceea-467a-9575-9c6d7e0d9f3g"), None);
    }

    #[test]
    fn a_boolean_is_folded_and_nothing_else_is_one() {
        assert_eq!(
            parsed("boolean", "TRUE"),
            Some((CqlValue::Boolean(true), "true".to_owned()))
        );
        assert_eq!(parsed("boolean", "1"), None);
    }

    #[test]
    fn a_blob_takes_the_form_the_grid_draws_it_in() {
        assert_eq!(
            parsed("blob", "0x000FFF"),
            Some((
                CqlValue::Blob(vec![0x00, 0x0f, 0xff]),
                "0x000fff".to_owned()
            ))
        );
        assert_eq!(
            parsed("blob", "0x"),
            Some((CqlValue::Blob(Vec::new()), "0x".to_owned()))
        );
        assert_eq!(parsed("blob", "000fff"), None, "the marker is not optional");
        assert_eq!(parsed("blob", "0x0ff"), None, "half a byte is not a byte");
    }

    #[test]
    fn a_declaration_keeps_its_meaning_through_case_and_a_frozen_wrapper() {
        assert_eq!(
            parsed("  INT ", "7"),
            Some((CqlValue::Int(7), "7".to_owned()))
        );
        assert!(editable("frozen<uuid>"));
    }

    #[test]
    fn the_types_a_one_line_field_is_the_wrong_instrument_for_are_refused() {
        for cql_type in [
            "list<text>",
            "set<int>",
            "map<text, int>",
            "counter",
            "tuple<int, text>",
            "address",
        ] {
            assert!(!editable(cql_type), "{cql_type}");
            assert_eq!(parse(cql_type, "x"), None, "{cql_type}");
        }
    }

    #[test]
    fn the_four_numbers_literal_of_refuses_are_editable_here() {
        for cql_type in ["float", "double", "varint", "decimal"] {
            assert!(editable(cql_type), "{cql_type}");
        }
    }

    fn edit(text: &str) -> Edited {
        parse("text", text).expect("text takes anything")
    }

    #[test]
    fn setting_none_clears_what_was_there() {
        let mut edits = Edits::default();
        edits.set(1, 2, Some(edit("x")));
        assert_eq!(edits.cells(), 1);

        edits.set(1, 2, None);

        assert!(edits.is_empty());
    }

    #[test]
    fn edits_group_by_row_in_page_order_and_by_column_within_a_row() {
        let mut edits = Edits::default();
        edits.set(9, 3, Some(edit("c")));
        edits.set(2, 5, Some(edit("b")));
        edits.set(9, 1, Some(edit("a")));

        let grouped: Vec<(usize, Vec<usize>)> = edits
            .by_row()
            .map(|(row, columns)| (row, columns.into_iter().map(|(at, _)| at).collect()))
            .collect();

        assert_eq!(grouped, [(2, vec![5]), (9, vec![1, 3])]);
        assert_eq!((edits.cells(), edits.rows()), (3, 2));
    }

    #[test]
    fn a_field_reports_why_it_does_not_parse_and_nothing_when_it_does() {
        let mut field = Field::opened(0, "qty", "INT", "7".to_owned());
        assert_eq!(field.error(), None);

        field.line_mut().insert('x');

        assert_eq!(field.error().as_deref(), Some("Not a int."));
        assert_eq!(field.parsed(), None);
    }
}
