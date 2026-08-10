# Table DDL — a cursor over the described rows

**Status:** implemented.
**Date:** 2026-08-10

Companion to [table metadata design](2026-08-09-table-metadata-design.md),
which built the panel, and [panel scrolling design](2026-08-09-panel-scrolling-design.md),
which gave it a viewport.

---

## 1. The problem

The Table DDL panel is already focusable — `a` focuses it, `Tab` reaches it,
and `j`/`k`/`PageUp`/`PageDown`/`Home`/`End` scroll it. It does not *look*
focusable, because it draws no cursor: `update.rs` calls it "a scrolled view
with nothing a press can act on" and returns `None` from `panel_cursor`. The
user focuses the panel and nothing visibly happens.

Two changes follow from that, and one is unrelated to it but shares the file.

1. The panel gains a cursor over the rows it describes, so focus is legible
   and a reader can point at a property rather than at a screen.
2. A property whose value wrapped is one cursor stop, highlighted across every
   line it occupies.
3. Independently: the option ordering sinks the bulky properties to the bottom,
   so the compact part of the panel scans as a block.

The cursor is a cursor and nothing more. No action is bound to it —
`Enter` in Table DDL stays a no-op. Copying a value, or jumping from a key
column to that column in Contents, are later slices; this one exists so there
is something for them to act on.

## 2. Option ordering

`BY_IMPORTANCE` today leads with `comment` and places `compaction` and
`compression` fourth and fifth, on the stated rationale that it "leads with
what silently costs data or latency". Those three are also the three whose
values are long enough to wrap, so the compact rows a reader scans are split
by two or three multi-line blocks.

The list is re-cut and its rationale rewritten to say what it now is: **a
layout rule, not an importance rule.** Compact rows first, bulky blocks last.

`in_display_order` grows from two chunks to three:

1. ranked, in `BY_IMPORTANCE` order — `default_time_to_live`,
   `gc_grace_seconds`, `caching`, and the rest, unchanged among themselves;
2. unranked, alphabetically — whatever the running cluster reports that this
   list predates or postdates;
3. bulky, in a fixed order — `comment`, `compaction`, `compression`.

The bulky chunk goes *after* the unranked tail rather than between the two, so
a wide option from a future Cassandra major cannot land between the compact
table and the blocks below it. That is the whole point of the re-cut, and it
would be undone by an ordering that only holds for the clusters we know about.

This is a real trade, and it should be recorded as one: `comment` is arguably
the most useful single thing the panel shows about a table, and it moves from
first to last. It is bought back by the panel above it being scannable, and by
`comment` being where a reader now reliably knows to look. The doc comment on
`BY_IMPORTANCE` must stop claiming importance order — a comment that describes
the list it used to be is worse than none.

**The wrap rule itself does not change.** `property_lines` still decides one
line versus name-over-indented-value by measurement: a value that fits at the
panel's width gets a line, one that does not wraps. Which properties are
"bulky" for *ordering* is a fixed list; whether any given one *wraps* remains a
function of its value and the panel's width, as it is today.

## 3. What the cursor moves over

Exactly the `DetailRow`s, in the order the projection emits them: every key
column of the `Partition Key` and `Clustering Key` sections, every property of
`Options`, or every property of `Replication` for a keyspace.

Not selectable: the panel heading (`shop.orders_by_customer`), the section
headings, and the blank lines between groups. A heading is a label for the rows
beneath it and has nothing a cursor could mean; making it a stop would cost
three dead presses on a table with one partition column.

The consequence the user asked for falls out: from the last partition column,
`j` lands on the first clustering column. Section boundaries are invisible to
motion, which is right — they group the rows, they do not separate them.

**A wrapped property is one stop, not one per line.** `compaction` occupying
five lines is one item, and all five lines take `Token::Selected`. This is what
makes the non-tabular content highlight as a block. It needs no new machinery:
the body is a single `Paragraph`, so styling every `Line` of the item and
letting ratatui clip is the whole implementation.

## 4. Motion

Table DDL stops being the special case in the motion table. `j`/`k`,
`PageDown`/`PageUp` and `Home`/`End` all move the *cursor*; the viewport
follows. The panel now behaves as Tables and Contents do, and the
`if panel == Panel::Ddl { … return }` branch at the head of `scroll` goes away
along with the `Panel::Ddl => 0` and `Panel::Ddl => None` arms of `row_count`
and `panel_cursor`.

