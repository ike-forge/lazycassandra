# Keyspace list — design

Populates the Tables panel with the cluster's keyspaces, and closes
[architecture.md](../architecture.md) §8 by choosing a driver.

**Status:** approved, not implemented.
**Date:** 2026-08-08

---

## 1. Scope

The Tables panel is a tree: keyspaces at the root, tables under an unfolded
keyspace. This slice builds the root only.

In scope: choosing and wiring a CQL driver, connecting at startup, reading the
cluster's keyspaces, rendering them with a movable selection, and showing
connection and schema failures.

Out of scope, and deliberately: unfolding a keyspace into its tables,
`execute`/`fetch_page` and CQL value decoding, config file layering, TLS and
auth, reconnect, more than one connection, and focus movement between panels.

## 2. Driver

`scylla` 1.7.0, default features, using its metadata API.

`cassandra-cpp` is excluded by the non-negotiable in AGENTS.md — it is FFI over
the DataStax C++ driver — and its last release was 2024-06. That leaves
`scylla` against `cdrs-tokio` 9.0.2. Both are maintained, both are
`MIT OR Apache-2.0`, and both already satisfy `deny.toml`.

The difference that decides it is schema introspection.
`scylla::cluster::metadata` returns, as parsed types, very nearly the model
[`lc-domain/src/model/schema.rs`](../../crates/lc-domain/src/model/schema.rs)
already declares:

```
Keyspace { strategy, durable_writes, tables, views, user_defined_types }
Table    { columns, partition_key, clustering_key, partitioner }
Column   { typ, kind }
Strategy { SimpleStrategy, NetworkTopologyStrategy, LocalStrategy, Other }
```

`cdrs-tokio` has no metadata module at all. Every keyspace, table, column kind
and clustering order would be parsed out of `system_schema` by hand, and that
parsing would be ours to keep correct across Cassandra versions forever. The
same reasoning applies to paging later: `PagingState`/`PagingStateResponse` and
`execute_iter` map onto the `Cluster` port as it already stands.

The price is dependency hygiene. Resolved against the current workspace,
`scylla` introduces seven new duplicate version pairs where `cdrs-tokio`
introduces one:

| | new duplicates | total deps |
|---|---|---|
| `scylla` 1.7.0 | `darling`, `darling_core`, `darling_macro` 0.20/0.24; `foldhash` 0.1/0.2; `getrandom` 0.3/0.4; `hashbrown` 0.14.5; `socket2` 0.5/0.6 | 133 |
| `cdrs-tokio` 9.0.2 | `syn` 1.0.109 | 124 |

`deny.toml` bans multiple versions, so each becomes a `skip` entry carrying a
reason that names whose transitive choice it is — the style of the two entries
already there. Seven skip entries is a smaller and more visible cost than a
hand-written introspection layer.

`scylla`'s MSRV is 1.88, under the workspace's 1.97.1. Its `default` feature set
is empty; TLS lands with the openssl or rustls feature when auth is designed,
and the duplicate counts above are measured without it.

## 3. The adapter

`lc-outbound/src/cassandra.rs`:

```rust
pub struct CassandraCluster {
    conn: ConnId,
    contact_point: String,
    posture: Posture,
    session: tokio::sync::RwLock<Option<Session>>,
}
```

`Cluster::connect` carries a `ConnId` and no address, so the adapter has to
already know where that id points; `&self` is why the session sits behind a
lock. One target rather than a map is the honest match for a `State` that also
holds exactly one connection. `connect` and `describe` reject any other
`ConnId` with `DbError::Other`. When a second connection is designed, this
becomes a `BTreeMap<ConnId, _>` and the port grows an address — the shape of
that is better decided against a real second connection than guessed at now.

`describe` calls `session.get_cluster_state()` and maps `keyspaces_iter()` into
our `Schema`:

| scylla | ours |
|---|---|
| `Strategy::SimpleStrategy { replication_factor }` | `Replication::SimpleStrategy { factor }` |
| `Strategy::NetworkTopologyStrategy { datacenter_repfactors }` | `Replication::NetworkTopologyStrategy { factors }` |
| `Strategy::LocalStrategy`, `Strategy::Other { name, .. }` | `Replication::Other { class }` |

