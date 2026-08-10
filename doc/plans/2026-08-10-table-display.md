# Table display implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Contents grid navigable — focus follows a read, a cell cursor moves in both axes, rows are numbered, and columns past the right edge are reachable and announced by a scrollbar.

**Architecture:** The design is [doc/specs/2026-08-10-table-display-design.md](../specs/2026-08-10-table-display-design.md); read it before Task 1. All column arithmetic (natural widths, what fits from an offset, the gutter width, the horizontal viewport) lands in a new pure module `lc-domain/src/grid.rs`, because "how many columns fit from offset N" is offset-dependent and architecture.md §3 forbids that crossing back into `update` through `Msg::Measured`. The renderer reports only the grid body's character width — a function of terminal size alone — and then draws exactly what the view model names.

**Tech stack:** Rust 2024 workspace, four crates (`lc-domain` pure, `lc-inbound` ratatui 0.30.2 + crossterm, `lc-outbound`, `lc-app`). Tests are plain `#[cfg(test)] mod tests` in the file under test.

## Global constraints

Copied from AGENTS.md and doc/conventions.md. Every task's requirements implicitly include these.

- `mise run ci` must be clean before a task is done: `cargo fmt --all`, clippy with warnings denied, licences, advisories, and the fast test tiers. Zero warnings, zero ignored tests.
- No `unsafe`, no FFI — `unsafe_code = "forbid"` is set workspace-wide.
- `lc-domain` must never depend on `tokio`, `ratatui`, or `crossterm`. Task 1 adds `unicode-width` to it, which is none of those; no other dependency is added to that crate by this plan.
- Any `#[allow(...)]` must be item-scoped and carry a reason. This plan needs none.
- No narrating comments. Comments are for edge cases, past bugs, upstream defects, and Cassandra/protocol semantics. Prefer a rename or an extracted function over a comment.
- Determinism in the fast tiers: no wall clock, no network, no unseeded randomness, no `HashMap` iteration order.
- Conventional Commits, imperative, lowercase, no trailing period. Commit freely; **never `pull`, never `push`**.
- Reference the design section a change follows from in the commit body, as the existing history does.
- Every new key binding needs a row in `doc/shortcuts.md` §5 naming its test. That document treats this as mandatory, not optional.

## File structure

| File | Responsibility after this plan |
|---|---|
| `crates/lc-domain/src/grid.rs` | **New.** Pure column arithmetic: `widths`, `fitting`, `gutter`, `ColumnScroll`. No knowledge of panels or focus. |
| `crates/lc-domain/src/contents.rs` | The Contents panel's state — now a cell cursor (row + column) and a column viewport. |
| `crates/lc-domain/src/state.rs` | `Geometry` gains the grid's character width; `CONTENTS_HINTS` names the new motion; `Token` gains the cell highlight. |
| `crates/lc-domain/src/update.rs` | Dispatch: horizontal motion by focus, focus-on-read, and the column viewport's follow/clamp epilogue. |
| `crates/lc-domain/src/view/contents.rs` | The projection: which columns are drawn, at what widths, with which cell selected and how wide the gutter is. |
| `crates/lc-inbound/src/body.rs` | Draws what the view model names. Loses `widths()` and `fitting()`; gains the gutter, the cell highlight, and a second `Extent`. |
| `crates/lc-inbound/src/render.rs` | Reports the grid width; draws the horizontal scrollbar over the bottom border. |
| `crates/lc-inbound/src/theme.rs` | The cell-highlight token's concrete style. |
| `doc/shortcuts.md` | §1, §2, §4, §5 updated for the new motion and the focus jump. |

Task order is dependency order: 1 (arithmetic) → 2 (state) → 3 (dispatch) → 4 (projection) → 5 (rendering) → 6 (focus jump) → 7 (docs). Tasks 1–5 leave the tree compiling and green at every commit; the grid is not visibly different until Task 5.

---

### Task 1: The column arithmetic

**Files:**
- Create: `crates/lc-domain/src/grid.rs`
- Modify: `crates/lc-domain/src/lib.rs` (add `pub mod grid;`)
- Modify: `crates/lc-domain/Cargo.toml` (add `unicode-width.workspace = true`)
- Test: in `crates/lc-domain/src/grid.rs` itself

**Interfaces:**
- Consumes: `crate::cell::text`, `crate::model::Page`.
- Produces, and every later task depends on these exact signatures:
  - `pub const MAX_COLUMN_WIDTH: u16 = 24;`
  - `pub const COLUMN_SPACING: u16 = 1;`
  - `pub fn widths(page: &Page) -> Vec<u16>`
  - `pub fn fitting(widths: &[u16], first: usize, available: u16) -> usize`
  - `pub fn gutter(rows: usize) -> u16`
  - `pub struct ColumnScroll` with `pub const fn first(self) -> usize` and `pub fn follow(&mut self, column: usize, widths: &[u16], available: u16)`

- [ ] **Step 1: Add the dependency**

In `crates/lc-domain/Cargo.toml`, under `[dependencies]`, keeping the list alphabetical:

```toml
async-trait.workspace = true
serde.workspace = true
thiserror.workspace = true
# A table of character display widths — no terminal, no runtime, no driver, so
# architecture.md §2's rule holds. The grid's column widths are computed here
# rather than in the renderer because the offsets they bound live here
# (doc/specs/2026-08-10-table-display-design.md §6).
unicode-width.workspace = true
```

`unicode-width = "0.2.2"` is already in the workspace root `Cargo.toml`, so no version goes here.

- [ ] **Step 2: Write the failing tests**

Create `crates/lc-domain/src/grid.rs` containing the module doc, the constants, `todo!()` bodies for the four items, and this test module. It will not compile yet — that is the failure this step is looking for.

