//! Every binding the program has, and a cursor over them.
//!
//! The palette claims to be complete, which the hint bar deliberately does not
//! (docs/superpowers/specs/2026-08-09-command-palette-design.md §2). `update::tests`'
//! sweep is what holds that claim up: reusing [`Binding`] means a row names
//! `KeyPress` values rather than a string, so a row cannot describe a press no
//! arm of `handle_key` claims.

use crate::model::{Key, KeyPress};
use crate::search::Sift;
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
    // Inert for the reason the export form's rows are: `tab` accepts a
    // completion only while one is being offered, and moves focus every other
    // frame. A palette that pressed it would move focus (design §5.5).
    Command::new(&[Stroke::key(Key::Tab)], "accept completion", Run::Inert),
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
}

/// The catalogue as a flat list, headings included.
///
/// Headings are rows so that the cursor and the renderer count in the same
/// scheme; [`CommandsPanel`] is what keeps the cursor off them.
#[must_use]
pub fn rows() -> Vec<CommandRow> {
    CATALOGUE
        .iter()
        .enumerate()
        .flat_map(|(index, group)| {
            let rule = (index > 0).then_some(CommandRow::Separator);

            rule.into_iter()
                .chain(std::iter::once(CommandRow::Heading(group.heading)))
                .chain(group.commands.iter().map(|command| CommandRow::Command {
                    keys: label(command.binding.keys),
                    action: command.binding.action,
                    liveness: command.run.liveness(),
                }))
        })
        .collect()
}

/// How many rows [`rows`] produces, without building them.
#[must_use]
pub fn row_count() -> usize {
    CATALOGUE
        .iter()
        .enumerate()
        .map(|(index, group)| usize::from(index > 0) + 1 + group.commands.len())
        .sum()
}

/// The command drawn on row `index`, or `None` for a heading or a rule.
#[must_use]
pub fn command_at(index: usize) -> Option<&'static Command> {
    let mut at = 0;

    for (group_index, group) in CATALOGUE.iter().enumerate() {
        // The separator, then the heading: neither is selectable, and both
        // sit below this group's first command.
        at += usize::from(group_index > 0);
        if index <= at {
            return None;
        }
        let first = at + 1;
        at += 1 + group.commands.len();
        if index < at {
            return group.commands.get(index - first);
        }
    }

    None
}

fn is_command(index: usize) -> bool {
    command_at(index).is_some()
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

    /// The selected row, as an index into [`rows`].
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.cursor.index()
    }

    #[must_use]
    pub const fn sift(&self) -> Option<&Sift> {
        self.sift.as_ref()
    }

    /// Opens a search on the row the cursor is on.
    pub fn open_sift(&mut self) {
        self.sift = Some(Sift::opened(self.selected()));
    }

    /// Drops the search, leaving the cursor where it landed.
    pub fn close_sift(&mut self) {
        self.sift = None;
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

    /// The first command at or after the origin whose action matches,
    /// wrapping once; the origin itself when nothing does (design §6).
    fn reseat(&mut self) {
        let Some(sift) = &self.sift else {
            return;
        };
        let origin = sift.origin();
        let seat = sift
            .search()
            .and_then(|search| {
                let total = row_count();
                (0..total)
                    .map(|step| (origin + step) % total)
                    .find(|index| {
                        command_at(*index)
                            .is_some_and(|command| search.matches(command.binding.action))
                    })
            })
            .unwrap_or(origin);

        self.seat(seat, Direction::Down);
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
        self.seat(row_count().saturating_sub(1), Direction::Up);
    }

    /// Seats the cursor on the first command at or beyond `target` in
    /// `direction`, and failing that on the nearest one the other way — so a
    /// motion that runs out of catalogue stops on the last command rather
    /// than on the heading it happened to reach.
    fn seat(&mut self, target: usize, direction: Direction) {
        let seat = seek(target, direction).or_else(|| seek(target, direction.reversed()));

        if let Some(index) = seat {
            self.cursor.go_to(index, row_count());
        }
    }
}

