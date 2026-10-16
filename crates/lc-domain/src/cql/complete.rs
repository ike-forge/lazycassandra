//! What the word under the cursor could become, from CQL's grammar and the
//! loaded schema.

use super::keywords::{COLUMN_WORDS, KEYWORDS, PROPERTIES, TYPES};
use super::lex::{Kind, Piece, pieces};
use super::resolve::{Context, Expect, Name, context_at};
use super::templates::{TEMPLATES, Template};
use crate::editor::Editor;
use crate::model::{Schema, Table};

/// A cap, not a scroll: past this the prefix narrows the list.
const MOST: usize = 12;

/// Offer order comes from [`suggest`]'s chain, not this enum's `Ord`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateKind {
    Keyword,
    Keyspace,
    Table,
    Column,
    Template,
    Property,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Then {
    Nothing,
    /// No CQL statement ends on a keyword, so the space is a keystroke saved.
    Space,
    /// A template's first hole.
    Caret(usize),
    /// The statement names no table, so the column's is written as a `FROM`.
    From(Name),
}

/// `label` and `text` differ for a column offered with no table in scope, shown
/// qualified but typed bare (`SELECT shop.orders.id` is not CQL), and for a
/// template, whose label draws its holes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub text: String,
    pub label: String,
    pub kind: CandidateKind,
    then: Then,
}

impl Candidate {
    #[must_use]
    pub fn plain(text: impl Into<String>, kind: CandidateKind) -> Self {
        let text = text.into();
        Self {
            label: text.clone(),
            text,
            kind,
            then: Then::Nothing,
        }
    }

    fn keyword(text: &'static str) -> Self {
        Self {
            then: Then::Space,
            ..Self::plain(text, CandidateKind::Keyword)
        }
    }

    fn template(template: Template) -> Self {
        Self {
            text: template.text(),
            label: template.label.to_owned(),
            kind: CandidateKind::Template,
            then: Then::Caret(template.caret()),
        }
    }
}

/// Never empty and never just what is typed: only [`suggest`] builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    candidates: Vec<Candidate>,
    selected: usize,
    replaced: usize,
    /// Measured against the text the offers were made against, so a `FROM` is
    /// written before the word is rewritten.
    statement_end: usize,
}

impl Completion {
    #[must_use]
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// Wraps: the list is short.
    pub const fn next(&mut self) {
        self.selected = (self.selected + 1) % self.candidates.len();
    }

    pub const fn previous(&mut self) {
        self.selected = match self.selected {
            0 => self.candidates.len() - 1,
            selected => selected - 1,
        };
    }

    /// Through the editor's own mutators, so its invariants hold.
    pub fn accept(&self, editor: &mut Editor) {
        let candidate = &self.candidates[self.selected];

        // First, while `statement_end` still describes the text it was measured
        // against.
        if let Then::From(table) = &candidate.then {
            editor.insert_at(self.statement_end, &format!(" FROM {}", qualify(table)));
        }

        for _ in 0..self.replaced {
            editor.backspace();
        }
        editor.write(&candidate.text);

        match &candidate.then {
            Then::Caret(caret) => {
                for _ in *caret..candidate.text.chars().count() {
                    editor.left();
                }
            }
            Then::Space if !on_space(editor) => editor.write(" "),
            Then::Nothing | Then::Space | Then::From(_) => {}
        }
    }
}

/// `keyspace` is the one Tables shows; it makes unqualified table names
/// completable.
#[must_use]
pub fn suggest(
    editor: &Editor,
    schema: Option<&Schema>,
    keyspace: Option<&str>,
) -> Option<Completion> {
    let word = word_before(editor);
    let (qualifier, prefix) = split(&word);

    if prefix.is_empty() {
        return None;
    }

    let text = editor.text();
    let scanned = pieces(&text);
    let offset = editor.offset();
    let context = context_at(&scanned, offset);

    let candidates = match (qualifier, context.expects) {
        (Some(qualifier), Expect::Column) => {
            columns_of(schema, &qualified(&context, qualifier), prefix)
        }
        // `WHERE id = shop.` is a value, not a table.
        (Some(qualifier), Expect::Keyword | Expect::Table) => tables_of(schema, qualifier, prefix),
        (Some(_), _) => Vec::new(),
        (None, Expect::Column) => columns(schema, &context, keyspace, prefix),
        (None, Expect::Table) => names_first(schema, keyspace, prefix),
        (None, Expect::Keyspace) => keyspaces(schema, prefix),
        (None, Expect::Type) => types(prefix),
        (None, Expect::Property) => properties(prefix),
        // Keywords only: `LIMIT` and `ALLOW` follow a value, and `GRANT SEL`
        // types a permission.
        (None, Expect::Alias | Expect::Value | Expect::Nothing) => {
            keywords(prefix).take(MOST).collect()
        }
        (None, Expect::Keyword) => {
            let start = at_statement_start(&scanned, offset - word.chars().count());
            unqualified(schema, keyspace, prefix, start)
        }
    };

    // No popup offering only the word just typed, compared case-insensitively.
    if candidates.is_empty()
        || candidates
            .iter()
            .all(|candidate| candidate.text.eq_ignore_ascii_case(prefix))
    {
        return None;
    }

    Some(Completion {
        candidates,
        selected: 0,
        replaced: prefix.chars().count(),
        statement_end: statement_end(&scanned, offset),
    })
}

