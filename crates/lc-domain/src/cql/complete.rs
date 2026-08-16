//! What the word under the cursor could become.
//!
//! Schema-aware completion is what vision.md §5.1 promises the panel for, and
//! it is the one place the editor reads the cluster's own vocabulary: the
//! keyspaces and tables `Schema` already holds, alongside CQL's fixed grammar
//! (design §5).

use super::keywords::KEYWORDS;
use super::lex::pieces;
use super::resolve::{Context, Expect, Name, context_at};
use crate::editor::Editor;
use crate::model::{Schema, Table};

/// The most candidates ever offered at once.
///
/// A cap and not a scroll: past this many, the prefix is what narrows the
/// list, and a popup taller than the panel it hangs off says less than a
/// shorter one (design §5.3).
const MOST: usize = 12;

/// What kind of thing a candidate is — what the popup colours it by.
///
/// The order the list is offered in comes from the chain [`suggest`] builds
/// for the cursor's position, not from this enum's derived `Ord`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateKind {
    Keyword,
    Keyspace,
    Table,
    Column,
}

/// One offer.
///
/// `label` is what the popup draws and `text` is what accepting types. They
/// differ for a column offered with no table in scope, which is shown
/// qualified so two `id` columns are told apart but typed bare, because
/// `SELECT shop.orders.id` is not CQL (design §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub text: String,
    pub label: String,
    pub kind: CandidateKind,
}

impl Candidate {
    /// A candidate that draws exactly what it types.
    #[must_use]
    pub fn plain(text: impl Into<String>, kind: CandidateKind) -> Self {
        let text = text.into();
        Self {
            label: text.clone(),
            text,
            kind,
        }
    }
}

/// The offers standing against the word before the cursor.
///
/// Never empty and never a single candidate identical to what is already
/// typed: both are constructed only through [`suggest`], which returns `None`
/// rather than build one — so a popup on screen always has something to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    candidates: Vec<Candidate>,
    selected: usize,
    /// Characters before the cursor that accepting replaces.
    replaced: usize,
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

    /// Steps to the next candidate, wrapping — a list this short is a ring,
    /// and stopping at the end would cost a keystroke to get back to the top.
    pub const fn next(&mut self) {
        self.selected = (self.selected + 1) % self.candidates.len();
    }

    pub const fn previous(&mut self) {
        self.selected = match self.selected {
            0 => self.candidates.len() - 1,
            selected => selected - 1,
        };
    }

    /// Writes the selected candidate over the word it was offered against.
    ///
    /// Goes through the editor's own mutators rather than reaching into its
    /// lines, so the invariants `Editor` holds are held here too — there is no
    /// second way to change the buffer (design §5.4).
    pub fn accept(&self, editor: &mut Editor) {
        for _ in 0..self.replaced {
            editor.backspace();
        }

        for character in self.candidates[self.selected].text.chars() {
            editor.insert(character);
        }
    }
}

/// What the word before the cursor could become, or `None` where nothing is
/// worth offering.
///
/// `keyspace` is the keyspace the Tables panel is showing, which is what makes
/// an unqualified table name completable at all: `orders` means nothing
/// without one, and the panel beside the editor is already saying which one
/// the user is looking at (design §5.2).
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
    let context = context_at(&pieces(&text), offset_of(editor));

    let candidates = match (qualifier, context.expects) {
        (Some(qualifier), Expect::Column) => {
            columns_of(schema, &qualified(&context, qualifier), prefix)
        }
        (Some(qualifier), _) => tables_of(schema, qualifier, prefix),
        (None, Expect::Column) => columns(schema, &context, keyspace, prefix),
        (None, Expect::Table) => names_first(schema, keyspace, prefix),
        (None, Expect::Keyspace) => keyspaces(schema, prefix),
        (None, _) => unqualified(schema, keyspace, prefix),
    };

    // A list whose only offers are the word already typed adds nothing, and a
    // popup that appears the moment a name is finished is the papercut this
    // check exists to prevent. Case-insensitively, for the reason `matches`
    // is: a user who typed `select` in lower case did not ask to be offered
    // `SELECT` (design §5.1).
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
    })
}

