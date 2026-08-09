# Panel scrolling — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One vertical vocabulary in every panel — `j`/`k`, `PageUp`/`PageDown`, `Home`/`End` — and a Table DDL panel that can reach its own content below the fold.

**Architecture:** Tables and Contents keep scrolling as a consequence of their
selection moving, and gain paging motions. Table DDL gets a real viewport
offset, counted in wrapped lines so a page motion cannot skip unread text.
The domain cannot count those lines — wrapping needs a width — so
`lc_inbound::render` returns a `Geometry` and `lc-app` feeds it back as
`Msg::Measured`, sending only when it changed so the loop terminates.

**Tech Stack:** Rust, `ratatui` 0.30.2, no new dependencies.

**Spec:** [doc/specs/2026-08-09-panel-scrolling-design.md](../specs/2026-08-09-panel-scrolling-design.md)

## Global Constraints

- `AGENTS.md` non-negotiables apply to every task: no `unsafe`, no narrating
  comments, no warnings, no ignored tests.
- `lc-domain` gains no dependency. Tasks 1–3 touch it; none needs one.
- `mise run ci` must be clean before a task is considered done. Individual
  steps use `cargo test -p <crate>` for speed; the final step of each task
  runs the gate.
- Conventional Commits, imperative, lowercase, no trailing period.
- Test names are sentences about behaviour, matching the surrounding files
  (`a_cursor_starts_on_the_first_row`), not `test_foo`.
- `unwrap_used` and `expect_used` are `warn` outside tests. Use
  `unwrap_or`/`map_or` in library code.
- Tasks land in order. Task 3 changes a `PanelBody` variant and Task 5
  changes `render`'s return type; each fixes its own callers in the same
  task, so the workspace compiles at every commit.

---

### Task 1: A viewport that stops at the end

**Files:**
- Modify: `crates/lc-domain/src/state.rs` (`State` at 15-30, `Cursor` at 158-198, tests at 256+)

**Interfaces:**
- Produces:
  - `lc_domain::state::Scroll` — `offset(self) -> usize`, `down(&mut self, len: usize, height: usize)`, `up(&mut self)`, `page_down(&mut self, len: usize, height: usize)`, `page_up(&mut self, height: usize)`, `to_start(&mut self)`, `to_end(&mut self, len: usize, height: usize)`, `clamp_to(&mut self, len: usize, height: usize)`.
  - `lc_domain::state::Geometry` — `new(heights: [usize; 4], ddl_lines: usize) -> Self`, `height(self, panel: Panel) -> usize`, `ddl_lines(self) -> usize`.
  - `Cursor::page_down(&mut self, len: usize, height: usize)`, `Cursor::page_up(&mut self, height: usize)`, `Cursor::to_start(&mut self)`, `Cursor::to_end(&mut self, len: usize)`.
  - `State::ddl: Scroll` and `State::geometry: Geometry`.
