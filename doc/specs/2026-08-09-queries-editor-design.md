# Queries editor — design

**Date:** 2026-08-09
**Status:** implemented

The Queries panel is blank. It is the one panel with no content at all, and
vision.md §5.1 names what belongs there: "a first-class CQL editor with
schema-aware completion", the reason the tool is worth opening.

This slice builds the writable buffer and nothing else. Executing what is
typed, the session log, completion and highlighting are each their own slice
(§9), and each of them needs a buffer to exist first.

It also settles the question `doc/shortcuts.md` §4 deliberately left open:

> **Text entry.** The Queries panel is blank. When it takes CQL, every letter
> becomes contended, and the mechanism that resolves it — a panel that
> swallows keys, or a mode — gets designed against the real editor rather than
> guessed at now.

The real editor is here, so §2 answers it.

---

## 1. Why not a widget crate

The obvious move is `tui-textarea`, and it does not work.

**It does not compile against this workspace.** `tui-textarea` 0.7.0, the
latest release, requires `ratatui ^0.29` and `crossterm ^0.28`. The workspace
is on `ratatui 0.30.2` and `crossterm 0.29`. Cargo would resolve a second copy
of both into the tree, and `TextArea`'s `Widget` impl would target 0.29's
traits — so `lc-inbound`'s `Frame` could not render it at all. This is a hard
blocker, not a preference, and it holds until upstream releases for 0.30.

The two version-compatible alternatives were checked against the registry:

| Crate | ratatui req | Outcome |
|---|---|---|
| `tui-textarea` 0.7.0 | `^0.29` | **Rejected.** Does not build here; see above. |
| `edtui` 0.11.6 | `^0.30` | Rejected on §1's architectural objection below. A vim-modal editor that owns `EditorState` and consumes `crossterm` events itself. |
| `tui-input` 0.15.3 | `^0.30` | Rejected. Single-line only, which is the wrong shape for CQL. |

**The architectural objection applies to all three, and is the real reason.**
Each owns the buffer, the cursor and the key handling inside a widget living in
`lc-inbound`. That collides with three load-bearing rules at once:

- `update` in `lc-domain` is the one place state changes (architecture.md §3).
  A buffer inside a widget is application state outside the domain.
- `handle_key` is the one dispatch point, and `lc-inbound` "decides nothing"
  (shortcuts.md §2). A widget's `input()` method is a second key table.
- The fast tiers drive the domain headless (architecture.md §10). A scenario
  test could never type a query, because the text would live in a crate the
  domain cannot see.

The text also has to reach the domain regardless: the domain is what builds a
`Statement`, gates it on `Posture` (architecture.md §4), and will later match it
against `Schema` for completion. Storing it in `lc-inbound` means storing it
twice.

**And the leverage is lowest exactly where it would be wanted later.** Syntax
highlighting through any of these crates means handing the widget pre-styled
`Line`s — which is the `Token` → `Style` mapping of architecture.md §6, ours
either way. None of them provides an autocomplete popup at all.

So the buffer is written here. It is small: §2 is the whole data structure, and
what would have been reused is the part we would have had to write anyway.

## 2. The buffer

New module, `crates/lc-domain/src/editor.rs`.

```rust
pub struct Editor {
    lines: Vec<String>,
    cursor: Position,
    desired_column: usize,
}

pub struct Position {
    pub line: usize,
    pub column: usize,
}
```

Two invariants, held by making the fields private and re-establishing them in
every mutator — the way `Cursor` and `Scroll` already work in `state.rs`, so a
renderer is never handed one to defend against (conventions.md §1):

- **`lines` is never empty.** An empty buffer is one empty line, so there is no
  "no current line" state for a cursor to point at.
- **`cursor` is always in range.** `cursor.line < lines.len()` and
  `cursor.column <= lines[cursor.line].chars().count()`.

**`column` counts `char`s.** Not bytes, which would add a
char-boundary invariant to maintain across every edit. Not display width, which
is the renderer's unit — `unicode-width` is deliberately an `lc-inbound`
dependency and not a domain one, and §5 is where widths belong. Statements are
short enough that the char-index-to-byte-index walk per edit is not worth
avoiding.

