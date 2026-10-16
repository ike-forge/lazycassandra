//! Spelling a value the cluster returned back into a CQL literal that addresses
//! it. A whitelist: `None` for anything uncertain, because a refusal costs a
//! greyed key and a wrong guess deletes the wrong row.

use crate::model::CqlValue;

/// The type matters: `varint`, `decimal`, `date`, `time`, `timestamp` and
/// `inet` all arrive as [`CqlValue::Text`], and quoting is right for four of
/// them only.
#[must_use]
pub fn of(cql_type: &str, value: &CqlValue) -> Option<String> {
    match (normalised(cql_type).as_str(), value) {
        // Types whose string form CQL parses back to the same value.
        (
            "text" | "varchar" | "ascii" | "inet" | "date" | "time" | "timestamp",
            CqlValue::Text(text),
        ) => Some(quoted(text)),
        // A fake's timestamp; the real adapter renders one as text.
        ("timestamp", CqlValue::Timestamp(millis)) => Some(millis.to_string()),
        ("uuid" | "timeuuid", CqlValue::Uuid(uuid)) => Some(uuid.clone()),
        ("tinyint" | "smallint" | "int" | "bigint", CqlValue::Int(number)) => {
            Some(number.to_string())
        }
        ("boolean", CqlValue::Boolean(flag)) => Some(flag.to_string()),
        ("blob", CqlValue::Blob(bytes)) => Some(hex(bytes)),
        // Refused on purpose: `float`/`double` may not compare equal after
        // re-spelling; `varint`/`decimal` would mean trusting a rendering as a
        // bare number; no key holds `null`; collections and `counter` cannot be
        // keys.
        _ => None,
    }
}

/// Lower case, `frozen<…>` removed: `system_schema` reports the declared case.
pub(crate) fn normalised(cql_type: &str) -> String {
    let trimmed = cql_type.trim().to_ascii_lowercase();

    trimmed
        .strip_prefix("frozen<")
        .and_then(|inner| inner.strip_suffix('>'))
        .map_or(trimmed.clone(), |inner| inner.trim().to_owned())
}

/// A single quote is escaped by doubling.
pub(crate) fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold("0x".to_owned(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn text_is_quoted_and_an_embedded_quote_doubled() {
        assert_eq!(
            of("text", &CqlValue::Text("ada".to_owned())).as_deref(),
            Some("'ada'")
        );
        assert_eq!(
            of("text", &CqlValue::Text("o'hara".to_owned())).as_deref(),
            Some("'o''hara'"),
            "a name holding a quote must not end the literal"
        );
    }

    #[test]
    fn a_uuid_is_bare_and_a_number_is_bare() {
        assert_eq!(
            of("uuid", &CqlValue::Uuid("8f14e45f".to_owned())).as_deref(),
            Some("8f14e45f")
        );
        assert_eq!(of("int", &CqlValue::Int(-7)).as_deref(), Some("-7"));
        assert_eq!(
            of("boolean", &CqlValue::Boolean(true)).as_deref(),
            Some("true")
        );
    }

    #[test]
    fn a_blob_is_hex_with_a_leading_marker() {
        assert_eq!(
            of("blob", &CqlValue::Blob(vec![0x00, 0x0f, 0xff])).as_deref(),
            Some("0x000fff")
        );
    }

    #[test]
    fn the_types_that_arrive_as_text_are_quoted_or_refused_by_type_not_by_variant() {
        let rendered = CqlValue::Text("1970-01-01".to_owned());

        assert_eq!(of("date", &rendered).as_deref(), Some("'1970-01-01'"));
        assert_eq!(
            of("varint", &CqlValue::Text("1099511627776".to_owned())),
            None,
            "a bare number this program did not produce is not one it will write"
        );
        assert_eq!(of("decimal", &CqlValue::Text("3.01".to_owned())), None);
    }

    #[test]
    fn a_timestamp_spells_either_shape_it_can_arrive_in() {
        assert_eq!(
            of(
                "timestamp",
                &CqlValue::Text("2023-11-14T22:13:20.000Z".to_owned())
            )
            .as_deref(),
            Some("'2023-11-14T22:13:20.000Z'")
        );
        assert_eq!(
            of("timestamp", &CqlValue::Timestamp(1_699_999_999_000)).as_deref(),
            Some("1699999999000")
        );
    }

    #[test]
    fn a_declaration_keeps_its_meaning_through_case_and_a_frozen_wrapper() {
        assert_eq!(
            of("  TEXT ", &CqlValue::Text("ada".to_owned())).as_deref(),
            Some("'ada'")
        );
        assert_eq!(
            of("frozen<uuid>", &CqlValue::Uuid("8f14e45f".to_owned())).as_deref(),
            Some("8f14e45f")
        );
    }

    #[test]
    fn a_type_and_a_variant_that_disagree_spell_nothing() {
        // Schema and page disagree: guessing is what this module refuses.
        assert_eq!(of("int", &CqlValue::Text("7".to_owned())), None);
        assert_eq!(of("text", &CqlValue::Int(7)), None);
    }

    #[test]
    fn the_refusals_are_refusals() {
        assert_eq!(of("text", &CqlValue::Null), None);
        assert_eq!(of("double", &CqlValue::Float(0.5)), None);
        assert_eq!(of("float", &CqlValue::Float(0.5)), None);
        assert_eq!(
            of("list<int>", &CqlValue::List(vec![CqlValue::Int(1)])),
            None
        );
        assert_eq!(of("set<int>", &CqlValue::Set(vec![CqlValue::Int(1)])), None);
        assert_eq!(
            of("map<text,int>", &CqlValue::Map(BTreeMap::new())),
            None,
            "no collection can be part of a primary key"
        );
    }
}
