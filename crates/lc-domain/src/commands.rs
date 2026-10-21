//! Every binding the program has, and a cursor over them. Unlike the hint bar,
//! the palette claims to be complete; `update::tests`' sweep holds it to that.

use crate::contents::ContentsPanel;
use crate::model::Key;
use crate::panel::Panel;
use crate::posture::Posture;
use crate::search::{Search, Sift};
use crate::view::Binding;
use crate::view::Liveness;
use crate::view::Stroke;
use crate::view::hints::label;
use crate::viewport::Cursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub heading: &'static str,
    pub commands: &'static [Command],
}

/// Executability lives here, not on [`Binding`]: it is the palette's concern
/// only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub binding: Binding,
    pub run: Run,
    pub gate: Gate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Anywhere,
    /// The refusal is `update`'s; this is what the palette draws.
    Mutates,
    More,
}

impl Gate {
    const fn permitted(self, posture: Posture) -> bool {
        match self {
            Self::Anywhere | Self::More => true,
            Self::Mutates => posture.permits_mutation(),
        }
    }
}

impl Command {
    const fn new(keys: &'static [Stroke], action: &'static str, run: Run) -> Self {
        Self {
            binding: Binding { keys, action },
            run,
            gate: Gate::Anywhere,
        }
    }

    const fn paging(keys: &'static [Stroke], action: &'static str) -> Self {
        Self {
            gate: Gate::More,
            ..Self::new(keys, action, Run::In(Panel::Contents))
        }
    }

    const fn mutating(keys: &'static [Stroke], action: &'static str, run: Run) -> Self {
        Self {
            gate: Gate::Mutates,
            ..Self::new(keys, action, run)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Run {
    Here,
    In(Panel),
    /// The row is an edit, and a closed buffer claims no key.
    Editing,
    /// The row means something only inside a mode that is not up, and its key
    /// means something else outside it.
    Inert,
}

impl Run {
    const fn liveness(self) -> Liveness {
        match self {
            Self::Inert => Liveness::Inert,
            Self::Here | Self::In(_) | Self::Editing => Liveness::Live,
        }
    }
}

impl Command {
    /// Still listed: a row that vanished on a read-only connection would teach
    /// that the tool cannot drop a table.
    #[must_use]
    pub const fn refused(&self, posture: Posture) -> Option<Posture> {
        if self.gate.permitted(posture) {
            None
        } else {
            Some(posture)
        }
    }
}

const GLOBAL: &[Command] = &[
    Command::new(&[Stroke::key(Key::Char('q'))], "quit", Run::Here),
    Command::new(&[Stroke::key(Key::Char('?'))], "commands", Run::Here),
    Command::new(&[Stroke::ctrl(Key::Char('t'))], "choose a theme", Run::Here),
    Command::new(
        &[Stroke::ctrl(Key::Char('o'))],
        "choose a connection",
        Run::Here,
    ),
    Command::new(&[Stroke::ctrl(Key::Char('c'))], "cancel queries", Run::Here),
    // Queries is the one panel this does nothing in: the editor takes `/` as
    // text.
    Command::new(
        &[Stroke::key(Key::Char('/'))],
        "search this panel",
        Run::Here,
    ),
    Command::new(&[Stroke::key(Key::Char('n'))], "next match", Run::Here),
    Command::new(&[Stroke::key(Key::Char('N'))], "previous match", Run::Here),
    // One row for the whole chain; the column has no room for all four rungs.
    Command::new(&[Stroke::key(Key::Escape)], "back out one step", Run::Here),
    Command::new(&[Stroke::key(Key::Tab)], "next panel", Run::Here),
    Command::new(&[Stroke::key(Key::BackTab)], "previous panel", Run::Here),
    Command::new(&[Stroke::key(Key::Char('1'))], "focus Tables", Run::Here),
    Command::new(&[Stroke::key(Key::Char('2'))], "focus Table DDL", Run::Here),
    // A row per tab teaches that `2` again goes one tab further.
    Command::new(
        &[Stroke::key(Key::Char('2'))],
        "focus Table columns",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::Char('2'))],
        "focus Table indexes",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::Char('3'))],
        "focus Table contents",
        Run::Here,
    ),
    Command::new(&[Stroke::key(Key::Char('4'))], "write a query", Run::Here),
    Command::new(&[Stroke::key(Key::Char('5'))], "focus Logs", Run::Here),
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
    Command::new(
        &[Stroke::key(Key::Char('h')), Stroke::key(Key::Left)],
        "left, or fold",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::Char('l')), Stroke::key(Key::Right)],
        "right, unfold, read",
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
    Command::new(
        &[Stroke::key(Key::Home), Stroke::key(Key::Char('g'))],
        "to the start",
        Run::Here,
    ),
    Command::new(
        &[Stroke::key(Key::End), Stroke::key(Key::Char('G'))],
        "to the end",
        Run::Here,
    ),
];

