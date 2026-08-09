# Panel focus shortcuts — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One key per panel — `t` Tables, `a` Table DDL, `c` Contents, `u` Queries — with that letter underlined in the panel's title.

**Architecture:** `Panel` owns its shortcut, so `handle_key` gains one arm and a
fifth panel cannot compile without declaring a key. The title travels to the
renderer as a `PanelTitle` that carries a validated byte range for the letter,
computed where the title is built because the Contents title is dynamic and the
match is case-insensitive. Drawing it is a new semantic token,
`Token::Shortcut`, that the theme maps to `UNDERLINED`.

**Tech Stack:** Rust, `ratatui` 0.30.2, no new dependencies.

**Spec:** [doc/specs/2026-08-09-panel-focus-shortcuts-design.md](../specs/2026-08-09-panel-focus-shortcuts-design.md)

## Global Constraints

- `AGENTS.md` non-negotiables apply to every task: no `unsafe`, no narrating
  comments, no warnings, no ignored tests.
- `lc-domain` gains no dependency. Tasks 1 and 2 touch it; neither needs one.
- `mise run ci` must be clean before a task is considered done. Individual
  steps use `cargo test -p <crate>` for speed; the final step of each task runs
  the gate.
- Conventional Commits, imperative, lowercase, no trailing period.
- Test names are sentences about behaviour, matching the surrounding files
  (`a_cursor_starts_on_the_first_row`), not `test_foo`.

---

### Task 1: `Panel` owns its shortcut

**Files:**
- Modify: `crates/lc-domain/src/state.rs` (impl block at 104-129, tests at 234+)
- Modify: `crates/lc-domain/src/update.rs` (`handle_key` at 206-232, tests at 690+)

**Interfaces:**
- Produces: `Panel::shortcut(self) -> char` (const), `Panel::from_shortcut(key: char) -> Option<Panel>`. Task 2 calls `shortcut`.

- [ ] **Step 1: Write the failing tests in `state.rs`**

Add to `mod tests` in `crates/lc-domain/src/state.rs`:

```rust
    #[test]
    fn every_panel_has_a_distinct_shortcut() {
        let mut claimed: Vec<char> = Vec::new();

        for panel in Panel::ALL {
            let key = panel.shortcut();
            assert!(
                !claimed.contains(&key),
                "{panel:?} claims '{key}', which another panel already has"
            );
            claimed.push(key);
        }
    }

    #[test]
    fn a_shortcut_leads_back_to_the_panel_that_claims_it() {
        for panel in Panel::ALL {
            assert_eq!(Panel::from_shortcut(panel.shortcut()), Some(panel));
        }

        assert_eq!(Panel::from_shortcut('z'), None);
    }

    #[test]
    fn the_shortcuts_are_the_ones_the_design_names() {
        assert_eq!(
            Panel::ALL.map(Panel::shortcut),
            ['t', 'a', 'c', 'u'],
            "the keys are a promise to the user's fingers, not an implementation detail"
        );
    }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p lc-domain state::tests`
Expected: FAIL — `no method named shortcut found for enum Panel`.

- [ ] **Step 3: Add the two methods**

In `crates/lc-domain/src/state.rs`, inside `impl Panel`, after `previous()`:

```rust
    /// The key that focuses this panel from anywhere.
    ///
    /// Underlined in the panel's title, so the binding is on screen rather
    /// than in a manual (vision.md §4, principle 1). Two of the four are not
    /// leading letters — the titles are fixed by the draft, and the keys were
    /// chosen to be reachable rather than to match a first character.
    #[must_use]
    pub const fn shortcut(self) -> char {
        match self {
            Self::Tables => 't',
            Self::Ddl => 'a',
            Self::Contents => 'c',
            Self::Queries => 'u',
        }
    }

    /// The panel `key` focuses, if any panel claims it.
    #[must_use]
    pub fn from_shortcut(key: char) -> Option<Self> {
        Self::ALL.into_iter().find(|panel| panel.shortcut() == key)
    }
```

- [ ] **Step 4: Run them and watch them pass**

