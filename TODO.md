- Add integration coverage for Cassandra 4.x. The fixture runs
  `cassandra:5.0.8` only.
- Test performance (for display and querying) when datasets are huge
- Health block refresh: reopening is the only reload. A key or a `Msg::Tick`
  interval needs deciding. SSTable count and percent
  repaired wait on a CQL source; 5.0.8 has none.
- Show the connection's posture at all times. Today it is named only in the
  connection picker, the palette's disabled rows, the bar and the refusal
  modal; nothing on the frame says "read-only" while you browse.

From the table-list review:

- A refresh binding. `describe` refuses the whole schema when a DDL change
  races its two reads, and only a successful drop re-issues
  `Effect::LoadSchema` — otherwise recovery is a restart.
- Untested case: ctrl/alt against `Enter`, `l` and `h` on the tree. Only
  `j` is pinned.

From the table-contents slice:

- `CqlValue::Timestamp` is now produced by nothing — the adapter formats
  timestamps to text because the domain has no calendar. Either give it one or
  drop the variant; an arm nothing constructs is a claim the model does not
  honour.
- `CqlValue::Map` is keyed by `String`, so a CQL map with non-textual keys
  degrades to rendered text. `Vec<(CqlValue, CqlValue)>` is the honest fix.
- `Effect::Cancel` is still inert: the `Cluster` port has no cancel, so a
  superseded read runs to completion on the cluster. Only the `in_flight` drop
  protects the state.

From the table metadata slice:

- Revisit the option layout order once it has been used for a few weeks.
  It is a first guess, and `RANKED` and `BULKY` in `model/schema.rs` are the
  place to change.
- `describe` now makes three reads, so the window for a DDL change to race it
  is wider than the earlier two-read implementation. Same failure, same recovery
  — the refresh binding above.

From the panel scrolling slice:

- A long `Notice` still cannot scroll. One sentence is the assumption; a
  `DbError` carrying a paragraph would break it.

From the mouse input slice:

- A wheel notch is three rows, which is a guess. `WHEEL_ROWS` in `update/pointer.rs`
  is the one place to change it after a few weeks of use.

From the queries editor slice:

- Undo and redo, selection, clipboard, and word motions. The buffer has none.
- `ctrl-f` lays the whole buffer out rather than the statement under the
  cursor, which `ctrl-r` now runs.
  Classification still reads each statement by its first verb.
- Completion inside a quoted identifier. `"My` offers nothing — the word
  scanner stops at the quote.
- Stepping between a statement shape's holes. The caret lands in the first and
  stays there; `Tab` is the accept key and cannot also be the next-hole key
  while a popup may still be up.
- `WITH` options are one list for keyspaces and tables alike. Telling
  `durable_writes` from `gc_grace_seconds` needs the resolver to carry which
  of the two the clause hangs off.
- The formatter's layout is fixed: two-column indent, upper-case keywords, one
  clause to a line. A preference would need a `[format]` section in `Config`.
- `Overlay::Message` over a focused Cql tab. Every other overlay claims keys
  before the editor; a message does not, so typed keys land in the buffer
  while no cursor is drawn, and the bar still promises `esc leave` although
  `Esc` dismisses the message first. Making the message claim keys fixes both.
- `crates/lc-inbound/src/body.rs`'s `shifted`, the editor's horizontal
  scroll, snaps the offset `left` to a character boundary of the *cursor's*
  line and then applies that same offset to every drawn line. A line whose
  character boundaries fall elsewhere loses one cell more than `left` claims
  and draws one column left of where it belongs. Reachable now that the panel
  grows with its buffer: lines of mixed character width, scrolled sideways.
  The fix is per-line boundary snapping.
- A click in the Cql tab focuses the panel but cannot position the text
  cursor, and the wheel does not scroll the buffer: `panel_scroll` answers
  `None` for Cql because the editor stores no scroll offset, both of its
  offsets being derived from the cursor and the area per frame.
  Whether the editor gets a stored viewport of its own, the way the list
  panels do, is a design question, unanswered on
  purpose.

From the query execution slice:

- `v`, `y` and `/` do not reach the Cql tab's statements: the band walks them,
  but nothing selects, yanks or searches over them.
- A destructive statement typed into the editor is refused rather than
  confirmed. `d` has a typed confirmation now; the buffer's statement names
  its target in text nothing parses, so there is nothing to ask the user to
  type back.

From the command palette slice:

- Contextual ordering. The palette shows the same groups whatever has focus;
  vision.md §4 principle 1 promises a *contextual* map.

From the panel search slice:

- Search in the Cql tab. `/` types a `/` there, and every other printable key
  is text too, so search has no unmodified key left to claim without breaking
  the panel-takes-text rule. A `ctrl-/` would put search behind a different
  key there than everywhere else. The Logs tab is searchable already.

From the table display slice:

- Pinned key columns. Useful the moment a table is wide enough to scroll, and
  blocked on a rule for what happens when the pinned block alone does not fit.
- Column widths that survive a restart. They grow as pages land and widen
  into the panel's spare room, but start over with every read; `ConfigSink`
  can write a key back now, but nothing holds a
  width.
- The horizontal thumb glyph has never been looked at on a real terminal. The
  bottom bar reuses the vertical bar's machinery with `━`; it is pinned by
  position, not by appearance.
