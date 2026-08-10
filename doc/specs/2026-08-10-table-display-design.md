# Table display — design

**Date:** 2026-08-10
**Status:** designed

Reading a table lands you in it, the cursor names a cell rather than a row, the
rows are numbered, and a grid wider than its panel says so.

Four complaints about the Contents panel, and they are one complaint. The panel
shows a page of rows and offers nothing to do with them: focus stays behind in
Tables, the selection is a whole row so nothing can be said about a value, the
columns that do not fit are silently dropped, and nothing on screen admits it.
This slice makes the grid navigable in both axes and honest about its edges.

The grid it acts on is [table contents](2026-08-09-table-contents-design.md)'s,
which shipped a page of rows and left navigation inside it to a later slice.

---

## 1. Focus follows a read

`Enter`, `Space`, and a left click on a table row do what they already do —
select the row, cancel the outstanding read, issue the new one — and then move
focus to Contents.

**Immediately, not when the page lands.** Focus is a consequence of the
keypress, not of the network. Contents is focused while its body still says
"Loading rows…", the border says so before the data arrives, and no frame ever
moves focus under a user who has since gone elsewhere. A focus change that
waits on a query is a focus change that happens at an unpredictable time, which
is the same defect as an unpredictable binding.

All three gestures do it, which keeps doc/shortcuts.md §7's rule — every
gesture reaches the same state a key does — true, and leaves one thing to
learn rather than three. Returning is `Tab` or `t`; the tree selection is
untouched, so browsing the next table is two keys.

The `Enter`/`Space` synonym survives intact. Both were considered for a split —
`Space` reading without leaving, to make browsing cheap — and rejected: two
tests pin the synonym today, and a key that is *almost* another key's synonym
is worse than either rule held straight.

## 2. Two cursors name one cell

`ContentsPanel` gains a column cursor beside its row cursor. Together they name
a cell; neither is meaningful alone.

| Key | Moves |
|---|---|
| `j` `k` `↓` `↑` | The row cursor, as today |
| `h` `l` `←` `→` | The column cursor, stopping at either edge |
| `Home` `End` | The **row** cursor, to the first/last row |
| `PageUp` `PageDown` | The **row** cursor, by a screen of rows |

Both cursors reset to the first cell on every new read — `requested` and
`refused` already do this for the row cursor, and the column cursor joins them.
Both are inert while the panel is `Empty`, `Loading` or `Failed`, by the same
route motion is already inert there: the panel reports a count of zero and the
cursor cannot leave zero.

### The rule `h`/`l` breaks, and the rule it does not

doc/shortcuts.md §2 says the same key means the same thing in every panel, and
`h`/`l` mean fold/unfold in Tables. Binding them to column motion in Contents
would break that rule as it is currently worded. The wording is what is wrong.

Rewritten, the four keys mean **move left / move right**, and each panel
resolves that in its own vocabulary:

| Panel | `h` / `←` | `l` / `→` |
|---|---|---|
| Tables | Fold — step out | Unfold — step in |
| Contents | Previous column | Next column |
| Table DDL | Inert | Inert |
| Queries | Cursor left (text) | Cursor right (text) |

This is the same shape §2 already accepts for `j`/`k`: "down one row" resolves
to a tree row, a grid row, or a wrapped line depending on the panel, and is
inert where the panel has nothing to move. Horizontal motion now reads the same
way. The vim/arrows synonym rule survives untouched in both directions, which
the two rejected alternatives — arrows-only in Contents, or a fresh pair from
the free set — each cost.

The key budget in §3 is unchanged: no key is claimed that was not claimed
already.

### Horizontal motion is one column at a time

No `Home`/`End` over columns, no horizontal paging. `Home` and `End` keep
meaning the ends of the *document*, which in a grid is the row list — the
meaning they have in Tables and Table DDL. Giving them a second axis in one
panel would be a third exception to a rule that already carries two.

## 3. What the cell looks like