// Actions are capped at twenty characters: MIN_W derives from the longest. The
// heading supplies the noun.
const TABLES: &[Command] = &[
    Command::new(
        &[Stroke::key(Key::Enter)],
        "fold/unfold, or read",
        Run::In(Panel::Tables),
    ),
    DROP_TABLE,
    TABLE_HEALTH,
];

const TABLE_HEALTH: Command = Command::new(
    &[Stroke::key(Key::Char('H'))],
    "table health",
    Run::In(Panel::Tables),
);

const HEALTH: &[Command] = &[
    TABLE_HEALTH,
    Command::new(
        &[Stroke::key(Key::Enter)],
        "fold/unfold a metric",
        Run::Inert,
    ),
];

// Listed under their panel as well as under their flow's group.
const DROP_TABLE: Command = Command::mutating(
    &[Stroke::key(Key::Char('d'))],
    "drop this",
    Run::In(Panel::Tables),
);
const DELETE_ROWS: Command = Command::mutating(
    &[Stroke::key(Key::Char('d'))],
    "delete these rows",
    Run::In(Panel::Contents),
);
const EDIT_VALUE: Command = Command::mutating(
    &[Stroke::key(Key::Char('e'))],
    "edit this value",
    Run::In(Panel::Contents),
);
const COMMIT_EDITS: Command = Command::mutating(
    &[Stroke::ctrl(Key::Char('s'))],
    "commit the edits",
    Run::In(Panel::Contents),
);
const COMMIT_ROW: Command = Command::mutating(
    &[Stroke::ctrl(Key::Char('s'))],
    "commit this row",
    Run::Inert,
);
const EXPORT_ROWS: Command = Command::new(
    &[Stroke::ctrl(Key::Char('e'))],
    "export these rows",
    Run::In(Panel::Contents),
);

/// The confirmation's keys mean something else outside it, so they are inert
/// here.
const DROP: &[Command] = &[
    DROP_TABLE,
    DELETE_ROWS,
    Command::mutating(&[Stroke::key(Key::Char('y'))], "yes, drop it", Run::Inert),
    Command::mutating(
        &[Stroke::key(Key::Enter)],
        "drop the typed name",
        Run::Inert,
    ),
    // Not gated: cancelling is the way out.
    Command::new(
        &[Stroke::key(Key::Char('n')), Stroke::key(Key::Escape)],
        "cancel the drop",
        Run::Inert,
    ),
];

/// "Commit this row" is inert: from the grid `ctrl-s` would write every row.
const EDIT: &[Command] = &[
    Command::mutating(
        &[Stroke::key(Key::Char('i'))],
        "insert a row",
        Run::In(Panel::Contents),
    ),
    Command::mutating(&[Stroke::key(Key::Enter)], "insert the row", Run::Inert),
    Command::mutating(
        &[Stroke::key(Key::Char('x'))],
        "unset the field",
        Run::Inert,
    ),
    EDIT_VALUE,
    COMMIT_EDITS,
    COMMIT_ROW,
    Command::mutating(
        &[Stroke::key(Key::Char('y'))],
        "yes, write them",
        Run::Inert,
    ),
    Command::new(
        &[Stroke::key(Key::Char('n')), Stroke::key(Key::Escape)],
        "cancel the commit",
        Run::Inert,
    ),
];