```rust
//! The grid's column arithmetic.
//!
//! Separate from [`crate::contents`] because it is arithmetic and not state:
//! how wide a column is, how many fit from a given offset, and where the
//! horizontal viewport has to sit to show a cell. `update` needs all four
//! answers, and they cannot be measured in the renderer — "how many fit from
//! offset N" varies with N, and architecture.md §3 forbids an offset-dependent
//! measurement crossing back
//! (doc/specs/2026-08-10-table-display-design.md §6).

use unicode_width::UnicodeWidthStr as _;

use crate::cell;
use crate::model::Page;

/// How wide a column may grow before its values are ellipsised.
///
/// A uuid is 36 columns and a blob is unbounded. Horizontal scrolling reaches
/// columns, not the tails of values (design §7).
pub const MAX_COLUMN_WIDTH: u16 = 24;

/// One blank column between cells, and between the gutter and the first
/// column.
pub const COLUMN_SPACING: u16 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CqlValue, Row};

    fn page(columns: &[&str], rows: &[&[CqlValue]]) -> Page {
        Page {
            columns: columns.iter().map(|name| (*name).to_owned()).collect(),
            rows: rows
                .iter()
                .map(|values| Row {
                    values: values.to_vec(),
                })
                .collect(),
            next: None,
        }
    }

    #[test]
    fn a_column_is_as_wide_as_the_widest_thing_in_it() {
        let page = page(
            &["id", "name"],
            &[
                &[CqlValue::Int(1), CqlValue::Text("alice".to_owned())],
                &[CqlValue::Int(2), CqlValue::Text("bartholomew".to_owned())],
            ],
        );

        assert_eq!(widths(&page), vec![2, 11], "the heading counts too");
    }

    #[test]
    fn a_heading_wider_than_every_value_sets_the_width() {
        let page = page(&["description"], &[&[CqlValue::Text("hi".to_owned())]]);

        assert_eq!(widths(&page), vec![11]);
    }

    #[test]
    fn a_column_is_capped() {
        let page = page(
            &["blob"],
            &[&[CqlValue::Text("x".repeat(100))]],
        );

        assert_eq!(widths(&page), vec![MAX_COLUMN_WIDTH]);
    }

    #[test]
    fn a_column_with_no_rows_is_still_as_wide_as_its_heading() {
        let page = page(&["id"], &[]);

        assert_eq!(widths(&page), vec![2]);
    }

    #[test]
    fn only_the_columns_that_fit_are_counted() {
        // 5 + 1 + 5 + 1 + 5 = 17 for three, 11 for two.
        assert_eq!(fitting(&[5, 5, 5], 0, 17), 3);
        assert_eq!(fitting(&[5, 5, 5], 0, 16), 2);
        assert_eq!(fitting(&[5, 5, 5], 0, 11), 2);
        assert_eq!(fitting(&[5, 5, 5], 0, 10), 1);
    }

    #[test]
    fn fitting_counts_from_the_offset() {
        assert_eq!(fitting(&[20, 5, 5], 1, 11), 2, "the wide first column is not drawn");
    }

    #[test]
    fn a_panel_too_narrow_for_one_column_still_draws_it() {
        assert_eq!(
            fitting(&[20], 0, 3),
            1,
            "an ellipsised first column is more use than an empty panel"
        );
    }

    #[test]
    fn fitting_past_the_last_column_is_nothing() {
        assert_eq!(fitting(&[5, 5], 2, 40), 0);
    }

    #[test]
    fn the_gutter_is_as_wide_as_the_last_row_number() {
        assert_eq!(gutter(0), 0, "no rows, no gutter");
        assert_eq!(gutter(9), 1);
        assert_eq!(gutter(10), 2);
        assert_eq!(gutter(1000), 4);
    }

    #[test]
    fn a_visible_column_moves_the_viewport_nowhere() {
        let mut scroll = ColumnScroll::default();

        scroll.follow(1, &[5, 5, 5], 17);

        assert_eq!(scroll.first(), 0);
    }

    #[test]
    fn the_viewport_moves_the_shortest_distance_that_shows_a_column_to_the_right() {
        let mut scroll = ColumnScroll::default();

        scroll.follow(2, &[5, 5, 5], 11);

        assert_eq!(
            scroll.first(),
            1,
            "columns 1 and 2 fit in 11; the offset must not overshoot to 2"
        );
    }

    #[test]
    fn the_viewport_follows_a_column_to_the_left() {
        let mut scroll = ColumnScroll::default();
        scroll.follow(2, &[5, 5, 5], 11);

        scroll.follow(0, &[5, 5, 5], 11);

        assert_eq!(scroll.first(), 0);
    }

    #[test]
    fn a_column_too_wide_for_the_panel_is_shown_from_its_own_start() {
        let mut scroll = ColumnScroll::default();

        scroll.follow(1, &[5, 40], 10);

        assert_eq!(scroll.first(), 1);
    }

    #[test]
    fn following_a_column_that_is_gone_clamps_the_viewport() {
        let mut scroll = ColumnScroll::default();
        scroll.follow(5, &[5, 5, 5, 5, 5, 5], 11);

        scroll.follow(0, &[5], 11);

        assert_eq!(scroll.first(), 0, "a narrower page must not strand the offset");
    }
}
```

- [ ] **Step 3: Run the tests and watch them fail**

Run: `cargo test -p lc-domain grid::`
Expected: a compile error — `widths`, `fitting`, `gutter` and `ColumnScroll` do not exist yet (or `todo!()` panics if you stubbed them).

- [ ] **Step 4: Write the implementation**

Add above the test module in `crates/lc-domain/src/grid.rs`:

```rust
/// Each column's natural width: the widest of its heading and its values,
/// capped.
#[must_use]
pub fn widths(page: &Page) -> Vec<u16> {
    page.columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let widest = page
                .rows
                .iter()
                .filter_map(|row| row.values.get(index))
                .map(|value| cell::text(value).width())
                .chain(std::iter::once(name.width()))
                .max()
                .unwrap_or(1);

            u16::try_from(widest)
                .unwrap_or(MAX_COLUMN_WIDTH)
                .clamp(1, MAX_COLUMN_WIDTH)
        })
        .collect()
}

/// How many columns fit in `available`, drawing from `first`.
///
/// At least one whenever `first` names a column: a panel too narrow for even
/// that one still shows what it can of it, which is more use than an empty
/// panel.
#[must_use]
pub fn fitting(widths: &[u16], first: usize, available: u16) -> usize {
    let mut used = 0;
    let mut count = 0;

    for width in widths.iter().skip(first) {
        let spacing = if count == 0 { 0 } else { COLUMN_SPACING };
        let next = used.saturating_add(spacing).saturating_add(*width);

        if next > available && count > 0 {
            break;
        }

        used = next;
        count += 1;
    }

    count
}

/// The row-number gutter's width: the digits of the last row's number.
#[must_use]
pub fn gutter(rows: usize) -> u16 {
    if rows == 0 {
        return 0;
    }

    let digits = rows.to_string().len();

    u16::try_from(digits).unwrap_or(u16::MAX)
}

/// Which column the grid is drawn from.
///
/// Not [`crate::state::Scroll`]: every method there takes a `height`, because
/// a row viewport shows a fixed number of rows whatever the offset. How many
/// columns fit depends on which one you start from, so the window is a
/// function of the offset rather than a constant beside it (design §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColumnScroll {
    first: usize,
}

impl ColumnScroll {
    /// The leftmost column drawn.
    #[must_use]
    pub const fn first(self) -> usize {
        self.first
    }

    /// Moves the shortest distance that puts `column` on screen.
    pub fn follow(&mut self, column: usize, widths: &[u16], available: u16) {
        self.first = self.first.min(column);

        while self.first < column && column >= self.first + fitting(widths, self.first, available) {
            self.first += 1;
        }
    }
}
```

`follow` terminates: `fitting` returns at least one whenever `first` names a column, so the guard is false once `first == column`.

- [ ] **Step 5: Register the module**

In `crates/lc-domain/src/lib.rs`, add `pub mod grid;` in the existing alphabetical run of `pub mod` declarations.

- [ ] **Step 6: Run the tests and the full gate**

Run: `cargo test -p lc-domain grid::`
Expected: PASS, 13 tests.

Run: `mise run ci`
Expected: clean. If clippy objects to `fitting`'s loop shape, satisfy it rather than allowing it.

- [ ] **Step 7: Commit**

```bash
git add crates/lc-domain/src/grid.rs crates/lc-domain/src/lib.rs crates/lc-domain/Cargo.toml
git commit -m "feat(domain): give the grid its column arithmetic

Widths, what fits from an offset, the gutter, and the horizontal
viewport, in one pure module. Lands in lc-domain rather than the
renderer because 'how many columns fit from offset N' varies with N,
and architecture.md §3 forbids an offset-dependent measurement
crossing back through Msg::Measured
(doc/specs/2026-08-10-table-display-design.md §6).

Costs lc-domain a unicode-width dependency: a table of character
widths, and none of the three things architecture.md §2 bars."
```

