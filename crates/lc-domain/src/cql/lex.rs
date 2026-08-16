//! CQL, split into the runs a reader distinguishes.
//!
//! A scanner and not a parser. It answers "what kind of run is this" for every
//! byte of the buffer, which is all the highlighter needs and all the
//! formatter is allowed to assume — a formatter that reordered a clause it had
//! mis-parsed would rewrite the user's statement into a different one, and
//! `Statement::typed` is a sniffer for the same reason
//! (design §2).

use super::keywords;

/// What one run of source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Keyword,
    /// A name: a keyspace, a table, a column, a function, a bind marker.
    Identifier,
    /// A string, a number, a uuid, a blob, or one of `true`/`false`/`null`.
    Literal,
    Comment,
    /// Everything left: operators, brackets, commas, the statement separator.
    Punctuation,
    Whitespace,
}

/// One run, kept whole. `text` is the source verbatim — the formatter rewrites
/// only the case of a [`Kind::Keyword`], and nothing else is retyped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Piece {
    pub kind: Kind,
    pub text: String,
}

/// `text` as its runs, in order and losing nothing: concatenating every
/// `Piece::text` reproduces the input.
pub(crate) fn pieces(text: &str) -> Vec<Piece> {
    let source: Vec<char> = text.chars().collect();
    let mut pieces = Vec::new();
    let mut at = 0;

    while at < source.len() {
        let (kind, len) = run(&source, at);
        pieces.push(Piece {
            kind,
            text: source[at..at + len].iter().collect(),
        });
        at += len;
    }

    pieces
}

/// The run starting at `at`, as its kind and its length in characters. Never
/// zero: every branch consumes at least the character it looked at.
fn run(source: &[char], at: usize) -> (Kind, usize) {
    let first = source[at];

    if first.is_whitespace() {
        return (
            Kind::Whitespace,
            while_matching(source, at, char::is_whitespace),
        );
    }

    if let Some(len) = comment(source, at) {
        return (Kind::Comment, len);
    }

    if first == '\'' {
        return (Kind::Literal, quoted(source, at, '\''));
    }

    if first == '"' {
        return (Kind::Identifier, quoted(source, at, '"'));
    }

    if first.is_ascii_digit() {
        return (Kind::Literal, number(source, at));
    }

    if first.is_alphabetic() || first == '_' {
        let len = while_matching(source, at, |c| c.is_alphanumeric() || c == '_');
        let word: String = source[at..at + len].iter().collect();

        if keywords::is_value_word(&word) {
            return (Kind::Literal, len);
        }

        let kind = if keywords::is_keyword(&word) {
            Kind::Keyword
        } else {
            Kind::Identifier
        };

        return (kind, len);
    }

    // `:name` and `?` are bind markers, which name a value the way a column
    // does; `$$…$$` is CQL's other string form.
    if first == ':'
        && at + 1 < source.len()
        && (source[at + 1].is_alphabetic() || source[at + 1] == '_')
    {
        let len = while_matching(source, at + 1, |c| c.is_alphanumeric() || c == '_');
        return (Kind::Identifier, len + 1);
    }

    if first == '$' && source.get(at + 1) == Some(&'$') {
        return (Kind::Literal, dollar_quoted(source, at));
    }

    (Kind::Punctuation, 1)
}

/// A `--` or `//` comment to the end of its line, or a `/* */` block to its
/// terminator or to the end of the buffer.
fn comment(source: &[char], at: usize) -> Option<usize> {
    let pair = (source[at], *source.get(at + 1)?);

    match pair {
        ('-', '-') | ('/', '/') => Some(while_matching(source, at, |c| c != '\n')),
        ('/', '*') => {
            let mut end = at + 2;
            while end < source.len() {
                if source[end] == '*' && source.get(end + 1) == Some(&'/') {
                    return Some(end + 2 - at);
                }
                end += 1;
            }
            Some(source.len() - at)
        }
        _ => None,
    }
}

/// A run delimited by `quote`, in which the delimiter doubled is the delimiter
/// itself — CQL's only escape, in both its string and its identifier form. An
/// unterminated run reaches the end of the buffer rather than resynchronising:
/// a buffer being typed into is unterminated for as long as it takes to type
/// the closing quote.
fn quoted(source: &[char], at: usize, quote: char) -> usize {
    let mut end = at + 1;

    while end < source.len() {
        if source[end] == quote {
            if source.get(end + 1) == Some(&quote) {
                end += 2;
                continue;
            }
            return end + 1 - at;
        }
        end += 1;
    }

    source.len() - at
}

fn dollar_quoted(source: &[char], at: usize) -> usize {
    let mut end = at + 2;

    while end < source.len() {
        if source[end] == '$' && source.get(end + 1) == Some(&'$') {
            return end + 2 - at;
        }
        end += 1;
    }

    source.len() - at
}

/// A number, or a uuid — which starts with a digit as often as not, and whose
/// hyphens would otherwise scan as three subtractions.
fn number(source: &[char], at: usize) -> usize {
    if let Some(len) = uuid(source, at) {
        return len;
    }

    if source[at] == '0' && matches!(source.get(at + 1), Some('x' | 'X')) {
        return while_matching(source, at + 2, char::is_alphanumeric) + 2;
    }

    let mut end = at;
    while end < source.len() {
        let character = source[end];
        let exponent_sign =
            matches!(character, '+' | '-') && end > at && matches!(source[end - 1], 'e' | 'E');

        let exponent = matches!(character, 'e' | 'E')
            && source
                .get(end + 1)
                .is_some_and(|next| next.is_ascii_digit() || matches!(next, '+' | '-'));

        if !(character.is_ascii_digit() || character == '.' || exponent_sign || exponent) {
            break;
        }

        end += 1;
    }

    end - at
}

