# Visual polish — design

Five independent presentation changes: rounded panel corners, a loader in the
hint bar, a scrollbar that stops doubling the border, and a Table DDL panel
that separates a table's shape from its settings and lays the short properties
out as a table.

**Status:** planned.
**Date:** 2026-08-09

---

## 1. Scope

Nothing here changes what the tool knows or can do. Every change is in how a
frame is drawn, with two exceptions that reach further and are called out
because they do:

- the loader needs `Msg::Tick` to actually arrive, which means a ticker in
  `lc-app` and a frame counter in the domain (§4);
- the loader and the property-name highlight both spend colour that
  [vision.md](../vision.md) §4 principle 5 forbids today, so that principle is
  amended (§7).

Out of scope: the regular and static column list, a `?` overlay, per-panel
themes, and any change to what the Table DDL panel fetches.

## 2. Rounded corners

`Block::bordered().border_type(BorderType::Rounded)` wherever a block is
drawn — the four panels in `render.rs`, the overlay, and the too-small notice.
A terminal cell has no sub-cell geometry, so `╭ ╮ ╰ ╯` is the only radius
available and "2px" is what it looks like.

Nothing else moves: `Block::inner` is unaffected by border type, so no panel
gains or loses a column.

## 3. Scrollbar

Two defects, one cause. `Scrollbar::new` selects
`symbols::scrollbar::DOUBLE_VERTICAL` for a vertical orientation — track `║`,
thumb `█` — and the scrollbar is drawn over the block's right border column.
The `║` is what reads as a double border.

- **Track** becomes `│`, identical to the border glyph underneath it, so the
  track disappears rather than being drawn.
- **Thumb** becomes `▐`, half a cell, flush with the panel's outer edge.

The track is currently styled `Token::Muted` along with the thumb, which dims
the stretch of border it overwrites. Track and thumb get separate styles: the
track takes the block's own border style — bold when the panel is focused —
and the thumb keeps a style of its own. A scrollbar that is invisible except
for its thumb is the point; a dim stripe interrupting a bold border is the bug
that would remain after fixing the glyphs.

The `ScrollbarState` arithmetic in `render.rs` is correct and is not touched.

## 4. Loader

### 4.1 The dependency

`ratatui-opentui-loader` 0.2 — MIT, `ratatui = "0.30"`, no other dependencies.
It clears `deny.toml` on all three counts: the licence is already allowed, the
ratatui requirement resolves to the 0.30.2 already in the tree so
`multiple-versions` stays satisfied, and it is a registry crate rather than a
git one.

It is chosen over an in-house scanner because `tick()` is a pure
`frame_index = (frame_index + 1) % total_frames` — no `Instant`, no
`SystemTime`, nothing the determinism rule objects to.

### 4.2 Where the frame number lives

`Msg::Tick` exists today, nothing sends it, and `update` discards it. Animation
is the first thing that needs it, and it enters through that one door rather
than through a clock read in the renderer.

- `lc-app` spawns a ticker sending `Msg::Tick` every 40 ms, the interval the
  crate is built around.
- `State` gains a frame counter. `update` advances it on `Tick` and marks the
  state dirty **only while work is in flight**: connecting, schema loading, or
  a page outstanding. Idle, a tick costs one channel message and no draw.
- Going idle resets the counter to zero, so the resting pose is always the
  same frame rather than wherever the animation stopped.
- `ViewModel` gains `activity: Activity`, an enum of `Resting` and
  `Working { frame }`. The renderer draws both the same way — `Resting` is
  frame zero — but the distinction is what `lc-app` and `update` are deciding,
  so it is a type rather than a naked `usize` the domain promises to zero.

### 4.3 Drawing it

`lc-inbound` builds a `KittLoader` and advances it `frame` times each draw. The
renderer stays a pure function of the `ViewModel`, which is what keeps the
render tier deterministic and its tests exact. The period is 54 frames — width
8, plus 9 held at the right edge, plus 7 back, plus 30 held at the left — and
54 is a private field of the widget, so a test pins it by asserting
`into_line` at frame 0 equals `into_line` at frame 54 rather than a comment
claiming it.

The accent is `#fab283`, one named constant in `theme.rs`. The crate is RGB
only: `with_color` falls back to pure red for any non-`Color::Rgb` input, so
an ANSI accent is not available and is not attempted.