**`desired_column` is the sticky column for vertical motion.** Moving down
through a short line and back up restores the column rather than truncating it.
Every edit and every horizontal motion resets it to `cursor.column`; the
vertical motions — `up`, `down`, `page_up`, `page_down` — read it and leave it
alone. Without it, one pass over a short line silently loses the column, which
is the kind of papercut that reads as a broken editor.

The operations:

| | |
|---|---|
| Edit | `insert(char)` · `insert_newline()` · `backspace()` · `delete()` |
| Motion | `left` · `right` · `up` · `down` · `line_start` · `line_end` · `page_up(height)` · `page_down(height)` |
| Read | `lines()` · `cursor()` · `text()` |

`text()` joins with `\n` and is what a later slice hands to `Statement`.

`backspace` at column 0 joins the line to the one above; `delete` at
end-of-line pulls the next line up. Both are no-ops at the respective ends of
the buffer rather than errors.

`Position` is declared here and re-exported through `crate::view` alongside
`PanelBody`, so a renderer takes one import for the body and the cursor in it.

**In `State` it is `queries: Editor`**, alongside `tables` and `contents`. Not
a `QueriesPanel` wrapper: the panel *is* the editor until the session log
arrives, and a struct with one field would be a claim the model does not
honour. When the log lands, that is when the wrapper earns its name —
architecture.md §11's open question about `State` gets a third data point
either way.

## 3. Focus is the mode

`handle_key` stays the one dispatch point. It gains one early route, placed
after the existing ctrl/alt guard and before the existing match:

```
ctrl/alt guard  →  if focus.takes_text(), offer the key to the editor  →  existing match
```

The editor answers with whether it consumed the key. What it does not consume
falls through unchanged.

The predicate is `Panel::takes_text`, not `focus == Panel::Queries`, so the
route and the hint bar (§6) read the same one exhaustive match rather than two
conditions that can drift.

| | Keys | Behaviour |
|---|---|---|
| **Editor claims** | `Char(c)` · `Backspace` · `Delete` · `Enter` · `←` `→` `↑` `↓` · `Home` `End` · `PageUp` `PageDown` | insert, delete, newline, move the cursor |
| **Falls through** | `Tab` · `Shift-Tab` · `Esc` · `Insert` · `F1`–`F12` | their existing global meaning |

Three consequences, all deliberate:

**`Esc` gains a second meaning.** With an overlay up it dismisses the overlay,
unchanged. Otherwise, with Queries focused, it moves focus to `Tables`.
`Panel::default()` rather than a remembered previous panel: deterministic, no
field in `State` whose only job is remembering, and no undefined answer for
the frame where Queries was the startup focus. One keystroke restores `q`,
`t`, `j` and the rest.

**`q` does not quit while the editor has focus.** It types a `q`. This is the
breakage shortcuts.md §2 anticipated, and §6 makes it visible rather than
leaving it to be discovered.

**`Home` and `End` change meaning, and this one needs arguing.** Everywhere
else they reach the ends of the *document*; in the editor they reach the ends
of the *line*. shortcuts.md §2 says the same key means the same thing in every
panel, so this is a divergence and not an accident. It is taken because
line-start and line-end is what `Home` and `End` do in every text editor a user
has ever used, and an editor that sent the cursor to the far end of the buffer
instead would be the surprising one. The document ends stay reachable through
`PageUp` and `PageDown`, which keep their meaning. The alternative — reserving
document-ends for `ctrl-Home`/`ctrl-End` — needs the ctrl layer, which this
slice declines to design.

**Shift and ctrl need no new handling.** A shifted letter already arrives as a
distinct `Key::Char('T')`, so capitals type themselves. The ctrl/alt guard runs
*first*, so `ctrl-a` is still refused outright rather than inserting an `a` —
`a_modified_key_is_not_a_binding` keeps holding, and the ctrl layer stays
undesigned and unclaimed (shortcuts.md §2).

`PageUp`/`PageDown` take the panel height from
`state.geometry.height(Panel::Queries)`, which is already measured. No new
message and no new geometry.

### Why not an explicit insert mode

`i` to enter and `Esc` to leave is the vim-shaped answer, and vision.md §8
notes part of the audience expects it. It was rejected because it contradicts
vision.md §4 principle 2 — the same key at every level of skill. Under insert
mode, `t` means two different things depending on state that is invisible
unless a cue is invented for it; the novice reading the hint bar and the expert
pressing from memory can both be wrong about which regime they are in. Under
focus-as-mode the regime is the focused border, which is already on screen and
already means something.

