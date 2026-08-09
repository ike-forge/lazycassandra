# LazyCassandra — Vision

> A terminal UI for Apache Cassandra, in the tradition of LazyGit, LazySQL, and k9s.
> Fast for experts. Legible for everyone else.

**Status:** pre-implementation. This document is the north star, not a spec.
**Last updated:** 2026-08-09

---

## 1. Why this exists

Cassandra's tooling asks you to hold too much in your head.

`cqlsh` is a bare REPL. It will happily let you run a query that scans every
partition in the cluster, and tells you nothing until the coordinator times out.
It has no memory of your schema, no navigation, no sense of place.

`nodetool` lives in a completely separate mental model — a different binary, a
different vocabulary, a different session. The thing you need to know about a
table (is it compacting? is it full of tombstones?) is one tool away from the
thing you're doing (reading from that table).

Generic SQL GUIs — DBeaver, TablePlus — treat Cassandra as if it were Postgres
with a funny accent. They show you a table and a row count. They do not show you
the partition key, which is the only thing that actually matters.

DataStax Studio and friends are heavy, browser-based, and slow to reach for.

None of these are bad tools. But there is an obvious gap: **nothing in the
Cassandra world feels like `lazygit`.** Nothing you keep open in a split, drive
entirely by keyboard, and reach for reflexively a hundred times a day.

That is the tool we want to build.

## 2. The thesis

Cassandra is unusual among databases: **it punishes modelling mistakes at query
time, and it punishes them silently.** A query that works fine on your laptop
with 200 rows will melt a production node at 200 million. The failure mode is
not an error — it is a latency cliff, or a coordinator OOM, or a tombstone
avalanche three weeks later.

Every other database lets you be sloppy and fixes it with an index. Cassandra
does not. It demands you understand the physical layout of your data.

And yet the physical layout is the thing the tooling shows you least. The
partition key is buried in a `DESCRIBE` dump. Whether a table is drowning in
tombstones lives in a different binary. What's actually in a partition takes a
hand-written query and a guess at the paging. The information exists — it is
just slow and awkward enough to reach that nobody reaches for it.

So the central design commitment of LazyCassandra is:

**Make the shape of your data continuously visible, and make exploring it fast
enough that you actually do it.**

Partition key, clustering columns, replication, table health: ambient properties
of the interface, present while you work, rather than facts you go and look up
after something breaks. Navigation that costs a keystroke instead of a
hand-written query. State that's on screen instead of one tool away.

The bet is that most Cassandra mistakes are not failures of knowledge but
failures of visibility — people who broadly know how partitions work, shipping
something bad anyway because the relevant fact was three commands away at the
moment they needed it. Close that distance and a lot of the problem goes with
it.

This is also, quietly, how the tool teaches. Not by grading your queries — by
making the model impossible to ignore. Spend a week with the partition key
always on screen and you will have internalised the most important thing about
Cassandra without anyone lecturing you about it.

## 3. Who it's for

**The on-call engineer** who needs to know, right now, whether that table is
compacting and why p99 went to hell. Wants speed and density.

**The application developer** who writes CQL occasionally and doesn't think in
partitions day to day. Needs to see what a table actually looks like before
writing against it, without a detour through schema files. Wants context.

**The data engineer / DBA** who lives in this cluster. Wants keyboard-driven
everything, scriptability, and no ceremony.

These are not three products. The whole design problem — see §4 — is serving all
three from one interface without the tool feeling patronising to the third or
hostile to the second.

## 4. The interface philosophy

This is the part of the project we care most about, and the part with the most
research still to do. See §8 for the open questions.

The guiding idea: **a single interface with a gradient, not a beginner mode and
an expert mode.**

Modal "simple/advanced" toggles are a failure of design. They fork the product,
double the maintenance, and — worst — they mean the novice's practice does not
build toward expertise. Everything they learned in simple mode is discarded at
the boundary. The interfaces we admire (LazyGit, Vim, Midnight Commander, k9s)
do the opposite: a beginner and an expert are using *the same* interface, and
the beginner's every action is already the expert's action, just slower and with
more scaffolding visible.

### Principles

**1. Discoverability is ambient, not modal.**
The available actions are always on screen, contextual to what's focused —
LazyGit's bottom bar is the reference implementation. You should never need to
open a manual to find out what you can do to the thing you are looking at. `?`
gives the full contextual map, not a wall of every binding in the program.