/// Keywords first, then the keyspaces, then the tables of the one on screen.
///
/// Grammar first because it is what the shortest prefix is most often
/// reaching for — `se` is `SELECT` long before it is a table called `sessions`
/// — and because a keyword is the same wherever the cursor is, while a name
/// depends on a panel the user may not be looking at (design §5.2).
fn unqualified(schema: Option<&Schema>, keyspace: Option<&str>, prefix: &str) -> Vec<Candidate> {
    let keywords = keywords(prefix);

    let keyspaces = schema.into_iter().flat_map(|schema| {
        schema
            .keyspaces
            .keys()
            .filter(|name| matches(name, prefix))
            .map(|name| Candidate::plain(name.clone(), CandidateKind::Keyspace))
    });

    let tables = keyspace
        .into_iter()
        .flat_map(|keyspace| tables_of(schema, keyspace, prefix));

    keywords.chain(keyspaces).chain(tables).take(MOST).collect()
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

/// Tables of the keyspace in scope, then keyspaces, then keywords: what
/// `Expect::Table` wants (design §6) — a name before the grammar that happens
/// to share its prefix.
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

/// The grammar's own words, which every position offers: a clause keyword is
/// reachable from inside the clause before it, and `SELECT id, fr` is how
/// `FROM` gets typed.
fn keywords(prefix: &str) -> impl Iterator<Item = Candidate> + '_ {
    KEYWORDS
        .iter()
        .filter(move |keyword| matches(keyword, prefix))
        .map(|keyword| Candidate::plain(*keyword, CandidateKind::Keyword))
}

/// Keyspaces alone: what `Expect::Keyspace` wants (design §6) — `USE` takes
/// nothing else.
fn keyspaces(schema: Option<&Schema>, prefix: &str) -> Vec<Candidate> {
    schema
        .into_iter()
        .flat_map(|schema| schema.keyspaces.keys())
        .filter(|name| matches(name, prefix))
        .map(|name| Candidate::plain(name.clone(), CandidateKind::Keyspace))
        .take(MOST)
        .collect()
}

/// The columns a select list can name, offered from the one table in scope
/// or, where the statement names none or several, from every table there is
/// (design §6) — and then the keywords, which no column position may hide.
///
/// A select list and a `WHERE` list both stay `Expect::Column` until a
/// keyword ends them, so keywords offered only elsewhere would make `FROM`
/// and `LIMIT` untypeable through the popup.
fn columns(
    schema: Option<&Schema>,
    context: &Context,
    keyspace: Option<&str>,
    prefix: &str,
) -> Vec<Candidate> {
    let columns = match context.tables.as_slice() {
        [table] => columns_of(schema, &resolved(table, keyspace), prefix),
        _ => every_column(schema, prefix),
    };

    columns
        .into_iter()
        .chain(keywords(prefix))
        .take(MOST)
        .collect()
}

/// A table's columns in the order vision.md §2 reads them: partition,
/// clustering, static, regular — the physical layout, not the alphabet.
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

/// Every column of every table, labelled by where it lives.
///
/// Offered where the statement names no table yet: the qualifier tells two
/// `id` columns apart, and accepting still types the bare name, because a
/// dotted column is not CQL (design §6).
///
/// In [`Schema::keyspaces_in_display_order`], not the map's own order: the
/// cluster's `system*` keyspaces sort early and hold enough tables between
/// them to fill the `MOST` cap on their own, which would leave a user with
/// none of their own columns offered — the same precedence the tree already
/// draws them in (view/tables.rs).
fn every_column(schema: Option<&Schema>, prefix: &str) -> Vec<Candidate> {
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
        })
        .take(MOST)
        .collect()
}

fn column_names(table: &Table) -> impl Iterator<Item = String> + '_ {
    table.columns().map(|column| column.name.clone())
}

/// `table` with a missing keyspace filled from the panel's visible one — what
/// makes a bare `FROM orders` completable at all (design §5.2).
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

/// The qualifier before a column prefix, resolved against the tables the
/// statement already names: `orders.` after `FROM shop.orders` means
/// `shop.orders`, not a keyspace called `orders` (design §6).
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