Run: `cargo test -p lc-domain state::tests`
Expected: PASS.

- [ ] **Step 5: Write the failing binding tests in `update.rs`**

Add to `mod tests` in `crates/lc-domain/src/update.rs`, beside the other key
tests. `press` and `browsing` are the existing helpers in that module:

```rust
    #[test]
    fn a_shortcut_focuses_its_panel_from_any_other_panel() {
        for from in Panel::ALL {
            for to in Panel::ALL {
                let mut state = State {
                    focus: from,
                    ..State::default()
                };

                press(&mut state, Key::Char(to.shortcut()));

                assert_eq!(
                    state.focus, to,
                    "'{}' should reach {to:?} from {from:?}",
                    to.shortcut()
                );
            }
        }
    }

    /// Also the collision test: a shortcut that an earlier arm already answers
    /// would never reach the focus arm, and the focus assertion would fail.
    #[test]
    fn a_shortcut_moves_focus_and_does_nothing_else() {
        for panel in Panel::ALL {
            let mut state = browsing(&["a", "b", "c"]);
            press(&mut state, Key::Char('j'));
            let cursor = state.tables.cursor;

            let effects = press(&mut state, Key::Char(panel.shortcut()));

            assert_eq!(state.focus, panel);
            assert!(
                effects.is_empty(),
                "'{}' issued {effects:?}",
                panel.shortcut()
            );
            assert!(!state.should_quit, "'{}' quit the app", panel.shortcut());
            assert_eq!(
                state.tables.cursor, cursor,
                "'{}' disturbed the tree",
                panel.shortcut()
            );
        }
    }

    #[test]
    fn a_shifted_shortcut_stays_unbound() {
        for panel in Panel::ALL {
            // Focus starts elsewhere, so an unmoved focus is evidence rather
            // than the state it began in.
            let elsewhere = panel.next();
            let mut state = State {
                focus: elsewhere,
                ..State::default()
            };
            let shifted = panel.shortcut().to_ascii_uppercase();

            press(&mut state, Key::Char(shifted));

            assert_eq!(
                state.focus, elsewhere,
                "'{shifted}' must stay free for a binding of its own"
            );
        }
    }
```

- [ ] **Step 6: Run them and watch them fail**

Run: `cargo test -p lc-domain update::tests::a_shortcut`
Expected: FAIL — focus stays on `Panel::Tables`, since nothing answers `t`.

- [ ] **Step 7: Add the arm to `handle_key`**

In `crates/lc-domain/src/update.rs`, replace the trailing `_ => {}` of the
`match press.key` block (line 228) with:

```rust
        // Last, so every binding above keeps precedence over a panel key.
        Key::Char(key) => {
            if let Some(panel) = Panel::from_shortcut(key) {
                state.focus = panel;
            }
        }
        _ => {}
```

- [ ] **Step 8: Run the crate's tests**

Run: `cargo test -p lc-domain`
Expected: PASS, all of them.

- [ ] **Step 9: Gate and commit**

```bash
mise run ci
git add crates/lc-domain/src/state.rs crates/lc-domain/src/update.rs
git commit -m "feat(domain): focus a panel with the key it claims"
```

---

### Task 2: The title carries where its shortcut is

**Files:**
- Create: `crates/lc-domain/src/view/title.rs`
- Modify: `crates/lc-domain/src/view/mod.rs` (module list at 10-16, `PanelView` at 33-38, `view_model` at 74-84, tests at 159+)
- Modify: `crates/lc-domain/src/view/contents.rs` (test helper `contents_title` at 136-138)

**Interfaces:**
- Consumes: `Panel::shortcut()` from Task 1.
- Produces: `lc_domain::view::PanelTitle` with `text(&self) -> &str` and `split(&self) -> (&str, &str, &str)`. `PanelView::title` changes type from `String` to `PanelTitle`. Task 3 calls `split`.

- [ ] **Step 1: Write the failing tests for the new type**

Create `crates/lc-domain/src/view/title.rs` with the tests and a module doc
only — no implementation yet:

```rust
//! A panel title that knows where its focus shortcut sits.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_shortcut_is_marked_despite_its_case() {
        let title = PanelTitle::marking("Tables".to_owned(), 't');

        assert_eq!(title.split(), ("", "T", "ables"));
    }

    #[test]
    fn a_shortcut_inside_a_word_is_marked_where_it_sits() {
        assert_eq!(
            PanelTitle::marking("Queries".to_owned(), 'u').split(),
            ("Q", "u", "eries")
        );
        assert_eq!(
            PanelTitle::marking("Table DDL".to_owned(), 'a').split(),
            ("T", "a", "ble DDL")
        );
    }

    #[test]
    fn only_the_first_occurrence_is_marked() {
        assert_eq!(
            PanelTitle::marking("Table contents".to_owned(), 't').split(),
            ("", "T", "able contents")
        );
    }

    #[test]
    fn a_title_that_lacks_its_shortcut_draws_unmarked() {
        assert_eq!(
            PanelTitle::marking("Tables".to_owned(), 'z').split(),
            ("Tables", "", "")
        );
    }

    #[test]
    fn a_multibyte_character_before_the_mark_does_not_shift_it() {
        // The Contents title interpolates an em dash, so the mark's index is
        // a byte offset and not a character count.
        let title = PanelTitle::marking("Table contents — shop.orders".to_owned(), 'p');
        let (before, mark, after) = title.split();

        assert_eq!(mark, "p");
        assert_eq!(before, "Table contents — sho");
        assert_eq!(after, ".orders");
    }

    #[test]
    fn the_three_pieces_reassemble_into_the_title() {
        for shortcut in ['c', 'z'] {
            let title = PanelTitle::marking("Table contents — shop.orders".to_owned(), shortcut);
            let (before, mark, after) = title.split();

            assert_eq!(format!("{before}{mark}{after}"), title.text());
        }
    }
}
```

- [ ] **Step 2: Wire the module in and watch it fail**

In `crates/lc-domain/src/view/mod.rs`, add `mod title;` to the module list and
`pub use title::PanelTitle;` to the re-exports.

Run: `cargo test -p lc-domain view::title`
Expected: FAIL — `cannot find type PanelTitle in this scope`.

- [ ] **Step 3: Implement the type**

Above the test module in `crates/lc-domain/src/view/title.rs`:

```rust
use std::ops::Range;

/// A panel title and the position of the key that focuses the panel.
///
/// The range is private for the reason [`crate::state::Cursor`]'s index is: it
/// is checked once, here, so no renderer can be handed one that slices a
/// character in half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTitle {
    text: String,
    mark: Option<Range<usize>>,
}

impl PanelTitle {
    /// `text` with its first case-insensitive `shortcut` marked.
    ///
    /// A title that does not contain its own shortcut draws unmarked rather
    /// than failing; `view::tests` pins that case shut for the panels that
    /// exist.
    pub(crate) fn marking(text: String, shortcut: char) -> Self {
        let mark = text
            .char_indices()
            .find(|(_, character)| character.eq_ignore_ascii_case(&shortcut))
            .map(|(index, character)| index..index + character.len_utf8());

        Self { text, mark }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The title in three pieces: what precedes the shortcut, the shortcut,
    /// and what follows it. The middle piece is empty for an unmarked title.
    #[must_use]
    pub fn split(&self) -> (&str, &str, &str) {
        let Some(mark) = self.mark.clone() else {
            return (&self.text, "", "");
        };

        (
            &self.text[..mark.start],
            &self.text[mark.start..mark.end],
            &self.text[mark.end..],
        )
    }
}
```

- [ ] **Step 4: Run the tests and watch them pass**

Run: `cargo test -p lc-domain view::title`
Expected: PASS, six tests.

- [ ] **Step 5: Give `PanelView` the new type**

In `crates/lc-domain/src/view/mod.rs`:

```rust
pub struct PanelView {
    pub panel: Panel,
    pub title: PanelTitle,
    pub body: PanelBody,
}
```

