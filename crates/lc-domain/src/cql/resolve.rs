//! What each name in a statement means by where it sits.
//!
//! A pass over the scan `lex` already produced, not a second scan and not a
//! parser: it assigns a [`Role`] to every identifier and collects the tables
//! each statement names. Grammar position alone decides a role — the schema
//! is never consulted — so a name the cluster has never heard of is coloured
//! by what the statement is doing with it
//! (docs/superpowers/specs/2026-08-16-cql-semantics-design.md §3).
//!
//! This covers CQL's data statements — `SELECT`, `INSERT`, `UPDATE`,
//! `DELETE`, `TRUNCATE`, `USE`, and `BEGIN … BATCH` over them — and its
//! schema statements — `CREATE`/`ALTER`/`DROP` of `KEYSPACE` and `TABLE`,
//! including `WITH` properties and column definitions. It also answers what
//! a cursor sits on — [`context_at`] — which `suggest` calls to switch its
//! candidates on the grammar position under the cursor.

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
    /// The name after `AS`, which stands for the selector it renames rather
    /// than for anything the schema holds.
    Alias,
    /// Right after a column name inside a `CREATE`/`ALTER` table body, where
    /// a column definition alternates name and type.
    Type,
    /// After `WITH`, where a name is a `CREATE`/`ALTER` option rather than a
    /// column — `replication`, `durable_writes`, `comment`.
    Property,
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

    /// How many statements the buffer holds — a `BEGIN BATCH` counts as one
    /// per `;`-separated scope, not one for the batch as a whole, and a
    /// trailing `;` with nothing typed after it does not open a statement of
    /// its own: [`Machine::finish`] never lets that placeholder scope
    /// survive into here.
    #[must_use]
    pub fn statements(&self) -> usize {
        self.tables.len()
    }
}

/// What a name typed at a cursor would be, and what tables are in scope
/// there (design §5).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Context {
    pub expects: Expect,
    pub tables: Vec<Name>,
}

/// The context at `offset`, counted in characters from the start of the text
/// `pieces` was scanned from.
///
/// A cursor inside an identifier takes that identifier's own expectation —
/// the word being typed is the one being offered against. A cursor in
/// whitespace takes the expectation the next name would carry.
///
/// Crate-internal like [`resolve`]: `Piece` is not part of the public API,
/// so a caller outside `cql` has no scan to hand it and reaches this through
/// `suggest` instead.
#[must_use]
pub(crate) fn context_at(pieces: &[Piece], offset: usize) -> Context {
    let analysis = resolve(pieces);
    let index = piece_at(pieces, offset);

    Context {
        expects: analysis.expects(index),
        tables: analysis.tables_at(index.saturating_sub(1)).to_vec(),
    }
}

/// The index of the piece `offset` falls inside, or one past the last piece
/// where it is at the very end of the text.
fn piece_at(pieces: &[Piece], offset: usize) -> usize {
    let mut at = 0;

    for (index, piece) in pieces.iter().enumerate() {
        let len = piece.text.chars().count();
        if offset < at + len {
            return index;
        }
        at += len;
    }

    pieces.len()
}

/// `pieces` with every name given the meaning its position carries.
///
/// Crate-internal, like [`Piece`] itself: a caller outside `cql` has no scan
/// to hand it, and reaches this through `highlight` or `suggest` instead.
#[must_use]
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

/// What a statement's opening keyword decided about the rest of it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Verb {
    #[default]
    Unread,
    /// `GRANT`, `LIST`, … — anything this module has no clause table for.
    /// Every name in the statement is left with no role.
    Uncovered,
    /// `SELECT`, `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `USE`.
    Data,
    /// `CREATE` or `ALTER`, whose table body alternates a column name and
    /// its type.
    Schema,
}

