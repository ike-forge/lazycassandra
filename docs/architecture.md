# LazyCassandra — Architecture

Companion to [VISION.md](VISION.md). This document covers structure and the
decisions already made; open questions are marked as such in §11 rather than
guessed at.

**Status:** Phase 1 begun — four crates, a running binary, the four-panel TUI
shell rendering, the driver decided (§8), the first end-to-end query path: a
keypress becomes an `Effect::Execute` and a driver page becomes a grid
navigable in both axes, a writable CQL buffer in the Queries panel
(`editor.rs`), a themed renderer with a runtime picker (`ctrl-t`) over twelve built-in
themes plus whatever a config file adds (§6), and layered TOML configuration
with startup diagnostics (§7).
**Last updated:** 2026-08-15

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
| Driver | `scylla` 1.7.0 | Its `cluster::metadata` already returns the model §4 declares; the alternatives have no metadata API or are FFI. See §8 |

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
  lc-domain/      # pure core. depends on: std, serde, thiserror, async-trait, unicode-width
  lc-inbound/     # TUI adapter. depends on: lc-domain, ratatui, crossterm, unicode-width
  lc-outbound/    # driver + config + storage adapters. depends on: lc-domain, tokio, <driver>
  lc-app/         # binary: composition root, effect runtime, main loop
```

`lc-inbound` and `lc-outbound` never see each other. Only `lc-app` depends on
all three, and it is deliberately thin — wiring, not logic.

The domain's list is short on purpose, and the test it has to pass is what it
names rather than how long it is: no terminal, no runtime, no driver.
`unicode-width` earns its place by that test — it is a table of character
widths, and the domain needs one because it computes the Contents grid's column
widths itself. Those widths are a pure function of the page and they have to be
where the offsets are, for the reason §3 gives
(specs/2026-08-10-table-display-design.md §6). A crate that measures a string
is not a crate that draws one.

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
effect results  ───┼──▶ mpsc<Msg> ──▶ update() ──▶ Vec<Effect> ──▶ effect runtime
frame geometry  ───┘         ▲                                            │
                             └────────────────────────────────────────────┘
                                       (spawns tokio tasks, sends Msg back)
```

**Frame geometry is the one edge that runs backwards**, from the renderer into
the domain as `Msg::Measured`: only the renderer knows how tall a panel is or
how many lines a body wrapped to, and a scroll offset has to be bounded by both
(specs/2026-08-09-panel-scrolling-design.md §3). Two guards keep it from
looping: `lc-app` sends only when the geometry differs from the last one sent,
and `update` marks dirty only when the clamp actually moved the offset — so a
measurement that changes nothing draws no frame and produces no further
measurement. Both rest on one invariant: **`Geometry` is a function of the
terminal size and the view model alone, never of the offsets it bounds.**
Measuring anything that varies with an offset closes the loop.

`Geometry` now also carries the character width the Contents grid's body has,
and that is the invariant holding rather than bending. A width is a function of
the terminal size; *how many columns fit starting at offset N* is not, and it is
never measured — the renderer sends the width back, `update` owns the widths and
does the fitting itself, and nothing offset-dependent crosses into the domain
(specs/2026-08-10-table-display-design.md §6). One scalar also leaves `Geometry`
`Copy`, which a `Vec<u16>` of column widths would have cost.

The renderer now reports a second thing, and it is worth not confusing the two:
a hit map, which resolves a pointer position to a panel and a row
(specs/2026-08-09-mouse-input-design.md §4). It stops at `lc-app` and re-enters
the domain never, so it needs no guard and is free to depend on the offsets
`Geometry` may not. Only what crosses back into `update` can close a loop.

`async-trait` is in the domain's list because the port traits in §5 are defined
here and must stay `dyn`-compatible. It is a proc macro over boxed futures, not
a runtime — it pulls in no executor, so the rule above still holds.

