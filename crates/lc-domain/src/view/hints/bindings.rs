use super::{Binding, Stroke};
use crate::contents::SelectionMode;
use crate::model::Key;
use crate::panel::Panel;
use crate::queries::CqlMode;
use crate::state::State;

pub(super) const ENTER: Stroke = Stroke::key(Key::Enter);
pub(super) const ESC: Stroke = Stroke::key(Key::Escape);
pub(super) const TAB: Stroke = Stroke::key(Key::Tab);
const SPACE: Stroke = Stroke::key(Key::Char(' '));

const fn key(character: char) -> Stroke {
    Stroke::key(Key::Char(character))
}

const fn ctrl(character: char) -> Stroke {
    Stroke::ctrl(Key::Char(character))
}

pub(super) const PALETTE: Stroke = key('?');

/// Closes the common tail: the one hint that finds every other binding. Panel
/// numbers in the titles already teach `1`-`5`, so `tab` is off the bar.
pub(crate) const GLOBAL: &[Binding] = &[Binding::new(&[PALETTE], "commands")];

/// Only the overlay bars name it; the palette lists it for the panels.
pub(crate) const QUIT: Binding = Binding::new(&[key('q')], "quit");

pub(super) const CLOSE: Binding = Binding::new(&[ESC], "close");
const CANCEL: Binding = Binding::new(&[ESC], "cancel");
const NO: Binding = Binding::new(&[key('n'), ESC], "cancel");
/// Not global: `/` is text in Queries.
pub(super) const SEARCH: Binding = Binding::new(&[key('/')], "search");
pub(super) const SELECT: Binding = Binding::new(&[key('v')], "select");

/// `esc` is named only here among the globals: with the palette open it always
/// closes it. `⏎ run` first, so it is dropped first.
pub(super) const PALETTE_GLOBAL: &[Binding] = &[
    Binding::new(&[ENTER], "run"),
    Binding::new(&[PALETTE, ESC], "close"),
    QUIT,
    SEARCH,
];

/// No `quit`: `diagnostics_key` claims `q`.
pub(super) const DIAGNOSTICS_GLOBAL: &[Binding] = &[CLOSE];

/// `j/k` named: nothing else says the block moves.
pub(super) const MOVE: Binding = Binding::new(&[key('j'), key('k')], "move");
pub(super) const COPY: Binding = Binding::new(&[key('y')], "copy");
pub(super) const COPY_LINE: Binding = Binding::new(&[key('Y')], "copy line");
pub(super) const EDIT: Binding = Binding::new(&[key('e')], "edit");
pub(super) const INSERT: Binding = Binding::new(&[ENTER], "insert");
pub(super) const UNSET: Binding = Binding::new(&[key('x')], "unset");
pub(super) const COMMIT_ROW: Binding = Binding::new(&[ctrl('s')], "commit row");
pub(super) const INSPECT_GLOBAL: &[Binding] = &[CLOSE];

/// No panel group and no `q`: the field claims every printable key.
pub(super) const FIELD: &[Binding] = &[Binding::new(&[ENTER], "save"), CANCEL];

pub(super) const COMMIT: &[Binding] = &[Binding::new(&[key('y')], "write them"), NO];

pub(super) const DISCARD: &[Binding] = &[
    Binding::new(&[key('y')], "quit and discard"),
    Binding::new(&[key('n'), ESC], "stay"),
];

/// Left off entirely where nothing is uncommitted.
pub(super) const COMMIT_HINT: Binding = Binding::new(&[ctrl('s')], "commit");

/// `j`/`k` named as "preview": the motion's side effect is what it teaches.
pub(crate) const THEMES: &[Binding] = &[Binding::new(&[key('j'), key('k')], "preview"), SEARCH];

/// `⏎` keeps the previewed theme; `esc` restores the original.
pub(crate) const THEMES_GLOBAL: &[Binding] = &[Binding::new(&[ENTER], "keep"), CANCEL, QUIT];

/// No `j`/`k`: moving this cursor previews nothing.
pub(super) const CONNECTIONS: &[Binding] = &[SEARCH];

pub(super) const CONNECTIONS_GLOBAL: &[Binding] = &[Binding::new(&[ENTER], "connect"), CLOSE, QUIT];

/// Last of the panel's hints, so a narrow bar drops it before `run`.
pub(super) const HISTORY_HINT: Binding = Binding::new(&[ctrl('g')], "history");

pub(super) const HISTORY: &[Binding] = &[SEARCH];

/// `⏎` appends and runs nothing, the difference from `ctrl-r` worth saying.
pub(super) const HISTORY_GLOBAL: &[Binding] = &[Binding::new(&[ENTER], "append"), CLOSE, QUIT];

/// `⏎` named: it folds here rather than choosing.
pub(super) const HEALTH: &[Binding] = &[Binding::new(&[ENTER], "fold/unfold")];

pub(super) const HEALTH_GLOBAL: &[Binding] = &[CLOSE, QUIT];

/// Two sets: the spellings answer different keys. No `q`: `drop_key` claims it.
pub(super) const DROP_YES_NO: &[Binding] = &[Binding::new(&[key('y')], "drop it"), NO];
pub(super) const DELETE_YES_NO: &[Binding] = &[Binding::new(&[key('y')], "delete them"), NO];

