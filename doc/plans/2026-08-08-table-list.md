# Table List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Tables panel into a two-level tree — keyspaces at the root,
tables under an unfolded keyspace — and populate `Keyspace::tables` for the
first time.

**Architecture:** Unfolding is a pure `State` change that emits no `Effect`,
because `scylla`'s `get_cluster_state()` already holds every table's metadata
when `describe` returns. A new `lc-domain/src/tree.rs` owns the unfold set and
flattens the tree exactly once, so `update`'s motions and `view_model` cannot
disagree about what row 5 is. The adapter fills `Table` from
`get_cluster_state()` plus one `system_schema.columns` query, which supplies
the three things scylla's metadata drops.

**Tech Stack:** Rust 1.97.1, `scylla` 1.7.0, `ratatui` + `crossterm`,
`cargo-nextest`, `mise` for tasks.

**Design spec:** [doc/specs/2026-08-08-table-list-design.md](../specs/2026-08-08-table-list-design.md).
Read it before starting — every "why" below is short because the spec carries it.

## Global Constraints

- **No `unsafe`, no FFI.** `unsafe_code = "forbid"` is a workspace lint.
- **`lc-domain` must not depend on `tokio`, `ratatui`, `crossterm`, or any
  driver.** If an edit seems to need one, the design is wrong.
- **Zero warnings, zero failing or ignored tests.** `mise run ci` must be clean
  before a task is done. Any `#[allow(...)]` is item-scoped and carries a
  reason.
- **No narrating comments.** Expert Rust reader assumed. Comments are for edge
  cases, past bugs, upstream defects, and Cassandra/protocol semantics. Public
  items in `lc-domain` carry `///` docs.
- **Determinism in the fast tiers.** No wall clock, no network, no unseeded
  randomness, no reliance on `HashMap` iteration order.
- **Conventional Commits**, imperative, lowercase, no trailing period. Commit
  freely including to `master`. **Never `pull`, never `push`.**
- **Test tiers:** `mise run test` runs domain + scenario + render.
  `mise run test:it` runs the integration tier and needs `mise run db:up`
  first.

## File Structure

| File | Responsibility |
|---|---|
| `crates/lc-domain/src/tree.rs` | **New.** `TablesPanel` (cursor + unfold set), `Node`, the one flattening, and the three transitions. |
| `crates/lc-domain/src/state.rs` | Loses `TablesPanel` to `tree.rs`; `Cursor` gains `go_to`; `State` gains `tables_and_schema`. |
| `crates/lc-domain/src/lib.rs` | Declares `pub mod tree`. |
| `crates/lc-domain/src/model/schema.rs` | `Table` gains `static_columns`. |
| `crates/lc-domain/src/update.rs` | Bindings from spec §5; motion and clamping count tree rows. |
| `crates/lc-domain/src/view.rs` | `PanelBody::Tree`, `TreeRow`, `Marker`; projects `Node` → `TreeRow`. |
| `crates/lc-inbound/src/body.rs` | Draws markers and indentation. |
| `crates/lc-inbound/src/render.rs` | Render-tier tests. |
| `crates/lc-outbound/src/cassandra.rs` | Fills `Table` from driver metadata + `system_schema.columns`. |
| `crates/lc-outbound/tests/scenario.rs` | Whole-session tests through `FakeCluster`. |
| `crates/lc-outbound/tests/cassandra.rs` | Integration tier against the fixture. |

---

### Task 1: A table's static columns

`ColumnKind::Static` has nowhere to go in our `Table`. A static column is
stored with the partition rather than the row, so folding it into
`regular_columns` would be a lie the DDL slice inherits. Spec §3.

**Files:**
- Modify: `crates/lc-domain/src/model/schema.rs:38-44` (`Table`), `:79-88` (`Table::columns`)

**Interfaces:**
- Consumes: nothing.
- Produces: `Table { name: String, primary_key: PrimaryKey, static_columns: Vec<Column>, regular_columns: Vec<Column> }`.
  `Table::columns(&self) -> impl Iterator<Item = &Column>` yields partition,
  then clustering, then static, then regular.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module at the bottom of
`crates/lc-domain/src/model/schema.rs`, after the existing `schema` helper:

```rust
    fn column(name: &str, cql_type: &str) -> Column {
        Column {
            name: name.to_owned(),
            cql_type: cql_type.to_owned(),
        }
    }

    fn readings() -> Table {
        Table {
            name: "readings".to_owned(),
            primary_key: PrimaryKey {
                partition: vec![column("device_id", "text")],
                clustering: vec![ClusteringColumn {
                    column: column("reading_at", "timestamp"),
                    order: ClusteringOrder::Descending,
                }],
            },
            static_columns: vec![column("firmware", "text")],
            regular_columns: vec![column("temperature_c", "float")],
        }
    }

    #[test]
    fn every_column_is_listed_with_the_key_first_and_static_before_regular() {
        let names: Vec<&str> = readings().columns().map(|c| c.name.as_str()).collect();

        assert_eq!(
            names,
            ["device_id", "reading_at", "firmware", "temperature_c"]
        );
    }

    #[test]
    fn a_static_column_is_not_a_regular_one() {
        let table = readings();

        assert!(
            !table.regular_columns.iter().any(|c| c.name == "firmware"),
            "a static column repeats per partition, not per row"
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run -p lc-domain static_before_regular`
Expected: a **compile** error — `Table` has no field named `static_columns`.
That is the failing state for a type-level change.

- [ ] **Step 3: Add the field**

