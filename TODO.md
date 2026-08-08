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
- Focus has no direct jumps (`1`–`4`); `Tab`/`Shift-Tab` cycling is all there
  is, and two of the four panels it reaches are still blank.
- Grid columns are capped at 24 and anything past the right edge is not drawn.
  Horizontal navigation and column priority are unbuilt (vision.md §8).
