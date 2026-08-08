# Table contents — design

Fills the Contents panel with a table's rows, and with it opens the first
end-to-end query path: a keypress becomes an `Effect::Execute`, a driver page
becomes a `ViewModel`, and a ratatui `Table` draws it.

**Status:** implemented.
**Date:** 2026-08-09

---

## 1. Scope

`Enter` on a table row in the Tables tree runs a `SELECT` and the Contents
panel draws the result: header of column names, one row per row.

In scope: a focus model, so the panel can be reached at all; the statement and
its posture gate; a `ContentsPanel` holding the request and its result;
`CassandraCluster::execute`, which has been returning "not implemented yet"
since the port was defined; mapping the driver's values into our `CqlValue`;
a `Grid` body in the view model; and the renderer.

Out of scope, and deliberately: paging — `Page::next` is stored and never read;
cell selection; filters; sorting; horizontal scrolling and column priority;
editing; and the Ddl and Queries panels, which stay `Blank`.

## 2. Reachability comes first

The shell has had no way to move focus. `j`/`k` are gated on
`focus == Panel::Tables` and nothing ever changes `focus`, so a result of more
rows than fit in the panel would have had rows nobody could reach. A grid you
can only see the top of is not a grid.

So this slice adds the smallest focus model that makes rows reachable:

| key | effect |
|---|---|
| `Tab` | the next panel in `Panel::ALL` order, wrapping |
| `Shift-Tab` | the previous one, wrapping |

`Key::Tab` and `Key::BackTab` already exist in the domain's input vocabulary
and `lc-inbound` already translates them, so this costs two match arms.
crossterm delivers Shift-Tab as `BackTab` with the shift modifier set, and
`handle_key` refuses ctrl and alt but deliberately not shift, so it passes the
existing gate unchanged.

`j`/`k` stop belonging to the Tables panel and route by focus instead: Tables
moves the tree cursor, Contents moves the row cursor, Ddl and Queries have no
rows and do nothing. The tree transitions — `h`, `l`, `Enter`, `Space` — stay
gated to Tables.

Direct jumps (`1`–`4`, which is lazygit's actual model and a better fit for
vision.md §4 principle 4) are **not** here. They spend four keys and deserve
their own spec; cycling is what makes rows reachable, and nothing about it has
to be taken back when jumps land.

## 3. The statement

`Enter` on a `Node::Table` builds:

```sql
SELECT * FROM "shop"."customers_by_id" LIMIT 200
```

**Identifiers are always double-quoted**, with any embedded `"` doubled.
Cassandra folds an unquoted identifier to lower case, and `system_schema`
reports names in their true form — so quoting is the only rendering that is
correct for a keyspace named `MyKs`, and it costs two characters of the CQL
that vision.md §4 principle 3 commits to showing.

**The bound is `LIMIT` in the CQL, not a page size on the request.** What is on
screen is exactly what the shown statement asked for; a page size would put the
real bound somewhere the user cannot see. The cost is that paging later has to
replace the `LIMIT` with a cursor rather than merely start reading
`Page::next`, and that is the right trade while the CQL is the teaching
surface and paging is not yet built.

`StatementKind::Read`, so `submit`'s posture gate admits it on a read-only
connection — which is the default posture and the one this panel will spend
almost all its life under.

## 4. Domain — `lc-domain/src/contents.rs`

A module of its own, mirroring `tree.rs`. That is architecture.md §11's "does
`State` stay one struct?" answered one panel further: per-panel state with its
own vocabulary, not another field on `State`.

```rust
pub struct ContentsPanel {
    pub cursor: Cursor,
    state: ContentsState,
}

pub enum ContentsState {
    Empty,
    Loading { query: QueryId, table: TableRef },
    Loaded  { table: TableRef, page: Page },
    Failed  { table: TableRef, error: DbError },
}

/// Which table a request was about.
pub struct TableRef { pub keyspace: String, pub table: String }
```

