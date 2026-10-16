//! What outranks focus: modals, pickers and prompts, at most one at a time.

use crate::commands::CommandsPanel;
use crate::commit::{CommitPrompt, Discard};
use crate::config::Diagnostic;
use crate::contents::Filter;
use crate::drop::DropPrompt;
use crate::export::ExportForm;
use crate::health::HealthPanel;
use crate::inspect::Inspector;
use crate::search::{Prompt, Sift};
use crate::style::Severity;
use crate::theme::ThemeChoice;
use crate::viewport::Cursor;

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
///
/// `PartialEq` and not `Eq`: the export form holds the rows a confirmation
/// would write, and a `CqlValue` can be a float.
#[derive(Debug, Clone, PartialEq)]
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
        /// The live search over the list, while one is up. A field rather
        /// than a sibling variant: a prompt and a picker must both be up
        /// (overlay search design §4).
        sift: Option<Sift>,
    },
    /// The connection picker: the `[[connections]]` a config defines, one of
    /// which the user chooses to connect to.
    ///
    /// No `restored` twin to [`Self::Themes`]': a cursor here previews
    /// nothing, so there is no state under the picker for `Esc` to put back
    /// — dismissing it leaves the connection unchosen and the panels empty
    /// (connection picker design §3).
    Connections {
        cursor: Cursor,
        /// The live search over the list. A field rather than a sibling
        /// variant, for `Themes`' reason: a prompt and a picker must both be
        /// up (overlay search design §4).
        sift: Option<Sift>,
    },
    /// The statements the connection in use has been asked, newest first,
    /// one of which the user chooses to load into the buffer. Previews and
    /// connects nothing, so it has no `restored` twin either: choosing is a
    /// copy into the buffer, and the buffer is only touched by `⏎`
    /// (query history design §3).
    History {
        cursor: Cursor,
        /// The live search over the list, for `Themes`' reason.
        sift: Option<Sift>,
    },
    /// What the config files got wrong, drawn over the panels at startup.
    Diagnostics(DiagnosticsPanel),
    /// The filter prompt over one grid column. A modal like the export form
    /// rather than a bar-row prompt like [`Overlay::Search`]: a filter drops
    /// rows where a search only marks them, and the two must not read alike
    /// (docs/superpowers/specs/2026-08-31-grid-sort-filter-design.md §3).
    Filter(Filter),
    /// The export form. Its overwrite confirmation is a field on the form
    /// rather than a sixth variant here, so "a confirmation with nothing
    /// behind it" cannot be written down (export design §7).
    Export(ExportForm),
    /// The drop confirmation. Whether it is answered with `y` or with the
    /// target's name typed back is a field on the prompt rather than two
    /// variants here: it is one question with two spellings, and every other
    /// consequence of being this overlay is the same either way
    /// (drop design §2).
    Drop(DropPrompt),
    /// One table's health, read from every node (table health design).
    Health(HealthPanel),
    /// A value or a row read whole, over the grid it came from
    /// (docs/superpowers/specs/2026-09-17-cell-inspector-design.md).
    Inspect(Inspector),
    /// The commit confirmation: every uncommitted edit, as the CQL that would
    /// write them (cell editing design §5).
    Commit(CommitPrompt),
    /// `q` with edits outstanding. A variant rather than a `Message`, so the
    /// type says that answering it ends the process — `Fatal`'s argument
    /// (cell editing design §6).
    Discard(Discard),
    /// A startup failure: the connection or the schema read, either of which
    /// happens before there is an application to put a trail in. Its only
    /// exit is `Effect::Quit`.
    ///
    /// A variant rather than a flag on [`Message`]: the type says "this modal
    /// ends the process", so a future reconnect path has to choose it
    /// deliberately rather than inherit fatality by accident
    /// (query log design §7).
    Fatal(Message),
}

/// The config diagnostics, and where the cursor is among them.
///
/// A list rather than a [`Message`]: a config load yields many entries, and
/// `overlay.rs`'s sizing under-counts an embedded newline by its own
/// admission, so a multi-line message truncates (design §9). The cursor is
/// private for [`CommandsPanel`]'s reason: every motion goes through a named
/// method here, so nothing outside this module can seat it past the last row
/// (conventions.md §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsPanel {
    rows: Vec<Diagnostic>,
    cursor: Cursor,
}

impl DiagnosticsPanel {
    /// Opens on the first diagnostic.
    #[must_use]
    pub fn new(rows: Vec<Diagnostic>) -> Self {
        Self {
            rows,
            cursor: Cursor::default(),
        }
    }

    /// Every diagnostic the config load produced, in the order it produced
    /// them.
    #[must_use]
    pub fn rows(&self) -> &[Diagnostic] {
        &self.rows
    }

    /// The selected row, as an index into [`Self::rows`].
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.cursor.index()
    }

    /// Moves to the next diagnostic, stopping on the last one.
    pub fn down(&mut self) {
        self.cursor.down(self.rows.len());
    }

    /// Moves to the previous diagnostic, stopping on the first one.
    pub fn up(&mut self) {
        self.cursor.up();
    }

    /// Moves a screen of diagnostics down, stopping on the last one.
    pub fn page_down(&mut self, height: usize) {
        self.cursor.page_down(self.rows.len(), height);
    }

    /// Moves a screen of diagnostics up, stopping on the first one.
    pub fn page_up(&mut self, height: usize) {
        self.cursor.page_up(height);
    }

    /// Moves to the first diagnostic.
    pub fn to_start(&mut self) {
        self.cursor.to_start();
    }

    /// Moves to the last diagnostic.
    pub fn to_end(&mut self) {
        self.cursor.to_end(self.rows.len());
    }
}