In `crates/lc-domain/src/model/schema.rs`, replace the `Table` struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub primary_key: PrimaryKey,
    /// Columns stored once per partition rather than once per row.
    ///
    /// Separate from `regular_columns` because static-ness is a fact about
    /// physical layout, which is the thing this model exists to keep visible.
    pub static_columns: Vec<Column>,
    /// Everything that is neither part of the primary key nor static.
    pub regular_columns: Vec<Column>,
}
```

and replace `Table::columns`:

```rust
impl Table {
    /// Every column: partition key, then clustering key, then static, then
    /// regular.
    pub fn columns(&self) -> impl Iterator<Item = &Column> {
        self.primary_key
            .partition
            .iter()
            .chain(self.primary_key.clustering.iter().map(|c| &c.column))
            .chain(self.static_columns.iter())
            .chain(self.regular_columns.iter())
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `mise run test`
Expected: PASS. Nothing constructs a `Table` literal yet outside this test, so
no other file breaks.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src/model/schema.rs
git commit -m "feat(domain): give a table its static columns

A static column is stored with the partition rather than the row, so
folding it into regular_columns would misreport physical layout — the
one thing vision.md §2 exists to keep honest.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: The tree module

`TablesPanel` moves out of `state.rs` and gains the unfold set, the flattening,
and the three transitions from spec §5. Pure domain, no rendering, no keys yet.

**Files:**
- Create: `crates/lc-domain/src/tree.rs`
- Modify: `crates/lc-domain/src/lib.rs:6-12` (module list)
- Modify: `crates/lc-domain/src/state.rs:60-65` (delete `TablesPanel`), `:9-11` (imports), `:123-152` (`Cursor`), `:154-182` (`impl State`)

**Interfaces:**
- Consumes: `Table::static_columns` from Task 1 (only via `Table` literals in tests).
- Produces:
  - `lc_domain::tree::TablesPanel { pub cursor: Cursor }` — `unfolded` is private.
    `Debug + Clone + Default + PartialEq + Eq`, **not `Copy`**.
  - `TablesPanel::rows<'a>(&self, schema: &'a Schema) -> Vec<Node<'a>>`
  - `TablesPanel::selected<'a>(&self, schema: &'a Schema) -> Option<Node<'a>>`
  - `TablesPanel::toggle(&mut self, schema: &Schema)`
  - `TablesPanel::open(&mut self, schema: &Schema)`
  - `TablesPanel::close(&mut self, schema: &Schema)`
  - `lc_domain::tree::Node<'a>` — `Keyspace { keyspace: &'a Keyspace, unfolded: bool }` | `Table { keyspace: &'a Keyspace, table: &'a Table }`
  - `lc_domain::state::Cursor::go_to(&mut self, index: usize, len: usize)`
  - `lc_domain::state::State::tables_and_schema(&mut self) -> Option<(&mut TablesPanel, &Schema)>`

- [ ] **Step 1: Write the failing tests**

Create `crates/lc-domain/src/tree.rs` containing **only** the shared builders
and the test module for now, so the tests fail before the implementation
exists. `tests_support` is `pub(crate)` because `update.rs` and `view.rs` build
the same schemas in Tasks 3 and 4 — the other two crates cannot see it and keep
their own:

```rust
/// Schema builders shared by every module in this crate that projects a tree.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use crate::model::{Column, PrimaryKey, Replication};
    use std::collections::BTreeMap;

    pub(crate) fn table(name: &str) -> Table {
        Table {
            name: name.to_owned(),
            primary_key: PrimaryKey {
                partition: vec![Column {
                    name: "id".to_owned(),
                    cql_type: "uuid".to_owned(),
                }],
                clustering: Vec::new(),
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
        }
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
}
```

- [ ] **Step 2: Run the tests to verify they fail**

First add `pub mod tree;` to `crates/lc-domain/src/lib.rs` (between `pub mod
style;` and `pub mod update;` — the list is alphabetical), then:

Run: `cargo nextest run -p lc-domain tree::`
Expected: compile errors — `TablesPanel`, `Node`, `Schema`, `Keyspace` and
`Cursor::go_to` are not in scope in `tree.rs`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/lc-domain/src/tree.rs`, above the test module:

```rust
//! The Tables panel's tree: keyspaces at the root, tables under an unfolded
//! keyspace.
//!
//! One flattening, used by both `update`'s motions and `view_model`, so the
//! two cannot disagree about what row 5 is.

use std::collections::BTreeSet;

use crate::model::{Keyspace, Schema, Table};
use crate::state::Cursor;

/// The Tables panel's own state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TablesPanel {
    pub cursor: Cursor,
    /// Keyspaces showing their tables.
    ///
    /// Private for the reason [`Cursor`]'s index is: the only ways in are the
    /// transitions below, so nothing can unfold a keyspace the schema lacks.
    /// Ordered rather than hashed per conventions.md §4.
    unfolded: BTreeSet<String>,
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
    /// The tree, flattened in display order: user keyspaces first, then the
    /// cluster's own, each with its tables spliced in when unfolded.
    #[must_use]
    pub fn rows<'a>(&self, schema: &'a Schema) -> Vec<Node<'a>> {
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

