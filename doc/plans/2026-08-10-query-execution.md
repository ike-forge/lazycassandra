# Query execution implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ctrl-r` runs the whole Queries buffer as one statement and lands its page in the Contents panel.

**Architecture:** `handle_key`'s blanket ctrl/alt refusal splits: alt stays refused, ctrl routes into a `ctrl` key table holding one member. The buffer is classified by first keyword into a `StatementKind`, refused outright if destructive, and otherwise handed to the `submit` that already exists — so the posture gate, the in-flight registry and the effect runtime are untouched. `ContentsState` stops being about a table and becomes about a `Subject`, which is either a table or a typed statement, so one grid serves both paths.

**Tech Stack:** Rust 2024 · `ratatui` 0.30.2 · `crossterm` 0.29 · `cargo nextest`.

**Design:** [doc/specs/2026-08-10-query-execution-design.md](../specs/2026-08-10-query-execution-design.md). Section references below (§1–§10) are to that document.

## Global Constraints

- **No new dependencies.** Nothing in this plan needs one; adding a crate to a `Cargo.toml` here is a mistake.
- **`lc-domain` must not gain `ratatui`, `crossterm` or `tokio`.** Every file this plan touches in `lc-domain` stays free of them.
- **No `unsafe`, no FFI.** Enforced by `unsafe_code = "forbid"`.
- **Zero warnings.** `mise run ci` must be clean. Any `#[allow(...)]` must be item-scoped with a reason. `clippy::pedantic` is on and `unwrap_used` / `expect_used` warn — outside tests, use `unwrap_or`, `map_or`, or `saturating_*`.
- **No narrating comments** (conventions.md §2). The comments written into this plan are the ones that earn their place — edge cases, and rules the code cannot state. Do not add others.
- **Determinism.** Everything here is fast-tier: no clock, no socket, no filesystem, no `HashMap` iteration order.
- **Public items in `lc-domain` carry `///` docs.**
- **Conventional Commits**, imperative, lowercase, no trailing period.
- **Never `git pull`, never `git push`.**

## Commands

| | |
|---|---|
| Build | `mise run build` |
| Format | `mise run fmt` |
| Fast tests | `mise run test` |
| One test | `cargo nextest run -p lc-domain -E 'test(the_test_name)'` |
| Everything CI gates on | `mise run ci` |

## File Structure

| File | Responsibility after this plan |
|---|---|
| `crates/lc-domain/src/model/statement.rs` | Gains `Statement::typed`, the first-keyword classifier, and its tests. The one place a CQL parser will later land. |
| `crates/lc-domain/src/contents.rs` | `ContentsState` carries a `Subject` (table or typed statement) instead of a `TableRef`. `TableRef` itself is unchanged. |
| `crates/lc-domain/src/view/contents.rs` | Titles from `Subject::label()`; column key-tokens resolve only when the subject names a table. |
| `crates/lc-domain/src/model/input.rs` | Gains `Modifiers::NONE`. |
| `crates/lc-domain/src/view/hints.rs` | `Binding` carries `modifiers`; `label` renders `ctrl-r`. |
| `crates/lc-domain/src/state.rs` | `QUERIES_HINTS` gains the run binding; every `Binding` literal gains `modifiers`. |
| `crates/lc-domain/src/commands.rs` | Gains the Queries group; every `Binding` literal gains `modifiers`. |
| `crates/lc-domain/src/update.rs` | The `ctrl` layer, `run`, and `submit_into_contents` shared by `run` and `read`. |
| `doc/shortcuts.md` | §1, §2, §3, §5 updated in the last task. |

Nothing in `lc-app`, `lc-inbound` or `lc-outbound` changes. `Effect::Execute` is already interpreted by the effect runtime and `Msg::PageReady` already reaches `state.contents` — a query's page travels the road a table read built.

---

### Task 1: Classify a typed statement

**Files:**
- Modify: `crates/lc-domain/src/model/statement.rs`
- Test: same file, `mod tests`

**Interfaces:**
- Consumes: `StatementKind`, `Statement` — both already in this file.
- Produces: `Statement::typed(cql: &str) -> Statement`, used by Task 4.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/lc-domain/src/model/statement.rs`:

```rust
    #[test]
    fn selecting_is_a_read() {
        assert_eq!(Statement::typed("SELECT * FROM shop.orders").kind, StatementKind::Read);
    }

    #[test]
    fn case_and_leading_space_do_not_change_a_kind() {
        assert_eq!(Statement::typed("  select 1").kind, StatementKind::Read);
        assert_eq!(Statement::typed("InSeRt INTO t (a) VALUES (1)").kind, StatementKind::Write);
    }

    #[test]
    fn dropping_and_truncating_are_destructive() {
        for cql in ["DROP TABLE shop.orders", "truncate shop.orders"] {
            assert_eq!(
                Statement::typed(cql).kind,
                StatementKind::Destructive,
                "{cql} must not reach the cluster without confirmation"
            );
        }
    }

    #[test]
    fn creating_and_altering_are_schema_changes() {
        assert_eq!(Statement::typed("CREATE TABLE t (a int PRIMARY KEY)").kind, StatementKind::Schema);
        assert_eq!(Statement::typed("ALTER TABLE t ADD b int").kind, StatementKind::Schema);
    }

    #[test]
    fn an_unknown_verb_is_treated_as_mutating() {
        // The conservative direction: a verb this table has not learned is
        // refused on a read-only connection rather than waved through it.
        assert!(Statement::typed("LIST ROLES").mutates());
        assert!(Statement::typed("-- a comment first\nSELECT 1").mutates());
    }

    #[test]
    fn the_cql_is_kept_verbatim() {
        let cql = "SELECT *\nFROM shop.orders";
        assert_eq!(Statement::typed(cql).cql, cql);
    }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo nextest run -p lc-domain -E 'test(statement)'`
Expected: FAIL — `no function or associated item named 'typed' found for struct 'Statement'`.

- [ ] **Step 3: Implement**

Inside `impl Statement`, after `select_all`:

```rust
    /// A statement typed by hand, classified by its first keyword.
    ///
    /// A sniffer rather than a parser: a statement opening with a comment
    /// classifies as [`StatementKind::Write`] and is refused on a read-only
    /// connection. That is the conservative direction, and it is where a real
    /// CQL parser lands when one arrives
    /// (doc/specs/2026-08-10-query-execution-design.md §4).
    #[must_use]
    pub fn typed(cql: &str) -> Self {
        Self {
            cql: cql.to_owned(),
            kind: kind_of(cql),
        }
    }