The selected row keeps the tint it has today; the selected cell is drawn over
it with a stronger highlight. Two tokens, so the theme can tell them apart and
a future delete/edit has an unambiguous target on screen.

Both are drawn, not one. A cell highlight alone loses the row on a wide grid;
a row tint alone is what the panel does today and says nothing about which
value is selected. The row answers *where am I*, the cell answers *what will
this act on*.

The cell highlight is drawn only when the panel has focus — same as every other
selection in the program.

## 4. The row-number gutter

A pinned leftmost column: right-aligned, muted, `1`-based, numbering the
**fetched page**. Row 40 is drawn `40` whatever the viewport and whatever the
column offset. Its width is the digit count of the last row in the page, so a
9-row page spends one column and a 1000-row page spends four.

The gutter is chrome, and the consequences are all consequences of that:

- It is not a grid column. The column cursor cannot reach it, and
  `column_count` does not count it.
- It never scrolls off. The horizontal viewport starts to its right.
- A click on it is a click on that row, exactly as a click anywhere else in the
  row is today. It is *not* chrome for the purpose of doc/shortcuts.md §7's hit
  map — a header is chrome because pressing it means nothing; a row number is
  part of its row.

It numbers the page, not the table. Cassandra pages have no absolute row index
to show, and inventing one that resets on the next page would be a lie. What
the number is for is saying "the third one" out loud and finding your place
after a scroll — both of which the page-local index serves.

## 5. The horizontal viewport

A column offset, mirroring the row offset the panel already has. The view draws
whole columns from the offset until the available width runs out.

**Whole columns, not characters.** A character-granular offset would let a wide
column be half-drawn at either edge, which reads better on a 60-character blob
— and cannot be done with ratatui's `Table` at all. The grid would need
hand-drawn rows and its own clipping, which is a larger slice than all four of
these changes together, and it buys nothing the column cap in §7 does not
already bound.

The offset follows the cell cursor the shortest distance that puts it on
screen. This is `Scroll::follow`'s existing contract, over columns instead of
rows, and the reason `Scroll` is reused rather than a second type written.

**The wheel stays vertical.** doc/shortcuts.md §7 already drops horizontal
wheel events at the edge, and this slice does not pick them up: a notch moves
the row viewport, and columns are reached with the keys §2 names. The rule that
nothing the user acts on is mouse-only is unaffected either way.

## 6. The measurement edge

This is the part that needs care, and it decides where the code goes.

architecture.md §3 states the invariant that keeps the geometry loop from
closing: **`Geometry` is a function of the terminal size and the view model
alone, never of the offsets it bounds.** "How many columns fit starting at
offset N" is offset-dependent. Measuring it in the renderer and sending it back
as `Msg::Measured` would close the loop the invariant exists to prevent.

So the split is:

**The domain computes column widths.** Each column's width is the widest of its
cells and its heading, capped (§7). That is a pure function of the page —
`lc-domain` already owns the cell text via `cell::text` — and it makes the
fitting arithmetic available where the offsets live.

This costs `lc-domain` a dependency on `unicode-width`. The crate rule in
architecture.md §2 is that the domain's list names no terminal, no runtime and
no driver; `unicode-width` is a table of character widths and is none of the
three, so the rule holds by its letter and by its purpose. It is still a change
to a deliberately short list and the commit says so.

The alternative — the renderer measuring the widths, which are themselves
offset-independent, and sending them back — was rejected on two counts: it
costs `Geometry` its `Copy` (a `Vec<u16>` where four `usize`s live today), and
it puts a display concern on the one edge that runs backwards, which is the
edge with the least room for one.

**`Geometry` gains one scalar**, the character width available to the grid
body. A function of the terminal size alone, so the invariant survives and
`Geometry` stays `Copy`.

**`update` does the fitting.** Given the widths, the available width and the
offset, it decides which columns are drawn, applies `follow`, and clamps. All
of it inside `update`, none of it crossing back.

The result is a renderer that decides less than it does today: `body.rs` draws
the columns the view model names, at the widths it gives.

## 7. The column cap stays