---

### Task 2: The cell cursor in the panel's state

**Files:**
- Modify: `crates/lc-domain/src/contents.rs`
- Test: in `crates/lc-domain/src/contents.rs`

**Interfaces:**
- Consumes: `grid::ColumnScroll` from Task 1.
- Produces: `ContentsPanel::column: Cursor` (public field, like `cursor`), `ContentsPanel::columns: ColumnScroll` (public field, like `scroll`), and `pub fn column_count(&self) -> usize`.

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `crates/lc-domain/src/contents.rs`. Note the existing helper `page(rows: i64)` builds a one-column page; these tests need a wider one, so add `wide_page` beside it.

```rust
    fn wide_page(columns: usize) -> Page {
        Page {
            columns: (0..columns).map(|i| format!("c{i}")).collect(),
            rows: vec![Row {
                values: (0..columns).map(|i| CqlValue::Int(i as i32)).collect(),
            }],
            next: None,
        }
    }

    #[test]
    fn a_fresh_panel_has_no_columns_to_move_over() {
        let panel = ContentsPanel::default();

        assert_eq!(panel.column_count(), 0);
        assert_eq!(panel.column.index(), 0);
        assert_eq!(panel.columns.first(), 0);
    }

    #[test]
    fn a_loaded_page_reports_its_columns() {
        let (mut panel, query) = loading();

        panel.page_ready(query, wide_page(4));

        assert_eq!(panel.column_count(), 4);
    }

    #[test]
    fn a_request_returns_the_cell_cursor_to_the_first_cell() {
        let (mut panel, query) = loading();
        panel.page_ready(query, wide_page(4));
        panel.cursor.go_to(0, 1);
        panel.column.go_to(3, 4);
        panel.columns.follow(3, &[1, 1, 1, 1], 4);

        panel.requested(QueryId::next(), TableRef::new("shop", "carts"));

        assert_eq!(panel.column.index(), 0);
        assert_eq!(panel.columns.first(), 0);
    }

    #[test]
    fn a_refusal_returns_the_cell_cursor_to_the_first_cell() {
        let (mut panel, query) = loading();
        panel.page_ready(query, wide_page(4));
        panel.column.go_to(3, 4);

        panel.refused(
            TableRef::new("shop", "carts"),
            DbError::Unreachable("no".to_owned()),
        );

        assert_eq!(panel.column.index(), 0);
        assert_eq!(panel.columns.first(), 0);
    }
```

`DbError` and `Row` may need adding to the test module's `use` list; `CqlValue` and `Row` are already imported there.

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p lc-domain contents::`
Expected: compile error — no field `column`, no field `columns`, no method `column_count`.

- [ ] **Step 3: Write the implementation**

In `crates/lc-domain/src/contents.rs`, add the import and the two fields:

```rust
use crate::grid::ColumnScroll;
```

```rust
pub struct ContentsPanel {
    pub cursor: Cursor,
    /// The selected column. With [`ContentsPanel::cursor`] it names one cell,
    /// which is what a future delete or edit acts on
    /// (doc/specs/2026-08-10-table-display-design.md §2).
    pub column: Cursor,
    /// The first row drawn (doc/specs/2026-08-09-list-viewport-design.md §1).
    pub scroll: Scroll,
    /// The first column drawn (doc/specs/2026-08-10-table-display-design.md §5).
    pub columns: ColumnScroll,
    /// Private for the reason [`crate::tree::TablesPanel`]'s unfold set is:
    /// the only ways in are the transitions below.
    state: ContentsState,
}
```

In `requested` and in `refused`, beside the two existing resets:

```rust
        self.column = Cursor::default();
        self.columns = ColumnScroll::default();
