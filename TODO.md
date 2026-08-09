- Support/test older cassandra version (<5)
- Test performance (for display and querying) when datasets are huge
- Add readonly mode (make obvious visually)
- Write actions must all be obvisous (special color/ui)
- Display shortcuts on the UI (dynamic/contextualized) — the hint bar
  vision.md §7 Phase 0 promises; doc/shortcuts.md §4 is what it replaces

From the table-list review (doc/specs/2026-08-08-table-list-design.md):

- Give `TablesPanel` its own `down`/`up` taking `&Schema`, so cursor motion
  cannot be handed the wrong row count. Today `update.rs` reaches through the
  public `cursor` field with a caller-supplied `len`; `render.rs` already
  passes a keyspace count where a tree-row count is meant, correct only
  because those fixtures have no tables.
- A refresh binding. `describe` refuses the whole schema when a DDL change
  races its two reads, and nothing re-issues `Effect::LoadSchema` — recovery
  is a restart (table-list design §8).
- Untested spec cells: `l`/`Right` on a table row, and ctrl/alt against the
  three tree bindings. Both correct by construction, neither pinned.

From the table-contents slice (doc/specs/2026-08-09-table-contents-design.md §9):

- Paging. `LIMIT 200` is a wall, not a window: `Effect::FetchNextPage` and
  `Page::next` exist and nothing drives them.
- `CqlValue::Timestamp` is now produced by nothing — the adapter formats
  timestamps to text because the domain has no calendar. Either give it one or
  drop the variant; an arm nothing constructs is a claim the model does not
  honour.
- `CqlValue::Map` is keyed by `String`, so a CQL map with non-textual keys
  degrades to rendered text. `Vec<(CqlValue, CqlValue)>` is the honest fix.
- `Effect::Cancel` is still inert: the `Cluster` port has no cancel, so a
  superseded read runs to completion on the cluster. Only the `in_flight` drop
  protects the state.
- Grid columns are capped at 24 and anything past the right edge is not drawn.
  Horizontal navigation and column priority are unbuilt (vision.md §8).

From the table metadata slice (doc/specs/2026-08-09-table-metadata-design.md):

- Display `system_schema.columns`: the regular and static column list in the
  DDL panel. The data has been in `Table` since the table-list slice; only the
  projection and the rendering are missing.
- Revisit the option importance order once it has been used for a few weeks.
  It is a first guess, and `BY_IMPORTANCE` in `model/schema.rs` is the one
  place to change.
- `describe` now makes three reads, so the window for a DDL change to race it
  is wider than the table-list spec's §3 measured. Same failure, same recovery
  — the refresh binding above.

From the panel scrolling slice (doc/specs/2026-08-09-panel-scrolling-design.md):

- `ctrl-d` and `ctrl-u`. `handle_key` refuses every ctrl and alt press, so
  vim's half-page motions need that guard reworked and the test that pins
  "a modified key is not a binding" rewritten — worth doing when the ctrl
  layer is designed, not in passing (design §5).
- A long `Notice` still cannot scroll. One sentence is the assumption; a
  `DbError` carrying a paragraph would break it.

From the command palette slice (doc/specs/2026-08-09-command-palette-design.md §10):

- Search. The palette is named Commands and carries a selection precisely so
  this can land; it needs text input, which needs the mode design §7 defers.
- Execution. `⏎` selects a row that cannot be run, because a `Binding` is keys
  and a label. Making it runnable means feeding a synthesised `KeyPress` back
  through `handle_key` rather than growing a second dispatch point.
- Contextual ordering. The palette shows the same three groups whatever has
  focus; vision.md §4 principle 1 promises a *contextual* map.
