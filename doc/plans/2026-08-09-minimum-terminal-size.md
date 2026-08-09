# Minimum terminal size — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Below 40×6 the shell is not drawn at all; a short centred notice says what is needed instead.

**Architecture:** `layout` stops returning `PanelRects` and returns a `Shell`
enum whose other variant carries no panels, so `render` has no path on which it
draws a panel into a frame too small to hold one. The floor is derived from the
panel floors already in `layout.rs`, not declared. The notice is a new
`too_small` module in `lc-inbound`, drawn without a theme token because it is
the only thing on screen.

**Tech Stack:** Rust, `ratatui` 0.30.2, no new dependencies. `lc-domain` is not
touched at all — this is entirely a renderer-side change.

**Spec:** [doc/specs/2026-08-09-minimum-terminal-size-design.md](../specs/2026-08-09-minimum-terminal-size-design.md)

## Global Constraints

- `AGENTS.md` non-negotiables apply to every task: no `unsafe`, no narrating
  comments, no warnings, no ignored tests, no `#[allow]` without a reason.
- `lc-domain` gains no dependency and no code. If a task seems to need one, the
  design is wrong — stop and raise it.
- `mise run ci` must be clean before a task is considered done. Individual steps
  use `cargo test -p lc-inbound` for speed; the last step of each task runs the
  gate.
- Conventional Commits, imperative, lowercase, no trailing period.
- Test names are sentences about behaviour, matching the surrounding files
  (`both_floors_still_fit_at_six_rows`), never `test_foo`.
- The multiplication sign in the notice is `×` (U+00D7), not the letter `x`.

---

### Task 1: `layout` gates on a minimum and returns a `Shell`

**Files:**
- Modify: `crates/lc-inbound/src/layout.rs` (constants at 9-18, `layout` at 43-58, tests at 83-191)
- Modify: `crates/lc-inbound/src/render.rs:22` (the single caller, minimally — Task 2 does the real work)

**Interfaces:**
- Produces: `pub enum Shell { Panels(PanelRects), TooSmall(Rect) }` and
  `pub fn layout(area: Rect) -> Shell`, plus `pub(crate) const MIN_W: u16` and
  `pub(crate) const MIN_H: u16`. Task 2 matches on `Shell` and reads both
  constants.
- Consumes: nothing.

- [ ] **Step 1: Write the failing tests in `layout.rs`**

Add to `mod tests` in `crates/lc-inbound/src/layout.rs`:

```rust
    #[test]
    fn a_frame_below_the_minimum_gets_no_panels() {
        for (width, height) in [(39, 6), (40, 5), (39, 5), (1, 1), (0, 0)] {
            let area = Rect::new(0, 0, width, height);

            assert_eq!(
                layout(area),
                Shell::TooSmall(area),
                "{width}x{height} is below {MIN_W}x{MIN_H}"
            );
        }
    }

    #[test]
    fn the_minimum_is_the_size_at_which_every_panel_still_fits() {
        let r = rects(MIN_W, MIN_H);

        for panel in Panel::ALL {
            assert!(
                !r.get(panel).is_empty(),
                "{panel:?} is empty at the minimum, so the minimum is wrong"
            );
        }
    }

    #[test]
    fn the_minimum_is_derived_from_the_floors_rather_than_chosen() {
        assert_eq!((MIN_W, MIN_H), (40, 6));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lc-inbound layout`
Expected: FAIL to compile — `cannot find type Shell in this scope`, and
`MIN_W`/`MIN_H` not found.

- [ ] **Step 3: Add the constants and the `Shell` type**

In `crates/lc-inbound/src/layout.rs`, after the existing constants (below
`MAIN_RESERVE_W` at line 18):

```rust
/// The smallest frame the shell is drawn into: both columns on their width
/// floors, and one panel above another on its height floor in each.
pub(crate) const MIN_W: u16 = SIDEBAR_MIN_W + MAIN_RESERVE_W;
pub(crate) const MIN_H: u16 = 2 * PANEL_MIN_H;
```

Then, above `PanelRects`:

```rust
/// What a frame is big enough to be.
///
/// An enum rather than a `PanelRects` plus a separate `fits` predicate, so
/// there is no path on which a caller draws a panel into a frame too small to
/// hold one (conventions.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Panels(PanelRects),
    /// The frame, which fell short of `MIN_W` × `MIN_H`.
    TooSmall(Rect),
}
```

