# Mouse input — design

**Date:** 2026-08-09
**Status:** designed, not implemented

Click a keyspace to fold it, a table to read it, a row to select it, and roll
the wheel over a panel to scroll it.

doc/shortcuts.md §4 currently lists the mouse under "deliberately unbound", and
§3 says every mouse event "is dropped at the edge and never reaches the domain".
That was the right default while there was nothing to point at; it is a
statement of scope, not a principle, and this design retires it. Everything else
in that file stays true — in particular the key budget, which the mouse spends
nothing from.

---

## 1. The rule that keeps the mouse honest

**Every gesture decomposes into bindings that already exist.**

A click on a tree row is `Cursor::go_to` followed by the `Enter` arm. A click on
a grid row is `Cursor::go_to`. A wheel notch is three `Motion::Down`s. Nothing
the mouse does is reachable only by mouse.

This is not tidiness. doc/shortcuts.md §2 requires a binding to be visible, and
the hint bar names keys — a gesture cannot appear in it. A mouse-only capability
would therefore be a capability with nowhere to announce itself, discoverable
only by a user who already suspected it was there, which is the failure mode
principle 1 of vision.md §4 exists to prevent. Holding the decomposition rule
means the bar keeps telling the whole truth: everything the program can do is
still named there, and the mouse is a second way to reach it.

It also bounds the design. "Should a click on a column header sort the grid?" is
answered before it is asked — there is no sort binding, so there is no sort
gesture.

## 2. What each gesture does

| Gesture | Lands on | Effect |
|---|---|---|
| Left press | keyspace row | Focus Tables, select it, fold/unfold |
| Left press | table row | Focus Tables, select it, read it into Contents |
| Left press | Contents row | Focus Contents, select it |
| Left press | chrome | Focus that panel, nothing else |
| Left press | anywhere, overlay up | Dismiss the overlay, nothing else |
| Left press | the hint bar | Nothing |
| Wheel | a panel that scrolls | Three rows or lines. Focus does not move |
| Wheel | Queries, or overlay up | Nothing |
| Right, middle, drag, move, release, horizontal wheel | anywhere | Dropped at the edge |
| Any modified press | anywhere | Dropped |

"Chrome" is a border, a title, a grid's header row, a `Notice`, the Ddl body,
the Queries panel, and the blank space past the last row — everything inside a
panel that is not a row a cursor can sit on.

**A click acts even when the panel it lands in is not focused.** One click
focuses Tables *and* reads the table. The pointer carries a position, so it
does not need focus to say what it means, and a first-click-focuses rule would
double every cross-panel interaction for a safety nobody asked for. Chrome is
the escape hatch: it moves focus and does nothing else, which is what makes a
panel's border a safe place to click.

## 3. Where the decision is made

A click is a coordinate. Coordinates are the renderer's business —
architecture.md §2 gives `lc-inbound` the layout and gives the domain no notion
of width at all. Behaviour, meanwhile, lives in `update`. Three ways to bridge
that:

**Chosen — `lc-inbound` resolves, and the domain receives a resolved gesture.**
The renderer reports a hit map alongside the geometry it already reports;
`lc-app` holds it and hands it to the translator with each event. No `Rect` ever
crosses into the domain, and `lc-inbound` still decides nothing — it translates
a crossterm event into domain vocabulary exactly as `to_key_press` does.

**Rejected — send raw `(x, y)` and let the domain hit-test.** It would need
panel rects, which means a second copy of the layout rules in `lc-domain`, kept
in step with `layout.rs` by hand. The crate boundary exists to make that
impossible rather than merely discouraged.

**Rejected — a `Renderer` struct owning its map between frames.** A tidier call
site in `main.rs`, bought by turning a free function into stateful adapter and
reshaping every render test. No behavioural gain.

## 4. The hit map

`render` returns both halves of what a frame turned out to be:

```rust
// lc-inbound
pub struct Rendered {
    pub geometry: Geometry,
    pub map: FrameMap,
}
```

`geometry` still travels into the domain as `Msg::Measured`, still only when it
differs from the last one sent. `map` stops at `lc-app`, which holds it beside
the `Option<Geometry>` it already holds.

**The two run backwards for different reasons and only one needs a guard.**
`Geometry` re-enters the domain, so it can close a redraw–measure–redraw loop
and is guarded by the invariant in architecture.md §3: it is a function of the
terminal size and the view model alone. The hit map re-enters nothing. Nobody
reads it until a human moves a mouse, so it is free to depend on the offsets
`Geometry` may not.