/// Grammar first: short prefixes mostly reach for keywords, and keywords do not
/// depend on the panel.
fn unqualified(
    schema: Option<&Schema>,
    keyspace: Option<&str>,
    prefix: &str,
    at_start: bool,
) -> Vec<Candidate> {
    let templates = if at_start {
        templates(prefix)
    } else {
        Vec::new()
    };

    let keyspaces = keyspaces(schema, prefix);
    let tables = keyspace
        .into_iter()
        .flat_map(|keyspace| tables_of(schema, keyspace, prefix));

    ahead(keywords(prefix), templates)
        .into_iter()
        .chain(keyspaces)
        .chain(tables)
        .take(MOST)
        .collect()
}

/// Half the popup is reserved: `a` alone matches twelve keywords and would hide
/// `ALTER TABLE <>`.
fn ahead(leading: impl Iterator<Item = Candidate>, trailing: Vec<Candidate>) -> Vec<Candidate> {
    let room = MOST - trailing.len().min(MOST / 2);

    leading.take(room).chain(trailing).take(MOST).collect()
}

/// None once the opening keyword is typed in full.
fn templates(prefix: &str) -> Vec<Candidate> {
    TEMPLATES
        .iter()
        .filter(|template| {
            template.keyword().len() > prefix.len() && matches(template.keyword(), prefix)
        })
        .map(|template| Candidate::template(*template))
        .collect()
}

/// `INSERT` and `INTO` share a prefix with `INT` but are not types.
fn types(prefix: &str) -> Vec<Candidate> {
    TYPES
        .iter()
        .filter(|word| matches(word, prefix))
        .map(|word| Candidate::plain(*word, CandidateKind::Keyword))
        .take(MOST)
        .collect()
}

/// Lower case, as Cassandra spells them.
fn properties(prefix: &str) -> Vec<Candidate> {
    PROPERTIES
        .iter()
        .filter(|word| matches(word, prefix))
        .map(|word| Candidate::plain(*word, CandidateKind::Property))
        .take(MOST)
        .collect()
}

fn tables_of(schema: Option<&Schema>, keyspace: &str, prefix: &str) -> Vec<Candidate> {
    schema
        .and_then(|schema| schema.keyspaces.get(keyspace))
        .into_iter()
        .flat_map(|keyspace| keyspace.tables.keys())
        .filter(|name| matches(name, prefix))
        .map(|name| Candidate::plain(name.clone(), CandidateKind::Table))
        .take(MOST)
        .collect()
}

fn names_first(schema: Option<&Schema>, keyspace: Option<&str>, prefix: &str) -> Vec<Candidate> {
    let tables = keyspace
        .into_iter()
        .flat_map(|keyspace| tables_of(schema, keyspace, prefix));

    tables
        .chain(keyspaces(schema, prefix))
        .chain(keywords(prefix))
        .take(MOST)
        .collect()
}

/// Every position offers them: `SELECT id, fr` is how `FROM` gets typed.
fn keywords(prefix: &str) -> impl Iterator<Item = Candidate> + '_ {
    KEYWORDS
        .iter()
        .filter(move |keyword| matches(keyword, prefix))
        .map(|keyword| Candidate::keyword(keyword))
}

/// `se| * FROM t` must not gain a second space.
fn on_space(editor: &Editor) -> bool {
    let cursor = editor.cursor();

    editor
        .lines()
        .get(cursor.line)
        .and_then(|line| line.chars().nth(cursor.column))
        .is_some_and(|character| character == ' ')
}

fn keyspaces(schema: Option<&Schema>, prefix: &str) -> Vec<Candidate> {
    schema
        .into_iter()
        .flat_map(|schema| schema.keyspaces.keys())
        .filter(|name| matches(name, prefix))
        .map(|name| Candidate::plain(name.clone(), CandidateKind::Keyspace))
        .take(MOST)
        .collect()
}

/// Columns lead wherever the position takes one; the grammar behind them is
/// [`COLUMN_WORDS`], so `SELECT up` no longer offers `UPDATE`.
fn columns(
    schema: Option<&Schema>,
    context: &Context,
    keyspace: Option<&str>,
    prefix: &str,
) -> Vec<Candidate> {
    let columns = match context.tables.as_slice() {
        [table] => columns_of(schema, &resolved(table, keyspace), prefix),
        // A standing but empty `FROM` (`SELECT <> FROM <>`) is not missing;
        // writing one would give two.
        tables => every_column(schema, prefix, tables.is_empty() && !context.table_clause),
    };

    ahead(columns.into_iter(), column_words(prefix).collect())
}

