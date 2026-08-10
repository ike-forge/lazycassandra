# Themes — design

**Status:** approved, not implemented.
**Date:** 2026-08-10

A colour role palette, a default theme that still wears the terminal's
background, four Catppuccin flavours that repaint everything, and a runtime
picker with live preview.

---

## 1. What is wrong today

`lc-inbound/src/theme.rs` maps every semantic token onto one of the sixteen
ANSI colours plus two truecolor accents. It has a single variant,
`Theme::TerminalDefault`, chosen once in `main.rs` and never changed.

Two problems.

**Not enough distinctions exist.** The Tables tree draws a user keyspace and a
user table identically — both spend no token at all. Table options, query
syntax and the loader have no vocabulary of their own. The domain cannot ask
for a colour it has no token for, so the fix starts in `lc-domain/src/style.rs`.

**Adding a theme means adding a match arm per token.** Eighteen tokens times
five themes is ninety arms that must stay consistent by hand. Themes have to
become *data* before there can be five of them.

## 2. Amendment to vision.md §4, principle 5

Principle 5 currently reserves meaning — key structure, severity, write
posture — for the sixteen ANSI colours, so that it survives a terminal whose
palette is unusable. Accents are permitted only where something is *marked*
rather than *meant*.

That rule is withdrawn. It buys resilience against a configuration that is
close to extinct, and it costs the default theme every distinction that would
make the grid readable at a glance — which is the actual daily problem.

The principle keeps its name and narrows its claim:

> **Wear the terminal's clothes.** The background is yours and LazyCassandra
> never paints over it; body text keeps your terminal's own foreground. What
> is drawn *on top* is ours, truecolor included, chosen to read on a light and
> a dark terminal alike.

Two consequences follow.

- "Never sets a background" becomes "never paints a panel field". The cursor
  row is a background, deliberately: no single tint reads against both a light
  and a dark terminal, so the bar has to bring its own. The panel it sits in
  is still the user's.
- A poor-palette fallback is now a *theme's* job. If one is ever wanted it
  ships as a sixth flavour, not as a constraint on the other five.

`architecture.md` §6 is rewritten to match, and keeps its real claim: the
domain names meanings and never colours.

## 3. Roles

`Theme` becomes a `Palette` — a struct of named roles — and one
`Token -> Style` function shared by every flavour. A new flavour is a new
`const Palette`, not ninety match arms.

Four roles are `Option<Color>`. `None` means *not ours to set*, which puts the
promise of §2 in the type system rather than in a test comment.

| Role | Type | Drawn as |
|---|---|---|
| `base` | `Option<Color>` | the whole frame's background |
| `surface` | `Option<Color>` | an overlay's background, after `Clear` |
| `text` | `Option<Color>` | body text, `Severity::Info` |
| `border` | `Option<Color>` | panel borders |
| `highlight` | `Color` | the cursor row's background |
| `highlight_text` | `Option<Color>` | that row's text — see §3.1 |
| `muted` | `Color` | system keyspaces, hint actions, nulls, scrollbar track |
| `partition` | `Color` | partition key columns, bold |
| `clustering` | `Color` | clustering key columns |
| `keyspace` | `Color` | a user keyspace in the Tables tree |
| `table` | `Color` | a user table in the Tables tree |
| `property` | `Color` | table options and replication settings |
| `accent` | `Color` | the loader's scanner head |
| `match_` | `Color` | a cell the applied filter matched |
| `success` | `Color` | `Posture::Writable` |
| `warning` | `Color` | `Severity::Warning` |
| `danger` | `Color` | `Severity::Danger`, `Posture::Protected` |
| `keyword` | `Color` | CQL keywords |
| `reference` | `Color` | keyspace, table and column names in CQL |
| `literal` | `Color` | CQL string and numeric literals |

Three reductions against the roles as first listed:

- **Posture stops owning colours.** `Writable` rides on `success`, `Protected`
  on `danger` with bold, `ReadOnly` on `text` — read-only is the default
  posture and the baseline everything else is read against, so spending a
  colour on it would dilute the two that matter.
- **`Severity::Info` rides on `text`.** It is the baseline of its own scale for
  the same reason.
- **The three query roles reuse existing values in every flavour.** `reference`
  is the `table` colour, because a reference in CQL *is* a table or column
  name. They stay separate fields so a future CQL highlighter can pull them
  apart without touching the flavours.

### 3.1 Why `highlight_text` is optional

ratatui applies a highlight style with `Buffer::set_style`, which *overwrites*
a cell's foreground rather than filling one in. A row highlight carrying a
foreground therefore flattens the key-structure colours on the row it lands on;
one carrying only a background leaves them alone. There is no third option.

So each theme declares which it can afford:

- `highlight_text: None` — the theme owns its whole palette, has verified every
  role reads against `base`, and lets the cursor row keep its column colours.
  Every Catppuccin flavour is this.
- `highlight_text: Some(_)` — the theme cannot know what it is drawn over, so
  the cursor row forces a legible foreground and gives up its column colours
  for that one row. The default theme is this, and it is no worse than the
  `Modifier::REVERSED` it replaces, which flattened the row too.

`Option` rather than a `bool` beside a colour, so "forces a foreground" and
"which foreground" cannot be said apart (conventions.md §1).

### New tokens

`lc-domain/src/style.rs` gains `Keyspace`, `Table`, `Keyword`, `Reference` and
`Literal`. The last three are defined and mapped but not yet emitted — there is
no CQL highlighter. They are in this slice because a theme is not complete
without them and because a `Palette` field no token reads is dead code the
zero-warnings rule would reject.

## 4. The default palette

`base`, `surface`, `text` and `border` are `None`.

The theme cannot know whether it is drawn on white or on black, so every value
holds a WCAG contrast ratio of at least 3.0 against **both**. That narrows the
usable band to a relative luminance of roughly 0.10–0.30, which is why these
are deeper and more saturated than a scheme designed for one background would
be. It is a constraint, not a preference, and `the_default_palette_reads_on_
black_and_on_white` in §8 is what holds it.

| Role | Value | | vs. white | vs. black |
|---|---|---|---|---|
| `partition` | `#c9821f` | amber, bold | 3.1 | 6.7 |
| `clustering` | `#10847c` | teal | 4.6 | 4.6 |
| `keyspace` | `#8a4fd4` | mauve | 5.2 | 4.1 |
| `table` | `#2b6fd8` | blue | 4.9 | 4.4 |
| `property` | `#b8489a` | magenta | 4.8 | 4.4 |
| `accent` | `#d1662f` | rust | 3.7 | 5.7 |
| `match_` | `#5f8f16` | olive | 3.9 | 5.4 |
| `muted` | `#7a7f8f` | grey | 4.0 | 5.3 |
| `success` | `#2e8b4f` | green | 4.2 | 4.9 |
| `warning` | `#a8781a` | ochre | 3.9 | 5.3 |
| `danger` | `#c8322f` | red | 5.3 | 4.0 |
| `highlight` | `#45485a` | | — | — |
| `highlight_text` | `Some(#e8eaf2)` | 7.2 against `highlight` | | |
| `keyword` | `#8a4fd4` | = `keyspace` | | |
| `reference` | `#2b6fd8` | = `table` | | |
| `literal` | `#2e8b4f` | = `success` | | |

This is a first draft to iterate on, not a finished scheme. The tests in §8 are
what stop iteration from producing something unreadable.

## 5. The four Catppuccin flavours

One recipe, four ingredient sets. Each role names a Catppuccin colour, so the
flavours cannot drift apart and a fifth costs one table of hex values.

| Role | Catppuccin colour |
|---|---|
| `base` | `base` |
| `surface` | `surface0` |
| `text` | `text` |
| `border` | `surface1` |
| `highlight` | `surface1` |
| `highlight_text` | `None` |
| `muted` | `overlay1` |
| `partition` | `yellow`, bold |
| `clustering` | `teal` |
| `keyspace` | `mauve` |
| `table` | `blue` |
| `property` | `pink` |
| `accent` | `peach` |
| `match_` | `flamingo` |
| `success` | `green` |
| `warning` | `yellow` |
| `danger` | `red` |
| `keyword` | `mauve` |
| `reference` | `blue` |
| `literal` | `green` |

`partition` and `warning` are both `yellow`. They never share a surface — one
is a grid column header and its cells, the other a status message — and
collapsing them keeps the flavour tables short.

Every flavour takes `highlight_text: None` (§3.1) — each knows its own `base`
and has its roles verified against it, so the cursor row is a bar of `surface1`
and the partition key on it stays the partition key's colour.

Latte is the light flavour, and it is where that pays: `text` is `#4c4f69` and
`surface1` is `#bcc0cc`, the opposite arrangement to every dark flavour, and
neither the recipe nor the token mapping has to know which it is.

Bases: Latte `#eff1f5`, Frappé `#303446`, Macchiato `#24273a`, Mocha `#1e1e2e`.
The full ingredient tables are transcribed into `theme/catppuccin.rs` from the
upstream palette.

## 6. Choosing a theme

The domain names themes, never colours. `architecture.md` §6 holds: a theme
*name* is not a colour.

`lc-domain/src/theme.rs` (new) defines:

```rust
pub enum ThemeChoice {
    TerminalDefault,
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
}
```

with its ordered list and its display labels. `State.theme` holds one.
`lc-inbound` implements `From<ThemeChoice> for Theme`; `lc-app` drops the
`Theme` it holds today and renders with `Theme::from(view.theme)`.

