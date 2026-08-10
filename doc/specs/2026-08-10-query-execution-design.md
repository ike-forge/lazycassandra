# Query execution — design

**Date:** 2026-08-10
**Status:** implemented

The Queries panel takes text and nothing more. A query can be typed and never
run, which makes the editor a notepad
([queries editor design](2026-08-09-queries-editor-design.md) opening: "This
slice builds the writable buffer and nothing else. Executing what is typed …
is its own slice").

This is that slice. `ctrl-r` runs the whole buffer, and its page lands in the
Contents panel.

Deliberately out of scope: selecting *which* statement to run when the buffer
holds several, the session log, completion, highlighting, and the typed
confirmation vision.md §4 principle 6 requires before a destructive statement
executes. The buffer is one statement, whole, or it is refused.

---

## 1. The ctrl layer opens

`doc/shortcuts.md` §2 says today:

> **Ctrl and alt are refused wholesale.** `handle_key` returns early on either
> modifier rather than folding the key into its unmodified meaning: `ctrl-j` is
> a different binding that does not exist yet, not a slower `j`. The ctrl layer
> gets designed as a layer (`ctrl-d`/`ctrl-u` are the first candidates,
> TODO.md), not smuggled in one key at a time.

Running the buffer has no unmodified key available. Queries takes text, so
every printable key there is a character: `r` types an `r`, and the letters are
spent for this panel by construction. What is left is the named keys the editor
has not claimed — `Insert`, `F1`–`F12` — and the modifier layers.

`ctrl-r` is chosen over `F5`. It is what a query tool binds, it is a control
character in every terminal (0x12) so it needs no keyboard-protocol
negotiation, and an F-key is a worse fit for the most-pressed binding in the
panel the tool exists for. Against it: `ctrl-r` is reverse-history-search in
every shell. That collision is accepted — the reverse search it collides with
is a shell's, not this program's, and nothing here will ever bind an
incremental history search to the same key.

So the rule is rewritten rather than broken. Ctrl becomes a designed layer with
exactly one member, and the half of the rule that carries the weight survives
unchanged: **a ctrl key is a binding of its own, never a synonym for the plain
one.** `ctrl-j` still does nothing. Alt stays refused wholesale, and so does
ctrl+alt together — a two-modifier chord is a third layer, undesigned.

> **Superseded on merge (2026-08-10).** The layer did not open with one
> member. The Table DDL cursor slice
> ([design](2026-08-10-ddl-selection-design.md) §4.1) opened it in parallel
> with `ctrl-e`/`ctrl-y`, and the merge of the two branches landed a layer of
> three. The dispatcher is named `ctrl_key`, not `ctrl`. Everything the
> section argues for `ctrl-r` — the key choice, the shell collision, and the
> half of the rule that survives — still holds; only "exactly one member" and
> the function's name do not. `doc/shortcuts.md` §2 carries the current rule.

## 2. Where it is dispatched

`handle_key`'s early return splits in place. It stays above the palette check
and above the takes-text check, so no existing arm changes precedence:

```rust
if press.modifiers.alt {
    return Vec::new();
}

if press.modifiers.ctrl {
    return ctrl(state, press.key);
}
```

`ctrl` is the layer's whole key table:

```rust
fn ctrl(state: &mut State, key: Key) -> Vec<Effect> {
    // An open palette is what the user is looking at, and it claims every key
    // before the panels are consulted (command palette design §7). The layer
    // is dispatched above that check, so it honours the rule itself.
    if matches!(state.overlay, Some(Overlay::Commands(_))) {
        return Vec::new();
    }

    match key {
        Key::Char('r') => run(state),
        _ => Vec::new(),
    }
}
```

The palette guard is duplicated here rather than reordered into the palette
check, because moving the ctrl branch below that check would make ctrl-modified
keys reach `palette_key`, which matches on `press.key` alone — `ctrl-j` would
move the palette cursor, and the "ctrl is not a synonym" half of §1 would be
false inside an overlay.

`run` is gated on focus, the way `activate` is gated on `Panel::Tables`:
pressing `ctrl-r` from Tables, Ddl or Contents does nothing. The buffer is
edited in Queries and run from Queries. Re-running from the panel showing the
result is a plausible convenience and is deliberately not bound — it would be
the only action in the program reachable from a panel it does not belong to.

## 3. What is sent

The whole buffer, `state.queries.text()`, trimmed. Whitespace-only is a silent
no-op: nothing is submitted, no message is raised, and the Contents panel keeps
what it was showing. Pressing `ctrl-r` on an empty editor is a mistake with no
consequence, and an error box for it would be noise.

The buffer is sent as one statement. A buffer holding two `SELECT`s separated
by a semicolon is one statement containing a semicolon, and the cluster
rejects it — the failure surfaces through the ordinary `Msg::Failed` path with
the driver's own words. Splitting on statement boundaries needs a CQL parser
and is the next slice, not this one.

## 4. Classification

`submit` gates on `Statement::mutates()`, so free text needs a
[`StatementKind`]. `Statement` gains a second constructor beside `select_all`:

```rust
pub fn typed(cql: &str) -> Self
```

It classifies on the first keyword, ASCII-case-insensitively, after skipping
leading whitespace:

| First keyword | Kind |
|---|---|
| `SELECT` | `Read` |
| `INSERT`, `UPDATE`, `DELETE`, `BEGIN` | `Write` |
| `DROP`, `TRUNCATE` | `Destructive` |
| `CREATE`, `ALTER` | `Schema` |
| anything else | `Write` |

The default is the conservative one. An unrecognised verb — a typo, a CQL
construct this table has not learned, a statement this program has never seen —
is treated as mutating, so it is refused on a read-only connection rather than
waved through it. Being wrong in that direction costs a user an error message;
being wrong in the other costs them a write they had asked the posture to
prevent.

This is a keyword sniffer and not a parser, and it is wrong in ways a parser
would not be: a leading comment (`-- note\nSELECT …`) classifies as `Write` and
is refused read-only. That is the conservative failure, it is honest about what
the sniffer knows, and it disappears when the parser the next slice needs
arrives. `Statement::typed` is where that parser lands, so nothing outside
`statement.rs` has to change when it does.

Classification never *widens* what a posture permits — it only decides which
side of the existing gate a statement falls on. The gate itself
(`submit`'s `stmt.mutates() && !posture.permits_mutation()`) is untouched.

## 5. Destructive statements are refused

`Destructive` never reaches `submit`. `run` raises the same
`Overlay::Message` a failed connection does, with `Severity::Danger`, saying
that dropping and truncating need the typed confirmation that is not built
yet, and returns no effects.

vision.md §4 principle 6 makes confirmation the condition on these statements
executing at all. Until the confirmation flow exists, the honest state is
"refused", not "executed because the gate is unbuilt". When it lands, this
refusal becomes the prompt — a call site that already exists, rather than a
gate that has to be remembered and added.

A `Destructive` statement on a read-only connection is refused here, before the
posture is consulted. Both answers are "no"; this one names the reason that
will still be true after the connection is made writable.

## 6. The Contents panel takes a second kind of subject

`ContentsState`'s three loaded-ish variants each carry a `TableRef`. That field
becomes a `Subject`:

```rust
pub enum Subject {
    Table(TableRef),
    Typed(String),
}
```

`TableRef` stays exactly as it is, including `qualified()` — it is what the
tree hands in, and a page that outlives a schema reload still has to say which
table it came from. `Subject::label()` is what the title formats:
`keyspace.table` for a table, and for a typed statement the statement itself
with every run of whitespace — newlines included — collapsed to a single
space, so a buffer written over four lines still titles the panel in one. The
renderer's block title clips what does not fit; nothing truncates in the
domain.

`ContentsPanel::requested`, `page_ready`, `refused`, `failed` and `awaited`
take and return `Subject` instead of `TableRef`. No new state, no second
panel, no branch on which kind of subject is loading — a page is a page.

Running a query cancels whatever read Contents was waiting on, and reading a
table cancels whatever query it was waiting on. That is one rule, and it is
already written once in `read()`: the cancel-the-outstanding-then-submit body
is extracted to take a `Subject` and a `Statement`, and `read()` and `run()`
each call it with theirs. One grid, one outstanding query, both directions.

The panel's empty-state text ("Press Enter on a table") stays as it is. It
describes the tree, the panel starts empty before either path has run, and
naming both ways in is a hint bar's job.

## 7. A binding grows a modifier

`view::hints::Binding` is `{ keys: &'static [Key], action: &'static str }`,
and its doc comment states the invariant that makes the bar trustworthy:

> Not a display string: a string could name a key no arm of `handle_key`
> claims and nothing would ever catch it, and a bar that lies is worse than no
> bar. `update::tests` presses every one of these.

A ctrl binding recorded as a bare `Key::Char('r')` would break that sweep in
the worst way: the sweep would press an unmodified `r`, which in Queries is
text, and would conclude the bar is honest about a binding it never tested.

So `Binding` gains `modifiers: Modifiers`, the type `KeyPress` already carries.
Every existing literal gains `modifiers: Modifiers::NONE` — a new `const`
on `Modifiers` — which is mechanical churn across `view/hints.rs` and
`commands.rs` and buys the sweep the ability to press a binding *as written*:
`KeyPress { key, modifiers: binding.modifiers }`.

`hints::label` prefixes `ctrl-` when the modifier is set, so the bar and the
palette both render `ctrl-r run`. A bool field is rejected for the reason
conventions.md §1 rejects bools generally, and `Modifiers` is not a new
vocabulary — it is the one the domain already uses for exactly this.

> **Superseded on merge (2026-08-10).** `Binding` did not grow a `modifiers`
> field. The Table DDL cursor slice reached the same conclusion from the same
> premise and answered it by changing `keys` to `&'static [KeyPress]`
> (master, `refactor(hints): key a binding by a press rather than a key`),
> which carries the modifier per key rather than per binding. That shape won
> the merge: it is strictly more expressive, and it needs no `Modifiers` field
> on a type whose whole job is to name presses. `Modifiers::NONE` survives, as
> `KeyPress::new`'s body. The label is `^r` rather than `ctrl-r`: `hints::name`
> prefixes a caret per press, which is master's convention and the one the
> `^e`/`^y` hints already use. The tests this section named,
> `a_ctrl_binding_is_labelled_with_its_modifier` and
> `an_unmodified_binding_is_labelled_bare`, were dropped as duplicates of
> master's `a_binding_behind_ctrl_is_named_with_a_caret`, which asserts both
> halves. The sweeps press `binding.keys` directly rather than rebuilding a
> `KeyPress` from a separate field.

## 8. Visibility

`QUERIES_HINTS` gains `ctrl-r run`, first in the list: it is what the panel is
for, and `⏎ newline` and `esc leave` are what it does while you get there.

The palette has no Queries section at all — the editor slice added none, so a
panel that takes text is the one thing the catalogue's completeness claim does
not currently cover. `ctrl-r` cannot be added to a section that does not exist,
so the section is added whole: `ctrl-r run`, and the editor's own non-text
bindings (`⏎`, `bksp`/`del`, the arrows, `home`/`end`, `esc`). Listing only
`ctrl-r` under a heading called "Queries" would say the panel does one thing.

That has a consequence for the two catalogue sweeps in `update.rs`. Both share
`sweep_fixtures()`, and none of its four states focuses Queries — deliberately,
because `no_key_does_anything_the_palette_does_not_list` asks a question with no
meaning in a panel where every printable key is text. So the fixtures split:
that sweep keeps the four it has, and `every_key_the_palette_lists_does_something`
answers the Queries group from an editing state — the only one where `bksp`,
`del` and `ctrl-r` can answer — and every other group from the four. Each
group gets the state it describes and no other. The keys that leave the "not
listed" sweep's reach — `bksp` and `del`, newly catalogued — get a test of
their own saying they do nothing outside a panel that takes text.

`ctrl-r run` leads the Queries bar, and on an empty or whitespace-only buffer
it runs nothing (§3). The hint is greyed rather than hidden in that frame: a
`Hint` carries a `Liveness`, `hints::bar` marks the run hint `Inert` while
`state.queries.text().trim()` is empty, and `lc-inbound` draws an inert hint
with `Token::Muted` for both spans — same place, same width, without the bold
a live key carries. `every_hint_the_bar_shows_does_something` sweeps the live
hints only, which is the honest form of what the bar claims
(`doc/shortcuts.md` §2).

`doc/shortcuts.md` changes in the same commit: §1's Queries table gains the
row, §2's ctrl rule is rewritten as §1 above describes, §3 gains a ctrl line to
the budget (`ctrl-r` taken; the rest of the layer free, `ctrl-d`/`ctrl-u`
still spoken for by TODO.md), §4 drops nothing, and §5 gains the rows below.

## 9. Tests

In `update.rs` unless noted.

| Guarantee | Test |
|---|---|
| The buffer runs, and a typed read survives a read-only connection | `ctrl_r_runs_the_query_buffer` |
| It runs only from Queries | `ctrl_r_outside_queries_runs_nothing` |
| An empty or whitespace-only buffer is inert | `an_empty_buffer_runs_nothing` |
| The result lands in Contents | `a_run_query_titles_the_panel_with_what_was_typed` |
| Either path cancels the other | `running_a_query_cancels_a_pending_table_read`, `reading_a_table_cancels_a_pending_query` |
| Destructive is refused, unexecuted | `a_destructive_statement_is_refused_until_confirmation_exists` |
| The posture gate still applies | `a_typed_write_is_refused_on_a_read_only_connection` |
| Ctrl is still not a synonym | `an_unbound_ctrl_key_does_nothing` |
| Alt is still refused wholesale | `an_alt_modified_key_is_not_a_binding`, `ctrl_alt_r_does_not_run_the_buffer` |
| The palette outranks the ctrl layer | `an_open_palette_swallows_ctrl_r` |
| Classification | the `model/statement.rs` tests: `selecting_is_a_read`, `an_unknown_verb_is_treated_as_mutating`, `case_and_leading_space_do_not_change_a_kind`, `dropping_and_truncating_are_destructive`, `creating_and_altering_are_schema_changes`, `the_cql_is_kept_verbatim` |
| A typed subject titles the panel in one line | `a_typed_subject_collapses_to_one_line` (`contents.rs`) |
| The bar and palette name it | `the_queries_bar_names_the_run_binding` (`view/hints.rs`), `the_palette_names_the_run_binding`, `the_palette_has_a_queries_section` (`commands.rs`) |
| The bar greys it while the buffer has nothing to run | `the_run_hint_is_inert_while_there_is_nothing_to_run`, `the_run_hint_is_live_once_there_is_a_statement`, `nothing_but_the_run_hint_is_ever_inert` (`view/hints.rs`), `an_inert_hint_is_greyed_rather_than_dropped` (`lc-inbound`'s `hints.rs`) |
| A modifier reaches the label | `a_binding_behind_ctrl_is_named_with_a_caret` (`view/hints.rs`) — see §7's note |
| Every binding is pressed as written | the sweeps, pressing `binding.keys` whole: `every_key_the_palette_lists_does_something`, `no_key_does_anything_the_palette_does_not_list`, `every_hint_the_bar_shows_does_something` |
| The editing keys stay inside the editor | `the_editing_keys_do_nothing_outside_a_panel_that_takes_text` |

The Queries group of `every_key_the_palette_lists_does_something` is answered
by the editor fixture *alone*, not by that fixture alongside the other four:
`⏎`, `esc` and `home`/`end` all do something in a non-editor state too, so a
sweep that accepts any fixture's answer would let the editor stop claiming
them and stay green.

The existing `a_modified_key_is_not_a_binding` is narrowed rather than deleted:
it asserted that *any* ctrl key is inert, which stops being true. It becomes
`an_unbound_ctrl_key_does_nothing`, over a key the layer does not claim.

## 10. What this leaves open

- **Running one statement of several.** Needs the CQL parser; `Statement::typed`
  is where it lands (§4).
- **The typed confirmation** destructive statements wait on (§5).
- **Paging a query's result.** `Effect::FetchNextPage` exists and Contents does
  not drive it for a table read either. Unchanged here, and unchanged for both
  paths at once when it arrives.
- **Re-running from Contents** (§2), and any second member of the ctrl layer.