    /// The node the cursor is on.
    #[must_use]
    pub fn selected<'a>(&self, schema: &'a Schema) -> Option<Node<'a>> {
        self.rows(schema).into_iter().nth(self.cursor.index())
    }

    /// Opens a folded keyspace, folds an open one. Does nothing on a table.
    pub fn toggle(&mut self, schema: &Schema) {
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
    pub fn open(&mut self, schema: &Schema) {
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
    pub fn close(&mut self, schema: &Schema) {
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
}
```

- [ ] **Step 4: Move `TablesPanel` out of `state.rs` and add `Cursor::go_to`**

In `crates/lc-domain/src/state.rs`:

Delete the `TablesPanel` struct (the `/// The Tables panel's own state.` doc
comment through the closing brace) and add its import beside the others near
the top of the file:

```rust
use crate::tree::TablesPanel;
```

Add `go_to` to `impl Cursor`, after `clamp_to`:

```rust
    /// Moves to `index`, or to the last row when the list is shorter.
    pub const fn go_to(&mut self, index: usize, len: usize) {
        self.index = index;
        self.clamp_to(len);
    }
```

Add to `impl State`, after `schema()`:

```rust
    /// The tree's state and the schema it is a tree of.
    ///
    /// One method rather than two accessors because the borrow checker cannot
    /// split `&mut self` across separate `tables()` and `schema()` calls, and
    /// every transition needs both.
    pub fn tables_and_schema(&mut self) -> Option<(&mut TablesPanel, &Schema)> {
        match &self.connection {
            ConnectionState::Connected(Connection {
                schema: SchemaState::Loaded(schema),
                ..
            }) => Some((&mut self.tables, schema)),
            _ => None,
        }
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `mise run test`
Expected: PASS. `state.rs`'s own `TablesPanel` tests do not exist (it had
none), and `view.rs`/`update.rs` only touch `state.tables.cursor`, which is
unchanged.

If a compile error names `TablesPanel: Copy`, delete the `Copy` at that call
site — `BTreeSet` is not `Copy` and nothing relied on it.

- [ ] **Step 6: Check lints**

Run: `mise run lint`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/lc-domain/src/tree.rs crates/lc-domain/src/state.rs crates/lc-domain/src/lib.rs
git commit -m "feat(domain): flatten the tables tree with an unfold set

Unfolding emits no effect: describe already returns every table, so
there is no round trip to make. TablesPanel moves out of state.rs with
the set, the flattening and the three transitions, because update's
motions and view_model must agree about what row 5 is and the only way
to guarantee that is one flattening.

Follows doc/specs/2026-08-08-table-list-design.md §4.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: Bindings

Spec §5's table, gated on `focus == Panel::Tables` like the existing motions.
Motion and the `SchemaLoaded` clamp start counting tree rows rather than
keyspaces.

**Files:**
- Modify: `crates/lc-domain/src/update.rs:141-155` (`SchemaLoaded`), `:195-201` (key match), `:206-218` (`move_selection`)

**Interfaces:**
- Consumes: `TablesPanel::{rows, toggle, open, close}` and
  `State::tables_and_schema` from Task 2.
- Produces: no new public API. Behaviour only.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/lc-domain/src/update.rs`. First delete its
local `schema_of` helper and redefine it on top of Task 2's shared builder,
which keeps one definition of what a test schema is:

```rust
    use crate::tree::tests_support::schema_of as tabled_schema;

    fn schema_of(names: &[&str]) -> Schema {
        tabled_schema(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
    }
```

The existing `use crate::model::{Keyspace, Modifiers, Replication, StatementKind};`
import may now flag `Keyspace` and `Replication` as unused — drop whichever
clippy names. Then add the new tests:

```rust
    fn browsing_tables(keyspaces: &[(&str, &[&str])]) -> State {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(keyspaces)),
            },
        );
        state
    }

    fn row_count(state: &State) -> usize {
        state
            .schema()
            .map_or(0, |schema| state.tables.rows(schema).len())
    }

    #[test]
    fn enter_and_space_both_toggle_a_keyspace() {
        for key in [Key::Enter, Key::Char(' ')] {
            let mut state = browsing_tables(&[("shop", &["orders", "users"])]);

            press(&mut state, key);
            assert_eq!(row_count(&state), 3, "{key:?} did not unfold");

            press(&mut state, key);
            assert_eq!(row_count(&state), 1, "{key:?} did not fold");
        }
    }

    #[test]
    fn l_and_right_open_a_keyspace() {
        for key in [Key::Char('l'), Key::Right] {
            let mut state = browsing_tables(&[("shop", &["orders"])]);

            press(&mut state, key);

            assert_eq!(row_count(&state), 2, "{key:?} did not open");
        }
    }

    #[test]
    fn h_and_left_close_a_keyspace() {
        for key in [Key::Char('h'), Key::Left] {
            let mut state = browsing_tables(&[("shop", &["orders"])]);
            press(&mut state, Key::Char('l'));

            press(&mut state, key);

            assert_eq!(row_count(&state), 1, "{key:?} did not close");
        }
    }

    #[test]
    fn h_from_a_table_steps_out_to_its_keyspace_before_folding() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        assert_eq!(state.tables.cursor.index(), 1);

        press(&mut state, Key::Char('h'));

        assert_eq!(state.tables.cursor.index(), 0);
        assert_eq!(row_count(&state), 3, "stepping out must not fold");
    }

    #[test]
    fn enter_on_a_table_does_nothing_yet() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('l'));
        let before = state.tables.clone();

        press(&mut state, Key::Enter);

        assert_eq!(
            state.tables, before,
            "Enter on a table is reserved for acting on it"
        );
    }

    #[test]
    fn j_walks_into_an_unfolded_keyspaces_tables() {
        let mut state = browsing_tables(&[("shop", &["orders", "users"]), ("typezoo", &[])]);
        press(&mut state, Key::Char('l'));

        for _ in 0..3 {
            press(&mut state, Key::Char('j'));
        }

        assert_eq!(
            state.tables.cursor.index(),
            3,
            "motion counts tree rows, not keyspaces"
        );
    }

    #[test]
    fn a_tree_transition_belongs_to_the_focused_panel_only() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        state.focus = Panel::Queries;

        press(&mut state, Key::Char('l'));

        assert_eq!(row_count(&state), 1);
    }

    #[test]
    fn a_reload_that_shrinks_the_tree_pulls_the_selection_inside_it() {
        let mut state = browsing_tables(&[("shop", &["a", "b", "c"])]);
        let conn = current_conn(&state);
        press(&mut state, Key::Char('l'));
        for _ in 0..3 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(state.tables.cursor.index(), 3);

        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(tabled_schema(&[("shop", &["a"])])),
            },
        );

        assert_eq!(
            state.tables.cursor.index(),
            1,
            "the clamp must count tree rows, not keyspaces"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p lc-domain update::`
Expected: the new tests FAIL — `l`, `h`, `Enter` and `Space` are unbound, so
row counts stay at 1 and the clamp test lands on 0.

- [ ] **Step 3: Bind the keys and count tree rows**

In `crates/lc-domain/src/update.rs`, replace the `Msg::SchemaLoaded` arm:

```rust
        Msg::SchemaLoaded { conn, schema } => {
            let applied = match &mut state.connection {
                ConnectionState::Connected(connection) if connection.id == conn => {
                    connection.schema = SchemaState::Loaded(*schema);
                    true
                }
                _ => false,
            };
            if applied {
                let rows = tree_rows(state);
                state.tables.cursor.clamp_to(rows);
                state.mark_dirty();
            }
            Vec::new()
        }
```

Replace the key match arms in `handle_key`:

```rust
    match press.key {
        Key::Char('q') => return quit(state),
        Key::Escape => state.overlay = None,
        Key::Char('j') | Key::Down => move_selection(state, Motion::Down),
        Key::Char('k') | Key::Up => move_selection(state, Motion::Up),
        Key::Enter | Key::Char(' ') => transition(state, Transition::Toggle),
        Key::Char('l') | Key::Right => transition(state, Transition::Open),
        Key::Char('h') | Key::Left => transition(state, Transition::Close),
        _ => {}
    }
```

Replace `move_selection` and add the rest, keeping `Motion` where it is:

```rust
#[derive(Clone, Copy)]
enum Transition {
    Toggle,
    Open,
    Close,
}

fn move_selection(state: &mut State, motion: Motion) {
    if state.focus != Panel::Tables {
        return;
    }

    let rows = tree_rows(state);
    match motion {
        Motion::Down => state.tables.cursor.down(rows),
        Motion::Up => state.tables.cursor.up(),
    }
}

fn transition(state: &mut State, transition: Transition) {
    if state.focus != Panel::Tables {
        return;
    }

    let Some((tables, schema)) = state.tables_and_schema() else {
        return;
    };

    match transition {
        Transition::Toggle => tables.toggle(schema),
        Transition::Open => tables.open(schema),
        Transition::Close => tables.close(schema),
    }
}

fn tree_rows(state: &State) -> usize {
    state
        .schema()
        .map_or(0, |schema| state.tables.rows(schema).len())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `mise run test`
Expected: PASS, including the pre-existing
`the_selection_cannot_run_off_a_list_it_has_not_loaded` and
`motion_belongs_to_the_focused_panel_only`.

- [ ] **Step 5: Check lints**

Run: `mise run lint`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-domain/src/update.rs
git commit -m "feat(domain): bind open and close on the tables tree

Enter and Space toggle; l/Right open or step in; h/Left fold or step
out. h from a table is a motion before it is a fold, so h stays a
reliable outward move at any depth.

Motion and the SchemaLoaded clamp now count tree rows: counting
keyspaces stranded the cursor as soon as one unfolded.

Follows doc/specs/2026-08-08-table-list-design.md §5.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: Project and draw the tree

`PanelBody::Keyspaces` is replaced by `PanelBody::Tree`. This breaks `body.rs`
and `scenario.rs`, so all three move together.

**Files:**
- Modify: `crates/lc-domain/src/view.rs:30-57` (`PanelBody`, `KeyspaceRow`), `:103-133` (`tables_body`, `keyspaces`), tests
- Modify: `crates/lc-inbound/src/body.rs` (whole file)
- Modify: `crates/lc-inbound/src/render.rs:139-190` (test helpers and keyspace tests)
- Modify: `crates/lc-outbound/tests/scenario.rs:15-31` (helper), `:58-77` (first test)

**Interfaces:**
- Consumes: `TablesPanel::rows` and `Node` from Task 2.
- Produces:
  - `lc_domain::view::PanelBody::Tree { rows: Vec<TreeRow>, selected: usize }` — `Keyspaces` and `KeyspaceRow` are gone.
  - `lc_domain::view::TreeRow { label: String, depth: u8, marker: Marker, token: Option<Token> }`
  - `lc_domain::view::Marker` — `Folded | Unfolded | Leaf`

- [ ] **Step 1: Write the failing tests**

In `crates/lc-domain/src/view.rs`, replace the two keyspace tests
(`keyspaces_are_listed_with_the_clusters_own_dimmed_and_last` and
`the_selection_is_projected_from_the_cursor`) with:

```rust
    fn tabled(keyspaces: &[(&str, &[&str])]) -> State {
        state_with(ConnectionState::Connected(Connection {
            id: ConnId::next(),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(crate::tree::tests_support::schema_of(keyspaces)),
        }))
    }

    #[test]
    fn keyspaces_are_listed_with_the_clusters_own_dimmed_and_last() {
        let state = loaded(&["system_schema", "typezoo", "shop"]);

        let PanelBody::Tree { rows, selected } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(
            rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(),
            ["shop", "typezoo", "system_schema"]
        );
        assert_eq!(rows[0].token, None, "a user keyspace spends no token");
        assert_eq!(rows[2].token, Some(Token::Muted));
        assert_eq!(selected, 0);
    }

    #[test]
    fn a_keyspace_with_no_tables_offers_nothing_to_unfold() {
        let state = loaded(&["shop"]);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(
            rows[0].marker,
            Marker::Leaf,
            "an unfoldable-looking row that unfolds to nothing is worse than none"
        );
    }

    #[test]
    fn a_keyspace_with_tables_is_folded_until_it_is_opened() {
        let mut state = tabled(&[("shop", &["orders", "users"])]);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].marker, Marker::Folded);
        assert_eq!(rows[0].depth, 0);

        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };
        assert_eq!(
            rows.iter()
                .map(|row| (row.depth, row.marker, row.label.as_str()))
                .collect::<Vec<_>>(),
            [
                (0, Marker::Unfolded, "shop"),
                (1, Marker::Leaf, "orders"),
                (1, Marker::Leaf, "users"),
            ]
        );
    }

    #[test]
    fn a_table_is_muted_exactly_when_its_keyspace_is() {
        let mut state = tabled(&[("system_schema", &["tables"])]);
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(rows[0].token, Some(Token::Muted));
        assert_eq!(
            rows[1].token,
            Some(Token::Muted),
            "one rule per row, not two rules that can drift"
        );
    }

    #[test]
    fn the_selection_is_projected_from_the_cursor() {
        let mut state = loaded(&["shop", "typezoo"]);
        state.tables.cursor.down(2);

        let PanelBody::Tree { selected, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(selected, 1);
    }
```

`view.rs`'s existing `schema_of` and `loaded` helpers stay as they are: they
build table-less keyspaces, which still compile after Task 1. The test module
already opens with `use super::*;`, so `Marker` and `TreeRow` need no import —
only `tabled` above is new.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p lc-domain view::`
Expected: compile errors — `PanelBody::Tree`, `TreeRow` and `Marker` do not
exist.

- [ ] **Step 3: Add the projection**

In `crates/lc-domain/src/view.rs`, add to the imports:

```rust
use crate::tree::{Node, TablesPanel};
```

Replace the `Keyspaces` variant and `KeyspaceRow` with:

```rust
    Tree {
        rows: Vec<TreeRow>,
        selected: usize,
    },
}

/// One row of the Tables tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub label: String,
    /// 0 for a keyspace, 1 for a table.
    pub depth: u8,
    pub marker: Marker,
    /// `Some(Token::Muted)` for the cluster's own keyspaces and their tables.
    /// A user keyspace spends no token: it is the baseline everything else is
    /// read against.
    pub token: Option<Token>,
}

/// Whether a row has children, and whether they are showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Folded,
    Unfolded,
    /// Nothing to unfold: a table, or a keyspace with no tables.
    Leaf,
}
```

Replace `keyspaces` with `tree`, and update `tables_body`'s last arm to
`SchemaState::Loaded(schema) => tree(&state.tables, schema)`:

```rust
fn tree(tables: &TablesPanel, schema: &Schema) -> PanelBody {
    let rows: Vec<TreeRow> = tables.rows(schema).iter().map(tree_row).collect();

    if rows.is_empty() {
        return waiting("No keyspaces");
    }

    PanelBody::Tree {
        rows,
        selected: tables.cursor.index(),
    }
}

fn tree_row(node: &Node<'_>) -> TreeRow {
    let (keyspace, label, depth, marker) = match node {
        Node::Keyspace { keyspace, unfolded } => {
            let marker = if keyspace.tables.is_empty() {
                Marker::Leaf
            } else if *unfolded {
                Marker::Unfolded
            } else {
                Marker::Folded
            };
            (*keyspace, keyspace.name.clone(), 0, marker)
        }
        Node::Table { keyspace, table } => (*keyspace, table.name.clone(), 1, Marker::Leaf),
    };

    TreeRow {
        label,
        depth,
        marker,
        token: match keyspace.kind() {
            KeyspaceKind::User => None,
            KeyspaceKind::System => Some(Token::Muted),
        },
    }
}
```

- [ ] **Step 4: Draw it**

Replace the body of `crates/lc-inbound/src/body.rs` from the `COLLAPSED`
constant onward:

```rust
use lc_domain::style::Token;
use lc_domain::view::{Marker, PanelBody, TreeRow};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use crate::theme::Theme;

const FOLDED: &str = "▸ ";
const UNFOLDED: &str = "▾ ";
/// A row with nothing to unfold still spends the marker's width, so names stay
/// aligned down the panel.
const LEAF: &str = "  ";

const INDENT: &str = "  ";

pub(crate) fn render(frame: &mut Frame, area: Rect, body: &PanelBody, theme: Theme) {
    if area.is_empty() {
        return;
    }

    match body {
        PanelBody::Blank => {}
        PanelBody::Notice { text, token } => notice(frame, area, text, *token, theme),
        PanelBody::Tree { rows, selected } => tree(frame, area, rows, *selected, theme),
    }
}

fn notice(frame: &mut Frame, area: Rect, text: &str, token: Option<Token>, theme: Theme) {
    let mut paragraph = Paragraph::new(text).wrap(Wrap { trim: true });
    if let Some(token) = token {
        paragraph = paragraph.style(theme.style(token));
    }
    frame.render_widget(paragraph, area);
}

fn tree(frame: &mut Frame, area: Rect, rows: &[TreeRow], selected: usize, theme: Theme) {
    let items: Vec<ListItem> = rows.iter().map(|row| item(row, theme)).collect();

    let list = List::new(items).highlight_style(theme.style(Token::Selected));
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(list, area, &mut state);
}

fn item(row: &TreeRow, theme: Theme) -> ListItem<'static> {
    let indent = INDENT.repeat(usize::from(row.depth));
    let marker = match row.marker {
        Marker::Folded => FOLDED,
        Marker::Unfolded => UNFOLDED,
        Marker::Leaf => LEAF,
    };

    let item = ListItem::new(format!("{indent}{marker}{}", row.label));
    match row.token {
        Some(token) => item.style(theme.style(token)),
        None => item,
    }
}
```

- [ ] **Step 5: Update the render tests**

In `crates/lc-inbound/src/render.rs`, replace the `schema_of` and `browsing`
helpers and add a tree test. `schema_of` keeps building table-less keyspaces —
it just needs the new field:

```rust
    fn table(name: &str) -> lc_domain::model::Table {
        lc_domain::model::Table {
            name: name.to_owned(),
            primary_key: lc_domain::model::PrimaryKey {
                partition: vec![lc_domain::model::Column {
                    name: "id".to_owned(),
                    cql_type: "uuid".to_owned(),
                }],
                clustering: Vec::new(),
            },
            static_columns: Vec::new(),
            regular_columns: Vec::new(),
        }
    }

    fn schema_of(names: &[&str]) -> Schema {
        tabled_schema(&names.iter().map(|n| (*n, &[][..])).collect::<Vec<_>>())
    }

    fn tabled_schema(keyspaces: &[(&str, &[&str])]) -> Schema {
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
```

Then add, after `keyspaces_are_listed_inside_the_tables_panel`:

```rust
    #[test]
    fn an_unfolded_keyspaces_tables_are_indented_beneath_it() {
        let mut state = State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loaded(tabled_schema(&[
                    ("shop", &["orders", "users"]),
                    ("typezoo", &[]),
                ])),
            }),
            ..State::default()
        };
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let buffer = draw_state(40, 12, &state);

        assert_eq!(row(&buffer, 1), "│▾ shop        ││                      │");
        assert_eq!(row(&buffer, 2), "│    orders    ││                      │");
        assert_eq!(row(&buffer, 3), "│    users     ││                      │");
        assert_eq!(
            row(&buffer, 4),
            "│  typezoo     ││                      │",
            "a keyspace with no tables draws no marker"
        );
    }
```

The existing `keyspaces_are_listed_inside_the_tables_panel` asserts `▸ shop`
for table-less keyspaces; change its three expected rows to two leading spaces,
since `schema_of` builds keyspaces with no tables:

```rust
        assert_eq!(row(&buffer, 1), "│  shop        ││                      │");
        assert_eq!(row(&buffer, 2), "│  typezoo     ││                      │");
        assert_eq!(row(&buffer, 3), "│  system      ││                      │");
```

- [ ] **Step 6: Update the scenario test**

In `crates/lc-outbound/tests/scenario.rs`, change the import
`use lc_domain::view::PanelBody;` to
`use lc_domain::view::{Marker, PanelBody};` and replace the assertion inside
`connecting_and_describing_fills_the_tables_panel`:

```rust
    let PanelBody::Tree { rows, selected } = &view_model(&state).panels[0].body else {
        panic!("a described cluster lists its keyspaces");
    };
    assert_eq!(
        rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(),
        ["shop", "typezoo", "system_schema"]
    );
    assert_eq!(
        rows[0].marker,
        Marker::Leaf,
        "these fixtures have no tables to unfold"
    );
    assert_eq!(*selected, 0);
```

- [ ] **Step 7: Run everything**

Run: `mise run test`
Expected: PASS.

Run: `mise run lint`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/lc-domain/src/view.rs crates/lc-domain/src/tree.rs \
        crates/lc-inbound/src/body.rs crates/lc-inbound/src/render.rs \
        crates/lc-outbound/tests/scenario.rs
git commit -m "feat(domain,inbound): draw the tables tree

PanelBody::Keyspaces becomes Tree. The domain names a Marker and a
depth; the renderer picks the glyph and the indentation, the same
division that already keeps colours out of the view model.

A keyspace with no tables carries Marker::Leaf and draws no marker at
all, so a key that does nothing to it matches what is on screen.

Follows doc/specs/2026-08-08-table-list-design.md §6.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 5: Fill `Table` in the adapter

`describe` keeps `get_cluster_state()` and adds one `system_schema.columns`
query for the clustering order, CQL type text and static-ness that scylla's
metadata drops. Spec §3.

**Files:**
- Modify: `crates/lc-outbound/src/cassandra.rs:86-94` (`describe`), `:107-125` (`schema`, `to_keyspace`), tests

**Interfaces:**
- Consumes: `Table::static_columns` from Task 1.
- Produces: `Cluster::describe` now returns a `Schema` whose `Keyspace::tables`
  is populated. Private to the module:
  - `struct ColumnRow { column_name: String, kind: String, cql_type: String, clustering_order: String }`
  - `fn to_table(name: &str, partition_key: &[String], clustering_key: &[String], rows: &[ColumnRow]) -> Option<Table>`
  - `fn clustering_order(order: &str) -> ClusteringOrder`
  - `fn of_kind(rows: &[ColumnRow], kind: &str) -> Vec<Column>`

`scylla::cluster::metadata::Table` is `#[non_exhaustive]` and cannot be built
in a test, which is why `to_table` takes the two key-name slices rather than
the driver struct. That also keeps driver types out of the mapping entirely.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `crates/lc-outbound/src/cassandra.rs`:

```rust
    fn column_row(name: &str, kind: &str, cql_type: &str, order: &str) -> ColumnRow {
        ColumnRow {
            column_name: name.to_owned(),
            kind: kind.to_owned(),
            cql_type: cql_type.to_owned(),
            clustering_order: order.to_owned(),
        }
    }

    fn readings_rows() -> Vec<ColumnRow> {
        vec![
            column_row("device_id", "partition_key", "text", "none"),
            column_row("reading_at", "clustering", "timestamp", "desc"),
            column_row("seq", "clustering", "int", "asc"),
            column_row("temperature_c", "regular", "float", "none"),
            column_row("humidity_pct", "regular", "float", "none"),
            column_row("firmware", "static", "text", "none"),
            column_row("device_model", "static", "text", "none"),
        ]
    }

    fn readings() -> Table {
        to_table(
            "readings",
            &["device_id".to_owned()],
            &["reading_at".to_owned(), "seq".to_owned()],
            &readings_rows(),
        )
        .expect("every key column has a row")
    }

    #[test]
    fn a_clustering_column_keeps_the_order_the_table_declares() {
        let table = readings();

        assert_eq!(
            table
                .primary_key
                .clustering
                .iter()
                .map(|c| (c.column.name.as_str(), c.order))
                .collect::<Vec<_>>(),
            [
                ("reading_at", ClusteringOrder::Descending),
                ("seq", ClusteringOrder::Ascending),
            ]
        );
    }

    #[test]
    fn key_columns_keep_the_drivers_order_rather_than_the_rows() {
        let table = to_table(
            "composite",
            &["b".to_owned(), "a".to_owned()],
            &[],
            &[
                column_row("a", "partition_key", "int", "none"),
                column_row("b", "partition_key", "int", "none"),
            ],
        )
        .expect("every key column has a row");

        assert_eq!(
            table
                .primary_key
                .partition
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"],
            "partition order decides routing, so it is never re-sorted"
        );
    }

    #[test]
    fn static_columns_are_kept_apart_from_regular_ones() {
        let table = readings();

        assert_eq!(
            table
                .static_columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["device_model", "firmware"]
        );
        assert_eq!(
            table
                .regular_columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["humidity_pct", "temperature_c"],
            "Cassandra keeps no declaration order for these, so name order is what it knows"
        );
    }

    #[test]
    fn a_column_carries_the_cql_type_the_cluster_stores() {
        let table = to_table(
            "collections",
            &["id".to_owned()],
            &[],
            &[
                column_row("id", "partition_key", "int", "none"),
                column_row("frozen_list", "regular", "frozen<list<text>>", "none"),
            ],
        )
        .expect("every key column has a row");

        assert_eq!(table.regular_columns[0].cql_type, "frozen<list<text>>");
    }

    #[test]
    fn a_table_whose_key_column_has_no_row_is_refused() {
        let missing = to_table(
            "racing",
            &["device_id".to_owned()],
            &[],
            &[column_row("temperature_c", "regular", "float", "none")],
        );

        assert!(
            missing.is_none(),
            "a table displayed without its partition key is the one thing that must never happen"
        );
    }

    #[test]
    fn clustering_order_defaults_to_ascending_for_anything_but_desc() {
        assert_eq!(clustering_order("desc"), ClusteringOrder::Descending);
        assert_eq!(clustering_order("DESC"), ClusteringOrder::Descending);
        assert_eq!(clustering_order("asc"), ClusteringOrder::Ascending);
        assert_eq!(clustering_order("none"), ClusteringOrder::Ascending);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run -p lc-outbound cassandra::`
Expected: compile errors — `ColumnRow`, `to_table` and `clustering_order` do
not exist.

- [ ] **Step 3: Write the mapping**

In `crates/lc-outbound/src/cassandra.rs`, extend the domain import to:

```rust
use lc_domain::model::{
    ClusteringColumn, ClusteringOrder, Column, ConnId, DbError, Keyspace, Page, PagingState,
    PrimaryKey, Replication, Schema, Statement, Table,
};
```

Add above `fn schema`:

```rust
/// The three things `scylla::cluster::metadata` does not carry: clustering
/// order, the CQL type as text, and whether a column is static
/// (doc/specs/2026-08-08-table-list-design.md §3).
const COLUMNS_QUERY: &str = "SELECT keyspace_name, table_name, column_name, kind, type, \
                             clustering_order FROM system_schema.columns";

/// One row of `system_schema.columns`.
struct ColumnRow {
    column_name: String,
    kind: String,
    cql_type: String,
    clustering_order: String,
}

/// Every column in the cluster, grouped by the table it belongs to.
///
/// Materialized view columns are keyed by the view's own name, so they match
/// no entry in `keyspace.tables` and fall out of the join.
async fn column_rows(
    session: &Session,
) -> Result<BTreeMap<(String, String), Vec<ColumnRow>>, DbError> {
    let result = session
        .query_unpaged(COLUMNS_QUERY, &[])
        .await
        .map_err(|error| DbError::Other(error.to_string()))?
        .into_rows_result()
        .map_err(|error| DbError::Other(error.to_string()))?;

    let rows = result
        .rows::<(String, String, String, String, String, String)>()
        .map_err(|error| DbError::Other(error.to_string()))?;

    let mut grouped: BTreeMap<(String, String), Vec<ColumnRow>> = BTreeMap::new();
    for row in rows {
        let (keyspace, table, column_name, kind, cql_type, clustering_order) =
            row.map_err(|error| DbError::Other(error.to_string()))?;

        grouped.entry((keyspace, table)).or_default().push(ColumnRow {
            column_name,
            kind,
            cql_type,
            clustering_order,
        });
    }

    Ok(grouped)
}

/// Builds a table from the driver's key ordering and `system_schema`'s column
/// detail.
///
/// `None` when a key column has no row: the two reads disagreed because DDL
/// landed between them, and `PrimaryKey::partition` documents itself as never
/// empty.
fn to_table(
    name: &str,
    partition_key: &[String],
    clustering_key: &[String],
    rows: &[ColumnRow],
) -> Option<Table> {
    let by_name: BTreeMap<&str, &ColumnRow> = rows
        .iter()
        .map(|row| (row.column_name.as_str(), row))
        .collect();

    let partition: Vec<Column> = partition_key
        .iter()
        .map(|name| by_name.get(name.as_str()).map(|row| column(row)))
        .collect::<Option<_>>()?;

    let clustering: Vec<ClusteringColumn> = clustering_key
        .iter()
        .map(|name| {
            by_name.get(name.as_str()).map(|row| ClusteringColumn {
                column: column(row),
                order: clustering_order(&row.clustering_order),
            })
        })
        .collect::<Option<_>>()?;

    Some(Table {
        name: name.to_owned(),
        primary_key: PrimaryKey {
            partition,
            clustering,
        },
        static_columns: of_kind(rows, "static"),
        regular_columns: of_kind(rows, "regular"),
    })
}

fn column(row: &ColumnRow) -> Column {
    Column {
        name: row.column_name.clone(),
        cql_type: row.cql_type.clone(),
    }
}

/// Static and regular columns in name order: Cassandra records `position = -1`
/// for both and keeps no declaration order, so this is what the cluster
/// actually knows — `DESCRIBE` prints them the same way.
fn of_kind(rows: &[ColumnRow], kind: &str) -> Vec<Column> {
    let mut columns: Vec<Column> = rows
        .iter()
        .filter(|row| row.kind == kind)
        .map(column)
        .collect();
    columns.sort_by(|left, right| left.name.cmp(&right.name));
    columns
}

/// `clustering_order` is `asc`, `desc`, or `none` for a column outside the
/// clustering key — and `none` only ever reaches here for a key column when
/// the cluster is mid-upgrade, where ascending is Cassandra's own default.
fn clustering_order(order: &str) -> ClusteringOrder {
    if order.eq_ignore_ascii_case("desc") {
        ClusteringOrder::Descending
    } else {
        ClusteringOrder::Ascending
    }
}
```

Replace `schema` and `to_keyspace`:

```rust
fn schema(
    state: &ClusterState,
    columns: &BTreeMap<(String, String), Vec<ColumnRow>>,
) -> Result<Schema, DbError> {
    let keyspaces = state
        .keyspaces_iter()
        .map(|(name, keyspace)| Ok((name.to_owned(), to_keyspace(name, keyspace, columns)?)))
        .collect::<Result<BTreeMap<_, _>, DbError>>()?;

    Ok(Schema { keyspaces })
}

fn to_keyspace(
    name: &str,
    keyspace: &DriverKeyspace,
    columns: &BTreeMap<(String, String), Vec<ColumnRow>>,
) -> Result<Keyspace, DbError> {
    let mut tables = BTreeMap::new();

    for (table_name, table) in &keyspace.tables {
        let rows = columns
            .get(&(name.to_owned(), table_name.clone()))
            .map_or(&[][..], Vec::as_slice);

        let mapped = to_table(table_name, &table.partition_key, &table.clustering_key, rows)
            .ok_or_else(|| {
                DbError::Invalid(format!("schema changed while reading {name}.{table_name}"))
            })?;

        tables.insert(table_name.clone(), mapped);
    }

    Ok(Keyspace {
        name: name.to_owned(),
        replication: replication(&keyspace.strategy),
        tables,
    })
}
```

Replace `describe`:

```rust
    async fn describe(&self, conn: ConnId) -> Result<Schema, DbError> {
        let session = self.session(conn).await?;
        session
            .refresh_metadata()
            .await
            .map_err(|error| DbError::Other(error.to_string()))?;

        let columns = column_rows(&session).await?;

        schema(&session.get_cluster_state(), &columns)
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `mise run test`
Expected: PASS.

Run: `mise run lint`
Expected: clean. If clippy flags `of_kind`'s `sort_by`, switch it to
`sort_by(|left, right| left.name.cmp(&right.name))` as written — do not use
`sort_by_key`, which cannot borrow the field.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-outbound/src/cassandra.rs
git commit -m "feat(outbound): fill a keyspace's tables from system_schema

scylla's metadata carries no clustering order, no Display for
ColumnType, and nowhere to put a static column. One unpaged query over
system_schema.columns closes all three: its type column is the CQL text
Column::cql_type already documents, so no type formatter is needed.

Defaulting clustering order to ascending was not available — reporting a
DESC table as ASC is the confident wrong answer about physical layout
that vision.md §2 exists to prevent.

A table whose key columns have no rows is refused rather than shown
without a partition key: DDL landed between the two reads.

Follows doc/specs/2026-08-08-table-list-design.md §3.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 6: Pin the mapping against the fixture

The fast tier can only test the mapping against rows we invent. The integration
tier is what proves the query returns what we think it does. Spec §7.

**Files:**
- Modify: `crates/lc-outbound/tests/cassandra.rs` (add tests)

**Interfaces:**
- Consumes: everything from Tasks 1 and 5.
- Produces: nothing.

**Prerequisite:** `mise run db:up` must have run. If port 9042 is taken, use
`LC_CASSANDRA_PORT=9043 mise run db:up` and the same variable when testing.

- [ ] **Step 1: Write the failing tests**

Add to `crates/lc-outbound/tests/cassandra.rs`. Extend the import to:

```rust
use lc_domain::model::{ClusteringOrder, ConnId, Keyspace, KeyspaceKind, Replication};
```

and add a helper plus the tests:

```rust
async fn fixture_schema() -> lc_domain::model::Schema {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);

    cluster.connect(conn).await.expect("the fixture is up");
    cluster.describe(conn).await.expect("describe succeeds")
}

