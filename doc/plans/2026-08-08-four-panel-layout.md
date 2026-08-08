# Four-Panel Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Draw the four titled, proportionally sized panels of `doc/ui-draft.png` as the application's frame.

**Architecture:** `lc-domain` gains a four-variant `Panel` and a minimal `ViewModel` carrying titles and focus (architecture.md §3). `lc-inbound` gains a pure `layout(Rect) -> PanelRects` that computes the splits as `u16` arithmetic rather than delegating to ratatui's constraint solver, plus a `render` that draws one bordered block per panel. `lc-app` swaps its placeholder renderer for a call into `lc-inbound`.

**Tech Stack:** Rust 2024 edition, rustc 1.97.1, `ratatui` 0.30.2, `crossterm` 0.29.0, `cargo-nextest` 0.9.143.

**Spec:** `doc/specs/2026-08-08-four-panel-layout-design.md`

## Global Constraints

- **No `unsafe`, no FFI.** `unsafe_code = "forbid"` at the workspace root.
- **`lc-domain` depends on neither `ratatui` nor `crossterm` nor `tokio`.** No task adds a dependency to any `Cargo.toml`; every crate already has what it needs.
- **Zero warnings.** `mise run ci` (`fmt:check` + `lint` + `test` + `deny`) must be clean before any task is done. Clippy runs `--workspace --all-targets -D warnings` with `all` and `pedantic` enabled.
- **`#[must_use]` on public functions returning a value** — clippy `pedantic` enables `must_use_candidate`.
- **`Debug` on every public type** — `missing_debug_implementations = "warn"`.
- **`unwrap`/`expect` are permitted in tests only.** `clippy.toml` sets `allow-unwrap-in-tests = true` and `allow-expect-in-tests = true`; outside tests both are warnings.
- **Public items in `lc-domain` carry `///` doc comments** (architecture.md §12). Implementation comments follow conventions.md §2: no narrating idiomatic Rust.
- **Tests are inline `#[cfg(test)] mod tests`**, matching `crates/lc-domain/src/update.rs:117`. No `tests/` directories exist in this workspace. Test names are full sentences; assertions carry messages where the failure would otherwise be cryptic.
- **Determinism** (conventions.md §4): no clock, no network, no filesystem, no unseeded randomness, no `HashMap` iteration order. Every size used in a test is written out, never sampled.
- **Conventional Commits**, imperative, lowercase, no trailing period. Reference the spec or architecture section in the body when it explains *why*.
- **Never `git pull`, never `git push`.**

---

### Task 1: Replace the panel vocabulary

The current `Panel` has five variants chosen before the layout existed. It is defined but never matched anywhere in the workspace, so this is a safe swap.

**Files:**
- Modify: `crates/lc-domain/src/state.rs:32-42`
- Test: same file, new `#[cfg(test)] mod tests` at the bottom

**Interfaces:**
- Consumes: nothing.
- Produces: `lc_domain::state::Panel` with variants `Tables`, `Ddl`, `Contents`, `Queries` (that declaration order is load-bearing), `Panel::default() == Panel::Tables`, and the associated const `Panel::ALL: [Panel; 4]`.

- [ ] **Step 1: Write the failing test**

Append to `crates/lc-domain/src/state.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_panels_are_listed_in_reading_order() {
        assert_eq!(
            Panel::ALL,
            [Panel::Tables, Panel::Ddl, Panel::Contents, Panel::Queries]
        );
    }

    #[test]
    fn the_default_panel_is_tables() {
        assert_eq!(Panel::default(), Panel::Tables);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run --package lc-domain state::tests`
Expected: FAIL — compile error, `no variant or associated item named 'Tables'` / `no associated item named 'ALL'`.

- [ ] **Step 3: Write the implementation**

Replace the `Panel` enum at `crates/lc-domain/src/state.rs:32-42` with:

