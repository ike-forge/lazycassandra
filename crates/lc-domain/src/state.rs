//! The whole application state.
//!
//! One struct for now. architecture.md §11 flags that this may need to become
//! nested per-panel state with message routing once Phase 1 has four real
//! panels; deferring until that shape is known rather than guessing at it.

use std::collections::BTreeMap;

use crate::commands::CommandsPanel;
use crate::contents::ContentsPanel;
use crate::ddl::DdlPanel;
use crate::editor::Editor;
use crate::model::{ConnId, DbError, Key, KeyPress, QueryId, Schema, Statement};
use crate::posture::Posture;
use crate::search::{Filter, Filtered, Prompt};
use crate::style::Severity;
use crate::theme::ThemeChoice;
use crate::tree::TablesPanel;
use crate::view::hints::Binding;

#[derive(Debug, Clone, Default)]
pub struct State {
    pub connection: ConnectionState,
    focus: Panel,
    pub tables: TablesPanel,
    pub contents: ContentsPanel,
    pub ddl: DdlPanel,
    /// The Queries panel's buffer. Not a `QueriesPanel`: the panel is the
    /// editor until the session log arrives.
    pub queries: Editor,
    /// What the last frame turned out to be, so a motion knows how far a page
    /// is and where the end is.
    pub geometry: Geometry,
    /// A message or the command palette, drawn over the panels until it is
    /// dismissed.
    pub overlay: Option<Overlay>,
    /// Queries issued and not yet resolved. A result whose id is absent here
    /// was cancelled, and is dropped rather than applied.
    pub in_flight: BTreeMap<QueryId, Statement>,
    /// How far into its cycle the loader is. Elapsed time reaches the domain
    /// only as [`crate::Msg::Tick`], so this is the whole of what the renderer
    /// is told about it.
    pub loader: Loader,
    /// What the last `y` copied, lit for as long as [`Flash`] lasts. `None`
    /// once it has faded or the next key has ended it.
    pub flash: Option<Flash>,
    /// Set when something changed that the next frame must show. Avoids
    /// repainting on every keypress of a held-down `j` (architecture.md §3).
    pub dirty: bool,
    pub should_quit: bool,
    /// The theme in force. A name; `lc-inbound` owns what it looks like.
    pub theme: ThemeChoice,
}

/// The loader's phase.
///
/// Advances only while something is outstanding and resets when nothing is, so
/// an idle frame is always the same frame rather than wherever the animation
/// happened to stop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Loader {
    frame: usize,
}

impl Loader {
    /// Advance one frame, or fall back to rest. Reports whether the phase
    /// moved — an idle tick that changes nothing must not cost a repaint
    /// (docs/superpowers/specs/2026-08-09-visual-polish-design.md §4.2).
    pub const fn advance(&mut self, working: bool) -> bool {
        let next = if working {
            self.frame.wrapping_add(1)
        } else {
            0
        };
        let moved = next != self.frame;
        self.frame = next;
        moved
    }

    /// Unbounded: the renderer folds it into the widget's own cycle, whose
    /// length is the widget's business rather than the domain's.
    #[must_use]
    pub const fn frame(self) -> usize {
        self.frame
    }
}

/// How far along the one connection is.
///
/// `Option<Connection>` could not say "connecting" or "failed", and an error
/// field beside the schema would make illegal combinations representable.
#[derive(Debug, Clone, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    /// Carries the id so a failure can be matched to the attempt that is
    /// outstanding.
    Connecting(ConnId),
    Failed(DbError),
    Connected(Connection),
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub id: ConnId,
    pub posture: Posture,
    pub schema: SchemaState,
}

/// Whether the connected cluster has told us its shape yet.
#[derive(Debug, Clone, Default)]
pub enum SchemaState {
    #[default]
    Loading,
    Failed(DbError),
    Loaded(Schema),
}

/// A message drawn over the panels, dismissed with `Esc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub title: String,
    pub message: String,
    pub severity: Severity,
}

/// What outranks focus, at most one at a time.
///
/// An enum rather than a second `Option` field beside the message: two fields
/// would make "a failure and the palette, simultaneously" representable, and
/// the renderer would then need a precedence rule the domain never stated
/// (conventions.md §1). `Search` repeats the argument rather than testing it
/// again: a prompt beside the palette would make "a palette and a prompt at
/// once" representable the same way (panel search design §1).
///
/// Membership no longer means "drawn over the panels" — `Search` is drawn on
/// the hint bar row instead — but every other consequence of being an overlay
/// still holds: it claims keys before focus is consulted, `esc` dismisses it,
/// and a click spends itself on it rather than on whatever is underneath.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Message(Message),
    Commands(CommandsPanel),
    /// Drawn on the hint bar row rather than over the panels, and in this
    /// enum for what that buys: a prompt and a palette cannot both be up.
    Search(Prompt),
    /// The theme picker. `restored` is what `Esc` puts back: the cursor sets
    /// `State.theme` as it moves, so the frame previews each theme and there
    /// is nowhere else the previous one survives
    /// (docs/superpowers/specs/2026-08-10-themes-design.md §6).
    Themes {
        cursor: Cursor,
        restored: ThemeChoice,
    },
}

impl ConnectionState {
    /// Whether `conn` is the connection this state is about.
    ///
    /// The guard that drops results belonging to a connection the user has
    /// already left (architecture.md §3).
    #[must_use]
    pub fn is_current(&self, conn: ConnId) -> bool {
        match self {
            Self::Connecting(id) => *id == conn,
            Self::Connected(connection) => connection.id == conn,
            Self::Disconnected | Self::Failed(_) => false,
        }
    }
}

/// Panels are places: fixed positions, navigated with predictable motions
/// (vision.md §4, principle 4).
///
/// Declaration order is column order — top-left, bottom-left, top-right,
/// bottom-right — and [`Panel::ALL`] preserves it. Focus moves in
/// [`Panel::CYCLE`] instead, which is a different walk over the same frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    /// Keyspace → table tree.
    #[default]
    Tables,
    /// `CREATE TABLE` for the selected table.
    Ddl,
    /// Rows of the selected table.
    Contents,
    /// Statement entry and session log.
    Queries,
}

// Motion is not here, in any panel's bar: `j`/`k`, `home`/`end`, the arrows
// and `pgup`/`pgdn` are what a user of a terminal list already reaches for,
// and a bar that spends its width teaching them has none left for what is
// particular to the panel (docs/shortcuts.md §2). The palette still lists
// every one.
const TABLES_HINTS: &[Binding] = &[
    // `⏎` on a keyspace folds it and on a table reads it. The bar names the
    // focused panel and nothing finer (design §1), so one label has to cover
    // both — "open" is true of either row, "read" of half of them.
    Binding {
        keys: &[KeyPress::new(Key::Enter)],
        action: "open",
    },
    // Not global: it does nothing in Queries, where `/` is text (panel
    // search design §5.3).
    Binding {
        keys: &[KeyPress::new(Key::Char('/'))],
        action: "search",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('v'))],
        action: "select",
    },
];

