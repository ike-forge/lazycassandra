# Keyspace List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fill the Tables panel with the connected cluster's keyspaces, selectable with `j`/`k`, and show connection and schema failures in a centered block.

**Architecture:** `lc-domain` grows a connection state machine (`Disconnected → Connecting → Connected`, each able to fail), a `Cursor` whose index cannot leave its list, and `PanelBody` so a `PanelView` can carry content instead of only a title. `lc-outbound` gains a `scylla`-backed `Cluster` implementation that maps `scylla::cluster::metadata` onto our `Schema`. `lc-app` connects at startup from env vars and interprets `Effect::Connect`/`Effect::LoadSchema` as tokio tasks.

**Tech Stack:** Rust 2024 edition, rustc 1.97.1, `scylla` 1.7.0, `ratatui` 0.30.2, `crossterm` 0.29.0, `tokio` 1.53.1, `cargo-nextest` 0.9.143, `cargo-deny` 0.20.2.

**Spec:** `doc/specs/2026-08-08-keyspace-list-design.md`

## Global Constraints

- **No `unsafe`, no FFI.** `unsafe_code = "forbid"` at the workspace root.
- **`lc-domain` depends on neither `ratatui` nor `crossterm` nor `tokio` nor any driver.** Only Task 9 touches a dependency list, and only `lc-outbound`'s and the workspace's.
- **Zero warnings and zero deprecations.** `mise run ci` (`fmt:check` + `lint` + `test` + `deny`) must be clean before any task is done. Clippy runs `--workspace --all-targets -- -D warnings` with `all` and `pedantic` enabled.
- **`#[must_use]` on public functions returning a value** — clippy `pedantic` enables `must_use_candidate`.
- **`Debug` on every public type** — `missing_debug_implementations = "warn"`.
- **`unwrap`/`expect` are permitted in tests only.** `clippy.toml` sets `allow-unwrap-in-tests = true` and `allow-expect-in-tests = true`; outside tests both are warnings. `anyhow` is permitted in `lc-app` only.
- **Public items in `lc-domain` carry `///` doc comments** (architecture.md §12). Implementation comments follow conventions.md §2: no narrating idiomatic Rust.
- **Unit tests are inline `#[cfg(test)] mod tests`**, matching `crates/lc-domain/src/update.rs`. Test names are full sentences; assertions carry messages where the failure would otherwise be cryptic. Tasks 9 and 10 add the workspace's first `tests/` directory, for container and whole-session tests that are not about any single module.
- **Determinism** (conventions.md §4): no clock, no network, no filesystem, no unseeded randomness, no `HashMap` iteration order in the fast tiers. Real I/O lives only in the `integration`-gated tier.
- **`Msg` and `Effect` derive `PartialEq`** and are asserted on directly. Any type reachable from them must keep deriving it.
- **Conventional Commits**, imperative, lowercase, no trailing period. Reference the spec section in the body when it explains *why*.
- **Never `git pull`, never `git push`.**

---

### Task 1: Classify and order keyspaces

**Files:**
- Modify: `crates/lc-domain/src/model/schema.rs`
- Modify: `crates/lc-domain/src/model/mod.rs:13-21` (the `pub use` block)
- Test: inline in `crates/lc-domain/src/model/schema.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `KeyspaceKind::{User, System}`, `KeyspaceKind::of(name: &str) -> KeyspaceKind`, `Keyspace::kind(&self) -> KeyspaceKind`, `Schema::keyspaces_in_display_order(&self) -> impl Iterator<Item = &Keyspace>`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/lc-domain/src/model/schema.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn keyspace(name: &str) -> Keyspace {
        Keyspace {
            name: name.to_owned(),
            replication: Replication::SimpleStrategy { factor: 1 },
            tables: BTreeMap::new(),
        }
    }

    fn schema(names: &[&str]) -> Schema {
        Schema {
            keyspaces: names
                .iter()
                .map(|name| ((*name).to_owned(), keyspace(name)))
                .collect(),
        }
    }

    #[test]
    fn the_clusters_own_keyspaces_are_system_and_the_system_prefixed_ones() {
        assert_eq!(KeyspaceKind::of("system"), KeyspaceKind::System);
        assert_eq!(KeyspaceKind::of("system_schema"), KeyspaceKind::System);
        assert_eq!(KeyspaceKind::of("system_virtual_schema"), KeyspaceKind::System);
    }

    #[test]
    fn a_user_keyspace_merely_starting_with_system_is_not_the_clusters() {
        // The separator is load-bearing: `systematic` is somebody's schema.
        assert_eq!(KeyspaceKind::of("systematic"), KeyspaceKind::User);
        assert_eq!(KeyspaceKind::of("shop"), KeyspaceKind::User);
    }

    #[test]
    fn a_keyspaces_kind_follows_its_name() {
        assert_eq!(keyspace("shop").kind(), KeyspaceKind::User);
        assert_eq!(keyspace("system_auth").kind(), KeyspaceKind::System);
    }

    #[test]
    fn display_order_puts_user_keyspaces_before_the_clusters_own() {
        let schema = schema(&["system_schema", "typezoo", "system", "shop"]);

        let order: Vec<&str> = schema
            .keyspaces_in_display_order()
            .map(|keyspace| keyspace.name.as_str())
            .collect();

        assert_eq!(order, ["shop", "typezoo", "system", "system_schema"]);
    }

    #[test]
    fn display_order_of_an_empty_schema_is_empty() {
        assert_eq!(Schema::default().keyspaces_in_display_order().count(), 0);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run --package lc-domain model::schema`
Expected: FAIL — `cannot find type KeyspaceKind in this scope`.

- [ ] **Step 3: Write the implementation**

Append to `crates/lc-domain/src/model/schema.rs`, above the test module:

```rust
/// Whether a keyspace is the cluster's own or somebody's schema.
///
/// Derived from the name rather than stored on [`Keyspace`], so an adapter
/// cannot report a kind that disagrees with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyspaceKind {
    User,
    System,
}

impl KeyspaceKind {
    /// Classifies a keyspace by name. The separator matters: `system_auth` is
    /// Cassandra's, `systematic` is not.
    #[must_use]
    pub fn of(name: &str) -> Self {
        if name == "system" || name.starts_with("system_") {
            Self::System
        } else {
            Self::User
        }
    }
}

impl Keyspace {
    /// Whether the cluster owns this keyspace or the user does.
    #[must_use]
    pub fn kind(&self) -> KeyspaceKind {
        KeyspaceKind::of(&self.name)
    }
}

impl Schema {
    /// User keyspaces first, then the cluster's own, each alphabetically.
    ///
    /// The alphabetical part is free: `keyspaces` is a `BTreeMap`, so two
    /// filtered passes are cheaper and clearer than partitioning into vectors.
    pub fn keyspaces_in_display_order(&self) -> impl Iterator<Item = &Keyspace> {
        let user = self
            .keyspaces
            .values()
            .filter(|keyspace| keyspace.kind() == KeyspaceKind::User);
        let system = self
            .keyspaces
            .values()
            .filter(|keyspace| keyspace.kind() == KeyspaceKind::System);

        user.chain(system)
    }
}
```

Add `KeyspaceKind` to the re-export in `crates/lc-domain/src/model/mod.rs`, keeping the list alphabetical:

```rust
pub use schema::{
    ClusteringColumn, ClusteringOrder, Column, Keyspace, KeyspaceKind, PrimaryKey, Replication,
    Schema, Table,
};
```

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo nextest run --package lc-domain model::schema && mise run lint`
Expected: 5 tests pass, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src/model/schema.rs crates/lc-domain/src/model/mod.rs
git commit -m "feat(domain): classify keyspaces and give them a display order

The Tables panel shows user keyspaces above the cluster's own. Kind is
derived from the name rather than stored, so an adapter cannot report one
that disagrees with it (spec §4)."
```

---

### Task 2: A cursor that cannot leave its list

