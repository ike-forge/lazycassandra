//! Every binding the program has, and a cursor over them.
//!
//! The palette claims to be complete, which the hint bar deliberately does not
//! (docs/superpowers/specs/2026-08-09-command-palette-design.md §2). `update::tests`'
//! sweep is what holds that claim up: reusing [`Binding`] means a row names
//! `KeyPress` values rather than a string, so a row cannot describe a press no
//! arm of `handle_key` claims.

use crate::model::{Key, KeyPress};
use crate::search::{Search, Sift};
use crate::state::{Cursor, Panel};
use crate::view::Binding;
use crate::view::Liveness;
use crate::view::Stroke;
use crate::view::hints::label;

/// One labelled group of commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub heading: &'static str,
    pub commands: &'static [Command],
}

/// A binding, and what the palette does when `⏎` lands on it.
///
/// Executability is the palette's concern and not the hint bar's, so it is
/// held here rather than on [`Binding`], which both are built from
/// (docs/superpowers/specs/2026-08-15-palette-execution-design.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub binding: Binding,
    pub run: Run,
}

impl Command {
    const fn new(keys: &'static [Stroke], action: &'static str, run: Run) -> Self {
        Self {
            binding: Binding { keys, action },
            run,
        }
    }
}

/// Where a row's key is pressed when the palette runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Run {
    /// Against whatever has focus.
    Here,
    /// Focus this panel, then press it.
    In(Panel),
    /// Not pressed at all: the row means something only inside a mode that is
    /// not up, and its key means something else outside that mode (design §5).
    Inert,
}

impl Run {
    const fn liveness(self) -> Liveness {
        match self {
            Self::Inert => Liveness::Inert,
            Self::Here | Self::In(_) => Liveness::Live,
        }
    }
}

const GLOBAL: &[Command] = &[
    Command::new(&[Stroke::key(Key::Char('q'))], "quit", Run::Here),
    Command::new(&[Stroke::key(Key::Char('?'))], "commands", Run::Here),
    Command::new(&[Stroke::ctrl(Key::Char('t'))], "choose a theme", Run::Here),
    // Queries is the one panel this does nothing in: there the editor claims
    // `/` as text before the binding is consulted (panel search design §5.3,
    // §8). That is a panel that has not caught up rather than a key that is
    // not global (palette execution design §6).
    Command::new(
        &[Stroke::key(Key::Char('/'))],
        "search this panel",
        Run::Here,
    ),
    Command::new(&[Stroke::key(Key::Char('n'))], "next match", Run::Here),
    Command::new(&[Stroke::key(Key::Char('N'))], "previous match", Run::Here),
    Command::new(
        &[Stroke::key(Key::Escape)],
        "dismiss the overlay",
        Run::Here,
    ),
    Command::new(&[Stroke::key(Key::Tab)], "next panel", Run::Here),
    Command::new(&[Stroke::key(Key::BackTab)], "previous panel", Run::Here),
    Command::new(&[Stroke::key(Key::Char('1'))], "focus Tables", Run::Here),
    Command::new(&[Stroke::key(Key::Char('2'))], "focus Table DDL", Run::Here),
    // The one panel with a second tab names it, because the pairing is what
    // teaches the rule: two rows over the same key, and a reader who never
    // presses `]` still learns that `2` again reaches the Columns tab.
    Command::new(
        &[Stroke::key(Key::Char('2'))],
        "focus Table columns",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::Char('3'))],
        "focus Table contents",
        Run::Here,
    ),
    Command::new(&[Stroke::key(Key::Char('4'))], "focus Queries", Run::Here),
    // The rule itself — a number pressed again steps one tab further in —
    // lives here rather than under all five numbers: only the DDL panel has a
    // tab to reach today, and only that press is spelled out above.
    Command::new(&[Stroke::key(Key::Char(']'))], "next tab", Run::Here),
    Command::new(&[Stroke::key(Key::Char('['))], "prev tab", Run::Here),
    Command::new(&[Stroke::key(Key::Char('y'))], "yank", Run::Here),
    Command::new(&[Stroke::key(Key::Char('v'))], "select rows", Run::Here),
];