`LocalStrategy` collapsing into `Other` is the one lossy step, and it applies
only to `system`.

`Keyspace::tables` is mapped as empty. Filling it means turning scylla's
`partition_key`/`clustering_key` name lists and its `ColumnKind` map into our
`PrimaryKey`, which is the unfold slice's job and is not needed to draw a root.

`execute` and `fetch_page` return a typed `DbError` stating they are not
implemented. Decoding rows means covering the CQL type space, which belongs
with the Contents panel.

`scylla::cluster::metadata::Strategy` is `#[non_exhaustive]`, so the match
needs a fallback arm; it maps to `Replication::Other`.

## 4. Domain

### Keyspace kind

```rust
pub enum KeyspaceKind { User, System }
impl KeyspaceKind { pub fn of(name: &str) -> Self; }   // `system`, `system_*`
```

`Keyspace::kind()` delegates to it rather than the struct carrying a field, so
an adapter cannot set a kind that disagrees with the name.

Ordering is a domain rule, not a rendering one:

```rust
impl Schema {
    /// User keyspaces first, then system, each alphabetically.
    pub fn keyspaces_in_display_order(&self) -> impl Iterator<Item = &Keyspace>;
}
```

### Connection state

`Option<Connection>` cannot express "connecting" or "failed", and adding an
error field beside `schema: Option<Schema>` would make illegal combinations
representable:

```rust
pub enum ConnectionState { Disconnected, Connecting(ConnId), Failed(DbError), Connected(Connection) }
pub struct Connection { pub id: ConnId, pub posture: Posture, pub schema: SchemaState }
pub enum SchemaState { Loading, Failed(DbError), Loaded(Schema) }
```

`Connecting` carries the `ConnId` so a failed connect can be matched against
the attempt that is outstanding; without it, `ConnectFailed` would have nothing
to guard on. `ConnectionState::is_current(conn)` is that guard, and every
message uses it.

`State::posture()` yields a real posture only from `Connected`. Every other
variant is `ReadOnly`, which keeps the safe default structural rather than
remembered.

### Selection

```rust
pub struct Cursor { index: usize }   // private
impl Cursor {
    pub const fn index(self) -> usize;
    pub const fn down(&mut self, len: usize);
    pub const fn up(&mut self);
    pub const fn clamp_to(&mut self, len: usize);
}
```

The private field is the point: an out-of-range index cannot be constructed
from outside the module. It lives in `State` as `tables: TablesPanel`, a struct
rather than a bare field, because the unfold set lands there next and because
[architecture.md](../architecture.md) §11 already flags per-panel state as the
likely direction.

### Messages, effects, and `init`

`Msg::Failed` is keyed by `QueryId`, so a connect or describe failure has
nowhere to go today. Two messages are added:

```rust
Msg::ConnectFailed { conn: ConnId, error: DbError }
Msg::SchemaFailed  { conn: ConnId, error: DbError }
```

`update` gains an entry point so the startup connection is described in the
domain rather than performed behind its back:

```rust
pub fn init(conn: ConnId) -> (State, Vec<Effect>);   // Connecting, vec![Effect::Connect(conn)]
```

- `Connected` → `SchemaState::Loading`, emits `Effect::LoadSchema`.
- `SchemaLoaded` → `Loaded`, and clamps the cursor to the new row count.
- `ConnectFailed`, `SchemaFailed` → set their state and raise the overlay.

All four ignore a message whose `ConnId` is not the current one, the way
`SchemaLoaded` already does.

### Keys

`main.rs` currently rewrites `q` into `Msg::Quit` before calling `update`, with
a comment saying bindings belong in the domain. They move there now:

| key | effect |
|---|---|
| `j`, `Down` | Tables cursor down, when Tables has focus |
| `k`, `Up` | Tables cursor up, when Tables has focus |
| `Esc` | dismiss the overlay |
| `q` | quit |

## 5. View and rendering