fn seek(from: usize, direction: Direction) -> Option<usize> {
    let last = row_count().checked_sub(1)?;
    let mut index = from.min(last);

    loop {
        if is_command(index) {
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
            rows().contains(&CommandRow::Command {
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
            rows()
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
            rows().contains(&CommandRow::Command {
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
        let rows = rows();
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
        assert!(rows().contains(&CommandRow::Command {
            keys: "v".to_owned(),
            action: "select rows",
            liveness: Liveness::Live,
        }));
    }

    #[test]
    fn the_catalogue_lists_the_external_editor() {
        assert!(
            rows().contains(&CommandRow::Command {
                keys: "ctrl-e".to_owned(),
                action: "open in $EDITOR",
                liveness: Liveness::Live,
            }),
            "the palette claims to be complete"
        );
    }

    #[test]
    fn the_palette_lists_the_export_form() {
        let rows = rows();
        let headings: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                CommandRow::Heading(heading) => Some(*heading),
                CommandRow::Command { .. } | CommandRow::Separator => None,
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
        assert!(row_count() >= 6, "{} rows", row_count());
    }

    #[test]
    fn the_catalogue_flattens_to_a_heading_then_its_commands() {
        let rows = rows();

        assert_eq!(rows[0], CommandRow::Heading("Global"));
        assert_eq!(
            rows[1],
            CommandRow::Command {
                keys: "q".to_owned(),
                action: "quit",
                liveness: Liveness::Live,
            }
        );
        assert_eq!(rows.len(), row_count());
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
        let rows = rows();
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

        for _ in 0..row_count() {
            assert!(
                !matches!(rows()[panel.selected()], CommandRow::Separator),
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
        let headings: Vec<usize> = rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, CommandRow::Heading(_)))
            .map(|(index, _)| index)
            .collect();

        assert!(headings.len() > 1, "one group would not exercise the skip");

        let mut panel = CommandsPanel::opened();
        let mut seen = vec![panel.selected()];

        for _ in 1..row_count() {
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

        for _ in 1..row_count() {
            panel.up();
            assert!(
                !matches!(rows()[panel.selected()], CommandRow::Heading(_)),
                "walking up landed on row {}",
                panel.selected()
            );
        }
    }

    #[test]
    fn the_palette_cursor_cannot_run_off_the_catalogue() {
        let mut panel = CommandsPanel::opened();
        for _ in 0..row_count() * 2 {
            panel.down();
        }
        let last = panel.selected();

        assert_eq!(last, row_count() - 1, "the last row is the last command");

        for _ in 0..row_count() * 2 {
            panel.up();
        }

        assert_eq!(panel.selected(), 1);
    }

    #[test]
    fn home_and_end_land_on_the_first_and_last_command() {
        let mut panel = CommandsPanel::opened();

        panel.to_end();
        assert_eq!(panel.selected(), row_count() - 1);

        panel.to_start();
        assert_eq!(panel.selected(), 1);
    }

    #[test]
    fn paging_settles_on_a_command_rather_than_a_heading() {
        for height in 1..row_count() {
            let mut panel = CommandsPanel::opened();

            panel.page_down(height);
            assert!(
                !matches!(rows()[panel.selected()], CommandRow::Heading(_)),
                "paging down by {height} landed on a heading"
            );

            panel.page_up(height);
            assert!(
                !matches!(rows()[panel.selected()], CommandRow::Heading(_)),
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
                .filter(|command| command.binding.keys != [Stroke::key(Key::Tab)])
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
        let rows = rows();

        for (index, row) in rows.iter().enumerate() {
            match row {
                CommandRow::Command { keys, action, .. } => {
                    let command = command_at(index).unwrap_or_else(|| {
                        panic!("row {index} draws a command and resolves to none")
                    });

                    assert_eq!(&label(command.binding.keys), keys);
                    assert_eq!(command.binding.action, *action);
                }
                CommandRow::Heading(_) | CommandRow::Separator => {
                    assert!(
                        command_at(index).is_none(),
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
        rows()
            .iter()
            .position(|row| matches!(row, CommandRow::Command { action: it, .. } if *it == action))
            .expect("a command with that action")
    }

    /// Overlay search design §6: the scan runs over command rows only, so a
    /// search can no more land on a heading than `j` can.
    #[test]
    fn a_search_seats_the_palette_on_a_command() {
        let mut panel = CommandsPanel::opened();
        panel.open_sift();

        for character in "choose a theme".chars() {
            panel.push(character);
        }

        assert_eq!(panel.selected(), row_of("choose a theme"));
        assert!(
            command_at(panel.selected()).is_some(),
            "never a heading or a rule"
        );
    }

    /// Design §6: the term matches the action and nothing else — typing the
    /// key column's text finds nothing.
    #[test]
    fn the_palettes_search_reads_the_action_and_not_the_keys() {
        let mut panel = CommandsPanel::opened();
        let origin = panel.selected();
        panel.open_sift();

        for character in "ctrl-".chars() {
            panel.push(character);
        }

        assert_eq!(panel.selected(), origin, "no action says ctrl-");
    }

    /// Design §6: the scan wraps once past the end of the catalogue.
    #[test]
    fn the_palettes_search_wraps_past_the_end() {
        let mut panel = CommandsPanel::opened();
        panel.to_end();
        panel.open_sift();

        for character in "quit".chars() {
            panel.push(character);
        }

        assert_eq!(panel.selected(), row_of("quit"));
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
    }
}