```

And at module level, below `quoted`:

```rust
/// An unrecognised verb is mutating: being wrong that way costs an error
/// message, and being wrong the other way costs a write the posture was set to
/// prevent (design §4).
fn kind_of(cql: &str) -> StatementKind {
    let verb = cql.split_whitespace().next().unwrap_or_default();

    match verb.to_ascii_uppercase().as_str() {
        "SELECT" => StatementKind::Read,
        "DROP" | "TRUNCATE" => StatementKind::Destructive,
        "CREATE" | "ALTER" => StatementKind::Schema,
        _ => StatementKind::Write,
    }
}
```

`unwrap_or_default` rather than `unwrap`: `clippy::unwrap_used` warns outside tests, and an empty buffer never reaches here anyway (Task 4 refuses it first).

- [ ] **Step 4: Run them and watch them pass**

Run: `cargo nextest run -p lc-domain -E 'test(statement)'`
Expected: PASS, all six new tests plus the four existing ones.

- [ ] **Step 5: Commit**

```bash
mise run fmt
git add crates/lc-domain/src/model/statement.rs
git commit -m "feat(domain): classify a typed statement by its first keyword"
```

---

### Task 2: The Contents panel takes a subject, not a table

This task changes no behaviour. Every existing test must still pass, and the ones that name `TableRef` are updated to wrap it in `Subject::Table`.

**Files:**
- Modify: `crates/lc-domain/src/contents.rs`
- Modify: `crates/lc-domain/src/view/contents.rs`
- Modify: `crates/lc-domain/src/update.rs` (the `read` function and the tests that read `ContentsState`)
- Test: `crates/lc-domain/src/contents.rs` `mod tests`, `crates/lc-domain/src/view/contents.rs` `mod tests`

**Interfaces:**
- Consumes: `TableRef`, `ContentsState`, `ContentsPanel` — all already in `contents.rs`.
- Produces: `Subject::Table(TableRef)`, `Subject::Typed(String)`, `Subject::label() -> String`, `Subject::table() -> Option<&TableRef>`. `ContentsPanel::requested(&mut self, query: QueryId, subject: Subject)` and `refused(&mut self, subject: Subject, error: DbError)`. Used by Task 4.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/lc-domain/src/contents.rs`:

```rust
    #[test]
    fn a_table_subject_is_labelled_by_its_qualified_name() {
        assert_eq!(
            Subject::Table(TableRef::new("shop", "orders")).label(),
            "shop.orders"
        );
    }

    #[test]
    fn a_typed_subject_collapses_to_one_line() {
        assert_eq!(
            Subject::Typed("SELECT *\n  FROM shop.orders\nLIMIT 10".to_owned()).label(),
            "SELECT * FROM shop.orders LIMIT 10"
        );
    }

    #[test]
    fn only_a_table_subject_names_a_table() {
        assert_eq!(
            Subject::Table(TableRef::new("shop", "orders")).table(),
            Some(&TableRef::new("shop", "orders"))
        );
        assert_eq!(Subject::Typed("SELECT 1".to_owned()).table(), None);
    }

    #[test]
    fn a_typed_subject_travels_the_same_road_a_table_does() {
        let mut panel = ContentsPanel::default();
        let query = QueryId::next();
        panel.requested(query, Subject::Typed("SELECT 1".to_owned()));

        panel.page_ready(query, page(3));

        assert_eq!(panel.row_count(), 3);
        assert_eq!(panel.outstanding(), None, "the wait is over");
    }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo nextest run -p lc-domain -E 'test(contents)'`
Expected: FAIL — `cannot find type 'Subject' in this scope`.

- [ ] **Step 3: Add `Subject` and thread it through `contents.rs`**

In `crates/lc-domain/src/contents.rs`, add below `TableRef`'s `impl`:

```rust
/// What a page in the Contents panel is about.
///
/// The panel shows one grid whichever way the request was made — the tree's
/// `Enter`, or `ctrl-r` over the editor
/// (doc/specs/2026-08-10-query-execution-design.md §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Table(TableRef),
    Typed(String),
}

impl Subject {
    /// What the panel's title says it is showing.
    ///
    /// Every run of whitespace in a typed statement collapses to one space,
    /// newlines included, so a buffer written over four lines still titles the
    /// panel in one. Clipping to the frame is the renderer's job.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Table(table) => table.qualified(),
            Self::Typed(cql) => cql.split_whitespace().collect::<Vec<_>>().join(" "),
        }
    }

    /// The table whose layout a page's columns can be checked against —
    /// `None` for a typed statement, whose columns belong to no one table.
    #[must_use]
    pub const fn table(&self) -> Option<&TableRef> {
        match self {
            Self::Table(table) => Some(table),
            Self::Typed(_) => None,
        }
    }
}
```

Then in the same file replace the `TableRef` field in all three `ContentsState` variants and in the four transitions. The variants become:

```rust
    Loading {
        query: QueryId,
        subject: Subject,
    },
    Loaded {
        subject: Subject,
        page: Page,
    },
    Failed {
        subject: Subject,
        error: DbError,
    },
```

and the methods:

```rust
    /// Starts waiting on `query`, discarding whatever was showing.
    pub fn requested(&mut self, query: QueryId, subject: Subject) {
        self.state = ContentsState::Loading { query, subject };
        self.cursor = Cursor::default();
        self.scroll = Scroll::default();
    }

    /// Applies a page, if it is the one being waited on.
    pub fn page_ready(&mut self, query: QueryId, page: Page) {
        let Some(subject) = self.awaited(query) else {
            return;
        };

        self.state = ContentsState::Loaded { subject, page };
    }

    /// Records a request that never became a query, because the domain
    /// refused it before it could.
    pub fn refused(&mut self, subject: Subject, error: DbError) {
        self.state = ContentsState::Failed { subject, error };
        self.cursor = Cursor::default();
        self.scroll = Scroll::default();
    }

    /// Records a failure, if it belongs to the query being waited on.
    pub fn failed(&mut self, query: QueryId, error: DbError) {
        let Some(subject) = self.awaited(query) else {
            return;
        };

        self.state = ContentsState::Failed { subject, error };
    }
```

and `awaited`:

```rust
    /// What `query` was about, when it is the outstanding one.
    fn awaited(&self, query: QueryId) -> Option<Subject> {
        match &self.state {
            ContentsState::Loading {
                query: outstanding,
                subject,
            } if *outstanding == query => Some(subject.clone()),
            _ => None,
        }
    }
```

In that file's `mod tests`, `loading()` becomes:

```rust
    fn loading() -> (ContentsPanel, QueryId) {
        let mut panel = ContentsPanel::default();
        let query = QueryId::next();
        panel.requested(query, Subject::Table(TableRef::new("shop", "orders")));
        (panel, query)
    }
```

and any other `requested(` / `refused(` call in that module wraps its `TableRef` the same way.

- [ ] **Step 4: Update the view**

In `crates/lc-domain/src/view/contents.rs`, change the import to `use crate::contents::{ContentsState, Subject, TableRef};` and rewrite the four sites:

```rust
pub(super) fn title(state: &State) -> String {
    const BASE: &str = "Table contents";

    match state.contents.state() {
        ContentsState::Empty => BASE.to_owned(),
        ContentsState::Loading { subject, .. } | ContentsState::Failed { subject, .. } => {
            format!("{BASE} — {}", subject.label())
        }
        ContentsState::Loaded { subject, page } => {
            format!("{BASE} — {} ({})", subject.label(), rows(page.rows.len()))
        }
    }
}
```

```rust
        ContentsState::Loaded { subject, page } => grid(state, subject, page),
```

```rust
fn grid(state: &State, subject: &Subject, page: &Page) -> PanelBody {
    if page.rows.is_empty() {
        return waiting("No rows");
    }

    PanelBody::Grid {
        columns: page
            .columns
            .iter()
            .map(|name| GridColumn {
                name: name.clone(),
                token: column_token(state.schema(), subject.table(), name),
            })
            .collect(),
```

The rest of `grid` is unchanged. `column_token` takes the optional table, so a typed statement's columns simply carry no key token:

```rust
/// Where a column sits in the table's physical layout, which vision.md §2
/// exists to keep visible — here, in the grid being read, rather than a panel
/// away.
///
/// `None` for a typed statement: its columns can be a projection, an
/// aggregate, or a join of nothing, and claiming a partition key among them
/// would be a decoration the schema does not support.
fn column_token(schema: Option<&Schema>, table: Option<&TableRef>, column: &str) -> Option<Token> {
    let table = table?;
    let found = schema
        .and_then(|schema| schema.keyspaces.get(&table.keyspace))
        .and_then(|keyspace| keyspace.tables.get(&table.table))?;
```

The body below that line is unchanged.

- [ ] **Step 5: Update `update.rs`'s one construction site**

In `crates/lc-domain/src/update.rs`, `read` currently calls `state.contents.requested(query, table)` and `state.contents.refused(table, error)`. Wrap both:

```rust
        Ok((query, submitted)) => {
            state.contents.requested(query, Subject::Table(table));
            effects.extend(submitted);
        }
        Err(error) => state.contents.refused(Subject::Table(table), error),
```

`table` is moved into the first arm, so bind the subject once before the `match` instead:

```rust
    let stmt = Statement::select_all(&table.keyspace, &table.table, ROW_LIMIT);
    let subject = Subject::Table(table);
    match submit(state, stmt) {
        Ok((query, submitted)) => {
            state.contents.requested(query, subject);
            effects.extend(submitted);
        }
        Err(error) => state.contents.refused(subject, error),
    }
```

Add `Subject` to the `use crate::contents::{...}` line at the top of the file. Then run the compiler and fix every remaining site it names — the tests in `update.rs` that match on `ContentsState::Loaded { table, .. }` become `{ subject, .. }` and compare against `Subject::Table(TableRef::new(…))` or `subject.label()`.

- [ ] **Step 6: Run the whole fast tier**

Run: `mise run test`
Expected: PASS. No test was deleted and none changed its assertion — only how it names the subject.

- [ ] **Step 7: Commit**

```bash
mise run fmt
git add crates/lc-domain/src
git commit -m "refactor(domain): give the Contents panel a subject rather than a table"
```

---

### Task 3: A binding carries its modifier

Also behaviour-preserving. The point is that Task 5's `ctrl-r` row cannot be recorded as a bare `r`, which the sweeps would then press unmodified and call proven.

**Files:**
- Modify: `crates/lc-domain/src/model/input.rs`
- Modify: `crates/lc-domain/src/view/hints.rs`
- Modify: `crates/lc-domain/src/state.rs`
- Modify: `crates/lc-domain/src/commands.rs`
- Modify: `crates/lc-domain/src/update.rs` (the two sweeps)
- Test: `crates/lc-domain/src/view/hints.rs` `mod tests`