**Files:**
- Modify: `crates/lc-domain/src/state.rs`
- Test: inline in `crates/lc-domain/src/state.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Cursor` (unit-struct-like, `Default`, `Copy`) with `index(self) -> usize`, `down(&mut self, len: usize)`, `up(&mut self)`, `clamp_to(&mut self, len: usize)`.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` in `crates/lc-domain/src/state.rs`:

```rust
    #[test]
    fn a_cursor_starts_on_the_first_row() {
        assert_eq!(Cursor::default().index(), 0);
    }

    #[test]
    fn moving_down_stops_on_the_last_row() {
        let mut cursor = Cursor::default();

        for _ in 0..5 {
            cursor.down(3);
        }

        assert_eq!(cursor.index(), 2, "a cursor must not run off the end");
    }

    #[test]
    fn moving_up_stops_on_the_first_row() {
        let mut cursor = Cursor::default();
        cursor.down(3);

        cursor.up();
        cursor.up();

        assert_eq!(cursor.index(), 0, "a cursor must not run off the start");
    }

    #[test]
    fn an_empty_list_leaves_the_cursor_on_the_first_row() {
        let mut cursor = Cursor::default();

        cursor.down(0);

        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn clamping_pulls_the_cursor_into_a_list_that_shrank() {
        let mut cursor = Cursor::default();
        for _ in 0..9 {
            cursor.down(10);
        }

        cursor.clamp_to(4);

        assert_eq!(cursor.index(), 3);
    }

    #[test]
    fn clamping_leaves_a_cursor_that_is_already_inside_alone() {
        let mut cursor = Cursor::default();
        cursor.down(10);

        cursor.clamp_to(10);

        assert_eq!(cursor.index(), 1);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run --package lc-domain state::`
Expected: FAIL — `cannot find type Cursor in this scope`.

- [ ] **Step 3: Write the implementation**

Add to `crates/lc-domain/src/state.rs`, above the `State` struct:

```rust
/// A selection into a list, which cannot be moved past either end.
///
/// The index is private on purpose: an out-of-range selection cannot be
/// constructed from outside this module, so a renderer never has to defend
/// against one (conventions.md §1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    index: usize,
}

impl Cursor {
    /// The selected row.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    /// Moves to the next row, stopping on the last one.
    pub const fn down(&mut self, len: usize) {
        let last = len.saturating_sub(1);
        if self.index < last {
            self.index += 1;
        } else {
            self.index = last;
        }
    }

    /// Moves to the previous row, stopping on the first one.
    pub const fn up(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    /// Pulls the selection back inside a list that shrank.
    pub const fn clamp_to(&mut self, len: usize) {
        let last = len.saturating_sub(1);
        if self.index > last {
            self.index = last;
        }
    }
}
```

- [ ] **Step 4: Run the tests and the gate**

Run: `cargo nextest run --package lc-domain state:: && mise run lint`
Expected: 6 new tests pass alongside the 2 existing ones, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src/state.rs
git commit -m "feat(domain): add a cursor whose index cannot leave its list

The private field is the point: an out-of-range selection is not
constructible, so the renderer never has to check one (spec §4)."
```

---

### Task 3: Make the connection a state machine

**Files:**
- Modify: `crates/lc-domain/src/state.rs`
- Modify: `crates/lc-domain/src/update.rs` (the `Msg::Connected` and `Msg::SchemaLoaded` arms, and the `connected` test helper)
- Modify: `crates/lc-domain/src/lib.rs:14-16` (the `pub use` block)
- Test: inline in `crates/lc-domain/src/state.rs`

**Interfaces:**
- Consumes: `Cursor` from Task 2.
- Produces: `ConnectionState::{Disconnected, Connecting(ConnId), Failed(DbError), Connected(Connection)}` with `is_current(&self, conn: ConnId) -> bool`; `Connection { id: ConnId, posture: Posture, schema: SchemaState }`; `SchemaState::{Loading, Failed(DbError), Loaded(Schema)}`; `TablesPanel { cursor: Cursor }`; `Overlay { title: String, message: String, severity: Severity }`; `State.connection: ConnectionState`, `State.tables: TablesPanel`, `State.overlay: Option<Overlay>`; `State::schema(&self) -> Option<&Schema>`.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` in `crates/lc-domain/src/state.rs`:

```rust
    fn connected(schema: SchemaState) -> State {
        State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::Writable,
                schema,
            }),
            ..State::default()
        }
    }

    #[test]
    fn a_state_that_is_not_connected_is_read_only() {
        for connection in [
            ConnectionState::Disconnected,
            ConnectionState::Connecting(ConnId::next()),
            ConnectionState::Failed(DbError::Timeout),
        ] {
            let state = State {
                connection,
                ..State::default()
            };

            assert_eq!(
                state.posture(),
                Posture::ReadOnly,
                "only a live connection may carry a writable posture"
            );
        }
    }

    #[test]
    fn a_connected_state_carries_its_own_posture() {
        assert_eq!(connected(SchemaState::Loading).posture(), Posture::Writable);
    }

    #[test]
    fn a_schema_is_only_visible_once_it_has_loaded() {
        assert!(connected(SchemaState::Loading).schema().is_none());
        assert!(
            connected(SchemaState::Failed(DbError::Timeout))
                .schema()
                .is_none()
        );
        assert!(
            connected(SchemaState::Loaded(Schema::default()))
                .schema()
                .is_some()
        );
        assert!(State::default().schema().is_none());
    }

    #[test]
    fn only_the_outstanding_connection_is_current() {
        let conn = ConnId::next();
        let other = ConnId::next();

        assert!(ConnectionState::Connecting(conn).is_current(conn));
        assert!(!ConnectionState::Connecting(conn).is_current(other));
        assert!(!ConnectionState::Disconnected.is_current(conn));
        assert!(!ConnectionState::Failed(DbError::Timeout).is_current(conn));
    }
```

The test module's `use super::*;` already covers the new names; add `use crate::model::{ConnId, DbError, Schema};` and `use crate::posture::Posture;` to the test module only if the file's top-level imports do not already bring them in.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run --package lc-domain`
Expected: FAIL — `cannot find type ConnectionState in this scope`.

- [ ] **Step 3: Write the implementation**

In `crates/lc-domain/src/state.rs`, replace the `State` struct and the `Connection` struct, and extend the imports:

```rust
use crate::model::{ConnId, DbError, QueryId, Schema, Statement};
use crate::posture::Posture;
use crate::style::Severity;

#[derive(Debug, Clone, Default)]
pub struct State {
    pub connection: ConnectionState,
    pub focus: Panel,
    pub tables: TablesPanel,
    /// A message drawn over the panels until it is dismissed.
    pub overlay: Option<Overlay>,
    /// Queries issued and not yet resolved. A result whose id is absent here
    /// was cancelled, and is dropped rather than applied.
    pub in_flight: BTreeMap<QueryId, Statement>,
    /// Set when something changed that the next frame must show. Avoids
    /// repainting on every keypress of a held-down `j` (architecture.md §3).
    pub dirty: bool,
    pub should_quit: bool,
}

/// How far along the one connection is.
///
/// `Option<Connection>` could not say "connecting" or "failed", and an error
/// field beside the schema would make illegal combinations representable.
#[derive(Debug, Clone, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    /// Carries the id so a failure can be matched to the attempt that is
    /// outstanding.
    Connecting(ConnId),
    Failed(DbError),
    Connected(Connection),
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub id: ConnId,
    pub posture: Posture,
    pub schema: SchemaState,
}

/// Whether the connected cluster has told us its shape yet.
#[derive(Debug, Clone, Default)]
pub enum SchemaState {
    #[default]
    Loading,
    Failed(DbError),
    Loaded(Schema),
}

/// The Tables panel's own state. A struct rather than a bare field because
/// the unfold set lands here next (architecture.md §11).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TablesPanel {
    pub cursor: Cursor,
}

/// A message drawn over the panels, dismissed with `Esc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlay {
    pub title: String,
    pub message: String,
    pub severity: Severity,
}

impl ConnectionState {
    /// Whether `conn` is the connection this state is about.
    ///
    /// The guard that drops results belonging to a connection the user has
    /// already left (architecture.md §3).
    #[must_use]
    pub fn is_current(&self, conn: ConnId) -> bool {
        match self {
            Self::Connecting(id) => *id == conn,
            Self::Connected(connection) => connection.id == conn,
            Self::Disconnected | Self::Failed(_) => false,
        }
    }
}
```

Replace the `impl State` block:

```rust
impl State {
    /// Posture of the current connection, or the safe default when there is
    /// none.
    #[must_use]
    pub const fn posture(&self) -> Posture {
        match &self.connection {
            ConnectionState::Connected(connection) => connection.posture,
            ConnectionState::Disconnected
            | ConnectionState::Connecting(_)
            | ConnectionState::Failed(_) => Posture::ReadOnly,
        }
    }

    /// The cluster's shape, once it has arrived.
    #[must_use]
    pub const fn schema(&self) -> Option<&Schema> {
        match &self.connection {
            ConnectionState::Connected(Connection {
                schema: SchemaState::Loaded(schema),
                ..
            }) => Some(schema),
            _ => None,
        }
    }

    pub const fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}
```

In `crates/lc-domain/src/update.rs`, rewrite the two arms that touch `state.connection`:

```rust
        Msg::Connected { conn, posture } => {
            state.connection = ConnectionState::Connected(crate::state::Connection {
                id: conn,
                posture,
                schema: SchemaState::Loading,
            });
            state.mark_dirty();
            vec![Effect::LoadSchema(conn)]
        }

        Msg::SchemaLoaded { conn, schema } => {
            let applied = match &mut state.connection {
                ConnectionState::Connected(connection) if connection.id == conn => {
                    connection.schema = SchemaState::Loaded(*schema);
                    true
                }
                _ => false,
            };
            if applied {
                state.mark_dirty();
            }
            Vec::new()
        }
```

Add `use crate::state::{ConnectionState, SchemaState, State};` to `update.rs`'s imports.

Fix the `connected` helper in `update.rs`'s test module:

```rust
    fn connected(posture: Posture) -> State {
        State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture,
                schema: SchemaState::Loading,
            }),
            ..State::default()
        }
    }
```

and rewrite `schema_for_a_stale_connection_is_ignored`'s assertion, which can no longer unwrap an `Option`:

```rust
        let ConnectionState::Connected(connection) = state.connection else {
            panic!("the connection is still present");
        };
        assert!(
            matches!(connection.schema, SchemaState::Loading),
            "schema from a connection we are no longer on must not be applied"
        );
```

Extend the re-export in `crates/lc-domain/src/lib.rs`:

```rust
pub use state::{Connection, ConnectionState, Cursor, Overlay, SchemaState, State, TablesPanel};
```

- [ ] **Step 4: Run the tests and the gate**

Run: `mise run test && mise run lint`
Expected: all domain tests pass, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src
git commit -m "refactor(domain): make the connection a state machine

Option<Connection> could not express connecting or failed, and an error
field beside the schema would have made illegal combinations
representable. Connecting carries its ConnId so a failure has something to
be matched against (spec §4)."
```

---

### Task 4: Start connecting, and record what fails

**Files:**
- Modify: `crates/lc-domain/src/update.rs`
- Modify: `crates/lc-domain/src/lib.rs:16` (the `update` re-export)
- Test: inline in `crates/lc-domain/src/update.rs`

