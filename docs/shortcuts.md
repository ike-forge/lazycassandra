# Shortcuts

**Status:** current. This documents what is bound today and the rules a new
binding has to satisfy — not a wishlist.

Every keystroke in LazyCassandra passes through one function: `handle_key` in
[`crates/lc-domain/src/update/keys.rs`](../crates/lc-domain/src/update/keys.rs). There is
no second key table, no per-panel handler, and no binding in the renderer. If a
key does something, it does it there.

---

## 1. What is bound

### Global

Reachable from every panel that does not take text, in every connection
state. `Esc`, `Tab` and `Shift-Tab` are the exceptions that really are
unconditional; the marked rows are text instead, in the one place that takes
text — Queries' Cql buffer once `4`, `⏎` or `i` has opened it, argued in that
subsection below. `ctrl-t` and `ctrl-o` are unmarked because a chord is not
text anywhere: the ctrl layer is dispatched above the panel that takes text
(§2).

| Key | Action |
|---|---|
| `q`¹ | Quit. Cancels every in-flight query. |
| `?`¹ | Open the command palette: every binding, in one scrollable block. |
| `ctrl-t` | Open the theme picker: every theme, previewed as you move. |
| `ctrl-o` | Open the connection picker: every `[[connections]]` a config defines. |
| `ctrl-c` | Cancel every query in flight: the read, a page, a drop or a commit. The task is aborted and the panel says `cancelled`; a late answer is dropped. The cluster is not told, so a write may still land, and the drop or commit says so. |
| `/`¹ | Search the focused panel. |
| `n`¹ | Move to the next match of the focused panel's search, wrapping. |
| `N`¹ | Move to the previous match of the focused panel's search, wrapping. |
| `y`¹ ² | Yank the selection to the clipboard, as one line — in Table contents, the cell, the row or the column, per the mode. Flashes what it copied, then drops the selection and returns the cursor to where `v` was pressed. |
| `v`¹ | Start or freeze a selection of whole rows in the focused list panel. On a row already selected, drops the group holding it; on any other row, adds a group beside what is held. |
| `Esc` | Dismiss the overlay, stop a running `P`, close the open Cql buffer, leave visual mode, clear a range, clear the focused panel's search, or leave the panel for Tables — first hit wins (§2). |
| `Tab`³ | Focus the next `(panel, tab)` stop, wrapping: Tables; DDL, and its Columns and Indexes tabs where a table is selected; Table contents; Queries' Cql tab; Queries' Logs tab. |
| `Shift-Tab` | The same stops, walked backwards. |
| `1`¹ | Focus Tables. |
| `2`¹ | Focus DDL. Pressed again, its next tab. |
| `2`¹ | Focus Table columns — the same key, one tab further in. |
| `2`¹ | Focus Table indexes — the same key, one tab further still. |
| `3`¹ | Focus Table contents. |
| `4`¹ | Focus Queries with the Cql buffer open, ready to type. |
| `5`¹ | Focus Queries' Logs tab. |
| `]`¹ | The focused panel's next tab. |
| `[`¹ | Its previous tab. |

¹ Not reachable while the Cql buffer is open: it claims every printable key
before this table is consulted, so these type themselves there instead.
`Esc` closes the buffer and every one of them answers again. A Cql tab that
has not been opened, and the Logs tab, take no text at all.

² "One CSV line" holds for DDL's three tabs, and for two of Table contents'
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
| DDL, Indexes tab | The row's fields for its kind, as one CSV line — `status,text` for an indexed column, `class,SASIIndex` for a custom index's class |
| Tables | The node's bare name — `shop`, `shop.orders` — not a CSV line |
| Any panel, a selection held | Every selected row, in the order the rows are drawn — every column in column mode, every row of the rectangle in cell mode — one line each, joined with `\n`, no trailing newline and nothing between the groups. A single cell in cell mode still yanks bare. |

³ Except in Queries with a completion popup up, where it accepts the selected
candidate — as `⏎` does there too. The Queries subsection below is where that
is argued.

Each panel's title opens with the key that reaches it — `[1]-Tables`,
`[2]-DDL - Columns - Indexes`, `[3]-Table contents`, `[4]-Queries - [5]-Logs`
— so the
binding is learned by looking at the panel rather than by reading this file
(vision.md §4, principle 1). The number is drawn in the same style as the
panel's active tab: it names the panel, and so does the tab. A tab with a
number of its own, Logs' `[5]-`, carries it ahead of its name.

Moving focus — by `Tab`, `Shift-Tab`, a number, `Enter` on a table or a
click — drops the selection of the panel being left, so only the focused panel
ever holds one: a selection says what the next command acts on, and the next
command acts on what has focus.

`1` `2` `3` `4` run down the frame in the order the panels are drawn —
top-left, bottom-left, top-right, bottom-right — and `Tab` walks the same
order, now stopping at every tab a panel draws along the way: DDL's own tab
and, where a table is selected, its Columns and Indexes tabs beside it; then
Queries' Cql tab and its Logs tab. One order on screen, in the titles and under the
fingers. Four letters chosen for the words they sit inside used to do this
job; a letter has to be learned per panel, while four digits in frame order
are one rule, and a novice reading `[2]-` has been told the whole binding.

A number focuses its panel. Pressed again — that is, pressed while its panel
already has focus — it steps to the panel's next tab, wrapping: `2` `2`
reaches the Columns tab, a third `2` the Indexes tab and a fourth comes back;
`1` `1` stays on Tables. The palette says so directly for DDL: `2` takes a
row per tab, one under the other.

Queries is the exception. `4` arrives with the Cql buffer open, because a
newcomer pressing the number of the panel that takes queries expects to type
one; a buffer that needed `⏎` first read as broken. An open buffer takes the
next `4` as text, so it cannot also walk the strip, and Logs has a number of
its own instead: `5`, drawn in the title as `[5]-Logs`.
The general rule stays with `]`/`[`, but a reader who has not met
those keys yet learns from the repetition that the number pressed again goes
somewhere — which is how the tabs are found without reading this file
(vision.md §4, principle 1). `]` and `[` do the same walk forwards and
backwards from the focused panel, for a hand that learnt them in lazygit; on
a panel with one tab they do nothing, and in an open Cql buffer they are
text.

Arriving at the tab already drawn costs nothing: `2` from another panel is how
focus comes back to whichever DDL tab was up, and it leaves the cursor on the
row you were reading. Changing tab does move the cursor and the viewport to the
top — the tabs describe different rows, so a row index carried across addresses
nothing the user chose.

On a keyspace the DDL strip draws one tab, so walking stays on it. A table's
three no longer fit a sidebar of the usual width, where the strip draws them
short — `DDL - Cols - Idx` — rather than dropping the two not being read. A press that
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
| `Home` `g`, `End` `G` | The first command, the last command. |
| `⏎` | Close the palette and perform the selected command. |
| `?` `Esc` | Close it. |
| `q` | Quits, as everywhere else. |
| `/` | Filter the list: typing keeps the rows that match, marking the hit in each. `⏎` keeps the row and widens the list back out, `Esc` puts both back. |
| anything else | Inert. Nothing means "close" in one frame and something else in the next. |

`⏎` presses the row's own first key, in the panel the row belongs to — it
focuses that panel first, whether or not the action that follows has anything
to do. A row that means something only inside the export form is not pressed
at all: it is drawn faint, and `⏎` on it closes the palette and stops.

`/` searches the action text alone — never the key column, never a group
heading — and narrows the list to what matches: a group keeps the commands
that match and is dropped whole, heading and rule with it, when it keeps
none. The cursor still only ever lands on a command, the same
skip-the-heading rule `j`/`k` and paging already keep, and it lands on the
first surviving one at or beyond the row the prompt opened on. A term
matching nothing draws one faint line saying so, which `⏎` does nothing on.
The block itself is sized from the whole catalogue, so no keystroke moves a
border. The catalogue this table draws gained no row of its own for `/`: a
row saying `/` searches the palette would be a row about the palette inside
the palette, so the hint bar names the key instead — a mode names its own
keys.

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
holding what it was searched from. `ctrl-r` and `ctrl-g` act only from Queries,
and no prompt can be open while Queries has focus. That takes two halves:
Queries holds no search, so no prompt can be opened *over* it; and focus cannot
move *to* it under a live prompt, since the prompt swallows `Tab`/`Shift-Tab`,
types the panel numbers as text rather than acting on them, and a click is
spent dismissing the prompt (§6).

A chord the layer has no member for is still refused rather than typed: ctrl
is a layer of its own, and `ctrl-n` is not an `n`.

Searching is live: every edit re-applies the search, so the panel underneath
is the preview — Table DDL and Table contents highlight where a match sits
rather than narrowing to it, and neither drops a row for a term that fails to
match it. Tables narrows: a keyspace the term reaches neither by its own name
nor by one of its tables' is not a row, and one it does reach is drawn open
holding the tables the term reaches — all of them where it reaches the
keyspace by name. A term that reaches no keyspace leaves the panel
saying `No match`. `Enter` on an empty query therefore
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
| `Home` / `End`, `g` / `G` | The first theme, the last theme — previewing it, as every motion here does. |
| `Enter` | Closes, keeping whatever theme is set — and writes it to the config file it is read from, so the next launch opens on it. |
| `Esc` | Sets `State.theme` back to the theme the picker opened on, then closes. |
| `q` | Quits, as everywhere else. The picker takes motion, not text, so it leaves `q` alone — the palette's rule (§2). |
| `/` | Filter the list: typing keeps the themes that match, marking the hit and previewing the row it seats on. `⏎` keeps the theme and widens the list back out, `Esc` puts both back. |