```rust
/// Panels are places: fixed positions, navigated with predictable motions
/// (vision.md §4, principle 4).
///
/// Declaration order is reading order — top-left, bottom-left, top-right,
/// bottom-right — and [`Panel::ALL`] preserves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    /// Keyspace → table tree.
    #[default]
    Tables,
    /// `CREATE TABLE` for the selected table.
    Ddl,
    /// Rows of the selected table.
    Contents,
    /// Statement entry and session log.
    Queries,
}

impl Panel {
    /// Every panel, in reading order.
    pub const ALL: [Self; 4] = [Self::Tables, Self::Ddl, Self::Contents, Self::Queries];
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run --workspace`
Expected: PASS, including the pre-existing `update` tests.

- [ ] **Step 5: Verify the gate is clean**

Run: `mise run ci`
Expected: all four sub-tasks pass with no output from clippy.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-domain/src/state.rs
git commit -m "$(cat <<'EOF'
refactor!: replace Panel with the four panels the draft names

The five variants predate any layout. doc/ui-draft.png shows four, and
Panel was defined but never matched, so nothing else moves.

BREAKING CHANGE: Panel::{Keyspaces, Editor, Results, Log} are gone.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Project state into a ViewModel

architecture.md §3 promises `view_model(state) -> ViewModel`; it does not exist. This adds the smallest honest version so `lc-inbound` never reads `State`.

**Files:**
- Create: `crates/lc-domain/src/view.rs`
- Modify: `crates/lc-domain/src/lib.rs` (module declaration and re-export)
- Test: inline in `crates/lc-domain/src/view.rs`

**Interfaces:**
- Consumes: `Panel`, `Panel::ALL` from Task 1.
- Produces: `lc_domain::view::{ViewModel, PanelView, view_model}`. `ViewModel { panels: [PanelView; 4], focus: Panel }`, `PanelView { panel: Panel, title: String }`, `fn view_model(state: &State) -> ViewModel`. Titles are exactly `"Tables"`, `"Table DDL"`, `"Table contents"`, `"Queries"`.

- [ ] **Step 1: Write the failing test**

Create `crates/lc-domain/src/view.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_panel_is_projected_in_reading_order() {
        let view = view_model(&State::default());

        assert_eq!(view.panels.map(|panel| panel.panel), Panel::ALL);
    }

    #[test]
    fn panels_carry_the_titles_the_draft_names() {
        let view = view_model(&State::default());

        assert_eq!(
            view.panels.map(|panel| panel.title),
            ["Tables", "Table DDL", "Table contents", "Queries"].map(str::to_owned)
        );
    }

    #[test]
    fn focus_mirrors_the_state() {
        let state = State {
            focus: Panel::Contents,
            ..State::default()
        };

        assert_eq!(view_model(&state).focus, Panel::Contents);
    }
}
```

Add to `crates/lc-domain/src/lib.rs`, keeping the module list alphabetical (after `pub mod update;` is wrong — `view` sorts last, so append it there):

```rust
pub mod view;
```

and extend the re-export block:

```rust
pub use view::{PanelView, ViewModel, view_model};
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run --package lc-domain view::tests`
Expected: FAIL — compile error, `cannot find function 'view_model' in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/lc-domain/src/view.rs`, above the test module:

```rust
//! The domain's projection of itself for a renderer.
//!
//! architecture.md §3. One tree is rebuilt per frame; §11's question about
//! per-panel view models with dirty tracking stays open until panels carry
//! enough content to measure.

use crate::state::{Panel, State};

/// Everything a renderer needs for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewModel {
    /// In [`Panel::ALL`] order.
    pub panels: [PanelView; 4],
    pub focus: Panel,
}

/// One panel, described without reference to how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelView {
    pub panel: Panel,
    pub title: String,
}

/// Project `state` into the frame a renderer draws.
#[must_use]
pub fn view_model(state: &State) -> ViewModel {
    ViewModel {
        panels: Panel::ALL.map(|panel| PanelView {
            panel,
            title: title(panel).to_owned(),
        }),
        focus: state.focus,
    }
}

const fn title(panel: Panel) -> &'static str {
    match panel {
        Panel::Tables => "Tables",
        Panel::Ddl => "Table DDL",
        Panel::Contents => "Table contents",
        Panel::Queries => "Queries",
    }
}
```

