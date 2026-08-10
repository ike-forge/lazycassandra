# Table DDL Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Table DDL panel a cursor over the rows it describes, highlighted across every line a wrapped value occupies, and sink the bulky table options below the compact ones.

**Architecture:** The domain gains a `DdlPanel` holding a `Cursor` (counting described rows) beside the `Scroll` it already has (counting wrapped lines). Only the renderer can convert between the two units, so it reports the selected row's line span back through `Geometry`, and `measured()` follows it — one frame after the cursor moves, by construction. Everything else is mechanical: a `selected` field on `PanelBody::Details`, a style patch in the details renderer, and widening `Binding` from `Key` to `KeyPress` so the hint bar can name `^e`.

**Tech Stack:** Rust 2024, `ratatui` + `crossterm`, cargo workspace of four crates. Tests are `#[cfg(test)] mod tests` in-file for the domain, `TestBackend` snapshots for the renderer.

**Spec:** [`doc/specs/2026-08-10-ddl-selection-design.md`](../specs/2026-08-10-ddl-selection-design.md). Section references below (`§4.1`) point into it.

## Global Constraints

- **No `unsafe`, no FFI.** `unsafe_code = "forbid"` is set workspace-wide.
- **`lc-domain` must not depend on `tokio`, `ratatui`, or `crossterm`.** Nothing in this plan adds a dependency to any crate.
- **Zero warnings, zero failing or ignored tests.** `mise run ci` must be clean at the end of every task. Any `#[allow(...)]` must be item-scoped with a reason.
- **No narrating comments.** Comments are for edge cases, past bugs, upstream defects, and Cassandra/protocol semantics. Do not explain `?`, iterators, or lifetimes. Prefer a rename or an extracted function over a comment.
- **Determinism.** No wall-clock, network, filesystem, randomness, or `HashMap` iteration order in the fast tiers.
- **Conventional Commits**, imperative, lowercase, no trailing period. Reference the spec section a change follows from.
- **Commit freely, including to `master`. Never `pull`, never `push`.**
- Every commit body ends with:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  ```
- Run the full gate with `mise run ci`. Faster inner loop: `cargo test -p lc-domain`, `cargo test -p lc-inbound`.

---

## File Structure

| File | Responsibility after this plan |
|---|---|
| `crates/lc-domain/src/model/schema.rs` | `RANKED` / `BULKY` option order; `in_display_order` yields three chunks (Task 1) |
| `crates/lc-domain/src/state.rs` | `Span`, `Scroll::follow_span`, `Geometry::ddl_selection`, `DdlPanel` (Tasks 2, 3) |
| `crates/lc-domain/src/view/details.rs` | `Details::rows`/`row_count`, `of()`, `selected` in the projection (Tasks 3, 4) |
| `crates/lc-domain/src/view/mod.rs` | `PanelBody::Details { selected }`, `ddl_rows()` (Tasks 3, 4) |
| `crates/lc-domain/src/update.rs` | Ddl routed as a cursor panel; span follow in `measured`; `ctrl_key` (Tasks 3, 6) |
| `crates/lc-domain/src/model/input.rs` | `KeyPress::ctrl` (Task 5) |
| `crates/lc-domain/src/view/hints.rs` | `Binding.keys: &[KeyPress]`, `^` naming (Task 5) |
| `crates/lc-domain/src/commands.rs` | Palette catalogue gains a `Table DDL` group (Task 6) |
| `crates/lc-inbound/src/details.rs` | `lay_out()` returning styled lines and the selection span (Task 4) |
| `crates/lc-inbound/src/body.rs` | `Body.selection` (Task 4) |
| `crates/lc-inbound/src/render.rs` | Collects the span into `Geometry` (Task 4) |
| `doc/shortcuts.md`, `doc/architecture.md` | Reconciled with what is bound (Task 7) |

---

## Task 1: Sink the bulky options below the compact ones

Spec §2. Self-contained: no other task depends on it, and it can be reviewed alone.

**Files:**
- Modify: `crates/lc-domain/src/model/schema.rs` (the `BY_IMPORTANCE` const and `TableOptions::in_display_order`)
- Modify: `crates/lc-domain/src/view/details.rs` (the test `options_arrive_in_importance_order_rather_than_alphabetically`)

**Interfaces:**
- Consumes: nothing.
- Produces: `TableOptions::in_display_order(&self) -> impl Iterator<Item = (&str, &str)>` — signature unchanged, order changed.

- [ ] **Step 1: Write the failing test**

Add to `crates/lc-domain/src/model/schema.rs`'s `mod tests`. Build `TableOptions` directly rather than through a helper, so the test states its own input:

```rust
    #[test]
    fn the_bulky_options_are_listed_below_even_the_ones_this_build_has_never_heard_of() {
        let options = TableOptions::new(BTreeMap::from([
            ("comment".to_owned(), "Order history.".to_owned()),
            ("compaction".to_owned(), "{'class': 'SizeTieredCompactionStrategy'}".to_owned()),
            ("compression".to_owned(), "{'chunk_length_in_kb': '16'}".to_owned()),
            ("gc_grace_seconds".to_owned(), "864000".to_owned()),
            ("zzz_option_from_the_future".to_owned(), "on".to_owned()),
        ]));

        assert_eq!(
            options
                .in_display_order()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            [
                "gc_grace_seconds",
                "zzz_option_from_the_future",
                "comment",
                "compaction",
                "compression",
            ],
            "an unranked option must not land between the compact table and the blocks below it"
        );
    }
```

If `std::collections::BTreeMap` is not already imported in that test module, add `use std::collections::BTreeMap;` to it.

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p lc-domain the_bulky_options_are_listed_below`

Expected: FAIL — the actual order leads with `comment`, then `gc_grace_seconds`, `compaction`, `compression`, and puts `zzz_option_from_the_future` last.

- [ ] **Step 3: Re-cut the list**

In `crates/lc-domain/src/model/schema.rs`, replace the `BY_IMPORTANCE` const — **including its doc comment**, which currently claims importance order and would become a lie — with:

```rust
/// Table options in the order the panel lists them, most worth seeing first.
///
/// A layout rule, not an importance rule. These are the properties whose
/// values fit on one line, so they read as a scannable block; the three in
/// [`BULKY`] wrap and would split it
/// (doc/specs/2026-08-10-ddl-selection-design.md §2).
const RANKED: &[&str] = &[
    "default_time_to_live",
    "gc_grace_seconds",
    "caching",
    "bloom_filter_fp_chance",
    "speculative_retry",
    "additional_write_policy",
    "read_repair",
    "cdc",
    "crc_check_chance",
    "min_index_interval",
    "max_index_interval",
    "memtable",
    "memtable_flush_period_in_ms",
    "allow_auto_snapshot",
    "incremental_backups",
    "flags",
    "id",
    "extensions",
];

/// The options long enough to wrap the panel, drawn last.
///
/// Below the unranked tail rather than above it, so a wide option from a
/// Cassandra major this build predates cannot land between the compact table
/// and the blocks under it. `comment` loses the top of the panel to buy that,
/// which is a real cost and a deliberate one (design §2).
const BULKY: &[&str] = &["comment", "compaction", "compression"];

fn listed(name: &str) -> bool {
    RANKED.contains(&name) || BULKY.contains(&name)
}
```

