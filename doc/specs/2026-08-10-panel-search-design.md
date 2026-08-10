# Panel search — design

**Date:** 2026-08-10
**Status:** implemented

`/` opens a prompt on the hint bar row. What you type filters or highlights the
focused panel, live, and stays applied — shown greyed at the top right of that
panel — until `esc` clears it.

Three panels get it in this slice: Tables and Table DDL *filter* their rows,
Table contents *highlights* the cells that match. Queries gets nothing here and
§8 says why.

doc/shortcuts.md §3 lists `/` as unspent. This slice spends it.

---

## 1. Why the prompt is an overlay

doc/shortcuts.md §2 states the regime: *focus is the mode, and an overlay
outranks focus*. A panel either takes text or it does not, and
`Panel::takes_text` is the one exhaustive answer. Tables, Table DDL and Table
contents all answer no — so there is nowhere for a typed query to go without
breaking the rule the whole binding set rests on.

The command palette already established the way out. It is drawn over the
panels, claims every unmodified key before focus is consulted — only the ctrl
layer is dispatched above it (§3.1) — and is entered and left deliberately. The
search prompt is the same shape of thing: a state that outranks focus, entered
with one key, left with two.

So it joins `Overlay`:

```rust
pub enum Overlay {
    Message(Message),
    Commands(CommandsPanel),
    Search(Prompt),
}
```

**Not a new `State` field.** `Overlay`'s own doc comment already argues this:
two fields would make "a failure and the palette, simultaneously"
representable, and the renderer would then need a precedence rule the domain
never stated. A prompt beside the palette would make exactly that mistake a
second time. One variant, one enum, and "a palette and a prompt at once" cannot
be written down.

The cost is that `Overlay` stops meaning *a box drawn over the panels* and
starts meaning *what outranks focus*. The prompt is drawn on the bar row, not
over the panels. That is a doc-comment change in `state.rs`, not a behaviour
change: every consequence of being an overlay — claiming keys first, being
dismissed by `esc`, being spent by a click — is one this prompt wants.

### 1.1 The prompt claims every printable key

While it is up, `q` types a `q` and `?` types a `?`. This is the
panel-that-takes-text rule applied to an overlay, and it is the opposite of the
palette, which leaves `q` quitting.

The two differ because the palette takes *motion* and the prompt takes *text*.
There is no ambiguity to resolve: a user typing a search term has typed `/`
one keystroke earlier, and the bar in front of them says `esc cancel`.

---

## 2. The filter type

New module, `lc-domain/src/search.rs`:

```rust
pub struct Filter(String);

impl Filter {
    pub fn new(raw: &str) -> Option<Self>;
    pub fn matches(&self, haystack: &str) -> bool;
    pub fn as_str(&self) -> &str;
}
```

The invariant is **non-empty**, and the folded form is computed once at
construction rather than per candidate. `Option<Filter>` *is* "no filter", so
an empty filter is unrepresentable — and "committing an empty query clears the
filter" is then not a rule anyone has to remember to check. It is what
`Filter::new("")` returning `None` already means, stored directly.

Matching is **case-insensitive substring**, no metacharacters. Every keystroke
is literal, so no half-typed input is ever a syntax error the live prompt has
to render, and nothing unbounded sits on the per-keystroke path. `as_str`
returns the term as typed, because that is what the marker in §5.1 shows.

Folding is `str::to_lowercase`, not `to_ascii_lowercase`: identifiers are
ASCII, but the Table contents panel matches against cell values, which are
whatever the cluster stored.

### 2.1 Which panels can hold one

```rust
pub enum Filtered { Tables, Ddl, Contents }

impl Filtered {
    pub const fn of(panel: Panel) -> Option<Self>;  // Queries is the only None
    pub const fn panel(self) -> Panel;
}
```

`Option` rather than `TryFrom`: there is one way to fail and nothing to say
about it, so an error type would exist only to be discarded.

