# LazyCassandra — Architecture

Companion to [VISION.md](VISION.md). This document covers structure and the
decisions already made; open questions are marked as such in §11 rather than
guessed at.

**Status:** pre-implementation, Phase 0 not started.
**Last updated:** 2026-08-07

---

## 1. Decisions already made

| Decision | Choice | Why |
|---|---|---|
| Language | Rust | Latency floor (VISION §4.7), single static binary, type-safe long-lived state |
| TUI framework | `ratatui` + `crossterm` | De-facto Rust TUI stack, unopinionated about state, good test backend |
| State model | Elm-style (TEA) | Pure `update`, testable headless, natural fit for async results arriving as messages |
| Structure | Hexagonal — domain / inbound / outbound | Keeps the TUI ignorant of Cassandra and vice versa, enforced at crate level |
| Async runtime | `tokio` | Required by the realistic driver candidates; single multi-threaded runtime |
| Headless core | Internal boundary only | Drives testability. **No CLI is promised or designed.** See §9 |
| Driver | Deferred to a Phase 0 spike | See §8 |

## 2. The shape

A simple hexagon. Three layers, dependencies point inward only.

```
        ┌──────────────── inbound ────────────────┐
        │  TUI adapter: ratatui render, crossterm │
        │  input → Msg. Knows nothing about CQL.  │
        └────────────────────┬────────────────────┘
                             │ Msg / ViewModel
                             ▼
        ┌──────────────── domain ─────────────────┐
        │  State · Msg · update() · Effect        │
        │  Schema & CQL model · posture rules     │
        │  Port traits (definitions only)         │
        │  Pure. No I/O, no tokio, no ratatui.    │
        └────────────────────┬────────────────────┘
                             │ Effect / port traits
                             ▼
        ┌──────────────── outbound ───────────────┐
        │  Cassandra adapter · config loader ·    │
        │  secret resolver · local store          │
        └─────────────────────────────────────────┘
```

The rule that makes it real: **the domain crate's `Cargo.toml` lists neither
`ratatui` nor `tokio` nor any database driver.** The boundary is not a
convention anyone has to remember — it fails to compile.

### Crate layout

A cargo workspace, so the dependency rule is checked by the compiler:

```
crates/
  lc-domain/      # pure core. depends on: std, serde, thiserror
  lc-inbound/     # TUI adapter. depends on: lc-domain, ratatui, crossterm
  lc-outbound/    # driver + config + storage adapters. depends on: lc-domain, tokio, <driver>
  lc-app/         # binary: composition root, effect runtime, main loop
```

`lc-inbound` and `lc-outbound` never see each other. Only `lc-app` depends on
all three, and it is deliberately thin — wiring, not logic.

## 3. The loop

Standard TEA, with one twist that matters: **`update` does not perform effects,
it returns them.**

```rust
// lc-domain
pub fn update(state: &mut State, msg: Msg) -> Vec<Effect>;
pub fn view_model(state: &State) -> ViewModel;
```

`update` is synchronous, pure, and total. It mutates state and describes what
should happen next. It cannot open a socket, because it has nothing to open one
with.

`lc-app` owns the runtime that interprets `Effect`s:

```
crossterm events ──┐
                   ├──▶ mpsc<Msg> ──▶ update() ──▶ Vec<Effect> ──▶ effect runtime
effect results  ───┘         ▲                                            │
                             └────────────────────────────────────────────┘
                                       (spawns tokio tasks, sends Msg back)
```

```rust
enum Msg {
    Input(KeyEvent),
    Tick,
    SchemaLoaded { conn: ConnId, schema: Schema },
    PageReady   { query: QueryId, page: Result<Page, DbError> },
    Failed      { query: QueryId, error: DbError },
    // ...
}

enum Effect {
    Connect(ConnId),
    LoadSchema(ConnId),
    Execute { query: QueryId, stmt: Statement },
    FetchNextPage { query: QueryId, cursor: PagingState },
    Cancel(QueryId),
    RecordHistory(Statement),
    Quit,
}
```

