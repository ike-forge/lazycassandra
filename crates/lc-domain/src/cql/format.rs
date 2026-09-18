//! Laying a statement out, without claiming to have parsed it.
//!
//! The rewrite is total but shallow: whitespace is rebuilt from scratch,
//! keywords are upper-cased, and a clause keyword at bracket depth zero opens
//! a line. Nothing is reordered, nothing is dropped, and no literal, comment
//! or name is retyped — so a buffer the scanner misreads comes back laid out
//! oddly rather than meaning something else (design §4).

use super::keywords;
use super::lex::{Kind, Piece, pieces};

/// The indent a continued clause wears. Two, not four: the panel is a few rows
/// tall and every column of the line is one the statement could have used
/// (design §4.2).
const INDENT: &str = "  ";

/// `text`, laid out. Idempotent: formatting the result changes nothing.
#[must_use]
pub fn format(text: &str) -> String {
    let mut out = Layout::default();

    for piece in pieces(text).into_iter().filter(is_written) {
        out.push(&piece);
    }

    out.finish()
}

/// Whitespace is rebuilt rather than kept; everything else is written out.
fn is_written(piece: &Piece) -> bool {
    piece.kind != Kind::Whitespace
}

#[derive(Default)]
struct Layout {
    lines: Vec<String>,
    line: String,
    /// Bracket nesting. A clause keyword inside brackets is a function
    /// argument or a collection's contents, not a clause.
    depth: usize,
    /// Whether a `BEGIN BATCH` is open: its members are written one indent in,
    /// so where one statement ends and the next begins is visible without
    /// reading for the `;` (design §4.2).
    batched: bool,
    previous: Option<Piece>,
}

impl Layout {
    fn push(&mut self, piece: &Piece) {
        // Every rule below reads the piece as it will be written, not as it
        // was typed: `from` opens a clause exactly as `FROM` does.
        let piece = &Piece {
            kind: piece.kind,
            text: Self::written(piece),
        };

        // `APPLY BATCH` closes the block rather than sitting in it. The
        // indent for its line was written when the last member's `;` wrapped,
        // so it has to be taken back out.
        if piece.text == "APPLY" {
            self.batched = false;
            if self.blank() {
                self.line.clear();
            }
        }

        if self.breaks_before(piece) {
            self.wrap(continues(&piece.text));
        }

        if self.spaces_before(piece) {
            self.line.push(' ');
        }

        self.line.push_str(&piece.text);
        self.track(piece);

        if ends_a_line(piece) {
            self.wrap(false);
        }
    }

    /// A keyword is written upper case; everything else is written as typed. A
    /// name's case is load-bearing — CQL folds an unquoted identifier, so
    /// `MyKs` and `myks` are the same table while `"MyKs"` is a different one
    /// (`Statement::quoted`) — and a literal's case is its value.
    fn written(piece: &Piece) -> String {
        if piece.kind == Kind::Keyword {
            piece.text.to_ascii_uppercase()
        } else {
            piece.text.clone()
        }
    }

    /// Whether nothing has been written to the current line. Indent is not
    /// something written: a line holding only [`INDENT`] is still waiting for
    /// its first word.
    fn blank(&self) -> bool {
        self.line.trim_start().is_empty()
    }

    /// A clause opens a line — unless it is the tail of a compound one, which
    /// is what keeps `INSERT INTO`, `DELETE FROM`, `PER PARTITION LIMIT` and
    /// `CREATE TABLE IF NOT EXISTS` whole (design §4.1).
    fn breaks_before(&self, piece: &Piece) -> bool {
        if self.blank() || self.depth > 0 || piece.kind != Kind::Keyword {
            return false;
        }

        let compound = self.previous.as_ref().is_some_and(|previous| {
            previous.kind == Kind::Keyword
                && COMPOUND.contains(&(previous.text.as_str(), piece.text.as_str()))
        });

        !compound && is_clause(&piece.text)
    }

    fn spaces_before(&self, piece: &Piece) -> bool {
        if self.blank() {
            return false;
        }

        let Some(previous) = self.previous.as_ref() else {
            return false;
        };

        // A comment written after something on its line keeps a space before
        // it; every other rule below is about the shape of CQL itself.
        if piece.kind == Kind::Comment {
            return true;
        }

        if tight_after(&previous.text) || tight_before(&piece.text) {
            return false;
        }

        !(piece.text == "(" && previous.kind == Kind::Identifier && self.binds_brackets())
    }

