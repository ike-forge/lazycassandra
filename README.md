# LazyCassandra

A terminal UI for Apache Cassandra, in the tradition of LazyGit, LazySQL, and k9s.

**Status:** pre-implementation, Phase 0 not started. There is no working tool
yet — this repo currently holds the design docs and the workspace skeleton.

## Docs

- [docs/vision.md](docs/vision.md) — why this exists, who it's for, scope, roadmap
- [docs/architecture.md](docs/architecture.md) — crate layout, state model, open decisions
- [docs/conventions.md](docs/conventions.md) — how the code should read
- [docs/shortcuts.md](docs/shortcuts.md) — every binding, and the rules a new one must satisfy
- [docs/superpowers/specs/2026-08-08-cassandra-test-fixture-design.md](docs/superpowers/specs/2026-08-08-cassandra-test-fixture-design.md)
  — the local Cassandra the TUI is developed against

## Workspace

Hexagonal layout: `lc-domain` (pure core) ← `lc-inbound` (TUI) / `lc-outbound`
(Cassandra) ← `lc-app` (binary).

```
mise run build   # cargo build --workspace
mise run test    # fast test tiers
mise run check   # fmt, clippy with warnings denied, licences and advisories
mise run ci      # everything CI gates on
```

## Configuration

Two TOML files, layered lowest to highest: a user-level
`$XDG_CONFIG_HOME/lazycassandra/config.toml`, and a project
`.lazycassandra.toml` — the LazySQL model. **The project file is meant to be
committed to git**, so a teammate clones the repo, runs `lazycassandra`, and
is connected. Both are optional.

`lazycassandra --config <path>` adds a third layer above both, for a file
that belongs to neither the machine nor the checkout. It layers rather than
replaces: keys it leaves unstated still come from the files beneath it. A
path that is not there ends the launch rather than falling back.

`--connection <name>` opens that connection at startup instead of asking,
overriding `default_connection`. A name the config does not carry opens the
picker rather than something else.

A first run writes the user-level file if nothing is there: every setting
commented out at its default, with a line on what each one does. Delete it for
a fresh copy. Only the theme picker ever writes to it again, and only the
`theme` key.

```toml
theme = "default-dark"

[[connections]]
name     = "dev"
hosts    = ["127.0.0.1:9042"]
posture  = "read-only"
username = "cassandra"
password = { env = "DEV_CASSANDRA_PW" }
tls      = { ca = "~/ca.pem" }   # or `true` for the system roots; unset = plaintext
```

`tls` takes `ca`, `cert` and `key` (a client certificate), `verify`
(`full` | `chain` | `none`) and `server_name`. `chain` skips only the name
check, which self-hosted node certificates often fail.

A connection that states `cqlshrc` takes its login and its TLS settings
(`[connection] ssl`, `[ssl]`) from that file; a `tls` key beside it wins.

A file that gets something wrong does not stop the tool — the smallest
addressable unit is dropped and every drop is reported in a diagnostics
overlay at startup. See
[docs/superpowers/specs/2026-08-14-configuration-design.md](docs/superpowers/specs/2026-08-14-configuration-design.md)
for the full schema and the reasoning behind it.

## Clipboard

`y` copies the focused row by two routes at once, because neither reaches
everywhere:

- **OSC 52**, which asks the terminal. Needs a terminal that supports it
  (Windows Terminal, WezTerm, kitty, alacritty) and a multiplexer that
  forwards it (tmux with `set-clipboard on` — though some builds parse the
  sequence and forward nothing). This is the route that survives SSH onto a
  host with no display of its own.
- **A helper process**, which asks the platform: `clip.exe` under WSL,
  `wl-copy` under Wayland, `xclip` under X11, `pbcopy` on macOS. Whichever the
  environment offers, first that will run.

Where both work they are told the same line, so it does not matter which one
the clipboard listened to. On a host with neither — no display, no interop,
and a terminal that ignores OSC 52 — `y` still does nothing observable.

## Fixture cluster

A single-node Cassandra with three keyspaces covering the nominal case, every
CQL type, and a pile of identifier and terminal-rendering edge cases.

```
mise run db:up     # start and seed (about a minute on a cold volume)
mise run db:cqlsh  # poke at it
```

`mise run db:tls:up` starts a second node on 9142 (`LC_CASSANDRA_TLS_PORT`)
with TLS and `PasswordAuthenticator` (`cassandra`/`cassandra`), using a CA
generated into `docker/tls/certs`. `tests/tls.rs` runs against it.

Destroy both nodes, volumes included, with
`docker compose -f docker/compose.yaml --profile tls down -v`.

Set `LC_CASSANDRA_PORT` if something already holds 9042. It moves the port the
fixture publishes and the integration tier connects to; the application itself
reads `.lazycassandra.toml`, so a moved port means editing the `hosts` line
there to match.

`.lazycassandra.toml` at the repository root is what points a checkout at that
fixture. Without a config naming a connection the tool connects to nothing and
opens its connection picker instead — `ctrl-o` reopens it.