const CONTENTS: &[Command] = &[
    Command::new(
        &[Stroke::key(Key::Enter)],
        "inspect",
        Run::In(Panel::Contents),
    ),
    // Inert: it answers inside the inspector alone.
    Command::new(&[Stroke::key(Key::Char('Y'))], "copy this line", Run::Inert),
    Command::paging(&[Stroke::key(Key::Char('p'))], "fetch the next page"),
    Command::paging(&[Stroke::key(Key::Char('P'))], "fetch every page"),
    Command::new(
        &[Stroke::key(Key::Char('m'))],
        "cycle selection mode",
        Run::In(Panel::Contents),
    ),
    // Inert: they answer in column mode alone.
    Command::new(
        &[Stroke::key(Key::Char('s'))],
        "sort by this column",
        Run::Inert,
    ),
    Command::new(
        &[Stroke::key(Key::Char('f'))],
        "filter this column",
        Run::Inert,
    ),
    Command::new(
        &[Stroke::key(Key::Char('c'))],
        "clear this filter",
        Run::Inert,
    ),
    DELETE_ROWS,
    EDIT_VALUE,
    COMMIT_EDITS,
    COMMIT_ROW,
    EXPORT_ROWS,
];

const EXPORT: &[Command] = &[
    EXPORT_ROWS,
    Command::new(&[Stroke::key(Key::Tab)], "next field", Run::Inert),
    Command::new(&[Stroke::key(Key::BackTab)], "previous field", Run::Inert),
    Command::new(
        &[Stroke::key(Key::Char(' '))],
        "select, or press",
        Run::Inert,
    ),
    Command::new(
        &[Stroke::ctrl(Key::Char('s'))],
        "export to file",
        Run::Inert,
    ),
    Command::new(
        &[Stroke::ctrl(Key::Char('y'))],
        "copy to clipboard",
        Run::Inert,
    ),
    Command::new(&[Stroke::key(Key::Escape)], "close the form", Run::Inert),
];