`title` returns `&'static str` and `view_model` allocates, so a Phase 1 title such as `Tables — system_schema` needs no change at the type boundary. The `[PanelView; 4]` length is checked by the compiler against `Panel::ALL.map(..)`: adding a fifth panel fails to compile here rather than silently dropping one.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run --workspace`
Expected: PASS.

- [ ] **Step 5: Verify the gate is clean**

Run: `mise run ci`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-domain/src/view.rs crates/lc-domain/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(domain): add the ViewModel projection

architecture.md §3 has promised view_model() since before there was
anything to project. Minimal version: panel titles and focus, one tree
per frame. §11's granularity question stays open.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Give focus a semantic style token

The domain never names a colour (architecture.md §6), so drawing a focused panel needs a token first.

**Files:**
- Modify: `crates/lc-domain/src/style.rs:11-18`
- Modify: `crates/lc-inbound/src/theme.rs:29-47`
- Test: inline in `crates/lc-inbound/src/theme.rs`

**Interfaces:**
- Consumes: `Posture` (already imported by both files).
- Produces: `lc_domain::style::Token::Focused`, and `Theme::TerminalDefault` mapping it to `Modifier::BOLD` with no foreground and no background.

- [ ] **Step 1: Write the failing test**

Append to `crates/lc-inbound/src/theme.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_is_drawn_without_spending_a_colour() {
        let style = Theme::TerminalDefault.style(Token::Focused);

        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(
            style.fg, None,
            "focus must survive a terminal with a poor palette"
        );
    }

    #[test]
    fn no_token_sets_a_background() {
        let tokens = [
            Token::PartitionKey,
            Token::ClusteringKey,
            Token::RegularColumn,
            Token::Posture(Posture::ReadOnly),
            Token::Posture(Posture::Writable),
            Token::Posture(Posture::Protected),
            Token::Severity(Severity::Info),
            Token::Severity(Severity::Warning),
            Token::Severity(Severity::Danger),
            Token::Muted,
            Token::Focused,
        ];

        for token in tokens {
            assert_eq!(
                Theme::TerminalDefault.style(token).bg,
                None,
                "{token:?} set a background (vision.md §4, principle 5)"
            );
        }
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run --package lc-inbound theme::tests`
Expected: FAIL — compile error, `no variant named 'Focused' found for enum 'Token'`.

- [ ] **Step 3: Write the implementation**

In `crates/lc-domain/src/style.rs`, add a variant to `Token`:

```rust
/// A thing worth distinguishing visually.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    PartitionKey,
    ClusteringKey,
    RegularColumn,
    Posture(Posture),
    Severity(Severity),
    Muted,
    /// The panel accepting input.
    Focused,
}
```

The match in `Theme::terminal_default` is now non-exhaustive and will not compile. In `crates/lc-inbound/src/theme.rs`, add the arm after `Token::Muted`:

```rust
            Token::Muted => Style::new().add_modifier(Modifier::DIM),
            // Bold rather than a colour: focus has to read on a terminal
            // whose palette is unusable, and it must not compete with the
            // colours that carry meaning.
            Token::Focused => Style::new().add_modifier(Modifier::BOLD),
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run --workspace`
Expected: PASS.

- [ ] **Step 5: Verify the gate is clean**

Run: `mise run ci`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-domain/src/style.rs crates/lc-inbound/src/theme.rs
git commit -m "$(cat <<'EOF'
feat: add a Focused style token

Drawing the focused panel needs a meaning the domain can name without
naming a colour (architecture.md §6). Bold, no fg, no bg — it has to
read on a terminal with an unusable palette.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Compute the panel geometry

The heart of the change, and the only part with interesting behaviour. Pure `Rect` in, `Rect`s out — no frame, no state, no terminal.

**Files:**
- Create: `crates/lc-inbound/src/layout.rs`
- Modify: `crates/lc-inbound/src/lib.rs` (module declaration)
- Test: inline in `crates/lc-inbound/src/layout.rs`

**Interfaces:**
- Consumes: `Panel`, `Panel::ALL` from Task 1.
- Produces: `lc_inbound::layout::{PanelRects, layout}`. `fn layout(area: Rect) -> PanelRects` and `PanelRects::get(&self, panel: Panel) -> Rect`. `PanelRects` fields are private; `get` is the only accessor.

**Why not `Constraint`:** ratatui resolves `Layout` through a cassowary solver. Mixing `Percentage` with `Min` to mean "10%, but never below three rows" makes the small-terminal outcome a property of the solver's internal priorities — a ratatui version detail, not a decision of ours. conventions.md §4 wants behaviour that is reproducible *because it is specified*, so the splits are `u16` arithmetic and only resolved lengths reach `Rect`.

- [ ] **Step 1: Write the failing test**

Create `crates/lc-inbound/src/layout.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn rects(width: u16, height: u16) -> PanelRects {
        layout(Rect::new(0, 0, width, height))
    }

    #[test]
    fn the_draft_proportions_hold_when_there_is_room() {
        let r = rects(100, 100);

        assert_eq!(r.get(Panel::Tables), Rect::new(0, 0, 25, 60));
        assert_eq!(r.get(Panel::Ddl), Rect::new(0, 60, 25, 40));
        assert_eq!(r.get(Panel::Contents), Rect::new(25, 0, 75, 90));
        assert_eq!(r.get(Panel::Queries), Rect::new(25, 90, 75, 10));
    }

    #[test]
    fn the_layout_is_offset_by_the_area_origin() {
        let r = layout(Rect::new(7, 3, 100, 100));

        assert_eq!(r.get(Panel::Tables), Rect::new(7, 3, 25, 60));
        assert_eq!(r.get(Panel::Queries), Rect::new(32, 93, 75, 10));
    }

    #[test]
    fn queries_is_lifted_to_its_floor_on_a_short_terminal() {
        // 10% of 24 rows is 2: two borders and no content.
        let r = rects(80, 24);

        assert_eq!(r.get(Panel::Contents), Rect::new(20, 0, 60, 21));
        assert_eq!(r.get(Panel::Queries), Rect::new(20, 21, 60, 3));
    }

    #[test]
    fn the_sidebar_is_lifted_to_its_floor_on_a_narrow_terminal() {
        // 25% of 40 columns is 10.
        let r = rects(40, 24);

        assert_eq!(r.get(Panel::Tables).width, 16);
        assert_eq!(r.get(Panel::Contents).width, 24);
    }

    #[test]
    fn both_floors_still_fit_at_six_rows() {
        let r = rects(80, 6);

        assert_eq!(r.get(Panel::Contents).height, 3);
        assert_eq!(r.get(Panel::Queries).height, 3);
    }

    #[test]
    fn the_minor_panel_collapses_rather_than_shrink_below_its_floor() {
        let r = rects(80, 5);

        assert_eq!(r.get(Panel::Contents).height, 5);
        assert_eq!(
            r.get(Panel::Queries).height,
            0,
            "a one-row bordered panel looks present and shows nothing"
        );
        assert_eq!(r.get(Panel::Ddl).height, 0);
    }

    #[test]
    fn the_sidebar_disappears_before_the_main_column_does() {
        let r = rects(20, 24);

        assert_eq!(r.get(Panel::Tables).width, 0);
        assert_eq!(r.get(Panel::Contents).width, 20);
    }

    #[test]
    fn the_panels_tile_the_area_exactly() {
        let sizes = [
            (0, 0),
            (1, 1),
            (20, 24),
            (40, 12),
            (80, 5),
            (80, 24),
            (100, 100),
            (u16::MAX, u16::MAX),
        ];

        for (width, height) in sizes {
            let area = Rect::new(0, 0, width, height);
            let r = layout(area);

            let covered: u32 = Panel::ALL.iter().map(|&panel| r.get(panel).area()).sum();
            assert_eq!(
                covered,
                area.area(),
                "panels must cover {width}x{height} with no gap"
            );

            for (index, &first) in Panel::ALL.iter().enumerate() {
                for &second in &Panel::ALL[index + 1..] {
                    let (a, b) = (r.get(first), r.get(second));
                    assert!(
                        a.is_empty() || b.is_empty() || !a.intersects(b),
                        "{first:?} and {second:?} overlap at {width}x{height}"
                    );
                }
            }
        }
    }
}
```

Add to `crates/lc-inbound/src/lib.rs`, before `pub mod theme;`:

```rust
pub mod layout;
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run --package lc-inbound layout::tests`
Expected: FAIL — compile error, `cannot find function 'layout' in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/lc-inbound/src/layout.rs`, above the test module:

```rust
//! Panel geometry.
//!
//! Proportions come from doc/ui-draft.png; the floors and the collapse rule
//! from doc/specs/2026-08-08-four-panel-layout-design.md §3.