#[tokio::test]
async fn a_keyspaces_tables_arrive_with_the_schema_and_exclude_its_views() {
    let schema = fixture_schema().await;

    let shop = schema.keyspaces.get("shop").expect("shop exists");

    assert_eq!(
        shop.tables.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "audit_log_by_day",
            "customers_by_id",
            "inventory_by_warehouse",
            "order_items_by_order",
            "orders_by_customer",
            "product_view_counts",
            "products_by_category",
        ],
        "customers_by_email is a materialized view, not a table"
    );
}

#[tokio::test]
async fn a_tables_clustering_order_survives_the_round_trip() {
    let schema = fixture_schema().await;

    let orders = schema
        .keyspaces
        .get("shop")
        .expect("shop exists")
        .tables
        .get("orders_by_customer")
        .expect("orders_by_customer exists");

    assert_eq!(
        orders
            .primary_key
            .partition
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["customer_id"]
    );
    assert_eq!(
        orders
            .primary_key
            .clustering
            .iter()
            .map(|c| (c.column.name.as_str(), c.order))
            .collect::<Vec<_>>(),
        [
            ("order_date", ClusteringOrder::Descending),
            ("order_id", ClusteringOrder::Ascending),
        ],
        "the driver's metadata drops this, so the extra query is what earns it"
    );
}

