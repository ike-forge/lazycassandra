# Panel focus shortcuts — design

**Date:** 2026-08-09
**Status:** implemented

Four panels, four keys: `t` Tables, `a` Table DDL, `c` Contents, `u` Queries.
Each panel's title underlines the letter that reaches it, so the binding is
learned by looking at the panel rather than by reading a manual — vision.md §4
principle 1 (discoverability is ambient) applied to the focus model that
principle 4 already established.

`Tab` and `BackTab` cycling stays exactly as it is. This is additive.

---

## 1. The keys

| Panel | Key | Underlined in the title |
|---|---|---|
| `Tables` | `t` | **T**ables |
| `Ddl` | `a` | T**a**ble DDL |
| `Contents` | `c` | Table **c**ontents — … |
| `Queries` | `u` | Q**u**eries |

Two of the four are not leading letters, and no title is renamed to make them
leading: panel names are the ones `doc/ui-draft.png` fixed, and principle 4
says the layout does not rearrange itself under the user.

Lowercase only. `T` arrives as a distinct `Key::Char('T')` and stays
unbound, consistent with every other binding in `handle_key`.

## 2. Where the shortcut lives

`Panel` owns its key, beside `next()` and `previous()`:

```rust
impl Panel {
    pub const fn shortcut(self) -> char;
    pub fn from_shortcut(key: char) -> Option<Self>;
}
```

`shortcut` is an exhaustive match, so a fifth panel fails to compile until it
declares a key — the same guarantee `ALL` and `next()` already give.
`from_shortcut` is its inverse over `ALL`.

`handle_key` gains one arm, placed last so the existing bindings keep
precedence by match order:

```rust
Key::Char(key) => {
    if let Some(panel) = Panel::from_shortcut(key) {
        state.focus = panel;
    }
}
```

Global, unconditional, and independent of connection state — focus is a
property of the shell, not of the session. Pressing the focused panel's own key
is a no-op that still marks dirty, because `handle_key` marks dirty on entry.

**The CQL editor is not designed for here.** When the Queries panel takes text
input, every letter becomes contended, and the mechanism that resolves that —
a panel that swallows keys, or a mode — gets designed against the real editor.
Adding a `handle_global`/`handle_focused` seam now would be guessing at its
shape.

## 3. The marked title

The letter's position cannot be found at draw time: the Contents title is
built per frame (`Table contents — shop.orders (3 rows)`), and the match is
case-insensitive (`t` marks `T`). So the mark is computed where the title is
built, in the domain, and travels in the `ViewModel`.

`PanelView::title` changes from `String` to:

```rust
pub struct PanelTitle {
    text: String,
    /// Byte range of the shortcut letter, always on a char boundary.
    mark: Option<Range<usize>>,
}

impl PanelTitle {
    pub(crate) fn marking(text: String, shortcut: char) -> Self;
    pub fn text(&self) -> &str;
    /// The title in three pieces: before the letter, the letter, after it.
    pub fn split(&self) -> (&str, &str, &str);
}
```

The field is private for the reason `Cursor::index` is: validated once at
construction, so no renderer is ever handed an index that slices a character in
half. `marking` takes the first case-insensitive occurrence, which for every
title lands in the fixed prefix, ahead of any keyspace or table name.

`mark` is `Option` so the type is total — a title that does not contain its own
letter yields `(text, "", "")` and simply draws unmarked, rather than panicking
or forcing an `unwrap` at a construction site. That case is a bug, and §5 pins
it shut with a test rather than a runtime check.

`view::title()` becomes `PanelTitle::marking(text, panel.shortcut())`, which
puts the shortcut and the string that must contain it in one expression.

## 4. Drawing it

A new semantic token, not a modifier hardcoded in `render.rs`
(architecture.md §6):

```rust
Token::Shortcut,  // the key that focuses this panel
```

`Theme::TerminalDefault` maps it to `Modifier::UNDERLINED` and nothing else —
no foreground, no background, so it reads on a terminal with an unusable
palette and does not compete with the colours that carry meaning. Same
reasoning as `Focused` (bold) and `Selected` (reversed).

`render.rs` builds the title from `split()` as three spans instead of passing a
`&str`. Unmarked pieces carry no style, so the block's existing appearance is
unchanged apart from the one underlined cell. The underline is drawn on every
panel, focused or not: it is a label, not a state indicator.

## 5. Testing

**Domain.**

- Each shortcut focuses its panel from every starting focus, and emits no
  effects.
- The four shortcuts are distinct (`every_panel_has_a_distinct_shortcut`, a
  property test over `Panel::ALL`). Disjointness from the bindings
  `handle_key` already claims (`q`, `j`, `k`, `l`, `h`, space) is proven
  behaviourally rather than by naming those keys:
  `a_shortcut_focuses_its_panel_from_any_other_panel` requires every
  from/to pair over `Panel::ALL` to actually land on `to`, so a panel
  claiming one of those keys would have its arm swallowed by the earlier
  binding and the assertion would fail.
- `from_shortcut` is the inverse of `shortcut` over `ALL`, and rejects an
  unbound letter.
- Every panel's title marks its own shortcut, checked across every
  `ContentsState` variant so the dynamic title cannot drift out of the
  invariant.
- `split()` reassembles to `text()`.

**Render.** The title's marked cell carries `UNDERLINED` and its neighbours do
not. The existing row-string assertions in `render.rs` are untouched — the
text does not change, only the styling of one cell.

**Theme.** `Token::Shortcut` sets no background; the existing
`no_token_sets_a_background` list grows by one entry.

## 6. Knowingly left out

- **Uppercase synonyms.** `T`/`A`/`C`/`U` stay unbound.
- **A keybinding hint bar.** vision.md §7 Phase 0 promises one; it is its own
  piece of work and this design does not anticipate its shape.
- **User-defined keybindings.** vision.md §7 Phase 5.

---

## Amendment — 2026-08-09, queries editor

§5's "every from/to pair over `Panel::ALL`" is narrower than stated once the
Queries panel takes text (queries-editor-design.md §3). From Queries,
`t`/`a`/`c`/`u` are text, not bindings, so a from/to pair starting there no
longer lands and would fail the assertion as originally written.

The test was renamed to
`a_shortcut_focuses_its_panel_from_any_panel_that_does_not_take_text` and its
`from` side narrowed to panels that don't take text — three of the four, not
all four.

**The disjointness proof this section rests on survives intact**, and that is
the point worth stating: the inner loop still spans all four `to` panels from
each of the three non-text `from` panels, so a panel claiming a key an
earlier `handle_key` arm already takes still has its arm swallowed and still
fails the assertion. Nothing about *why* the test catches a shadowed arm has
changed; only which starting panels it can honestly claim to cover.

`a_shortcut_is_text_in_a_panel_that_takes_text` pins the half the narrowing
gave up: from Queries, a focus letter is text and no focus change happens at
all.

See [queries editor design](2026-08-09-queries-editor-design.md) §3.