const DDL_HINTS: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Char('/'))],
        action: "search",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('v'))],
        action: "select",
    },
];

const CONTENTS_HINTS: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Char('m'))],
        action: "mode",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('/'))],
        action: "search",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('v'))],
        action: "select",
    },
];

const QUERIES_HINTS: &[Binding] = &[
    // First: it is what the panel is for. `⏎ newline` and `esc leave` are what
    // it does on the way there.
    Binding {
        keys: &[KeyPress::ctrl(Key::Char('r'))],
        action: "run",
    },
    // Unconditionally true in every frame Queries is focused, so it is named
    // plainly rather than greyed — unlike `ctrl-r`, which lies on an empty
    // buffer (external editor design §5).
    Binding {
        keys: &[KeyPress::ctrl(Key::Char('e'))],
        action: "edit",
    },
    Binding {
        keys: &[KeyPress::new(Key::Enter)],
        action: "newline",
    },
    // Named here and nowhere else: `Esc` only leaves a panel that takes text,
    // so this is the one bar where saying so is true (hint bar design §3).
    Binding {
        keys: &[KeyPress::new(Key::Escape)],
        action: "leave",
    },
];

impl Panel {
    /// Every panel, in column order.
    pub const ALL: [Self; 4] = [Self::Tables, Self::Ddl, Self::Contents, Self::Queries];

    /// Every panel, in the order focus walks them: reading order across the
    /// frame — top-left, top-right, bottom-left, bottom-right. The tree and
    /// the rows it opens are adjacent under `Tab`, which is the pair a user
    /// moves between most.
    pub const CYCLE: [Self; 4] = [Self::Tables, Self::Contents, Self::Ddl, Self::Queries];

    /// The next panel in [`Panel::CYCLE`], wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Tables => Self::Contents,
            Self::Contents => Self::Ddl,
            Self::Ddl => Self::Queries,
            Self::Queries => Self::Tables,
        }
    }

    /// The previous panel in [`Panel::CYCLE`], wrapping.
    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Tables => Self::Queries,
            Self::Contents => Self::Tables,
            Self::Ddl => Self::Contents,
            Self::Queries => Self::Ddl,
        }
    }

    /// The key that focuses this panel from anywhere.
    ///
    /// Underlined in the panel's title, so the binding is on screen rather
    /// than in a manual (vision.md §4, principle 1). Three of the four are not
    /// leading letters: the four panels and the DDL panel's two tabs are six
    /// places under one home row, so the letters were chosen to sit together
    /// rather than to match a first character (docs/shortcuts.md §4).
    #[must_use]
    pub const fn shortcut(self) -> char {
        match self {
            Self::Tables => 'a',
            Self::Contents => 'b',
            Self::Ddl => 'd',
            Self::Queries => 'u',
        }
    }

    /// The panel `key` focuses, if any panel claims it.
    #[must_use]
    pub fn from_shortcut(key: char) -> Option<Self> {
        Self::ALL.into_iter().find(|panel| panel.shortcut() == key)
    }

    /// Whether letters typed at this panel are text rather than bindings.
    ///
    /// Focus is the mode (design §3). Exhaustive for the reason [`Panel::hints`]
    /// is: a fifth panel cannot compile without saying which it is.
    #[must_use]
    pub const fn takes_text(self) -> bool {
        match self {
            Self::Tables | Self::Ddl | Self::Contents => false,
            Self::Queries => true,
        }
    }

    /// What this panel responds to.
    ///
    /// Exhaustive, so a fifth panel cannot compile without declaring its
    /// bindings — the same guarantee `ALL` and `shortcut` give.
    #[must_use]
    pub const fn hints(self) -> &'static [Binding] {
        match self {
            Self::Tables => TABLES_HINTS,
            Self::Ddl => DDL_HINTS,
            Self::Contents => CONTENTS_HINTS,
            Self::Queries => QUERIES_HINTS,
        }
    }
}

/// A selection into a list, which cannot be moved past either end.
///
/// The index is private on purpose: an out-of-range selection cannot be
/// constructed from outside this module, so a renderer never has to defend
/// against one (conventions.md §1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    index: usize,
}

impl Cursor {
    /// The selected row.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }

    /// Moves to the next row, stopping on the last one.
    pub const fn down(&mut self, len: usize) {
        let last = len.saturating_sub(1);
        if self.index < last {
            self.index += 1;
        } else {
            self.index = last;
        }
    }

    /// Moves to the previous row, stopping on the first one.
    pub const fn up(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    /// Pulls the selection back inside a list that shrank.
    pub const fn clamp_to(&mut self, len: usize) {
        let last = len.saturating_sub(1);
        if self.index > last {
            self.index = last;
        }
    }

    /// Moves to `index`, or to the last row when the list is shorter.
    pub const fn go_to(&mut self, index: usize, len: usize) {
        self.index = index;
        self.clamp_to(len);
    }

    /// Moves a screen down, stopping on the last row.
    pub const fn page_down(&mut self, len: usize, height: usize) {
        self.go_to(self.index.saturating_add(height), len);
    }

    /// Moves a screen up, stopping on the first row.
    pub const fn page_up(&mut self, height: usize) {
        self.index = self.index.saturating_sub(height);
    }

    pub const fn to_start(&mut self) {
        self.index = 0;
    }

    pub const fn to_end(&mut self, len: usize) {
        self.go_to(len.saturating_sub(1), len);
    }
}

/// An inclusive run of rows, which cannot be built the wrong way round.
///
/// `first <= last` by construction, so nothing downstream compares the two
/// ends before drawing between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    first: usize,
    last: usize,
}

impl Range {
    /// The run between the two ends, in either order.
    #[must_use]
    pub const fn new(anchor: usize, cursor: usize) -> Self {
        if anchor <= cursor {
            Self {
                first: anchor,
                last: cursor,
            }
        } else {
            Self {
                first: cursor,
                last: anchor,
            }
        }
    }

    #[must_use]
    pub const fn first(self) -> usize {
        self.first
    }

    #[must_use]
    pub const fn last(self) -> usize {
        self.last
    }

    #[must_use]
    pub const fn contains(self, index: usize) -> bool {
        self.first <= index && index <= self.last
    }

    /// How many rows the run holds. Never zero — which is why this is not
    /// `len`, whose `is_empty` counterpart would answer a question a range
    /// cannot ask.
    #[must_use]
    pub const fn rows(self) -> usize {
        self.last - self.first + 1
    }
}

/// Inclusive runs of rows, sorted, disjoint and never adjacent.
///
/// Normalised on the way in and never afterwards: [`Ranges::insert`] absorbs
/// every group the new one overlaps *or touches*, because two groups meeting
/// at consecutive rows are one band on screen and a set that could hold both
/// spellings would let a renderer draw a seam nothing explains. Sorted, so a
/// yank reads rows in the order they are drawn without sorting them itself
/// (docs/superpowers/specs/2026-08-14-multi-group-selection-design.md §2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ranges(Vec<Range>);

