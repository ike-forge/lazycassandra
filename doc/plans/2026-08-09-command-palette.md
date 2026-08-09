# Command palette — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `?` opens a scrollable, centred block listing every binding the program has.

**Architecture:** A static `CATALOGUE` in `lc-domain/src/commands.rs` holds every
binding as the hint bar's `Binding` type, so a row cannot name a key
`handle_key` does not claim. `state.overlay` becomes an enum so a message and
the palette cannot be on screen at once. The palette is drawn by a new
`lc-inbound/src/palette.rs`, which also computes how many rows would fit and
reports it back through `Geometry` — zero means `?` is inert, so there is no
state in which the user is in a mode with nothing on screen saying so.

**Tech Stack:** Rust, `ratatui` 0.30.2, no new dependencies.

**Spec:** [doc/specs/2026-08-09-command-palette-design.md](../specs/2026-08-09-command-palette-design.md)

## Global Constraints

- `AGENTS.md` non-negotiables apply to every task: no `unsafe`, no narrating
  comments, no warnings, no ignored tests, no `#[allow]` without a reason.
- `lc-domain` gains no dependency. `lc-inbound` gains none either.
- `mise run ci` must be clean before a task is considered done. Individual
  steps use `cargo test -p lc-domain` or `-p lc-inbound` for speed; the last
  step of each task runs the gate.
- Conventional Commits, imperative, lowercase, no trailing period.
- Test names are sentences about behaviour, matching the surrounding files
  (`the_palette_cursor_steps_over_a_heading`), never `test_foo`.
- **No action string exceeds 20 characters and no keys label exceeds 9**, or
  the narrowest palette clips a row (spec §5). Task 1 pins this with a test;
  do not widen a string to make wording read better.
- Public items in `lc-domain` carry `///` docs (architecture.md §12).
- The palette's title is `Commands`, exactly — not `Shortcuts`, not `Help`.

---

### Task 1: The catalogue and a cursor that skips headings

Everything in this task is domain-only and reachable from nothing yet. It is
the data and the motion; wiring comes later.

**Files:**
- Create: `crates/lc-domain/src/commands.rs`
- Modify: `crates/lc-domain/src/lib.rs:6-15` (add `pub mod commands;` in
  alphabetical position, before `pub mod contents;`)
- Modify: `crates/lc-domain/src/view/hints.rs:70` (raise `fn label` to
  `pub(crate) fn label`)

**Interfaces:**
- Consumes: `crate::view::Binding` (`{ keys: &'static [Key], action: &'static str }`),
  `crate::view::hints::label(&[Key]) -> String`, `crate::state::Cursor`
  (`index() -> usize`, `go_to(&mut self, index: usize, len: usize)`).
- Produces, all `pub` from `lc_domain::commands`:
  - `struct Group { pub heading: &'static str, pub bindings: &'static [Binding] }`
  - `const CATALOGUE: &[Group]`
  - `enum CommandRow { Heading(&'static str), Command { keys: String, action: &'static str } }`
  - `struct CommandsPanel` with `opened() -> Self`, `selected(&self) -> usize`,
    `down(&mut self)`, `up(&mut self)`, `page_down(&mut self, height: usize)`,
    `page_up(&mut self, height: usize)`, `to_start(&mut self)`,
    `to_end(&mut self)`
  - `fn rows() -> Vec<CommandRow>`
  - `fn row_count() -> usize`

Task 2 uses none of it. Task 3 stores `CommandsPanel` in `Overlay`, projects
`rows()`/`selected()`, and calls `row_count()` from the renderer. Task 4 calls
every motion. Task 5 reads `CATALOGUE`.

- [ ] **Step 1: Raise `label` so the catalogue can render keys**

In `crates/lc-domain/src/view/hints.rs`, change the signature only:

```rust
pub(crate) fn label(keys: &[Key]) -> String {
    keys.iter().copied().map(name).collect::<Vec<_>>().join("/")
}
```

- [ ] **Step 2: Write the failing tests**

Create `crates/lc-domain/src/commands.rs` containing **only** this test module
for now, so the next step's failure is a real one:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The widest row `MIN_W` in the renderer is derived from
    /// (doc/specs/2026-08-09-command-palette-design.md §5): a `shift-tab`
    /// gutter, two columns of gap, and the longest action.
    const WIDEST_KEYS: usize = 9;
    const WIDEST_ACTION: usize = 20;

    fn commands() -> Vec<&'static Binding> {
        CATALOGUE.iter().flat_map(|group| group.bindings).collect()
    }

    #[test]
    fn no_command_row_is_wider_than_the_narrowest_palette() {
        for binding in commands() {
            let keys = label(binding.keys);

            assert!(
                keys.chars().count() <= WIDEST_KEYS,
                "{keys:?} is wider than the gutter MIN_W is derived from"
            );
            assert!(
                binding.action.chars().count() <= WIDEST_ACTION,
                "{:?} is longer than the action column MIN_W is derived from",
                binding.action
            );
        }
    }

    #[test]
    fn every_group_has_at_least_one_command() {
        for group in CATALOGUE {
            assert!(
                !group.bindings.is_empty(),
                "{} is a heading with nothing under it, and `opened` would seat the cursor on it",
                group.heading
            );
        }
    }

    /// `MIN_H` in the renderer assumes the catalogue fills a palette at its
    /// floor. A catalogue that shrank below it would be refused a palette on
    /// frames that could hold one.
    #[test]
    fn the_catalogue_is_taller_than_the_palettes_floor() {
        assert!(row_count() >= 6, "{} rows", row_count());
    }

    #[test]
    fn the_catalogue_flattens_to_a_heading_then_its_commands() {
        let rows = rows();

        assert_eq!(rows[0], CommandRow::Heading("Global"));
        assert_eq!(
            rows[1],
            CommandRow::Command {
                keys: "q".to_owned(),
                action: "quit",
            }
        );
        assert_eq!(rows.len(), row_count());
    }

    #[test]
    fn a_palette_opens_on_the_first_command() {
        assert_eq!(CommandsPanel::opened().selected(), 1);
    }

    #[test]
    fn the_palette_cursor_steps_over_a_heading() {
        let headings: Vec<usize> = rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, CommandRow::Heading(_)))
            .map(|(index, _)| index)
            .collect();

        assert!(headings.len() > 1, "one group would not exercise the skip");

        let mut panel = CommandsPanel::opened();
        let mut seen = vec![panel.selected()];

        for _ in 1..row_count() {
            panel.down();
            seen.push(panel.selected());
        }

        for heading in headings {
            assert!(
                !seen.contains(&heading),
                "the cursor landed on heading row {heading}"
            );
        }

        let mut panel = CommandsPanel::opened();
        panel.to_end();

        for _ in 1..row_count() {
            panel.up();
            assert!(
                !matches!(rows()[panel.selected()], CommandRow::Heading(_)),
                "walking up landed on row {}",
                panel.selected()
            );
        }
    }

    #[test]
    fn the_palette_cursor_cannot_run_off_the_catalogue() {
        let mut panel = CommandsPanel::opened();
        for _ in 0..row_count() * 2 {
            panel.down();
        }
        let last = panel.selected();

        assert_eq!(last, row_count() - 1, "the last row is the last command");

        for _ in 0..row_count() * 2 {
            panel.up();
        }

        assert_eq!(panel.selected(), 1);
    }

    #[test]
    fn home_and_end_land_on_the_first_and_last_command() {
        let mut panel = CommandsPanel::opened();

        panel.to_end();
        assert_eq!(panel.selected(), row_count() - 1);

        panel.to_start();
        assert_eq!(panel.selected(), 1);
    }

    #[test]
    fn paging_settles_on_a_command_rather_than_a_heading() {
        for height in 1..row_count() {
            let mut panel = CommandsPanel::opened();

            panel.page_down(height);
            assert!(
                !matches!(rows()[panel.selected()], CommandRow::Heading(_)),
                "paging down by {height} landed on a heading"
            );

            panel.page_up(height);
            assert!(
                !matches!(rows()[panel.selected()], CommandRow::Heading(_)),
                "paging up by {height} landed on a heading"
            );
        }
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p lc-domain commands`
Expected: FAIL — the module is not declared in `lib.rs`, and once it is, every
name is undefined.

- [ ] **Step 4: Declare the module**

In `crates/lc-domain/src/lib.rs`, add between `pub mod cell;` and
`pub mod contents;`:

```rust
pub mod commands;
```

- [ ] **Step 5: Write the catalogue**

Prepend to `crates/lc-domain/src/commands.rs`, above the test module:

```rust
//! Every binding the program has, and a cursor over them.
//!
//! The palette claims to be complete, which the hint bar deliberately does not
//! (doc/specs/2026-08-09-command-palette-design.md §2). `update::tests`'
//! sweep is what holds that claim up: reusing [`Binding`] means a row names
//! `Key` values rather than a string, so a row cannot describe a key no arm of
//! `handle_key` claims.

use crate::model::Key;
use crate::state::Cursor;
use crate::view::Binding;
use crate::view::hints::label;

/// One labelled group of bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub heading: &'static str,
    pub bindings: &'static [Binding],
}

const GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Key::Char('q')],
        action: "quit",
    },
    Binding {
        keys: &[Key::Char('?')],
        action: "commands",
    },
    Binding {
        keys: &[Key::Escape],
        action: "dismiss the overlay",
    },
    Binding {
        keys: &[Key::Tab],
        action: "next panel",
    },
    Binding {
        keys: &[Key::BackTab],
        action: "previous panel",
    },
    Binding {
        keys: &[Key::Char('t')],
        action: "focus Tables",
    },
    Binding {
        keys: &[Key::Char('a')],
        action: "focus Table DDL",
    },
    Binding {
        keys: &[Key::Char('c')],
        action: "focus Table contents",
    },
    Binding {
        keys: &[Key::Char('u')],
        action: "focus Queries",
    },
];

const MOTION: &[Binding] = &[
    Binding {
        keys: &[Key::Char('j'), Key::Down],
        action: "down one row",
    },
    Binding {
        keys: &[Key::Char('k'), Key::Up],
        action: "up one row",
    },
    Binding {
        keys: &[Key::PageDown],
        action: "down one screen",
    },
    Binding {
        keys: &[Key::PageUp],
        action: "up one screen",
    },
    Binding {
        keys: &[Key::Home],
        action: "to the start",
    },
    Binding {
        keys: &[Key::End],
        action: "to the end",
    },
];

// The actions are cut to twenty characters because MIN_W is derived from the
// longest one (design §5). The heading supplies the noun they drop: under
// `Tables`, "read" can only mean a table.
const TABLES: &[Binding] = &[
    Binding {
        keys: &[Key::Enter, Key::Char(' ')],
        action: "fold/unfold, or read",
    },
    Binding {
        keys: &[Key::Char('l'), Key::Right],
        action: "unfold the keyspace",
    },
    Binding {
        keys: &[Key::Char('h'), Key::Left],
        action: "fold, or step out",
    },
];

/// Every binding, grouped as the palette lists them.
pub const CATALOGUE: &[Group] = &[
    Group {
        heading: "Global",
        bindings: GLOBAL,
    },
    Group {
        heading: "Motion — focused panel",
        bindings: MOTION,
    },
    Group {
        heading: "Tables",
        bindings: TABLES,
    },
];

/// One line of the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRow {
    Heading(&'static str),
    Command {
        keys: String,
        action: &'static str,
    },
}

/// The catalogue as a flat list, headings included.
///
/// Headings are rows so that the cursor and the renderer count in the same
/// scheme; [`CommandsPanel`] is what keeps the cursor off them.
#[must_use]
pub fn rows() -> Vec<CommandRow> {
    CATALOGUE
        .iter()
        .flat_map(|group| {
            std::iter::once(CommandRow::Heading(group.heading)).chain(
                group.bindings.iter().map(|binding| CommandRow::Command {
                    keys: label(binding.keys),
                    action: binding.action,
                }),
            )
        })
        .collect()
}

/// How many rows [`rows`] produces, without building them.
#[must_use]
pub fn row_count() -> usize {
    CATALOGUE
        .iter()
        .map(|group| 1 + group.bindings.len())
        .sum()
}

fn is_command(index: usize) -> bool {
    let mut at = 0;

    for group in CATALOGUE {
        if index == at {
            return false;
        }
        at += 1 + group.bindings.len();
        if index < at {
            return true;
        }
    }

    false
}
```

- [ ] **Step 6: Write the cursor**

Append to `crates/lc-domain/src/commands.rs`, still above the test module:

```rust
#[derive(Clone, Copy)]
enum Direction {
    Down,
    Up,
}

impl Direction {
    const fn reversed(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
        }
    }
}

/// The palette's whole state.
///
/// The catalogue is static, so a cursor is all there is to remember. Every
/// motion seats it on a command, which is the invariant a renderer would
/// otherwise have to defend against — the same reason [`Cursor`]'s index is
/// private (conventions.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandsPanel {
    cursor: Cursor,
}

impl CommandsPanel {
    /// Opens on the first command, which is the row after the first heading.
    #[must_use]
    pub fn opened() -> Self {
        let mut panel = Self {
            cursor: Cursor::default(),
        };
        panel.to_start();
        panel
    }

    /// The selected row, as an index into [`rows`].
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.cursor.index()
    }

    pub fn down(&mut self) {
        self.seat(self.selected().saturating_add(1), Direction::Down);
    }

    pub fn up(&mut self) {
        self.seat(self.selected().saturating_sub(1), Direction::Up);
    }

    pub fn page_down(&mut self, height: usize) {
        self.seat(self.selected().saturating_add(height), Direction::Down);
    }

    pub fn page_up(&mut self, height: usize) {
        self.seat(self.selected().saturating_sub(height), Direction::Up);
    }

    pub fn to_start(&mut self) {
        self.seat(0, Direction::Down);
    }

    pub fn to_end(&mut self) {
        self.seat(row_count().saturating_sub(1), Direction::Up);
    }

    /// Seats the cursor on the first command at or beyond `target` in
    /// `direction`, and failing that on the nearest one the other way — so a
    /// motion that runs out of catalogue stops on the last command rather
    /// than on the heading it happened to reach.
    fn seat(&mut self, target: usize, direction: Direction) {
        let seat = seek(target, direction).or_else(|| seek(target, direction.reversed()));

        if let Some(index) = seat {
            self.cursor.go_to(index, row_count());
        }
    }
}