- [ ] **Step 4: Gate `layout`**

Replace the signature and the first line of the body of `layout`:

```rust
/// Divide `area` into the four panels of doc/ui-draft.png, or refuse it.
#[must_use]
pub fn layout(area: Rect) -> Shell {
    if area.width < MIN_W || area.height < MIN_H {
        return Shell::TooSmall(area);
    }

    let (main_w, side_w) = split(area.width, SIDEBAR_PERCENT, MAIN_RESERVE_W, SIDEBAR_MIN_W);
    let (tables_h, ddl_h) = split(area.height, DDL_PERCENT, PANEL_MIN_H, PANEL_MIN_H);
    let (contents_h, queries_h) = split(area.height, QUERIES_PERCENT, PANEL_MIN_H, PANEL_MIN_H);

    let main_x = area.x + side_w;

    Shell::Panels(PanelRects {
        tables: Rect::new(area.x, area.y, side_w, tables_h),
        ddl: Rect::new(area.x, area.y + tables_h, side_w, ddl_h),
        contents: Rect::new(main_x, area.y, main_w, contents_h),
        queries: Rect::new(main_x, area.y + contents_h, main_w, queries_h),
    })
}
```

- [ ] **Step 5: Update the test helper and the caller so the crate compiles**

The `rects` helper in `mod tests` must unwrap the new enum. Replace it:

```rust
    fn rects(width: u16, height: u16) -> PanelRects {
        panels(Rect::new(0, 0, width, height))
    }

    fn panels(area: Rect) -> PanelRects {
        match layout(area) {
            Shell::Panels(rects) => rects,
            Shell::TooSmall(area) => panic!("{area:?} is below {MIN_W}x{MIN_H}"),
        }
    }
```

`the_layout_is_offset_by_the_area_origin` calls `layout` directly; change its
first line to use the new helper:

```rust
        let r = panels(Rect::new(7, 3, 100, 100));
```

In `crates/lc-inbound/src/render.rs`, make line 22 compile without changing
behaviour for frames at or above the floor — Task 2 replaces this:

```rust
    let Shell::Panels(rects) = layout(frame.area()) else {
        return Geometry::default();
    };
```

and add `Shell` to the import on line 12:

```rust
use crate::layout::{Shell, layout};
```

- [ ] **Step 6: Run the new tests**

Run: `cargo test -p lc-inbound layout`
Expected: the three new tests PASS, and exactly three existing ones FAIL on the
`panic!` in `panels` because they name sizes now below the floor —
`the_minor_panel_collapses_rather_than_shrink_below_its_floor` (80×5),
`the_sidebar_disappears_before_the_main_column_does` (20×24), and
`the_panels_tile_the_area_exactly` (its list holds 0×0, 1×1, 20×24, 80×5).
Steps 7 and 8 deal with them.

- [ ] **Step 7: Move the below-floor tests onto `split`**

Two existing tests reach `split`'s collapse branch through sizes that now
return `TooSmall`, so they can no longer reach it. Delete
`the_minor_panel_collapses_rather_than_shrink_below_its_floor` and
`the_sidebar_disappears_before_the_main_column_does` from
`crates/lc-inbound/src/layout.rs`, and add in their place:

```rust
    #[test]
    fn split_collapses_the_minor_part_rather_than_shrink_it_below_its_floor() {
        // Unreachable through `layout`, which now refuses these sizes. `split`
        // stays total over them, so it is tested where it is total
        // (minimum terminal size design §5).
        assert_eq!(
            split(5, QUERIES_PERCENT, PANEL_MIN_H, PANEL_MIN_H),
            (5, 0),
            "a one-row bordered panel looks present and shows nothing"
        );
        assert_eq!(
            split(20, SIDEBAR_PERCENT, MAIN_RESERVE_W, SIDEBAR_MIN_W),
            (20, 0),
            "the sidebar goes before the main column does"
        );
    }

    #[test]
    fn split_lifts_the_minor_part_to_its_floor() {
        assert_eq!(split(24, QUERIES_PERCENT, PANEL_MIN_H, PANEL_MIN_H), (21, 3));
    }
```

- [ ] **Step 8: Narrow the tiling test to the sizes that yield panels**