```rust
pub struct ViewModel { pub panels: [PanelView; 4], pub focus: Panel, pub overlay: Option<Overlay> }
pub struct Overlay { pub title: String, pub message: String, pub severity: Severity }

pub struct PanelView { pub panel: Panel, pub title: String, pub body: PanelBody }
pub enum PanelBody {
    Blank,
    Notice(String),                                    // "Connecting…", "No keyspaces", errors
    Keyspaces { rows: Vec<KeyspaceRow>, selected: usize },
}
pub struct KeyspaceRow { pub name: String, pub token: Option<Token> }
```

`State::overlay` is a stored field set on failure and cleared by `Esc`, not
state derived from the connection — deriving it would make it impossible to
dismiss.

System rows carry `Some(Token::Muted)`; user rows carry `None`. The domain
names the meaning and the theme picks the pixels, which is
[architecture.md](../architecture.md) §6 unchanged.

One token is added: `Token::Selected`, which `TerminalDefault` maps to
`Modifier::REVERSED`. It sets no background, so the highlight inherits the
user's palette and the existing "no token sets a background" test still holds.

A keyspace row renders as `▸ shop`. The indicator is drawn now because the row
is a tree node whose children arrive next slice.

The overlay draws after the panels: `Clear` over a centered rect, then
`Block::bordered()` whose border style is
`theme.style(Token::Severity(Severity::Danger))`, with the message wrapped
inside. Width is capped at 60 columns and shrinks with the frame; a frame that
cannot hold a bordered block of 20×5 does not get one, which is the rule the
panels already follow. The failure is still readable as the Tables panel's
`Notice`, which is why that duplication earns its place.

Scrolling belongs to the renderer, not the domain — the domain does not know
the panel is eight rows tall. A `ratatui::widgets::List` with a `ListState`
built per frame from `selected` computes the offset itself, so no scroll state
is stored. The fixture's ten keyspaces in an eight-row panel already need this.

## 6. Wiring

`main.rs` reads `LC_CASSANDRA_HOST` (default `127.0.0.1`) and
`LC_CASSANDRA_PORT` (default `9042`) — the variable `docker/compose.yaml`
already uses — builds an `Arc<dyn Cluster>`, and starts from
`lc_domain::init(conn)` rather than `State::default()`.

Real config layering (vision.md §6) is a separate piece of Phase 0. Env vars
here are scaffolding for the fixture and are expected to be replaced by it, not
grown into a configuration mechanism.

`effects::run` takes the cluster and spawns tokio tasks for `Connect` and
`LoadSchema`, whose results re-enter through the existing channel. Every other
effect stays inert.

## 7. Tests

**Domain.** `Cursor` refuses to move past either end, and `clamp` pulls it in
when a shorter schema arrives. `init` emits exactly `Effect::Connect`.
`Connected` emits `LoadSchema` and sets `Loading`. Each failure message sets
its state and raises the overlay, and `Esc` clears it. All four new messages
ignore a stale `ConnId`. `KeyspaceKind::of` classifies `system`, `system_auth`
and a user name. Display order puts user keyspaces before system ones.

**Scenario.** `FakeCluster` gains a scripted failure mode; it can only succeed
today. Two sessions: connect → describe → assert the `ViewModel` rows and their
order, and connect → describe fails → assert the overlay.

**Render.** `TestBackend` snapshots of the Tables panel with user and system
rows and the selection on the second row, and of the overlay centered with a
red border.

**Integration.** Behind an `integration` feature on `lc-outbound`, which
`mise.toml` currently only guesses the name of. Against the fixture on
`LC_CASSANDRA_PORT`, `describe` returns `shop`, `typezoo` and `edge_cases` with
`NetworkTopologyStrategy`, `SimpleStrategy` and `NetworkTopologyStrategy`
respectively, and the system keyspaces sort after them.

## 8. What this closes and opens

Closes [architecture.md](../architecture.md) §8. The driver table there becomes
a decision with its reasoning, and `lc-outbound`'s "no CQL driver yet" comment
goes.

Opens, and left open: whether the `Cluster` port should carry an address rather
than a bare `ConnId`; whether schema caching lives in the domain or the adapter
(§11, untouched here because the schema is fetched once); and whether `State`
stays one struct, which `TablesPanel` starts to answer.