- Tasks 2, 3 and 5 all consume these.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/lc-domain/src/state.rs`, after the existing
`Cursor` tests:

```rust
    #[test]
    fn a_scroll_starts_at_the_top() {
        assert_eq!(Scroll::default().offset(), 0);
    }

    #[test]
    fn scrolling_down_stops_with_the_last_line_at_the_bottom() {
        let mut scroll = Scroll::default();

        for _ in 0..50 {
            scroll.down(20, 6);
        }

        assert_eq!(
            scroll.offset(),
            14,
            "the last line belongs at the bottom of the panel, not dragged to the top"
        );
    }

    #[test]
    fn scrolling_up_stops_at_the_top() {
        let mut scroll = Scroll::default();
        scroll.down(20, 6);

        scroll.up();
        scroll.up();

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn a_document_that_fits_never_scrolls() {
        let mut scroll = Scroll::default();

        scroll.down(6, 20);
        scroll.page_down(6, 20);
        scroll.to_end(6, 20);

        assert_eq!(scroll.offset(), 0, "there is nothing below the fold");
    }

    #[test]
    fn a_page_moves_exactly_one_panel_height() {
        let mut scroll = Scroll::default();

        scroll.page_down(100, 12);

        assert_eq!(scroll.offset(), 12);

        scroll.page_up(12);

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn the_last_page_is_short_and_lands_on_the_end() {
        let mut scroll = Scroll::default();
        scroll.page_down(20, 6);
        scroll.page_down(20, 6);
        assert_eq!(scroll.offset(), 12);

        scroll.page_down(20, 6);

        assert_eq!(scroll.offset(), 14);
    }

    #[test]
    fn a_scroll_reaches_both_ends_in_one_key() {
        let mut scroll = Scroll::default();

        scroll.to_end(20, 6);
        assert_eq!(scroll.offset(), 14);

        scroll.to_start();
        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn clamping_pulls_a_stale_offset_over_a_document_that_shrank() {
        let mut scroll = Scroll::default();
        scroll.to_end(60, 6);
        assert_eq!(scroll.offset(), 54);

        scroll.clamp_to(10, 6);

        assert_eq!(scroll.offset(), 4);
    }

    #[test]
    fn clamping_pulls_a_stale_offset_back_when_the_panel_grows() {
        let mut scroll = Scroll::default();
        scroll.to_end(20, 6);

        scroll.clamp_to(20, 30);

        assert_eq!(
            scroll.offset(),
            0,
            "a panel taller than its document shows all of it"
        );
    }

    #[test]
    fn paging_the_cursor_moves_a_screen_and_stops_on_the_last_row() {
        let mut cursor = Cursor::default();

        cursor.page_down(20, 6);
        assert_eq!(cursor.index(), 6);

        for _ in 0..5 {
            cursor.page_down(20, 6);
        }
        assert_eq!(cursor.index(), 19, "the cursor stops on the last row");

        cursor.page_up(6);
        assert_eq!(cursor.index(), 13);
    }

    #[test]
    fn the_cursor_reaches_both_ends_in_one_key() {
        let mut cursor = Cursor::default();

        cursor.to_end(20);
        assert_eq!(cursor.index(), 19);

        cursor.to_start();
        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn an_empty_list_has_no_end_to_jump_to() {
        let mut cursor = Cursor::default();

        cursor.to_end(0);

        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn geometry_reports_each_panels_own_height() {
        let geometry = Geometry::new([8, 14, 20, 1], 37);

        assert_eq!(geometry.height(Panel::Tables), 8);
        assert_eq!(geometry.height(Panel::Ddl), 14);
        assert_eq!(geometry.height(Panel::Contents), 20);
        assert_eq!(geometry.height(Panel::Queries), 1);
        assert_eq!(geometry.ddl_lines(), 37);
    }

    #[test]
    fn a_state_that_has_never_been_drawn_reports_no_room() {
        // The first frame is drawn before any measurement exists, so every
        // motion against this must be inert rather than wrong.
        let geometry = Geometry::default();

        for panel in Panel::ALL {
            assert_eq!(geometry.height(panel), 0);
        }
        assert_eq!(geometry.ddl_lines(), 0);
    }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p lc-domain state::tests`
Expected: FAIL — `cannot find type Scroll in this scope`.

- [ ] **Step 3: Add `Scroll` and `Geometry`**

In `crates/lc-domain/src/state.rs`, after the `impl Cursor` block:

```rust
/// A viewport offset over a document of known length.
///
/// The offset is private for the reason [`Cursor`]'s index is: every motion
/// re-establishes `offset <= len - height` here, so a renderer is never
/// handed one that points past the end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scroll {
    offset: usize,
}

impl Scroll {
    /// The first line drawn.
    #[must_use]
    pub const fn offset(self) -> usize {
        self.offset
    }

    /// Moves down a line, stopping with the last line at the bottom of the
    /// panel rather than dragging it to the top.
    pub const fn down(&mut self, len: usize, height: usize) {
        self.go_to(self.offset.saturating_add(1), len, height);
    }

    /// Moves up a line, stopping at the top.
    pub const fn up(&mut self) {
        self.offset = self.offset.saturating_sub(1);
    }

    pub const fn page_down(&mut self, len: usize, height: usize) {
        self.go_to(self.offset.saturating_add(height), len, height);
    }

    pub const fn page_up(&mut self, height: usize) {
        self.offset = self.offset.saturating_sub(height);
    }

    pub const fn to_start(&mut self) {
        self.offset = 0;
    }

    pub const fn to_end(&mut self, len: usize, height: usize) {
        self.offset = len.saturating_sub(height);
    }

    /// Pulls a stale offset back over a document that shrank or a panel that
    /// grew.
    pub const fn clamp_to(&mut self, len: usize, height: usize) {
        self.go_to(self.offset, len, height);
    }

    const fn go_to(&mut self, offset: usize, len: usize, height: usize) {
        let last = len.saturating_sub(height);
        // `Ord::min` is not const.
        self.offset = if offset > last { last } else { offset };
    }
}

/// What the last frame turned out to be.
///
/// The renderer's one report back into the domain
/// (doc/specs/2026-08-09-panel-scrolling-design.md §3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Geometry {
    heights: [usize; 4],
    ddl_lines: usize,
}

impl Geometry {
    /// `heights` in [`Panel::ALL`] order, each counted in the unit that panel
    /// scrolls in — tree rows, grid rows, or wrapped lines — and net of what
    /// the panel spends on chrome, such as the grid's header row.
    #[must_use]
    pub const fn new(heights: [usize; 4], ddl_lines: usize) -> Self {
        Self { heights, ddl_lines }
    }

    /// Rows of content `panel` can show.
    #[must_use]
    pub const fn height(self, panel: Panel) -> usize {
        self.heights[match panel {
            Panel::Tables => 0,
            Panel::Ddl => 1,
            Panel::Contents => 2,
            Panel::Queries => 3,
        }]
    }

    /// Lines the Ddl body wrapped to.
    ///
    /// The one length the domain cannot count for itself: wrapping needs a
    /// width, and width-dependent layout is the renderer's business. Tree and
    /// grid rows it counts, and must keep counting — two sources of truth for
    /// a row count is the drift `tree.rs`'s single flattening prevents.
    #[must_use]
    pub const fn ddl_lines(self) -> usize {
        self.ddl_lines
    }
}
```

- [ ] **Step 4: Add the `Cursor` motions**

In `crates/lc-domain/src/state.rs`, inside `impl Cursor`, after `go_to`:

```rust
    /// Moves a screen down, stopping on the last row.
    pub const fn page_down(&mut self, len: usize, height: usize) {
        self.go_to(self.index.saturating_add(height), len);
    }

    /// Moves a screen up, stopping on the first row.
    pub const fn page_up(&mut self, height: usize) {
        self.index = self.index.saturating_sub(height);
    }

    pub const fn to_start(&mut self) {
        self.index = 0;
    }

    pub const fn to_end(&mut self, len: usize) {
        self.go_to(len.saturating_sub(1), len);
    }
```

- [ ] **Step 5: Give `State` the two fields**

In `crates/lc-domain/src/state.rs`, inside `pub struct State`, after
`pub contents: ContentsPanel,`:

```rust
    /// The Table DDL panel scrolls a viewport rather than a selection: it has
    /// no row to act on, so there is nothing for a cursor to mean.
    pub ddl: Scroll,
    /// What the last frame turned out to be, so a motion knows how far a page
    /// is and where the end is.
    pub geometry: Geometry,
```

- [ ] **Step 6: Run the tests and watch them pass**

Run: `cargo test -p lc-domain state::tests`
Expected: PASS.

- [ ] **Step 7: Gate and commit**

```bash
mise run ci
git add crates/lc-domain/src/state.rs
git commit -m "feat(domain): give a panel a viewport that stops at its end"
```

---

### Task 2: One motion vocabulary, and the measurement that bounds it

**Files:**
- Modify: `crates/lc-domain/src/update.rs` (imports at 7-12, `Msg` at 22-54, `update` at 94-191, `Motion`/`move_selection`/`focused_row_count` at 200-276, `handle_key` at 206-238, tests at 385+)

**Interfaces:**
- Consumes: `Scroll`, `Geometry`, `Cursor::page_down`/`page_up`/`to_start`/`to_end`, `State::ddl`, `State::geometry` from Task 1.
- Produces: `Msg::Measured(Geometry)`. Task 5 sends it.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/lc-domain/src/update.rs`. Add `Geometry` to the
`use crate::state::{...}` line at the top of the file first — the tests share
the module's imports. `browsing`, `press`, `read_shop_orders` and `page_of`
are the existing helpers in that module.

```rust
    fn measured(state: &mut State, heights: [usize; 4], ddl_lines: usize) {
        update(state, Msg::Measured(Geometry::new(heights, ddl_lines)));
    }

    #[test]
    fn page_down_moves_the_tree_by_one_panel_height() {
        let mut state = browsing(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        measured(&mut state, [3, 0, 0, 0], 0);

        press(&mut state, Key::PageDown);

        assert_eq!(state.tables.cursor.index(), 3);

        press(&mut state, Key::PageUp);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn home_and_end_reach_the_ends_of_the_tree_in_one_key() {
        let mut state = browsing(&["a", "b", "c", "d"]);
        measured(&mut state, [3, 0, 0, 0], 0);

        press(&mut state, Key::End);

        assert_eq!(state.tables.cursor.index(), 3);

        press(&mut state, Key::Home);

        assert_eq!(state.tables.cursor.index(), 0);
    }

    #[test]
    fn page_down_moves_the_grid_by_one_panel_height() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let query = state.contents.outstanding().expect("the panel is waiting");
        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(30)),
            },
        );
        state.focus = Panel::Contents;
        measured(&mut state, [0, 0, 8, 0], 0);

        press(&mut state, Key::PageDown);

        assert_eq!(state.contents.cursor.index(), 8);
    }

    #[test]
    fn the_ddl_panel_scrolls_a_viewport_rather_than_a_selection() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;
        measured(&mut state, [0, 6, 0, 0], 20);
        let tree_row = state.tables.cursor.index();

        press(&mut state, Key::Char('j'));
        press(&mut state, Key::Char('j'));

        assert_eq!(state.ddl.offset(), 2);
        assert_eq!(
            state.tables.cursor.index(),
            tree_row,
            "the tree must not move while another panel has focus"
        );

        press(&mut state, Key::End);

        assert_eq!(
            state.ddl.offset(),
            14,
            "End leaves the last line at the bottom of the panel"
        );
    }

    #[test]
    fn a_motion_in_the_tree_leaves_the_ddl_viewport_alone() {
        let mut state = browsing(&["a", "b", "c"]);
        measured(&mut state, [3, 6, 0, 0], 20);

        press(&mut state, Key::PageDown);
        press(&mut state, Key::End);

        assert_eq!(state.ddl.offset(), 0);
    }

    #[test]
    fn a_motion_before_the_first_measurement_moves_nothing() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;

        press(&mut state, Key::PageDown);
        press(&mut state, Key::End);

        assert_eq!(
            state.ddl.offset(),
            0,
            "a panel of unknown height has no page and no end"
        );
    }

    #[test]
    fn the_new_motions_emit_no_effects() {
        for key in [Key::PageDown, Key::PageUp, Key::Home, Key::End] {
            let mut state = browsing(&["a", "b", "c"]);
            measured(&mut state, [3, 6, 6, 1], 20);

            let effects = press(&mut state, key);

            assert!(effects.is_empty(), "{key:?} issued {effects:?}");
        }
    }

    /// The frame this message describes is the frame that produced it.
    /// Marking dirty unconditionally would redraw, re-measure and redraw
    /// forever (design §3).
    #[test]
    fn a_measurement_that_changes_nothing_leaves_the_state_clean() {
        let mut state = browsing(&["a", "b", "c"]);
        let geometry = Geometry::new([3, 6, 6, 1], 20);
        update(&mut state, Msg::Measured(geometry));
        state.dirty = false;

        let effects = update(&mut state, Msg::Measured(geometry));

        assert!(effects.is_empty());
        assert!(
            !state.dirty,
            "a measurement of the frame already drawn must not ask for another"
        );
    }

    #[test]
    fn a_measurement_pulls_a_stale_offset_back_inside_the_panel() {
        let mut state = browsing(&["a", "b", "c"]);
        state.focus = Panel::Ddl;
        measured(&mut state, [0, 6, 0, 0], 40);
        press(&mut state, Key::End);
        assert_eq!(state.ddl.offset(), 34);
        state.dirty = false;

        // The tree cursor moved to a table with far fewer options.
        measured(&mut state, [0, 6, 0, 0], 8);

        assert_eq!(state.ddl.offset(), 2);
        assert!(state.dirty, "the offset moved, so the frame must be repainted");
    }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p lc-domain update::tests`
Expected: FAIL — `no variant named Measured found for enum Msg`.

- [ ] **Step 3: Add the message and its handler**

In `crates/lc-domain/src/update.rs`, add `Geometry` to the `use
crate::state::{...}` import, then add to `enum Msg`, after `Resized`:

```rust
    /// What the frame just drawn turned out to be. The renderer's one report
    /// back into the domain (design §3).
    Measured(Geometry),
```

and to `update`'s match, after the `Msg::Resized` arm:

```rust
        // No unconditional `mark_dirty`: this message describes the frame
        // that produced it, so redrawing on every one of them would
        // re-measure and redraw forever (design §3).
        Msg::Measured(geometry) => {
            state.geometry = geometry;

            let before = state.ddl;
            state
                .ddl
                .clamp_to(geometry.ddl_lines(), geometry.height(Panel::Ddl));

            if state.ddl != before {
                state.mark_dirty();
            }
            Vec::new()
        }
```

- [ ] **Step 4: Widen `Motion` and route it**

In `crates/lc-domain/src/update.rs`, replace the `Motion` enum and
`move_selection` with:

```rust
#[derive(Clone, Copy)]
enum Motion {
    Up,
    Down,
    PageUp,
    PageDown,
    Start,
    End,
}

/// Applies `motion` to whatever the focused panel moves: a selection in
/// Tables and Contents, a viewport in Ddl.
fn scroll(state: &mut State, motion: Motion) {
    let height = state.geometry.height(state.focus);

    if state.focus == Panel::Ddl {
        let len = state.geometry.ddl_lines();

        match motion {
            Motion::Down => state.ddl.down(len, height),
            Motion::Up => state.ddl.up(),
            Motion::PageDown => state.ddl.page_down(len, height),
            Motion::PageUp => state.ddl.page_up(height),
            Motion::Start => state.ddl.to_start(),
            Motion::End => state.ddl.to_end(len, height),
        }
        return;
    }

    let len = focused_row_count(state);

    let Some(cursor) = focused_cursor(state) else {
        return;
    };

    match motion {
        Motion::Down => cursor.down(len),
        Motion::Up => cursor.up(),
        Motion::PageDown => cursor.page_down(len, height),
        Motion::PageUp => cursor.page_up(height),
        Motion::Start => cursor.to_start(),
        Motion::End => cursor.to_end(len),
    }
}
```

and replace the doc comment on `focused_row_count` (its body is unchanged):

```rust
/// How many rows the focused panel's *cursor* moves over. Ddl scrolls a
/// viewport rather than a selection and never reaches here; Queries has
/// nothing, so a motion there is inert rather than moving another panel's
/// selection from a distance.
```

- [ ] **Step 5: Bind the four keys**

In `crates/lc-domain/src/update.rs`, in `handle_key`'s `match press.key`,
replace the two motion arms with six:

```rust
        Key::Char('j') | Key::Down => scroll(state, Motion::Down),
        Key::Char('k') | Key::Up => scroll(state, Motion::Up),
        Key::PageDown => scroll(state, Motion::PageDown),
        Key::PageUp => scroll(state, Motion::PageUp),
        Key::Home => scroll(state, Motion::Start),
        Key::End => scroll(state, Motion::End),
```

- [ ] **Step 6: Run the crate's tests**

Run: `cargo test -p lc-domain`
Expected: PASS, including the existing `motion_belongs_to_the_focused_panel_only`
and `a_panel_with_no_rows_swallows_motion_rather_than_passing_it_on` — the Ddl
branch returns before touching any cursor.

- [ ] **Step 7: Gate and commit**

```bash
mise run ci
git add crates/lc-domain/src/update.rs
git commit -m "feat(domain): scroll the focused panel with one vocabulary"
```

---

### Task 3: The DDL panel draws from its offset

**Files:**
- Modify: `crates/lc-domain/src/view/mod.rs` (`PanelBody::Details` at 68)
- Modify: `crates/lc-domain/src/view/details.rs` (`body` at 52-62, test helper `details` at 177-182, tests at 252+)
- Modify: `crates/lc-inbound/src/body.rs` (`render` at 39-55)
- Modify: `crates/lc-inbound/src/details.rs` (`render` at 32-41)
- Modify: `crates/lc-inbound/src/render.rs` (tests at 45+)

**Interfaces:**
- Consumes: `State::ddl` from Task 1.
- Produces: `PanelBody::Details { details: Details, offset: usize }`; `lc_inbound::details::lines(details: &Details, width: usize, theme: Theme) -> Vec<Line<'static>>` (crate-visible). Task 4 consumes both.

- [ ] **Step 1: Write the failing domain test**

Add to `mod tests` in `crates/lc-domain/src/view/details.rs`:

```rust
    #[test]
    fn the_projection_carries_the_panels_own_offset() {
        let mut state = browsing(orders());
        state.ddl.to_end(9, 3);

        let PanelBody::Details { offset, .. } = ddl_body(&state) else {
            panic!("a selected table has something to describe");
        };

        assert_eq!(offset, 6);
    }
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p lc-domain view::details`
Expected: FAIL — `expected tuple struct or tuple variant, found variant PanelBody::Details`.

- [ ] **Step 3: Change the variant**

In `crates/lc-domain/src/view/mod.rs`, replace the `Details(Details)` variant:

```rust
    /// A heading and grouped rows: what is selected, described.
    ///
    /// `offset` is the first line drawn, counted in wrapped lines. Only the
    /// renderer can produce those; the domain scrolls by them on the strength
    /// of the count `Msg::Measured` brings back (design §2).
    Details { details: Details, offset: usize },
```

- [ ] **Step 4: Project the offset**

In `crates/lc-domain/src/view/details.rs`, replace the two `Some(..)` arms of
`body` and add a helper beneath it:

```rust
    match state.tables.selected(schema) {
        Some(Node::Table { keyspace, table }) => scrolled(state, of_table(keyspace, table)),
        Some(Node::Keyspace { keyspace, .. }) => scrolled(state, of_keyspace(keyspace)),
        None => PanelBody::Blank,
    }
}

fn scrolled(state: &State, details: Details) -> PanelBody {
    PanelBody::Details {
        details,
        offset: state.ddl.offset(),
    }
}
```

- [ ] **Step 5: Fix the test helper that destructures it**

In `crates/lc-domain/src/view/details.rs`, `fn details`:

```rust
    fn details(state: &State) -> Details {
        let PanelBody::Details { details, .. } = ddl_body(state) else {
            panic!("a selected row has something to describe");
        };
        details
    }
```

- [ ] **Step 6: Run the domain tests**

Run: `cargo test -p lc-domain`
Expected: PASS. `lc-inbound` does not compile yet; the next step fixes it.

- [ ] **Step 7: Scroll the paragraph**

In `crates/lc-inbound/src/details.rs`, replace `render` with:

```rust
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    details: &Details,
    offset: usize,
    theme: Theme,
) {
    let lines = lines(details, usize::from(area.width), theme);
    let offset = u16::try_from(offset).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), area);
}

/// Every line the body occupies at `width`.
///
/// Also what the frame reports back as `Geometry::ddl_lines`, so the domain
/// can bound an offset over text it cannot wrap itself (design §2).
pub(crate) fn lines(details: &Details, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::raw(truncate(&details.heading, width)), Line::raw("")];
    for section in &details.sections {
        lines.extend(section_lines(section, width, theme));
    }

    lines
}
```

In `crates/lc-inbound/src/body.rs`, replace the `Details` arm of `render`:

```rust
        PanelBody::Details { details, offset } => {
            crate::details::render(frame, area, details, *offset, theme);
        }
```

- [ ] **Step 8: Write the failing render test**

Add to `mod tests` in `crates/lc-inbound/src/render.rs`. It needs
`use lc_domain::view::PanelBody;` alongside the existing imports:

```rust
    /// The `Details` the Ddl panel would draw for `state`.
    fn ddl_details(state: &State) -> lc_domain::view::Details {
        let PanelBody::Details { details, .. } = view_model(state).panels[1].body.clone() else {
            panic!("a selected row has something to describe");
        };
        details
    }

    #[test]
    fn the_ddl_panel_scrolls_to_the_lines_below_its_fold() {
        // At 40x20 the sidebar is 16 columns and the Ddl panel is 8 rows, so
        // its body is fourteen columns of six rows — and `orders()` wraps to
        // more than six lines in that.
        const WIDTH: usize = 14;
        const HEIGHT: usize = 6;

        let state = describing(orders());
        let all = crate::details::lines(&ddl_details(&state), WIDTH, Theme::TerminalDefault);
        assert!(
            all.len() > HEIGHT,
            "the fixture must overflow the panel or this test proves nothing"
        );
        let tail = all.last().expect("the body has lines").to_string();
        assert!(
            !tail.trim().is_empty(),
            "the assertions below need a tail to look for"
        );

        let mut scrolled = describing(orders());
        scrolled.ddl.to_end(all.len(), HEIGHT);

        let before = draw_state(40, 20, &state);
        let after = draw_state(40, 20, &scrolled);
        // Inside the Ddl panel only: rows 13..18, columns 1..14. A full-width
        // row would also carry whatever the Contents panel is saying.
        let visible = |buffer: &Buffer| -> Vec<String> {
            (13..19)
                .map(|y| {
                    (1..15)
                        .filter_map(|x| buffer.cell((x, y)))
                        .map(Cell::symbol)
                        .collect::<String>()
                })
                .collect()
        };

        assert!(
            !visible(&before).iter().any(|line| line.contains(tail.trim())),
            "the tail starts below the fold"
        );
        assert!(
            visible(&after).iter().any(|line| line.contains(tail.trim())),
            "scrolling to the end must bring it into view"
        );
    }
```

- [ ] **Step 9: Run it and watch it pass**

Run: `cargo test -p lc-inbound`
Expected: PASS. The existing `a_selected_tables_shape_is_drawn_above_its_options`
and its neighbours are unaffected — they draw at 100x40 with the offset at
zero, which is what `Paragraph::scroll((0, 0))` already does.

- [ ] **Step 10: Gate and commit**

```bash
mise run ci
git add crates/lc-domain/src/view crates/lc-inbound/src/details.rs crates/lc-inbound/src/body.rs crates/lc-inbound/src/render.rs
git commit -m "feat: draw the table DDL panel from its own scroll offset"
```

---

### Task 4: A bar on the border when something is hidden

**Files:**
- Modify: `crates/lc-inbound/src/body.rs` (`render` at 39-55, `tree` at 65-72, `grid` at 78-117)
- Modify: `crates/lc-inbound/src/details.rs` (`render` from Task 3)
- Modify: `crates/lc-inbound/src/render.rs` (`render` at 13-43, tests at 45+)

**Interfaces:**
- Consumes: `lc_inbound::details::lines` from Task 3.
- Produces: `lc_inbound::body::Extent { position, total, visible }` with `overflows(&self) -> bool`; `body::render` now returns `Option<Extent>` and `details::render` returns `Extent`. Task 5 consumes both.

- [ ] **Step 1: Write the failing render tests**

Add to `mod tests` in `crates/lc-inbound/src/render.rs`:

```rust
    /// The Tables panel's right border at 40x12: column 15, between the
    /// corners at rows 0 and 7.
    fn tables_border(buffer: &Buffer) -> String {
        (1..7)
            .filter_map(|y| buffer.cell((15, y)))
            .map(Cell::symbol)
            .collect()
    }

    #[test]
    fn a_panel_whose_content_overflows_grows_a_bar_on_its_border() {
        let names: Vec<String> = (0..12).map(|i| format!("ks{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();

        let buffer = draw_state(40, 12, &browsing(&refs, 0));

        assert!(
            tables_border(&buffer).contains('█'),
            "the thumb says how far through twelve keyspaces six rows are"
        );
    }

    #[test]
    fn a_panel_whose_content_fits_keeps_its_border_intact() {
        let buffer = draw_state(40, 12, &browsing(&["shop", "typezoo"], 0));

        assert_eq!(
            tables_border(&buffer),
            "││││││",
            "nothing is hidden, so nothing says otherwise"
        );
    }
```

- [ ] **Step 2: Run them and watch the first fail**

Run: `cargo test -p lc-inbound render::tests::a_panel_whose_content`
Expected: `a_panel_whose_content_overflows_grows_a_bar_on_its_border` FAILs —
the border is six `│`. The `fits` test passes already.

- [ ] **Step 3: Report the extent out of each body**

In `crates/lc-inbound/src/body.rs`, add beneath the constants:

```rust
/// Rows of a grid spent on its header rather than on a row.
const HEADER_ROWS: usize = 1;

/// What a scrollbar needs, in whatever unit the panel scrolls in.
pub(crate) struct Extent {
    pub(crate) position: usize,
    pub(crate) total: usize,
    pub(crate) visible: usize,
}

impl Extent {
    pub(crate) const fn overflows(&self) -> bool {
        self.total > self.visible
    }
}
```

Change `render`'s signature and every arm:

```rust
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    body: &PanelBody,
    theme: Theme,
) -> Option<Extent> {
    if area.is_empty() {
        return None;
    }

    match body {
        PanelBody::Blank => None,
        PanelBody::Notice { text, token } => {
            notice(frame, area, text, *token, theme);
            None
        }
        PanelBody::Tree { rows, selected } => Some(tree(frame, area, rows, *selected, theme)),
        PanelBody::Details { details, offset } => {
            Some(crate::details::render(frame, area, details, *offset, theme))
        }
        PanelBody::Grid {
            columns,
            rows,
            selected,
        } => Some(grid(frame, area, columns, rows, *selected, theme)),
    }
}
```

Give `tree` a return value — its body is otherwise unchanged:

```rust
fn tree(frame: &mut Frame, area: Rect, rows: &[TreeRow], selected: usize, theme: Theme) -> Extent {
    let items: Vec<ListItem> = rows.iter().map(|row| item(row, theme)).collect();

    let list = List::new(items).highlight_style(theme.style(Token::Selected));
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(list, area, &mut state);

    // Read back rather than recomputed: the widget owns the arithmetic that
    // keeps the selection on screen, and a second copy of it here would be a
    // second thing to keep right.
    Extent {
        position: state.offset(),
        total: rows.len(),
        visible: usize::from(area.height),
    }
}
```

and `grid` the same, with the header subtracted — replace only its last two
statements:

```rust
    frame.render_stateful_widget(table, area, &mut state);

    Extent {
        position: state.offset(),
        total: rows.len(),
        visible: usize::from(area.height).saturating_sub(HEADER_ROWS),
    }
}
```

In `crates/lc-inbound/src/details.rs`, give `render` a return value:

```rust
pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    details: &Details,
    offset: usize,
    theme: Theme,
) -> crate::body::Extent {
    let lines = lines(details, usize::from(area.width), theme);
    let total = lines.len();
    let offset = u16::try_from(offset).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), area);

    crate::body::Extent {
        position: usize::from(offset),
        total,
        visible: usize::from(area.height),
    }
}
```

- [ ] **Step 4: Draw the bar**

In `crates/lc-inbound/src/render.rs`, add to the imports:

```rust
use ratatui::layout::{Margin, Rect};
use ratatui::widgets::{Block, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::body::Extent;
```

Replace the two lines that draw the body:

```rust
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if let Some(extent) = crate::body::render(frame, inner, &panel.body, theme) {
            if extent.overflows() {
                scrollbar(frame, area, &extent, theme);
            }
        }
```

and add beneath `render`:

```rust
/// Over the block's right border, so it spends no content width — and only
/// when something is hidden, so its absence says nothing is.
fn scrollbar(frame: &mut Frame, area: Rect, extent: &Extent, theme: Theme) {
    let mut state = ScrollbarState::new(extent.total)
        .position(extent.position)
        .viewport_content_length(extent.visible);

    frame.render_stateful_widget(
        // No arrows: they would land on the block's corners.
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .style(theme.style(Token::Muted)),
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        }),
        &mut state,
    );
}
```

- [ ] **Step 5: Run the crate's tests**

Run: `cargo test -p lc-inbound`
Expected: PASS, including the exact row-string assertions. The panels those
tests draw either fit their content or are asserted on rows the bar does not
occupy; if one fails, the bar has appeared where the design says it should
not, and that is the bug rather than the test.

- [ ] **Step 6: Gate and commit**

```bash
mise run ci
git add crates/lc-inbound/src/body.rs crates/lc-inbound/src/details.rs crates/lc-inbound/src/render.rs
git commit -m "feat(inbound): mark on the border how far through a panel you are"
```

---

### Task 5: Feed the frame's geometry back

**Files:**
- Modify: `crates/lc-inbound/src/render.rs` (`render` at 13-43, test helpers `draw_focused` at 63-76 and `draw_state` at 247-256)
- Modify: `crates/lc-app/src/main.rs` (`run` at 41-78)

**Interfaces:**
- Consumes: `Geometry::new` from Task 1, `Msg::Measured` from Task 2, `Extent` from Task 4.
- Produces: `lc_inbound::render(frame, view, theme) -> Geometry`.

- [ ] **Step 1: Write the failing render test**

Add to `mod tests` in `crates/lc-inbound/src/render.rs`. It needs
`use lc_domain::state::Geometry;` alongside the existing imports:

```rust
    fn geometry_of(width: u16, height: u16, state: &State) -> Geometry {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");
        let mut geometry = Geometry::default();

        terminal
            .draw(|frame| geometry = render(frame, &view_model(state), Theme::TerminalDefault))
            .expect("TestBackend always draws");

        geometry
    }

    #[test]
    fn the_frame_reports_what_each_panel_can_show() {
        // At 40x20: a 16-column sidebar, Tables 12 rows tall and Ddl 8, so
        // ten and six rows of content inside their borders.
        let geometry = geometry_of(40, 20, &describing(orders()));

        assert_eq!(geometry.height(Panel::Tables), 10);
        assert_eq!(geometry.height(Panel::Ddl), 6);
        assert_eq!(
            geometry.ddl_lines(),
            crate::details::lines(&ddl_details(&describing(orders())), 14, Theme::TerminalDefault)
                .len()
        );
    }

    #[test]
    fn a_grid_reports_the_rows_it_can_show_rather_than_its_height() {
        let state = showing(page_of(&["id"], &[&[CqlValue::Int(1)]]));

        let geometry = geometry_of(60, 12, &state);

        // The Contents panel is 9 rows tall at this size: 7 inside its
        // border, one of which the header takes.
        assert_eq!(
            geometry.height(Panel::Contents),
            6,
            "a page motion must not count the header as a row it can land on"
        );
    }

    #[test]
    fn a_collapsed_panel_reports_no_room_at_all() {
        let geometry = geometry_of(40, 5, &describing(orders()));

        assert_eq!(geometry.height(Panel::Ddl), 0);
        assert_eq!(geometry.height(Panel::Queries), 0);
    }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p lc-inbound render::tests::the_frame_reports`
Expected: FAIL — `mismatched types: expected Geometry, found ()`.

- [ ] **Step 3: Return the geometry from `render`**

In `crates/lc-inbound/src/render.rs`, add `use lc_domain::state::{Geometry, Panel};`
to the imports (the module already imports `lc_domain::ViewModel`), then
replace the whole of `render` with:

```rust
/// Draw one titled block per panel, skipping any the terminal is too small to
/// hold, and report what the frame turned out to be.
///
/// The return value is the domain's only way to learn how tall a panel is and
/// how many lines the Ddl body wrapped to
/// (doc/specs/2026-08-09-panel-scrolling-design.md §3).
pub fn render(frame: &mut Frame, view: &ViewModel, theme: Theme) -> Geometry {
    let rects = layout(frame.area());
    let mut heights = [0; 4];
    let mut ddl_lines = 0;

    for (index, panel) in view.panels.iter().enumerate() {
        let area = rects.get(panel.panel);
        if area.is_empty() {
            continue;
        }

        let (before, shortcut, after) = panel.title.split();
        let title = Line::from(vec![
            Span::raw(before),
            Span::styled(shortcut, theme.style(Token::Shortcut)),
            Span::raw(after),
        ]);

        let mut block = Block::bordered().title(title);
        if panel.panel == view.focus {
            block = block.border_style(theme.style(Token::Focused));
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);

        match crate::body::render(frame, inner, &panel.body, theme) {
            Some(extent) => {
                heights[index] = extent.visible;
                if panel.panel == Panel::Ddl {
                    ddl_lines = extent.total;
                }
                if extent.overflows() {
                    scrollbar(frame, area, &extent, theme);
                }
            }
            // `Blank` and `Notice` do not scroll, so what such a panel can
            // show is simply the room it was given.
            None => heights[index] = usize::from(inner.height),
        }
    }

    if let Some(overlay) = &view.overlay {
        crate::overlay::render(frame, overlay, theme);
    }

    Geometry::new(heights, ddl_lines)
}
```

`scrollbar` from Task 4 is unchanged.

- [ ] **Step 4: Fix the two test helpers that draw**

`terminal.draw` needs a closure returning `()`, so in
`crates/lc-inbound/src/render.rs` both helpers need a brace:

```rust
        terminal
            .draw(|frame| {
                render(frame, &view_model(&state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");
```

Apply the same change to `draw_state`, whose closure takes `state` by
reference.

- [ ] **Step 5: Run the crate's tests**

Run: `cargo test -p lc-inbound`
Expected: PASS. `lc-app` does not compile yet.

- [ ] **Step 6: Send the measurement from the main loop**

In `crates/lc-app/src/main.rs`, add to the imports:

```rust
use lc_domain::state::Geometry;
use lc_domain::{Msg, State, update, view_model};
```

Add two functions above `run`:

```rust
/// Draws a frame and reports what it turned out to be.
fn draw(terminal: &mut DefaultTerminal, state: &State, theme: Theme) -> Result<Geometry> {
    let mut geometry = Geometry::default();

    terminal.draw(|frame| geometry = lc_inbound::render(frame, &view_model(state), theme))?;

    Ok(geometry)
}

/// Tells the domain what the frame turned out to be, but only when it
/// changed.
///
/// Sending unconditionally would redraw, re-measure and redraw forever
/// (doc/specs/2026-08-09-panel-scrolling-design.md §3).
fn report(tx: &mpsc::UnboundedSender<Msg>, last: &mut Option<Geometry>, geometry: Geometry) {
    if *last == Some(geometry) {
        return;
    }

    *last = Some(geometry);
    // A closed channel means the main loop has already ended.
    let _sent = tx.send(Msg::Measured(geometry));
}
```

Replace the two `terminal.draw(..)` calls in `run`, and declare the tracker
beside `state`:

```rust
    let mut measured: Option<Geometry> = None;

    for effect in &startup {
        effects::run(&tx, &cluster, effect);
    }

    report(&tx, &mut measured, draw(&mut terminal, &state, theme)?);
    state.dirty = false;

    while let Some(msg) = rx.recv().await {
        for effect in update(&mut state, msg) {
            effects::run(&tx, &cluster, &effect);
        }

        if state.should_quit {
            break;
        }

        if state.dirty {
            report(&tx, &mut measured, draw(&mut terminal, &state, theme)?);
            state.dirty = false;
        }
    }
```

- [ ] **Step 7: Run the workspace**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 8: Drive it against the fixture**

```bash
mise run db:up
mise run run
```

Focus the Table DDL panel with `a`, select a table in the tree, and check by
hand: `j`/`k` move a line, `PageDown`/`PageUp` move a screen, `End` leaves the
last line at the bottom of the panel with no gap beneath it, and the bar on
the right border appears only when the options run past the fold. Resize the
terminal and confirm the offset is pulled back in rather than leaving the
panel blank.

- [ ] **Step 9: Gate and commit**

```bash
mise run ci
git add crates/lc-inbound/src/render.rs crates/lc-app/src/main.rs
git commit -m "feat(app): report each frame's geometry back to the domain"
```

---

### Task 6: Record the decision

**Files:**
- Modify: `doc/specs/2026-08-09-panel-scrolling-design.md` (header)
- Modify: `TODO.md` (the table metadata block at 39-54)

- [ ] **Step 1: Mark the spec implemented**

Change the header line to `**Status:** implemented`.

- [ ] **Step 2: Retire the entry this closes and log what it deferred**

In `TODO.md`, delete the "Scroll the DDL panel" bullet and its four
continuation lines, then add beneath the existing list:

```markdown
From the panel scrolling slice (doc/specs/2026-08-09-panel-scrolling-design.md):

- `ctrl-d` and `ctrl-u`. `handle_key` refuses every ctrl and alt press, so
  vim's half-page motions need that guard reworked and the test that pins
  "a modified key is not a binding" rewritten — worth doing when the ctrl
  layer is designed, not in passing (design §5).
- A long `Notice` still cannot scroll. One sentence is the assumption; a
  `DbError` carrying a paragraph would break it.
```

- [ ] **Step 3: Commit**

```bash
git add doc/specs/2026-08-09-panel-scrolling-design.md TODO.md
git commit -m "docs: mark the panel scrolling design implemented"
```