A `Shell::TooSmall` frame produces an empty map. Nothing is drawn, so nothing is
clickable.

### Built from what was drawn, not recomputed

The map is a second description of the layout, and a second description can
drift from the first. Drift here is not a crash — it is clicks landing on the
wrong row, quietly, and in Tables that means reading a table the user did not
pick.

So the map is not computed. It is assembled from the values that did the
drawing: the `Rect` each panel was given, and the `Extent` its body returned.
`Extent::position` is already the first row drawn — computed every frame today
and thrown away above the scrollbar. The body reports what that number counts
in:

```rust
// lc-inbound/src/body.rs
pub(crate) struct Body {
    pub(crate) extent: Extent,
    pub(crate) unit: Unit,
}

/// What a body's scroll unit is, for a click that lands in it.
pub(crate) enum Unit {
    /// Selectable rows. `header` rows of chrome sit above the first one.
    Rows { header: u16 },
    /// Wrapped lines: scrolled, not addressable.
    Lines,
}
```

`Unit::Rows { header: 1 }` for the grid, `Unit::Rows { header: 0 }` for the
tree, `Unit::Lines` for the Ddl body, and no `Body` at all for `Blank` and
`Notice`, which already return `None`. An enum rather than a `bool` plus a
comment, per conventions.md §1: there is no way to say "these are rows" without
also saying how many are spent above the first.

The row a press at screen row `y` landed on is then
`extent.position + (y - inner.y - header)`, valid while that is below
`extent.total` and `y` is inside `inner`. Outside any of it, the press is
chrome.

## 5. The domain vocabulary

A new `lc-domain/src/pointer.rs`. It names `state::Panel`, so it sits beside
`state.rs` rather than under `model/`, where `KeyPress` lives.

```rust
/// A pointer gesture, already resolved against the frame it happened in.
///
/// The domain never sees a coordinate: `lc-inbound` owns the layout and reports
/// the panel and row a press landed on, the same way it reports a `KeyPress`
/// rather than a crossterm event.
pub enum Pointer {
    Click(Click),
    Wheel { panel: Panel, direction: Wheel },
}

pub enum Click {
    /// A row of a panel that has rows.
    Row { panel: Panel, row: usize },
    /// Border, title, header, blank space, or a body with no rows.
    Panel(Panel),
}

pub enum Wheel {
    Up,
    Down,
}
```

One `Msg::Pointer(Pointer)` arm, dispatched to one `handle_pointer` beside
`handle_key`. doc/shortcuts.md §2's "one dispatch point" holds for the pointer
by construction rather than by rule: there is exactly one function that can turn
a `Pointer` into behaviour, because there is exactly one arm that carries one.

Two helpers stop reading `state.focus` and start taking a panel:
`focused_row_count(state)` becomes `row_count(state, panel)`, and
`scroll(state, motion)` becomes `scroll_panel(state, panel, motion)`. The
keyboard paths pass `state.focus`. That a gesture names its panel and a key does
not is the only difference between the two routes into the same behaviour.

## 6. Five decisions inside the table

**A stale row is dropped, not clamped.** The map describes the *previous* frame.
Between the frame and the press a page can arrive or a keyspace can fold, and
`Cursor::go_to` clamps to the last row — so a click on a row that has since gone
would act on whatever now sits at the end of the list. In Contents that is a
wrong highlight; in Tables it is a query against a table the user never pointed
at. `handle_pointer` therefore requires `row < row_count(state, panel)` and
drops the gesture otherwise. Focus still moves: the panel was certainly there.

**Shift is refused, which the keyboard rule does not do.** `handle_key`
deliberately ignores shift so a shifted arrow stays a synonym for the plain one.
A pointer cannot afford that. Under capture, shift-drag is the terminal's own
text-selection escape hatch (§7), so a terminal that forwards a shifted press is
telling us the user is trying to select text, not click a row. ctrl and alt are
refused for the ordinary reason: a modified gesture is a binding that does not
exist yet.

**The wheel does not move focus.** Focus decides what the hint bar names and
where `j` goes. Having both jump under the pointer while scrolling past
something is disruptive, and scrolling is a glance where clicking is a
commitment. This is the softest call in the design; it is also the cheapest to
reverse.

**Over Tables and Contents the wheel moves the selection, not a viewport.**
Those panels store no offset — `body.rs` derives one per frame from the selected
index, which is why no scroll state exists anywhere for a wheel to move. The
visible consequence is that the view only scrolls once the selection reaches an
edge, exactly as it does under a held `j`. Honest, and slightly janky. If the
bounded result-set cache of architecture.md §11 ever forces a stored viewport,
the wheel should move that instead; TODO.md carries the note.

