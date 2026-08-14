//! The Tables panel's tree: keyspaces at the root, tables under an unfolded
//! keyspace.
//!
//! One flattening, used by both `update`'s motions and `view_model`, so the
//! two cannot disagree about what row 5 is.

use std::collections::BTreeSet;

use crate::model::{Keyspace, Schema, Table};
use crate::search::Filter;
use crate::state::{Cursor, Scroll, Selection};

/// The Tables panel's own state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TablesPanel {
    pub cursor: Cursor,
    /// The first row drawn. Moved by a wheel on its own, and pulled after a
    /// cursor motion (docs/superpowers/specs/2026-08-09-list-viewport-design.md §1).
    pub scroll: Scroll,
    /// The rows `v` has selected, if any
    /// (docs/superpowers/specs/2026-08-14-visual-mode-design.md §3).
    pub selection: Selection,
    /// Keyspaces showing their tables.
    ///
    /// Private for the reason [`Cursor`]'s index is: the only ways in are the
    /// transitions below, so nothing can unfold a keyspace the schema lacks.
    /// Ordered rather than hashed per conventions.md §4.
    unfolded: BTreeSet<String>,
    /// Narrows [`TablesPanel::rows`]. Private for the reason `unfolded` is:
    /// setting it is going to owe a cursor clamp, and a public field would let
    /// a caller skip it (design §2.1, §2.2).
    filter: Option<Filter>,
}