One imprecision is accepted rather than papered over: `PageDown` moves the
cursor by the panel's height counted in **items**, while the panel's height is
measured in **lines**. On a section of one-line properties the two agree; on a
run of wrapped blocks a page overshoots. Correcting it needs a line-to-item
map from the renderer, which is not worth a variable-length report across the
boundary (§5) for a panel that usually fits on one screen. `Home` and `End`
remain exact, and `j`/`k` are unaffected.

### 4.1 Scrolling without moving the cursor

Because a cursor and a viewport can now disagree — a single option can be
taller than the panel — the panel keeps a way to move the viewport alone:

| Key | Action |
|---|---|
| `Ctrl-e` | Scroll down one line. |
| `Ctrl-y` | Scroll up one line. |

Bound in Table DDL only. The other panels scroll in rows, where a cursor and a
viewport cannot diverge far enough to need it; binding it everywhere would add
two keys that do nothing observable in three places out of four.

The cursor does not move and may scroll out of view, in which case it is not
drawn — the existing rule the renderer already enforces for Tables and
Contents (`a_selection_scrolled_out_of_view_is_not_highlighted`, and its tree
counterpart). The next `j` or `k` snaps
the viewport back, because motion follows the cursor.

The mouse wheel is unchanged: it scrolls the panel under the pointer without
focusing it, and now also without moving its cursor. A wheel is a viewport
gesture in every panel already.

**`Binding.keys` is `&[Key]`, which cannot express a modifier**, so the hint
bar cannot render `^e` until it becomes `&[KeyPress]`. That widening is part of
this slice, not a follow-up: a binding the hint bar cannot name is a binding
the user has to read this file to discover, which
[hint bar design](2026-08-09-hint-bar-design.md) exists to prevent.

The panel's hints become `j k move`, `^e ^y scroll`, `PgDn PgUp page`,
`Home End ends`.

## 5. Where the cursor lives, and how the viewport follows it

### 5.1 State

`State.ddl` stops being a bare `Scroll` and becomes a `DdlPanel` holding a
`Cursor` and a `Scroll`, the shape `TablesPanel` and `ContentsPanel` already
have. This is the third panel to take architecture.md §11's nested-per-panel
escape hatch, and it takes it the same way as the other two rather than
inventing a third arrangement.

`panel_cursor` gains its `Panel::Ddl` arm, so the motion keys move it like any
other cursor.

**`LISTS` does not grow.** That constant names the panels whose offset is
followed *by index*, and every one of the three loops over it — `clamp_lists`,
`follow_moved_cursors`, and the resize pass in `measured` — assumes a row is a
line. Ddl breaks that assumption in both directions: its offset counts lines
while its cursor counts rows, and `scroll_len` already returns a different
quantity for it than `row_count` does. Adding it to `LISTS` would mean
branching inside all three loops, which is a worse statement of the same thing.
Ddl gets its own arm in the epilogue instead, following the span of §5.4.

Its cursor is clamped in the epilogue, unconditionally and beside
`clamp_lists`, for that function's own stated reason: a schema reload that
shrinks a table changes no geometry, so no measurement arrives to clamp
against.

### 5.2 The projection

`PanelBody::Details` gains `selected: usize`, alongside the `details` and
`offset` it already carries. This is the same field `PanelBody::Tree` and
`PanelBody::Grid` carry, meaning the same thing — an index into the rows the
body draws, in the order it draws them. No new shape crosses the boundary.

The renderer resolves it by counting `DetailRow`s as it walks sections, which
is the counting `Tree` already does over its row list. Row order is stated
once, by the order the projection emits sections and rows.

### 5.3 Bounding the cursor

The item count is a property of `Details`, which the domain builds — so
`update` counts rows off the projection and clamps the cursor itself, with no
frame in between. A table with fewer options than the last one pulls the cursor
back immediately rather than leaving it out of range until something is drawn.

`described()` already resets the panel when the described node changes; the
cursor resets to the first row along with the offset, for the reason already
recorded there — a new subject opens at its top.

### 5.4 Following, across wrapping

