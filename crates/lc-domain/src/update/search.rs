//! Panel search: the `/` prompt and `n`/`N`.

use crate::model::Key;
use crate::overlay::Overlay;
use crate::search::{Direction, Origin, Prompt, Searched};
use crate::state::State;

use super::{Effect, escape};

fn origin_of(state: &State, target: Searched) -> Origin {
    match target {
        Searched::Tables => Origin {
            row: state.tables.cursor().index(),
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

pub(super) fn restore_origin(state: &mut State, target: Searched, origin: Origin) {
    match target {
        Searched::Tables => state.move_tree_cursor(origin.row),
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

/// Falls back to `from` when nothing matches: a no-op for `n`/`N`, and the
/// restore to `origin` for a live prompt.
fn jump_to_match(state: &mut State, target: Searched, from: Origin, direction: Direction) {
    match target {
        Searched::Tables => {
            let row = state
                .schema()
                .and_then(|schema| state.tables.next_match(schema, from.row, direction));
            match row {
                Some(row) => state.move_tree_cursor(row),
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

/// Motions, so an extending range follows them.
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

/// Clears the target's search at once: the panel previews the empty buffer from
/// the first frame.
pub(super) fn open_prompt(state: &mut State) {
    let Some(target) = Searched::of(state.focus()) else {
        return;
    };

    let previous = state.search(target).cloned();
    let origin = origin_of(state, target);
    state.overlay = Some(Overlay::Search(Prompt::opened(target, previous, origin)));
    state.apply_search(target, None);
}

/// `Escape` is decided before `prompt` borrows: [`escape`] needs `&mut State`.
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