```

And beside `row_count`:

```rust
    /// The columns on screen, for a cursor that must not run past them.
    #[must_use]
    pub fn column_count(&self) -> usize {
        match &self.state {
            ContentsState::Loaded { page, .. } => page.columns.len(),
            _ => 0,
        }
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p lc-domain contents::`
Expected: PASS.

Run: `cargo test -p lc-domain`
Expected: PASS — `ContentsPanel` derives `Default` and `PartialEq`, and both new types do too, so nothing else needs touching.

- [ ] **Step 5: Commit**

```bash
git add crates/lc-domain/src/contents.rs
git commit -m "feat(domain): give the contents panel a cell cursor

A column cursor and a column viewport beside the row pair, reset by
the same two transitions. Nothing moves them yet
(doc/specs/2026-08-10-table-display-design.md §2)."
```

---

### Task 3: Horizontal motion, and the viewport that follows it

**Files:**
- Modify: `crates/lc-domain/src/state.rs` (`Geometry`)
- Modify: `crates/lc-domain/src/update.rs`
- Modify: `crates/lc-inbound/src/render.rs` (the one `Geometry::new` call site)
- Test: in `crates/lc-domain/src/update.rs`

**Interfaces:**
- Consumes: Task 1's `grid::{fitting, gutter, widths, ColumnScroll}`, Task 2's `column`/`columns`/`column_count`.
- Produces: `Geometry::new(heights: [usize; 4], ddl_lines: usize, command_rows: usize, grid_width: usize)` — a fourth parameter, in that position — and `Geometry::grid_width(self) -> usize`.

- [ ] **Step 1: Widen `Geometry`**

In `crates/lc-domain/src/state.rs`:

```rust
pub struct Geometry {
    heights: [usize; 4],
    ddl_lines: usize,
    command_rows: usize,
    /// Character columns the Contents grid's body has, gutter included. A
    /// function of the terminal size alone, which is what keeps §3's
    /// measurement loop open (doc/specs/2026-08-10-table-display-design.md §6).
    grid_width: usize,
}
```

Add `grid_width` as the fourth parameter of `Geometry::new`, in the same order, and add the accessor beside `ddl_lines()`:

```rust
    /// Character columns the Contents grid can draw into.
    #[must_use]
    pub const fn grid_width(self) -> usize {
        self.grid_width
    }
```

Then fix every call site. `cargo check --workspace --all-targets` lists them: one in `crates/lc-inbound/src/render.rs` (pass `grid_width`, wired properly in Task 5 — for now pass `0`) and several in `update.rs`'s tests (pass `0` unless the test is about width). Do not guess at the list; let the compiler produce it.

- [ ] **Step 2: Write the failing tests**

Append to `mod tests` in `crates/lc-domain/src/update.rs`. Reuse whatever helper the file already has for building a state showing a page; if none is reachable from this module, add this one beside the existing helpers:

```rust
    /// A state focused on Contents, showing a page of `columns` columns each
    /// three characters wide, in a grid `width` characters across.
    fn grid_of(columns: usize, width: usize) -> State {
        let mut state = tabled(&[("shop", &["orders"])]);
        let query = QueryId::next();
        state
            .contents
            .requested(query, TableRef::new("shop", "orders"));
        state.contents.page_ready(
            query,
            Page {
                columns: (0..columns).map(|i| format!("c{i}")).collect(),
                rows: vec![Row {
                    values: (0..columns).map(|_| CqlValue::Int(1)).collect(),
                }],
                next: None,
            },
        );
        state.focus = Panel::Contents;
        update(&mut state, Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0, width)));
        state
    }

    #[test]
    fn l_and_right_move_the_grid_cursor_one_column() {
        for key in [Key::Char('l'), Key::Right] {
            let mut state = grid_of(4, 80);

            press(&mut state, key);

            assert_eq!(state.contents.column.index(), 1, "{key:?}");
        }
    }

    #[test]
    fn h_and_left_move_the_grid_cursor_back_one_column() {
        for key in [Key::Char('h'), Key::Left] {
            let mut state = grid_of(4, 80);
            press(&mut state, Key::Char('l'));

            press(&mut state, key);

            assert_eq!(state.contents.column.index(), 0, "{key:?}");
        }
    }

    #[test]
    fn the_grid_cursor_cannot_run_off_either_edge_of_a_page() {
        let mut state = grid_of(2, 80);

        for _ in 0..5 {
            press(&mut state, Key::Char('l'));
        }
        assert_eq!(state.contents.column.index(), 1, "stops on the last column");

        for _ in 0..5 {
            press(&mut state, Key::Char('h'));
        }
        assert_eq!(state.contents.column.index(), 0, "stops on the first");
    }

    #[test]
    fn a_grid_with_no_page_swallows_horizontal_motion() {
        let mut state = State::default();
        state.focus = Panel::Contents;

        press(&mut state, Key::Char('l'));

        assert_eq!(state.contents.column.index(), 0);
        assert_eq!(state.contents.columns.first(), 0);
    }

    #[test]
    fn horizontal_motion_belongs_to_the_focused_panel_only() {
        let mut state = grid_of(4, 80);
        state.focus = Panel::Ddl;

        press(&mut state, Key::Char('l'));

        assert_eq!(state.contents.column.index(), 0);
    }

    #[test]
    fn the_column_viewport_follows_the_cell_cursor() {
        // Four columns of width 2 ("c0" and a one-digit value) with one space
        // between: 2+1+2 = 5 fits two columns in a grid 7 wide once the
        // one-digit gutter and its spacing are taken off.
        let mut state = grid_of(4, 7);

        press(&mut state, Key::Char('l'));
        assert_eq!(state.contents.columns.first(), 0, "column 1 is already drawn");

        press(&mut state, Key::Char('l'));
        assert_eq!(
            state.contents.columns.first(),
            1,
            "the shortest move that draws column 2"
        );
    }

    #[test]
    fn a_narrower_frame_pulls_the_column_viewport_back_to_the_cursor() {
        let mut state = grid_of(4, 80);
        for _ in 0..3 {
            press(&mut state, Key::Char('l'));
        }
        assert_eq!(state.contents.columns.first(), 0, "all four fit at 80");

        update(
            &mut state,
            Msg::Measured(Geometry::new([8, 8, 8, 1], 0, 0, 7)),
        );

        assert_eq!(
            state.contents.columns.first(),
            2,
            "the selected column must stay drawn after a resize"
        );
    }

    #[test]
    fn home_and_end_still_reach_the_ends_of_the_row_list() {
        let mut state = grid_of(4, 80);
        press(&mut state, Key::Char('l'));

        press(&mut state, Key::End);

        assert_eq!(
            state.contents.column.index(),
            1,
            "End reaches the ends of the document, which in a grid is the rows"
        );
        assert_eq!(state.contents.cursor.index(), 0, "one row, so End stays on it");
    }

    #[test]
    fn h_and_l_still_fold_a_keyspace_in_tables() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;

        press(&mut state, Key::Char('l'));
        let unfolded = tree_rows(&state);

        press(&mut state, Key::Char('h'));

        assert!(unfolded > tree_rows(&state), "h must still fold");
    }
```

Match the existing tests' helper names — this file already has `press`, `tabled` and `tree_rows`; use its spellings rather than these if they differ, and reuse its `Page` construction helper if one exists.

- [ ] **Step 3: Run the tests and watch them fail**

Run: `cargo test -p lc-domain update::`
Expected: the four `l`/`h` tests fail — the column cursor stays at 0, because `transition` returns early for any panel but Tables.

- [ ] **Step 4: Route horizontal motion by focus**

In `crates/lc-domain/src/update.rs`, replace the two `transition` arms:

```rust
        Key::Char('l') | Key::Right => go_right(state),
        Key::Char('h') | Key::Left => go_left(state),
```

and add, beside `transition`:

```rust
/// `l` and `→`: into the tree, or on to the next column.
///
/// The four horizontal keys mean *move left* and *move right*, and each panel
/// resolves that in its own vocabulary — the shape `j`/`k` already have
/// (doc/shortcuts.md §2, doc/specs/2026-08-10-table-display-design.md §2).
fn go_right(state: &mut State) {
    match state.focus {
        Panel::Tables => transition(state, Transition::Open),
        Panel::Contents => {
            let len = state.contents.column_count();
            state.contents.column.down(len);
        }
        Panel::Ddl | Panel::Queries => {}
    }
}

fn go_left(state: &mut State) {
    match state.focus {
        Panel::Tables => transition(state, Transition::Close),
        Panel::Contents => state.contents.column.up(),
        Panel::Ddl | Panel::Queries => {}
    }
}
```

`transition`'s own focus guard stays: it is still reachable from the pointer path, and a guard that is now redundant on one route is cheaper than a guard that is missing on another.

- [ ] **Step 5: Make the viewport follow, and clamp**

Still in `update.rs`, add:

```rust
/// The columns the grid is drawing over, and the width they have.
///
/// `None` when there is no page: nothing to fit, and nothing to follow.
fn grid_columns(state: &State) -> Option<(Vec<u16>, u16)> {
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };

    let gutter = grid::gutter(page.rows.len());
    let width = u16::try_from(state.geometry.grid_width()).unwrap_or(u16::MAX);
    let available = width
        .saturating_sub(gutter)
        .saturating_sub(if gutter == 0 { 0 } else { grid::COLUMN_SPACING });

    Some((grid::widths(page), available))
}

/// Pulls the column viewport over the selected cell, and back inside a page
/// that narrowed.
///
/// Unconditional, for [`clamp_lists`]'s reason: a new page changes no
/// geometry, so no [`Msg::Measured`] arrives to clamp against.
fn follow_grid_columns(state: &mut State) {
    let column = state.contents.column.index();

    let Some((widths, available)) = grid_columns(state) else {
        state.contents.columns = ColumnScroll::default();
        return;
    };

    state.contents.columns.follow(column, &widths, available);
}
```

Then in `update`, beside the existing epilogue. Note the column cursor is clamped before the viewport follows it, so a page that lost columns cannot strand either:

```rust
pub fn update(state: &mut State, msg: Msg) -> Vec<Effect> {
    let subject = described(state);
    let cursors = cursor_indices(state);
    let scrolls = (state.tables.scroll, state.contents.scroll);
    let columns = state.contents.columns;

    let effects = apply(state, msg);

    if described(state) != subject {
        state.ddl.to_start();
    }

    clamp_lists(state);
    state
        .contents
        .column
        .clamp_to(state.contents.column_count());
    follow_moved_cursors(state, cursors);
    follow_grid_columns(state);

    if (state.tables.scroll, state.contents.scroll) != scrolls
        || state.contents.columns != columns
    {
        state.mark_dirty();
    }

    effects
}
```

Add `use crate::grid::{self, ColumnScroll};` and `use crate::contents::ContentsState;` to the file's imports if they are not already there.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p lc-domain`
Expected: PASS, including the pre-existing `l_and_right_open_a_keyspace`, `h_and_left_close_a_keyspace` and `h_from_a_table_steps_out_to_its_keyspace_before_folding`, which the focus-routed arms must not have broken.