**Interfaces:**
- Consumes: `Modifiers` from `crate::model`.
- Produces: `Modifiers::NONE`; `Binding { keys, modifiers, action }`; `hints::label(binding: &Binding) -> String`. Used by Task 5.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/lc-domain/src/view/hints.rs`:

```rust
    #[test]
    fn an_unmodified_binding_is_labelled_bare() {
        assert_eq!(
            label(&Binding {
                keys: &[Key::Char('j'), Key::Down],
                modifiers: Modifiers::NONE,
                action: "move",
            }),
            "j/↓"
        );
    }

    #[test]
    fn a_ctrl_binding_is_labelled_with_its_modifier() {
        assert_eq!(
            label(&Binding {
                keys: &[Key::Char('r')],
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::NONE
                },
                action: "run",
            }),
            "ctrl-r"
        );
    }
```

Add `use crate::model::Modifiers;` to that test module if it is not already imported.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo nextest run -p lc-domain -E 'test(labelled)'`
Expected: FAIL — `struct 'Binding' has no field named 'modifiers'`.

- [ ] **Step 3: Add `Modifiers::NONE`**

In `crates/lc-domain/src/model/input.rs`, add above `impl KeyPress`:

```rust
impl Modifiers {
    /// Nothing held — what a bare key press carries, and what all but one
    /// binding declares.
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
    };
}
```

- [ ] **Step 4: Widen `Binding` and `label`**

In `crates/lc-domain/src/view/hints.rs`:

```rust
/// A binding, keyed by the values `handle_key` actually matches on.
///
/// Not a display string: a string could name a key no arm of `handle_key`
/// claims and nothing would ever catch it, and a bar that lies is worse than
/// no bar (design §2). `update::tests` presses every one of these, modifier
/// included — which is why the modifier is here and not implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub keys: &'static [Key],
    pub modifiers: Modifiers,
    pub action: &'static str,
}
```

```rust
fn rendered(binding: &Binding) -> Hint {
    Hint {
        keys: label(binding),
        action: binding.action,
    }
}

/// Shift is never consulted: a shifted letter already arrives as a distinct
/// `Key::Char` and `shift-tab` is `Key::BackTab`, so no binding sets it.
pub(crate) fn label(binding: &Binding) -> String {
    let keys = binding
        .keys
        .iter()
        .copied()
        .map(name)
        .collect::<Vec<_>>()
        .join("/");

    let mut label = String::new();
    if binding.modifiers.ctrl {
        label.push_str("ctrl-");
    }
    if binding.modifiers.alt {
        label.push_str("alt-");
    }
    label.push_str(&keys);
    label
}
```

Add `Modifiers` to the file's `use crate::model::{...}` line.

- [ ] **Step 5: Add the field to every literal the compiler names**

Run `mise run build`. Every `Binding { … }` literal in `view/hints.rs`, `state.rs` and `commands.rs` fails with E0063 (`missing field 'modifiers'`). Add `modifiers: Modifiers::NONE,` between `keys` and `action` at each one — 38 sites, all mechanical, none of them changing what the binding means. `state.rs` and `commands.rs` each need `Modifiers` added to their `use crate::model::{...}` line.

`commands.rs` also calls `label(binding.keys)` in `rows()` and in `no_command_row_is_wider_than_the_narrowest_palette`; both become `label(binding)`.

- [ ] **Step 6: Make the sweeps press what the binding says**

In `crates/lc-domain/src/update.rs`'s `mod tests`, add beside `press`:

```rust
    fn press_chord(state: &mut State, key: Key, modifiers: Modifiers) -> Vec<Effect> {
        update(state, Msg::Input(KeyPress { key, modifiers }))
    }
```

In `every_hint_the_bar_shows_does_something` and `every_global_hint_does_something_in_every_panel`, replace `press(&mut state, key)` with `press_chord(&mut state, key, binding.modifiers)`, and the assertion messages' `{key:?}` with `{}` fed by `crate::view::hints::label(binding)` so a failure names the chord rather than the bare key.

- [ ] **Step 7: Run everything**

Run: `mise run test`
Expected: PASS. `label` renders every existing binding exactly as before, so the bar and palette tests are untouched.

- [ ] **Step 8: Commit**

```bash
mise run fmt
git add crates/lc-domain/src
git commit -m "refactor(domain): let a binding carry the modifier it is pressed with"
```

---

### Task 4: `ctrl-r` runs the buffer

**Files:**
- Modify: `crates/lc-domain/src/update.rs`
- Test: `crates/lc-domain/src/update.rs` `mod tests`