Then replace `in_display_order`:

```rust
    /// Ranked properties first, then whatever this build has not heard of,
    /// alphabetically, then the bulky ones.
    ///
    /// The middle chunk is what keeps a cluster whose properties predate or
    /// postdate [`RANKED`] fully visible rather than half-shown.
    pub fn in_display_order(&self) -> impl Iterator<Item = (&str, &str)> {
        let named = |names: &'static [&'static str]| {
            names
                .iter()
                .filter_map(|name| self.get(name).map(|value| (*name, value)))
        };

        let rest = self
            .by_name
            .iter()
            .filter(|(name, _)| !listed(name))
            .map(|(name, value)| (name.as_str(), value.as_str()));

        named(RANKED).chain(rest).chain(named(BULKY))
    }
```

- [ ] **Step 4: Fix the projection test that asserted the old order**

In `crates/lc-domain/src/view/details.rs`, the test `options_arrive_in_importance_order_rather_than_alphabetically` asserts `comment`, `gc_grace_seconds`, `bloom_filter_fp_chance`. Rename it and flip the expectation:

```rust
    #[test]
    fn the_compact_options_are_listed_before_the_ones_that_wrap() {
        let details = details(&browsing(orders()));

        assert_eq!(
            section(&details, "Options").rows,
            [
                DetailRow::Property {
                    name: "gc_grace_seconds".to_owned(),
                    value: "864000".to_owned(),
                },
                DetailRow::Property {
                    name: "bloom_filter_fp_chance".to_owned(),
                    value: "0.01".to_owned(),
                },
                DetailRow::Property {
                    name: "comment".to_owned(),
                    value: "Order history.".to_owned(),
                },
            ]
        );
    }
```

- [ ] **Step 5: Run the whole gate**

Run: `mise run ci`

Expected: PASS. If a render snapshot in `crates/lc-inbound/src/render.rs` asserts the old option order, update its expectation to the new one — that is the change working, not a regression. Grep for `gc_grace` and `comment` under `crates/lc-inbound` before assuming there is none.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-domain/src/model/schema.rs crates/lc-domain/src/view/details.rs crates/lc-inbound/src/render.rs
git commit -m "refactor(ddl): order table options by layout rather than importance

The three options long enough to wrap were splitting the compact block a
reader scans. They now sort last, below even the unranked tail, so nothing a
future Cassandra major reports can land between the table and the blocks
(doc/specs/2026-08-10-ddl-selection-design.md §2).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Task 2: `Span`, `Scroll::follow_span`, and the geometry field

Spec §5.4. Pure domain primitives with no callers yet — reviewable as arithmetic.

**Files:**
- Modify: `crates/lc-domain/src/state.rs`

**Interfaces:**
- Consumes: `Scroll` (existing, private `offset`).
- Produces:
  - `pub struct Span { pub first: usize, pub lines: usize }`, `Copy + Default + PartialEq + Eq + Debug`
  - `Scroll::follow_span(&mut self, span: Span, height: usize)`
  - `Geometry::with_ddl_selection(self, ddl_selection: Span) -> Geometry` (const, `#[must_use]`)
  - `Geometry::ddl_selection(self) -> Span` (const, `#[must_use]`)
  - `Geometry::new` keeps its existing three-argument signature.

- [ ] **Step 1: Write the failing tests**

Add a `mod tests` block to `crates/lc-domain/src/state.rs` if there is none, or append to the existing one:

```rust
    #[test]
    fn following_a_span_already_in_view_moves_nothing() {
        let mut scroll = Scroll::default();
        scroll.follow_span(Span { first: 2, lines: 3 }, 10);

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn following_a_span_below_the_viewport_pulls_its_last_line_to_the_bottom() {
        let mut scroll = Scroll::default();
        scroll.follow_span(Span { first: 12, lines: 3 }, 10);

        assert_eq!(scroll.offset(), 5, "lines 5..15 are drawn, ending on the span");
    }

    #[test]
    fn following_a_span_above_the_viewport_pulls_its_first_line_to_the_top() {
        let mut scroll = Scroll::default();
        scroll.to_end(100, 10);
        scroll.follow_span(Span { first: 4, lines: 2 }, 10);

        assert_eq!(scroll.offset(), 4);
    }

    #[test]
    fn a_span_taller_than_the_panel_shows_its_head_rather_than_its_tail() {
        let mut scroll = Scroll::default();
        scroll.follow_span(Span { first: 20, lines: 30 }, 10);

        assert_eq!(
            scroll.offset(),
            20,
            "a property's name is the half worth reading; its tail without it is not"
        );
    }

    #[test]
    fn a_span_of_no_lines_is_nothing_to_follow() {
        let mut scroll = Scroll::default();
        scroll.to_end(100, 10);
        let before = scroll.offset();

        scroll.follow_span(Span::default(), 10);

        assert_eq!(scroll.offset(), before);
    }

    #[test]
    fn a_geometry_carries_the_selected_ddl_rows_span() {
        let span = Span { first: 7, lines: 4 };

        assert_eq!(
            Geometry::new([1, 2, 3, 4], 40, 5)
                .with_ddl_selection(span)
                .ddl_selection(),
            span
        );
        assert_eq!(
            Geometry::default().ddl_selection(),
            Span::default(),
            "a frame that drew no selection reports none"
        );
    }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p lc-domain follow_span`

Expected: FAIL to compile — `Span` and `follow_span` do not exist.

- [ ] **Step 3: Add `Span`**

In `crates/lc-domain/src/state.rs`, immediately above `pub struct Scroll`:

```rust
/// Where a selected row was drawn, counted in the lines a viewport scrolls
/// over.
///
/// The Table DDL panel is the one place a cursor and an offset count in
/// different units: the cursor walks described rows, the offset walks the
/// wrapped lines they occupy, and only the renderer knows the conversion
/// (doc/specs/2026-08-10-ddl-selection-design.md §5.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Span {
    /// The first line the row occupies.
    pub first: usize,
    /// How many it occupies. Zero means no row was drawn.
    pub lines: usize,
}
```

- [ ] **Step 4: Add `Scroll::follow_span`**

Inside `impl Scroll`, beneath the existing `follow`:

```rust
    /// Pulls the viewport the shortest distance that puts `span` inside it.
    ///
    /// [`Self::follow`]'s counterpart for a selection that occupies more than
    /// one line. A span taller than the panel cannot be made to fit, so its
    /// first line goes to the top: a property drawn tail-first has its name
    /// scrolled off, which is the half that says what the tail is.
    pub const fn follow_span(&mut self, span: Span, height: usize) {
        if span.lines == 0 || height == 0 {
            return;
        }

        if span.first < self.offset || span.lines >= height {
            self.offset = span.first;
            return;
        }

        let last = span.first + span.lines - 1;

        if last >= self.offset + height {
            self.offset = last + 1 - height;
        }
    }
```