Run: `mise run ci`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/lc-domain/src/state.rs crates/lc-domain/src/update.rs crates/lc-inbound/src/render.rs
git commit -m "feat(domain): move the cell cursor across the grid's columns

h/l and ←/→ dispatch on focus: fold and unfold in a tree, previous
and next column in a grid, inert elsewhere. The rule they read as is
'move left / move right' in every panel, which is the shape j/k
already have (doc/shortcuts.md §2).

Geometry gains the grid's character width — a function of the
terminal size alone, so the measurement loop stays open
(doc/specs/2026-08-10-table-display-design.md §6)."
```

---

### Task 4: What the renderer is told

**Files:**
- Modify: `crates/lc-domain/src/view/contents.rs`
- Modify: `crates/lc-domain/src/view/mod.rs` (`PanelBody::Grid`)
- Modify: `crates/lc-inbound/src/body.rs` (make it compile against the new shape; it still draws as before)
- Test: in `crates/lc-domain/src/view/contents.rs`

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces: `GridColumn { name: String, token: Option<Token>, width: u16 }` and

```rust
    Grid {
        columns: Vec<GridColumn>,
        rows: Vec<Vec<GridCell>>,
        selected: usize,
        offset: usize,
        /// The leftmost column drawn.
        first_column: usize,
        /// The selected column, an index into `columns`.
        column: usize,
        /// Character columns the row-number gutter takes, or zero for a grid
        /// with no rows.
        gutter: u16,
    },
```

`columns` and `rows` still carry **every** column — the renderer slices with `first_column` and the widths, so the view model stays a description of the page rather than of one frame's window.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/lc-domain/src/view/contents.rs`:

```rust
    #[test]
    fn a_grid_carries_the_width_of_every_column() {
        let state = showing(page_of(
            &["id", "name"],
            &[&[CqlValue::Int(1), CqlValue::Text("alice".to_owned())]],
        ));

        let PanelBody::Grid { columns, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(
            columns.iter().map(|c| c.width).collect::<Vec<_>>(),
            vec![2, 5]
        );
    }

    #[test]
    fn a_grid_names_the_selected_cell_and_the_first_column_drawn() {
        let mut state = showing(page_of(
            &["id", "name"],
            &[&[CqlValue::Int(1), CqlValue::Text("alice".to_owned())]],
        ));
        state.contents.column.go_to(1, 2);

        let PanelBody::Grid {
            column,
            first_column,
            selected,
            ..
        } = contents_body(&state)
        else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(column, 1);
        assert_eq!(first_column, 0);
        assert_eq!(selected, 0);
    }

    #[test]
    fn the_gutter_is_as_wide_as_the_last_row_number() {
        let rows: Vec<Vec<CqlValue>> = (0..12).map(|i| vec![CqlValue::Int(i)]).collect();
        let borrowed: Vec<&[CqlValue]> = rows.iter().map(Vec::as_slice).collect();
        let state = showing(page_of(&["id"], &borrowed));

        let PanelBody::Grid { gutter, .. } = contents_body(&state) else {
            panic!("a loaded page is a grid");
        };

        assert_eq!(gutter, 2, "twelve rows need two digits");
    }
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p lc-domain view::contents::`
Expected: compile error — no field `width` on `GridColumn`, no `column`/`first_column`/`gutter` on `PanelBody::Grid`.

- [ ] **Step 3: Widen the view model**

In `crates/lc-domain/src/view/contents.rs`:

```rust
pub struct GridColumn {
    pub name: String,
    /// The column's place in the physical layout —
    /// [`Token::PartitionKey`], [`Token::ClusteringKey`] or
    /// [`Token::RegularColumn`]. `None` when the schema no longer has the
    /// table, so no claim about its shape can be checked.
    pub token: Option<Token>,
    /// Character columns it is drawn in, capped by
    /// [`crate::grid::MAX_COLUMN_WIDTH`]. Computed here rather than in the
    /// renderer so the fitting arithmetic can live beside the offsets it
    /// bounds (doc/specs/2026-08-10-table-display-design.md §6).
    pub width: u16,
}
```

and in `grid`:

```rust
fn grid(state: &State, table: &TableRef, page: &Page) -> PanelBody {
    if page.rows.is_empty() {
        return waiting("No rows");
    }

    let widths = crate::grid::widths(page);

    PanelBody::Grid {
        columns: page
            .columns
            .iter()
            .zip(&widths)
            .map(|(name, width)| GridColumn {
                name: name.clone(),
                token: column_token(state.schema(), table, name),
                width: *width,
            })
            .collect(),
        rows: page
            .rows
            .iter()
            .map(|row| row.values.iter().map(grid_cell).collect())
            .collect(),
        selected: state.contents.cursor.index(),
        offset: state.contents.scroll.offset(),
        first_column: state.contents.columns.first(),
        column: state.contents.column.index(),
        gutter: crate::grid::gutter(page.rows.len()),
    }
}
```

Add the three fields to `PanelBody::Grid` in `crates/lc-domain/src/view/mod.rs` with the doc comments from the Interfaces block above.

- [ ] **Step 4: Keep `lc-inbound` compiling**

In `crates/lc-inbound/src/body.rs`, the `PanelBody::Grid` arm destructures the variant. Add the three new bindings and ignore them for now:

```rust
        PanelBody::Grid {
            columns,
            rows,
            selected,
            offset,
            first_column: _,
            column: _,
            gutter: _,
        } => Some(Body {
```

Task 5 uses them. Leave `widths()` and `fitting()` in `body.rs` alone until then.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p lc-domain view::contents::`
Expected: PASS.

Run: `mise run ci`
Expected: clean. The grid still draws exactly as it did.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-domain/src/view/contents.rs crates/lc-domain/src/view/mod.rs crates/lc-inbound/src/body.rs
git commit -m "feat(domain): tell the renderer which cell and which columns

Column widths, the first column drawn, the selected column and the
gutter's width all land in the view model, so the renderer draws what
it is given rather than deciding it
(doc/specs/2026-08-10-table-display-design.md §6, §8)."
```

---

### Task 5: Drawing it — gutter, cell highlight, horizontal scrollbar

**Files:**
- Modify: `crates/lc-domain/src/style.rs` (`Token::SelectedCell`)
- Modify: `crates/lc-inbound/src/theme.rs`
- Modify: `crates/lc-inbound/src/body.rs`
- Modify: `crates/lc-inbound/src/render.rs`
- Test: in `crates/lc-inbound/src/theme.rs` and `crates/lc-inbound/src/render.rs`

**Interfaces:**
- Consumes: Task 4's view model.
- Produces: `Body { extent: Extent, columns: Option<Extent>, unit: Unit }` — a third field on the struct `hit.rs` also reads. `hit.rs` needs no change: `RowStrip::of` matches on `body.unit` and `click_at` resolves on `y` alone.