const MOTION: &[Command] = &[
    Command::new(
        &[Stroke::key(Key::Char('j')), Stroke::key(Key::Down)],
        "down one row",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::Char('k')), Stroke::key(Key::Up)],
        "up one row",
        Run::Here,
    ),
    // Worded for the movement rather than for what a panel makes of it: the
    // four horizontal keys mean move left / move right, and each panel
    // resolves them in its own vocabulary (docs/shortcuts.md §2). A row per
    // panel repeated the pair under every heading that answers it, which is
    // three rows saying "these keys go sideways" in three wordings.
    Command::new(
        &[Stroke::key(Key::Char('h')), Stroke::key(Key::Left)],
        "left, or fold",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::Char('l')), Stroke::key(Key::Right)],
        "right, or unfold",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::PageDown), Stroke::ctrl(Key::Char('d'))],
        "down one screen",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::PageUp), Stroke::ctrl(Key::Char('u'))],
        "up one screen",
        Run::Here,
    ),
    // `gg` is one row's second spelling rather than a row of its own: it is
    // the landing `home` already makes, in the spelling a vim hand types
    // (docs/shortcuts.md §1).
    Command::new(
        &[
            Stroke::key(Key::Home),
            Stroke::Chord(&[KeyPress::new(Key::Char('g')), KeyPress::new(Key::Char('g'))]),
        ],
        "to the start",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::End), Stroke::key(Key::Char('G'))],
        "to the end",
        Run::Here,
    ),
];

// The actions are cut to twenty characters because MIN_W is derived from the
// longest one (design §5). The heading supplies the noun they drop: under
// `Tables`, "read" can only mean a table.
//
// The horizontal keys are not here: they are `Navigate`'s, and a panel group
// lists only what that panel alone answers.
const TABLES: &[Command] = &[Command::new(
    &[Stroke::key(Key::Enter)],
    "fold/unfold, or read",
    Run::In(Panel::Tables),
)];

const CONTENTS: &[Command] = &[Command::new(
    &[Stroke::key(Key::Char('m'))],
    "cycle selection mode",
    Run::In(Panel::Contents),
)];

/// The form's own keys. `Tab` and `Esc` also appear under `Global` meaning
/// something else, exactly as `l`/`h` appear under two panels: the palette may
/// not omit what a key does somewhere
/// (docs/superpowers/specs/2026-08-09-command-palette-design.md §2).
const EXPORT: &[Command] = &[
    Command::new(
        &[Stroke::ctrl(Key::Char('e'))],
        "export these rows",
        Run::In(Panel::Contents),
    ),
    Command::new(&[Stroke::key(Key::Tab)], "next field", Run::Inert),
    Command::new(&[Stroke::key(Key::BackTab)], "previous field", Run::Inert),
    Command::new(
        &[Stroke::key(Key::Char(' '))],
        "select, or export",
        Run::Inert,
    ),
    Command::new(&[Stroke::key(Key::Escape)], "close the form", Run::Inert),
];

// The editor's printable keys are not rows: every one of them types itself,
// which is the panel's whole rule rather than a list of bindings
// (docs/shortcuts.md §2). What is listed is what a key does *not* do elsewhere.
const QUERIES: &[Command] = &[
    Command::new(
        &[Stroke::ctrl(Key::Char('r'))],
        "run the buffer",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::ctrl(Key::Char('e'))],
        "open in $EDITOR",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::ctrl(Key::Char('f'))],
        "format the buffer",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::ctrl(Key::Char('n')), Stroke::ctrl(Key::Char('p'))],
        "walk the completions",
        Run::In(Panel::Queries),
    ),
    // Inert for the reason the export form's rows are: `tab` and `⏎` accept a
    // completion only while one is being offered, and mean what they mean
    // everywhere else in every other frame. A palette that pressed either
    // would move focus or split a line (design §5.5).
    Command::new(
        &[Stroke::key(Key::Tab), Stroke::key(Key::Enter)],
        "accept completion",
        Run::Inert,
    ),
    Command::new(
        &[Stroke::key(Key::Enter)],
        "split the line",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::key(Key::Backspace), Stroke::key(Key::Delete)],
        "delete a character",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::key(Key::Home)],
        "line start",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::key(Key::End)],
        "line end",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::key(Key::Escape)],
        "leave the editor",
        Run::In(Panel::Queries),
    ),
];

/// Every binding, grouped as the palette lists them.
pub const CATALOGUE: &[Group] = &[
    Group {
        heading: "Global",
        commands: GLOBAL,
    },
    Group {
        heading: "Navigate",
        commands: MOTION,
    },
    Group {
        heading: "Tables",
        commands: TABLES,
    },
    Group {
        heading: "Table contents",
        commands: CONTENTS,
    },
    Group {
        heading: "Export",
        commands: EXPORT,
    },
    Group {
        heading: "Queries",
        commands: QUERIES,
    },
];

/// One line of the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRow {
    /// The rule between one group and the next. A row rather than a gap the
    /// renderer inserts, for the reason a heading is one: there is a single
    /// row-numbering scheme, and the cursor and the renderer both count in it
    /// (docs/superpowers/specs/2026-08-09-command-palette-design.md §4).
    Separator,
    Heading(&'static str),
    Command {
        keys: String,
        action: &'static str,
        /// Faint when the row is documentation rather than a command: `⏎` on
        /// it does nothing, and a row that looks live and is not is the one
        /// way this list can mislead
        /// (docs/superpowers/specs/2026-08-15-palette-execution-design.md §8).
        liveness: Liveness,
    },
    /// What a term that matched nothing leaves behind. A row rather than an
    /// empty block: a search still typing towards its first match is not a
    /// failure state, and an empty bordered box reads as one
    /// (docs/superpowers/specs/2026-08-31-overlay-filter-design.md §3).
    Empty,
}

