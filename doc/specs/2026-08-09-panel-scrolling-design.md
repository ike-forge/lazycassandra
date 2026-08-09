# Panel scrolling — design

**Date:** 2026-08-09
**Status:** implemented

Every panel reaches its content. Three of the four already scroll after a
fashion — Tables and Contents because a moving selection drags the viewport
with it, Queries because it is empty — and Table DDL does not scroll at all:
it holds seven rows at 80x24 and a table's options run past that, so
everything below the fold is unreachable (`TODO.md`, from the table metadata
slice).

This closes that, and gives all four panels one vertical vocabulary rather
than leaving `j` and `k` dead in one of them — vision.md §4 principle 2, the
same key at every level.

---

## 1. Two things are being scrolled

Conflating them is what makes this awkward.

**Tables and Contents scroll as a consequence.** The domain owns a selection;
`ListState`/`TableState` derive the offset at draw time from the selected
index, so no scroll state is stored anywhere (`inbound/body.rs`). That
mechanism is untouched. Those panels gain paging motions and nothing else.

**Table DDL scrolls a viewport.** It has no selection, and does not grow one:
a selection nothing acts on is a promise the panel cannot keep, and a
reverse-video bar over a four-line wrapped option value reads as damage. It
gets a real offset.

A hidden cursor — a `Cursor` driving `ListState` with no highlight drawn —
was considered and rejected. It would need several `j` presses before
anything moved on screen, because the offset only changes once the selection
leaves the viewport. The panel is a document; it must move on the first press.

## 2. The DDL offset counts lines, not rows

One `DetailRow` becomes one to five physical lines at the sidebar's width:
`property_lines` puts the name and its value on separate lines and `wrap`
breaks the value again, and `compaction`'s class name alone is one
unbreakable 60-character token (`inbound/details.rs`).

So a page of rows is not a page of lines. An offset counted in rows would
make `PageDown` scroll roughly `height × 2` lines and **skip text the reader
never saw** — in a panel that exists to be read, worse than not scrolling.
Counting lines makes the page motion and the end-stop exact.

The cost is that the domain cannot compute the length it is scrolling over.
Wrapping needs a width, and width-dependent layout is the renderer's business
for the same reason scrolling was (`inbound/body.rs`, on column widths). The
number has to cross the boundary.

## 3. The measurement message

```rust
/// The geometry of the frame just drawn.
///
/// The one place the renderer tells the domain something. `Resized` is not
/// enough: the Ddl body's length changes when the Tables selection moves,
/// with no resize involved.
Measured(Geometry),
```

```rust
/// What the last frame turned out to be.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Geometry {
    /// Content rows each panel can show, in `Panel::ALL` order.
    ///
    /// A row is whatever that panel scrolls in — a tree row, a grid row, a
    /// wrapped line of the Ddl body — which is what lets one array serve all
    /// four.
    heights: [usize; 4],
    /// Lines the Ddl body wrapped to.
    ///
    /// The one length the domain cannot count for itself (§2). Tree rows and
    /// grid rows it counts, and must keep counting — two sources of truth for
    /// a row count is exactly the drift `tree.rs`'s single flattening exists
    /// to prevent.
    ddl_lines: usize,
}

impl Geometry {
    /// `heights` counted in the unit each panel scrolls in, and net of what
    /// the panel spends on chrome: not the panel's inner height, because the
    /// Contents grid spends a row on its header, which `lc-inbound` subtracts
    /// here so the header stays a rendering fact rather than something
    /// `update` has to know about.
    pub const fn new(heights: [usize; 4], ddl_lines: usize) -> Self;

    pub const fn height(self, panel: Panel) -> usize;
    pub const fn ddl_lines(self) -> usize;
}
```

`heights` counts in `usize` rather than `u16` so `height` stays a `const fn`
with no cast: `usize::from` is not const.

A panel the terminal has collapsed to zero rows reports `ddl_lines = 0` — the
body is not drawn, so there is nothing to measure — and the clamp below pulls
its offset to the top. Shrinking the terminal past the Ddl panel and growing it
back therefore returns that panel to the top. Accepted: it is the same answer
`clamp_to` gives for any panel taller than its document, and the alternative is
remembering an offset for a panel that is not on screen.

`height` takes a `Panel` rather than exposing the array, mirroring
`layout::Rects::get` on the other side of the boundary, so no caller indexes
by hand.

