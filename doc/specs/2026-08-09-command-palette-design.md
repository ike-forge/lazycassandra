# Command palette — design

**Date:** 2026-08-09
**Status:** implemented

A scrollable block, centred over the panels, listing every binding the program
has. `?` opens it.

doc/shortcuts.md §4 reserved `?` for exactly this and declined to guess its
shape: "vision.md §7 Phase 0 promises a contextual map. It is its own slice and
this set does not anticipate its shape." This is that slice.

It completes the pair the hint bar started. The bar
([design](2026-08-09-hint-bar-design.md)) names the focused panel's principal
bindings in one row and says so — §6 of that design is explicit that it has
never claimed to be the complete list. The palette is where the complete list
lives, and the two claims are different enough that §2 below keeps them apart
on purpose.

---

## 1. What it is, and what it is not yet

It is named **Commands**, not **Shortcuts**, and its rows carry a selection.

That is a deliberate bet on where this goes. A read-only help table would be a
smaller thing to build and would serve today's requirement exactly: list the
bindings, scroll them, close it. The name and the cursor are what let search
and `⏎`-to-run be added later without renaming the feature or retrofitting a
selection model onto a `Paragraph`.

What is **not** built now: search, execution, and any notion of a command that
is not already a key binding. §10 says what that costs.

The cursor is not decoration in the meantime. It is what the viewport follows,
exactly as the Tables tree's cursor is — the palette scrolls by moving a
selection, not by moving an offset, and §4 is why that choice is not free.

## 2. Where the list comes from

The palette claims to be complete. That is a stronger claim than the bar makes,
and a complete list that goes stale is the "bar that lies" the `Binding` type
was introduced to prevent (hint bar design §2).

A **catalogue** in the domain holds every binding, grouped into sections:

```rust
// lc-domain/src/commands.rs

/// One labelled group of bindings.
pub struct Group {
    pub heading: &'static str,
    pub bindings: &'static [Binding],
}

pub const CATALOGUE: &[Group] = &[ /* §6 */ ];
```

`Binding` is the hint bar's type, unchanged: `keys: &'static [Key]` and an
action string. Reusing it means the palette inherits the property the bar
already has — a row cannot name a key no arm of `handle_key` claims, because it
names `Key` values and `hints::label` renders them.

**The catalogue does not replace `Panel::hints()` or `GLOBAL`.** Those keep
their own terse labels. The bar has one row and says "move"; the palette has a
column and says "down one row". One label per key cannot serve both, and the
attempt would be worse than the duplication: `j`/`k` is "move" in Tables and
"scroll" in Table DDL, so the bar's wording is already per-panel in a way a
single global catalogue entry cannot express.

What is *not* allowed to drift is the keys, and §9's sweep is what enforces
that rather than a shared data structure. The bar and the palette may word the
same binding differently; neither may name a key that does nothing, and the
palette may not omit a key that does something.

## 3. State

`state.overlay` is today an `Option<Overlay>` where `Overlay` is a message —
title, body, severity. The palette is a second kind of thing drawn in the same
place, and the two must never be on screen at once.

```rust
/// Today's `Overlay`, renamed. Unchanged otherwise.
pub struct Message {
    pub title: String,
    pub message: String,
    pub severity: Severity,
}

/// What is drawn over the panels, at most one at a time.
pub enum Overlay {
    Message(Message),
    Commands(CommandsPanel),
}