use lc_domain::state::Panel;
use ratatui::layout::Rect;

const SIDEBAR_PERCENT: u16 = 25;
const DDL_PERCENT: u16 = 40;
const QUERIES_PERCENT: u16 = 10;

/// Two borders and one line of content. Below this a block shows nothing.
const PANEL_MIN_H: u16 = 3;
const SIDEBAR_MIN_W: u16 = 16;
const MAIN_MIN_W: u16 = 24;

/// Where each panel is drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelRects {
    tables: Rect,
    ddl: Rect,
    contents: Rect,
    queries: Rect,
}

impl PanelRects {
    /// The area assigned to `panel`, which may be empty when the terminal is
    /// too small to hold it.
    #[must_use]
    pub const fn get(&self, panel: Panel) -> Rect {
        match panel {
            Panel::Tables => self.tables,
            Panel::Ddl => self.ddl,
            Panel::Contents => self.contents,
            Panel::Queries => self.queries,
        }
    }
}

/// Divide `area` into the four panels of doc/ui-draft.png.
#[must_use]
pub fn layout(area: Rect) -> PanelRects {
    let (main_w, side_w) = split(area.width, SIDEBAR_PERCENT, MAIN_MIN_W, SIDEBAR_MIN_W);
    let (tables_h, ddl_h) = split(area.height, DDL_PERCENT, PANEL_MIN_H, PANEL_MIN_H);
    let (contents_h, queries_h) = split(area.height, QUERIES_PERCENT, PANEL_MIN_H, PANEL_MIN_H);

    let main_x = area.x + side_w;

    PanelRects {
        tables: Rect::new(area.x, area.y, side_w, tables_h),
        ddl: Rect::new(area.x, area.y + tables_h, side_w, ddl_h),
        contents: Rect::new(main_x, area.y, main_w, contents_h),
        queries: Rect::new(main_x, area.y + contents_h, main_w, queries_h),
    }
}

