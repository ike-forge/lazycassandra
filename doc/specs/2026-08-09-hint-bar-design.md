# Hint bar — design

**Date:** 2026-08-09
**Status:** implemented

One row along the bottom of the frame naming what the focused panel responds
to. vision.md §7 Phase 0 promises a contextual keybinding bar; §4 principle 1
says discoverability is ambient and you should never open a manual to find out
what you can do to the thing you are looking at. doc/shortcuts.md §2 states
"a binding must be visible" and then admits the only binding that satisfies it
is the focus letter underlined in a title. This closes that.

It builds on the minimum size gate
([design](2026-08-09-minimum-terminal-size-design.md)), which must land first:
the bar needs a row it can count on, and a floor is what guarantees one.

---

## 1. What "contextual" means here

**The focused panel, and nothing finer.** The set changes when focus changes and
at no other time. The cursor sitting on a keyspace rather than a table does not
change a single cell of the bar.

vision.md §4 principle 1 says "contextual to what's focused", which is literally
this. It is also the only granularity that keeps the bar readable: text that
rewrites itself under a held `j` is something you learn to stop looking at, and
a hint you have stopped looking at has failed at its only job.

Two alternatives were considered.

**Cursor-aware** — `⏎` reading "unfold" on a keyspace and "read" on a table — is
truer to the word contextual and teaches that one key has two meanings. It was
rejected for the jitter above, and §3 recovers most of what it offered by
choosing a label that is honest about both.

**A static global line** was rejected for teaching nothing about the panel you
are actually in, which is the entire point.

## 2. The hint model

A hint names keys, not a string that looks like keys:

```rust
// lc-domain/src/view/hints.rs

/// A binding, as the bar will say it.
pub struct Binding {
    pub keys: &'static [Key],
    pub action: &'static str,
}
```

`keys: &'static [Key]` rather than `"j/k"` is the one place this design spends
complexity, and it buys the guarantee that the bar cannot lie. A string can name
a key no arm of `handle_key` claims, and nothing would ever catch it — the bar
would drift out of sync with the bindings exactly as bindings are added and
retired, which is when a user most needs it to be right. Real `Key` values let
§8's property test press every hint the bar shows and require it to do
something. A bar that lies is worse than no bar; this is what stops it.

The cost is a label function, which is text, so the domain owns it
(architecture.md §6):

```rust
fn label(keys: &[Key]) -> String;   // [Char('j'), Char('k')]   => "j/k"
                                    // [PageDown, PageUp]       => "pgdn/pgup"
                                    // [Enter] => "⏎", [Char(' ')] => "space"
```

Multiple keys join with `/` in declaration order, and every label in §3 is
produced by this function rather than written out — so a key that changes name
changes it everywhere at once. The renderer never sees a `Key`.

Each panel owns its bindings beside the letter that focuses it:

```rust
// lc-domain/src/state.rs, beside shortcut()
impl Panel {
    pub const fn hints(self) -> &'static [Binding];
}
```

An exhaustive match, so a fifth panel fails to compile until it declares its
bindings — the same guarantee `ALL`, `next()`, and `shortcut()` already give.
`Queries` declares `&[]`; that is a claim about the panel, not an oversight, and
§3 gives it a sentence.

## 3. What each panel says

| Focus | Panel group | Global tail |
|---|---|---|
| Tables | `j/k move` · `l/h fold` · `⏎ open` | `tab panel` · `q quit` |
| Table DDL | `j/k scroll` · `pgdn/pgup page` · `home/end ends` | `tab panel` · `q quit` |
| Contents | `j/k move` · `pgdn/pgup page` · `home/end ends` | `tab panel` · `q quit` |
| Queries | *nothing here yet* | `tab panel` · `q quit` |

The global tail is what works everywhere. It repeats in all four panels rather
than being learned once and dropped, because a user who never presses `tab` has
no way to discover that three other panels exist.

**The focus letters are not in the bar.** `t`/`a`/`c`/`u` are already underlined
in the titles, which is a better place for them: the binding sits on the thing
it acts on. Restating them costs roughly twenty columns to teach the one binding
that is discoverable without the bar.

**`⏎ open`, not `⏎ read`.** At §1's granularity one label covers both meanings,
and `⏎` on a keyspace folds it rather than reading anything. `open` is true of
both rows; `read` is true of half of them.

**Named keys are spelled out, not drawn.** `home/end` and `pgdn/pgup` rather
than `⇤⇥` and `⇟⇞`: those glyphs are unreadable at a glance, several collide
with each other and with the tab arrow, and a terminal font is free to render
any of them as a box. The bar exists for someone who does not know the binding
yet. `⏎` is the exception — it is on the physical key.