### 4.4 Placement

Eight cells and a space at the left of the bottom row; the hint bar is given
`bar.width - 9`. The bar's existing fit policy — drop panel hints right to
left, then `tab panel`, keeping `q quit` longest — absorbs the loss with no new
rule. At the 40-column floor this costs one hint: the bar goes from
`j/k move  l/h fold │ tab panel  q quit` to dropping `l/h fold`.

The loader is drawn at every width. Hiding it below a threshold would trade a
visible, understood hint for an invisible one, and the bar already knows how
to give up hints in value order.

## 5. Table DDL

### 5.1 Headings and grouping

`PARTITION` and `CLUSTERING` become `Partition Key` and `Clustering Key`, and
`OPTIONS` and `REPLICATION` follow into title case for consistency. A blank
line precedes the settings section, so the panel reads as two groups — what
the table *is*, then what it is *configured to do* — rather than as one run of
headings.

This is a partial reversal of the "no blank line per section" rule in
`section_lines`, which spent no rows on separation because the panel has few.
One row buys the structural split; a row before every section would not.

### 5.2 Short properties as a table

A property whose name and value fit on one line is drawn on one line, with the
value right-aligned at the panel margin. A property that does not fit keeps
today's shape: name on its own line, value wrapped and indented beneath it.

This is the shape `column_lines` already gives a column and its CQL type, so
the two share one helper rather than growing a second alignment rule.

**Right margin, not a fixed value column.** A fixed column is computed from the
widest name, and one long name then poisons every other row: at a 28-column
inner width `bloom_filter_fp_chance` puts the column at x=23, and
`gc_grace_seconds` with `864000` needs 29. Right-aligning each row
independently cannot fail that way, aligns numbers on their last digit, and
lets a row that does not fit fall back on its own without disturbing its
neighbours.

**No short/long list.** `comment`, `compaction` and `compression` wrap because
they are long, not because they are named. A cluster whose properties predate
or postdate `BY_IMPORTANCE` gets the same treatment, and a wider sidebar
promotes more properties to one line without an edit.

Property names stay unindented, at column zero. Indenting them to match a
column's two would read more consistently and cost two of the columns that
decide whether a row fits at all.

At a narrow width nothing fits on one line and the panel looks as it does
today. That is the intended degradation, not a gap.

```
shop.orders_by_customer

Partition Key
  customer_id           uuid
Clustering Key
  order_date ↓          date
  order_id ↑        timeuuid

Options
comment
  Order history.
default_time_to_live       0
gc_grace_seconds      864000
compaction
  org.apache.cassandra.db.co
  mpaction.SizeTieredCompact
  ionStrategy
cdc                    false
```

### 5.3 Property names are highlighted

A new `Token::PropertyName`, drawn `#7aa2f7` — one constant in `theme.rs`,
distinct from the loader's peach, legible on light and dark backgrounds. The
value is left unstyled: in a twenty-column panel two competing treatments on
one line is one too many.

The token is named for what it marks rather than for its colour, so the domain
still names no colour (architecture.md §6).

## 6. Determinism

Nothing here reads a clock. The loader's phase is a domain-held integer
advanced by `Msg::Tick`, the renderer is a pure function of the `ViewModel`,
and the render tier's tests set the frame explicitly. `lc-app` owns the only
timer, which is where real time already lives.

## 7. What this amends

vision.md §4 principle 5 says LazyCassandra "draws with the sixteen ANSI
colors" and that "decoration is not a reason to spend a colour". The loader is
decoration and spends truecolor; the property-name highlight spends a colour
that carries no protocol meaning. Both contradict the principle as written.

The principle is narrowed rather than dropped. What survives is the part that
made it worth having: **the terminal's background is never overridden and body
text keeps the terminal's own foreground**, so the tool sits inside the user's
setup instead of repainting it. Highlights — icons, accents, marked names —
are ours to choose, truecolor included.

The `theme.rs` module doc repeats the old claim and follows. Its
`no_token_sets_a_background` test survives unchanged and becomes the mechanical
statement of the narrowed rule: the background is the part no token may touch.

The three `Posture` colours stay reserved. Nothing here draws write posture,
and the accent colours are picked to sit clear of blue, green and magenta so
that the indicator, when it lands, is not competing with decoration.