**Interfaces:**
- Consumes: `Statement::typed` (Task 1), `Subject` (Task 2), and the existing `submit`, `Overlay`, `Message`, `Severity`, `Panel`.
- Produces: `fn ctrl(state: &mut State, key: Key) -> Vec<Effect>`, `fn run(state: &mut State) -> Vec<Effect>`, `fn submit_into_contents(state: &mut State, subject: Subject, stmt: Statement) -> Vec<Effect>` — all private to the module. Task 5 relies on `ctrl-r` working from a Queries-focused state with text in the buffer.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/lc-domain/src/update.rs`. `editing` already exists in that module and focuses Queries with text typed in, and `browsing_tables` connects read-only — a `SELECT` is permitted there.

```rust
    /// `ctrl-r`, the one member of the ctrl layer
    /// (doc/specs/2026-08-10-query-execution-design.md §1).
    fn run_query(state: &mut State) -> Vec<Effect> {
        press_chord(
            state,
            Key::Char('r'),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        )
    }

    #[test]
    fn ctrl_r_runs_the_query_buffer() {
        let mut state = editing("SELECT * FROM shop.orders");

        let effects = run_query(&mut state);

        let query = state
            .contents
            .outstanding()
            .expect("the panel is waiting on the query it just ran");
        assert_eq!(
            effects,
            vec![
                Effect::Execute {
                    conn: current_conn(&state),
                    query,
                    stmt: Box::new(Statement::typed("SELECT * FROM shop.orders")),
                },
                Effect::RecordHistory(Box::new(Statement::typed("SELECT * FROM shop.orders"))),
            ]
        );
    }

    #[test]
    fn a_run_query_titles_the_panel_with_what_was_typed() {
        let mut state = editing("SELECT * FROM shop.orders");
        run_query(&mut state);
        let query = state.contents.outstanding().expect("waiting");

        update(
            &mut state,
            Msg::PageReady {
                query,
                page: Box::new(page_of(3)),
            },
        );

        assert_eq!(state.contents.row_count(), 3);
        let ContentsState::Loaded { subject, .. } = state.contents.state() else {
            panic!("the page landed, so the panel is loaded");
        };
        assert_eq!(
            subject.label(),
            "SELECT * FROM shop.orders",
            "the panel says which query it is showing"
        );
    }

    #[test]
    fn ctrl_r_outside_queries_runs_nothing() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            let mut state = editing("SELECT * FROM shop.orders");
            state.focus = panel;

            assert!(
                run_query(&mut state).is_empty(),
                "the buffer is run from the panel it is typed in, not from {panel:?}"
            );
            assert_eq!(state.contents.outstanding(), None);
        }
    }

    #[test]
    fn an_empty_buffer_runs_nothing() {
        for text in ["", "   \n  \n"] {
            let mut state = editing(text);

            assert!(run_query(&mut state).is_empty());
            assert_eq!(
                state.contents.outstanding(),
                None,
                "an empty buffer is a mistake with no consequence, not an error box"
            );
            assert!(state.overlay.is_none());
        }
    }

    #[test]
    fn running_a_query_cancels_a_pending_table_read() {
        let mut state = read_shop_orders();
        press(&mut state, Key::Enter);
        let read = state.contents.outstanding().expect("the read is in flight");
        state.focus = Panel::Queries;
        type_into(&mut state, "SELECT * FROM shop.users");

        let effects = run_query(&mut state);

        assert!(
            effects.contains(&Effect::Cancel(read)),
            "one grid means one outstanding query"
        );
        assert!(!state.in_flight.contains_key(&read));
    }

    #[test]
    fn reading_a_table_cancels_a_pending_query() {
        let mut state = read_shop_orders();
        state.focus = Panel::Queries;
        type_into(&mut state, "SELECT * FROM shop.users");
        run_query(&mut state);
        let query = state.contents.outstanding().expect("the query is in flight");
        state.focus = Panel::Tables;

        let effects = press(&mut state, Key::Enter);

        assert!(effects.contains(&Effect::Cancel(query)));
        assert!(!state.in_flight.contains_key(&query));
    }

    #[test]
    fn a_destructive_statement_is_refused_until_confirmation_exists() {
        let mut state = editing("DROP TABLE shop.orders");

        let effects = run_query(&mut state);

        assert!(
            effects.is_empty(),
            "nothing reaches the cluster without the confirmation vision.md §4 principle 6 requires"
        );
        assert_eq!(state.contents.outstanding(), None);
        assert!(
            matches!(
                state.overlay,
                Some(Overlay::Message(Message {
                    severity: Severity::Danger,
                    ..
                }))
            ),
            "the refusal says why"
        );
    }

    #[test]
    fn a_typed_write_is_refused_on_a_read_only_connection() {
        let mut state = editing("INSERT INTO shop.orders (id) VALUES (1)");

        let effects = run_query(&mut state);

        assert!(effects.is_empty());
        assert!(
            matches!(state.contents.state(), ContentsState::Failed { .. }),
            "the posture gate refuses it and the panel says so"
        );
    }

    #[test]
    fn an_unbound_ctrl_key_does_nothing() {
        let mut state = browsing_tables(&[("shop", &["orders"])]);
        press(&mut state, Key::Char('l'));

        press_chord(
            &mut state,
            Key::Char('j'),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        );

        assert_eq!(
            state.tables.cursor.index(),
            0,
            "ctrl-j is a binding of its own that does not exist yet, not a slower j"
        );
    }

    #[test]
    fn ctrl_alt_r_does_not_run_the_buffer() {
        let mut state = editing("SELECT * FROM shop.orders");

        let effects = press_chord(
            &mut state,
            Key::Char('r'),
            Modifiers {
                ctrl: true,
                alt: true,
                shift: false,
            },
        );

        assert!(
            effects.is_empty(),
            "two modifiers are a third layer, and no layer is designed"
        );
        assert_eq!(state.contents.outstanding(), None);
    }

    #[test]
    fn an_open_palette_swallows_ctrl_r() {
        let mut state = editing("SELECT * FROM shop.orders");
        state.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        press(&mut state, Key::Escape);
        press(&mut state, Key::Char('?'));

        let effects = run_query(&mut state);

        assert!(
            effects.is_empty(),
            "an open palette is what the user is looking at, and it claims every key"
        );
        assert_eq!(state.contents.outstanding(), None);
    }
```

Two helpers the tests above need. `editing` exists but builds from `browsing(&["a"])`, whose schema has no tables — `read_shop_orders` is what the cancel tests need, so add a typing helper beside `editing` and widen `editing`'s fixture:

```rust
    /// Types `text` into the focused editor, `\n` meaning Enter — the same
    /// convention `editor.rs`'s own helper uses.
    fn type_into(state: &mut State, text: &str) {
        for character in text.chars() {
            if character == '\n' {
                press(state, Key::Enter);
            } else {
                press(state, Key::Char(character));
            }
        }
    }
```

`editing` currently builds on `browsing(&["a"])`, a schema of keyspaces with no tables — the cancel tests need `shop.orders` to exist. Change its first line to `let mut state = browsing_tables(&[("shop", &["orders", "users"])]);` and replace its inline typing loop with a call to `type_into`. Keep its signature and doc comment.

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo nextest run -p lc-domain -E 'test(ctrl_r) + test(run_query) + test(destructive) + test(empty_buffer)'`
Expected: FAIL — the tests compile only after `press_chord` from Task 3 exists; the assertions fail because `ctrl` is still refused wholesale and nothing runs.

