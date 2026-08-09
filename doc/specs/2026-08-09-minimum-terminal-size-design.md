# Minimum terminal size — design

**Date:** 2026-08-09
**Status:** implemented

Below some size the four-panel shell stops being a smaller version of itself and
becomes a broken-looking one. `layout.rs` degrades gracefully and without limit:
at 80×5 the Table DDL and Queries panels vanish, at 20 columns the sidebar goes,
and at 1×1 the frame is a single corner character. Every one of those is the
collapse rule working exactly as designed — and none of them tells the user that
the terminal is the problem.

This puts a floor under it: a size below which the shell is not drawn at all,
and a notice that says what is needed instead.

Prerequisite for the hint bar
([design](2026-08-09-hint-bar-design.md)), which needs a row and can only be
guaranteed one above a floor. It is specified separately because it changes
behaviour on its own and is worth having whether or not the bar ever ships.

---

## 1. Where the floor comes from

Not a typed-in 80×24. The floors are already in `layout.rs`, and the minimum is
the size at which all four panels clear them simultaneously:

```rust
// lc-inbound/src/layout.rs
const MIN_W: u16 = SIDEBAR_MIN_W + MAIN_RESERVE_W;   // 16 + 24 = 40
const MIN_H: u16 = 2 * PANEL_MIN_H;                  //  3 +  3 =  6
```

Width: the sidebar cannot go below `SIDEBAR_MIN_W` and the main column reserves
`MAIN_RESERVE_W` before the sidebar is allowed to exist at all. Height: each
column holds two panels and `PANEL_MIN_H` is two borders and one line of
content, so six rows is the point at which every panel still shows a line.

Deriving rather than declaring means a change to any floor moves the gate with
it, and there is no second place to remember. It also keeps the gate honest:
40×6 is exactly the smallest frame the existing layout draws correctly, not a
guess at the smallest one worth supporting.

A readability floor — 80×24, or 80×12 — was considered and rejected. It would
refuse to draw frames that the layout in fact handles, and "this is unpleasant
but works" is the user's call to make, not ours. The gate exists to catch the
sizes where the output is wrong, not the ones where it is cramped.

## 2. Below the floor

The shell is not drawn: no panels, no borders, no overlay. In its place, centred
in the frame:

```
Too small
need 40×6
have 24×5
```

Three short lines rather than one sentence, because a sentence explaining that
the terminal is too narrow does not fit in a terminal that is too narrow. Nine
columns each; the numbers are formatted from `MIN_W`/`MIN_H` and the frame, so
the notice cannot drift from the gate it is explaining.

Lines drop as the frame shrinks further, least actionable first, so what you
must do survives longest:

| Rows | Shown |
|---|---|
| ≥ 3 | all three |
| 2 | `need 40×6`, `have 24×5` |
| 1 | `need 40×6` |
| 0 | nothing |

Below nine columns the lines are clipped by the terminal rather than shortened
further. At that width there is no message worth composing, and the function
stays total without a special case.

`q` still quits. `handle_key` has never consulted geometry and does not start
now — the one thing a user in a too-small terminal certainly wants to do keeps
working, which is also why the notice does not need to say so.

## 3. Making it unrepresentable

The gate is not a check the renderer is trusted to perform first. `layout`
returns a type that has no panels to draw below it:

```rust
pub enum Shell {
    Panels(PanelRects),
    /// The frame that fell short. What it fell short of is `MIN_W`/`MIN_H`.
    TooSmall(Rect),
}

pub fn layout(area: Rect) -> Shell;
```

`render` matches, and there is no path on which it draws a panel into a frame
too small to hold one (conventions.md §1 — push invariants into types). The
alternative, a `fits(area) -> bool` beside the existing `layout`, is a
convention that the next person to touch `render` can forget.

`PanelRects` and `split` are otherwise untouched.

## 4. Where the decision lives

`lc-inbound`. It is a decision about the frame's size, and size-dependent layout
is already the renderer's business for the same reason column widths and
wrapping are (panel-scrolling design §2).

