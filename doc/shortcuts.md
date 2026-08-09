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

Reachable from every panel, in every connection state.

| Key | Action |
|---|---|
| `q` | Quit. Cancels every in-flight query. |
| `Esc` | Dismiss the overlay. |
| `Tab` | Focus the next panel, in reading order, wrapping. |
| `Shift-Tab` | Focus the previous panel. |
| `t` | Focus **T**ables. |
| `a` | Focus T**a**ble DDL. |
| `c` | Focus Table **c**ontents. |
| `u` | Focus Q**u**eries. |

The four focus letters are underlined in their panel's title, so the binding is
learned by looking at the panel rather than by reading this file (vision.md §4,
principle 1). Two of them are not leading letters: the panel names are fixed by
`doc/ui-draft.png`, and no title is renamed to make a key convenient.
See [panel focus shortcuts design](specs/2026-08-09-panel-focus-shortcuts-design.md).

### Motion

Applies to the focused panel and nothing else. What moves depends on the panel:
Tables and Contents move a selection and let the viewport follow; Table DDL
moves a viewport over lines; Queries has neither, so motion there is inert
rather than reaching into another panel.

| Key | Action |
|---|---|
| `j` / `↓` | Down one row (one line in Table DDL). |
| `k` / `↑` | Up one. |
| `PageDown` | Down one panel height. |
| `PageUp` | Up one panel height. |
| `Home` | To the start. |
| `End` | To the end. |

`End` in Table DDL stops at the last line rather than scrolling past it; in a
list it lands on the last row. Both are the same `Motion::End`, resolved by the
focused panel ([panel scrolling design](specs/2026-08-09-panel-scrolling-design.md)).

### Tables panel

Accepted anywhere, but a no-op unless Tables has focus.

| Key | Action |
|---|---|
| `Enter` / `Space` | On a keyspace: fold/unfold. On a table: read it into Contents. |
| `l` / `→` | Unfold the selected keyspace. |
| `h` / `←` | Fold it; from a table row, step out to its keyspace first. |

Reading a table cancels whatever read Contents was waiting on.

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
and is revisited when the CQL editor lands (§4).

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

**The same key means the same thing in every panel.** `j` moves down in all four
or is inert; it never means something else somewhere. A panel that cannot honour
a motion swallows it rather than passing it on.

**A binding must be visible.** vision.md §4, principle 1: discoverability is
ambient. The focus letters are underlined in their titles; the hint bar along
the bottom row names each focused panel's principal bindings, not its full
surface — the bar has never claimed to be the complete list
([design](specs/2026-08-09-hint-bar-design.md) §6). `Esc` stays out of it
because it only does something when an overlay is up, and naming it always
would be a lie in most frames (design §3); `Shift-Tab` is `Tab`'s inverse, and
Tables' `PageDown`/`PageUp`/`Home`/`End` are increments on a move the bar
already names as `j/k move` — neither teaches something the bar's existing
hints don't. A binding that is none of those and shows up in neither a title
nor the bar is a binding only this file knows about.

**No modes.** There is no state in which a letter means something other than its
binding. This holds only until the Queries panel takes text input, and that is
the design that has to break it (§4).

**No key without a test.** Every binding above is pinned by name in §5.

---

## 3. The key budget

Taken, lowercase: `a` `c` `h` `j` `k` `l` `q` `t` `u`, plus `Space`.

Free, lowercase: `b` `d` `e` `f` `g` `i` `m` `n` `o` `p` `r` `s` `v` `w` `x`
`y` `z`, every digit, and every uppercase letter.

Named keys taken: `Enter` `Esc` `Tab` `Shift-Tab` `Home` `End` `PageUp`
`PageDown` `↑` `↓` `←` `→`.

Named keys the domain understands but nothing binds: `Backspace` `Delete`
`Insert` `F1`–`F12`. Anything crossterm reports outside that vocabulary is
dropped at the edge and never reaches the domain. The pointer is not in this
budget at all — see §7.

Claim from the free set first. Re-using a taken key means retiring a binding,
which is a spec-level change.

---

## 4. Deliberately unbound