fn seek(from: usize, direction: Direction) -> Option<usize> {
    let last = row_count().checked_sub(1)?;
    let mut index = from.min(last);

    loop {
        if is_command(index) {
            return Some(index);
        }
        match direction {
            Direction::Down if index < last => index += 1,
            Direction::Up if index > 0 => index -= 1,
            _ => return None,
        }
    }
}
```

- [ ] **Step 7: Run the tests**

Run: `cargo test -p lc-domain commands`
Expected: PASS, nine tests.

- [ ] **Step 8: Run the gate**

Run: `mise run ci`
Expected: clean. If clippy objects to `1 + group.bindings.len()` or to the
`loop`, fix the code rather than allowing the lint.

- [ ] **Step 9: Commit**

```bash
git add crates/lc-domain/src/commands.rs crates/lc-domain/src/lib.rs crates/lc-domain/src/view/hints.rs
git commit -m "feat(domain): catalogue every binding, with a cursor that skips headings

The palette's data and motion, reachable from nothing yet. Headings are rows so
the cursor and the renderer count in one scheme; CommandsPanel is what keeps the
cursor off them, so no caller can construct a selection on a heading.

Actions are capped at twenty characters because the renderer's MIN_W is derived
from the longest one; a test pins it rather than leaving it as arithmetic in the
design (doc/specs/2026-08-09-command-palette-design.md §5).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: Rename `Overlay` to `Message`

Pure rename, no behaviour change, no new types. Split out because it touches
five files and a reviewer should be able to approve it without reading the
palette at all.

**Files:**
- Modify: `crates/lc-domain/src/state.rs:29` (field type), `:68-75` (the struct)
- Modify: `crates/lc-domain/src/update.rs:10` (import), `:179`, `:198`, `:843`
- Modify: `crates/lc-domain/src/view/mod.rs:23` (import), `:33` (field), `:228`
- Modify: `crates/lc-inbound/src/overlay.rs` (throughout — the parameter, the
  test imports, and the `failure()` helper)
- Modify: `crates/lc-inbound/src/render.rs:114` (import), `:328`

**Interfaces:**
- Produces: `lc_domain::state::Message` with the same three public fields
  today's `Overlay` has. `State::overlay` is `Option<Message>`. Task 3 wraps
  both.
- Consumes: nothing.

- [ ] **Step 1: Rename the type**

In `crates/lc-domain/src/state.rs`, replace the struct and its doc comment:

```rust
/// A message drawn over the panels, dismissed with `Esc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub title: String,
    pub message: String,
    pub severity: Severity,
}
```

and the field at `:29`:

```rust
    /// A message drawn over the panels until it is dismissed.
    pub overlay: Option<Message>,