impl Ranges {
    #[must_use]
    pub fn of(range: Range) -> Self {
        Self(vec![range])
    }

    pub fn insert(&mut self, range: Range) {
        let mut first = range.first;
        let mut last = range.last;
        let mut kept: Vec<Range> = Vec::with_capacity(self.0.len() + 1);
        let mut placed = false;

        for held in self.0.drain(..) {
            if held.first <= last.saturating_add(1) && first <= held.last.saturating_add(1) {
                first = first.min(held.first);
                last = last.max(held.last);
            } else if held.last < first {
                kept.push(held);
            } else {
                if !placed {
                    kept.push(Range { first, last });
                    placed = true;
                }

                kept.push(held);
            }
        }

        if !placed {
            kept.push(Range { first, last });
        }

        self.0 = kept;
    }

    /// Drops the group holding `index`, answering with it.
    pub fn remove_holding(&mut self, index: usize) -> Option<Range> {
        let position = self.0.iter().position(|group| group.contains(index))?;

        Some(self.0.remove(position))
    }

    #[must_use]
    pub fn contains(&self, index: usize) -> bool {
        self.0.iter().any(|group| group.contains(index))
    }

    /// How many rows are selected across every group.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.0.iter().map(|group| group.rows()).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn groups(&self) -> impl Iterator<Item = Range> + '_ {
        self.0.iter().copied()
    }

    /// Every selected row, in the order the rows are drawn.
    pub fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().flat_map(|group| group.first..=group.last)
    }

    #[must_use]
    pub fn first(&self) -> Option<Range> {
        self.0.first().copied()
    }

    /// Pulls the groups back over a list that shrank, dropping the ones whose
    /// first row has gone.
    pub fn clamp_to(&mut self, len: usize) {
        let Some(last) = len.checked_sub(1) else {
            self.0.clear();
            return;
        };

        self.0.retain(|group| group.first <= last);

        for group in &mut self.0 {
            if group.last > last {
                group.last = last;
            }
        }
    }
}

/// The rows a panel has selected, as groups, and whether the cursor is still
/// extending one of them
/// (docs/superpowers/specs/2026-08-14-multi-group-selection-design.md §2).
///
/// While extending, the far end of the live group is the panel's own cursor
/// rather than a second index stored here: a group that is a function of the
/// cursor cannot fall out of step with it. Freezing is the one moment the
/// derived group is written down, and it is written down exactly because the
/// cursor is about to stop meaning it.
///
/// The anchor is kept beside the groups and outlives the extension. The two
/// are not the same question — the groups say which rows are selected, the
/// anchor says which row the user last pressed `v` on — and `y` sends the
/// cursor home to the anchor (visual mode design §5.1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    groups: Ranges,
    anchor: Option<usize>,
    extending: bool,
}

impl Selection {
    /// Opens a group at `cursor` and starts extending it, keeping what is
    /// already held.
    pub fn start(&mut self, cursor: usize) {
        self.anchor = Some(cursor);
        self.extending = true;
    }

    /// Freezes the group the cursor is making, merging it into what is held.
    pub fn stop(&mut self, cursor: usize) {
        if !self.extending {
            return;
        }

        if let Some(anchor) = self.anchor {
            self.groups.insert(Range::new(anchor, cursor));
        }

        self.extending = false;
    }

    /// `v`: freeze what is being extended, drop the group under the cursor, or
    /// open a new group there.
    ///
    /// The third arm pays for the first. Accumulating means `v` no longer
    /// discards, so without a way to drop a group the only way back from a
    /// mis-added one would be to clear everything — and the row to press it on
    /// is the row the user is already looking at
    /// (multi-group selection design §1).
    pub fn toggle(&mut self, cursor: usize) {
        if self.extending {
            self.stop(cursor);
            return;
        }

        if let Some(dropped) = self.groups.remove_holding(cursor) {
            if self.anchor.is_some_and(|anchor| dropped.contains(anchor)) {
                self.anchor = None;
            }

            return;
        }

        self.start(cursor);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    #[must_use]
    pub const fn is_extending(&self) -> bool {
        self.extending
    }

    /// Whether anything is selected at all.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.extending || !self.groups.is_empty()
    }

    /// The rows selected, given where the cursor is now. The one way out, so
    /// no caller can read the groups and forget the cursor, and none is handed
    /// a seam between a group and the extension running into it.
    #[must_use]
    pub fn ranges(&self, cursor: usize) -> Ranges {
        let mut ranges = self.groups.clone();

        if let (true, Some(anchor)) = (self.extending, self.anchor) {
            ranges.insert(Range::new(anchor, cursor));
        }

        ranges
    }

    /// The row `v` was last pressed on, which `y` sends the cursor back to.
    #[must_use]
    pub const fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// Pulls a selection back over a list that shrank.
    pub fn clamp_to(&mut self, len: usize) {
        // Found before `self.groups` is clamped: afterwards every surviving
        // group is a candidate to "contain" the pulled-back anchor, even one
        // that never held it (multi-group design §2).
        let held = self
            .anchor
            .and_then(|anchor| self.groups.groups().find(|group| group.contains(anchor)));

        self.groups.clamp_to(len);

        let Some(last) = len.checked_sub(1) else {
            self.clear();
            return;
        };

        self.anchor = match self.anchor {
            Some(anchor) if self.extending => Some(anchor.min(last)),
            // Frozen, the anchor is pulled back the same way its own group
            // is; only that group dropping outright takes the anchor with it
            // (design §2).
            Some(anchor) => held.and_then(|group| {
                (group.first() <= last).then(|| anchor.min(group.last().min(last)))
            }),
            None => None,
        };
    }
}

/// The afterglow a `y` leaves on what it copied
/// (docs/superpowers/specs/2026-08-14-visual-mode-design.md §6.1).
///
/// A yank spends the selection, so without this the only sign it happened
/// would be a band disappearing. The flash holds what was taken, brightly,
/// for long enough to read and no longer.
///
/// It carries the rectangle rather than reading the selection back, because by
/// the time it is drawn there is no selection left to read. `rows` and
/// `columns` follow the view model's convention: empty on an axis is the
/// whole of it, which is what row mode and column mode take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flash {
    panel: Panel,
    rows: Ranges,
    columns: Ranges,
    remaining: usize,
}

/// How long the afterglow lasts, counted in [`crate::Msg::Tick`]s.
///
/// Elapsed time reaches the domain only as ticks, so a duration is a count
/// here: twelve of `lc-app`'s forty-millisecond ticks, a little under half a
/// second. Long enough to catch the eye, short enough that it is over before
/// the next key.
pub const FLASH_TICKS: usize = 12;

impl Flash {
    #[must_use]
    pub fn lit(panel: Panel, rows: Ranges, columns: Ranges) -> Self {
        Self {
            panel,
            rows,
            columns,
            remaining: FLASH_TICKS,
        }
    }