fn column_words(prefix: &str) -> impl Iterator<Item = Candidate> + '_ {
    COLUMN_WORDS
        .iter()
        .filter(move |keyword| matches(keyword, prefix))
        .map(|keyword| Candidate::keyword(keyword))
}

/// Physical layout order, as vision.md §2 reads it.
fn columns_of(schema: Option<&Schema>, table: &Name, prefix: &str) -> Vec<Candidate> {
    let Some(found) = lookup(schema, table) else {
        return Vec::new();
    };

    column_names(found)
        .filter(|name| matches(name, prefix))
        .map(|name| Candidate::plain(name, CandidateKind::Column))
        .take(MOST)
        .collect()
}

/// Labelled by table; accepting types the bare name. `orphaned` also writes the
/// `FROM` that puts it in scope. Display order, not map order: `system*`
/// keyspaces sort early and would fill the cap.
fn every_column(schema: Option<&Schema>, prefix: &str, orphaned: bool) -> Vec<Candidate> {
    schema
        .into_iter()
        .flat_map(Schema::keyspaces_in_display_order)
        .flat_map(|keyspace| keyspace.tables.values().map(move |table| (keyspace, table)))
        .flat_map(|(keyspace, table)| {
            column_names(table).map(move |name| (keyspace.name.clone(), table.name.clone(), name))
        })
        .filter(|(_, _, name)| matches(name, prefix))
        .map(|(keyspace, table, name)| Candidate {
            label: format!("{keyspace}.{table}.{name}"),
            text: name,
            kind: CandidateKind::Column,
            then: if orphaned {
                Then::From(Name {
                    keyspace: Some(keyspace),
                    table,
                })
            } else {
                Then::Nothing
            },
        })
        .take(MOST)
        .collect()
}

fn qualify(table: &Name) -> String {
    match &table.keyspace {
        Some(keyspace) => format!("{keyspace}.{}", table.table),
        None => table.table.clone(),
    }
}

fn column_names(table: &Table) -> impl Iterator<Item = String> + '_ {
    table.columns().map(|column| column.name.clone())
}

/// Fills a missing keyspace from the panel's.
fn resolved(table: &Name, keyspace: Option<&str>) -> Name {
    Name {
        keyspace: table
            .keyspace
            .clone()
            .or_else(|| keyspace.map(str::to_owned)),
        table: table.table.clone(),
    }
}

fn lookup<'schema>(schema: Option<&'schema Schema>, table: &Name) -> Option<&'schema Table> {
    schema?
        .keyspaces
        .get(table.keyspace.as_deref()?)?
        .tables
        .get(&table.table)
}

/// `orders.` after `FROM shop.orders` means that table, not a keyspace.
fn qualified(context: &Context, qualifier: &str) -> Name {
    context
        .tables
        .iter()
        .find(|table| table.table.eq_ignore_ascii_case(qualifier))
        .cloned()
        .unwrap_or_else(|| Name {
            keyspace: None,
            table: qualifier.to_owned(),
        })
}

/// Case-insensitive: CQL folds unquoted names.
fn matches(candidate: &str, prefix: &str) -> bool {
    candidate.len() >= prefix.len()
        && candidate
            .chars()
            .zip(prefix.chars())
            .all(|(candidate, prefix)| candidate.eq_ignore_ascii_case(&prefix))
}

/// A name cannot span lines.
fn word_before(editor: &Editor) -> String {
    let cursor = editor.cursor();
    let line = editor.lines().get(cursor.line).map_or("", String::as_str);

    line.chars()
        .take(cursor.column)
        .collect::<String>()
        .chars()
        .rev()
        .take_while(|character| is_name(*character))
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect()
}

/// Never before `offset` and never in trailing whitespace, so a `FROM` lands
/// against the text.
fn statement_end(scanned: &[Piece], offset: usize) -> usize {
    let mut at = 0;
    let mut end = offset;

    for piece in scanned {
        let after = at + piece.text.chars().count();

        if after > offset {
            match piece.kind {
                Kind::Whitespace | Kind::Comment => {}
                Kind::Punctuation => match piece.text.chars().position(|c| c == ';') {
                    Some(index) => return end.max(at + index),
                    None => end = after,
                },
                Kind::Keyword | Kind::Identifier | Kind::Literal => end = after,
            }
        }

        at = after;
    }

    end
}

/// A template replaces a whole statement, so it is offered only where none has
/// begun.
fn at_statement_start(scanned: &[Piece], word_start: usize) -> bool {
    let mut fresh = true;
    let mut at = 0;

    for piece in scanned {
        if at >= word_start {
            break;
        }

        match piece.kind {
            Kind::Whitespace | Kind::Comment => {}
            // A `;` starts over; anything beside it (`);`) belonged to the one
            // that ended.
            Kind::Punctuation => {
                for character in piece.text.chars() {
                    fresh = character == ';';
                }
            }
            Kind::Keyword | Kind::Identifier | Kind::Literal => fresh = false,
        }

        at += piece.text.chars().count();
    }

    fresh
}

