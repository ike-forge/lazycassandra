//! Panel search: the `/` prompt and `n`/`N`
//! (docs/superpowers/specs/2026-08-10-panel-search-design.md).

use crate::model::Key;
use crate::search::{Direction, Origin, Prompt, Searched};
use crate::state::{Overlay, State};

use super::{Effect, escape, tree_rows};

/// Where `target`'s cursor is now.
fn origin_of(state: &State, target: Searched) -> Origin {
    match target {
        Searched::Tables => Origin {
            row: state.tables.cursor.index(),
            column: 0,
        },
        Searched::Ddl => Origin {
            row: state.ddl.cursor.index(),
            column: 0,
        },
        Searched::Contents => Origin {
            row: state.contents.cursor.index(),
            column: state.contents.column.index(),
        },
        Searched::Queries => Origin {
            row: state.log.cursor.index(),
            column: 0,
        },
    }
}

/// Puts `target`'s cursor back where it was.
pub(super) fn restore_origin(state: &mut State, target: Searched, origin: Origin) {
    match target {
        Searched::Tables => {
            let len = tree_rows(state);
            state.tables.cursor.go_to(origin.row, len);
        }
        Searched::Ddl => {
            let len = crate::view::ddl_rows(state);
            state.ddl.cursor.go_to(origin.row, len);
        }
        Searched::Contents => {
            let rows = state.contents.row_count();
            let columns = state.contents.column_count();
            state.contents.cursor.go_to(origin.row, rows);
            state.contents.column.go_to(origin.column, columns);
        }
        Searched::Queries => {
            let len = state.log.len();
            state.log.cursor.go_to(origin.row, len);
        }
    }
}

/// Moves `target`'s cursor to the next match from `from`, and falls back to
/// `from` itself when nothing matches (design §5.2) — a no-op for `n`/`N`,
/// which always call with `from` already the cursor, and the promised
/// restore to `origin` for a live `/` prompt whose term stops matching.
fn jump_to_match(state: &mut State, target: Searched, from: Origin, direction: Direction) {
    match target {
        Searched::Tables => {
            let row = state
                .schema()
                .and_then(|schema| state.tables.next_match(schema, from.row, direction));
            match row {
                Some(row) => {
                    let len = tree_rows(state);
                    state.tables.cursor.go_to(row, len);
                }
                None => restore_origin(state, target, from),
            }
        }
        Searched::Ddl => match crate::view::ddl_next_match(state, from.row, direction) {
            Some(row) => {
                let len = crate::view::ddl_rows(state);
                state.ddl.cursor.go_to(row, len);
            }
            None => restore_origin(state, target, from),
        },
        Searched::Contents => {
            let hit = state
                .contents
                .next_match((from.row, from.column), direction);
            match hit {
                Some((row, column)) => {
                    let rows = state.contents.row_count();
                    let columns = state.contents.column_count();
                    state.contents.cursor.go_to(row, rows);
                    state.contents.column.go_to(column, columns);
                }
                None => restore_origin(state, target, from),
            }
        }
        Searched::Queries => match state.log.next_match(from.row, direction) {
            Some(row) => {
                let len = state.log.len();
                state.log.cursor.go_to(row, len);
            }
            None => restore_origin(state, target, from),
        },
    }
}

/// `n` and `N` walk the focused panel's matches, wrapping.
///
/// Motions, so they take the path `j` and `k` take and an extending range
/// follows them. Inert where the panel holds no search — a panel that cannot
/// honour a motion swallows it (docs/shortcuts.md §2) — and unreachable from
/// Queries' Cql tab, which takes text and never sees this key at all; its
/// Logs tab holds a search like any other list (design §5.3).
pub(super) fn next_match_key(state: &mut State, direction: Direction) {
    let Some(target) = Searched::of(state.focus()) else {
        return;
    };

    if state.search(target).is_none() {
        return;
    }

    let from = origin_of(state, target);
    jump_to_match(state, target, from, direction);
}

/// `/` opens a prompt over the focused panel. Unreachable over Queries' Cql
/// tab, since the editor claims `/` as text before this is ever called
/// (panel search design §8); reachable over its Logs tab, which holds a
/// search like any other list (query log design §5.3).
///
/// Clears the target's search immediately rather than leaving `previous`
/// showing until the first keystroke: the buffer opens empty, and live
/// search (design §3.3) means the panel underneath is the preview of
/// whatever the buffer holds, from the first frame the prompt is up.
pub(super) fn open_prompt(state: &mut State) {
    let Some(target) = Searched::of(state.focus()) else {
        return;
    };

    let previous = state.search(target).cloned();
    let origin = origin_of(state, target);
    state.overlay = Some(Overlay::Search(Prompt::opened(target, previous, origin)));
    state.apply_search(target, None);
}

/// What an open prompt responds to.
///
/// Every printable key is a character, so `q` types a `q` — the opposite of
/// the palette, which leaves `q` quitting. The two differ because the palette
/// takes motion and this takes text (panel search design §1.1, §3.1).
///
/// `Escape` is decided before the prompt's mutable borrow is taken: it defers
/// to [`escape`], which needs `&mut State` whole, and no reborrow of it can
/// coexist with `prompt`. `Enter` stays inside the match because it only needs
/// `&mut state.overlay`, and `prompt` is dead in that arm.
pub(super) fn search_key(state: &mut State, key: Key) -> Vec<Effect> {
    if matches!(key, Key::Escape) {
        escape(state);
        return Vec::new();
    }

    let Some(Overlay::Search(prompt)) = &mut state.overlay else {
        return Vec::new();
    };

    match key {
        Key::Char(character) => prompt.push(character),
        Key::Backspace => prompt.pop(),
        Key::Enter => {
            state.overlay = None;
            return Vec::new();
        }
        _ => return Vec::new(),
    }

    let (target, search, origin) = (prompt.target(), prompt.search(), prompt.origin());
    state.apply_search(target, search);
    jump_to_match(state, target, origin, Direction::Forwards);

    Vec::new()
}