**Interfaces:**
- Consumes: `ConnectionState`, `SchemaState`, `Overlay`, `TablesPanel`, `Cursor` from Tasks 2–3; `Schema::keyspaces` from Task 1.
- Produces: `lc_domain::init(conn: ConnId) -> (State, Vec<Effect>)`; `Msg::ConnectFailed { conn: ConnId, error: DbError }`; `Msg::SchemaFailed { conn: ConnId, error: DbError }`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/lc-domain/src/update.rs`'s test module:

```rust
    use crate::model::{Keyspace, Replication};
    use crate::state::Overlay;
    use crate::style::Severity;
    use std::collections::BTreeMap;

    fn schema_of(names: &[&str]) -> Schema {
        Schema {
            keyspaces: names
                .iter()
                .map(|name| {
                    (
                        (*name).to_owned(),
                        Keyspace {
                            name: (*name).to_owned(),
                            replication: Replication::SimpleStrategy { factor: 1 },
                            tables: BTreeMap::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn the_application_starts_by_connecting() {
        let conn = ConnId::next();

        let (state, effects) = init(conn);

        assert_eq!(effects, vec![Effect::Connect(conn)]);
        assert!(state.connection.is_current(conn));
        assert!(state.dirty, "the first frame must be painted");
    }

    #[test]
    fn connecting_successfully_asks_for_the_schema() {
        let conn = ConnId::next();
        let (mut state, _) = init(conn);

        let effects = update(
            &mut state,
            Msg::Connected {
                conn,
                posture: Posture::ReadOnly,
            },
        );

        assert_eq!(effects, vec![Effect::LoadSchema(conn)]);
        assert!(state.schema().is_none(), "the schema is still loading");
    }

    #[test]
    fn a_failed_connection_is_recorded_and_raised() {
        let conn = ConnId::next();
        let (mut state, _) = init(conn);

        let effects = update(
            &mut state,
            Msg::ConnectFailed {
                conn,
                error: DbError::Unreachable("no route".to_owned()),
            },
        );

        assert!(effects.is_empty());
        assert!(matches!(state.connection, ConnectionState::Failed(_)));
        let overlay = state.overlay.expect("a failure is shown");
        assert_eq!(overlay.severity, Severity::Danger);
        assert!(overlay.message.contains("no route"));
    }

    #[test]
    fn a_failed_connection_for_a_stale_attempt_is_dropped() {
        let (mut state, _) = init(ConnId::next());

        update(
            &mut state,
            Msg::ConnectFailed {
                conn: ConnId::next(),
                error: DbError::Timeout,
            },
        );

        assert!(state.overlay.is_none());
        assert!(matches!(state.connection, ConnectionState::Connecting(_)));
    }

    #[test]
    fn a_failed_schema_is_recorded_and_raised() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);

        update(
            &mut state,
            Msg::SchemaFailed {
                conn,
                error: DbError::Invalid("unauthorized".to_owned()),
            },
        );

        assert!(state.overlay.is_some());
        let ConnectionState::Connected(connection) = &state.connection else {
            panic!("still connected");
        };
        assert!(matches!(connection.schema, SchemaState::Failed(_)));
    }

    #[test]
    fn a_loaded_schema_pulls_the_selection_back_inside_it() {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        for _ in 0..5 {
            state.tables.cursor.down(6);
        }

        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(&["shop", "typezoo"])),
            },
        );

        assert_eq!(
            state.tables.cursor.index(),
            1,
            "a selection past the end of a smaller schema must be pulled in"
        );
    }

    fn current_conn(state: &State) -> ConnId {
        let ConnectionState::Connected(connection) = &state.connection else {
            panic!("the helper builds a connected state");
        };
        connection.id
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run --package lc-domain update::`
Expected: FAIL — `cannot find function init in this scope`.

- [ ] **Step 3: Write the implementation**

In `crates/lc-domain/src/update.rs`, add the two message variants after `Msg::Connected`:

```rust
    ConnectFailed {
        conn: ConnId,
        error: DbError,
    },
    SchemaFailed {
        conn: ConnId,
        error: DbError,
    },
```

Add `init` above `update`:

```rust
/// The state the application starts in, and what it must do first.
///
/// TEA's `init`: the startup connection is *described* here rather than
/// performed behind the domain's back, so the first frame can already say
/// "Connecting…" (architecture.md §3).
#[must_use]
pub fn init(conn: ConnId) -> (State, Vec<Effect>) {
    let state = State {
        connection: ConnectionState::Connecting(conn),
        dirty: true,
        ..State::default()
    };

    (state, vec![Effect::Connect(conn)])
}
```

Guard `Msg::Connected` and add the two failure arms:

```rust
        Msg::Connected { conn, posture } => {
            if !state.connection.is_current(conn) {
                return Vec::new();
            }
            state.connection = ConnectionState::Connected(crate::state::Connection {
                id: conn,
                posture,
                schema: SchemaState::Loading,
            });
            state.mark_dirty();
            vec![Effect::LoadSchema(conn)]
        }

        Msg::ConnectFailed { conn, error } => {
            if !state.connection.is_current(conn) {
                return Vec::new();
            }
            state.overlay = Some(Overlay {
                title: "Connection failed".to_owned(),
                message: error.to_string(),
                severity: Severity::Danger,
            });
            state.connection = ConnectionState::Failed(error);
            state.mark_dirty();
            Vec::new()
        }

        Msg::SchemaFailed { conn, error } => {
            let applied = match &mut state.connection {
                ConnectionState::Connected(connection) if connection.id == conn => {
                    connection.schema = SchemaState::Failed(error.clone());
                    true
                }
                _ => false,
            };
            if applied {
                state.overlay = Some(Overlay {
                    title: "Schema unavailable".to_owned(),
                    message: error.to_string(),
                    severity: Severity::Danger,
                });
                state.mark_dirty();
            }
            Vec::new()
        }
```

Extend `Msg::SchemaLoaded` to clamp the cursor:

```rust
        Msg::SchemaLoaded { conn, schema } => {
            let rows = schema.keyspaces.len();
            let applied = match &mut state.connection {
                ConnectionState::Connected(connection) if connection.id == conn => {
                    connection.schema = SchemaState::Loaded(*schema);
                    true
                }
                _ => false,
            };
            if applied {
                state.tables.cursor.clamp_to(rows);
                state.mark_dirty();
            }
            Vec::new()
        }
```

Add `use crate::state::Overlay;` and `use crate::style::Severity;` to `update.rs`'s imports, and extend the re-export in `crates/lc-domain/src/lib.rs`:

```rust
pub use update::{Effect, Msg, init, update};
```

- [ ] **Step 4: Run the tests and the gate**

Run: `mise run test && mise run lint`
Expected: all domain tests pass, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src
git commit -m "feat(domain): connect on init and record connection failures

Msg::Failed is keyed by QueryId, so a connect or describe failure had
nowhere to go. Adds ConnectFailed and SchemaFailed, both guarded on the
outstanding ConnId, and clamps the selection when a smaller schema arrives
(spec §4)."
```

---

### Task 5: Move the keybindings into the domain

**Files:**
- Modify: `crates/lc-domain/src/update.rs`
- Modify: `crates/lc-app/src/main.rs:36-46` (delete the `q` rewrite)
- Test: inline in `crates/lc-domain/src/update.rs`

**Interfaces:**
- Consumes: `init`, `Msg::SchemaLoaded` from Task 4; `Cursor` from Task 2; `State::schema` from Task 3.
- Produces: `update` handling `Msg::Input` — `j`/`Down` and `k`/`Up` move `state.tables.cursor` when `state.focus == Panel::Tables`, `Esc` clears `state.overlay`, `q` quits.

- [ ] **Step 1: Write the failing tests**

Add to `crates/lc-domain/src/update.rs`'s test module:

```rust
    use crate::model::{Key, KeyPress, Modifiers};
    use crate::state::Panel;

    fn browsing(keyspaces: &[&str]) -> State {
        let mut state = connected(Posture::ReadOnly);
        let conn = current_conn(&state);
        update(
            &mut state,
            Msg::SchemaLoaded {
                conn,
                schema: Box::new(schema_of(keyspaces)),
            },
        );
        state
    }

    fn press(state: &mut State, key: Key) -> Vec<Effect> {
        update(state, Msg::Input(KeyPress::new(key)))
    }

    #[test]
    fn j_and_k_walk_the_keyspace_list() {
        let mut state = browsing(&["a", "b", "c"]);

        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), 2);

        press(&mut state, Key::Char('k'));

        assert_eq!(state.tables.cursor.index(), 1);
    }

    #[test]
    fn the_arrow_keys_do_what_j_and_k_do() {
        let mut state = browsing(&["a", "b"]);

        press(&mut state, Key::Down);

        assert_eq!(state.tables.cursor.index(), 1);

        press(&mut state, Key::Up);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn the_selection_cannot_run_off_a_list_it_has_not_loaded() {
        let mut state = connected(Posture::ReadOnly);

        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "there are no rows to move onto"
        );
    }

    #[test]
    fn motion_belongs_to_the_focused_panel_only() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Queries;

        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn escape_dismisses_the_overlay() {
        let mut state = browsing(&["a"]);
        state.overlay = Some(Overlay {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        });

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
    }

    #[test]
    fn q_quits() {
        let mut state = browsing(&["a"]);

        let effects = press(&mut state, Key::Char('q'));

        assert!(state.should_quit);
        assert_eq!(effects, vec![Effect::Quit]);
    }

    #[test]
    fn a_modified_key_is_not_a_binding() {
        let mut state = browsing(&["a", "b"]);

        update(
            &mut state,
            Msg::Input(KeyPress {
                key: Key::Char('j'),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
            }),
        );

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "ctrl-j is a different binding, not a slower j"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run --package lc-domain update::`
Expected: FAIL — `j_and_k_walk_the_keyspace_list` asserts 2, gets 0.

- [ ] **Step 3: Write the implementation**

In `crates/lc-domain/src/update.rs`, replace the `Msg::Input(_) | Msg::Resized` arm with:

```rust
        Msg::Input(press) => handle_key(state, press),

        Msg::Resized => {
            state.mark_dirty();
            Vec::new()
        }
```

Replace the `Msg::Quit` arm's body with a call to a shared helper, and add the helpers below `update`:

```rust
        Msg::Quit => quit(state),
```

```rust
fn quit(state: &mut State) -> Vec<Effect> {
    state.should_quit = true;
    let cancels = state.in_flight.keys().copied().map(Effect::Cancel);

    cancels.chain(std::iter::once(Effect::Quit)).collect()
}

#[derive(Clone, Copy)]
enum Motion {
    Up,
    Down,
}

fn handle_key(state: &mut State, press: KeyPress) -> Vec<Effect> {
    state.mark_dirty();

    // Modified keys are their own bindings, not slower versions of the plain
    // ones (vision.md §4, principle 2).
    if press.modifiers.ctrl || press.modifiers.alt {
        return Vec::new();
    }

    match press.key {
        Key::Char('q') => return quit(state),
        Key::Escape => state.overlay = None,
        Key::Char('j') | Key::Down => move_selection(state, Motion::Down),
        Key::Char('k') | Key::Up => move_selection(state, Motion::Up),
        _ => {}
    }

    Vec::new()
}

fn move_selection(state: &mut State, motion: Motion) {
    if state.focus != Panel::Tables {
        return;
    }

    let rows = state.schema().map_or(0, |schema| schema.keyspaces.len());
    match motion {
        Motion::Down => state.tables.cursor.down(rows),
        Motion::Up => state.tables.cursor.up(),
    }
}
```

Add `use crate::model::{Key, KeyPress};` and `use crate::state::Panel;` to `update.rs`'s imports.

In `crates/lc-app/src/main.rs`, delete the `q`-rewriting block and pass the message straight through:

```rust
    while let Some(msg) = rx.recv().await {
        for effect in update(&mut state, msg) {
            effects::run(&tx, &effect);
        }
```

Remove the now-unused `use lc_domain::model::{Key, KeyPress};` from `main.rs`.

- [ ] **Step 4: Run the tests and the gate**

Run: `mise run test && mise run lint`
Expected: all tests pass, clippy clean. `Motion` is `Copy` and passed by value; a reference to a two-variant fieldless enum trips clippy's `trivially_copy_pass_by_ref`.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src/update.rs crates/lc-app/src/main.rs
git commit -m "feat(domain): own the keybindings

main.rs rewrote q into Msg::Quit with a comment saying bindings belong in
the domain; they do, and there are now four of them. j/k and the arrows
move the Tables selection, Esc dismisses the overlay (spec §4)."
```

---

### Task 6: Give panels a body

**Files:**
- Modify: `crates/lc-domain/src/view.rs`
- Modify: `crates/lc-domain/src/style.rs`
- Modify: `crates/lc-domain/src/lib.rs:17` (the `view` re-export)
- Test: inline in `crates/lc-domain/src/view.rs`

**Interfaces:**
- Consumes: `ConnectionState`, `SchemaState`, `Overlay`, `TablesPanel` from Tasks 3–4; `Schema::keyspaces_in_display_order`, `Keyspace::kind`, `KeyspaceKind` from Task 1.
- Produces: `PanelBody::{Blank, Notice(String), Keyspaces { rows: Vec<KeyspaceRow>, selected: usize }}`; `KeyspaceRow { name: String, token: Option<Token> }`; `PanelView.body: PanelBody`; `ViewModel.overlay: Option<Overlay>`; `Token::Selected`.

- [ ] **Step 1: Write the failing tests**

Add to `crates/lc-domain/src/view.rs`'s test module:

```rust
    use crate::model::{ConnId, DbError, Keyspace, Replication, Schema};
    use crate::posture::Posture;
    use crate::state::{Connection, ConnectionState, Overlay, SchemaState};
    use crate::style::{Severity, Token};
    use std::collections::BTreeMap;

    fn schema_of(names: &[&str]) -> Schema {
        Schema {
            keyspaces: names
                .iter()
                .map(|name| {
                    (
                        (*name).to_owned(),
                        Keyspace {
                            name: (*name).to_owned(),
                            replication: Replication::SimpleStrategy { factor: 1 },
                            tables: BTreeMap::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn state_with(connection: ConnectionState) -> State {
        State {
            connection,
            ..State::default()
        }
    }

    fn loaded(names: &[&str]) -> State {
        state_with(ConnectionState::Connected(Connection {
            id: ConnId::next(),
            posture: Posture::ReadOnly,
            schema: SchemaState::Loaded(schema_of(names)),
        }))
    }

    fn tables_body(state: &State) -> PanelBody {
        view_model(state).panels[0].body.clone()
    }

    #[test]
    fn the_tables_panel_says_what_it_is_waiting_for() {
        assert_eq!(
            tables_body(&State::default()),
            PanelBody::Notice("Not connected".to_owned())
        );
        assert_eq!(
            tables_body(&state_with(ConnectionState::Connecting(ConnId::next()))),
            PanelBody::Notice("Connecting…".to_owned())
        );
        assert_eq!(
            tables_body(&state_with(ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loading,
            }))),
            PanelBody::Notice("Loading schema…".to_owned())
        );
    }

    #[test]
    fn a_failure_is_readable_in_the_panel_and_not_only_in_the_overlay() {
        let body = tables_body(&state_with(ConnectionState::Failed(DbError::Timeout)));

        let PanelBody::Notice(text) = body else {
            panic!("a failed connection has nothing to list");
        };
        assert!(text.contains("timed out"));
    }

    #[test]
    fn keyspaces_are_listed_with_the_clusters_own_dimmed_and_last() {
        let state = loaded(&["system_schema", "typezoo", "shop"]);

        let PanelBody::Keyspaces { rows, selected } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            ["shop", "typezoo", "system_schema"]
        );
        assert_eq!(rows[0].token, None, "a user keyspace spends no token");
        assert_eq!(rows[2].token, Some(Token::Muted));
        assert_eq!(selected, 0);
    }

    #[test]
    fn the_selection_is_projected_from_the_cursor() {
        let mut state = loaded(&["shop", "typezoo"]);
        state.tables.cursor.down(2);

        let PanelBody::Keyspaces { selected, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its keyspaces");
        };

        assert_eq!(selected, 1);
    }

    #[test]
    fn a_cluster_with_no_keyspaces_says_so_rather_than_listing_nothing() {
        assert_eq!(
            tables_body(&loaded(&[])),
            PanelBody::Notice("No keyspaces".to_owned())
        );
    }

    #[test]
    fn the_other_three_panels_are_still_blank() {
        let view = view_model(&loaded(&["shop"]));

        for panel in &view.panels[1..] {
            assert_eq!(panel.body, PanelBody::Blank, "{:?} has no content yet", panel.panel);
        }
    }

    #[test]
    fn the_overlay_is_carried_to_the_renderer() {
        let mut state = State::default();
        state.overlay = Some(Overlay {
            title: "Connection failed".to_owned(),
            message: "no route".to_owned(),
            severity: Severity::Danger,
        });

        assert_eq!(view_model(&state).overlay, state.overlay);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run --package lc-domain view::`
Expected: FAIL — `cannot find type PanelBody in this scope`.

- [ ] **Step 3: Write the implementation**

Add `Selected` to `crates/lc-domain/src/style.rs`'s `Token`, after `Focused`:

```rust
    /// The row the cursor is on.
    Selected,
```

Rewrite `crates/lc-domain/src/view.rs`:

```rust
use crate::model::{KeyspaceKind, Schema};
use crate::state::{ConnectionState, Overlay, Panel, SchemaState, State};
use crate::style::Token;

/// Everything a renderer needs for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewModel {
    /// In [`Panel::ALL`] order.
    pub panels: [PanelView; 4],
    pub focus: Panel,
    /// Drawn over the panels when something needs saying.
    pub overlay: Option<Overlay>,
}

/// One panel, described without reference to how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub panel: Panel,
    pub title: String,
    pub body: PanelBody,
}

/// What a panel has to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelBody {
    Blank,
    /// A sentence where content would be: waiting, empty, or failed.
    Notice(String),
    Keyspaces {
        rows: Vec<KeyspaceRow>,
        selected: usize,
    },
}

/// One row of the keyspace list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyspaceRow {
    pub name: String,
    /// `Some(Token::Muted)` for the cluster's own keyspaces. A user keyspace
    /// spends no token: it is the baseline everything else is read against.
    pub token: Option<Token>,
}

/// Project `state` into the frame a renderer draws.
#[must_use]
pub fn view_model(state: &State) -> ViewModel {
    ViewModel {
        panels: Panel::ALL.map(|panel| PanelView {
            panel,
            title: title(panel).to_owned(),
            body: body(state, panel),
        }),
        focus: state.focus,
        overlay: state.overlay.clone(),
    }
}

const fn title(panel: Panel) -> &'static str {
    match panel {
        Panel::Tables => "Tables",
        Panel::Ddl => "Table DDL",
        Panel::Contents => "Table contents",
        Panel::Queries => "Queries",
    }
}

fn body(state: &State, panel: Panel) -> PanelBody {
    match panel {
        Panel::Tables => tables_body(state),
        Panel::Ddl | Panel::Contents | Panel::Queries => PanelBody::Blank,
    }
}

fn tables_body(state: &State) -> PanelBody {
    match &state.connection {
        ConnectionState::Disconnected => PanelBody::Notice("Not connected".to_owned()),
        ConnectionState::Connecting(_) => PanelBody::Notice("Connecting…".to_owned()),
        ConnectionState::Failed(error) => PanelBody::Notice(error.to_string()),
        ConnectionState::Connected(connection) => match &connection.schema {
            SchemaState::Loading => PanelBody::Notice("Loading schema…".to_owned()),
            SchemaState::Failed(error) => PanelBody::Notice(error.to_string()),
            SchemaState::Loaded(schema) => keyspaces(schema, state.tables.cursor.index()),
        },
    }
}

fn keyspaces(schema: &Schema, selected: usize) -> PanelBody {
    let rows: Vec<KeyspaceRow> = schema
        .keyspaces_in_display_order()
        .map(|keyspace| KeyspaceRow {
            name: keyspace.name.clone(),
            token: match keyspace.kind() {
                KeyspaceKind::User => None,
                KeyspaceKind::System => Some(Token::Muted),
            },
        })
        .collect();

    if rows.is_empty() {
        return PanelBody::Notice("No keyspaces".to_owned());
    }

    PanelBody::Keyspaces { rows, selected }
}
```

Keep the three existing tests in the file; `panels_carry_the_titles_the_draft_names` and the other two still compile unchanged.

Extend the re-export in `crates/lc-domain/src/lib.rs`:

```rust
pub use view::{KeyspaceRow, PanelBody, PanelView, ViewModel, view_model};
```

- [ ] **Step 4: Run the tests and the gate**

Run: `mise run test && mise run lint`
Expected: all domain tests pass; `lc-inbound` still compiles because it only reads `panel.title` and `panel.panel`.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src
git commit -m "feat(domain): project keyspaces into the Tables panel

A PanelView carried only a title. It now carries a body: a notice while
connecting, loading or failed, and the keyspace list once the schema is in.
The domain names the meaning; the theme picks the pixels (spec §5)."
```

---

### Task 7: Draw the keyspace list

**Files:**
- Create: `crates/lc-inbound/src/body.rs`
- Modify: `crates/lc-inbound/src/render.rs`
- Modify: `crates/lc-inbound/src/theme.rs`
- Modify: `crates/lc-inbound/src/lib.rs:11-14` (the module list)
- Test: inline in `crates/lc-inbound/src/render.rs` and `crates/lc-inbound/src/theme.rs`

**Interfaces:**
- Consumes: `PanelBody`, `KeyspaceRow`, `Token::Selected` from Task 6.
- Produces: `body::render(frame: &mut Frame, area: Rect, body: &PanelBody, theme: Theme)`, crate-private.

- [ ] **Step 1: Write the failing tests**

Add to `crates/lc-inbound/src/theme.rs`'s test module — and add `Token::Selected` to the array in the existing `no_token_sets_a_background` test:

```rust
    #[test]
    fn the_selected_row_is_marked_without_spending_a_colour() {
        let style = Theme::TerminalDefault.style(Token::Selected);

        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(
            style.fg, None,
            "the selection inverts the user's own palette rather than replacing it"
        );
    }
```

Add to `crates/lc-inbound/src/render.rs`'s test module:

```rust
    use lc_domain::state::{Connection, ConnectionState, SchemaState};
    use lc_domain::model::{ConnId, Keyspace, Replication, Schema};
    use lc_domain::posture::Posture;
    use std::collections::BTreeMap;

    fn schema_of(names: &[&str]) -> Schema {
        Schema {
            keyspaces: names
                .iter()
                .map(|name| {
                    (
                        (*name).to_owned(),
                        Keyspace {
                            name: (*name).to_owned(),
                            replication: Replication::SimpleStrategy { factor: 1 },
                            tables: BTreeMap::new(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn browsing(names: &[&str], selected: usize) -> State {
        let mut state = State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loaded(schema_of(names)),
            }),
            ..State::default()
        };
        for _ in 0..selected {
            state.tables.cursor.down(names.len());
        }
        state
    }

    fn draw_state(width: u16, height: u16, state: &State) -> Buffer {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| render(frame, &view_model(state), Theme::TerminalDefault))
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    #[test]
    fn keyspaces_are_listed_inside_the_tables_panel() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 0));

        assert_eq!(row(&buffer, 1), "│▸ shop        ││                      │");
        assert_eq!(row(&buffer, 2), "│▸ typezoo     ││                      │");
        assert_eq!(row(&buffer, 3), "│▸ system      ││                      │");
    }

    #[test]
    fn the_clusters_own_keyspaces_are_dimmed() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "system"], 0));
        let dim = |y: u16| {
            buffer
                .cell((1, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::DIM)
        };

        assert!(!dim(1), "shop is the user's");
        assert!(dim(2), "system is the cluster's");
    }

    #[test]
    fn the_selected_row_is_the_one_the_cursor_is_on() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo", "system"], 1));
        let reversed = |y: u16| {
            buffer
                .cell((1, y))
                .expect("inside the panel")
                .modifier
                .contains(Modifier::REVERSED)
        };

        assert!(!reversed(1));
        assert!(reversed(2), "typezoo is selected");
        assert!(!reversed(3));
    }

    #[test]
    fn a_panel_with_nothing_to_list_says_what_it_is_waiting_for() {
        let state = State {
            connection: ConnectionState::Connecting(ConnId::next()),
            ..State::default()
        };

        let buffer = draw_state(40, 12, &state);

        assert_eq!(row(&buffer, 1), "│Connecting…   ││                      │");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run --package lc-inbound`
Expected: FAIL — the list rows come back as blanks, and `Token::Selected` does not exist in the theme match.

- [ ] **Step 3: Write the implementation**

Add the arm to `crates/lc-inbound/src/theme.rs`'s `terminal_default`, beside `Token::Focused`:

```rust
            // Reversed rather than a background: it inverts whatever palette
            // the terminal already has instead of importing one.
            Token::Selected => Style::new().add_modifier(Modifier::REVERSED),
```

Create `crates/lc-inbound/src/body.rs`:

```rust
//! Drawing what a panel contains.
//!
//! Scrolling lives here rather than in the domain: the domain has no idea the
//! panel is eight rows tall. A `ListState` built per frame from the selected
//! index computes the offset, so no scroll state is stored anywhere.

use lc_domain::style::Token;
use lc_domain::view::{KeyspaceRow, PanelBody};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use crate::theme::Theme;

/// A keyspace is a tree node whose children arrive when it is unfolded.
const COLLAPSED: &str = "▸ ";

pub(crate) fn render(frame: &mut Frame, area: Rect, body: &PanelBody, theme: Theme) {
    if area.is_empty() {
        return;
    }

    match body {
        PanelBody::Blank => {}
        PanelBody::Notice(text) => notice(frame, area, text, theme),
        PanelBody::Keyspaces { rows, selected } => keyspaces(frame, area, rows, *selected, theme),
    }
}

fn notice(frame: &mut Frame, area: Rect, text: &str, theme: Theme) {
    frame.render_widget(
        Paragraph::new(text)
            .style(theme.style(Token::Muted))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn keyspaces(frame: &mut Frame, area: Rect, rows: &[KeyspaceRow], selected: usize, theme: Theme) {
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| {
            let item = ListItem::new(format!("{COLLAPSED}{}", row.name));
            match row.token {
                Some(token) => item.style(theme.style(token)),
                None => item,
            }
        })
        .collect();

    let list = List::new(items).highlight_style(theme.style(Token::Selected));
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(list, area, &mut state);
}
```

In `crates/lc-inbound/src/render.rs`, draw the body inside each block:

```rust
        let inner = block.inner(area);
        frame.render_widget(block, area);
        crate::body::render(frame, inner, &panel.body, theme);
```

replacing the bare `frame.render_widget(block, area);`.

Register the module in `crates/lc-inbound/src/lib.rs`:

```rust
mod body;
pub mod input;
pub mod layout;
pub mod render;
pub mod theme;
```

- [ ] **Step 4: Run the tests and the gate**

Run: `mise run test && mise run lint`
Expected: all tests pass once one existing assertion is updated. `the_four_panels_are_drawn_where_the_draft_puts_them` draws a `State::default()`, whose Tables body is now the `Not connected` notice, so change its row-1 assertion to:

```rust
        assert_eq!(row(&buffer, 1), "│Not connected ││                      │");
```

Rows 0, 7, 8, 9 and 11 are borders and do not change.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-inbound/src
git commit -m "feat(inbound): draw panel bodies and the keyspace list

Scrolling is the renderer's: a ListState built per frame from the selected
index computes the offset, so the domain never learns how tall the panel is.
Selection reverses rather than setting a background, so it inherits the
terminal's palette (spec §5)."
```

---

### Task 8: Draw the failure overlay

**Files:**
- Modify: `crates/lc-inbound/src/render.rs`
- Create: `crates/lc-inbound/src/overlay.rs`
- Modify: `crates/lc-inbound/src/lib.rs` (the module list)
- Test: inline in `crates/lc-inbound/src/overlay.rs`

**Interfaces:**
- Consumes: `ViewModel.overlay`, `Overlay` from Tasks 3 and 6.
- Produces: `overlay::render(frame: &mut Frame, overlay: &Overlay, theme: Theme)`, crate-private.

- [ ] **Step 1: Write the failing tests**

Create the test module at the bottom of `crates/lc-inbound/src/overlay.rs`:

```rust
#[cfg(test)]
mod tests {
    use lc_domain::state::{ConnectionState, Overlay};
    use lc_domain::style::Severity;
    use lc_domain::{State, view_model};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::Color;

    use crate::render::render;
    use crate::theme::Theme;

    fn draw(width: u16, height: u16, overlay: Option<Overlay>) -> Buffer {
        let state = State {
            connection: ConnectionState::Disconnected,
            overlay,
            ..State::default()
        };
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| render(frame, &view_model(&state), Theme::TerminalDefault))
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    fn failure() -> Overlay {
        Overlay {
            title: "Connection failed".to_owned(),
            message: "could not reach the cluster".to_owned(),
            severity: Severity::Danger,
        }
    }

    fn text(buffer: &Buffer) -> String {
        buffer.content().iter().map(Cell::symbol).collect()
    }

    #[test]
    fn a_failure_is_drawn_over_the_panels() {
        let buffer = draw(60, 20, Some(failure()));

        let drawn = text(&buffer);
        assert!(drawn.contains("Connection failed"));
        assert!(drawn.contains("could not reach the cluster"));
    }

    #[test]
    fn the_overlay_border_carries_the_severity() {
        let buffer = draw(60, 20, Some(failure()));

        let red = (0..buffer.area.height).any(|y| {
            (0..buffer.area.width).any(|x| {
                buffer
                    .cell((x, y))
                    .is_some_and(|cell| cell.fg == Color::Red && cell.symbol() == "─")
            })
        });

        assert!(red, "a danger overlay is bordered in red");
    }

    #[test]
    fn the_overlay_is_centred() {
        // At 60x20 the block is 56 wide (the frame less two columns of margin
        // each side, under the 60-column cap) and 5 tall (its floor, since the
        // message wraps to one line). That centres it at (2, 7).
        let buffer = draw(60, 20, Some(failure()));
        let symbol = |x, y| {
            buffer
                .cell((x, y))
                .expect("in bounds")
                .symbol()
                .to_owned()
        };

        assert_eq!(symbol(2, 7), "┌");
        assert_eq!(symbol(57, 7), "┐");
        assert_eq!(symbol(2, 11), "└");
        assert_eq!(symbol(57, 11), "┘");
    }

    #[test]
    fn a_frame_too_small_for_a_legible_overlay_does_not_get_one() {
        let buffer = draw(18, 4, Some(failure()));

        assert!(
            !text(&buffer).contains("Connection failed"),
            "a truncated error block is worse than none; the panel still says it"
        );
    }

    #[test]
    fn no_overlay_leaves_the_panels_alone() {
        let with = draw(60, 20, Some(failure()));
        let without = draw(60, 20, None);

        assert_ne!(text(&with), text(&without));
        assert!(!text(&without).contains("Connection failed"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run --package lc-inbound overlay::`
Expected: FAIL — the file does not compile, `render` is not found in `crate::overlay`.

- [ ] **Step 3: Write the implementation**

Create `crates/lc-inbound/src/overlay.rs` above that test module:

```rust
//! The block drawn over the panels when something needs saying.

use lc_domain::state::Overlay;
use lc_domain::style::Token;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use crate::theme::Theme;

const MAX_W: u16 = 60;
/// Below this the block cannot hold a sentence, and the panel underneath is
/// already saying the same thing — so nothing is drawn rather than a truncated
/// box. Same rule the panels follow in `layout`.
const MIN_W: u16 = 20;
const MIN_H: u16 = 5;
/// Two borders and a column of padding on each side.
const CHROME_W: u16 = 4;

pub(crate) fn render(frame: &mut Frame, overlay: &Overlay, theme: Theme) {
    let area = frame.area();
    let Some(rect) = centred(area, overlay) else {
        return;
    };

    let block = Block::bordered()
        .title(overlay.title.as_str())
        .border_style(theme.style(Token::Severity(overlay.severity)));
    let inner = block.inner(rect);

    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(overlay.message.as_str()).wrap(Wrap { trim: true }),
        inner,
    );
}

fn centred(area: Rect, overlay: &Overlay) -> Option<Rect> {
    let width = area.width.saturating_sub(CHROME_W).min(MAX_W);
    if width < MIN_W || area.height < MIN_H {
        return None;
    }

    let text = Paragraph::new(overlay.message.as_str()).wrap(Wrap { trim: true });
    let lines = u16::try_from(text.line_count(width.saturating_sub(2))).unwrap_or(u16::MAX);
    let height = lines.saturating_add(2).clamp(MIN_H, area.height);

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}
```

In `crates/lc-inbound/src/render.rs`, draw it last, after the panel loop:

```rust
    if let Some(overlay) = &view.overlay {
        crate::overlay::render(frame, overlay, theme);
    }
```

Register the module in `crates/lc-inbound/src/lib.rs`:

```rust
mod body;
pub mod input;
pub mod layout;
mod overlay;
pub mod render;
pub mod theme;
```

- [ ] **Step 4: Run the tests and the gate**

Run: `mise run test && mise run lint`
Expected: all tests pass, clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-inbound/src
git commit -m "feat(inbound): draw failures in a centred block

Bordered with Token::Severity rather than a hardcoded red, so a theme keeps
control of what danger looks like. A frame too small for a legible block
gets none — the panel underneath is already saying it (spec §5)."
```

---

### Task 9: A scylla-backed Cluster

**Files:**
- Modify: `Cargo.toml` (the `[workspace.dependencies]` block)
- Modify: `crates/lc-outbound/Cargo.toml`
- Modify: `deny.toml` (the `[bans] skip` list)
- Create: `crates/lc-outbound/src/cassandra.rs`
- Modify: `crates/lc-outbound/src/lib.rs`
- Create: `crates/lc-outbound/tests/cassandra.rs`

**Interfaces:**
- Consumes: `lc_domain::ports::Cluster`, `lc_domain::model::{Schema, Keyspace, Replication}`.
- Produces: `lc_outbound::cassandra::CassandraCluster::new(conn: ConnId, contact_point: String, posture: Posture) -> CassandraCluster`, implementing `Cluster`.

- [ ] **Step 1: Add the dependency and see what `deny` says**

In `Cargo.toml`, add to `[workspace.dependencies]`, keeping it alphabetical:

```toml
scylla = "1.7.0"
```

In `crates/lc-outbound/Cargo.toml`, replace the "No CQL driver yet" comment and add the dependency and the feature:

```toml
[dependencies]
async-trait.workspace = true
lc-domain.workspace = true
scylla.workspace = true
serde.workspace = true
thiserror.workspace = true
tokio.workspace = true

[features]
# The integration tier (architecture.md §10). Needs a real cluster, so it is
# not part of `mise run ci`.
integration = []
```

Run: `cargo deny check bans`
Expected: FAIL, listing seven duplicated crates.

- [ ] **Step 2: Record each duplicate as a deliberate decision**

Add to `deny.toml`'s `skip` list, keeping the existing two entries:

```toml
    { crate = "darling@0.20.11", reason = "scylla's derives lag; darling_core and darling_macro follow it" },
    { crate = "darling_core@0.20.11", reason = "pulled by darling 0.20" },
    { crate = "darling_macro@0.20.11", reason = "pulled by darling 0.20" },
    { crate = "foldhash@0.1.5", reason = "hashbrown 0.15 in scylla's tree lags hashbrown 0.16's foldhash" },
    { crate = "getrandom@0.3.4", reason = "scylla's rand 0.9 has not moved to getrandom 0.4" },
    { crate = "hashbrown@0.14.5", reason = "a transitive dependency in scylla's tree lags" },
    { crate = "socket2@0.5.10", reason = "scylla pins socket2 0.5; tokio is on 0.6" },
```

Run: `cargo deny check`
Expected: PASS. If a version differs from the one named here, correct the entry to the version `cargo deny` reports — the pin is the point.

- [ ] **Step 3: Write the failing integration test**

Create `crates/lc-outbound/tests/cassandra.rs`:

```rust
//! The integration tier (architecture.md §10): a real cluster, real CQL.
//!
//! Run with `mise run test:it` after `mise run db:up`.

#![cfg(feature = "integration")]

use lc_domain::model::{ConnId, KeyspaceKind, Replication};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use lc_outbound::cassandra::CassandraCluster;

fn contact_point() -> String {
    let port = std::env::var("LC_CASSANDRA_PORT").unwrap_or_else(|_| "9042".to_owned());
    format!("127.0.0.1:{port}")
}

#[tokio::test]
async fn the_fixture_cluster_reports_the_keyspaces_the_seed_created() {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);

    cluster.connect(conn).await.expect("the fixture is up");
    let schema = cluster.describe(conn).await.expect("describe succeeds");

    let shop = schema.keyspaces.get("shop").expect("shop exists");
    assert_eq!(
        shop.replication,
        Replication::NetworkTopologyStrategy {
            factors: [("datacenter1".to_owned(), 1)].into_iter().collect(),
        }
    );

    let typezoo = schema.keyspaces.get("typezoo").expect("typezoo exists");
    assert_eq!(typezoo.replication, Replication::SimpleStrategy { factor: 1 });

    assert!(schema.keyspaces.contains_key("edge_cases"));
}

#[tokio::test]
async fn the_clusters_own_keyspaces_sort_after_the_fixtures() {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);

    cluster.connect(conn).await.expect("the fixture is up");
    let schema = cluster.describe(conn).await.expect("describe succeeds");

    let kinds: Vec<KeyspaceKind> = schema
        .keyspaces_in_display_order()
        .map(|keyspace| keyspace.kind())
        .collect();
    let first_system = kinds
        .iter()
        .position(|kind| *kind == KeyspaceKind::System)
        .expect("a real cluster has system keyspaces");

    assert!(
        kinds[..first_system]
            .iter()
            .all(|kind| *kind == KeyspaceKind::User),
        "system keyspaces must not be interleaved with the user's"
    );
}

#[tokio::test]
async fn connecting_with_an_id_the_adapter_does_not_serve_is_refused() {
    let conn = ConnId::next();
    let cluster = CassandraCluster::new(conn, contact_point(), Posture::ReadOnly);

    assert!(cluster.connect(ConnId::next()).await.is_err());
}
```

Run: `cargo nextest run --package lc-outbound --features integration`
Expected: FAIL — `unresolved import lc_outbound::cassandra`.

- [ ] **Step 4: Write the adapter**

Create `crates/lc-outbound/src/cassandra.rs`:

```rust
//! The `Cluster` port over `scylla`.
//!
//! Driver types stop here: everything crossing back into the domain is our own
//! vocabulary (architecture.md §5).

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use lc_domain::model::{ConnId, DbError, Keyspace, Page, PagingState, Replication, Schema, Statement};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::cluster::ClusterState;
use scylla::cluster::metadata::{Keyspace as DriverKeyspace, Strategy};
use tokio::sync::RwLock;

/// One cluster, reached over CQL.
///
/// `Cluster::connect` carries a [`ConnId`] and no address, so the adapter has
/// to already know where that id points; `&self` is why the session sits behind
/// a lock. One target rather than a map is the honest match for a `State` that
/// also holds exactly one connection.
#[derive(Debug)]
pub struct CassandraCluster {
    conn: ConnId,
    contact_point: String,
    posture: Posture,
    session: RwLock<Option<Arc<Session>>>,
}

impl CassandraCluster {
    #[must_use]
    pub fn new(conn: ConnId, contact_point: String, posture: Posture) -> Self {
        Self {
            conn,
            contact_point,
            posture,
            session: RwLock::new(None),
        }
    }

    fn check(&self, conn: ConnId) -> Result<(), DbError> {
        if conn == self.conn {
            Ok(())
        } else {
            Err(DbError::Other(
                "this adapter serves one connection".to_owned(),
            ))
        }
    }

    async fn session(&self, conn: ConnId) -> Result<Arc<Session>, DbError> {
        self.check(conn)?;
        self.session
            .read()
            .await
            .clone()
            .ok_or_else(|| DbError::Unreachable("not connected".to_owned()))
    }
}

#[async_trait]
impl Cluster for CassandraCluster {
    async fn connect(&self, conn: ConnId) -> Result<Posture, DbError> {
        self.check(conn)?;

        let session = SessionBuilder::new()
            .known_node(&self.contact_point)
            .build()
            .await
            .map_err(|error| DbError::Unreachable(error.to_string()))?;

        *self.session.write().await = Some(Arc::new(session));

        Ok(self.posture)
    }

    async fn describe(&self, conn: ConnId) -> Result<Schema, DbError> {
        let session = self.session(conn).await?;
        session
            .refresh_metadata()
            .await
            .map_err(|error| DbError::Other(error.to_string()))?;

        Ok(schema(&session.get_cluster_state()))
    }

    async fn execute(&self, _conn: ConnId, _stmt: &Statement) -> Result<Page, DbError> {
        Err(DbError::Other(
            "executing statements is not implemented yet".to_owned(),
        ))
    }

    async fn fetch_page(&self, _conn: ConnId, _cursor: &PagingState) -> Result<Page, DbError> {
        Err(DbError::Other("paging is not implemented yet".to_owned()))
    }
}

fn schema(state: &ClusterState) -> Schema {
    Schema {
        keyspaces: state
            .keyspaces_iter()
            .map(|(name, keyspace)| (name.to_owned(), to_keyspace(name, keyspace)))
            .collect(),
    }
}

fn to_keyspace(name: &str, keyspace: &DriverKeyspace) -> Keyspace {
    Keyspace {
        name: name.to_owned(),
        replication: replication(&keyspace.strategy),
        // Tables arrive with the unfold slice: mapping them means turning the
        // driver's partition_key/clustering_key name lists and its ColumnKind
        // map into our PrimaryKey, which drawing a tree root does not need.
        tables: BTreeMap::new(),
    }
}

fn replication(strategy: &Strategy) -> Replication {
    match strategy {
        Strategy::SimpleStrategy { replication_factor } => Replication::SimpleStrategy {
            factor: factor(*replication_factor),
        },
        Strategy::NetworkTopologyStrategy {
            datacenter_repfactors,
        } => Replication::NetworkTopologyStrategy {
            factors: datacenter_repfactors
                .iter()
                .map(|(datacenter, rf)| (datacenter.clone(), factor(*rf)))
                .collect(),
        },
        Strategy::LocalStrategy => Replication::Other {
            class: "LocalStrategy".to_owned(),
        },
        Strategy::Other { name, .. } => Replication::Other {
            class: name.clone(),
        },
        // `Strategy` is #[non_exhaustive]; a strategy we have never seen is
        // still a strategy, and its name is the useful part.
        other => Replication::Other {
            class: format!("{other:?}"),
        },
    }
}

/// Replication factors arrive as `usize`. Nothing near `u32::MAX` is a real
/// factor, so saturating is the honest conversion.
fn factor(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
```

Register it in `crates/lc-outbound/src/lib.rs`, replacing the "no driver yet" paragraph:

```rust
//! Port implementations: cluster, config, secrets, local store.
//!
//! The only crate that talks to the outside world. The [`Cluster`] port is
//! implemented over `scylla` (doc/specs/2026-08-08-keyspace-list-design.md §2);
//! driver types stop at the adapter.
//!
//! [`Cluster`]: lc_domain::ports::Cluster

pub mod cassandra;
pub mod fake;
```

- [ ] **Step 5: Run the fast tier, the gate, and the integration tier**

Run: `mise run test && mise run lint && mise run deny`
Expected: PASS. The integration test is compiled out without the feature.

Run: `mise run db:up && mise run test:it`
Expected: the three integration tests pass. If port 9042 is taken, use `LC_CASSANDRA_PORT=9043 mise run db:up` and the same variable for `test:it`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock deny.toml crates/lc-outbound
git commit -m "feat(outbound): implement Cluster over scylla

Closes the architecture.md §8 driver spike. scylla's cluster::metadata
already returns the model schema.rs declares, where cdrs-tokio has no
metadata API at all and cassandra-cpp is FFI. The seven new deny.toml skips
are the price, and each names whose transitive choice it is (spec §2, §3)."
```

---

### Task 10: Drive whole sessions through the fake

**Files:**
- Modify: `crates/lc-outbound/src/fake.rs`
- Create: `crates/lc-outbound/tests/scenario.rs`

**Interfaces:**
- Consumes: `init`, `Msg`, `Effect` from Task 4; `view_model`, `PanelBody` from Task 6.
- Produces: `FakeCluster::failing(error: DbError) -> FakeCluster`.

- [ ] **Step 1: Write the failing scenario tests**

Create `crates/lc-outbound/tests/scenario.rs`:

```rust
//! Whole sessions driven through the in-memory cluster (architecture.md §10).
//!
//! Deterministic: no clock, no socket, no runtime beyond the one `#[tokio::test]`
//! provides to await the port.

use std::collections::{BTreeMap, VecDeque};

use lc_domain::model::{ConnId, DbError, Keyspace, Replication, Schema};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use lc_domain::view::PanelBody;
use lc_domain::{Effect, Msg, State, init, update, view_model};
use lc_outbound::fake::FakeCluster;

fn schema_of(names: &[&str]) -> Schema {
    Schema {
        keyspaces: names
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    Keyspace {
                        name: (*name).to_owned(),
                        replication: Replication::SimpleStrategy { factor: 1 },
                        tables: BTreeMap::new(),
                    },
                )
            })
            .collect(),
    }
}

/// Runs the effects a session produces, feeding results back through `update`
/// exactly as `lc-app`'s runtime does.
async fn run(cluster: &dyn Cluster, state: &mut State, effects: Vec<Effect>) {
    let mut pending: VecDeque<Effect> = effects.into();

    while let Some(effect) = pending.pop_front() {
        let msg = match effect {
            Effect::Connect(conn) => match cluster.connect(conn).await {
                Ok(posture) => Msg::Connected { conn, posture },
                Err(error) => Msg::ConnectFailed { conn, error },
            },
            Effect::LoadSchema(conn) => match cluster.describe(conn).await {
                Ok(schema) => Msg::SchemaLoaded {
                    conn,
                    schema: Box::new(schema),
                },
                Err(error) => Msg::SchemaFailed { conn, error },
            },
            _ => continue,
        };

        pending.extend(update(state, msg));
    }
}

#[tokio::test]
async fn connecting_and_describing_fills_the_tables_panel() {
    let cluster = FakeCluster::new(
        schema_of(&["system_schema", "typezoo", "shop"]),
        Posture::ReadOnly,
    );
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);

    run(&cluster, &mut state, effects).await;

    let PanelBody::Keyspaces { rows, selected } = &view_model(&state).panels[0].body else {
        panic!("a described cluster lists its keyspaces");
    };
    assert_eq!(
        rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        ["shop", "typezoo", "system_schema"]
    );
    assert_eq!(*selected, 0);
}

