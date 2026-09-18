//! The buffer, split into the statements a run acts on.
//!
//! A third reader of the scan [`lex`](super::lex) already produced, beside the
//! highlighter and [`resolve`](super::resolve): a `;` outside a batch ends a
//! statement, and `BEGIN … APPLY BATCH` is one statement rather than the
//! several scopes `resolve` splits it into. The protocol carries one statement
//! per request, and half a batch is not one
//! (cql statement selection design §3).

use super::lex::{Kind, pieces};

/// One runnable statement, as the characters it spans.
///
/// `end` is one past the last character, and both count `char`s — the unit
/// `Editor` counts its cursor in, so an offset means the same thing on both
/// sides of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// The statement itself, cut out of the buffer it was found in.
    #[must_use]
    pub fn of(self, text: &str) -> String {
        text.chars().take(self.end).skip(self.start).collect()
    }
}

/// Every statement in `text`, in the order they are written.
///
/// Whitespace and comments between statements fall outside every span: a span
/// is the statement, so the band drawn over one and the text sent for it are
/// the same run of characters. A trailing `;` belongs to the statement it
/// terminates.
#[must_use]
pub fn statements(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut at = 0;
    let mut start = None;
    let mut end = 0;
    let mut batched = false;

    for piece in pieces(text) {
        let next = at + piece.text.chars().count();

        match piece.kind {
            Kind::Whitespace | Kind::Comment => {}
            Kind::Punctuation if piece.text == ";" && !batched => {
                if let Some(start) = start.take() {
                    spans.push(Span { start, end: next });
                }
            }
            _ => {
                if piece.kind == Kind::Keyword {
                    match piece.text.to_ascii_uppercase().as_str() {
                        "BEGIN" => batched = true,
                        "APPLY" => batched = false,
                        _ => {}
                    }
                }

                start.get_or_insert(at);
                end = next;
            }
        }

        at = next;
    }

    if let Some(start) = start {
        spans.push(Span { start, end });
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str) -> Vec<String> {
        statements(text)
            .into_iter()
            .map(|span| span.of(text))
            .collect()
    }

    #[test]
    fn an_empty_buffer_holds_no_statement() {
        assert!(statements("").is_empty());
        assert!(statements("   \n\t ").is_empty());
    }

    #[test]
    fn a_buffer_with_no_separator_is_one_statement() {
        assert_eq!(
            split("SELECT * FROM shop.orders"),
            ["SELECT * FROM shop.orders"]
        );
    }

    #[test]
    fn a_semicolon_ends_a_statement_and_stays_with_it() {
        assert_eq!(
            split("SELECT * FROM a; SELECT * FROM b"),
            ["SELECT * FROM a;", "SELECT * FROM b"]
        );
    }

    #[test]
    fn the_whitespace_around_a_statement_falls_outside_it() {
        let text = "\n  SELECT 1;\n\n  SELECT 2\n";

        assert_eq!(split(text), ["SELECT 1;", "SELECT 2"]);
        assert_eq!(statements(text)[0].start, 3);
    }

    #[test]
    fn a_comment_between_statements_belongs_to_neither() {
        assert_eq!(
            split("SELECT 1;\n-- why\nSELECT 2;\n/* done */"),
            ["SELECT 1;", "SELECT 2;"]
        );
    }

    /// The comment opens the statement's span where it precedes a keyword on
    /// the same run: it is not between statements, it is inside one.
    #[test]
    fn a_comment_inside_a_statement_is_part_of_it() {
        assert_eq!(
            split("SELECT * -- all\nFROM a"),
            ["SELECT * -- all\nFROM a"]
        );
    }

    #[test]
    fn a_batch_is_one_statement_however_many_semicolons_it_holds() {
        let batch = "BEGIN BATCH \
                     INSERT INTO a (id) VALUES (1); \
                     INSERT INTO b (id) VALUES (2); \
                     APPLY BATCH";

        assert_eq!(split(batch), [batch]);
    }

    #[test]
    fn a_statement_after_a_batch_is_its_own() {
        let text = "BEGIN BATCH INSERT INTO a (id) VALUES (1); APPLY BATCH; SELECT * FROM a";

        assert_eq!(
            split(text),
            [
                "BEGIN BATCH INSERT INTO a (id) VALUES (1); APPLY BATCH;",
                "SELECT * FROM a"
            ]
        );
    }

    #[test]
    fn an_unterminated_batch_swallows_the_rest_of_the_buffer() {
        let text = "BEGIN BATCH INSERT INTO a (id) VALUES (1); SELECT * FROM a";

        assert_eq!(split(text), [text]);
    }

    #[test]
    fn a_semicolon_inside_a_literal_separates_nothing() {
        assert_eq!(
            split("INSERT INTO a (v) VALUES ('one; two')"),
            ["INSERT INTO a (v) VALUES ('one; two')"]
        );
    }

    #[test]
    fn a_stray_semicolon_yields_no_empty_statement() {
        assert!(statements(";").is_empty());
        assert_eq!(split(";;SELECT 1;;"), ["SELECT 1;"]);
    }

    #[test]
    fn a_multibyte_character_counts_as_one() {
        let text = "SELECT 'café';SELECT 1";

        assert_eq!(split(text), ["SELECT 'café';", "SELECT 1"]);
        assert_eq!(statements(text)[1].start, 14);
    }
}
