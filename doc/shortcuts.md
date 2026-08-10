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
unconditional; the seven marked rows are text instead, inside the one panel
that takes text — see the Queries subsection below.

| Key | Action |
|---|---|
| `q`¹ | Quit. Cancels every in-flight query. |
| `?`¹ | Open the command palette: every binding, in one scrollable block. |
| `/`¹ | Search the focused panel. |
| `Esc` | Dismiss the overlay, clear the focused panel's filter, or leave the editor for Tables — first hit wins (§2). |
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

### The search prompt

`/` opens it over Tables, Table DDL or Table contents — the only panels a
filter can apply to. It is drawn on the bar row, not over the panels, but it
is an `Overlay` exactly like the command palette: it outranks focus and
claims every printable key before the panel underneath ever sees one, `q`
and `?` included — see §2's third exception.

| Key | Effect |
|---|---|
| any printable key | Appends to the query, then applies it live. |
| `Backspace` | Removes the last character, then applies. |
| `Enter` | Closes the prompt, leaving whatever filter is applied. |
| `Esc` | Restores the filter that was applied before the prompt opened, then closes. |
| anything else | Inert, including `Tab`/`Shift-Tab` — the panel being filtered cannot change under a live query. |

Filtering is live: every edit re-applies the filter, so the panel underneath
is the preview. `Enter` on an empty query therefore clears the filter — there
is no separate binding for that, only the empty query the invariant already
turns into "no filter".
See [panel search design](specs/2026-08-10-panel-search-design.md) §3.

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

`l` and `h` on a keyspace are swallowed while a filter is applied — and so
are `Enter`/`Space` and a click, on a keyspace row, for the same reason. A
retained keyspace is forced open by the filter itself, so folding it would
change nothing on screen — the projection puts it right back — and a key
that looks broken is worse than a key that does nothing (panel search design
§4.1). This is the existing rule that a panel which cannot honour a motion
swallows it rather than passing it on (§2). `Enter`/`Space` and a click still
read a table into Contents under a filter; only the fold/unfold path is
swallowed.

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

This now has three exceptions, in that order of precedence. An open command
palette claims every key before anything else is consulted, because it is drawn
over the panels and the panel underneath is not what the user is looking at.
Failing that, an open search prompt claims every key the same way: it is the
same shape of thing — a state that outranks focus, entered with `/` and left
with `Enter` or `Esc` — but it claims every printable key as text rather than
as motion, `q` and `?` typing themselves exactly as they would in a panel
that takes text (panel search design §1.1). Failing that, a panel that takes
text claims the printable and editing keys before the table is reached.
Everything none of the three claims is still dispatched globally and still
refused by panels that have nothing to do with it.

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
([design](specs/2026-08-09-hint-bar-design.md) §6). `Esc` is named wherever it
is unconditionally true, in that frame, that pressing it does something:
Queries, where it leaves the editor for Tables; an open palette or search
prompt, where it closes it; and a panel carrying a filter, where it clears
one (panel search design §5.3). It stays out of a panel with neither an
overlay up nor a filter applied, where it still only does something
conditionally, and naming it there would still be a lie in most frames
(design §3); `Shift-Tab` is `Tab`'s inverse, and Tables'
`PageDown`/`PageUp`/`Home`/`End` are increments on a move the bar already names
as `j/k move` — neither teaches something the bar's existing hints don't. A
binding that is in none of a title, the bar, or the palette is a binding only
this file knows about.

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

The search prompt sits at the same rank, entered deliberately with `/` and
left with `Enter` or `Esc`. Unlike the palette it takes text, not motion:
every printable key appends to the query rather than doing anything else, `q`
and `?` included — the panel-that-takes-text rule applied to an overlay
(panel search design §1.1).

**`Esc` resolves in one order, first hit wins.** An overlay up is dismissed
first — a search prompt cancels, restoring the filter it found applied; the
palette just closes. Failing that, the focused panel's filter, if it has one,
clears. Failing that, Queries hands focus back to Tables. Failing all three,
`Esc` does nothing. The first rung preserves
`escape_dismisses_the_overlay_before_it_leaves_the_editor`; extending it down
the chain is what "the thing most recently put on screen comes off first"
means once a panel can carry state of its own (panel search design §6).

