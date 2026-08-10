# Panel search implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `/` opens a prompt on the hint bar row whose query filters Tables and Table DDL and highlights matching Table contents cells, live, staying applied until `esc` clears it.

**Architecture:** A `Filter` newtype in `lc-domain` whose only invariant is non-emptiness, stored per panel beside the state it filters. The prompt is a third `Overlay` variant, so it inherits the "an overlay outranks focus" precedence the command palette already established. Tables filters inside `TablesPanel::rows` — the single flattening both motions and the view model read — and Table DDL filters inside its projection. Table contents flags cells and the renderer paints them.

**Tech Stack:** Rust workspace, no new dependencies. `lc-domain` (pure, no tokio/ratatui/crossterm), `lc-inbound` (ratatui rendering + theme).

**Spec:** [doc/specs/2026-08-10-panel-search-design.md](../specs/2026-08-10-panel-search-design.md). Every task cites the section it implements. Read the section before writing the code.

## Global Constraints

- `unsafe_code = "forbid"`. No `unsafe`, no FFI.
- `lc-domain` must never depend on `tokio`, `ratatui` or `crossterm`.
- Zero warnings, zero failing or ignored tests. `mise run ci` must be clean before a task is done.
- Any `#[allow(...)]` must be item-scoped and carry a reason.
- No narrating comments. Comments are for edge cases, past bugs, upstream defects, and Cassandra/protocol semantics the code cannot express. Before commenting: rename, extract a named function, or introduce a type.
- Determinism: no wall-clock, no network, no unseeded randomness, no `HashMap` iteration order in the fast tiers.
- Conventional Commits, imperative, lowercase, no trailing period. Reference the spec section the change follows from.
- Never `git pull`, never `git push`. Committing is expected.
- Doc comments on public items explain **why**, in the style of the surrounding code — see `state.rs`'s `Overlay` and `Cursor` for the register.

**Commands:**

```
mise run build   # cargo build --workspace
mise run fmt     # cargo fmt --all
mise run check   # fmt, clippy -D warnings, licences, advisories
mise run test    # fast tiers (domain, scenario, render)
mise run ci      # everything CI gates on
```

Single test: `cargo test -p lc-domain <test_name>` / `cargo test -p lc-inbound <test_name>`.

---

## File Structure

**Created:**

| File | Responsibility |
|---|---|
| `crates/lc-domain/src/search.rs` | `Filter`, `Filtered`, `Prompt`. The whole vocabulary of searching. |
| `crates/lc-domain/src/ddl.rs` | `DdlPanel` — the Table DDL panel's own state (scroll + filter). |

**Modified:**

| File | Change |
|---|---|
| `crates/lc-domain/src/lib.rs` | Declare `search` and `ddl`. |
| `crates/lc-domain/src/state.rs` | `Overlay::Search`, `state.ddl: DdlPanel`, `State::filter`/`apply_filter`, `SEARCH` hints. |
| `crates/lc-domain/src/tree.rs` | `TablesPanel::filter`, filtered flattening. |
| `crates/lc-domain/src/contents.rs` | `ContentsPanel::filter`. |
| `crates/lc-domain/src/update.rs` | `/` arm, `search_key`, `escape` chain, `transition` guard, `state.ddl.scroll` rename. |
| `crates/lc-domain/src/view/mod.rs` | `PanelView::filter`, `OverlayView::Search`. |
| `crates/lc-domain/src/view/tables.rs` | Nothing-matched notice. |
| `crates/lc-domain/src/view/details.rs` | Row filtering, nothing-matched notice, `state.ddl.scroll`. |
| `crates/lc-domain/src/view/contents.rs` | `GridCell::matched`. |
| `crates/lc-domain/src/view/hints.rs` | `SEARCH_GLOBAL`, `esc clear`. |
| `crates/lc-domain/src/commands.rs` | `/ search` in the catalogue. |
| `crates/lc-domain/src/style.rs` | `Token::Match`. |
| `crates/lc-inbound/src/theme.rs` | `Token::Match` style. |
| `crates/lc-inbound/src/render.rs` | Filter marker on the top border, prompt on the bar row. |
| `crates/lc-inbound/src/body.rs` | Paint `GridCell::matched`. |
| `doc/shortcuts.md` | §1, §2, §3, §5, §7. |

---

## Task 1: `Filter` and `Filtered`

Implements spec §2 and §2.1. Pure vocabulary, no consumers yet.

**Files:**
- Create: `crates/lc-domain/src/search.rs`
- Modify: `crates/lc-domain/src/lib.rs`

**Interfaces:**
- Consumes: `crate::state::Panel`.
- Produces:
  - `Filter::new(&str) -> Option<Filter>`, `Filter::matches(&self, &str) -> bool`, `Filter::as_str(&self) -> &str`
  - `Filtered::{Tables, Ddl, Contents}`, `Filtered::of(Panel) -> Option<Filtered>`, `Filtered::panel(self) -> Panel`
  - Both `Debug + Clone + PartialEq + Eq`; `Filtered` also `Copy`.

- [ ] **Step 1: Write the failing tests**

Create `crates/lc-domain/src/search.rs` with the tests only (no `impl` yet), so the compile failure is the first red:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_is_not_a_filter() {
        assert_eq!(Filter::new(""), None, "an empty filter is what None means");
    }

    #[test]
    fn matching_ignores_case_in_both_directions() {
        let filter = Filter::new("ORD").expect("non-empty");

        assert!(filter.matches("orders"));
        assert!(filter.matches("ORDERS"));
        assert!(Filter::new("ord").expect("non-empty").matches("ORDERS"));
    }

    #[test]
    fn matching_is_a_substring_anywhere_and_not_a_prefix() {
        let filter = Filter::new("by").expect("non-empty");

        assert!(filter.matches("orders_by_customer"));
        assert!(!filter.matches("orders"));
    }

    #[test]
    fn a_metacharacter_is_matched_literally() {
        assert!(Filter::new(".*").expect("non-empty").matches("a.*b"));
        assert!(!Filter::new(".*").expect("non-empty").matches("anything"));
    }

    #[test]
    fn folding_reaches_past_ascii() {
        // Cell values are whatever the cluster stored, so `to_lowercase`
        // rather than `to_ascii_lowercase` (design §2).
        assert!(Filter::new("é").expect("non-empty").matches("CAFÉ"));
    }

    #[test]
    fn the_term_is_kept_as_typed_for_the_marker() {
        assert_eq!(Filter::new("ORD").expect("non-empty").as_str(), "ORD");
    }

    #[test]
    fn every_panel_but_queries_can_hold_a_filter() {
        assert_eq!(Filtered::of(Panel::Tables), Some(Filtered::Tables));
        assert_eq!(Filtered::of(Panel::Ddl), Some(Filtered::Ddl));
        assert_eq!(Filtered::of(Panel::Contents), Some(Filtered::Contents));
        assert_eq!(Filtered::of(Panel::Queries), None);
    }

    #[test]
    fn a_target_names_the_panel_it_came_from() {
        for panel in Panel::ALL {
            if let Some(target) = Filtered::of(panel) {
                assert_eq!(target.panel(), panel);
            }
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p lc-domain search::`
Expected: compile error — `cannot find type Filter in this scope` (the module is not declared yet, so it may not even be compiled; add `mod search;` to `lib.rs` first if so, then expect the same error).

- [ ] **Step 3: Write the implementation**

Prepend to `crates/lc-domain/src/search.rs`:

```rust
//! Searching a panel: the term, where it can be applied, and the prompt that
//! collects it.
//!
//! doc/specs/2026-08-10-panel-search-design.md.

use crate::state::Panel;

/// A search term that is applied to a panel.
///
/// The invariant is non-emptiness, which is what makes `Option<Filter>` mean
/// "no filter" — an empty query clears rather than matching everything, and
/// that follows from [`Filter::new`] rather than from a check anyone has to
/// remember (design §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    term: String,
    folded: String,
}

impl Filter {
    #[must_use]
    pub fn new(raw: &str) -> Option<Self> {
        if raw.is_empty() {
            return None;
        }

        Some(Self {
            term: raw.to_owned(),
            folded: raw.to_lowercase(),
        })
    }

    /// Case-insensitive substring, with no metacharacters: every keystroke of
    /// a live prompt is literal, so no half-typed query is ever an error state
    /// the panel underneath has to render (design §2).
    #[must_use]
    pub fn matches(&self, haystack: &str) -> bool {
        haystack.to_lowercase().contains(&self.folded)
    }

    /// As typed, which is what the panel's marker shows (design §5.1).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.term
    }
}

/// A panel that can hold a filter.
///
/// Queries cannot, until it has a normal mode to reach a prompt from (design
/// §8). Keying a filter off [`Panel`] instead would leave an unreachable
/// `Panel::Queries` arm in every accessor; this makes them total, and makes
/// the compiler list them when that slice lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filtered {
    Tables,
    Ddl,
    Contents,
}