The `QueryId` lives *inside* `Loading` rather than beside it, so "loading with
no outstanding query" is unrepresentable. `state` is private for the reason
`TablesPanel::unfolded` is: the only ways in are the transitions, and
`state()` hands out a shared reference for the projection to read.

`TableRef` is owned rather than a borrow of the `Schema` because a result
outlives the schema reload that dropped its table. A page whose table has
vanished still draws, still says which table it was, and is simply stale — which
is honest, where a dangling borrow would not compile and a silent clear would
lose the user's place.

### Two gates, not one

`ContentsPanel::page_ready` applies a page only when the id matches the
outstanding request. `State::in_flight` separately drops results for a query
that was cancelled. They answer different questions — *is this mine* and *is
anyone still waiting* — and both are needed the moment a second query source
(the Queries panel) exists.

### Replacing a request cancels the one it replaces

`Enter` on a second table removes the outstanding id from `in_flight` and emits
`Effect::Cancel` before submitting. `Effect::Cancel` is still inert in the
effect runtime — the `Cluster` port has no cancel method — so the `in_flight`
removal is what actually stops a late page from being applied. That is
pre-existing (architecture.md §3 describes the cancel token as the plan) and
this slice does not close it.

### Three signature changes

- **`submit` returns the `QueryId`**: `Result<(QueryId, Vec<Effect>), DbError>`.
  The panel has to record the id it is waiting for, and reading it back out of
  the returned `Effect::Execute` would work and would be worse. `submit` has no
  caller outside `update.rs`, so this is free.
- **`Effect::Execute` and `Effect::FetchNextPage` carry `conn: ConnId`.** The
  effect runtime has to call `cluster.execute(conn, …)` and had no id to pass.
  The domain knows which connection it is querying; the alternative is
  `lc-app` tracking a current connection behind the domain's back.
- **`Msg::PageReady` and `Msg::Failed` stop dropping their payload** and hand it
  to the panel, still behind the `in_flight` guard.

### A failed query does not raise the overlay

`Msg::SchemaFailed` raises one; `Msg::Failed` does not. A schema failure
disables the whole application, so it interrupts. A failed browse query
disables one panel, and an overlay per mistyped table is precisely the warning
noise vision.md §8 flags. The panel's own `Notice`, in the danger token, is
where it belongs — the same treatment the Tables panel already gives a schema
failure.

### History

`submit` also emits `Effect::RecordHistory`, so a browse `SELECT` will reach
the history store once one exists. That is deliberate rather than overlooked:
vision.md §4 principle 3 wants the session log to show every statement the tool
ran, not only the typed ones. The effect is inert today.

## 5. Adapter — `execute`

```rust
session.query_unpaged(&stmt.cql, &[])       // bounded by the LIMIT in §3
    .into_rows_result()
    .column_specs()  → Page::columns
    .rows::<scylla::value::Row>()  → Page::rows
```

`Page::next` is `None`: `query_unpaged` has no cursor to hand back, and with a
`LIMIT` there is nothing further to ask for.

### Values

`CqlValue`'s doc comment promised the long tail would arrive as `Text`
"carrying the driver's own rendering until a driver is chosen". A driver is
chosen, and it turns out scylla's `CqlValue` has no `Display` — so the adapter
writes that rendering. The enum is unchanged.

| our variant | driver variants |
|---|---|
| `Null` | a `None` column |
| `Boolean` | `Boolean` |
| `Int(i64)` | `TinyInt`, `SmallInt`, `Int`, `BigInt`, `Counter` |
| `Float(f64)` | `Float`, `Double` |
| `Text` | `Ascii`, `Text`, `Empty`, **and the whole tail, rendered** |
| `Blob` | `Blob` |
| `Uuid` | `Uuid`, `Timeuuid` |
| `List` / `Set` | `List`, `Vector` / `Set` |
| `Map` | `Map`, **when every key is textual** |
| `Timestamp` | *nothing — see below* |

The tail rendered to `Text` is `Decimal`, `Varint`, `Duration`, `Date`, `Time`,
`Inet`, `Tuple`, `UserDefinedType`, and whatever `#[non_exhaustive]` adds next.
The catch-all arm follows the precedent `replication()` already set for
`Strategy`: a variant we have never seen still gets a name.