**No key without a test.** Every binding above is pinned by name in §5.

---

## 3. The key budget

Taken, lowercase: `a` `c` `h` `j` `k` `l` `q` `t` `u`, plus `Space`, `?` and
`/`.

Free, lowercase: `b` `d` `e` `f` `g` `i` `m` `n` `o` `p` `r` `s` `v` `w` `x`
`y` `z`, every digit, and every uppercase letter.

Named keys taken: `Enter` `Esc` `Tab` `Shift-Tab` `Home` `End` `PageUp`
`PageDown` `↑` `↓` `←` `→`, plus `Backspace` and `Delete`, which only the
editor binds, inside Queries.

Named keys the domain understands but nothing binds: `Insert` `F1`–`F12`.
Anything crossterm reports outside that vocabulary is dropped at the edge and
never reaches the domain. The pointer is not in this budget at all — see §7.

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
| `ctrl-d` / `ctrl-u` | Wants the ctrl layer designed first; see §2. |
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
| An empty query is no filter | `an_empty_query_is_not_a_filter` (`search.rs`) |
| Matching folds case, and is a substring, literally | `matching_ignores_case_in_both_directions`, `matching_is_a_substring_anywhere_and_not_a_prefix`, `a_metacharacter_is_matched_literally`, `folding_reaches_past_ascii` (`search.rs`) |
| The term is kept as typed, for the marker | `the_term_is_kept_as_typed_for_the_marker` (`search.rs`) |
| `Filtered` refuses Queries and accepts the rest | `every_panel_but_queries_can_hold_a_filter`, `a_target_names_the_panel_it_came_from` (`search.rs`) |
| A matching keyspace brings all its tables | `a_matching_keyspace_brings_all_of_its_tables` (`tree.rs`) |
| A matching table brings its keyspace and no sibling | `a_matching_table_brings_its_keyspace_and_no_sibling` (`tree.rs`) |
| A keyspace with nothing matching is dropped | `a_keyspace_with_nothing_matching_is_dropped` (`tree.rs`) |
| A retained keyspace is drawn open whatever its fold state | `a_retained_keyspace_is_drawn_open_whatever_its_fold_state`, `a_keyspace_matching_by_name_is_open_only_when_it_has_tables` (`tree.rs`) |
| Clearing the filter restores the fold state exactly | `clearing_the_filter_restores_the_fold_state_exactly` (`tree.rs`) |
| `l`/`h` are swallowed while a filter is applied | `l_and_h_are_swallowed_while_a_filter_is_applied`, `folding_under_a_filter_does_not_disturb_the_fold_state_it_will_return_to`, `l_and_h_still_fold_once_the_filter_is_gone` |
| The cursor clamps as a filter shrinks the tree | `the_cursor_clamps_as_a_filter_shrinks_the_tree` |
| A property matches on its name and on its value | `a_property_matches_on_its_name`, `a_property_matches_on_its_value` (`view/details.rs`) |
| A column matches on its name and on its CQL type | `a_column_matches_on_its_name`, `a_column_matches_on_its_cql_type` (`view/details.rs`) |
| A section emptied by the filter disappears | `a_section_emptied_by_the_filter_disappears` (`view/details.rs`) |
| Headings are not matched | `a_heading_is_not_matched` (`view/details.rs`) |
| Nothing matched gives a muted notice, in Tables and Table DDL | `a_filter_that_matches_nothing_says_so` (`view/tables.rs`), `nothing_matching_says_which_term_found_nothing` (`view/details.rs`) |
| A filter changes nothing unless one is applied | `an_unfiltered_empty_schema_still_says_no_keyspaces` (`view/tables.rs`), `an_unfiltered_table_keeps_its_existing_behaviour` (`view/details.rs`), `no_filter_flags_nothing` (`view/contents.rs`) |
| Matching cells are flagged and the grid keeps every row | `a_filter_flags_the_cells_that_match_and_keeps_every_row` (`view/contents.rs`) |
| A matched null is still a null | `a_matched_null_is_still_a_null` (`view/contents.rs`) |
| `/` opens the prompt in each of the three panels that can hold a filter | `slash_opens_the_prompt_in_every_panel_that_can_hold_a_filter` |
| `/` is text in Queries and opens nothing | `slash_is_text_in_queries_and_opens_nothing` |
| `q` and `?` type themselves in the prompt | `q_and_the_question_mark_type_themselves_in_the_prompt` |
| The prompt filters live, per keystroke | `the_prompt_filters_live_per_keystroke`, `backspace_narrows_the_query_and_re_applies_it`, `backspace_on_an_empty_query_is_inert_rather_than_closing` |
| `Enter` commits and closes; an empty commit clears | `enter_commits_and_closes_leaving_the_filter_applied`, `an_empty_commit_clears_the_filter`, `backspacing_a_query_to_empty_clears_the_filter_live` |
| `Esc` cancels, restoring the filter that was there | `escape_cancels_the_prompt_restoring_the_filter_that_was_there` |
| `Esc`'s full precedence chain | `escape_clears_the_focused_panels_filter_when_no_overlay_is_up`, `escape_prefers_the_overlay_to_the_filter`, `escape_prefers_the_filter_to_leaving_the_editor` |
| `Tab` is inert while the prompt is up | `tab_is_inert_while_the_prompt_is_up` |
| A click cancels the prompt | `a_click_cancels_the_prompt` |
| A filter survives a focus change | `a_filter_survives_a_focus_change` |
| A filter is read back from the panel it was applied to | `a_filter_is_read_back_from_the_panel_it_was_applied_to` (`state.rs`) |
| The three filters are independent | `the_three_filters_are_independent` (`state.rs`) |
| A panel is projected carrying the filter applied to it | `a_panel_carries_the_filter_applied_to_it` (`view/mod.rs`) |
| An open prompt is projected as its query | `an_open_prompt_is_projected_as_its_query` (`view/mod.rs`) |
| The bar names `/ search` where it does something, and not in Queries | `the_bar_names_search_in_every_panel_that_can_hold_a_filter`, `the_bar_does_not_name_search_in_queries` (`view/hints.rs`) |
| The bar names `esc clear` only where a filter is applied | `the_bar_names_esc_clear_only_where_a_filter_is_applied` (`view/hints.rs`) |
| The bar names the prompt's own keys while it is up | `the_bar_names_what_the_prompt_responds_to` (`view/hints.rs`) |
| The palette lists `/` | `the_catalogue_lists_search` (`commands.rs`) |
| The marker is drawn, muted, right-aligned | `a_panel_wears_the_filter_applied_to_it`, `the_marker_is_drawn_muted` (`inbound/render.rs`) |
| The marker is dropped rather than truncated | `the_marker_is_dropped_rather_than_truncated`, `the_marker_fits_flush_against_the_corner_it_is_tuned_for` (`inbound/render.rs`) |
| An unfocused panel still wears its marker | `an_unfocused_panel_still_wears_its_marker` (`inbound/render.rs`) |
| The prompt owns the bar row, and its cursor | `an_open_prompt_owns_the_bar_row`, `an_open_prompts_own_cursor_replaces_the_editors`, `the_prompts_cursor_sits_after_the_query` (`inbound/render.rs`) |
| A matched cell reads apart from an unmatched one, on and off the selected row | `a_matched_cell_reads_apart_from_an_unmatched_one_on_and_off_the_selected_row` (`inbound/render.rs`) |
| `Token::Match` is legible over `Token::Selected` | `a_match_is_legible_over_a_selected_row` (`inbound/theme.rs`) |

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

**A click on an open search prompt cancels it**, restoring the filter that
was applied before the prompt opened — the same thing `Esc` does to it, and
for the same reason: a click that lands somewhere unexpected must not leave
state behind (panel search design §3.2).

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