/// Case-insensitively, because CQL folds an unquoted name and because a user
/// typing `se` for `SELECT` is not saying anything about case (design §5.1).
fn matches(candidate: &str, prefix: &str) -> bool {
    candidate.len() >= prefix.len()
        && candidate
            .chars()
            .zip(prefix.chars())
            .all(|(candidate, prefix)| candidate.eq_ignore_ascii_case(&prefix))
}

/// The run of name characters ending at the cursor, `.` included.
///
/// Read off the cursor's own line: a name does not span a line break, so
/// nothing before this line can be part of the word being typed.
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

/// The cursor as a character offset into the buffer's text, which is what
/// `context_at` is indexed by.
///
/// Characters, not bytes: a multi-byte character before the cursor must not
/// shift where the resolver thinks the cursor sits.
fn offset_of(editor: &Editor) -> usize {
    let cursor = editor.cursor();
    let before: usize = editor
        .lines()
        .iter()
        .take(cursor.line)
        // The newline each line is followed by is a character of the text.
        .map(|line| line.chars().count() + 1)
        .sum();

    before + cursor.column
}

fn is_name(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '.'
}

/// `shop.ord` as the keyspace it names and the table prefix under the cursor;
/// `ord` as no keyspace and the same prefix.
///
/// The last dot wins, so a stray one earlier in the run cannot make the
/// qualifier something the schema has never heard of.
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

    /// `shop.orders` given real physical layout — a partition key, a
    /// clustering column, a static column and a regular column, each named
    /// so no other table's fixture columns share its first letter, which is
    /// what keeps a one-character prefix pointing at `orders` alone in the
    /// tests below. Every other table keeps a single-column partition key of
    /// its own so the fixture stays a real schema, not a name list.
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

        // A clustering column but no static or regular one, so the chain in
        // `columns_of` is pinned against an empty *tail* and not only an
        // empty middle.
        let order_items = schema
            .keyspaces
            .get_mut("shop")
            .and_then(|keyspace| keyspace.tables.get_mut("order_items"))
            .expect("order_items is in the fixture");
        order_items.primary_key.clustering = vec![ClusteringColumn {
            column: column("created", "timestamp"),
            order: ClusteringOrder::Ascending,
        }];

        // A static column but no clustering one, so the chain is pinned
        // against an empty *middle* too.
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
        // `keys` is the only name in `secrets` and it is fully typed, so the
        // list would say only what the buffer already says.
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

        assert_eq!(editor.text(), "SELECT");
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
        // Every keyword, keyspace and table at once: the shortest prefix any
        // of them share.
        let most = offers("s", Some("shop")).len();

        assert!(most <= MOST, "{most} candidates would not fit a popup");
    }

    #[test]
    fn a_select_list_over_one_table_offers_that_tables_columns_first() {
        let schema = schema();

        // The cursor is at the end of the buffer in `typed`; place it after `i`.
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
        // `columns_of` directly, not `suggest`: an empty prefix matches
        // every column, which is what it takes to observe all four in
        // sequence rather than the single one a real prefix would leave.
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
        // `system_auth` sorts before `zoo` and holds more tables than the
        // popup has room for: in the map's own order it would take every
        // slot and the user would see none of their own columns.
        let mut names = Vec::new();
        for index in 0..=MOST {
            names.push(format!("t{index}"));
        }
        let system: Vec<&str> = names.iter().map(String::as_str).collect();
        let schema = schema_of(&[("system_auth", &system), ("zoo", &["visits"])]);

        let labels: Vec<String> = every_column(Some(&schema), "id")
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
        let completion = suggest(&editor, Some(&schema), Some("shop")).expect("offers");
        let column = completion
            .candidates()
            .iter()
            .find(|candidate| candidate.kind == CandidateKind::Column)
            .expect("columns are offered with no table in scope");

        assert_eq!(column.label, "shop.orders.id", "the label says which table");
        assert_eq!(column.text, "id", "accepting types CQL, not the label");

        completion.accept(&mut editor);
        assert_eq!(editor.text(), "SELECT id");
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
        // `se` is `SELECT` and `SET` as well as `secrets`, and the panel is
        // on a keyspace with tables of its own: only the `Expect::Keyspace`
        // branch can leave keyspaces alone in the list.
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