// Printable keys are not rows: they all type themselves.
const QUERIES: &[Command] = &[
    Command::new(
        &[Stroke::key(Key::Enter), Stroke::key(Key::Char('i'))],
        "edit the buffer",
        Run::In(Panel::Queries),
    ),
    Command::new(&[Stroke::key(Key::Escape)], "stop editing", Run::Editing),
    // On the closed Cql tab, rows are statements.
    Command::new(
        &[Stroke::key(Key::Char('j'))],
        "next statement",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::key(Key::Char('k'))],
        "prev statement",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::ctrl(Key::Char('r'))],
        "run the statement",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[Stroke::ctrl(Key::Char('g'))],
        "open the history",
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
        Run::Editing,
    ),
    // Inert: outside a completion they move focus or split a line.
    Command::new(
        &[Stroke::key(Key::Tab), Stroke::key(Key::Enter)],
        "accept completion",
        Run::Inert,
    ),
    Command::new(&[Stroke::key(Key::Enter)], "split the line", Run::Editing),
    Command::new(
        &[Stroke::key(Key::Backspace), Stroke::key(Key::Delete)],
        "delete a character",
        Run::Editing,
    ),
    Command::new(&[Stroke::key(Key::Home)], "line start", Run::Editing),
    Command::new(&[Stroke::key(Key::End)], "line end", Run::Editing),
];

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
        heading: "Health",
        commands: HEALTH,
    },
    Group {
        heading: "Drop and delete",
        commands: DROP,
    },
    Group {
        heading: "Edit",
        commands: EDIT,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRow {
    /// A row, so cursor and renderer share one numbering.
    Separator,
    Heading(&'static str),
    Command {
        keys: String,
        action: &'static str,
        /// Faint when the row is documentation rather than a command.
        liveness: Liveness,
        refused: Option<Posture>,
    },
    /// A row, not an empty block: an empty bordered box reads as a failure.
    Empty,
}

/// The unfiltered index puts the cursor back when the prompt closes.
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

    fn row(self, posture: Posture, contents: &ContentsPanel) -> CommandRow {
        match self {
            Self::Separator => CommandRow::Separator,
            Self::Heading(heading) => CommandRow::Heading(heading),
            Self::Command { command, .. } => CommandRow::Command {
                keys: label(command.binding.keys),
                action: command.binding.action,
                liveness: if command.gate == Gate::More && !contents.has_more() {
                    Liveness::Inert
                } else {
                    command.run.liveness()
                },
                refused: command.refused(posture),
            },
        }
    }
}

/// Matches the action alone, so the mark lands where the term named.
fn slots(search: Option<&Search>) -> Vec<Slot> {
    let mut slots = Vec::new();
    let mut at = 0;

    for (index, group) in CATALOGUE.iter().enumerate() {
        // The separator, then the heading.
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

        // The first surviving group never opens with a rule.
        if !slots.is_empty() {
            slots.push(Slot::Separator);
        }
        slots.push(Slot::Heading(group.heading));
        slots.extend(kept);
    }

    slots
}

/// `posture` changes how a row is drawn, never how many there are, so no border
/// moves.
#[must_use]
pub fn rows(
    search: Option<&Search>,
    posture: Posture,
    contents: &ContentsPanel,
) -> Vec<CommandRow> {
    let slots = slots(search);

    if slots.is_empty() {
        return vec![CommandRow::Empty];
    }

    slots
        .into_iter()
        .map(|slot| slot.row(posture, contents))
        .collect()
}

/// The block is sized from `row_count(None)`, so a filter moves no border.
#[must_use]
pub fn row_count(search: Option<&Search>) -> usize {
    slots(search).len().max(1)
}

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

/// Every motion seats the cursor on a command, never a heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandsPanel {
    cursor: Cursor,
    /// Held here so seating stays behind this type's methods.
    sift: Option<Sift>,
}

impl CommandsPanel {
    #[must_use]
    pub fn opened() -> Self {
        let mut panel = Self {
            cursor: Cursor::default(),
            sift: None,
        };
        panel.to_start();
        panel
    }

    #[must_use]
    pub const fn selected(&self) -> usize {
        self.cursor.index()
    }

    #[must_use]
    pub const fn sift(&self) -> Option<&Sift> {
        self.sift.as_ref()
    }

    #[must_use]
    pub fn search(&self) -> Option<Search> {
        self.sift.as_ref().and_then(Sift::search)
    }