/// 8-4-4-4-12 hexadecimal, and nothing else: a shape this exact cannot be
/// mistaken for an expression.
fn uuid(source: &[char], at: usize) -> Option<usize> {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut end = at;

    for (index, group) in GROUPS.iter().enumerate() {
        if index > 0 {
            if source.get(end) != Some(&'-') {
                return None;
            }
            end += 1;
        }

        for _ in 0..*group {
            if !source.get(end)?.is_ascii_hexdigit() {
                return None;
            }
            end += 1;
        }
    }

    // A longer word starting with a uuid is a word, not a uuid.
    if source
        .get(end)
        .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
    {
        return None;
    }

    Some(end - at)
}

fn while_matching(source: &[char], at: usize, matches: impl Fn(char) -> bool) -> usize {
    let mut end = at;
    while end < source.len() && matches(source[end]) {
        end += 1;
    }

    end - at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Kind, String)> {
        pieces(text)
            .into_iter()
            .filter(|piece| piece.kind != Kind::Whitespace)
            .map(|piece| (piece.kind, piece.text))
            .collect()
    }

    fn kinds_only(text: &str) -> Vec<Kind> {
        kinds(text).into_iter().map(|(kind, _)| kind).collect()
    }

    #[test]
    fn every_character_lands_in_exactly_one_piece() {
        let text = "SELECT * FROM shop.orders WHERE id = 7; -- done\n";

        let rebuilt: String = pieces(text)
            .iter()
            .map(|piece| piece.text.as_str())
            .collect();

        assert_eq!(rebuilt, text);
    }

    #[test]
    fn a_select_splits_into_grammar_names_and_punctuation() {
        assert_eq!(
            kinds_only("SELECT * FROM shop.orders"),
            vec![
                Kind::Keyword,
                Kind::Punctuation,
                Kind::Keyword,
                Kind::Identifier,
                Kind::Punctuation,
                Kind::Identifier,
            ]
        );
    }

    #[test]
    fn a_keyword_is_grammar_in_any_case() {
        assert_eq!(kinds_only("select"), vec![Kind::Keyword]);
    }

    #[test]
    fn a_string_survives_a_doubled_quote_inside_it() {
        assert_eq!(
            kinds("'it''s'"),
            vec![(Kind::Literal, "'it''s'".to_owned())]
        );
    }

    #[test]
    fn a_quoted_name_is_a_name_and_not_a_string() {
        assert_eq!(
            kinds("\"MyKs\".\"Orders\""),
            vec![
                (Kind::Identifier, "\"MyKs\"".to_owned()),
                (Kind::Punctuation, ".".to_owned()),
                (Kind::Identifier, "\"Orders\"".to_owned()),
            ]
        );
    }

    #[test]
    fn an_unterminated_string_runs_to_the_end_rather_than_resynchronising() {
        // The state of every buffer between the opening quote and the closing
        // one, which is a state the user types through on the way to a valid
        // statement.
        assert_eq!(kinds("WHERE a = 'ab"), {
            vec![
                (Kind::Keyword, "WHERE".to_owned()),
                (Kind::Identifier, "a".to_owned()),
                (Kind::Punctuation, "=".to_owned()),
                (Kind::Literal, "'ab".to_owned()),
            ]
        });
    }

    #[test]
    fn a_line_comment_ends_at_its_line_and_a_block_comment_at_its_terminator() {
        assert_eq!(
            kinds("-- one\nSELECT /* two */ 1"),
            vec![
                (Kind::Comment, "-- one".to_owned()),
                (Kind::Keyword, "SELECT".to_owned()),
                (Kind::Comment, "/* two */".to_owned()),
                (Kind::Literal, "1".to_owned()),
            ]
        );
    }

    #[test]
    fn a_block_comment_spans_lines() {
        assert_eq!(
            kinds("/* one\ntwo */ SELECT"),
            vec![
                (Kind::Comment, "/* one\ntwo */".to_owned()),
                (Kind::Keyword, "SELECT".to_owned()),
            ]
        );
    }

    #[test]
    fn a_uuid_is_one_literal_and_not_three_subtractions() {
        assert_eq!(
            kinds("123e4567-e89b-12d3-a456-426614174000"),
            vec![(
                Kind::Literal,
                "123e4567-e89b-12d3-a456-426614174000".to_owned()
            )]
        );
    }

    #[test]
    fn a_subtraction_of_two_numbers_stays_three_pieces() {
        assert_eq!(
            kinds_only("1-2"),
            vec![Kind::Literal, Kind::Punctuation, Kind::Literal]
        );
    }

    #[test]
    fn a_number_takes_its_fraction_and_its_exponent() {
        assert_eq!(kinds("1.5e-3"), vec![(Kind::Literal, "1.5e-3".to_owned())]);
    }

    #[test]
    fn a_blob_is_one_literal() {
        assert_eq!(
            kinds("0xCAFEBABE"),
            vec![(Kind::Literal, "0xCAFEBABE".to_owned())]
        );
    }

    #[test]
    fn the_value_words_are_literals_rather_than_grammar() {
        assert_eq!(
            kinds_only("null true false"),
            vec![Kind::Literal, Kind::Literal, Kind::Literal]
        );
    }

    #[test]
    fn a_bind_marker_names_a_value_the_way_a_column_does() {
        assert_eq!(
            kinds(":since"),
            vec![(Kind::Identifier, ":since".to_owned())]
        );
    }

    #[test]
    fn a_multibyte_name_is_one_piece() {
        assert_eq!(kinds("café"), vec![(Kind::Identifier, "café".to_owned())]);
    }
}
