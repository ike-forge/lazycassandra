//! What outranks focus: modals, pickers and prompts.

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub title: String,
    pub message: String,
    pub severity: Severity,
}

/// An enum, so two at once is unrepresentable and the renderer needs no
/// precedence rule. `PartialEq` only: the export form holds float rows.
#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    Message(Message),
    Commands(CommandsPanel),
    /// Drawn on the hint bar row, but an overlay: it claims keys and a click
    /// spends on it.
    Search(Prompt),
    /// `restored` is what `Esc` puts back: moving the cursor writes
    /// `State.theme`.
    Themes {
        cursor: Cursor,
        restored: ThemeChoice,
        /// A field: a prompt and its picker are up together.
        sift: Option<Sift>,
    },
    /// No `restored`: the cursor previews nothing.
    Connections {
        cursor: Cursor,
        /// A field: a prompt and its picker are up together.
        sift: Option<Sift>,
    },
    /// Previews nothing; `⏎` copies into the buffer.
    History {
        cursor: Cursor,
        /// A field: a prompt and its picker are up together.
        sift: Option<Sift>,
    },
    Diagnostics(DiagnosticsPanel),
    /// A modal, not a bar prompt like [`Overlay::Search`]: a filter drops rows
    /// and must not read like a search.
    Filter(Filter),
    /// The overwrite confirmation is a field on the form, so it cannot exist
    /// without one.
    Export(ExportForm),
    /// The two spellings are a field on the prompt: one question either way.
    Drop(DropPrompt),
    Health(HealthPanel),
    Inspect(Inspector),
    Commit(CommitPrompt),
    /// Not a `Message`: answering it ends the process.
    Discard(Discard),
    /// Before there is an application to return to; its only exit is
    /// `Effect::Quit`. A variant, so a reconnect path must choose it
    /// deliberately.
    Fatal(Message),
}

/// A list, not a [`Message`]: a load yields many entries and multi-line
/// messages truncate. The cursor is private so nothing seats it past the last
/// row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsPanel {
    rows: Vec<Diagnostic>,
    cursor: Cursor,
}

impl DiagnosticsPanel {
    #[must_use]
    pub fn new(rows: Vec<Diagnostic>) -> Self {
        Self {
            rows,
            cursor: Cursor::default(),
        }
    }

    #[must_use]
    pub fn rows(&self) -> &[Diagnostic] {
        &self.rows
    }

    #[must_use]
    pub const fn selected(&self) -> usize {
        self.cursor.index()
    }

    pub fn down(&mut self) {
        self.cursor.down(self.rows.len());
    }

    pub fn up(&mut self) {
        self.cursor.up();
    }

    pub fn page_down(&mut self, height: usize) {
        self.cursor.page_down(self.rows.len(), height);
    }

    pub fn page_up(&mut self, height: usize) {
        self.cursor.page_up(height);
    }

    pub fn to_start(&mut self) {
        self.cursor.to_start();
    }

    pub fn to_end(&mut self) {
        self.cursor.to_end(self.rows.len());
    }
}