    /// Burns one tick. Reports whether the frame changed — which only the tick
    /// that puts the flash out does, so a flash costs two repaints rather than
    /// one per tick ([`Loader::advance`]'s rule).
    pub const fn spend(&mut self) -> bool {
        self.remaining = self.remaining.saturating_sub(1);
        self.remaining == 0
    }

    #[must_use]
    pub const fn panel(&self) -> Panel {
        self.panel
    }

    #[must_use]
    pub const fn rows(&self) -> &Ranges {
        &self.rows
    }

    #[must_use]
    pub const fn columns(&self) -> &Ranges {
        &self.columns
    }
}

/// Where a selected row was drawn, counted in the lines a viewport scrolls
/// over.
///
/// The Table DDL panel is the one place a cursor and an offset count in
/// different units: the cursor walks described rows, the offset walks the
/// wrapped lines they occupy, and only the renderer knows the conversion
/// (docs/superpowers/specs/2026-08-10-ddl-selection-design.md §5.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Span {
    /// The first line the row occupies.
    pub first: usize,
    /// How many it occupies. Zero means no row was drawn.
    pub lines: usize,
}

/// A viewport offset over a document of known length.
///
/// The offset is private for the reason [`Cursor`]'s index is: every motion
/// re-establishes `offset <= len - height` here, so a renderer is never
/// handed one that points past the end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scroll {
    offset: usize,
}

impl Scroll {
    /// The first line drawn.
    #[must_use]
    pub const fn offset(self) -> usize {
        self.offset
    }

    /// Moves down a line, stopping with the last line at the bottom of the
    /// panel rather than dragging it to the top.
    pub const fn down(&mut self, len: usize, height: usize) {
        self.go_to(self.offset.saturating_add(1), len, height);
    }

    /// Moves up a line, stopping at the top.
    pub const fn up(&mut self) {
        self.offset = self.offset.saturating_sub(1);
    }

    pub const fn page_down(&mut self, len: usize, height: usize) {
        self.go_to(self.offset.saturating_add(height), len, height);
    }

    pub const fn page_up(&mut self, height: usize) {
        self.offset = self.offset.saturating_sub(height);
    }

    pub const fn to_start(&mut self) {
        self.offset = 0;
    }

    pub const fn to_end(&mut self, len: usize, height: usize) {
        self.go_to(len, len, height);
    }

    /// Pulls a stale offset back over a document that shrank or a panel that
    /// grew.
    pub const fn clamp_to(&mut self, len: usize, height: usize) {
        self.go_to(self.offset, len, height);
    }

    /// Pulls the viewport the shortest distance that puts `index` inside it.
    ///
    /// What makes a selection stay visible now that the renderer no longer
    /// arranges it — `update` applies this to whichever cursor moved
    /// (docs/superpowers/specs/2026-08-09-list-viewport-design.md §3).
    pub const fn follow(&mut self, index: usize, height: usize) {
        if index < self.offset {
            self.offset = index;
        } else if height > 0 && index >= self.offset + height {
            self.offset = index + 1 - height;
        }
    }

    /// Pulls the viewport the shortest distance that puts `span` inside it.
    ///
    /// [`Self::follow`]'s counterpart for a selection that occupies more than
    /// one line. A span taller than the panel cannot be made to fit, so its
    /// first line goes to the top: a property drawn tail-first has its name
    /// scrolled off, which is the half that says what the tail is.
    pub const fn follow_span(&mut self, span: Span, height: usize) {
        if span.lines == 0 || height == 0 {
            return;
        }

        if span.first < self.offset || span.lines >= height {
            self.offset = span.first;
            return;
        }

        let last = span.first + span.lines - 1;

        if last >= self.offset + height {
            self.offset = last + 1 - height;
        }
    }

    const fn go_to(&mut self, offset: usize, len: usize, height: usize) {
        let last = len.saturating_sub(height);
        // `Ord::min` is not const.
        self.offset = if offset > last { last } else { offset };
    }
}

/// What the last frame turned out to be.
///
/// The renderer's one report back into the domain
/// (docs/superpowers/specs/2026-08-09-panel-scrolling-design.md §3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Geometry {
    heights: [usize; 4],
    ddl_lines: usize,
    ddl_selection: Span,
    command_rows: usize,
    /// Character columns the Contents grid's body has, gutter included. A
    /// function of the terminal size alone, which is what keeps §3's
    /// measurement loop open (docs/superpowers/specs/2026-08-10-table-display-design.md §6).
    grid_width: usize,
}

impl Geometry {
    /// `heights` in [`Panel::ALL`] order, each counted in the unit that panel
    /// scrolls in — tree rows, grid rows, or wrapped lines — and net of what
    /// the panel spends on chrome, such as the grid's header row.
    #[must_use]
    pub const fn new(heights: [usize; 4], ddl_lines: usize, command_rows: usize) -> Self {
        Self {
            heights,
            ddl_lines,
            ddl_selection: Span { first: 0, lines: 0 },
            command_rows,
            grid_width: 0,
        }
    }

    /// Rows of content `panel` can show.
    #[must_use]
    pub const fn height(self, panel: Panel) -> usize {
        self.heights[match panel {
            Panel::Tables => 0,
            Panel::Ddl => 1,
            Panel::Contents => 2,
            Panel::Queries => 3,
        }]
    }

    /// Lines the Ddl body wrapped to.
    ///
    /// The one length the domain cannot count for itself: wrapping needs a
    /// width, and width-dependent layout is the renderer's business. Tree and
    /// grid rows it counts, and must keep counting — two sources of truth for
    /// a row count is the drift `tree.rs`'s single flattening prevents.
    #[must_use]
    pub const fn ddl_lines(self) -> usize {
        self.ddl_lines
    }

    /// Character columns the Contents grid can draw into.
    #[must_use]
    pub const fn grid_width(self) -> usize {
        self.grid_width
    }

    /// Rows the command palette would show at this frame size, whether or not
    /// one is open.
    ///
    /// Zero means it does not fit, which is what makes `?` inert rather than
    /// opening a palette nothing draws
    /// (docs/superpowers/specs/2026-08-09-command-palette-design.md §5.1).
    #[must_use]
    pub const fn command_rows(self) -> usize {
        self.command_rows
    }

    /// The span the frame drew the Ddl selection at.
    ///
    /// Split from [`Self::new`] because almost every caller is a test that has
    /// no selection to describe, and a fourth positional `Span::default()` at
    /// each of them says nothing.
    #[must_use]
    pub const fn with_ddl_selection(self, ddl_selection: Span) -> Self {
        Self {
            ddl_selection,
            ..self
        }
    }

    /// How wide the frame drew the Contents grid's body.
    ///
    /// Split from [`Self::new`] for [`Self::with_ddl_selection`]'s reason: a
    /// test that is not about the grid's width has nothing to say here, and a
    /// positional zero at each of them says nothing either.
    #[must_use]
    pub const fn with_grid_width(self, grid_width: usize) -> Self {
        Self { grid_width, ..self }
    }

