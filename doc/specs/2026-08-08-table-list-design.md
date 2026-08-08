# Table list — design

Turns the Tables panel from a keyspace list into a tree, and fills
`Keyspace::tables` for the first time.

**Status:** implemented.
**Date:** 2026-08-08

---

## 1. Scope

The Tables panel is a two-level tree: keyspaces at the root, tables under an
unfolded keyspace. The
[keyspace list slice](2026-08-08-keyspace-list-design.md) built the root and
left `Keyspace::tables` empty; this one fills it and makes the root unfold.

In scope: mapping scylla's table metadata into our `Table`, an unfold set in
`TablesPanel`, flattening the tree in the domain, bindings that open and close a
keyspace, and rendering rows with depth and a fold marker.

Out of scope, and deliberately: materialized views, secondary indexes and UDTs —
scylla lists them beside `tables` and the tree shows tables only; acting on a
selected table, so the DDL and Contents panels stay `Blank`; focus movement
between panels; and refreshing a schema after it has loaded.

A table row shows its name and nothing else. The sidebar is 25% of the frame
with a 16-column floor, so a row has roughly 25 usable columns — and the
fixture already carries a 48-character table name. Rendering a truncated
partition key there would teach the wrong physical shape, which is worse than
teaching none; the DDL panel sits directly below and is where vision.md §2's
commitment gets its full treatment.

## 2. Unfolding is not I/O

`scylla`'s `get_cluster_state()` already holds every table's metadata in memory
by the time `describe` returns. A per-keyspace fetch on unfold would invent a
round trip that does not exist, and with it a loading state and a failure state
per node.

So `describe` returns the whole `Schema`, and unfolding is a pure `State`
change that emits no `Effect`. Nothing in the domain gains an in-flight node.

The cost is one large `Schema` on a cluster with thousands of tables. That is
one allocation against the round trips it saves, and the memory question that
architecture.md §11 actually flags is the *result set*, not the schema.

## 3. Adapter — filling `Table`

`describe` keeps `get_cluster_state()` and adds one unpaged query.

### What scylla does not give us

`scylla::cluster::metadata` was chosen because it returns very nearly our model
already (architecture.md §8). Three gaps show up the moment `Table` has to be
populated rather than mapped as empty:

| gap | why it matters |
|---|---|
| No clustering order | `metadata::Table` has `clustering_key: Vec<String>` and no ASC/DESC. Its own fetch selects `kind, position, type` from `system_schema.columns` and drops `clustering_order`. `ClusteringColumn::order` cannot be filled. |
| No `Display` for `ColumnType` | `Column::cql_type` is documented as the CQL type *as the cluster reports it*. Producing that from scylla's parsed tree means a recursive formatter over `Native`, `Collection`, `Vector`, `UserDefinedType`, `Tuple` and `Custom`, kept correct against the CQL type space forever. |
| `ColumnKind::Static` has no home | Our `Table` has `primary_key` and `regular_columns` only. |

Defaulting clustering order to ascending is not available: the fixture's
`shop.orders_by_customer` is `CLUSTERING ORDER BY (order_date DESC, order_id
ASC)`, and silently reporting it as ascending is exactly the kind of confident
wrong answer about physical layout that vision.md §2 exists to prevent.

### The query

```sql
SELECT keyspace_name, table_name, column_name, kind, type, clustering_order
  FROM system_schema.columns
```

One unpaged query for the whole cluster, grouped by `(keyspace_name,
table_name)` and joined onto the tables `get_cluster_state()` listed.

`type` is the CQL type as text — the same string `DESCRIBE` prints, and
literally what `Column::cql_type` documents. Taking it here means the recursive
formatter is never written, so the second gap closes as a side effect of
closing the first.

Materialized view columns live in `system_schema.columns` under the view's own
`table_name`. They match no table in `keyspace.tables`, so they fall out of the
join and views stay out of the tree without a filter. Secondary indexes are in
`system_schema.indexes` and never appear here at all.

