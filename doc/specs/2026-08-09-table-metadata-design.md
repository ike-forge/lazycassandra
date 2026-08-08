# Table metadata — design

Fills the Table DDL panel: the selected table's physical shape, then every
property `system_schema.tables` reports about it.

**Status:** implemented.
**Date:** 2026-08-09

---

## 1. Scope

The Table DDL panel stops being `Blank` and becomes a detail pane driven by the
Tables cursor:

- **a table** — its primary key structure, then its table options in importance
  order
- **a keyspace** — its replication class and factors
- **anything else** — `Blank`

In scope: a second unpaged query in `describe`, `TableOptions` on `Table`, a
`Details` panel body with its projection, and rendering it in a panel roughly
twenty columns wide.

Out of scope, and deliberately: the regular and static column list, which is
what [the table list slice](2026-08-08-table-list-design.md) already fetched
and this one does not yet draw; scrolling and focusing the panel (§7); indexes
and materialized views; and any write path.

The panel repeats none of the Tables panel's connection notices. "Not
connected" said twice in one sidebar is noise, so with no selection the panel
is simply blank.

## 2. Where the data comes from

Table-list §2 settled that unfolding is not I/O and `describe` returns the whole
`Schema`. This follows it: one more unpaged query, joined onto the tables
`get_cluster_state()` listed, and no new in-flight state anywhere.

```sql
SELECT * FROM system_schema.tables
```

### Why `SELECT *`

The column set of `system_schema.tables` moves between majors. 3.11 has
`read_repair_chance` and `dclocal_read_repair_chance`; 4.0 removes both and adds
`read_repair` and `additional_write_policy`; 5.0 adds `memtable`,
`allow_auto_snapshot` and `incremental_backups`. A named `SELECT` is a hard
error against any cluster missing one of the names — and TODO already carries
support for Cassandra before 5.

`SELECT *` reports whatever that cluster knows, so an older cluster yields fewer
properties rather than an error, and a newer one yields properties this code has
never heard of. That is the behaviour the version list above actually calls for.

`keyspace_name` and `table_name` are dropped: they are the panel's heading.

### A missing row is not a failure

Table-list §3 has `describe` refuse the whole schema when a table's *columns* go
missing, because `PrimaryKey::partition` documents itself as never empty and a
table shown without its partition key is what vision.md §2 exists to prevent.

Options carry no such invariant. A table whose options row is absent — a DDL
change landing between the two reads — renders its key block and an empty
`OPTIONS` section. Refusing the schema over a missing `bloom_filter_fp_chance`
would trade a whole usable screen for a cosmetic gap.

The cost of the extra read is that `describe` now makes three, so the window in
which DDL can race it is wider. Same failure mode, same recovery — a restart,
until the refresh binding TODO already carries lands.

## 3. `TableOptions` is a string map, deliberately

```rust
pub struct Table {
    // …
    pub options: TableOptions,
}

/// Every property `system_schema.tables` reports for this table, as text.
pub struct TableOptions { by_name: BTreeMap<String, String> }

impl TableOptions {
    /// Ranked properties first, then anything unranked, alphabetically.
    pub fn in_display_order(&self) -> impl Iterator<Item = (&str, &str)>;
}
```

conventions.md §1 lists stringly-typed state among the things to avoid, so this
needs its argument made rather than assumed.

There is no invariant here to push into a type. `PrimaryKey` earns its structure
because partition and clustering columns *mean* different things and conflating
them is the mistake the tool exists to prevent. Table options are a
version-varying dictionary of settings the cluster reports about itself, and a
struct of twenty-three fields would encode a claim that is false: that we know
the full set. On a 3.11 cluster it would silently drop `read_repair_chance`; on
whatever ships next it would drop that too. A map is not a weaker model of this
data, it is the accurate one.

What the domain does own is the *ordering*, which is real knowledge and lives in
a ranking table beside the type. When a property gains meaning the tool acts on
— Phase 2 warning that `gc_grace_seconds` is zero — that is a typed accessor
parsing one entry, not a reason to type all twenty-three now.

### Importance order

```
comment · default_time_to_live · gc_grace_seconds · compaction · compression ·
caching · bloom_filter_fp_chance · speculative_retry · additional_write_policy ·
read_repair · cdc · crc_check_chance · min_index_interval · max_index_interval ·
memtable · memtable_flush_period_in_ms · allow_auto_snapshot ·
incremental_backups · flags · id · extensions
```

Leading with what silently costs data or latency: what the table is for, the TTL
that deletes rows out from under you, the tombstone window, and the compaction
strategy that decides the table's physical behaviour. Trailing with what gets
looked up once a year. Anything unranked — an older cluster's columns, a newer
cluster's — sorts after all of these, alphabetically, so nothing is ever hidden
by not being in the list.

This order is a first guess to be lived with, not a considered answer; §7 of
this document is where it gets revisited.

## 4. Adapter — `CqlValue` to text

Driver types stop at the adapter (architecture.md §5), so each value is rendered
to a `String` there:

| kind | rendered as |
|---|---|
| text, ascii, numbers, booleans, uuid | bare — `864000`, `false`, the comment as prose |
| map, set, list | CQL literal form, entries sorted by key — `{'class': 'org.apache…SizeTieredCompactionStrategy', 'max_threshold': '32'}` |
| blob | `0x` and lowercase hex, so `extensions` is showable rather than skipped |
| null | `null` |
| anything else | its `Debug` form |