Queries has no filter this slice. Keying the filter off `Panel` would leave a
`Panel::Queries => {}` arm in every accessor, unreachable by design and rotting
until the vim slice arrives. `Filtered` makes

```rust
impl State {
    pub fn filter(&self, target: Filtered) -> Option<&Filter>;
    /// Sets it and repairs what it invalidates (§3.3).
    pub fn apply_filter(&mut self, target: Filtered, filter: Option<Filter>);
}
```

`apply_filter` rather than a `&mut Option<Filter>` handed out: a caller that
sets the filter and forgets the clamp leaves a cursor pointing past the rows
it now has, and there is no reason for that to be a caller's job.

total. When Queries gains search, it gains a variant here and the compiler
lists the places that must answer for it.

### 2.2 Where a filter lives

Beside the state it filters, not in a table on `State`:

- `TablesPanel.filter: Option<Filter>`
- `ContentsPanel.filter: Option<Filter>`
- `state.ddl` becomes
  `DdlPanel { cursor: Cursor, scroll: Scroll, filter: Option<Filter> }`, in a
  new `lc-domain/src/ddl.rs`

The Table DDL panel is the one that grows a struct where it had a bare
`Scroll`. That follows the reason `tree.rs` and `contents.rs` exist at all
(architecture.md §11): per-panel state with its own vocabulary, rather than
another field on `State`. The `cursor` is the Table DDL selection slice's, and
only strengthens the argument — a panel with a cursor, a viewport and a filter
is exactly the per-panel state that section is about.

`cursor` and `scroll` are public; `filter` is not, and is reached through
`filter()` / `set_filter()` for the reason `TablesPanel`'s is (§2.1): setting
one owes a repair, and a public field would let a caller skip `apply_filter`.

A filter therefore survives a focus change, and the three are independent — you
can have one applied in Tables and a different one in Table contents.

---

## 3. The prompt

```rust
pub struct Prompt {
    target: Filtered,
    buffer: String,
    previous: Option<Filter>,
}
```

`target` is captured when the prompt opens, from the panel that had focus. It
is not re-read per keystroke: `tab` is inert while the prompt is up (§3.2), so
the panel being filtered cannot change under a live query.

`previous` is the filter that was applied before the prompt opened. Live
filtering overwrites the target's filter on every keystroke, so cancelling has
to have something to put back — see §3.1.

### 3.1 Its keys

| Key | Effect |
|---|---|
| any printable key | Appends to `buffer`, then applies (§3.3). |
| `Backspace` | Removes the last character, then applies. |
| `Enter` | Closes the prompt, leaving whatever is applied. |
| `Esc` | Restores `previous` to the target and closes. |
| anything the ctrl layer does not claim | Inert. |

The last row said "anything else" when this was written, and that is no longer
true: the ctrl layer landed in another slice, and `handle_key` routes a
ctrl-modified key into `ctrl_key` *above* the palette and therefore above this
prompt (doc/shortcuts.md §2). So `ctrl-e` and `ctrl-y` still scroll the Table
DDL viewport with a prompt open over it, and `ctrl-r` would run the Queries
buffer if a prompt could be open while Queries had focus — it cannot, and that
takes two halves. Queries holds no filter, so no prompt can be opened *over*
it; and focus cannot move *to* it under a live prompt, since the prompt
swallows `tab`/`shift-tab` (§3.2), types the panel-shortcut letters as text
(§1.1), and a click is spent dismissing the prompt rather than reaching the
panel beneath it (§3.2). Both halves are needed: the first alone leaves
`ctrl-r` one focus change away from firing under a live query.

The prompt is inert to the rest, and being reachable-past is right rather than
merely tolerable. Ctrl is a layer of its own, designed as a layer and not
smuggled in one key at a time; a member moves no focus and changes no `target`,
which §3 captures at open, so the panel under a live query cannot change
identity beneath it. And where a member raises a message overlay in place of
the prompt, `raise` restores `previous` to the target before replacing it, so a
displaced prompt leaves the panel exactly as it was found — the same guarantee
`Esc` gives.

