# Four-panel layout — design

**Date:** 2026-08-08
**Status:** implemented (see §8 for what was knowingly left)
**Source:** `doc/ui-draft.png`

Establishes the TUI shell promised by vision.md §7 Phase 0 — panel layout with
titled, proportionally sized blocks. Focus movement, panel content, and
scrolling are explicitly out of scope.

---

## 1. Panels

The draft shows four blocks. `State::Panel` currently carries five variants
(`Keyspaces`, `Tables`, `Editor`, `Results`, `Log`) chosen before the layout
existed. It is replaced by the four the draft names, so the enum describes what
is on screen rather than a superset of it:

```rust
// lc-domain/src/state.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    #[default]
    Tables,
    Ddl,
    Contents,
    Queries,
}

impl Panel {
    pub const ALL: [Self; 4] = [Self::Tables, Self::Ddl, Self::Contents, Self::Queries];
}
```

`Tables` subsumes the keyspace → table tree the draft labels
`Tables (keyspace -> tables)`. `Log` is dropped; nothing renders it, and
reintroducing it later is a one-line enum change plus a layout decision that
does not exist yet.

Declaration order is reading order — top-left, bottom-left, top-right,
bottom-right — and `ALL` preserves it. Iteration over `ALL` is therefore
deterministic, satisfying conventions.md §4's ban on unspecified iteration
order without needing a sort.

## 2. ViewModel

architecture.md §3 promises `view_model(state) -> ViewModel`. It does not exist;
this change introduces the smallest honest version rather than letting
`lc-inbound` read `State` directly.

```rust
// lc-domain/src/view.rs
pub struct ViewModel {
    pub panels: [PanelView; 4],
    pub focus: Panel,
}

pub struct PanelView {
    pub panel: Panel,
    pub title: String,
}

pub fn view_model(state: &State) -> ViewModel;
```

Titles live in the domain. `String` rather than `&'static str` so a Phase 1
title of `Tables — system_schema` needs no type change at the boundary.

One `ViewModel` is rebuilt per frame. architecture.md §11's question about
per-panel view models with dirty tracking stays open — there is nothing to
measure until panels carry content.

### Titles

| Panel | Title |
|---|---|
| `Tables` | `Tables` |
| `Ddl` | `Table DDL` |
| `Contents` | `Table contents` |
| `Queries` | `Queries` |

## 3. Layout

### Geometry

Three nested two-way splits:

```
area
├── left column   25% width
│   ├── Tables    60% of column height
│   └── Ddl       40% of column height
└── right column  75% width
    ├── Contents  90% of column height
    └── Queries   10% of column height
```

### Why the splits are computed, not solved

Ratatui's `Layout` resolves constraints through a cassowary solver. Mixing
`Constraint::Percentage` with `Constraint::Min` to express "10%, but never
below three rows" makes the outcome depend on the solver's internal priorities,
which are a property of the ratatui version rather than of this design. Under
conventions.md §4 that is a reproducibility problem: the pin protects us from
the version changing by accident, not from the behaviour being unspecified in
the first place.

The splits are therefore computed as `u16` arithmetic, and only the resolved
lengths reach ratatui:

```rust
// lc-inbound/src/layout.rs
fn split(total: u16, minor_percent: u16, major_floor: u16, minor_floor: u16) -> (u16, u16) {
    let share = u32::from(total) * u32::from(minor_percent) / 100;
    let share = u16::try_from(share).unwrap_or(u16::MAX);

    let cap = total.saturating_sub(major_floor);
    let minor = if cap < minor_floor { 0 } else { share.clamp(minor_floor, cap) };

    (total - minor, minor)
}
```

The percentage is taken in `u32`: `u16::MAX * 25` overflows `u16`, and while no
real terminal is that wide, the function has to be total for every value its
parameter type admits, not merely for plausible ones.

`clamp` panics when its bounds cross, so the `cap < minor_floor` branch is
load-bearing rather than defensive — inside the `else`, `minor_floor <= cap`
holds by construction.

Each split names a *minor* side (the one carrying the percentage) and a *major*
side (the one taking the remainder):

| Split | Minor | Percent | Major floor | Minor floor |
|---|---|---|---|---|
| Root, horizontal | left column | 25 | 24 cols | 16 cols |
| Left, vertical | `Ddl` | 40 | 3 rows | 3 rows |
| Right, vertical | `Queries` | 10 | 3 rows | 3 rows |

A floor of 3 is two border rows plus one line of content — the point below
which a bordered block shows nothing.

### Behaviour at each size

- **Roomy.** `total * percent / 100` exceeds both floors; the draft's
  proportions hold exactly.
- **Tight.** The percentage falls under the minor floor; `.max()` lifts the
  minor side to its floor and the major side absorbs the loss. A 24-row
  terminal gives `Queries` 3 rows rather than the 2 that 10% would yield.
- **Desperate.** Both floors cannot fit. The minor side goes to zero outright
  and the major side takes everything: `Contents` keeps its rows, `Queries`
  disappears, and the zero-extent rect is skipped at render time.

The jump straight to zero is the point of the `cap < minor_floor` branch. A
plain clamp would hand the minor side whatever scraps remain — one or two rows
on a 4-row column — which is a bordered box with no interior: a panel that
looks present and shows nothing. Losing it outright says more.

The function is total. `saturating_sub` cannot underflow, the branch keeps
`clamp`'s bounds ordered, and `minor <= total` always holds, so `total - minor`
cannot wrap and no `Rect` gets an impossible extent — at any `u16` input.

### Interface

```rust
pub struct PanelRects { /* one Rect per Panel */ }

impl PanelRects {
    pub fn get(&self, panel: Panel) -> Rect;
}

pub fn layout(area: Rect) -> PanelRects;
```