    /// Whether `(` straight after a name is that name's argument list.
    ///
    /// It is, everywhere but in the statements whose brackets hold a column
    /// list: `count(id)` is a call and `INSERT INTO orders (id, total)` is
    /// not, and the two are the same three pieces. The clause the line opened
    /// with is what tells them apart — the one thing a scanner can see without
    /// becoming a parser (design §4.1).
    fn binds_brackets(&self) -> bool {
        !DEFINES
            .iter()
            .any(|clause| self.line.trim_start().starts_with(clause))
    }

    fn track(&mut self, piece: &Piece) {
        if piece.text == "BATCH"
            && self
                .previous
                .as_ref()
                .is_some_and(|previous| previous.text == "BEGIN")
        {
            self.batched = true;
        }

        match piece.text.as_str() {
            "(" | "[" | "{" => self.depth += 1,
            ")" | "]" | "}" => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }

        self.previous = Some(piece.clone());
    }

    /// Closes the line and opens the next, indented or not. A line with
    /// nothing written to it is not closed: two clause keywords in a row would
    /// otherwise leave a blank line between them.
    fn wrap(&mut self, indented: bool) {
        if self.blank() {
            self.line.clear();
            return;
        }

        self.lines.push(std::mem::take(&mut self.line));
        for _ in 0..usize::from(self.batched) + usize::from(indented) {
            self.line.push_str(INDENT);
        }
    }

    fn finish(mut self) -> String {
        self.wrap(false);

        self.lines.join("\n")
    }
}

/// The keyword pairs that are one clause written in two words. Only pairs
/// whose second word opens a clause on its own need listing: every other
/// keyword continues whatever clause it is in and never breaks a line.
const COMPOUND: &[(&str, &str)] = &[
    ("AGGREGATE", "IF"),
    ("COLUMNFAMILY", "IF"),
    ("CREATE", "OR"),
    ("DELETE", "FROM"),
    ("FUNCTION", "IF"),
    ("INDEX", "IF"),
    ("INSERT", "INTO"),
    ("KEYSPACE", "IF"),
    ("PARTITION", "LIMIT"),
    ("ROLE", "IF"),
    ("TABLE", "IF"),
    ("TYPE", "IF"),
    ("USER", "IF"),
    ("VIEW", "IF"),
];

/// The clauses whose brackets hold a definition rather than an argument list.
const DEFINES: &[&str] = &["ALTER", "CREATE", "DROP", "INSERT"];

/// `AND` and `OR` continue the clause above rather than opening one, so they
/// are indented under it — the shape that makes a three-condition `WHERE`
/// readable at a glance.
fn continues(text: &str) -> bool {
    matches!(text, "AND" | "OR")
}

/// The statement separator ends its line, and so does a comment that runs to
/// one — anything written after it on the same line would be inside it.
fn ends_a_line(piece: &Piece) -> bool {
    piece.text == ";" || (piece.kind == Kind::Comment && !piece.text.starts_with("/*"))
}

/// Nothing is written straight after these.
fn tight_after(text: &str) -> bool {
    matches!(text, "(" | "[" | "{" | ".")
}

/// Nothing is written straight before these.
fn tight_before(text: &str) -> bool {
    matches!(text, ")" | "]" | "}" | "." | "," | ";")
}