```rust
enum Msg {
    // Not crossterm's KeyEvent: the domain owns its own input vocabulary and
    // lc-inbound translates. Same direction of travel as §6's style tokens.
    Input(KeyPress),
    Tick,
    SchemaLoaded { conn: ConnId, schema: Schema },
    PageReady   { query: QueryId, page: Result<Page, DbError> },
    Failed      { query: QueryId, error: DbError },
    // ...
}

enum Effect {
    Connect(ConnId),
    LoadSchema(ConnId),
    Execute { conn: ConnId, query: QueryId, stmt: Statement },
    FetchNextPage { conn: ConnId, query: QueryId, cursor: PagingState },
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
  state.rs      # State, Panel, selection, in-flight registry
  tree.rs       # the Tables panel's tree: unfold set, flattening, transitions
  contents.rs   # the Contents panel: the outstanding read and its result
  editor.rs     # the Queries panel's text buffer
  cell.rs       # one CqlValue as the text a grid draws
  update.rs     # update() and its per-panel handlers
  view.rs       # ViewModel projection (see §3)
  ports.rs      # trait definitions (see §5)
  posture.rs    # read-only / writable / protected rules
  style.rs      # semantic style tokens (see §6)
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
| `ConfigSource` | locate, layer and validate config files; never fails |
| `SecretResolver` | resolve a credential (literal / env / file) to a value |
| `Store` | query history, saved queries |

Ports are `dyn`-compatible so a fake can be swapped in wholesale; that means
`#[async_trait]` on the async ones until native AFIT is `dyn`-safe.

**`Cluster` is defined in our vocabulary, not the driver's.** It returns our
`Schema` and `Page` types. Driver types stop at the adapter. This is what made
§8 cheap to close — swapping drivers touches one crate.

## 6. Theme plumbing

VISION principle 5 says colour is a relationship, not a property: a theme
declares its own ground and is held to contrast against it. That falls out
of the architecture rather than being bolted on.

The domain never names a colour. It emits **semantic style tokens**:

```rust
enum Token {
    PartitionKey, ClusteringKey, RegularColumn, Posture(Posture),
    Severity(Severity), Muted, Faint, Keyspace, Table, Keyword, Reference,
    Literal, /* … */
}
```

A theme is a `Palette` — a struct of 30 plain `Color` fields, no
`Option<Color>` — plus one `Token -> ratatui::Style` function shared by every
theme, both in `lc-inbound`. `ThemeChoice` lives in `lc-domain` instead,
because a theme *name* is not a colour: `State.theme` holds one.

`ThemeChoice` has three variants: `Ours(Ours)` and `Ported(Ported)` for the
twelve built-ins, and `Custom(ThemeName)` for a palette a config file
supplies under `[[themes]]`. `Ours` holds the eight palettes we author —
Default, Ash, Beacon and Quill, each light and dark — and `Ported` holds the
four Catppuccin flavours we take as given. The `Ours`/`Ported` split is
structural rather than a naming convention because the contrast sweep — in
`theme::tests`, over the ratios `contrast.rs` computes — is written against
`Ours::ALL`: it takes an `Ours` and cannot name a `Ported` theme by type, so
"we impose no contrast requirement on a ported palette" is a fact the
compiler enforces, not a policy someone could forget to apply. Adding a
ported theme is one enum variant and one `const`, with no test to appease.

Adding one of ours is not enforced by the type system, because `Ours::ALL` is
a hand-written array and the compiler cannot check that it lists every
variant. What catches an omission is `Ours::next`, an exhaustive match each
variant of which names the one declared after it: a ninth variant fails to
compile until `next` is extended, and `walking_from_the_first_theme_of_ours_reaches_every_one`
then fails until `ALL` lists it too. Two steps, both of which have to be taken
before the build is green again — but a deliberate edit to both could still
leave a palette unswept.

A built-in resolves through `Theme::of`, the match from name to `Palette`
that reads the built-in's own `const`. A custom one does not — `Theme::of`
alone cannot answer for a palette it never sees. Resolution goes through
`lc-inbound`'s `Themes` table instead, built once at startup from
`Config.themes`: for each custom theme it starts from the palette `extends`
names and overwrites the stated roles, so a partial palette is always
complete. `Theme::of(choice)` becomes `themes.get(&choice)` — one lookup, no
per-frame work, and the same table answers for a built-in and a custom theme
alike.

