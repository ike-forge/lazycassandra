# Agent instructions — LazyCassandra

Read these before touching code, in this order:

- [doc/vision.md](doc/vision.md) — why the project exists, scope, roadmap
- [doc/architecture.md](doc/architecture.md) — crate boundaries, state model, decisions made vs. open
- [doc/conventions.md](doc/conventions.md) — how the code should read
- [doc/shortcuts.md](doc/shortcuts.md) — the bound keys and the rules governing
  them; read before adding or changing a binding

The rules below are pulled from `doc/conventions.md` because they are easy to
violate by default. They do not replace that document — read it for the why.

## Non-negotiables

- **No `unsafe`, no FFI.** Enforced by `unsafe_code = "forbid"`. If you think
  you need either, stop and raise it — it's a design decision, not an
  implementation detail.
- **Push invariants into types.** Newtype over validated `String`, enum over
  `bool` + comment, unrepresentable state over a runtime check.
- **Crate boundaries are load-bearing.** `lc-domain` must never depend on
  `tokio`, `ratatui`, or `crossterm`. If an edit needs one of those in
  `lc-domain`, the design is wrong, not the dependency list.
- **Zero warnings, zero failing/ignored tests.** `mise run ci` must be clean
  before calling anything done. A deprecation warning is scheduled breakage,
  not background noise. Any `#[allow(...)]` must be item-scoped and carry a
  reason (dependency, version, upstream issue).
- **No narrating comments.** Assume an expert Rust reader — don't explain `?`
  or lifetimes. Before commenting, try: rename, extract a named function,
  introduce a type. Comments are for edge cases, past bugs, upstream defects,
  and Cassandra/protocol semantics the code can't express.
- **Determinism.** No wall-clock reads, no real network/filesystem/cluster,
  no unseeded randomness, no `sleep`-as-sync, no reliance on `HashMap`
  iteration order, in the fast test tiers. Time enters the domain only as
  `Msg::Tick`. Real I/O belongs in the integration tier.
- **Git: commit freely (including to `main`), never `pull`, never `push`.**
  Both are explicit human actions only.
- **Conventional Commits.** Subject is `<type>[scope]: <description>`,
  imperative, lowercase, no trailing period (`feat`, `fix`, `refactor`,
  `test`, `docs`, `chore`, `perf`, `ci`). Flag breaking changes with `!` or a
  `BREAKING CHANGE:` footer.

## Workflow

```
mise run build   # cargo build --workspace
mise run fmt     # cargo fmt --all
mise run lint    # clippy, warnings denied
mise run test    # fast tiers (domain, scenario, render)
mise run ci      # everything CI gates on — run this before considering work done
```

When a change follows from a specific vision/architecture/conventions
section, reference it in the commit message.
