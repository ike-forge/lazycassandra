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
unconditional; the five marked rows are text instead, inside the one panel
that takes text — see the Queries subsection below.

| Key | Action |
|---|---|
| `q`¹ | Quit. Cancels every in-flight query. |
| `Esc` | Dismiss the overlay. |
| `Tab` | Focus the next panel, in reading order, wrapping. |
| `Shift-Tab` | Focus the previous panel. |
| `t`¹ | Focus **T**ables. |
| `a`¹ | Focus T**a**ble DDL. |
| `c`¹ | Focus Table **c**ontents. |
| `u`¹ | Focus Q**u**eries. |

¹ Not reachable from Queries. A panel that takes text claims every printable
key before this table is consulted, so these type themselves there instead.

The four focus letters are underlined in their panel's title, so the binding is
learned by looking at the panel rather than by reading this file (vision.md §4,
principle 1). Two of them are not leading letters: the panel names are fixed by
`doc/ui-draft.png`, and no title is renamed to make a key convenient.
See [panel focus shortcuts design](specs/2026-08-09-panel-focus-shortcuts-design.md).

### Motion

Applies to the focused panel and nothing else. What moves depends on the panel:
Tables and Contents move a selection and let the viewport follow; Table DDL
moves a viewport over lines; Queries moves the cursor through its buffer.