    /// Where the Ddl panel drew its selected row.
    ///
    /// The second thing only the renderer can produce, for the same reason as
    /// [`Self::ddl_lines`]: it is a function of the wrapping.
    #[must_use]
    pub const fn ddl_selection(self) -> Span {
        self.ddl_selection
    }
}

impl State {
    #[must_use]
    pub const fn focus(&self) -> Panel {
        self.focus
    }

    /// Moves focus, dropping the selection of the panel being left.
    ///
    /// A selection says what the next command acts on and the next command
    /// acts on what has focus, so two panels holding one say two things and
    /// `y` answers only one of them. The field is private so this is the only
    /// way to change focus and the rule cannot be forgotten by a seventh path
    /// (multi-group selection design §4).
    pub fn focus_on(&mut self, panel: Panel) {
        if self.focus == panel {
            return;
        }

        match self.focus {
            Panel::Tables => self.tables.selection.clear(),
            Panel::Ddl => self.ddl.selection.clear(),
            Panel::Contents => {
                self.contents.rows.clear();
                self.contents.columns.clear();
            }
            Panel::Queries => {}
        }

        self.focus = panel;
    }

    /// Posture of the current connection, or the safe default when there is
    /// none.
    #[must_use]
    pub const fn posture(&self) -> Posture {
        match &self.connection {
            ConnectionState::Connected(connection) => connection.posture,
            ConnectionState::Disconnected
            | ConnectionState::Connecting(_)
            | ConnectionState::Failed(_) => Posture::ReadOnly,
        }
    }

    /// The connection a statement would run on.
    #[must_use]
    pub const fn conn_id(&self) -> Option<ConnId> {
        match &self.connection {
            ConnectionState::Connected(connection) => Some(connection.id),
            ConnectionState::Disconnected
            | ConnectionState::Connecting(_)
            | ConnectionState::Failed(_) => None,
        }
    }

    /// The cluster's shape, once it has arrived.
    #[must_use]
    pub const fn schema(&self) -> Option<&Schema> {
        match &self.connection {
            ConnectionState::Connected(Connection {
                schema: SchemaState::Loaded(schema),
                ..
            }) => Some(schema),
            _ => None,
        }
    }

    /// The tree's state and the schema it is a tree of.
    ///
    /// One method rather than two accessors because the borrow checker cannot
    /// split `&mut self` across separate `tables()` and `schema()` calls, and
    /// every transition needs both.
    pub fn tables_and_schema(&mut self) -> Option<(&mut TablesPanel, &Schema)> {
        match &self.connection {
            ConnectionState::Connected(Connection {
                schema: SchemaState::Loaded(schema),
                ..
            }) => Some((&mut self.tables, schema)),
            _ => None,
        }
    }

    pub const fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Whether the tool is waiting on the cluster for anything.
    ///
    /// What the loader animates for, and the only thing that makes a tick
    /// worth a repaint.
    #[must_use]
    pub fn is_working(&self) -> bool {
        !self.in_flight.is_empty()
            || match &self.connection {
                ConnectionState::Connecting(_) => true,
                ConnectionState::Connected(connection) => {
                    matches!(connection.schema, SchemaState::Loading)
                }
                ConnectionState::Disconnected | ConnectionState::Failed(_) => false,
            }
    }

    /// The filter applied to `target`, if any.
    #[must_use]
    pub fn filter(&self, target: Filtered) -> Option<&Filter> {
        match target {
            Filtered::Tables => self.tables.filter(),
            Filtered::Ddl => self.ddl.filter(),
            Filtered::Contents => self.contents.filter(),
        }
    }

