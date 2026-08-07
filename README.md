# LazyCassandra

A terminal UI for Apache Cassandra, in the tradition of LazyGit, LazySQL, and k9s.

**Status:** pre-implementation, Phase 0 not started. There is no working tool
yet — this repo currently holds the design docs and the workspace skeleton.

## Docs

- [doc/vision.md](doc/vision.md) — why this exists, who it's for, scope, roadmap
- [doc/architecture.md](doc/architecture.md) — crate layout, state model, open decisions
- [doc/conventions.md](doc/conventions.md) — how the code should read

## Workspace

Hexagonal layout: `lc-domain` (pure core) ← `lc-inbound` (TUI) / `lc-outbound`
(Cassandra) ← `lc-app` (binary).

```
mise run build   # cargo build --workspace
mise run test    # fast test tiers
mise run lint    # clippy, warnings denied
mise run ci      # everything CI gates on
```