**2. The same key, at every level of skill.**
The novice reads the hint bar and presses `d`. The expert presses `d` without
looking. There is no second, faster path that the novice is missing — the fast
path *is* the visible path. Depth is reached by composing (counts, ranges,
filters, chained actions), not by discovering a hidden alternative.

**3. Show the CQL. Always.**
Every action the tool takes is expressible as CQL, and the tool shows it — in a
statement preview before execution, and in a session log after. This is the
single most important teaching mechanism in the product. Using LazyCassandra
should make you better at Cassandra, not dependent on LazyCassandra. It also
means a user can always escape the abstraction: copy the CQL, paste it in
`cqlsh`, put it in a migration.

**4. Panels are places.**
Spatial stability is a memory aid. The Midnight Commander and LazyGit lineage:
persistent named panels in fixed positions, navigated with predictable motions.
You build muscle memory for *where things are*, not just what keys do. Layout
does not rearrange itself under you.

**5. Wear the terminal's clothes.**
The background is yours and LazyCassandra never overrides it; body text keeps
your terminal's own foreground. The tool sits inside your setup instead of
repainting over it — no imported colour scheme, nothing that clashes with the
split next to it.

Highlights are ours. Where something has to be *marked* — an accent, an icon,
a property name — we pick the colour, truecolor included. What that buys is
bounded on purpose: meaning still belongs to the sixteen ANSI colours, because
those are the ones that survive a terminal whose palette is unusable. Write
posture, severity and key structure are drawn in them and always will be. An
accent marks where to look; it never carries the meaning.

Themes are supported for people who want them, and one may ship for terminals
with poor palettes. But the out-of-the-box experience is deliberately the least
disruptive one, and the default must stay legible on light and dark backgrounds
alike, because it cannot know which it is on.

**6. Destructive things are obvious, reversible, or confirmed — pick two.**
Read-only is the default posture on any connection not explicitly marked
writable. Writes are visibly distinct. Drops and truncates require typed
confirmation. The tool should never be the reason a keyspace disappeared.

**7. Speed is a feature of the interface, not just the engine.**
Sub-frame response to keypresses. Nothing blocks the UI thread. A query running
against a slow cluster does not freeze the tool — it shows progress and can be
cancelled. Perceived latency is a UX property, and Rust is chosen partly for
this.

**8. Density with a floor of legibility.**
Terminal real estate is scarce and experts want it packed. But a screen that
requires you to already know what it means is a screen that only serves experts.
Every dense view has a label, and every abbreviation is expandable.

## 5. Scope

Four areas, in priority order. Each is only worth building if it's better than
reaching for the existing tool.

### 5.1 Data & query browsing — *the core*

The reason to open the tool. Navigate keyspaces → tables → partitions → rows.
A first-class CQL editor with schema-aware completion. Result grids that page
properly through large partitions rather than pretending everything fits in
memory. Row-level inspection and editing. Query history that persists.

The differentiator: the partition key and clustering columns are never
implicit. When you're looking at a table, you're looking at its physical shape.

### 5.2 Schema management

Create, alter, and drop keyspaces, tables, indexes, UDTs, and materialized
views. Visualise replication strategy and per-datacenter factors. Render the
primary key structure so it's readable at a glance rather than parsed out of a
`DESCRIBE` dump.

Every schema change previews its CQL (see §4, principle 3) and can be exported
as a migration.

### 5.3 Cluster operations & observability

Node status and the token ring. Compaction and repair state. Table-level metrics
— SSTable counts, tombstone ratios, read/write latency. Enough that the on-call
engineer doesn't have to leave for `nodetool`, and enough that a table's health
is visible from the table view itself rather than a separate screen.

### 5.4 Query analysis & safety

Static analysis of a query before it runs. `ALLOW FILTERING`. Missing partition
key restriction. Unbounded `IN` clauses. Multi-partition reads. Known
tombstone-heavy access patterns.

Presented as explanation, not refusal: *what* is wrong, *why* it's expensive in
Cassandra specifically, and what the alternative usually is.

Deliberately last, and deliberately modest. This is a small set of well-known
patterns, not a query optimiser — LazyCassandra does not model your cluster or
predict your latency. If the earlier phases do their job, the visible data model
catches most of this before a warning ever fires.

## 6. Configuration

Config is a `.lazycassandra.yml` in the project root, layered under a user-level
config in `$XDG_CONFIG_HOME/lazycassandra/`. The LazySQL model: **the project
file is meant to be committed to git.**