`Enter` on an empty buffer therefore clears the filter, because §2's invariant
already turned an empty query into `None`. There is no separate "clear"
binding to learn and none to test.

`Esc` restoring `previous` is what makes live filtering safe to explore with: a
prompt opened over an applied filter and then abandoned leaves the panel
exactly as it was found.

### 3.2 What it does not claim

`tab` and `shift-tab` are inert rather than focus-moving. This is a departure
from Queries, where they still leave, and it follows from `target` being
captured: a `tab` that moved focus would leave a live prompt filtering a panel
the user is no longer looking at. It is not a new departure for an overlay —
the palette already swallows both, per
`an_open_palette_swallows_the_keys_it_does_not_use`.

The prompt is dismissed by a click, like any other overlay — doc/shortcuts.md
§7, *a click spends itself on the overlay*. A click cancels rather than
commits, for the same reason `esc` does: a click that lands somewhere
unexpected should not leave state behind.

### 3.3 Live, per keystroke

Every edit to `buffer` writes `Filter::new(&buffer)` straight into the target's
filter. There is no separate committed-vs-pending copy; the panel underneath
*is* the preview.

A filter shrinks what a panel holds, so `apply_filter` clamps the **Tables
cursor** to the filtered row count with the existing `Cursor::clamp_to`.

The **Table DDL** offset is not clamped there, and deliberately. Its length is
counted in wrapped lines, which only the renderer can produce — the domain
learns it from `Msg::Measured`, whose handler already clamps
`state.ddl.scroll` and marks dirty when that moved. A filter applied at
keystroke *n* is drawn at frame *n*, measured, and repaired on the message that
comes back. Clamping against `geometry.ddl_lines()` at apply time would clamp
against the *previous* frame's count, which is a guess, and a wrong one exactly
when the filter just shrank the body.

The panel's **cursor** is a different matter, and needs no special case at all.
It counts rows, which the domain can count for itself, and `view::ddl_rows`
counts the rows the panel *draws* — `details::of` applies the filter before
returning, so the filtered count is the only count anyone sees. `clamp_lists`
runs in `update`'s epilogue on every message and clamps the cursor against it,
so a filter that empties a section pulls the cursor back on the same keystroke
that applied it, with no line count involved.

Table contents needs neither: it filters nothing (§4.3).

`open_prompt` clears the target's filter the moment the prompt opens, rather
than leaving the old filter showing until the first keystroke: an empty
buffer live-applies to no filter, so the panel underneath is the preview from
frame one, and `Esc` restores it via `previous` (§3.1) exactly as if it had
never been cleared.

A filter that shrinks the Table DDL body below a scrolled offset draws one
blank frame before `Msg::Measured` clamps it — the frame between the
keystroke landing and the remeasure coming back. The no-clamp-at-apply-time
reasoning above is what causes it, and it is sound; the artifact is one frame
wide and the loop terminates, so it is a known cost of that reasoning rather
than a bug.

---

## 4. What each panel does with it

### 4.1 Tables — filters, and forces matches open

`TablesPanel::rows` is the one flattening both `update`'s motions and
`view_model` read, so the filter is applied there and the two cannot disagree
about what row 5 is.

Retention, per keyspace:

| Case | Rows produced |
|---|---|
| The keyspace name matches | The keyspace, and **all** its tables |
| Only some of its tables match | The keyspace, and **only** the matching tables |
| Neither | None — the keyspace is dropped |

A keyspace retained under a filter is drawn **unfolded**, whatever its fold
state. The alternative hides the filter's own results: a match inside a folded
keyspace would make the panel report nothing while the row sits one keypress
away.

**`l` and `h` on a keyspace are swallowed while a filter is applied.** They
cannot be honoured — the projection forces retained keyspaces open, so folding
would remove the keyspace from the unfold set and change nothing on screen, and
a key that looks broken is worse than a key that is inert. Swallowing is the
existing answer for this case: doc/shortcuts.md §2, *a panel that cannot honour
a motion swallows it rather than passing it on*, already tested by
`a_panel_with_no_rows_swallows_motion_rather_than_passing_it_on`.