impl Filtered {
    #[must_use]
    pub const fn of(panel: Panel) -> Option<Self> {
        match panel {
            Panel::Tables => Some(Self::Tables),
            Panel::Ddl => Some(Self::Ddl),
            Panel::Contents => Some(Self::Contents),
            Panel::Queries => None,
        }
    }

    #[must_use]
    pub const fn panel(self) -> Panel {
        match self {
            Self::Tables => Panel::Tables,
            Self::Ddl => Panel::Ddl,
            Self::Contents => Panel::Contents,
        }
    }
}
```

In `crates/lc-domain/src/lib.rs`, declare the module next to the other panel-state modules (`pub mod search;`), matching how `tree`/`contents` are declared there.

- [ ] **Step 4: Run tests**

Run: `cargo test -p lc-domain search::`
Expected: PASS, 7 tests.

- [ ] **Step 5: Full gate and commit**

```bash
mise run ci
git add crates/lc-domain/src/search.rs crates/lc-domain/src/lib.rs
git commit -m "feat(domain): add the search filter and the panels that can hold one

Non-emptiness is the invariant, so Option<Filter> is what \"no filter\"
means and an empty query clears rather than matching everything.
Filtered keeps the accessors in the next commit total, Queries having
no filter until it has a normal mode (design §2, §2.1, §8)."
```

---

## Task 2: Where a filter lives

Implements spec §2.2 and the `State` accessors from §2.1. Storage only — nothing reads a filter yet.

**Files:**
- Create: `crates/lc-domain/src/ddl.rs`
- Modify: `crates/lc-domain/src/lib.rs`, `crates/lc-domain/src/state.rs`, `crates/lc-domain/src/tree.rs`, `crates/lc-domain/src/contents.rs`
- Mechanical: `crates/lc-domain/src/update.rs`, `crates/lc-domain/src/view/details.rs`, `crates/lc-inbound/src/render.rs` (the `state.ddl` → `state.ddl.scroll` rename)

**Interfaces:**
- Consumes: `Filter`, `Filtered` from Task 1.
- Produces:
  - `DdlPanel { pub scroll: Scroll, pub filter: Option<Filter> }`, `Debug + Clone + Default + PartialEq + Eq`
  - `TablesPanel.filter: Option<Filter>` (public field, like `cursor` and `scroll`)
  - `ContentsPanel.filter: Option<Filter>` (public field)
  - `State::filter(&self, Filtered) -> Option<&Filter>`
  - `State::apply_filter(&mut self, Filtered, Option<Filter>)` — sets it; the Tables clamp lands in Task 4.

- [ ] **Step 1: Write the failing tests**

In `crates/lc-domain/src/state.rs`'s `mod tests`:

```rust
#[test]
fn a_filter_is_read_back_from_the_panel_it_was_applied_to() {
    let mut state = State::default();

    state.apply_filter(Filtered::Ddl, Filter::new("ttl"));

    assert_eq!(state.filter(Filtered::Ddl).map(Filter::as_str), Some("ttl"));
    assert_eq!(state.filter(Filtered::Tables), None);
    assert_eq!(state.filter(Filtered::Contents), None);
}

#[test]
fn the_three_filters_are_independent() {
    let mut state = State::default();

    state.apply_filter(Filtered::Tables, Filter::new("ord"));
    state.apply_filter(Filtered::Contents, Filter::new("null"));

    assert_eq!(state.filter(Filtered::Tables).map(Filter::as_str), Some("ord"));
    assert_eq!(state.filter(Filtered::Contents).map(Filter::as_str), Some("null"));
}

#[test]
fn applying_none_clears_the_one_that_was_there() {
    let mut state = State::default();
    state.apply_filter(Filtered::Tables, Filter::new("ord"));

    state.apply_filter(Filtered::Tables, None);

    assert_eq!(state.filter(Filtered::Tables), None);
}
```

Add `use crate::search::{Filter, Filtered};` to that test module.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p lc-domain state::tests::a_filter_is_read_back`
Expected: compile error — `no method named apply_filter found for struct State`.

- [ ] **Step 3: Write the implementation**

Create `crates/lc-domain/src/ddl.rs`:

```rust
//! The Table DDL panel's state: a viewport over the selected node's
//! description, and the filter narrowing it.
//!
//! A module of its own for the reason [`crate::tree`] and [`crate::contents`]
//! are ones — per-panel state with its own vocabulary rather than another
//! field on `State` (architecture.md §11).

use crate::search::Filter;
use crate::state::Scroll;

/// The Table DDL panel's own state.
///
/// It scrolls a viewport rather than a selection: it has no row to act on, so
/// there is nothing for a cursor to mean.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DdlPanel {
    pub scroll: Scroll,
    pub filter: Option<Filter>,
}
```

Declare `pub mod ddl;` in `lib.rs`.

In `state.rs`: change the `ddl` field to `pub ddl: DdlPanel`, moving its existing doc comment onto `DdlPanel` (it is already reproduced above — delete it from the field rather than duplicating it). Import `crate::ddl::DdlPanel` and `crate::search::{Filter, Filtered}`.

Add to `impl State`:

```rust
/// The filter applied to `target`, if any.
#[must_use]
pub const fn filter(&self, target: Filtered) -> Option<&Filter> {
    match target {
        Filtered::Tables => self.tables.filter.as_ref(),
        Filtered::Ddl => self.ddl.filter.as_ref(),
        Filtered::Contents => self.contents.filter.as_ref(),
    }
}

/// Applies `filter` to `target` and repairs what it invalidates.
///
/// Not a `&mut Option<Filter>` handed out: a caller that sets the filter and
/// forgets the repair leaves a cursor pointing past the rows the panel now
/// has, and there is no reason for that to be a caller's job (design §3.3).
pub fn apply_filter(&mut self, target: Filtered, filter: Option<Filter>) {
    match target {
        Filtered::Tables => self.tables.filter = filter,
        Filtered::Ddl => self.ddl.filter = filter,
        Filtered::Contents => self.contents.filter = filter,
    }
}
```

If `const fn` does not compile for `filter` (`Option::as_ref` is not const on the pinned toolchain), drop the `const` — do not add an allow.

In `tree.rs`, add to `TablesPanel`:

```rust
/// Narrows [`TablesPanel::rows`]. Public like `cursor` and `scroll`: the
/// invariant that mattered is inside [`Filter`] already.
pub filter: Option<Filter>,
```

In `contents.rs`, add the same field to `ContentsPanel` with a one-line doc.

Then the mechanical rename, `state.ddl` → `state.ddl.scroll`, at every site listed by:

```bash
grep -rn "\.ddl\b" crates --include "*.rs" | grep -v "ddl_lines\|Panel::Ddl\|mod ddl\|ddl::"
```

Two sites need thought rather than a rename:
- `update.rs:604-609` — `let before = state.ddl;` compares the whole panel. `Scroll` is `Copy`, `DdlPanel` is not. Change to `let before = state.ddl.scroll;` and `if state.ddl.scroll != before`.
- `update.rs:641` — `panel_scroll` returns `Option<&mut Scroll>`; the arm becomes `Panel::Ddl => Some(&mut state.ddl.scroll)`.

- [ ] **Step 4: Run the tests**

Run: `mise run test`
Expected: PASS, including the three new ones. Every pre-existing test must still pass — the rename changes no behaviour.

- [ ] **Step 5: Full gate and commit**

```bash
mise run ci
git add -A
git commit -m "feat(domain): give each searchable panel a filter to hold

Table DDL grows a panel struct where it had a bare Scroll, for the
reason tree.rs and contents.rs exist: per-panel state with its own
vocabulary (architecture.md §11). apply_filter rather than a mutable
handle, so the repair a filter owes cannot be forgotten by a caller
(design §2.1, §2.2)."
```

---

## Task 3: Tables filters, and forces its matches open

Implements spec §4.1's retention table. `TablesPanel::rows` is the one flattening, so this is the only place the tree is narrowed.

**Files:**
- Modify: `crates/lc-domain/src/tree.rs`

**Interfaces:**
- Consumes: `TablesPanel.filter` (Task 2), `Filter::matches` (Task 1).
- Produces: `TablesPanel::rows(&self, &Schema) -> Vec<Node<'_>>` — same signature, now filter-aware. No new public API.

- [ ] **Step 1: Write the failing tests**

In `tree.rs`'s `mod tests`. `tests_support::schema_of` builds `&[(keyspace, &[table])]`:

```rust
fn shop_and_logs() -> Schema {
    tests_support::schema_of(&[
        ("shop", &["orders", "users"][..]),
        ("logs", &["order_events", "spans"][..]),
    ])
}

fn labels(panel: &TablesPanel, schema: &Schema) -> Vec<String> {
    panel
        .rows(schema)
        .iter()
        .map(|node| match node {
            Node::Keyspace { keyspace, .. } => keyspace.name.clone(),
            Node::Table { table, .. } => format!("  {}", table.name),
        })
        .collect()
}

#[test]
fn a_matching_table_brings_its_keyspace_and_no_sibling() {
    let schema = shop_and_logs();
    let panel = TablesPanel {
        filter: Filter::new("order"),
        ..TablesPanel::default()
    };

    assert_eq!(
        labels(&panel, &schema),
        ["shop", "  orders", "logs", "  order_events"],
        "users and spans miss, and their keyspaces survive on their children"
    );
}

#[test]
fn a_matching_keyspace_brings_all_of_its_tables() {
    let schema = shop_and_logs();
    let panel = TablesPanel {
        filter: Filter::new("shop"),
        ..TablesPanel::default()
    };

    assert_eq!(labels(&panel, &schema), ["shop", "  orders", "  users"]);
}

#[test]
fn a_keyspace_with_nothing_matching_is_dropped() {
    let schema = shop_and_logs();
    let panel = TablesPanel {
        filter: Filter::new("zzz"),
        ..TablesPanel::default()
    };

    assert!(panel.rows(&schema).is_empty());
}

#[test]
fn a_retained_keyspace_is_drawn_open_whatever_its_fold_state() {
    let schema = shop_and_logs();
    let mut panel = TablesPanel::default();

    // Folded: the default. Without the filter it shows two keyspaces.
    assert_eq!(panel.rows(&schema).len(), 2);

    panel.filter = Filter::new("order");

    assert!(
        matches!(
            panel.rows(&schema).first(),
            Some(Node::Keyspace { unfolded: true, .. })
        ),
        "a filter that hid its own matches would be no filter at all"
    );
}

#[test]
fn clearing_the_filter_restores_the_fold_state_exactly() {
    let schema = shop_and_logs();
    let mut panel = TablesPanel::default();
    panel.unfold_for_test("logs", &schema);
    let before = panel.rows(&schema);

    panel.filter = Filter::new("order");
    let _ = panel.rows(&schema);
    panel.filter = None;

    assert_eq!(
        panel.rows(&schema),
        before,
        "filtering is a projection; it must not write the unfold set"
    );
}

#[test]
fn a_keyspace_matching_by_name_is_open_only_when_it_has_tables() {
    let schema = tests_support::schema_of(&[("empty", &[][..])]);
    let panel = TablesPanel {
        filter: Filter::new("empty"),
        ..TablesPanel::default()
    };

    assert!(matches!(
        panel.rows(&schema).as_slice(),
        [Node::Keyspace { unfolded: false, .. }]
    ));
}
```

`unfold_for_test` does not exist. Use the real transitions instead — select the keyspace and call `open`:

```rust
fn unfolded_on(name: &str, schema: &Schema) -> TablesPanel {
    let mut panel = TablesPanel::default();
    panel.select_keyspace_for_rows(name, schema);
    panel.open(schema);
    panel
}
```

`select_keyspace` is private. Simplest honest route: walk the cursor with `rows` and `cursor.go_to`, which the test module can do:

```rust
fn unfolded_on(name: &str, schema: &Schema) -> TablesPanel {
    let mut panel = TablesPanel::default();
    let index = panel
        .rows(schema)
        .iter()
        .position(|node| matches!(node, Node::Keyspace { keyspace, .. } if keyspace.name == name))
        .expect("the keyspace is in the schema");
    panel.cursor.go_to(index, panel.rows(schema).len());
    panel.open(schema);
    panel
}
```

Use `unfolded_on("logs", &schema)` in the restore test instead of `unfold_for_test`.

Add `use crate::search::Filter;` to the test module.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p lc-domain tree::tests`
Expected: the four filter tests FAIL — `rows` ignores the filter, so `a_matching_table_brings_its_keyspace_and_no_sibling` returns all four keyspace/table rows unfolded-by-default (two rows).

- [ ] **Step 3: Write the implementation**

Replace `TablesPanel::rows` in `tree.rs`:

```rust
/// The tree, flattened in display order: user keyspaces first, then the
/// cluster's own, each with its tables spliced in when unfolded.
///
/// One flattening, so `update`'s motions and `view_model` cannot disagree
/// about what row 5 is — which is why the filter is applied here and not in
/// the projection (design §4.1).
#[must_use]
pub fn rows<'a>(&self, schema: &'a Schema) -> Vec<Node<'a>> {
    match &self.filter {
        Some(filter) => self.filtered(schema, filter),
        None => self.whole(schema),
    }
}

fn whole<'a>(&self, schema: &'a Schema) -> Vec<Node<'a>> {
    let mut rows = Vec::new();

    for keyspace in schema.keyspaces_in_display_order() {
        let unfolded = self.is_unfolded(keyspace);
        rows.push(Node::Keyspace { keyspace, unfolded });

        if unfolded {
            rows.extend(
                keyspace
                    .tables
                    .values()
                    .map(|table| Node::Table { keyspace, table }),
            );
        }
    }

    rows
}

/// A keyspace survives on its own name or on a table's, and is drawn open
/// either way: a match inside a folded keyspace would make the panel report
/// nothing while the row sits one keypress away.
///
/// The unfold set is not written. Clearing the filter therefore restores the
/// fold state exactly, and a live prompt cannot accumulate an unfold per
/// prefix as the term is typed (design §4.1).
fn filtered<'a>(&self, schema: &'a Schema, filter: &Filter) -> Vec<Node<'a>> {
    let mut rows = Vec::new();

    for keyspace in schema.keyspaces_in_display_order() {
        let named = filter.matches(&keyspace.name);
        let tables: Vec<_> = keyspace
            .tables
            .values()
            .filter(|table| named || filter.matches(&table.name))
            .collect();

        if !named && tables.is_empty() {
            continue;
        }

        rows.push(Node::Keyspace {
            keyspace,
            unfolded: !tables.is_empty(),
        });
        rows.extend(tables.into_iter().map(|table| Node::Table { keyspace, table }));
    }

    rows
}
```

Import `crate::search::Filter` at the top of `tree.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p lc-domain tree::`
Expected: PASS. Then `mise run test` — every existing Tables test must still pass, since `filter` defaults to `None` and `whole` is the old body verbatim.

- [ ] **Step 5: Commit**

```bash
mise run ci
git add crates/lc-domain/src/tree.rs
git commit -m "feat(domain): filter the tables tree, forcing matches open

Applied in rows(), the one flattening motions and the view model share,
so a filtered panel cannot disagree with itself about what row 5 is. A
retained keyspace draws open without the unfold set being written, so
clearing restores the fold state exactly (design §4.1)."
```

---

## Task 4: Tables answers for what the filter did

Implements the rest of spec §4.1 (`l`/`h` swallowed, cursor clamp) and §4.4's Tables notice.

**Files:**
- Modify: `crates/lc-domain/src/state.rs` (the clamp inside `apply_filter`), `crates/lc-domain/src/update.rs`, `crates/lc-domain/src/view/tables.rs`

**Interfaces:**
- Consumes: `TablesPanel::rows` (Task 3), `State::apply_filter` (Task 2).
- Produces: no new API. `apply_filter(Filtered::Tables, _)` now clamps `state.tables.cursor`.

- [ ] **Step 1: Write the failing tests**

In `update.rs`'s `mod tests` (use whatever helper the file already uses to build a connected, tabled state — `tabled(&[("shop", &["orders", "users"])])` in the view tests; `update.rs` has its own equivalents, follow them):

```rust
#[test]
fn l_and_h_are_swallowed_while_a_filter_is_applied() {
    let mut state = tabled_state(&[("shop", &["orders", "users"])]);
    state.focus = Panel::Tables;
    state.apply_filter(Filtered::Tables, Filter::new("shop"));
    let before = state.tables.rows(state.schema().expect("loaded")).len();

    press(&mut state, Key::Char('h'));
    press(&mut state, Key::Char('l'));

    assert_eq!(
        state.tables.rows(state.schema().expect("loaded")).len(),
        before,
        "a filter forces its matches open, so folding could only look broken"
    );
}

#[test]
fn l_and_h_still_fold_once_the_filter_is_gone() {
    let mut state = tabled_state(&[("shop", &["orders", "users"])]);
    state.focus = Panel::Tables;

    press(&mut state, Key::Char('l'));

    assert_eq!(state.tables.rows(state.schema().expect("loaded")).len(), 3);
}

#[test]
fn the_cursor_clamps_as_a_filter_shrinks_the_tree() {
    let mut state = tabled_state(&[("shop", &["orders", "users"])]);
    state.focus = Panel::Tables;
    press(&mut state, Key::Char('l'));
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));
    assert_eq!(state.tables.cursor.index(), 2, "on users");

    state.apply_filter(Filtered::Tables, Filter::new("orders"));

    assert_eq!(
        state.tables.cursor.index(),
        1,
        "two rows survive, so the last one is index 1"
    );
}
```

In `view/tables.rs`'s `mod tests`:

```rust
#[test]
fn a_filter_that_matches_nothing_says_so() {
    let mut state = tabled(&[("shop", &["orders"])]);
    state.apply_filter(Filtered::Tables, Filter::new("zzz"));

    assert_eq!(
        tables_body(&state),
        PanelBody::Notice {
            text: "No keyspace or table matches \"zzz\"".to_owned(),
            token: Some(Token::Muted),
        }
    );
}