- [ ] **Step 5: Add the geometry field**

Add `ddl_selection: Span` to `struct Geometry` between `ddl_lines` and `command_rows`. Leave `Geometry::new` at three arguments — roughly fifteen call sites pass three, and none of them cares about a selection — and add a builder plus an accessor to `impl Geometry`:

```rust
    /// The span the frame drew the Ddl selection at.
    ///
    /// Split from [`Self::new`] because almost every caller is a test that has
    /// no selection to describe, and a fourth positional `Span::default()` at
    /// each of them says nothing.
    #[must_use]
    pub const fn with_ddl_selection(self, ddl_selection: Span) -> Self {
        Self {
            heights: self.heights,
            ddl_lines: self.ddl_lines,
            ddl_selection,
            command_rows: self.command_rows,
        }
    }

    /// Where the Ddl panel drew its selected row.
    ///
    /// The second thing only the renderer can produce, for the same reason as
    /// [`Self::ddl_lines`]: it is a function of the wrapping.
    #[must_use]
    pub const fn ddl_selection(self) -> Span {
        self.ddl_selection
    }
```

`Geometry::new` must now name the field explicitly:

```rust
        Self {
            heights,
            ddl_lines,
            ddl_selection: Span { first: 0, lines: 0 },
            command_rows,
        }
```

(`Span::default()` is not const-callable in a `const fn`; the literal is.)

- [ ] **Step 6: Run the tests**

Run: `cargo test -p lc-domain`

Expected: PASS, including the six new ones.

- [ ] **Step 7: Run the gate and commit**

```bash
mise run ci
git add crates/lc-domain/src/state.rs
git commit -m "feat(domain): let a viewport follow a multi-line span

The Ddl cursor will count described rows while its offset counts wrapped
lines. Span is the conversion the renderer reports back, and follow_span is
what a Scroll does with one (doc/specs/2026-08-10-ddl-selection-design.md
§5.4).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Task 3: A cursor over the described rows

Spec §3, §4, §5.1, §5.3. The panel gains a cursor and the motion keys move it. Nothing is drawn differently yet — that is Task 4 — so this task is verified entirely at the domain tier.

**Files:**
- Modify: `crates/lc-domain/src/state.rs` (add `DdlPanel`, change `State.ddl`)
- Modify: `crates/lc-domain/src/view/details.rs` (extract `of()`, add `Details::rows`/`row_count`)
- Modify: `crates/lc-domain/src/view/mod.rs` (add `ddl_rows`)
- Modify: `crates/lc-domain/src/update.rs` (route Ddl as a cursor panel; follow the span in `measured`)

**Interfaces:**
- Consumes: `Span`, `Scroll::follow_span`, `Geometry::ddl_selection` (Task 2).
- Produces:
  - `pub struct DdlPanel { pub cursor: Cursor, pub scroll: Scroll }`, `Copy + Default`
  - `State.ddl: DdlPanel` — **every existing `state.ddl.<scroll method>` becomes `state.ddl.scroll.<method>`**
  - `Details::rows(&self) -> impl Iterator<Item = &DetailRow>`
  - `Details::row_count(&self) -> usize`
  - `lc_domain::view::ddl_rows(state: &State) -> usize`

- [ ] **Step 1: Write the failing tests**

Two in `crates/lc-domain/src/view/details.rs`'s `mod tests`, which owns the `orders()` fixture:

```rust
    #[test]
    fn the_rows_the_cursor_walks_are_the_described_ones_and_no_headings() {
        let details = details(&browsing(orders()));

        assert_eq!(
            details.row_count(),
            6,
            "one partition column, two clustering, three options — and no heading"
        );
        assert_eq!(
            details.rows().nth(1),
            Some(&DetailRow::Column {
                name: "order_date".to_owned(),
                order: Some(ClusteringOrder::Descending),
                cql_type: "date".to_owned(),
            }),
            "down from the last partition column lands on the first clustering one"
        );
    }

    #[test]
    fn the_projection_carries_the_panels_own_selection() {
        let mut state = browsing(orders());
        state.ddl.cursor.go_to(3, 6);

        let PanelBody::Details { selected, .. } = ddl_body(&state) else {
            panic!("a selected table has something to describe");
        };

        assert_eq!(selected, 3);
    }
```

And three in `crates/lc-domain/src/update.rs`'s `mod tests`, next to the existing Ddl tests. They derive their expectations from `ddl_rows` rather than hardcoding the fixture's shape:

```rust
    #[test]
    fn motion_in_the_ddl_panel_walks_the_rows_it_describes() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        let rows = crate::view::ddl_rows(&state);
        assert!(rows > 2, "the fixture describes a key and some options");

        for _ in 0..rows + 3 {
            press(&mut state, Key::Char('j'));
        }
        assert_eq!(state.ddl.cursor.index(), rows - 1, "j stops on the last row");

        press(&mut state, Key::Home);
        assert_eq!(state.ddl.cursor.index(), 0);

        press(&mut state, Key::End);
        assert_eq!(state.ddl.cursor.index(), rows - 1);
    }

    #[test]
    fn describing_another_node_opens_the_ddl_panel_at_its_first_row() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        press(&mut state, Key::End);
        assert!(state.ddl.cursor.index() > 0);

        state.focus = Panel::Tables;
        press(&mut state, Key::Char('k'));

        assert_eq!(state.ddl.cursor.index(), 0);
        assert_eq!(state.ddl.scroll.offset(), 0);
    }

    #[test]
    fn a_measurement_pulls_the_viewport_onto_the_selected_row() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;

        update(
            &mut state,
            Msg::Measured(
                Geometry::new([8, 4, 8, 1], 40, 0)
                    .with_ddl_selection(Span { first: 20, lines: 2 }),
            ),
        );

        assert_eq!(
            state.ddl.scroll.offset(),
            18,
            "the row measured at lines 20..22 has to be on screen in a four-line panel"
        );
    }
```

Add `use crate::state::Span;` to the update test module's imports if `state::*` is not already glob-imported there.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p lc-domain ddl`

Expected: FAIL to compile — `DdlPanel`, `Details::rows`, `Details::row_count`, `view::ddl_rows`, and the `selected` field do not exist.

- [ ] **Step 3: Add `DdlPanel` and repoint `State.ddl`**

In `crates/lc-domain/src/state.rs`, beneath `Scroll`:

```rust
/// The Table DDL panel: a cursor over the rows it describes, and a viewport
/// over the lines they wrapped to.
///
/// The two count in different units — see [`Span`] — which is why this panel
/// is not in `update`'s `LISTS` and follows a span rather than an index.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DdlPanel {
    pub cursor: Cursor,
    pub scroll: Scroll,
}
```

Change the `ddl` field on `State`, replacing its current doc comment (which says the panel has no row to act on):

```rust
    /// The Table DDL panel's cursor and viewport.
    pub ddl: DdlPanel,
```