and in `view_model`:

```rust
        panels: Panel::ALL.map(|panel| PanelView {
            panel,
            title: PanelTitle::marking(title(state, panel), panel.shortcut()),
            body: body(state, panel),
        }),
```

The private `fn title(state, panel) -> String` keeps its name and body.

- [ ] **Step 6: Fix the two tests that read a title as a `String`**

In `crates/lc-domain/src/view/mod.rs`, `panels_carry_the_titles_the_draft_names`:

```rust
        assert_eq!(
            view.panels.map(|panel| panel.title.text().to_owned()),
            ["Tables", "Table DDL", "Table contents", "Queries"].map(str::to_owned)
        );
```

In `crates/lc-domain/src/view/contents.rs`, the `contents_title` helper:

```rust
    fn contents_title(state: &State) -> String {
        view_model(state).panels[2].title.text().to_owned()
    }
```

- [ ] **Step 7: Write the failing invariant test**

Add to `mod tests` in `crates/lc-domain/src/view/mod.rs`. It needs
`use crate::contents::TableRef;`, `use crate::model::{DbError, Page, QueryId};`
and `use super::tests_support::tabled;` alongside the existing imports:

```rust
    /// Every `ContentsState`, because that panel's title is built per frame
    /// and is the one that could drift out of the invariant below.
    fn every_contents_state() -> Vec<State> {
        let query = QueryId::next();
        let mut loading = tabled(&[("shop", &["orders"])]);
        loading
            .contents
            .requested(query, TableRef::new("shop", "orders"));

        let mut loaded = loading.clone();
        loaded.contents.page_ready(query, Page::default());

        let mut failed = loading.clone();
        failed.contents.failed(query, DbError::Timeout);

        vec![State::default(), loading, loaded, failed]
    }

    #[test]
    fn every_panel_title_marks_the_key_that_focuses_it() {
        for state in every_contents_state() {
            for panel in view_model(&state).panels {
                let (_, mark, _) = panel.title.split();

                assert!(
                    mark.eq_ignore_ascii_case(&panel.panel.shortcut().to_string()),
                    "{:?} titled {:?} does not mark '{}'",
                    panel.panel,
                    panel.title.text(),
                    panel.panel.shortcut()
                );
            }
        }
    }
```

- [ ] **Step 8: Run the crate's tests**

Run: `cargo test -p lc-domain`
Expected: PASS. `lc-inbound` does not compile yet — `render.rs` still passes
`panel.title.as_str()` to `Block::title`. Task 3 fixes it; do not touch it
here.

- [ ] **Step 9: Make the workspace compile again**

In `crates/lc-inbound/src/render.rs:22`, swap the one expression:

```rust
        let mut block = Block::bordered().title(panel.title.text());
```

Run: `cargo test --workspace`
Expected: PASS. Nothing is underlined yet; that is Task 3.

- [ ] **Step 10: Gate and commit**

```bash
mise run ci
git add crates/lc-domain/src/view crates/lc-inbound/src/render.rs
git commit -m "feat(domain): mark the focus shortcut inside a panel title"
```

---

### Task 3: Draw the underline

**Files:**
- Modify: `crates/lc-domain/src/style.rs` (`Token` at 11-22)
- Modify: `crates/lc-inbound/src/theme.rs` (`terminal_default` at 27-54, tests at 57+)
- Modify: `crates/lc-inbound/src/render.rs` (`render` at 13-35, tests at 37+)

**Interfaces:**
- Consumes: `PanelTitle::split()` from Task 2.
- Produces: `Token::Shortcut`, mapped by `Theme::TerminalDefault` to `Modifier::UNDERLINED` and nothing else.

- [ ] **Step 1: Write the failing theme test**

Add to `mod tests` in `crates/lc-inbound/src/theme.rs`, and add
`Token::Shortcut` to the `tokens` array in `no_token_sets_a_background`:

```rust
    #[test]
    fn a_shortcut_is_underlined_without_spending_a_colour() {
        let style = Theme::TerminalDefault.style(Token::Shortcut);

        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(
            style.fg, None,
            "every colour in this theme already carries a meaning"
        );
    }
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p lc-inbound theme`
Expected: FAIL — `no variant named Shortcut found for enum Token`.

- [ ] **Step 3: Add the token and its mapping**

In `crates/lc-domain/src/style.rs`, inside `enum Token`, after `Focused`:

```rust
    /// The key that focuses a panel, inside that panel's title.
    Shortcut,
```

In `crates/lc-inbound/src/theme.rs`, inside `terminal_default`:

```rust
            // Underlined rather than coloured: it labels a key, and it has to
            // stay readable beside a title that is bold when focused.
            Token::Shortcut => Style::new().add_modifier(Modifier::UNDERLINED),
```

- [ ] **Step 4: Run the theme tests and watch them pass**

Run: `cargo test -p lc-inbound theme`
Expected: PASS.

- [ ] **Step 5: Write the failing render test**

Add to `mod tests` in `crates/lc-inbound/src/render.rs`. The coordinates come
from the rows asserted in `the_four_panels_are_drawn_where_the_draft_puts_them`:

```rust
    #[test]
    fn each_panel_underlines_the_key_that_focuses_it() {
        let buffer = draw(40, 12);
        let underlined = |x, y| {
            buffer
                .cell((x, y))
                .expect("cell is in bounds")
                .modifier
                .contains(Modifier::UNDERLINED)
        };

        // "┌Tables" and "┌Table contents" on row 0, "┌Table DDL" on row 8,
        // "┌Queries" on row 9 — the border occupies the first column of each.
        assert!(underlined(1, 0), "Tables underlines its T");
        assert!(underlined(23, 0), "Table contents underlines its c");
        assert!(underlined(2, 8), "Table DDL underlines its a");
        assert!(underlined(18, 9), "Queries underlines its u");
    }

    #[test]
    fn nothing_but_the_shortcut_is_underlined() {
        let buffer = draw(40, 12);
        let underlined = |x, y| {
            buffer
                .cell((x, y))
                .expect("cell is in bounds")
                .modifier
                .contains(Modifier::UNDERLINED)
        };

        for x in [0, 2, 3, 6] {
            assert!(!underlined(x, 0), "column {x} of row 0 is not the mark");
        }
        assert!(!underlined(1, 8), "the T of Table DDL is not its mark");
    }
```

- [ ] **Step 6: Run them and watch them fail**

Run: `cargo test -p lc-inbound render::tests::each_panel_underlines`
Expected: FAIL — the assertion at `(1, 0)`, since the title is drawn as one
unstyled string.

- [ ] **Step 7: Draw the title as three spans**

In `crates/lc-inbound/src/render.rs`, add to the imports:

```rust
use ratatui::text::{Line, Span};
```

and replace line 22 with:

```rust
        let (before, shortcut, after) = panel.title.split();
        let title = Line::from(vec![
            Span::raw(before),
            Span::styled(shortcut, theme.style(Token::Shortcut)),
            Span::raw(after),
        ]);

        let mut block = Block::bordered().title(title);
```

- [ ] **Step 8: Run the crate's tests**

Run: `cargo test -p lc-inbound`
Expected: PASS, including the untouched row-string and focus assertions — the
text does not move, only the styling of one cell per panel.

- [ ] **Step 9: Gate and commit**

```bash
mise run ci
git add crates/lc-domain/src/style.rs crates/lc-inbound/src/theme.rs crates/lc-inbound/src/render.rs
git commit -m "feat(inbound): underline the key that focuses each panel"
```

---

### Task 4: Record the decision

**Files:**
- Modify: `doc/specs/2026-08-09-panel-focus-shortcuts-design.md` (header)

- [ ] **Step 1: Mark the spec implemented**

Change the header line to `**Status:** implemented`.

- [ ] **Step 2: Commit**

```bash
git add doc/specs/2026-08-09-panel-focus-shortcuts-design.md
git commit -m "docs: mark the panel focus shortcut design implemented"
```