In `the_panels_tile_the_area_exactly`, replace the `sizes` array — the four
below-floor entries are covered by `a_frame_below_the_minimum_gets_no_panels`,
and `MIN_W`/`MIN_H` joins as the tightest case:

```rust
        let sizes = [
            (MIN_W, MIN_H),
            (40, 12),
            (40, 24),
            (80, 24),
            (100, 100),
            (u16::MAX, u16::MAX),
        ];
```

and its body's first line, so it goes through the helper:

```rust
            let area = Rect::new(0, 0, width, height);
            let r = panels(area);
```

- [ ] **Step 9: Run the whole crate's tests**

Run: `cargo test -p lc-inbound`
Expected: PASS. The render tests still pass because Step 5 kept every frame at
or above the floor behaving exactly as before.

Note: `a_collapsed_panel_is_not_drawn_at_all` in `render.rs` draws at 40×5 and
now sees an empty frame. Its three assertions are `contains`/`!contains` on rows
of an empty buffer, so `!contains("Table DDL")` and `!contains("Queries")` still
hold but `contains("Tables")` does not. If it fails here, do not patch it in
this task — Task 2 replaces it. If it passes, leave it; Task 2 replaces it
anyway.

- [ ] **Step 10: Run the gate**

Run: `mise run ci`
Expected: clean — zero warnings, all tests pass, `cargo deny` ok.

- [ ] **Step 11: Commit**

```bash
git add crates/lc-inbound/src/layout.rs crates/lc-inbound/src/render.rs
git commit -m "feat(inbound): refuse a frame too small to hold the shell

layout returns Shell::Panels or Shell::TooSmall, so no caller can draw a
panel into a frame below the floor. The floor is derived from the panel
floors rather than declared, and split keeps its collapse branch — tested
directly, since layout no longer reaches it."
```

---

### Task 2: The notice, and `render` matching on the `Shell`

**Files:**
- Create: `crates/lc-inbound/src/too_small.rs`
- Modify: `crates/lc-inbound/src/lib.rs` (module list at 10-16)
- Modify: `crates/lc-inbound/src/render.rs` (`render` at 21-66, tests at 232-243)
- Modify: `doc/specs/2026-08-09-minimum-terminal-size-design.md` (status line)

**Interfaces:**
- Consumes: `Shell::{Panels, TooSmall}`, `MIN_W`, `MIN_H` from Task 1.
- Produces: `pub(crate) fn too_small::render(frame: &mut Frame, area: Rect)`.
  Nothing later in this slice depends on it; the hint bar slice does not either.

- [ ] **Step 1: Write the failing tests in `render.rs`**

Replace `a_collapsed_panel_is_not_drawn_at_all` in
`crates/lc-inbound/src/render.rs` with:

```rust
    #[test]
    fn a_frame_below_the_minimum_draws_the_notice_and_nothing_else() {
        for (width, height) in [(39, 6), (40, 5), (39, 5)] {
            let buffer = draw(width, height);
            let text: String = (0..height).map(|y| row(&buffer, y)).collect();

            assert!(
                text.contains("need 40×6"),
                "{width}x{height} must say what it needs, got {text:?}"
            );
            assert!(
                !text.contains('┌'),
                "{width}x{height} must draw no panel, got {text:?}"
            );
        }
    }

    #[test]
    fn the_notice_names_the_frame_it_was_given() {
        let buffer = draw(24, 5);
        let text: String = (0..5).map(|y| row(&buffer, y)).collect();

        assert!(text.contains("have 24×5"), "got {text:?}");
    }

    #[test]
    fn the_notice_drops_its_least_useful_line_first() {
        let lines = |height| {
            let buffer = draw(30, height);
            (0..height)
                .map(|y| row(&buffer, y).trim().to_owned())
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
        };

        assert_eq!(lines(3), ["Too small", "need 40×6", "have 30×3"]);
        assert_eq!(lines(2), ["need 40×6", "have 30×2"]);
        assert_eq!(lines(1), ["need 40×6"]);
    }

    #[test]
    fn nothing_is_drawn_over_the_notice() {
        let state = State {
            overlay: Some(Overlay {
                title: "Cannot connect".to_owned(),
                message: "no route to host".to_owned(),
                severity: Severity::Danger,
            }),
            ..State::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(39, 6))
            .expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(frame, &view_model(&state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");

        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..6).map(|y| row(&buffer, y)).collect();

        assert!(
            !text.contains("Cannot connect"),
            "an overlay has nothing to sit over, got {text:?}"
        );
    }

    #[test]
    fn a_frame_below_the_minimum_measures_nothing() {
        let state = State::default();
        let mut terminal = Terminal::new(TestBackend::new(39, 6))
            .expect("TestBackend always initialises");
        let mut geometry = Geometry::default();

        terminal
            .draw(|frame| {
                geometry = render(frame, &view_model(&state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");

        assert_eq!(geometry, Geometry::default());
    }
```