```

- [ ] **Step 2: Follow the compiler**

Run: `cargo build --workspace`

Rename every reported `Overlay` to `Message`. There are eleven sites across
the five files listed above, all of the form `Overlay {` → `Message {` or
`Overlay,` → `Message,` in a `use`. Change nothing else — no field names, no
test names, no strings.

- [ ] **Step 3: Run the gate**

Run: `mise run ci`
Expected: clean, and the same test count as before the rename.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor: rename Overlay to Message

Frees the name for the enum that gains a second variant next
(doc/specs/2026-08-09-command-palette-design.md §3). No behaviour change.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: `Overlay` becomes an enum, and the palette is drawn

After this task a palette put into `State` by hand renders correctly and the
frame reports whether one fits. No key opens it yet.

**Files:**
- Modify: `crates/lc-domain/src/state.rs` (add `Overlay`, extend `Geometry`)
- Modify: `crates/lc-domain/src/update.rs:179`, `:198`, `:843`, `:1363`, `:1484`
- Modify: `crates/lc-domain/src/view/mod.rs` (add `OverlayView`, project it)
- Modify: `crates/lc-domain/src/state.rs:729` (a `Geometry::new` call in tests)
- Create: `crates/lc-inbound/src/palette.rs`
- Modify: `crates/lc-inbound/src/lib.rs` (declare the module)
- Modify: `crates/lc-inbound/src/render.rs:21-73` (`command_rows`, the overlay
  match, `scrollbar` visibility), `:328`
- Modify: `crates/lc-inbound/src/overlay.rs:21` (parameter type)

**Interfaces:**
- Consumes: `lc_domain::commands::{CommandRow, CommandsPanel, rows, row_count}`
  from Task 1; `lc_domain::state::Message` from Task 2.
- Produces:
  - `lc_domain::state::Overlay` — `Message(Message) | Commands(CommandsPanel)`
  - `lc_domain::view::OverlayView` — `Message(Message) | Commands { rows: Vec<CommandRow>, selected: usize }`
  - `Geometry::new(heights: [usize; 4], ddl_lines: usize, command_rows: usize)`
    and `Geometry::command_rows(self) -> usize`
  - `crate::render::scrollbar` raised to `pub(crate)`

  Task 4 constructs `Overlay::Commands` and reads `command_rows()`.

- [ ] **Step 1: Write the failing render tests**

Add to `mod tests` in `crates/lc-inbound/src/render.rs`, and add
`use lc_domain::commands::CommandsPanel;` plus `Overlay` to the existing
`lc_domain::state::{...}` import:

```rust
    fn with_palette(width: u16, height: u16) -> Buffer {
        let state = State {
            overlay: Some(Overlay::Commands(CommandsPanel::opened())),
            ..State::default()
        };
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("TestBackend always initialises");

        terminal
            .draw(|frame| {
                render(frame, &view_model(&state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");

        terminal.backend().buffer().clone()
    }

    /// The row a bordered block's top edge sits on, found by its left corner.
    fn top_border(buffer: &Buffer, expect: &str) -> (u16, u16) {
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if buffer.cell((x, y)).is_some_and(|cell| cell.symbol() == "┌") {
                    let row: String = (x..buffer.area.width)
                        .filter_map(|at| buffer.cell((at, y)))
                        .map(Cell::symbol)
                        .collect();
                    if row.contains(expect) {
                        return (x, y);
                    }
                }
            }
        }
        panic!("no block titled {expect:?} was drawn");
    }

    fn block_width(buffer: &Buffer, expect: &str) -> u16 {
        let (x, y) = top_border(buffer, expect);
        (x + 1..buffer.area.width)
            .find(|at| {
                buffer
                    .cell((*at, y))
                    .is_some_and(|cell| cell.symbol() == "┐")
            })
            .expect("a bordered block has a right corner")
            - x
            + 1
    }

    #[test]
    fn the_palette_lists_the_catalogue() {
        let drawn = text(&with_palette(100, 40));

        assert!(drawn.contains("Commands"), "the block is titled");
        // Strings only the palette can produce: the hint bar already draws
        // "q quit" along the bottom row, so asserting on "quit" would pass
        // with no palette at all.
        assert!(drawn.contains("Global"));
        assert!(drawn.contains("dismiss the overlay"));
        assert!(drawn.contains("focus Table contents"));
        assert!(drawn.contains("unfold the keyspace"));
    }

    #[test]
    fn the_selected_command_is_highlighted() {
        let buffer = with_palette(100, 40);
        let (x, y) = top_border(&buffer, "Commands");

        // Row 0 of the list is the `Global` heading, so the opened cursor sits
        // on row 1: one down for the border, one for the heading.
        let selected = y + 2;
        let styled = |at: u16| {
            buffer
                .cell((x + 2, at))
                .expect("inside the block")
                .modifier
                .contains(Modifier::REVERSED)
        };

        assert!(styled(selected), "the cursor's row is marked");
        assert!(!styled(selected + 1), "and only the cursor's row is");
    }

    #[test]
    fn a_palette_that_overflows_says_so() {
        // 70% of 24 rows leaves 14 for a catalogue in the low twenties.
        let buffer = with_palette(100, 24);
        let (x, y) = top_border(&buffer, "Commands");
        let width = block_width(&buffer, "Commands");

        let track = (y + 1..buffer.area.height).any(|at| {
            buffer
                .cell((x + width - 1, at))
                .is_some_and(|cell| cell.symbol() != "│")
        });

        assert!(track, "a catalogue taller than the box gets a scrollbar");
        assert!(
            !text(&with_palette(100, 80)).is_empty(),
            "and a frame tall enough to hold it all still draws"
        );
    }

    #[test]
    fn the_palette_is_centred() {
        let buffer = with_palette(100, 40);
        let (x, _) = top_border(&buffer, "Commands");
        let width = block_width(&buffer, "Commands");

        assert_eq!(x, (100 - width) / 2, "left margin equals right margin");
    }

    #[test]
    fn a_wide_frame_does_not_get_a_wide_palette() {
        assert_eq!(block_width(&with_palette(200, 40), "Commands"), 60);
        assert_eq!(block_width(&with_palette(100, 40), "Commands"), 50);
    }

    #[test]
    fn a_tall_frame_does_not_get_a_palette_full_of_blank_rows() {
        let buffer = with_palette(100, 80);
        let (x, y) = top_border(&buffer, "Commands");

        let bottom = (y + 1..buffer.area.height)
            .find(|at| {
                buffer
                    .cell((x, *at))
                    .is_some_and(|cell| cell.symbol() == "└")
            })
            .expect("a bordered block has a bottom edge");

        assert_eq!(
            usize::from(bottom - y) + 1,
            lc_domain::commands::row_count() + 2,
            "the block ends where the catalogue does, not at 70% of 80 rows"
        );
    }

    #[test]
    fn a_frame_too_small_for_a_palette_does_not_get_one() {
        assert!(
            !text(&with_palette(60, 40)).contains("Commands"),
            "half of 60 columns is under the palette's floor"
        );
    }

    #[test]
    fn the_frame_reports_how_many_rows_a_palette_would_show() {
        let state = State::default();
        let mut terminal =
            Terminal::new(TestBackend::new(100, 24)).expect("TestBackend always initialises");
        let mut geometry = Geometry::default();

        terminal
            .draw(|frame| {
                geometry = render(frame, &view_model(&state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");

        // 70% of 24 rows is 16, less two borders — reported with no palette
        // open, because it is what gates `?` (design §5.1).
        assert_eq!(geometry.command_rows(), 14);
    }

    #[test]
    fn a_frame_too_small_for_a_palette_reports_no_rows() {
        let state = State::default();
        let mut terminal =
            Terminal::new(TestBackend::new(60, 24)).expect("TestBackend always initialises");
        let mut geometry = Geometry::default();

        terminal
            .draw(|frame| {
                geometry = render(frame, &view_model(&state), Theme::TerminalDefault);
            })
            .expect("TestBackend always draws");

        assert_eq!(geometry.command_rows(), 0);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p lc-inbound palette`
Expected: FAIL to compile — `Overlay` is not an enum, `command_rows` does not
exist.

- [ ] **Step 3: Make `Overlay` an enum**

In `crates/lc-domain/src/state.rs`, add below `Message`:

```rust
/// What is drawn over the panels, at most one at a time.
///
/// An enum rather than a second `Option` field beside the message: two fields
/// would make "a failure and the palette, simultaneously" representable, and
/// the renderer would then need a precedence rule the domain never stated
/// (conventions.md §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Message(Message),
    Commands(CommandsPanel),
}
```

with `use crate::commands::CommandsPanel;` added to the imports, and change the
field back to:

```rust
    /// A message or the command palette, drawn over the panels until it is
    /// dismissed.
    pub overlay: Option<Overlay>,
```

- [ ] **Step 4: Extend `Geometry`**

In `crates/lc-domain/src/state.rs`, add the field, the parameter and the
accessor:

```rust
pub struct Geometry {
    heights: [usize; 4],
    ddl_lines: usize,
    command_rows: usize,
}
```

```rust
    #[must_use]
    pub const fn new(heights: [usize; 4], ddl_lines: usize, command_rows: usize) -> Self {
        Self {
            heights,
            ddl_lines,
            command_rows,
        }
    }

    /// Rows the command palette would show at this frame size, whether or not
    /// one is open.
    ///
    /// Zero means it does not fit, which is what makes `?` inert rather than
    /// opening a palette nothing draws
    /// (doc/specs/2026-08-09-command-palette-design.md §5.1).
    #[must_use]
    pub const fn command_rows(self) -> usize {
        self.command_rows
    }
```

- [ ] **Step 5: Add `OverlayView` and project it**

In `crates/lc-domain/src/view/mod.rs`, add `use crate::commands::{self, CommandRow};`,
change the import at `:23` to bring in `Message` and `Overlay`, change the
field, and add the projection:

```rust
/// An overlay as a renderer needs it.
///
/// Splits from [`Overlay`] because the palette's state is a cursor and its
/// projection is rows — the same reason `TablesPanel` and [`PanelBody::Tree`]
/// are not one type. `Message` is shared rather than restated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayView {
    Message(Message),
    Commands {
        rows: Vec<CommandRow>,
        selected: usize,
    },
}

fn overlay(state: &State) -> Option<OverlayView> {
    Some(match state.overlay.as_ref()? {
        Overlay::Message(message) => OverlayView::Message(message.clone()),
        Overlay::Commands(panel) => OverlayView::Commands {
            rows: commands::rows(),
            selected: panel.selected(),
        },
    })
}
```

Change `ViewModel`'s field to `pub overlay: Option<OverlayView>`, add
`OverlayView` to the `pub use` list, and in `view_model` replace
`overlay: state.overlay.clone(),` with `overlay: overlay(state),`.

- [ ] **Step 6: Follow the compiler through the domain**

Run: `cargo test -p lc-domain`

Fix, in order:
- `update.rs:179`, `:198`, `:843` — wrap in `Overlay::Message(Message { … })`.
- `update.rs:1363` — `Geometry::new(heights, ddl_lines, 0)`.
- `update.rs:1484` — `Geometry::new([3, 6, 6, 1], 20, 0)`.
- `state.rs:729` — `Geometry::new([8, 14, 20, 1], 37, 0)`.
- `view/mod.rs:228` — the `the_overlay_is_carried_to_the_renderer` test now
  compares against a projection, so rewrite its assertion. Its module needs
  `use crate::commands::CommandsPanel;` and `use crate::state::{Message, Overlay};`
  alongside the existing imports, and `OverlayView` comes in through the
  `use super::*` already there:

```rust
    #[test]
    fn the_overlay_is_carried_to_the_renderer() {
        let message = Message {
            title: "Connection failed".to_owned(),
            message: "no route".to_owned(),
            severity: Severity::Danger,
        };
        let state = State {
            overlay: Some(Overlay::Message(message.clone())),
            ..State::default()
        };

        assert_eq!(view_model(&state).overlay, Some(OverlayView::Message(message)));
    }

    #[test]
    fn an_open_palette_is_projected_as_rows_and_a_selection() {
        let state = State {
            overlay: Some(Overlay::Commands(CommandsPanel::opened())),
            ..State::default()
        };

        assert_eq!(
            view_model(&state).overlay,
            Some(OverlayView::Commands {
                rows: crate::commands::rows(),
                selected: 1,
            })
        );
    }
```

- [ ] **Step 7: Write the palette renderer**

Create `crates/lc-inbound/src/palette.rs`:

```rust
//! The command palette: every binding, drawn over the panels.

use lc_domain::commands::CommandRow;
use lc_domain::style::Token;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState};

use crate::body::Extent;
use crate::theme::Theme;

const WIDTH_PERCENT: u16 = 50;
const HEIGHT_PERCENT: u16 = 70;
/// The widest row the catalogue can produce, plus both borders and the pad.
/// Derived from the catalogue rather than chosen — `commands::tests` is what
/// stops a longer action being written
/// (doc/specs/2026-08-09-command-palette-design.md §5).
const MIN_W: u16 = 34;
/// Past this every row is trailing space.
const MAX_W: u16 = 60;
/// Two borders, a heading, and enough commands to be worth opening.
const MIN_H: u16 = 8;
/// Between the left border and the keys column.
const PAD: u16 = 1;
/// Between the keys column and the action column.
const GAP: &str = "  ";

/// Where a palette listing `rows` rows goes in `area`, or `None` when it does
/// not fit.
fn rect(area: Rect, rows: usize) -> Option<Rect> {
    let width = percent(area.width, WIDTH_PERCENT).min(MAX_W);
    let wanted = u16::try_from(rows.saturating_add(2)).unwrap_or(u16::MAX);
    let height = percent(area.height, HEIGHT_PERCENT).min(wanted);

    if width < MIN_W || height < MIN_H {
        return None;
    }

    Some(Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}

/// Rows a palette would show in `area`, whether or not one is open. Zero means
/// it does not fit, and is what gates `?` in the domain.
pub(crate) fn visible_rows(area: Rect, rows: usize) -> usize {
    rect(area, rows).map_or(0, |rect| usize::from(rect.height.saturating_sub(2)))
}

/// Widened to `u32` first: at a terminal width near `u16::MAX` the product
/// would overflow, which panics in debug rather than clamping.
fn percent(value: u16, share: u16) -> u16 {
    u16::try_from(u32::from(value) * u32::from(share) / 100).unwrap_or(u16::MAX)
}

pub(crate) fn render(frame: &mut Frame, rows: &[CommandRow], selected: usize, theme: Theme) {
    let Some(area) = rect(frame.area(), rows.len()) else {
        return;
    };

    let block = Block::bordered().title("Commands");
    let inner = block.inner(area).inner(Margin {
        horizontal: PAD,
        vertical: 0,
    });

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let gutter = rows.iter().map(keys_width).max().unwrap_or(0);
    let items: Vec<ListItem> = rows.iter().map(|row| item(row, gutter, theme)).collect();
    let mut state = ListState::default().with_selected(Some(selected));

    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.style(Token::Selected)),
        inner,
        &mut state,
    );

    let extent = Extent {
        position: state.offset(),
        total: rows.len(),
        visible: usize::from(inner.height),
    };

    if extent.overflows() {
        crate::render::scrollbar(frame, area, &extent, theme);
    }
}

/// The palette says in a column what the bar says in a row, so it says it with
/// the bar's tokens — a `CommandKey` token would be a second name for one
/// meaning (architecture.md §6).
fn item(row: &CommandRow, gutter: usize, theme: Theme) -> ListItem<'static> {
    match row {
        CommandRow::Heading(heading) => ListItem::new(Line::from(Span::styled(
            (*heading).to_owned(),
            theme.style(Token::Muted),
        ))),
        CommandRow::Command { keys, action } => ListItem::new(Line::from(vec![
            Span::styled(format!("{keys:<gutter$}"), theme.style(Token::HintKey)),
            Span::styled(format!("{GAP}{action}"), theme.style(Token::HintAction)),
        ])),
    }
}

/// Columns, on the same single-width assumption the hint bar makes of the
/// labels `hints::name` produces.
fn keys_width(row: &CommandRow) -> usize {
    match row {
        CommandRow::Heading(_) => 0,
        CommandRow::Command { keys, .. } => keys.chars().count(),
    }
}
```

- [ ] **Step 8: Wire it into `render`**

In `crates/lc-inbound/src/lib.rs`, declare the module alongside the others:

```rust
mod palette;
```

In `crates/lc-inbound/src/render.rs`, raise the helper to `pub(crate)`:

```rust
pub(crate) fn scrollbar(frame: &mut Frame, area: Rect, extent: &Extent, theme: Theme) {
```

and append to its doc comment:

```rust
/// `pub(crate)` for the palette, which scrolls the same way and must not grow
/// a second copy of the arithmetic above.
```

Change `render` to measure the palette and to match on the overlay:

```rust
pub fn render(frame: &mut Frame, view: &ViewModel, theme: Theme) -> Geometry {
    let command_rows = crate::palette::visible_rows(frame.area(), lc_domain::commands::row_count());

    let rects = match layout(frame.area()) {
        Shell::Panels(rects) => rects,
        Shell::TooSmall(area) => {
            crate::too_small::render(frame, area);
            return Geometry::default();
        }
    };
```

and at the end:

```rust
    match &view.overlay {
        Some(OverlayView::Message(message)) => crate::overlay::render(frame, message, theme),
        Some(OverlayView::Commands { rows, selected }) => {
            crate::palette::render(frame, rows, *selected, theme);
        }
        None => {}
    }

    Geometry::new(heights, ddl_lines, command_rows)
}
```

with `use lc_domain::view::OverlayView;` added.

- [ ] **Step 9: Retarget `overlay::render`**

In `crates/lc-inbound/src/overlay.rs`, three changes and nothing else — the
wrapping and sizing logic is untouched:

```rust
use lc_domain::state::Message;

pub(crate) fn render(frame: &mut Frame, overlay: &Message, theme: Theme) {
```

```rust
fn centred(area: Rect, overlay: &Message) -> Option<Rect> {
```

and in `mod tests`, change the import to
`use lc_domain::state::{ConnectionState, Message, Overlay};`, have `failure()`
return a `Message`, and wrap at the one place it is stored:

```rust
    fn draw(width: u16, height: u16, overlay: Option<Message>) -> Buffer {
        let state = State {
            connection: ConnectionState::Disconnected,
            overlay: overlay.map(Overlay::Message),
            ..State::default()
        };
```

- [ ] **Step 10: Run the tests**

Run: `cargo test -p lc-domain && cargo test -p lc-inbound`
Expected: PASS.

If `a_wide_frame_does_not_get_a_wide_palette` reports 50 where 60 is expected
at 200 columns, `MAX_W` was not applied; if `the_palette_is_centred` is off by
one, the rect arithmetic rounded the wrong way — match `overlay::centred`,
which this mirrors deliberately.

- [ ] **Step 11: Run the gate**

Run: `mise run ci`
Expected: clean.

- [ ] **Step 12: Commit**

```bash
git add -A
git commit -m "feat: draw the command palette over the panels

Overlay becomes an enum, so a message and the palette cannot be on screen at
once and the renderer needs no precedence rule the domain never stated. Geometry
gains command_rows: what a palette would show at this frame size, reported
whether or not one is open, because it is what will gate the binding.

No key opens it yet.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: `?` opens it, and an open palette takes the keys

**Files:**
- Modify: `crates/lc-domain/src/update.rs:267-303` (`handle_key`), plus two new
  functions beneath it, plus tests
- Modify: `crates/lc-domain/src/view/hints.rs` (`GLOBAL`, `bar`, tests)
- Modify: `crates/lc-domain/src/view/mod.rs` (the one `hints::bar` call)
- Modify: `doc/shortcuts.md` §1, §2, §3, §4, §5
- Modify: `TODO.md`
- Modify: `doc/specs/2026-08-09-command-palette-design.md` (status line)

**Interfaces:**
- Consumes: `CommandsPanel::{opened, down, up, page_down, page_up, to_start, to_end, selected}`,
  `Geometry::command_rows`, `Overlay::Commands`.
- Produces: nothing new. Task 5 tests what this binds.

- [ ] **Step 1: Write the failing domain tests**

Add to `mod tests` in `crates/lc-domain/src/update.rs`, and add
`use crate::commands::CommandsPanel;` and `Message` to the imports:

```rust
    /// A frame big enough for a palette. `browsing` leaves geometry at its
    /// default, whose `command_rows` is zero — which is itself a binding
    /// (`the_palette_does_not_open_where_it_cannot_be_drawn`).
    fn measured(keyspaces: &[&str]) -> State {
        let mut state = browsing(keyspaces);
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        state
    }

    fn palette(state: &State) -> Option<&CommandsPanel> {
        match state.overlay.as_ref()? {
            Overlay::Commands(panel) => Some(panel),
            Overlay::Message(_) => None,
        }
    }

    #[test]
    fn question_mark_opens_the_palette_and_closes_it() {
        let mut state = measured(&["a"]);

        press(&mut state, Key::Char('?'));
        assert!(palette(&state).is_some());

        press(&mut state, Key::Char('?'));
        assert!(state.overlay.is_none());
    }

    #[test]
    fn escape_closes_the_palette() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Escape);

        assert!(state.overlay.is_none());
    }

    #[test]
    fn opening_the_palette_replaces_the_message_it_covers() {
        let mut state = measured(&["a"]);
        state.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

        press(&mut state, Key::Char('?'));
        assert!(palette(&state).is_some());

        press(&mut state, Key::Escape);
        assert!(
            state.overlay.is_none(),
            "esc leaves nothing rather than peeling back to the message"
        );
    }

    #[test]
    fn the_palette_does_not_open_where_it_cannot_be_drawn() {
        let mut state = browsing(&["a"]);
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 0);

        press(&mut state, Key::Char('?'));

        assert!(
            state.overlay.is_none(),
            "a mode with nothing on screen saying so is worse than an inert key"
        );
    }

    #[test]
    fn an_open_palette_takes_the_motion_the_panel_would_have() {
        let mut state = measured(&["a", "b", "c"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('j'));

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "the tree did not move behind the palette"
        );
        assert_eq!(palette(&state).expect("still open").selected(), 2);

        press(&mut state, Key::Char('k'));
        assert_eq!(palette(&state).expect("still open").selected(), 1);

        press(&mut state, Key::End);
        assert_eq!(
            palette(&state).expect("still open").selected(),
            crate::commands::row_count() - 1
        );

        press(&mut state, Key::Home);
        assert_eq!(palette(&state).expect("still open").selected(), 1);
    }

    #[test]
    fn q_quits_from_inside_the_palette() {
        let mut state = measured(&["a"]);
        press(&mut state, Key::Char('?'));

        press(&mut state, Key::Char('q'));

        assert!(state.should_quit);
    }

    #[test]
    fn an_open_palette_swallows_the_keys_it_does_not_use() {
        for key in [Key::Char('t'), Key::Tab, Key::Enter, Key::Char(' ')] {
            let mut state = measured(&["a"]);
            press(&mut state, Key::Char('?'));
            let before = state.focus;

            press(&mut state, key);

            assert!(palette(&state).is_some(), "{key:?} dismissed the palette");
            assert_eq!(state.focus, before, "{key:?} reached past the palette");
        }
    }
```

- [ ] **Step 2: Write the failing hint-bar test**

Add to `mod tests` in `crates/lc-domain/src/view/hints.rs`:

```rust
    #[test]
    fn the_bar_names_the_palette_that_reaches_the_rest() {
        assert_eq!(
            bar(&State::default()).global[0].keys,
            "?",
            "the way to every other binding survives longest but one"
        );
    }

    #[test]
    fn the_bar_names_what_the_palette_responds_to() {
        let state = State {
            overlay: Some(Overlay::Commands(CommandsPanel::opened())),
            ..State::default()
        };

        let bar = bar(&state);

        let HintGroup::Keys(hints) = &bar.panel else {
            panic!("an open palette has bindings");
        };
        assert_eq!(hints[0].action, "scroll");
        assert_eq!(bar.global[0].keys, "esc");
        assert_eq!(bar.global[0].action, "close");
        assert_eq!(bar.global.last().expect("q is last").action, "quit");
    }
```

with `use crate::commands::CommandsPanel;`, `use crate::state::{Overlay, State};`
added to the test module.

- [ ] **Step 3: Run to verify both fail**

Run: `cargo test -p lc-domain`
Expected: FAIL — `bar` takes a `Panel`, and `?` is unbound.

- [ ] **Step 4: Bind `?` and route an open palette**

In `crates/lc-domain/src/update.rs`, add `use crate::commands::CommandsPanel;`
and `Overlay` is already imported. Insert into `handle_key` directly after the
modifier guard:

```rust
    // An open palette takes the keys it uses. The one mode in the program, and
    // the reason doc/shortcuts.md §2's "no modes" rule now carries a carve-out
    // (doc/specs/2026-08-09-command-palette-design.md §7).
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return palette_key(state, press.key);
    }
```

and add the `?` arm immediately below `q`'s, so no later arm can shadow it:

```rust
        Key::Char('q') => return quit(state),
        Key::Char('?') => open_palette(state),
```

Add beneath `handle_key`:

```rust
/// `?` opens the palette, replacing whatever overlay was up.
///
/// Refused where the frame cannot draw one: opening a palette nothing draws
/// leaves the user in a mode with nothing on screen saying so, wondering why
/// `j` stopped moving the tree (design §5.1).
fn open_palette(state: &mut State) {
    if state.geometry.command_rows() == 0 {
        return;
    }

    state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));
}

/// What an open palette responds to.
///
/// `q` still quits, because a key that means "quit" everywhere except in one
/// overlay is not a key anyone can trust without looking. Everything the
/// palette does not use is inert rather than dismissing, so no letter means
/// "close" in one frame and something else in the next (design §7).
fn palette_key(state: &mut State, key: Key) -> Vec<Effect> {
    let height = state.geometry.command_rows();

    match key {
        Key::Char('q') => return quit(state),
        Key::Char('?') | Key::Escape => {
            state.overlay = None;
            return Vec::new();
        }
        _ => {}
    }

    let Some(Overlay::Commands(panel)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => panel.down(),
        Key::Char('k') | Key::Up => panel.up(),
        Key::PageDown => panel.page_down(height),
        Key::PageUp => panel.page_up(height),
        Key::Home => panel.to_start(),
        Key::End => panel.to_end(),
        _ => {}
    }

    Vec::new()
}
```

- [ ] **Step 5: Put `?` on the bar, and let the palette claim it**

In `crates/lc-domain/src/view/hints.rs`, add `?` at the head of `GLOBAL`:

```rust
pub(crate) const GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Key::Char('?')],
        action: "commands",
    },
    Binding {
        keys: &[Key::Tab],
        action: "panel",
    },
    Binding {
        keys: &[Key::Char('q')],
        action: "quit",
    },
];
```

and append to its doc comment:

```rust
/// `?` leads because the bar drops global hints from the left: `q quit` is last
/// to go because a user who can read one hint needs the way out, and `?` is
/// next because it is the way to every binding the bar had no room for.
```

Add the palette's own two sets and switch on them:

```rust
const PALETTE: &[Binding] = &[
    Binding {
        keys: &[Key::Char('j'), Key::Char('k')],
        action: "scroll",
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

/// `esc` is named only here. The bar leaves it out everywhere else because it
/// does something only when an overlay is up, and naming it always would be a
/// lie in most frames (hint bar design §3). With the palette open it is never
/// a lie, and it is half of what the user is owed for `q` still quitting.
const PALETTE_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Key::Escape],
        action: "close",
    },
    Binding {
        keys: &[Key::Char('q')],
        action: "quit",
    },
];

pub(crate) fn bar(state: &State) -> HintBar {
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return HintBar {
            panel: HintGroup::Keys(PALETTE.iter().map(rendered).collect()),
            global: PALETTE_GLOBAL.iter().map(rendered).collect(),
        };
    }

    HintBar {
        panel: match state.focus.hints() {
            [] => HintGroup::Idle("nothing here yet"),
            bindings => HintGroup::Keys(bindings.iter().map(rendered).collect()),
        },
        global: GLOBAL.iter().map(rendered).collect(),
    }
}
```

with `use crate::state::{Overlay, State};` replacing the `Panel` import if
`Panel` becomes unused. In `crates/lc-domain/src/view/mod.rs`, change the call
to `hints: hints::bar(state),`.

Fix the two existing `bar(Panel::…)` tests in `hints.rs` to build a `State`
with that `focus` instead.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p lc-domain`
Expected: PASS.

- [ ] **Step 7: Update `doc/shortcuts.md`**

§1, in the Global table, after the `q` row:

```markdown
| `?` | Open the command palette: every binding, in one scrollable block. |
```

§2, replace the **No modes** paragraph:

```markdown
**One mode, opened deliberately.** No letter changes meaning. The one state
that captures keys is the command palette, which the user opened with `?` and
closes with either `?` or `esc`, both named on the bar while it is up
([design](specs/2026-08-09-command-palette-design.md) §7). Motion scrolls the
palette rather than the panel underneath, `q` still quits, and every other key
is inert rather than dismissing — a key that means "close" in one frame and
something else in the next is the ambiguity this rule exists to prevent. Text
entry in the Queries panel is still the design that has to break the rest of
it (§4).
```

§2, extend the **A binding must be visible** paragraph's last sentence:

```markdown
A binding that is in none of a title, the bar, or the palette is a binding only
this file knows about.
```

§3, move `?` into the taken set:

```markdown
Taken, lowercase: `a` `c` `h` `j` `k` `l` `q` `t` `u`, plus `Space` and `?`.
```

§4, delete the `?` help row entirely.

§5, add:

```markdown
| `?` opens and closes the palette | `question_mark_opens_the_palette_and_closes_it`, `escape_closes_the_palette` |
| It replaces a message rather than stacking | `opening_the_palette_replaces_the_message_it_covers` |
| It does not open where it cannot be drawn | `the_palette_does_not_open_where_it_cannot_be_drawn` |
| An open palette takes motion and swallows the rest | `an_open_palette_takes_the_motion_the_panel_would_have`, `an_open_palette_swallows_the_keys_it_does_not_use`, `q_quits_from_inside_the_palette` |
| The cursor never lands on a heading | `the_palette_cursor_steps_over_a_heading`, `paging_settles_on_a_command_rather_than_a_heading` (`commands.rs`) |
| No row outgrows the narrowest palette | `no_command_row_is_wider_than_the_narrowest_palette` (`commands.rs`) |
| The bar names `?`, and the palette's own keys | `the_bar_names_the_palette_that_reaches_the_rest`, `the_bar_names_what_the_palette_responds_to` (`view/hints.rs`) |
| The palette is drawn, centred, capped and scrollable | `the_palette_lists_the_catalogue`, `the_palette_is_centred`, `a_wide_frame_does_not_get_a_wide_palette`, `a_tall_frame_does_not_get_a_palette_full_of_blank_rows`, `a_frame_too_small_for_a_palette_does_not_get_one` (`inbound/render.rs`) |
```

- [ ] **Step 8: Update `TODO.md` and the spec status**

Append to `TODO.md`:

```markdown
From the command palette slice (doc/specs/2026-08-09-command-palette-design.md §10):

- Search. The palette is named Commands and carries a selection precisely so
  this can land; it needs text input, which needs the mode design §7 defers.
- Execution. `⏎` selects a row that cannot be run, because a `Binding` is keys
  and a label. Making it runnable means feeding a synthesised `KeyPress` back
  through `handle_key` rather than growing a second dispatch point.
- Contextual ordering. The palette shows the same three groups whatever has
  focus; vision.md §4 principle 1 promises a *contextual* map.
```

In `doc/specs/2026-08-09-command-palette-design.md`, change the status line to
`**Status:** implemented`.

- [ ] **Step 9: Run the gate**

Run: `mise run ci`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "feat(domain): bind ? to the command palette

Every binding in one scrollable block, and the first state in the program that
captures keys. Motion scrolls the palette rather than the panel underneath, q
still quits, and everything else is inert rather than dismissing.

Breaks doc/shortcuts.md §2's no-modes rule, which §2 anticipated; §2 now carries
the carve-out and names the two keys that close it. ? joins GLOBAL ahead of tab,
and esc is named on the bar in the one state where it always does something.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 5: The sweep that holds the completeness claim up

The palette says it lists everything. This is what makes that true. It is its
own task because it is a guarantee rather than a feature, and because it may
well find something — if it does, the finding is the point.

**Files:**
- Modify: `crates/lc-domain/src/update.rs` (tests only)

**Interfaces:**
- Consumes: `crate::commands::CATALOGUE`, `crate::model::Key`, `view_model`.
- Produces: nothing.

- [ ] **Step 1: Write the sweep**

Add to `mod tests` in `crates/lc-domain/src/update.rs`, with
`use crate::commands::CATALOGUE;` and `use crate::view_model;`:

```rust
    /// Every `Key` the domain understands.
    ///
    /// The `match` is dead weight at runtime and load-bearing at compile time:
    /// a variant added to `Key` without a line here fails to compile, so the
    /// sweep below cannot quietly stop sweeping.
    fn every_key() -> Vec<Key> {
        match Key::Enter {
            Key::Char(_)
            | Key::Enter
            | Key::Escape
            | Key::Backspace
            | Key::Tab
            | Key::BackTab
            | Key::Delete
            | Key::Insert
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
            | Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::F(_) => {}
        }

        [
            Key::Enter,
            Key::Escape,
            Key::Backspace,
            Key::Tab,
            Key::BackTab,
            Key::Delete,
            Key::Insert,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
        ]
        .into_iter()
        .chain((1..=12).map(Key::F))
        .chain((0x20..=0x7e_u8).map(|byte| Key::Char(char::from(byte))))
        .collect()
    }

    fn catalogued() -> Vec<Key> {
        CATALOGUE
            .iter()
            .flat_map(|group| group.bindings)
            .flat_map(|binding| binding.keys)
            .copied()
            .collect()
    }

    /// States that between them exercise every binding: one with an overlay to
    /// dismiss, an unfolded keyspace to fold, and the tree at its first row;
    /// one focused somewhere other than Tables so the focus letters differ;
    /// and one with the tree folded and moved off its first row, so `k`,
    /// `pgup`, `home` and `l` all have somewhere to go.
    ///
    /// The unfold matters: with every keyspace already folded, `h` and `←`
    /// would be a no-op in all three and the sweep would call them unbound.
    fn sweep_fixtures() -> Vec<State> {
        let geometry = Geometry::new([8, 14, 20, 1], 37, 14);

        let mut covered = browsing_tables(&[("shop", &["orders"])]);
        covered.geometry = geometry;
        press(&mut covered, Key::Char('l'));
        covered.overlay = Some(Overlay::Message(Message {
            title: "Schema unavailable".to_owned(),
            message: "boom".to_owned(),
            severity: Severity::Danger,
        }));

        let mut elsewhere = browsing_tables(&[("shop", &["orders"])]);
        elsewhere.geometry = geometry;
        elsewhere.focus = Panel::Ddl;

        let mut moved = browsing(&["a", "b", "c"]);
        moved.geometry = geometry;
        press(&mut moved, Key::Char('j'));
        press(&mut moved, Key::Char('j'));

        vec![covered, elsewhere, moved]
    }

    /// Whether pressing `key` changes anything a user could observe.
    ///
    /// The view model rather than the `State`, which is not `PartialEq` and
    /// whose `dirty` flag `handle_key` sets on every press. It is also the
    /// better question: a key that changes nothing visible and emits no effect
    /// is not a binding, whatever it touched on the way through.
    fn does_something(state: &State, key: Key) -> bool {
        let mut after = state.clone();
        let effects = press(&mut after, key);

        !effects.is_empty()
            || after.should_quit != state.should_quit
            || view_model(&after) != view_model(state)
    }

    #[test]
    fn no_key_does_anything_the_palette_does_not_list() {
        let catalogued = catalogued();

        for state in sweep_fixtures() {
            for key in every_key() {
                if catalogued.contains(&key) {
                    continue;
                }

                assert!(
                    !does_something(&state, key),
                    "{key:?} does something and doc/shortcuts.md's palette does not list it"
                );
            }
        }
    }

    #[test]
    fn every_key_the_palette_lists_does_something() {
        let fixtures = sweep_fixtures();

        for key in catalogued() {
            assert!(
                fixtures.iter().any(|state| does_something(state, key)),
                "the palette lists {key:?} and no state responds to it"
            );
        }
    }
```

- [ ] **Step 2: Run the sweep**

Run: `cargo test -p lc-domain sweep_fixtures no_key_does_anything every_key_the_palette`

Expected: PASS. If either fails, **the failure is information, not a test bug**:

- A key doing something the catalogue omits → add it to `CATALOGUE` in
  `commands.rs` and to `doc/shortcuts.md` §1, keeping the action within 20
  characters.
- A catalogued key doing nothing anywhere → either the fixtures do not reach it
  (add a fixture, and say in the commit which binding needed it) or the binding
  does not exist and the catalogue is lying (delete the row).

Do not weaken `does_something` to make a failure go away.

- [ ] **Step 3: Run the gate**

Run: `mise run ci`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "test(domain): sweep every key against the palette's catalogue

The palette claims to list everything, which is a stronger claim than the hint
bar makes and needs more than a shared data structure to hold up. Presses every
Key the domain understands against three fixtures and asserts both directions:
nothing responds that the catalogue omits, and nothing is catalogued that
responds nowhere.

Compares view models rather than State, which is not PartialEq and whose dirty
flag every press sets — and which is the better question anyway.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Notes for the implementer

**If the sweep fails in Task 5, do not go back and change Tasks 1–4 silently.**
Fix it in Task 5's commit and say what it found. That failure is the most
valuable output this plan can produce.

**`mise run ci` is the gate, not `cargo test`.** Clippy runs with warnings
denied, and the palette's `format!("{keys:<gutter$}")`, the `u32` arithmetic in
`percent`, and the `loop` in `seek` are the three places it is most likely to
have an opinion. Fix the code; do not add `#[allow]`.

**The two decisions most likely to be second-guessed in review** are recorded
in the spec with their costs, and are not open: `q` quits from inside the
palette (§7), and the width caps at 60 columns rather than tracking 50% of a
wide terminal (§5).
