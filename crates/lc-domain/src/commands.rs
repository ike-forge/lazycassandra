//! Every binding the program has, and a cursor over them.
//!
//! The palette claims to be complete, which the hint bar deliberately does not
//! (docs/superpowers/specs/2026-08-09-command-palette-design.md §2). `update::tests`'
//! sweep is what holds that claim up: reusing [`Binding`] means a row names
//! `KeyPress` values rather than a string, so a row cannot describe a press no
//! arm of `handle_key` claims.

use crate::model::{Key, KeyPress};
use crate::state::{Cursor, Panel};
use crate::view::Binding;
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
    const fn new(keys: &'static [KeyPress], action: &'static str, run: Run) -> Self {
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

const GLOBAL: &[Command] = &[
    Command::new(&[KeyPress::new(Key::Char('q'))], "quit", Run::Here),
    Command::new(&[KeyPress::new(Key::Char('?'))], "commands", Run::Here),
    Command::new(
        &[KeyPress::ctrl(Key::Char('t'))],
        "choose a theme",
        Run::Here,
    ),
    // Queries is the one panel this does nothing in: there the editor claims
    // `/` as text before the binding is consulted (panel search design §5.3,
    // §8). That is a panel that has not caught up rather than a key that is
    // not global (palette execution design §6).
    Command::new(
        &[KeyPress::new(Key::Char('/'))],
        "search this panel",
        Run::Here,
    ),
    Command::new(
        &[KeyPress::new(Key::Escape)],
        "dismiss the overlay",
        Run::Here,
    ),
    Command::new(&[KeyPress::new(Key::Tab)], "next panel", Run::Here),
    Command::new(&[KeyPress::new(Key::BackTab)], "previous panel", Run::Here),
    Command::new(&[KeyPress::new(Key::Char('1'))], "focus Tables", Run::Here),
    Command::new(
        &[KeyPress::new(Key::Char('2'))],
        "focus Table DDL",
        Run::Here,
    ),
    Command::new(
        &[KeyPress::new(Key::Char('3'))],
        "focus Table contents",
        Run::Here,
    ),
    Command::new(&[KeyPress::new(Key::Char('4'))], "focus Queries", Run::Here),
    Command::new(&[KeyPress::new(Key::Char(']'))], "next tab", Run::Here),
    Command::new(&[KeyPress::new(Key::Char('['))], "prev tab", Run::Here),
    Command::new(&[KeyPress::new(Key::Char('y'))], "yank", Run::Here),
    Command::new(&[KeyPress::new(Key::Char('v'))], "select rows", Run::Here),
];

const MOTION: &[Command] = &[
    Command::new(
        &[KeyPress::new(Key::Char('j')), KeyPress::new(Key::Down)],
        "down one row",
        Run::Here,
    ),
    Command::new(
        &[KeyPress::new(Key::Char('k')), KeyPress::new(Key::Up)],
        "up one row",
        Run::Here,
    ),
    // Worded for the movement rather than for what a panel makes of it: the
    // four horizontal keys mean move left / move right, and each panel
    // resolves them in its own vocabulary (docs/shortcuts.md §2). A row per
    // panel repeated the pair under every heading that answers it, which is
    // three rows saying "these keys go sideways" in three wordings.
    Command::new(
        &[KeyPress::new(Key::Char('h')), KeyPress::new(Key::Left)],
        "left, or fold",
        Run::Here,
    ),
    Command::new(
        &[KeyPress::new(Key::Char('l')), KeyPress::new(Key::Right)],
        "right, or unfold",
        Run::Here,
    ),
    Command::new(
        &[KeyPress::new(Key::PageDown), KeyPress::ctrl(Key::Char('d'))],
        "down one screen",
        Run::Here,
    ),
    Command::new(
        &[KeyPress::new(Key::PageUp), KeyPress::ctrl(Key::Char('u'))],
        "up one screen",
        Run::Here,
    ),
    Command::new(&[KeyPress::new(Key::Home)], "to the start", Run::Here),
    Command::new(&[KeyPress::new(Key::End)], "to the end", Run::Here),
];

// The actions are cut to twenty characters because MIN_W is derived from the
// longest one (design §5). The heading supplies the noun they drop: under
// `Tables`, "read" can only mean a table.
//
// The horizontal keys are not here: they are `Navigate`'s, and a panel group
// lists only what that panel alone answers.
const TABLES: &[Command] = &[Command::new(
    &[KeyPress::new(Key::Enter)],
    "fold/unfold, or read",
    Run::In(Panel::Tables),
)];

const CONTENTS: &[Command] = &[Command::new(
    &[KeyPress::new(Key::Char('m'))],
    "cycle selection mode",
    Run::In(Panel::Contents),
)];

/// The form's own keys. `Tab` and `Esc` also appear under `Global` meaning
/// something else, exactly as `l`/`h` appear under two panels: the palette may
/// not omit what a key does somewhere
/// (docs/superpowers/specs/2026-08-09-command-palette-design.md §2).
const EXPORT: &[Command] = &[
    Command::new(
        &[KeyPress::ctrl(Key::Char('e'))],
        "export these rows",
        Run::In(Panel::Contents),
    ),
    Command::new(&[KeyPress::new(Key::Tab)], "next field", Run::Inert),
    Command::new(&[KeyPress::new(Key::BackTab)], "previous field", Run::Inert),
    Command::new(
        &[KeyPress::new(Key::Char(' '))],
        "select, or export",
        Run::Inert,
    ),
    Command::new(&[KeyPress::new(Key::Escape)], "close the form", Run::Inert),
];

// The editor's printable keys are not rows: every one of them types itself,
// which is the panel's whole rule rather than a list of bindings
// (docs/shortcuts.md §2). What is listed is what a key does *not* do elsewhere.
const QUERIES: &[Command] = &[
    Command::new(
        &[KeyPress::ctrl(Key::Char('r'))],
        "run the buffer",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[KeyPress::ctrl(Key::Char('e'))],
        "open in $EDITOR",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[KeyPress::new(Key::Enter)],
        "split the line",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[KeyPress::new(Key::Backspace), KeyPress::new(Key::Delete)],
        "delete a character",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[KeyPress::new(Key::Home)],
        "line start",
        Run::In(Panel::Queries),
    ),
    Command::new(
        &[KeyPress::new(Key::End)],
        "line end",
        Run::In(Panel::Queries),
    ),
    // `Run::Here`, not `In(Panel::Queries)`: escape's whole point in the
    // editor is to leave it, so focusing Queries first would only be undone
    // by the key itself. It is `Navigate`'s reasoning applied to one more
    // key that every panel resolves in its own vocabulary — Global's own
    // `esc` row already says as much (design §5).
    Command::new(&[KeyPress::new(Key::Escape)], "leave the editor", Run::Here),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandsPanel {
    cursor: Cursor,
}

impl CommandsPanel {
    /// Opens on the first command, which is the row after the first heading.
    #[must_use]
    pub fn opened() -> Self {
        let mut panel = Self {
            cursor: Cursor::default(),
        };
        panel.to_start();
        panel
    }

    /// The selected row, as an index into [`rows`].
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.cursor.index()
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

    /// The widest row `MIN_W` in the renderer is derived from
    /// (docs/superpowers/specs/2026-08-09-command-palette-design.md §5): a
    /// thirteen-column key gutter, two columns of gap, and the longest action.
    /// No row fills the gutter today — `pgdn/ctrl-d` is the widest — and the
    /// renderer's `MIN_W` is derived from the constant rather than from the
    /// catalogue, so the two stay pinned to each other.
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

    #[test]
    fn the_palette_names_the_run_binding() {
        assert!(
            rows().contains(&CommandRow::Command {
                keys: "ctrl-r".to_owned(),
                action: "run the buffer",
            }),
            "the palette claims to be complete, and ctrl-r is the one binding no key types"
        );
    }

    #[test]
    fn the_catalogue_lists_visual_mode() {
        assert!(rows().contains(&CommandRow::Command {
            keys: "v".to_owned(),
            action: "select rows",
        }));
    }

    #[test]
    fn the_catalogue_lists_the_external_editor() {
        assert!(
            rows().contains(&CommandRow::Command {
                keys: "ctrl-e".to_owned(),
                action: "open in $EDITOR",
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
                .any(|command| command.binding.keys == [KeyPress::new(Key::Char('/'))]),
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
            commands().iter().any(|command| command
                .binding
                .keys
                .contains(&KeyPress::new(Key::Char('m')))),
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
                let horizontal = command
                    .binding
                    .keys
                    .contains(&KeyPress::new(Key::Char('h')))
                    || command
                        .binding
                        .keys
                        .contains(&KeyPress::new(Key::Char('l')));

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
                .filter(|command| command.binding.action != "leave the editor")
                .all(|command| command.run == Run::In(Panel::Queries))
        );
        assert_eq!(
            group("Queries")
                .commands
                .iter()
                .find(|command| command.binding.action == "leave the editor")
                .map(|command| command.run),
            Some(Run::Here),
            "escape's whole point in the editor is to leave it, so \
             focusing Queries first would only be undone by the key itself"
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
            let expected = if command.binding.keys == [KeyPress::ctrl(Key::Char('e'))] {
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
        let named = |action: &str, key: KeyPress| {
            assert!(
                commands()
                    .iter()
                    .any(|command| command.binding.action == action
                        && command.binding.keys.first() == Some(&key)),
                "no row presses {key:?} for {action:?}"
            );
        };

        named("next tab", KeyPress::new(Key::Char(']')));
        named("prev tab", KeyPress::new(Key::Char('[')));
        named("next field", KeyPress::new(Key::Tab));
        named("previous field", KeyPress::new(Key::BackTab));
        named("line start", KeyPress::new(Key::Home));
        named("line end", KeyPress::new(Key::End));

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
}