#[tokio::test]
async fn static_columns_are_reported_apart_from_regular_ones() {
    let schema = fixture_schema().await;

    let table = schema
        .keyspaces
        .get("typezoo")
        .expect("typezoo exists")
        .tables
        .get("static_columns")
        .expect("static_columns exists");

    assert_eq!(
        table
            .static_columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["calibrated_at", "device_model", "firmware"]
    );
    assert_eq!(
        table
            .regular_columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["humidity_pct", "temperature_c"],
        "regular columns come back alphabetical, which is all Cassandra knows"
    );
}

#[tokio::test]
async fn a_column_reports_the_cql_type_the_cluster_stores() {
    let schema = fixture_schema().await;

    let table = schema
        .keyspaces
        .get("typezoo")
        .expect("typezoo exists")
        .tables
        .get("collection_types")
        .expect("collection_types exists");

    let frozen = table
        .regular_columns
        .iter()
        .find(|c| c.name == "frozen_list")
        .expect("frozen_list exists");

    assert_eq!(frozen.cql_type, "frozen<list<text>>");
}
```

- [ ] **Step 2: Bring the fixture up**

Run: `mise run db:up`
Expected: the cassandra container reports healthy and the seed container exits
0. This takes about a minute.

- [ ] **Step 3: Run the integration tier**

Run: `mise run test:it`
Expected: PASS. If `a_column_reports_the_cql_type_the_cluster_stores` fails on
whitespace (`frozen<list<text>>` vs `frozen<list<text> >`), assert the real
value the cluster returned rather than reformatting it — the point of the field
is that it is verbatim.

- [ ] **Step 4: Run the full gate**

Run: `mise run ci`
Expected: clean. `ci` does not include `test:it`, so run both.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-outbound/tests/cassandra.rs
git commit -m "test(outbound): pin the table mapping against the fixture

The fast tier can only check the mapping against rows we invent; these
prove system_schema.columns returns what the mapping assumes — including
that a materialized view never reaches the tree.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 7: Update the docs the slice changed

`architecture.md` §4 lists the domain's module layout and does not know about
`tree.rs`; the spec's status is still `designed`.

**Files:**
- Modify: `doc/architecture.md` (§4 module list, §11 open questions)
- Modify: `doc/specs/2026-08-08-table-list-design.md` (status line)

**Interfaces:**
- Consumes: nothing.
- Produces: nothing.

- [ ] **Step 1: Add `tree.rs` to the module list**

In `doc/architecture.md` §4, insert into the code block after `state.rs`:

```
  tree.rs       # the Tables panel's tree: unfold set, flattening, transitions