/// The palette's whole state: the catalogue is static, so a cursor is all
/// there is to remember.
pub struct CommandsPanel {
    cursor: Cursor,
}
```

An enum rather than a second `Option<CommandsPanel>` field beside the existing
one. Two fields make "a failure message and the palette, simultaneously"
representable, and then the renderer has to invent a precedence rule the domain
never stated (conventions.md §1). One field means `Esc` keeps meaning one
thing, and the renderer keeps having one branch.

**`?` over a message replaces it.** The user asked for the palette explicitly
and gets it; the failure is not lost, because the panel underneath is already
narrating the same condition — that is why the message overlay is dismissible
at all. `Esc` from the palette then leaves nothing, rather than peeling back to
the message. A stack of overlays is a thing to build when something needs one.

`CommandsPanel` lives in its own module, `lc-domain/src/commands.rs`, with the
catalogue. Same reason `TablesPanel` and `ContentsPanel` left `state.rs`
(architecture.md §11).

## 4. Rows, and a cursor that skips headings

The palette flattens to rows the way the tree does:

```rust
pub enum CommandRow {
    Heading(&'static str),
    Command { keys: String, action: &'static str },
}
```

`keys` is a `String`, not `&[Key]`: `hints::label` has already turned the keys
into text by the time a row exists, and the renderer never sees a `Key` — same
boundary `Hint` draws.

A heading is a row, so the flat list is what both the cursor and the renderer
count in, and there is one row-numbering scheme rather than two that must agree.
But a heading is not selectable, so `CommandsPanel` cannot simply delegate to
`Cursor`:

- `down` / `up` step to the next or previous **command** row, stopping on the
  last and first command rather than on a heading.
- `page_down` / `page_up` move by the visible row count, then settle onto the
  nearest command in the direction of travel — and, if that runs out of
  catalogue, onto the last or first command, as `down` and `up` do.
- `to_start` / `to_end` land on the first and last command.
- A palette is opened with its cursor on the first command, which is row 1 —
  row 0 is the `Global` heading.

Every motion therefore terminates on a command row, and the invariant that
`Cursor`'s private index buys the panels — no caller can construct an
out-of-range selection — is extended here to "no caller can construct a
selection on a heading". A renderer never has to defend against either.

The alternative — index only commands and have the view map a command index to a
flat row — was rejected because the mapping has to exist somewhere and it is
cheaper as four motion methods than as a translation layer every renderer and
every test has to apply.

The view model mirrors the state's split:

```rust
// lc-domain/src/view/mod.rs
pub enum OverlayView {
    Message(Message),
    Commands { rows: Vec<CommandRow>, selected: usize },
}
```

`Message` is shared with `state::Overlay` rather than restated, so the one
overlay kind that is pure data stays one type. The palette's variant differs
because its state is a cursor and its projection is rows — the same reason
`TablesPanel` and `PanelBody::Tree` are not the same type.

## 5. Size

Half the frame wide, clamped; seven tenths tall, capped at what the catalogue
actually needs.

```rust
// lc-inbound/src/palette.rs
const WIDTH_PERCENT: u16 = 50;
const HEIGHT_PERCENT: u16 = 70;
/// The widest row the catalogue can produce, plus both borders and both pad
/// columns: a `shift-tab` gutter (9), two columns of gap, the longest action
/// (20). Below this a row would be clipped, and the panels underneath are
/// more use than a truncated table. Same rule the message overlay follows.
const MIN_W: u16 = 35;
/// Past this a two-column table is mostly whitespace — every row is at most
/// 31 columns and the rest is trailing space.
const MAX_W: u16 = 60;
/// Two borders, a heading, and enough commands to be worth opening.
const MIN_H: u16 = 8;
```

`MIN_W` is **derived from the catalogue, not chosen**, and that makes it a
constraint running the other way: no action may exceed 20 characters and no
keys label 9, or the narrowest palette clips a row. §9 pins it with a test
rather than leaving it as arithmetic in this document. It is what §6's wordings
are cut to, and it is why `⏎`'s row says "fold/unfold, or read" rather than
spelling out both halves.

The consequence worth stating: at `MIN_W` = 35 and a width of half the frame,
**no palette is drawn below 70 columns.** Nothing between 40 (the shell's own
floor) and 70 gets one, and `?` is inert there.

The width cap is a deliberate departure from "50%". On a 200-column terminal
half the frame is a hundred columns holding a `shift-tab` and two words; the
table stops being a table and becomes two distant columns the eye has to track
between. 60 is where that stops paying, and it is the cap the message overlay
already uses.

The height cap is the same argument vertically, and it is why the palette
usually does not scroll: `min(70% of the frame, rows + 2)` means a terminal
tall enough to show the whole catalogue gets a box that ends where the catalogue
does, not one with twenty blank rows under the last command. At 24 rows — 16 for
the palette, 14 of them content, against a catalogue in the low twenties — it
scrolls, and the scrollbar appears.

If the computed width is under `MIN_W` or the computed height under `MIN_H`, the
palette does not fit, and §5.1's gate is what that turns into.

## 5.1 Geometry, and the gate that stops an invisible mode

`Geometry` gains a fifth number:

```rust
pub const fn new(heights: [usize; 4], ddl_lines: usize, command_rows: usize) -> Self;

/// Rows a palette would show at this frame size, whether or not one is open.
pub const fn command_rows(self) -> usize;
```

"Whether or not one is open" is the important half. The renderer computes it
from `frame.area()` every frame, so it is a function of the terminal size alone
— architecture.md §3's invariant, that `Geometry` never depends on the offsets
it bounds, holds, and the measure/redraw loop still terminates.

It does three jobs:

1. **Paging.** `pgdn`/`pgup` need a page height.
2. **Clamping.** Nothing here, today: the catalogue is static, so no motion can
   be left pointing past the end. It matters the moment the catalogue becomes
   dynamic, and costs nothing to have.
3. **The gate.** `command_rows() == 0` means the palette does not fit this
   frame, and `?` is a no-op.

The gate is the reason this number exists rather than being derived in the
renderer. Without it the palette opens in a frame too small to draw it, and the
user is in a mode with no pixel on screen saying so, wondering why `j` stopped
moving the tree. `Shell::TooSmall` already returns `Geometry::default()`, whose
`command_rows` is zero, so the too-small screen gets the right behaviour for
free — at open time.

The gate has to hold a second time, or a resize reopens the hole it closed.
`Msg::Measured` stores every new `Geometry` regardless of what is on screen, so
a palette opened at a generous size and left open survives a shrink on its own:
`command_rows` goes to zero, nothing draws it, and `handle_key` is still routing
every key to the palette because `state.overlay` never heard about the resize.
`Msg::Measured` closes an open `Overlay::Commands` the moment `command_rows()`
reads zero, marking the frame dirty exactly when that closing is itself a
visible change — the same rule the `ddl` clamp immediately above it already
follows, and for the same reason: the measure/redraw loop has to terminate.

## 6. The catalogue

Three groups. Everything doc/shortcuts.md §1 lists, plus `?`.

| Group | Keys | Action |
|---|---|---|
| Global | `q` | quit |
| | `?` | commands |
| | `esc` | dismiss the overlay |
| | `tab` | next panel |
| | `shift-tab` | previous panel |
| | `t` | focus Tables |
| | `a` | focus Table DDL |
| | `c` | focus Table contents |
| | `u` | focus Queries |
| Motion — focused panel | `j`/`↓` | down one row |
| | `k`/`↑` | up one row |
| | `pgdn` | down one screen |
| | `pgup` | up one screen |
| | `home` | to the start |
| | `end` | to the end |
| Tables | `⏎`/`space` | fold/unfold, or read |
| | `l`/`→` | unfold the keyspace |
| | `h`/`←` | fold, or step out |

Every action is at most 20 characters, because §5 derives `MIN_W` from the
longest one and a longer action would either clip or push the palette out of
reach of a 70-column terminal.

`⏎`'s action still names both meanings, which the bar's "open" could not afford
to (hint bar design §1) — a column buys enough room for "fold/unfold, or read"
even though it does not buy enough for a sentence. The group heading supplies
the noun the action drops: under `Tables`, "read" can only mean a table.

**There is no fourth group for the palette's own bindings.** `esc`, `j`, `k` and
the paging keys are already in the table above, doing the same thing they do
everywhere, and the bar names them while the palette is up (§7). A group that
restated them would be the palette's only self-referential content and would
imply the others are somehow different.

## 7. Bindings, and the mode

`?` is added to `handle_key` beside `q`, above the panel-focus arm. It is not a
letter, so it cannot collide with a focus key, but match order is precedence
(doc/shortcuts.md §2) and there is no reason to place it where a future arm
could shadow it.

When the palette is open, `handle_key` routes early — after the ctrl/alt guard,
before every other arm:

| Key | While the palette is open |
|---|---|
| `?`, `esc` | close it |
| `j` `k` `↓` `↑` `pgdn` `pgup` `home` `end` | move the palette's cursor |
| `q` | quit, and cancel everything in flight — unchanged |
| `⏎`, `space` | inert. Reserved for running the selected command (§1) |
| anything else | inert |

Two of these are decisions rather than defaults.

**`q` still quits.** The alternative — `q` closes the palette, as it does in
several TUIs — was rejected because `q` quitting is the one binding a user
learns first and must be able to trust without checking what is on screen. A key
that means "quit" everywhere except in one overlay is the modal ambiguity this
is otherwise being careful about. The cost is real: someone who expects `q` to
dismiss a popup will quit the program. `esc` and `?` are both on the bar while
the palette is up, which is the mitigation.

**Every other key is inert rather than dismissing.** "Any key closes it" feels
forgiving and was rejected for the same reason: `t` meaning "close the palette"
one moment and "focus Tables" the next is precisely the thing that makes a
binding untrustworthy.

This **breaks doc/shortcuts.md §2's "No modes" rule**, which §2 anticipates:
"There is no state in which a letter means something other than its binding.
This holds only until the Queries panel takes text input, and that is the design
that has to break it." The palette breaks it first and more narrowly. The rule
becomes: no letter changes meaning, and the only state that captures keys is an
overlay the user opened deliberately and can close with either of two keys named
on the bar. Text entry still gets its own design; nothing here decides it.

One dispatch point survives intact. `handle_key` is still the only place a key
becomes behaviour — the mode is one early return inside it, not a second key
table.

### On the bar

doc/shortcuts.md §2 requires a binding to be visible, so `?` goes on the bar:

```rust
pub(crate) const GLOBAL: &[Binding] = &[
    Binding { keys: &[Key::Tab],       action: "panel" },
    Binding { keys: &[Key::Char('?')], action: "commands" },
    Binding { keys: &[Key::Char('q')], action: "quit" },
];
```

Between `tab panel` and `q quit`. The bar drops global hints from the left when
the frame narrows (hint bar design §6), so position is a priority, read from the
right: `q quit` is last to go because a user who can read one hint needs the way
out, `? commands` goes just before it because it is now the way to everything
else, and `tab panel` — which teaches one binding rather than all of them — is
the first to go.

**While the palette is open the bar describes the palette**, not the panel
underneath it:

```
 j/k scroll  pgdn/pgup page  home/end ends │ ?/esc close  q quit
```

`HintBar` already has the two halves this needs — a panel group and a global
one — so this is a substitution at the top of `hints::bar`, not a new shape.

This is the one state in which `Esc` is named on the bar. Hint bar design §3
kept it off deliberately, because `Esc` does something only when an overlay is
up and "naming it always would be a lie in most frames". With the palette open
it is never a lie, and `?`/`Esc` together — both close it — are the mitigation
§7 owes the user for `q` still quitting: naming only `Esc` would leave `?`
looking like a one-way door once the palette is open.

## 8. Drawing it

A new `lc-inbound/src/palette.rs`, beside `overlay.rs` rather than inside it:
the two share a centring idea and nothing else — one wraps a sentence to a
width, the other lays out a table and tracks a selection.

`Clear` over the rect, then a bordered block titled `Commands`, then the rows.
Rows are drawn with `List` and a `ListState` seated on `selected`, exactly as
the Tables tree is — the widget owns the arithmetic that keeps the selection on
screen, and a second copy of it here would be a second thing to keep right
(the comment `body::tree` already carries).

Two columns, with the keys gutter sized to the widest `keys` label in the
catalogue. That is a width question, so it is the renderer's, for the same
reason column sizing and wrapping are.

Styling spends no new tokens:

| Element | Token |
|---|---|
| A group heading | `Muted` |
| The cursor's row | `Selected` |
| A `keys` cell | `HintKey` |
| An `action` cell | `HintAction` |

`HintKey` and `HintAction` already mean "a key, and what it does" (style.rs).
The palette says the same thing in a column that the bar says in a row, so it
should read the same; a `CommandKey` token would be a second name for one
meaning, which architecture.md §6 exists to prevent.

The scrollbar is the existing helper in `render.rs`, over the block's right
border and only when the catalogue overflows the box — so its absence says
nothing is hidden. It is private today because `render` was its only caller;
this slice raises it to `pub(crate)` rather than growing a second copy of the
`ScrollbarState` arithmetic its doc comment spends four paragraphs getting
right.

## 9. Testing

**The sweep is the load-bearing test.** It presses every key in the language
against a set of fixture states and asserts both directions:

- No key does anything unless the catalogue lists it.
- Every key the catalogue lists does something in at least one fixture.

"Does anything" is `view_model(&state)` before and after, plus the returned
effects, plus `should_quit`. Not `State` itself, which is not `PartialEq` and
whose `dirty` flag `handle_key` sets unconditionally — and the view model is the
better question anyway: a key that changes nothing visible and emits no effect
is not a binding, whatever it touched internally.

Several fixtures rather than one, because no single state exercises everything:
`esc` needs an overlay to dismiss, `t` needs focus somewhere other than Tables,
`⏎` needs a loaded schema. Three cover it — Tables focused with a schema and a
message overlay up, Table DDL focused with neither, Contents focused with rows.

The key enumeration must fail to compile when a `Key` variant is added, or the
sweep quietly stops sweeping. A `match` over a representative value, returning
the full list, gets that from exhaustiveness checking.

Beyond the sweep, in `lc-domain`:

| Guarantee | Test |
|---|---|
| `?` opens and closes it | `question_mark_opens_the_palette_and_closes_it` |
| `esc` closes it | `escape_closes_the_palette` |
| It replaces a message rather than stacking | `opening_the_palette_replaces_the_message_it_covers` |
| The cursor never lands on a heading | `the_palette_cursor_steps_over_a_heading` |
| Motion stops at both ends | `the_palette_cursor_cannot_run_off_the_catalogue` |
| Motion goes to the palette, not the panel | `an_open_palette_takes_the_motion_the_panel_would_have` |
| `q` quits with it open | `q_quits_from_inside_the_palette` |
| Other keys are inert | `an_open_palette_swallows_the_keys_it_does_not_use` |
| A frame too small leaves `?` inert | `the_palette_does_not_open_where_it_cannot_be_drawn` |
| The bar names the palette's own keys | `the_bar_names_what_the_palette_responds_to` |
| No row outgrows the narrowest palette | `no_command_row_is_wider_than_the_narrowest_palette` |

In `lc-inbound`, against `TestBackend`:

| Guarantee | Test |
|---|---|
| Centred at half the width and under 70% of the height | `the_palette_is_centred` |
| Width is capped rather than tracking a wide terminal | `a_wide_frame_does_not_get_a_wide_palette` |
| Height stops at the content | `a_tall_frame_does_not_get_a_palette_full_of_blank_rows` |
| Headings and rows are drawn | `the_palette_lists_the_catalogue` |
| The cursor row is marked | `the_selected_command_is_highlighted` |
| A catalogue taller than the box gets a scrollbar | `a_palette_that_overflows_says_so` |
| Too small draws nothing | `a_frame_too_small_for_a_palette_does_not_get_one` |

## 10. Knowingly left out

**Search.** Asked to be left out, and it is the feature the name most invites.
It needs text input, which needs the design §7 defers. The row model does not
prevent it: filtering a `Vec<CommandRow>` and re-seating the cursor is all it
would take.

**Execution.** `⏎` selects nothing to run because a `Binding` is keys and a
label, not a thing that can be invoked. Making it invokable means the catalogue
entry carries what `handle_key` would have done, which is either a second
dispatch point (forbidden) or a synthesised `KeyPress` fed back through
`handle_key` (not forbidden, and probably the answer — but it is a design, not
a detail).

**Contextual ordering.** The palette shows the same three groups regardless of
which panel has focus. vision.md §4 principle 1 promises `?` gives "the full
contextual map"; this is the full map without the contextual part. Floating the
focused panel's group to the top is a small change and a real question about
whether a list that reorders itself is still learnable — the same jitter
argument the hint bar design §1 made about cursor-aware hints.

**A scrollbar that is usually absent.** Height caps at the content, so on any
terminal tall enough the palette does not scroll at all and the scrollbar never
appears. That is correct and worth stating: the scrollbar's absence means
nothing is hidden, and at 24 rows it will be present.

**Overlay stacking.** §3 replaces rather than stacks. If a second overlay kind
ever needs to be reachable from the first, that is when a stack earns its place.

## 11. Documentation

Same commit as the implementation:

- **doc/shortcuts.md §1** — `?` joins the Global table.
- **§2** — the "No modes" rule gains the carve-out §7 describes, and the
  "a binding must be visible" paragraph gains the palette as a third place a
  binding can be visible, alongside a title and the bar.
- **§3** — `?` moves into the taken set.
- **§4** — the `?` help row is deleted; it is bound.
- **§5** — the tests in §8 are added.
- **TODO.md** — §9's search and execution items.
