//! The theme picker
//! (docs/superpowers/specs/2026-08-10-themes-design.md).

use crate::config::Config;
use crate::model::Key;
use crate::overlay::Overlay;
use crate::search::Sift;
use crate::state::State;
use crate::theme::{BuiltIn, ThemeChoice};
use crate::viewport::Cursor;

use super::{Effect, escape, picker_matches, quit};

/// Puts back the theme a live picker opened on, if the overlay is that
/// picker. Otherwise a no-op. Three sites unwind a preview: [`escape`], which
/// does it while taking the overlay apart, the resize-shrink close in
/// [`measured`], and [`unwind_overlay`] — which every replacement from a
/// message shares, so no such site can copy the check and diverge from the
/// rest.
pub(super) fn restore_theme(state: &mut State) {
    if let Some(Overlay::Themes { restored, .. }) = &state.overlay {
        state.theme = restored.clone();
    }
}

/// Where `choice` sits in the picker's list: the built-ins, then the
/// config's own, in config order — the same order [`crate::view`] labels it.
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

/// The choice the picker's `index`th row names, or `None` past the end of
/// the built-ins and the config's own.
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

/// The picker's rows in picker order: the built-ins, then the config's own.
///
/// The one builder for this order, shared with [`crate::view`]'s
/// `OverlayView::Themes`: the search's whole correctness claim is that
/// `theme_labels[i]` is the label the renderer draws at row `i`, so the two
/// cannot be allowed to drift into separate copies (review, item 2).
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

/// `ctrl-t` opens the theme picker, seated on the theme in force.
///
/// Refused where the frame cannot draw the list, for the reason
/// [`open_palette`] gives. Gated on the picker's own room, [`Geometry::theme_rows`],
/// rather than the palette's: the picker used to be the smaller block always,
/// but the list is every built-in plus every theme a config defines, and a
/// long enough custom list no longer fits where a palette would (design §9).
///
/// Refused over a live prompt too, which no unmodified key needs to say: the
/// ctrl layer is dispatched above every overlay, so this is the one opener a
/// prompt cannot swallow, and replacing one would strand the panel under a
/// half-typed query with nothing left holding the search it had.
///
/// A picker's own live sift needs no matching guard here: `ctrl_key` already
/// refuses every ctrl chord over any open overlay, `Overlay::Themes { .. }`
/// included, before this function is ever called, so a sift up inside the
/// picker is covered by that blanket refusal, not this one.
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

/// What an open picker responds to.
///
/// Moving the cursor sets the theme, so the frame underneath is the preview —
/// the same live-as-you-type rule the search prompt follows (panel search
/// design §1).
///
/// `q` still quits, for the reason [`palette_key`] gives. The ends are
/// reachable here as they are in every other list, `home`/`end` and `g`/`G`
/// alike: paging does land on either end in one press in a list this short,
/// but a hand that types `G` in the panels and gets nothing in the picker has
/// found the one list that is different, which costs more than the arm
/// (docs/shortcuts.md §1).
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

/// The picker's cursor is the preview: the theme it sits on is the theme in
/// force, and every motion that moves it says so (themes design §6).
///
/// The cursor counts in the rows a live sift left, so the row it holds is
/// resolved through those before the config is asked for a theme. A term that
/// matched nothing holds no row, and the preview stays where it was.
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

/// What a picker with a live sift responds to.
///
/// The overlay takes text, so the letters that move it or quit type
/// themselves — the panel-that-takes-text rule, applied to an overlay
/// (overlay search design §5). `Escape` defers to [`escape`], which needs
/// `&mut State` whole, so it is decided before the overlay is borrowed —
/// [`search_key`]'s reason.
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
            // An empty query is no search, so the list the cursor is counting
            // in is already the whole one: closing here moves nothing.
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

    // The first surviving row at or beyond the one the prompt opened on, and
    // the last one when the term kept nothing beyond it (overlay filter
    // design §1).
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