Every async result re-enters through the same `Msg` channel as a keystroke.
There is exactly one place state changes.

### Concurrency

- **Main thread**: render + `update`. Never awaits, never blocks. This is what
  buys VISION principle 7 — a slow cluster cannot freeze the UI, because the UI
  thread never touches the cluster.
- **Input**: crossterm events read on a dedicated thread, forwarded as `Msg`.
- **I/O**: the effect runtime spawns tokio tasks. Results return as `Msg`.
- **Cancellation**: every in-flight effect carries a `QueryId` and a cancel
  token. Navigating away from a running query issues `Effect::Cancel` rather
  than orphaning the task. Late results for a cancelled `QueryId` are dropped in
  `update`.
- **Rendering**: redraw on state change or tick, not per event. A dirty flag on
  `State` avoids repainting on every keypress in a held-down `j`.

### Why this pays off

Testing the whole application becomes synchronous. `update` is a function from
`(State, Msg)` to `(State, Vec<Effect>)` — no runtime, no mocks, no timing. See
§10.

## 4. Domain model

```
lc-domain/src/
  model/        # Schema, Keyspace, Table, PrimaryKey, CqlValue, Statement
  state/        # State, panel focus, selection, in-flight registry
  update/       # update() and its per-panel handlers
  ports/        # trait definitions (see §5)
  posture/      # read-only / writable / protected rules
  style/        # semantic style tokens (see §6)
```

Two model choices worth stating now:

**Primary key structure is a first-class type, not a column list.** `PrimaryKey`
distinguishes partition columns from clustering columns and carries clustering
order. VISION's central commitment is making this visible; it needs to be
unmissable in the type system, not reconstructed from a `Vec<Column>` at render
time.

**Connection posture gates effects in the domain.** `Posture` is
`ReadOnly | Writable | Protected`. A mutating `Statement` produced against a
`ReadOnly` connection is rejected in `update` and never becomes an `Effect`. The
safety rail is structural — the TUI cannot bypass it, because the TUI does not
construct effects.

## 5. Ports

Traits are **defined** in `lc-domain` and **implemented** in `lc-outbound`. The
domain does not call them — the effect runtime in `lc-app` does. This is what
lets the domain stay sync and dependency-free while the ports themselves are
async.

| Port | Responsibility |
|---|---|
| `Cluster` | connect, describe schema, execute, fetch page, node/table state |
| `ConfigSource` | locate and layer config files |
| `SecretResolver` | resolve a credential reference (env / file / command) to a value |
| `Store` | query history, saved queries |

Ports are `dyn`-compatible so a fake can be swapped in wholesale; that means
`#[async_trait]` on the async ones until native AFIT is `dyn`-safe.

**`Cluster` is defined in our vocabulary, not the driver's.** It returns our
`Schema` and `Page` types. Driver types stop at the adapter. This is what makes
§8's deferral cheap — swapping drivers touches one crate.

## 6. Theme plumbing

VISION principle 5 says the default theme is the terminal's own. That falls out
of the architecture rather than being bolted on.

The domain never names a colour. It emits **semantic style tokens**:

```rust
enum Token { PartitionKey, ClusteringKey, Posture(Posture), Severity(Severity), Muted, /* … */ }
```

A `Theme` in `lc-inbound` maps `Token → ratatui::Style`. The default theme maps
only to the sixteen ANSI colours (`Color::Red`, `Color::Blue`, …) and **never
sets a background** — so it inherits whatever palette the user's terminal
already has, on light or dark alike. Themes that hardcode RGB are possible, and
are strictly opt-in.

Practical consequence: nothing in the domain has to change to add a theme, and
no theme can invent a meaning the domain didn't define.

## 7. Configuration

Layered, lowest to highest: built-in defaults → user config
(`$XDG_CONFIG_HOME/lazycassandra/config.yml`) → project `.lazycassandra.yml` →
environment → command-line flags.

Loading and merging is `lc-outbound`; the merged, validated result is a domain
type. Invalid config fails at startup with a pointed message, not lazily on
first use.

**Secrets are references, never values** (VISION §6). The config type makes this
unrepresentable rather than merely discouraged:

```rust
enum CredentialRef { Env(String), File(PathBuf), Command(Vec<String>) }
```

There is no `Password(String)` variant, so a literal password in a committed
file is a parse error rather than a leak. Resolution happens through
`SecretResolver` at connect time, and resolved secrets are never written back to
disk or into the history store.

## 8. Driver selection — deferred

Per VISION §10, this is a Phase 0 spike, not a guess made now. Candidates:

| Candidate | Notes |
|---|---|
| `scylla-rust-driver` | Most actively maintained pure-Rust CQL driver; tokio-native; works against Apache Cassandra despite the name; would make an eventual Scylla phase nearly free |
| `cdrs-tokio` | Pure Rust, Cassandra-first framing; smaller community and slower cadence |
| `cassandra-cpp` | FFI over the DataStax C++ driver; most battle-tested protocol implementation, but a C build dependency undermines the single-static-binary goal |

Spike criteria, in priority order: paging API that supports true incremental
fetch (VISION §5.1 depends on it); prepared statements; TLS and auth; schema
introspection completeness; token-aware routing; maintenance health; build
story.

The spike is a throwaway implementation of the `Cluster` port against a real
cluster. Whichever candidate wins, the port boundary in §5 stands.

## 9. On the headless core

The domain being drivable without a terminal is a **testing property**, not a
product feature. There is no CLI, no JSON output mode, and no scripting host in
scope, and none of the API surface here is a stability promise to anyone outside
this repo.

The boundary exists because it makes the domain testable and keeps the layers
honest. If a CLI is ever wanted, this architecture permits it — but nothing is
being designed for it now, and "we might want a CLI" is not an argument in any
design discussion until that changes.

## 10. Testing strategy

Four tiers, fastest first.

**Domain unit tests** — the bulk. Synchronous, no runtime, no mocks:
given a `State` and a `Msg`, assert the resulting state and the emitted
`Vec<Effect>`. Asserting on effects means we test *what the app decided to do*
without doing it.

**Scenario tests** — an in-memory fake `Cluster` with a scripted schema, driven
through whole sessions (connect → browse → query → page). Catches
sequencing bugs the unit tests can't see. Still no terminal, still fast.

**Render tests** — ratatui's `TestBackend` renders a `ViewModel` to an in-memory
buffer; snapshot the result. Guards layout and the theme mapping.

**Integration tests** — against a real Cassandra in a container. Slow tier, run
separately from the default `cargo test`. This is where driver behaviour,
paging, and schema introspection get verified for real.

## 11. Open questions

Architectural, and genuinely undecided. Distinct from the UI research questions
in VISION §8.

- **`ViewModel` granularity.** One tree rebuilt per frame is simplest and
  probably fine at terminal sizes; per-panel view models with dirty tracking are
  faster but add bookkeeping. Measure before choosing.
- **Result set memory model.** A page-at-a-time window is the honest answer for
  arbitrarily large partitions, but scrollback and re-sorting want more retained.
  Needs a bounded cache with an explicit eviction story.
- **Does `State` stay one struct?** TEA's single-state model gets unwieldy at
  five panels. Nested per-panel state with message routing is the usual escape
  hatch; deferring until the shape of Phase 1 is real.
- **Where schema caching lives.** Domain (testable, but the domain starts
  holding a cache invalidation problem) or adapter (simpler domain, but cache
  behaviour becomes untestable at the fast tier).
- **Panic and terminal restoration.** Raw mode must be restored on panic or the
  user's terminal is left broken. An RAII guard plus a panic hook is the plan;
  the interaction with tokio worker panics needs verifying.

## 12. Conventions

- Errors: `thiserror` enums in the domain, typed and matched on. `anyhow` is
  permitted in `lc-app` wiring only. A failed query is a `Msg`, never a panic
  and never an early return that unwinds the loop.
- No `unwrap()` outside tests and the composition root.
- Public items in `lc-domain` carry doc comments; it is the crate everything
  else is written against.
- Formatting and lints: `rustfmt` default, `clippy` with warnings denied in CI.
