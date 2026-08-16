//! What each name in a statement means by where it sits.
//!
//! A pass over the scan `lex` already produced, not a second scan and not a
//! parser: it assigns a [`Role`] to every identifier and collects the tables
//! each statement names. Grammar position alone decides a role — the schema
//! is never consulted — so a name the cluster has never heard of is coloured
//! by what the statement is doing with it
//! (docs/superpowers/specs/2026-08-16-cql-semantics-design.md §3).
//!
//! This task covers `SELECT` alone and leaves the module's only caller its
//! own tests: `complete.rs` starts calling [`resolve`] once task 5 adds
//! `context_at`, which is also when the machine's coverage grows past a
//! single statement kind.

use super::lex::{Kind, Piece};

/// What a name is, where it stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Keyspace,
    Table,
    Column,
    /// A name introduced by `AS`, which stands for a selector rather than for
    /// anything the schema holds.
    Alias,
    Function,
    /// A CQL type, in a column definition or a `CAST`.
    Type,
    /// An option name in a `WITH` clause — `replication`, `compaction`.
    Property,
}

/// A table as the statement wrote it.
///
/// Folded to lower case where it was unquoted, because CQL folds an unquoted
/// name and `Schema` is indexed by the folded one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub keyspace: Option<String>,
    pub table: String,
}

/// What a name typed at a position would be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Expect {
    #[default]
    Keyword,
    Keyspace,
    Table,
    Column,
    Value,
    /// A position that takes no name at all — inside `ALLOW FILTERING`, past
    /// the end of a statement the machine stopped following.
    Nothing,
}

/// Every statement in the buffer, and what each of its pieces means.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Analysis {
    /// One entry per piece, in the order `lex::pieces` produced them.
    roles: Vec<Option<Role>>,
    /// What an identifier at each piece would be. One longer than `roles`:
    /// the last entry is the expectation past the final piece, which is where
    /// a cursor at the end of the buffer sits.
    expects: Vec<Expect>,
    /// The statement each piece belongs to, as an index into `tables`.
    statement: Vec<usize>,
    tables: Vec<Vec<Name>>,
}

impl Analysis {
    /// The role of the piece at `index`, or `None` where it carries no name.
    #[must_use]
    pub fn role(&self, index: usize) -> Option<Role> {
        self.roles.get(index).copied().flatten()
    }

    /// What a name typed at `index` would be.
    #[must_use]
    pub fn expects(&self, index: usize) -> Expect {
        self.expects.get(index).copied().unwrap_or_default()
    }

    /// The tables named by the statement the piece at `index` belongs to.
    #[must_use]
    pub fn tables_at(&self, index: usize) -> &[Name] {
        self.statement
            .get(index)
            .and_then(|statement| self.tables.get(*statement))
            .map_or(&[], Vec::as_slice)
    }
}

/// `pieces` with every name given the meaning its position carries.
///
/// Crate-internal, like [`Piece`] itself: a caller outside `cql` has no scan
/// to hand it, and reaches this through `highlight` or `suggest` instead.
#[must_use]
#[allow(
    dead_code,
    reason = "resolve's first in-crate caller lands in task 5's context_at"
)]
pub(crate) fn resolve(pieces: &[Piece]) -> Analysis {
    let mut machine = Machine {
        tables: vec![Vec::new()],
        ..Machine::default()
    };

    for piece in pieces {
        machine.step(piece);
    }
    machine.finish(pieces.len())
}

/// The clause the fold is inside, which is what an identifier's role is read
/// off. One flat state rather than a state per statement kind: every clause
/// that takes a column takes it the same way, and the verb only decides which
/// clause comes first.
#[derive(Debug, Default)]
#[allow(
    dead_code,
    reason = "resolve's first in-crate caller lands in task 5's context_at"
)]
struct Machine {
    expect: Expect,
    /// Whether the statement's first keyword has been read yet. Distinct from
    /// `covered`: once the verb is decided, a later keyword that happens to
    /// share a spelling with a covered verb — `SELECT` as a permission name
    /// in `GRANT SELECT ON …` — must not flip the statement's coverage.
    verb_seen: bool,
    /// Whether the statement's verb is one this module resolves. Meaningless
    /// until `verb_seen` is `true`; stays `false` for the rest of a statement
    /// whose verb this module does not cover, which leaves every name plain.
    covered: bool,
    depth: usize,
    roles: Vec<Option<Role>>,
    expects: Vec<Expect>,
    statement: Vec<usize>,
    tables: Vec<Vec<Name>>,
    /// The keyspace of a qualified name whose table has not been read yet,
    /// paired with the index of the role it was provisionally given: a bare
    /// name in table position is a table until a `.` proves it was a
    /// keyspace, at which point that earlier role is rewritten in place.
    pending_keyspace: Option<(usize, String)>,
    /// Whether the piece immediately before the current one — no whitespace,
    /// no comment, nothing skipped — was a `.`. The only thing that makes
    /// `pending_keyspace` consumable: without it, two bare names in table
    /// position back to back (`FROM shop orders`, `FROM shop AS x`) would
    /// read as one qualified name instead of staying separate.
    dot_before: bool,
}