This is the only hard part. The domain owns the offset, but **only the renderer
knows how many lines an item wrapped to**, so the domain cannot compute the
offset that puts item 7 on screen.

`Geometry` gains one field: the line span of the **selected** item as drawn,
`(first_line, lines)`, recorded by the renderer as it draws that item and
reported with the rest of the measurement. `Scroll` gains `follow_span`
alongside `follow` — pull the viewport the shortest distance that puts the span
inside it, and when the span is taller than the panel, put its first line at
the top rather than its last at the bottom.

**The follow happens in `measured`, not in `update`'s epilogue**, and this is
forced rather than chosen. When `j` moves the cursor from row 5 to row 6, the
geometry in hand still describes row *5* — it was measured from the previous
frame. There is no correct offset for the epilogue to compute. So the epilogue
moves the cursor and stops; the frame draws; the measurement comes back
describing row 6; `measured` follows it and marks dirty. One frame late, by
construction.

The wheel is safe from this without a guard, and the reason is worth stating
because it is not obvious. `measured` follows unconditionally, so a wheel
scrolling the cursor off screen looks like it should be undone at the next
measurement — but a wheel changes no height, no wrapped-line count, and no
span, so the geometry is identical and `lc-app` sends no measurement at all.
The same argument covers `Ctrl-e`/`Ctrl-y`. Only something that changes the
frame's shape can reach `measured`, and everything that changes the frame's
shape *should* re-follow.

**Why this is safe under the §3 loop guard.** architecture.md §3 requires that
`Geometry` be a function of the terminal size and the view model alone, never
of the offsets it bounds — a measurement that varies with an offset closes the
loop. This span depends on the width, the content, and the cursor. The cursor
is part of the view model (§5.2), not an offset, so the invariant holds:

```
cursor moves → frame drawn → span measured → offset follows → repaint
             → same span measured → geometry unchanged → no further frame
```

`Geometry` stays `Copy`; nothing variable-length crosses the boundary. The
alternative — reporting a full item-to-line map — would make `Geometry` `Clone`
and grow with the table, and buys only the `PageDown` precision §4 already
declined.

**The cost, stated.** The frame in which the cursor moves is drawn with the
previous offset, so a cursor jumping to a far item is off screen for exactly
one frame before the follow lands. This is already how the existing DDL clamp
behaves, accepted in
[panel scrolling design](2026-08-09-panel-scrolling-design.md) §3; it is
accepted again here rather than moving text wrapping into the domain, which
would put a `unicode`-aware layout engine behind the crate boundary that exists
to keep rendering out.

## 6. Testing

**Domain tier.**

- `in_display_order` puts the ranked chunk first, the unranked tail
  alphabetically second, and `comment`/`compaction`/`compression` last —
  including with an option the ranked list has never heard of, which must land
  *above* the bulky chunk.
- `j` from the last partition column selects the first clustering column.
- The cursor never lands on a heading: for a fixture with all four section
  kinds, walking from `Home` to `End` visits exactly the `DetailRow`s.
- The cursor clamps when the selected table changes to one with fewer rows.
- The cursor and offset both reset when the described node changes, and neither
  resets when the tree merely unfolds — the existing `Described` identity.
- `Ctrl-e` moves the offset and not the cursor; `j` afterwards brings the
  cursor back into view.
- `Scroll::follow` over a span taller than the panel puts the span's first line
  at the top.

**Render tier.**

- A wrapped property drawn with the cursor on it has *every* one of its lines
  reversed, and the property below it none.
- A one-line property has exactly its own line reversed.
- A section heading is never drawn as the selection.
- A selected block scrolled fully out of view is not highlighted — the DDL half
  of `a_selection_scrolled_out_of_view_is_not_highlighted`.
- A selected block scrolled *partly* out of view keeps its visible lines
  highlighted.

## 7. Not in this slice

- Any action on the selected row. `Enter` in Table DDL stays a no-op.
- Copy to clipboard. There is no clipboard anywhere in the program yet
  (TODO.md), and adding one for this panel alone would decide its shape for
  the editor too.
- Horizontal motion or expansion of a truncated value. Nothing is truncated —
  the panel wraps instead ([table metadata design](2026-08-09-table-metadata-design.md) §6).
- `Ctrl-e`/`Ctrl-y` in any other panel.