- [ ] **Step 1: Add the token and its style**

In `crates/lc-domain/src/style.rs`, beside `Selected`:

```rust
    /// The row the cursor is on.
    Selected,
    /// The cell the cursor is on, drawn over the selected row: the row says
    /// where you are, the cell says what a command would act on
    /// (doc/specs/2026-08-10-table-display-design.md §3).
    SelectedCell,
```

In `crates/lc-inbound/src/theme.rs`, extend the `Selected` arm:

```rust
            // Reversed video needs no explicit colour, so a selection reads
            // the same way Focused does on an impoverished palette.
            Token::Selected => Style::new().add_modifier(Modifier::REVERSED),
            // Underlined on top of that reversal: the cell has to be visible
            // *against* the selected row, so it cannot spend the same
            // modifier, and a colour would compete with the key-structure
            // colours the grid already carries.
            Token::SelectedCell => Style::new()
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::UNDERLINED),
```

and add the test beside the existing theme tests:

```rust
    #[test]
    fn a_selected_cell_reads_against_the_selected_row_without_spending_a_colour() {
        let row = Theme::TerminalDefault.style(Token::Selected);
        let cell = Theme::TerminalDefault.style(Token::SelectedCell);

        assert_ne!(row.add_modifier, cell.add_modifier);
        assert_eq!(cell.fg, None);
        assert_eq!(cell.bg, None);
    }
```

- [ ] **Step 2: Write the failing render tests**

Append to `mod tests` in `crates/lc-inbound/src/render.rs`. It already has helpers for building a state showing a page and for asserting against a `Buffer`; reuse their spellings. `has_scrollbar` is there and checks the right border — these need a bottom-border twin:

```rust
    /// Whether the block titled `title` has a horizontal scrollbar drawn over
    /// its bottom border. The corners stay `╰` and `╯` whether or not one is
    /// drawn, so the cells between them are what to look at: an untouched
    /// bottom border is all `─`.
    fn has_horizontal_scrollbar(buffer: &Buffer, title: &str) -> bool {
        let area = block_area(buffer, title);
        let y = area.bottom() - 1;

        (area.x + 1..area.right() - 1)
            .any(|x| buffer[(x, y)].symbol() != symbols::line::HORIZONTAL)
    }

    #[test]
    fn a_grid_wider_than_its_panel_gets_a_horizontal_scrollbar() {
        let buffer = frame_showing_grid(12, 40, 20);

        assert!(has_horizontal_scrollbar(&buffer, "Table contents"));
    }

    #[test]
    fn a_grid_that_fits_gets_no_horizontal_scrollbar() {
        let buffer = frame_showing_grid(2, 200, 20);

        assert!(
            !has_horizontal_scrollbar(&buffer, "Table contents"),
            "its absence is what says nothing is hidden"
        );
    }

    #[test]
    fn the_selected_cell_is_marked_apart_from_the_rest_of_its_row() {
        let buffer = frame_showing_grid(4, 40, 3);
        let area = block_area(&buffer, "Table contents");
        // The top border, then the grid's header row, then the first row.
        let y = area.y + 2;
        let styles: Vec<_> = (area.x + 1..area.right() - 1)
            .map(|x| buffer[(x, y)].modifier)
            .collect();

        assert!(
            styles.windows(2).any(|pair| pair[0] != pair[1]),
            "the selected row must not be uniformly styled: the cell sits inside it"
        );
    }

    #[test]
    fn rows_are_numbered_from_one() {
        let buffer = frame_showing_grid(2, 200, 20);
        let text = row_text(&buffer, "Table contents", 1);

        assert!(text.trim_start().starts_with('1'), "got {text:?}");
    }

    #[test]
    fn the_row_number_is_the_pages_own_index_after_a_scroll() {
        // A grid four rows tall over a twelve-row page, scrolled by three.
        let buffer = frame_showing_scrolled_grid(12, 3);
        let text = row_text(&buffer, "Table contents", 1);

        assert!(
            text.trim_start().starts_with('4'),
            "the fourth row is still row 4, got {text:?}"
        );
    }
```

Write `frame_showing_grid(columns, width, rows)` and `frame_showing_scrolled_grid(rows, offset)` beside the file's existing frame helpers, building a `State` the way its neighbours do and rendering into a `TestBackend` of the given width. `row_text(buffer, title, line)` returns the `line`th body row of that block as a `String`; if the file already has an equivalent, use it instead of adding one.

- [ ] **Step 3: Run them and watch them fail**

Run: `cargo test -p lc-inbound render::`
Expected: FAIL — no horizontal scrollbar, no row numbers.

- [ ] **Step 4: Draw the gutter, the cell, and the columns in view**

In `crates/lc-inbound/src/body.rs`: delete `widths()` and `fitting()`, delete the `MAX_COLUMN_WIDTH` and `COLUMN_SPACING` constants, and add `use lc_domain::grid::{COLUMN_SPACING, fitting};` — the arithmetic lives there now, and a second copy of it here is exactly the drift `hit.rs`'s module doc warns about. Then rewrite `grid`:

```rust
/// The view model's grid, as the renderer reads it.
///
/// A struct rather than seven more parameters: the fields arrive together
/// from one `PanelBody::Grid` and mean nothing apart.
#[derive(Clone, Copy)]
struct GridView<'a> {
    columns: &'a [GridColumn],
    rows: &'a [Vec<GridCell>],
    selected: usize,
    offset: usize,
    first_column: usize,
    column: usize,
    gutter: u16,
}

fn grid(frame: &mut Frame, area: Rect, view: &GridView, theme: Theme) -> (Extent, Extent) {
    let GridView {
        columns,
        rows,
        selected,
        offset,
        first_column,
        column,
        gutter,
    } = *view;

    let height = usize::from(area.height).saturating_sub(usize::from(HEADER_ROWS));

    let widths: Vec<u16> = columns.iter().map(|column| column.width).collect();
    let available = area
        .width
        .saturating_sub(gutter)
        .saturating_sub(if gutter == 0 { 0 } else { COLUMN_SPACING });
    let shown = fitting(&widths, first_column, available);
    let drawn = first_column..first_column + shown;

    let constraints = std::iter::once(Constraint::Length(gutter))
        .chain(widths[drawn.clone()].iter().copied().map(Constraint::Length));

    let header = Row::new(
        std::iter::once(Cell::from(Text::raw(" ".repeat(usize::from(gutter))))).chain(
            columns[drawn.clone()]
                .iter()
                .map(|column| styled(&column.name, column.token, column.width, theme)),
        ),
    );

    let body = rows.iter().enumerate().map(|(index, cells)| {
        Row::new(
            std::iter::once(number(index, gutter, theme)).chain(
                cells[drawn.clone()]
                    .iter()
                    .zip(&widths[drawn.clone()])
                    .map(|(cell, width)| styled(&cell.text, cell.token, *width, theme)),
            ),
        )
    });

    let table = Table::new(body, constraints)
        .header(header)
        .column_spacing(COLUMN_SPACING)
        .row_highlight_style(theme.style(Token::Selected))
        .cell_highlight_style(theme.style(Token::SelectedCell));
    // The gutter is a column of the widget but not of the page, so the
    // selected column is one further right than the domain's index.
    let cell = visible(selected, offset, height).map(|row| (row, column - first_column + 1));
    let mut state = TableState::default().with_offset(offset).with_selected_cell(cell);

    frame.render_stateful_widget(table, area, &mut state);

    (
        Extent {
            position: state.offset(),
            total: rows.len(),
            visible: height,
        },
        Extent {
            position: first_column,
            total: columns.len(),
            visible: shown,
        },
    )
}

/// A row's number, right-aligned in the gutter.
fn number(index: usize, gutter: u16, theme: Theme) -> Cell<'static> {
    let text = format!("{:>width$}", index + 1, width = usize::from(gutter));

    Cell::from(Text::raw(text)).style(theme.style(Token::Muted))
}
```