/// One row of the flattened tree.
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
    /// The filter narrowing [`TablesPanel::rows`], if any.
    #[must_use]
    pub fn filter(&self) -> Option<&Filter> {
        self.filter.as_ref()
    }

    /// Sets the filter. Not public: [`State::apply_filter`](crate::state::State::apply_filter)
    /// is the one caller, and the cursor clamp a change here owes is its job.
    pub(crate) fn set_filter(&mut self, filter: Option<Filter>) {
        self.filter = filter;
    }

    /// The tree, flattened in display order: user keyspaces first, then the
    /// cluster's own, each with its tables spliced in when unfolded.
    ///
    /// One flattening, so `update`'s motions and `view_model` cannot disagree
    /// about what row 5 is — which is why the filter is applied here and not
    /// in the projection (design §4.1).
    #[must_use]
    pub fn rows<'a>(&self, schema: &'a Schema) -> Vec<Node<'a>> {
        match &self.filter {
            Some(filter) => Self::filtered(schema, filter),
            None => self.whole(schema),
        }
    }

    fn whole<'a>(&self, schema: &'a Schema) -> Vec<Node<'a>> {
        let mut rows = Vec::new();

        for keyspace in schema.keyspaces_in_display_order() {
            let unfolded = self.is_unfolded(keyspace);
            rows.push(Node::Keyspace { keyspace, unfolded });

            if unfolded {
                rows.extend(
                    keyspace
                        .tables
                        .values()
                        .map(|table| Node::Table { keyspace, table }),
                );
            }
        }

        rows
    }

    /// A keyspace survives on its own name or on a table's, and is drawn open
    /// either way: a match inside a folded keyspace would make the panel
    /// report nothing while the row sits one keypress away.
    ///
    /// The unfold set is not written. Clearing the filter therefore restores
    /// the fold state exactly, and a live prompt cannot accumulate an unfold
    /// per prefix as the term is typed (design §4.1).
    fn filtered<'a>(schema: &'a Schema, filter: &Filter) -> Vec<Node<'a>> {
        let mut rows = Vec::new();

        for keyspace in schema.keyspaces_in_display_order() {
            let named = filter.matches(&keyspace.name);
            let tables: Vec<_> = keyspace
                .tables
                .values()
                .filter(|table| named || filter.matches(&table.name))
                .collect();

            if !named && tables.is_empty() {
                continue;
            }

            rows.push(Node::Keyspace {
                keyspace,
                unfolded: !tables.is_empty(),
            });
            rows.extend(
                tables
                    .into_iter()
                    .map(|table| Node::Table { keyspace, table }),
            );
        }

        rows
    }

    /// The node the cursor is on.
    #[must_use]
    pub fn selected<'a>(&self, schema: &'a Schema) -> Option<Node<'a>> {
        self.rows(schema).into_iter().nth(self.cursor.index())
    }

    /// Opens a folded keyspace, folds an open one. Does nothing on a table.
    ///
    /// Swallowed while a filter is applied: [`Self::filtered`] draws every
    /// retained keyspace open regardless of the unfold set, so folding would
    /// change nothing on screen while still writing `unfolded` — the one
    /// thing design §4.1 promises filtering never does. This is the single
    /// entry point for that guard, so every caller (keys, a click via
    /// `activate`) inherits it rather than needing its own; docs/shortcuts.md
    /// §2's rule that a panel swallows a motion it cannot honour is why the
    /// key is dropped here instead of half-honoured.
    pub fn toggle(&mut self, schema: &Schema) {
        if self.filter.is_some() {
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

    /// Opens a folded keyspace; on an open one, steps into its first table.
    ///
    /// Swallowed under a filter for the reason [`Self::toggle`] is (design
    /// §4.1): every retained keyspace is already drawn open, so there is
    /// nothing left for this to do that stepping onto a table row would not
    /// already offer, and unfolding a filtered-out keyspace would write a
    /// fold state the clear has to un-write later.
    pub fn open(&mut self, schema: &Schema) {
        if self.filter.is_some() {
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

    /// Folds an open keyspace; on a table, steps out to its keyspace without
    /// folding it, so a second press folds.
    ///
    /// Swallowed under a filter for the reason [`Self::toggle`] is (design
    /// §4.1).
    pub fn close(&mut self, schema: &Schema) {
        if self.filter.is_some() {
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

    /// A keyspace with no tables is never unfolded, whatever the set says —
    /// which is also what makes a name left over from a previous schema inert.
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
        self.cursor.clamp_to(self.rows(schema).len());
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

    /// Whether the range covers more than the row the cursor is on.
    ///
    /// What makes `Enter` inert and the Table DDL panel describe nothing: both
    /// act on one node, and a range names several
    /// (docs/superpowers/specs/2026-08-14-visual-mode-design.md §4).
    #[must_use]
    pub fn selects_many(&self) -> bool {
        self.selection
            .range(self.cursor.index())
            .is_some_and(|range| range.rows() > 1)
    }
}

/// Schema builders shared by every module in this crate that projects a tree.
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

    /// `(keyspace name, its table names)`.
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

    /// Every row as `(depth, name)`, which is what the assertions care about.
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

    fn filter_labels(panel: &TablesPanel, schema: &Schema) -> Vec<String> {
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
    fn a_matching_table_brings_its_keyspace_and_no_sibling() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();
        panel.set_filter(Filter::new("order"));

        assert_eq!(
            filter_labels(&panel, &schema),
            ["logs", "  order_events", "shop", "  orders"],
            "users and spans miss, and their keyspaces survive on their children"
        );
    }

    #[test]
    fn a_matching_keyspace_brings_all_of_its_tables() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();
        panel.set_filter(Filter::new("shop"));

        assert_eq!(
            filter_labels(&panel, &schema),
            ["shop", "  orders", "  users"]
        );
    }

    #[test]
    fn a_keyspace_with_nothing_matching_is_dropped() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();
        panel.set_filter(Filter::new("zzz"));

        assert!(panel.rows(&schema).is_empty());
    }

    #[test]
    fn a_retained_keyspace_is_drawn_open_whatever_its_fold_state() {
        let schema = shop_and_logs();
        let mut panel = TablesPanel::default();

        // Folded: the default. Without the filter it shows two keyspaces.
        assert_eq!(panel.rows(&schema).len(), 2);

        panel.set_filter(Filter::new("order"));

        assert!(
            matches!(
                panel.rows(&schema).first(),
                Some(Node::Keyspace { unfolded: true, .. })
            ),
            "a filter that hid its own matches would be no filter at all"
        );
    }

    #[test]
    fn clearing_the_filter_restores_the_fold_state_exactly() {
        let schema = shop_and_logs();
        let mut panel = unfolded_on("logs", &schema);
        let before = panel.rows(&schema);

        panel.set_filter(Filter::new("order"));
        let _ = panel.rows(&schema);
        panel.set_filter(None);

        assert_eq!(
            panel.rows(&schema),
            before,
            "filtering is a projection; it must not write the unfold set"
        );
    }

    #[test]
    fn a_keyspace_matching_by_name_is_open_only_when_it_has_tables() {
        let schema = schema_of(&[("empty", &[][..])]);
        let mut panel = TablesPanel::default();
        panel.set_filter(Filter::new("empty"));

        assert!(matches!(
            panel.rows(&schema).as_slice(),
            [Node::Keyspace {
                unfolded: false,
                ..
            }]
        ));
    }
}