`MAX_COLUMN_WIDTH` is 24 and does not move. Horizontal scrolling reaches
*columns*, not the tails of values: a 36-character uuid is still ellipsised,
and seeing a value in full is a later slice — an expanded-cell view, which
wants a design of its own and is not needed for a cell cursor to have a target.

A column narrower than its cap is still drawn at its natural width, as today.

## 8. Where the code goes

| File | Change |
|---|---|
| `lc-domain/src/contents.rs` | `column: Cursor` and `columns: Scroll` beside the existing pair; `column_count()` beside `row_count()`; both reset in `requested`/`refused` |
| `lc-domain/src/state.rs` | `Geometry` gains the grid's available character width; `CONTENTS_HINTS` names the column motion, so the bar teaches it |
| `lc-domain/src/update.rs` | `h`/`l`/`←`/`→` dispatch on focus rather than reaching only the tree; `Enter`/`Space`/click on a table set focus; the column viewport follows and clamps |
| `lc-domain/src/view/contents.rs` | `GridColumn` gains `width`; `PanelBody::Grid` gains the first drawn column, the selected column, and the gutter's width |
| `lc-inbound/src/body.rs` | `widths()` and `fitting()` deleted; the gutter and cell highlight drawn; a second `Extent` returned for the horizontal bar |
| `lc-inbound/src/render.rs` | `scrollbar` grows a bottom-border orientation |
| `lc-inbound/src/theme.rs` | The cell-highlight token |
| `lc-inbound/src/hit.rs` | Row resolution shifted by the gutter |
| `doc/shortcuts.md` | §1 the new motions, §2 the rewritten left/right rule, §4 horizontal motion leaves the unbound table, §5 a row per new test |

`hit.rs` changes shape not at all: a click still resolves to a panel and a row.
The gutter and the column offset move where a row's cells start, not what a
click means.

## 9. The scrollbar

Drawn over the panel's **bottom border**, mirroring the vertical bar on the
right border, and only when a column is off screen — the vertical bar's rule,
that its absence is what says nothing is hidden
([list viewport](2026-08-09-list-viewport-design.md) §1), applies unchanged.

It measures in columns. The thumb says "columns 2–4 of 7", not a proportion of
characters, because columns are what the keys move.

A panel showing every column and every row draws neither bar, which is the
common case on a small table and the one worth keeping clean.

## 10. Testing

Domain tier — deterministic, no clock, no I/O:

- The column cursor stops at the first and last column.
- A new read resets both cursors and both offsets.
- Motion in an unfocused Contents is a no-op, and a panel with no page swallows
  it rather than passing it on.
- `h`/`l` still fold and unfold in Tables, from both a keyspace row and a table
  row.
- The column viewport follows the cell cursor the shortest distance that puts
  it on screen, in both directions.
- A page with fewer columns than the last one clamps the column cursor.
- `Enter`, `Space` and a click on a table each leave focus in Contents; each
  leaves the tree selection where it was.
- A read that fails still leaves focus in Contents.

View tier: the grid names its selected cell and its first drawn column; the
gutter numbers the page absolutely; the widths are the capped natural ones.

Render tier: the gutter is drawn muted and pinned and survives a scrolled
viewport; the cell highlight is drawn over the row tint and only under focus;
the horizontal bar appears exactly when a column is hidden; a click still
resolves to the row that was drawn.

doc/shortcuts.md §5 gains a row per binding, which that document requires of
every binding and is not optional here.

## 11. Deliberately out of scope

| Not built | Why |
|---|---|
| Column resizing | Wants a persisted preference; there is no config system to hold one |
| Pinned key columns | Useful on wide tables, but needs a rule for when the pinned block itself does not fit — its own slice |
| Full-value inspection | An expanded-cell view, wanted independently of this one |
| `Home`/`End` and paging over columns | A third exception to a two-exception rule; §2 |
| Horizontal wheel | Dropped at the edge today and stays dropped; §5 |
| Delete/edit against the selected cell | What the cell cursor is *for*, and the reason it ships first |