fn is_name(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '.'
}

/// The last dot wins.
fn split(word: &str) -> (Option<&str>, &str) {
    match word.rsplit_once('.') {
        Some((qualifier, prefix)) if !qualifier.is_empty() => (Some(qualifier), prefix),
        _ => (None, word),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClusteringColumn, ClusteringOrder, Column, PrimaryKey};
    use crate::tree::tests_support::schema_of;

    fn typed(text: &str) -> Editor {
        let mut editor = Editor::default();
        editor.replace(text);
        editor
    }

    fn column(name: &str, cql_type: &str) -> Column {
        Column {
            name: name.to_owned(),
            cql_type: cql_type.to_owned(),
        }
    }

    /// Column names avoid sharing first letters with other tables', so a
    /// one-character prefix points at `orders` alone.
    fn schema() -> Schema {
        let mut schema = schema_of(&[
            ("shop", &["orders", "order_items", "customers"]),
            ("secrets", &["keys"]),
        ]);

        let orders = schema
            .keyspaces
            .get_mut("shop")
            .and_then(|keyspace| keyspace.tables.get_mut("orders"))
            .expect("orders is in the fixture");

        orders.primary_key = PrimaryKey {
            partition: vec![column("id", "uuid")],
            clustering: vec![ClusteringColumn {
                column: column("line", "int"),
                order: ClusteringOrder::Ascending,
            }],
        };
        orders.static_columns = vec![column("region", "text")];
        orders.regular_columns = vec![column("total", "decimal")];

        for (keyspace, table, key) in [
            ("shop", "order_items", "sku"),
            ("shop", "customers", "email"),
            ("secrets", "keys", "fingerprint"),
        ] {
            let table = schema
                .keyspaces
                .get_mut(keyspace)
                .and_then(|keyspace| keyspace.tables.get_mut(table))
                .expect("fixture table");

            table.primary_key = PrimaryKey {
                partition: vec![column(key, "text")],
                clustering: Vec::new(),
            };
        }

        // No static or regular column: pins an empty tail.
        let order_items = schema
            .keyspaces
            .get_mut("shop")
            .and_then(|keyspace| keyspace.tables.get_mut("order_items"))
            .expect("order_items is in the fixture");
        order_items.primary_key.clustering = vec![ClusteringColumn {
            column: column("created", "timestamp"),
            order: ClusteringOrder::Ascending,
        }];

        // No clustering column: pins an empty middle.
        let customers = schema
            .keyspaces
            .get_mut("shop")
            .and_then(|keyspace| keyspace.tables.get_mut("customers"))
            .expect("customers is in the fixture");
        customers.static_columns = vec![column("verified", "boolean")];

        schema
    }

    fn offers(text: &str, keyspace: Option<&str>) -> Vec<String> {
        suggest(&typed(text), Some(&schema()), keyspace)
            .map(|completion| {
                completion
                    .candidates()
                    .iter()
                    .map(|candidate| candidate.text.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_name_or_a_keyword_labels_itself() {
        let schema = schema();
        let completion = suggest(&typed("ord"), Some(&schema), Some("shop")).expect("offers");

        for candidate in completion.candidates() {
            assert_eq!(
                candidate.label, candidate.text,
                "a name has nothing to add to itself"
            );
        }
    }

    #[test]
    fn a_plain_candidate_draws_what_it_types() {
        let candidate = Candidate::plain("SELECT", CandidateKind::Keyword);

        assert_eq!(candidate.label, "SELECT");
        assert_eq!(candidate.text, "SELECT");
    }

    #[test]
    fn a_prefix_reaches_the_keyword_it_starts() {
        assert_eq!(
            offers("se", None).first().map(String::as_str),
            Some("SELECT")
        );
    }

    #[test]
    fn the_prefix_is_matched_whatever_case_it_is_typed_in() {
        assert_eq!(offers("SE", None), offers("se", None));
    }

    #[test]
    fn nothing_is_offered_against_no_word_at_all() {
        assert!(suggest(&typed("SELECT "), Some(&schema()), None).is_none());
        assert!(suggest(&typed(""), Some(&schema()), None).is_none());
    }

    #[test]
    fn a_word_with_one_offer_that_is_itself_raises_no_popup() {
        assert!(suggest(&typed("SELECT * FROM secrets.keys"), Some(&schema()), None).is_none());
    }

    #[test]
    fn a_keyspace_name_is_offered_beside_the_keywords() {
        assert!(offers("sh", None).contains(&"shop".to_owned()));
    }

    #[test]
    fn a_qualifier_narrows_the_offers_to_that_keyspaces_tables() {
        assert_eq!(
            offers("SELECT * FROM shop.order", None),
            vec!["order_items".to_owned(), "orders".to_owned()],
            "the tables of shop, and no keyword or keyspace"
        );
    }

    #[test]
    fn a_qualifier_the_schema_has_never_heard_of_offers_nothing() {
        assert!(offers("SELECT * FROM nowhere.o", None).is_empty());
    }

    #[test]
    fn an_unqualified_table_needs_the_keyspace_the_tables_panel_is_on() {
        assert!(!offers("cust", None).contains(&"customers".to_owned()));
        assert!(offers("cust", Some("shop")).contains(&"customers".to_owned()));
    }

    #[test]
    fn accepting_writes_the_candidate_over_the_word_it_was_offered_against() {
        let mut editor = typed("SELECT * FROM shop.order");
        let completion = suggest(&editor, Some(&schema()), None).expect("two tables match");

        completion.accept(&mut editor);

        assert_eq!(editor.text(), "SELECT * FROM shop.order_items");
        assert_eq!(
            editor.cursor().column,
            "SELECT * FROM shop.order_items".chars().count(),
            "the cursor lands after what was written"
        );
    }

    #[test]
    fn accepting_a_keyword_writes_it_in_the_case_cql_is_read_in() {
        let mut editor = typed("se");
        let completion = suggest(&editor, None, None).expect("SELECT matches");

        completion.accept(&mut editor);

        assert_eq!(editor.text(), "SELECT ");
    }

    #[test]
    fn accepting_a_keyword_writes_the_space_the_word_after_it_needs() {
        let mut editor = typed("SELECT * fr");
        let completion = suggest(&editor, Some(&schema()), Some("shop")).expect("FROM matches");

        completion.accept(&mut editor);

        assert_eq!(editor.text(), "SELECT * FROM ");
        assert_eq!(
            editor.cursor().column,
            "SELECT * FROM ".chars().count(),
            "the cursor lands past the space, on the word the clause wants next"
        );
    }

    #[test]
    fn a_keyword_accepted_before_a_space_does_not_write_a_second_one() {
        let mut editor = typed("se * FROM t");
        editor.line_start();
        editor.right();
        editor.right();
        let completion = suggest(&editor, None, None).expect("SELECT matches");

        completion.accept(&mut editor);

        assert_eq!(editor.text(), "SELECT * FROM t");
    }

    #[test]
    fn accepting_a_type_writes_no_space() {
        let mut editor = typed("CREATE TABLE shop.t (id uui");
        let completion = suggest(&editor, Some(&schema()), Some("shop")).expect("uuid matches");

        completion.accept(&mut editor);

        assert_eq!(
            editor.text(),
            "CREATE TABLE shop.t (id UUID",
            "a comma or a bracket comes after a type, not another word"
        );
    }

    #[test]
    fn accepting_leaves_the_rest_of_the_line_alone() {
        let mut editor = typed("se * FROM t");
        editor.line_start();
        editor.right();
        editor.right();
        let completion = suggest(&editor, None, None).expect("SELECT matches");

        completion.accept(&mut editor);

        assert_eq!(editor.text(), "SELECT * FROM t");
    }

    #[test]
    fn the_selection_is_a_ring() {
        let mut completion = suggest(&typed("se"), None, None).expect("some keyword matches");
        let last = completion.candidates().len() - 1;

        completion.previous();
        assert_eq!(
            completion.selected(),
            last,
            "back from the first is the last"
        );

        completion.next();
        assert_eq!(completion.selected(), 0);
    }

    #[test]
    fn no_more_than_a_popups_worth_is_offered() {
        // The shortest prefix keywords, keyspaces and tables all share.
        let most = offers("s", Some("shop")).len();

        assert!(most <= MOST, "{most} candidates would not fit a popup");
    }

    #[test]
    fn a_select_list_over_one_table_offers_that_tables_columns_first() {
        let schema = schema();

        // Places the cursor after `i`.
        let mut editor = typed("SELECT i FROM shop.orders");
        for _ in 0.." FROM shop.orders".len() {
            editor.left();
        }
        let completion = suggest(&editor, Some(&schema), Some("shop")).expect("offers");

        assert_eq!(
            completion.candidates().first().map(|c| c.text.as_str()),
            Some("id"),
            "the fixture's only column starting with `i`"
        );
        assert_eq!(
            completion.candidates().first().map(|c| c.kind),
            Some(CandidateKind::Column),
            "a select list over a known table leads with its columns"
        );
    }

    #[test]
    fn a_column_position_still_reaches_the_keyword_that_ends_it() {
        assert!(
            offers("SELECT id, fr", Some("shop")).contains(&"FROM".to_owned()),
            "`fr` in a select list is how FROM gets typed"
        );
        assert!(
            offers("SELECT * fr", Some("shop")).contains(&"FROM".to_owned()),
            "`*` never leaves the column position, and FROM must survive it"
        );

        let after_where = offers("SELECT id FROM shop.orders WHERE li", Some("shop"));
        assert!(
            after_where.contains(&"line".to_owned()),
            "the table's own column leads"
        );
        assert!(
            after_where.contains(&"LIMIT".to_owned()),
            "and the keyword that ends the clause is still reachable"
        );
    }

    #[test]
    fn a_tables_columns_are_offered_in_physical_order() {
        // An empty prefix shows all four column kinds in sequence.
        let schema = schema();
        let orders = Name {
            keyspace: Some("shop".to_owned()),
            table: "orders".to_owned(),
        };

        let names: Vec<String> = columns_of(Some(&schema), &orders, "")
            .into_iter()
            .map(|candidate| candidate.text)
            .collect();

        assert_eq!(
            names,
            ["id", "line", "region", "total"],
            "partition, then clustering, then static, then regular"
        );
    }

    #[test]
    fn an_empty_segment_of_the_physical_order_contributes_no_columns() {
        let schema = schema();

        let order_items = Name {
            keyspace: Some("shop".to_owned()),
            table: "order_items".to_owned(),
        };
        let names: Vec<String> = columns_of(Some(&schema), &order_items, "")
            .into_iter()
            .map(|candidate| candidate.text)
            .collect();
        assert_eq!(
            names,
            ["sku", "created"],
            "a clustering column with no static or regular one to chain in"
        );

        let customers = Name {
            keyspace: Some("shop".to_owned()),
            table: "customers".to_owned(),
        };
        let names: Vec<String> = columns_of(Some(&schema), &customers, "")
            .into_iter()
            .map(|candidate| candidate.text)
            .collect();
        assert_eq!(
            names,
            ["email", "verified"],
            "a static column with an empty clustering list ahead of it"
        );

        let keys = Name {
            keyspace: Some("secrets".to_owned()),
            table: "keys".to_owned(),
        };
        let names: Vec<String> = columns_of(Some(&schema), &keys, "")
            .into_iter()
            .map(|candidate| candidate.text)
            .collect();
        assert_eq!(
            names,
            ["fingerprint"],
            "clustering, static and regular all empty"
        );
    }

    #[test]
    fn a_system_keyspaces_columns_never_crowd_out_the_users_own() {
        // `system_auth` sorts first and could fill every slot.
        let mut names = Vec::new();
        for index in 0..=MOST {
            names.push(format!("t{index}"));
        }
        let system: Vec<&str> = names.iter().map(String::as_str).collect();
        let schema = schema_of(&[("system_auth", &system), ("zoo", &["visits"])]);

        let labels: Vec<String> = every_column(Some(&schema), "id", true)
            .into_iter()
            .map(|candidate| candidate.label)
            .collect();

        assert_eq!(
            labels.first().map(String::as_str),
            Some("zoo.visits.id"),
            "a user's keyspace is offered before the cluster's own"
        );
    }

    #[test]
    fn a_select_list_with_no_table_offers_every_column_qualified() {
        let schema = schema();
        let mut editor = typed("SELECT i");
        let mut completion = suggest(&editor, Some(&schema), Some("shop")).expect("offers");
        let at = completion
            .candidates()
            .iter()
            .position(|candidate| candidate.kind == CandidateKind::Column)
            .expect("columns are offered with no table in scope");
        let column = &completion.candidates()[at];

        assert_eq!(column.label, "shop.orders.id", "the label says which table");
        assert_eq!(column.text, "id", "accepting types CQL, not the label");

        for _ in 0..at {
            completion.next();
        }
        completion.accept(&mut editor);
        assert_eq!(editor.text(), "SELECT id FROM shop.orders");
    }

    #[test]
    fn more_than_one_table_in_scope_offers_every_column_qualified_too() {
        let schema = schema();
        let text = "SELECT t FROM shop.orders, shop.order_items";
        let suffix = " FROM shop.orders, shop.order_items";
        let mut editor = typed(text);
        for _ in 0..suffix.len() {
            editor.left();
        }
        let completion = suggest(&editor, Some(&schema), Some("shop")).expect("offers");

        let column = completion
            .candidates()
            .iter()
            .find(|candidate| candidate.kind == CandidateKind::Column)
            .expect("columns are offered with more than one table in scope");

        assert_eq!(
            column.label, "shop.orders.total",
            "more than one table in scope is treated as none"
        );
    }

    #[test]
    fn a_select_list_leads_with_the_columns_even_where_it_names_no_table() {
        let schema = schema();
        let completion = suggest(&typed("SELECT to"), Some(&schema), Some("shop")).expect("offers");

        assert_eq!(
            completion.candidates().first().map(|c| c.kind),
            Some(CandidateKind::Column),
            "a select list is a list of columns before it is anything else"
        );
        assert!(
            completion
                .candidates()
                .iter()
                .any(|candidate| candidate.text == "TO"),
            "and the grammar is still reachable behind them"
        );
    }

    #[test]
    fn a_column_position_offers_no_keyword_it_could_not_take() {
        let offered = offers("SELECT de", Some("shop"));

        for impossible in ["DECIMAL", "DELETE", "DESCRIBE"] {
            assert!(
                !offered.contains(&impossible.to_owned()),
                "{impossible} cannot appear in a select list"
            );
        }
        assert!(
            offered.contains(&"DESC".to_owned()),
            "the word an ORDER BY takes is one the position does take"
        );
    }

    #[test]
    fn a_value_position_still_offers_the_whole_grammar() {
        assert!(
            offers("SELECT * FROM shop.orders WHERE id = de", Some("shop"))
                .contains(&"DELETE".to_owned()),
            "only a column list is narrowed; a value takes the words the \
             statement machine stopped following"
        );
    }

    #[test]
    fn accepting_a_column_with_no_table_in_scope_writes_the_clause_that_puts_it_in_scope() {
        let schema = schema();
        let mut editor = typed("SELECT to");
        let mut completion = suggest(&editor, Some(&schema), Some("shop")).expect("offers");
        let at = completion
            .candidates()
            .iter()
            .position(|candidate| candidate.text == "total")
            .expect("total is offered");

        for _ in 0..at {
            completion.next();
        }
        completion.accept(&mut editor);

        assert_eq!(editor.text(), "SELECT total FROM shop.orders");
        assert_eq!(
            editor.cursor().column,
            "SELECT total".chars().count(),
            "the cursor stays on the select list, not on the clause it wrote"
        );
    }

    #[test]
    fn a_column_accepted_where_a_from_already_stands_writes_no_second_one() {
        let schema = schema();
        let tail = " FROM ;";
        let mut editor = typed(&format!("SELECT to{tail}"));
        for _ in 0..tail.chars().count() {
            editor.left();
        }
        let mut completion = suggest(&editor, Some(&schema), Some("shop")).expect("offers");
        let at = completion
            .candidates()
            .iter()
            .position(|candidate| candidate.text == "total")
            .expect("total is offered");

        for _ in 0..at {
            completion.next();
        }
        completion.accept(&mut editor);

        assert_eq!(editor.text(), "SELECT total FROM ;");
    }

    #[test]
    fn the_clause_lands_at_the_end_of_the_statement_the_cursor_is_in() {
        let schema = schema();
        let tail = "; TRUNCATE shop.customers";
        let mut editor = typed(&format!("SELECT to{tail}"));
        for _ in 0..tail.chars().count() {
            editor.left();
        }
        let mut completion = suggest(&editor, Some(&schema), Some("shop")).expect("offers");
        let at = completion
            .candidates()
            .iter()
            .position(|candidate| candidate.text == "total")
            .expect("total is offered");

        for _ in 0..at {
            completion.next();
        }
        completion.accept(&mut editor);

        assert_eq!(
            editor.text(),
            "SELECT total FROM shop.orders; TRUNCATE shop.customers"
        );
    }

    #[test]
    fn more_than_one_table_in_scope_writes_no_clause_because_none_is_missing() {
        let schema = schema();
        let text = "SELECT to FROM shop.orders, shop.order_items";
        let suffix = " FROM shop.orders, shop.order_items";
        let mut editor = typed(text);
        for _ in 0..suffix.chars().count() {
            editor.left();
        }
        let mut completion = suggest(&editor, Some(&schema), Some("shop")).expect("offers");
        let at = completion
            .candidates()
            .iter()
            .position(|candidate| candidate.text == "total")
            .expect("total is offered");

        for _ in 0..at {
            completion.next();
        }
        completion.accept(&mut editor);

        assert_eq!(editor.text(), text.replace("SELECT to", "SELECT total"));
    }

    fn labels(text: &str, keyspace: Option<&str>) -> Vec<String> {
        suggest(&typed(text), Some(&schema()), keyspace)
            .map(|completion| {
                completion
                    .candidates()
                    .iter()
                    .map(|candidate| candidate.label.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_statement_start_offers_the_shapes_its_prefix_opens() {
        assert!(labels("sel", Some("shop")).contains(&"SELECT <> FROM <>".to_owned()));
        assert!(labels("upd", Some("shop")).contains(&"UPDATE <> SET <> WHERE <>".to_owned()));
    }

    #[test]
    fn keywords_never_crowd_the_shapes_out_of_the_popup() {
        assert!(labels("a", Some("shop")).contains(&"ALTER TABLE <> ADD <> <>".to_owned()));
    }

    #[test]
    fn a_shape_is_offered_only_where_there_is_no_statement_yet() {
        assert!(
            !labels("SELECT sel", Some("shop"))
                .iter()
                .any(|label| label.contains("<>")),
            "a shape replaces a statement, and this one has begun"
        );
        assert!(
            !labels("SELECT * FROM shop.orders; sel", Some("shop")).is_empty(),
            "the statement after a `;` is a fresh one"
        );
        assert!(
            labels("SELECT * FROM shop.orders; sel", Some("shop"))
                .contains(&"SELECT <> FROM <>".to_owned())
        );
    }

    #[test]
    fn a_prefix_spelling_the_whole_opening_keyword_offers_no_shape() {
        assert!(
            !labels("select", None)
                .iter()
                .any(|label| label.contains("<>")),
            "the word is typed; a popup over a finished word is the papercut"
        );
    }

    #[test]
    fn accepting_a_shape_types_it_whole_and_lands_on_its_first_hole() {
        let mut editor = typed("sel");
        let mut completion = suggest(&editor, None, None).expect("offers");
        let at = completion
            .candidates()
            .iter()
            .position(|candidate| candidate.kind == CandidateKind::Template)
            .expect("the SELECT shape is offered");

        for _ in 0..at {
            completion.next();
        }
        completion.accept(&mut editor);

        assert_eq!(editor.text(), "SELECT  FROM ;");
        assert_eq!(
            editor.cursor().column,
            "SELECT ".chars().count(),
            "the cursor waits where the select list goes"
        );
    }

    #[test]
    fn a_batch_shape_is_written_across_the_lines_it_needs() {
        let mut editor = typed("beg");
        let mut completion = suggest(&editor, None, None).expect("offers");
        let at = completion
            .candidates()
            .iter()
            .position(|candidate| candidate.kind == CandidateKind::Template)
            .expect("the BEGIN BATCH shape is offered");

        for _ in 0..at {
            completion.next();
        }
        completion.accept(&mut editor);

        assert_eq!(editor.lines(), ["BEGIN BATCH", "", "APPLY BATCH;"]);
        assert_eq!(
            editor.cursor(),
            crate::editor::Position { line: 1, column: 0 }
        );
    }

    #[test]
    fn a_value_position_offers_grammar_and_no_names() {
        let offered = offers("SELECT * FROM shop.orders WHERE id = sh", Some("shop"));

        assert!(
            !offered.contains(&"shop".to_owned()),
            "a keyspace is not a value"
        );
    }

    #[test]
    fn a_column_definition_offers_types_and_not_the_keywords_that_share_their_prefix() {
        let offered = offers("CREATE TABLE shop.t (id in", Some("shop"));

        assert!(offered.contains(&"INT".to_owned()));
        assert!(offered.contains(&"INET".to_owned()));
        assert!(
            !offered.contains(&"INSERT".to_owned()) && !offered.contains(&"INTO".to_owned()),
            "neither is a type, and this position takes nothing else"
        );
    }

    #[test]
    fn a_with_clause_offers_the_options_it_takes() {
        let offered = offers("CREATE TABLE shop.t (id uuid) WITH comp", Some("shop"));

        assert_eq!(offered, ["compaction".to_owned(), "compression".to_owned()]);
    }

    #[test]
    fn a_cursor_after_from_offers_names_before_keywords() {
        let schema = schema();
        let completion =
            suggest(&typed("SELECT * FROM ord"), Some(&schema), Some("shop")).expect("offers");

        assert_eq!(
            completion.candidates().first().map(|c| c.kind),
            Some(CandidateKind::Table),
            "a FROM clause wants a table, and a keyword starting with `ord` is noise there"
        );
    }

    #[test]
    fn a_cursor_after_use_offers_keyspaces_only() {
        let schema = schema();
        // Only the `Expect::Keyspace` branch leaves keyspaces alone.
        let completion = suggest(&typed("USE se"), Some(&schema), Some("shop")).expect("offers");

        assert!(
            completion
                .candidates()
                .iter()
                .all(|candidate| candidate.kind == CandidateKind::Keyspace),
            "USE takes a keyspace and nothing else"
        );
        assert!(
            completion
                .candidates()
                .iter()
                .any(|candidate| candidate.text == "secrets")
        );
    }

    #[test]
    fn a_statement_the_resolver_does_not_follow_still_offers_what_it_always_did() {
        let schema = schema();
        let completion = suggest(&typed("GRANT sel"), Some(&schema), Some("shop")).expect("offers");

        assert_eq!(
            completion.candidates().first().map(|c| c.kind),
            Some(CandidateKind::Keyword)
        );
    }

    #[test]
    fn a_qualified_word_in_column_position_offers_that_tables_columns() {
        let schema = schema();
        let text = "SELECT orders.t FROM shop.orders";
        let suffix = " FROM shop.orders";
        let mut editor = typed(text);
        for _ in 0..suffix.len() {
            editor.left();
        }
        let completion = suggest(&editor, Some(&schema), None).expect("offers");

        assert_eq!(
            completion.candidates().first().map(|c| c.text.as_str()),
            Some("total"),
            "`orders.` in a column position offers that table's own columns"
        );
    }
}