**A wheel notch is three rows**, or three lines in the Ddl panel — whatever the
panel scrolls in. One is sluggish enough that the pointer feels broken; three is
what terminals and terminal applications have settled on. It is a named
constant, not a literal, because it is a guess a week of use may correct.

## 7. What capture costs, and who pays

`EnableMouseCapture` takes click-drag text selection away from the terminal.
Most terminals leave shift-drag as a bypass; some do not, and a user in one of
those loses selection inside LazyCassandra entirely.

That cost is accepted unconditionally: capture is on from startup, with no
toggle and no flag. The alternatives were a binding that suspends capture —
which spends a key and owes the user a visible indicator of which mode they are
in, for a problem shift-drag already solves nearly everywhere — and an opt-in
env var, which is invisible, and invisible in a tool whose first principle is
ambient discoverability. There is also no configuration system yet to opt in
through (`main.rs`'s env vars are scaffolding, not a mechanism), so opt-in would
mean shipping the feature switched off for everyone.

Revisit when config layering lands: this is exactly the kind of preference
vision.md §6's user-level config should carry.

## 8. Capture must survive a panic

`ratatui::restore()` disables raw mode and leaves the alternate screen. It knows
nothing about mouse capture, and the panic hook `ratatui::init()` installs only
calls `restore()`. Leak capture and the user's terminal prints `<35;58;12M` at
them on every pointer movement until they run `reset` — the interaction is the
one architecture.md §11 already flags as open, in a new form.

One type in `lc-app` owns the whole lifecycle:

```rust
/// Enables mouse capture and guarantees its release — on return, on `?`, and
/// on panic. `ratatui`'s hook restores raw mode and the alternate screen but
/// not this, so the hook is chained rather than replaced.
struct MouseCapture(());
```

Constructed after `ratatui::init()`, so ratatui's hook is already in place to
chain onto. It enables capture, takes the current hook and sets one that
disables capture before delegating; `Drop` disables. On an unwind both fire,
which is harmless — the sequence is idempotent. There is no way to hold capture
without holding the thing that releases it (conventions.md §1).

## 9. Testing

Fast tiers only. No new I/O, and nothing here needs a terminal.

**`lc-inbound` — hit-testing.** The new arithmetic, so most of the weight:
a press on each drawn row of a scrolled tree resolves to that row's index; a
press on a grid's header row is chrome rather than row 0; a press below the last
row is chrome; borders and titles are chrome; a press on a `TooSmall` frame
resolves to nothing; right button, drag, release and horizontal wheel produce
nothing; a modified press produces nothing.

**`render.rs` — the drift guard.** Render a known tree and a known grid, then
require the reported map to resolve every drawn row back to the index that drew
it. This is the test that catches the map and the drawing disagreeing, which is
the one failure in this design that is silent.

**`lc-domain` — behaviour.** A click on a keyspace folds and unfolds it; a click
on a table reads it and cancels the outstanding read; a click on a Contents row
moves the cursor and emits no effect; a click focuses the panel it landed in; a
click on chrome focuses and does nothing else; a click whose row no longer
exists is dropped rather than clamped; a click dismisses an overlay and does
nothing else; a wheel leaves focus alone; a wheel over Queries is inert; a wheel
over Ddl scrolls lines.

## 10. Documentation this changes

- **doc/shortcuts.md** — §3's claim that every mouse event is dropped at the
  edge becomes false, and §4's `Mouse` row leaves "deliberately unbound". A new
  section states §1's decomposition rule and that gestures spend nothing from
  the key budget. §5 gains the tests above.
- **doc/architecture.md** §3 — the edge that runs backwards now carries two
  things. One sentence distinguishing them, as §4 above does.
- **TODO.md** — the wheel-moves-selection note.

## 11. Not in this slice

- **Double-click, drag, and hover.** Nothing in §1 decomposes to them: there is
  no second activation, no range selection, and no tooltip surface to hover.
- **Clicking a column header to sort.** No sort binding exists (§1).
- **Dragging a panel border to resize.** Layout comes from doc/ui-draft.png and
  is not user-adjustable; making it so is a design of its own.
- **Clicking a hint in the bar to invoke it.** Tempting and cheap, but a hint
  that is also a button stops being a label, and the bar's job is to teach the
  key. Worth revisiting once the bar has been used for a while.
- **A pointer for the Queries panel.** It is blank. Text entry brings a caret,
  a selection, and a click-to-position-caret gesture with it, and that is the
  editor's design to make.
