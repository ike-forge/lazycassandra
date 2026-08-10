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

- `ctrl-d` and `ctrl-u`. The ctrl layer exists now and has three members:
  `ctrl-e`/`ctrl-y` in Table DDL
  (doc/specs/2026-08-10-ddl-selection-design.md §4.1) and `ctrl-r` in Queries
  (doc/specs/2026-08-10-query-execution-design.md). Vim's half-page motions
  still are not in it, and want the layer designed rather than a fourth arm
  added in passing (design §5).
- A long `Notice` still cannot scroll. One sentence is the assumption; a
  `DbError` carrying a paragraph would break it.

From the mouse input slice (doc/specs/2026-08-09-mouse-input-design.md):

- A wheel notch is three rows, which is a guess. `WHEEL_ROWS` in `update.rs`
  is the one place to change it after a few weeks of use.

From the queries editor slice (doc/specs/2026-08-09-queries-editor-design.md §9):

- Size the Queries panel by what is in it. It gets one content row at 80x24
  and 120x40 — the 3-row floor beats `QUERIES_PERCENT` until the terminal is
  about forty rows tall — so multi-line CQL is legible only by scrolling.
  The panel is meant to grow with its content and its focus, which is a
  layout change with its own spec, not a tweak to the percentage.
- The session log. architecture.md §4 calls this panel "statement entry and
  session log"; only the first half exists. When it lands, `State.queries`
  becomes a `QueriesPanel` holding both.
- Undo and redo, selection, clipboard, and word motions. The buffer has none.
- Schema-aware completion and syntax highlighting (vision.md §5.1). The
  `Token` path of architecture.md §6 is already the right shape for the
  second.
- An overlay up over a focused Queries breaks two promises, both because
  "focus is the mode" (design §3) does not yet weigh `state.overlay`. First,
  the hint bar's `esc leave` is momentarily false: `escape` prioritises the
  overlay, so `Esc` spends the press dismissing it instead of leaving, and the
  bar promises something the next keystroke will not do — the first binding
  where a modal can pre-empt what the bar claims, every other hint staying
  true under an overlay. Second, every other key the editor claims still
  reaches the buffer regardless of the overlay: typing is not lost, but
  `render.rs` deliberately draws no cursor while an overlay is up, so the
  text lands invisibly until the overlay is dismissed (nothing is corrupted
  and `Backspace` recovers it, but nothing on screen says the keystroke
  landed). The obvious guard — `state.overlay.is_none() && state.focus.takes_text()`
  before offering a key to the editor — would fix the second symptom but
  break the first kind of promise instead: `q` would quit and `t`/`a`/`c`/`u`
  would move focus while a modal is up, which is "focus is the mode" failing
  in the other direction. Closing either honestly means making the bar and
  the key route both overlay-aware together — `bar(focus)` becoming
  `bar(focus, has_overlay)`, and `handle_key` answering what an overlay does
  to every claimed key, not just `Esc` — which is a design change rather than
  a fix, and one decision closes both.
- `crates/lc-inbound/src/body.rs`'s `shifted`, the editor's horizontal
  scroll, snaps the offset `left` to a character boundary of the *cursor's*
  line and then applies that same offset to every drawn line. A line whose
  character boundaries fall elsewhere loses one cell more than `left` claims
  and draws one column left of where it belongs. Only visible with two or
  more content rows on screen, lines of mixed character width, and the view
  scrolled sideways — not reachable at the terminal sizes the panel gets
  today (§9's one-content-row table above), but it will matter once the
  dynamic-height slice lands. The fix is per-line boundary snapping, worth
  doing only once the panel can show more than one line.
- The pointer and the editor have not met. A click in the Queries panel
  focuses it but cannot position the text cursor — `click_at` in `update.rs`
  returns right after setting `state.focus` for `Panel::Ddl | Panel::Queries`
  — and the wheel does not scroll the buffer: `panel_scroll` has no arm for
  `Panel::Queries` because the editor stores no scroll offset to hand back,
  both of its offsets being derived from the cursor and the area per frame
  (design §5). Both gaps want the same decision: whether the editor gets a
  stored viewport of its own, the way the list panels do (list viewport
  design), or keeps deriving its offsets from the cursor and stays out of
  reach of the pointer. That is a design question, not a bug, and it is
  unanswered on purpose. Table DDL shares that `click_at` arm for a plainer
  reason: it already has a row cursor (ddl selection design), so wiring its
  click is the same `go_to` call Tables and Contents already make — an
  unwired binding, not an open question.

From the query execution slice (doc/specs/2026-08-10-query-execution-design.md):

- Running the buffer sends all of it as one statement. Splitting on statement
  boundaries, and running only the one under the cursor, needs a CQL parser —
  it lands in `Statement::typed`
  (doc/specs/2026-08-10-query-execution-design.md §3, §10).
- A destructive statement typed into the editor is refused rather than
  confirmed. The typed-confirmation flow vision.md §4 principle 6 requires
  replaces that refusal (design §5).
- A query's result does not page. Neither does a table read; `Effect::FetchNextPage`
  exists and nothing drives it. Both paths get it at once (design §10).

From the command palette slice (doc/specs/2026-08-09-command-palette-design.md §10):

- Search. The palette is named Commands and carries a selection precisely so
  this can land; it needs text input, which needs the mode design §7 defers.
- Execution. `⏎` selects a row that cannot be run, because a `Binding` is keys
  and a label. Making it runnable means feeding a synthesised `KeyPress` back
  through `handle_key` rather than growing a second dispatch point.
- Contextual ordering. The palette shows the same groups whatever has focus;
  vision.md §4 principle 1 promises a *contextual* map. It now lists `l`/`h`
  twice, once under each panel that resolves them, which is the honest shape for
  a flat catalogue and exactly the duplication a contextual one would collapse.

From the panel search slice (doc/specs/2026-08-10-panel-search-design.md §8):

- Search in Queries. `/` types a `/` there, and every other printable key is
  text too, so search has no unmodified key left to claim without breaking the
  panel-takes-text rule. The ctrl layer exists now and could hold one, but a
  `ctrl-/` would put search behind a different key in Queries than in the other
  three panels, which is what the *same key means the same thing in every
  panel* rule is for. The route in is `esc` then `/`, once Queries has a
  vim-style normal mode — that slice's decision, not this one's, since it also
  has to invent how one leaves the panel once `esc` no longer does that.

From the table display slice (doc/specs/2026-08-10-table-display-design.md §11):

- Expanded-cell inspection. `MAX_COLUMN_WIDTH` is 24 and horizontal scrolling
  reaches columns, not the tails of values, so a 36-character uuid is still
  ellipsised and nothing shows it in full. The cell cursor is the target such a
  view needs, and it now exists; the view itself wants a design of its own.
- Pinned key columns. Useful the moment a table is wide enough to scroll, and
  blocked on a rule for what happens when the pinned block alone does not fit.
- Column widths that survive a restart. They are recomputed from each page, so
  resizing one is not a thing to do — a preference needs a config system to hold
  it, and there is none.
- `lc_domain::grid::available(width, gutter)`. The width the grid body has after
  the gutter is spelled out in `update::grid_columns`, in `body::grid`, and again
  in two render tests. Four copies of one expression across two crates, agreeing
  by convention rather than by type.
- The horizontal thumb glyph has never been looked at on a real terminal. The
  bottom bar reuses the vertical bar's machinery with `━`; it is pinned by
  position, not by appearance.