/// Split `total` into a major and a minor part, in that order.
///
/// The minor part takes `minor_percent`, lifted to `minor_floor` when the
/// percentage falls short of it, and dropped to zero when honouring it would
/// push the major part below `major_floor`.
fn split(total: u16, minor_percent: u16, major_floor: u16, minor_floor: u16) -> (u16, u16) {
    // u16 arithmetic overflows here at terminal widths that cannot exist but
    // that the parameter type admits.
    let share = u32::from(total) * u32::from(minor_percent) / 100;
    let share = u16::try_from(share).unwrap_or(u16::MAX);

    let cap = total.saturating_sub(major_floor);
    // Not defensive: `clamp` panics on crossed bounds, and this branch is what
    // guarantees `minor_floor <= cap` below.
    let minor = if cap < minor_floor {
        0
    } else {
        share.clamp(minor_floor, cap)
    };

    (total - minor, minor)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run --package lc-inbound layout::tests`
Expected: PASS, 8 tests.

If `the_panels_tile_the_area_exactly` fails at `(u16::MAX, u16::MAX)`, the cause is `Rect::new` clamping via `x.saturating_add(width) - x`. It should not trigger — the splits sum to exactly `total` — but if it does, the bug is in `split`, not in the test.

- [ ] **Step 5: Verify the gate is clean**

Run: `mise run ci`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-inbound/src/layout.rs crates/lc-inbound/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(inbound): compute the four-panel geometry

25/75 horizontally, 60/40 and 90/10 vertically, each panel floored at
three rows so it never renders as borders around nothing.

Splits are u16 arithmetic rather than mixed Percentage/Min constraints:
behaviour at small sizes should be a property of this code, not of
ratatui's constraint solver (conventions.md §4).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Draw the panels

**Files:**
- Create: `crates/lc-inbound/src/render.rs`
- Modify: `crates/lc-inbound/src/lib.rs` (module declaration and re-export)
- Test: inline in `crates/lc-inbound/src/render.rs`

**Interfaces:**
- Consumes: `view_model`/`ViewModel` (Task 2), `Token::Focused` (Task 3), `layout`/`PanelRects` (Task 4).
- Produces: `lc_inbound::render(frame: &mut Frame, view: &ViewModel, theme: Theme)`.

`ratatui::backend::TestBackend` is a plain re-export from `ratatui-core` and needs no feature flag — the spec left this open; it is now checked. Note that `TestBackend::assert_buffer_lines` compares style as well as symbol, so a bold focused border would not match plain `&str` expectations; these tests read symbols and modifiers separately instead.

- [ ] **Step 1: Write the failing test**

Create `crates/lc-inbound/src/render.rs` containing only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use lc_domain::State;
    use lc_domain::view::view_model;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::Modifier;

    use super::*;

    fn draw(width: u16, height: u16) -> Buffer {
        let state = State::default();
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| render(frame, &view_model(&state), Theme::TerminalDefault))
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .filter_map(|x| buffer.cell((x, y)))
            .map(Cell::symbol)
            .collect()
    }

    #[test]
    fn the_four_panels_are_drawn_where_the_draft_puts_them() {
        // At 40x12 the sidebar sits on its 16-column floor, Tables takes 8
        // rows to Ddl's 4, and Queries sits on its 3-row floor.
        let buffer = draw(40, 12);

        assert_eq!(row(&buffer, 0), "┌Tables────────┐┌Table contents────────┐");
        assert_eq!(row(&buffer, 1), "│              ││                      │");
        assert_eq!(row(&buffer, 7), "└──────────────┘│                      │");
        assert_eq!(row(&buffer, 8), "┌Table DDL─────┐└──────────────────────┘");
        assert_eq!(row(&buffer, 9), "│              │┌Queries───────────────┐");
        assert_eq!(row(&buffer, 11), "└──────────────┘└──────────────────────┘");
    }

    #[test]
    fn only_the_focused_panel_is_bold() {
        let buffer = draw(40, 12);
        let bold = |x, y| {
            buffer
                .cell((x, y))
                .expect("corner is in bounds")
                .modifier
                .contains(Modifier::BOLD)
        };

        // Top-left corner of each panel; Tables is focused by default.
        assert!(bold(0, 0), "Tables is focused");
        assert!(!bold(16, 0), "Contents is not");
        assert!(!bold(0, 8), "Ddl is not");
        assert!(!bold(16, 9), "Queries is not");
    }

    #[test]
    fn a_collapsed_panel_is_not_drawn_at_all() {
        // Five rows leaves no room for a second panel in either column.
        let buffer = draw(40, 5);
        let screen: String = (0..5).map(|y| row(&buffer, y)).collect();

        assert!(screen.contains("Tables"));
        assert!(screen.contains("Table contents"));
        assert!(!screen.contains("Table DDL"), "Ddl collapsed to zero rows");
        assert!(!screen.contains("Queries"), "Queries collapsed to zero rows");
    }
}
```

Add to `crates/lc-inbound/src/lib.rs` (module list stays alphabetical, so `render` goes between `layout` and `theme`):

```rust
pub mod render;
```

and extend the re-export block:

```rust
pub use render::render;
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run --package lc-inbound render::tests`
Expected: FAIL — compile error, `cannot find function 'render' in this scope`.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/lc-inbound/src/render.rs`, above the test module:

```rust
//! Drawing a frame.

use lc_domain::style::Token;
use lc_domain::view::ViewModel;
use ratatui::Frame;
use ratatui::widgets::Block;

use crate::layout::layout;
use crate::theme::Theme;

/// Draw one titled block per panel, skipping any the terminal is too small to
/// hold.
pub fn render(frame: &mut Frame, view: &ViewModel, theme: Theme) {
    let rects = layout(frame.area());

    for panel in &view.panels {
        let area = rects.get(panel.panel);
        if area.is_empty() {
            continue;
        }

        let mut block = Block::bordered().title(panel.title.as_str());
        if panel.panel == view.focus {
            block = block.border_style(theme.style(Token::Focused));
        }

        frame.render_widget(block, area);
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo nextest run --workspace`
Expected: PASS.

If `the_four_panels_are_drawn_where_the_draft_puts_them` fails on the border glyphs, compare against the actual buffer in the panic output rather than adjusting the expectation blindly — the row widths are 16 and 24 and must sum to 40.

- [ ] **Step 5: Verify the gate is clean**

Run: `mise run ci`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-inbound/src/render.rs crates/lc-inbound/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(inbound): render the four titled panels

One bordered block per panel, the focused one bold. Panels the terminal
cannot hold are skipped rather than drawn as empty borders.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Wire the renderer into the binary

Replaces the `press q to quit` placeholder. Pure wiring, which is all `lc-app` is for (architecture.md §2) — the behaviour it exposes is already covered by Tasks 1–5, so verification is the gate plus running the binary.

**Files:**
- Modify: `crates/lc-app/src/main.rs:9-15` (imports), `:28-34` (setup and first draw), `:56-59` (redraw), `:65-70` (delete the placeholder `render`)

**Interfaces:**
- Consumes: `lc_domain::view::view_model` (Task 2), `lc_inbound::render` (Task 5), `lc_inbound::Theme`.
- Produces: nothing other tasks depend on.

- [ ] **Step 1: Replace the imports**

In `crates/lc-app/src/main.rs`, replace lines 9–15 with:

```rust
use anyhow::Result;
use lc_domain::model::{Key, KeyPress};
use lc_domain::view::view_model;
use lc_domain::{Msg, State, update};
use lc_inbound::Theme;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc;
```

`ratatui::style::Stylize` and `ratatui::text::Line` are no longer used and must go, or the build warns.

- [ ] **Step 2: Draw through lc-inbound**

In `run`, after `let mut state = State::default();` add:

```rust
    let theme = Theme::default();
```

and replace both `terminal.draw(...)` calls — the one before the loop and the one inside the `if state.dirty` block — with:

```rust
        terminal.draw(|frame| lc_inbound::render(frame, &view_model(&state), theme))?;
```

(no leading indent adjustment for the first one; it sits at one level of indentation, the second at two).

- [ ] **Step 3: Delete the placeholder renderer**

Remove the entire `fn render(frame: &mut ratatui::Frame, _state: &State)` at the bottom of the file, lines 65–70.

- [ ] **Step 4: Verify it builds and the gate is clean**

Run: `mise run ci`
Expected: clean — in particular no `unused import` warning, which is the likely failure if Step 1 was applied partially.

- [ ] **Step 5: Verify it draws**

Run: `mise run run`
Expected: four bordered panels titled `Tables`, `Table DDL`, `Table contents`, `Queries`, laid out as in `doc/ui-draft.png`, with the `Tables` border bold. Resize the terminal narrow and short to watch the floors engage and the minor panels drop out. Press `q` to quit and confirm the terminal is restored.

- [ ] **Step 6: Commit**

```bash
git add crates/lc-app/src/main.rs
git commit -m "$(cat <<'EOF'
feat(app): draw the panel layout instead of the placeholder

Closes the Phase 0 TUI shell item in vision.md §9: the binary now renders
doc/ui-draft.png. The temporary q-to-quit binding stays until the focus
model lands.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

## What this plan does not do

Per spec §6: focus movement and its keybindings, panel content of any kind, scrolling, resize handling beyond what `frame.area()` reports, restoring a `Log` panel, themes beyond `TerminalDefault`.

One documentation follow-up is deliberately left out of the task list because it belongs with the focus work, not here: `doc/architecture.md` §4 sketches `state/` as holding "panel focus", which stays true, but §11's `ViewModel` granularity entry is narrowed by Task 2 — one tree per frame is now the status quo rather than a hypothesis. Update §11 when there is measurement to report.
