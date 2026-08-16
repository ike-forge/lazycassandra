//! CQL as the editor needs it: coloured, formatted, and completed.
//!
//! All three read the same scan of the buffer ([`lex`]), so the word the
//! highlighter paints as grammar is the word the formatter uppercases and the
//! word completion offers — three answers that could otherwise drift apart
//! (docs/superpowers/specs/2026-08-16-query-editor-design.md §2).

mod complete;
mod format;
mod keywords;
mod lex;

pub use complete::{Candidate, CandidateKind, Completion, suggest};
pub use format::format;

use crate::style::{Segment, Token};
use lex::Kind;

/// `text` as one styled line per buffer line, in order.
///
/// Exactly as many lines out as `text` has: a caller drawing them beside a
/// cursor counted in buffer lines cannot be handed a different count
/// (design §3).
#[must_use]
pub fn highlight(text: &str) -> Vec<Vec<Segment>> {
    let mut lines = Vec::new();
    let mut line: Vec<Segment> = Vec::new();

    for piece in lex::pieces(text) {
        let token = token(piece.kind);

        for (index, chunk) in piece.text.split('\n').enumerate() {
            if index > 0 {
                lines.push(std::mem::take(&mut line));
            }
            if !chunk.is_empty() {
                push(&mut line, chunk, token);
            }
        }
    }

    lines.push(line);
    lines
}

/// Adjacent runs of one meaning are drawn as one span. The renderer splits
/// what it draws again — against a search term, and against the horizontal
/// offset — and every extra seam is another place that split has to land.
fn push(line: &mut Vec<Segment>, text: &str, token: Option<Token>) {
    match line.last_mut() {
        Some(last) if last.token == token => last.text.push_str(text),
        _ => line.push(Segment {
            text: text.to_owned(),
            token,
        }),
    }
}

/// Punctuation and whitespace carry no meaning of their own: `,` in a select
/// list is the panel's text colour, the way a panel's name is the panel's name
/// (style.rs's [`Segment`] doc).
const fn token(kind: Kind) -> Option<Token> {
    match kind {
        Kind::Keyword => Some(Token::Keyword),
        Kind::Identifier => Some(Token::Reference),
        Kind::Literal => Some(Token::Literal),
        Kind::Comment => Some(Token::Comment),
        Kind::Punctuation | Kind::Whitespace => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::text;

    fn tokens(line: &[Segment]) -> Vec<(Option<Token>, &str)> {
        line.iter()
            .map(|segment| (segment.token, segment.text.as_str()))
            .collect()
    }

    #[test]
    fn an_empty_buffer_is_one_empty_line() {
        assert_eq!(highlight(""), vec![Vec::new()]);
    }

    #[test]
    fn the_lines_out_are_the_lines_in() {
        let source = "SELECT *\nFROM shop.orders\n\nWHERE id = 7";

        let lines = highlight(source);

        assert_eq!(lines.len(), source.lines().count());
        for (styled, plain) in lines.iter().zip(source.split('\n')) {
            assert_eq!(text(styled), plain, "a line is repainted, never rewritten");
        }
    }

    #[test]
    fn grammar_names_and_values_are_told_apart() {
        let lines = highlight("SELECT id FROM orders WHERE id = 7");

        assert_eq!(
            tokens(&lines[0]),
            vec![
                (Some(Token::Keyword), "SELECT"),
                (None, " "),
                (Some(Token::Reference), "id"),
                (None, " "),
                (Some(Token::Keyword), "FROM"),
                (None, " "),
                (Some(Token::Reference), "orders"),
                (None, " "),
                (Some(Token::Keyword), "WHERE"),
                (None, " "),
                (Some(Token::Reference), "id"),
                (None, " = "),
                (Some(Token::Literal), "7"),
            ],
            "punctuation and the spaces around it are one unstyled run"
        );
    }

    #[test]
    fn a_block_comment_keeps_its_colour_across_the_lines_it_spans() {
        let lines = highlight("/* one\ntwo */ SELECT");

        assert_eq!(tokens(&lines[0]), vec![(Some(Token::Comment), "/* one")]);
        assert_eq!(
            tokens(&lines[1]),
            vec![
                (Some(Token::Comment), "two */"),
                (None, " "),
                (Some(Token::Keyword), "SELECT"),
            ]
        );
    }

    #[test]
    fn a_trailing_newline_leaves_an_empty_last_line() {
        assert_eq!(highlight("SELECT\n").len(), 2);
    }
}