- [ ] **Step 3: Split the modifier gate and add the layer**

In `handle_key`, replace the early return:

```rust
    // alt turns a key into a separate binding, refused here rather than
    // reinterpreted, and so does alt on top of ctrl: a two-modifier chord is a
    // third layer and no layer is designed. shift is not checked: a shifted
    // letter already arrives as a different Key::Char, and a shifted arrow is
    // left as a synonym for the plain one so it stays free for a future
    // "extend selection" binding.
    if press.modifiers.alt {
        return Vec::new();
    }

    if press.modifiers.ctrl {
        return ctrl(state, press.key);
    }
```

and add, below `handle_key`:

```rust
/// The ctrl layer's whole key table
/// (doc/specs/2026-08-10-query-execution-design.md §1).
///
/// Dispatched above the palette check, so it honours that rule itself rather
/// than inheriting it: routing a modified key into `palette_key`, which
/// matches on the key alone, would make `ctrl-j` move the palette cursor.
fn ctrl(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return Vec::new();
    }

    match key {
        Key::Char('r') => run(state),
        _ => Vec::new(),
    }
}
```

- [ ] **Step 4: Add `run`, and share the submit path with `read`**

Below `read`, replace its body and add the two new functions:

```rust
/// Replaces whatever the Contents panel is showing with a read of `table`.
fn read(state: &mut State, table: TableRef) -> Vec<Effect> {
    let stmt = Statement::select_all(&table.keyspace, &table.table, ROW_LIMIT);
    submit_into_contents(state, Subject::Table(table), stmt)
}

/// `ctrl-r`: the Queries buffer, whole, as one statement (design §3).
fn run(state: &mut State) -> Vec<Effect> {
    if state.focus != Panel::Queries {
        return Vec::new();
    }

    let text = state.queries.text();
    let cql = text.trim();
    if cql.is_empty() {
        return Vec::new();
    }

    let stmt = Statement::typed(cql);

    // Refused rather than executed: vision.md §4 principle 6 makes typed
    // confirmation the condition on these running at all, and the flow that
    // asks for it is unbuilt. This call site becomes the prompt (design §5).
    if stmt.kind.is_destructive() {
        state.overlay = Some(Overlay::Message(Message {
            title: "Not run".to_owned(),
            message: "Dropping and truncating wait on a typed confirmation, which is not built yet."
                .to_owned(),
            severity: Severity::Danger,
        }));
        state.mark_dirty();
        return Vec::new();
    }

    submit_into_contents(state, Subject::Typed(cql.to_owned()), stmt)
}

/// Puts `stmt`'s result in the Contents panel, cancelling whatever it was
/// waiting on. One grid, one outstanding query, whichever way it was asked for.
fn submit_into_contents(state: &mut State, subject: Subject, stmt: Statement) -> Vec<Effect> {
    let mut effects = Vec::new();

    // Dropping the id is what actually stops a late page from being applied;
    // the effect is still inert (architecture.md §3).
    if let Some(superseded) = state.contents.outstanding() {
        state.in_flight.remove(&superseded);
        effects.push(Effect::Cancel(superseded));
    }

    match submit(state, stmt) {
        Ok((query, submitted)) => {
            state.contents.requested(query, subject);
            effects.extend(submitted);
        }
        Err(error) => state.contents.refused(subject, error),
    }

    effects
}
```

- [ ] **Step 5: Retire the superseded test**

`a_modified_key_is_not_a_binding` asserted that *any* ctrl key is inert, which is no longer true. Delete it — `an_unbound_ctrl_key_does_nothing` from Step 1 is the same assertion over a key the layer does not claim. `a_modified_key_is_still_not_text` stays as written: `ctrl-a` is still not an `a` in the editor.

- [ ] **Step 6: Run the whole fast tier**

Run: `mise run test`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
mise run fmt
git add crates/lc-domain/src/update.rs
git commit -m "feat(domain): run the query buffer with ctrl-r"
```

---

### Task 5: Say so in the bar and the palette

**Files:**
- Modify: `crates/lc-domain/src/state.rs` (`QUERIES_HINTS`)
- Modify: `crates/lc-domain/src/commands.rs` (the catalogue)
- Modify: `crates/lc-domain/src/update.rs` (the two catalogue sweeps)
- Test: `crates/lc-domain/src/view/hints.rs`, `crates/lc-domain/src/commands.rs`, `crates/lc-domain/src/update.rs`

**Interfaces:**
- Consumes: `Binding` with `modifiers` (Task 3), `ctrl-r` working from an editing state (Task 4).
- Produces: nothing later tasks call. Task 6 documents what this adds.

- [ ] **Step 1: Write the failing tests**

In `crates/lc-domain/src/view/hints.rs` `mod tests`:

```rust
    #[test]
    fn the_queries_bar_names_the_run_binding() {
        let mut state = fits();
        state.focus = Panel::Queries;

        let panel = bar(&state).panel;

        assert_eq!(
            panel.first().map(|hint| (hint.keys.as_str(), hint.action)),
            Some(("ctrl-r", "run")),
            "the panel exists to run queries, so the bar says that first"
        );
    }
```

In `crates/lc-domain/src/commands.rs` `mod tests`:

```rust
    #[test]
    fn the_palette_names_the_run_binding() {
        assert!(
            rows().contains(&CommandRow::Command {
                keys: "ctrl-r".to_owned(),
                action: "run the buffer",
            }),
            "the palette claims to be complete, and ctrl-r is the one binding no key types"
        );
    }

    #[test]
    fn the_palette_has_a_queries_section() {
        assert!(
            CATALOGUE.iter().any(|group| group.heading == "Queries"),
            "a panel that takes text still has bindings, and the palette lists every binding"
        );
    }