Superseded: `layout` now returns `Shell`, which is `Panels(PanelRects)` above
a minimum size and `TooSmall(Rect)` below it
(doc/specs/2026-08-09-minimum-terminal-size-design.md §3).

`layout` is pure — `Rect` in, `Rect`s out, no frame, no state — which is what
makes §5's geometry tests possible without a terminal.

## 4. Rendering

```rust
// lc-inbound/src/render.rs
pub fn render(frame: &mut Frame, vm: &ViewModel, theme: Theme);
```

Computes `layout(frame.area())`, then for each `PanelView` in `vm.panels` draws
`Block::bordered().title(&pv.title)` into `rects.get(pv.panel)`, skipping rects
of zero width or height.

`lc-app`'s placeholder `render` becomes a call to
`lc_inbound::render(frame, &view_model(&state), theme)`. The temporary
`q`-to-quit binding in `main.rs` is untouched — bindings are a separate concern
with their own commit.

### Focus

`ViewModel` carries `focus`, and `Panel::Tables` is `#[default]`, so `Tables`
renders as focused from the first frame. Nothing moves focus; the keybindings
are later work.

Drawing focus needs a style, and the domain never names a colour
(architecture.md §6), so the token vocabulary gains one variant:

```rust
// lc-domain/src/style.rs
pub enum Token {
    // …
    Focused,
}
```

`Theme::TerminalDefault` maps it to `Modifier::BOLD` on the block's border, and
sets no background — principle 5 holds, and the distinction survives on a
terminal with a poor palette because it spends no colour at all.

## 5. Testing

Per architecture.md §10, at the fastest tier each fact admits.

**Domain unit** — `view_model` returns four panels in `Panel::ALL` order with
the §2 titles, and `focus` mirrors `State::focus`.

**Layout unit** — pure `Rect` assertions, no terminal:

- Exact rects at 100×100, proving 25/75, 60/40 and 90/10.
- Floors engaging on a short terminal: `Queries` at 3 rows, not 2, at 80×24.
- The desperate case: on a column too short for both floors, `Queries` is zero
  rows — never the one or two it would get from a plain clamp — and `Contents`
  takes the remainder.
- A tiling invariant over a fixed table of sizes — the four rects cover `area`
  exactly, with no overlap and no gap. The sizes are enumerated, not sampled;
  conventions.md §4 rules out unseeded randomness.

**Render** — `TestBackend` snapshots at 80×24 and 120×40, asserting the four
titles and their border positions. Ratatui 0.30 splits into `ratatui-core` and
`ratatui-widgets`; whether `TestBackend` needs a feature enabled is checked
during implementation, not assumed here.

## 6. Out of scope

Focus movement and its keybindings · panel content of any kind · scrolling ·
resize handling beyond what `frame.area()` already reports · restoring a `Log`
panel · themes beyond `TerminalDefault`.

## 7. Consequences

- architecture.md §4's sketch of `state/` lists "panel focus"; the panel names
  there change with this design. §11's `ViewModel` granularity question is
  narrowed, not answered — one tree per frame is now the status quo rather than
  a hypothesis.
- The `split` helper is the single place proportions and floors are decided.
  Adding a fifth panel means another split with its own floors, not a rewrite.

## 8. Known gaps

Raised during implementation review and deliberately not fixed. Recorded here
because a gap nobody wrote down is indistinguishable from one nobody found.

**One deviation from §6.** Resize *is* handled, contrary to the out-of-scope
line above. `Event::Resize` was being dropped, so the layout never repainted
when the terminal changed size — which made "adaptable sizing" untrue in
practice. `Msg::Resized` carries no geometry; the renderer still reads
`frame.area()`.

**Deviation from §5.** The render snapshots run at 40×12 and 40×5, not the
80×24 and 120×40 this document specified. Consequence: no render-tier test at a
realistic terminal size, and none where the sidebar takes its true 25% — at 40
columns it sits on its floor. The roomy case is covered at the layout tier
through the same code path.

**Untested by construction.** `split`'s upper clamp bound never binds at any of
the three call sites: the collapse branch fires first in every case. The
generality is deliberate (§7), but the ceiling reads as more exercised than it
is. Nothing asserts that unfocused panels get *no* border style, only that they
are not bold. Only zero-*height* collapse is exercised, not zero-*width*, though
both take the same `area.is_empty()` branch.

**Hand-maintained lists.**

- `Panel::ALL` completeness is enforced by an exhaustive `match` in
  `state.rs`. Adding a variant stops the compile — but the stop can be
  silenced by adding a match arm without extending `ALL`, and the panel is
  then invisible to `view_model`. The accompanying `Panel::ALL.len() == 4`
  assertion is inert (the length is a compile-time constant) and its message
  reads backwards.
- `no_token_sets_a_background` in `theme.rs` iterates a hand-written list of
  all eleven `Token` values. Complete today; a future variant would be
  silently unchecked. `Token` is a product of three nested enums, so an
  exhaustive mechanism is real work for little return — revisit when a second
  theme exists.

**Ruled won't-fix.** `layout.rs` uses plain `+` for `area.x + side_w` and the
`area.y` offsets. Overflow needs a `Rect` built by struct literal that already
violates the invariant `Rect::new` enforces (`x.saturating_add(width) - x`).
`saturating_add` would trade a debug-build panic for a silently wrong rect,
which is worse.

**Unverified.** Nobody has run the binary in a real TTY. The render tests
assert the layout at the buffer level, but no automated check covers terminal
restoration on `q` or how the panels look at a real terminal size.
