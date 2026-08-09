# List viewport — design

**Date:** 2026-08-09
**Status:** implemented

The wheel scrolls the rows and leaves the selection where you put it.

[Mouse input](2026-08-09-mouse-input-design.md) §6 predicted this. It shipped
the wheel moving the *selection* in Tables and Contents, because neither panel
stored a viewport for it to move — `body.rs` builds a `ListState` per frame from
the selected index and throws it away, so the only thing that can move the view
is moving the selection. The design said so, TODO.md recorded it, and the first
person to roll a wheel over a long table said the wheel was broken.

They were right. This gives those panels a viewport.

---

## 1. Two motions, and only one touches the cursor

| Motion | Moves | Then |
|---|---|---|
| Wheel notch | The offset | Nothing |
| `j` `k` `PageUp` `PageDown` `Home` `End` | The cursor | The offset is pulled to cover it |
| A click on a row | The cursor | The offset is pulled to cover it — a no-op, the row was on screen |
| A tree transition, a schema reload | The cursor | The offset is pulled to cover it |

The selection is where you are. The wheel is a look somewhere else, and a
keypress returns you.

**A selection scrolled out of view stays where it is.** It is not dragged to the
edge, and the next `j` moves it from where it actually sits rather than from
whatever is on screen — so the view jumps back to it. The alternative, moving
the selection to the nearest visible row, was rejected: it relocates the user's
place in response to a keypress that asked to move by one, which is the same
surprise the wheel was just cured of.

## 2. Where the offset lives

`Scroll` already exists in `state.rs` — the Ddl panel's viewport, with a private
offset that no motion can push past the end. `TablesPanel` and `ContentsPanel`
each gain one beside their `Cursor`. Nothing new is invented; a type that was
written for one panel acquires two more owners.

The offset is domain state rather than renderer state for the reason the Ddl
offset is: a viewport that survives between frames is state, and the domain is
where state lives. What the renderer keeps is the arithmetic that needs a width.

## 3. Where the pull happens

Not at each motion. `update` already snapshots something before `apply` and acts
on the difference afterwards — `described(state)`, which resets the Ddl offset
when the described node changes. The same shape carries this:

```rust
let subject = described(state);
let cursors = cursors(state);
let effects = apply(state, msg);

if described(state) != subject {
    state.ddl.to_start();
}
clamp_lists(state);
follow_moved_cursors(state, cursors);
```

**Detecting the change beats threading a height.** The alternative is to pass a
panel height into every cursor motion, including `TablesPanel::open`, `close`
and `toggle`, which today know nothing about how tall the panel is and have no
business learning. Comparing the index before and after is one place, catches
every path — the keyboard, a click, a tree transition, a schema reload clamping
a stale selection — and cannot be forgotten by a path added later.

It is also what makes the feature work rather than fight itself. A wheel notch
leaves both indices untouched, so nothing pulls the view back.

### The clamp is unconditional; the follow is not

`clamp_lists` runs on every message. Fold a keyspace and the tree shrinks, but
the *panel* does not, so no `Msg::Measured` arrives — geometry is a function of
the terminal size and the view model, and neither changed. A stale offset would
otherwise sit past the end of a shortened list until the next resize.
`Scroll::clamp_to` only ever reduces an offset, so running it always is safe and
cannot fight a wheel that is bounded by the same arithmetic.

`follow_moved_cursors` runs only for a panel whose index actually moved. That
distinction is the whole feature.

## 4. The renderer must stop telling ratatui about an off-screen selection

`ListState` and `TableState` accept an offset, and ratatui honours it — but only
when nothing is selected. `ratatui-widgets-0.3.2/src/list/rendering.rs`'s
`get_items_bounds` says so outright: it honours the offset "if nothing is
selected", and otherwise walks the offset until the selected index is inside the
window. Hand it a `Some` it cannot see and it drags the view back, undoing this
design at the last moment before the buffer is written.