A ctrl layer keeping `ctrl-q` and `ctrl-t` alive inside the editor was rejected
for scope: shortcuts.md §2 requires the ctrl layer to be designed as a layer
rather than smuggled in one key at a time, and pulling that design into this
slice would also mean reworking the ctrl guard and rewriting the test that
pins it.

## 4. The ViewModel

One new variant:

```rust
PanelBody::Editor {
    lines: Vec<String>,
    cursor: Position,
}
```

`view::body()` returns it for `Panel::Queries` in place of `PanelBody::Blank`.

**Nothing else in the domain-to-renderer contract changes.** No new `Msg`, no
change to `Geometry` or `Msg::Measured`. Queries' height is already reported:
`render` falls back to `inner.height` for a body with no `Extent`, and once the
editor returns one, `extent.visible` is that same number. The one edge that
runs backwards (architecture.md §3) stays exactly as wide as it is.

This is only true because of §5's no-wrap decision. A wrapping editor would
need the renderer to report a wrapped-line count, the way Ddl already does with
`ddl_lines`.

## 5. Drawing it

In `lc-inbound/src/body.rs`, whose module doc states the rule this follows:
"Scrolling lives here rather than in the domain… so no scroll state is stored
anywhere."

**One buffer line is always one screen row.** Long lines are not wrapped; the
viewport scrolls horizontally instead. Two reasons. It is what code editors do,
and it keeps the domain counting its own lines — Ddl has to ask the renderer
for `ddl_lines` because wrapping needs a width, and conventions favour the
domain counting rows itself where it can. More concretely, the panel is one
content row tall at 80×24 (§7), where a wrapped line would push the cursor's
own row off screen.

Both offsets are derived per frame from the cursor and the area, and stored
nowhere:

- **Vertical** — the smallest offset keeping `cursor.line` within
  `[offset, offset + height)`.
- **Horizontal** — the same, measured in display columns via `unicode-width`.
  This is the conversion §2 pushed out of the domain: the domain says "column
  7 in chars", the renderer works out how many cells that is.

The visible slice is drawn as a `Text` with no `Wrap`. The function returns an
`Extent`, so the existing scrollbar appears when the buffer outgrows the panel
without any new code.

**The cursor is the real terminal cursor**, set with
`frame.set_cursor_position` and only when Queries has focus. `ratatui-core`
0.1.2's `Terminal::draw` shows or hides the terminal cursor based on exactly
that call, so `lc-app` needs no change and an unfocused editor leaves no stray
cursor behind. `body::render` gains a `focused: bool`; `render.rs` already
computes `panel.panel == view.focus` one line above the call site.

## 6. Making it visible

vision.md §4 principle 1: a binding that is in neither a title nor the bar is a
binding only `shortcuts.md` knows about.

`Panel::Queries` gains `⏎ newline` · `esc leave`. Cursor motion by arrow is
self-evident and the bar names a panel's principal bindings rather than its
full surface (hint-bar design §6).

Two knock-ons the bar cannot avoid:

**`GLOBAL`'s `q quit` becomes false in Queries.** The hint-bar design §2 is
explicit that "a bar that lies is worse than no bar", so `bar(focus)` drops it
when the focused panel takes text. That needs `Panel::takes_text(self) -> bool`
— an exhaustive match, the same guarantee `shortcut()` and `hints()` give, so a
fifth panel cannot compile without answering for itself. `tab panel` stays: it
is still true, and it is the way out.

**`HintGroup::Idle` becomes unconstructible and is deleted.** It exists solely
for Queries — hint-bar design §3: "The panel has no bindings and pretending
otherwise…". With all four panels bound, no input can produce it, and TODO.md
already names this shape as a defect for `CqlValue::Timestamp`: an arm nothing
constructs is a claim the model does not honour. `HintBar.panel` becomes a
plain `Vec<Hint>`. Six code sites and an amendment note on the hint-bar design.

## 7. The panel is one row tall, and stays that way

Measured, not estimated — `layout` was run at real sizes:

| Terminal | Queries panel | Content rows |
|---|---|---|
| 80×24 | 3 | 1 |
| 100×30 | 3 | 1 |
| 120×40 | 3 | 1 |
| 160×50 | 4 | 2 |
| 200×80 | 7 | 5 |

`QUERIES_PERCENT` is 10 and never beats the 3-row `PANEL_MIN_H` floor until the
terminal is about forty rows tall. So on every common terminal the editor shows
one line at a time, scrolling vertically to keep the cursor visible.

**This slice does not change it.** The panel is to become dynamic — sized by
whether it is empty, holds a short query or a long one, and whether it has
focus — and that is its own slice (§9). Raising the floor or the percentage now
would be work thrown away, and it would take rows from Contents on the basis of
a rule about to be replaced.

The cost is honest and worth stating: multiline CQL is cramped until that slice
lands. Horizontal scrolling (§5) is what keeps it usable in the meantime, since
the cursor's own line is always the one on screen.

## 8. Testing

All fast tier. Nothing here touches a clock, a socket or a filesystem
(conventions.md §4).

**`editor.rs` units** — the bulk, and pure:

- insert at start, middle and end of a line
- `Enter` splits a line mid-word, and at both of its ends
- `Backspace` at column 0 joins to the previous line; at buffer start it is a
  no-op
- `Delete` at end-of-line pulls the next line up; at buffer end it is a no-op
- motion clamps at all four edges of the buffer
- `desired_column` survives a pass over a short line
- a `é` and a CJK character insert and delete as single characters
- `text()` round-trips a multi-line buffer

**`update.rs`** — the §3 contract:

| Guarantee | Test |
|---|---|
| Letters are text, not shortcuts | `a_letter_typed_in_queries_is_text_not_a_shortcut` |
| Quitting is not reachable from the editor | `q_does_not_quit_while_the_editor_has_focus` |
| `Esc` prefers the overlay | `escape_dismisses_the_overlay_before_it_leaves_the_editor` |
| `Esc` is the way out | `escape_leaves_the_editor_for_tables` |
| `Tab` still works | `tab_still_leaves_the_editor` |
| Ctrl is still refused | `a_modified_key_is_still_not_text` |
| `Home`/`End` reach the line's ends, not the buffer's | `home_and_end_reach_the_ends_of_the_line_in_the_editor` |
| Other panels are unaffected | `typing_outside_queries_still_focuses_a_panel` |

**Render** — against `TestBackend`, which records the cursor:

- the buffer draws its lines
- `backend.cursor_position()` lands on the cell the cursor names
- no cursor is set when Queries is unfocused
- a line longer than the panel scrolls horizontally, cursor still on screen
- a buffer taller than the panel scrolls vertically and grows a scrollbar

## 9. Knowingly left out

Each of these is a slice, not an oversight.

- **Executing the buffer.** No `Effect::Execute`, no `Statement` built. It
  brings its own questions: where a result lands when `ContentsPanel` models
  exactly one outstanding read keyed to a `TableRef`, how posture gates it, and
  what cancels the table read already in flight.
- **The session log.** architecture.md §4 calls this panel "statement entry and
  session log". Only the first half is built.
- **Dynamic panel height** by content and focus — §7. The next slice on this
  panel.
- **Autocomplete and syntax highlighting.** vision.md §5.1 and §7 Phase 1. The
  `Token` path of architecture.md §6 is already the right shape for the second.
- **Undo and redo**, selection, clipboard, word motions, bracket matching.
- **A placeholder in an empty buffer.** The panel simply draws nothing.

## 10. Documentation

Changed in the same commit as the code:

- **`doc/shortcuts.md`** — §1 gains a Queries section; §2's "No modes",
  "Bindings are global; panels refuse" and "The same key means the same thing
  in every panel" are rewritten rather than appended to, because all three are
  false as written once §3 lands; §4's "Text entry" row leaves the
  deliberately-unbound table; §5 gains the rows from §8.
- **`doc/architecture.md`** — §11's open question on `State`, which tracks how
  many panels have taken a module of their own.
- **`doc/specs/2026-08-09-hint-bar-design.md`** — an amendment note for the
  `HintGroup::Idle` removal (§6), so the design is not left describing a
  variant that no longer exists.
- **`TODO.md`** — the deferrals in §9.
