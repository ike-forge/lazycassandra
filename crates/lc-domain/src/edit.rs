//! Typing a value back: what a column accepts, and the uncommitted set that
//! holds it until `ctrl-s`
//! (docs/superpowers/specs/2026-09-18-cell-editing-design.md).
//!
//! [`crate::literal`] run backwards, and deliberately not the same whitelist.
//! That one re-spells a rendering *the driver produced*, so it refuses
//! `float`, `double`, `varint` and `decimal`: a key compared for equality
//! against a number this program re-spelled is a row it might not find. Here
//! the text is the user's own and authoritative, so those four are accepted
//! — the refusal was never about the type (design §3).

use std::collections::BTreeMap;

use crate::line::Line;
use crate::literal;
use crate::model::CqlValue;

/// A value the user typed, under the column's declared type.
///
/// One type rather than two fields, so the drawn value and the literal cannot
/// drift: [`parse`] is the only constructor and produces both at once from
/// one reading of the text (design §3).
#[derive(Debug, Clone, PartialEq)]
pub struct Edited {
    value: CqlValue,
    literal: String,
}

impl Edited {
    /// What the grid and the inspector draw, and what a clean cell is
    /// compared against.
    #[must_use]
    pub const fn value(&self) -> &CqlValue {
        &self.value
    }

    /// The CQL literal a `SET` clause spells this with.
    #[must_use]
    pub fn literal(&self) -> &str {
        &self.literal
    }
}

/// Whether a column declared `cql_type` can be typed into at all — §2's type
/// gate, asked before a field is opened rather than after one is filled.
///
/// A column this refuses is refused with a message naming the type, so it is
/// read by the refusal as well as by the gate.
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

/// Whether the empty string is a value of a column declared `cql_type`, as
/// opposed to the absence of one. Only the three free-text types: an empty
/// `date` is the cluster's to reject, and no other type has one.
#[must_use]
pub fn takes_empty(cql_type: &str) -> bool {
    matches!(
        literal::normalised(cql_type).as_str(),
        "text" | "varchar" | "ascii"
    )
}

/// `text` read as a value of a column declared `cql_type`, or `None` where it
/// is not one.
///
/// `None` means two different things to two different callers and neither has
/// to tell them apart: [`editable`] has already answered "can this column be
/// typed into", so a `None` here is always "that is not a `<type>`".
#[must_use]
pub fn parse(cql_type: &str, text: &str) -> Option<Edited> {
    let (value, literal) = match literal::normalised(cql_type).as_str() {
        // The string types and the four whose value has a string form CQL
        // parses back to the same value. Not validated beyond being text: a
        // malformed `date` is the cluster's to reject, and a client-side
        // date parser is a second opinion about a format this program does
        // not own.
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
        // Spelled verbatim rather than re-rendered from the parse: `1.50`
        // and `1e3` are literals CQL reads, and a round trip through `f64`
        // would write back `1.5` and `1000` — and would lose a `varint` or
        // `decimal` too wide for one.
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

/// 8-4-4-4-12 lower-case hex, the one form the driver renders and CQL reads
/// bare.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();

    groups.len() == 5
        && groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|group| group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `0x` and an even number of hex digits, the form [`crate::cell`] draws a
/// blob in and CQL accepts back. `0x` alone is the empty blob.
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

/// The cells of one page that have been typed into and not yet written.
///
/// Keyed by `(page row index, column index)` rather than by drawn position:
/// `GridPage::order` changes under a sort, a filter and an appended page, and
/// an edit must not migrate to a different row when the user sorts
/// (design §4).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Edits(BTreeMap<(usize, usize), Edited>);

impl Edits {
    /// Records `edited` at `(row, column)`, or clears what is there when
    /// `edited` is `None` — which is how an edit that restores the value it
    /// started from leaves no mark (design §1).
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

    /// Cells typed into, which is what the footer and the quit warning count.
    #[must_use]
    pub fn cells(&self) -> usize {
        self.0.len()
    }

    /// Rows holding at least one of them.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.by_row().count()
    }

    /// The edits grouped by page row, in page order, each row's columns in
    /// column order — which is the order the statement reads in (design §5).
    ///
    /// `BTreeMap`'s own key order does both: `(row, column)` sorts by row and
    /// then by column, so the grouping is a fold over a sorted range rather
    /// than a second index.
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

    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Every edit, for writing into the page once the cluster has taken them.
    pub fn drain(&mut self) -> impl Iterator<Item = ((usize, usize), Edited)> {
        std::mem::take(&mut self.0).into_iter()
    }
}

/// The open field: the column being typed into, the text so far, and whether
/// it parses.
///
/// The column's declared type is captured rather than looked up per
/// keystroke: the overlay claims every key, so the schema underneath cannot
/// change while the field is up — the argument [`crate::contents::Filter`]
/// makes for capturing the column's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    column: usize,
    name: String,
    cql_type: String,
    line: Line,
    /// Whether the text has changed since the field opened, so an empty field
    /// that was typed to empty can be told from one never touched
    /// (insert design §3).
    touched: bool,
}

impl Field {
    /// Opens over `column`, seeded with the text the cell draws — so `⏎`
    /// straight away is a no-op rather than a blanking (design §1).
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

    /// What `⏎` would accept, `None` while the text is not a value of this
    /// column's type.
    #[must_use]
    pub fn parsed(&self) -> Option<Edited> {
        parse(&self.cql_type, self.line.text())
    }

    /// Why `⏎` is greyed, `None` while it is not.
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
        // A client-side date parser is a second opinion about a format this
        // program does not own; the cluster rejects what it will not take.
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
        // The whole difference from `literal::of`: the text is the user's, so
        // a round trip through `f64` would write back `1.5` for `1.50`.
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
