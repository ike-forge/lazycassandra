//! What each name in a statement means by where it sits. A pass over `lex`'s
//! scan, not a parser: grammar position alone decides a role, so an unknown
//! name is coloured by what the statement does with it.

use super::lex::{Kind, Piece};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Keyspace,
    Table,
    Column,
    /// Stands for a selector, not anything the schema holds.
    Alias,
    Function,
    Type,
    Property,
}

/// Only `Select` is named: its table is the one its rows came from, where
/// others write to theirs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Statement {
    Select,
    #[default]
    Other,
}

/// Folded to lower case where unquoted: `Schema` is indexed by the folded name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub keyspace: Option<String>,
    pub table: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Expect {
    #[default]
    Keyword,
    Keyspace,
    Table,
    Column,
    Value,
    Alias,
    Type,
    Property,
    /// Inside `ALLOW FILTERING`, or past where the machine stopped following.
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Analysis {
    roles: Vec<Option<Role>>,
    /// One longer than `roles`: the last entry is past the final piece, where
    /// an end-of-buffer cursor sits.
    expects: Vec<Expect>,
    /// Indexes `tables` and `scopes`.
    statement: Vec<usize>,
    tables: Vec<Vec<Name>>,
    scopes: Vec<Scope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Scope {
    kind: Statement,
    /// True even when no name followed: an empty `FROM` is unfinished, not
    /// missing.
    table_clause: bool,
}

impl Analysis {
    #[must_use]
    pub fn role(&self, index: usize) -> Option<Role> {
        self.roles.get(index).copied().flatten()
    }

    #[must_use]
    pub fn expects(&self, index: usize) -> Expect {
        self.expects.get(index).copied().unwrap_or_default()
    }

    #[must_use]
    pub fn tables_at(&self, index: usize) -> &[Name] {
        self.statement
            .get(index)
            .and_then(|statement| self.tables.get(*statement))
            .map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn statement_at(&self, index: usize) -> Statement {
        self.scope_at(index)
            .map(|scope| scope.kind)
            .unwrap_or_default()
    }

    #[must_use]
    pub fn table_clause_at(&self, index: usize) -> bool {
        self.scope_at(index).is_some_and(|scope| scope.table_clause)
    }

    fn scope_at(&self, index: usize) -> Option<&Scope> {
        self.statement
            .get(index)
            .and_then(|statement| self.scopes.get(*statement))
    }

    /// A trailing `;` with nothing after it opens no statement.
    #[must_use]
    pub fn statements(&self) -> usize {
        self.tables.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Context {
    pub expects: Expect,
    pub tables: Vec<Name>,
    pub table_clause: bool,
}

/// Inside an identifier, the cursor takes that identifier's expectation; in
/// whitespace, the next name's.
#[must_use]
pub(crate) fn context_at(pieces: &[Piece], offset: usize) -> Context {
    let analysis = resolve(pieces);
    let index = piece_at(pieces, offset);

    Context {
        expects: analysis.expects(index),
        tables: analysis.tables_at(index.saturating_sub(1)).to_vec(),
        table_clause: analysis.table_clause_at(index.saturating_sub(1)),
    }
}

/// A cursor at the end of a word is inside it: `CREATE TABLE t (id in` is a
/// type being typed, not `IN`. Just past a `;` belongs to the next statement.
fn piece_at(pieces: &[Piece], offset: usize) -> usize {
    let mut at = 0;

    for (index, piece) in pieces.iter().enumerate() {
        let end = at + piece.text.chars().count();
        let inside = match piece.kind {
            Kind::Keyword | Kind::Identifier => offset <= end,
            Kind::Punctuation | Kind::Literal | Kind::Whitespace | Kind::Comment => offset < end,
        };

        if inside {
            return index;
        }
        at = end;
    }

    pieces.len()
}

#[must_use]
pub(crate) fn resolve(pieces: &[Piece]) -> Analysis {
    let mut machine = Machine {
        tables: vec![Vec::new()],
        scopes: vec![Scope::default()],
        ..Machine::default()
    };

    for piece in pieces {
        machine.step(piece);
    }
    machine.finish(pieces.len())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Verb {
    #[default]
    Unread,
    /// No clause table: every name is left without a role.
    Uncovered,
    Data,
    Schema,
    /// Apart from [`Verb::Schema`]: `CREATE INDEX … ON t (c)` takes a plain
    /// column after `(`.
    SchemaTable,
}

/// One flat state: every clause that takes a column takes it the same way.
#[derive(Debug, Default)]
struct Machine {
    expect: Expect,
    /// What `,` and the end of an alias restore. Set only by clause keywords,
    /// never by `=` or `AS`.
    base: Expect,
    /// Fixed at the first keyword, so `SELECT` in `GRANT SELECT ON …` cannot
    /// re-decide it.
    verb: Verb,
    /// Inside a column definition's name/type pair.
    alternating: bool,
    roles: Vec<Option<Role>>,
    expects: Vec<Expect>,
    statement: Vec<usize>,
    tables: Vec<Vec<Name>>,
    scopes: Vec<Scope>,
    /// A bare name in table position is a table until a `.` proves it a
    /// keyspace; then its role is rewritten.
    pending_keyspace: Option<(usize, String)>,
    /// Whitespace and comments do not break it. Without it `FROM shop orders`
    /// would read as one qualified name.
    dot_before: bool,
    /// Only an adjacent `(` makes this a `Function`, so `count(id), max(id)`
    /// cannot relabel `id`.
    last_identifier: Option<usize>,
    /// Not `verb != Unread`: a stray identifier or `)` after `;` reaches no
    /// `keyword()`, and would look like the placeholder a trailing `;` opens.
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
        // A batch's wrapper words change nothing; nested statements resolve as
        // standalone.
        if matches!(word, "BEGIN" | "BATCH" | "APPLY" | "UNLOGGED" | "COUNTER") {
            return;
        }

        if self.verb == Verb::Unread {
            if word == "SELECT"
                && let Some(scope) = self.scopes.last_mut()
            {
                scope.kind = Statement::Select;
            }

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

        if word == "TABLE" && self.verb == Verb::Schema {
            self.verb = Verb::SchemaTable;
        }

        self.expect = match word {
            // `GRANT … ON` never reaches here: `GRANT` is uncovered.
            "FROM" | "INTO" | "UPDATE" | "TRUNCATE" | "TABLE" | "ON" => {
                if let Some(scope) = self.scopes.last_mut() {
                    scope.table_clause = true;
                }
                self.enter(Expect::Table)
            }
            "USE" | "KEYSPACE" | "SCHEMA" => self.enter(Expect::Keyspace),
            "SELECT" | "DELETE" | "WHERE" | "SET" | "BY" | "TOKEN" | "PRIMARY" | "RENAME"
            | "DROP" => self.enter(Expect::Column),
            // `AND` continues the open clause: in `WITH` it chains options.
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
            // Before a pending table or keyspace name, `IF [NOT] EXISTS`
            // modifies it and opens no clause.
            "IF" if !matches!(self.expect, Expect::Table | Expect::Keyspace) => {
                self.enter(Expect::Column)
            }
            // Interrupts rather than opens a clause, so `base` is left alone.
            "AS" => Expect::Alias,
            _ => self.expect,
        };
    }

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
                        // `INSERT INTO t (` and `CREATE TABLE t (` open a
                        // column list with no clause keyword.
                        self.expect = self.enter(Expect::Column);
                    }
                    if self.verb == Verb::SchemaTable {
                        self.alternating = true;
                    }
                }
                ')' => self.alternating = false,
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
                // Only an adjacent `.` qualifies: `FROM shop orders` stays two
                // names.
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
                self.expect = self.base;
                Some(Role::Alias)
            }
            Expect::Keyword | Expect::Value | Expect::Nothing => None,
        }
    }

    /// Recorded as a bare table first, so a half-typed `shop.` still names
    /// something.
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
        self.scopes.push(Scope::default());
        self.expect = Expect::Keyword;
        self.base = Expect::Keyword;
        self.verb = Verb::Unread;
        self.alternating = false;
        self.pending_keyspace = None;
        self.last_identifier = None;
        self.has_content = false;
    }

    fn finish(mut self, pieces: usize) -> Analysis {
        debug_assert_eq!(self.roles.len(), pieces);
        self.expects.push(self.expect);

        // Drop the placeholder scope a trailing `;` opened, so `select … from
        // t;` counts as one statement.
        if !self.has_content && self.tables.last().is_some_and(Vec::is_empty) {
            self.tables.pop();
            self.scopes.pop();
        }

        Analysis {
            roles: self.roles,
            expects: self.expects,
            statement: self.statement,
            tables: self.tables,
            scopes: self.scopes,
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
        // `uuid` and `int` lex as keywords and carry no role.
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
            // `text` lexes as a keyword.
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
    fn a_create_index_names_the_table_it_is_built_over_and_the_column_it_covers() {
        assert_eq!(
            roles("CREATE INDEX orders_by_total ON shop.orders (total)"),
            vec![
                ("orders_by_total".to_owned(), None),
                ("shop".to_owned(), Some(Role::Keyspace)),
                ("orders".to_owned(), Some(Role::Table)),
                ("total".to_owned(), Some(Role::Column)),
            ]
        );
    }

    #[test]
    fn a_cursor_at_the_end_of_a_word_is_offered_against_that_words_position() {
        // Without the word's own position, `in` reads as `IN` and expects a
        // value.
        assert_eq!(context("CREATE TABLE t (id in|").expects, Expect::Type);
        assert_eq!(
            context("SELECT * FROM t WHERE id in|").expects,
            Expect::Column,
            "the same half-typed word in a condition list is a column"
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
        "CREATE INDEX orders_by_total ON k.t (total)",
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
        // The buffer is usually half-typed: a prefix must neither panic nor put
        // a role on a keyword.
        for text in CORPUS {
            for cut in 0..=text.len() {
                if !text.is_char_boundary(cut) {
                    continue;
                }

                let scanned = pieces(&text[..cut]);
                let analysis = resolve(&scanned);

                for (index, piece) in scanned.iter().enumerate() {
                    if matches!(piece.kind, Kind::Keyword | Kind::Literal | Kind::Whitespace) {
                        assert_eq!(
                            analysis.role(index),
                            None,
                            "{:?} gave {:?} a role",
                            &text[..cut],
                            piece.text
                        );
                    }
                }
            }
        }
    }

    fn context(text: &str) -> Context {
        let offset = text.find('|').expect("a test marks its cursor");
        let text = text.replace('|', "");

        context_at(&pieces(&text), text[..offset].chars().count())
    }

    #[test]
    fn a_statement_says_whether_a_clause_has_already_named_its_table() {
        assert!(
            context("SELECT na| FROM shop.orders").table_clause,
            "a resolved table is a clause that named one"
        );
        assert!(
            context("SELECT na| FROM ;").table_clause,
            "and so is a clause left empty — the word is what opens it"
        );
        assert!(
            !context("SELECT na|").table_clause,
            "a select list with nothing after it has no clause yet"
        );
        assert!(
            context("UPDATE shop.orders SET to| = 1").table_clause,
            "the verb itself opens one where there is no FROM to read"
        );
        assert!(
            !context("SELECT id FROM shop.orders; SELECT na|").table_clause,
            "and the clause belongs to its own statement, not to the buffer"
        );
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
        // "café" is 4 characters, 5 bytes.
        let at = context("SELECT * FROM café|");

        assert_eq!(at.expects, Expect::Table);
    }

    #[test]
    fn every_offset_of_every_statement_in_the_grammar_has_its_tables_in_scope() {
        for text in CORPUS {
            let scanned = pieces(text);
            // Only a single-statement entry has one table set for every offset.
            // The batched entry is still walked, for the panic.
            let whole = (!text.contains(';'))
                .then(|| resolve(&scanned).tables_at(scanned.len() - 1).to_vec());

            for offset in 0..=text.chars().count() {
                let at = context_at(&scanned, offset);

                if let Some(whole) = &whole {
                    assert_eq!(
                        &at.tables, whole,
                        "{text:?} lost its tables at offset {offset}"
                    );
                }
            }
        }
    }
}