Add `Overlay` and `Severity` to the test imports at the top of `mod tests` —
`use lc_domain::state::{Connection, ConnectionState, Geometry, Overlay, Panel, SchemaState};`
and `use lc_domain::style::Severity;`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p lc-inbound render`
Expected: FAIL — the notice tests find an empty buffer, because Task 1's early
return draws nothing.

- [ ] **Step 3: Write the notice module**

Create `crates/lc-inbound/src/too_small.rs`:

```rust
//! What is drawn when the frame cannot hold the shell.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::layout::{MIN_H, MIN_W};

/// The most lines the notice draws, and so the most rows it needs.
const LINES: u16 = 3;

pub(crate) fn render(frame: &mut Frame, area: Rect) {
    let lines = lines(area);
    if lines.is_empty() {
        return;
    }

    let height = area.height.min(LINES);
    let y = area.y + (area.height - height) / 2;
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        Rect::new(area.x, y, area.width, height),
    );
}

/// Short lines rather than a sentence: a sentence explaining that the terminal
/// is too narrow does not fit in a terminal that is too narrow. They drop least
/// actionable first, so what the user must do survives longest.
fn lines(area: Rect) -> Vec<Line<'static>> {
    let need = Line::from(format!("need {MIN_W}×{MIN_H}"));
    let have = Line::from(format!("have {}×{}", area.width, area.height));

    match area.height {
        0 => Vec::new(),
        1 => vec![need],
        2 => vec![need, have],
        _ => vec![Line::from("Too small"), need, have],
    }
}
```

No theme token: the notice is the only thing on screen, so it has nothing to be
distinguished from, and vision.md §4 principle 5 says colour carries meaning
rather than decoration.

- [ ] **Step 4: Register the module**

In `crates/lc-inbound/src/lib.rs`, insert `mod too_small;` after `pub mod
theme;`, keeping the list alphabetical, so it reads:

```rust
mod body;
mod details;
pub mod input;
pub mod layout;
mod overlay;
pub mod render;
pub mod theme;
mod too_small;
```

- [ ] **Step 5: Match on the `Shell` in `render`**

In `crates/lc-inbound/src/render.rs`, replace the `let ... else` that Task 1
left at line 22:

```rust
    let rects = match layout(frame.area()) {
        Shell::Panels(rects) => rects,
        Shell::TooSmall(area) => {
            crate::too_small::render(frame, area);
            return Geometry::default();
        }
    };
```

The early return is what keeps the overlay off the screen: it sits below this
point in the function.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p lc-inbound`
Expected: PASS, all of them.

- [ ] **Step 7: Mark the design implemented**

In `doc/specs/2026-08-09-minimum-terminal-size-design.md`, change the status
line to:

```markdown
**Status:** implemented
```

- [ ] **Step 8: Run the gate**

Run: `mise run ci`
Expected: clean — zero warnings, all tests pass, `cargo deny` ok.

- [ ] **Step 9: Commit**

```bash
git add crates/lc-inbound/src/too_small.rs crates/lc-inbound/src/lib.rs \
        crates/lc-inbound/src/render.rs doc/specs/2026-08-09-minimum-terminal-size-design.md
git commit -m "feat(inbound): say the terminal is too small rather than drawing a broken frame

Three short lines, dropped least actionable first, in place of a shell that
below 40x6 degrades into a corner character and no explanation. q still
quits: handle_key has never consulted geometry."
```

---

## Verification

After both tasks, by hand rather than by test — the fast tiers cannot resize a
real terminal:

```
mise run db:up
cargo run
```

Drag the terminal below 40 columns and below 6 rows, on each axis
independently. Expect the notice, expect it to track the size as you drag, and
expect `q` to quit from inside it. Drag back out and expect the panels to
return with focus and selection where you left them.