| Key | Action |
|---|---|
| `j` / `↓` | Down one row (one line in Table DDL; in Queries, `↓` only — `j` is text). |
| `k` / `↑` | Up one (in Queries, `↑` only — `k` is text). |
| `PageDown` | Down one panel height. |
| `PageUp` | Up one panel height. |
| `Home` | To the start (to the **line**'s start in Queries — §2). |
| `End` | To the end (to the **line**'s end in Queries — §2). |

`End` in Table DDL stops at the last line rather than scrolling past it; in a
list it lands on the last row. Both are the same `Motion::End`, resolved by the
focused panel ([panel scrolling design](specs/2026-08-09-panel-scrolling-design.md)).
Queries never reaches `Motion::End` at all: the editor claims `Home`/`End`
itself, before the match that would call it (queries editor design §3).

### Tables panel

Accepted anywhere, but a no-op unless Tables has focus — except in Queries,
which takes text: there `Enter` inserts a newline, `Space`, `l` and `h` type
themselves, and `→`/`←` move the cursor instead. None of them is a no-op,
but Queries is where `l`/`→` and `h`/`←` stop being the synonyms this table
pairs them into — §2's rewritten vim/arrows rule. See the Queries
subsection below.

| Key | Action |
|---|---|
| `Enter` / `Space` | On a keyspace: fold/unfold. On a table: read it into Contents. |
| `l` / `→` | Unfold the selected keyspace. |
| `h` / `←` | Fold it; from a table row, step out to its keyspace first. |

Reading a table cancels whatever read Contents was waiting on.

### Queries panel

The panel takes text, so while it has focus every printable key is a
character and not a binding. What it does not claim keeps its global meaning.

| Key | Action |
|---|---|
| any printable key | Insert it. `q`, `t`, `j` and the rest are text here. |
| `Enter` | Split the line at the cursor. |
| `Backspace` / `Delete` | Delete before / at the cursor; at a line's edge, the line break. |
| `←` `→` `↑` `↓` | Move the cursor. `←`/`→` wrap between lines. |
| `Home` / `End` | To the start / end of the **line**. |
| `PageUp` / `PageDown` | Up / down one panel height. |
| `Esc` | Leave the editor for Tables. |

`Tab` and `Shift-Tab` still move focus. Nothing else global survives inside
the editor — including `q`, which types a `q`.
See [queries editor design](specs/2026-08-09-queries-editor-design.md).

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

This now has one exception, and only one: a panel that takes text claims the
printable and editing keys before the table is consulted. Everything it does
not claim is still dispatched globally and still refused by panels that have
nothing to do with it.

**Match order is precedence.** The panel-focus arm is last, so it can only claim
letters no earlier arm took. Adding an arm above it can silently steal a focus
key; the test that catches this is named in §5.

**Lowercase only.** `T`, `A`, `C`, `U` arrive as distinct `Key::Char`s and stay
unbound. Shift is never consulted for letters, and a shifted arrow is left as a
synonym for the plain one so `Shift-↓` stays free for a future "extend
selection".

**Ctrl and alt are refused wholesale.** `handle_key` returns early on either
modifier rather than folding the key into its unmodified meaning: `ctrl-j` is a
different binding that does not exist yet, not a slower `j`. The ctrl layer gets
designed as a layer (`ctrl-d`/`ctrl-u` are the first candidates, TODO.md), not
smuggled in one key at a time.

**Vim and arrows are synonyms, both ways.** `j`/`↓`, `k`/`↑`, `l`/`→`, `h`/`←`.
Neither set is the real one. Anyone who reaches for arrows should never hit a
motion that only vim keys have, and the reverse (vision.md §4, principle 2 — the
same key at every level of skill).

The pairing breaks in Queries, and only there: a panel that takes text claims
`j`/`k`/`l`/`h` as characters before this match is reached, so only the
arrows still move the cursor. This is the vim-key half of the same exception
§3's key budget note makes for every letter.

**The same key means the same thing in every panel.** `j` moves down in
Tables, Contents and Table DDL — the panels that treat it as motion — or is
inert; it never means something else there. A panel that cannot honour a
motion swallows it rather than passing it on. Queries is the exception: it
takes text, so `j` types itself, along with `k`/`l`/`h` — the vim/arrows rule
above.

`Home` and `End` are a second exception. Everywhere else they reach the ends
of the document; in the editor they reach the ends of the line, because that
is what they do in every text editor a user has met. The document's ends
stay reachable with `PageUp` and `PageDown`.

A focus letter (`t`/`a`/`c`/`u`) is a third exception: it moves focus in
every panel that does not take text, and is text in the one panel that does.
The rule's own enforcement test was renamed for this — §5.

**A binding must be visible.** vision.md §4, principle 1: discoverability is
ambient. The focus letters are underlined in their titles; the hint bar along
the bottom row names each focused panel's principal bindings, not its full
surface — the bar has never claimed to be the complete list
([design](specs/2026-08-09-hint-bar-design.md) §6). `Esc` is named in the one
bar where it is unconditionally true that pressing it does something —
Queries, where it leaves the editor for Tables — and stays out of the other
three, where it still only does something when an overlay is up, and naming
it there would still be a lie in most frames (design §3); `Shift-Tab` is
`Tab`'s inverse, and Tables' `PageDown`/`PageUp`/`Home`/`End` are increments on
a move the bar already names as `j/k move` — neither teaches something the
bar's existing hints don't. A binding that is none of those and shows up in
neither a title nor the bar is a binding only this file knows about.

**Focus is the mode.** A panel either takes text or it does not, and
`Panel::takes_text` is the one exhaustive answer. In a panel that takes text
every printable key is a character; everywhere else every printable key is a
binding. There is no state within a panel that changes what a key means, so
the regime is always the focused border — which is on screen and already
means something. This replaces the older "no modes" rule, which held only
until the Queries panel took text.

**No key without a test.** Every binding above is pinned by name in §5.

---

## 3. The key budget

Taken, lowercase: `a` `c` `h` `j` `k` `l` `q` `t` `u`, plus `Space`.

Free, lowercase: `b` `d` `e` `f` `g` `i` `m` `n` `o` `p` `r` `s` `v` `w` `x`
`y` `z`, every digit, and every uppercase letter.

Named keys taken: `Enter` `Esc` `Tab` `Shift-Tab` `Home` `End` `PageUp`
`PageDown` `↑` `↓` `←` `→`, plus `Backspace` and `Delete`, which only the
editor binds, inside Queries.

Named keys the domain understands but nothing binds: `Insert` `F1`–`F12`.
Anything crossterm reports outside that vocabulary — including every mouse
event — is dropped at the edge and never reaches the domain.

Claim from the free set first. Re-using a taken key means retiring a binding,
which is a spec-level change.

> Every printable key is also text in a panel that takes text. That does not
> spend it: `d` is still free to be bound for the other three panels.

---

## 4. Deliberately unbound

| Not bound | Why |
|---|---|
| `?` help | vision.md §7 Phase 0 promises a contextual map. It is its own slice and this set does not anticipate its shape. |
| `1`–`4` focus | Superseded by `t`/`a`/`c`/`u`, which the panel can show in its own title; a digit cannot. |
| `T` `A` `C` `U` | Reserved. A shifted letter is a binding of its own, not a synonym. |
| `ctrl-d` / `ctrl-u` | Wants the ctrl layer designed first; see §2. |
| `r` refresh | Nothing re-issues `Effect::LoadSchema` after a racing DDL change; recovery is a restart. The binding waits on the reload path, not the other way round (TODO.md). |
| Horizontal motion | The grid caps at 24 columns and draws nothing past the right edge. Column navigation is unbuilt (vision.md §8). |
| User-defined bindings | vision.md §7 Phase 5. |
| Mouse | No `EnableMouseCapture`, no handler. Not a gap to be filled by accident. |

---

## 5. Where it is enforced

Tests in `crates/lc-domain/src/update.rs` unless noted.

| Guarantee | Test |
|---|---|
| `j`/`k` walk a list | `j_and_k_walk_the_keyspace_list` |
| Arrows are synonyms | `the_arrow_keys_do_what_j_and_k_do` |
| Motion stops at both ends | `the_selection_cannot_run_off_a_list_it_has_not_loaded`, `the_grid_cursor_cannot_run_off_a_page` |
| Motion is the focused panel's only | `motion_belongs_to_the_focused_panel_only`, `a_panel_with_no_rows_swallows_motion_rather_than_passing_it_on` |
| `Enter`/`Space` agree | `enter_and_space_both_toggle_a_keyspace`, `space_reads_a_table_the_same_way_enter_does` |
| `l`/`h` and arrows agree | `l_and_right_open_a_keyspace`, `h_and_left_close_a_keyspace` |
| `h` steps out before folding | `h_from_a_table_steps_out_to_its_keyspace_before_folding` |
| Tree keys are Tables-only | `a_tree_transition_belongs_to_the_focused_panel_only`, `enter_outside_the_tree_reads_nothing` |
| `q` quits and cancels | `q_quits`, `quitting_cancels_everything_still_in_flight` |
| `Esc` clears the overlay | `escape_dismisses_the_overlay` |
| `Tab`/`Shift-Tab` cycle and wrap | `tab_walks_the_panels_in_reading_order_and_wraps`, `shift_tab_walks_them_the_other_way` |
| Paging and `Home`/`End` | `page_down_moves_the_tree_by_one_panel_height`, `home_and_end_reach_the_ends_of_the_tree_in_one_key` |
| Ctrl/alt are not synonyms | `a_modified_key_is_not_a_binding` |
| Shifted letters stay free | `a_shifted_shortcut_stays_unbound` |
| Focus letters are distinct and total | `every_panel_has_a_distinct_shortcut`, `a_shortcut_leads_back_to_the_panel_that_claims_it`, `the_shortcuts_are_the_ones_the_design_names` — all in `state.rs` |
| No focus letter is shadowed by an earlier arm | `a_shortcut_focuses_its_panel_from_any_panel_that_does_not_take_text` |
| A focus letter is text in a panel that takes text | `a_shortcut_is_text_in_a_panel_that_takes_text` |
| Focus letters have no side effects | `a_shortcut_moves_focus_and_does_nothing_else` |
| Every title marks its own letter | `every_panel_title_marks_the_key_that_focuses_it` (`view/mod.rs`), plus the `PanelTitle` tests in `view/title.rs` |
| The marked cell, and only it, is underlined | `each_panel_underlines_the_key_that_focuses_it`, `nothing_but_the_shortcut_is_underlined` (`inbound/render.rs`), `a_shortcut_is_underlined_without_spending_a_colour` (`inbound/theme.rs`) |
| Letters are text in the editor | `a_letter_typed_in_queries_is_text_not_a_shortcut` |
| Quitting is not reachable from the editor | `q_does_not_quit_while_the_editor_has_focus` |
| `Esc` prefers the overlay, then leaves | `escape_dismisses_the_overlay_before_it_leaves_the_editor`, `escape_leaves_the_editor_for_tables` |
| `Tab` still leaves | `tab_still_leaves_the_editor` |
| Ctrl is still not text | `a_modified_key_is_still_not_text` |
| The editing keys reach the buffer | `the_editing_keys_reach_the_buffer` |
| `Home`/`End` take the line's ends | `home_and_end_reach_the_ends_of_the_line_in_the_editor` |
| Other panels keep their letters | `typing_outside_queries_still_focuses_a_panel` |
| The buffer's own behaviour | the `editor.rs` tests |

`a_shortcut_focuses_its_panel_from_any_panel_that_does_not_take_text` is what
makes the match-order rule in §2 enforceable: a panel claiming a letter an
earlier arm already takes has its arm swallowed, focus does not land, and the
test fails. Its companion, `a_shortcut_is_text_in_a_panel_that_takes_text`,
pins the half the rename gave up: from Queries, a focus letter is text and no
focus change happens at all.

---

## 6. Adding one

1. Check §3. Take a free key.
2. Write the spec if it changes a rule in §2; otherwise put the reasoning in the
   slice's design doc.
3. Add the arm to `handle_key`, above the panel-focus arm only if it must
   outrank a focus letter — and say why in the commit.
4. Pin it with a test, and add the row to §5.
5. Make it visible. A panel binding declares itself in `Panel::hints()`; a
   global one goes in `GLOBAL`. If it belongs in neither, say in the commit
   why it should stay hidden.
6. Update §1 and §3 here in the same commit.