The private `unfolded` set is **never written by filtering**. Clearing the
filter therefore restores the fold state exactly as it was before `/` was
pressed. This is the reason the forcing is a projection concern and not a
mutation: a filter that seeded the unfold set would accumulate an unfold per
prefix as the user typed `o`, `r`, `d`.

`⏎` on a table still reads it. Filtering changes which rows are reachable, not
what a row does.

### 4.2 Table DDL — filters rows, not headings

`view::details` drops the `DetailRow`s that miss:

- `DetailRow::Property` matches on its **name or its value**.
- `DetailRow::Column` matches on its **name or its CQL type**.

A `Section` left with no rows disappears, which is the mechanism `Section`'s
own doc comment already describes — a group with nothing in it disappears by
being empty rather than by a projection remembering to skip it. Filtering needs
no new code for that; it only has to produce the empty group.

Section headings (`Partition Key`, `Options`) and `Details.heading` (the table
name) are **not** matched. The search is over properties and values, and a
heading match would resurrect a whole section whose rows all missed.

### 4.3 Table contents — highlights, filters nothing

The grid keeps every row, in order. `GridCell` gains a field:

```rust
pub struct GridCell {
    pub text: String,
    pub token: Option<Token>,
    /// Whether the applied filter matched this cell's text.
    pub matched: bool,
}
```

A **separate field, not a second meaning for `token`**. `token` carries
`Some(Token::Muted)` for a null, which is what keeps a null apart from a text
column whose value is the word "null"; overwriting it on a match would lose
that distinction for exactly the cells the user just went looking for.

The renderer paints a matched cell with a new `Token::Match` (§5.4). Where a
cell is both — a matched null — the renderer **composes** `Token::Match` with
`Token::Muted` rather than letting one replace the other: `Match` is fg-only
amber and `Muted` is `DIM`, so the two combine (`Style::patch`, the idiom
`body.rs` already uses for the selected row) into a style that reads as a
match and still reads as a null.

**Cell-level, not span-level.** A wide value is truncated at the panel edge, so
a span highlight would silently fail to show that a cell matched whenever the
match sits past the cut. Colouring the whole cell is true regardless of where
the panel had to stop drawing.

Nothing else changes: the cursor does not seek matches, no column is hidden,
and the row count in the title still counts rows the cluster returned.

### 4.4 Nothing matched

Tables and Table DDL fall back to a notice, in `Token::Muted`:

```
No keyspace or table matches "ord"
```

```
No property matches "ord"
```

This reuses `PanelBody::Notice`, the same shape already used for *Loading
rows…* and *Press Enter on a table*.

Table contents gets none. It filters nothing, so a query that matches no cell
leaves a correct, complete grid on screen with no highlights in it — which is
already the answer, and a notice would have nowhere to go that was not covering
the rows it is describing.

---

## 5. What is on screen

### 5.1 The marker

`PanelView` gains `filter: Option<String>` — the term as typed.

The renderer right-aligns `/ord` on the panel's **top border**, in
`Token::Muted`, and **drops it entirely** when the border cannot hold both the
title and the marker. Giving way rather than truncating is the hint bar's
existing rule for a row that has run out of room, and a truncated filter term
is a lie about what is applied.

It is drawn in every panel that has one, focused or not. That is the point of
it: a filter that survives a focus change needs to say so from the panel it is
applied to, not from the bar, which only ever describes the focused panel.

### 5.2 The prompt on the bar row

`OverlayView::Search { query: String }`, drawn at the left of the bar row as
`/ord` with the cursor after it.

`hints::bar` grows a third early return beside the palette's, naming the
prompt's own keys on the right of the same row:

```rust
const SEARCH_GLOBAL: &[Binding] = &[
    Binding { keys: &[Key::Enter],  action: "apply"  },
    Binding { keys: &[Key::Escape], action: "cancel" },
];
```

The panel group is empty while the prompt is up: the focused panel responds to
nothing, so naming its bindings would be the kind of lie the bar's design §2
exists to prevent.

### 5.3 `esc clear`, and `/ search`

doc/shortcuts.md §2 requires every binding to be visible somewhere, and the
hint bar design §3 restricts `esc` to bars where pressing it unconditionally
does something.

- **`/ search`** joins the focused panel's group in Tables, Table DDL and Table
  contents. It is a panel binding, not a global one: it does nothing in
  Queries.
- **`esc clear`** joins that group only when the focused panel **has a filter
  applied**. In that frame it is unconditionally true, which is exactly the
  test the existing restriction sets.
- The palette catalogue gains a `/ search` row.

Both are subject to the bar's existing width-fitting: they are dropped from a
narrow frame like any other hint, and the palette is where the complete list
lives.

### 5.4 The token

```rust
pub enum Token {
    // …
    /// A cell whose text answered the applied filter.
    Match,
}
```

The domain names the meaning; the theme in `lc-inbound` decides how to draw it
(architecture.md §6). It must stay legible over `Token::Selected`, since the
cursor's row can contain matches.

---

## 6. `esc` grows a precedence chain

`handle_key`'s `esc` arm resolves in this order, first hit wins:

| Order | Condition | Effect |
|---|---|---|
| 1 | An overlay is up | Dismiss it — a prompt cancels, restoring `previous` |
| 2 | The focused panel has a filter | Clear it |
| 3 | Queries has focus | Leave the editor for Tables |
| 4 | Otherwise | Nothing |

1 before 2 preserves the existing
`escape_dismisses_the_overlay_before_it_leaves_the_editor`, and extends the
same logic: the thing most recently put on screen comes off first.

2 before 3 means `esc` in a filtered Queries panel would clear before leaving —
unreachable this slice, since Queries cannot hold a filter (§2.1), and the
ordering is stated now so the vim slice inherits a decision rather than making
one.

---

## 7. Not built

- **Match navigation.** No `n`/`N`, no cursor that seeks the next match. The
  filter already puts the matches on screen in Tables and Table DDL; in Table
  contents the highlights are visible in the rows the user is scrolling anyway.
- **Regex, fuzzy matching, ranking.** §2 argues the substring choice. Ranking
  in particular would fight the fixed display order Tables and Table DDL both
  depend on.