/// The clause the fold is inside, which is what an identifier's role is read
/// off. One flat state rather than a state per statement kind: every clause
/// that takes a column takes it the same way, and the verb only decides which
/// clause comes first.
#[derive(Debug, Default)]
struct Machine {
    expect: Expect,
    /// The clause `expect` returns to once a value or an alias that
    /// interrupted it is done. Set only when a keyword actually opens a
    /// clause — `SET`, `WHERE`, `VALUES`, `FROM`, … — never by `=` or `AS`,
    /// which is what makes it the thing left standing after either kind of
    /// interruption. A `,` restores `expect` from it, which is the one
    /// mechanism behind both a `SET a = 1, b = 2` list staying on `Column`
    /// after each value and a `VALUES (1, 2)` list staying on `Value`; an
    /// alias reads it the same way once the aliased name is consumed.
    base: Expect,
    /// What the statement's verb decided, fixed the moment the first keyword
    /// is read: distinct from a fresh `Verb::Unread`, a later keyword that
    /// happens to share a spelling with a verb — `SELECT` as a permission
    /// name in `GRANT SELECT ON …` — must not re-decide it.
    verb: Verb,
    /// Whether the position is inside a column definition's name/type pair —
    /// a `CREATE TABLE` body between its parentheses, or the single pair
    /// after `ADD`. Read by `identifier` to flip `Expect::Column` to
    /// `Expect::Type` once the name is consumed.
    alternating: bool,
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
    /// Whether a `.` is the last grammatically significant thing seen —
    /// whitespace and comments don't count as breaking it, since CQL allows
    /// either around the dot in a qualified name. The only thing that makes
    /// `pending_keyspace` consumable: without it, two bare names in table
    /// position back to back (`FROM shop orders`, `FROM shop AS x`) would
    /// read as one qualified name instead of staying separate.
    dot_before: bool,
    /// The `roles` index of the identifier piece last seen, provided nothing
    /// but whitespace or a comment has come since — the same carry rule as
    /// `dot_before`. A `(` rewrites that piece's role to `Function` only
    /// when this is `Some`, which is what keeps `count(id), max(id)` from
    /// having the `(` after `max` walk back past the comma and relabel `id`.
    last_identifier: Option<usize>,
    /// Whether any piece but whitespace or a comment has been seen since the
    /// last `end_statement` — not `verb != Verb::Unread`, because a stray
    /// identifier or a lone `)` after a `;` never reaches `keyword()` either,
    /// and would otherwise leave a scope that holds real, if nonsense, text
    /// looking exactly like the placeholder a trailing `;` opens on the
    /// chance nothing follows. `finish` reads this to tell the two apart.
    has_content: bool,
}

impl Machine {
    fn step(&mut self, piece: &Piece) {
        self.expects.push(self.expect);
        self.statement.push(self.tables.len() - 1);
        self.has_content |= !matches!(piece.kind, Kind::Whitespace | Kind::Comment);

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

        self.dot_before = match piece.kind {
            Kind::Whitespace | Kind::Comment => self.dot_before,
            Kind::Punctuation => piece.text == ".",
            Kind::Keyword | Kind::Literal | Kind::Identifier => false,
        };
        self.last_identifier = match piece.kind {
            Kind::Whitespace | Kind::Comment => self.last_identifier,
            Kind::Identifier => Some(self.roles.len()),
            Kind::Keyword | Kind::Punctuation | Kind::Literal => None,
        };
        self.roles.push(role);
    }

    fn keyword(&mut self, word: &str) {
        // A batch is a wrapper, not a statement: the words that open and
        // close it change nothing, and the statements it nests are resolved
        // exactly as they would be standalone.
        if matches!(word, "BEGIN" | "BATCH" | "APPLY" | "UNLOGGED" | "COUNTER") {
            return;
        }

        if self.verb == Verb::Unread {
            self.verb = match word {
                "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "TRUNCATE" | "USE" | "DROP" => {
                    Verb::Data
                }
                "CREATE" | "ALTER" => Verb::Schema,
                _ => Verb::Uncovered,
            };
        }

        if self.verb == Verb::Uncovered {
            self.expect = Expect::Nothing;
            return;
        }