`lc_inbound::render` returns a `Geometry`. `terminal.draw` yields a
`CompletedFrame` rather than its closure's value, so the measurement is
captured out of the closure — in one helper, because `lc-app` draws both
before the loop and inside it:

```rust
fn draw(terminal: &mut DefaultTerminal, state: &State, theme: Theme) -> Result<Geometry> {
    let mut geometry = Geometry::default();
    terminal.draw(|frame| geometry = lc_inbound::render(frame, &view_model(state), theme))?;
    Ok(geometry)
}
```

`lc-app` holds the last geometry it sent and sends `Msg::Measured` **only when
the new one differs**, in a helper because both draw sites report:

```rust
fn report(tx: &mpsc::UnboundedSender<Msg>, last: &mut Option<Geometry>, geometry: Geometry) {
    if *last == Some(geometry) {
        return;
    }

    *last = Some(geometry);
    // A closed channel means the main loop has already ended.
    let _sent = tx.send(Msg::Measured(geometry));
}
```

The send error is swallowed rather than propagated: the only way the channel is
closed is that the loop this measurement feeds has already stopped.

This is a feedback edge into a loop that has not had one, so it has to
terminate visibly rather than by argument:

- `lc-app` sends nothing when the geometry is unchanged.
- `update` marks dirty on `Measured` **only if the clamp actually moved the
  offset**. A `Measured` that changes nothing leaves `dirty` false, so no
  frame is drawn, so no further measurement is taken.

The scrollbar cannot oscillate into it either: it is drawn over the block's
border column (§6), so its appearance does not change any panel's inner width
and cannot change the measurement that produced it.

`Msg::Resized` stays exactly as it is — it carries no geometry and exists to
trigger the repaint that produces the measurement.

## 4. `Scroll`

```rust
/// A viewport offset over a document of known length.
///
/// The index is private for the reason `Cursor`'s is: the invariant is
/// established here, so a renderer is never handed an offset past the end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scroll {
    offset: usize,
}

impl Scroll {
    pub const fn offset(self) -> usize;

    pub const fn down(&mut self, len: usize, height: usize);
    pub const fn up(&mut self);
    pub const fn page_down(&mut self, len: usize, height: usize);
    pub const fn page_up(&mut self, height: usize);
    pub const fn to_start(&mut self);
    pub const fn to_end(&mut self, len: usize, height: usize);
    /// Pulls a stale offset back over a document that shrank or a panel that
    /// grew.
    pub const fn clamp_to(&mut self, len: usize, height: usize);
}
```

Every bound is `offset <= len.saturating_sub(height)`, in one place. That is
what makes `End` leave the last line at the bottom of the panel rather than
dragging it to the top with a gap beneath it, and what makes a panel that
grows past its content scroll back to zero on its own.

`Cursor` gains the matching motions, in the shape its existing `down(len)`
already has:

```rust
pub const fn page_down(&mut self, len: usize, height: usize);
pub const fn page_up(&mut self, height: usize);
pub const fn to_start(&mut self);
pub const fn to_end(&mut self, len: usize);
```

`State` grows two fields:

```rust
pub ddl: Scroll,
pub geometry: Geometry,
```

**The offset belongs to the node being described, not to the panel.** `update`
compares what the Tables selection is on before and after applying a message —
keyspace name, or keyspace and table name — and returns the offset to the top
when it differs, so a table opens at its heading rather than at line 34 of the
last one. Unfolding is not part of that identity: it leaves the keyspace
selected and the panel on the same subject. Stating it once around `update`
rather than at each of the four paths that can move the selection (`scroll`,
`transition`, `activate`, the `SchemaLoaded` clamp) is what keeps a fifth path
from being added without one.

`Scroll` and `Geometry` live in `state.rs` beside `Cursor`. The Ddl panel gets
no module of its own — it holds one offset and no transitions, so there is
nothing for a module to keep together. Architecture.md §11's question about
`State` staying one struct is unchanged by this, not answered.

## 5. Bindings

`Home`, `End`, `PageUp` and `PageDown` already translate from crossterm
(`inbound/input.rs`); they are simply unhandled in `update`.

| Key | Tables · Contents | Table DDL |
|---|---|---|
| `j` / `Down`, `k` / `Up` | cursor ±1 row | offset ±1 line |
| `PageDown` / `PageUp` | cursor ± `height` rows | offset ± `height` lines |
| `Home` / `End` | first / last row | top / last line at the bottom |