Now build and let the compiler find every use: `cargo build -p lc-domain 2>&1 | head -40`. Each `state.ddl.<method>` becomes `state.ddl.scroll.<method>` — in `update.rs` this is the `scroll()` Ddl branch, the subject-change reset in `update()`, and the clamp in `measured()`; in `view/details.rs` it is `state.ddl.offset()`.

- [ ] **Step 4: Give `Details` its rows, and the view its count**

In `crates/lc-domain/src/view/details.rs`, add beneath the `Details` struct:

```rust
impl Details {
    /// Every row the panel describes, in the order it draws them — which is
    /// the order the cursor walks. A heading is not a row: it labels the group
    /// beneath it, and there is nothing a cursor on one could mean
    /// (doc/specs/2026-08-10-ddl-selection-design.md §3).
    pub fn rows(&self) -> impl Iterator<Item = &DetailRow> {
        self.sections.iter().flat_map(|section| section.rows.iter())
    }

    #[must_use]
    pub fn row_count(&self) -> usize {
        self.sections.iter().map(|section| section.rows.len()).sum()
    }
}
```

Extract the projection so the count and the body share one source. Replace `body`:

```rust
pub(super) fn body(state: &State) -> PanelBody {
    match of(state) {
        Some(details) => scrolled(state, details),
        None => PanelBody::Blank,
    }
}

/// What the Tables cursor is on, described — or nothing, when it is on
/// nothing.
pub(super) fn of(state: &State) -> Option<Details> {
    let schema = state.schema()?;

    Some(match state.tables.selected(schema)? {
        Node::Table { keyspace, table } => of_table(keyspace, table),
        Node::Keyspace { keyspace, .. } => of_keyspace(keyspace),
    })
}
```

In `crates/lc-domain/src/view/mod.rs`, beside `view_model`:

```rust
/// How many rows the Table DDL panel describes.
///
/// Built by the same projection the panel draws rather than counted a second
/// way: two row counts that can disagree is exactly the drift `tree.rs`'s
/// single flattening exists to prevent.
#[must_use]
pub fn ddl_rows(state: &State) -> usize {
    details::of(state).map_or(0, |details| details.row_count())
}
```

- [ ] **Step 5: Route Ddl as a cursor panel**

In `crates/lc-domain/src/update.rs`:

1. **Delete the whole `if panel == Panel::Ddl { … return; }` block** at the head of `scroll()`, and rewrite that function's doc comment — it says "a viewport in Ddl, a selection elsewhere", which stops being true:

```rust
/// Applies `motion` to the focused panel's selection.
///
/// The focused panel and no other. A wheel names the panel under the pointer
/// and moves its viewport directly ([`wheel`]), so nothing but a key reaches
/// here.
///
/// In Ddl the height is counted in wrapped lines and the cursor in rows, so a
/// page overshoots on a run of wrapped values. Accepted rather than corrected:
/// the fix needs a line-to-row map from the renderer, and `home`/`end` are
/// exact regardless (design §4).
```

2. `row_count` gains its Ddl arm, and its doc comment loses the claim that Ddl never reaches it:

```rust
/// How many rows `panel`'s *cursor* moves over. Queries never arrives here —
/// the editor claims the motion keys first (queries editor design §3) — so
/// its arm is unreachable in practice and kept for exhaustiveness.
fn row_count(state: &State, panel: Panel) -> usize {
    match panel {
        Panel::Tables => tree_rows(state),
        Panel::Ddl => crate::view::ddl_rows(state),
        Panel::Contents => state.contents.row_count(),
        Panel::Queries => 0,
    }
}
```

3. `panel_cursor` gains `Panel::Ddl => Some(&mut state.ddl.cursor)`, leaving `Panel::Queries => None`.

4. `cursor_index` gains `Panel::Ddl => state.ddl.cursor.index()`, leaving `Panel::Queries => 0`.

5. The subject-change reset in `update()` resets both:

```rust
    if described(state) != subject {
        state.ddl.cursor.to_start();
        state.ddl.scroll.to_start();
    }
```

6. Beside the `clamp_lists(state)` call in `update()`, clamp the Ddl cursor. Add it inside `clamp_lists` itself, with a comment saying why it is not in the loop:

```rust
    // Not in the loop above: `LISTS` is the panels whose offsets are followed
    // by index, and Ddl's offset counts lines while its cursor counts rows
    // (design §5.1). The clamp is unconditional for the same reason the loop
    // is — a schema reload that shrinks a table changes no geometry, so no
    // measurement arrives to clamp against.
    state.ddl.cursor.clamp_to(crate::view::ddl_rows(state));
```

7. In `measured()`, replace the existing Ddl clamp block with a follow and a clamp:

```rust
    // Follows here rather than in `update`'s epilogue because the epilogue's
    // geometry describes the *previous* frame, and so the previously selected
    // row: there is no correct offset for it to compute (design §5.4).
    //
    // Unconditional, and safe from the wheel without a guard: a wheel changes
    // no height, no wrapped-line count and no span, so `lc-app` sends no
    // measurement at all. The same covers ctrl-e and ctrl-y.
    let before = state.ddl.scroll;
    let height = geometry.height(Panel::Ddl);
    state.ddl.scroll.follow_span(geometry.ddl_selection(), height);
    state.ddl.scroll.clamp_to(geometry.ddl_lines(), height);

    if state.ddl.scroll != before {
        state.mark_dirty();
    }
```

8. `scroll_len`'s `Panel::Ddl => state.geometry.ddl_lines()` **stays** — the wheel scrolls the viewport in lines and must keep doing so.

9. `click_at`'s `Panel::Ddl | Panel::Queries => Vec::new()` **stays**. The hit map resolves lines for this panel, not rows, so a click cannot name a row; the comment there is still accurate. Selecting by click is out of scope (spec §7).

- [ ] **Step 6: Repair the two tests this invalidates**

`a_panel_with_no_rows_swallows_motion_rather_than_passing_it_on` states a premise that is now false — Ddl has rows. Keep what it was really guarding and rename it:

```rust
    #[test]
    fn motion_in_one_panel_never_reaches_another() {
        let mut state = read_shop_orders();
        let tree_row = state.tables.cursor.index();

        state.focus = Panel::Ddl;
        press(&mut state, Key::Char('j'));

        assert_eq!(state.tables.cursor.index(), tree_row);
        assert_eq!(state.contents.cursor.index(), 0);
    }
```

The `sized` helper builds a `Geometry` with no selection, which is still what its callers want — leave it. Then run `cargo test -p lc-domain 2>&1 | tail -30` and fix any other test that asserted the Ddl offset moved on `j`: those now assert a cursor move instead, and the offset moves only once a measurement arrives.

- [ ] **Step 7: Run the gate**

Run: `mise run ci`

Expected: PASS. `lc-inbound` still compiles because `PanelBody::Details` has not changed shape yet.

- [ ] **Step 8: Commit**