This is a small, deliberate reversal of the reasoning in
[the driver spec](2026-08-08-keyspace-list-design.md) §2, which counted
hand-reading `system_schema` against `cdrs-tokio`. The argument there was about
parsing the *entire* schema by hand and owning its version drift. Six columns of
one table, whose shape has been stable since Cassandra 3.0, is a different
quantity of the same thing — and the alternative is not "no hand-written code",
it is a CQL type formatter that must track the type space instead.

### Mapping

- **Partition columns** in the order of `metadata::Table::partition_key`, which
  is already position-ordered.
- **Clustering columns** in the order of `metadata::Table::clustering_key`, each
  paired with its `clustering_order`.
- **Static and regular columns** alphabetically. Cassandra records `position =
  -1` for both and does not preserve declaration order, so alphabetical is the
  correct semantics rather than a fallback; §7's integration test pins it.
- **`Table::static_columns: Vec<Column>`** is a new field. A static column is
  per-partition rather than per-row, so folding it into `regular_columns` would
  be a lie the DDL slice then has to unpick. The fixture's
  `typezoo.static_columns` exists for this.

### When the two reads disagree

`get_cluster_state()` reads scylla's cached metadata; the query reads live
`system_schema`. A DDL change landing between them leaves a table with no
column rows.

`describe` returns `DbError::Invalid` in that case rather than yielding a table
whose `PrimaryKey::partition` is empty. `PrimaryKey` documents that field as
never empty, and a table displayed without a partition key is the single thing
vision.md §2 says must never happen. The domain already routes this to
`Msg::SchemaFailed`, the overlay, and the Tables panel's own `Notice`.

Recovery today is a restart, because nothing re-issues `Effect::LoadSchema`.
§8 records that as opened.

## 4. Domain

### A module for the tree

`lc-domain/src/tree.rs` is new and owns the Tables panel's tree. `state.rs`
keeps `State`, `Cursor`, `Panel` and the connection types; it is already 338
lines and the tree is a separable thing with its own vocabulary.

```rust
pub struct TablesPanel {
    pub cursor: Cursor,
    unfolded: BTreeSet<String>,   // keyspace names
}

pub enum Node<'a> {
    Keyspace { keyspace: &'a Keyspace, unfolded: bool },
    Table    { keyspace: &'a Keyspace, table: &'a Table },
}

impl TablesPanel {
    pub fn rows<'a>(&self, schema: &'a Schema) -> Vec<Node<'a>>;
    pub fn selected<'a>(&self, schema: &'a Schema) -> Option<Node<'a>>;
}
```

`rows` walks `keyspaces_in_display_order()` and splices each unfolded
keyspace's tables in after it. One function, so `update`'s motion logic and
`view_model` cannot disagree about what row 5 is — the bug that a second
flattening would eventually produce.

`unfolded` is private for the reason `Cursor::index` is: the only ways to
change it are the three transitions in §5, so no caller can unfold a keyspace
the schema does not have.

`BTreeSet` rather than `HashSet` even though only membership is tested, per
conventions.md §4 — an ordered collection by default costs nothing here and
removes the question.

`Keyspace::tables` is already a `BTreeMap`, so tables come out alphabetical
with no ordering rule of their own. Keyspaces keep theirs: user before system,
each alphabetical.

`TablesPanel` stops being `Copy`, which `BTreeSet` forces. `Cursor` stays
`Copy`.

### Reload

The unfold set survives a schema reload: a keyspace still present stays open.
Names that vanished stay in the set and are inert, because `rows` consults it
only for keyspaces the schema has. Pruning them would cost a pass to buy
nothing.

`Msg::SchemaLoaded` already clamps the cursor; it now clamps to the tree's row
count rather than the keyspace count.

## 5. Keys

Every binding is gated on `focus == Panel::Tables`, as `j`/`k` already are.

| key | keyspace, folded | keyspace, unfolded | keyspace with no tables | table |
|---|---|---|---|---|
| `Enter`, `Space` | unfold | fold | — | — |
| `l`, `→` | unfold | move to first table | — | — |
| `h`, `←` | — | fold | — | move to parent keyspace |

`Enter` and `Space` are what a novice tries first; `h`/`l` are the directional
motion an expert already has in their fingers. Neither is a hidden faster path
to the other — vision.md §4, principle 2.

