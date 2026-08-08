- Support/test older cassandra version (<5)
- Test performance (for display and querying) when datasets are huge
- Add readonly mode (make obvious visually)
- Write actions must all be obvisous (special color/ui)
- Write up formal shortcut document
- Display shortcuts on the UI (dynamic/contextualized)

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

From the table metadata slice (doc/specs/2026-08-09-table-metadata-design.md):

- Display `system_schema.columns`: the regular and static column list in the
  DDL panel. The data has been in `Table` since the table-list slice; only the
  projection and the rendering are missing.
- Scroll and focus the DDL panel. It holds seven rows at 80x24 and about 38 at
  100x100, and a table's options run past both — a map-valued property alone
  wraps to four or five lines. Everything below the fold is unreachable today,
  because focus movement between panels does not exist (metadata design §7).
- Revisit the option importance order once it has been used for a few weeks.
  It is a first guess, and `BY_IMPORTANCE` in `model/schema.rs` is the one
  place to change.
- `describe` now makes three reads, so the window for a DDL change to race it
  is wider than the table-list spec's §3 measured. Same failure, same recovery
  — the refresh binding above.

