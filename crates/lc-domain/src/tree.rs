//! The Tables panel's tree, flattened once so `update` and `view_model` agree
//! on what row 5 is.

use std::collections::BTreeSet;

use crate::model::{Keyspace, Schema, Table};
use crate::search::{Direction, Search};
use crate::selection::Selection;
use crate::viewport::{Cursor, Scroll};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TablesPanel {
    /// Private so every motion is bounded by [`Self::rows`].
    cursor: Cursor,
    pub scroll: Scroll,
    pub selection: Selection,
    /// Private: only the transitions below write it, so no keyspace outside the
    /// schema is unfolded.
    unfolded: BTreeSet<String>,
    /// Private: setting it owes a cursor clamp.
    search: Option<Search>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node<'a> {
    Keyspace {
        keyspace: &'a Keyspace,
        unfolded: bool,
    },
    Table {
        keyspace: &'a Keyspace,
        table: &'a Table,
    },
}

impl TablesPanel {
    #[must_use]
    pub fn search(&self) -> Option<&Search> {
        self.search.as_ref()
    }

    /// Not public: [`State::apply_search`](crate::state::State::apply_search)
    /// owes the clamp.
    pub(crate) fn set_search(&mut self, search: Option<Search>) {
        self.search = search;
    }

    /// A search keeps keyspaces it reaches, drawn open, holding only the tables
    /// it reaches. The unfold set is never written, so clearing restores the
    /// fold state exactly.
    #[must_use]
    pub fn rows<'a>(&self, schema: &'a Schema) -> Vec<Node<'a>> {
        let mut rows = Vec::new();

        for keyspace in schema.keyspaces_in_display_order() {
            if !self.retains(keyspace) {
                continue;
            }

            let unfolded = self.is_unfolded(keyspace) || self.holds_a_match(keyspace);
            rows.push(Node::Keyspace { keyspace, unfolded });

            if unfolded {
                rows.extend(
                    keyspace
                        .tables
                        .values()
                        .filter(|table| self.reaches(keyspace, table))
                        .map(|table| Node::Table { keyspace, table }),
                );
            }
        }

        rows
    }

    fn retains(&self, keyspace: &Keyspace) -> bool {
        let Some(search) = &self.search else {
            return true;
        };

        search.matches(&keyspace.name)
            || keyspace
                .tables
                .values()
                .any(|table| search.matches(&table.name))
    }

    /// A term matching the keyspace keeps every table in it: `l` and `⏎` are
    /// swallowed under a search, so nothing else could open it.
    fn reaches(&self, keyspace: &Keyspace, table: &Table) -> bool {
        let Some(search) = &self.search else {
            return true;
        };

        search.matches(&table.name) || search.matches(&keyspace.name)
    }

    /// A keyspace with no tables has nothing to open.
    fn holds_a_match(&self, keyspace: &Keyspace) -> bool {
        self.search.is_some() && !keyspace.tables.is_empty() && self.retains(keyspace)
    }

    /// `from` is exclusive. Derived from `rows`, not a stored match list, which
    /// every reload, fold and term change would invalidate.
    #[must_use]
    pub fn next_match(&self, schema: &Schema, from: usize, direction: Direction) -> Option<usize> {
        let search = self.search.as_ref()?;
        let rows = self.rows(schema);
        let len = rows.len();

        (0..len)
            .scan(from, |at, _| {
                *at = direction.step(*at, len);
                Some(*at)
            })
            .find(|at| match rows[*at] {
                Node::Keyspace { keyspace, .. } => search.matches(&keyspace.name),
                Node::Table { table, .. } => search.matches(&table.name),
            })
    }

    #[must_use]
    pub const fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// Only for the shared list motion paths, bounded by `update`'s
    /// `row_count`.
    pub(crate) const fn cursor_mut(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    /// Only for the shared visual-mode paths.
    pub(crate) const fn selection_and_cursor(&mut self) -> (&mut Selection, &mut Cursor) {
        (&mut self.selection, &mut self.cursor)
    }

    pub fn go_to(&mut self, row: usize, schema: &Schema) {
        self.cursor.go_to(row, self.rows(schema).len());
    }

    pub fn clamp(&mut self, schema: &Schema) {
        self.cursor.clamp_to(self.rows(schema).len());
    }

    #[must_use]
    pub fn selected<'a>(&self, schema: &'a Schema) -> Option<Node<'a>> {
        self.rows(schema).into_iter().nth(self.cursor.index())
    }

    /// Swallowed under a search: the keyspace is drawn open regardless, so
    /// folding would only write `unfolded`. Every caller goes through here.
    pub fn toggle(&mut self, schema: &Schema) {
        if self.search.is_some() {
            return;
        }

        let Some(Node::Keyspace { keyspace, unfolded }) = self.selected(schema) else {
            return;
        };

        if unfolded {
            self.fold(&keyspace.name, schema);
        } else {
            self.unfold(keyspace);
        }
    }

    /// Swallowed under a search, as [`Self::toggle`] is.
    pub fn open(&mut self, schema: &Schema) {
        if self.search.is_some() {
            return;
        }

        let Some(Node::Keyspace { keyspace, unfolded }) = self.selected(schema) else {
            return;
        };

        if unfolded {
            self.cursor.down(self.rows(schema).len());
        } else {
            self.unfold(keyspace);
        }
    }

    /// Swallowed under a search, as [`Self::toggle`] is.
    pub fn close(&mut self, schema: &Schema) {
        if self.search.is_some() {
            return;
        }

        match self.selected(schema) {
            Some(Node::Keyspace {
                keyspace,
                unfolded: true,
            }) => self.fold(&keyspace.name, schema),
            Some(Node::Table { keyspace, .. }) => self.select_keyspace(&keyspace.name, schema),
            _ => {}
        }
    }

    /// Never for a keyspace with no tables, which also makes stale names inert.
    fn is_unfolded(&self, keyspace: &Keyspace) -> bool {
        !keyspace.tables.is_empty() && self.unfolded.contains(&keyspace.name)
    }

    fn unfold(&mut self, keyspace: &Keyspace) {
        if !keyspace.tables.is_empty() {
            self.unfolded.insert(keyspace.name.clone());
        }
    }

    fn fold(&mut self, name: &str, schema: &Schema) {
        self.unfolded.remove(name);
        self.clamp(schema);
    }

    fn select_keyspace(&mut self, name: &str, schema: &Schema) {
        let rows = self.rows(schema);
        let found = rows.iter().position(
            |node| matches!(node, Node::Keyspace { keyspace, .. } if keyspace.name == name),
        );

        if let Some(index) = found {
            self.cursor.go_to(index, rows.len());
        }
    }

    /// `Enter` and the DDL panel act on one node.
    #[must_use]
    pub fn selects_many(&self) -> bool {
        self.selection.ranges(self.cursor.index()).rows() > 1
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use crate::model::{
        ClusteringColumn, ClusteringOrder, Column, PrimaryKey, Replication, TableOptions,
    };
    use std::collections::BTreeMap;

    pub(crate) fn table(name: &str) -> Table {
        Table {
            name: name.to_owned(),
            primary_key: PrimaryKey {
                partition: vec![Column {
                    name: "id".to_owned(),
                    cql_type: "uuid".to_owned(),
                }],
                clustering: vec![ClusteringColumn {
                    column: Column {
                        name: "created_at".to_owned(),
                        cql_type: "timestamp".to_owned(),
                    },
                    order: ClusteringOrder::Ascending,
                }],
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
            indexes: Vec::new(),
            options: options(&[
                ("gc_grace_seconds", "864000"),
                ("bloom_filter_fp_chance", "0.01"),
                ("caching", "{'keys': 'ALL'}"),
                ("comment", "fixture"),
            ]),
        }
    }

    pub(crate) fn options(pairs: &[(&str, &str)]) -> TableOptions {
        TableOptions::new(
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        )
    }

    pub(crate) fn schema_of(keyspaces: &[(&str, &[&str])]) -> Schema {
        Schema {
            keyspaces: keyspaces
                .iter()
                .map(|(name, tables)| {
                    (
                        (*name).to_owned(),
                        Keyspace {
                            name: (*name).to_owned(),
                            replication: Replication::SimpleStrategy { factor: 1 },
                            tables: tables
                                .iter()
                                .map(|t| ((*t).to_owned(), table(t)))
                                .collect::<BTreeMap<_, _>>(),
                        },
                    )
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::schema_of;
    use super::*;

    fn labels(panel: &TablesPanel, schema: &Schema) -> Vec<(u8, String)> {
        panel
            .rows(schema)
            .iter()
            .map(|node| match node {
                Node::Keyspace { keyspace, .. } => (0, keyspace.name.clone()),
                Node::Table { table, .. } => (1, table.name.clone()),
            })
            .collect()
    }

    #[test]
    fn a_folded_tree_is_just_its_keyspaces() {
        let schema = schema_of(&[("shop", &["orders", "users"]), ("system", &["local"])]);

        assert_eq!(
            labels(&TablesPanel::default(), &schema),
            [(0, "shop".to_owned()), (0, "system".to_owned())]
        );
    }

    #[test]
    fn an_unfolded_keyspace_lists_its_tables_alphabetically_beneath_it() {
        let schema = schema_of(&[("shop", &["users", "orders"]), ("typezoo", &["t"])]);
        let mut panel = TablesPanel::default();

        panel.open(&schema);

        assert_eq!(
            labels(&panel, &schema),
            [
                (0, "shop".to_owned()),
                (1, "orders".to_owned()),
                (1, "users".to_owned()),
                (0, "typezoo".to_owned()),
            ]
        );
    }

    #[test]
    fn the_clusters_own_keyspaces_stay_last_when_one_is_unfolded() {
        let schema = schema_of(&[("system_schema", &["tables"]), ("shop", &["orders"])]);
        let mut panel = TablesPanel::default();

        panel.open(&schema);

        assert_eq!(
            labels(&panel, &schema),
            [
                (0, "shop".to_owned()),
                (1, "orders".to_owned()),
                (0, "system_schema".to_owned()),
            ],
            "display order is the keyspace list's rule, unchanged by unfolding"
        );
    }

    #[test]
    fn toggling_opens_a_folded_keyspace_and_closes_an_open_one() {
        let schema = schema_of(&[("shop", &["orders"])]);
        let mut panel = TablesPanel::default();

        panel.toggle(&schema);
        assert_eq!(panel.rows(&schema).len(), 2);

        panel.toggle(&schema);
        assert_eq!(panel.rows(&schema).len(), 1);
    }

    #[test]
    fn a_keyspace_with_no_tables_never_unfolds() {
        let schema = schema_of(&[("empty", &[])]);
        let mut panel = TablesPanel::default();

        panel.toggle(&schema);
        panel.open(&schema);

        assert_eq!(
            panel.rows(&schema).len(),
            1,
            "there is nothing to show, so the row must not claim otherwise"
        );
        assert!(matches!(
            panel.selected(&schema),
            Some(Node::Keyspace {
                unfolded: false,
                ..
            })
        ));
    }

    #[test]
    fn opening_an_already_open_keyspace_steps_into_its_first_table() {
        let schema = schema_of(&[("shop", &["orders", "users"])]);
        let mut panel = TablesPanel::default();

        panel.open(&schema);
        panel.open(&schema);

        assert!(matches!(
            panel.selected(&schema),
            Some(Node::Table { table, .. }) if table.name == "orders"
        ));
    }

    #[test]
    fn closing_from_a_table_lands_on_its_keyspace_without_folding_it() {
        let schema = schema_of(&[("shop", &["orders", "users"])]);
        let mut panel = TablesPanel::default();
        panel.open(&schema);
        panel.open(&schema);

        panel.close(&schema);

        assert_eq!(panel.cursor.index(), 0);
        assert_eq!(
            panel.rows(&schema).len(),
            3,
            "the first close is a motion, not a fold"
        );

        panel.close(&schema);

        assert_eq!(panel.rows(&schema).len(), 1, "the second close folds");
    }

    #[test]
    fn closing_from_the_second_keyspace_folds_that_one_and_not_the_first() {
        let schema = schema_of(&[("a", &["t1"]), ("b", &["t2"])]);
        let mut panel = TablesPanel::default();
        panel.open(&schema);
        panel.cursor.go_to(2, panel.rows(&schema).len());
        panel.open(&schema);
        assert_eq!(panel.rows(&schema).len(), 4);

        panel.close(&schema);

        assert_eq!(
            labels(&panel, &schema),
            [
                (0, "a".to_owned()),
                (1, "t1".to_owned()),
                (0, "b".to_owned()),
            ]
        );
    }

    #[test]
    fn closing_an_already_folded_keyspace_does_nothing() {
        let schema = schema_of(&[("shop", &["orders"])]);
        let mut panel = TablesPanel::default();

        panel.close(&schema);

        assert_eq!(panel.rows(&schema).len(), 1);
        assert_eq!(panel.cursor.index(), 0);
    }

    #[test]
    fn folding_pulls_a_stranded_cursor_back_inside_the_tree() {
        let schema = schema_of(&[("shop", &["a", "b", "c"])]);
        let mut panel = TablesPanel::default();
        panel.open(&schema);
        panel.cursor.go_to(3, panel.rows(&schema).len());

        // Reach the keyspace row and fold from there.
        panel.close(&schema);
        panel.close(&schema);

        assert_eq!(panel.cursor.index(), 0);
        assert_eq!(panel.rows(&schema).len(), 1);
    }

    #[test]
    fn selecting_past_the_end_of_the_tree_yields_nothing() {
        let schema = schema_of(&[]);

        assert!(TablesPanel::default().selected(&schema).is_none());
    }

    #[test]
    fn an_unfolded_keyspace_stays_open_across_a_reload() {
        let before = schema_of(&[("shop", &["orders"])]);
        let mut panel = TablesPanel::default();
        panel.open(&before);

        let after = schema_of(&[("shop", &["orders", "users"])]);

        assert_eq!(panel.rows(&after).len(), 3, "shop is still open");
    }

    #[test]
    fn a_keyspace_that_vanished_leaves_no_trace_in_the_tree() {
        let before = schema_of(&[("gone", &["t"]), ("shop", &["orders"])]);
        let mut panel = TablesPanel::default();
        panel.open(&before);

        let after = schema_of(&[("shop", &["orders"])]);

        assert_eq!(
            labels(&panel, &after),
            [(0, "shop".to_owned())],
            "a stale name in the unfold set is inert, not a phantom row"
        );
    }

    #[test]
    fn go_to_cannot_land_outside_the_list() {
        let mut cursor = Cursor::default();

        cursor.go_to(9, 3);

        assert_eq!(cursor.index(), 2);
    }

    fn shop_and_logs() -> Schema {
        schema_of(&[
            ("shop", &["orders", "users"][..]),
            ("logs", &["order_events", "spans"][..]),
        ])
    }

    fn search_labels(panel: &TablesPanel, schema: &Schema) -> Vec<String> {
        panel
            .rows(schema)
            .iter()
            .map(|node| match node {
                Node::Keyspace { keyspace, .. } => keyspace.name.clone(),
                Node::Table { table, .. } => format!("  {}", table.name),
            })
            .collect()
    }

    fn unfolded_on(name: &str, schema: &Schema) -> TablesPanel {
        let mut panel = TablesPanel::default();
        let index = panel
            .rows(schema)
            .iter()
            .position(
                |node| matches!(node, Node::Keyspace { keyspace, .. } if keyspace.name == name),
            )
            .expect("the keyspace is in the schema");
        panel.cursor.go_to(index, panel.rows(schema).len());
        panel.open(schema);
        panel
    }

    #[test]
    fn a_matching_keyspace_is_forced_open_and_a_sibling_it_misses_is_dropped() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();
        panel.set_search(Search::new("shop"));

        assert_eq!(
            search_labels(&panel, &schema),
            ["shop", "  orders", "  users"],
            "logs holds no match at all, so it is not a row; shop matches and opens"
        );
    }

    #[test]
    fn a_keyspace_kept_by_one_matching_table_shows_only_that_table() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();
        panel.set_search(Search::new("users"));

        assert_eq!(
            search_labels(&panel, &schema),
            ["shop", "  users"],
            "a table the term does not reach is not a row"
        );
    }

    #[test]
    fn a_keyspace_the_term_names_keeps_every_table_it_has() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();
        panel.set_search(Search::new("shop"));

        assert_eq!(
            search_labels(&panel, &schema),
            ["shop", "  orders", "  users"],
            "the term reaches every table by its qualified name"
        );
    }

    #[test]
    fn a_search_nothing_reaches_leaves_no_rows() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();
        panel.set_search(Search::new("zzz"));

        assert!(panel.rows(&schema).is_empty());
    }

    #[test]
    fn a_keyspace_holding_a_match_stays_open_whatever_its_fold_state() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();

        assert_eq!(panel.rows(&schema).len(), 2);

        panel.set_search(Search::new("order"));

        assert!(
            matches!(
                panel.rows(&schema).first(),
                Some(Node::Keyspace { unfolded: true, .. })
            ),
            "a search that hid its own matches would be no search at all"
        );
    }

    #[test]
    fn clearing_the_search_restores_the_fold_state_exactly() {
        let schema = shop_and_logs();
        let mut panel = unfolded_on("logs", &schema);
        let before = panel.rows(&schema);

        panel.set_search(Search::new("order"));
        let _ = panel.rows(&schema);
        panel.set_search(None);

        assert_eq!(
            panel.rows(&schema),
            before,
            "searching is a projection; it must not write the unfold set"
        );
    }

    #[test]
    fn a_keyspace_matching_by_name_is_kept_but_open_only_when_it_has_tables() {
        let schema = schema_of(&[("empty", &[][..])]);
        let mut panel = TablesPanel::default();
        panel.set_search(Search::new("empty"));

        assert!(matches!(
            panel.rows(&schema).as_slice(),
            [Node::Keyspace {
                unfolded: false,
                ..
            }]
        ));
    }

    #[test]
    fn clearing_a_search_brings_back_the_keyspaces_it_dropped() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();

        let whole = search_labels(&panel, &schema);
        panel.set_search(Search::new("zzz"));
        panel.set_search(None);

        assert_eq!(search_labels(&panel, &schema), whole);
    }

    #[test]
    fn a_keyspace_holding_a_match_is_drawn_open() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();
        panel.set_search(Search::new("order"));

        let labels = search_labels(&panel, &schema);

        assert!(
            labels.iter().any(|label| label.contains("orders")),
            "a match inside a folded keyspace is not left hidden: {labels:?}"
        );
    }

    fn schema() -> Schema {
        schema_of(&[
            ("shop", &["orders", "orders_by_customer"][..]),
            ("logs", &["order_events"][..]),
        ])
    }

    #[test]
    fn the_next_match_is_the_one_after_the_cursor() {
        let schema = schema();
        let mut panel = TablesPanel::default();
        panel.set_search(Search::new("order"));

        let first = panel
            .next_match(&schema, 0, Direction::Forwards)
            .expect("a match");
        let second = panel
            .next_match(&schema, first, Direction::Forwards)
            .expect("a second match");

        assert!(second > first, "n moves forward: {first} then {second}");
    }

    #[test]
    fn the_next_match_wraps_and_a_lone_match_returns_to_itself() {
        let schema = schema();
        let mut panel = TablesPanel::default();
        panel.set_search(Search::new("orders_by_customer"));

        let only = panel
            .next_match(&schema, 0, Direction::Forwards)
            .expect("a match");

        assert_eq!(
            panel.next_match(&schema, only, Direction::Forwards),
            Some(only),
            "one match is its own next"
        );
    }

    #[test]
    fn no_search_and_no_match_are_both_none() {
        let schema = schema();
        let mut panel = TablesPanel::default();

        assert_eq!(panel.next_match(&schema, 0, Direction::Forwards), None);

        panel.set_search(Search::new("zzz"));
        assert_eq!(panel.next_match(&schema, 0, Direction::Forwards), None);
    }

    #[test]
    fn a_keyspace_holding_no_match_keeps_its_fold_state() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();
        let folded = search_labels(&panel, &schema);

        panel.set_search(Search::new("orders"));
        panel.set_search(None);

        assert_eq!(
            search_labels(&panel, &schema),
            folded,
            "the unfold set is never written by a search"
        );
    }
}
