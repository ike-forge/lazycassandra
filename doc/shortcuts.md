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
| `?` | Open the command palette: every binding, in one scrollable block. |
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
`state.focus` and returns — while the palette is closed; while it is open, every
key is dispatched to it instead (§2, "One mode, opened deliberately"). This
keeps one flat, greppable list of everything the program responds to, at the
cost of a guard per action — the trade is deliberate and is revisited when the
CQL editor lands (§4).

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
would be a lie in most frames (design §3) — except while the palette is open,
the one frame where it is never a lie, and where it is named (§2); `Shift-Tab`
is `Tab`'s inverse, and Tables' `PageDown`/`PageUp`/`Home`/`End` are increments
on a move the bar already names as `j/k move` — neither teaches something the
bar's existing hints don't. A binding that is in none of a title, the bar, or
the palette is a binding only this file knows about.

**One mode, opened deliberately.** No letter changes meaning. The one state
that captures keys is the command palette, which the user opened with `?` and
closes with either `?` or `esc`, both named on the bar while it is up
([design](specs/2026-08-09-command-palette-design.md) §7). Motion scrolls the
palette rather than the panel underneath, `q` still quits, and every other key
is inert rather than dismissing — a key that means "close" in one frame and
something else in the next is the ambiguity this rule exists to prevent. Text
entry in the Queries panel is still the design that has to break the rest of
it (§4).

**No key without a test.** Every binding above is pinned by name in §5.

---

## 3. The key budget

Taken, lowercase: `a` `c` `h` `j` `k` `l` `q` `t` `u`, plus `Space` and `?`.

Free, lowercase: `b` `d` `e` `f` `g` `i` `m` `n` `o` `p` `r` `s` `v` `w` `x`
`y` `z`, every digit, and every uppercase letter.

Named keys taken: `Enter` `Esc` `Tab` `Shift-Tab` `Home` `End` `PageUp`
`PageDown` `↑` `↓` `←` `→`.

Named keys the domain understands but nothing binds: `Backspace` `Delete`
`Insert` `F1`–`F12`. Anything crossterm reports outside that vocabulary —
including every mouse event — is dropped at the edge and never reaches the
domain.

Claim from the free set first. Re-using a taken key means retiring a binding,
which is a spec-level change.

---

## 4. Deliberately unbound

| Not bound | Why |
|---|---|
| `1`–`4` focus | Superseded by `t`/`a`/`c`/`u`, which the panel can show in its own title; a digit cannot. |
| `T` `A` `C` `U` | Reserved. A shifted letter is a binding of its own, not a synonym. |
| `ctrl-d` / `ctrl-u` | Wants the ctrl layer designed first; see §2. |
| `r` refresh | Nothing re-issues `Effect::LoadSchema` after a racing DDL change; recovery is a restart. The binding waits on the reload path, not the other way round (TODO.md). |
| Text entry | The Queries panel is blank. When it takes CQL, every letter becomes contended, and the mechanism that resolves it — a panel that swallows keys, or a mode — gets designed against the real editor rather than guessed at now. |
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
| No focus letter is shadowed by an earlier arm | `a_shortcut_focuses_its_panel_from_any_other_panel` |
| Focus letters have no side effects | `a_shortcut_moves_focus_and_does_nothing_else` |
| Every title marks its own letter | `every_panel_title_marks_the_key_that_focuses_it` (`view/mod.rs`), plus the `PanelTitle` tests in `view/title.rs` |
| The marked cell, and only it, is underlined | `each_panel_underlines_the_key_that_focuses_it`, `nothing_but_the_shortcut_is_underlined` (`inbound/render.rs`), `a_shortcut_is_underlined_without_spending_a_colour` (`inbound/theme.rs`) |
| `?` opens and closes the palette | `question_mark_opens_the_palette_and_closes_it`, `escape_closes_the_palette` |
| It replaces a message rather than stacking | `opening_the_palette_replaces_the_message_it_covers` |
| It does not open where it cannot be drawn | `the_palette_does_not_open_where_it_cannot_be_drawn` |
| A resize that leaves it undrawable closes it too | `a_frame_that_shrinks_below_the_palette_closes_it` |
| An open palette takes motion and swallows the rest | `an_open_palette_takes_the_motion_the_panel_would_have`, `an_open_palette_swallows_the_keys_it_does_not_use`, `q_quits_from_inside_the_palette` |
| The cursor never lands on a heading | `the_palette_cursor_steps_over_a_heading`, `paging_settles_on_a_command_rather_than_a_heading` (`commands.rs`) |
| No row outgrows the narrowest palette | `no_command_row_is_wider_than_the_narrowest_palette` (`commands.rs`) |
| The bar names `?`, and the palette's own keys | `the_bar_names_the_palette_that_reaches_the_rest`, `the_bar_names_what_the_palette_responds_to` (`view/hints.rs`) |
| The bar drops `?` where no palette would fit | `the_bar_drops_the_palette_hint_where_no_palette_would_fit`, `every_global_hint_does_something_in_every_panel` (`view/hints.rs`, `update.rs`) |
| `?` earns its place in the width-fitting bar, and is the first global to go | `a_frame_with_room_for_the_palette_shows_its_hint_and_drops_it_first_among_the_globals` (`inbound/hints.rs`) |
| The palette is drawn, centred, capped and scrollable | `the_palette_lists_the_catalogue`, `the_palette_is_centred`, `a_wide_frame_does_not_get_a_wide_palette`, `a_tall_frame_does_not_get_a_palette_full_of_blank_rows`, `a_frame_too_small_for_a_palette_does_not_get_one` (`inbound/render.rs`) |

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
