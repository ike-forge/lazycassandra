//! One [`CqlValue`] as the text a grid draws.
//!
//! Rendering lives in the domain rather than the adapter so that every
//! renderer agrees on what a blob or a collection looks like, and so the
//! result is testable without a cluster.

use crate::model::CqlValue;

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// The cell text for `value`.
///
/// Public because the adapter needs it too: a value nested inside a tuple or
/// a UDT has to render the way it would in a column of its own.
#[must_use]
pub fn text(value: &CqlValue) -> String {
    match value {
        // Distinguishable from a text column holding the word by the faint
        // token the projection puts on it, not by the text.
        CqlValue::Null => "null".to_owned(),
        CqlValue::Boolean(value) => value.to_string(),
        CqlValue::Int(value) => value.to_string(),
        CqlValue::Float(value) => value.to_string(),
        CqlValue::Text(value) | CqlValue::Uuid(value) => value.clone(),
        CqlValue::Blob(bytes) => hex(bytes),
        CqlValue::Timestamp(millis) => millis.to_string(),
        CqlValue::List(values) => sequence(values, '[', ']'),
        CqlValue::Set(values) => sequence(values, '{', '}'),
        CqlValue::Map(entries) => map(entries),
    }
}

/// `0x` and two lower-case digits per byte, the form `cqlsh` prints and CQL
/// accepts back as a literal.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("0x");

    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)]);
        out.push(HEX[usize::from(byte & 0x0f)]);
    }

    out
}

fn sequence(values: &[CqlValue], open: char, close: char) -> String {
    let rendered: Vec<String> = values.iter().map(text).collect();

    format!("{open}{}{close}", rendered.join(", "))
}

fn map(entries: &std::collections::BTreeMap<String, CqlValue>) -> String {
    let rendered: Vec<String> = entries
        .iter()
        .map(|(key, value)| format!("{key}: {}", text(value)))
        .collect();

    format!("{{{}}}", rendered.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn scalars_render_the_way_cqlsh_prints_them() {
        assert_eq!(text(&CqlValue::Null), "null");
        assert_eq!(text(&CqlValue::Boolean(true)), "true");
        assert_eq!(text(&CqlValue::Int(-42)), "-42");
        assert_eq!(text(&CqlValue::Float(1.5)), "1.5");
        assert_eq!(text(&CqlValue::Text("ada".to_owned())), "ada");
        assert_eq!(text(&CqlValue::Uuid("8f14e45f".to_owned())), "8f14e45f");
        assert_eq!(
            text(&CqlValue::Timestamp(1_700_000_000_000)),
            "1700000000000"
        );
    }

    #[test]
    fn a_blob_is_a_hex_literal_cql_would_accept_back() {
        assert_eq!(text(&CqlValue::Blob(vec![0x1f, 0x8b, 0x00])), "0x1f8b00");
        assert_eq!(text(&CqlValue::Blob(Vec::new())), "0x");
    }

    #[test]
    fn a_list_and_a_set_are_told_apart_by_their_brackets() {
        let values = vec![CqlValue::Int(1), CqlValue::Int(2)];

        assert_eq!(text(&CqlValue::List(values.clone())), "[1, 2]");
        assert_eq!(text(&CqlValue::Set(values)), "{1, 2}");
    }

    #[test]
    fn a_map_renders_its_entries_in_key_order() {
        let entries = BTreeMap::from([
            ("b".to_owned(), CqlValue::Int(2)),
            ("a".to_owned(), CqlValue::Int(1)),
        ]);

        assert_eq!(text(&CqlValue::Map(entries)), "{a: 1, b: 2}");
    }

    #[test]
    fn a_collection_renders_its_elements_rather_than_their_shape() {
        let nested = CqlValue::List(vec![
            CqlValue::Set(vec![CqlValue::Text("x".to_owned())]),
            CqlValue::Null,
        ]);

        assert_eq!(text(&nested), "[{x}, null]");
    }

    #[test]
    fn an_empty_collection_is_still_recognisable_as_one() {
        assert_eq!(text(&CqlValue::List(Vec::new())), "[]");
        assert_eq!(text(&CqlValue::Set(Vec::new())), "{}");
        assert_eq!(text(&CqlValue::Map(BTreeMap::new())), "{}");
    }
}