So `body.rs` passes `Some(selected)` only when the selection falls inside the
drawn window, and `None` otherwise:

```rust
let mut state = ListState::default()
    .with_offset(offset)
    .with_selected(visible(selected, offset, height));
```

The visible consequence is correct on its own terms — a row that is not drawn is
not highlighted — and it is worth a test of its own, because without it ratatui
silently reinstates the old behaviour and every other test still passes.

`PanelBody::Tree` and `PanelBody::Grid` therefore carry an `offset` beside their
`selected`, exactly as `PanelBody::Details` already carries one.

### What this does to the hit map

The read-back stays. `tree` and `grid` in `body.rs` still call `state.offset()`
after `render_stateful_widget` runs, rather than reporting the offset they
handed in, because the widget is free to move it — the same walk described
above. That read-back is what keeps
[mouse input](2026-08-09-mouse-input-design.md) §4's rule true: the map is
assembled from what did the drawing, not from what was asked for, so a click
while scrolled resolves against the offset the widget actually drew from
rather than the one it was asked to use.

## 5. Three edges

**A new table opens at the top.** `ContentsPanel::requested` and `refused` reset
the cursor today and must reset the offset with it, or reading a second table
opens it halfway down the first one's scroll position.

**The grid's height is already net of its header.** `Geometry` reports Contents'
height with the header row already deducted, so one number bounds both the
follow and the clamp with no special case.

**A wheel notch is three `Scroll::down`s**, bounded by the same `(len, height)`
as `End` — so it stops with the last row at the bottom rather than dragging it
to the top.

## 6. What this costs

**Render-test fixtures break, and they should.** `browsing(names, selected)` and
`the_grid_scrolls_to_keep_the_selection_on_screen` move a cursor by calling
`state.contents.cursor.down(20)` directly, bypassing `update` — which is exactly
where the follow now lives. Those fixtures will draw at offset zero and fail.

This is not incidental damage. Those tests could reach past `update` only
because scroll position was a derived quantity that no state had to be right
about. It is state now, and a fixture that fabricates a selection without the
scroll that belongs to it is describing a state the application cannot be in.
They move to driving `update`, or set the offset explicitly.

## 7. What gets better for free

Wheeling over Tables currently changes the selected node, which trips
`described()` and resets the Table DDL panel's scroll — so a wheel in one panel
scrolls a second one back to its top. Once the wheel stops moving the selection,
that stops happening, and it needs no code of its own to fix.

## 8. Testing

**Domain.** A wheel scrolls the view and leaves the selection where it was; a
motion after a wheel brings the selection back into view; a wheel stops at both
ends; walking off the bottom still drags the view with it; reading a table opens
its grid at the top; a list that shrank pulls its offset back; a wheel over
Tables no longer resets the Ddl panel.

**Render.** A stored offset decides which rows are drawn; a selection scrolled
out of view is not highlighted. The second pins §4's rule, and is the one test
whose absence lets ratatui quietly restore the behaviour this design removes.

## 9. Documentation this changes

- **doc/shortcuts.md** §7 — the wheel row and the paragraph beneath it.
- **[mouse input](2026-08-09-mouse-input-design.md)** §6 — its fourth decision
  is what this retires. Amended in place with a pointer here, rather than left
  to contradict a later document.
- **TODO.md** — loses the item that predicted this.

## 10. Not in this slice

- **Horizontal scrolling.** The grid still caps at 24 columns and draws nothing
  past the right edge (vision.md §8).
- **A scrolloff margin.** Vim keeps a few lines below the cursor; this pulls the
  offset the minimum distance. Worth revisiting once the viewport is real, and
  cheap to add to `Scroll` when it is.
- **Dragging the scrollbar.** It is drawn over the block's right border and
  spends no content width; making it a target is a mouse slice of its own, and
  it would be the first gesture that does not decompose into an existing
  binding (mouse input §1).
