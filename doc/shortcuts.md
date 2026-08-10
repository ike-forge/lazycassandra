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
unconditional; the six marked rows are text instead, inside the one panel
that takes text — see the Queries subsection below.

| Key | Action |
|---|---|
| `q`¹ | Quit. Cancels every in-flight query. |
| `?`¹ | Open the command palette: every binding, in one scrollable block. |
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
Tables, Contents and Table DDL move a selection and let the viewport follow;
Queries moves the cursor through its buffer.

| Key | Action |
|---|---|
| `j` / `↓` | Down one row (in Queries, `↓` only — `j` is text). |
| `k` / `↑` | Up one (in Queries, `↑` only — `k` is text). |
| `PageDown` | Down one panel height. |
| `PageUp` | Up one panel height. |
| `Home` | To the start (to the **line**'s start in Queries — §2). |
| `End` | To the end (to the **line**'s end in Queries — §2). |

`End` reaches the last row everywhere it applies, Table DDL included — a list
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

### Table DDL panel

Accepted anywhere, but a no-op unless Table DDL has focus.

| Key | Action |
|---|---|
| `ctrl-e` | Scroll the viewport down one line, leaving the cursor where it is. |
| `ctrl-y` | Scroll it up one line. |

Two of the ctrl layer's three members (§2), and the only panel that needs a
key to move a viewport on its own: a table option can be taller than the
panel, so this is the one place a selection and a viewport can genuinely
disagree — elsewhere the two never diverge, and there
is no viewport to move independently of the selection. A selection scrolled
out of view is not drawn; the next `j` or `k` brings the viewport back to it,
because motion follows the cursor.
See [Table DDL selection design](specs/2026-08-10-ddl-selection-design.md) §4.1.

### Queries panel

The panel takes text, so while it has focus every printable key is a
character and not a binding. What it does not claim keeps its global meaning.

| Key | Action |
|---|---|
| `ctrl-r` | Run the whole buffer. Its page replaces whatever Contents was showing. A buffer that is empty or all whitespace runs nothing, and the bar greys the hint out while that is so — §2. |
| any printable key | Insert it. `q`, `t`, `j` and the rest are text here. |
| `Enter` | Split the line at the cursor. |
| `Backspace` / `Delete` | Delete before / at the cursor; at a line's edge, the line break. |
| `←` `→` `↑` `↓` | Move the cursor. `←`/`→` wrap between lines. |
| `Home` / `End` | To the start / end of the **line**. |
| `PageUp` / `PageDown` | Up / down one panel height. |
| `Esc` | Leave the editor for Tables. |

`Tab` and `Shift-Tab` still move focus. Nothing else global survives inside
the editor — including `q`, which types a `q`. `ctrl-r` is not global either:
it is the ctrl layer's Queries member and does nothing outside this panel,
just as `ctrl-e`/`ctrl-y` do nothing outside Table DDL.
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

This now has three exceptions, in that order of precedence. First, a modifier:
`handle_key` returns on alt and routes ctrl into `ctrl_key`, the layer's own
key table, before either check below — so `ctrl-r` is never a plain `r`, in the
editor or in the palette (`ctrl_key` repeats the palette guard itself, for
exactly that reason). Then an open command palette claims every unmodified key
before anything else is consulted, because it is drawn over the panels and the
panel underneath is not what the user is looking at. Failing that, a panel that
takes text claims the printable and editing keys before the table is reached.
Everything none of the three claims is still dispatched globally and still
refused by panels that have nothing to do with it.

**Match order is precedence.** The panel-focus arm is last, so it can only claim
letters no earlier arm took. Adding an arm above it can silently steal a focus
key; the test that catches this is named in §5.

**Lowercase only.** `T`, `A`, `C`, `U` arrive as distinct `Key::Char`s and stay
unbound. Shift is never consulted for letters, and a shifted arrow is left as a
synonym for the plain one so `Shift-↓` stays free for a future "extend
selection".

**Ctrl is one designed layer; alt is refused wholesale.** `handle_key` routes a
ctrl-modified key into `ctrl_key`, which is the layer's whole key table, and
returns early on alt and on ctrl+alt together — a two-modifier chord is a third
layer, undesigned. What the rule protects is unchanged: a ctrl key is a binding
of its own and never a synonym for the plain one, so `ctrl-j` still does
nothing, and a member pressed outside its panel is inert rather than a binding
for whatever is focused. The layer has three members. `ctrl-e` and `ctrl-y`
scroll the Table DDL viewport without moving its cursor, which is the one
panel whose cursor and viewport can disagree
([ddl selection design](specs/2026-08-10-ddl-selection-design.md) §4.1).
`ctrl-r` runs the Queries buffer: the panel takes text, so every printable key
there is a character and the action the panel exists for has no unmodified key
available ([query execution design](specs/2026-08-10-query-execution-design.md)
§1). Each member gates on its own panel; `ctrl_key` itself only refuses the
palette. `ctrl-d`/`ctrl-u` are still unbound and still want designing.

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
([design](specs/2026-08-09-hint-bar-design.md) §6). `Esc` is named in the two
bars where it is unconditionally true that pressing it does something —
Queries, where it leaves the editor for Tables, and an open palette, where it
closes it — and stays out of the other three, where it still only does
something when an overlay is up, and naming it there would still be a lie in
most frames (design §3); `Shift-Tab` is `Tab`'s inverse, and Tables'
`PageDown`/`PageUp`/`Home`/`End` are increments on a move the bar already names
as `j/k move` — neither teaches something the bar's existing hints don't. A
binding that is in none of a title, the bar, or the palette is a binding only
this file knows about.

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

**Focus is the mode, and an overlay outranks focus.** A panel either takes
text or it does not, and `Panel::takes_text` is the one exhaustive answer. In a
panel that takes text every printable key is a character; everywhere else every
printable key is a binding. There is no state within a panel that changes what
a key means, so the regime is always the focused border — which is on screen
and already means something. This replaces the older "no modes" rule, which
held only until the Queries panel took text.

The command palette sits above that. It is opened deliberately with `?`, closes
with either `?` or `esc` — both named on its bar
([design](specs/2026-08-09-command-palette-design.md) §7) — and while it is up it
takes motion, leaves `q` quitting, and makes every other key inert rather than
dismissing. A key that means "close" in one frame and something else in the
next is the ambiguity both halves of this rule exist to prevent.

**No key without a test.** Every binding above is pinned by name in §5.

---

## 3. The key budget

Taken, lowercase: `a` `c` `h` `j` `k` `l` `q` `t` `u`, plus `Space` and `?`.

Free, lowercase: `b` `d` `e` `f` `g` `i` `m` `n` `o` `p` `r` `s` `v` `w` `x`
`y` `z`, every digit, and every uppercase letter.

Named keys taken: `Enter` `Esc` `Tab` `Shift-Tab` `Home` `End` `PageUp`
`PageDown` `↑` `↓` `←` `→`, plus `Backspace` and `Delete`, which only the
editor binds, inside Queries.

Ctrl taken: `ctrl-e` and `ctrl-y` in Table DDL, `ctrl-r` in Queries. The rest
of the layer is free — including `e`, `y` and `r` themselves everywhere but the
panel that claims them — with `ctrl-d`/`ctrl-u` spoken for (§4). Alt is not a
layer and nothing may claim from it.

Named keys the domain understands but nothing binds: `Insert` `F1`–`F12`.
Anything crossterm reports outside that vocabulary is dropped at the edge and
never reaches the domain. The pointer is not in this budget at all — see §7.

The ctrl namespace is separate from the plain-key budget above: `r` is free as
a plain key and taken behind ctrl.

Claim from the free set first. Re-using a taken key means retiring a binding,
which is a spec-level change.

> Every printable key is also text in a panel that takes text. That does not
> spend it: `d` is still free to be bound for the other three panels.

---

## 4. Deliberately unbound

| Not bound | Why |
|---|---|
| `1`–`4` focus | Superseded by `t`/`a`/`c`/`u`, which the panel can show in its own title; a digit cannot. |
| `T` `A` `C` `U` | Reserved. A shifted letter is a binding of its own, not a synonym. |
| `ctrl-d` / `ctrl-u` | The ctrl layer exists now but these two are not in it yet; see §2. |
| `r` refresh | Nothing re-issues `Effect::LoadSchema` after a racing DDL change; recovery is a restart. The binding waits on the reload path, not the other way round (TODO.md). |
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
| Motion is the focused panel's only | `motion_belongs_to_the_focused_panel_only`, `motion_in_one_panel_never_reaches_another` |
| `Enter`/`Space` agree | `enter_and_space_both_toggle_a_keyspace`, `space_reads_a_table_the_same_way_enter_does` |
| `l`/`h` and arrows agree | `l_and_right_open_a_keyspace`, `h_and_left_close_a_keyspace` |
| `h` steps out before folding | `h_from_a_table_steps_out_to_its_keyspace_before_folding` |
| Tree keys are Tables-only | `a_tree_transition_belongs_to_the_focused_panel_only`, `enter_outside_the_tree_reads_nothing` |
| `q` quits and cancels | `q_quits`, `quitting_cancels_everything_still_in_flight` |
| `Esc` clears the overlay | `escape_dismisses_the_overlay` |
| `Tab`/`Shift-Tab` cycle and wrap | `tab_walks_the_panels_in_reading_order_and_wraps`, `shift_tab_walks_them_the_other_way` |
| Paging and `Home`/`End` | `page_down_moves_the_tree_by_one_panel_height`, `home_and_end_reach_the_ends_of_the_tree_in_one_key` |
| Ctrl/alt are not synonyms | `an_unbound_ctrl_key_does_nothing`, `an_alt_modified_key_is_not_a_binding` |
| `ctrl-e`/`ctrl-y` scroll Table DDL's viewport and leave its cursor alone | `ctrl_e_and_ctrl_y_scroll_the_ddl_panel_without_moving_its_cursor` |
| `ctrl-e` belongs to Table DDL alone | `ctrl_e_belongs_to_the_ddl_panel_alone` |
| An open palette still claims it first | `an_open_palette_swallows_ctrl_e` |
| Shifted letters stay free | `a_shifted_shortcut_stays_unbound` |
| Focus letters are distinct and total | `every_panel_has_a_distinct_shortcut`, `a_shortcut_leads_back_to_the_panel_that_claims_it`, `the_shortcuts_are_the_ones_the_design_names` — all in `state.rs` |
| No focus letter is shadowed by an earlier arm | `a_shortcut_focuses_its_panel_from_any_panel_that_does_not_take_text` |
| A focus letter is text in a panel that takes text | `a_shortcut_is_text_in_a_panel_that_takes_text` |
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
| The cursor never lands on a heading | `the_palette_cursor_steps_over_a_heading`, `paging_settles_on_a_command_rather_than_a_heading` (`commands.rs`) |
| No row outgrows the narrowest palette | `no_command_row_is_wider_than_the_narrowest_palette` (`commands.rs`) |
| The bar names `?`, and the palette's own keys | `the_bar_names_the_palette_between_panel_and_quit`, `the_bar_names_what_the_palette_responds_to` (`view/hints.rs`) |
| The bar drops `?` where no palette would fit | `the_bar_drops_the_palette_hint_where_no_palette_would_fit`, `every_global_hint_does_something_in_every_panel` (`view/hints.rs`, `update.rs`) |
| `?` earns its place in the width-fitting bar, and goes before `q quit` but after `tab panel` | `a_frame_with_room_for_the_palette_shows_its_hint_and_drops_it_after_tab_panel` (`inbound/hints.rs`) |
| The palette is drawn, centred, capped and scrollable | `the_palette_lists_the_catalogue`, `the_palette_is_centred`, `a_wide_frame_does_not_get_a_wide_palette`, `a_tall_frame_does_not_get_a_palette_full_of_blank_rows`, `a_frame_too_small_for_a_palette_does_not_get_one` (`inbound/render.rs`) |
| `ctrl-r` runs the buffer | `ctrl_r_runs_the_query_buffer`, `a_run_query_titles_the_panel_with_what_was_typed` |
| It runs only from Queries | `ctrl_r_outside_queries_runs_nothing` |
| An empty buffer is inert | `an_empty_buffer_runs_nothing` |
| Either path cancels the other | `running_a_query_cancels_a_pending_table_read`, `reading_a_table_cancels_a_pending_query` |
| Destructive is refused, unexecuted | `a_destructive_statement_is_refused_until_confirmation_exists` |
| The posture gate still applies | `a_typed_write_is_refused_on_a_read_only_connection` |
| A second modifier is not a binding | `ctrl_alt_r_does_not_run_the_buffer` |
| The palette outranks the ctrl layer | `an_open_palette_swallows_ctrl_r` |
| A statement is classified by its first keyword | the `statement.rs` tests |
| A modifier reaches the label | `a_binding_behind_ctrl_is_named_with_a_caret` (`view/hints.rs`) |
| The bar and palette name it | `the_queries_bar_names_the_run_binding` (`view/hints.rs`), `the_palette_names_the_run_binding`, `the_palette_has_a_queries_section` (`commands.rs`) |
| The bar greys `ctrl-r run` while there is nothing to run | `the_run_hint_is_inert_while_there_is_nothing_to_run`, `the_run_hint_is_live_once_there_is_a_statement`, `nothing_but_the_run_hint_is_ever_inert` (`view/hints.rs`) |
| A greyed hint is drawn distinctly and keeps its width | `an_inert_hint_is_greyed_rather_than_dropped` (`inbound/hints.rs`) |
| The bar's sweep asks only what it claims | `every_hint_the_bar_shows_does_something` |
| The editing keys stay inside the editor | `the_editing_keys_do_nothing_outside_a_panel_that_takes_text` |

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

**Queries is the exception: the wheel over it does nothing.** `panel_scroll`
has no arm for `Panel::Queries` — the editor derives both offsets from the
cursor and the area every frame and stores none for a notch to move. Whether
it should get a stored viewport of its own is an open question (TODO.md).

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

**The wheel scrolls; it does not select.** Tables, Contents and Table DDL each
hold a viewport of their own, so a notch moves the rows (or, in Table DDL,
the lines) and leaves the selection — the cursor, in Table DDL — where you put
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