**`esc` is not in the tail.** It dismisses the overlay and does nothing
otherwise, so at §1's granularity it would be a lie in most frames. An overlay
that says how to dismiss itself is the right home for it, and is its own slice.

**Queries says `nothing here yet`.** The panel has no bindings and pretending
otherwise would break §2's guarantee at the first test. The sentence lives in
the projection rather than in `state.rs`, so `hints()` stays a list of bindings
and nothing else:

```rust
// lc-domain/src/view/hints.rs
match panel.hints() {
    [] => HintGroup::Idle("nothing here yet"),
    bindings => HintGroup::Keys(bindings.iter().map(Hint::rendered).collect()),
}
```

## 4. The bar in the ViewModel

```rust
pub struct Hint {
    pub keys: String,           // label(binding.keys)
    pub action: &'static str,
}

pub enum HintGroup {
    Keys(Vec<Hint>),
    Idle(&'static str),
}

pub struct HintBar {
    pub panel: HintGroup,
    pub global: Vec<Hint>,
}
```

`ViewModel` gains `hints: HintBar`, built from `state.focus` alone. `Binding` is
the static source of truth keyed by `Key`; `Hint` is what crosses the boundary,
already text. Two types rather than one because only the first can be checked
against `handle_key` and only the second can be drawn.

## 5. The row

**Depends on the minimum size gate**
([design](2026-08-09-minimum-terminal-size-design.md)), which must land first.
That slice establishes `Shell::Panels | Shell::TooSmall` and a floor of 40×6
derived from the panel floors. This one raises the floor by the bar's height and
takes the row:

```rust
// lc-inbound/src/layout.rs
const BAR_H: u16 = 1;
const MIN_H: u16 = 2 * PANEL_MIN_H + BAR_H;   // 6 -> 7
```

`MIN_W` is unchanged; the bar spends no width. `Shell::Panels` gains the bar's
rect, so the two are produced together and `render` cannot lay out the panels
against the full frame and then draw the bar over one of their rows.

Above the floor the panels receive `area` minus the bottom row. At exactly 40×7
they get 40×6 — `SIDEBAR_MIN_W + MAIN_RESERVE_W` across, two `PANEL_MIN_H` down
— so every panel clears its floor, and every larger frame is at least as roomy.
**The bar therefore never has to be dropped and no drop rule exists.** Below the
floor there is no bar and no panel, only the other slice's notice, whose text
picks up the raised `MIN_H` automatically because it formats from the constant.

That the gate makes a drop rule unnecessary is a benefit of ordering these two
slices this way, not a reason the gate exists; it stands on its own.

**`Geometry` is unchanged.** Panel heights are read off the rects the panels
actually got, so reserving a row needs no arithmetic anywhere else — a panel one
row shorter reports itself one row shorter. The zero-height case is the other
slice's (minimum size design §6) and the bar does not widen it.

## 6. Truncation

The bar composes as one space of indent, hints joined by two spaces, and the two
groups separated by ` │ `. At 80 columns every panel's full bar fits — Tables
comes to 47 columns, Table DDL to 62 — so truncation is a narrow-terminal path,
not the common one. It is still reachable at the gate: 40 columns cannot hold
Tables' 47.

Whole hints are dropped, never part of one, lowest value first:

1. panel hints, right to left, down to the first one;
2. `tab panel`;
3. the first panel hint, and the separator with it;
4. never `q quit`.

The panel's first hint outranks `tab panel` because it is the one thing the bar
knows that the titles do not. `q quit` is last because a user who can read only
one hint needs the way out.

No ellipsis. An absent hint is silent rather than marked: the bar has never
claimed to be the complete list, and two columns spent saying so are two columns
not spent on a hint.

Worked, for Tables. Each row is the widest bar that fits, so the width column is
the threshold at which the row above it stops fitting:

| Width | Bar | Columns |
|---|---|---|
| ≥ 47 | `j/k move  l/h fold  ⏎ open │ tab panel  q quit` | 47 |
| 39–46 | `j/k move  l/h fold │ tab panel  q quit` | 39 |
| 29–38 | `j/k move │ tab panel  q quit` | 29 |
| 18–28 | `j/k move │ q quit` | 18 |
| 7–17 | `q quit` | 7 |
| < 7 | *(empty)* | 0 |

The gate floors width at 40, so only the first two rows are reachable in the
running application. The function is still total over the rest, for the same
reason `split`'s collapse branch is.

Table DDL is the widest bar at 62 columns and truncates twice by the gate: at 40
it shows `j/k scroll │ tab panel  q quit`.

## 7. Drawing it

Two tokens, neither spending a colour, consistent with `Focused`, `Shortcut`,
and `Selected`:

```rust
Token::HintKey,     // the key you press
Token::HintAction,  // what it does
```

`Theme::TerminalDefault` maps `HintKey` to `Modifier::BOLD` and `HintAction` to
the same muted treatment `Token::Muted` already carries, so the bar reads on a
terminal with an unusable palette and never competes with the colours that carry
meaning. The separator draws as `HintAction`.

`lc-inbound/src/hints.rs` owns composition, width arithmetic, and §6's drop
order, and is the only new file in the crate. Truncation measures rendered
labels, which is the renderer's business for the same reason column widths and
wrapping are (panel-scrolling design §2).

## 8. Testing

**Domain.**

- Every panel's `hints()` is exhaustive by construction; `Queries` is the only
  empty one, pinned so emptying another panel's list is a deliberate edit.
- **Every hint does something.** For each panel, for each binding, for each key
  in it: focus that panel in a populated state, press the key, and require
  either the `ViewModel` to differ or an effect to be emitted. `State` does not
  implement `PartialEq` and `handle_key` marks dirty unconditionally, so the
  `ViewModel` is both the available comparison and the honest one — a hint that
  changes nothing the user can see is a hint worth deleting.

  The fixture has to earn this: the schema needs unfolded keyspaces and tables,
  Contents needs a page of rows, Ddl needs a document longer than its panel, a
  `Msg::Measured` must have given the panels a height, and **every cursor and
  offset must start strictly inside its range** — at index 0, `k` and `home`
  move nothing and the test would fail on a correct hint.
- `label` joins on `/` in declaration order and names `Enter` and `Char(' ')`
  readably.

**Render.**

- The bar occupies exactly the bottom row of the frame, and the panels occupy
  every row above it and none of that one.
- Each width in §6's table yields exactly that bar.
- The bar reflects focus: the same state, drawn once per `Panel::ALL`, gives
  four different bars, and the Queries one carries §3's sentence.

**Layout.** `the_panels_tile_the_area_exactly` tiles the frame minus the bar's
row rather than the frame; the size list gains 40×7, where every panel is
non-empty and the bar has its row. The floor moving from 6 to 7 rows updates the
minimum-size slice's `TooSmall` assertions — 40×6 changes side.

Nothing below the floor is retested here. That behaviour, and its notice, belong
to the minimum size slice and are unchanged by this one beyond the constant.

**Theme.** Neither new token sets a background — `no_token_sets_a_background`
grows by two entries.

## 9. Knowingly left out

- **`?` and a full help screen.** vision.md §7 Phase 0 promises the contextual
  map; this is the bar, not the map. The bar is what you see without asking.
- **`esc`, and an overlay that says how to dismiss itself.** §3.
- **Cursor-aware hints.** §1. Reachable later without changing the model: the
  projection would take more than `state.focus`.
- **Wrapping to a second row.** The bar's height is a constant, and `BAR_H` is
  the one place that would change.
- **Colour.** Two modifiers, no palette. Themes are Phase 5.
- **User-defined keybindings.** vision.md §7 Phase 5. `hints()` returning a
  `&'static` slice is exactly what a user-defined binding would have to break,
  and that is the right slice to break it in.

## 10. Documentation

doc/shortcuts.md §2's "a binding must be visible" stops being aspirational and
names the bar. §6's checklist gains the step that a new binding declares itself
in `Panel::hints()` or `GLOBAL`, or says in the commit why it stays hidden. §4
is unchanged: the bar was never in its table of deliberately-unbound keys.

---

## Amendment — 2026-08-09, queries editor

§3's "Queries says *nothing here yet*" and the `HintGroup::Idle` variant of §4
are both retired. The Queries panel takes text and names its own bindings, so
no panel has an empty hint list and nothing can construct `Idle`.
`HintBar.panel` is a `Vec<Hint>`.

§9's "`esc`, and an overlay that says how to dismiss itself" is half-retired:
the Queries panel's bar now names `esc leave` (`state.rs`'s `QUERIES_HINTS`),
so `esc` is no longer wholly left out. §3's "`esc` is not in the tail" still
holds — `esc leave` is a panel hint, not a `GLOBAL` one, so it names nothing
in the other three panels' bars. The overlay's own dismiss-hint half of the
§9 bullet is still left out, and the bar's promise is itself imperfect while
Queries has focus: an overlay up over it still spends `Esc` on dismissing the
overlay rather than leaving, which the bar does not say (TODO.md).

`GLOBAL` also stopped being unconditional. `q quit` is dropped for a panel that
takes text, where `q` types a `q` — §2's rule that a bar which lies is worse
than no bar, applied to the bar's own tail. `tab panel` is unaffected and is
still the way out.

See [queries editor design](2026-08-09-queries-editor-design.md) §6.