Motion belongs to the focused panel, as it already does. `scroll` answers `Ddl`
before it reaches `focused_row_count`/`focused_cursor`, which keep returning
`0`/`None` for it — a viewport is not a cursor, so there is nothing for those
two to hand back. `Queries` returns nothing either, so its measured height is
carried and unused until it has content.

`ctrl-d` and `ctrl-u` are **not** in this slice. `handle_key` refuses every
ctrl and alt press as a stated decision, and defining vim's half-page motions
means reworking that guard and the test that pins "a modified key is not a
binding" — a change whose blast radius is every existing binding, and which
should be made when the ctrl layer is designed rather than in passing. It goes
to `TODO.md`.

## 6. Drawing it

`body::render` returns what it already computed:

```rust
/// What a scrollbar needs, in whatever unit the panel scrolls in.
pub(crate) struct Extent {
    position: usize,
    total: usize,
    visible: usize,
}

impl Extent {
    pub(crate) const fn overflows(&self) -> bool;
}
```

Tables and Contents read the offset back out of the `ListState`/`TableState`
they own, after `render_stateful_widget` has computed it. Ddl reports the
offset from the `ViewModel` against its own line count. `Blank` and `Notice`
report nothing: a notice is one sentence, and the long form of a failure is
what the overlay is for.

`render.rs` draws a `Scrollbar` with `ScrollbarOrientation::VerticalRight`
when `total > visible`, over the block's right border column — so it spends no
content width, and its absence is itself information: nothing is hidden.

`ScrollbarState` is given `total - visible` as its content length, not `total`.
Its `position` is modelled as a selection index over `0..content_length`, and
ours is a top-of-viewport offset whose maximum is `total - visible`; handed
`total`, the thumb never reaches the bottom of the track even with the last
line on screen.

It draws in `Token::Muted`. The bar is chrome, not meaning, and
architecture.md §6 forbids spending a token on decoration; `Muted` is the
token that already says "present, not the point".

One `ViewModel` change:

```rust
Details { details: Details, offset: usize },
```

## 7. Testing

**Domain.**

- `Scroll` stops at zero going up and at `len - height` going down, and
  `to_end` leaves the last line visible rather than at the top.
- A page moves exactly `height`, and a page against a document shorter than
  the panel moves nothing.
- `clamp_to` pulls a stale offset in when the document shrinks (the Tables
  selection moves to a table with fewer options) and when the panel grows.
- Motion routes to the focused panel only, extended to `Ddl`: `j` with
  Contents focused must not move the Ddl offset, and vice versa.
- **`Measured` that changes nothing leaves `dirty` false.** This is the test
  that matters most — it is what pins the loop in §3 shut. A `Measured` that
  does clamp the offset marks dirty.
- Every motion emits no effects, in the shape
  `a_shortcut_moves_focus_and_does_nothing_else` already uses.
- A motion that lands the tree on a different node returns the Ddl offset to
  the top; unfolding a keyspace, which changes what is on screen but not what
  is being described, leaves it alone.

**Render.**

- At a size where the Ddl body overflows, a below-the-fold line appears after
  scrolling and the top line is gone — the same shape as the existing
  `the_selection_stays_visible_when_the_list_outgrows_the_panel`.
- The scrollbar column is drawn when the content overflows, and the border is
  intact when it fits.
- The thumb sits at whichever end of the track the viewport is on, so the two
  ends of the bar mean what they look like.
- The scrollbar costs no content width: one exact row-string assertion at a
  size where the panel *does* overflow, pinning content and bar together, on
  top of the existing ones at a size where nothing does.
- **What a frame measures does not depend on where it is scrolled.** This is
  the invariant both of §3's termination guards rest on, and nothing else in
  the suite would notice it breaking.

**Determinism.** `Measured` is a message like any other — no clock, no
randomness, no dependence on real terminal size in the fast tiers. The render
tier drives `TestBackend` at fixed sizes as it already does.

## 8. Knowingly left out

- **Horizontal scrolling in the Contents grid.** Columns past the right edge
  are still dropped. Column priority and horizontal navigation are vision.md
  §8's open research question, and answering it inside a vertical-scrolling
  slice would be guessing.
- **`ctrl-d` / `ctrl-u`** (§5).
- **Scrolling a `Notice`.**
- **The Queries panel's content.** It scrolls when it has something to scroll.
- **`Cursor`'s caller-supplied `len`.** The smell logged in `TODO.md` from the
  table-list review is neither fixed nor worsened here; the new motions take
  `len` in the same shape the existing ones do, so the eventual fix is still
  one change in one place.
