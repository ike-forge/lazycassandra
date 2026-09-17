- Support/test older cassandra version (<5). The fixture runs `cassandra:5.0.8`
  only.
- Test performance (for display and querying) when datasets are huge
- Show the connection's posture at all times. Today it is named only in the
  connection picker, the palette's disabled rows, the bar and the refusal
  modal; nothing on the frame says "read-only" while you browse.

From the table-list review (docs/superpowers/specs/2026-08-08-table-list-design.md):

- Give `TablesPanel` its own `down`/`up` taking `&Schema`, so cursor motion
  cannot be handed the wrong row count. Today `update.rs` reaches through the
  public `cursor` field with a caller-supplied `len`; `render.rs` already
  passes a keyspace count where a tree-row count is meant, correct only
  because those fixtures have no tables.
- A refresh binding. `describe` refuses the whole schema when a DDL change
  races its two reads, and only a successful drop re-issues
  `Effect::LoadSchema` — otherwise recovery is a restart (table-list design §8).
- Untested spec cell: ctrl/alt against `Enter`, `l` and `h` on the tree. Only
  `j` is pinned.

From the table-contents slice (docs/superpowers/specs/2026-08-09-table-contents-design.md §9):

- Paging. `LIMIT 200` is a wall, not a window: `Cluster::execute_paged` and
  `Page::next` exist and only export drives them.
- `CqlValue::Timestamp` is now produced by nothing — the adapter formats
  timestamps to text because the domain has no calendar. Either give it one or
  drop the variant; an arm nothing constructs is a claim the model does not
  honour.
- `CqlValue::Map` is keyed by `String`, so a CQL map with non-textual keys
  degrades to rendered text. `Vec<(CqlValue, CqlValue)>` is the honest fix.
- `Effect::Cancel` is still inert: the `Cluster` port has no cancel, so a
  superseded read runs to completion on the cluster. Only the `in_flight` drop
  protects the state.

From the table metadata slice (docs/superpowers/specs/2026-08-09-table-metadata-design.md):

- Revisit the option layout order once it has been used for a few weeks.
  It is a first guess, and `RANKED` and `BULKY` in `model/schema.rs` are the
  place to change.
- `describe` now makes three reads, so the window for a DDL change to race it
  is wider than the table-list spec's §3 measured. Same failure, same recovery
  — the refresh binding above.

From the panel scrolling slice (docs/superpowers/specs/2026-08-09-panel-scrolling-design.md):

- A long `Notice` still cannot scroll. One sentence is the assumption; a
  `DbError` carrying a paragraph would break it.

From the mouse input slice (docs/superpowers/specs/2026-08-09-mouse-input-design.md):

- A wheel notch is three rows, which is a guess. `WHEEL_ROWS` in `update.rs`
  is the one place to change it after a few weeks of use.

From the queries editor slice (docs/superpowers/specs/2026-08-09-queries-editor-design.md §9):

- Undo and redo, selection, clipboard, and word motions. The buffer has none.
- A CQL parser, and what waits on it: running the statement under the cursor
  and splitting a buffer on statement boundaries. `ctrl-f` lays the whole
  buffer out for the same reason `ctrl-r` runs all of it. Classification
  already lexes each `;`-separated statement, but still by its first verb.
- Completion inside a quoted identifier. `"My` offers nothing — the word
  scanner stops at the quote.
- Stepping between a statement shape's holes. The caret lands in the first and
  stays there; `Tab` is the accept key and cannot also be the next-hole key
  while a popup may still be up
  (docs/superpowers/specs/2026-08-17-completion-ranking-design.md §9).
- `WITH` options are one list for keyspaces and tables alike. Telling
  `durable_writes` from `gc_grace_seconds` needs the resolver to carry which
  of the two the clause hangs off (completion ranking design §9).
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
  offsets being derived from the cursor and the area per frame (design §5).
  Whether the editor gets a stored viewport of its own, the way the list
  panels do (list viewport design), is a design question, unanswered on
  purpose.

From the query execution slice (docs/superpowers/specs/2026-08-10-query-execution-design.md):

- Running the buffer sends all of it as one statement. Splitting on statement
  boundaries, and running only the one under the cursor, needs a CQL parser
  (design §3, §10).
- A destructive statement typed into the editor is refused rather than
  confirmed. `d` has a typed confirmation now; the buffer's statement names
  its target in text nothing parses, so there is nothing to ask the user to
  type back (design §5).

From the command palette slice (docs/superpowers/specs/2026-08-09-command-palette-design.md §10):

- Contextual ordering. The palette shows the same groups whatever has focus;
  vision.md §4 principle 1 promises a *contextual* map.

From the panel search slice (docs/superpowers/specs/2026-08-10-panel-search-design.md §8):

- Search in the Cql tab. `/` types a `/` there, and every other printable key
  is text too, so search has no unmodified key left to claim without breaking
  the panel-takes-text rule. A `ctrl-/` would put search behind a different
  key there than everywhere else. The Logs tab is searchable already.

From the table display slice (docs/superpowers/specs/2026-08-10-table-display-design.md §11):

- Pinned key columns. Useful the moment a table is wide enough to scroll, and
  blocked on a rule for what happens when the pinned block alone does not fit.
- Column widths that survive a restart. They are recomputed from each page;
  `ConfigSink` can write a key back now (theme persistence design), but
  nothing holds a width.
- The horizontal thumb glyph has never been looked at on a real terminal. The
  bottom bar reuses the vertical bar's machinery with `━`; it is pinned by
  position, not by appearance.

From the configuration slice (docs/superpowers/specs/2026-08-14-configuration-design.md §11):

- `grid.row_limit` parses and validates but nothing reads it:
  `update::ROW_LIMIT` is what applies, and the defaults disagree (500 against
  200).