Two consequences worth stating rather than discovering:

- **A map with non-textual keys arrives as `Text`, fully rendered.** Our `Map`
  is `BTreeMap<String, CqlValue>` and CQL map keys are any type. Rendering keys
  to strings to fit them in would let two distinct keys collide into one entry,
  which is a wrong answer where a rendered string is merely a coarse one.
- **`CqlValue::Timestamp` is produced by nothing.** Formatting epoch millis as
  a date needs a calendar, and `lc-domain` depends on std, serde, thiserror and
  async-trait — a list architecture.md §2 treats as load-bearing. So the
  adapter formats timestamps to ISO-8601 and ships them as `Text`. §9 records
  the unproduced variant as opened.

`Decimal` and `Varint` are arbitrary precision, and rendering them from
`as_signed_be_bytes_slice_and_exponent` means hand-writing two's-complement
bignum-to-decimal conversion. scylla's `bigdecimal-04` and `num-bigint-04`
features supply `From<CqlDecimal>` and `From<CqlVarint>` conversions to types
that already have `Display`, so those features are enabled on the `lc-outbound`
dependency instead. `chrono` — already in the tree as a non-optional dependency
of `scylla` — formats timestamps, dates and times.

This is the opposite call from the table-list spec's §3, which hand-read
`system_schema` rather than take a dependency. The distinction is what the code
would have to be correct about: six columns of a table stable since Cassandra
3.0 is a bounded, checkable amount of knowledge, and arbitrary-precision
arithmetic is not.

## 6. View and rendering

```rust
pub enum PanelBody {
    Blank,
    Notice { text: String, token: Option<Token> },
    Tree { rows: Vec<TreeRow>, selected: usize },
    Grid { columns: Vec<GridColumn>, rows: Vec<Vec<GridCell>>, selected: usize },
}

pub struct GridColumn { pub name: String, pub token: Option<Token> }
pub struct GridCell   { pub text: String, pub token: Option<Token> }
```

**Cell text is rendered in the domain**, from `CqlValue`. A blob is
`0x1f8b…`, a list is `[a, b, c]`, a set is `{a, b, c}`, a map is `{k: v}`, and
a null is the word `null` carrying `Token::Muted` — which is what keeps it
distinguishable from a text column whose value is literally `"null"`. The
token on a cell is the same device `TreeRow` already uses; the domain names the
meaning and the theme decides how to draw it (architecture.md §6).

**Header columns carry the key structure.** A column matched by name against
the selected table's `PrimaryKey` gets `Token::PartitionKey` or
`Token::ClusteringKey`; everything else gets `Token::RegularColumn`. Those
three tokens have existed in `style.rs` since the theme landed and nothing has
spent them until now. This is vision.md §2's central commitment at close to
zero cost: the physical shape of the table is visible in the grid you are
reading, not one panel away.

The table-list spec put the full key treatment in the DDL panel and kept the
sidebar to a bare name, because a 25-column sidebar row cannot carry more. The
Contents header is a different surface with the width to say it.

**Widths are the renderer's business**, for the reason scrolling already is:
the domain has no idea the panel is sixty columns wide. `body.rs` measures each
column as `max(header, widest cell)` in display width, caps it at 24, and
truncates anything longer to 23 plus `…`. Columns that fall past the right edge
are not drawn. Measurement is `unicode-width` — already in the tree via
ratatui — because a `chars().count()` width is wrong for the CJK text a
Cassandra cluster will certainly contain.

Vertical scrolling reuses the tree's device exactly: a `TableState` built per
frame from `selected`, no scroll state stored anywhere.

**The panel title says which table and how many rows** — `Table contents —
shop.orders (12 rows)`. The contents do not follow the tree cursor, so without
this the panel is ambiguous the moment you press `j`. vision.md §4 principle 8:
every dense view has a label.