/// One row, and where that row sits in the unfiltered catalogue.
///
/// The two numberings share exactly this: the index is what puts a cursor
/// seated in a filtered list back into the full one when the prompt closes
/// (overlay filter design §3).
#[derive(Clone, Copy)]
enum Slot {
    Separator,
    Heading(&'static str),
    Command {
        at: usize,
        command: &'static Command,
    },
}

impl Slot {
    const fn command(self) -> Option<&'static Command> {
        match self {
            Self::Command { command, .. } => Some(command),
            Self::Separator | Self::Heading(_) => None,
        }
    }

    const fn at(self) -> Option<usize> {
        match self {
            Self::Command { at, .. } => Some(at),
            Self::Separator | Self::Heading(_) => None,
        }
    }

    fn row(self) -> CommandRow {
        match self {
            Self::Separator => CommandRow::Separator,
            Self::Heading(heading) => CommandRow::Heading(heading),
            Self::Command { command, .. } => CommandRow::Command {
                keys: label(command.binding.keys),
                action: command.binding.action,
                liveness: command.run.liveness(),
            },
        }
    }
}

/// The catalogue as a flat list under `search`, headings included: every
/// command whose action matches, and no heading of a group that kept none.
///
/// Headings are rows so that the cursor and the renderer count in the same
/// scheme; [`CommandsPanel`] is what keeps the cursor off them. Matching is
/// the action alone, for the reason overlay search design §6 gives: a row
/// matching in its key column would put the mark where the term did not name.
fn slots(search: Option<&Search>) -> Vec<Slot> {
    let mut slots = Vec::new();
    let mut at = 0;

    for (index, group) in CATALOGUE.iter().enumerate() {
        // The separator, then the heading: neither is selectable, and both
        // sit above this group's first command.
        let heading = at + usize::from(index > 0);
        let first = heading + 1;
        at = first + group.commands.len();

        let mut kept = group
            .commands
            .iter()
            .enumerate()
            .filter(|(_, command)| {
                search.is_none_or(|search| search.matches(command.binding.action))
            })
            .map(|(offset, command)| Slot::Command {
                at: first + offset,
                command,
            })
            .peekable();

        if kept.peek().is_none() {
            continue;
        }

        // The rule separates groups that are drawn, so the first surviving
        // group never opens with one however many were filtered out above it.
        if !slots.is_empty() {
            slots.push(Slot::Separator);
        }
        slots.push(Slot::Heading(group.heading));
        slots.extend(kept);
    }

    slots
}

/// The rows the palette draws under `search`, `None` meaning no prompt or a
/// query too short to mean one.
#[must_use]
pub fn rows(search: Option<&Search>) -> Vec<CommandRow> {
    let slots = slots(search);

    if slots.is_empty() {
        return vec![CommandRow::Empty];
    }

    slots.into_iter().map(Slot::row).collect()
}

/// How many rows [`rows`] produces. The block's geometry is derived from
/// `row_count(None)` rather than from this call's answer, so a filter moves
/// no border (overlay filter design §2).
#[must_use]
pub fn row_count(search: Option<&Search>) -> usize {
    slots(search).len().max(1)
}

/// The command drawn on row `index`, or `None` for a heading, a rule or the
/// empty line.
#[must_use]
pub fn command_at(search: Option<&Search>, index: usize) -> Option<&'static Command> {
    slots(search).get(index).copied().and_then(Slot::command)
}

#[derive(Clone, Copy)]
enum Direction {
    Down,
    Up,
}

impl Direction {
    const fn reversed(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
        }
    }
}

/// The palette's whole state.
///
/// The catalogue is static, so a cursor is all there is to remember. Every
/// motion seats it on a command, which is the invariant a renderer would
/// otherwise have to defend against — the same reason [`Cursor`]'s index is
/// private (conventions.md §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandsPanel {
    cursor: Cursor,
    /// The live search over the catalogue, while one is up. Held here rather
    /// than beside the overlay so that seating stays behind this type's
    /// methods, which is what keeps the cursor off a heading
    /// (overlay search design §4).
    sift: Option<Sift>,
}

impl CommandsPanel {
    /// Opens on the first command, which is the row after the first heading.
    #[must_use]
    pub fn opened() -> Self {
        let mut panel = Self {
            cursor: Cursor::default(),
            sift: None,
        };
        panel.to_start();
        panel
    }

