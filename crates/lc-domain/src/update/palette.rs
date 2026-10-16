//! The command palette
//! (docs/superpowers/specs/2026-08-09-command-palette-design.md).

use crate::commands::{CommandsPanel, Run};
use crate::model::Key;
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::state::State;

use super::{Effect, escape, handle_key, quit};

/// `?` opens the palette, replacing whatever overlay was up.
///
/// Refused where the frame cannot draw one: opening a palette nothing draws
/// leaves the user in a mode with nothing on screen saying so, wondering why
/// `j` stopped moving the tree (design §5.1).
pub(super) fn open_palette(state: &mut State) {
    if state.geometry.command_rows() == 0 {
        return;
    }

    state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));
}

/// What an open palette responds to.
///
/// `q` still quits, because a key that means "quit" everywhere except in one
/// overlay is not a key anyone can trust without looking. Everything the
/// palette does not use is inert rather than dismissing, so no letter means
/// "close" in one frame and something else in the next (design §7).
pub(super) fn palette_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(&state.overlay, Some(Overlay::Commands(panel)) if panel.sift().is_some()) {
        return palette_sift_key(state, key);
    }

    let height = state.geometry.command_rows();

    match key {
        Key::Char('q') => return quit(state),
        Key::Char('?') | Key::Escape => {
            state.overlay = None;
            return Vec::new();
        }
        Key::Enter => return run_selected(state),
        _ => {}
    }

    let Some(Overlay::Commands(panel)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => panel.down(),
        Key::Char('k') | Key::Up => panel.up(),
        Key::PageDown => panel.page_down(height),
        Key::PageUp => panel.page_up(height),
        Key::Home => panel.to_start(),
        Key::End => panel.to_end(),
        Key::Char('/') => panel.open_sift(),
        _ => {}
    }

    Vec::new()
}

/// What a palette with a live sift responds to.
///
/// Text, for the reason [`themes_sift_key`] takes text: an overlay with a
/// prompt up is a panel that takes text (overlay search design §5).
fn palette_sift_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let Some(Overlay::Commands(panel)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => panel.push(character),
        Key::Backspace => panel.pop(),
        Key::Enter => panel.close_sift(),
        _ => {}
    }

    Vec::new()
}

/// `⏎`: close the palette, then perform the row the cursor is on.
///
/// The row's own first key, pressed through [`handle_key`], is what performs
/// it. A table of actions here would be the second dispatch point
/// docs/shortcuts.md §2 forbids, and a row that ran something other than what
/// its key runs is the "bar that lies" one level down
/// (docs/superpowers/specs/2026-08-15-palette-execution-design.md §2).
///
/// The recursion terminates: the overlay is gone before the call, so
/// `handle_key`'s palette arm cannot be re-entered on this pass.
fn run_selected(state: &mut State) -> Vec<Effect> {
    let Some(Overlay::Commands(open)) = &state.overlay else {
        return Vec::new();
    };
    let command = open.command();
    state.overlay = None;

    let Some(command) = command else {
        return Vec::new();
    };

    match command.run {
        Run::Inert => return Vec::new(),
        Run::In(panel) => state.focus_on(panel),
        Run::Editing => {
            state.focus_on(Panel::Queries);
            state.queries.start_editing();
        }
        Run::Here => {}
    }

    let Some(stroke) = command.binding.keys.first().copied() else {
        return Vec::new();
    };

    handle_key(state, stroke.press())
}