```
┌Table contents — shop.customers_by_id (3 rows)──────────┐
│customer_id                          email          tier│
│8f14e45f-ceea-467a-9c2b-8c4d2a1e9f01 ada@example.c… gold│
│c9f0f895-fb98-4b1e-b5aa-3d0f7c6e2b14 bob@example.c… null│
│45c48cce-2e2d-4fbf-9d1a-6b7e3f8c0a25 cy@example.co… silv│
└────────────────────────────────────────────────────────┘
```

### What each state draws

| `ContentsState` | body |
|---|---|
| `Empty` | `Notice` — "Press Enter on a table", muted |
| `Loading` | `Notice` — "Loading rows…", muted |
| `Failed` | `Notice` — the error, danger |
| `Loaded`, no rows | `Notice` — "No rows", muted |
| `Loaded` | `Grid` |

"Press Enter on a table" rather than "Nothing selected": the empty state is the
one every user sees first, and vision.md §4 principle 1 wants the available
action on screen rather than in a manual.

## 7. Tests

**Domain.** `Tab` and `Shift-Tab` cycling `Panel::ALL` in both directions and
wrapping. `j`/`k` routed by focus, including that they no longer move the tree
when Contents is focused. `Enter` on a table emitting `Execute` with the exact
CQL of §3, and quoting a name that needs it. `Enter` on a keyspace still
toggling. The posture gate. A second `Enter` cancelling the first request. A
page applied only when its id matches, and dropped when it does not. A failure
landing in the panel and *not* in the overlay. The row cursor clamped by a
smaller page. Every `CqlValue` shape's rendered text, including the null token.
Header tokens matched against the primary key.

**Scenario.** `FakeCluster` gains a scripted page. Connect → describe → `l` →
`j` → `Enter` → assert the `ViewModel`'s `Grid`: columns, tokens, cell text.

**Render.** `TestBackend` snapshots: the header and two rows drawn where they
belong; a column capped and ellipsised; the selected row reversed; a page
longer than the panel scrolling to keep the selection visible; the key columns
styled.

**Integration.** Against the fixture, behind the `integration` feature. The
`typezoo` keyspace exists for exactly this:

- `all_scalar_types` — every scalar renders, and `col_decimal`, `col_varint`,
  `col_timestamp`, `col_date`, `col_time`, `col_inet` and `col_duration` come
  back as readable text rather than a `Debug` dump.
- `collection_types` — lists, sets, maps, frozen and nested.
- `udt_types` and `tuple_types` — rendered, not refused.
- `nulls_and_empties` — a null column arrives as `CqlValue::Null`, distinct
  from an empty string.
- `LIMIT` is respected, and `Page::columns` matches the table's own column
  order.

## 8. What this closes

The `Cluster` port's `execute`, unimplemented since it was defined. The first
`Effect` the runtime interprets beyond `Connect` and `LoadSchema`. The three
key-structure tokens `style.rs` declared and nothing used. And the Contents
panel, which has been `Blank` since the four-panel layout landed.

## 9. What this opens

- **Paging.** `LIMIT 200` is a wall, not a window. `Effect::FetchNextPage` and
  `Page::next` exist and nothing drives them; §3 records why the `LIMIT` is
  where it is and what replacing it costs.
- **`CqlValue::Timestamp` is produced by nothing** (§5). Either the domain
  gains a calendar and produces it, or the variant goes — an enum arm nothing
  constructs is a claim the model does not honour. The cell-inspection slice
  will have to pick.
- **A map with non-textual keys degrades to text** (§5). `Map` wanting to be
  `Vec<(CqlValue, CqlValue)>` is the honest fix and is a model change.
- **`Effect::Cancel` is still inert.** Replacing a request now depends on the
  `in_flight` drop alone, so a superseded query runs to completion on the
  cluster. The `Cluster` port needs a cancel before that is real.
- **Focus has no direct jumps** (§2), and the panels a `Tab` now reaches — Ddl
  and Queries — are still blank places.
- Still open from the table-list slice: a refresh binding, and where schema
  caching lives.