```

In `crates/lc-domain/src/update.rs` `mod tests`:

```rust
    #[test]
    fn the_editing_keys_do_nothing_outside_a_panel_that_takes_text() {
        for panel in [Panel::Tables, Panel::Ddl, Panel::Contents] {
            for key in [Key::Backspace, Key::Delete] {
                let mut state = populated(panel);
                let before = crate::view::view_model(&state);

                let effects = press(&mut state, key);

                assert!(
                    effects.is_empty() && crate::view::view_model(&state) == before,
                    "{key:?} belongs to the editor and {panel:?} does not take text"
                );
            }
        }
    }
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo nextest run -p lc-domain -E 'test(the_palette_names_the_run_binding) + test(the_queries_bar_names_the_run_binding) + test(queries_section)'`
Expected: FAIL — the bar's first Queries hint is `⏎ newline`, and no group is headed "Queries".

- [ ] **Step 3: Add the hint**

In `crates/lc-domain/src/state.rs`, `QUERIES_HINTS` becomes:

```rust
const QUERIES_HINTS: &[Binding] = &[
    // First: it is what the panel is for. `⏎ newline` and `esc leave` are what
    // it does on the way there.
    Binding {
        keys: &[Key::Char('r')],
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        action: "run",
    },
    Binding {
        keys: &[Key::Enter],
        modifiers: Modifiers::NONE,
        action: "newline",
    },
    // Named here and nowhere else: `Esc` only leaves a panel that takes text,
    // so this is the one bar where saying so is true (hint bar design §3).
    Binding {
        keys: &[Key::Escape],
        modifiers: Modifiers::NONE,
        action: "leave",
    },
];
```

- [ ] **Step 4: Add the palette group**

In `crates/lc-domain/src/commands.rs`, below `TABLES`:

```rust
// The editor's printable keys are not rows: every one of them types itself,
// which is the panel's whole rule rather than a list of bindings
// (doc/shortcuts.md §2). What is listed is what a key does *not* do elsewhere.
const QUERIES: &[Binding] = &[
    Binding {
        keys: &[Key::Char('r')],
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        action: "run the buffer",
    },
    Binding {
        keys: &[Key::Enter],
        modifiers: Modifiers::NONE,
        action: "split the line",
    },
    Binding {
        keys: &[Key::Backspace, Key::Delete],
        modifiers: Modifiers::NONE,
        action: "delete a character",
    },
    Binding {
        keys: &[Key::Home, Key::End],
        modifiers: Modifiers::NONE,
        action: "line start/end",
    },
    Binding {
        keys: &[Key::Escape],
        modifiers: Modifiers::NONE,
        action: "leave the editor",
    },
];
```

and append the group to `CATALOGUE`, after `Tables`:

```rust
    Group {
        heading: "Queries",
        bindings: QUERIES,
    },
```

Check the width rule while writing these: `no_command_row_is_wider_than_the_narrowest_palette` caps keys at 9 columns (`bksp/del` is 8, `ctrl-r` is 6) and actions at 20 (`delete a character` is 18).

- [ ] **Step 5: Teach the catalogue sweeps about the editor**

In `crates/lc-domain/src/update.rs`, `catalogued()` returns `Vec<Key>` and both sweeps press bare keys. Change `catalogued` to return the presses the catalogue actually claims:

```rust
    fn catalogued() -> Vec<KeyPress> {
        CATALOGUE
            .iter()
            .flat_map(|group| group.bindings)
            .flat_map(|binding| {
                binding.keys.iter().map(|&key| KeyPress {
                    key,
                    modifiers: binding.modifiers,
                })
            })
            .collect()
    }
```

Change `does_something` to take a `KeyPress`:

```rust
    fn does_something(state: &State, press: KeyPress) -> bool {
        let mut after = state.clone();
        let effects = update(&mut after, Msg::Input(press));

        !effects.is_empty()
            || after.should_quit != state.should_quit
            || crate::view::view_model(&after) != crate::view::view_model(state)
    }
```

`no_key_does_anything_the_palette_does_not_list` keeps its four fixtures and now sweeps the ctrl layer too, which is what proves `ctrl-r` is its only member:

```rust
    #[test]
    fn no_key_does_anything_the_palette_does_not_list() {
        let catalogued = catalogued();

        for state in sweep_fixtures() {
            for key in every_key() {
                for modifiers in [
                    Modifiers::NONE,
                    Modifiers {
                        ctrl: true,
                        ..Modifiers::NONE
                    },
                ] {
                    let press = KeyPress { key, modifiers };
                    if catalogued.contains(&press) {
                        continue;
                    }

                    assert!(
                        !does_something(&state, press),
                        "{press:?} does something and doc/shortcuts.md's palette does not list it"
                    );
                }
            }
        }
    }
```

`every_key_the_palette_lists_does_something` needs the one fixture the other sweep must not have — a state focused on Queries, where every printable key is text and the "nothing beyond the palette" question has no meaning:

```rust
    /// `sweep_fixtures` plus an editing state. Only this sweep gets it: in a
    /// panel that takes text every printable key does something, so it would
    /// tell `no_key_does_anything_the_palette_does_not_list` that the palette
    /// is missing the alphabet. What is true there instead is pinned by
    /// `a_letter_typed_in_queries_is_text_not_a_shortcut`.
    fn sweep_fixtures_with_editor() -> Vec<State> {
        let mut fixtures = sweep_fixtures();
        let mut editor = editing("SELECT * FROM shop.orders");
        editor.geometry = Geometry::new([8, 14, 20, 1], 37, 14);
        fixtures.push(editor);
        fixtures
    }

    #[test]
    fn every_key_the_palette_lists_does_something() {
        let fixtures = sweep_fixtures_with_editor();

        for press in catalogued() {
            assert!(
                fixtures.iter().any(|state| does_something(state, press)),
                "the palette lists {press:?} and no state responds to it"
            );
        }
    }