#[allow(
    dead_code,
    reason = "resolve's first in-crate caller lands in task 5's context_at"
)]
impl Machine {
    fn step(&mut self, piece: &Piece) {
        self.expects.push(self.expect);
        self.statement.push(self.tables.len() - 1);

        let role = match piece.kind {
            Kind::Keyword => {
                self.keyword(&piece.text.to_ascii_uppercase());
                None
            }
            Kind::Punctuation => {
                self.punctuation(&piece.text);
                None
            }
            Kind::Whitespace | Kind::Comment | Kind::Literal => None,
            Kind::Identifier => self.identifier(&piece.text),
        };

        self.dot_before = piece.kind == Kind::Punctuation && piece.text == ".";
        self.roles.push(role);
    }

    fn keyword(&mut self, word: &str) {
        if !self.verb_seen {
            self.verb_seen = true;
            self.covered = matches!(
                word,
                "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "TRUNCATE" | "USE"
            );
        }

        if !self.covered {
            self.expect = Expect::Nothing;
            return;
        }

        self.expect = match word {
            "FROM" | "INTO" | "UPDATE" | "TRUNCATE" => Expect::Table,
            "USE" => Expect::Keyspace,
            "SELECT" | "DELETE" | "WHERE" | "AND" | "IF" | "SET" | "BY" => Expect::Column,
            "VALUES" | "USING" | "LIMIT" => Expect::Value,
            // `AS` introduces an alias, which is a name this task does not
            // yet give a role — but it must still stop the identifier that
            // follows from being read as a second table.
            "ALLOW" | "FILTERING" | "EXISTS" | "NOT" | "AS" => Expect::Nothing,
            _ => self.expect,
        };
    }

    fn punctuation(&mut self, text: &str) {
        for character in text.chars() {
            match character {
                '(' => self.depth += 1,
                ')' => self.depth = self.depth.saturating_sub(1),
                ';' => self.end_statement(),
                '=' | '<' | '>' | '!' => self.expect = Expect::Value,
                _ => {}
            }
        }
    }

    fn identifier(&mut self, text: &str) -> Option<Role> {
        let name = fold(text);

        match self.expect {
            Expect::Keyspace => {
                self.record(Name {
                    keyspace: None,
                    table: name,
                });
                Some(Role::Keyspace)
            }
            Expect::Table => {
                // Only a `.` immediately before this identifier proves a
                // pending keyspace was meant to qualify it; anything else —
                // including a bare word right before, `FROM shop orders` —
                // drops it rather than fusing two unrelated names.
                let qualified = self.pending_keyspace.take().filter(|_| self.dot_before);

                if let Some((index, keyspace)) = qualified {
                    self.roles[index] = Some(Role::Keyspace);
                    self.record(Name {
                        keyspace: Some(keyspace),
                        table: name,
                    });
                } else {
                    self.pending_keyspace = Some((self.roles.len(), name.clone()));
                    self.record(Name {
                        keyspace: None,
                        table: name,
                    });
                }
                Some(Role::Table)
            }
            Expect::Column => Some(Role::Column),
            Expect::Keyword | Expect::Value | Expect::Nothing => None,
        }
    }

    /// The keyspace of a qualified name is recorded first as a bare table and
    /// rewritten when the table itself arrives, so a half-typed `shop.` still
    /// names something.
    fn record(&mut self, name: Name) {
        let Some(current) = self.tables.last_mut() else {
            return;
        };

        match current.last_mut() {
            Some(last) if name.keyspace.is_some() && last.keyspace.is_none() => *last = name,
            _ => current.push(name),
        }
    }

    fn end_statement(&mut self) {
        self.tables.push(Vec::new());
        self.expect = Expect::Keyword;
        self.verb_seen = false;
        self.covered = false;
        self.depth = 0;
        self.pending_keyspace = None;
    }

    fn finish(mut self, pieces: usize) -> Analysis {
        debug_assert_eq!(self.roles.len(), pieces);
        self.expects.push(self.expect);

        Analysis {
            roles: self.roles,
            expects: self.expects,
            statement: self.statement,
            tables: self.tables,
        }
    }
}

