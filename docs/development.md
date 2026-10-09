# Developing LazyCassandra

Read the [vision](vision.md), [architecture](architecture.md),
[conventions](conventions.md), and [shortcut reference](shortcuts.md) before
changing the app.

## Build and check

Tool versions are pinned in [mise.toml](../mise.toml). Install them with
`mise install`, then use the workspace tasks:

```sh
mise run build   # build the workspace
mise run run     # launch the TUI
mise run fmt     # format Rust sources
mise run check   # formatting, clippy, licences, advisories
mise run test    # fast domain, scenario, and render tiers
mise run ci      # everything CI gates on
```

The workspace separates the pure domain (`lc-domain`), terminal adapter
(`lc-inbound`), Cassandra/config adapters (`lc-outbound`), and binary
(`lc-app`). Dependencies point inward; see the architecture for the boundaries.

## Sample Cassandra cluster

The Docker fixture provides three keyspaces: ordinary application tables,
a CQL type zoo, and identifier/rendering edge cases.

```sh
mise run db:up     # start Cassandra and seed it
mise run db:cqlsh  # open cqlsh in the container
mise run test:it   # integration tests against the fixture
```

The repository's [.lazycassandra.toml](../.lazycassandra.toml) selects the
fixture as the default connection, with writes enabled. Starting the app from
this checkout therefore connects to the fixture automatically.

If port 9042 is occupied, set `LC_CASSANDRA_PORT` for the fixture tasks and
update the config's `hosts` entry to match. That variable configures the
fixture and integration tests, not the application.

See the [fixture design](superpowers/specs/2026-08-08-cassandra-test-fixture-design.md)
for its schema and seeding details.

## Clipboard

Copying uses both OSC 52 terminal escape sequences and an available platform
helper: `pbcopy` on macOS, `clip.exe` under WSL, `wl-copy` on Wayland, or
`xclip` on X11. Over SSH, the terminal must support OSC 52 and any multiplexer
must forward it; for tmux, check `set-clipboard on`. With neither route
available, the system clipboard will not receive the copied value.