`cells[drawn]` is safe only if every row has as many cells as there are columns, which `view::contents::grid` guarantees by construction from `Page`. If a row could be short, slice with `.get(..)` instead — check `Page`'s invariant before choosing.

Give `Body` its third field and set it in the `Grid` arm:

```rust
pub(crate) struct Body {
    pub(crate) extent: Extent,
    /// The horizontal extent, for the bodies that scroll sideways. Only a
    /// grid does.
    pub(crate) columns: Option<Extent>,
    pub(crate) unit: Unit,
}
```

Every other `Body` construction in the file gets `columns: None`. The `Grid`
arm of `render` — the one Task 4 left with `first_column: _` — becomes:

```rust
        PanelBody::Grid {
            columns,
            rows,
            selected,
            offset,
            first_column,
            column,
            gutter,
        } => {
            let (extent, horizontal) = grid(
                frame,
                area,
                &GridView {
                    columns,
                    rows,
                    selected: *selected,
                    offset: *offset,
                    first_column: *first_column,
                    column: *column,
                    gutter: *gutter,
                },
                theme,
            );

            Some(Body {
                extent,
                columns: Some(horizontal),
                unit: Unit::Rows { header: HEADER_ROWS },
            })
        }
```

- [ ] **Step 5: Draw the horizontal bar**

In `crates/lc-inbound/src/render.rs`, generalise `scrollbar` to take an orientation, keeping the existing vertical call working:

```rust
pub(crate) fn scrollbar(
    frame: &mut Frame,
    area: Rect,
    extent: &Extent,
    border: Style,
    theme: Theme,
) {
    bar(frame, area, extent, ScrollbarOrientation::VerticalRight, border, theme);
}

/// Over the bottom border, mirroring [`scrollbar`] on the right one, and only
/// when a column is off screen — so its absence says nothing is hidden
/// (doc/specs/2026-08-10-table-display-design.md §9).
fn horizontal_scrollbar(
    frame: &mut Frame,
    area: Rect,
    extent: &Extent,
    border: Style,
    theme: Theme,
) {
    bar(frame, area, extent, ScrollbarOrientation::HorizontalBottom, border, theme);
}
```

`bar` is the existing body, with the orientation and the track symbol taken as parameters — `symbols::line::VERTICAL` for the vertical bar, `symbols::line::HORIZONTAL` for the horizontal one — and the margin swapped: `Margin { horizontal: 1, vertical: 0 }` for the horizontal bar, so it does not land on the block's corners. Keep the doc comment on `scrollbar` explaining `ScrollbarState`'s normalisation; it is true of both.

In the panel loop, beside the vertical call:

```rust
                if drawn.extent.overflows() {
                    scrollbar(frame, area, &drawn.extent, border, theme);
                }
                if let Some(columns) = &drawn.columns {
                    if columns.overflows() {
                        horizontal_scrollbar(frame, area, columns, border, theme);
                    }
                }
```

- [ ] **Step 6: Report the grid's width**

Still in `render.rs`, in the same loop, capture the Contents body's inner width and pass it to `Geometry::new` in place of the `0` Task 3 left:

```rust
    let mut grid_width = 0;
```

```rust
                if panel.panel == Panel::Contents {
                    grid_width = usize::from(inner.width);
                }
```

```rust
        geometry: Geometry::new(heights, ddl_lines, command_rows, grid_width),
```

Set it for every body kind, not only `Grid` — it is the panel's width, which is what the domain needs to decide what would fit once a page arrives.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p lc-inbound`
Expected: PASS, including the pre-existing `the_map_resolves_a_scrolled_grid_to_the_rows_that_were_drawn` and `a_grids_header_is_chrome_rather_than_its_first_row` — the gutter must not have moved which row a press lands on.

Run: `mise run ci`
Expected: clean.

- [ ] **Step 8: Look at it**

Run the app against the local fixture and press `c`, then `l` a few times on a wide table. Confirm: the numbers are muted and pinned, the cell is visible against the selected row, and the bottom bar appears only when a column is hidden. The fixture's Cassandra port is contested — check what holds 9042 before overriding `LC_CASSANDRA_PORT`.

- [ ] **Step 9: Commit**

```bash
git add crates/lc-domain/src/style.rs crates/lc-inbound/
git commit -m "feat(inbound): draw the cell, the row numbers and the h-scrollbar

The renderer no longer decides which columns fit — it draws the
columns the view model names, from the offset it names, and reports
the width it had. A pinned muted gutter numbers the page, the
selected cell is underlined over the selected row's reversal, and a
bar over the bottom border says when a column is off screen
(doc/specs/2026-08-10-table-display-design.md §3, §4, §9)."
```

---

### Task 6: Focus follows a read

**Files:**
- Modify: `crates/lc-domain/src/update.rs`
- Modify: `crates/lc-domain/src/state.rs` (`CONTENTS_HINTS`)
- Test: in `crates/lc-domain/src/update.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: no new API. `read` sets `state.focus = Panel::Contents`.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/lc-domain/src/update.rs`:

```rust
    #[test]
    fn reading_a_table_lands_focus_in_contents() {
        for key in [Key::Enter, Key::Char(' ')] {
            let mut state = tabled(&[("shop", &["orders"])]);
            state.focus = Panel::Tables;
            press(&mut state, Key::Char('l'));
            press(&mut state, Key::Char('j'));

            press(&mut state, key);

            assert_eq!(state.focus, Panel::Contents, "{key:?}");
        }
    }

    #[test]
    fn a_click_on_a_table_lands_focus_in_contents_too() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;
        press(&mut state, Key::Char('l'));

        click_row(&mut state, Panel::Tables, 1);

        assert_eq!(state.focus, Panel::Contents);
    }

    #[test]
    fn focus_moves_before_the_page_arrives() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Enter);

        assert!(
            matches!(state.contents.state(), ContentsState::Loading { .. }),
            "focus is a consequence of the keypress, not of the network"
        );
        assert_eq!(state.focus, Panel::Contents);
    }

    #[test]
    fn a_refused_read_still_lands_focus_in_contents() {
        // No connection, so `submit` refuses before a query is made.
        let mut state = tabled(&[("shop", &["orders"])]);
        state.connection = ConnectionState::Disconnected;
        state.focus = Panel::Tables;
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));

        press(&mut state, Key::Enter);

        assert_eq!(state.focus, Panel::Contents);
    }

    #[test]
    fn unfolding_a_keyspace_leaves_focus_in_tables() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;

        press(&mut state, Key::Enter);

        assert_eq!(
            state.focus,
            Panel::Tables,
            "a fold is not a read and moves nothing"
        );
    }

    #[test]
    fn reading_a_table_leaves_the_tree_selection_where_it_was() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.focus = Panel::Tables;
        press(&mut state, Key::Char('l'));
        press(&mut state, Key::Char('j'));
        let selected = state.tables.cursor.index();

        press(&mut state, Key::Enter);

        assert_eq!(state.tables.cursor.index(), selected);
    }
```