#[test]
fn an_unfiltered_empty_schema_still_says_no_keyspaces() {
    assert_eq!(
        tables_body(&loaded(&[])),
        PanelBody::Notice {
            text: "No keyspaces".to_owned(),
            token: Some(Token::Muted),
        }
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p lc-domain l_and_h_are_swallowed the_cursor_clamps_as_a_filter a_filter_that_matches_nothing`
Expected: FAIL — `h` still folds, the cursor stays at 2, and the notice reads `No keyspaces`.

- [ ] **Step 3: Write the implementation**

In `state.rs`, extend `apply_filter`'s Tables arm to repair the cursor. It needs the schema, which `State` has:

```rust
pub fn apply_filter(&mut self, target: Filtered, filter: Option<Filter>) {
    match target {
        Filtered::Tables => {
            self.tables.filter = filter;
            if let Some(schema) = self.schema() {
                let rows = self.tables.rows(schema).len();
                self.tables.cursor.clamp_to(rows);
            }
        }
        Filtered::Ddl => self.ddl.filter = filter,
        Filtered::Contents => self.contents.filter = filter,
    }
}
```

`self.schema()` borrows `self` immutably while `self.tables` is borrowed mutably. Sidestep by computing the count first:

```rust
Filtered::Tables => {
    self.tables.filter = filter;
    let rows = self
        .schema()
        .map(|schema| self.tables.rows(schema).len());
    if let Some(rows) = rows {
        self.tables.cursor.clamp_to(rows);
    }
}
```

The Table DDL offset is deliberately not clamped here — `Msg::Measured` already repairs it against a real, wrapped line count, and clamping against the previous frame's count would be a guess that is wrong exactly when the filter just shrank the body (design §3.3). Say so in a comment on the `Filtered::Ddl` arm.

In `update.rs`, guard `transition`:

```rust
fn transition(state: &mut State, transition: Transition) {
    if state.focus != Panel::Tables {
        return;
    }

    // A filter forces every retained keyspace open (design §4.1), so folding
    // would change nothing on screen. Swallowed rather than half-honoured:
    // doc/shortcuts.md §2, a panel that cannot honour a motion swallows it.
    if state.tables.filter.is_some() {
        return;
    }

    // …existing body
}
```

In `view/tables.rs`, replace the empty-rows branch of `tree`:

```rust
fn tree(tables: &TablesPanel, schema: &Schema) -> PanelBody {
    let rows: Vec<TreeRow> = tables.rows(schema).iter().map(tree_row).collect();

    if rows.is_empty() {
        return match &tables.filter {
            Some(filter) => waiting(&format!(
                "No keyspace or table matches \"{}\"",
                filter.as_str()
            )),
            None => waiting("No keyspaces"),
        };
    }

    PanelBody::Tree { /* unchanged */ }
}
```

- [ ] **Step 4: Run the tests**

Run: `mise run test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
mise run ci
git add -A
git commit -m "feat(domain): answer for what a tables filter did

l/h are swallowed while a filter is applied, because a forced-open
keyspace cannot honour them and a key that looks broken is worse than
one that is inert. The cursor clamps to what survived, and a filter
that matches nothing says which term found nothing (design §4.1, §4.4)."
```

---

## Task 5: Table DDL filters its rows

Implements spec §4.2 and §4.4's DDL notice.

**Files:**
- Modify: `crates/lc-domain/src/view/details.rs`

**Interfaces:**
- Consumes: `state.ddl.filter` (Task 2), `Filter::matches` (Task 1).
- Produces: no new public API. `Details` gains a private `retain`; `DetailRow` a private `matches`.

- [ ] **Step 1: Write the failing tests**

In `view/details.rs`'s `mod tests`, following the helpers already there (the file builds a state and reads `panel_body(&state, 1)`):

```rust
#[test]
fn a_property_matches_on_its_name() {
    let state = filtered_ddl("compaction");

    assert!(rows_of(&state).iter().any(|row| matches!(
        row,
        DetailRow::Property { name, .. } if name.contains("compaction")
    )));
}

#[test]
fn a_property_matches_on_its_value() {
    // The value carries the strategy; the property name does not.
    let state = filtered_ddl("SizeTieredCompactionStrategy");

    assert!(rows_of(&state).iter().any(|row| matches!(
        row,
        DetailRow::Property { value, .. } if value.contains("SizeTiered")
    )));
}

#[test]
fn a_column_matches_on_its_name_and_on_its_cql_type() {
    let by_name = filtered_ddl("customer_id");
    assert!(rows_of(&by_name).iter().any(|row| matches!(
        row,
        DetailRow::Column { name, .. } if name == "customer_id"
    )));

    let by_type = filtered_ddl("uuid");
    assert!(rows_of(&by_type).iter().any(|row| matches!(
        row,
        DetailRow::Column { cql_type, .. } if cql_type == "uuid"
    )));
}

#[test]
fn a_section_emptied_by_the_filter_disappears() {
    let state = filtered_ddl("customer_id");
    let PanelBody::Details { details, .. } = panel_body(&state, 1) else {
        panic!("a table is selected");
    };

    assert!(
        details.sections.iter().all(|section| !section.rows.is_empty()),
        "an emptied group disappears by being empty, as Section's doc says"
    );
}

#[test]
fn a_heading_is_not_matched() {
    // "Partition Key" is a heading; no row in that section carries the word.
    let state = filtered_ddl("Partition");

    assert_eq!(
        panel_body(&state, 1),
        PanelBody::Notice {
            text: "No property matches \"Partition\"".to_owned(),
            token: Some(Token::Muted),
        },
        "matching a heading would resurrect a group whose rows all missed"
    );
}

#[test]
fn nothing_matching_says_which_term_found_nothing() {
    assert_eq!(
        panel_body(&filtered_ddl("zzz"), 1),
        PanelBody::Notice {
            text: "No property matches \"zzz\"".to_owned(),
            token: Some(Token::Muted),
        }
    );
}
```

Write the two helpers against whatever fixture the file already has — a state with a table selected in Tables. Sketch:

```rust
fn filtered_ddl(term: &str) -> State {
    let mut state = /* the file's existing "a table is selected" fixture */;
    state.apply_filter(Filtered::Ddl, Filter::new(term));
    state
}

fn rows_of(state: &State) -> Vec<DetailRow> {
    match panel_body(state, 1) {
        PanelBody::Details { details, .. } => {
            details.sections.into_iter().flat_map(|s| s.rows).collect()
        }
        other => panic!("expected details, got {other:?}"),
    }
}
```

Check the fixture's actual column names and types before asserting on `customer_id`/`uuid`/`SizeTieredCompactionStrategy` — use the names the file's own fixture builds, not these.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p lc-domain view::details`
Expected: the heading and nothing-matching tests FAIL (no notice is produced); the others may pass vacuously, since nothing is filtered yet. That is fine — Step 4 is what proves them.

- [ ] **Step 3: Write the implementation**

In `view/details.rs`:

```rust
impl Details {
    /// Drops the rows that miss, then the groups that emptied.
    ///
    /// Headings are not matched: the search is over properties and values, and
    /// a heading match would resurrect a whole group whose rows all missed
    /// (design §4.2).
    fn retain(&mut self, filter: &Filter) {
        for section in &mut self.sections {
            section.rows.retain(|row| row.matches(filter));
        }

        self.sections.retain(|section| !section.rows.is_empty());
    }
}

impl DetailRow {
    fn matches(&self, filter: &Filter) -> bool {
        match self {
            Self::Column { name, cql_type, .. } => {
                filter.matches(name) || filter.matches(cql_type)
            }
            Self::Property { name, value } => filter.matches(name) || filter.matches(value),
        }
    }
}
```

Rewrite `scrolled` to apply it:

```rust
fn scrolled(state: &State, mut details: Details) -> PanelBody {
    let Some(filter) = state.ddl.filter.as_ref() else {
        return PanelBody::Details {
            details,
            offset: state.ddl.scroll.offset(),
        };
    };

    details.retain(filter);

    if details.sections.is_empty() {
        return waiting(&format!("No property matches \"{}\"", filter.as_str()));
    }

    PanelBody::Details {
        details,
        offset: state.ddl.scroll.offset(),
    }
}
```

Import `crate::search::Filter` and `super::waiting`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p lc-domain view::details`
Expected: PASS. Then `mise run test`.

- [ ] **Step 5: Commit**

```bash
mise run ci
git add crates/lc-domain/src/view/details.rs
git commit -m "feat(domain): filter the table ddl panel by property and value

A column matches on its name or its CQL type, a property on its name or
its value, and a group emptied by the filter disappears through the
mechanism Section already had. Headings are not matched: one would
resurrect a group whose rows all missed (design §4.2, §4.4)."
```

---

## Task 6: Table contents highlights matching cells

Implements spec §4.3 and §5.4.

**Files:**
- Modify: `crates/lc-domain/src/style.rs`, `crates/lc-domain/src/view/contents.rs`, `crates/lc-inbound/src/theme.rs`, `crates/lc-inbound/src/body.rs`

**Interfaces:**
- Consumes: `state.contents.filter` (Task 2).
- Produces: `GridCell { pub text: String, pub token: Option<Token>, pub matched: bool }`, `Token::Match`.

Every existing `GridCell { .. }` literal in tests must gain `matched: false`. Find them with `grep -rn "GridCell {" crates --include "*.rs"`.

- [ ] **Step 1: Write the failing tests**

In `view/contents.rs`'s `mod tests`, using the file's existing loaded-page fixture:

```rust
#[test]
fn a_filter_flags_the_cells_that_match_and_keeps_every_row() {
    let mut state = /* the file's existing loaded-grid fixture */;
    let before = match panel_body(&state, 2) {
        PanelBody::Grid { rows, .. } => rows.len(),
        other => panic!("expected a grid, got {other:?}"),
    };

    state.apply_filter(Filtered::Contents, Filter::new("ali"));

    let PanelBody::Grid { rows, .. } = panel_body(&state, 2) else {
        panic!("a filter must not turn the grid into a notice");
    };
    assert_eq!(rows.len(), before, "contents highlights; it does not filter");
    assert!(
        rows.iter().flatten().any(|cell| cell.matched),
        "the fixture contains a cell whose text holds \"ali\""
    );
    assert!(
        rows.iter().flatten().any(|cell| !cell.matched),
        "and one that does not"
    );
}

#[test]
fn a_matched_null_is_still_a_null() {
    let mut state = /* fixture with a CqlValue::Null in it */;
    state.apply_filter(Filtered::Contents, Filter::new("null"));

    let PanelBody::Grid { rows, .. } = panel_body(&state, 2) else {
        panic!("expected a grid");
    };
    let null = rows
        .iter()
        .flatten()
        .find(|cell| cell.token == Some(Token::Muted))
        .expect("the fixture has a null");

    assert!(
        null.matched,
        "a null renders as the word null, so it answers this filter"
    );
    assert_eq!(
        null.token,
        Some(Token::Muted),
        "and must stay distinguishable from a text column holding \"null\""
    );
}

#[test]
fn no_filter_flags_nothing() {
    let state = /* the loaded-grid fixture */;
    let PanelBody::Grid { rows, .. } = panel_body(&state, 2) else {
        panic!("expected a grid");
    };

    assert!(rows.iter().flatten().all(|cell| !cell.matched));
}
```

In `crates/lc-inbound/src/theme.rs`, extend the existing exhaustive token sweep (the array at ~line 98) with `Token::Match`, and add:

```rust
#[test]
fn a_match_is_legible_over_a_selected_row() {
    let theme = Theme::TerminalDefault;
    let selected = theme.style(Token::Selected);
    let matched = theme.style(Token::Match);

    assert_ne!(
        matched, selected,
        "the cursor's row can contain matches; both must still read"
    );
    assert!(
        matched.fg.is_some() || matched.bg.is_some(),
        "REVERSED over REVERSED cancels out, so Match must carry a colour"
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p lc-domain view::contents && cargo test -p lc-inbound theme`
Expected: compile error — `GridCell` has no field `matched`, `Token` has no variant `Match`.

- [ ] **Step 3: Write the implementation**

`style.rs`, on `Token`:

```rust
/// A cell whose text answered the applied filter.
///
/// Named for what it marks, like [`Token::PropertyName`]: the domain still
/// names no colour (architecture.md §6).
Match,
```

`view/contents.rs`:

```rust
/// One value, already rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCell {
    pub text: String,
    /// `Some(Token::Muted)` for a null, which is what keeps it apart from a
    /// text column whose value is the word "null".
    pub token: Option<Token>,
    /// Whether the applied filter matched `text`.
    ///
    /// A field of its own rather than a second meaning for `token`:
    /// overwriting the null marker on a match would lose that distinction for
    /// exactly the cells the user went looking for (design §4.3).
    pub matched: bool,
}
```

Thread the filter through `grid` and `grid_cell`:

```rust
rows: page
    .rows
    .iter()
    .map(|row| {
        row.values
            .iter()
            .map(|value| grid_cell(value, state.contents.filter.as_ref()))
            .collect()
    })
    .collect(),
```

```rust
fn grid_cell(value: &CqlValue, filter: Option<&Filter>) -> GridCell {
    let text = cell::text(value);

    GridCell {
        matched: filter.is_some_and(|filter| filter.matches(&text)),
        token: match value {
            CqlValue::Null => Some(Token::Muted),
            _ => None,
        },
        text,
    }
}
```

`theme.rs`: give `Token::Match` its own arm. `REVERSED` is taken by `Token::Selected` and would cancel out on a selected row, so use a colour:

```rust
Token::Match => Style::new().fg(Color::Black).bg(Color::Yellow),
```

`body.rs`: where a grid cell's span is styled, prefer `Token::Match` over the cell's own token when `matched` is set — a matched null shows as a match, and is still the word `null`. Follow the file's existing style-composition shape rather than inventing one.

Add `matched: false` to every existing `GridCell` literal the compiler names.

- [ ] **Step 4: Run the tests**

Run: `mise run test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
mise run ci
git add -A
git commit -m "feat(domain): highlight the table contents cells a filter matches

Cell-level rather than span-level: a wide value is truncated at the
panel edge, so a span highlight would silently fail whenever the match
sits past the cut. A field of its own rather than a second meaning for
token, so a matched null is still a null (design §4.3, §5.4)."
```

---

## Task 7: The prompt

Implements spec §1, §1.1, §3, §3.1, §3.2, §3.3 and §6. This is the task that makes the feature reachable.

**Files:**
- Modify: `crates/lc-domain/src/search.rs`, `crates/lc-domain/src/state.rs`, `crates/lc-domain/src/update.rs`

**Interfaces:**
- Consumes: `Filter`, `Filtered` (Task 1); `State::apply_filter` (Tasks 2, 4).
- Produces:
  - `Prompt::opened(Filtered, Option<Filter>) -> Prompt`, `Prompt::target(&self) -> Filtered`, `Prompt::query(&self) -> &str`, `Prompt::push(&mut self, char)`, `Prompt::pop(&mut self)`, `Prompt::previous(&self) -> Option<&Filter>`
  - `Overlay::Search(Prompt)`

- [ ] **Step 1: Write the failing tests**

In `update.rs`'s `mod tests`:

```rust
fn searching(state: &State) -> Option<&Prompt> {
    match &state.overlay {
        Some(Overlay::Search(prompt)) => Some(prompt),
        _ => None,
    }
}

#[test]
fn slash_opens_the_prompt_in_every_panel_that_can_hold_a_filter() {
    for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
        let mut state = tabled_state(&[("shop", &["orders"])]);
        state.focus = panel;

        press(&mut state, Key::Char('/'));

        assert_eq!(
            searching(&state).map(Prompt::target),
            Filtered::of(panel),
            "{panel:?} must reach a prompt"
        );
    }
}

#[test]
fn slash_is_text_in_queries_and_opens_nothing() {
    let mut state = State {
        focus: Panel::Queries,
        ..State::default()
    };

    press(&mut state, Key::Char('/'));

    assert!(state.overlay.is_none(), "the editor claims it first");
    assert_eq!(state.queries.lines(), ["/"]);
}

#[test]
fn q_and_the_question_mark_type_themselves_in_the_prompt() {
    let mut state = tabled_state(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Char('q'));
    press(&mut state, Key::Char('?'));

    assert!(!state.should_quit, "q is text while a prompt is up");
    assert_eq!(searching(&state).map(Prompt::query), Some("q?"));
}

#[test]
fn the_prompt_filters_live_per_keystroke() {
    let mut state = tabled_state(&[("shop", &["orders", "users"])]);
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Char('u'));

    assert_eq!(
        state.filter(Filtered::Tables).map(Filter::as_str),
        Some("u"),
        "the panel underneath is the preview"
    );
}

#[test]
fn backspace_narrows_the_query_and_re_applies_it() {
    let mut state = tabled_state(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('o'));
    press(&mut state, Key::Char('z'));

    press(&mut state, Key::Backspace);

    assert_eq!(searching(&state).map(Prompt::query), Some("o"));
    assert_eq!(state.filter(Filtered::Tables).map(Filter::as_str), Some("o"));
}

#[test]
fn backspace_on_an_empty_query_is_inert_rather_than_closing() {
    let mut state = tabled_state(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Backspace);

    assert_eq!(searching(&state).map(Prompt::query), Some(""));
}

#[test]
fn enter_commits_and_closes_leaving_the_filter_applied() {
    let mut state = tabled_state(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('o'));

    press(&mut state, Key::Enter);

    assert!(state.overlay.is_none());
    assert_eq!(state.filter(Filtered::Tables).map(Filter::as_str), Some("o"));
}

#[test]
fn an_empty_commit_clears_the_filter() {
    let mut state = tabled_state(&[("shop", &["orders"])]);
    state.apply_filter(Filtered::Tables, Filter::new("o"));
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Enter);

    assert!(state.overlay.is_none());
    assert_eq!(state.filter(Filtered::Tables), None);
}