There is no CLI flag and no config key. Persistence arrives with the config
subsystem of `architecture.md` §7, which is a separate design; until then a
theme lasts for a session.

### The picker

A new `Overlay::Themes { cursor, restored }`, reusing the centred-list
machinery of `palette.rs`.

| Key | Effect |
|---|---|
| `T` | Opens it, cursor on the current theme, `restored` recording it. |
| `j` / `k`, `Down` / `Up` | Moves the cursor **and sets `State.theme`** — the app repaints in that theme immediately. |
| `Enter` | Closes, keeping whatever is set. |
| `Esc` | Sets `State.theme` back to `restored`, then closes. |

`restored` is the entire cost of live preview, and it is why the picker is a
struct variant rather than a cursor.

`T` is free — `Shift-Tab` is the only shift binding today. The picker is also
reachable from the command palette, which gains a row for it; that row may push
`palette.rs`'s derived `MIN_W`, and the test over the catalogue is what will
say so.

Overlay precedence is unchanged: the picker follows the rules in
`shortcuts.md` §2 exactly as the command palette does.

## 7. Rendering

- `render.rs` paints `base` across the frame before anything else when it is
  `Some`, and gives every panel `Block` the `border` colour when it is `Some`.
- `overlay.rs`, `palette.rs` and the new `themes.rs` paint `surface` after
  `Clear`, so an overlay separates from the field it covers. On the dark
  flavours `surface0` sits above `base` and reads as lifted; on Latte it sits
  below and reads as inset. Distinctness is the requirement, not a direction.
- `Token::Selected` stops being `Modifier::REVERSED` and becomes
  `bg(highlight)`, plus `fg` when the palette carries a `highlight_text`
  (§3.1). Nothing in `body.rs`, `details.rs` or `palette.rs` changes: they
  already hand the token to ratatui's `highlight_style` / `row_highlight_style`,
  and a background-only style leaves the row's own colours alone — which is
  exactly the behaviour the flavours want and the default theme declines.
- `Token::SelectedCell` keeps a modifier rather than a colour, because it must
  read *against* the highlighted row whatever that row's background is.
- `loader.rs` takes `accent` from the palette. The module-level `ACCENT`
  constant goes away, along with the test that finds the scanner head by
  comparing against it — it looks the head up through the palette instead.
- `view/tables.rs` stops using `Option<Token>` with `None` meaning baseline. A
  user keyspace emits `Keyspace`, a user table emits `Table`, and anything the
  cluster owns still emits `Muted`.

## 8. Tests

Contrast is where the tests earn their keep: a mistyped hex becomes a failure
rather than an unreadable panel, and that is what makes iterating on §4 and §5
safe. It takes three forms, because the themes make three different promises.

**`every_flavour_reads_against_its_own_base`.** For each Catppuccin flavour,
every foreground role holds a WCAG contrast ratio of at least 3.0 against that
flavour's `base`. `muted` is exempt at 2.0 — it is *supposed* to recede, and
holding it to the same floor as `danger` would mean it had stopped receding.

**`the_default_palette_reads_on_black_and_on_white`.** The default theme knows
neither its background nor its foreground, so every role is checked against
both `#000000` and `#ffffff` at 3.0. This is the strictest of the three and the
one that dictated the §4 values.

**`a_highlight_bar_is_distinct_from_its_field`.** `highlight` differs from
`base` by a relative luminance of at least 0.02 in every flavour, and where
`highlight_text` is `Some` it holds 4.5 against `highlight`. Not a per-role
check against `highlight`: Catppuccin Latte's own `blue` on its own `surface1`
is about 2.4, and a test that failed upstream Catppuccin would be testing our
taste rather than our transcription.

**`the_default_theme_paints_no_panel_field`** replaces
`no_token_sets_a_background`. It asserts `base`, `surface`, `text` and `border`
are `None`, and that no token returns a background except `Selected`.

**`every_catppuccin_flavour_sets_every_surface`** — the mirror: a flavour with
a `None` surface is a half-finished theme.

**Domain tests** for the picker: `T` opens with the cursor on the current
theme; moving previews; `Enter` keeps; `Esc` restores.

**Render tests**: a snapshot of the picker overlay, and a snapshot of one panel
under Mocha — the first frame that has ever had a background.

## 9. Out of scope

- Persisting the chosen theme. Needs the config subsystem
  (`architecture.md` §7).
- User-supplied themes. `vision.md` §7 puts them in Phase 5.
- A CQL highlighter. The three query roles exist and are mapped; nothing emits
  their tokens yet.
- Changing what focus looks like. It stays a bold title; a lifted surface for
  the focused panel was considered and rejected, because it would compete with
  the cursor row for the same visual channel.