`click_row` is whatever the file's existing pointer tests use to synthesise a click on a row — reuse it; do not add a second one. `tabled` builds a connected state in the existing tests, so `a_refused_read_still_lands_focus_in_contents` has to knock the connection out; match how the file's other refusal tests do it rather than the sketch above if they differ.

- [ ] **Step 2: Run and watch them fail**

Run: `cargo test -p lc-domain update::`
Expected: the first four fail — focus stays `Panel::Tables`.

- [ ] **Step 3: Move focus in `read`**

In `crates/lc-domain/src/update.rs`, at the top of `read`:

```rust
/// Replaces whatever the Contents panel is showing with a read of `table`,
/// and takes focus there.
///
/// Focus moves now rather than when the page lands: it is a consequence of the
/// keypress, and a focus change that waits on the network happens at a time
/// the user cannot predict
/// (doc/specs/2026-08-10-table-display-design.md §1).
fn read(state: &mut State, table: TableRef) -> Vec<Effect> {
    state.focus = Panel::Contents;

    let mut effects = Vec::new();
```

Both `Enter`/`Space` (through `activate`) and the click path already funnel through `read`, so one line covers all three gestures. Confirm that with `grep -n "read(state" crates/lc-domain/src/update.rs` before believing it.

- [ ] **Step 4: Teach the hint bar the new motion**

In `crates/lc-domain/src/state.rs`, `CONTENTS_HINTS` gains a binding after `move`. The bar names a focused panel's principal bindings and drops from the left as the frame narrows, so this goes where its value ranks:

```rust
const CONTENTS_HINTS: &[Binding] = &[
    Binding {
        keys: &[Key::Char('j'), Key::Char('k')],
        action: "move",
    },
    Binding {
        keys: &[Key::Char('l'), Key::Char('h')],
        action: "column",
    },
    Binding {
        keys: &[Key::PageDown, Key::PageUp],
        action: "page",
    },
    Binding {
        keys: &[Key::Home, Key::End],
        action: "ends",
    },
];
```

Check `crates/lc-domain/src/commands.rs`'s `no_command_row_is_wider_than_the_narrowest_palette` test still passes — the palette lists the catalogue and a new row must fit it.

- [ ] **Step 5: Run everything**

Run: `cargo test --workspace`
Expected: PASS. Watch specifically for pre-existing tests that assert focus after an `Enter` — `enter_and_space_both_toggle_a_keyspace` and `space_reads_a_table_the_same_way_enter_does` are the likely ones. If either asserts `state.focus == Panel::Tables` after reading a table, that assertion is what this task changes: update it and say so in the commit.

Run: `mise run ci`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-domain/
git commit -m "feat(domain): land focus in contents when a table is read

Enter, Space and a click all take focus to the panel they filled, and
they do it when the key is pressed rather than when the page arrives —
a focus change that waits on the network moves at a time the user
cannot predict. One line in `read`, because all three gestures already
funnel through it (doc/specs/2026-08-10-table-display-design.md §1)."
```

---

### Task 7: The documentation the bindings owe

**Files:**
- Modify: `doc/shortcuts.md`
- Modify: `doc/architecture.md` (the status line and the crate dependency list)
- Modify: `TODO.md`

**Interfaces:** none.

- [ ] **Step 1: §1 — what is bound**

In the Motion table, add the row:

```markdown
| `l` / `→` | Right one column in Contents; unfold the selected keyspace in Tables. |
| `h` / `←` | Left one column in Contents; fold it in Tables, stepping out from a table row first. |
```

and remove `l`/`h` from the **Tables panel** table, leaving `Enter`/`Space` there. Amend that section's preamble so it no longer claims those two keys as Tables-only. In the Tables table, `Enter` / `Space` now reads: "On a keyspace: fold/unfold. On a table: read it into Contents and focus that panel."

- [ ] **Step 2: §2 — the rewritten rule**

Replace the "Vim and arrows are synonyms, both ways" section's second half and extend "The same key means the same thing in every panel" with the design's §2 wording: the four horizontal keys mean *move left* / *move right*, resolved by each panel — fold/unfold in a tree, previous/next column in a grid, inert in Table DDL, text in Queries. State that this is the same shape `j`/`k` already have, and that the vim/arrow pairing is untouched.

- [ ] **Step 3: §4 — one row leaves**

Delete the "Horizontal motion" row from *Deliberately unbound*. Replace it with rows for what this slice still does not bind:

```markdown
| `Home`/`End` over columns | Both reach the ends of the document, which in a grid is the row list. A second axis for one panel would be a third exception to a two-exception rule. |
| Horizontal wheel | Dropped at the edge, and this slice did not pick it up. |
```

- [ ] **Step 4: §5 — a row per new test**

Add rows naming the tests written in Tasks 1–6, with their files. Every test name in this plan appears here, including the render and theme ones. Follow the table's existing format exactly.

- [ ] **Step 5: §7 — the pointer**

Update the "Left click | A table" row: "Focus **Contents**, select the table, read it into Contents". Add a sentence under it noting that a click on a row's number is a click on that row, because resolution is on the `y` axis alone.

- [ ] **Step 6: architecture.md**

Update the **Status** line and **Last updated** date. In §2's crate layout block, `lc-domain`'s dependency list gains `unicode-width`, and the sentence after the block — the one asserting the list is short on purpose — gets a clause explaining why a width table does not breach it, pointing at the design's §6. In §3, note that `Geometry` now also carries the grid's character width and why that keeps the invariant.

- [ ] **Step 7: TODO.md**

Remove whatever entries this slice closed (horizontal navigation, column priority) and add what it opened: expanded-cell inspection for values past the 24-character cap, pinned key columns, and column widths that survive a restart.

- [ ] **Step 8: Verify and commit**

Run: `mise run ci`
Expected: clean.

Re-read `doc/shortcuts.md` §5 against the test names in the tree: `grep -rn "fn a_grid_wider_than_its_panel"` for each row you added. A row naming a test that does not exist is worse than no row.

```bash
git add doc/ TODO.md
git commit -m "docs: record the grid's two-axis navigation and the focus jump

shortcuts.md §1 and §2 rewrite the horizontal keys as 'move left /
move right', resolved per panel — the shape j/k already had. §4 gives
up the row that said horizontal motion was unbuilt, §5 names the
tests, §7 records the click's new destination. architecture.md admits
unicode-width to lc-domain's list and says why the geometry invariant
survives (doc/specs/2026-08-10-table-display-design.md)."
```

---

## Verification before calling it done

- [ ] `mise run ci` clean from a clean tree.
- [ ] `git log --oneline` shows seven commits, each one green on its own.
- [ ] Every test named in `doc/shortcuts.md` §5 exists — check by grep, not by memory.
- [ ] The app was actually run against the fixture and the grid looked right (Task 5, Step 8). A render test asserting a buffer is not the same as having looked at it.