That makes one rule non-negotiable: **the config file holds references to
secrets, never secrets.** Credentials resolve from environment variables, files
on disk, or an external command — the committed file names where a password
comes from, and never contains one. A config that would leak if pushed to a
public repo is a design bug.

The project file describes shared environments — the dev cluster everyone on the
team uses — so a new engineer clones the repo, runs `lazycassandra`, and is
connected. Personal and production connections live in the user-level config.

Connections carry an explicit posture: read-only, writable, or protected. The
interface reflects it visibly at all times, so you always know which cluster you
are about to be destructive on.

## 7. Roadmap

Phases, not dates. Each phase should be independently useful — the tool should
be worth using at the end of Phase 1.

**Phase 0 — Foundations.** Connection handling, config loading and layering,
credential resolution, schema introspection. The TUI shell: panel layout,
focus model, contextual keybinding bar, help system. No features yet, but the
interface skeleton is right.

**Phase 1 — Browse.** Keyspace/table/partition navigation. Result grid with real
paging. CQL editor with schema-aware completion. Statement preview and session
log. Query history. *This is the minimum viable tool.*

**Phase 2 — Understand.** Primary key and replication visualisation. Query
static analysis and warnings (§5.4). Table health surfaced inline. The
teaching layer.

**Phase 3 — Change.** Row editing. Schema DDL with preview and migration export.
Write-posture enforcement and confirmation flows.

**Phase 4 — Operate.** Node and ring view, compaction and repair state, metrics.
The k9s-adjacent surface.

**Phase 5 — Extend.** User-supplied themes (the terminal-inheriting default
lands in Phase 0), user-defined keybindings, saved queries, plugin
or scripting surface. Possibly ScyllaDB support, if the driver story is clean.

## 8. Open research questions

The UI is the hard part and it is not solved. These are the questions we
actually need to answer, and they should be answered with prototypes and real
users, not in a document.

- **How does the interface reveal depth over time without a mode switch?** Do
  hints fade with use? Does the tool track what you already know? Is adaptive UI
  a good idea at all, or does it break the spatial stability of principle 4?
- **What is the right primary spatial metaphor?** LazyGit's panel stack, k9s's
  single-resource-view-with-drilldown, and Midnight Commander's dual pane are
  three different answers. Cassandra's hierarchy (cluster → keyspace → table →
  partition → row) may want its own.
- **How do you render a partition key so a novice understands it in one look?**
  This is a genuine information-design problem and probably the highest-leverage
  visual in the product.
- **How much of a result set can a terminal usefully show?** Wide tables with
  blobs and collections break naive grids. Column priority, horizontal
  navigation, and detail views all need testing.
- **How do warnings avoid becoming noise?** A tool that warns on everything gets
  ignored on everything. Severity model, dismissal, and per-project
  configuration need thought.
- **Where does the CQL editor sit on the modal/non-modal question?** Vim
  keybindings are expected by a chunk of the audience and alienating to another.

### Prior art to study properly

`lazygit` (contextual hints, panel model, keybinding density) · `k9s` (resource
navigation, live state, command mode) · `lazysql` (config model, connection
management) · `LazyVim` (progressive disclosure of a hostile-by-default tool) ·
Midnight Commander (spatial stability, function-key model) · `htop` (dense
real-time display that a novice can still read) · `tig`, `gitui`, `bottom`,
`atuin`.

## 9. Non-goals

- **Not a GUI, and not a web app.** Terminal only.
- **Not a generic database client.** Cassandra's model is the point. Supporting
  Postgres would mean abandoning §2.
- **Not an ETL, migration, or backup tool.** It inspects and operates; it does
  not own your data lifecycle.
- **Not a monitoring system.** It shows live cluster state; it does not store
  history, alert, or replace Prometheus.
- **Not multi-vendor at launch.** Apache Cassandra 4.x/5.x. ScyllaDB is a
  plausible Phase 5 addition. Astra/DSE are not planned.
- **Not a replacement for understanding Cassandra.** The tool teaches; it does
  not paper over the data model.

## 10. Technical direction

**Rust.** Chosen for the latency floor (principle 7), for a single static binary
with no runtime, and because a long-lived TUI holding cluster state benefits
from the type system. `ratatui` for rendering. Driver selection is deliberately
open pending Phase 0 evaluation.

Architecturally: the TUI layer must not know how to talk to Cassandra, and the
Cassandra layer must not know there is a TUI. A headless core that can be driven
by tests — and eventually by a script or a non-interactive command — is the
constraint that keeps both halves honest.
