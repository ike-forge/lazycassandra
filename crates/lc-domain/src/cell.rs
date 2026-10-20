//! One [`CqlValue`] as the text a grid draws. In the domain, so every renderer
//! agrees and it tests without a cluster.

use crate::model::CqlValue;
use unicode_normalization::UnicodeNormalization;
use unicode_width::UnicodeWidthChar;

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// Public for the adapter: a value nested in a tuple or UDT renders as it would
/// alone.
#[must_use]
pub fn text(value: &CqlValue) -> String {
    match value {
        // Told apart from the text "null" by the faint token, not the text.
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

/// [`text`] made safe to draw. A raw ESC, `\n` or `\t` reaches the terminal as
/// a command or a line break and shifts every cell after it; yank and export
/// keep [`text`].
///
/// Nothing zero-width is drawn either. The `ConPTY` that Alacritty ships on
/// Windows (`OpenConsole` 1.24) repaints a line by writing it whole and letting
/// the terminal wrap, and it measures any combining mark, ZWSP or ZWJ sequence
/// differently from the terminal, so the wrap lands off and every line below
/// shifts. NFC first, so a decomposed accent survives as one character.
#[must_use]
pub fn display(value: &CqlValue) -> String {
    text(value)
        .nfc()
        .map(visible)
        .filter(|character| character.width() != Some(0))
        .collect()
}

/// C0 controls take their Unicode control picture, which is one column wide.
/// The rest (C1, bidi overrides and isolates) reorder or move the cursor with
/// nothing to show for it.
fn visible(character: char) -> char {
    match character {
        '\0'..='\u{1f}' => char::from_u32(0x2400 + u32::from(character)).unwrap_or('\u{fffd}'),
        '\u{7f}' => '\u{2421}',
        '\u{80}'..='\u{9f}'
        | '\u{61c}'
        | '\u{200e}'
        | '\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}' => '\u{fffd}',
        other => other,
    }
}

/// The form `cqlsh` prints and CQL accepts back.
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
    fn display_replaces_what_would_move_the_terminal() {
        let hostile = CqlValue::Text("a\x1b[2Jb\nc\td\u{7f}e\u{202e}f\u{85}".to_owned());

        assert_eq!(
            display(&hostile),
            "a\u{241b}[2Jb\u{240a}c\u{2409}d\u{2421}e\u{fffd}f\u{fffd}"
        );
        assert_eq!(
            text(&hostile).chars().count(),
            display(&hostile).chars().count()
        );
    }

    #[test]
    fn display_splits_sequences_a_terminal_may_draw_part_by_part() {
        let sequences =
            CqlValue::Text("👩\u{200d}👩\u{200d}👧 🏴\u{e0067}\u{e0062}\u{e007f}".to_owned());

        assert_eq!(display(&sequences), "👩👩👧 🏴");
    }

    #[test]
    fn display_composes_a_decomposed_accent() {
        let decomposed = CqlValue::Text("e\u{301}".to_owned());

        assert_eq!(display(&decomposed), "\u{e9}");
    }

    #[test]
    fn display_drops_marks_with_no_composed_form() {
        let stacked = CqlValue::Text("x\u{301}\u{302} \u{e01}\u{e31}".to_owned());

        assert_eq!(display(&stacked), "x \u{e01}");
    }

    #[test]
    fn display_drops_zero_width_spaces() {
        let spaced = CqlValue::Text("a\u{200b}b\u{feff}c".to_owned());

        assert_eq!(display(&spaced), "abc");
    }

    #[test]
    fn display_leaves_ordinary_text_alone() {
        let text_value = CqlValue::Text("全角 \u{e9} 👩 \u{5d1}".to_owned());

        assert_eq!(display(&text_value), text(&text_value));
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
