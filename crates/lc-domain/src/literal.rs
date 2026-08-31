//! Spelling a value the cluster returned back into the CQL literal that
//! addresses it (docs/superpowers/specs/2026-08-31-drop-design.md §3.1).
//!
//! The one direction the program has never had to go. Reading needs only a
//! rendering a human can see ([`crate::cell`]); a `WHERE` clause needs a
//! literal the *cluster* will read back as the same value, and being wrong
//! about that deletes the wrong row.
//!
//! So this is a whitelist, keyed on the column's declared CQL type as well as
//! on the value's variant, and it answers `None` for everything it is not
//! sure of. A refusal costs a greyed key; a wrong guess costs data. That is
//! the same conservative direction `Statement::typed`'s sniffer already takes
//! for the other reason (query execution design §4).

use crate::model::CqlValue;

/// The literal for `value` under a column declared `cql_type`, or `None`
/// where this program cannot spell one it is sure of.
///
/// The type matters because [`CqlValue`] is lossy on purpose: `varint`,
/// `decimal`, `date`, `time`, `timestamp` and `inet` all arrive as
/// [`CqlValue::Text`] carrying the driver's own rendering (`lc-outbound`'s
/// `value.rs`), and quoting is right for four of those and wrong for two.
/// The variant alone cannot tell them apart.
#[must_use]
pub fn of(cql_type: &str, value: &CqlValue) -> Option<String> {
    match (normalised(cql_type).as_str(), value) {
        // The three text types, and — after them — the four whose value has a
        // string form CQL parses back to the same value, each arriving as the
        // driver's rendering of exactly that form. One arm, because the
        // literal is the same shape; the list is the knowledge.
        (
            "text" | "varchar" | "ascii" | "inet" | "date" | "time" | "timestamp",
            CqlValue::Text(text),
        ) => Some(quoted(text)),
        // A `timestamp` a fake hands over as the model's own variant. The
        // real adapter renders one as text above; both spell the same
        // instant, and a count of milliseconds is a timestamp literal.
        ("timestamp", CqlValue::Timestamp(millis)) => Some(millis.to_string()),
        ("uuid" | "timeuuid", CqlValue::Uuid(uuid)) => Some(uuid.clone()),
        ("tinyint" | "smallint" | "int" | "bigint", CqlValue::Int(number)) => {
            Some(number.to_string())
        }
        ("boolean", CqlValue::Boolean(flag)) => Some(flag.to_string()),
        ("blob", CqlValue::Blob(bytes)) => Some(hex(bytes)),
        // Everything else is refused, and the notable refusals are
        // deliberate rather than missing:
        //
        // `float`/`double`, because a key column compared for equality
        // against a literal this program re-spelled is a row it might not
        // find — and might find instead of its neighbour.
        //
        // `varint`/`decimal`, because they arrive as text and would have to
        // be written bare, which means trusting a rendering this program did
        // not produce to be a bare number.
        //
        // `null`, because no primary key column may hold one; a row that
        // somehow shows one is a row this program will not write a `WHERE`
        // for.
        //
        // The collections and `counter`, because none of them can be part of
        // a primary key at all.
        _ => None,
    }
}

/// The type as the whitelist spells it: lower case, and with a `frozen<…>`
/// wrapper taken off. `system_schema` reports what was declared, and a
/// declaration's case is the user's.
fn normalised(cql_type: &str) -> String {
    let trimmed = cql_type.trim().to_ascii_lowercase();

    trimmed
        .strip_prefix("frozen<")
        .and_then(|inner| inner.strip_suffix('>'))
        .map_or(trimmed.clone(), |inner| inner.trim().to_owned())
}

/// A CQL string literal. The single quote is the only character with a
/// meaning inside one, and it is escaped by doubling — the rule
/// `Statement::quoted` follows for identifiers, with the other quote.
fn quoted(text: &str) -> String {
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
        // The schema says one thing and the page holds another: whichever is
        // wrong, guessing is what this module exists not to do.
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