#[tokio::test]
async fn a_cluster_that_cannot_be_reached_raises_an_overlay() {
    let cluster = FakeCluster::failing(DbError::Unreachable("no route to host".to_owned()));
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);

    run(&cluster, &mut state, effects).await;

    let overlay = view_model(&state).overlay.expect("the failure is shown");
    assert!(overlay.message.contains("no route to host"));
    assert!(
        matches!(&view_model(&state).panels[0].body, PanelBody::Notice(text) if text.contains("no route to host")),
        "the panel says it too, so dismissing the overlay does not erase the failure"
    );
}

#[tokio::test]
async fn a_cluster_that_connects_but_cannot_be_described_raises_an_overlay() {
    let cluster = FakeCluster::failing_to_describe(DbError::Invalid("unauthorized".to_owned()));
    let conn = ConnId::next();
    let (mut state, effects) = init(conn);

    run(&cluster, &mut state, effects).await;

    let overlay = view_model(&state).overlay.expect("the failure is shown");
    assert_eq!(overlay.title, "Schema unavailable");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo nextest run --package lc-outbound`
Expected: FAIL — `no function or associated item named failing found`.

- [ ] **Step 3: Give the fake a failure mode**

Rewrite `crates/lc-outbound/src/fake.rs`'s struct and constructors, leaving the `Cluster` impl's `execute`/`fetch_page` as they are:

```rust
#[derive(Debug, Clone, Default)]
pub struct FakeCluster {
    schema: Schema,
    posture: Posture,
    /// What `connect` returns instead of succeeding.
    connect_error: Option<DbError>,
    /// What `describe` returns instead of succeeding.
    describe_error: Option<DbError>,
}

impl FakeCluster {
    #[must_use]
    pub const fn new(schema: Schema, posture: Posture) -> Self {
        Self {
            schema,
            posture,
            connect_error: None,
            describe_error: None,
        }
    }

    /// A cluster that cannot be reached at all.
    #[must_use]
    pub fn failing(error: DbError) -> Self {
        Self {
            connect_error: Some(error),
            ..Self::default()
        }
    }

    /// A cluster that connects and then refuses to describe itself — the shape
    /// of a connection without the permissions to read `system_schema`.
    #[must_use]
    pub fn failing_to_describe(error: DbError) -> Self {
        Self {
            describe_error: Some(error),
            ..Self::default()
        }
    }
}
```

and the two implicated methods:

```rust
    async fn connect(&self, _conn: ConnId) -> Result<Posture, DbError> {
        self.connect_error
            .clone()
            .map_or(Ok(self.posture), Err)
    }

    async fn describe(&self, _conn: ConnId) -> Result<Schema, DbError> {
        self.describe_error
            .clone()
            .map_or_else(|| Ok(self.schema.clone()), Err)
    }
```

- [ ] **Step 4: Run the tests and the gate**

Run: `mise run test && mise run lint`
Expected: the three scenario tests pass alongside everything else.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-outbound
git commit -m "test(scenario): drive connect and describe through the fake

Two failure modes, because a cluster that cannot be reached and one that
refuses to describe itself land in different places in the state machine
(architecture.md §10)."
```

---

### Task 11: Connect at startup

**Files:**
- Modify: `crates/lc-app/src/main.rs`
- Modify: `crates/lc-app/src/effects.rs`
- Modify: `crates/lc-app/Cargo.toml` (add `lc-outbound`)

**Interfaces:**
- Consumes: `CassandraCluster::new` from Task 9; `init` from Task 4.
- Produces: nothing further tasks depend on.

- [ ] **Step 1: Add the dependency**

In `crates/lc-app/Cargo.toml`, add to `[dependencies]`:

```toml
lc-outbound.workspace = true
```

- [ ] **Step 2: Interpret the two effects**

Rewrite `crates/lc-app/src/effects.rs`:

```rust
//! Interprets the effects `update` returned.
//!
//! This is the only place that performs I/O on the domain's behalf. Effects
//! become tokio tasks; their results re-enter through the same channel as a
//! keystroke (architecture.md §3).

use std::sync::Arc;

use lc_domain::ports::Cluster;
use lc_domain::{Effect, Msg};
use tokio::sync::mpsc::UnboundedSender;

/// The effects that still have no port behind them are inert. `Quit` needs no
/// work here — `update` has already set `State::should_quit`.
pub(crate) fn run(tx: &UnboundedSender<Msg>, cluster: &Arc<dyn Cluster>, effect: &Effect) {
    match effect {
        Effect::Connect(conn) => {
            let (tx, cluster, conn) = (tx.clone(), Arc::clone(cluster), *conn);
            tokio::spawn(async move {
                let msg = match cluster.connect(conn).await {
                    Ok(posture) => Msg::Connected { conn, posture },
                    Err(error) => Msg::ConnectFailed { conn, error },
                };
                // A closed channel means the main loop has already ended.
                let _sent = tx.send(msg);
            });
        }

        Effect::LoadSchema(conn) => {
            let (tx, cluster, conn) = (tx.clone(), Arc::clone(cluster), *conn);
            tokio::spawn(async move {
                let msg = match cluster.describe(conn).await {
                    Ok(schema) => Msg::SchemaLoaded {
                        conn,
                        schema: Box::new(schema),
                    },
                    Err(error) => Msg::SchemaFailed { conn, error },
                };
                let _sent = tx.send(msg);
            });
        }

        Effect::Execute { .. }
        | Effect::FetchNextPage { .. }
        | Effect::Cancel(_)
        | Effect::RecordHistory(_)
        | Effect::Quit => {}
    }
}
```

- [ ] **Step 3: Start from `init` against a real cluster**

In `crates/lc-app/src/main.rs`, replace `run`'s opening and add the contact point helper:

```rust
/// The fixture's convention (docker/compose.yaml). Scaffolding until the
/// config layering of vision.md §6 lands — not a configuration mechanism.
fn contact_point() -> String {
    let host = std::env::var("LC_CASSANDRA_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = std::env::var("LC_CASSANDRA_PORT").unwrap_or_else(|_| "9042".to_owned());

    format!("{host}:{port}")
}

async fn run(mut terminal: DefaultTerminal) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    input::spawn_reader(tx.clone());

    let conn = ConnId::next();
    let cluster: Arc<dyn Cluster> = Arc::new(CassandraCluster::new(
        conn,
        contact_point(),
        Posture::ReadOnly,
    ));

    let (mut state, startup) = lc_domain::init(conn);
    let theme = Theme::default();

    for effect in &startup {
        effects::run(&tx, &cluster, effect);
    }

    terminal.draw(|frame| lc_inbound::render(frame, &view_model(&state), theme))?;
    state.dirty = false;

    while let Some(msg) = rx.recv().await {
        for effect in update(&mut state, msg) {
            effects::run(&tx, &cluster, &effect);
        }

        if state.should_quit {
            break;
        }

        if state.dirty {
            terminal.draw(|frame| lc_inbound::render(frame, &view_model(&state), theme))?;
            state.dirty = false;
        }
    }

    Ok(())
}
```

and the imports:

```rust
use std::sync::Arc;

use anyhow::Result;
use lc_domain::model::ConnId;
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use lc_domain::{update, view_model};
use lc_inbound::Theme;
use lc_outbound::cassandra::CassandraCluster;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;
```

`State` leaves the import list: `init` returns it, so it is never named here.

- [ ] **Step 4: Run the gate and the application**

Run: `mise run ci`
Expected: clean.

Run: `mise run db:up`, then `mise run run` (use `LC_CASSANDRA_PORT=9043` on both if 9042 is taken).
Expected: the Tables panel shows `▸ shop`, `▸ typezoo`, `▸ edge_cases`, then the dimmed `system*` keyspaces. `j`/`k` move the highlight; `q` quits.

Run: `LC_CASSANDRA_PORT=1 mise run run`
Expected: a red-bordered "Connection failed" block in the middle of the screen. `Esc` dismisses it, leaving the failure text in the Tables panel. `q` quits.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-app Cargo.lock
git commit -m "feat(app): connect at startup and load the schema

Effect::Connect and Effect::LoadSchema become tokio tasks whose results
re-enter through the keystroke channel. The contact point comes from
LC_CASSANDRA_HOST/PORT, which is scaffolding for the fixture rather than a
configuration mechanism (spec §6)."
```

---

### Task 12: Record that the driver question is closed

**Files:**
- Modify: `doc/architecture.md` §1 and §8
- Modify: `doc/specs/2026-08-08-keyspace-list-design.md` (the status line)

- [ ] **Step 1: Replace the deferral with the decision**

In `doc/architecture.md`, change the §1 table's driver row from `Deferred to a Phase 0 spike` to:

```markdown
| Driver | `scylla` 1.7 | Its `cluster::metadata` already returns the model §4 declares; the alternatives have no metadata API or are FFI. See §8 |
```

Replace §8's body with the outcome, keeping the candidate table as the record of what was compared and adding the result:

```markdown
## 8. Driver — decided

`scylla` 1.7, default features. The spike is written up in
[specs/2026-08-08-keyspace-list-design.md](specs/2026-08-08-keyspace-list-design.md) §2.

| Candidate | Outcome |
|---|---|
| `scylla-rust-driver` | **Chosen.** `cluster::metadata` returns keyspaces, tables, column kinds and replication strategy as parsed types; `PagingState`/`execute_iter` map onto the `Cluster` port unchanged |
| `cdrs-tokio` | Rejected: no metadata API at all, so every `system_schema` query and its version drift would be ours |
| `cassandra-cpp` | Rejected: FFI over the DataStax C++ driver, which `unsafe_code = "forbid"` and the single-binary goal both rule out |

The cost is seven `deny.toml` skip entries where `cdrs-tokio` would have cost
one. Each names whose transitive choice it is, so the tree converging is
visible rather than silently tolerated.
```

Update the document's `**Status:**` and `**Last updated:**` lines.

- [ ] **Step 2: Mark the spec implemented**

In `doc/specs/2026-08-08-keyspace-list-design.md`, change `**Status:** approved, not implemented.` to `**Status:** implemented.`

- [ ] **Step 3: Verify the whole gate one last time**

Run: `mise run ci`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add doc/architecture.md doc/specs/2026-08-08-keyspace-list-design.md
git commit -m "docs(architecture): close the driver spike

§8 was an open question with three candidates. It is scylla 1.7, and the
reasoning is in the keyspace-list spec."
```
