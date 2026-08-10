# Query execution — design

**Date:** 2026-08-10
**Status:** designed

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
`keyspace.table` for a table, and for a typed statement its first line with
runs of whitespace collapsed to single spaces. The renderer's block title
clips what does not fit; nothing truncates in the domain.

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

## 8. Visibility

`QUERIES_HINTS` gains `ctrl-r run`, first in the list: it is what the panel is
for, and `⏎ newline` and `esc leave` are what it does while you get there.

The palette's Queries section gains the same row. `commands.rs` has a test
that no row outgrows the narrowest palette; `ctrl-r` is four columns wider than
the widest key label there today, and the row is short enough that it holds.

`doc/shortcuts.md` changes in the same commit: §1's Queries table gains the
row, §2's ctrl rule is rewritten as §1 above describes, §3 gains a ctrl line to
the budget (`ctrl-r` taken; the rest of the layer free, `ctrl-d`/`ctrl-u`
still spoken for by TODO.md), §4 drops nothing, and §5 gains the rows below.

## 9. Tests

In `update.rs` unless noted.

| Guarantee | Test |
|---|---|
| The buffer runs | `ctrl_r_runs_the_query_buffer` |
| It runs only from Queries | `ctrl_r_outside_queries_runs_nothing` |
| An empty buffer is inert | `an_empty_buffer_runs_nothing`, `a_whitespace_only_buffer_runs_nothing` |
| The result lands in Contents | `a_run_query_loads_its_page_into_contents` |
| Either path cancels the other | `running_a_query_cancels_a_pending_table_read`, `reading_a_table_cancels_a_pending_query` |
| Destructive is refused, unexecuted | `a_destructive_statement_is_refused_until_confirmation_exists`, `a_refused_destructive_statement_submits_nothing` |
| The posture gate still applies | `a_typed_write_is_refused_on_a_read_only_connection`, `a_typed_read_survives_a_read_only_connection` |
| Ctrl is still not a synonym | `an_unbound_ctrl_key_does_nothing`, `ctrl_j_does_not_move_a_selection` |
| Alt is still refused wholesale | `an_alt_modified_key_is_not_a_binding`, `ctrl_alt_r_does_not_run_the_buffer` |
| The palette outranks the ctrl layer | `an_open_palette_swallows_ctrl_r` |
| Classification | the `statement.rs` tests: `select_is_a_read`, `an_unknown_verb_is_treated_as_mutating`, `case_does_not_change_a_statements_kind`, `drop_and_truncate_are_destructive` |
| A typed subject titles the panel | `a_typed_statement_titles_the_panel_with_its_first_line`, `a_multi_line_statement_collapses_to_one_title_line` (`view/contents.rs`) |
| The bar and palette name it | `the_queries_bar_names_the_run_binding` (`view/hints.rs`), `the_palette_names_the_run_binding` (`commands.rs`) |
| Every binding is pressed as written | the existing sweep, now pressing `binding.modifiers` |

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