/// CQL folds an unquoted name and keeps a quoted one exactly.
#[allow(
    dead_code,
    reason = "resolve's first in-crate caller lands in task 5's context_at"
)]
fn fold(text: &str) -> String {
    match text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(quoted) => quoted.to_owned(),
        None => text.to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cql::lex::pieces;

    /// The role of every identifier in `text`, paired with its text, in order.
    fn roles(text: &str) -> Vec<(String, Option<Role>)> {
        let scanned = pieces(text);
        let analysis = resolve(&scanned);

        scanned
            .iter()
            .enumerate()
            .filter(|(_, piece)| piece.kind == Kind::Identifier)
            .map(|(index, piece)| (piece.text.clone(), analysis.role(index)))
            .collect()
    }

    #[test]
    fn a_select_names_its_columns_and_its_qualified_table() {
        assert_eq!(
            roles("SELECT id, total FROM shop.orders WHERE id = 7"),
            vec![
                ("id".to_owned(), Some(Role::Column)),
                ("total".to_owned(), Some(Role::Column)),
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("id".to_owned(), Some(Role::Column)),
            ]
        );
    }

    #[test]
    fn an_unqualified_table_is_a_table_and_names_no_keyspace() {
        assert_eq!(
            roles("SELECT * FROM orders"),
            vec![("orders".to_owned(), Some(Role::Table))]
        );

        let scanned = pieces("SELECT * FROM orders");
        assert_eq!(
            resolve(&scanned).tables_at(scanned.len() - 1),
            &[Name {
                keyspace: None,
                table: "orders".to_owned()
            }]
        );
    }

    #[test]
    fn a_statement_records_the_table_it_names_at_every_piece_of_it() {
        // The column being typed is before the FROM clause; the table is
        // still in scope, which is what completion turns on.
        let scanned = pieces("SELECT id FROM shop.orders");
        let analysis = resolve(&scanned);

        assert_eq!(
            analysis.tables_at(2),
            &[Name {
                keyspace: Some("shop".to_owned()),
                table: "orders".to_owned()
            }]
        );
    }

    #[test]
    fn a_name_is_folded_unless_it_was_quoted() {
        let scanned = pieces("SELECT * FROM Shop.\"Orders\"");

        assert_eq!(
            resolve(&scanned).tables_at(scanned.len() - 1),
            &[Name {
                keyspace: Some("shop".to_owned()),
                table: "Orders".to_owned()
            }]
        );
    }

    #[test]
    fn an_alias_does_not_fuse_with_the_table_before_it() {
        assert_eq!(
            roles("SELECT * FROM shop AS x"),
            vec![
                ("shop".to_owned(), Some(Role::Table)),
                ("x".to_owned(), None),
            ]
        );

        let scanned = pieces("SELECT * FROM shop AS x");
        assert_eq!(
            resolve(&scanned).tables_at(scanned.len() - 1),
            &[Name {
                keyspace: None,
                table: "shop".to_owned()
            }]
        );
    }

    #[test]
    fn two_bare_table_names_in_a_row_stay_separate() {
        let scanned = pieces("SELECT * FROM shop orders");

        assert_eq!(
            resolve(&scanned).tables_at(scanned.len() - 1),
            &[
                Name {
                    keyspace: None,
                    table: "shop".to_owned()
                },
                Name {
                    keyspace: None,
                    table: "orders".to_owned()
                }
            ]
        );
    }

    #[test]
    fn an_uncovered_statement_leaves_every_name_plain() {
        assert_eq!(
            roles("GRANT SELECT ON shop.orders TO ike"),
            vec![
                ("shop".to_owned(), None),
                ("orders".to_owned(), None),
                ("ike".to_owned(), None),
            ]
        );
    }

    #[test]
    fn a_literal_is_never_a_name() {
        let scanned = pieces("SELECT * FROM t WHERE id = 7");
        let analysis = resolve(&scanned);

        for (index, piece) in scanned.iter().enumerate() {
            if piece.kind == Kind::Literal {
                assert_eq!(analysis.role(index), None, "{:?} is a value", piece.text);
            }
        }
    }

    #[test]
    fn every_prefix_of_a_statement_resolves_without_panicking() {
        let text = "SELECT id, total FROM shop.orders WHERE id = 7";

        for cut in 0..=text.len() {
            if !text.is_char_boundary(cut) {
                continue;
            }
            let _ = resolve(&pieces(&text[..cut]));
        }
    }
}