Practical consequence: nothing in the domain has to change to add a
built-in theme, and no theme can invent a meaning the domain didn't define.
See specs/2026-08-13-colour-palettes-design.md for the roles, all twelve
built-in themes, and the runtime picker (`ctrl-t`).

## 7. Configuration

Layered, lowest to highest: built-in defaults → user config
(`$XDG_CONFIG_HOME/lazycassandra/config.toml`) → project
`.lazycassandra.toml`. Environment and command-line layers are designed for
and not yet built.

Discovery, parsing, merging and validation are `lc-outbound`; the merged,
validated result is `lc_domain::config::Config`, which is total: no
`Option` except where absence is itself meaningful — whether a password or
a `default_connection` was stated, say. Everywhere else, `Option` lives in
`lc-outbound`'s wire structs and is resolved to a value — stated or
defaulted — before it crosses into `Config`. A file that is wrong does not
stop the tool: the
smallest addressable unit is dropped, the rest applies, and every drop is
reported in a modal at startup.

Credentials are `Literal`, `Env` or `File`. There is no `Command` variant,
so nothing in the configuration path executes anything.

Full design: specs/2026-08-14-configuration-design.md.

## 8. Driver — decided

`scylla` 1.7.0, default features. The spike is written up in
[specs/2026-08-08-keyspace-list-design.md](specs/2026-08-08-keyspace-list-design.md) §2.

| Candidate | Outcome |
|---|---|
| `scylla-rust-driver` | **Chosen.** `cluster::metadata` returns keyspaces, tables, column kinds and replication strategy as parsed types; `PagingState`/`execute_iter` map onto the `Cluster` port unchanged |
| `cdrs-tokio` | Rejected: no metadata API at all, so every `system_schema` query and its version drift would be ours |
| `cassandra-cpp` | Rejected: FFI over the DataStax C++ driver, which `unsafe_code = "forbid"` and the single-binary goal both rule out |

Wiring it in cost ten `deny.toml` skip entries — `darling`, `darling_core`,
`darling_macro` (0.20 vs. 0.24), `foldhash` 0.1, `getrandom` 0.3, `hashbrown`
0.14.5 (via `dashmap`), `hashbrown` 0.15.5, `r-efi` 5.3.0, `socket2` 0.5.10 and
`windows-sys` 0.52.0 — plus one licence, `BSD-3-Clause`, added to the
allow-list for `snap` 1.1.2, the Snappy codec `scylla-cql` pulls in. Each skip
entry names whose transitive choice it is, so the tree converging is visible
rather than silently tolerated. The pre-implementation comparison in the spec
found `cdrs-tokio` would have cost one skip entry against seven for
`scylla` — the estimate that decided the trade, not the final count.

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
  four panels. Nested per-panel state with message routing is the usual escape
  hatch; `TablesPanel` took it first and `ContentsPanel` followed, each a
  module of its own. `DdlPanel` took it third, the same `Cursor`-and-`Scroll`
  shape, but stayed a struct in `state.rs` rather than earning a module of its
  own — two fields was not enough state to need one. `editor.rs` is a third
  module, but it is a *buffer* rather than a panel — `State.queries` is an
  `Editor`, not a `QueriesPanel` — so the question of what routes messages to
  per-panel state is still open and still answered by `update` dispatching by
  hand.
- **Where schema caching lives.** Domain (testable, but the domain starts
  holding a cache invalidation problem) or adapter (simpler domain, but cache
  behaviour becomes untestable at the fast tier).
- **Panic and terminal restoration.** Raw mode must be restored on panic or the
  user's terminal is left broken. An RAII guard plus a panic hook is the plan;
  the interaction with tokio worker panics needs verifying.

## 12. Conventions

Structural rules only. Style — comments, commit practice — is in
[conventions.md](conventions.md).

- Errors: `thiserror` enums in the domain, typed and matched on. `anyhow` is
  permitted in `lc-app` wiring only. A failed query is a `Msg`, never a panic
  and never an early return that unwinds the loop.
- No `unwrap()` outside tests and the composition root.
- Public items in `lc-domain` carry doc comments; it is the crate everything
  else is written against.
- Formatting and lints: `rustfmt` default, `clippy` with warnings denied in CI.
