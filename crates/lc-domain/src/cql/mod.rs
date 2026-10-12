//! CQL as the editor needs it: coloured, formatted, completed, and split into
//! the statements a run acts on.
//!
//! All four read the same scan of the buffer ([`lex`]), so the word the
//! highlighter paints as grammar is the word the formatter uppercases, the
//! word completion offers, and the word that opens a statement — answers that
//! could otherwise drift apart
//! (docs/superpowers/specs/2026-08-16-query-editor-design.md §2).

mod complete;
mod format;
mod keywords;
mod lex;
mod resolve;
mod statements;
mod templates;

pub use complete::{Candidate, CandidateKind, Completion, suggest};
pub use format::format;
pub use resolve::{Analysis, Context, Expect, Name, Role, Statement};
pub use statements::{Span, statements};

use crate::style::{Segment, Token};
use lex::Kind;
use resolve::resolve;

/// `cql` with everything CQL treats as case-insensitive lower-cased: keywords
/// and unquoted names. String literals, quoted names and comments keep their
/// case, so two statements are the same statement exactly when their folds
/// are equal.
#[must_use]
pub fn fold(cql: &str) -> String {
    lex::pieces(cql)
        .into_iter()
        .map(|piece| match piece.kind {
            Kind::Keyword => piece.text.to_lowercase(),
            Kind::Identifier if !piece.text.starts_with('"') => piece.text.to_lowercase(),
            _ => piece.text,
        })
        .collect()
}

/// The one table `cql` names, for a caller that has CQL text and no pieces
/// of its own — `view` must not reach into [`lex`], which is private to this
/// module.
///
/// `None` where the buffer resolves to anything other than exactly one
/// `SELECT` naming exactly one table: a `BEGIN BATCH` splits into one scope
/// per `;`, so its first scope alone can name one table while the buffer as
/// a whole names several, and a caller that only checked `tables_at(0)`
/// would be fooled by it. The verb matters for the same reason the count
/// does — an `INSERT INTO shop.orders` names a table it wrote and never
/// read, so its columns say nothing about the rows on screen
/// (cql semantics design §7).
#[must_use]
pub fn sole_table(cql: &str) -> Option<Name> {
    let scanned = lex::pieces(cql);
    let analysis = resolve(&scanned);

    if analysis.statements() != 1 || analysis.statement_at(0) != Statement::Select {
        return None;
    }

    match analysis.tables_at(0) {
        [name] => Some(name.clone()),
        _ => None,
    }
}