#[test]
fn escape_cancels_the_prompt_restoring_the_filter_that_was_there() {
    let mut state = tabled_state(&[("shop", &["orders", "users"])]);
    state.apply_filter(Filtered::Tables, Filter::new("orders"));
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('u'));

    press(&mut state, Key::Escape);

    assert!(state.overlay.is_none());
    assert_eq!(
        state.filter(Filtered::Tables).map(Filter::as_str),
        Some("orders"),
        "an abandoned prompt leaves the panel as it was found"
    );
}

#[test]
fn escape_clears_the_focused_panels_filter_when_no_overlay_is_up() {
    let mut state = tabled_state(&[("shop", &["orders"])]);
    state.focus = Panel::Contents;
    state.apply_filter(Filtered::Contents, Filter::new("o"));

    press(&mut state, Key::Escape);

    assert_eq!(state.filter(Filtered::Contents), None);
}

#[test]
fn escape_prefers_the_overlay_to_the_filter() {
    let mut state = tabled_state(&[("shop", &["orders"])]);
    state.apply_filter(Filtered::Tables, Filter::new("o"));
    state.overlay = Some(Overlay::Message(Message {
        title: "t".to_owned(),
        message: "m".to_owned(),
        severity: Severity::Info,
    }));

    press(&mut state, Key::Escape);

    assert!(state.overlay.is_none());
    assert_eq!(
        state.filter(Filtered::Tables).map(Filter::as_str),
        Some("o"),
        "one press does one thing"
    );
}

