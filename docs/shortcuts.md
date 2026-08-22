# Shortcuts

**Status:** current. This documents what is bound today and the rules a new
binding has to satisfy — not a wishlist.

Every keystroke in LazyCassandra passes through one function: `handle_key` in
[`crates/lc-domain/src/update.rs`](../crates/lc-domain/src/update.rs). There is
no second key table, no per-panel handler, and no binding in the renderer. If a
key does something, it does it there.

---

## 1. What is bound

### Global

Reachable from every panel that does not take text, in every connection
state. `Esc`, `Tab` and `Shift-Tab` are the exceptions that really are
unconditional; the ten marked rows are text instead, on the one tab that
takes text — Queries' Cql tab, argued in that subsection below. Its Logs tab
takes no text at all, so most of the marked rows reach it exactly as
written; the footnote below says which few still do not, and why. `ctrl-t`
is unmarked because a chord is not text anywhere: the ctrl layer is
dispatched above the panel that takes text (§2).

| Key | Action |
|---|---|
| `q`¹ | Quit. Cancels every in-flight query. |
| `?`¹ | Open the command palette: every binding, in one scrollable block. |
| `ctrl-t` | Open the theme picker: every theme, previewed as you move. |
| `/`¹ | Search the focused panel. |
| `n`¹ | Move to the next match of the focused panel's search, wrapping. |
| `N`¹ | Move to the previous match of the focused panel's search, wrapping. |
| `y`¹ ² | Yank the selection to the clipboard, as one line — in Table contents, the cell, the row or the column, per the mode. Flashes what it copied, then drops the selection and returns the cursor to where `v` was pressed. |
| `v`¹ | Start or freeze a selection of whole rows in the focused list panel. On a row already selected, drops the group holding it; on any other row, adds a group beside what is held. |
| `Esc` | Dismiss the overlay, leave visual mode, clear a range, clear the focused panel's search, or leave the editor for Tables — first hit wins (§2). |
| `Tab`³ | Focus the next `(panel, tab)` stop, wrapping: Tables; DDL, and its Columns tab where one is drawn; Table contents; Queries' Cql tab; Queries' Logs tab. This is how the Logs tab is reached. |
| `Shift-Tab` | The same stops, walked backwards. |
| `1`¹ | Focus Tables. |
| `2`¹ | Focus DDL. Pressed again, its next tab. |
| `2`¹ | Focus Table columns — the same key, one tab further in. |
| `3`¹ | Focus Table contents. |
| `4`¹ | Focus Queries. |
| `]`¹ | The focused panel's next tab. |
| `[`¹ | Its previous tab. |

¹ Not reachable from the Cql tab: a panel that takes text claims every
printable key before this table is consulted, so these type themselves
there instead. The Logs tab is not that panel — it takes no text, so `q`,
`?`, `/`, `n`, `N`, `y`, `v` and `1`–`3` all reach it exactly as this table
says. `4`, `]` and `[` are the three exceptions even there, inert on the
Logs tab too, but not as text: `4` has no strip left to walk once the tab
cycle belongs to `Tab`/`Shift-Tab` alone (see the paragraph below), and
`]`/`[` walk nothing for the same reason.

² "One CSV line" holds for DDL's two tabs, and for two of Table contents'
three selection modes. Tables is the exception it always was, with a bare
name instead; Table contents' cell mode is a second exception, conditional
on the mode rather than fixed to the panel:

| Panel | What `y` puts on the clipboard |
|---|---|
| Table contents, cell mode | The cell's value, bare and unescaped |
| Table contents, row mode | Every column of the row, as one CSV line |
| Table contents, column mode | Every row's value in the column, as one CSV line, no heading |
| DDL, DDL tab | The row's fields for its kind, as one CSV line |
| DDL, Columns tab | `name,type,kind`, as one CSV line |
| Tables | The node's bare name — `shop`, `shop.orders` — not a CSV line |
| Any panel, a selection held | Every selected row, in the order the rows are drawn — every column in column mode, every row of the rectangle in cell mode — one line each, joined with `\n`, no trailing newline and nothing between the groups. A single cell in cell mode still yanks bare. |

See [yank design](specs/2026-08-13-yank-design.md) §2 and
[yank selection modes](specs/2026-08-13-yank-selection-modes-design.md) §1.

³ Except in Queries with a completion popup up, where it accepts the selected
candidate — as `⏎` does there too. The Queries subsection below is where that
is argued.

Each panel's title opens with the key that reaches it — `[1]-Tables`,
`[2]-Table DDL - Table columns`, `[3]-Table contents`, `[4]-Queries`
(`[4]-Queries - Logs` on its second tab) — so the
binding is learned by looking at the panel rather than by reading this file
(vision.md §4, principle 1). The number is drawn in the same style as the
panel's active tab: it names the panel, and so does the tab.
See [panel numbers design](specs/2026-08-15-panel-numbers-design.md).

Moving focus — by `Tab`, `Shift-Tab`, a number, `Enter` on a table or a
click — drops the selection of the panel being left, so only the focused panel
ever holds one: a selection says what the next command acts on, and the next
command acts on what has focus
([multi-group selection design](specs/2026-08-14-multi-group-selection-design.md) §4).

`1` `2` `3` `4` run down the frame in the order the panels are drawn —
top-left, bottom-left, top-right, bottom-right — and `Tab` walks the same
order, now stopping at every tab a panel draws along the way: DDL's own tab
and, where a table is selected, its Columns tab beside it; then Queries' Cql
tab and its Logs tab. One order on screen, in the titles and under the
fingers. Four letters chosen for the words they sit inside used to do this
job; a letter has to be learned per panel, while four digits in frame order
are one rule, and a novice reading `[2]-` has been told the whole binding.

A number focuses its panel. Pressed again — that is, pressed while its panel
already has focus — it steps to the panel's next tab, wrapping, but only on
the DDL panel: `2` `2` reaches the Columns tab and a third `2` comes back;
`1` `1` stays on Tables. Queries has two tabs now as well, Cql and Logs, and
`4` does not walk them the way `2` walks DDL's — a genuine asymmetry, not an
oversight. The reason is the panel itself: the Cql tab takes text, so a `4`
typed there is a character like any other digit rather than a second press
left over for `handle_key` to reinterpret, and arriving at Queries by its
number always lands on Cql, because leaving the panel by any other route has
already reverted it there (see the Logs tab subsection below). The trail is
reached a different way instead — `Tab`/`Shift-Tab` walking every
`(panel, tab)` stop a strip can draw, DDL's two and Queries' two alike (see
the table above) — so the Logs tab costs no digit of its own. The palette
says so directly for DDL: `2` takes two rows, one per tab, one under the
other. The general rule stays with `]`/`[`, but a reader who has not met
those keys yet learns from the repetition that the number pressed again goes
somewhere — which is how the tabs are found without reading this file
(vision.md §4, principle 1). `]` and `[` do the same walk forwards and
backwards from the focused panel, for a hand that learnt them in lazygit; on
a panel with one tab they do nothing, and so do they anywhere in Queries —
text on the Cql tab, and simply unanswered on the Logs tab, since the walk
answers to DDL alone, the same reason a repeated `4` goes nowhere there.

Arriving at the tab already drawn costs nothing: `2` from another panel is how
focus comes back to whichever DDL tab was up, and it leaves the cursor on the
row you were reading. Changing tab does move the cursor and the viewport to the
top — the tabs describe different rows, so a row index carried across addresses
nothing the user chose.

On a keyspace the DDL strip draws one tab, so walking stays on it. A press that
stored a tab nothing draws would put the panel somewhere the user cannot see it
is. The tab the panel was left on is kept, not reset: stepping onto a keyspace
and back is navigation, and it may not cost you the tab you were reading.

### The command palette

`?` opens it over any panel. It outranks focus and claims every unmodified key
before the panel underneath is consulted (§2).

| Key | Action |
|---|---|
| `j` `k` `↑` `↓` | Move the cursor, command to command. Headings and rules are stepped over. |
| `PageDown` `PageUp` `ctrl-d` `ctrl-u` | Page the list. |
| `Home` `gg`, `End` `G` | The first command, the last command. |
| `⏎` | Close the palette and perform the selected command. |
| `?` `Esc` | Close it. |
| `q` | Quits, as everywhere else. |
| `/` | Search the list: typing walks the cursor to the next matching row, marking the hit. `⏎` keeps the row, `Esc` puts it back. |
| anything else | Inert. Nothing means "close" in one frame and something else in the next. |

`⏎` presses the row's own first key, in the panel the row belongs to — it
focuses that panel first, whether or not the action that follows has anything
to do. A row that means something only inside the export form is not pressed
at all: it is drawn faint, and `⏎` on it closes the palette and stops
([palette execution design](superpowers/specs/2026-08-15-palette-execution-design.md) §8).

`/` searches the action text alone — never the key column, never a group
heading — and the cursor it walks only ever lands on a command, the same
skip-the-heading rule `j`/`k` and paging already keep. A term matching
nothing leaves the cursor where the prompt opened. The catalogue this table
draws gained no row of its own for it: a row saying `/` searches the palette
would be a row about the palette inside the palette, so the hint bar names
the key instead — a mode names its own keys
([overlay search design](superpowers/specs/2026-08-16-overlay-search-design.md) §6, §8).

### The search prompt

`/` opens it over Tables, DDL or Table contents — the only panels a
search can apply to. It is drawn on the bar row, not over the panels, but it
is an `Overlay` exactly like the command palette: it outranks focus and
claims every printable key before the panel underneath ever sees one, `q`
and `?` included — see §2's exceptions.

| Key | Effect |
|---|---|
| any printable key | Appends to the query, applies it live, and jumps the cursor to the first match, searching from where `/` was pressed. |
| `Backspace` | Removes the last character, then applies and jumps the same way. |
| `Enter` | Closes the prompt, leaving whatever search is applied and the cursor wherever it landed. |
| `Esc` | Restores the search and the cursor to where they stood before the prompt opened, then closes. |
| anything the ctrl layer does not claim | Inert, including `Tab`/`Shift-Tab` — the panel being searched cannot change under a live query. |

Ctrl is dispatched above every overlay (§2), so a chord reaches `ctrl_key`
before the prompt is consulted — and every member is inert here anyway, each
for its own reason. `ctrl-d`/`ctrl-u` page nothing because `PageDown`/`PageUp`
page nothing here, and the two spellings reach one implementation. `ctrl-t` is
refused outright: it is the one opener a prompt cannot swallow, and a picker
opened over a live query would strand the panel searched with nothing left
holding what it was searched from. `ctrl-r` runs the buffer only from Queries,
and no prompt can be open while Queries has focus. That takes two halves:
Queries holds no search, so no prompt can be opened *over* it; and focus cannot
move *to* it under a live prompt, since the prompt swallows `Tab`/`Shift-Tab`,
types the panel numbers as text rather than acting on them, and a click is
spent dismissing the prompt (§7).

A chord the layer has no member for is still refused rather than typed: ctrl
is a layer of its own, and `ctrl-n` is not an `n`.

Searching is live: every edit re-applies the search, so the panel underneath
is the preview — Tables and Table contents highlight where a match sits
rather than narrowing to it, and so does Table DDL; neither panel drops a
row for a term that fails to match it. `Enter` on an empty query therefore
clears the search — there is no separate binding for that, only the empty
query the invariant already turns into "no search". The jump is measured
from `origin`, the cursor position the prompt opened on, rather than from
wherever the last keystroke landed: searching from the current cursor would
walk the view forward as the term is extended, which is not what an
incremental search means. No match leaves the cursor at `origin`, whether
that keystroke is the prompt's first or one that walks a term that used to
match into one that no longer does. `Esc` restores `origin` alongside the
search it restores, so an abandoned prompt leaves the panel exactly as it
was found.
See [panel search design](specs/2026-08-10-panel-search-design.md) §3 and
[search that highlights rather than hides](specs/2026-08-15-search-highlight-design.md) §5.2.

### The theme picker