The domain could not make it in any case. It learns geometry only from the
`Msg::Measured` of the frame just drawn, so it is always one frame behind;
gating the layout on a stale size would flicker for a frame on every resize, in
both directions.

## 5. What it does to the collapse rule

`split`'s collapse-to-zero branch — the one that drops a panel rather than
shrink it below its floor — becomes unreachable for any size that reaches
`Shell::Panels`. The gate is precisely the point at which it would first fire.

It stays. `layout` is public and total, its tests cover every size including
`0×0` and `u16::MAX`, and the branch is what makes the function total over the
`Rect` its parameter admits. Removing it would trade a proven-total function for
one that is correct only because of a caller's guard.

It does have to change how it is tested. The two tests that reach the branch
today — `the_minor_panel_collapses_rather_than_shrink_below_its_floor` at 80×5
and `the_sidebar_disappears_before_the_main_column_does` at 20×24 — both name
sizes that now return `TooSmall`, so neither can reach the branch through
`layout` any more. They become direct tests of `split`, which is private and
in the same module. A branch nothing can reach and nothing tests is dead code
wearing a totality argument; tested at the unit that owns it, the argument
holds.

The same is true of `PANEL_MIN_H` on the height axis: it now describes the
minimum rather than a floor anything falls to.

## 6. Geometry

`Geometry` is unchanged. Panel heights are read off the rects the panels
actually got, so `Shell::TooSmall` reports `heights` of all zeros and
`ddl_lines` of zero — which is a state the domain already handles, and already
reaches today whenever a panel collapses.

`Cursor::page_down(len, 0)` is a no-op and `Cursor::up`/`down` still move, so a
selection stays usable and bounded. A `Scroll` offset does not survive the gate
at all: below the floor `render` returns `Geometry::default()`, so
`Msg::Measured` calls `Scroll::clamp_to(0, 0)` (`update.rs:154`), and
`go_to`'s `last = len.saturating_sub(height)` is `0` regardless of what the
offset was — the Ddl scroll is zeroed, not merely clamped. Scroll the Table
DDL panel to line 200, shrink the terminal below the floor, restore it: the
DDL is back at the top, not where it was left.

That is NOT a regression. At every size the gate now catches, the Ddl panel
had already collapsed to zero height and its offset was already being zeroed
on every `Msg::Measured`; the gate only changes how often that path runs, not
whether it can. Cursors are unaffected either way — they are bounded by row
counts, not panel geometry.

## 7. Testing

**Layout.**

- `layout` returns `TooSmall` below the floor on each axis independently and at
  the corner: 39×6, 40×5, 39×5, 0×0.
- `layout` returns `Panels` at exactly 40×6, and every panel is non-empty there
  — the claim §1 derives the constants from.
- The existing size list gains 40×6, and the sizes below the floor move to the
  `TooSmall` assertions. `the_panels_tile_the_area_exactly` keeps its meaning
  for every size that yields `Panels`.
- `split` collapses and lifts its minor part, tested directly at the sizes
  `layout` no longer passes it (§5).

**Render.**

- Below the floor: no block, no border, and no overlay is drawn, even when the
  view model carries one.
- The notice's line set at 3, 2, 1, and 0 rows.
- The notice names the frame it was given and the minimum from the constants,
  so a changed floor changes the message.

## 8. Knowingly left out

- **A resize hysteresis.** Crossing the floor redraws immediately in both
  directions. A terminal parked exactly on the boundary during a drag will
  flicker; that is a cost of one row of hysteresis to fix and nobody has felt
  it yet.
- **Per-panel gates** — hiding the sidebar to keep the main column usable below
  the floor. That is a responsive layout, a much larger design, and vision.md §4
  principle 4 says the layout does not rearrange itself under the user.
- **A configurable minimum.** The floor is derived; a user-set one would have to
  be validated against the same derivation to mean anything.
- **The hint bar's row.** The bar raises `MIN_H` by its own height when it lands
  ([design](2026-08-09-hint-bar-design.md) §5). This slice does not anticipate
  it beyond deriving `MIN_H` rather than declaring it.