/// The keywords that open a clause. Every other keyword continues whatever
/// clause it is in.
///
/// A list rather than [`keywords::KEYWORDS`] filtered somehow: which words
/// open a clause is the layout's own knowledge, and the type names — `TEXT`,
/// `INT`, `MAP` — are keywords that must never break a line.
fn is_clause(text: &str) -> bool {
    const CLAUSES: &[&str] = &[
        "ALLOW", "ALTER", "AND", "APPLY", "BEGIN", "CREATE", "DELETE", "DROP", "FROM", "GRANT",
        "GROUP", "IF", "INSERT", "INTO", "LIMIT", "OR", "ORDER", "PER", "REVOKE", "SELECT", "SET",
        "TRUNCATE", "UPDATE", "USE", "USING", "VALUES", "WHERE", "WITH",
    ];

    debug_assert!(
        CLAUSES.iter().all(|clause| keywords::is_keyword(clause)),
        "a clause opener that is not a keyword can never be reached"
    );

    CLAUSES.contains(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Formatting is idempotent, so every expectation below is asserted twice:
    /// once against the source and once against its own output.
    fn formatted(source: &str) -> String {
        let once = format(source);
        assert_eq!(format(&once), once, "formatting is idempotent");

        once
    }

    #[test]
    fn an_empty_buffer_formats_to_nothing() {
        assert_eq!(formatted(""), "");
        assert_eq!(formatted("   \n  "), "");
    }

    #[test]
    fn a_select_breaks_at_each_clause() {
        assert_eq!(
            formatted("select * from shop.orders where id = 7 limit 10"),
            "SELECT *\nFROM shop.orders\nWHERE id = 7\nLIMIT 10"
        );
    }

    #[test]
    fn a_name_keeps_its_case_and_a_keyword_does_not() {
        assert_eq!(
            formatted("select Id from \"MyKs\".Orders"),
            "SELECT Id\nFROM \"MyKs\".Orders"
        );
    }

    #[test]
    fn a_conjunction_is_indented_under_the_clause_it_continues() {
        assert_eq!(
            formatted("SELECT * FROM t WHERE a = 1 AND b = 2 AND c = 3"),
            "SELECT *\nFROM t\nWHERE a = 1\n  AND b = 2\n  AND c = 3"
        );
    }

    #[test]
    fn a_batch_indents_its_members_and_not_its_brackets() {
        assert_eq!(
            formatted(
                "BEGIN BATCH UPDATE t SET a = 1 WHERE id = 7; DELETE FROM t WHERE id = 8; APPLY BATCH"
            ),
            "BEGIN BATCH\n  UPDATE t\n  SET a = 1\n  WHERE id = 7;\n  DELETE FROM t\n  WHERE id = 8;\nAPPLY BATCH"
        );
    }

    #[test]
    fn a_conjunction_inside_a_batch_is_indented_under_its_member() {
        assert_eq!(
            formatted("BEGIN BATCH UPDATE t SET a = 1 WHERE id = 7 AND at = 8; APPLY BATCH"),
            "BEGIN BATCH\n  UPDATE t\n  SET a = 1\n  WHERE id = 7\n    AND at = 8;\nAPPLY BATCH"
        );
    }

    #[test]
    fn a_compound_clause_stays_on_one_line() {
        assert_eq!(
            formatted("SELECT * FROM t PER PARTITION LIMIT 1 ORDER BY a DESC ALLOW FILTERING"),
            "SELECT *\nFROM t\nPER PARTITION LIMIT 1\nORDER BY a DESC\nALLOW FILTERING"
        );
    }

    #[test]
    fn an_insert_keeps_into_with_its_verb() {
        assert_eq!(
            formatted("insert into shop.orders (id, total) values (1, 2)"),
            "INSERT INTO shop.orders (id, total)\nVALUES (1, 2)"
        );
    }

    #[test]
    fn a_clause_keyword_inside_brackets_is_not_a_clause() {
        assert_eq!(
            formatted("SELECT id FROM t WHERE a IN (1, 2) AND b = token(c)"),
            "SELECT id\nFROM t\nWHERE a IN (1, 2)\n  AND b = token(c)"
        );
    }

    #[test]
    fn a_function_binds_its_brackets_and_a_keyword_does_not() {
        assert_eq!(
            formatted("SELECT count ( id ) FROM t WHERE a IN ( 1 )"),
            "SELECT count(id)\nFROM t\nWHERE a IN (1)"
        );
    }

    #[test]
    fn each_statement_gets_its_own_line() {
        assert_eq!(formatted("SELECT 1; SELECT 2;"), "SELECT 1;\nSELECT 2;");
    }

    #[test]
    fn a_line_comment_closes_its_line_and_keeps_what_follows_off_it() {
        assert_eq!(
            formatted("SELECT 1 -- why\nFROM t"),
            "SELECT 1 -- why\nFROM t"
        );
    }

    #[test]
    fn a_block_comment_stays_where_it_was_written() {
        assert_eq!(
            formatted("SELECT /* all */ * FROM t"),
            "SELECT /* all */ *\nFROM t"
        );
    }

    #[test]
    fn a_string_is_never_retyped() {
        assert_eq!(
            formatted("update t set name = 'From  Where' where id = 1"),
            "UPDATE t\nSET name = 'From  Where'\nWHERE id = 1"
        );
    }

    #[test]
    fn a_create_table_keeps_its_definition_together() {
        assert_eq!(
            formatted(
                "create table if not exists shop.orders (id uuid primary key, total int) with comment = 'x'"
            ),
            "CREATE TABLE IF NOT EXISTS shop.orders (id UUID PRIMARY KEY, total INT)\nWITH comment = 'x'"
        );
    }

    #[test]
    fn already_formatted_text_is_left_alone() {
        let laid_out = "SELECT *\nFROM shop.orders\nWHERE id = 7\n  AND total > 10\nLIMIT 200";

        assert_eq!(formatted(laid_out), laid_out);
    }
}