    /// Applies `filter` to `target`, and repairs what it invalidates.
    ///
    /// Not a `&mut Option<Filter>` handed out: repairing what a filter
    /// invalidates — a cursor left pointing past the rows the panel now has —
    /// belongs here rather than to every caller that sets one. The Tables
    /// cursor clamp lands with the filtered tree it needs. A filter renumbers
    /// the rows under a range, so the repair it owes includes dropping one.
    pub fn apply_filter(&mut self, target: Filtered, filter: Option<Filter>) {
        match target {
            Filtered::Tables => {
                self.tables.set_filter(filter);
                let rows = self.schema().map(|schema| self.tables.rows(schema).len());
                if let Some(rows) = rows {
                    self.tables.cursor.clamp_to(rows);
                }
                self.tables.selection.clear();
            }
            // No repair here, and the two halves of the panel are repaired
            // elsewhere for different reasons (design §3.3). Its offset is
            // counted in wrapped lines, a width-dependent count only the
            // renderer can produce, so `Msg::Measured` clamps it against a real
            // count; clamping here would clamp against the previous frame's,
            // which is wrong exactly when the filter just shrank the body. Its
            // cursor counts rows, and `update`'s epilogue clamps it on every
            // message against `view::ddl_rows`, which counts the *filtered*
            // rows — so the clamp lands on the same keystroke that applied the
            // filter, with no line count involved.
            Filtered::Ddl => {
                self.ddl.set_filter(filter);
                self.ddl.selection.clear();
            }
            // No range repair here either: this filter only flags matching
            // cells and never renumbers a row, so a range still addresses
            // exactly what it did before the filter changed.
            Filtered::Contents => self.contents.set_filter(filter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnId, DbError, Schema};
    use crate::posture::Posture;
    use crate::search::{Filter, Filtered};

    #[test]
    fn all_panels_are_listed_in_column_order_and_none_is_missing() {
        for panel in Panel::ALL {
            // A new variant fails to compile here until it is added to ALL.
            let expected_position = match panel {
                Panel::Tables => 0,
                Panel::Ddl => 1,
                Panel::Contents => 2,
                Panel::Queries => 3,
            };
            assert_eq!(
                Panel::ALL[expected_position],
                panel,
                "{panel:?} is not at its column-order position"
            );
        }
        assert_eq!(
            Panel::ALL.len(),
            4,
            "a variant was added without extending ALL"
        );
    }

    #[test]
    fn the_default_panel_is_tables() {
        assert_eq!(Panel::default(), Panel::Tables);
    }

    #[test]
    fn the_cycle_walks_reading_order_and_wraps() {
        // Pinned against CYCLE rather than restated, so the two orderings
        // cannot drift apart when a panel is added.
        let walked: Vec<Panel> =
            std::iter::successors(Some(Panel::CYCLE[0]), |panel| Some(panel.next()))
                .take(Panel::CYCLE.len())
                .collect();

        assert_eq!(walked, Panel::CYCLE);
        assert_eq!(Panel::CYCLE[Panel::CYCLE.len() - 1].next(), Panel::CYCLE[0]);
    }

    #[test]
    fn the_cycle_holds_every_panel_exactly_once() {
        for panel in Panel::ALL {
            assert_eq!(
                Panel::CYCLE.iter().filter(|other| **other == panel).count(),
                1,
                "{panel:?} is missing from CYCLE or listed twice"
            );
        }
    }

    #[test]
    fn stepping_back_undoes_stepping_forward() {
        for panel in Panel::ALL {
            assert_eq!(panel.next().previous(), panel);
            assert_eq!(panel.previous().next(), panel);
        }
    }

    #[test]
    fn every_panel_has_a_distinct_shortcut() {
        let mut claimed: Vec<char> = Vec::new();

        for panel in Panel::ALL {
            let key = panel.shortcut();
            assert!(
                !claimed.contains(&key),
                "{panel:?} claims '{key}', which another panel already has"
            );
            claimed.push(key);
        }
    }

    #[test]
    fn a_shortcut_leads_back_to_the_panel_that_claims_it() {
        for panel in Panel::ALL {
            assert_eq!(Panel::from_shortcut(panel.shortcut()), Some(panel));
        }

        assert_eq!(Panel::from_shortcut('z'), None);
    }

    #[test]
    fn the_shortcuts_are_the_ones_the_design_names() {
        assert_eq!(
            Panel::ALL.map(Panel::shortcut),
            ['a', 'd', 'b', 'u'],
            "the keys are a promise to the user's fingers, not an implementation detail"
        );
    }

    #[test]
    fn a_cursor_starts_on_the_first_row() {
        assert_eq!(Cursor::default().index(), 0);
    }

    #[test]
    fn moving_down_stops_on_the_last_row() {
        let mut cursor = Cursor::default();

        for _ in 0..5 {
            cursor.down(3);
        }

        assert_eq!(cursor.index(), 2, "a cursor must not run off the end");
    }

    #[test]
    fn moving_up_stops_on_the_first_row() {
        let mut cursor = Cursor::default();
        cursor.down(3);

        cursor.up();
        cursor.up();

        assert_eq!(cursor.index(), 0, "a cursor must not run off the start");
    }

    #[test]
    fn an_empty_list_leaves_the_cursor_on_the_first_row() {
        let mut cursor = Cursor::default();

        cursor.down(0);

        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn clamping_pulls_the_cursor_into_a_list_that_shrank() {
        let mut cursor = Cursor::default();
        for _ in 0..9 {
            cursor.down(10);
        }

        cursor.clamp_to(4);

        assert_eq!(cursor.index(), 3);
    }

    #[test]
    fn clamping_leaves_a_cursor_that_is_already_inside_alone() {
        let mut cursor = Cursor::default();
        cursor.down(10);

        cursor.clamp_to(10);

        assert_eq!(cursor.index(), 1);
    }

    #[test]
    fn a_scroll_starts_at_the_top() {
        assert_eq!(Scroll::default().offset(), 0);
    }

    #[test]
    fn scrolling_down_stops_with_the_last_line_at_the_bottom() {
        let mut scroll = Scroll::default();

        for _ in 0..50 {
            scroll.down(20, 6);
        }

        assert_eq!(
            scroll.offset(),
            14,
            "the last line belongs at the bottom of the panel, not dragged to the top"
        );
    }

    #[test]
    fn scrolling_up_stops_at_the_top() {
        let mut scroll = Scroll::default();
        scroll.down(20, 6);

        scroll.up();
        scroll.up();

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn a_document_that_fits_never_scrolls() {
        let mut scroll = Scroll::default();

        scroll.down(6, 20);
        scroll.page_down(6, 20);
        scroll.to_end(6, 20);

        assert_eq!(scroll.offset(), 0, "there is nothing below the fold");
    }

    #[test]
    fn a_page_moves_exactly_one_panel_height() {
        let mut scroll = Scroll::default();

        scroll.page_down(100, 12);

        assert_eq!(scroll.offset(), 12);

        scroll.page_up(12);

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn the_last_page_is_short_and_lands_on_the_end() {
        let mut scroll = Scroll::default();
        scroll.page_down(20, 6);
        scroll.page_down(20, 6);
        assert_eq!(scroll.offset(), 12);

        scroll.page_down(20, 6);

        assert_eq!(scroll.offset(), 14);
    }

    #[test]
    fn a_scroll_reaches_both_ends_in_one_key() {
        let mut scroll = Scroll::default();

        scroll.to_end(20, 6);
        assert_eq!(scroll.offset(), 14);

        scroll.to_start();
        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn clamping_pulls_a_stale_offset_over_a_document_that_shrank() {
        let mut scroll = Scroll::default();
        scroll.to_end(60, 6);
        assert_eq!(scroll.offset(), 54);

        scroll.clamp_to(10, 6);

        assert_eq!(scroll.offset(), 4);
    }

    #[test]
    fn clamping_pulls_a_stale_offset_back_when_the_panel_grows() {
        let mut scroll = Scroll::default();
        scroll.to_end(20, 6);

        scroll.clamp_to(20, 30);

        assert_eq!(
            scroll.offset(),
            0,
            "a panel taller than its document shows all of it"
        );
    }

    #[test]
    fn paging_the_cursor_moves_a_screen_and_stops_on_the_last_row() {
        let mut cursor = Cursor::default();

        cursor.page_down(20, 6);
        assert_eq!(cursor.index(), 6);

        for _ in 0..5 {
            cursor.page_down(20, 6);
        }
        assert_eq!(cursor.index(), 19, "the cursor stops on the last row");

        cursor.page_up(6);
        assert_eq!(cursor.index(), 13);
    }

    #[test]
    fn the_cursor_reaches_both_ends_in_one_key() {
        let mut cursor = Cursor::default();

        cursor.to_end(20);
        assert_eq!(cursor.index(), 19);

        cursor.to_start();
        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn an_empty_list_has_no_end_to_jump_to() {
        let mut cursor = Cursor::default();

        cursor.to_end(0);

        assert_eq!(cursor.index(), 0);
    }

    #[test]
    fn geometry_reports_each_panels_own_height() {
        let geometry = Geometry::new([8, 14, 20, 1], 37, 0);

        assert_eq!(geometry.height(Panel::Tables), 8);
        assert_eq!(geometry.height(Panel::Ddl), 14);
        assert_eq!(geometry.height(Panel::Contents), 20);
        assert_eq!(geometry.height(Panel::Queries), 1);
        assert_eq!(geometry.ddl_lines(), 37);
    }

    #[test]
    fn a_state_that_has_never_been_drawn_reports_no_room() {
        // The first frame is drawn before any measurement exists, so every
        // motion against this must be inert rather than wrong.
        let geometry = Geometry::default();

        for panel in Panel::ALL {
            assert_eq!(geometry.height(panel), 0);
        }
        assert_eq!(geometry.ddl_lines(), 0);
    }

    fn connected(schema: SchemaState) -> State {
        State {
            connection: ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: Posture::Writable,
                schema,
            }),
            ..State::default()
        }
    }

    #[test]
    fn a_state_that_is_not_connected_is_read_only() {
        for connection in [
            ConnectionState::Disconnected,
            ConnectionState::Connecting(ConnId::next()),
            ConnectionState::Failed(DbError::Timeout),
        ] {
            let state = State {
                connection,
                ..State::default()
            };

            assert_eq!(
                state.posture(),
                Posture::ReadOnly,
                "only a live connection may carry a writable posture"
            );
        }
    }

    #[test]
    fn a_connected_state_carries_its_own_posture() {
        assert_eq!(connected(SchemaState::Loading).posture(), Posture::Writable);
    }

    #[test]
    fn a_schema_is_only_visible_once_it_has_loaded() {
        assert!(connected(SchemaState::Loading).schema().is_none());
        assert!(
            connected(SchemaState::Failed(DbError::Timeout))
                .schema()
                .is_none()
        );
        assert!(
            connected(SchemaState::Loaded(Schema::default()))
                .schema()
                .is_some()
        );
        assert!(State::default().schema().is_none());
    }

    #[test]
    fn only_the_outstanding_connection_is_current() {
        let conn = ConnId::next();
        let other = ConnId::next();

        assert!(ConnectionState::Connecting(conn).is_current(conn));
        assert!(!ConnectionState::Connecting(conn).is_current(other));
        assert!(!ConnectionState::Disconnected.is_current(conn));
        assert!(!ConnectionState::Failed(DbError::Timeout).is_current(conn));
    }

    #[test]
    fn every_panel_says_what_it_responds_to() {
        for panel in Panel::ALL {
            assert!(
                !panel.hints().is_empty(),
                "{panel:?} responds to nothing, which no panel does any more"
            );
        }
    }

    #[test]
    fn no_binding_claims_a_key_with_no_action_to_go_with_it() {
        for panel in Panel::ALL {
            for binding in panel.hints() {
                assert!(!binding.keys.is_empty(), "{panel:?} has a keyless hint");
                assert!(!binding.action.is_empty(), "{panel:?} has a nameless hint");
            }
        }
    }

    #[test]
    fn following_a_selection_below_the_viewport_puts_it_on_the_last_row() {
        let mut scroll = Scroll::default();

        scroll.follow(9, 5);

        assert_eq!(scroll.offset(), 5, "rows 5..=9 are drawn");
    }

    #[test]
    fn following_a_selection_above_the_viewport_puts_it_on_the_first_row() {
        let mut scroll = Scroll::default();
        scroll.follow(9, 5);

        scroll.follow(2, 5);

        assert_eq!(scroll.offset(), 2);
    }

    #[test]
    fn following_a_selection_already_on_screen_moves_nothing() {
        let mut scroll = Scroll::default();
        scroll.follow(9, 5);

        for index in 5..=9 {
            scroll.follow(index, 5);
            assert_eq!(scroll.offset(), 5, "row {index} was already drawn");
        }
    }

    #[test]
    fn following_into_a_panel_with_no_room_does_not_underflow() {
        let mut scroll = Scroll::default();

        scroll.follow(9, 0);

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn a_filter_is_read_back_from_the_panel_it_was_applied_to() {
        let mut state = State::default();

        state.apply_filter(Filtered::Ddl, Filter::new("ttl"));

        assert_eq!(state.filter(Filtered::Ddl).map(Filter::as_str), Some("ttl"));
        assert_eq!(state.filter(Filtered::Tables), None);
        assert_eq!(state.filter(Filtered::Contents), None);
    }

    #[test]
    fn the_three_filters_are_independent() {
        let mut state = State::default();

        state.apply_filter(Filtered::Tables, Filter::new("ord"));
        state.apply_filter(Filtered::Contents, Filter::new("null"));

        assert_eq!(
            state.filter(Filtered::Tables).map(Filter::as_str),
            Some("ord")
        );
        assert_eq!(
            state.filter(Filtered::Contents).map(Filter::as_str),
            Some("null")
        );
    }

    #[test]
    fn applying_none_clears_the_one_that_was_there() {
        let mut state = State::default();
        state.apply_filter(Filtered::Tables, Filter::new("ord"));

        state.apply_filter(Filtered::Tables, None);

        assert_eq!(state.filter(Filtered::Tables), None);
    }

    #[test]
    fn following_a_span_already_in_view_moves_nothing() {
        let mut scroll = Scroll::default();
        scroll.follow_span(Span { first: 2, lines: 3 }, 10);

        assert_eq!(scroll.offset(), 0);
    }

    #[test]
    fn following_a_span_below_the_viewport_pulls_its_last_line_to_the_bottom() {
        let mut scroll = Scroll::default();
        scroll.follow_span(
            Span {
                first: 12,
                lines: 3,
            },
            10,
        );

        assert_eq!(
            scroll.offset(),
            5,
            "lines 5..15 are drawn, ending on the span"
        );
    }

    #[test]
    fn following_a_span_above_the_viewport_pulls_its_first_line_to_the_top() {
        let mut scroll = Scroll::default();
        scroll.to_end(100, 10);
        scroll.follow_span(Span { first: 4, lines: 2 }, 10);

        assert_eq!(scroll.offset(), 4);
    }

    #[test]
    fn a_span_taller_than_the_panel_shows_its_head_rather_than_its_tail() {
        let mut scroll = Scroll::default();
        scroll.follow_span(
            Span {
                first: 20,
                lines: 30,
            },
            10,
        );

        assert_eq!(
            scroll.offset(),
            20,
            "a property's name is the half worth reading; its tail without it is not"
        );
    }

    #[test]
    fn a_span_of_no_lines_is_nothing_to_follow() {
        let mut scroll = Scroll::default();
        scroll.to_end(100, 10);
        let before = scroll.offset();

        scroll.follow_span(Span::default(), 10);

        assert_eq!(scroll.offset(), before);
    }

    #[test]
    fn a_range_is_the_two_ends_in_order_whichever_way_it_was_made() {
        let down = Range::new(2, 5);
        let up = Range::new(5, 2);

        assert_eq!((down.first(), down.last()), (2, 5));
        assert_eq!(
            (up.first(), up.last()),
            (2, 5),
            "extending upwards is the same range"
        );
        assert_eq!(down.rows(), 4);
        assert!(down.contains(2) && down.contains(5) && !down.contains(6));
    }

    #[test]
    fn freezing_writes_down_the_range_the_cursor_made() {
        let mut selection = Selection::default();
        selection.start(5);

        assert_eq!(
            selection.ranges(2),
            Ranges::of(Range::new(2, 5)),
            "while extending, the far end is the cursor"
        );

        selection.stop(2);

        assert_eq!(
            selection.ranges(9),
            Ranges::of(Range::new(2, 5)),
            "frozen, the cursor walks free"
        );
        assert_eq!(selection.anchor(), Some(5), "the anchor is where v was");
    }

    #[test]
    fn an_extension_running_into_a_group_shows_no_seam() {
        let mut selection = Selection::default();
        selection.start(0);
        selection.stop(2);
        selection.start(4);

        assert_eq!(
            selection.ranges(3),
            Ranges::of(Range::new(0, 4)),
            "the extension touched the frozen group, so the two are one"
        );
    }

    #[test]
    fn nothing_selected_is_an_empty_set_and_no_anchor() {
        let selection = Selection::default();

        assert!(selection.ranges(4).is_empty());
        assert!(!selection.is_active());
        assert_eq!(selection.anchor(), None);
    }

    #[test]
    fn clamping_pulls_an_extending_anchor_back_into_the_list() {
        let mut selection = Selection::default();
        selection.start(9);

        selection.clamp_to(5);

        assert_eq!(selection.anchor(), Some(4));
        assert_eq!(selection.ranges(0), Ranges::of(Range::new(0, 4)));
    }

    #[test]
    fn clamping_away_the_group_holding_the_anchor_takes_the_anchor() {
        // `y` sends the cursor home to the anchor. An anchor whose rows are
        // gone would send it somewhere the user never was
        // (multi-group selection design §3).
        let mut selection = Selection::default();
        selection.start(8);
        selection.stop(9);

        selection.clamp_to(4);

        assert!(selection.ranges(0).is_empty());
        assert_eq!(selection.anchor(), None);
    }

    #[test]
    fn clamping_a_frozen_group_pulls_its_surviving_anchor_back_too() {
        // The group and the anchor are pulled back the same way; only a
        // group that is dropped outright takes its anchor with it
        // (multi-group selection design §2).
        let mut selection = Selection::default();
        selection.start(9);
        selection.stop(2);

        selection.clamp_to(5);

        assert_eq!(selection.ranges(0), Ranges::of(Range::new(2, 4)));
        assert_eq!(selection.anchor(), Some(4));
    }

    #[test]
    fn v_freezes_then_opens_a_second_group_then_drops_one() {
        let mut selection = Selection::default();
        selection.toggle(0);
        selection.toggle(1);

        assert_eq!(selection.ranges(1), Ranges::of(Range::new(0, 1)));

        selection.toggle(3);
        selection.toggle(3);

        let mut both = Ranges::of(Range::new(0, 1));
        both.insert(Range::new(3, 3));
        assert_eq!(selection.ranges(3), both, "the first group survived");

        selection.toggle(0);

        assert_eq!(
            selection.ranges(0),
            Ranges::of(Range::new(3, 3)),
            "v on a held row drops the group holding it"
        );
    }

    #[test]
    fn dropping_the_group_holding_the_anchor_takes_the_anchor() {
        // Homing `y` to a group the user just dropped would send the cursor
        // where they never were (design §3).
        let mut selection = Selection::default();
        selection.toggle(2);
        selection.toggle(4);

        assert_eq!(selection.anchor(), Some(2));

        selection.toggle(3);

        assert_eq!(selection.anchor(), None);
        assert!(!selection.is_active());
    }

    #[test]
    fn dropping_another_group_keeps_the_anchor() {
        let mut selection = Selection::default();
        selection.toggle(0);
        selection.toggle(0);
        selection.toggle(5);
        selection.toggle(5);

        selection.toggle(0);

        assert_eq!(
            selection.anchor(),
            Some(5),
            "the anchor's group still stands"
        );
    }

    #[test]
    fn a_geometry_carries_the_selected_ddl_rows_span() {
        let span = Span { first: 7, lines: 4 };

        assert_eq!(
            Geometry::new([1, 2, 3, 4], 40, 5)
                .with_ddl_selection(span)
                .ddl_selection(),
            span
        );
        assert_eq!(
            Geometry::default().ddl_selection(),
            Span::default(),
            "a frame that drew no selection reports none"
        );
    }

    #[test]
    fn groups_that_overlap_or_merely_touch_become_one() {
        // Two groups meeting at consecutive rows are one band on screen, and a
        // set holding both spellings would let a renderer draw a seam the user
        // cannot explain (multi-group selection design §2).
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 2));
        ranges.insert(Range::new(2, 4));

        assert_eq!(ranges.groups().collect::<Vec<_>>(), vec![Range::new(0, 4)]);

        let mut touching = Ranges::default();
        touching.insert(Range::new(0, 2));
        touching.insert(Range::new(3, 5));

        assert_eq!(
            touching.groups().collect::<Vec<_>>(),
            vec![Range::new(0, 5)],
            "3 follows 2, so the two are one band"
        );
    }

    #[test]
    fn a_gap_of_one_row_keeps_two_groups() {
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 1));
        ranges.insert(Range::new(3, 4));

        assert_eq!(
            ranges.groups().collect::<Vec<_>>(),
            vec![Range::new(0, 1), Range::new(3, 4)]
        );
        assert_eq!(ranges.rows(), 4);
        assert!(!ranges.contains(2), "the gap is not selected");
        assert!(ranges.contains(0) && ranges.contains(4));
    }

    #[test]
    fn the_order_groups_arrived_in_does_not_show() {
        let mut forwards = Ranges::default();
        forwards.insert(Range::new(0, 1));
        forwards.insert(Range::new(5, 6));
        forwards.insert(Range::new(3, 3));

        let mut backwards = Ranges::default();
        backwards.insert(Range::new(3, 3));
        backwards.insert(Range::new(5, 6));
        backwards.insert(Range::new(0, 1));

        assert_eq!(forwards, backwards);
        assert_eq!(
            forwards.indices().collect::<Vec<_>>(),
            vec![0, 1, 3, 5, 6],
            "indices come out in the order the rows are drawn"
        );
    }

    #[test]
    fn a_group_bridging_two_others_swallows_both() {
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 1));
        ranges.insert(Range::new(5, 6));
        ranges.insert(Range::new(1, 5));

        assert_eq!(ranges.groups().collect::<Vec<_>>(), vec![Range::new(0, 6)]);
    }

    #[test]
    fn removing_answers_with_the_group_that_held_the_row() {
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 1));
        ranges.insert(Range::new(4, 6));

        assert_eq!(ranges.remove_holding(5), Some(Range::new(4, 6)));
        assert_eq!(ranges.groups().collect::<Vec<_>>(), vec![Range::new(0, 1)]);
        assert_eq!(ranges.remove_holding(3), None, "no group holds row 3");
    }

    #[test]
    fn clamping_drops_only_the_groups_whose_head_is_gone() {
        // A group whose first row has gone no longer addresses anything the
        // user pointed at; one whose tail was truncated still does (visual
        // mode design §2, applied per group).
        let mut ranges = Ranges::default();
        ranges.insert(Range::new(0, 1));
        ranges.insert(Range::new(4, 8));
        ranges.insert(Range::new(10, 12));

        ranges.clamp_to(6);

        assert_eq!(
            ranges.groups().collect::<Vec<_>>(),
            vec![Range::new(0, 1), Range::new(4, 5)]
        );

        ranges.clamp_to(0);

        assert!(ranges.is_empty(), "an empty list selects nothing");
    }
}