    /// The selected row, as an index into [`rows`] built with [`Self::search`].
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.cursor.index()
    }

    #[must_use]
    pub const fn sift(&self) -> Option<&Sift> {
        self.sift.as_ref()
    }

    /// What the open sift means, and so which rows the palette is showing —
    /// `None` while there is no prompt or the query is still empty.
    #[must_use]
    pub fn search(&self) -> Option<Search> {
        self.sift.as_ref().and_then(Sift::search)
    }

    /// The command the cursor is on, `None` on a heading, a rule or the row a
    /// term that matched nothing leaves.
    #[must_use]
    pub fn command(&self) -> Option<&'static Command> {
        self.slots()
            .get(self.selected())
            .copied()
            .and_then(Slot::command)
    }

    /// Opens a search on the row the cursor is on.
    pub fn open_sift(&mut self) {
        self.sift = Some(Sift::opened(self.selected()));
    }

    /// Drops the search, putting the cursor back on the same command in the
    /// list the palette widens back out to (overlay filter design §1).
    pub fn close_sift(&mut self) {
        let at = self
            .slots()
            .get(self.selected())
            .copied()
            .and_then(Slot::at);
        self.sift = None;

        if let Some(index) = at {
            self.seat(index, Direction::Down);
        }
    }

    pub fn push(&mut self, character: char) {
        if let Some(sift) = &mut self.sift {
            sift.push(character);
            self.reseat();
        }
    }

    /// Deletes a character, or closes the search when there is none left —
    /// which is what a `backspace` on an empty query means (design §5).
    pub fn pop(&mut self) {
        let Some(sift) = &mut self.sift else {
            return;
        };

        if sift.pop() {
            self.reseat();
        } else {
            self.sift = None;
        }
    }

    /// Seats the cursor back where the search opened, and drops it.
    pub fn restore_origin(&mut self) {
        if let Some(sift) = self.sift.take() {
            self.seat(sift.origin(), Direction::Down);
        }
    }

    /// The first surviving command at or beyond the row the prompt opened on,
    /// and the last one when the term kept nothing beyond it — so backspacing
    /// back to an empty term returns the reader to where they started
    /// (overlay filter design §1).
    fn reseat(&mut self) {
        let Some(sift) = &self.sift else {
            return;
        };
        let origin = sift.origin();
        let slots = self.slots();
        let target = slots
            .iter()
            .position(|slot| slot.at().is_some_and(|at| at >= origin))
            .unwrap_or(slots.len());

        self.seat_in(&slots, target, Direction::Down);
    }

    /// Moves to the next command, skipping a heading, and stopping on the
    /// last one.
    pub fn down(&mut self) {
        self.seat(self.selected().saturating_add(1), Direction::Down);
    }

    /// Moves to the previous command, skipping a heading, and stopping on
    /// the first one.
    pub fn up(&mut self) {
        self.seat(self.selected().saturating_sub(1), Direction::Up);
    }

    /// Moves a screen of commands down, skipping a heading, and stopping on
    /// the last one.
    pub fn page_down(&mut self, height: usize) {
        self.seat(self.selected().saturating_add(height), Direction::Down);
    }

    /// Moves a screen of commands up, skipping a heading, and stopping on
    /// the first one.
    pub fn page_up(&mut self, height: usize) {
        self.seat(self.selected().saturating_sub(height), Direction::Up);
    }

    /// Moves to the first command, the row after the first heading.
    pub fn to_start(&mut self) {
        self.seat(0, Direction::Down);
    }

    /// Moves to the last command.
    pub fn to_end(&mut self) {
        let slots = self.slots();
        self.seat_in(&slots, slots.len().saturating_sub(1), Direction::Up);
    }

    fn slots(&self) -> Vec<Slot> {
        slots(self.search().as_ref())
    }

    /// Seats the cursor on the first command at or beyond `target` in
    /// `direction`, and failing that on the nearest one the other way — so a
    /// motion that runs out of catalogue stops on the last command rather
    /// than on the heading it happened to reach.
    fn seat(&mut self, target: usize, direction: Direction) {
        let slots = self.slots();
        self.seat_in(&slots, target, direction);
    }

    /// Row zero when there is no command to seat on at all, which only a term
    /// that matched nothing produces: the cursor holds the empty line, and
    /// `⏎` there finds no command to run.
    fn seat_in(&mut self, slots: &[Slot], target: usize, direction: Direction) {
        let seat = seek(slots, target, direction)
            .or_else(|| seek(slots, target, direction.reversed()))
            .unwrap_or(0);

        self.cursor.go_to(seat, slots.len().max(1));
    }
}