```bash
git add crates/lc-domain
git commit -m "feat(ddl): move a cursor over the rows the panel describes

The panel was focusable and drew nothing to say so. Motion now walks the
described rows — headings skipped, so the last partition column steps to the
first clustering one — and the viewport follows the span the renderer reports
(doc/specs/2026-08-10-ddl-selection-design.md §3, §4, §5).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Task 4: Draw the selection, and report where it landed

Spec §3, §5.2, §5.4. This is the task that makes the feature visible.

**Files:**
- Modify: `crates/lc-domain/src/view/mod.rs` (`PanelBody::Details` gains `selected`)
- Modify: `crates/lc-domain/src/view/details.rs` (`scrolled` fills it)
- Modify: `crates/lc-inbound/src/details.rs` (`lay_out`, styling, span)
- Modify: `crates/lc-inbound/src/body.rs` (`Body.selection`)
- Modify: `crates/lc-inbound/src/render.rs` (collect the span into `Geometry`)

**Interfaces:**
- Consumes: `Span` (Task 2), `DdlPanel`, `Details::rows` (Task 3).
- Produces:
  - `PanelBody::Details { details: Details, selected: usize, offset: usize }`
  - `lc_inbound::details::lay_out(details: &Details, selected: usize, width: usize, theme: Theme) -> Laid`, where `pub(crate) struct Laid { pub(crate) lines: Vec<Line<'static>>, pub(crate) selection: Span }`
  - `lc_inbound::details::render(frame, area, details, selected, offset, theme) -> (crate::body::Extent, Span)`
  - `crate::body::Body { extent, unit, selection: Span }`

- [ ] **Step 1: Write the failing render tests**

In `crates/lc-inbound/src/render.rs`'s `mod tests`, beside the existing DDL tests. Reuse whatever helper those tests already use to build a state with `shop.orders` selected (`a_selected_tables_shape_is_drawn_above_its_options` shows the pattern) and whatever `reversed(row)` helper the selection tests use; if `reversed` is scoped to another test module, lift it or write the equivalent buffer read inline.

```rust
    #[test]
    fn every_line_of_a_selected_property_is_highlighted() {
        let mut state = /* the state `a_selected_tables_shape_is_drawn_above_its_options` builds */;
        state.focus = Panel::Ddl;
        // The comment is last and long enough to wrap (design §2).
        state.ddl.cursor.to_end(lc_domain::view::ddl_rows(&state));

        let buffer = draw(&state);
        let highlighted = highlighted_rows(&buffer);

        assert!(
            highlighted.len() > 1,
            "a wrapped value is one cursor stop, so all of its lines are the selection"
        );
        assert!(
            highlighted.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "the lines of one row are contiguous, got {highlighted:?}"
        );
    }

    #[test]
    fn a_section_heading_is_never_drawn_as_the_selection() {
        let mut state = /* same */;
        state.focus = Panel::Ddl;
        state.ddl.cursor.to_start();

        let buffer = draw(&state);

        for row in highlighted_rows(&buffer) {
            let text = row_text(&buffer, row);
            assert!(
                !text.trim_end().ends_with("Partition Key") && !text.starts_with("Options"),
                "row {row} is a heading: {text:?}"
            );
        }
    }

    #[test]
    fn the_frame_reports_where_it_drew_the_ddl_selection() {
        let mut state = /* same */;
        state.focus = Panel::Ddl;
        state.ddl.cursor.go_to(0, lc_domain::view::ddl_rows(&state));

        let span = geometry_of(WIDTH, HEIGHT, &state).ddl_selection();

        assert_eq!(span.lines, 1, "the first partition column fits on one line");
        assert_eq!(
            span.first,
            highlighted_rows(&draw(&state))[0] - /* the body's first row within the frame */ 0,
            "the reported span is where the highlight was actually drawn"
        );
    }
```

`highlighted_rows(&buffer) -> Vec<usize>` and `row_text(&buffer, row) -> String` are helpers to add to that test module if absent. `Theme::TerminalDefault` maps `Token::Selected` to `Style::new().add_modifier(Modifier::REVERSED)` (`crates/lc-inbound/src/theme.rs:75`), so the first collects every row index whose first content cell carries `Modifier::REVERSED`; the second concatenates a row's cell symbols. The existing `reversed(row)` helper in that module already does the first for one row — prefer reusing it over a second reader.

For the third test, resolve the body's offset within the frame from the existing `geometry_of` helper's own arithmetic rather than hardcoding `0` — read it before writing the assertion. If that proves awkward, assert only `span.lines == 1 && span.first == 2` (heading, blank, then the `Partition Key` heading at 2 and its column at 3 → `first == 3`), derived by reading `lay_out`'s output directly in a unit test of `lay_out` instead. **A unit test of `lay_out` is the better test here** — prefer it:

```rust
    #[test]
    fn lay_out_reports_the_span_of_the_row_it_highlighted() {
        let details = /* build a Details with one partition column and one long comment */;
        let laid = lay_out(&details, 0, 28, Theme::TerminalDefault);

        assert_eq!(
            laid.selection,
            lc_domain::state::Span { first: 3, lines: 1 },
            "heading, blank, section heading, then the first column"
        );

        let laid = lay_out(&details, details.row_count() - 1, 28, Theme::TerminalDefault);
        assert!(laid.selection.lines > 1, "the comment wraps");
        assert_eq!(
            laid.selection.first + laid.selection.lines,
            laid.lines.len(),
            "the last row's span ends where the body does"
        );
    }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p lc-inbound ddl`

Expected: FAIL to compile — `lay_out`, `Laid`, and `state.ddl.cursor` in a `PanelBody::Details` context do not exist yet.

- [ ] **Step 3: Carry the selection through the projection**

In `crates/lc-domain/src/view/mod.rs`, add the field to `PanelBody::Details` and extend its doc comment:

```rust
    /// A heading and grouped rows: what is selected, described.
    ///
    /// `selected` indexes the rows in the order [`Details::rows`] yields them,
    /// the same meaning it has on [`PanelBody::Tree`] and [`PanelBody::Grid`].
    ///
    /// `offset` is the first line drawn, counted in wrapped lines. Only the
    /// renderer can produce those; the domain scrolls by them on the strength
    /// of the count `Msg::Measured` brings back (panel scrolling design §2).
    Details {
        details: Details,
        selected: usize,
        offset: usize,
    },
```

In `crates/lc-domain/src/view/details.rs`:

```rust
fn scrolled(state: &State, details: Details) -> PanelBody {
    PanelBody::Details {
        details,
        selected: state.ddl.cursor.index(),
        offset: state.ddl.scroll.offset(),
    }
}
```

- [ ] **Step 4: Lay out the body with the selection styled**

In `crates/lc-inbound/src/details.rs`, replace `pub(crate) fn lines` with `lay_out`. First find its other callers — `rg 'details::lines' crates/` — and repoint them at `lay_out(..).lines`.

```rust
use lc_domain::state::Span;

/// A body's lines, and where the selected row landed among them.
pub(crate) struct Laid {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) selection: Span,
}

/// Every line the body occupies at `width`, with the selected row highlighted
/// across all of its own.
///
/// The span is what the frame reports back as `Geometry::ddl_selection`, so
/// the domain can bound an offset over text it cannot wrap itself
/// (doc/specs/2026-08-10-ddl-selection-design.md §5.4).
pub(crate) fn lay_out(details: &Details, selected: usize, width: usize, theme: Theme) -> Laid {
    let mut lines = vec![Line::raw(truncate(&details.heading, width)), Line::raw("")];
    let mut selection = Span::default();
    let mut row = 0;

    for (index, section) in details.sections.iter().enumerate() {
        // The one row spent on separation, and only where it buys the split
        // between what the table is and what it is configured to do. A section
        // that opens the panel already has the heading's blank line above it.
        if index > 0 && section.group == Group::Settings {
            lines.push(Line::raw(""));
        }

        let style = section
            .token
            .map_or_else(Style::new, |token| theme.style(token));

        lines.push(Line::styled(truncate(&section.heading, width), style));

        for detail in &section.rows {
            let mut drawn = row_lines(detail, width, style, theme);

            if row == selected {
                let highlight = theme.style(Token::Selected);
                drawn = drawn.into_iter().map(|line| patched(line, highlight)).collect();
                selection = Span {
                    first: lines.len(),
                    lines: drawn.len(),
                };
            }

            lines.extend(drawn);
            row += 1;
        }
    }

    Laid { lines, selection }
}

/// `style` applied over each span's own, rather than under it: a property's
/// name carries `PropertyName`, and a line-level style would sit beneath that
/// and never reach the cells it covers.
fn patched(line: Line<'static>, style: Style) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|span| Span2::styled(span.content, span.style.patch(style)))
            .collect::<Vec<_>>(),
    )
}
```

Name collision: this file already imports `ratatui::text::Span`, and now needs `lc_domain::state::Span`. Import the domain one as `use lc_domain::state::Span;` and alias ratatui's at its use site — `use ratatui::text::{Line, Span as TextSpan};` — then use `TextSpan` in `patched` and in `property_lines`. Do **not** invent `Span2`; that placeholder above is there to make the collision impossible to miss.

`section_lines` becomes unused — delete it rather than leaving it dead.

Then `render`:

```rust
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    details: &Details,
    selected: usize,
    offset: usize,
    theme: Theme,
) -> (crate::body::Extent, Span) {
    let laid = lay_out(details, selected, usize::from(area.width), theme);
    let total = laid.lines.len();
    // `Paragraph::scroll` is u16, and a body past 65535 lines is unreachable;
    // the extent keeps the offset itself, so the thumb stays right regardless.
    let scroll = u16::try_from(offset).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(laid.lines).scroll((scroll, 0)), area);

    (
        crate::body::Extent {
            position: offset,
            total,
            visible: usize::from(area.height),
        },
        laid.selection,
    )
}
```

A selection scrolled out of view needs no guard here — unlike `List` and `Table`, `Paragraph` honours the offset it is given and clips, so lines above it are simply not drawn. That is what makes the "not highlighted off screen" rule hold for free; the module doc should say so, because the next reader will look for the guard the other two panels have.

- [ ] **Step 5: Carry the span out through `Body`**

In `crates/lc-inbound/src/body.rs`, add to `struct Body`:

```rust
    /// Where the body drew its selection, in the unit [`Extent`] counts in.
    ///
    /// Only the Ddl body reports one: it is the one panel whose selection
    /// spans more than a line, and so the one whose offset cannot be derived
    /// from a row index.
    pub(crate) selection: Span,
```

Fill it in `render`'s match — `Span::default()` for `Tree`, `Editor` and `Grid`, and for `Details`:

```rust
        PanelBody::Details {
            details,
            selected,
            offset,
        } => {
            let (extent, selection) = crate::details::render(frame, area, details, *selected, *offset, theme);
            Some(Body {
                extent,
                unit: Unit::Lines,
                selection,
            })
        }
```

`Unit::Lines` is unchanged and deliberate: the panel still scrolls in lines, and the hit map must keep resolving lines rather than rows, so a click stays inert (spec §7).

- [ ] **Step 6: Feed it into `Geometry`**

In `crates/lc-inbound/src/render.rs`, add `let mut ddl_selection = Span::default();` beside `let mut ddl_lines = 0;`, set it inside the existing `if panel.panel == Panel::Ddl` arm, and build:

```rust
        geometry: Geometry::new(heights, ddl_lines, command_rows)
            .with_ddl_selection(ddl_selection),
```

- [ ] **Step 7: Run the tests**

Run: `cargo test -p lc-inbound && cargo test -p lc-domain`

Expected: PASS. Existing DDL snapshot tests may need their `PanelBody::Details` destructuring updated for the new field; that is mechanical.

- [ ] **Step 8: Run the gate and commit**

```bash
mise run ci
git add crates/lc-domain crates/lc-inbound
git commit -m "feat(ddl): highlight the selected row across every line it occupies

A wrapped property is one cursor stop, so all of its lines take Token::Selected
— which is what makes a comment or a compaction map read as a block. The
renderer reports the span back so the viewport can follow a selection it alone
can locate (doc/specs/2026-08-10-ddl-selection-design.md §5.2, §5.4).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Task 5: Let a binding name a modifier

Spec §4.1. Mechanical and wide. Kept separate from Task 6 so the behavioural change is reviewable without the churn.

**Files:**
- Modify: `crates/lc-domain/src/model/input.rs` (`KeyPress::ctrl`)
- Modify: `crates/lc-domain/src/view/hints.rs` (`Binding`, `label`, `name`)
- Modify: `crates/lc-domain/src/state.rs` (the four `*_HINTS` consts)
- Modify: `crates/lc-domain/src/commands.rs` (the catalogue consts)
- Modify: `crates/lc-domain/src/update.rs` (the tests that sweep every binding)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `KeyPress::ctrl(key: Key) -> KeyPress` (const, `#[must_use]`)
  - `Binding { keys: &'static [KeyPress], action: &'static str }`
  - `hints::label(keys: &[KeyPress]) -> String`, rendering ctrl as `^e`

- [ ] **Step 1: Write the failing test**

In `crates/lc-domain/src/view/hints.rs`'s `mod tests`:

```rust
    #[test]
    fn a_binding_behind_ctrl_is_named_with_a_caret() {
        assert_eq!(
            label(&[
                KeyPress::ctrl(Key::Char('e')),
                KeyPress::ctrl(Key::Char('y')),
            ]),
            "^e/^y"
        );
        assert_eq!(label(&[KeyPress::new(Key::Char('j'))]), "j");
    }
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p lc-domain a_binding_behind_ctrl`

Expected: FAIL to compile — `KeyPress::ctrl` does not exist and `label` takes `&[Key]`.

- [ ] **Step 3: Add `KeyPress::ctrl`**

In `crates/lc-domain/src/model/input.rs`, inside `impl KeyPress`:

```rust
    /// `key` with ctrl held.
    ///
    /// A binding of its own, never a modifier on the plain key's meaning —
    /// `handle_key` refuses ctrl wholesale except for the arms that name it
    /// (shortcuts.md §2).
    #[must_use]
    pub const fn ctrl(key: Key) -> Self {
        Self {
            key,
            modifiers: Modifiers {
                ctrl: true,
                alt: false,
                shift: false,
            },
        }
    }
```

- [ ] **Step 4: Widen `Binding`**

In `crates/lc-domain/src/view/hints.rs`:

```rust
use crate::model::{Key, KeyPress};

pub struct Binding {
    pub keys: &'static [KeyPress],
    pub action: &'static str,
}
```

Split `name` into the key's own name and the modifier prefix:

```rust
pub(crate) fn label(keys: &[KeyPress]) -> String {
    keys.iter().copied().map(name).collect::<Vec<_>>().join("/")
}

fn name(press: KeyPress) -> String {
    let key = key_name(press.key);

    if press.modifiers.ctrl {
        format!("^{key}")
    } else {
        key
    }
}

/// Named rather than drawn, for the keys with no printable form: the arrow
/// glyphs collide with each other, and a terminal font may draw any of them
/// as a box (hint bar design §3).
fn key_name(key: Key) -> String {
    // ...the existing body of `name`, unchanged...
}
```

Update `bar`'s `?` filter, which currently compares against a `Key`:

```rust
        .filter(|binding| fits || !binding.keys.contains(&KeyPress::new(Key::Char('?'))))
```

- [ ] **Step 5: Convert every binding literal**

`rg 'keys: &\[' crates/lc-domain/src` finds all of them: the four `*_HINTS` and `GLOBAL`/`QUIT` in `state.rs` and `hints.rs`, and `GLOBAL`/`MOTION`/`TABLES` in `commands.rs`. Each `Key::X` becomes `KeyPress::new(Key::X)`; add `KeyPress` to each file's imports. No action strings change.

- [ ] **Step 6: Repair the binding sweeps**

`crates/lc-domain/src/update.rs`'s tests press every binding in `CATALOGUE` and in the hint bar to prove none names a dead key. They currently iterate `&[Key]` and call the `press(&mut state, key)` helper. They now iterate `&[KeyPress]` and must go through `update` directly so the modifier survives:

```rust
    fn press_binding(state: &mut State, binding: KeyPress) -> Vec<Effect> {
        update(state, Msg::Input(binding))
    }
```

Replace the `press(&mut state, key)` call inside each sweep with `press_binding(&mut state, keypress)`. Leave the existing `press(&mut state, Key)` helper alone — every other test uses it.

- [ ] **Step 7: Run the gate and commit**

Run: `mise run ci`

Expected: PASS, with no behaviour change — `^e` is nameable but nothing binds it yet.

```bash
git add crates/lc-domain
git commit -m "refactor(hints): key a binding by a press rather than a key

A Binding could not express a modifier, so the bar could not name one and the
sweep could not press one. Widening it to KeyPress is what lets ctrl-e be
bound and shown rather than buried in a design document
(doc/specs/2026-08-10-ddl-selection-design.md §4.1).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Task 6: `Ctrl-e` and `Ctrl-y` scroll without moving the cursor

Spec §4.1.

**Files:**
- Modify: `crates/lc-domain/src/update.rs` (`handle_key` guard, new `ctrl_key`)
- Modify: `crates/lc-domain/src/state.rs` (`DDL_HINTS`)
- Modify: `crates/lc-domain/src/commands.rs` (a `Table DDL` group)

**Interfaces:**
- Consumes: `DdlPanel` (Task 3), `KeyPress::ctrl` and the widened `Binding` (Task 5).
- Produces: no new public API.

- [ ] **Step 1: Write the failing tests**

In `crates/lc-domain/src/update.rs`'s `mod tests`:

```rust
    fn ctrl(state: &mut State, key: Key) -> Vec<Effect> {
        update(state, Msg::Input(KeyPress::ctrl(key)))
    }

    #[test]
    fn ctrl_e_and_ctrl_y_scroll_the_ddl_panel_without_moving_its_cursor() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        sized(&mut state, [8, 4, 8, 1], 40);

        ctrl(&mut state, Key::Char('e'));

        assert_eq!(state.ddl.scroll.offset(), 1);
        assert_eq!(
            state.ddl.cursor.index(),
            0,
            "the viewport moved and the selection did not"
        );

        ctrl(&mut state, Key::Char('y'));

        assert_eq!(state.ddl.scroll.offset(), 0);
    }

    #[test]
    fn ctrl_e_belongs_to_the_ddl_panel_alone() {
        let mut state = read_shop_orders();
        state.focus = Panel::Tables;
        sized(&mut state, [8, 4, 8, 1], 40);

        ctrl(&mut state, Key::Char('e'));

        assert_eq!(state.ddl.scroll.offset(), 0);
        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn an_open_palette_swallows_ctrl_e() {
        let mut state = read_shop_orders();
        state.focus = Panel::Ddl;
        sized(&mut state, [8, 4, 8, 1], 40);
        press(&mut state, Key::Char('?'));

        ctrl(&mut state, Key::Char('e'));

        assert_eq!(state.ddl.scroll.offset(), 0);
    }
```

The existing `ctrl-j is a different binding, not a slower j` and `ctrl-a is a binding that does not exist yet, not a slower a` tests must keep passing unchanged. Do not touch them — they are the guard on this carve-out staying narrow.

Note `sized` builds a `Geometry` with `ddl_lines = 40` and a Ddl height of `4`, so there are lines to scroll over.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p lc-domain ctrl_e`

Expected: FAIL — the offset stays `0`; `handle_key` returns early on any ctrl press.

- [ ] **Step 3: Carve out ctrl in `handle_key`**

Replace the guard in `crates/lc-domain/src/update.rs`:

```rust
    // alt turns a key into a separate binding and none is bound, so it is
    // refused rather than reinterpreted. ctrl is refused the same way, past
    // the two arms `ctrl_key` claims. shift is not checked: a shifted letter
    // already arrives as a different Key::Char, and a shifted arrow is left as
    // a synonym for the plain one so it stays free for a future "extend
    // selection" binding.
    if press.modifiers.alt {
        return Vec::new();
    }

    if press.modifiers.ctrl {
        return ctrl_key(state, press.key);
    }
```

Add the handler beside `scroll`:

```rust
/// The bindings behind `ctrl`, which is all of them for one panel.
///
/// Table DDL alone has a cursor and a viewport that count in different units,
/// so it alone can have a selection its viewport has left — and so it alone
/// needs a way to move one without the other (design §4.1).
fn ctrl_key(state: &mut State, key: Key) -> Vec<Effect> {
    if state.focus != Panel::Ddl || matches!(state.overlay, Some(Overlay::Commands(_))) {
        return Vec::new();
    }

    let len = state.geometry.ddl_lines();
    let height = state.geometry.height(Panel::Ddl);

    match key {
        Key::Char('e') => state.ddl.scroll.down(len, height),
        Key::Char('y') => state.ddl.scroll.up(),
        _ => {}
    }

    Vec::new()
}
```

`handle_key` already called `state.mark_dirty()` before this point, so nothing else is needed.

- [ ] **Step 4: Name it in the hint bar**

In `crates/lc-domain/src/state.rs`, rewrite `DDL_HINTS`. `j`/`k` now move a selection, so `scroll` moves to the keys that actually scroll:

```rust
const DDL_HINTS: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Char('j')), KeyPress::new(Key::Char('k'))],
        action: "move",
    },
    Binding {
        keys: &[
            KeyPress::ctrl(Key::Char('e')),
            KeyPress::ctrl(Key::Char('y')),
        ],
        action: "scroll",
    },
    Binding {
        keys: &[KeyPress::new(Key::PageDown), KeyPress::new(Key::PageUp)],
        action: "page",
    },
    Binding {
        keys: &[KeyPress::new(Key::Home), KeyPress::new(Key::End)],
        action: "ends",
    },
];
```

- [ ] **Step 5: Name it in the palette**

In `crates/lc-domain/src/commands.rs`, add a group and register it. Actions are cut to twenty characters because `MIN_W` derives from the longest — `scroll one line` is fifteen:

```rust
const DDL: &[Binding] = &[Binding {
    keys: &[
        KeyPress::ctrl(Key::Char('e')),
        KeyPress::ctrl(Key::Char('y')),
    ],
    action: "scroll one line",
}];
```

Add to `CATALOGUE`, after `Tables`:

```rust
    Group {
        heading: "Table DDL",
        bindings: DDL,
    },
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p lc-domain`

Expected: PASS. The palette grew by two rows; a test asserting an exact palette row count or an exact `selected` index after `PageDown` may need its number updated — check `an_open_palette_takes_the_motion_the_panel_would_have`, whose `PageDown` expectation of `15` holds only while the catalogue is longer than that.

Then `cargo test -p lc-inbound` — the palette's rendered height and the `MIN_W` derivation both read the catalogue.

- [ ] **Step 7: Run the gate and commit**

```bash
mise run ci
git add crates/lc-domain
git commit -m "feat(ddl): scroll the panel with ctrl-e and ctrl-y

A property can be taller than the panel, so the cursor and the viewport can
disagree and the user needs to move one alone. Bound in Table DDL only: the
other panels scroll in rows, where the two cannot diverge
(doc/specs/2026-08-10-ddl-selection-design.md §4.1).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Task 7: Reconcile the documents with what is bound

`doc/shortcuts.md` is explicitly "what is bound today", so leaving it stale is a defect, not a chore.

**Files:**
- Modify: `doc/shortcuts.md`
- Modify: `doc/architecture.md` (§11)
- Modify: `doc/specs/2026-08-10-ddl-selection-design.md` (status line)
- Modify: `TODO.md` if it carries an entry this closes

**Interfaces:** none.

- [ ] **Step 1: Update `doc/shortcuts.md`**

Four edits:

1. The Motion preamble ("Tables and Contents move a selection and let the viewport follow; Table DDL moves a viewport over lines") — Table DDL now moves a selection too. Say instead that Tables, Contents and Table DDL move a selection and let the viewport follow, and that Queries moves a text cursor.
2. The `j` / `↓` row loses "(one line in Table DDL…)".
3. The paragraph beginning "`End` in Table DDL stops at the last line rather than scrolling past it" — it now lands on the last row, like a list. Rewrite it, and add the `PageDown` imprecision from spec §4 as the one place the panel is not exact.
4. Add a **Table DDL panel** subsection after **Tables panel**:

```markdown
### Table DDL panel

Accepted anywhere, but a no-op unless Table DDL has focus.

| Key | Action |
|---|---|
| `Ctrl-e` | Scroll down one line, leaving the selection where it is. |
| `Ctrl-y` | Scroll up one line. |

The only bindings behind `ctrl`, and the only panel that needs them: a table
option can be taller than the panel, so this is the one place a selection and
a viewport can disagree. A selection scrolled out of view is not drawn, and the
next `j` or `k` brings the viewport back to it.
See [Table DDL selection design](specs/2026-08-10-ddl-selection-design.md).
```

Also check §2's rules text for the claim that ctrl is refused wholesale, and the mouse section's row about the wheel and the DDL panel — the wheel now also leaves the cursor alone, which is worth one clause.

- [ ] **Step 2: Update `doc/architecture.md` §11**

The "Does `State` stay one struct?" bullet says `TablesPanel` took the escape hatch first and `ContentsPanel` followed. `DdlPanel` is the third. Update the sentence and keep the question open — this plan does not answer it.

- [ ] **Step 3: Mark the spec implemented**

Change the spec's `**Status:** design, not yet implemented.` to `**Status:** implemented.`

- [ ] **Step 4: Check `TODO.md`**

`rg -n 'DDL|ddl' TODO.md`. Line 113 mentions `update` returning right after setting `state.focus` for `Panel::Ddl | Panel::Queries`. If that entry is now stale for Ddl, narrow it to Queries; if it describes something this plan did not touch, leave it.

- [ ] **Step 5: Run the gate and commit**

```bash
mise run ci
git add doc TODO.md
git commit -m "docs(ddl): reconcile the shortcut and architecture docs with the cursor

Table DDL is no longer the panel that moves a viewport instead of a selection,
and ctrl is no longer refused wholesale
(doc/specs/2026-08-10-ddl-selection-design.md §4).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Self-review notes

Checked against the spec, section by section:

- §2 option ordering → Task 1.
- §3 what the cursor moves over, wrapped row as one stop → Tasks 3 (`Details::rows`) and 4 (the highlight).
- §4 motion, `PageDown` imprecision → Task 3 (routing) and Task 7 (documenting it).
- §4.1 `Ctrl-e`/`Ctrl-y`, `Binding` widening, hint text → Tasks 5 and 6.
- §5.1 `DdlPanel`, `LISTS` unchanged, cursor clamped in the epilogue → Task 3.
- §5.2 `selected` on the projection → Task 4.
- §5.3 clamping off the projection, reset on subject change → Task 3.
- §5.4 span, `follow_span`, follow in `measured` → Tasks 2, 3, 4.
- §6 tests → distributed across the tasks that introduce the behaviour.
- §7 out of scope → nothing implements `Enter`, clipboard, or click-to-select; Task 4 step 5 states why `Unit::Lines` stays.

One place where the plan asks the implementer to read before writing rather than handing them the answer: the existing render-test helper names and the state-builder those DDL tests use (Task 4, step 1). That is a lookup in a named file, not a decision.

Verified while writing, so the implementer does not have to re-derive them:

- `Theme::TerminalDefault` maps `Token::Selected` to `Modifier::REVERSED` (`crates/lc-inbound/src/theme.rs:75`).
- `ratatui` is `0.30.2`, where `Line.spans` is a public field and `Style::patch` takes and returns a `Style`.
- The `orders()` fixture in `view/details.rs` has one partition column, two clustering columns and three options — hence `row_count() == 6` in Task 3.
- `read_shop_orders()` (`update.rs:1931`) leaves the Tables cursor on a table, so the DDL panel has something to describe.
- `handle_key` calls `state.mark_dirty()` before the modifier guard, so `ctrl_key` needs no dirty marking of its own.