    #[must_use]
    pub fn command(&self) -> Option<&'static Command> {
        self.slots()
            .get(self.selected())
            .copied()
            .and_then(Slot::command)
    }

    pub fn open_sift(&mut self) {
        self.sift = Some(Sift::opened(self.selected()));
    }

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

    pub fn restore_origin(&mut self) {
        if let Some(sift) = self.sift.take() {
            self.seat(sift.origin(), Direction::Down);
        }
    }

    /// The last match when none lies beyond, so backspacing to empty returns to
    /// the start.
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

    pub fn down(&mut self) {
        self.seat(self.selected().saturating_add(1), Direction::Down);
    }

    pub fn up(&mut self) {
        self.seat(self.selected().saturating_sub(1), Direction::Up);
    }

    pub fn page_down(&mut self, height: usize) {
        self.seat(self.selected().saturating_add(height), Direction::Down);
    }

    pub fn page_up(&mut self, height: usize) {
        self.seat(self.selected().saturating_sub(height), Direction::Up);
    }

    pub fn to_start(&mut self) {
        self.seat(0, Direction::Down);
    }

    pub fn to_end(&mut self) {
        let slots = self.slots();
        self.seat_in(&slots, slots.len().saturating_sub(1), Direction::Up);
    }

    fn slots(&self) -> Vec<Slot> {
        slots(self.search().as_ref())
    }

    /// Falls back the other way, so running out stops on a command, not a
    /// heading.
    fn seat(&mut self, target: usize, direction: Direction) {
        let slots = self.slots();
        self.seat_in(&slots, target, direction);
    }

    /// Row zero only when a term matched nothing.
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
    use crate::panel::Panel;
    use crate::view::Liveness;

    /// `ctrl-n/ctrl-p` fills the key gutter exactly; the renderer's `MIN_W`
    /// derives from these.
    const WIDEST_KEYS: usize = 13;
    const WIDEST_ACTION: usize = 20;

    fn commands() -> Vec<&'static Command> {
        CATALOGUE.iter().flat_map(|group| group.commands).collect()
    }

    /// A posture that refuses nothing.
    fn rows(search: Option<&Search>) -> Vec<CommandRow> {
        super::rows(search, Posture::Writable, &ContentsPanel::default())
    }

    fn row_for(action: &str, posture: Posture) -> CommandRow {
        super::rows(None, posture, &ContentsPanel::default())
            .into_iter()
            .find(|row| matches!(row, CommandRow::Command { action: a, .. } if *a == action))
            .unwrap_or_else(|| panic!("no row for {action}"))
    }

    fn liveness_of(action: &str, posture: Posture) -> Liveness {
        let CommandRow::Command { liveness, .. } = row_for(action, posture) else {
            unreachable!("row_for only returns commands")
        };
        liveness
    }

    fn refusal_of(action: &str, posture: Posture) -> Option<Posture> {
        let CommandRow::Command { refused, .. } = row_for(action, posture) else {
            unreachable!("row_for only returns commands")
        };
        refused
    }

    #[test]
    fn a_read_only_connection_refuses_every_command_that_would_change_the_cluster() {
        for action in [
            "drop this",
            "delete these rows",
            "yes, drop it",
            "drop the typed name",
        ] {
            assert_eq!(
                refusal_of(action, Posture::ReadOnly),
                Some(Posture::ReadOnly),
                "`{action}` would be refused, so the palette says so"
            );
        }
    }

    #[test]
    fn a_read_only_connection_drops_no_row_from_the_palette() {
        assert_eq!(
            super::rows(None, Posture::ReadOnly, &ContentsPanel::default()).len(),
            super::rows(None, Posture::Writable, &ContentsPanel::default()).len()
        );
        assert_eq!(
            row_count(None),
            super::rows(None, Posture::ReadOnly, &ContentsPanel::default()).len()
        );
    }

    #[test]
    fn a_writable_connection_refuses_nothing_and_greys_only_what_the_run_greys() {
        for action in ["drop this", "delete these rows"] {
            assert_eq!(liveness_of(action, Posture::Writable), Liveness::Live);
            assert_eq!(refusal_of(action, Posture::Writable), None);
        }

        // `Run::Inert` is independent of the posture.
        assert_eq!(
            liveness_of("yes, drop it", Posture::Writable),
            Liveness::Inert
        );
    }

    #[test]
    fn cancelling_a_drop_is_not_gated_on_the_posture() {
        let cancel = commands()
            .into_iter()
            .find(|command| command.binding.action == "cancel the drop")
            .expect("no cancel row");

        assert_eq!(cancel.gate, Gate::Anywhere);
    }

    #[test]
    fn every_gated_command_is_a_drop_a_delete_or_an_edit() {
        for command in commands() {
            if command.gate == Gate::Mutates {
                assert!(
                    DROP.contains(command) || EDIT.contains(command),
                    "`{}` is gated but is in neither mutating group",
                    command.binding.action
                );
            }
        }
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

    #[test]
    fn g_is_set_beside_home() {
        assert!(rows(None).contains(&CommandRow::Command {
            keys: "home/g".to_owned(),
            action: "to the start",
            liveness: Liveness::Live,
            refused: None,
        }));
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
                action: "run the statement",
                liveness: Liveness::Live,
                refused: None,
            }),
            "the palette claims to be complete, and ctrl-r is the one binding no key types"
        );
    }

    #[test]
    fn the_panel_number_names_every_tab_its_further_presses_reach() {
        let rows = rows(None);
        let ddl = rows
            .iter()
            .position(|row| {
                *row == CommandRow::Command {
                    keys: "2".to_owned(),
                    action: "focus Table DDL",
                    liveness: Liveness::Live,
                    refused: None,
                }
            })
            .expect("no row focuses the DDL panel");

        assert_eq!(
            rows[ddl + 1..=ddl + 2],
            [
                CommandRow::Command {
                    keys: "2".to_owned(),
                    action: "focus Table columns",
                    liveness: Liveness::Live,
                    refused: None,
                },
                CommandRow::Command {
                    keys: "2".to_owned(),
                    action: "focus Table indexes",
                    liveness: Liveness::Live,
                    refused: None,
                },
            ]
        );
    }

    #[test]
    fn the_catalogue_lists_visual_mode() {
        assert!(rows(None).contains(&CommandRow::Command {
            keys: "v".to_owned(),
            action: "select rows",
            liveness: Liveness::Live,
            refused: None,
        }));
    }

    #[test]
    fn the_catalogue_lists_the_external_editor() {
        assert!(
            rows(None).contains(&CommandRow::Command {
                keys: "ctrl-e".to_owned(),
                action: "open in $EDITOR",
                liveness: Liveness::Live,
                refused: None,
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
                refused: None,
            }
        );
        assert_eq!(rows.len(), row_count(None));
    }

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
                .all(|command| command.run == Run::In(Panel::Contents)
                    || command.run == Run::Inert)
        );
        assert!(
            group("Queries")
                .commands
                .iter()
                .filter(|command| {
                    command.binding.keys != [Stroke::key(Key::Tab), Stroke::key(Key::Enter)]
                })
                .all(|command| matches!(command.run, Run::In(Panel::Queries) | Run::Editing)),
            "every row but the completion's, which is inert for the reason the \
             export form's rows are"
        );
    }

    #[test]
    fn a_panels_group_lists_the_mutations_it_opens() {
        let group = |heading| {
            CATALOGUE
                .iter()
                .find(|group| group.heading == heading)
                .unwrap_or_else(|| panic!("no {heading} group"))
        };

        assert!(group("Tables").commands.contains(&DROP_TABLE));
        for opener in [DELETE_ROWS, EDIT_VALUE, COMMIT_EDITS, EXPORT_ROWS] {
            assert!(group("Table contents").commands.contains(&opener));
        }
    }

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
                    refused: None,
                },
            ],
            "one heading, one command, and no rule above the first group kept"
        );
    }

    #[test]
    fn a_search_seats_the_palette_on_a_command() {
        let panel = typed("choose a theme");

        assert_eq!(
            panel.command().map(|command| command.binding.action),
            Some("choose a theme")
        );
    }

    #[test]
    fn the_palettes_search_reads_the_action_and_not_the_keys() {
        let panel = typed("ctrl-");

        assert_eq!(rows(panel.search().as_ref()), [CommandRow::Empty]);
        assert_eq!(panel.selected(), 0);
        assert!(panel.command().is_none(), "the empty line runs nothing");
    }

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

    #[test]
    fn closing_a_sift_keeps_the_command_the_filter_found() {
        let mut panel = typed("choose a theme");

        panel.close_sift();

        assert_eq!(rows(panel.search().as_ref()).len(), row_count(None));
        assert_eq!(panel.selected(), row_of("choose a theme"));
    }

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