/// `⏎` is greyed until the typed name matches.
pub(super) const DROP_TYPED: &[Binding] = &[Binding::new(&[ENTER], "drop it"), CANCEL];
pub(super) const DELETE_TYPED: &[Binding] = &[Binding::new(&[ENTER], "delete them"), CANCEL];

pub(super) const NEXT_OPTION: Binding = Binding::new(&[TAB], "next option");
/// Text on the path stop.
pub(super) const TOGGLE: Binding = Binding::new(&[SPACE], "toggle");
pub(super) const EXPORT: Binding = Binding::new(&[SPACE, ENTER], "export");
pub(super) const EXPORT_OVERWRITE: &[Binding] = &[
    Binding::new(&[SPACE, ENTER], "overwrite"),
    Binding::new(&[ESC], "back"),
];

/// Empty panel group: the focused panel responds to nothing under a prompt.
pub(super) const SEARCH_GLOBAL: &[Binding] = &[Binding::new(&[ENTER], "apply"), CANCEL];

/// Only while a popup is up; the `⏎ newline` hint drops for those frames.
pub(super) const COMPLETE: Binding = Binding::new(&[TAB, ENTER], "complete");

pub(super) const CLEAR: Binding = Binding::new(&[ESC], "clear");
/// [`CLEAR`] wins where both apply: one key, one hint.
pub(super) const LEAVE: Binding = Binding::new(&[ESC], "leave");
pub(super) const STOP: Binding = Binding::new(&[ESC], "stop");
/// Not [`LEAVE`]: closing the buffer keeps focus.
pub(super) const DONE: Binding = Binding::new(&[ESC], "done");
pub(super) const NEXT_MATCH: Binding = Binding::new(&[key('n')], "next match");

/// "open" is true of both a keyspace and a table row.
const OPEN: Binding = Binding::new(&[ENTER], "open");
pub(super) const DROP: Binding = Binding::new(&[key('d')], "drop");
pub(super) const HEALTH_OF: Binding = Binding::new(&[key('H')], "health");

/// Only where it opens.
pub(super) const INSPECT_ROW: Binding = Binding::new(&[ENTER], "inspect");
/// Only while there is more to fetch; greyed while a page is in flight.
pub(super) const NEXT_PAGE: Binding = Binding::new(&[key('p')], "next page");
pub(super) const EVERY_PAGE: Binding = Binding::new(&[key('P')], "all");
pub(super) const EXPORT_ROWS: Binding = Binding::new(&[ctrl('e')], "export");
/// Greyed where the rows lack an addressable primary key.
pub(super) const DELETE: Binding = Binding::new(&[key('d')], "delete");
/// Greyed where no form could open; off on a read-only connection.
pub(super) const INSERT_ROW: Binding = Binding::new(&[key('i')], "insert");
const MODE: Binding = Binding::new(&[key('m')], "mode");
pub(super) const SORT: Binding = Binding::new(&[key('s')], "sort");
pub(super) const FILTER: Binding = Binding::new(&[key('f')], "filter");
/// Clears this column's filter only, unlike `esc clear`.
pub(super) const CLEAR_FILTER: Binding = Binding::new(&[key('c')], "clear filter");

pub(super) const RUN: Binding = Binding::new(&[ctrl('r')], "run");
/// Never greyed: always true while Queries is focused.
const EDITOR: Binding = Binding::new(&[ctrl('e')], "$EDITOR");
pub(super) const FORMAT: Binding = Binding::new(&[ctrl('f')], "format");

// Motion is in no panel's bar: users already reach for it, and the palette
// lists it.
const TABLES_HINTS: &[Binding] = &[OPEN, SEARCH, DROP, HEALTH_OF, SELECT];

const DDL_HINTS: &[Binding] = &[SEARCH, SELECT];

const CONTENTS_HINTS: &[Binding] = &[
    INSPECT_ROW,
    NEXT_PAGE,
    EVERY_PAGE,
    EXPORT_ROWS,
    DELETE,
    INSERT_ROW,
    MODE,
    SEARCH,
    SELECT,
];

/// Named only in column mode, whose cursor is a column.
const CONTENTS_COLUMN_HINTS: &[Binding] = &[
    SORT,
    FILTER,
    CLEAR_FILTER,
    INSPECT_ROW,
    NEXT_PAGE,
    EVERY_PAGE,
    EXPORT_ROWS,
    MODE,
    SEARCH,
    SELECT,
];

/// `⏎` first: nothing else says how to start typing.
const CQL_HINTS: &[Binding] = &[Binding::new(&[ENTER], "edit"), RUN, EDITOR];

/// `run` first: it is what the panel is for.
const QUERIES_HINTS: &[Binding] = &[RUN, EDITOR, FORMAT, Binding::new(&[ENTER], "newline")];

const LOGS_HINTS: &[Binding] = &[SEARCH, SELECT];

impl Panel {
    /// Exhaustive, so a new panel cannot compile without its bindings.
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

impl State {
    /// On `State`, not `Panel`: Queries' bindings depend on its tab, and
    /// Contents' on its mode.
    #[must_use]
    pub fn hints(&self) -> &'static [Binding] {
        match self.focus() {
            Panel::Queries => match self.queries.mode() {
                None => LOGS_HINTS,
                Some(CqlMode::Browsing) => CQL_HINTS,
                Some(CqlMode::Editing) => QUERIES_HINTS,
            },
            Panel::Contents if self.contents.mode == SelectionMode::Column => CONTENTS_COLUMN_HINTS,
            panel => panel.hints(),
        }
    }
}