/// `text` as one styled line per buffer line, in order.
///
/// Exactly as many lines out as `text` has: a caller drawing them beside a
/// cursor counted in buffer lines cannot be handed a different count
/// (design §3).
#[must_use]
pub fn highlight(text: &str) -> Vec<Vec<Segment>> {
    let scanned = lex::pieces(text);
    let analysis = resolve(&scanned);
    let mut lines = Vec::new();
    let mut line: Vec<Segment> = Vec::new();

    for (index, piece) in scanned.iter().enumerate() {
        let token = token(piece.kind, analysis.role(index));

        for (at, chunk) in piece.text.split('\n').enumerate() {
            if at > 0 {
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

/// `text` as the trail logs it: one line, highlighted once.
///
/// Collapses whitespace between tokens to a single space and drops comments
/// — but never touches the inside of a token, so a string literal spanning
/// several source lines, or holding a run of spaces, survives byte for byte.
/// Working from the lexer's pieces rather than the raw text is what makes
/// that possible: a comment or a run of whitespace is a token in its own
/// right here, never a substring of one that carries meaning
/// (query log design §3, §6).
#[must_use]
pub fn issued(text: &str) -> Vec<Segment> {
    let scanned = lex::pieces(text);
    let analysis = resolve(&scanned);
    let mut line = Vec::new();
    let mut pending_space = false;

    for (index, piece) in scanned.iter().enumerate() {
        if matches!(piece.kind, Kind::Whitespace | Kind::Comment) {
            pending_space = pending_space || !line.is_empty();
            continue;
        }

        if pending_space {
            push(&mut line, " ", None);
            pending_space = false;
        }

        push(
            &mut line,
            &piece.text,
            token(piece.kind, analysis.role(index)),
        );
    }

    line
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
/// list is the panel's text colour, the way a panel's name is the panel's
/// name (style.rs's [`Segment`] doc).
///
/// A name's role outranks its kind: the tree, the DDL panel and the editor
/// spend one colour on a keyspace and another on a table
/// (cql semantics design §4).
const fn token(kind: Kind, role: Option<Role>) -> Option<Token> {
    match kind {
        Kind::Keyword => Some(Token::Keyword),
        Kind::Identifier => Some(match role {
            Some(Role::Keyspace) => Token::Keyspace,
            Some(Role::Table) => Token::Table,
            _ => Token::Reference,
        }),
        Kind::Literal => Some(Token::Literal),
        Kind::Comment => Some(Token::Comment),
        Kind::Punctuation | Kind::Whitespace => None,
    }
}

/// The opening word of every statement in `text`, upper-cased, in order.
///
/// The word a statement is *classified* by, which is why it is scanned rather
/// than split off the front: a leading comment is not a verb, and `;` inside a
/// string is not a separator — both of which a `split_whitespace` on the raw
/// buffer gets wrong, and getting them wrong is a write reaching a connection
/// that forbade one (posture design §2).
///
/// One entry per statement, so a buffer holding several is classified by all
/// of them. A statement with no word in it at all — trailing whitespace past
/// the last `;` — contributes nothing.
#[must_use]
pub(crate) fn opening_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut open = false;

    for piece in lex::pieces(text) {
        match piece.kind {
            Kind::Comment | Kind::Whitespace => {}
            Kind::Punctuation if piece.text == ";" => open = false,
            _ if open => {}
            _ => {
                open = true;
                words.push(piece.text.to_ascii_uppercase());
            }
        }
    }

    words
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
                (Some(Token::Table), "orders"),
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

    #[test]
    fn issuing_a_statement_collapses_whitespace_between_tokens_but_not_inside_a_literal() {
        let issued = issued("INSERT INTO t (v)\n  VALUES ('a  b')");

        assert_eq!(text(&issued), "INSERT INTO t (v) VALUES ('a  b')");
    }

    #[test]
    fn issuing_a_statement_drops_a_comment_but_keeps_the_clause_it_stood_before() {
        let issued = issued("SELECT * FROM t -- get orders\nWHERE id = 1");

        assert_eq!(text(&issued), "SELECT * FROM t WHERE id = 1");
    }

    #[test]
    fn a_qualified_table_paints_its_keyspace_and_its_table_apart() {
        let lines = highlight("SELECT id FROM shop.orders");

        assert_eq!(
            tokens(&lines[0]),
            vec![
                (Some(Token::Keyword), "SELECT"),
                (None, " "),
                (Some(Token::Reference), "id"),
                (None, " "),
                (Some(Token::Keyword), "FROM"),
                (None, " "),
                (Some(Token::Keyspace), "shop"),
                (None, "."),
                (Some(Token::Table), "orders"),
            ]
        );
    }

    #[test]
    fn a_name_the_cluster_never_heard_of_is_painted_by_its_position() {
        let lines = highlight("SELECT * FROM shp.ordrs");

        assert!(
            tokens(&lines[0]).contains(&(Some(Token::Keyspace), "shp")),
            "colour says what the grammar means, not what the schema has"
        );
    }

    #[test]
    fn a_name_in_a_statement_the_resolver_does_not_follow_stays_a_reference() {
        let lines = highlight("GRANT SELECT ON shop.orders TO ike");

        assert!(tokens(&lines[0]).contains(&(Some(Token::Reference), "shop")));
    }

    fn sole(name: Option<Name>) -> Option<(Option<String>, String)> {
        name.map(|name| (name.keyspace, name.table))
    }

    /// A trailing `;` opens a scope on the chance something follows it
    /// (`Machine::end_statement`), but nothing here ever does — that
    /// placeholder must not count as a second statement and defeat the
    /// one-statement guard below.
    #[test]
    fn a_trailing_semicolon_still_names_the_one_table() {
        assert_eq!(
            sole(sole_table("select * from shop.orders;")),
            Some((Some("shop".to_owned()), "orders".to_owned()))
        );
    }

    #[test]
    fn whitespace_before_or_after_a_trailing_semicolon_still_names_the_one_table() {
        assert_eq!(
            sole(sole_table("select * from shop.orders ;")),
            Some((Some("shop".to_owned()), "orders".to_owned())),
            "whitespace before the semicolon"
        );
        assert_eq!(
            sole(sole_table("select * from shop.orders;   ")),
            Some((Some("shop".to_owned()), "orders".to_owned())),
            "whitespace after the semicolon"
        );
    }

    #[test]
    fn a_statement_that_is_not_a_select_names_no_sole_table() {
        // Each names exactly one table, and none of them read a row from it.
        assert_eq!(sole_table("insert into shop.orders (id) values (1)"), None);
        assert_eq!(sole_table("truncate shop.orders"), None);
        assert_eq!(sole_table("update shop.orders set total = 1"), None);
        assert_eq!(
            sole_table("create table shop.orders (id uuid primary key)"),
            None
        );
        assert_eq!(sole_table("use shop"), None);
    }

    #[test]
    fn two_statements_separated_by_a_semicolon_name_no_sole_table() {
        assert_eq!(sole_table("select * from a; select * from b"), None);
    }

    #[test]
    fn a_batch_names_no_sole_table() {
        assert_eq!(
            sole_table(
                "begin batch insert into shop.orders (id) values (1); \
                 insert into shop.customers (id) values (2); apply batch"
            ),
            None
        );
    }

    #[test]
    fn a_lone_semicolon_or_an_empty_buffer_names_no_sole_table_and_does_not_panic() {
        assert_eq!(sole_table(";"), None);
        assert_eq!(sole_table(""), None);
    }

    fn statements_in(cql: &str) -> usize {
        resolve(&lex::pieces(cql)).statements()
    }

    /// A stray word after a `;` is a second statement the resolver could not
    /// follow, not an empty tail: `Verb::Unread` alone could not tell the
    /// two apart, since a bare identifier reaches neither `keyword()` nor
    /// `record()` either.
    #[test]
    fn a_stray_word_after_a_semicolon_is_a_second_statement_not_an_empty_tail() {
        assert_eq!(sole_table("select * from a; notacommand"), None);
        assert_eq!(statements_in("select * from a; notacommand"), 2);
    }

    #[test]
    fn a_stray_close_paren_after_a_semicolon_is_a_second_statement_not_an_empty_tail() {
        assert_eq!(sole_table("select * from a; )"), None);
        assert_eq!(statements_in("select * from a; )"), 2);
    }

    #[test]
    fn a_line_comment_after_a_semicolon_is_still_an_empty_tail() {
        assert_eq!(
            sole(sole_table("select * from a; -- a comment")),
            Some((None, "a".to_owned()))
        );
    }

    #[test]
    fn a_block_comment_after_a_semicolon_is_still_an_empty_tail() {
        assert_eq!(
            sole(sole_table("select * from a;  /* c */")),
            Some((None, "a".to_owned()))
        );
    }
}
