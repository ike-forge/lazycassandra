//! The theme picker.

use crate::config::Config;
use crate::log::Outcome;
use crate::model::Key;
use crate::overlay::{Message, Overlay};
use crate::search::Sift;
use crate::state::State;
use crate::style::Severity;
use crate::theme::{BuiltIn, ThemeChoice};
use crate::viewport::Cursor;

use super::{Effect, escape, logged, picker_matches, quit, raise, sentence};

/// Every site that unwinds a preview goes through here.
pub(super) fn restore_theme(state: &mut State) {
    if let Some(Overlay::Themes { restored, .. }) = &state.overlay {
        state.theme = restored.clone();
    }
}

fn theme_picker_index(config: &Config, choice: &ThemeChoice) -> Option<usize> {
    BuiltIn::ALL
        .iter()
        .position(|built_in| ThemeChoice::from(*built_in) == *choice)
        .or_else(|| {
            config
                .themes
                .iter()
                .position(
                    |theme| matches!(choice, ThemeChoice::Custom(name) if *name == theme.name),
                )
                .map(|index| index + BuiltIn::ALL.len())
        })
}

fn theme_at_picker_index(config: &Config, index: usize) -> Option<ThemeChoice> {
    BuiltIn::ALL
        .get(index)
        .map(|built_in| ThemeChoice::from(*built_in))
        .or_else(|| {
            config
                .themes
                .get(index - BuiltIn::ALL.len())
                .map(|theme| ThemeChoice::Custom(theme.name.clone()))
        })
}

/// Shared with the view: a search is correct only if `theme_labels[i]` is what
/// row `i` draws.
pub(crate) fn theme_labels(config: &Config) -> Vec<String> {
    BuiltIn::ALL
        .iter()
        .map(|built_in| built_in.label().to_owned())
        .chain(
            config
                .themes
                .iter()
                .map(|theme| theme.name.as_str().to_owned()),
        )
        .collect()
}

/// Gated on the picker's own room: a long config theme list may not fit where a
/// palette would. Refused over a live prompt, which ctrl chords would otherwise
/// replace.
pub(super) fn open_themes(state: &mut State) {
    let last = BuiltIn::ALL.len() + state.config.themes.len();
    let busy = matches!(state.overlay, Some(Overlay::Search(_)));
    if last > state.geometry.theme_rows() || busy {
        return;
    }

    let mut cursor = Cursor::default();
    let index = theme_picker_index(&state.config, &state.theme).unwrap_or(0);
    cursor.go_to(index, last);

    state.overlay = Some(Overlay::Themes {
        cursor,
        restored: state.theme.clone(),
        sift: None,
    });
}

/// Moving the cursor previews the theme.
pub(super) fn themes_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(state.overlay, Some(Overlay::Themes { sift: Some(_), .. })) {
        return themes_sift_key(state, key);
    }

    match key {
        Key::Char('q') => return quit(state),
        Key::Enter => {
            let chosen = state.theme.clone();
            let changed = match state.overlay.take() {
                Some(Overlay::Themes { restored, .. }) => restored != chosen,
                _ => false,
            };

            return if changed {
                vec![Effect::SaveTheme(chosen)]
            } else {
                Vec::new()
            };
        }
        Key::Escape => {
            escape(state);
            return Vec::new();
        }
        _ => {}
    }

    let last = BuiltIn::ALL.len() + state.config.themes.len();
    let height = state.geometry.command_rows();
    let Some(Overlay::Themes { cursor, sift, .. }) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char('j') | Key::Down => cursor.down(last),
        Key::Char('k') | Key::Up => cursor.up(),
        Key::PageDown => cursor.page_down(last, height),
        Key::PageUp => cursor.page_up(height),
        Key::Home => cursor.to_start(),
        Key::End => cursor.to_end(last),
        Key::Char('/') => *sift = Some(Sift::opened(cursor.index())),
        _ => {}
    }

    preview_theme(state);

    Vec::new()
}

/// The cursor counts in the rows a sift left. A term that matched nothing holds
/// no row, and the preview stays.
pub(super) fn preview_theme(state: &mut State) {
    let Some(Overlay::Themes { cursor, sift, .. }) = &state.overlay else {
        return;
    };
    let labels = theme_labels(&state.config);
    let search = sift.as_ref().and_then(Sift::search);
    let choice = picker_matches(&labels, search.as_ref())
        .get(cursor.index())
        .copied()
        .and_then(|at| theme_at_picker_index(&state.config, at));

    if let Some(choice) = choice {
        state.theme = choice;
    }
}

/// `Escape` is decided before the overlay is borrowed: [`escape`] needs `&mut
/// State`.
fn themes_sift_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let labels = theme_labels(&state.config);
    let Some(Overlay::Themes { cursor, sift, .. }) = &mut state.overlay else {
        return Vec::new();
    };
    let Some(open) = sift else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => open.push(character),
        Key::Backspace => {
            // An empty query is no search: closing here moves nothing.
            if !open.pop() {
                *sift = None;
                return Vec::new();
            }
        }
        Key::Enter => {
            let at = picker_matches(&labels, open.search().as_ref())
                .get(cursor.index())
                .copied();
            *sift = None;

            if let Some(index) = at {
                cursor.go_to(index, labels.len());
            }
            return Vec::new();
        }
        _ => return Vec::new(),
    }

    let origin = open.origin();
    let matches = picker_matches(&labels, open.search().as_ref());
    let seat = matches
        .iter()
        .position(|at| *at >= origin)
        .unwrap_or_else(|| matches.len().saturating_sub(1));
    cursor.go_to(seat, matches.len());

    preview_theme(state);

    Vec::new()
}

/// Not silent, unlike the first-run example write: that one would warn on every
/// launch.
pub(super) fn theme_save_failed(state: &mut State, reason: String) -> Vec<Effect> {
    logged(
        state,
        sentence("Saving the theme"),
        Outcome::Failed {
            detail: reason.clone(),
        },
    );
    // The theme stays applied.
    raise(
        state,
        Message {
            title: "Theme".to_owned(),
            message: reason,
            severity: Severity::Warning,
        },
    );
    state.mark_dirty();
    Vec::new()
}