Scalars bare and collections delimited is a deliberate mix rather than an
oversight: a comment reads as prose and gains nothing from quotes, while a
collection is unparseable by eye without its braces. Strings *inside* a
collection are quoted, with `'` doubled, because that is where they need
delimiting.

Map entries are sorted by their rendered key. `CqlValue::Map` is a `Vec`, so
without that sort the panel's contents would depend on the order a coordinator
happened to serialise a frozen map in (conventions.md §4).

The `Debug` fallback is not laziness: `CqlValue` is `#[non_exhaustive]`, so a
wildcard arm is mandatory, and the same escape hatch already covers
`Strategy`'s unknown variants in `replication`.

## 5. View model

```rust
pub enum PanelBody {
    Blank,
    Notice { text, token },
    Tree { rows, selected },
    Details(Details),          // new
}

pub struct Details {
    /// `shop.orders_by_customer`, or a keyspace name.
    pub heading: String,
    pub sections: Vec<Section>,
}

pub struct Section {
    /// `PARTITION`, `CLUSTERING`, `OPTIONS`, `REPLICATION`.
    pub heading: String,
    /// Styles the heading and every row beneath it.
    pub token: Option<Token>,
    pub rows: Vec<DetailRow>,
}

pub enum DetailRow {
    /// Name and CQL type on one line, the type pushed to the right margin.
    Column { name: String, order: Option<ClusteringOrder>, cql_type: String },
    /// Name on its own line, value indented and wrapped beneath it.
    Property { name: String, value: String },
}
```

`Token::PartitionKey` and `Token::ClusteringKey` have sat unused in `style.rs`
since the theme work; this is what they were for. The domain names the
clustering *direction* and `body.rs` picks `↑`/`↓`, the same division that
already maps `Marker` to `▸`/`▾` (architecture.md §6).

Sections rather than a flat row list with blank-line markers: vertical rhythm
then cannot be forgotten or emitted twice, and a section with no rows — a table
with no clustering columns — disappears by being empty rather than by a
projection remembering to skip it.

One `Details` serves the keyspace view too, so the panel has one body kind and
one renderer rather than two that drift.

## 6. Rendering

```
┌─Table DDL─────────────┐
│shop.orders_by_customer│
│                       │
│PARTITION              │
│  customer_id     uuid │
│CLUSTERING             │
│  order_date ↓    date │
│  order_id ↑  timeuuid │
│OPTIONS                │
│comment                │
│  Order history for    │
│  one customer, newes… │
│default_time_to_live   │
│  0                    │
│gc_grace_seconds       │
│  864000               │
└───────────────────────┘
```

One blank line after the panel heading and none after that: section headings sit
unindented and styled, which separates the groups without spending a line on
each. `OPTIONS` exists as a heading rather than an implicit trailing block
because vision.md §4, principle 8 asks every dense view to carry a label.

**Full property names, never abbreviated.** Principle 8's other half is that
every abbreviation is expandable, and there is no expansion mechanism yet — so
the honest move is to spend the width rather than owe the user a decoder. A
value therefore goes on its own indented line, which is also where the long ones
need the room.

**Values wrap, they do not truncate.** `compaction` is around 120 characters and
the panel is about twenty wide. Truncation would hide that data permanently,
because there is no scroll and no expansion; wrapping shows all of it down to
the bottom of the panel. The cost is real — four map-valued properties eat
roughly fifteen lines between them — and it is the reason §3's ordering carries
so much weight.

A single token longer than the panel is the one place `…` appears, since nothing
else can be done with it.

A column row puts its type at the right margin when both fit, and drops the type
to an indented line beneath the name when they do not — the same shape a
property already has, rather than a third rule.

## 7. What this does not do

**The panel does not scroll.** At 100×100 it holds 38 rows and reaches roughly
`compression`; at 80×24 it holds **seven**, which is the heading and the
partition key. Scrolling needs the panel to be focusable and focus movement
between panels does not exist at all yet, so building it here would be a second
slice wearing this one's clothes. The importance ordering is what carries the
weight in the meantime — and living with that for a few weeks is exactly how the
ordering earns its revision.

TODO carries this, the deferred column list, and the widened DDL race.

## 8. Tests

**Domain.** The ranking, unranked names falling to the end alphabetically, and
empty options. The projection for a table (heading, key sections, options in
order), for a keyspace (replication class and factors), for a table with no
clustering columns (no `CLUSTERING` section), and for every state with no
selection (`Blank`).

**Adapter.** The value formatter per kind, including blob hex, quoting inside
collections, map entries sorted by key, and `null`. A row missing a column an
older cluster would not have, yielding fewer properties rather than an error.

**Scenario.** Connect → describe → `l` → `j` onto a table, asserting the Ddl
panel's sections; and back onto the keyspace, asserting the replication view.

**Render.** A `TestBackend` snapshot guarding the indent, the right-aligned
type, `↑`/`↓`, the section headings and value wrapping.

**Integration.** Against the 5.0.8 fixture: `shop.orders_by_customer` reports
the comment the seed set, `gc_grace_seconds` of 864000 and a
`SizeTieredCompactionStrategy` compaction class; every fixture table reports a
non-empty option set.

## 9. What this closes and opens

Closes the table-list spec's deferral of the DDL panel, and spends
`Token::PartitionKey` and `Token::ClusteringKey` for the first time — vision.md
§2's commitment now has a place on screen that is always showing the selected
table's physical shape.

Opens: panel focus and scrolling, which §7 makes newly load-bearing; the column
list; and whether `Details` is the shape the Contents and Queries panels want
too, which is the first evidence architecture.md §11's per-panel view model
question has had.
