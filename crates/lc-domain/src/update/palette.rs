//! The command palette (command palette design).

use crate::commands::{CommandsPanel, Run};
use crate::model::Key;
use crate::overlay::Overlay;
use crate::panel::Panel;
use crate::state::State;

use super::{Effect, escape, handle_key, quit};

/// Refused where the frame cannot draw one: an undrawn palette would still
/// swallow `j`.
pub(super) fn open_palette(state: &mut State) {
    if state.geometry.command_rows() == 0 {
        return;
    }

    state.overlay = Some(Overlay::Commands(CommandsPanel::opened()));
}

/// `q` still quits; unused keys are inert rather than dismissing, so no letter
/// means "close" in only one frame.
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

/// Performs the row by pressing its first key through [`handle_key`], so a row
/// cannot run something its key does not. Terminates: the overlay is gone
/// before the call.
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