`Enter` on a table does nothing in this slice. It is the natural binding for
"act on this table" once the DDL and Contents panels have something to show,
and giving it a second meaning here would have to be taken back.

`h` on a table moves to the parent without folding it; a second `h` folds. The
usual tree behaviour, and it makes `h` a reliable "outward" motion regardless
of depth.

A keyspace with no tables draws no marker at all (§6), so a key doing nothing
to it matches what is on screen. This is why it is not unfolded into an empty
node: an unfoldable-looking row that unfolds to nothing is worse than a row
that never claimed to unfold.

Every fold clamps the cursor. Folding with the cursor on the keyspace itself
cannot strand it, but a reload can, and one clamp covers both.

## 6. View and rendering

`PanelBody::Keyspaces` is replaced rather than joined:

```rust
pub enum PanelBody {
    Blank,
    Notice { text: String, token: Option<Token> },
    Tree { rows: Vec<TreeRow>, selected: usize },
}

pub struct TreeRow {
    pub label: String,
    pub depth: u8,
    pub marker: Marker,
    pub token: Option<Token>,
}

pub enum Marker { Folded, Unfolded, Leaf }
```

`Marker` is the meaning; `body.rs` maps it to `▸ `, `▾ ` and `  ` and indents by
`depth`. That division is not new — `COLLAPSED: &str = "▸ "` already lives in
the renderer while the view model names tokens, which is architecture.md §6
applied to glyphs.

`Marker::Leaf` covers both a table and a keyspace with no tables. Depth
distinguishes them on screen and neither has children, so one variant is
honest.

A table inherits its keyspace's token, so a system keyspace's tables are muted
by the same rule that mutes the keyspace — one rule applied per row rather than
two rules that can drift.

`view.rs` maps each `Node` to a `TreeRow` — the projection stays there, so
`tree.rs` never names a marker or a token and the domain still has exactly one
flattening.

```
┌─Tables─────────────┐
│  empty_keyspace    │
│▾ shop              │
│    audit_log_by_day│
│    customers_by_id │
│    orders_by_custo…│
│▸ typezoo           │
│▸ system_schema     │
└────────────────────┘
```

User keyspaces first and alphabetical, the cluster's own last and muted —
`keyspaces_in_display_order()` unchanged.

Scrolling is unchanged: a `ListState` built per frame from `selected`, no
scroll state stored (keyspace list spec §5).

## 7. Tests

**Domain.** Flattening with keyspaces folded and unfolded: row order, depth and
marker per row, including the marker-less empty keyspace. Every cell of §5's
table, including `Enter` on a table doing nothing and `h` on a table landing on
its parent without folding it. The cursor clamped on fold and on a reload that
shrinks the tree. Motion counting tree rows rather than keyspaces. The unfold
set surviving a reload and ignoring names the new schema does not have.

**Scenario.** `FakeCluster` already takes a whole `Schema`, so it needs no
change. Connect → describe with tables → `l` → assert the `ViewModel` rows and
their markers; and `h` from a table landing on its parent.

**Render.** A `TestBackend` snapshot with one keyspace unfolded and the
selection on a table, guarding indentation and the marker mapping.

**Integration.** Against the fixture, behind the existing `integration`
feature:

- `shop` lists its seven tables and not the `customers_by_email` materialized
  view.
- `shop.orders_by_customer` reports partition `customer_id`, then clustering
  `order_date` descending and `order_id` ascending, in that order.
- `typezoo.static_columns` puts `device_model`, `firmware` and `calibrated_at`
  in `static_columns` and not in `regular_columns`.
- Regular columns come back alphabetical.

## 8. What this closes and opens

Closes the keyspace list spec's deferral of `Keyspace::tables`, and answers
architecture.md §11's "does `State` stay one struct?" a little further:
per-panel state now has its own module, not just its own field.

Opens: a refresh binding, which §3 makes newly load-bearing — a schema that
failed to re-read has no recovery short of a restart. Left open as before:
whether the `Cluster` port carries an address, and where schema caching lives.
