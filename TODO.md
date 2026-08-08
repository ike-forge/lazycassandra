- Support/test older cassandra version (<5)
- Test performance (for display and querying) when datasets are huge
- Add readonly mode (make obvious visually)
- Write actions must all be obvisous (special color/ui)
- Write up formal shortcut document
- Display shortcuts on the UI (dynamic/contextualized)

From the table-list review (doc/specs/2026-08-08-table-list-design.md):

- `mise run test:it` cannot pass from the host: the fixture node advertises
  `broadcast_address` 172.18.0.2, which a VM-backed container runtime does not
  route to. `connect` succeeds on the published port, then the driver's pool
  times out, so `describe` is what fails. Affects every integration test, not
  just the new ones. Fix in `docker/compose.yaml` (advertise a routable
  address) or add a task that runs the tier inside the cluster's netns.
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