- **Column filtering in Table contents.** Filtering rows out of a page would
  make the title's row count a lie about what is drawn, and hiding columns is
  its own slice (vision.md §8's unbuilt column navigation is the same area).
- **A shared filter across panels.** Three independent filters, because the
  three panels are looking at three different things.
- **History.** The prompt opens empty every time.

---

## 8. Queries, and the vim slice

Queries takes text, so `/` types a `/` there — CQL comments and division both
need it — and every other printable key is taken with it. That is the whole of
the obstacle: inside the editor there is no unmodified key left for search to
claim, and claiming one anyway would break the panel-takes-text rule, which is
the rule the editor exists under rather than an inconvenience beside it.

The ctrl layer is no longer an argument here either way. This section once
rested on doc/shortcuts.md §2 refusing ctrl wholesale, and on that refusal
saying the layer must be *designed as a layer, not smuggled in one key at a
time*. The layer has since been designed, in the query execution and Table DDL
selection slices, so the prohibition it appealed to is gone. The conclusion is
unchanged, because the reason was never really the prohibition: a `ctrl-/` for
search would be a fourth member added for the convenience of one panel, and it
would put search behind a different key in Queries than in the other three —
the one thing doc/shortcuts.md §2's *same key means the same thing in every
panel* rule is for.

The route in is `esc` then `/`, once Queries has a vim-style normal mode: `esc`
leaves insert mode instead of leaving the panel, and `/` in normal mode is
search — the same `/`, in a panel that has stopped taking text. That is the vim
slice's decision to make, not this one's — this slice would have to invent the
whole mode split, including how one leaves the panel at all once `esc` no
longer does it.

What this slice owes that slice is that nothing has to be redesigned when it
lands:

- `Prompt` targets a `Filtered`, which gains a `Queries` variant and a compiler
  error list.
- Highlighting is already the Table contents shape (§4.3) — a flag on a
  projected unit, painted with `Token::Match`. Queries wants spans within a
  line rather than whole cells, which is a new projection field on the editor
  body, not a new mechanism.
- §6's `esc` chain already states where a Queries filter would sit.

Until then, `esc` in Queries leaves the editor, exactly as
`escape_leaves_the_editor_for_tables` pins it today.

---

## 9. Tests

`lc-domain` unless noted.

| Guarantee | Where |
|---|---|
| An empty query is no filter | `search.rs` |
| Matching folds case, and is substring | `search.rs` |
| `Filtered` refuses Queries and accepts the rest | `search.rs` |
| A matching keyspace brings all its tables | `tree.rs` |
| A matching table brings its keyspace and no sibling | `tree.rs` |
| A keyspace with nothing matching is dropped | `tree.rs` |
| A retained keyspace is drawn open whatever its fold state | `tree.rs` |
| Clearing the filter restores the fold state exactly | `tree.rs` |
| `l`/`h` are swallowed while a filter is applied | `update.rs` |
| The cursor clamps as a filter shrinks the tree | `update.rs` |
| A property matches on its name and on its value | `view/details.rs` |
| A column matches on its name and on its CQL type | `view/details.rs` |
| A section emptied by the filter disappears | `view/details.rs` |
| Headings are not matched | `view/details.rs` |
| The scroll offset clamps as a filter shrinks the lines | `update.rs` |
| Matching cells are flagged and the grid keeps every row | `view/contents.rs` |
| A matched null is still a null | `view/contents.rs` |
| Nothing matched gives a muted notice in Tables and Table DDL | `view/*.rs` |
| `/` opens the prompt in each of the three panels | `update.rs` |
| `/` is text in Queries and opens nothing | `update.rs` |
| `q` and `?` type themselves in the prompt | `update.rs` |
| The prompt filters live, per keystroke | `update.rs` |
| `⏎` commits and closes; an empty `⏎` clears | `update.rs` |
| `esc` cancels, restoring the filter that was there | `update.rs` |
| `esc`'s full precedence chain | `update.rs` |
| `tab` is inert while the prompt is up | `update.rs` |
| The ctrl layer reaches past the prompt; an unclaimed chord is still not text | `update.rs` |
| A filter that shrinks Table DDL pulls its cursor back | `update.rs` |
| A click cancels the prompt | `update.rs` |
| A filter survives a focus change | `update.rs` |
| The three filters are independent | `update.rs` |
| The bar names `/ search` where it does something | `view/hints.rs` |
| The bar names `esc clear` only where a filter is applied | `view/hints.rs` |
| The bar names the prompt's own keys while it is up | `view/hints.rs` |
| The palette lists `/` | `commands.rs` |
| The marker is drawn, muted, right-aligned | `inbound/render.rs` |
| The marker is dropped rather than truncated | `inbound/render.rs` |
| The prompt owns the bar row | `inbound/render.rs` |
| `Token::Match` is legible over `Token::Selected` | `inbound/theme.rs` |

---

## 10. Documentation

In the same commit, per doc/shortcuts.md §6:

- **§1** — `/` in the global table; `esc`'s chain; `l`/`h` swallowed under a
  filter in the Tables subsection.
- **§2** — the prompt as an exception to *bindings are global; panels refuse*,
  ordered after the ctrl layer and the palette and before a panel that takes
  text; and `esc`'s precedence chain as a named rule.
- **§3** — `/` moves from unspent to taken.
- **§5** — the rows from §9 above.
- **§7** — a click cancels an open prompt.