#[test]
fn escape_prefers_the_filter_to_leaving_the_editor() {
    // Unreachable today — Queries holds no filter (design §8) — so this pins
    // the ordering for the vim slice by checking the two lower rungs still
    // resolve in order.
    let mut state = tabled_state(&[("shop", &["orders"])]);
    state.focus = Panel::Queries;

    press(&mut state, Key::Escape);

    assert_eq!(state.focus, Panel::Tables);
}

#[test]
fn tab_is_inert_while_the_prompt_is_up() {
    let mut state = tabled_state(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));

    press(&mut state, Key::Tab);
    press(&mut state, Key::BackTab);

    assert_eq!(state.focus, Panel::Tables);
    assert!(searching(&state).is_some(), "and the prompt survives");
}

#[test]
fn a_click_cancels_the_prompt() {
    let mut state = tabled_state(&[("shop", &["orders", "users"])]);
    state.apply_filter(Filtered::Tables, Filter::new("orders"));
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('u'));

    update(&mut state, Msg::Pointer(Pointer::Click(Click::Panel(Panel::Tables))));

    assert!(state.overlay.is_none());
    assert_eq!(
        state.filter(Filtered::Tables).map(Filter::as_str),
        Some("orders"),
        "a click that lands somewhere unexpected must not leave state behind"
    );
}

#[test]
fn a_filter_survives_a_focus_change() {
    let mut state = tabled_state(&[("shop", &["orders"])]);
    press(&mut state, Key::Char('/'));
    press(&mut state, Key::Char('o'));
    press(&mut state, Key::Enter);

    press(&mut state, Key::Tab);
    press(&mut state, Key::Char('t'));

    assert_eq!(state.filter(Filtered::Tables).map(Filter::as_str), Some("o"));
}
```

Match the file's own helper names (`press`, `update`, the state fixture) rather than these — read the top of `update.rs`'s test module first and use what is there.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p lc-domain slash_opens_the_prompt`
Expected: compile error — no `Overlay::Search`, no `Prompt`.

- [ ] **Step 3: Write the implementation**

Append to `search.rs`:

```rust
/// An open search prompt.
///
/// Lives in [`crate::state::Overlay`] rather than beside it, so "a palette and
/// a prompt at once" cannot be written down — the argument that enum's own doc
/// comment already makes about a message and a palette (design §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    target: Filtered,
    query: String,
    /// What was applied before this prompt opened, and what `esc` restores.
    /// Live filtering overwrites the target's filter on every keystroke, so
    /// cancelling needs something to put back (design §3).
    previous: Option<Filter>,
}

impl Prompt {
    /// Opens over `target`, remembering the filter it is about to overwrite.
    #[must_use]
    pub fn opened(target: Filtered, previous: Option<Filter>) -> Self {
        Self {
            target,
            query: String::new(),
            previous,
        }
    }

    /// Captured at open, not re-read per keystroke: `tab` is inert while a
    /// prompt is up, so the panel being filtered cannot change under a live
    /// query (design §3.2).
    #[must_use]
    pub const fn target(&self) -> Filtered {
        self.target
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    #[must_use]
    pub const fn previous(&self) -> Option<&Filter> {
        self.previous.as_ref()
    }

    /// What the query now means, for a caller that must apply it.
    #[must_use]
    pub fn filter(&self) -> Option<Filter> {
        Filter::new(&self.query)
    }

    pub fn push(&mut self, character: char) {
        self.query.push(character);
    }

    pub fn pop(&mut self) {
        self.query.pop();
    }
}
```

In `state.rs`, add the variant and extend `Overlay`'s doc comment to say that it means *what outranks focus* rather than *a box over the panels* — the prompt is drawn on the bar row (design §1):

```rust
pub enum Overlay {
    Message(Message),
    Commands(CommandsPanel),
    /// Drawn on the hint bar row rather than over the panels, and in this enum
    /// for what that buys: a prompt and a palette cannot both be up.
    Search(Prompt),
}
```

In `update.rs`'s `handle_key`, add a claim beside the palette's, **before** the `takes_text` check (an overlay outranks focus, and Queries can hold no prompt anyway):

```rust
// A prompt takes text, so it claims every printable key — the
// panel-that-takes-text rule, applied to an overlay (design §1.1).
if matches!(state.overlay, Some(Overlay::Search(_))) {
    return search_key(state, press.key);
}
```

Add the `/` arm to the match, **above** the trailing `Key::Char(key)` panel-focus arm:

```rust
Key::Char('/') => open_prompt(state),
```

And the two functions:

```rust
/// `/` opens a prompt over the focused panel. Refused where the panel holds no
/// filter, which is Queries — unreachable, since the editor claims `/` as text
/// first (design §8), and stated here rather than assumed.
fn open_prompt(state: &mut State) {
    let Some(target) = Filtered::of(state.focus) else {
        return;
    };

    let previous = state.filter(target).cloned();
    state.overlay = Some(Overlay::Search(Prompt::opened(target, previous)));
}

/// What an open prompt responds to.
///
/// Every printable key is a character, so `q` types a `q` — the opposite of
/// the palette, which leaves `q` quitting. The two differ because the palette
/// takes motion and this takes text (design §1.1, §3.1).
fn search_key(state: &mut State, key: Key) -> Vec<Effect> {
    let Some(Overlay::Search(prompt)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => prompt.push(character),
        Key::Backspace => prompt.pop(),
        Key::Enter => {
            state.overlay = None;
            return Vec::new();
        }
        Key::Escape => {
            let Some(Overlay::Search(prompt)) = state.overlay.take() else {
                return Vec::new();
            };
            state.apply_filter(prompt.target(), prompt.previous().cloned());
            return Vec::new();
        }
        _ => return Vec::new(),
    }

    let (target, filter) = (prompt.target(), prompt.filter());
    state.apply_filter(target, filter);

    Vec::new()
}
```

`Enter` needs no apply: every keystroke already applied. It only closes.

Rewrite `escape` for §6's chain. `Overlay::take()` is destructive, so match on
what it returned rather than testing `state.overlay` again afterwards:

```rust
/// `Esc` resolves in one order, first hit wins: the overlay, then the focused
/// panel's filter, then leaving a panel that takes text. One press does one of
/// the three, never two — the thing most recently put on screen comes off
/// first (design §6).
fn escape(state: &mut State) {
    match state.overlay.take() {
        Some(Overlay::Search(prompt)) => {
            state.apply_filter(prompt.target(), prompt.previous().cloned());
            return;
        }
        Some(Overlay::Message(_) | Overlay::Commands(_)) => return,
        None => {}
    }

    if let Some(target) = Filtered::of(state.focus)
        && state.filter(target).is_some()
    {
        state.apply_filter(target, None);
        return;
    }

    if state.focus.takes_text() {
        state.focus = Panel::default();
    }
}
```

If let-chains are not available on the pinned toolchain, nest the `if let`.