`ctrl-t` opens `Overlay::Themes`, an `Overlay` of the same rank as the command
palette (§2): it outranks focus and claims every key before the panel
underneath ever sees one. It is refused on the picker's own gate — a frame too
small to draw its list gets no picker — which is not the palette's gate: a
config can define enough themes that the picker no longer fits where a
palette would. It is also refused over a live search prompt (see the
prompt's subsection above). The palette carries a row for it.

The list is the twelve built-in themes, followed by whatever `[[themes]]`
the config layers define — a config-defined theme is never listed before a
built-in, so the picker's first twelve rows are the same across every
project. A config file with no custom themes leaves the list exactly as it
was before config existed.

| Key | Effect |
|---|---|
| `j` / `k`, `Down` / `Up` | Moves the cursor, **and sets `State.theme`** — the frame repaints in that theme immediately. |
| `PageDown` / `PageUp`, `ctrl-d` / `ctrl-u` | The same, by a screen — which here is the whole list. |
| `Home` / `End`, `gg` / `G` | The first theme, the last theme — previewing it, as every motion here does. |
| `Enter` | Closes, keeping whatever theme is set — and writes it to the config file it is read from, so the next launch opens on it ([theme persistence design](superpowers/specs/2026-08-22-theme-persistence-design.md) §2). |
| `Esc` | Sets `State.theme` back to the theme the picker opened on, then closes. |
| `q` | Quits, as everywhere else. The picker takes motion, not text, so it leaves `q` alone — the palette's rule (§2). |
| `/` | Search the list: typing walks the cursor to the next matching row, marking the hit and previewing it. `⏎` keeps the theme, `Esc` puts it back. |

Search here previews the same way motion already does: the cursor is what
sets `State.theme`, so walking it with `/` walks the preview too. A term
matching nothing leaves the cursor, and so the preview, right where the
prompt opened — see §2's rule below for the key itself
([overlay search design](superpowers/specs/2026-08-16-overlay-search-design.md) §5, §6).

The ends are reachable here as they are in every other list. Paging does land
on either end in one press — the list is every theme there is and always
shorter than the block drawing it, so a screen *is* the whole list — and
`Home`/`End` and `gg`/`G` were inert here for exactly that reason until the
chord arrived. What changed the answer is that a hand which types `G` in the
panels and in the palette and gets nothing in the picker has found the one
list that is different, which costs more than the arm does.

Live preview is why the picker restores rather than merely reverts: a message
arriving over an open picker, or a frame shrinking below it, unwinds the
preview the same way `Esc` does, so a half-previewed theme never strands
itself. See [themes design](specs/2026-08-10-themes-design.md) §6.

The picker is behind ctrl because a shifted letter was not free to give it a
plain-key opener of its own: `T` was the one shifted letter ever bound and
has been given back, so `A`, `B`, `C`, `D` and `U` stay unbound (§2, §4). `N`
is the program's only shifted letter now, and it is spent walking a search
backwards (§1's global table), not opening the picker. `a`, `b`, `c`, `d`,
`t` and `u` are free too, since the panels moved to `1`–`4`.

### The configuration diagnostics overlay

`Overlay::Diagnostics` opens itself, once, at startup — there is no key that
opens it, because there is nothing to preview: it exists only to say what a
config file got wrong before the first frame is drawn. If neither config
file has a problem it never appears at all.

| Key | Effect |
|---|---|
| `j` / `k`, `Down` / `Up` | Moves the cursor. |
| `PageDown` / `PageUp`, `ctrl-d` / `ctrl-u` | The same, by a screen. |
| `Home` / `End`, `gg` / `G` | To the first / last diagnostic. |
| `Esc` | Closes it. |

Unlike the palette, the prompt and the theme picker, `q` does nothing here
rather than quitting: a diagnostics list is read once and dismissed, and the
one key a user reaches for on a screen full of text they did not ask to read
is `Esc`, not `q` — see [design](specs/2026-08-14-configuration-design.md) §9.

### The export form

`ctrl-e` opens `Overlay::Export` when Table contents is focused on a loaded
page with at least one row, no export is already running, and the frame is
big enough to draw the form — a form nothing draws would claim every key
with nothing on screen saying so, which is the rule `?` already follows for
the palette. A frame that shrinks past it closes it, for the same reason.
It is the
Contents half of a chord that means something else in Queries; see that
subsection below and §2's ctrl paragraph for the other half. Like the
picker and the palette it outranks focus and claims every key before the
panel underneath sees one, `q` included, which quits everywhere else and
does nothing here.

| Key | Effect |
|---|---|
| `Tab` / `Shift-Tab` | Move to the next / previous of the form's four stops, wrapping. |
| `Space` | Activates the focused stop: on a radio group, advances to the next enabled option, wrapping; on the Export stop, runs the export. |
| `Enter` | Runs the export, and only from the Export stop. On the Scope or Format radio it does nothing — a radio group is not a button, and a keystroke landing there must not write a file. |
| `j` / `k`, `Down` / `Up` | Move within a radio group — the second way to reach what `Space` also reaches there. |
| any printable key, `Backspace` / `Delete`, `←` / `→`, `Home` / `End` | Text, and only on the path stop — the panel-that-takes-text rule, applied inside an overlay (§2). `Space` types a space there rather than activating anything, and `Enter` does nothing there either. |
| `Esc` | Closes the form and writes nothing. |

`ctrl-d`/`ctrl-u` are the only ctrl chords answered while the form is up —
dispatched above the overlay guard the way they are everywhere (§1's motion
table) — and they page nothing here: the form has no list long enough to
page. Every other ctrl chord is refused while the form is up, the same
refusal the palette and the picker already get.

Failing to open the destination with `ExportError::Exists` raises a
confirmation over the form rather than a sixth overlay variant:

| Key | Effect |
|---|---|
| `Space` / `Enter` | Confirms, and re-emits the export with `overwrite: true`. |
| `Esc` | Returns to the form, every field exactly as it was. |

See [export design](specs/2026-08-14-export-design.md) §2, §7.

### Motion

Applies to the focused panel and nothing else. What moves depends on the panel:
Tables, Contents and DDL move a selection and let the viewport follow;
Queries moves the cursor through its buffer.

| Key | Action |
|---|---|
| `j` / `↓` | Down one row (in Queries, `↓` only — `j` is text; with a completion popup up, the next candidate). |
| `k` / `↑` | Up one (in Queries, `↑` only — `k` is text; with a popup up, the previous candidate). |
| `l` / `→` | Right one column in Contents; unfold the selected keyspace in Tables. |
| `h` / `←` | Left one column in Contents; fold it in Tables, stepping out from a table row first. |
| `PageDown` / `ctrl-d` | Down one panel height. |
| `PageUp` / `ctrl-u` | Up one panel height. |
| `Home`, `gg` | To the start (to the **line**'s start in Queries — §2). |
| `End`, `G` | To the end (to the **line**'s end in Queries — §2). |

`gg` and `G` are the vim spelling of `Home` and `End`, and reach the same
`Motion::Start`/`Motion::End` rather than a copy of them — the relation
`ctrl-d`/`ctrl-u` already have to `PageDown`/`PageUp` (§2). They apply to
whatever list the frame is driving: the focused panel, or the overlay in front
of it when that overlay takes motion — the command palette, the diagnostics
list and the theme picker all answer them.

`g` on its own is a prefix, not a motion: it arms the chord and moves nothing,
and the very next press spends it whatever that press is, so `g` then `j` is a
`j`. It is armed in `State.pending`, taken at the top of `handle_key` so every
path below — the ctrl layer, an overlay, a panel that takes text — disarms it,
and a pointer gesture clears it too. In Queries and under a search prompt both
keys are text, the panel-that-takes-text rule as everywhere else.

`ctrl-d`/`ctrl-u` are the vim spelling of the two paging keys and reach the
same code rather than a copy of it, overlays included: whatever `PageDown`
does in the frame you are in — page a panel, page the palette, page the
picker, page the editor, or nothing under a search prompt — `ctrl-d` does.

`End` reaches the last row everywhere it applies, DDL included — a list
has an end, not a document with room to scroll past its last line. Both are
the same `Motion::End`, resolved by the focused panel
([panel scrolling design](specs/2026-08-09-panel-scrolling-design.md)). Table
DDL is the one place paging is not exact: `PageDown`/`PageUp` move the cursor
by the panel's height counted in wrapped **lines**, while the cursor itself
counts **rows**, so a page overshoots on a run of wrapped values. `Home` and
`End` stay exact regardless — landing on index zero or the last row needs no
counting in either unit. The fix needs a line-to-row map from the renderer,
and the design accepted the imprecision rather than build one for a panel
that usually fits on one screen
([ddl selection design](specs/2026-08-10-ddl-selection-design.md) §4).
Queries never reaches `Motion::End` at all: the editor claims `Home`/`End`
itself, before the match that would call it (queries editor design §3).

The Contents grid has two axes, and only `h`/`l`/`←`/`→` reach the second one.
`Home`, `End`, `PageUp` and `PageDown` move the **row** cursor there, as they do
everywhere else; the column cursor moves one column at a time or not at all, and
§4 says why ([table display design](specs/2026-08-10-table-display-design.md) §2).
The column offset follows the cell the way the row offset follows the row, and
the panel's bottom border grows a scrollbar exactly when a column is off screen.

### Visual mode

`v` starts a selection from the cursor: press it in Tables, DDL or Table
contents and every row from there to wherever the cursor moves next is in
range, drawn as a band rather than picked one at a time. A second `v`
freezes what is extending — the range stops following the cursor, which is
then free to walk off it without losing what was marked
([visual mode design](specs/2026-08-14-visual-mode-design.md) §2).

`v` reads as one verb with three arms, decided by where the cursor is when it
is pressed: extending a group freezes it, as above; a row no group holds opens
a new group there and starts extending it, beside whatever is already frozen;
a row a group already holds drops that group. There is no way to restart a
selection from scratch with `v` alone — `Esc` first is the route, since it is
already the key that means *drop what is held*
([multi-group selection design](specs/2026-08-14-multi-group-selection-design.md) §1).

`Esc` is a chain of five rungs now rather than three, first hit wins: an
overlay dismisses; failing that, an extending range freezes exactly the way
a second `v` would, so a reflex `esc` never discards a range still growing;
failing that, the selection clears — every group at once, however many there
are, rather than one at a time; failing that, the focused panel's search
clears; failing that, Queries hands focus back to Tables. §2 restates this as
the rule it now is.

In Table contents, which axis `v` extends is `m`'s to say: cell mode grows
a rectangle on both cursors at once, row mode a run of rows, column mode a
run of columns — the same three shapes the panel already draws
([selection modes design](specs/2026-08-13-selection-modes-design.md) §2).
Pressing `m` while a selection stands clears it, groups and all, rather than
reinterpreting it on the mode's new axis: a rectangle read as a column range
would name cells nobody selected, and a key that quietly repaints what is
marked is worse than one that asks you to mark it again.

A Tables selection of more than one row — one range of several, or several
groups of one — makes `Enter` inert and leaves the DDL panel describing
nothing: reading a table needs one table, and a selection of several rows says
which rows are of interest, not which one to open. A selection of exactly one
row is Tables as it always was: `Enter` still reads it.

A selection does not outlive what it was drawn against, groups and all. The
DDL selection clears when the panel's subject or its tab changes, since a
group describes rows on a tab about a table and neither survives a change to
either. Tables' and DDL's selections also clear when their search changes — Tables
because forcing a matching keyspace open can renumber the rows a range
addressed, the same reason a cursor already gets pulled back; DDL's clears
too, though a search no longer drops or reorders a DDL row for it to
renumber. Table contents is the exception it always was: its search only
flags matching cells and never touches a row's position, so a selection
there survives a search changing. A
click clears the whole selection in whichever panel it lands in, alongside
everything else a click already resets.

`y` over a selection yanks every row it holds, across every group — every
column, in column mode — as one line each, in the order the rows are drawn,
joined with `\n` and no trailing newline, with nothing between the groups; a
single cell in cell mode still yanks bare, unescaped, exactly as it always
did. §1's `y` footnote table carries the row-by-row shape
([multi-group selection design](specs/2026-08-14-multi-group-selection-design.md) §5).

### Tables panel

Accepted anywhere, but a no-op unless Tables has focus — except in Queries,
which takes text: there `Enter` inserts a newline, or accepts a completion
while one is being offered. See the Queries subsection below.

| Key | Action |
|---|---|
| `Enter` | On a keyspace: fold/unfold. On a table: read it into Contents and focus that panel. |

`Space` used to be `Enter`'s synonym here and is now free (§3): one key for one
action, and a second name for a key everyone already knows buys nothing.

`l`/`h` and `→`/`←` are motion, not tree keys, and the Motion table above is
where they live: unfolding is what *move right* means in a tree, and the same
key means the next column in a grid — §2.

Reading a table cancels whatever read Contents was waiting on, and moves focus
to Contents immediately rather than when the page lands: focus is a consequence
of the keypress, not of the network
([table display design](specs/2026-08-10-table-display-design.md) §1). A refused
read moves it too — the panel is where the refusal is narrated. Returning is
`Tab` or `t`, and the tree selection is where you left it.

`l` and `h` on a keyspace are swallowed while a search is applied — and so
are `Enter` and a click, on a keyspace row, for the same reason. A
keyspace holding a match is forced open by the search itself, so folding it
would change nothing on screen — the projection puts it right back — and a
key that looks broken is worse than a key that does nothing (search
highlight design §4.1). This is the existing rule that a panel which cannot honour a motion
swallows it rather than passing it on (§2). `Enter` and a click still read a
table into Contents under a search; only the fold/unfold path is swallowed.

### DDL panel

Its tabs, and nothing else. `2` is a global key (§1) rather than a binding of
this panel — it is how you reach it from anywhere, so a panel that does not yet
have focus has to answer it. `[` and `]` are the exception that proves it: they
walk the *focused* panel's strip, so they mean nothing here until `2` has been
pressed. `Tab`/`Shift-Tab` reach it too, now stopping on both of its tabs on
the way through the cycle (§1) — the same two-stop treatment Queries' strip
gets, though Queries reaches its second stop by a different route (see the
Logs tab subsection below). Motion, search and `Esc` reach
it the way they reach any other panel. `ctrl-e`/`ctrl-y`, which moved the
viewport a line without the cursor, are gone.

DDL is still the one panel whose cursor and viewport count in different
units and can therefore disagree
([design](specs/2026-08-10-ddl-selection-design.md) §4.1). What is left to move
the viewport alone is the wheel (§7), which is a look somewhere else rather
than a way to reach a row; the next `j` or `k` brings the viewport back to the
cursor. `ctrl-e`/`ctrl-y` went because a viewport the cursor keeps yanking back
is not a way to read a panel from its heading down; what replaces them is a
slice of its own (§4).

`ctrl-e` lost this meaning, and did not gain a single one in its place: it
opens the external editor from Queries and the export form from Table
contents (§1's export-form subsection), each gated on its own panel exactly
as `ctrl-r`'s one meaning already is (§2). Losing the DDL half is what "one
key, one action" costs and buys here — a chord that moved the viewport
without the cursor was the last of its kind, not a chord whose meaning
depends on focus; that shape survives elsewhere in the layer.

### Table contents panel

Accepted anywhere, but a no-op unless Table contents has focus — except in
Queries, which takes text: there `m` types itself. See the Queries subsection
below.

| Key | Action |
|---|---|
| `m` | Cycle the selection mode: row → column → cell, wrapping. |
| `ctrl-e` | Open the export form. Inert on an empty grid, while an export is running, and on a frame with no room to draw the form. |

The panel opens in row mode, which lights the whole row: reading a table is
reading rows. Cell mode draws the selected cell over a faint wash across its
row and its column, and column mode lights the column and its heading. Whichever mode is up, `j`/`k`
still move the row cursor and `h`/`l` the column cursor exactly as the Motion
table above says — the mode changes what is painted, not what a key does.

### Queries panel

Two tabs, Cql and Logs. This subsection is the Cql tab, the buffer the panel
opens on; the Logs tab is read-only and has a subsection of its own,
directly below. The panel takes text only while the Cql tab is drawn, so
while it has focus every printable key is a character and not a binding here
— the Logs tab claims none of them. What it does not claim keeps its global
meaning.

| Key | Action |
|---|---|
| `ctrl-r` | Run the whole buffer. Its page replaces whatever Contents was showing. A buffer that is empty or all whitespace runs nothing, and the bar greys the hint out while that is so — §2. |
| `ctrl-e` | Open the buffer in `$VISUAL`, `$EDITOR` or `vi`, and replace it with whatever the editor saved. The program is suspended while the editor holds the terminal. |
| `ctrl-f` | Lay the buffer out: one clause to a line, keywords upper case, `AND` indented under the clause it continues. Nothing is reordered and no name, string or comment is retyped. The bar greys the hint out on a buffer already laid out, which an empty one is. |
| `ctrl-n` / `ctrl-p` | The next / previous completion. With no popup up, raise one against the word under the cursor. |
| `↓`³ / `↑`³ | The next / previous completion — **only** while a popup is up, and the same step `ctrl-n`/`ctrl-p` take. With none up, they move the cursor, as everywhere else. |
| `Tab`³ / `Enter`³ | Accept the selected completion — **only** while a popup is up. A keyword lands with the space the word after it needs, unless the cursor is already on one; a name, a type and a statement shape land as they are drawn. With no popup up, `Tab` moves focus and `⏎` splits the line, as everywhere else. |
| any printable key | Insert it, and offer completions against the word it lands in. `q`, `t`, `j` and the rest are text here. |
| `Enter` | Split the line at the cursor. |
| `Backspace` / `Delete` | Delete before / at the cursor; at a line's edge, the line break. |
| `←` `→` `↑` `↓` | Move the cursor. `←`/`→` wrap between lines, and stay motions under a popup — which is what takes it down. |
| `Home` / `End` | To the start / end of the **line**. |
| `PageUp` / `PageDown`, `ctrl-u` / `ctrl-d` | Up / down one panel height. |
| `Esc`³ | Take the completion popup down; with none up, leave the editor for Tables. |

³ The five keys the completion popup claims, and it claims them only while it
is drawn. All five are exceptions to §2's *the same key means the same thing
in every panel*, taken for the reason `Home` and `End` already diverge here: it
is what the key does in every editor the audience has met. None is invisible —
the popup is on screen with a row selected, and the bar swaps `tab panel` and
`⏎ newline` for `tab ⏎ complete` in exactly the frames the swap is true.
`Esc`'s second press still leaves the panel, so its hint stays true either way.
The arrows need no swap of their own: a list with a row selected is what a
reader already presses them in, and *down one row* is what the palette calls
them under `Navigate` — the popup is the row list this frame is driving, which
is §2's *each panel resolves a motion in its own vocabulary* rather than a
sixth meaning to advertise.

Typing raises the popup automatically, and it goes down of its own accord the
moment nothing matches. What it offers depends on what the grammar takes at
the cursor: the grammar leads where it has narrowed something, and CQL's own
words lead everywhere else.

| Position | Offered, in order |
|---|---|
| The start of a statement | keywords, statement shapes, keyspaces, the visible keyspace's tables |
| After `FROM`, `INTO`, `UPDATE`, `TRUNCATE`, `TABLE`, `ON` | the visible keyspace's tables, keyspaces, keywords |
| After `USE`, `KEYSPACE` | keyspaces, and nothing else |
| A column list over one named table | that table's columns, then keywords |
| A column list naming no table | keywords, then every column, qualified by where it lives |
| A column definition's type | CQL's type names, and nothing else |
| After `WITH` | the `WITH` options, and nothing else |
| A value, an alias, a statement the editor does not follow | keywords alone |
| After a `keyspace.` | that keyspace's tables, and nothing else |

A **statement shape** is a whole statement rather than one word of it, drawn
with its holes marked — `SELECT <> FROM <>`, `CREATE TABLE <> (<> PRIMARY
KEY)`. Accepting one types the skeleton, terminated with `;`, and leaves the
cursor in the first hole. Nine are offered, and only where a statement begins.

A keyword is accepted upper case; a name is accepted exactly as the cluster
spells it. Accepting a **qualified column** — one offered where the statement
names no table — also writes the `FROM` clause that puts it in scope, at the
end of that statement, leaving the cursor on the select list. See
[completion ranking design](specs/2026-08-17-completion-ranking-design.md).

`Tab` and `Shift-Tab` still reach here, except as ³ says, but from the Cql tab
neither leaves the panel outright: `Tab` lands on the Logs tab first, the
panel's own second stop in the cycle (§1), and only a second `Tab` from there
wraps on to Tables; `Shift-Tab` from Cql steps back to Table contents
instead, since Logs sits after Cql in the cycle, not before it. Nothing else
global survives inside the editor — including `q`, which types a `q`. `ctrl-r` is not global either:
it is the ctrl layer's Queries member and does nothing outside this panel,
and neither does `ctrl-e`, which opens the buffer in the user's editor
([external editor design](specs/2026-08-12-external-editor-design.md) §1).
See [queries editor design](specs/2026-08-09-queries-editor-design.md) and
[query editor design](specs/2026-08-16-query-editor-design.md).

### Logs tab

Queries' second tab: a read-only, scrollable trail of every statement the
session issued and what came back, one entry per connection attempt, schema
read, query and export. Reached only by `Tab`/`Shift-Tab` walking the cycle
of `(panel, tab)` stops (§1) — `4` does not walk onto it, and the paragraph
above explains why. Once drawn it takes no text at all, unlike the Cql tab
beside it: `j`/`k` and the arrows move the cursor entry to entry, `PageUp`/
`PageDown` and `ctrl-d`/`ctrl-u` page it, `Home`/`End` and `gg`/`G` reach
either end, `v` starts or freezes a selection of whole entries, `/` searches
it and `n`/`N` walk the matches, `y` yanks — the held selection if one is
active, otherwise the entry under the cursor — each entry as its two rows
joined, the statement and its outcome (query log design §5.3). `q` quits
here, unlike the Cql tab, where it types itself.

Nothing here writes, deletes or clears an entry: a trail with a delete key
would not be a trail (query log design §5.3). Leaving the panel — by `Tab`,
`Shift-Tab`, another panel's number, or a click — reverts the tab to Cql,
because the panel's unfocused height is a function of the CQL buffer it
hugs, not of the trail (query log design §5.2); returning to Logs costs
`Tab`/`Shift-Tab` walking the cycle again, or `4` then `Tab` once. The
trail's own search does not outlive the tab being drawn either — switching
to Cql clears it, since the hint bar cannot advertise `n` while `n` is
literal text there.

---

## 2. The rules

These are constraints on the binding set, not style advice. A change that
breaks one of them is a design decision and needs a spec.

**One dispatch point.** `handle_key` is the only place a key becomes behaviour.
`lc-inbound` translates crossterm events into `KeyPress`
([`inbound/input.rs`](../crates/lc-inbound/src/input.rs)) and decides nothing;
`lc-app` forwards them and decides nothing. A binding that lives anywhere else
is a bug.

**Bindings are global; panels refuse.** There is no focused-vs-global key table.
Every key is dispatched unconditionally and a panel-specific action checks
`state.focus` and returns. This keeps one flat, greppable list of everything the
program responds to, at the cost of a guard per action — the trade is deliberate
and is revisited when the CQL editor lands.

This now has four exceptions, in that order of precedence. First, a modifier:
`handle_key` returns on alt and routes ctrl into `ctrl_key`, the layer's own
key table, before any check below — so `ctrl-r` is never a plain `r`, in the
editor, in the palette, or under a search prompt (`ctrl_key` repeats the
palette guard itself, for exactly that reason; it does not repeat the prompt's,
because the prompt has no motion for a ctrl chord to be mistaken for). Then an
open command palette claims every unmodified key before anything else is
consulted, because it is drawn over the panels and the panel underneath is not
what the user is looking at. Failing that, an open search prompt claims every
unmodified key the same way: it is the same shape of thing — a state that
outranks focus, entered with `/` and left with `Enter` or `Esc` — but it claims
every printable key as text rather than as motion, `q` and `?` typing
themselves exactly as they would in a panel that takes text (panel search
design §1.1). Failing that, a panel that takes text claims the printable and
editing keys before the table is reached.
Everything none of the four claims is still dispatched globally and still
refused by panels that have nothing to do with it.

**Match order is precedence.** The panel-focus arm is last, so it can only claim
letters no earlier arm took. Adding an arm above it can silently steal a focus
key; the test that catches this is named in §5.

**One key, one action.** A second key for something a key already does is a
key spent, a row on the palette, and one more thing to keep in step. Three
kinds of duplicate survive, and only these three: the arrows and
`PageUp`/`PageDown`/`Home`/`End`, which are how a user who has never met vim
reaches a list at all (vision.md §4, principle 2); the pointer, which
necessarily repeats what the keyboard does (§7); and `ctrl-d`/`ctrl-u`, which
are the vim spelling of a paging key rather than a second implementation of
paging — they dispatch to `PageDown`/`PageUp`'s own arm, and `gg`/`G` are the
same kind of spelling for `Home`/`End` (§1's motion table). `Space` on a tree
row was a duplicate of the fourth kind and is gone.

**Shift is never consulted for letters.** `A`, `C`, `T`, `U` arrive as
distinct `Key::Char`s and stay unbound; a shifted letter is a binding of its
own, not a synonym for the plain one. `T` was an exception and has been
given back — the theme picker moved to `ctrl-t` (§1's picker subsection).
Two exceptions remain: `N`, which walks a search backwards and is not a
synonym for `n` (§1's global table; search highlight design §5.3), and `G`,
which reaches the end of a list and is not a synonym for `g` — `g` alone is a
prefix that moves nothing (§1's motion table). A
shifted arrow is left as a synonym for the plain one, and stays that way:
`v` is how a selection extends, so no arrow needs a shifted meaning of its
own. Binding one now would be a fourth kind of duplicate — the same key,
twice — and §2's own rule above names only three that survive.

**Ctrl is one designed layer; alt is refused wholesale.** `handle_key` routes a
ctrl-modified key into `ctrl_key`, which is the layer's whole key table, and
returns early on alt and on ctrl+alt together — a two-modifier chord is a third
layer, undesigned. What the rule protects is unchanged: a ctrl key is a binding
of its own and never a synonym for the plain one, so `ctrl-j` still does
nothing, and a member pressed outside its panel is inert rather than a binding
for whatever is focused. The layer has nine members now. `ctrl-r` runs the Queries
buffer, `ctrl-e` opens that buffer in an external editor, `ctrl-f` lays it
out, and `ctrl-n`/`ctrl-p` walk the completion popup: the panel takes
text, so every printable key there is a character and none of those actions has
an unmodified key available
([query execution design](specs/2026-08-10-query-execution-design.md) §1,
[external editor design](specs/2026-08-12-external-editor-design.md) §1).
`ctrl-e` carries a second meaning too, over Table contents: it opens the
export form, gated on focus exactly as `ctrl-r`'s one meaning already is —
a chord is never a synonym for the plain key, and nothing in the rule says
it may carry only one meaning of its own (export design §1). `ctrl-t` opens
the theme picker, for the reason §1's picker subsection gives. `ctrl-f`,
`ctrl-n` and `ctrl-p` are Queries members exactly as `ctrl-r` is, gated on that
panel and inert outside it; none of them shadows the plain `f`, `n` or `p`,
and `n`/`N` still walk a search
([query editor design](specs/2026-08-16-query-editor-design.md) §4.2, §5.5).
`ctrl-d` and `ctrl-u` are the exception the rule can afford, and are described
under §1's motion table: they delegate to the paging arm rather than carry a
meaning of their own, so the layer still holds nothing that shadows a plain
key. The first four gate on their own panel and their own overlay;
`ctrl-d`/`ctrl-u` are dispatched above the guard `ctrl_key` applies to the
others, since an overlay pages exactly as it does under `PageDown`.

**Vim and arrows are synonyms, both ways.** `j`/`↓`, `k`/`↑`, `l`/`→`, `h`/`←`.
Neither set is the real one. Anyone who reaches for arrows should never hit a
motion that only vim keys have, and the reverse (vision.md §4, principle 2 — the
same key at every level of skill).

The pairing breaks in Queries, and only there: a panel that takes text claims
`j`/`k`/`l`/`h` as characters before this match is reached, so only the
arrows still move the cursor. This is the vim-key half of the same exception
§3's key budget note makes for every letter.

Giving the Contents grid a column cursor cost the pairing nothing, which is why
it was preferred to the two alternatives — arrows-only in the grid, or a fresh
pair from §3's free set. `l` and `→` reach the same action in every panel, and
so do `h` and `←`.

**The same key means the same thing in every panel.** `j` moves down in
Tables, Contents and DDL — the panels that treat it as motion — or is
inert; it never means something else there. A panel that cannot honour a
motion swallows it rather than passing it on. Queries is the exception: it
takes text, so `j` types itself, along with `k`/`l`/`h` — the vim/arrows rule
above.

The horizontal four are the same rule rather than an exception to it. `h`/`←`
mean **move left** and `l`/`→` mean **move right**, and each panel resolves that
in its own vocabulary:

| Panel | `h` / `←` | `l` / `→` |
|---|---|---|
| Tables | Fold — step out | Unfold — step in |
| Contents | Previous column | Next column |
| DDL | Inert | Inert |
| Queries | Cursor left, in text | Cursor right, in text |

That is the shape `j`/`k` have had since the first list: "down one row" is a
tree row, a grid row or a wrapped line depending on what the panel is showing,
and is inert where it is showing nothing. Reading the horizontal keys as "fold"
and "unfold" was a description of the only panel that had claimed them, not a
rule ([table display design](specs/2026-08-10-table-display-design.md) §2).

`Home` and `End` are a second exception. Everywhere else they reach the ends
of the document; in the editor they reach the ends of the line, because that
is what they do in every text editor a user has met. The document's ends
stay reachable with `PageUp` and `PageDown`.

A panel number (`1`–`4`) is a third exception: it moves focus in every panel
that does not take text, and is text on the one tab that does — Queries'
Cql tab, not the panel whole. So are `[` and `]`. Queries' Logs tab takes
none of them as text, but `4`, `[` and `]` still do nothing there, for a
different reason §1's tabs discussion gives. The rule's own enforcement
test was renamed for this — §5.

`Tab`, `Enter` and `Esc` under a completion popup are a fourth, and the only
one conditional on something drawn rather than on which panel is focused: while
the popup is up `Tab` and `⏎` accept the selected candidate and `Esc` takes the
popup down, and in every other frame all three mean what they mean everywhere.
`↑`/`↓` join them there — they walk the popup while one is up and move the
cursor otherwise — and are the mildest of the five: what they move is still the
row list on screen.
The condition is on screen — the popup itself — and the bar drops `tab panel`
and `⏎ newline` for exactly those frames rather than name something the next
keystroke will not do
([query editor design](specs/2026-08-16-query-editor-design.md) §5.4,
[completion ranking design](specs/2026-08-17-completion-ranking-design.md) §5).

**A binding must be visible.** vision.md §4, principle 1: discoverability is
ambient. Each panel's title opens with the key that reaches it; the hint bar along
the bottom row names each focused panel's principal bindings, not its full
surface — the bar has never claimed to be the complete list
([design](specs/2026-08-09-hint-bar-design.md) §6). `Esc` is named wherever it
is unconditionally true, in that frame, that pressing it does something:
Queries, where it leaves the editor for Tables; an open palette or search
prompt, where it closes it; and a panel carrying a search, where it clears
one (panel search design §5.3). It stays out of a panel with neither an
overlay up nor a search applied, where it still only does something
conditionally, and naming it there would still be a lie in most frames
(design §3). A binding that is in none of a title, the bar, or the palette is
a binding only this file knows about.

**Motion is assumed, not taught.** `j`/`k`, `h`/`l`, `Home`/`End`, the arrows
and `PageUp`/`PageDown` are off every panel's bar: a user of a terminal list
reaches for them before reading anything, and a bar that spends its width on
them has none left for what is particular to the panel — which is the only
thing a *contextual* bar is for. `Shift-Tab` is `Tab`'s inverse and is off it
for the same reason. The one exception is the theme picker's `j/k preview`,
where what the hint teaches is the side effect rather than the motion (§1's
picker subsection).
The palette lists every one of them regardless — that is the division of
labour the next paragraph states.

A hint the bar shows must be true *in the frame it is shown in*, and one
binding is not: `ctrl-r` on an empty or whitespace-only buffer runs nothing
(query execution design §3), and an empty buffer is what the Queries panel
starts as. Hiding the hint until the buffer fills would hide the thing the
panel exists for from the user who has not typed yet, so it is **greyed
instead** — same place, same width, without the weight a live key carries.
Greyed is the third answer beside naming and omitting, and it is what a
conditional binding gets; it is not a licence to name a key and grey it rather
than think about whether it belongs on the bar. `esc` is still omitted from the
other three bars rather than greyed there: it would be grey in most frames, and
a permanently grey hint teaches nothing.

The palette is the half of that pair with no discretion. The bar picks what a
panel's hints teach; the palette claims to be complete, so it may not omit a key
that does something ([command palette design](specs/2026-08-09-command-palette-design.md)
§2). That claim is about keys, not about wordings. The shared motions — `j`/`k`,
`h`/`l`, the arrows, the paging keys, `Home`/`End` — are listed once, under
`Navigate`, and worded for the movement rather than for what a panel makes of
it: `h`/`←` is "left, or fold". A panel's own group then carries only what that
panel alone answers — `⏎` in the tree, `m` in the grid. Listing the horizontal
pair under every heading that resolves it, which the catalogue did until
2026-08-15, bought a per-panel noun and cost the reader the same two keys three
times over before anything particular to a panel.

**Focus is the mode, and an overlay outranks focus.** A panel either takes
text or it does not, and `Panel::takes_text` is the one exhaustive answer. In a
panel that takes text every printable key is a character; everywhere else every
printable key is a binding. There is no state within a panel that changes what
a key means, so the regime is always the focused border — which is on screen
and already means something. This replaces the older "no modes" rule, which
held only until the Queries panel took text.

The Table contents selection mode does not touch this. `m` changes what the
grid highlights — a row, a column, or the cell they cross — and no key changes
meaning: `j`/`k` move the row cursor and `h`/`l` the column cursor in all three
modes, exactly as they did when there was only one way to draw them
([design](specs/2026-08-13-selection-modes-design.md) §3). A mode that rebound
those keys would break this rule, and is not what this is.

The command palette sits above that. It is opened deliberately with `?`, closes
with either `?` or `esc` — both named on its bar
([design](specs/2026-08-09-command-palette-design.md) §7) — and while it is up it
takes motion, runs the selected row on `⏎`, leaves `q` quitting, and makes
every other key inert rather than dismissing. A key that means "close" in one
frame and something else in the next is the ambiguity both halves of this
rule exist to prevent.

**A modal that claims every key says so by dimming what it covers.** While the
palette, the message overlay, the theme picker or the diagnostics list is up,
there is no focused panel — all four take the border and title an unfocused
panel takes — and the cursor row, the cursor cell and the washes around it are
not drawn: a panel that still drew them would be claiming a press that cannot
reach it. The hint bar recedes with them — every hint on it goes `Faint`, the
weight an inert hint already carries, because the modal has claimed those keys
and is listing them itself. The search prompt and the
export form are excluded — the prompt reads against the panel it filters, and
the form confirms what the grid beneath it has selected
([command palette design](specs/2026-08-09-command-palette-design.md) §8).

The search prompt sits at the same rank, entered deliberately with `/` and
left with `Enter` or `Esc`. Unlike the palette it takes text, not motion:
every printable key appends to the query rather than doing anything else, `q`
and `?` included — the panel-that-takes-text rule applied to an overlay
(panel search design §1.1).

The theme picker sits at the same rank too, entered deliberately with `ctrl-t` and
left with `Enter` or `Esc`. Like the palette it takes motion, not text: `j`/`k`
and the arrows move its cursor, and moving previews — the one overlay whose
motion has a side effect outside itself, since it writes `State.theme` on
every step (§1's picker subsection, themes design §6). Like the palette it
also leaves `q` quitting rather than swallowing it: a key that means quit
everywhere except in one overlay is not a key anyone can trust without
looking, so an overlay that changes that meaning has to say so.

The configuration diagnostics overlay sits at the same rank, but is entered
by nothing the user presses — it opens itself at startup, if either config
file had something to report, and is left with `Esc` alone. It takes motion
the same as the palette and the picker, and it is the one overlay of the
four where `q` does not quit: see its own subsection above.

The palette and the picker each carry a `/` of their own now, and while
either's is up the overlay it belongs to stops taking motion and takes text
instead — the panel-that-takes-text rule, applied a second time, inside an
overlay that already claimed every key once. Every printable key types
itself there, `q` included, the same swap the search prompt already makes
of the palette and the picker's own `q`-quits rule. The list under it never
narrows — a cursor walks to the match instead, the same live-as-you-type
preview the search prompt gives Tables, DDL and Table contents — and nothing
of the search outlives the prompt: `⏎` keeps only the row the cursor found,
`Esc` puts the cursor back where the prompt opened, and either way the query
itself is gone once the prompt closes, with no highlight and no `n`/`N` left
to walk
([overlay search design](superpowers/specs/2026-08-16-overlay-search-design.md) §2, §5, §6).

**`Esc` resolves in one order, first hit wins.** A live sift inside an
overlay comes off first, restoring the cursor to the row the prompt opened on
and, in the picker, the theme it previewed with it — the overlay itself stays
up. Failing that, an overlay up is dismissed — a search prompt cancels,
restoring the search and the cursor it found applied when it opened; the
palette just closes. Failing that, an extending
range in the focused panel freezes, the way a second `v` would, so `Esc`
never discards a range that is still growing. Failing that, the selection
clears — every group at once, however many are held. Failing that, the
focused panel's search, if it has one, clears. Failing that, Queries hands
focus back to Tables. Failing all
six, `Esc` does nothing. The first rung preserves
`escape_dismisses_the_overlay_before_it_leaves_the_editor`; extending it down
the chain is what "the thing most recently put on screen comes off first"
means once a panel can carry state of its own (panel search design §6, visual
mode design §4, overlay search design §1).

**No key without a test.** Every binding above is pinned by name in §5.

---

## 3. The key budget

Taken, lowercase: `g` `h` `j` `k` `l` `m` `n` `q` `v` `y`, plus `?` and `/`.
`g` is spent as a prefix rather than as a binding: it is the first half of
`gg` and does nothing alone, which spends it just the same.
Taken, digits: `1` `2` `3` `4`.
Taken, punctuation: `[` and `]`.
Taken, uppercase: `N` and `G` — the only shifted letters bound (§2).

Free, lowercase: `a` `b` `c` `d` `e` `f` `i` `o` `p` `r` `s` `t` `u`
`w` `x` `z`, the digits `5`–`9` and `0`, and every uppercase letter but `N`
and `G`.

`Space` is claimed for the first time, inside the export form only: it
activates the focused stop there (§1's export-form subsection) and is free
everywhere else, unlike a plain letter — a panel's own claim on a letter
stops at that panel's border, and so does the form's.

Named keys taken: `Enter` `Esc` `Tab` `Shift-Tab` `Home` `End` `PageUp`
`PageDown` `↑` `↓` `←` `→`, plus `Backspace` and `Delete`, which only the
editor and the export form's path stop bind.

Ctrl taken: `ctrl-r`, `ctrl-f`, `ctrl-n` and `ctrl-p` in Queries, `ctrl-e` in
two panels — Queries and Table contents (§1's export-form subsection) —
`ctrl-t` everywhere, and `ctrl-d`/`ctrl-u` everywhere `PageDown`/`PageUp` do
anything. The rest of the layer is free — including `r`, `e`, `t`, `d`, `u`,
`f` and `p` as plain keys, which nothing binds. `n` is bound plain, to the next
search match, and `ctrl-n` is a binding of its own rather than a synonym for
it. Alt is not a layer and nothing may claim from it.

Named keys the domain understands but nothing binds: `Insert` `F1`–`F12`.
Anything crossterm reports outside that vocabulary is dropped at the edge and
never reaches the domain. The pointer is not in this budget at all — see §7.

The ctrl namespace is separate from the plain-key budget above: `r` is free as
a plain key and taken behind ctrl.

Claim from the free set first. Re-using a taken key means retiring a binding,
which is a spec-level change.

> Every printable key is also text in a panel that takes text. That does not
> spend it: `s` is still free to be bound for the other three panels.

---

## 4. Deliberately unbound

| Not bound | Why |
|---|---|
| `a` `b` `c` `d` `u` focus | Were the four panels and the Columns tab until §1 moved to `1`–`4`. A letter has to be learned per panel; four digits in frame order are one rule, and `[1]-` in the title tells a novice the whole binding. Free, and deliberately left so for a while — a finger that learnt one should find nothing rather than something else. |
| `A` `B` `C` `D` `U` | Reserved. A shifted letter is a binding of its own, not a synonym. `T` opened the theme picker until the picker moved to `ctrl-t`, and is free again. |
| `t` focus | Was Tables before either scheme (§1). Free for the same reason the letters above are. |
| `Space` on a tree row | Was `Enter`'s synonym there. One key, one action (§2) — true still, even though `Space` is no longer unbound everywhere: it activates a stop inside the export form (§1), and a tree row is not one. |
| `ctrl-e` / `ctrl-y` | Moved the DDL viewport a line without its cursor. Reading a panel from its heading down needs a design, not two chords. The yank slice did not claim `ctrl-y`: `y` was free as a plain key, and a yank in Queries needs a selection first (yank design §1). |
| `r` refresh | Nothing re-issues `Effect::LoadSchema` after a racing DDL change; recovery is a restart. The binding waits on the reload path, not the other way round (TODO.md). |
| `Home`/`End` over columns | Both reach the ends of the document, which in a grid is the row list. A second axis for one panel would be a third exception to a two-exception rule. |
| Horizontal wheel | Dropped at the edge, and this slice did not pick it up. A notch moves the row viewport; columns are reached with the keys §1 names. |
| An expand-cell key | Horizontal scrolling reaches columns, not the tails of values, so a 36-character uuid is still ellipsised. Showing one in full is a view with a design of its own, and a cell cursor is what it was waiting for (TODO.md). |
| Drag-to-select | Weighed and refused: `lc-inbound` drops drag and move events at the edge today (§7), so honouring one would need a new gesture, press-origin state, and an autoscroll rule at the panel edges — for a selection two keystrokes already make. |
| `Shift-↓` extend selection | `v` is the way to extend a selection; a shifted arrow would be a second name for a key that already does it, the fourth kind of duplicate §2's "one key, one action" refuses. |
| User-defined bindings | vision.md §7 Phase 5. |

---

## 5. Where it is enforced

Tests in `crates/lc-domain/src/update.rs` unless noted.

| Guarantee | Test |
|---|---|
| `j`/`k` walk a list | `j_and_k_walk_the_keyspace_list` |
| Arrows are synonyms | `the_arrow_keys_do_what_j_and_k_do` |
| Motion stops at both ends | `the_selection_cannot_run_off_a_list_it_has_not_loaded`, `the_grid_cursor_cannot_run_off_a_page` |
| Motion is the focused panel's only | `motion_belongs_to_the_focused_panel_only`, `motion_in_one_panel_never_reaches_another` |
| `Enter` folds and reads, and `Space` does neither | `enter_toggles_a_keyspace`, `space_is_not_a_second_name_for_enter`, `space_reads_no_table` |
| `l`/`h` and arrows agree | `l_and_right_open_a_keyspace`, `h_and_left_close_a_keyspace` |
| `h` steps out before folding | `h_from_a_table_steps_out_to_its_keyspace_before_folding` |
| Tree keys are Tables-only | `a_tree_transition_belongs_to_the_focused_panel_only`, `enter_outside_the_tree_reads_nothing` |
| `l`/`h` walk the grid's columns | `l_and_right_move_the_grid_cursor_one_column`, `h_and_left_move_the_grid_cursor_back_one_column` |
| The cell cursor stops at either edge | `the_grid_cursor_cannot_run_off_either_edge_of_a_page`, `a_grid_with_no_page_swallows_horizontal_motion` |
| Horizontal motion is the focused panel's only | `horizontal_motion_belongs_to_the_focused_panel_only` |
| `Home`/`End` keep the row axis in a grid | `home_and_end_still_reach_the_ends_of_the_row_list` |
| A read returns the cell cursor to the first cell | `a_request_returns_the_cell_cursor_to_the_first_cell`, `a_refusal_returns_the_cell_cursor_to_the_first_cell`, `a_fresh_panel_has_no_columns_to_move_over`, `a_loaded_page_reports_its_columns` (`contents.rs`) |
| The column viewport follows the cell cursor | `the_column_viewport_follows_the_cell_cursor`, `a_narrower_frame_pulls_the_column_viewport_back_to_the_cursor`; `ColumnScroll` itself by `a_visible_column_moves_the_viewport_nowhere`, `the_viewport_moves_the_shortest_distance_that_shows_a_column_to_the_right`, `the_viewport_follows_a_column_to_the_left`, `a_column_too_wide_for_the_panel_is_shown_from_its_own_start`, `following_a_column_that_is_gone_clamps_the_viewport` (`grid.rs`) |
| A column is as wide as the widest thing in it, capped | `a_column_is_as_wide_as_the_widest_thing_in_it`, `a_heading_wider_than_every_value_sets_the_width`, `a_column_is_capped`, `a_column_with_no_rows_is_still_as_wide_as_its_heading` (`grid.rs`) |
| Only whole columns are drawn | `only_the_columns_that_fit_are_counted`, `fitting_counts_from_the_offset`, `a_panel_too_narrow_for_one_column_still_draws_it`, `fitting_past_the_last_column_is_nothing` (`grid.rs`) |
| The grid names its cell, its first column and its widths | `a_grid_carries_the_width_of_every_column`, `a_grid_names_the_selected_cell_and_the_first_column_drawn`, `the_gutter_is_as_wide_as_the_last_row_number` (`view/contents.rs`, and the same name in `grid.rs` for the arithmetic under it) |
| A read lands focus in Contents, by key or by click | `reading_a_table_lands_focus_in_contents`, `a_click_on_a_table_lands_focus_in_contents_too`, `focus_moves_before_the_page_arrives`, `a_refused_read_still_lands_focus_in_contents` |
| Unfolding does not move focus, and a read leaves the tree alone | `unfolding_a_keyspace_leaves_focus_in_tables`, `reading_a_table_leaves_the_tree_selection_where_it_was`, `reading_a_table_lands_focus_in_contents_and_j_walks_the_grid_rather_than_the_tree` (`lc-outbound/tests/scenario.rs`) |
| The cell is drawn over its row, and the gutter numbers the page | `the_selected_cell_is_marked_apart_from_the_rest_of_its_row`, `the_gutter_is_muted_and_the_values_beside_it_are_not`, `rows_are_numbered_from_one`, `the_row_number_is_the_pages_own_index_after_a_scroll` (`inbound/render.rs`) |
| The cell highlight reads against the row's without a colour | `a_selected_cell_reads_against_the_selected_row_without_spending_a_colour` (`inbound/theme/mod.rs`) |
| `m` cycles the selection mode, and does so only in Contents | `m_cycles_the_contents_selection_mode`, `m_does_nothing_outside_the_grid`, `m_types_an_m_in_queries` |
| A fresh panel opens in row mode, cycling wraps, and a new request keeps the mode | `a_fresh_panel_selects_a_row`, `the_mode_cycles_row_column_cell_and_wraps`, `a_new_request_keeps_the_mode_and_clears_the_cursors` (`contents.rs`) |
| Row mode lights the row and column mode lights the column and its heading | `row_mode_lights_the_whole_row_and_no_column`, `column_mode_lights_the_column_and_its_heading`, `a_heading_is_unwashed_once_the_crosshair_scrolls_out_of_view`, `column_mode_lights_nothing_when_its_cursor_is_off_the_columns_drawn` (`inbound/render.rs`) |
| The wash leaves a flavour's column colours, and a key column's heading colour, alone | `the_crosshair_leaves_a_flavours_column_colours_alone`, `a_key_columns_heading_keeps_its_colour_under_the_wash` (`inbound/render.rs`) |
| The crosshair is fainter than the cursor row and flattens no colour the row keeps | `the_crosshair_is_fainter_than_the_cursor_row`, `the_bands_paint_a_background_and_spend_no_foreground` (`inbound/theme/mod.rs`) |
| The grid is drawn from the column the viewport names | `the_grid_is_drawn_from_the_column_the_viewport_names`, `a_column_wider_than_the_panel_is_ellipsised_rather_than_squeezed`, `the_frame_reports_how_wide_the_contents_panel_is` (`inbound/render.rs`) |
| A hidden column, and only that, draws the bottom bar | `a_grid_wider_than_its_panel_gets_a_horizontal_scrollbar`, `a_grid_that_fits_gets_no_horizontal_scrollbar`, `the_horizontal_thumb_sits_at_whichever_end_of_the_track_the_viewport_is_on` (`inbound/render.rs`) |
| The palette lists every key that does something, and only those | `no_key_does_anything_the_palette_does_not_list`, `every_key_the_palette_lists_does_something`, `the_catalogue_lists_the_selection_mode_key`, `the_catalogue_lists_every_key_a_panel_hints_at` (`commands.rs`) |
| A repeated number keeps focus and walks the tabs | `a_repeated_number_keeps_focus_and_walks_the_panels_tabs`, `pressing_two_three_times_returns_to_the_first_tab` |
| A tab change resets the cursor and viewport and keeps the search | `a_tab_change_puts_the_cursor_and_the_viewport_back_at_the_top`, `a_tab_change_leaves_the_search_where_it_was` (`ddl.rs`) |
| The columns tab lists every column with its kind | `the_columns_tab_lists_every_column_with_the_part_it_plays`, `the_columns_tab_shows_the_columns_the_ddl_tab_leaves_out` (`view/details.rs`), `the_columns_tab_draws_every_column_with_its_kind_and_type` (`inbound/render.rs`) |
| A strip too narrow keeps the drawn tab whole | `a_border_wide_enough_names_both_tabs_and_fades_the_one_not_drawn`, `a_border_too_narrow_for_either_strip_keeps_the_tab_it_is_drawing_whole` (`inbound/render.rs`) |
| `q` quits and cancels | `q_quits`, `quitting_cancels_everything_still_in_flight` |
| `Esc` clears the overlay | `escape_dismisses_the_overlay` |
| `Tab`/`Shift-Tab` cycle every `(panel, tab)` stop, in order, and wrap | `tab_walks_every_stop_in_order_and_wraps`, `shift_tab_walks_the_same_stops_the_other_way`, `tab_steps_ddls_two_tabs_as_separate_stops`, `tab_from_queries_cql_lands_on_queries_logs`, `tab_from_queries_logs_lands_on_tables_and_reverts_to_cql`, `shift_tab_from_tables_lands_on_queries_logs`, `a_full_forward_cycle_returns_to_where_it_started`, `shift_tab_undoes_tab_from_every_stop` |
| The Logs tab takes no text, and `q` quits it | `the_trail_takes_no_text`, `q_quits_from_the_trail` |
| The trail is searchable, and `n` walks its matches | `the_trail_is_searchable`, `n_walks_the_trails_matches_and_wraps`, `escape_over_the_trail_clears_its_search` |
| `y` yanks an entry's two rows, or a held selection's | `yanking_an_entry_copies_its_statement_and_its_outcome`, `yanking_a_visual_selection_joins_every_selected_entrys_two_rows`, `yanking_the_trail_lights_the_entries_it_took`, `y_on_the_cql_tab_yanks_nothing` |
| Paging and `Home`/`End` | `page_down_moves_the_tree_by_one_panel_height`, `home_and_end_reach_the_ends_of_the_tree_in_one_key` |
| `gg`/`G` reach the ends of a panel's list | `gg_and_shift_g_reach_the_ends_of_the_tree`, `gg_and_shift_g_reach_the_ends_of_the_ddl_rows`, `gg_and_shift_g_reach_the_ends_of_the_grids_rows` |
| `g` alone is a prefix, and the next key spends it | `a_lone_g_moves_nothing`, `a_key_between_the_two_gs_spends_the_prefix` |
| `gg`/`G` reach the ends of an overlay's list too | `gg_and_shift_g_reach_the_ends_of_an_open_palette`, `gg_and_shift_g_reach_the_ends_of_the_diagnostics`, `the_ends_are_reachable_in_the_picker` |
| Both are text where a panel or a prompt takes text | `g_is_text_in_queries_and_under_a_prompt` |
| Ctrl/alt are not synonyms | `an_unbound_ctrl_key_does_nothing`, `an_alt_modified_key_is_not_a_binding` |
| `ctrl-d`/`ctrl-u` page whatever `PageDown`/`PageUp` page | `ctrl_d_and_ctrl_u_page_the_focused_panel`, `ctrl_d_pages_an_open_palette_rather_than_the_panel_underneath`, `ctrl_d_pages_the_editor_the_way_pgdn_does`, `ctrl_d_and_ctrl_u_page_the_picker_too`, `ctrl_d_is_inert_under_a_search_prompt` |
| The letters the numbers replaced stay free | `the_letters_the_numbers_replaced_stay_unbound` |
| `ctrl-t` opens the picker on the theme in force | `the_picker_opens_on_the_theme_in_force` |
| Moving the cursor previews the theme live | `moving_the_cursor_previews_the_theme` |
| `Enter` keeps the previewed theme | `enter_keeps_the_previewed_theme` |
| `Esc` restores the theme the picker opened on | `escape_restores_the_theme_the_picker_opened_on` |
| The picker cursor stops at either end of the list | `the_picker_cursor_cannot_run_off_the_list` |
| Paging reaches the ends of the theme list, previewing | `paging_reaches_the_ends_of_the_theme_list` |
| `q` still quits with the picker open | `q_still_quits_with_the_picker_open` |
| The ends are reachable in the picker, in both spellings | `the_ends_are_reachable_in_the_picker` |
| Moving the picker cursor asks for a repaint | `moving_the_picker_cursor_asks_for_a_repaint` |
| The bar names what the picker responds to, and nothing else | `the_bar_names_what_the_picker_responds_to` (`view/hints.rs`) |
| The picker is refused where it cannot be drawn | `a_frame_too_small_for_a_list_refuses_the_picker` |
| A shrink below the picker closes it and restores the theme | `a_frame_that_shrinks_below_the_picker_closes_it_and_restores_the_theme` |
| A message over an open picker restores the theme | `a_message_arriving_over_an_open_picker_restores_the_theme` |
| A click dismisses the picker and restores the theme | `a_click_dismisses_the_picker_and_restores_the_theme` |
| The picker lists every theme, and a narrow frame draws none of it | `the_picker_lists_every_theme`, `a_frame_too_narrow_for_the_list_draws_none_of_it` (`inbound/themes.rs`) |
| The cursor row is a bar of the flavour's own surface | `the_cursor_row_is_a_bar_of_the_flavours_own_surface` (`inbound/themes.rs`) |
| The cursor row, its crosshair and a range are the only tokens that paint a background, and none spends a foreground | `the_bands_paint_a_background_and_spend_no_foreground` (`inbound/theme/mod.rs`) |
| A range reads louder than the crosshair and quieter than the cursor row | `the_range_is_louder_than_the_crosshair_and_quieter_than_the_cursor_row` (`inbound/theme/mod.rs`) |
| Every palette of ours reads on every ground it declares, the crosshair included | `every_palette_of_ours_reads_on_every_ground_it_declares`, `the_neutral_ramp_of_ours_is_a_staircase`, `every_border_of_ours_is_perceptible` (`inbound/theme/mod.rs`) |
| Every flavour reads against its own base, and a highlight bar is distinct from its field | `every_flavour_reads_against_its_own_base`, `a_highlight_bar_is_distinct_from_its_field` (`inbound/theme/ported/catppuccin.rs`) |
| Every palette sets every surface | `Palette` holds plain `Color`s and no `Option<Color>`, so nothing can decline one and there is nothing left to test (`inbound/theme/mod.rs`) |
| Panel numbers are distinct and total | `every_panel_has_a_distinct_number`, `a_number_leads_back_to_the_panel_that_claims_it`, `the_numbers_are_the_ones_the_design_names` — all in `state.rs` |
| No number is shadowed by an earlier arm | `a_number_focuses_its_panel_from_any_panel_that_does_not_take_text` |
| A number and a bracket are text in a panel that takes text | `a_number_is_text_in_a_panel_that_takes_text`, `a_bracket_is_text_in_a_panel_that_takes_text` |
| Numbers have no side effects | `a_number_moves_focus_and_does_nothing_else` |
| Walking the tabs wraps both ways | `walking_the_tabs_wraps_and_stepping_back_undoes_it` (`ddl.rs`), `brackets_walk_the_focused_panels_tabs_both_ways` |
| `[`/`]` do nothing from a panel with no tabs | `brackets_do_nothing_from_a_panel_with_no_tabs` |
| A keyspace is offered no Columns tab, and walking stores none | `a_keyspace_is_offered_no_columns_tab_and_keeps_describing_itself` (`view/details.rs`), `walking_on_a_keyspace_stays_on_the_only_tab_there_is` |
| A tab change restarts the cursor; arriving at the tab drawn does not | `showing_a_tab_draws_it`, `a_tab_change_puts_the_cursor_and_the_viewport_back_at_the_top`, `showing_the_tab_already_drawn_leaves_the_cursor_and_the_viewport_alone`, `a_tab_change_leaves_the_search_where_it_was` (`ddl.rs`) |
| Every title opens with its own number | `every_panel_title_opens_with_the_key_that_focuses_it` (`view/mod.rs`), `each_panel_opens_its_title_with_the_key_that_focuses_it` (`inbound/render.rs`), plus the `PanelTitle` tests in `view/title.rs` |
| No title wears the hint bar's key colour | `no_title_wears_the_shortcut_colour` (`inbound/render.rs`), `a_hot_key_is_underlined_and_nothing_else_is` (`inbound/theme/mod.rs`) |
| A click folds, reads, and selects | `a_click_on_a_keyspace_folds_and_unfolds_it`, `a_click_on_a_table_reads_it`, `a_click_on_a_grid_row_moves_the_cursor_and_asks_for_nothing` |
| A click focuses what it lands in | `a_click_focuses_the_panel_it_landed_in`, `a_click_on_chrome_focuses_and_does_nothing_else` |
| A click on a cell moves both cursors, and on a heading only the column | `a_click_on_a_cell_moves_both_cursors`, `a_click_on_a_heading_moves_the_column_cursor_and_no_row` |
| A click never changes the selection mode | `a_click_on_a_cell_leaves_the_selection_mode_alone` |
| A click on a DDL row selects it | `a_click_on_a_ddl_row_moves_the_cursor` |
| A click on a tab draws it, and a stale one only focuses | `a_click_on_a_tab_focuses_its_panel_and_draws_it`, `a_click_on_a_tab_the_strip_no_longer_draws_only_focuses`, `a_click_on_a_single_tab_panels_title_focuses_it` |
| A stale row or column is dropped, not clamped | `a_click_on_a_row_that_has_gone_is_dropped_rather_than_clamped`, `a_click_on_a_ddl_row_that_has_gone_is_dropped_rather_than_clamped`, `a_click_on_a_column_that_has_gone_is_dropped_rather_than_clamped` |
| A click spends itself on the overlay | `a_click_dismisses_the_overlay_and_does_nothing_else`, `a_wheel_does_not_dismiss_the_overlay` |
| The wheel scrolls without focusing | `a_wheel_scrolls_the_panel_under_it_without_focusing_it`, `a_wheel_over_the_ddl_panel_scrolls_its_lines`, `a_wheel_over_a_panel_with_nothing_to_scroll_is_inert`, `a_wheel_over_tables_moves_the_viewport_rather_than_the_selection`, `a_wheel_over_tables_no_longer_scrolls_the_ddl_panel_back` |
| Only the left button and the vertical wheel are bound | `only_the_left_button_is_bound`, `a_release_a_drag_and_a_move_are_dropped`, `a_horizontal_wheel_is_dropped` (`inbound/input.rs`) |
| A modified gesture is not a binding | `a_modified_press_is_not_a_gesture` (`inbound/input.rs`) |
| The reader thread's filter agrees with `to_pointer`'s | `is_bound_accepts_everything_to_pointer_would` (`inbound/input.rs`) |
| The map resolves to the row that was drawn | `the_map_resolves_every_drawn_keyspace_to_the_row_that_drew_it`, `the_map_resolves_a_scrolled_tree_to_the_rows_that_were_drawn`, `the_map_resolves_a_scrolled_grid_to_the_rows_that_were_drawn` (`inbound/render.rs`) |
| The map resolves to the column, tab, and wrapped row that were drawn | `the_map_resolves_every_cell_to_the_column_that_drew_it`, `the_map_resolves_every_drawn_tab_to_the_key_that_reaches_it`, `a_single_tab_panel_resolves_its_title_to_its_own_key`, `the_map_resolves_every_line_of_a_wrapped_ddl_row_to_that_row` (`inbound/render.rs`) |
| Chrome is not a row | `a_grids_heading_names_a_column_rather_than_its_first_row`, `a_press_on_the_chrome_of_a_panel_names_that_panel`, `the_hint_bar_and_a_frame_below_the_minimum_are_not_clickable` (`inbound/render.rs`) |
| Letters are text in the editor | `a_letter_typed_in_queries_is_text_not_a_shortcut` |
| `?` is a bind marker in the editor, not the palette | `a_question_mark_typed_in_queries_is_a_bind_marker_not_the_palette` |
| An open palette outranks a panel that takes text | `an_open_palette_outranks_a_panel_that_takes_text` |
| Quitting is not reachable from the editor | `q_does_not_quit_while_the_editor_has_focus` |
| `Esc` prefers the overlay, then leaves | `escape_dismisses_the_overlay_before_it_leaves_the_editor`, `escape_leaves_the_editor_for_tables` |
| `Tab` still leaves | `tab_still_leaves_the_editor` |
| Ctrl is still not text | `a_modified_key_is_still_not_text` |
| The editing keys reach the buffer | `the_editing_keys_reach_the_buffer` |
| `Home`/`End` take the line's ends | `home_and_end_reach_the_ends_of_the_line_in_the_editor` |
| Other panels keep their letters | `typing_outside_queries_still_focuses_a_panel` |
| The buffer's own behaviour | the `editor.rs` tests |
| `?` opens and closes the palette | `question_mark_opens_the_palette_and_closes_it`, `escape_closes_the_palette` |
| It replaces a message rather than stacking | `opening_the_palette_replaces_the_message_it_covers` |
| It does not open where it cannot be drawn | `the_palette_does_not_open_where_it_cannot_be_drawn` |
| A resize that leaves it undrawable closes it too | `a_frame_that_shrinks_below_the_palette_closes_it` |
| An open palette takes motion and swallows the rest | `an_open_palette_takes_the_motion_the_panel_would_have`, `an_open_palette_swallows_the_keys_it_does_not_use`, `q_quits_from_inside_the_palette` |
| `⏎` runs the selected row, in its own panel | `enter_closes_the_palette_and_focuses_the_panel_the_row_belongs_to`, `enter_presses_the_rows_own_key`, `enter_on_the_run_row_runs_the_buffer`, `every_row_can_be_run` |
| A row filed under a panel does something there | `a_row_filed_under_a_panel_stays_in_that_panel` |
| A form row is documentation, not a command | `enter_on_a_form_row_closes_the_palette_and_stops`, `the_export_form_rows_are_documentation_rather_than_commands` (`commands.rs`) |
| The cursor never lands on a heading | `the_palette_cursor_steps_over_a_heading`, `paging_settles_on_a_command_rather_than_a_heading` (`commands.rs`) |
| No row outgrows the narrowest palette | `no_command_row_is_wider_than_the_narrowest_palette` (`commands.rs`) |
| The bar names `?`, and the palette's own keys | `the_bar_names_the_palette_between_panel_and_quit`, `the_bar_names_what_the_palette_responds_to` (`view/hints.rs`) |
| The bar drops `?` where no palette would fit | `the_bar_drops_the_palette_hint_where_no_palette_would_fit`, `every_global_hint_does_something_in_every_panel` (`view/hints.rs`, `update.rs`) |
| `?` earns its place in the width-fitting bar, and goes before `q quit` but after `tab panel` | `a_frame_with_room_for_the_palette_shows_its_hint_and_drops_it_after_tab_panel` (`inbound/hints.rs`) |
| The palette is drawn, centred, capped and scrollable | `the_palette_lists_the_catalogue`, `the_palette_is_centred`, `a_wide_frame_does_not_get_a_wide_palette`, `a_tall_frame_does_not_get_a_palette_full_of_blank_rows`, `a_frame_too_small_for_a_palette_does_not_get_one` (`inbound/render.rs`) |
| An empty query is no search | `an_empty_query_is_not_a_search` (`search.rs`) |
| Matching folds case, and is a substring, literally | `matching_ignores_case_in_both_directions`, `matching_is_a_substring_anywhere_and_not_a_prefix`, `a_metacharacter_is_matched_literally`, `folding_reaches_past_ascii` (`search.rs`) |
| The term is kept as typed, for the marker | `the_term_is_kept_as_typed_for_the_marker` (`search.rs`) |
| `Searched` refuses Queries and accepts the rest | `every_panel_but_queries_can_hold_a_search`, `a_target_names_the_panel_it_came_from` (`search.rs`) |
| A search forces the keyspace holding a match open, and leaves its sibling's fold state alone | `a_matching_keyspace_is_forced_open_and_its_sibling_keeps_its_fold_state`, `a_keyspace_holding_a_match_stays_open_whatever_its_fold_state`, `a_keyspace_matching_by_name_is_open_only_when_it_has_tables` (`tree.rs`) |
| A search drops no row, in either panel | `a_search_drops_no_row` (`tree.rs`), `a_search_drops_no_detail_row` (`view/details.rs`) |
| Clearing a search restores the fold state exactly | `clearing_the_search_restores_the_fold_state_exactly` (`tree.rs`) |
| `l`/`h`, `Enter`, and a click on a keyspace row are all swallowed while a search is applied | `l_and_h_are_swallowed_while_a_search_is_applied`, `folding_under_a_search_does_not_disturb_the_fold_state_it_will_return_to`, `l_and_h_still_fold_once_the_search_is_gone`, `no_tables_entry_point_writes_the_fold_state_under_a_search`, `enter_still_reads_a_table_under_a_search` |
| A property matches on its name and on its value | `a_property_matches_on_its_name`, `a_property_matches_on_its_value` (`view/details.rs`) |
| A column matches on its name and on its CQL type | `a_column_matches_on_its_name`, `a_column_matches_on_its_cql_type` (`view/details.rs`) |
| A heading is never in the sequence a search walks, which is what keeps it unmatchable | `the_rows_the_cursor_walks_are_the_described_ones_and_no_headings` (`view/details.rs`) |
| A section is never emptied by a search | `a_section_is_never_emptied_by_a_search` (`view/details.rs`) |
| A search that matches nothing still draws the panel, rather than a notice | `a_search_that_matches_nothing_still_draws_the_tree` (`view/tables.rs`) |
| A search changes nothing unless one is applied | `a_cluster_with_no_keyspaces_says_so_rather_than_listing_nothing` (`view/tables.rs`), `no_search_flags_nothing` (`view/contents.rs`) |
| Matching cells are flagged and the grid keeps every row | `a_search_flags_the_cells_that_match_and_keeps_every_row` (`view/contents.rs`) |
| A matched null is still a null | `a_matched_null_is_still_a_null` (`view/contents.rs`) |
| `/` opens the prompt in each of the three panels that can hold a search | `slash_opens_the_prompt_in_every_panel_that_can_hold_a_search` |
| `/` is text in Queries and opens nothing | `slash_is_text_in_queries_and_opens_nothing` |
| `q` and `?` type themselves in the prompt | `q_and_the_question_mark_type_themselves_in_the_prompt` |
| The prompt searches live, per keystroke, and jumps to the first match | `the_prompt_searches_live_per_keystroke`, `backspace_narrows_the_query_and_re_applies_it`, `backspace_on_an_empty_query_is_inert_rather_than_closing` |
| Every keystroke jumps from where `/` was pressed, not from the last landing | `extending_a_term_does_not_walk_the_cursor_forward` |
| A term that matches nothing leaves the cursor where it was | `a_term_that_matches_nothing_leaves_the_cursor_where_it_was` |
| `Enter` commits and closes, keeping the cursor where it landed; an empty commit clears the search | `enter_commits_and_closes_leaving_the_search_applied`, `enter_keeps_where_the_cursor_landed`, `an_empty_commit_clears_the_search`, `backspacing_a_query_to_empty_clears_the_search_live` |
| `Esc` cancels, restoring the search and the cursor together | `escape_cancels_the_prompt_restoring_the_search_that_was_there`, `escape_restores_the_search_and_the_cursor_together` |
| `Esc`'s full precedence chain | `escape_clears_the_focused_panels_search_when_no_overlay_is_up`, `escape_prefers_the_overlay_to_the_search`, `escape_leaves_queries_when_nothing_else_claims_it` (rung 3 alone — rung 2 before rung 3 is unpinnable until Queries can hold a search, design §8) |
| `Tab` is inert while the prompt is up | `tab_is_inert_while_the_prompt_is_up` |
| A click cancels the prompt | `a_click_cancels_the_prompt` |
| A prompt refuses the one chord that would open an overlay over it, and an unclaimed chord is still not text | `ctrl_t_is_refused_over_a_live_prompt`, `an_unclaimed_ctrl_chord_is_not_text_in_the_prompt` |
| `n` walks to the next match and wraps; `N` walks backwards; either is inert with no search applied | `n_walks_to_the_next_match_and_wraps`, `shift_n_walks_backwards`, `n_is_inert_where_no_search_is_applied` |
| `n` types a character in Queries, like any letter | `n_types_a_character_in_queries` |
| A live `v` range follows an `n` | `a_live_range_follows_an_n` |
| A jump past the DDL viewport is repaired on the measurement that follows | `the_measurement_after_a_search_pulls_a_stale_ddl_offset_back_inside`, `escape_clearing_a_ddl_search_re_follows_the_selection` |
| A search survives a focus change | `a_search_survives_a_focus_change` |
| A search is read back from the panel it was applied to | `a_search_is_read_back_from_the_panel_it_was_applied_to` (`state.rs`) |
| The three searches are independent | `the_three_searches_are_independent` (`state.rs`) |
| A panel is projected carrying the search applied to it | `a_panel_carries_the_search_applied_to_it` (`view/mod.rs`) |
| An open prompt is projected as its query | `an_open_prompt_is_projected_as_its_query` (`view/mod.rs`) |
| The bar names `/ search` where it does something, and not in Queries | `the_bar_names_search_in_every_panel_that_can_hold_a_search`, `the_bar_does_not_name_search_in_queries` (`view/hints.rs`) |
| The bar names `esc clear` only where a search is applied | `the_bar_names_esc_clear_only_where_a_search_is_applied` (`view/hints.rs`) |
| The bar names `n next match` only where a search is applied | `the_bar_names_n_only_where_a_search_is_applied` (`view/hints.rs`) |
| The bar names the prompt's own keys while it is up | `the_bar_names_what_the_prompt_responds_to` (`view/hints.rs`) |
| The palette lists `/`, under Global | `the_catalogue_lists_search_among_the_global_keys` (`commands.rs`) |
| The marker is drawn, muted, right-aligned | `a_panel_wears_the_search_applied_to_it`, `the_marker_is_drawn_muted` (`inbound/render.rs`) |
| The marker is dropped rather than truncated | `the_marker_is_dropped_rather_than_truncated`, `the_marker_fits_flush_against_the_corner_it_is_tuned_for` (`inbound/render.rs`) |
| An unfocused panel still wears its marker | `an_unfocused_panel_still_wears_its_marker` (`inbound/render.rs`) |
| The prompt owns the bar row, and its cursor | `an_open_prompt_owns_the_bar_row`, `an_open_prompts_own_cursor_replaces_the_editors`, `the_prompts_cursor_sits_after_the_query` (`inbound/render.rs`) |
| A match is painted where it sits, in Tables, and the search drops no row from the panel | `a_match_is_painted_where_it_sits`, `a_search_drops_no_row_from_the_panel` (`inbound/render.rs`) |
| A matched cell reads apart from an unmatched one, on and off the selected row | `a_matched_cell_reads_apart_from_an_unmatched_one_on_and_off_the_selected_row` (`inbound/render.rs`) |
| `Token::Match` is legible over `Token::Selected` | `a_match_is_legible_over_a_selected_row` (`inbound/theme/mod.rs`) |
| `ctrl-r` runs the buffer | `ctrl_r_runs_the_query_buffer`, `a_run_query_titles_the_panel_with_what_was_typed` |
| It runs only from Queries | `ctrl_r_outside_queries_runs_nothing` |
| An empty buffer is inert | `an_empty_buffer_runs_nothing` |
| Either path cancels the other | `running_a_query_cancels_a_pending_table_read`, `reading_a_table_cancels_a_pending_query` |
| Destructive is refused, unexecuted | `a_destructive_statement_is_refused_until_confirmation_exists` |
| The posture gate still applies | `a_typed_write_is_refused_on_a_read_only_connection` |
| A second modifier is not a binding | `ctrl_alt_r_does_not_run_the_buffer` |
| The palette outranks the ctrl layer | `an_open_palette_swallows_ctrl_r` |
| A statement is classified by its first keyword | the `statement.rs` tests |
| A modifier reaches the label | `a_binding_behind_ctrl_is_named_with_a_ctrl_prefix` (`view/hints.rs`) |
| The bar and palette name it | `the_queries_bar_names_the_run_binding` (`view/hints.rs`), `the_palette_names_the_run_binding`, `the_palette_has_a_queries_section` (`commands.rs`) |
| The bar greys `ctrl-r run` while there is nothing to run | `the_run_hint_is_inert_while_there_is_nothing_to_run`, `the_run_hint_is_live_once_there_is_a_statement`, `nothing_but_the_run_hint_is_ever_inert` (`view/hints.rs`) |
| A greyed hint is drawn distinctly and keeps its width | `an_inert_hint_is_greyed_rather_than_dropped` (`inbound/hints.rs`) |
| The bar's sweep asks only what it claims | `every_hint_the_bar_shows_does_something` |
| The editing keys stay inside the editor | `the_editing_keys_do_nothing_outside_a_panel_that_takes_text` |
| `ctrl-e` hands the Queries buffer to an editor | `ctrl_e_hands_the_queries_buffer_to_an_editor` |
| `ctrl-e` is inert outside Queries | `ctrl_e_is_inert_outside_queries` |
| An open palette swallows it too | `ctrl_e_under_an_overlay_does_nothing` |
| A closed editor replaces the buffer and moves the cursor to its end | `a_closed_editor_replaces_the_buffer` |
| A failed editor keeps the buffer and raises a message | `a_failed_editor_says_so_and_keeps_the_buffer` |
| The palette lists it | `the_catalogue_lists_the_external_editor` (`commands.rs`) |
| The bar names it after `ctrl-r run` | `the_queries_bar_names_the_editor_binding_after_run` (`view/hints.rs`) |
| No other bar names it | `no_other_bar_names_the_editor_binding` (`view/hints.rs`) |
| `ctrl-e` opens the export form over a loaded grid, and does nothing over an empty one | `ctrl_e_over_a_loaded_contents_panel_opens_the_export_form`, `ctrl_e_does_nothing_over_an_empty_contents_panel` |
| `ctrl-e` keeps its Queries meaning; the two are gated on focus | `ctrl_e_in_queries_still_opens_the_external_editor` |
| `ctrl-f` lays the buffer out, and is inert outside Queries | `ctrl_f_lays_the_buffer_out`, `ctrl_f_does_nothing_outside_queries` |
| Formatting is idempotent, reorders nothing and retypes no literal | the `cql/format.rs` tests |
| The bar greys `ctrl-f format` on a buffer already laid out | `nothing_but_a_hint_that_would_do_nothing_is_ever_inert` (`view/hints.rs`) |
| The buffer reaches the renderer painted by what its words mean | `the_buffer_reaches_the_renderer_painted`, the `cql/mod.rs` tests, `the_buffer_is_painted_by_what_its_words_mean` (`inbound/render.rs`) |
| A prefix offers the keyword it starts, and a qualified one that keyspace's tables | `typing_a_prefix_offers_the_keyword_it_starts`, `a_qualified_prefix_offers_that_keyspaces_tables`, `an_unqualified_table_comes_from_the_keyspace_the_tree_is_on` |
| `Tab` accepts a completion, and still moves focus without one | `tab_accepts_what_is_offered_rather_than_moving_focus`, `tab_still_moves_focus_when_nothing_is_being_offered` |
| `⏎` accepts a completion, and still splits the line without one | `enter_accepts_what_is_offered_and_splits_the_line_without_a_popup` (`queries.rs`) |
| The popup leads with the names the position takes, and follows them with the grammar that position can hold | `a_select_list_leads_with_the_columns_even_where_it_names_no_table`, `a_select_list_over_one_table_offers_that_tables_columns_first` (`cql/complete.rs`) |
| A column position offers no keyword it could not take; a value position offers every one | `a_column_position_offers_no_keyword_it_could_not_take`, `a_value_position_still_offers_the_whole_grammar`, `a_value_position_offers_grammar_and_no_names` (`cql/complete.rs`) |
| A statement shape is offered where a statement begins, and typed whole | `a_statement_start_offers_the_shapes_its_prefix_opens`, `accepting_a_shape_types_it_whole_and_lands_on_its_first_hole` (`cql/complete.rs`) |
| A keyword lands with a space, a type without one, and a space already there is not doubled | `accepting_a_keyword_writes_the_space_the_word_after_it_needs`, `accepting_a_type_writes_no_space`, `a_keyword_accepted_before_a_space_does_not_write_a_second_one` (`cql/complete.rs`) |
| Accepting a qualified column writes the `FROM` that puts it in scope, and never a second one | `accepting_a_column_with_no_table_in_scope_writes_the_clause_that_puts_it_in_scope`, `a_column_accepted_where_a_from_already_stands_writes_no_second_one` (`cql/complete.rs`) |
| `Esc` takes the popup down before it leaves the editor | `escape_takes_the_popup_down_before_it_leaves_the_editor` |
| A motion takes the popup down | `a_motion_takes_the_popup_down` |
| `ctrl-n` raises a popup and then walks it | `ctrl_n_raises_a_popup_and_then_walks_it` |
| `↑`/`↓` walk the popup while one is up, and move the cursor without one | `the_arrows_walk_the_popup_while_one_is_up`, `the_arrows_walk_the_popup_rather_than_the_buffer`, `walking_the_popup_leaves_the_cursor_where_it_is`, `an_arrow_with_no_popup_up_is_a_motion_again` (`queries.rs`) |
| The popup hangs off the cursor, and only for a focused editor | `the_completion_popup_hangs_off_the_cursor`, `an_unfocused_editor_draws_no_popup` (`inbound/render.rs`) |
| `ctrl-e` is inert while an export is already running | `ctrl_e_is_inert_while_an_export_is_already_running` |
| `ctrl-e` is inert where the frame cannot draw the form, and a frame that shrinks past it closes it | `ctrl_e_is_inert_where_the_frame_cannot_draw_the_form`, `a_frame_that_shrank_past_the_form_closes_it`, `fits_agrees_with_what_the_form_actually_draws` |
| A second `ctrl-e`, and the picker's own opener, leave an open form alone | `a_second_ctrl_e_leaves_the_open_form_as_it_was`, `the_theme_picker_cannot_replace_an_open_export_form` |
| `Tab`/`Shift-Tab` walk the form's own stops, not the panels | `tab_moves_between_the_forms_stops_rather_than_between_panels` |
| `Space` toggles the focused radio, and types a space only on the path stop | `space_toggles_the_focused_radio`, `space_on_the_path_stop_types_a_space` |
| `Enter` runs the export only from the Export stop, and is inert on a radio group | `enter_on_the_export_stop_runs_it`, `enter_on_a_radio_group_does_not_run_the_export` |
| A printable key is text only on the path stop | `a_printable_key_types_itself_only_on_the_path_stop` |
| A path too long for the field is drawn around its cursor | `a_path_too_long_for_the_field_is_drawn_around_its_cursor`, `a_window_holds_the_end_of_a_line_the_cursor_is_at_the_end_of`, `a_window_follows_the_cursor_back_into_the_middle_of_a_line` |
| `q` does not quit while the form is up | `q_does_not_quit_while_the_form_is_up` |
| `Esc` closes the form and writes nothing | `esc_closes_the_form_and_writes_nothing` |
| An existing destination reopens the form asking to overwrite, putting back whatever it replaces | `an_existing_file_reopens_the_form_asking_to_overwrite`, `an_existing_file_landing_on_an_open_picker_puts_the_theme_back` |
| Confirming re-emits the export with `overwrite: true`, and re-emits what was attempted rather than what the panel now holds | `confirming_re_emits_the_export_with_overwrite_set`, `confirming_re_emits_what_was_attempted_not_what_the_panel_now_holds`, `confirming_still_exports_when_the_panel_has_moved_on_entirely` |
| `Esc` from the confirmation goes back to the form intact | `esc_from_the_confirmation_goes_back_to_the_form_intact` |
| `y` yanks the focused row, per panel that has one | `y_yanks_a_grid_row`, `y_yanks_a_column_row`, `y_yanks_a_ddl_row`, `y_yanks_a_tree_node` |
| `y` follows the selection mode | `y_yanks_the_cell_in_cell_mode`, `y_yanks_the_row_in_row_mode`, `y_yanks_the_column_in_column_mode` |
| A cell yanks its value, unescaped and untruncated | `a_cell_yanks_without_quoting`, `a_cell_yank_is_not_ellipsised` (`yank.rs`) |
| A null cell and an empty column yank nothing at all | `a_null_cell_yanks_nothing` (`yank.rs`), `y_on_a_null_cell_raises_no_effect`, `y_in_column_mode_on_a_page_with_no_rows_yanks_nothing` |
| A column yank carries every row, in order, with no heading | `a_column_yanks_every_row_in_order_and_no_heading`, `a_null_in_a_column_yanks_as_an_empty_field`, `a_column_field_is_escaped_the_way_a_row_field_is`, `a_row_too_short_contributes_an_empty_field` (`yank.rs`), `a_column_yank_carries_rows_the_viewport_hides` |
| A yank carries the columns the viewport hides | `a_yanked_row_carries_every_column_of_the_page` |
| A yank carries the value, not the drawn cell | `a_yanked_value_is_not_ellipsised` (`yank.rs`) |
| A null yanks as nothing, and the word does not | `a_null_yanks_as_an_empty_field`, `a_text_null_yanks_as_the_word` (`yank.rs`) |
| A column and a DDL row carry their own fields | `a_column_row_carries_the_part_it_plays`, `a_partition_column_yanks_without_an_order`, `a_property_and_an_identity_row_yank_their_two_fields`, `a_keyspace_yanks_its_name_and_a_table_its_qualified_one` (`yank.rs`) |
| Nothing under the cursor yanks nothing, and a search that matches nothing still yanks the row under the cursor | `y_on_an_empty_panel_yanks_nothing`, `y_still_yanks_the_row_under_the_cursor_when_the_search_matches_nothing` |
| `y` types itself in Queries, and yanks nothing | `y_is_text_in_the_editor` |
| The encoder quotes exactly what it must | `a_field_is_quoted_only_when_it_has_to_be`, `an_embedded_quote_is_doubled`, `a_line_ends_without_a_newline` (`yank.rs`) |
| `v` starts a range at the cursor, and motion extends it | `v_starts_a_range_at_the_cursor_and_motion_extends_it` |
| A second `v` freezes the range and the cursor walks out of it | `a_second_v_freezes_the_range_and_the_cursor_walks_out_of_it` |
| `Esc` freezes a live range before it clears one | `escape_freezes_a_live_range_before_it_clears_one` |
| `Esc` prefers the selection to the search | `escape_prefers_the_selection_to_the_search` |
| `v` in cell mode extends a rectangle | `v_in_cell_mode_extends_a_rectangle` |
| `m` clears the range it would otherwise reinterpret | `m_clears_the_range_it_would_reinterpret` |
| `Enter` is inert while more than one row is selected | `enter_is_inert_while_more_than_one_row_is_selected` |
| A multi-row selection leaves the DDL panel describing nothing | `a_multi_row_selection_leaves_the_panel_describing_nothing` (`view/details.rs`) |
| A click clears the range it lands beside | `a_click_clears_the_range_it_lands_beside` |
| A new subject clears the DDL range | `a_new_subject_clears_the_ddl_range` |
| A tab change clears the range | `a_tab_change_clears_the_range` (`ddl.rs`) |
| A search clears the range it would renumber | `a_search_clears_the_range_it_would_renumber` |
| `y` yanks every row of a range, one per line | `y_yanks_every_row_of_a_range_one_per_line` |
| `y` in cell mode yanks the block and nothing outside it | `y_in_cell_mode_yanks_the_block_and_nothing_outside_it` |
| `y` on a single cell still yanks it bare | `y_on_a_single_cell_still_yanks_it_bare` |
| Every row of a range is banded, and the cursor row reads apart from them | `every_row_of_a_range_is_banded_and_the_cursor_row_reads_apart_from_them` (`inbound/render.rs`) |
| `y` drops the range and sends each cursor back to its anchor | `y_drops_the_selection_and_sends_the_cursor_back_to_its_anchor`, `a_selection_grown_upwards_sends_the_cursor_back_down`, `a_frozen_range_still_knows_where_to_send_the_cursor`, `cell_mode_sends_both_cursors_home`, `the_tree_collapses_its_selection_on_y_too` |
| A frozen group keeps the end it grew from, pulled back the same way on a clamp | `clamping_pulls_an_extending_anchor_back_into_the_list`, `clamping_a_frozen_group_pulls_its_surviving_anchor_back_too` (`state.rs`) |
| A `y` that yanks nothing resets nothing and lights nothing | `a_y_that_yanks_nothing_leaves_the_selection_alone`, `a_y_that_yanks_nothing_lights_nothing` |
| `y` lights what it took, on the axes its mode bounds | `y_lights_up_what_it_yanked`, `row_mode_lights_the_whole_row_and_column_mode_the_whole_column`, `a_bare_y_lights_the_row_it_took` |
| The flash fades, costs one repaint doing it, and the next key ends it | `the_flash_fades_and_only_the_tick_that_ends_it_costs_a_repaint`, `the_next_key_ends_the_flash` |
| The flash inverts the rows it took, and reaches one panel | `a_yanks_afterglow_inverts_the_rows_it_took`, `a_yank_in_the_tree_lights_the_tree_alone` (`inbound/render.rs`) |
| The bar names `esc` while a range stands, and leaves it off a panel with neither a range nor a search | `the_bar_names_esc_while_a_range_stands`, `the_bar_leaves_esc_off_a_panel_with_neither_a_range_nor_a_search` (`view/hints.rs`) |
| `v` on an unheld row opens a second group beside what is frozen, and `Esc` takes both at once | `v_builds_two_groups_and_esc_takes_both` |
| `v` on a held row drops that group and no other | `v_on_a_selected_row_drops_that_group_only`, `v_freezes_then_opens_a_second_group_then_drops_one` (`state.rs`) |
| Dropping a group takes the anchor with it only when the anchor was inside it | `dropping_the_group_holding_the_anchor_takes_the_anchor`, `dropping_another_group_keeps_the_anchor`, `clamping_away_the_group_holding_the_anchor_takes_the_anchor` (`state.rs`) |
| `y` over several groups yanks their rows in the order they are drawn, with nothing between the groups, and sends the cursor home to the last `v` | `y_over_two_groups_yanks_their_rows_in_drawn_order`, `y_sends_the_cursor_home_to_the_last_v_and_spends_every_group`, `a_single_cell_in_one_group_each_way_still_yanks_bare` |
| Switching panels clears the selection of the panel left behind, by every route, and re-focusing the same panel does not | `switching_panels_clears_the_selection_of_the_panel_left_behind`, `re_focusing_the_focused_panel_keeps_its_selection`, `leaving_contents_clears_both_of_its_axes`, `a_click_clears_the_selection_of_the_panel_it_leaves`, `enter_on_a_table_leaves_no_selection_behind_in_tables`, `esc_out_of_queries_clears_what_queries_left_behind` |

`a_number_focuses_its_panel_from_any_panel_that_does_not_take_text` is what
makes the match-order rule in §2 enforceable: a panel claiming a key an earlier
arm already takes has its arm swallowed, focus does not land, and the test
fails. Its companion, `a_number_is_text_in_a_panel_that_takes_text`, pins the
other half: from Queries, a panel number is text and no focus change happens at
all.

---

## 6. Adding one

1. Check §3. Take a free key.
2. Write the spec if it changes a rule in §2; otherwise put the reasoning in the
   slice's design doc.
3. Add the arm to `handle_key`, above the panel-focus arm only if it must
   outrank a panel number — and say why in the commit.
4. Pin it with a test, and add the row to §5.
5. Make it visible. A panel binding declares itself in `Panel::hints()`; a
   global one goes in `GLOBAL`. If it belongs in neither, say in the commit
   why it should stay hidden.
6. Update §1 and §3 here in the same commit.

---

## 7. The pointer

Bound, and spending nothing from §3's budget. See the
[mouse input design](specs/2026-08-09-mouse-input-design.md).

| Gesture | Lands on | Action |
|---|---|---|
| Left click | A keyspace | Focus Tables, select it, fold/unfold |
| Left click | A table | Select it, read it into Contents, and focus **Contents** |
| Left click | A Contents cell | Focus Contents, select its row and its column |
| Left click | A Contents heading | Focus Contents, select that column, leave the row where it is |
| Left click | A DDL row | Focus DDL, select it |
| Left click | A tab on a panel's top border | Whatever that tab's key does: focus the panel, and draw the tab |
| Left click | Chrome | Focus that panel |
| Left click | Anywhere, overlay up | Dismiss the overlay |
| Left click | The hint bar | Nothing |
| Wheel | The panel under the pointer | Scrolls three rows or lines, moving neither focus nor the selection |

**A click on an open search prompt cancels it**, restoring the search and
the cursor that stood before the prompt opened — the same thing `Esc` does
to it, and for the same reason: a click that lands somewhere unexpected must
not leave state behind (panel search design §3.2; search highlight design
§5.2).

**Queries is the exception: the wheel over it does nothing.** `panel_scroll`
has no arm for `Panel::Queries` — the editor derives both offsets from the
cursor and the area every frame and stores none for a notch to move. Whether
it should get a stored viewport of its own is an open question (TODO.md).

A table is the one target whose click leaves focus somewhere other than where
the press landed, and it does that because the key does: `Enter` and the click
reach the same state — the rule stated below, held rather than
bent.

A click on a row's number is a click on that row, and names no column: the bar
and the gutter share one widget column that is not a column of the page
([table display design](specs/2026-08-10-table-display-design.md) §8), so a
press there moves the row cursor and leaves the column cursor alone.

**A click never changes the selection mode.** It names whatever cells it can
reach — a cell names both cursors, a heading only the column — and which of them
the grid draws a band around stays `m`'s business. So a press in row mode still
moves the column cursor under the highlight, and the next `m` finds it where the
pointer left it.

Chrome is a border, a notice, the Queries panel, a title away from any tab, the
blank over a grid's gutter, and the blank space past the last row or column. In
the DDL panel it is also the blanks between groups and the section headings: the
body wraps one row over several lines, and those lines belong to no row. A row
number is not chrome — a row number is part of its row. Every other gesture —
right and middle buttons, drags, releases, horizontal wheels, and anything with a
modifier held — is dropped at the edge.

**The hint bar wins over the overlay.** A press on the bar is dropped before
anything else sees it, including a dismissible overlay, so it never dismisses
one — `Esc` always does. A long message can push the overlay's box down far
enough to cover the bar row too; a press on what looks like the overlay's own
bottom edge is this same case, not a bug.

**Every gesture reaches the same state a key does — with one stated
exception.** A click on a tree row is a selection followed by `Enter`. This is
what keeps §2's "a binding must be visible" true: the hint bar names keys, so a
capability reachable only by mouse would have nowhere to announce itself.

**The wheel is that exception, and it is deliberate.** A notch moves a viewport
and nothing else, and no key does that — `j` and `k` move the selection and let
the view follow. So the wheel is the one gesture that reaches a state the
keyboard cannot. The rule it still honours is the one that matters: *nothing
the user acts on* is reachable only by mouse. The selection, which every
command operates against, is moved only by things the bar names; what the wheel
moves alone is which rows you are looking at, and the panel shows that in the
scrollbar rather than in the bar
([list viewport design](specs/2026-08-09-list-viewport-design.md) §1).

Any further gesture is measured against the narrower rule, not this exception.

**The wheel scrolls; it does not select.** Tables, Contents and DDL each
hold a viewport of their own, so a notch moves the rows (or, in DDL,
the lines) and leaves the selection — the cursor, in DDL — where you put
it. Scroll far enough and the selected row is simply not on screen, and the
next `j`, or a resize, brings the view back to it.

**One dispatch point, again.** `handle_pointer` in
[`update.rs`](../crates/lc-domain/src/update.rs) is to a gesture what
`handle_key` is to a key. `lc-inbound` resolves the coordinate against the frame
it happened in and decides nothing.

**Capture is on from startup**, which costs click-drag text selection;
shift-drag is the bypass on terminals that offer one, and a shifted press is
refused for exactly that reason. There is no toggle and no flag until there is
a config system to hold the preference.