Search here previews the same way motion already does: the cursor is what
sets `State.theme`, so the row a filter seats it on is the theme in force.
The list narrows to what matches, in a block every theme still sizes, and a
term matching nothing draws one faint line saying so and leaves the preview
on whatever it last showed — reverting per keystroke would flash the
terminal's colours at a half-typed word, and `Esc` is what puts a preview
back. See §2's rule below for the key itself.

The ends are reachable here as they are in every other list. Paging does land
on either end in one press — the list is every theme there is and always
shorter than the block drawing it, so a screen *is* the whole list — and
`Home`/`End` and `g`/`G` were inert here for exactly that reason until the
vim spellings arrived. What changed the answer is that a hand which types `G` in the
panels and in the palette and gets nothing in the picker has found the one
list that is different, which costs more than the arm does.

Live preview is why the picker restores rather than merely reverts: a message
arriving over an open picker, or a frame shrinking below it, unwinds the
preview the same way `Esc` does, so a half-previewed theme never strands
itself.

The picker is behind ctrl because a shifted letter was not free to give it a
plain-key opener of its own: `T` was the one shifted letter ever bound and
has been given back, so `A`, `B`, `C`, `D` and `U` stay unbound (§2, §4). `N`
is the program's only shifted letter now, and it is spent walking a search
backwards (§1's global table), not opening the picker. `a`, `b`, `c`, `d`,
`t` and `u` are free too, since the panels moved to `1`–`4`.

### The connection picker

`ctrl-o` opens `Overlay::Connections`, an `Overlay` of the same rank as the
theme picker: it outranks focus, claims every key, is refused on its own gate
— its block is wider, so a frame that fits one picker need not fit the other
— and is refused over a live search prompt for the same reason `ctrl-t` is.
The palette carries a row for it.

It also opens **by itself** at startup, whenever a config defines connections
but names no `default_connection`, or defines none at all. That is the only
overlay that opens unasked, and it is why: nothing is connected to unless a
file named it, so without the picker a fresh checkout would draw four empty
panels and no explanation. A config that names a default skips it and connects.

| Key | Effect |
|---|---|
| `j` / `k`, `Down` / `Up` | Moves the cursor. Previews nothing — opening a connection is a round trip, not a repaint. |
| `PageDown` / `PageUp`, `ctrl-d` / `ctrl-u` | The same, by a screen. |
| `Home` / `End`, `g` / `G` | The first connection, the last. |
| `Enter` | Opens the connection under the cursor, and closes. |
| `Esc` | Closes, leaving the tool connected to whatever it already was — at startup, nothing. |
| `q` | Quits, as everywhere else. The picker takes motion, not text (§2). |
| `/` | Filter the list by name. `⏎` closes the prompt onto its row, `Esc` puts the cursor back. |

The two `⏎`s are deliberate. The first closes a filter onto the row the cursor
holds; the second opens it. A term typed to *find* an entry should not also
connect to it, because the row the cursor happens to hold may not be the one
the user was still narrowing towards — and unlike the theme picker, landing on
the wrong row here costs a connection rather than a repaint. A term matching
nothing draws one faint line saying so, opens nothing, and puts the cursor back
where the prompt started, so the next `⏎` cannot open an entry the filter had
hidden.

`Esc` leaving the picker closed is what `ctrl-o` exists for: dismissing it is a
choice, and nothing re-offers it, so without a way back a dismissed picker
would be final and a connection opened by mistake would cost a restart.

### The query history

`ctrl-g` opens `Overlay::History` from Queries, whichever of the Cql tab's
modes it is in. An `Overlay` of the same rank as the pickers: it outranks
focus, claims every key, is refused on its own gate and over a live search
prompt. It lists what the connection in use has been asked from the buffer —
`ctrl-r` only, never a table read from the tree — newest first, each
statement as one line, a repeat of the newest statement, bar keyword and
name case, replacing it instead of appearing twice. At most `history.limit` are kept per connection (default 100; 0 turns
the history off, and `ctrl-g` then answers with a "history is disabled" toast).
The palette carries a row for it.

Kept between runs in `$XDG_STATE_HOME/lazycassandra/history.toml` (default
`~/.local/state`), keyed by connection name. Statements can hold credentials,
so the file is `0600`.

| Key | Effect |
|---|---|
| `j` / `k`, `Down` / `Up` | Moves the cursor. Previews nothing. |
| `PageDown` / `PageUp`, `ctrl-d` / `ctrl-u` | The same, by a screen. |
| `Home` / `End`, `g` / `G` | The newest entry, the oldest. |
| `Enter` | Appends the entry under the cursor to the buffer, after a blank line, and closes. An empty buffer takes it alone. Runs nothing: `ctrl-r` is the one key that sends a statement. |
| `Esc` | Closes, leaving the buffer as it was. |
| `q` | Quits, as everywhere else. The list takes motion, not text. |
| `/` | Filter the list. `⏎` closes the prompt onto its row without loading it, `Esc` puts the cursor back — the connection picker's two `⏎`s, for its reason. |

The block scrolls, where the pickers refuse a list that does not fit: a
hundred rows is far more than a frame holds.

`ctrl-g` is the history's chord because `ctrl-r`, the CLI's reverse search,
already runs a statement here, and a chord is never a synonym for another
(§2). Of the free ctrl letters, `ctrl-h` is Backspace on terminals that cannot
tell the two apart.

### The health block

`H` opens `Overlay::Health` over the table under the Tables cursor, and sends
one read of every node's view of it. An `Overlay` of the inspector's size and
rank. Inert on a keyspace, over a range of several rows, before a schema has
loaded, and on a frame with no room for the block; the bar greys it there. `h`
was the obvious letter and is motion (§2); a shifted letter is a binding of its
own.

| Key | Effect |
|---|---|
| `j` / `k`, `Down` / `Up` | Moves the cursor. |
| `PageDown` / `PageUp`, `ctrl-d` / `ctrl-u` | The same, by a screen. |
| `Home` / `End`, `g` / `G` | The first row, the last. |
| `l` / `Right` | Unfolds the row; on an open one, steps onto the first row under it. |
| `h` / `Left` | Folds the row; on a folded or leaf row, steps out to its parent. |
| `Enter` | Folds or unfolds the row. Inert on a leaf. |
| `Esc` | Closes. A read still out lands in the Logs trail and nowhere else. |
| `q` | Quits, as everywhere else. |
| Left click on a row | Selects it, then `Enter`. Elsewhere in the block, nothing; outside it, closes. |

### Toasts

A toast is a three-second line in the top-right corner. It asks nothing and
takes no key. Successes toast; failures stay modal, because they have to be
read.

| Toast | When |
|---|---|
| `connected to <name>` | The connection opens. |
| `exporting to <path>`, `exported N rows to <path>` | An export starts, and finishes. |
| `copying to the clipboard`, `copied N rows to the clipboard` | A copy from the export form starts, and finishes. |
| `dropped …`, `deleted …`, `committed N cells to …` | The cluster accepted the statement. |
| `edit staged, close the inspector and ctrl-s to commit` | An inspector edit is accepted and leaves a mark. |
| `stopped fetching pages` | `Esc` ends a running `P`. |
| `cancelled`, `nothing is running` | `ctrl-c` aborted what was in flight, or found nothing. |
| `history not saved, see the logs` | The history file could not be written. Also a trail line. |
| `nothing to commit`, `nothing to export`, `nothing in the history yet`, `history is disabled` | A bound chord with nothing to act on. |
| `a drop is already running`, `a commit is already running`, `an export is already running` | The chord's job is still outstanding. |
| `no room to draw …` | The frame cannot draw the block the chord opens. |
| `drop takes one node, not a selection`, `these rows cannot be addressed by primary key` | `d` where the question cannot be asked. |

A new toast replaces the one standing. A failed query toasts nothing: the
Contents panel shows it in place of the buffer on purpose.

### The configuration diagnostics overlay

`Overlay::Diagnostics` opens itself, once, at startup — there is no key that
opens it, because there is nothing to preview: it exists only to say what a
config file got wrong before the first frame is drawn. If neither config
file has a problem it never appears at all.

| Key | Effect |
|---|---|
| `j` / `k`, `Down` / `Up` | Moves the cursor. |
| `PageDown` / `PageUp`, `ctrl-d` / `ctrl-u` | The same, by a screen. |
| `Home` / `End`, `g` / `G` | To the first / last diagnostic. |
| `Esc` | Closes it. |

Unlike the palette, the prompt and the theme picker, `q` does nothing here
rather than quitting: a diagnostics list is read once and dismissed, and the
one key a user reaches for on a screen full of text they did not ask to read
is `Esc`, not `q`.

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
| `Tab` / `Shift-Tab` | Move to the next / previous of the form's stops — path, format, scope, the first-N count, Export to file, Copy to clipboard — wrapping; the count is reachable only under the first-N scope. The form opens on the path, and the scope stop is skipped where both scopes would write the same file (the form says so in a sentence instead of offering a radio). |
| `Space` | Activates the focused stop: on a radio group, advances to the next option, wrapping (scope: visible rows, whole table, first N rows); on a button, runs it. |
| `Enter` | Runs the focused button, and only from a button. On the Scope or Format radio it does nothing — a radio group is not a button, and a keystroke landing there must not write a file. |
| `ctrl-s` / `ctrl-y` | Export to file / copy to clipboard, from any stop — the path's included, which takes every printable key as text, so the buttons' keys sit behind ctrl. |
| `j` / `k`, `Down` / `Up` | Move within a radio group — the second way to reach what `Space` also reaches there. |
| any printable key (digits only on the count), `Backspace` / `Delete`, `←` / `→`, `Home` / `End` | Text, and only on the path stop — the panel-that-takes-text rule, applied inside an overlay (§2). `Space` types a space there rather than activating anything, and `Enter` does nothing there either. |
| `Esc` | Closes the form and writes nothing. |

The bar names what the stop under `tab` answers to: `tab next option` always,
`space toggle` on a radio group, nothing more on the path stop, where `space` is text, and
`space/⏎ export` and `space/⏎ copy` on the two buttons. `esc close` follows.
The buttons are drawn as every confirmation's are, right-aligned on the last
row with their key highlighted — `ctrl-s Export to file`, `ctrl-y Copy to
clipboard` — and the one under `tab` is filled solid. The overwrite
confirmation's are `⏎ Yes, overwrite` and `esc Back`, and its bar is
`space/⏎ overwrite` and `esc back`.

`ctrl-s` and `ctrl-y` are answered while the form is up and its overwrite
question is not; `ctrl-d`/`ctrl-u` are too, dispatched above the overlay guard
the way they are everywhere (§1's motion table), and they page nothing here:
the form has no list long enough to page. Every other ctrl chord is refused
while the form is up, the same refusal the palette and the picker already get.

Copy to clipboard takes the format and scope and ignores the path. It holds the
whole result in memory, so it is capped at `export.clipboard_mb` (10 MB by
default, 1024-based). The first chunk past the cap fails the copy and stops the
fetch: nothing reaches the clipboard and `Copy failed` names the cap. A partial
copy is never made, since a truncated CSV pastes as if it were whole.

Failing to open the destination with `ExportError::Exists` raises a
confirmation over the form rather than a sixth overlay variant:

| Key | Effect |
|---|---|
| `Space` / `Enter` | Confirms, and re-emits the export with `overwrite: true`. |
| `Esc` | Returns to the form, every field exactly as it was. |

### The drop confirmation

`d` opens `Overlay::Drop` over whatever the focused panel's cursor names —
a keyspace or a table on Tables, rows on Table contents — when the connection
permits mutation, the frame is big enough to draw the block, and nothing is
already running. Two panels for one key, gated on focus, exactly as `ctrl-e`
is. A frame that shrinks past the block closes it, the same rule the export
form follows.

The bar leaves `d` off on a connection whose posture refuses it, and greys it
wherever else the key would open nothing, reading the very derivation the key
reads — so the bar cannot promise a question `d` refuses.

Like the form it outranks focus and claims every key before the panel sees
one, `q` included. The block names the statement it would run, because every
action this program takes is shown as CQL first (vision.md §4, principle 3),
and for rows it names the count as well — in the border and again under the
statement.

| Focus | Cursor on | It runs |
|---|---|---|
| Tables | a keyspace | `DROP KEYSPACE` |
| Tables | a table | `DROP TABLE` |
| Table contents | its rows | `DELETE`, or one `BEGIN BATCH` of them |

On the tree `d` reads the **cursor** and is refused over a range of more than
one row: `v` selects rows for `y`, and "drop these four keyspaces" is a
different feature with a different confirmation. On the grid `d` reads the
**range**, because there `v` already selects the rows a command acts on and
`y` reads it the same way. Whole rows in every selection mode — a column
range addresses no row, and a cell is not a thing `DELETE` removes.

Over the grid it is refused unless the rows can be *addressed*: the panel is
showing a table rather than a typed statement, that table is in the loaded
schema, every partition and clustering column is among the page's columns,
and every one of those values is one this program can spell as a CQL literal.
A `DELETE` restricts the whole primary key or it deletes something else.

On a connection whose posture permits no mutation `d` opens nothing at all and
says so, rather than being one of the silent refusals above: the bar leaves it
off, the palette marks its rows `(disabled, read-only)`, and pressing it raises
the read-only modal.

What the block asks depends on what is being taken away and on the
connection's posture. A keyspace is always typed back — it takes every table
in it — and `Posture::Protected` raises a table to the same bar. A row delete
is a `Write` rather than a destructive statement, so that posture rule does
not reach it:

| Target | `Writable` | `Protected` |
|---|---|---|
| Rows | `y`/`n` | `y`/`n` |
| Table | `y`/`n` | the name typed back |
| Keyspace | the name typed back | the name typed back |

| Key | Effect, `y`/`n` | Effect, typed |
|---|---|---|
| `y` | Runs the drop. | Text. |
| `n` | Closes it, and runs nothing. | Text. |
| `Enter` | Inert. | Runs the drop, and only once the field equals the name asked for. The bar greys `⏎ drop it` until it does. |
| any printable key (digits only on the count), `Backspace` / `Delete`, `←` / `→`, `Home` / `End` | Inert. | Text and editing, the panel-that-takes-text rule applied inside an overlay (§2). |
| `Esc` | Closes it, and runs nothing. | The same. |
| anything else | Inert. Nothing means "close" in one frame and something else in the next. |

`n` is deliberately *not* a cancel on the typed spelling: a field that takes
text claims every printable key, and a keyspace named `nation` could not be
typed if it did. `Esc` is the way out of every text field in this program,
and the block names it rather than leaving it to be found.

The name is matched verbatim rather than case-folded. CQL folds an unquoted
identifier and `system_schema` does not, so a keyspace called `MyKs` is
confirmed by typing `MyKs` — what the tree draws is what the field asks for.

`ctrl-d`/`ctrl-u` reach the confirmation and page nothing there, and every
other ctrl chord is refused, exactly as with the export form.

Whatever went stale is re-read. A drop that succeeds re-issues
`Effect::LoadSchema`, so the tree stops naming what is gone — that is the
reload path §4 records `r` as waiting on. A delete re-submits the Contents
panel's own read, which resets its cursor, viewport and selection: the rows
the selection named are the rows that are gone. `r` is still unbound, because
a refresh key is a binding with its own argument to make.

### Motion

Applies to the focused panel and nothing else. What moves depends on the panel:
Tables, Contents and DDL move a selection and let the viewport follow; an open
Cql buffer moves the cursor through its text, and a closed one moves the band
from statement to statement — the Cql tab's rows are the statements in it.

| Key | Action |
|---|---|
| `j` / `↓` | Down one row (in an open Cql buffer, `↓` only — `j` is text; with a completion popup up, the next candidate). On a closed Cql tab, the next statement. |
| `k` / `↑` | Up one (in an open Cql buffer, `↑` only — `k` is text; with a popup up, the previous candidate). On a closed Cql tab, the previous statement. |
| `l` / `→` | Right one column in Contents — the cell cursor in cell and column mode, the window itself in row mode; in Tables, unfold the selected keyspace, step into an unfolded one, or read the selected table into Contents — as `Enter` does. |
| `h` / `←` | Left one column in Contents, the same way; fold it in Tables, stepping out from a table row first. |
| `PageDown` / `ctrl-d` | Down one panel height. |
| `PageUp` / `ctrl-u` | Up one panel height. |
| `Home`, `g` | To the start (to the **line**'s start in an open Cql buffer — §2; to the first statement on a closed one). |
| `End`, `G` | To the end (to the **line**'s end in an open Cql buffer — §2; to the last statement on a closed one). |

`g` and `G` are the vim-style spelling of `Home` and `End`, and reach the same
`Motion::Start`/`Motion::End` rather than a copy of them — the relation
`ctrl-d`/`ctrl-u` already have to `PageDown`/`PageUp` (§2). They apply to
whatever list the frame is driving: the focused panel, or the overlay in front
of it when that overlay takes motion — the command palette, the diagnostics
list, the theme picker and the inspector all answer them.

`g` is one press rather than vim's `gg`: no other binding starts with `g`, so
a prefix would only cost a keystroke and a pending state. In an open Cql buffer
and under a search prompt both keys are text, the panel-that-takes-text rule as
everywhere else; on a closed Cql tab they reach the first and last statement.

`ctrl-d`/`ctrl-u` are the vim spelling of the two paging keys and reach the
same code rather than a copy of it, overlays included: whatever `PageDown`
does in the frame you are in — page a panel, page the palette, page the
picker, page the editor, or nothing under a search prompt — `ctrl-d` does.

`End` reaches the last row everywhere it applies, DDL included — a list
has an end, not a document with room to scroll past its last line. Both are
the same `Motion::End`, resolved by the focused panel. Table
DDL is the one place paging is not exact: `PageDown`/`PageUp` move the cursor
by the panel's height counted in wrapped **lines**, while the cursor itself
counts **rows**, so a page overshoots on a run of wrapped values. `Home` and
`End` stay exact regardless — landing on index zero or the last row needs no
counting in either unit. The fix needs a line-to-row map from the renderer,
and we accept the imprecision rather than build one for a panel
that usually fits on one screen.
An open Cql buffer never reaches `Motion::End` at all: the editor claims
`Home`/`End` itself, before the match that would call it. A closed one resolves every motion but the two paging ones against its
statements, and leaves paging inert — the buffer's viewport follows its cursor
rather than being stored, so a statement step already brings its statement on
screen.

The Contents grid has two axes, and only `h`/`l`/`←`/`→` reach the second one.
`Home`, `End`, `PageUp` and `PageDown` move the **row** cursor there, as they do
everywhere else; the column cursor moves one column at a time or not at all, and
§4 says why.
The column offset follows the cell the way the row offset follows the row, and
the panel's bottom border grows a scrollbar exactly when a column is off screen.

Row mode is the one place the pair works the other way round. It paints no
column, so a press that only moved the cursor between two drawn columns would
be a key that visibly did nothing — ten presses on a grid drawing ten columns
before the grid first moved. There the four keys move the **window** by a
column instead and the invisible cursor rides along on the column just
revealed, so every press scrolls and a press against either edge of the page
moves nothing.

### Visual mode

`v` starts a selection from the cursor: press it in Tables, DDL or Table
contents and every row from there to wherever the cursor moves next is in
range, drawn as a band rather than picked one at a time. A second `v`
freezes what is extending — the range stops following the cursor, which is
then free to walk off it without losing what was marked.

`v` reads as one verb with three arms, decided by where the cursor is when it
is pressed: extending a group freezes it, as above; a row no group holds opens
a new group there and starts extending it, beside whatever is already frozen;
a row a group already holds drops that group. There is no way to restart a
selection from scratch with `v` alone — `Esc` first is the route, since it is
already the key that means *drop what is held*.

`Esc` is a chain of five rungs now rather than three, first hit wins: an
overlay dismisses; failing that, an extending range freezes exactly the way
a second `v` would, so a reflex `esc` never discards a range still growing;
failing that, the selection clears — every group at once, however many there
are, rather than one at a time; failing that, the focused panel's search
clears; failing that, the panel hands focus back to Tables. §2 restates this as
the rule it now is.

In Table contents, which axis `v` extends is `m`'s to say: cell mode grows
a rectangle on both cursors at once, row mode a run of rows, column mode a
run of columns — the same three shapes the panel already draws.
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
did. §1's `y` footnote table carries the row-by-row shape.

### Tables panel

Accepted anywhere, but a no-op unless Tables has focus — except in Table
contents, where `Enter` opens the inspector, and in Queries, which takes text:
there `Enter` inserts a newline, or accepts a completion while one is being
offered. See those subsections below.

| Key | Action |
|---|---|
| `Enter` | On a keyspace: fold/unfold. On a table: read it into Contents and focus that panel. |
| `H` | Open the health block over the table under the cursor. Inert on a keyspace, over a range of more than one row, and on a frame with no room to draw it. See the health block's own subsection in §1. |
| `d` | Open the drop confirmation over the node under the cursor. Inert on a connection that is not writable, on a frame with no room to draw the block, while a drop is already running, and over a range of more than one row. The same key answers Table contents, aimed at its rows. See the drop confirmation's own subsection in §1. |

`Space` used to be `Enter`'s synonym here and is now free (§3): one key for one
action, and a second name for a key everyone already knows buys nothing.

`l`/`h` and `→`/`←` are motion, not tree keys, and the Motion table above is
where they live: unfolding is what *move right* means in a tree, and the same
key means the next column in a grid — §2.

On a table row `l` is `⏎`: the tree has no level below a table, so *move
right* out of one is the panel to its right, and a hand walking `l` `l` `l`
lands in Contents rather than on a key that does nothing. The two are not
synonyms above that row — on an open keyspace `⏎` folds where `l` steps in —
and `l` inherits `⏎`'s two conditions on the read itself: a selection of more
than one row makes it inert, and a search swallows only the fold path, never
the read.

Reading a table cancels whatever read Contents was waiting on, and moves focus
to Contents immediately rather than when the page lands: focus is a consequence
of the keypress, not of the network. A refused
read moves it too — the panel is where the refusal is narrated. Returning is
`Tab` or `t`, and the tree selection is where you left it.

`l` and `h` on a keyspace are swallowed while a search is applied — and so
are `Enter` and a click, on a keyspace row, for the same reason. A
keyspace holding a match is forced open by the search itself, so folding it
would change nothing on screen — the projection puts it right back — and a
key that looks broken is worse than a key that does nothing. This is the existing rule that a panel which cannot honour a motion
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
units and can therefore disagree. What is left to move
the viewport alone is the wheel (§6), which is a look somewhere else rather
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
| `Enter` | Open the inspector over the row under the cursor in row mode, or the cell under it in cell mode. Inert in column mode, with nothing loaded, on an empty grid, while a range is held, and on a frame with no room to draw the block. See the inspector's own subsection below. |
| `e` | In cell mode, `Enter`, then `e` in the inspector, in one press: opens the inspector and an edit field over the value, refused the way the inspector's `e` is. In row mode no field opens: the inspector's cursor rests on the row's first editable column, not its first, and `e` there picks the value. Opens nothing where `Enter` opens nothing. |
| `m` | Cycle the selection mode: row → column → cell, wrapping. |
| `p` | Fetch the next page of the result and append it. Inert once the result has ended and while a page is in flight. |
| `P` | Fetch pages until the result ends or a stop is reached. Inert where `p` is. |
| `ctrl-e` | Open the export form. Inert on an empty grid, while an export is running, and on a frame with no room to draw the form. |
| `d` | Open the delete confirmation over the selected rows, or the row under the cursor when no range is held. Inert unless those rows can be addressed by their whole primary key. See the drop confirmation's own subsection in §1. |
| `i` | Open the inspector over a new row: every column of the table, unset. See the insert form's own subsection below. Inert on a frame with no room, refused with a message on a typed statement's grid, and refused with the posture modal on a read-only connection. |
| `ctrl-s` | Open the commit confirmation over every uncommitted edit. Inert with nothing edited, while a commit is in flight, and on a frame with no room to draw the block. Refused with a message on a read-only connection. See the commit confirmation's own subsection below. |

The panel opens in row mode, which lights the whole row: reading a table is
reading rows. Cell mode draws the selected cell over a faint wash across its
row and its column, and column mode lights the column and its heading. Whichever mode is up, `j`/`k`
still move the row cursor and `h`/`l` still go a column left or right exactly
as the Motion table above says — the mode changes what is painted, and, for
`h`/`l` in row mode alone, whether the column that moves is the cursor's or
the window's.

Column mode binds three keys of its own, and they are bound *behind the mode*:
each addresses the column the cursor is on, and column mode is the one mode
whose cursor is a column. In row and cell mode the three are unbound letters,
and the bar names them only where they answer.

| Key | Action (column mode only) |
|---|---|
| `s` | Sort the fetched rows by this column: descending, then ascending, then the order the cluster returned. One column at a time. |
| `f` | Open the filter prompt over this column, holding whatever filter already stands there. |
| `c` | Clear this column's filter. Greyed until there is one. |

Every heading carries two marks — a sort arrow and a funnel — drawn faint
while idle and in the theme's focus colour while in force, so what a column is
doing is on screen. Both are clickable (§6). Sorting and filtering reach the
rows already fetched and never re-run the statement; a filter hides rows, and
the panel title says how much of the page is left.

A read fetches `grid.page_rows` rows (200 by default); nothing more arrives
until `p` or `P` asks, and reaching the last row fetches nothing. `P` stops at
the next 100 MB of estimated row memory or the next million rows above where
it was pressed, whichever comes first; pressed again it continues to the next
multiple. `p` ignores both stops. `Esc` stops a running `P`: the page in flight
lands and no further one is asked for. The bottom border counts the rows
fetched and says whether there is more, a page is loading or `P` paused past a stop, with
the estimate from 10 MB up; every size is 1024-based. Export streams and is bound by neither stop.

### The filter prompt

`f` opens `Overlay::Filter` over a loaded grid in column mode. It is a modal
over the panels rather than a prompt on the bar row, because it does something
`/` does not: `/` marks matching cells and keeps every row, and this drops the
rows it does not match. Like the export form it claims every key before the
panel underneath sees one, `q` included.

| Key | Effect |
|---|---|
| any printable key | Appends to the term. Nothing is applied until the prompt closes. |
| `Backspace` | Removes the last character. Emptying the term is how a filter is cleared. |
| `Enter` | Applies the term and closes. An empty term clears the column's filter. |
| `Esc` | Closes, leaving the column filtered exactly as it was. |
| anything else | Inert, `ctrl-d`/`ctrl-u` included: the modal has no list to page. |

### The cell inspector

`Enter` over Table contents opens `Overlay::Inspect`. The grid caps a column
at `grid.max_column_width` (30 by default) and ellipsises the rest, widening
truncated columns into whatever room a grid that fits leaves; this is where a
value is read whole. Row mode shows every column of the row under the
cursor, one field to a column; cell mode shows the cell under it. Column
mode opens nothing: its cursor is a column, not one value.
It reads the grid as drawn, sorted and filtered. JSON text is re-indented
without a token changing, and a collection is laid out one element to a line.
Like the palette it dims the panels, claims every key and leaves `q` quitting.

| Key | Effect |
|---|---|
| `j` / `k`, `Down` / `Up` | Move the cursor a line. |
| `PageDown` / `PageUp`, `ctrl-d` / `ctrl-u` | Move it a body height. |
| `Home` / `End`, `g` / `G` | The first line, the last. |
| `y` | Yank what the grid's `y` would in the mode it was opened from: the bare value, or the row as one CSV line. The inspector stays open. |
| `Y` | Yank the line the cursor is on, its indent dropped and, in row mode, without the column name beside it. Nothing over a null field. |
| `e` | Open an edit field over the value the cursor is on. Refused *with a message* where the column is part of the primary key, where its type cannot be typed into, where the row's whole primary key is not in the page, where the grid came from a typed statement, and on a read-only connection. |
| `ctrl-s` | Commit this row: the commit confirmation over this row's edits alone, every other dirty row staying dirty. Accepts an open edit field first; inert while its text does not parse and over a row with nothing uncommitted. Over the insert form it opens the insert confirmation, as `Enter` does. |
| `Enter` / `Esc` | Close it. Over the insert form, `Enter` opens the confirmation instead. |
| `q` | Quits, or raises the uncommitted-changes warning. |
| anything else | Inert, every other ctrl chord included. |

The cursor is one laid-out line — a JSON member, a collection element, a line
of a field — banded across every row it wrapped to, the way a wrapped Logs
entry is. The wheel moves it wherever the pointer is, and a click closes the
inspector. A frame that shrinks past the block closes it too.

### Editing a value

`e` inside the inspector opens a one-line field over the value the cursor is
on. It claims every printable key and the editing keys, the way the search
prompt and the export form do; nothing else in the block moves while it is
open.

| Key | Effect |
|---|---|
| `Enter` | Accept it. Inert — and the `⏎` hint greyed — while the text is not a value of the column's declared type; the field stays open with the reason under it rather than losing what was typed. |
| `Esc` | Abandon it, changing nothing. |
| `Left` / `Right`, `Home` / `End`, `Backspace` / `Delete` | Move and edit within the field. |
| anything else printable | Types itself. |

An accepted value is **uncommitted**: held on the page, drawn in the grid and
the inspector in the warning colour, and on the cluster nowhere until
`ctrl-s`. An edit that restores the value it started from is not an edit — the
mark disappears. The Contents border counts what is outstanding.

The value is typed as text and read against the column's declared type. `text`,
`varchar` and `ascii` take anything; `inet`, `date`, `time` and `timestamp`
take any text and let the cluster judge it; `uuid`/`timeuuid` take 8-4-4-4-12
hex; the integer types take an `i64`; `varint`, `decimal`, `float` and `double`
take a number and are written verbatim; `boolean` takes `true`/`false`; `blob`
takes `0x` and an even number of hex digits. Every other type — the
collections, `counter`, a UDT — is refused, as is a primary key column, which
CQL cannot `UPDATE` at all.

Edits belong to the page they were made on: a sort, a filter and an appended
page all carry them, and a re-read discards them. A sorted or filtered grid
reads the value the cluster sent rather than the edit, so an edited row does
not move out from under the cursor that just edited it.

### Inserting a row

`i` over Table contents opens the inspector over a row that does not exist
yet, titled `· new row`: one field per column, schema order, all unset. `e`
types into a field as in *Editing a value*, with three differences: key
columns are typeable, an empty `text`, `varchar` or `ascii` is a value (`''`)
while an empty field of any other type unsets its column, and nothing reaches
the page.

| Key | Effect |
|---|---|
| `e` | As in the inspector. |
| `x` | Unset the field under the cursor. Greyed over a field holding nothing. |
| `Enter` | Open the confirmation over the `INSERT`. Refused with a toast, keeping the form, while any key column is unset. |
| `Esc` | Close the form, discarding what was typed. |

A write the cluster refuses keeps the form: the next `i` gives it back, until
the panel's content is replaced. The confirmation is the commit confirmation, titled `Insert n cells into
ks.t`; `n` and `Esc` there return to the form. On success the grid is
re-read: a new row has no place in the page to land, and where it sorts is the
cluster's to say.

### The commit confirmation

`ctrl-s` over Table contents opens `Overlay::Commit` over every uncommitted
edit — or, inside the inspector, over the edits of the row it shows,
which is the only edit written when `y` answers it — showing the CQL that would run. One dirty row is a bare `UPDATE`;
several are one logged batch, so a confirmation that said "3 rows" has
promised three rows. Like the drop confirmation it dims the panels and claims
every key.

| Key | Effect |
|---|---|
| `y` | Run it. The posture is checked here, not only where the block opened. |
| `n` / `Esc` | Abandon it, leaving the edits standing — or, over an insert, handing the form back with what was typed. |
| `j` / `k`, `PageDown` / `PageUp`, `ctrl-d` / `ctrl-u`, `Home` / `End`, `g` / `G` | Scroll the statement. Unlike the drop confirmation, which elides a long batch to its head, this block scrolls: every statement a commit would run is readable. |
| anything else | Inert. `⏎` answers nothing — an `UPDATE` is a write, so there is no typed spelling. |

On success the edits are written into the page in place and the marks clear;
the page is not re-read, so the cursor, the paging and the arrangement are
kept. On failure they stay dirty and the reason is raised and logged.

### The uncommitted-changes warning

`q` with anything uncommitted raises `Overlay::Discard` instead of quitting.

| Key | Effect |
|---|---|
| `y` | Quit, discarding the edits. |
| `n` / `Esc` | Stay. |
| anything else | Inert, a second `q` included: the question has to be answered rather than pressed past. |

The guard lives in `quit` itself, so the palette's quit row, the inspector's
`q` and `Msg::Quit` all reach it.

### Queries panel

Two tabs, Cql and Logs. This subsection is the Cql tab, the buffer the panel
opens on; the Logs tab is read-only and has a subsection of its own,
directly below.

`4` and a press on the buffer arrive with it open. `Tab`, `Shift-Tab` and a
tab click arrive with it closed, taking no text: every global key keeps its
meaning. Closed, its rows are the statements the buffer
holds: `j`/`k` band one after another and `ctrl-r` runs the banded one.
The band is drawn in both modes — it says what `ctrl-r` would send, and
`ctrl-r` answers in both. `⏎` or `i` opens it, and from then on every
printable key is a character; `Esc` closes it again without leaving the
panel. The caret is drawn only while it is open, and the bar reads `⏎ edit`
before and `esc done` after. Leaving the panel or the tab closes it, and
nothing reopens it on the way back. Vim motions stop at the buffer's edge: it
is edited with the keys below, and `ctrl-e` is the way to any other editor.

| Key | Action |
|---|---|
| `Enter` / `i` | With the buffer closed: open it. Types nothing. |
| `j` / `k` / `↓` / `↑` | With the buffer closed: band the next / previous statement. A step off either end stays where it is. |
| `g` / `G` / `Home` / `End` | With the buffer closed: band the first / last statement. |
| `ctrl-r` | Run the banded statement — the one the cursor is in, in either mode. Its page replaces whatever Contents was showing. A buffer holding no statement runs nothing, and the bar greys the hint out while that is so — §2. |
| `ctrl-g` | Open the query history: what this connection has been asked from the buffer, newest first. Inert with nothing remembered, and the bar names it only once there is something to open. See the query history's own subsection in §1. |
| `ctrl-e` | Open the buffer in `$VISUAL`, `$EDITOR` or `vi`, and replace it with whatever the editor saved. The program is suspended while the editor holds the terminal. |
| `ctrl-f` | Lay the buffer out: one clause to a line, keywords upper case, `AND` indented under the clause it continues. Nothing is reordered and no name, string or comment is retyped. The bar greys the hint out on a buffer already laid out, which an empty one is. |
| `ctrl-n` / `ctrl-p` | The next / previous completion. With no popup up, raise one against the word under the cursor. |
| `↓`³ / `↑`³ | The next / previous completion — **only** while a popup is up, and the same step `ctrl-n`/`ctrl-p` take. With none up, they move the cursor, as everywhere else. |
| `Tab`³ / `Enter`³ | Accept the selected completion — **only** while a popup is up. A keyword lands with the space the word after it needs, unless the cursor is already on one; a name, a type and a statement shape land as they are drawn. With no popup up, `Tab` moves focus and `⏎` splits the line, as everywhere else. |
| any printable key | Insert it, and offer completions against the word it lands in. `q`, `t`, `j` and the rest are text here. |
| `Enter` | With the buffer open: split the line at the cursor. |
| `Backspace` / `Delete` | Delete before / at the cursor; at a line's edge, the line break. |
| `←` `→` `↑` `↓` | Move the cursor. `←`/`→` wrap between lines, and stay motions under a popup — which is what takes it down. |
| `Home` / `End` | To the start / end of the **line**. |
| `PageUp` / `PageDown`, `ctrl-u` / `ctrl-d` | Up / down one panel height. |
| `Esc`³ | Take the completion popup down; with none up, close the buffer. A closed buffer's `Esc` leaves the panel for Tables. |

³ The five keys the completion popup claims, and it claims them only while it
is drawn. All five are exceptions to §2's *the same key means the same thing
in every panel*, taken for the reason `Home` and `End` already diverge here: it
is what the key does in every editor the audience has met. None is invisible —
the popup is on screen with a row selected, and the bar swaps `⏎ newline` for
`tab ⏎ complete` in exactly the frames the swap is true.
`Esc`'s second press still closes the buffer, so its hint stays true either way.
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
end of that statement, leaving the cursor on the select list.

`Tab` and `Shift-Tab` still reach here, except as ³ says, but from the Cql tab
neither leaves the panel outright: `Tab` lands on the Logs tab first, the
panel's own second stop in the cycle (§1), and only a second `Tab` from there
wraps on to Tables; `Shift-Tab` from Cql steps back to Table contents
instead, since Logs sits after Cql in the cycle, not before it. Nothing else
global survives inside an open buffer — including `q`, which types a `q`. `ctrl-r` is not global either:
it is the ctrl layer's Queries member and does nothing outside this panel,
and neither does `ctrl-e`, which opens the buffer in the user's editor.

### Logs tab

Queries' second tab: a read-only, scrollable trail of every statement the
session issued and what came back, one entry per connection attempt, schema
read, query and export. Reached by `5`, by `]` from a closed Cql tab, by
`Tab`/`Shift-Tab` walking the cycle of `(panel, tab)` stops (§1), or by a
click on the tab. It takes no text at all: `j`/`k` and the arrows move the cursor entry to entry, `PageUp`/
`PageDown` and `ctrl-d`/`ctrl-u` page it, `Home`/`End` and `g`/`G` reach
either end, `v` starts or freezes a selection of whole entries, `/` searches
it and `n`/`N` walk the matches, `y` yanks — the held selection if one is
active, otherwise the entry under the cursor — each entry as its two rows
joined, the statement and its outcome. `q` quits
here, unlike the Cql tab, where it types itself.

Nothing here writes, deletes or clears an entry: a trail with a delete key
would not be a trail. Leaving the panel — by `Tab`,
`Shift-Tab`, another panel's number, or a click — reverts the tab to Cql,
because the panel's unfocused height is a function of the CQL buffer it
hugs, not of the trail; returning to Logs costs
`5`, `Tab`/`Shift-Tab` walking the cycle again, or a click. `Esc`
leaves it for Tables the way it leaves any other panel (§2), reverting the
tab on the way out like every other route off it. The trail's own search does
not outlive the tab being drawn either — switching to Cql clears it, since the
hint bar cannot advertise `n` while `n` is literal text there.

---

## 2. The rules

These are constraints on the binding set, not style advice. A change that
breaks one of them must document and justify the new rule here.

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
themselves exactly as they would in a panel that takes text. Failing that, a panel that takes text claims the printable and
editing keys before the table is reached.
Everything none of the four claims is still dispatched globally and still
refused by panels that have nothing to do with it.

**Match order is precedence.** The panel-focus arm is last, so it can only claim
letters no earlier arm took. Adding an arm above it can silently steal a focus
key; `a_number_focuses_its_panel_from_any_panel_that_does_not_take_text` catches
it.

**One key, one action.** A second key for something a key already does is a
key spent, a row on the palette, and one more thing to keep in step. Three
kinds of duplicate survive, and only these three: the arrows and
`PageUp`/`PageDown`/`Home`/`End`, which are how a user who has never met vim
reaches a list at all (vision.md §4, principle 2); the pointer, which
necessarily repeats what the keyboard does (§6); and `ctrl-d`/`ctrl-u`, which
are the vim spelling of a paging key rather than a second implementation of
paging — they dispatch to `PageDown`/`PageUp`'s own arm, and `g`/`G` are the
same kind of spelling for `Home`/`End` (§1's motion table). `Space` on a tree
row was a duplicate of the fourth kind and is gone.

**Shift is never consulted for letters.** `A`, `C`, `T`, `U` arrive as
distinct `Key::Char`s and stay unbound; a shifted letter is a binding of its
own, not a synonym for the plain one. `T` was an exception and has been
given back — the theme picker moved to `ctrl-t` (§1's picker subsection).
Three exceptions remain: `N`, which walks a search backwards and is not a
synonym for `n`; `G`,
which reaches the end of a list and is not a synonym for `g`, which reaches
the start (§1's motion table); and `P`, which fetches every page and is not
a synonym for `p`, which fetches one (§1's Table contents subsection). A
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
for whatever is focused. The layer has eleven members now. `ctrl-r` runs the Queries
panel's banded statement, `ctrl-e` opens the buffer in an external editor, `ctrl-f` lays it
out, and `ctrl-n`/`ctrl-p` walk the completion popup: the panel takes
text, so every printable key there is a character and none of those actions has
an unmodified key available.
`ctrl-e` carries a second meaning too, over Table contents: it opens the
export form, gated on focus exactly as `ctrl-r`'s one meaning already is —
a chord is never a synonym for the plain key, and nothing in the rule says
it may carry only one meaning of its own. `ctrl-s` carries one more, inside
the export form, and `ctrl-y` means something only there: the form's path
takes text, so its two buttons have no plain key available (§1's export-form
subsection). `ctrl-t` opens
the theme picker, for the reason §1's picker subsection gives. `ctrl-g` opens
the query history, for the reason §1's history subsection gives. `ctrl-f`,
`ctrl-n` and `ctrl-p` are Queries members exactly as `ctrl-r` is, gated on that
panel and inert outside it; none of them shadows the plain `f`, `n` or `p`,
and `n`/`N` still walk a search.
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

The pairing breaks in an open Cql buffer, and only there: it claims
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
| Tables | Fold — step out | Unfold — step in — read a table |
| Contents | Previous column | Next column |
| DDL | Inert | Inert |
| Queries | Cursor left, in text | Cursor right, in text |

That is the shape `j`/`k` have had since the first list: "down one row" is a
tree row, a grid row or a wrapped line depending on what the panel is showing,
and is inert where it is showing nothing. Reading the horizontal keys as "fold"
and "unfold" was a description of the only panel that had claimed them, not a
rule.

`Home` and `End` are a second exception. Everywhere else they reach the ends
of the document; in the editor they reach the ends of the line, because that
is what they do in every text editor a user has met. The document's ends
stay reachable with `PageUp` and `PageDown`.

A panel number (`1`–`5`) is a third exception: it moves focus everywhere but
an open Cql buffer, where it is text. So are `[` and `]`.
`Tab`, `Enter` and `Esc` under a completion popup are a fourth, and the only
one conditional on something drawn rather than on which panel is focused: while
the popup is up `Tab` and `⏎` accept the selected candidate and `Esc` takes the
popup down, and in every other frame all three mean what they mean everywhere.
`↑`/`↓` join them there — they walk the popup while one is up and move the
cursor otherwise — and are the mildest of the five: what they move is still the
row list on screen.
The condition is on screen — the popup itself — and the bar drops `⏎ newline`
for exactly those frames rather than name something the next
keystroke will not do.

**A binding must be visible.** vision.md §4, principle 1: discoverability is
ambient. Each panel's title opens with the key that reaches it; the hint bar along
the bottom row names each focused panel's principal bindings, not its full
surface — the bar has never claimed to be the complete list. `Esc` is named wherever it
is unconditionally true, in that frame, that pressing it does something —
which, since the chain's last rung became every panel rather than the editor
alone, is every frame but Tables' own. The bar names the rung that would fire:
`esc stop` on Table contents while `P` runs, `esc done` over an open Cql
buffer, `esc clear` on a panel carrying a search or a live range, `esc leave`
on any other panel but Tables, and `esc close` on an open palette or search
prompt.
It stays out of Tables with neither an overlay up nor a search applied, where
it still does nothing at all, and naming it there would be a lie. A binding that is in none of a title, the bar, or the palette is
a binding only this file knows about.

**The panel bar reads specific to common.** The panel's own bindings come
first; the keys that mean the same in every panel that has them close the bar,
in the order `/`, `n`, `v`, `y`, the `esc` rung, `?`. `tab` and `q` are off it: the
numbers in the titles already teach the panels, and `?` finds both. `?` is the
last hint a narrowing frame drops, and is off the bar while a Cql buffer is
open, where it is text. The overlays' bars still name `q`.

**Motion is assumed, not taught.** `j`/`k`, `h`/`l`, `Home`/`End`, the arrows
and `PageUp`/`PageDown` are off every panel's bar: a user of a terminal list
reaches for them before reading anything, and a bar that spends its width on
them has none left for what is particular to the panel — which is the only
thing a *contextual* bar is for. `Shift-Tab` is `Tab`'s inverse and is off it
for the same reason. The two exceptions are the theme picker's `j/k preview`,
where what the hint teaches is the side effect rather than the motion (§1's
picker subsection), and the inspector's `j/k move`, where nothing else on
screen says the block has a cursor.
The palette lists every one of them regardless — that is the division of
labour the next paragraph states.

A hint the bar shows must be true *in the frame it is shown in*, and one
binding is not: `ctrl-r` on a buffer holding no statement runs nothing, and an empty buffer is what the Queries panel
starts as. Hiding the hint until the buffer fills would hide the thing the
panel exists for from the user who has not typed yet, so it is **greyed
instead** — same place, same width, without the weight a live key carries.
Greyed is the third answer beside naming and omitting, and it is what a
conditional binding gets; it is not a licence to name a key and grey it rather
than think about whether it belongs on the bar. `esc` is still omitted from the
other three bars rather than greyed there: it would be grey in most frames, and
a permanently grey hint teaches nothing.

**A key the connection's posture refuses is off the bar, named as refused in
the palette, and answered when it is pressed.** A read-only connection leaves
`d` off the bar on both panels it is bound in, and draws the palette's
drop-and-delete rows, under their panel's heading too, with a muted `(disabled, read-only)` after their keys.
`ctrl-r` is greyed rather than dropped, against a buffer holding anything but a
read: the same key in the same frame runs a `SELECT` and would be refused a
`DELETE`, so it follows the text as typed. The palette keeps its rows: it
claims to be complete, and a row that vanished on a read-only connection would
teach that the tool cannot drop a table at all. Pressing one anyway raises a modal naming the posture and
the config key that changes it — greying alone leaves a user who pressed the
key with nothing, and no bar can say *which* connection is the read-only one.
There is no way past the modal, deliberately: a posture is a property of the
connection, and an override key would make it a suggestion.

The palette is the half of that pair with no discretion. The bar picks what a
panel's hints teach; the palette claims to be complete, so it may not omit a key
that does something. That claim is about keys, not about wordings. The shared motions — `j`/`k`,
`h`/`l`, the arrows, the paging keys, `Home`/`End` — are listed once, under
`Navigate`, and worded for the movement rather than for what a panel makes of
it: `h`/`←` is "left, or fold". A panel's own group then carries only what that
panel alone answers — `⏎` in the tree, `m` in the grid. Listing the horizontal
pair under every heading that resolves it, which the catalogue did until
2026-08-15, bought a per-panel noun and cost the reader the same two keys three
times over before anything particular to a panel.

**Focus decides the mode, with one step inside Queries, and an overlay
outranks focus.** `State::takes_text` is the one exhaustive answer: true only
while Queries has focus and its Cql buffer is open. There every printable key
is a character; everywhere else every printable key is a binding. Opening the
buffer is the one state within a panel that changes what a key means, and it
is entered by a press of its own (`⏎`/`i`), shown by the caret and the bar,
and left by `Esc` or by leaving the panel.

The Table contents selection mode does not touch this. `m` changes what the
grid highlights — a row, a column, or the cell they cross — and no key changes
meaning: `j`/`k` move the row cursor and `h`/`l` go one column left or right in
all three modes, exactly as they did when there was only one way to draw them. Row mode spends that
column on the window rather than on the cursor it does not paint, which is the same key with the same meaning resolved in
the vocabulary of what is on screen — the shape §2 already gives `h`/`l` across
panels. A mode that rebound those keys — to reorder columns, say — would break
this rule, and is not what this is.

The command palette sits above that. It is opened deliberately with `?`, closes
with either `?` or `esc` — both named on its bar — and while it is up it
takes motion, runs the selected row on `⏎`, leaves `q` quitting, and makes
every other key inert rather than dismissing. A key that means "close" in one
frame and something else in the next is the ambiguity both halves of this
rule exist to prevent.

**A modal that claims every key says so by dimming what it covers.** While the
palette, the message overlay, the theme picker, the diagnostics list or the
inspector is up, there is no focused panel — all of them take the border and
title an unfocused panel takes — and the cursor row, the cursor cell and the washes around it are
not drawn: a panel that still drew them would be claiming a press that cannot
reach it. The hint bar recedes with them — every hint on it goes `Faint`, the
weight an inert hint already carries, because the modal has claimed those keys
and is listing them itself. The search prompt, the
export form and the filter prompt are excluded — the prompt reads against the
panel it filters, the form confirms what the grid beneath it has selected, and
the filter narrows that same grid.

The search prompt sits at the same rank, entered deliberately with `/` and
left with `Enter` or `Esc`. Unlike the palette it takes text, not motion:
every printable key appends to the query rather than doing anything else, `q`
and `?` included — the panel-that-takes-text rule applied to an overlay.

The theme picker sits at the same rank too, entered deliberately with `ctrl-t` and
left with `Enter` or `Esc`. Like the palette it takes motion, not text: `j`/`k`
and the arrows move its cursor, and moving previews — the one overlay whose
motion has a side effect outside itself, since it writes `State.theme` on
every step. Like the palette it
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
of the palette and the picker's own `q`-quits rule. Both lists narrow to
the rows the term matched, marking the hit where it sits, in a block the
unfiltered list still sizes — and in the picker the row the filter seats on
is the theme previewed, since its cursor is the preview. Nothing of the
search outlives the prompt: `⏎` keeps only the row the cursor found and
widens the list back out around it, `Esc` puts the cursor back where the
prompt opened, and the query itself is gone once the prompt closes, with no
highlight and no `n`/`N` left to walk.

**`Esc` resolves in one order, first hit wins.** A live sift inside an
overlay comes off first, restoring the cursor to the row the prompt opened on
and, in the picker, the theme it previewed with it — the overlay itself stays
up. Failing that, an overlay up is dismissed — a search prompt cancels,
restoring the search and the cursor it found applied when it opened; the
palette just closes. Failing that, a running `P` in a focused Table
contents stops, the page in flight still landing.
Failing that, an open Cql buffer closes, and focus stays on Queries.
Failing that, an extending
range in the focused panel freezes, the way a second `v` would, so `Esc`
never discards a range that is still growing. Failing that, the selection
clears — every group at once, however many are held. Failing that, the
focused panel's search, if it has one, clears. Failing that, the focused
panel hands focus back to Tables — every panel, not the editor alone: a key
that backs out of Queries and sits inert in DDL beside it is one key meaning
two things (§2). On Tables, which is where that rung lands, `Esc` does
nothing. The first rung preserves
`escape_dismisses_the_overlay_before_it_leaves_the_editor`; extending it down
the chain is what "the thing most recently put on screen comes off first"
means once a panel can carry state of its own.

**No key without a test.** Every binding above is pinned by one.

---

## 3. The key budget

Taken, lowercase: `c` `d` `e` `f` `g` `h` `j` `k` `l` `m` `n` `p` `q` `s` `v`
`y`, plus `?` and `/`. `i` is taken by Table contents, where it opens the
insert form, and by Queries, where it opens the buffer: a panel's claim on a
letter stops at its border. `c`, `f` and `s` are taken by Table contents' column
mode alone — a panel's claim on a letter stops at that panel's border, and
this one stops at a mode's (§1's Table contents subsection). `e` opens an
edit field, from the inspector and from the grid, and is free everywhere
else.
Taken, digits: `1` `2` `3` `4` `5`.
Taken, punctuation: `[` and `]`.
Taken, uppercase: `G`, `H`, `N`, `P` and `Y` — the only shifted letters bound (§2).
`H` answers in Tables alone.
`p`, `P` and `e` answer in Table contents alone, and `Y` and `e` in the
inspector. `x` answers inside the insert form alone.

Free, lowercase: `a` `b` `o` `r` `t` `u` `w` `z`, the
digits `6`–`9` and `0`, and every uppercase letter but `G`, `H`, `N`, `P` and `Y`.

`Space` is claimed for the first time, inside the export form only: it
activates the focused stop there (§1's export-form subsection) and is free
everywhere else, unlike a plain letter — a panel's own claim on a letter
stops at that panel's border, and so does the form's.

`d` is one key gated on two panels, the shape `ctrl-e` already has: the tree
and the grid, each aimed at what that panel holds.

`y` and `n` are bound plain already — yank, and the next search match — and
the drop confirmation, the commit confirmation and the uncommitted-changes
warning all answer with the same two letters while one of them is up. That is
not a second binding of either: an overlay claims keys before focus is
consulted (§2), so the letters mean one thing on a panel and one thing inside
the modal, the way `j` means one thing in the tree and another in the
palette.

Named keys taken: `Enter` `Esc` `Tab` `Shift-Tab` `Home` `End` `PageUp`
`PageDown` `↑` `↓` `←` `→`, plus `Backspace` and `Delete`, which only the
editor and the export form's path stop bind.

`Enter` is gated on focus three ways, the shape `d` has on two: the tree folds
or reads, the grid inspects, and the editor splits a line (§1).

Ctrl taken: `ctrl-r`, `ctrl-g`, `ctrl-f`, `ctrl-n` and `ctrl-p` in Queries, `ctrl-e` in
two panels — Queries and Table contents (§1's export-form subsection) —
`ctrl-s` in Table contents, its inspector and the export form, `ctrl-y` in the
export form alone, `ctrl-t` and `ctrl-o` everywhere, and
`ctrl-d`/`ctrl-u` everywhere
`PageDown`/`PageUp` do anything. The rest of the layer is free — including
`r`, `t`, `o` and `u` as plain keys, which nothing binds. `s` is bound plain,
in Table contents' column mode, and `ctrl-s` is a binding of its own rather
than a synonym for it. `n`
is bound plain, to the next
search match, and `p` to the next page; `ctrl-n` and `ctrl-p` are bindings
of their own rather than synonyms for either. Alt is not a layer and nothing may claim from it.

`Backspace`, `Delete`, `←`/`→` and `Home`/`End` have four text fields: the
editor, the export form's path stop, the drop confirmation's typed name, and
the inspector's edit field.

Named keys the domain understands but nothing binds: `Insert` `F1`–`F12`.
Anything crossterm reports outside that vocabulary is dropped at the edge and
never reaches the domain. The pointer is not in this budget at all — see §6.

The ctrl namespace is separate from the plain-key budget above: `r` is free as
a plain key and taken behind ctrl.

Claim from the free set first. Re-using a taken key means retiring a binding,
which changes the rules documented here.

> Every printable key is also text in a panel that takes text. That does not
> spend it: `s` is still free to be bound for the other three panels.

---

## 4. Deliberately unbound

| Not bound | Why |
|---|---|
| `a` `b` `u` focus | Were three of the five panel and tab letters until §1 moved to `1`–`4`. A letter has to be learned per panel; four digits in frame order are one rule, and `[1]-` in the title tells a novice the whole binding. Free, and deliberately left so for a while — a finger that learnt one should find nothing rather than something else. `c` and `d` were the other two and are free no longer: `c` clears a column filter (§1's Table contents subsection) and `d` opens the drop confirmation (§1). Letters reclaimed for the nouns they start, not for panels. |
| `A` `B` `C` `D` `U` | Reserved. A shifted letter is a binding of its own, not a synonym. `T` opened the theme picker until the picker moved to `ctrl-t`, and is free again. |
| `t` focus | Was Tables before either scheme (§1). Free for the same reason the letters above are. |
| `Space` on a tree row | Was `Enter`'s synonym there. One key, one action (§2) — true still, even though `Space` is no longer unbound everywhere: it activates a stop inside the export form (§1), and a tree row is not one. |
| `ctrl-e` / `ctrl-y` over DDL | Moved the DDL viewport a line without its cursor. Reading a panel from its heading down needs a design, not two chords. The clipboard binding did not claim `ctrl-y`: `y` was free as a plain key, and a yank in Queries needs a selection first. `ctrl-y` has since been claimed inside the export form alone, where the path field leaves `y` no plain meaning. |
| `r` refresh | The reload path exists now — a successful drop re-issues `Effect::LoadSchema` (§1's drop subsection) — but it has exactly that one caller. A refresh key needs a rule for what a racing DDL change costs a panel mid-read; that remains open in TODO.md. |
| `Home`/`End` over columns | Both reach the ends of the document, which in a grid is the row list. A second axis for one panel would be a third exception to a two-exception rule. |
| Horizontal wheel | Dropped at the edge, and this slice did not pick it up. A notch moves the row viewport; columns are reached with the keys §1 names. |
| Drag-to-select | Weighed and refused: `lc-inbound` drops drag and move events at the edge today (§6), so honouring one would need a new gesture, press-origin state, and an autoscroll rule at the panel edges — for a selection two keystrokes already make. |
| `Shift-↓` extend selection | `v` is the way to extend a selection; a shifted arrow would be a second name for a key that already does it, the fourth kind of duplicate §2's "one key, one action" refuses. |
| User-defined bindings | vision.md §7 Phase 5. |

---

## 5. Adding one

1. Check §3. Take a free key.
2. If it changes a rule in §2, document and justify that change here.
3. Add the arm to `handle_key`, above the panel-focus arm only if it must
   outrank a panel number — and say why in the commit.
4. Pin it with a test.
5. Make it visible. A panel binding declares itself in `Panel::hints()`; a
   global one goes in `GLOBAL`. If it belongs in neither, say in the commit
   why it should stay hidden.
6. Update §1 and §3 here in the same commit.

---

## 6. The pointer

Bound, and spending nothing from §3's budget.

| Gesture | Lands on | Action |
|---|---|---|
| Left click | A keyspace | Focus Tables, select it, fold/unfold |
| Left click | A table | Select it, read it into Contents, and focus **Contents** |
| Left click | A Contents cell | Focus Contents, select its row and its column |
| Left click | A Contents heading | Focus Contents, select that column, leave the row where it is |
| Left click | A Contents heading's sort mark | Select that column and step it through the sort cycle — the `s` binding, reached by pointer |
| Left click | A Contents heading's filter mark | Select that column and open the filter prompt over it — the `f` binding |
| Left click | A DDL row | Focus DDL, select it |
| Left click | A tab on a panel's top border | Whatever that tab's key does: focus the panel, and draw the tab |
| Left click | Chrome | Focus that panel |
| Left click | Anywhere, overlay up | Dismiss the overlay — except on a confirmation's or the health block's own block |
| Left press | A confirmation's button (drop, commit, discard, export's two buttons and Yes, overwrite) | Lights it solid; dragging off unlights it |
| Left release | The button that was pressed | Its key: `y`, `n`, `⏎` or `esc`, answered as if typed. Released anywhere else: nothing |
| Left click | A confirmation's block, off its buttons | Nothing |
| Left press | The export form's path or count field | Focus it and put the caret before the character pressed (at the end, past the text); the count field also chooses the first-N scope |
| Left press | An export format or scope radio | Focus its group and choose that option, rewriting the path's extension for a format. A scope the form does not offer is inert |
| Left release | An export form button | Focus it and run it, from whichever stop the form was on |
| Left click | A health block row | Select it and fold/unfold it — `Enter`, reached by pointer |
| Left click | The health block, off its rows | Nothing |
| Left click | The hint bar | Nothing |
| Wheel | The panel under the pointer | Scrolls three rows or lines, moving neither focus nor the selection |
| Wheel | Anywhere, inspector up | Scrolls the inspector three lines |
| Wheel | Anywhere, a list overlay up | Moves its cursor three rows: the palette, the pickers, the diagnostics, the history, the commit confirmation and the health block |

**A click on an open search prompt cancels it**, restoring the search and
the cursor that stood before the prompt opened — the same thing `Esc` does
to it, and for the same reason: a click that lands somewhere unexpected must
not leave state behind.

**Queries is the exception: the wheel over it does nothing.** `panel_scroll`
has no arm for `Panel::Queries` — the editor derives both offsets from the
cursor and the area every frame and stores none for a notch to move. Whether
it should get a stored viewport of its own is an open question (TODO.md).

A table is the one target whose click leaves focus somewhere other than where
the press landed, and it does that because the key does: `Enter` and the click
reach the same state — the rule stated below, held rather than
bent.

A click on a row's number is a click on that row, and names no column: the bar
and the gutter share one widget column that is not a column of the page, so a
press there moves the row cursor and leaves the column cursor alone.

**The two heading marks answer a press in any selection mode**, unlike the
keys they stand for. A press names the column it landed on, where a key has
only the cursor to go on — so the mark needs no mode to disambiguate it, and it
moves the column cursor to what was pressed before acting.

**A click never changes the selection mode.** It names whatever cells it can
reach — a cell names both cursors, a heading only the column — and which of them
the grid draws a band around stays `m`'s business. So a press in row mode still
moves the column cursor under the highlight, and the next `m` finds it where the
pointer left it.

A press on the Queries panel opens its Cql buffer, as `4` does; on
the Logs tab it only focuses.

Chrome is a border, a notice, a title away from any tab, the
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
scrollbar rather than in the bar.

Any further gesture is measured against the narrower rule, not this exception.

**The wheel scrolls; it does not select.** Tables, Contents and DDL each
hold a viewport of their own, so a notch moves the rows (or, in DDL,
the lines) and leaves the selection — the cursor, in DDL — where you put
it. Scroll far enough and the selected row is simply not on screen, and the
next `j`, or a resize, brings the view back to it.

**One dispatch point, again.** `handle_pointer` in
[`update/pointer.rs`](../crates/lc-domain/src/update/pointer.rs) is to a gesture what
`handle_key` is to a key. `lc-inbound` resolves the coordinate against the frame
it happened in and decides nothing.

**Capture is on from startup**, which costs click-drag text selection;
shift-drag is the bypass on terminals that offer one, and a shifted press is
refused for exactly that reason. There is no toggle and no flag until there is
a config system to hold the preference.