```

`home` and `end` in the editor move the cursor within a line, so the editing fixture must have its cursor somewhere they can move it from — `editing` leaves the cursor at the end of the typed text, which `home` moves. If either key reports "no state responds", check that before changing the catalogue.

- [ ] **Step 6: Run the whole fast tier**

Run: `mise run test`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
mise run fmt
git add crates/lc-domain/src
git commit -m "feat(domain): name the run binding in the hint bar and the palette"
```

---

### Task 6: Update the documentation

**Files:**
- Modify: `doc/shortcuts.md` (§1, §2, §3, §5)
- Modify: `doc/specs/2026-08-10-query-execution-design.md` (status)
- Modify: `TODO.md`

**Interfaces:**
- Consumes: everything Tasks 1–5 built.
- Produces: nothing.

- [ ] **Step 1: §1, the Queries table**

Add as the first row of the Queries panel table in `doc/shortcuts.md` §1:

```markdown
| `ctrl-r` | Run the whole buffer. Its page replaces whatever Contents was showing. |
```

and extend the sentence below that table, which currently reads "Nothing else global survives inside the editor — including `q`, which types a `q`.", with: "`ctrl-r` is not global — it is the ctrl layer's only member and does nothing outside this panel."

- [ ] **Step 2: §2, the ctrl rule**

Replace the paragraph beginning **Ctrl and alt are refused wholesale.** with:

```markdown
**Ctrl is one designed layer; alt is refused wholesale.** `handle_key` routes a
ctrl-modified key into `ctrl`, which is the layer's whole key table, and returns
early on alt and on ctrl+alt together — a two-modifier chord is a third layer,
undesigned. What the rule protects is unchanged: a ctrl key is a binding of its
own and never a synonym for the plain one, so `ctrl-j` still does nothing. The
layer has one member, `ctrl-r`, which runs the Queries buffer: the panel takes
text, so every printable key there is a character and the action the panel
exists for has no unmodified key available
([query execution design](specs/2026-08-10-query-execution-design.md) §1).
`ctrl-d`/`ctrl-u` are still unbound and still want designing.
```

- [ ] **Step 3: §3, the budget**

After the "Named keys taken" paragraph, add:

```markdown
Ctrl taken: `ctrl-r`. The rest of the layer is free, with `ctrl-d`/`ctrl-u`
spoken for (§4). Alt is not a layer and nothing may claim from it.
```

- [ ] **Step 4: §5, the enforcement table**

Add these rows:

| Guarantee | Test |
|---|---|
| `ctrl-r` runs the buffer | `ctrl_r_runs_the_query_buffer`, `a_run_query_titles_the_panel_with_what_was_typed` |
| It runs only from Queries | `ctrl_r_outside_queries_runs_nothing` |
| An empty buffer is inert | `an_empty_buffer_runs_nothing` |
| Either path cancels the other | `running_a_query_cancels_a_pending_table_read`, `reading_a_table_cancels_a_pending_query` |
| Destructive is refused, unexecuted | `a_destructive_statement_is_refused_until_confirmation_exists` |
| The posture gate still applies | `a_typed_write_is_refused_on_a_read_only_connection` |
| Ctrl is still not a synonym | `an_unbound_ctrl_key_does_nothing`, `ctrl_alt_r_does_not_run_the_buffer` |
| The palette outranks the ctrl layer | `an_open_palette_swallows_ctrl_r` |
| A statement is classified by its first keyword | the `statement.rs` tests |
| A modifier reaches the label | `an_unmodified_binding_is_labelled_bare`, `a_ctrl_binding_is_labelled_with_its_modifier` (`view/hints.rs`) |
| The bar and palette name it | `the_queries_bar_names_the_run_binding` (`view/hints.rs`), `the_palette_names_the_run_binding`, `the_palette_has_a_queries_section` (`commands.rs`) |
| The editing keys stay inside the editor | `the_editing_keys_do_nothing_outside_a_panel_that_takes_text` |

Also update the existing row for `a_modified_key_is_not_a_binding` — the test was renamed to `an_unbound_ctrl_key_does_nothing` in Task 4.

- [ ] **Step 5: The spec's status, and TODO.md**

In `doc/specs/2026-08-10-query-execution-design.md`, change `**Status:** designed` to `**Status:** implemented`.

In `TODO.md`, add under whatever section holds the Queries entries:

```markdown
- Running the buffer sends all of it as one statement. Splitting on statement
  boundaries, and running only the one under the cursor, needs a CQL parser —
  it lands in `Statement::typed`
  (doc/specs/2026-08-10-query-execution-design.md §3, §10).
- A destructive statement typed into the editor is refused rather than
  confirmed. The typed-confirmation flow vision.md §4 principle 6 requires
  replaces that refusal (design §5).
- A query's result does not page. Neither does a table read; `Effect::FetchNextPage`
  exists and nothing drives it. Both paths get it at once (design §10).
```

- [ ] **Step 6: Run everything CI gates on**

Run: `mise run ci`
Expected: clean — fmt, clippy with warnings denied, licences, advisories, and every fast tier.

- [ ] **Step 7: Commit**

```bash
git add doc TODO.md
git commit -m "docs(shortcuts): record the ctrl layer and the run binding"
```

---

## Verification

After Task 6, confirm by hand rather than by assertion:

1. `mise run ci` is clean.
2. `cargo run -p lc-app` against a live cluster (`docker/`), then: focus Queries with `u`, type `SELECT * FROM system_schema.keyspaces`, press `ctrl-r`, and watch the rows land in Contents with the title naming the statement.
3. In the same session, press `ctrl-r` on an empty buffer (nothing happens), type `DROP TABLE x.y` and press it (a red box, and nothing runs), then `t` and `Enter` on a table (the grid goes back to the table, titled by its name).