`search_key`'s `Escape` arm and `escape`'s first arm now do the same thing. Have `search_key` delegate: `Key::Escape => { escape(state); return Vec::new(); }`.

In `handle_pointer`, the existing overlay branch sets `state.overlay = None` on a click, which would drop a prompt without restoring `previous`. Route it through `escape` instead:

```rust
if state.overlay.is_some() {
    if matches!(pointer, Pointer::Click(_)) {
        escape(state);
    }
    return Vec::new();
}
```

`escape` with an overlay up only ever dismisses it, so this changes nothing for a message or a palette.

- [ ] **Step 4: Run the tests**

Run: `mise run test`
Expected: PASS. Pay attention to `escape_dismisses_the_overlay_before_it_leaves_the_editor`, `escape_leaves_the_editor_for_tables`, `escape_closes_the_palette`, `a_click_dismisses_the_overlay_and_does_nothing_else` and `an_open_palette_outranks_a_panel_that_takes_text` — all must still pass unchanged.

- [ ] **Step 5: Commit**

```bash
mise run ci
git add -A
git commit -m "feat(domain): open a search prompt with /

The prompt joins Overlay rather than becoming a second state field, so
a palette and a prompt cannot both be up, and it inherits the
overlay-outranks-focus precedence a panel that takes no text needs to
accept a typed query. Esc gains a precedence chain: overlay, then
filter, then leave the editor (design §1, §3, §6)."
```

---

## Task 8: Making it visible

Implements spec §5.2, §5.3 and the `PanelView::filter` half of §5.1. doc/shortcuts.md §2 requires every binding to be visible somewhere; this is where `/` and `esc` declare themselves.

**Files:**
- Modify: `crates/lc-domain/src/state.rs` (panel hints), `crates/lc-domain/src/view/hints.rs`, `crates/lc-domain/src/view/mod.rs`, `crates/lc-domain/src/commands.rs`

**Interfaces:**
- Consumes: `Overlay::Search`, `Prompt` (Task 7); `State::filter` (Task 2).
- Produces:
  - `PanelView { panel, title, body, filter: Option<String> }`
  - `OverlayView::Search { query: String }`

- [ ] **Step 1: Write the failing tests**

In `view/hints.rs`'s `mod tests`:

```rust
#[test]
fn the_bar_names_search_in_every_panel_that_can_hold_a_filter() {
    for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
        let state = State { focus: panel, ..fits() };

        assert!(
            bar(&state).panel.iter().any(|hint| hint.keys == "/"),
            "{panel:?} responds to / and must say so"
        );
    }
}

#[test]
fn the_bar_does_not_name_search_in_queries() {
    let state = State { focus: Panel::Queries, ..fits() };

    assert!(bar(&state).panel.iter().all(|hint| hint.keys != "/"));
}

#[test]
fn the_bar_names_esc_clear_only_where_a_filter_is_applied() {
    let mut state = State { focus: Panel::Tables, ..fits() };
    assert!(bar(&state).panel.iter().all(|hint| hint.action != "clear"));

    state.apply_filter(Filtered::Tables, Filter::new("o"));

    assert!(
        bar(&state).panel.iter().any(|hint| hint.keys == "esc" && hint.action == "clear"),
        "in this frame pressing esc unconditionally does something"
    );
}

#[test]
fn the_bar_names_what_the_prompt_responds_to() {
    let mut state = fits();
    state.overlay = Some(Overlay::Search(Prompt::opened(Filtered::Tables, None)));

    let bar = bar(&state);

    assert!(bar.panel.is_empty(), "the panel underneath responds to nothing");
    assert_eq!(
        bar.global
            .iter()
            .map(|hint| (hint.keys.as_str(), hint.action))
            .collect::<Vec<_>>(),
        [("⏎", "apply"), ("esc", "cancel")]
    );
}
```

In `view/mod.rs`'s `mod tests`:

```rust
#[test]
fn a_panel_carries_the_filter_applied_to_it() {
    let mut state = tabled(&[("shop", &["orders"])]);
    state.apply_filter(Filtered::Contents, Filter::new("ORD"));

    let view = view_model(&state);

    assert_eq!(view.panels[2].filter.as_deref(), Some("ORD"), "as typed");
    assert_eq!(view.panels[0].filter, None);
    assert_eq!(view.panels[3].filter, None, "Queries holds none");
}

#[test]
fn an_open_prompt_is_projected_as_its_query() {
    let mut state = tabled(&[("shop", &["orders"])]);
    let mut prompt = Prompt::opened(Filtered::Tables, None);
    prompt.push('o');
    state.overlay = Some(Overlay::Search(prompt));

    assert_eq!(
        view_model(&state).overlay,
        Some(OverlayView::Search { query: "o".to_owned() })
    );
}
```

In `commands.rs`'s `mod tests`, follow whatever the existing catalogue tests assert; add:

```rust
#[test]
fn the_catalogue_lists_search() {
    assert!(
        CATALOGUE
            .iter()
            .flat_map(|group| group.bindings)
            .any(|binding| binding.keys == [Key::Char('/')]),
        "the palette claims to be complete (design §2)"
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p lc-domain view::hints view::tests commands::tests`
Expected: FAIL — no `/` hint, no `filter` field on `PanelView`, no `OverlayView::Search`.

- [ ] **Step 3: Write the implementation**

In `state.rs`, add the search binding to `TABLES_HINTS`, `DDL_HINTS` and `CONTENTS_HINTS` (not `QUERIES_HINTS`):

```rust
Binding {
    keys: &[Key::Char('/')],
    action: "search",
},
```

In `view/hints.rs`, add the prompt's early return beside the palette's, and the conditional `esc clear`:

```rust
/// What the bar names while a prompt is up. The panel group is empty: the
/// focused panel responds to nothing, and naming its bindings would be the
/// lie design §2 exists to prevent.
const SEARCH_GLOBAL: &[Binding] = &[
    Binding {
        keys: &[Key::Enter],
        action: "apply",
    },
    Binding {
        keys: &[Key::Escape],
        action: "cancel",
    },
];

/// `esc` is named only where pressing it unconditionally does something (hint
/// bar design §3). With a filter applied, it clears one — which is what keeps
/// clearing discoverable (design §5.3).
const CLEAR: Binding = Binding {
    keys: &[Key::Escape],
    action: "clear",
};
```

In `bar`:

```rust
if matches!(state.overlay, Some(Overlay::Search(_))) {
    return HintBar {
        panel: Vec::new(),
        global: SEARCH_GLOBAL.iter().map(rendered).collect(),
    };
}
```

and, where the panel group is built:

```rust
let mut panel: Vec<Hint> = state.focus.hints().iter().map(rendered).collect();
if Filtered::of(state.focus).is_some_and(|target| state.filter(target).is_some()) {
    panel.push(rendered(&CLEAR));
}
```

In `view/mod.rs`:

```rust
pub struct PanelView {
    pub panel: Panel,
    pub title: PanelTitle,
    pub body: PanelBody,
    /// The applied filter's term, as typed. Drawn on the panel's own top
    /// border, focused or not: a filter that survives a focus change has to
    /// say so from the panel it is applied to (design §5.1).
    pub filter: Option<String>,
}
```

```rust
panels: Panel::ALL.map(|panel| PanelView {
    panel,
    title: PanelTitle::marking(title(state, panel), panel.shortcut()),
    body: body(state, panel),
    filter: Filtered::of(panel)
        .and_then(|target| state.filter(target))
        .map(|filter| filter.as_str().to_owned()),
}),
```

```rust
pub enum OverlayView {
    Message(Message),
    Commands { rows: Vec<CommandRow>, selected: usize },
    Search { query: String },
}
```

with the matching arm in `overlay()`:

```rust
Overlay::Search(prompt) => OverlayView::Search {
    query: prompt.query().to_owned(),
},
```

In `commands.rs`, add a `SEARCH` group after `TABLES`, or extend `GLOBAL` — `/` works in three of four panels, so it is not global. Add a group:

```rust
// The actions are cut to twenty characters because MIN_W is derived from the
// longest one (command palette design §5).
const SEARCH: &[Binding] = &[
    Binding {
        keys: &[Key::Char('/')],
        action: "search this panel",
    },
];
```

and a `Group { heading: "Search", bindings: SEARCH }` in `CATALOGUE`. Check `no_command_row_is_wider_than_the_narrowest_palette` still passes — shorten the action if not.

- [ ] **Step 4: Run the tests**

Run: `mise run test`
Expected: PASS. `every_global_hint_does_something_in_every_panel` and `the_bar_names_the_palette_between_panel_and_quit` must be unaffected — `/` and `esc clear` are panel hints, not global ones.

- [ ] **Step 5: Commit**

```bash
mise run ci
git add -A
git commit -m "feat(domain): make search declare itself

/ joins the three panels that answer it, esc clear joins the bar only
where a filter is applied — the one frame where naming esc is not a
lie — and the palette lists /. Every panel carries its applied term for
the marker (design §5.1, §5.2, §5.3)."
```