| Not bound | Why |
|---|---|
| `?` help | vision.md §7 Phase 0 promises a contextual map. It is its own slice and this set does not anticipate its shape. |
| `1`–`4` focus | Superseded by `t`/`a`/`c`/`u`, which the panel can show in its own title; a digit cannot. |
| `T` `A` `C` `U` | Reserved. A shifted letter is a binding of its own, not a synonym. |
| `ctrl-d` / `ctrl-u` | Wants the ctrl layer designed first; see §2. |
| `r` refresh | Nothing re-issues `Effect::LoadSchema` after a racing DDL change; recovery is a restart. The binding waits on the reload path, not the other way round (TODO.md). |
| Text entry | The Queries panel is blank. When it takes CQL, every letter becomes contended, and the mechanism that resolves it — a panel that swallows keys, or a mode — gets designed against the real editor rather than guessed at now. |
| Horizontal motion | The grid caps at 24 columns and draws nothing past the right edge. Column navigation is unbuilt (vision.md §8). |
| User-defined bindings | vision.md §7 Phase 5. |

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
| No focus letter is shadowed by an earlier arm | `a_shortcut_focuses_its_panel_from_any_other_panel` |
| Focus letters have no side effects | `a_shortcut_moves_focus_and_does_nothing_else` |
| Every title marks its own letter | `every_panel_title_marks_the_key_that_focuses_it` (`view/mod.rs`), plus the `PanelTitle` tests in `view/title.rs` |
| The marked cell, and only it, is underlined | `each_panel_underlines_the_key_that_focuses_it`, `nothing_but_the_shortcut_is_underlined` (`inbound/render.rs`), `a_shortcut_is_underlined_without_spending_a_colour` (`inbound/theme.rs`) |
| A click folds, reads, and selects | `a_click_on_a_keyspace_folds_and_unfolds_it`, `a_click_on_a_table_reads_it`, `a_click_on_a_grid_row_moves_the_cursor_and_asks_for_nothing` |
| A click focuses what it lands in | `a_click_focuses_the_panel_it_landed_in`, `a_click_on_chrome_focuses_and_does_nothing_else` |
| A stale row is dropped, not clamped | `a_click_on_a_row_that_has_gone_is_dropped_rather_than_clamped` |
| A click spends itself on the overlay | `a_click_dismisses_the_overlay_and_does_nothing_else`, `a_wheel_does_not_dismiss_the_overlay` |
| The wheel scrolls without focusing | `a_wheel_scrolls_the_panel_under_it_without_focusing_it`, `a_wheel_over_the_ddl_panel_scrolls_its_lines`, `a_wheel_over_a_panel_with_nothing_to_scroll_is_inert`, `a_wheel_over_tables_moves_the_viewport_rather_than_the_selection`, `a_wheel_over_tables_no_longer_scrolls_the_ddl_panel_back` |
| Only the left button and the vertical wheel are bound | `only_the_left_button_is_bound`, `a_release_a_drag_and_a_move_are_dropped`, `a_horizontal_wheel_is_dropped` (`inbound/input.rs`) |
| A modified gesture is not a binding | `a_modified_press_is_not_a_gesture` (`inbound/input.rs`) |
| The reader thread's filter agrees with `to_pointer`'s | `is_bound_accepts_everything_to_pointer_would` (`inbound/input.rs`) |
| The map resolves to the row that was drawn | `the_map_resolves_every_drawn_keyspace_to_the_row_that_drew_it`, `the_map_resolves_a_scrolled_tree_to_the_rows_that_were_drawn`, `the_map_resolves_a_scrolled_grid_to_the_rows_that_were_drawn` (`inbound/render.rs`) |
| Chrome is not a row | `a_grids_header_is_chrome_rather_than_its_first_row`, `a_press_on_the_chrome_of_a_panel_names_that_panel`, `the_hint_bar_and_a_frame_below_the_minimum_are_not_clickable` (`inbound/render.rs`) |

`a_shortcut_focuses_its_panel_from_any_other_panel` is what makes the
match-order rule in §2 enforceable: a panel claiming a letter an earlier arm
already takes has its arm swallowed, focus does not land, and the test fails.

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

---

## 7. The pointer

Bound, and spending nothing from §3's budget. See the
[mouse input design](specs/2026-08-09-mouse-input-design.md).

| Gesture | Lands on | Action |
|---|---|---|
| Left click | A keyspace | Focus Tables, select it, fold/unfold |
| Left click | A table | Focus Tables, select it, read it into Contents |
| Left click | A Contents row | Focus Contents, select it |
| Left click | Chrome | Focus that panel |
| Left click | Anywhere, overlay up | Dismiss the overlay |
| Left click | The hint bar | Nothing |
| Wheel | The panel under the pointer | Scrolls three rows or lines, moving neither focus nor the selection |

Chrome is a border, a title, a grid's header, a notice, the Ddl body, the
Queries panel, and the blank space past the last row. Every other gesture —
right and middle buttons, drags, releases, horizontal wheels, and anything with
a modifier held — is dropped at the edge.

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

**The wheel scrolls; it does not select.** Tables and Contents each hold a
viewport of their own, so a notch moves the rows and leaves the selection where
you put it — scroll far enough and the selected row is simply not on screen, and
the next `j`, or a resize, brings the view back to it.

**One dispatch point, again.** `handle_pointer` in
[`update.rs`](../crates/lc-domain/src/update.rs) is to a gesture what
`handle_key` is to a key. `lc-inbound` resolves the coordinate against the frame
it happened in and decides nothing.

**Capture is on from startup**, which costs click-drag text selection;
shift-drag is the bypass on terminals that offer one, and a shifted press is
refused for exactly that reason. There is no toggle and no flag until there is
a config system to hold the preference.