        self.expect = match word {
            "FROM" | "INTO" | "UPDATE" | "TRUNCATE" | "TABLE" => self.enter(Expect::Table),
            "USE" | "KEYSPACE" | "SCHEMA" => self.enter(Expect::Keyspace),
            "SELECT" | "DELETE" | "WHERE" | "SET" | "BY" | "TOKEN" | "PRIMARY" | "RENAME"
            | "DROP" => self.enter(Expect::Column),
            // `AND` continues whatever clause is already open rather than
            // assuming a `WHERE`/`IF` condition list: in a `WITH` clause
            // it chains another option (`WITH a = 1 AND b = 2`), and it must
            // not steal that clause back into `Column`.
            "AND" => {
                let base = self.base;
                self.enter(base)
            }
            "ADD" => {
                self.alternating = true;
                self.enter(Expect::Column)
            }
            "WITH" => self.enter(Expect::Property),
            "VALUES" | "USING" | "LIMIT" | "IN" | "CONTAINS" => self.enter(Expect::Value),
            // `IF` opens an `UPDATE`/`DELETE` condition list the same way
            // `WHERE` does, unless a table or keyspace name is still
            // outstanding — `DROP TABLE IF EXISTS …`, `CREATE … IF NOT
            // EXISTS …` — in which case `IF [NOT] EXISTS` is a modifier on
            // that pending name and must not steal its clause.
            "IF" if !matches!(self.expect, Expect::Table | Expect::Keyspace) => {
                self.enter(Expect::Column)
            }
            // `AS` interrupts the clause it sits in rather than opening one
            // of its own, so it leaves `base` alone — `identifier` reads
            // `base` to resume that clause once the alias is consumed.
            "AS" => Expect::Alias,
            // `ALLOW FILTERING`, `IF [NOT] EXISTS`, `DISTINCT`, `JSON`: none
            // of these take a name, so they fall through unchanged like any
            // other keyword this machine has no clause for.
            _ => self.expect,
        };
    }

    /// Opens a clause: what `expect` becomes now, and what `,` and an alias
    /// return it to once something has interrupted it.
    fn enter(&mut self, clause: Expect) -> Expect {
        self.base = clause;
        clause
    }

    fn punctuation(&mut self, text: &str) {
        for character in text.chars() {
            match character {
                '(' => {
                    if self.expect == Expect::Column {
                        if let Some(index) = self.last_identifier {
                            self.roles[index] = Some(Role::Function);
                        }
                    } else if self.expect == Expect::Table {
                        // `INSERT INTO t (` opens the column list right after
                        // the table, with no clause keyword in between; so
                        // does `CREATE TABLE t (`, whose body alternates a
                        // column name and its type rather than staying a
                        // plain name list.
                        self.expect = self.enter(Expect::Column);
                    }
                    if self.verb == Verb::Schema {
                        self.alternating = true;
                    }
                    self.depth += 1;
                }
                ')' => {
                    self.depth = self.depth.saturating_sub(1);
                    self.alternating = false;
                }
                ',' => self.expect = self.base,
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
            Expect::Column => {
                if self.alternating {
                    self.expect = Expect::Type;
                }
                Some(Role::Column)
            }
            Expect::Type => {
                self.expect = Expect::Column;
                Some(Role::Type)
            }
            Expect::Property => Some(Role::Property),
            Expect::Alias => {
                // An alias only ever renames the one selector `AS` follows;
                // the clause it interrupted resumes right after, the same
                // way a `,` resumes it past a value.
                self.expect = self.base;
                Some(Role::Alias)
            }
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
        self.base = Expect::Keyword;
        self.verb = Verb::Unread;
        self.alternating = false;
        self.depth = 0;
        self.pending_keyspace = None;
        self.last_identifier = None;
        self.has_content = false;
    }

    fn finish(mut self, pieces: usize) -> Analysis {
        debug_assert_eq!(self.roles.len(), pieces);
        self.expects.push(self.expect);

        // `end_statement` opens the next scope the moment it sees `;`, on
        // the chance something follows it. Nothing but whitespace or a
        // comment ever did here — so the buffer holds one statement fewer
        // than one scope per `;` would suggest: a trailing `;` (whitespace
        // or a trailing comment after it included) must not make an
        // ordinary `select … from t;` look like two statements and defeat
        // `sole_table`'s one-statement guard. `has_content` rather than
        // `verb != Verb::Unread`: a stray identifier or a lone `)` reaches
        // neither `keyword()` nor `record()`, so checking the verb alone
        // would pop a scope that holds real, if nonsense, text and make it
        // disappear from the count entirely.
        if !self.has_content && self.tables.last().is_some_and(Vec::is_empty) {
            self.tables.pop();
        }

        Analysis {
            roles: self.roles,
            expects: self.expects,
            statement: self.statement,
            tables: self.tables,
        }
    }
}

/// CQL folds an unquoted name and keeps a quoted one exactly.
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

    /// The four shapes a `.` and its neighbours can take in table position,
    /// pinned together so a fix for one cannot silently break another:
    /// whitespace around a real dot still qualifies, but no dot at all —
    /// whether the second word is an alias or just another bare name — never
    /// fuses two names into one.
    #[test]
    fn only_a_real_dot_qualifies_a_table_whitespace_or_not() {
        let qualified = |text: &str| {
            let scanned = pieces(text);
            resolve(&scanned).tables_at(scanned.len() - 1).to_vec()
        };

        assert_eq!(
            qualified("SELECT * FROM shop.orders"),
            vec![Name {
                keyspace: Some("shop".to_owned()),
                table: "orders".to_owned()
            }],
            "no space around the dot"
        );

        assert_eq!(
            qualified("SELECT * FROM shop . orders"),
            vec![Name {
                keyspace: Some("shop".to_owned()),
                table: "orders".to_owned()
            }],
            "CQL allows space around the dot"
        );

        assert_eq!(
            qualified("SELECT * FROM shop AS x"),
            vec![Name {
                keyspace: None,
                table: "shop".to_owned()
            }],
            "AS introduces an alias, not a second table"
        );
        assert_eq!(
            roles("SELECT * FROM shop AS x"),
            vec![
                ("shop".to_owned(), Some(Role::Table)),
                ("x".to_owned(), Some(Role::Alias)),
            ]
        );

        assert_eq!(
            qualified("SELECT * FROM shop orders"),
            vec![
                Name {
                    keyspace: None,
                    table: "shop".to_owned()
                },
                Name {
                    keyspace: None,
                    table: "orders".to_owned()
                }
            ],
            "two bare names in a row are not fused"
        );
    }

    #[test]
    fn a_comment_between_the_dot_and_its_neighbour_still_qualifies() {
        let scanned = pieces("SELECT * FROM shop /* c */ . orders");

        assert_eq!(
            resolve(&scanned).tables_at(scanned.len() - 1),
            &[Name {
                keyspace: Some("shop".to_owned()),
                table: "orders".to_owned()
            }]
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
    fn an_insert_names_its_table_and_its_column_list() {
        assert_eq!(
            roles("INSERT INTO shop.orders (id, total) VALUES (7, 'x')"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("id".to_owned(), Some(Role::Column)),
                ("total".to_owned(), Some(Role::Column)),
            ],
            "the values are literals, and a value is never a name"
        );
    }

    #[test]
    fn an_update_names_its_table_then_its_assignments_and_conditions() {
        assert_eq!(
            roles("UPDATE shop.orders SET total = 3 WHERE id = 7 IF total = 2"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("total".to_owned(), Some(Role::Column)),
                ("id".to_owned(), Some(Role::Column)),
                ("total".to_owned(), Some(Role::Column)),
            ]
        );
    }

    #[test]
    fn a_comma_in_a_set_clause_resumes_column_rather_than_staying_on_value() {
        assert_eq!(
            roles("UPDATE shop.orders SET total = 3, price = 5 WHERE id = 7"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("total".to_owned(), Some(Role::Column)),
                ("price".to_owned(), Some(Role::Column)),
                ("id".to_owned(), Some(Role::Column)),
            ],
            "a value ending at a comma must not leave every later assignment unrolled"
        );
    }

    #[test]
    fn an_inserts_name_list_and_value_list_each_keep_their_own_role_past_a_comma() {
        assert_eq!(
            roles("INSERT INTO shop.orders (id, total) VALUES (7, 8)"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("id".to_owned(), Some(Role::Column)),
                ("total".to_owned(), Some(Role::Column)),
            ],
            "the name list stays columns and the value list stays values, \
             on both sides of their own comma"
        );
    }

    #[test]
    fn a_delete_names_the_columns_before_its_from() {
        assert_eq!(
            roles("DELETE total FROM shop.orders WHERE id = 7"),
            vec![
                ("total".to_owned(), Some(Role::Column)),
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("id".to_owned(), Some(Role::Column)),
            ]
        );
    }

    #[test]
    fn truncate_takes_a_table_with_or_without_the_word_table() {
        assert_eq!(
            roles("TRUNCATE shop.orders"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table))
            ]
        );
        assert_eq!(
            roles("TRUNCATE TABLE shop.orders"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table))
            ]
        );
    }

    #[test]
    fn use_takes_a_keyspace_and_nothing_else() {
        assert_eq!(
            roles("USE shop"),
            vec![("shop".to_owned(), Some(Role::Keyspace))]
        );
    }

    #[test]
    fn a_batch_resolves_each_statement_it_nests() {
        assert_eq!(
            roles(
                "BEGIN BATCH \
                 INSERT INTO shop.orders (id) VALUES (1); \
                 UPDATE shop.customers SET name = 'a' WHERE id = 2; \
                 APPLY BATCH"
            ),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("id".to_owned(), Some(Role::Column)),
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("customers".to_owned(), Some(Role::Table)),
                ("name".to_owned(), Some(Role::Column)),
                ("id".to_owned(), Some(Role::Column)),
            ]
        );
    }

    #[test]
    fn a_select_list_alias_is_not_a_column() {
        assert_eq!(
            roles("SELECT id AS number FROM t"),
            vec![
                ("id".to_owned(), Some(Role::Column)),
                ("number".to_owned(), Some(Role::Alias)),
                ("t".to_owned(), Some(Role::Table)),
            ]
        );
    }

    #[test]
    fn a_call_in_a_select_list_is_a_function_over_its_columns() {
        assert_eq!(
            roles("SELECT count(id), writetime(total) FROM t"),
            vec![
                ("count".to_owned(), Some(Role::Function)),
                ("id".to_owned(), Some(Role::Column)),
                ("writetime".to_owned(), Some(Role::Function)),
                ("total".to_owned(), Some(Role::Column)),
                ("t".to_owned(), Some(Role::Table)),
            ]
        );
    }

    #[test]
    fn a_second_statement_starts_a_fresh_scope() {
        let scanned = pieces("SELECT * FROM a; SELECT * FROM b");
        let analysis = resolve(&scanned);
        let last = scanned.len() - 1;

        assert_eq!(
            analysis.tables_at(0),
            &[Name {
                keyspace: None,
                table: "a".to_owned()
            }]
        );
        assert_eq!(
            analysis.tables_at(last),
            &[Name {
                keyspace: None,
                table: "b".to_owned()
            }]
        );
    }

    #[test]
    fn a_create_table_names_its_keyspace_table_columns_and_their_types() {
        // `uuid` and `int` lex as `Kind::Keyword` (keywords.rs), so they
        // never carry a role and `roles` skips them; the identifiers that
        // remain are the keyspace, the table, and the two column names.
        assert_eq!(
            roles("CREATE TABLE shop.orders (id uuid, total int, PRIMARY KEY (id))"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("id".to_owned(), Some(Role::Column)),
                ("total".to_owned(), Some(Role::Column)),
                ("id".to_owned(), Some(Role::Column)),
            ]
        );
    }

    #[test]
    fn a_user_defined_type_in_a_column_definition_is_a_type() {
        assert_eq!(
            roles("CREATE TABLE t (id address)"),
            vec![
                ("t".to_owned(), Some(Role::Table)),
                ("id".to_owned(), Some(Role::Column)),
                ("address".to_owned(), Some(Role::Type)),
            ]
        );
    }

    #[test]
    fn a_with_clause_names_options_rather_than_columns() {
        assert_eq!(
            roles("CREATE KEYSPACE shop WITH replication = {'class': 'SimpleStrategy'}"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("replication".to_owned(), Some(Role::Property)),
            ]
        );
    }

    #[test]
    fn dropping_and_altering_name_what_they_act_on() {
        assert_eq!(
            roles("DROP TABLE IF EXISTS shop.orders"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
            ]
        );
        assert_eq!(
            // `text` lexes as a keyword, so it carries no role; `note` is
            // the only identifier `ADD` introduces.
            roles("ALTER TABLE shop.orders ADD note text"),
            vec![
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("note".to_owned(), Some(Role::Column)),
            ]
        );
        assert_eq!(
            roles("DROP KEYSPACE shop"),
            vec![("shop".to_owned(), Some(Role::Keyspace))]
        );
    }

    #[test]
    fn a_chained_with_option_stays_a_property_rather_than_reopening_a_condition_list() {
        assert_eq!(
            roles("CREATE TABLE t (id uuid) WITH comment = 'x' AND compaction = {'class': 'X'}"),
            vec![
                ("t".to_owned(), Some(Role::Table)),
                ("id".to_owned(), Some(Role::Column)),
                ("comment".to_owned(), Some(Role::Property)),
                ("compaction".to_owned(), Some(Role::Property)),
            ],
            "the map literal's keys carry no role either"
        );
        assert_eq!(
            roles(
                "ALTER KEYSPACE k WITH durable_writes = true \
                 AND replication = {'class': 'SimpleStrategy'}"
            ),
            vec![
                ("k".to_owned(), Some(Role::Keyspace)),
                ("durable_writes".to_owned(), Some(Role::Property)),
                ("replication".to_owned(), Some(Role::Property)),
            ]
        );
    }

    #[test]
    fn a_where_clauses_and_still_yields_a_column() {
        assert_eq!(
            roles("SELECT * FROM t WHERE a = 1 AND b = 2"),
            vec![
                ("t".to_owned(), Some(Role::Table)),
                ("a".to_owned(), Some(Role::Column)),
                ("b".to_owned(), Some(Role::Column)),
            ]
        );
    }

    /// Statements lifted from the CQL grammar, one per clause position §3's
    /// table names, covering every statement kind this module resolves.
    const CORPUS: &[&str] = &[
        "SELECT JSON DISTINCT id FROM t WHERE token(id) > 1 GROUP BY id ORDER BY id DESC LIMIT 10 ALLOW FILTERING",
        "INSERT INTO t JSON '{\"id\": 1}' IF NOT EXISTS USING TTL 60",
        "UPDATE t USING TTL 60 SET c = c + 1 WHERE id IN (1, 2) IF EXISTS",
        "DELETE FROM t USING TIMESTAMP 1 WHERE id = 1 IF EXISTS",
        "BEGIN UNLOGGED BATCH INSERT INTO t (id) VALUES (1); APPLY BATCH",
        "CREATE KEYSPACE IF NOT EXISTS k WITH replication = {'class': 'SimpleStrategy'}",
        "ALTER KEYSPACE k WITH durable_writes = true",
        "CREATE TABLE IF NOT EXISTS k.t (id uuid PRIMARY KEY) WITH comment = 'x'",
        "ALTER TABLE k.t DROP note",
        "DROP TABLE IF EXISTS k.t",
        "TRUNCATE TABLE k.t",
        "USE k",
        "CREATE TABLE t (id uuid) WITH comment = 'x' AND compaction = {'class': 'X'}",
    ];

    #[test]
    fn no_statement_in_the_grammar_puts_a_role_on_a_keyword_or_a_value() {
        for text in CORPUS {
            let scanned = pieces(text);
            let analysis = resolve(&scanned);

            for (index, piece) in scanned.iter().enumerate() {
                if matches!(piece.kind, Kind::Keyword | Kind::Literal | Kind::Whitespace) {
                    assert_eq!(
                        analysis.role(index),
                        None,
                        "{text:?} gave {:?} a role",
                        piece.text
                    );
                }
            }
        }
    }

    #[test]
    fn every_prefix_of_every_statement_in_the_grammar_resolves() {
        // The buffer is half-typed far more often than it is finished; a
        // resolver that panicked on a prefix would take the whole frame down.
        for text in CORPUS {
            for cut in 0..=text.len() {
                if text.is_char_boundary(cut) {
                    let _ = resolve(&pieces(&text[..cut]));
                }
            }
        }
    }

    /// The context at the `|` in `text`, which is removed before scanning.
    fn context(text: &str) -> Context {
        let offset = text.find('|').expect("a test marks its cursor");
        let text = text.replace('|', "");

        context_at(&pieces(&text), text[..offset].chars().count())
    }

    #[test]
    fn a_cursor_in_a_select_list_expects_a_column_of_the_table_named_after_it() {
        let at = context("SELECT id, na| FROM shop.orders");

        assert_eq!(at.expects, Expect::Column);
        assert_eq!(
            at.tables,
            vec![Name {
                keyspace: Some("shop".to_owned()),
                table: "orders".to_owned()
            }]
        );
    }

    #[test]
    fn a_cursor_in_a_select_list_with_no_from_yet_has_no_tables() {
        let at = context("SELECT co|");

        assert_eq!(at.expects, Expect::Column);
        assert!(at.tables.is_empty());
    }

    #[test]
    fn a_cursor_after_from_expects_a_table() {
        assert_eq!(context("SELECT * FROM |").expects, Expect::Table);
        assert_eq!(context("SELECT * FROM ord|").expects, Expect::Table);
    }

    #[test]
    fn a_cursor_after_use_expects_a_keyspace() {
        assert_eq!(context("USE |").expects, Expect::Keyspace);
    }

    #[test]
    fn a_cursor_in_an_insert_column_list_expects_a_column() {
        let at = context("INSERT INTO shop.orders (i|");

        assert_eq!(at.expects, Expect::Column);
        assert_eq!(at.tables.len(), 1);
    }

    #[test]
    fn a_cursor_after_set_expects_a_column() {
        assert_eq!(context("UPDATE shop.orders SET |").expects, Expect::Column);
    }

    #[test]
    fn a_cursor_at_the_start_of_the_buffer_expects_a_keyword() {
        assert_eq!(context("|").expects, Expect::Keyword);
        assert_eq!(context("sel|").expects, Expect::Keyword);
    }

    #[test]
    fn a_cursor_past_a_value_expects_no_name() {
        assert_eq!(
            context("SELECT * FROM t WHERE id = |").expects,
            Expect::Value
        );
    }

    #[test]
    fn a_cursor_right_after_a_semicolon_is_the_next_statements_fresh_scope() {
        let at = context("SELECT * FROM a;| SELECT * FROM b");

        assert_eq!(at.expects, Expect::Keyword);
    }

    #[test]
    fn a_multi_byte_identifier_is_offset_in_characters_not_bytes() {
        // "café" is 4 characters but 5 bytes; a byte offset landing between
        // the 'é' and the space that follows would either panic slicing the
        // buffer or land on the wrong piece.
        let at = context("SELECT * FROM café|");

        assert_eq!(at.expects, Expect::Table);
    }

    #[test]
    fn every_offset_of_every_statement_in_the_grammar_has_a_context() {
        for text in CORPUS {
            let scanned = pieces(text);
            for offset in 0..=text.chars().count() {
                let _ = context_at(&scanned, offset);
            }
        }
    }
}