---

## Task 9: Drawing it

Implements spec §5.1's marker and §5.2's prompt row.

**Files:**
- Modify: `crates/lc-inbound/src/render.rs`

**Interfaces:**
- Consumes: `PanelView::filter`, `OverlayView::Search` (Task 8).
- Produces: no API. Rendering only.

- [ ] **Step 1: Write the failing tests**

In `render.rs`'s `mod tests`, using its existing `draw_state` / `top_border` / `block_width` helpers:

```rust
#[test]
fn a_panel_wears_the_filter_applied_to_it() {
    let mut state = /* the file's connected fixture */;
    state.apply_filter(Filtered::Tables, Filter::new("ord"));

    let buffer = draw_state(80, 24, &state);

    assert!(
        row_text(&buffer, top_border(&buffer, "Tables").1).contains("/ord"),
        "the term sits on the panel's own top border"
    );
}

#[test]
fn the_marker_is_drawn_muted() {
    let mut state = /* the file's connected fixture */;
    state.apply_filter(Filtered::Tables, Filter::new("ord"));

    let buffer = draw_state(80, 24, &state);
    let style = style_of_first(&buffer, "/ord");

    assert_eq!(style, Theme::TerminalDefault.style(Token::Muted));
}

#[test]
fn the_marker_is_dropped_rather_than_truncated() {
    let mut state = /* the file's connected fixture */;
    state.apply_filter(
        Filtered::Tables,
        Filter::new("a_very_long_search_term_indeed"),
    );

    let buffer = draw_state(40, 24, &state);
    let border = row_text(&buffer, top_border(&buffer, "Tables").1);

    assert!(
        !border.contains('/'),
        "a truncated term is a lie about what is applied"
    );
    assert!(border.contains("Tables"), "and the title survives");
}

#[test]
fn an_open_prompt_owns_the_bar_row() {
    let mut state = /* the file's connected fixture */;
    let mut prompt = Prompt::opened(Filtered::Tables, None);
    prompt.push('o');
    prompt.push('r');
    state.overlay = Some(Overlay::Search(prompt));

    let buffer = draw_state(80, 24, &state);
    let bar = row_text(&buffer, 23);

    assert!(bar.contains("/or"), "the query is on the bar");
    assert!(bar.contains("cancel"), "and so are the prompt's own keys");
    assert!(!bar.contains("j/k move"), "the panel's hints are gone");
}
```

`row_text` and `style_of_first` may not exist. The file has equivalents (it inspects buffers by cell); use those, or add small helpers next to the ones already there.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p lc-inbound render::tests::a_panel_wears_the_filter`
Expected: FAIL — no marker is drawn.

- [ ] **Step 3: Write the implementation**

In `render`'s per-panel loop, after the title is built:

```rust
// Right-aligned on the top border, and dropped whole when the border cannot
// hold both: a truncated term is a lie about what is applied (design §5.1).
let mut block = Block::bordered()
    .border_type(BorderType::Rounded)
    .title(title);

if let Some(term) = &panel.filter {
    let marker = format!("/{term}");
    if marker_fits(area.width, panel.title.text(), &marker) {
        block = block.title(
            Line::from(Span::styled(marker, theme.style(Token::Muted)))
                .right_aligned(),
        );
    }
}
```

with

```rust
/// Both titles and the two corners plus a space either side of each.
fn marker_fits(width: u16, title: &str, marker: &str) -> bool {
    let needed = title.chars().count() + marker.chars().count() + 6;
    usize::from(width) >= needed
}
```

Tune the constant against the actual `Block` padding — check what the drawn border looks like at the boundary width and pick the number that makes `the_marker_is_dropped_rather_than_truncated` pass at 40 columns and the marker appear at 80.

For the prompt, in the `match &view.overlay` at the end of `render`, add an arm that draws over `hint_bar` instead of over the panels. The bar was already rendered above; render the query over the left of that same rect:

```rust
Some(OverlayView::Search { query }) => {
    frame.render_widget(
        Line::from(format!(" /{query}")),
        hint_bar,
    );
}
```

That would erase the `⏎ apply / esc cancel` hints the bar drew. It must not: compose one line instead — the query on the left, `crate::hints::line(&view.hints, …)` right-aligned in what remains. Follow `bar_split`'s existing shape for dividing the row.

Cursor: the prompt is a text field, so `frame.set_cursor_position` belongs after the query, and the per-panel `focused` flag (already `panel.panel == view.focus && view.overlay.is_none()`) already suppresses every panel cursor while an overlay is up. No change needed there.

- [ ] **Step 4: Run the tests**

Run: `mise run test`
Expected: PASS. `the_hint_bar_and_a_frame_below_the_minimum_are_not_clickable` and the `FrameMap` tests must be unaffected — the marker is chrome on a border that already maps to `Click::Panel`.

- [ ] **Step 5: Commit**

```bash
mise run ci
git add crates/lc-inbound/src/render.rs
git commit -m "feat(inbound): draw the filter marker and the search prompt

The term rides the panel's own top border, muted, and is dropped whole
where the border cannot hold it — a truncated term is a lie about what
is applied. The prompt takes the bar row it shares with its own keys
(design §5.1, §5.2)."
```

---

## Task 10: The documentation the binding owes

doc/shortcuts.md §6 requires §1, §3 and §5 to be updated in the same commit as a new binding. This slice's binding landed across Tasks 7–9, so this task pays that debt and closes the spec.

**Files:**
- Modify: `doc/shortcuts.md`, `doc/specs/2026-08-10-panel-search-design.md`, `TODO.md`

- [ ] **Step 1: Update `doc/shortcuts.md` §1**

- Global table: `/` — *Search the focused panel. Not reachable from Queries* (with the same ¹ footnote the other letters carry).
- Global table: revise the `Esc` row to *Dismiss the overlay, or clear the focused panel's filter* and point at §2's chain.
- New subsection, **The search prompt**, with §3.1's key table verbatim.
- Tables subsection: a line saying `l`/`h` are swallowed while a filter is applied, and why.

- [ ] **Step 2: Update §2**

Add the prompt as a third exception under *bindings are global; panels refuse*, ordered **after** the palette and **before** a panel that takes text, and state `Esc`'s precedence chain from spec §6 as a rule of its own.

- [ ] **Step 3: Update §3 and §7**

- §3: move `/` out of the free set into *Taken*. It is punctuation, so it joins `Space` and `?` in that sentence rather than the letter list.
- §7: a click cancels an open prompt, restoring the filter that was there.

- [ ] **Step 4: Update §5**

Add a row per guarantee, naming the test that pins it. Take the names from the spec's §9 table and reconcile them against the tests actually written in Tasks 1–9 — §5 is checked by reading, so a name that drifted is a broken link.

- [ ] **Step 5: Close the spec and the TODO**

- Flip the spec's `**Status:** designed` to `**Status:** implemented`.
- In `TODO.md`, remove or amend anything this slice settled; if the Queries-search question is not already there, add it, pointing at spec §8.

- [ ] **Step 6: Verify and commit**

Run `mise run ci` one final time, and re-read spec §9's table against `grep -rn "fn a_\|fn the_\|fn slash_" crates` to confirm every named guarantee exists.

```bash
git add doc/shortcuts.md doc/specs/2026-08-10-panel-search-design.md TODO.md
git commit -m "docs(shortcuts): record / and the esc precedence chain

The prompt is a third exception to bindings-are-global, ranked after
the palette and before a panel that takes text. / leaves the free set,
esc gains its chain, and §5 names the tests that hold both up
(doc/shortcuts.md §6)."
```

---

## Self-review notes

**Spec coverage.** §1 → Task 7. §1.1 → Task 7. §2 → Task 1. §2.1 → Tasks 1, 2. §2.2 → Task 2. §3 → Task 7. §3.1 → Task 7. §3.2 → Task 7. §3.3 → Tasks 4 (Tables clamp), 7 (live apply); the DDL non-clamp is a deliberate no-op documented in Task 4. §4.1 → Tasks 3, 4. §4.2 → Task 5. §4.3 → Task 6. §4.4 → Tasks 4, 5. §5.1 → Tasks 8 (projection), 9 (drawing). §5.2 → Tasks 8, 9. §5.3 → Task 8. §5.4 → Task 6. §6 → Task 7. §7 → nothing is built, by design. §8 → Task 10 (TODO pointer). §9 → every task. §10 → Task 10.

**Known soft spots**, flagged rather than hidden:

- Tasks 5, 6 and 9 tell the implementer to use the fixtures already in those test modules rather than inventing new ones, so a few assertions are written against placeholder column names and buffer helpers. Read the file first; the shape of the test is the instruction, not the fixture literals.
- Task 9's `marker_fits` constant has to be tuned against a real drawn border. The two tests bracket it.
- Task 9's bar composition (query + right-aligned hints in one row) is described, not written out, because it must follow `bar_split`'s existing division of that row.