```

- [ ] **Step 2: Update §11's open question about `State`**

Replace the `Does `State` stay one struct?` bullet's last sentence with:

```
  hatch; `TablesPanel` has now taken it, moving to its own module with the
  unfold set. Whether the other three panels follow is still open.
```

- [ ] **Step 3: Mark the spec implemented**

In `doc/specs/2026-08-08-table-list-design.md`, change
`**Status:** designed.` to `**Status:** implemented.`

- [ ] **Step 4: Verify nothing else went stale**

Run: `rtk proxy grep -rn "Keyspaces\|KeyspaceRow" doc/ crates/`
Expected: matches only inside
`doc/specs/2026-08-08-keyspace-list-design.md` (a historical record of that
slice, which stays as written) and prose uses of the word. Any live code match
is a leftover from Task 4.

- [ ] **Step 5: Commit**

```bash
git add doc/architecture.md doc/specs/2026-08-08-table-list-design.md
git commit -m "docs: record the tree module and mark the table list done

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage.** §1 scope → Tasks 2–5, with the out-of-scope list untouched
(no DDL/Contents work, no focus movement, no refresh). §2 unfolding is not I/O
→ Task 2, no `Effect` anywhere in the transitions. §3 adapter, all three gaps
and the disagreement failure → Task 5, pinned by Task 6. §4 tree module,
private set, reload behaviour → Task 2. §5 key table, every cell → Task 3. §6
`PanelBody::Tree`, `Marker`, indentation → Task 4. §7 all four test tiers →
Tasks 2, 3, 4 (domain, scenario, render) and 6 (integration). §8's "opens" is
recorded in Task 7's §11 edit.

**Type consistency.** `TablesPanel::{rows, selected, toggle, open, close}`,
`Node::{Keyspace, Table}`, `TreeRow::{label, depth, marker, token}`,
`Marker::{Folded, Unfolded, Leaf}`, `Cursor::go_to`,
`State::tables_and_schema`, `ColumnRow::{column_name, kind, cql_type,
clustering_order}` and `to_table`'s four-parameter signature are used
identically wherever they appear.

**One thing to watch during execution.** `State::tables_and_schema` returns
`(&mut TablesPanel, &Schema)` from a single `&mut self`. This compiles only
because the two come from disjoint fields accessed directly in that method's
body. If it is refactored into two calls, it will stop compiling — that is the
reason the doc comment on it exists.