fn seek(slots: &[Slot], from: usize, direction: Direction) -> Option<usize> {
    let last = slots.len().checked_sub(1)?;
    let mut index = from.min(last);

    loop {
        if slots[index].command().is_some() {
            return Some(index);
        }
        match direction {
            Direction::Down if index < last => index += 1,
            Direction::Up if index > 0 => index -= 1,
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Panel;
    use crate::view::Liveness;

    /// The widest row `MIN_W` in the renderer is derived from
    /// (docs/superpowers/specs/2026-08-09-command-palette-design.md §5): a
    /// thirteen-column key gutter, two columns of gap, and the longest action.
    /// `ctrl-n/ctrl-p` fills the gutter exactly, and the renderer's `MIN_W` is
    /// derived from the constant rather than from the catalogue, so the two
    /// stay pinned to each other.
    const WIDEST_KEYS: usize = 13;
    const WIDEST_ACTION: usize = 20;

    fn commands() -> Vec<&'static Command> {
        CATALOGUE.iter().flat_map(|group| group.commands).collect()
    }

    #[test]
    fn no_command_row_is_wider_than_the_narrowest_palette() {
        for command in commands() {
            let keys = label(command.binding.keys);

            assert!(
                keys.chars().count() <= WIDEST_KEYS,
                "{keys:?} is wider than the gutter MIN_W is derived from"
            );
            assert!(
                command.binding.action.chars().count() <= WIDEST_ACTION,
                "{:?} is longer than the action column MIN_W is derived from",
                command.binding.action
            );
        }
    }

    /// A chord is spelled as it is typed, and set beside the key that does
    /// the same thing rather than given a row of its own.
    #[test]
    fn the_chord_is_spelled_as_one_stroke() {
        assert!(
            rows(None).contains(&CommandRow::Command {
                keys: "home/gg".to_owned(),
                action: "to the start",
                liveness: Liveness::Live,
            }),
            "the palette must spell gg as two presses of one key, not two alternatives"
        );
    }

    #[test]
    fn a_row_that_cannot_be_run_says_so() {
        let row = |action: &str| {
            rows(None)
                .into_iter()
                .find(|row| matches!(row, CommandRow::Command { action: a, .. } if *a == action))
                .unwrap_or_else(|| panic!("no row labelled {action:?}"))
        };

        assert!(matches!(
            row("next field"),
            CommandRow::Command {
                liveness: Liveness::Inert,
                ..
            }
        ));
        assert!(matches!(
            row("export these rows"),
            CommandRow::Command {
                liveness: Liveness::Live,
                ..
            }
        ));
    }

    #[test]
    fn the_palette_names_the_run_binding() {
        assert!(
            rows(None).contains(&CommandRow::Command {
                keys: "ctrl-r".to_owned(),
                action: "run the buffer",
                liveness: Liveness::Live,
            }),
            "the palette claims to be complete, and ctrl-r is the one binding no key types"
        );
    }

    /// The second press is a row of its own, directly under the first: a
    /// reader who never presses `]` still learns that `2` reaches the Columns
    /// tab, and the pairing only teaches it while the two rows stay adjacent
    /// (docs/shortcuts.md §1).
    #[test]
    fn the_panel_number_names_the_tab_its_second_press_reaches() {
        let rows = rows(None);
        let ddl = rows
            .iter()
            .position(|row| {
                *row == CommandRow::Command {
                    keys: "2".to_owned(),
                    action: "focus Table DDL",
                    liveness: Liveness::Live,
                }
            })
            .expect("no row focuses the DDL panel");

        assert_eq!(
            rows[ddl + 1],
            CommandRow::Command {
                keys: "2".to_owned(),
                action: "focus Table columns",
                liveness: Liveness::Live,
            }
        );
    }

    #[test]
    fn the_catalogue_lists_visual_mode() {
        assert!(rows(None).contains(&CommandRow::Command {
            keys: "v".to_owned(),
            action: "select rows",
            liveness: Liveness::Live,
        }));
    }

    #[test]
    fn the_catalogue_lists_the_external_editor() {
        assert!(
            rows(None).contains(&CommandRow::Command {
                keys: "ctrl-e".to_owned(),
                action: "open in $EDITOR",
                liveness: Liveness::Live,
            }),
            "the palette claims to be complete"
        );
    }

    #[test]
    fn the_palette_lists_the_export_form() {
        let rows = rows(None);
        let headings: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                CommandRow::Heading(heading) => Some(*heading),
                CommandRow::Command { .. } | CommandRow::Separator | CommandRow::Empty => None,
            })
            .collect();

        assert!(headings.contains(&"Export"));
    }

    #[test]
    fn the_palette_has_a_queries_section() {
        assert!(
            CATALOGUE.iter().any(|group| group.heading == "Queries"),
            "a panel that takes text still has bindings, and the palette lists every binding"
        );
    }

    #[test]
    fn every_group_has_at_least_one_command() {
        for group in CATALOGUE {
            assert!(
                !group.commands.is_empty(),
                "{} is a heading with nothing under it, and `opened` would seat the cursor on it",
                group.heading
            );
        }
    }

    /// `MIN_H` in the renderer assumes the catalogue fills a palette at its
    /// floor. A catalogue that shrank below it would be refused a palette on
    /// frames that could hold one.
    #[test]
    fn the_catalogue_is_taller_than_the_palettes_floor() {
        assert!(row_count(None) >= 6, "{} rows", row_count(None));
    }

    #[test]
    fn the_catalogue_flattens_to_a_heading_then_its_commands() {
        let rows = rows(None);

        assert_eq!(rows[0], CommandRow::Heading("Global"));
        assert_eq!(
            rows[1],
            CommandRow::Command {
                keys: "q".to_owned(),
                action: "quit",
                liveness: Liveness::Live,
            }
        );
        assert_eq!(rows.len(), row_count(None));
    }

    /// `/` is global, and Queries is the one panel that has not caught up:
    /// there the editor claims it as text before the binding is consulted
    /// (panel search design §5.3). A heading of its own said that far more
    /// heavily than a comment does.
    #[test]
    fn the_catalogue_lists_search_among_the_global_keys() {
        let global = CATALOGUE
            .iter()
            .find(|group| group.heading == "Global")
            .expect("no Global group");

        assert!(
            global
                .commands
                .iter()
                .any(|command| command.binding.keys == [Stroke::key(Key::Char('/'))]),
            "no global row binds /"
        );
        assert!(
            !CATALOGUE.iter().any(|group| group.heading == "Search"),
            "one row does not need a heading"
        );
    }

    #[test]
    fn the_catalogue_lists_the_selection_mode_key() {
        assert!(
            commands()
                .iter()
                .any(|command| command.binding.keys.contains(&Stroke::key(Key::Char('m')))),
            "no row binds m"
        );
    }

    /// A hint is the half of the pair that chooses what to teach; the palette
    /// is the half with no discretion (docs/shortcuts.md §5). So a key a bar
    /// names is by construction a key that does something, and the catalogue
    /// owes it a row. `m` reached a bar and no group, which is what this
    /// catches without anyone having to read the document.
    #[test]
    fn the_catalogue_lists_every_key_a_panel_hints_at() {
        let catalogued = commands();

        for panel in Panel::ALL {
            for hint in panel.hints() {
                for press in hint.keys {
                    assert!(
                        catalogued
                            .iter()
                            .any(|command| command.binding.keys.contains(press)),
                        "{panel:?} hints at {press:?} and no group lists it"
                    );
                }
            }
        }
    }

    #[test]
    fn a_rule_stands_between_one_group_and_the_next() {
        let rows = rows(None);
        let rules = rows
            .iter()
            .filter(|row| matches!(row, CommandRow::Separator))
            .count();

        assert_eq!(rules, CATALOGUE.len() - 1, "one per join, none at the top");
        assert!(!matches!(rows[0], CommandRow::Separator));

        for (index, row) in rows.iter().enumerate() {
            if matches!(row, CommandRow::Separator) {
                assert!(
                    matches!(rows[index + 1], CommandRow::Heading(_)),
                    "the rule at {index} separates nothing"
                );
            }
        }
    }

    #[test]
    fn the_cursor_steps_over_a_rule() {
        let mut panel = CommandsPanel::opened();

        for _ in 0..row_count(None) {
            assert!(
                !matches!(rows(None)[panel.selected()], CommandRow::Separator),
                "the cursor landed on the rule at {}",
                panel.selected()
            );
            panel.down();
        }
    }

    /// The horizontal keys moved to `Navigate` and no panel group restates
    /// them: a key each panel resolves for itself is still one key, and three
    /// wordings of "these go sideways" is what the palette was reading as.
    #[test]
    fn only_the_shared_group_lists_the_horizontal_keys() {
        for group in CATALOGUE {
            for command in group.commands {
                let horizontal = command.binding.keys.contains(&Stroke::key(Key::Char('h')))
                    || command.binding.keys.contains(&Stroke::key(Key::Char('l')));

                assert!(
                    !horizontal || group.heading == "Navigate",
                    "{} restates a key Navigate already lists",
                    group.heading
                );
            }
        }
    }

    #[test]
    fn a_palette_opens_on_the_first_command() {
        assert_eq!(CommandsPanel::opened().selected(), 1);
    }

    #[test]
    fn the_palette_cursor_steps_over_a_heading() {
        let headings: Vec<usize> = rows(None)
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, CommandRow::Heading(_)))
            .map(|(index, _)| index)
            .collect();

        assert!(headings.len() > 1, "one group would not exercise the skip");

        let mut panel = CommandsPanel::opened();
        let mut seen = vec![panel.selected()];

        for _ in 1..row_count(None) {
            panel.down();
            seen.push(panel.selected());
        }

        for heading in headings {
            assert!(
                !seen.contains(&heading),
                "the cursor landed on heading row {heading}"
            );
        }

        let mut panel = CommandsPanel::opened();
        panel.to_end();

        for _ in 1..row_count(None) {
            panel.up();
            assert!(
                !matches!(rows(None)[panel.selected()], CommandRow::Heading(_)),
                "walking up landed on row {}",
                panel.selected()
            );
        }
    }

    #[test]
    fn the_palette_cursor_cannot_run_off_the_catalogue() {
        let mut panel = CommandsPanel::opened();
        for _ in 0..row_count(None) * 2 {
            panel.down();
        }
        let last = panel.selected();

        assert_eq!(
            last,
            row_count(None) - 1,
            "the last row is the last command"
        );

        for _ in 0..row_count(None) * 2 {
            panel.up();
        }

        assert_eq!(panel.selected(), 1);
    }

    #[test]
    fn home_and_end_land_on_the_first_and_last_command() {
        let mut panel = CommandsPanel::opened();

        panel.to_end();
        assert_eq!(panel.selected(), row_count(None) - 1);

        panel.to_start();
        assert_eq!(panel.selected(), 1);
    }

    #[test]
    fn paging_settles_on_a_command_rather_than_a_heading() {
        for height in 1..row_count(None) {
            let mut panel = CommandsPanel::opened();

            panel.page_down(height);
            assert!(
                !matches!(rows(None)[panel.selected()], CommandRow::Heading(_)),
                "paging down by {height} landed on a heading"
            );

            panel.page_up(height);
            assert!(
                !matches!(rows(None)[panel.selected()], CommandRow::Heading(_)),
                "paging up by {height} landed on a heading"
            );
        }
    }

    #[test]
    fn a_row_that_belongs_to_a_panel_says_which() {
        let group = |heading: &str| {
            CATALOGUE
                .iter()
                .find(|group| group.heading == heading)
                .unwrap_or_else(|| panic!("no {heading} group"))
        };

        assert!(
            group("Global")
                .commands
                .iter()
                .all(|command| command.run == Run::Here),
            "a global binding acts wherever focus is"
        );
        assert!(
            group("Navigate")
                .commands
                .iter()
                .all(|command| command.run == Run::Here),
            "each panel resolves a motion in its own vocabulary"
        );
        assert!(
            group("Tables")
                .commands
                .iter()
                .all(|command| command.run == Run::In(Panel::Tables))
        );
        assert!(
            group("Table contents")
                .commands
                .iter()
                .all(|command| command.run == Run::In(Panel::Contents))
        );
        assert!(
            group("Queries")
                .commands
                .iter()
                .filter(|command| {
                    command.binding.keys != [Stroke::key(Key::Tab), Stroke::key(Key::Enter)]
                })
                .all(|command| command.run == Run::In(Panel::Queries)),
            "every row but the completion's, which is inert for the reason the \
             export form's rows are"
        );
    }

    /// The form's `tab`, `space` and `esc` mean what they say only while the
    /// form is up, and mean something else outside it. `ctrl-e` is what opens
    /// it, and is the one row in the group that can be run from a palette.
    #[test]
    fn the_export_form_rows_are_documentation_rather_than_commands() {
        let export = CATALOGUE
            .iter()
            .find(|group| group.heading == "Export")
            .expect("no Export group");

        for command in export.commands {
            let expected = if command.binding.keys == [Stroke::ctrl(Key::Char('e'))] {
                Run::In(Panel::Contents)
            } else {
                Run::Inert
            };

            assert_eq!(command.run, expected, "{}", command.binding.action);
        }
    }

    #[test]
    fn a_row_index_resolves_to_the_command_drawn_on_it() {
        let rows = rows(None);

        for (index, row) in rows.iter().enumerate() {
            match row {
                CommandRow::Command { keys, action, .. } => {
                    let command = command_at(None, index).unwrap_or_else(|| {
                        panic!("row {index} draws a command and resolves to none")
                    });

                    assert_eq!(&label(command.binding.keys), keys);
                    assert_eq!(command.binding.action, *action);
                }
                CommandRow::Heading(_) | CommandRow::Separator | CommandRow::Empty => {
                    assert!(
                        command_at(None, index).is_none(),
                        "row {index} is not a command and resolves to one"
                    );
                }
            }
        }
    }

    /// `⏎` presses a row's first key, so a row naming a key and its opposite
    /// would make that press a coin flip
    /// (docs/superpowers/specs/2026-08-15-palette-execution-design.md §4).
    #[test]
    fn a_row_names_one_action_or_a_set_of_synonyms() {
        let named = |action: &str, key: Stroke| {
            assert!(
                commands()
                    .iter()
                    .any(|command| command.binding.action == action
                        && command.binding.keys.first() == Some(&key)),
                "no row presses {key:?} for {action:?}"
            );
        };

        named("next tab", Stroke::key(Key::Char(']')));
        named("prev tab", Stroke::key(Key::Char('[')));
        named("next field", Stroke::key(Key::Tab));
        named("previous field", Stroke::key(Key::BackTab));
        named("line start", Stroke::key(Key::Home));
        named("line end", Stroke::key(Key::End));

        for command in commands() {
            assert!(
                !command.binding.action.contains('/')
                    || command.binding.keys.len() == 1
                    || command.binding.action.contains(", or "),
                "{:?} names several keys and several actions",
                command.binding.action
            );
        }
    }

    fn row_of(action: &str) -> usize {
        rows(None)
            .iter()
            .position(|row| matches!(row, CommandRow::Command { action: it, .. } if *it == action))
            .expect("a command with that action")
    }

    fn typed(term: &str) -> CommandsPanel {
        let mut panel = CommandsPanel::opened();
        panel.open_sift();

        for character in term.chars() {
            panel.push(character);
        }

        panel
    }

    fn actions(panel: &CommandsPanel) -> Vec<&'static str> {
        rows(panel.search().as_ref())
            .iter()
            .filter_map(|row| match row {
                CommandRow::Command { action, .. } => Some(*action),
                CommandRow::Heading(_) | CommandRow::Separator | CommandRow::Empty => None,
            })
            .collect()
    }

    /// Overlay filter design §1: a group keeps the commands that match, and a
    /// group that keeps none is dropped whole — its heading and rule with it.
    #[test]
    fn a_search_narrows_the_palette_to_the_rows_that_match() {
        let panel = typed("theme");
        let drawn = rows(panel.search().as_ref());

        assert_eq!(actions(&panel), ["choose a theme"]);
        assert_eq!(
            drawn,
            [
                CommandRow::Heading("Global"),
                CommandRow::Command {
                    keys: label(&[Stroke::ctrl(Key::Char('t'))]),
                    action: "choose a theme",
                    liveness: Liveness::Live,
                },
            ],
            "one heading, one command, and no rule above the first group kept"
        );
    }

    /// Design §1, and overlay search design §6 before it: the cursor seats on
    /// a command, never on the heading the filter kept above it.
    #[test]
    fn a_search_seats_the_palette_on_a_command() {
        let panel = typed("choose a theme");

        assert_eq!(
            panel.command().map(|command| command.binding.action),
            Some("choose a theme")
        );
    }

    /// Design §6: the term matches the action and nothing else — typing the
    /// key column's text keeps no row.
    #[test]
    fn the_palettes_search_reads_the_action_and_not_the_keys() {
        let panel = typed("ctrl-");

        assert_eq!(rows(panel.search().as_ref()), [CommandRow::Empty]);
        assert_eq!(panel.selected(), 0);
        assert!(panel.command().is_none(), "the empty line runs nothing");
    }

    /// Design §1: the cursor lands on the first match at or beyond the row the
    /// prompt opened on, so a search from the end of the list does not jump
    /// the reader back to the top of it.
    #[test]
    fn a_search_seats_from_the_row_it_opened_on() {
        let mut panel = CommandsPanel::opened();
        panel.to_end();
        panel.open_sift();

        for character in "next".chars() {
            panel.push(character);
        }

        let found = panel.command().expect("a command matches");
        let last = actions(&panel).last().copied().expect("at least one match");

        assert_eq!(found.binding.action, last, "the last match, not the first");
    }

    /// Design §1: `⏎` widens the list back out and keeps the cursor on the
    /// command the filter left it on.
    #[test]
    fn closing_a_sift_keeps_the_command_the_filter_found() {
        let mut panel = typed("choose a theme");

        panel.close_sift();

        assert_eq!(rows(panel.search().as_ref()).len(), row_count(None));
        assert_eq!(panel.selected(), row_of("choose a theme"));
    }

    /// Design §1: `Esc` puts the cursor back on the row the prompt opened on.
    #[test]
    fn restoring_a_palette_sift_puts_the_cursor_back() {
        let mut panel = CommandsPanel::opened();
        panel.down();
        let origin = panel.selected();
        panel.open_sift();
        panel.push('q');

        panel.restore_origin();

        assert_eq!(panel.selected(), origin);
        assert!(panel.sift().is_none());
    }

    /// Design §5: a `backspace` that finds the query empty closes the prompt
    /// and leaves the cursor.
    #[test]
    fn popping_an_empty_palette_sift_closes_it() {
        let mut panel = CommandsPanel::opened();
        panel.open_sift();
        panel.push('q');
        let landed = panel.selected();

        panel.pop();
        assert!(panel.sift().is_some());

        panel.pop();
        assert!(panel.sift().is_none());
        assert_eq!(panel.selected(), landed, "closing moves nothing");
        assert_eq!(
            rows(panel.search().as_ref()).len(),
            row_count(None),
            "and the whole catalogue is back"
        );
    }
}
