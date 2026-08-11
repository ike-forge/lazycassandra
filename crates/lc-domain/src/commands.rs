//! Every binding the program has, and a cursor over them.
//!
//! The palette claims to be complete, which the hint bar deliberately does not
//! (docs/superpowers/specs/2026-08-09-command-palette-design.md §2). `update::tests`'
//! sweep is what holds that claim up: reusing [`Binding`] means a row names
//! `KeyPress` values rather than a string, so a row cannot describe a press no
//! arm of `handle_key` claims.

use crate::model::{Key, KeyPress};
use crate::state::Cursor;
use crate::view::Binding;
use crate::view::hints::label;

/// One labelled group of bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Group {
    pub heading: &'static str,
    pub bindings: &'static [Binding],
}

const GLOBAL: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Char('q'))],
        action: "quit",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('?'))],
        action: "commands",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('T'))],
        action: "choose a theme",
    },
    Binding {
        keys: &[KeyPress::new(Key::Escape)],
        action: "dismiss the overlay",
    },
    Binding {
        keys: &[KeyPress::new(Key::Tab)],
        action: "next panel",
    },
    Binding {
        keys: &[KeyPress::new(Key::BackTab)],
        action: "previous panel",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('t'))],
        action: "focus Tables",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('a'))],
        action: "focus Table DDL",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('c'))],
        action: "focus Table contents",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('u'))],
        action: "focus Queries",
    },
];

/// `/` joins the focused panel's own group, not `GLOBAL`: it does nothing in
/// Queries, where the editor claims it as text before this binding is ever
/// consulted (panel search design §5.3, §8). `GLOBAL` would assert it works
/// everywhere, which is false there.
const SEARCH: &[Binding] = &[Binding {
    keys: &[KeyPress::new(Key::Char('/'))],
    action: "search this panel",
}];

const MOTION: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Char('j')), KeyPress::new(Key::Down)],
        action: "down one row",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('k')), KeyPress::new(Key::Up)],
        action: "up one row",
    },
    Binding {
        keys: &[KeyPress::new(Key::PageDown)],
        action: "down one screen",
    },
    Binding {
        keys: &[KeyPress::new(Key::PageUp)],
        action: "up one screen",
    },
    Binding {
        keys: &[KeyPress::new(Key::Home)],
        action: "to the start",
    },
    Binding {
        keys: &[KeyPress::new(Key::End)],
        action: "to the end",
    },
];

// The actions are cut to twenty characters because MIN_W is derived from the
// longest one (design §5). The heading supplies the noun they drop: under
// `Tables`, "read" can only mean a table.
const TABLES: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Enter), KeyPress::new(Key::Char(' '))],
        action: "fold/unfold, or read",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('l')), KeyPress::new(Key::Right)],
        action: "unfold the keyspace",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('h')), KeyPress::new(Key::Left)],
        action: "fold, or step out",
    },
];

// `l`/`h` are listed twice, here and under `Tables`, and that is the point:
// the four horizontal keys mean move left / move right and each panel resolves
// them in its own vocabulary (docs/shortcuts.md §2). One row saying
// "unfold the keyspace" would omit what they do in a grid, and the palette may
// not omit a key that does something
// (docs/superpowers/specs/2026-08-09-command-palette-design.md §2).
const CONTENTS: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Char('l')), KeyPress::new(Key::Right)],
        action: "next column",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('h')), KeyPress::new(Key::Left)],
        action: "previous column",
    },
];

const DDL: &[Binding] = &[Binding {
    keys: &[
        KeyPress::ctrl(Key::Char('e')),
        KeyPress::ctrl(Key::Char('y')),
    ],
    action: "scroll one line",
}];

// The editor's printable keys are not rows: every one of them types itself,
// which is the panel's whole rule rather than a list of bindings
// (docs/shortcuts.md §2). What is listed is what a key does *not* do elsewhere.
const QUERIES: &[Binding] = &[
    Binding {
        keys: &[KeyPress::ctrl(Key::Char('r'))],
        action: "run the buffer",
    },
    Binding {
        keys: &[KeyPress::new(Key::Enter)],
        action: "split the line",
    },
    Binding {
        keys: &[KeyPress::new(Key::Backspace), KeyPress::new(Key::Delete)],
        action: "delete a character",
    },
    Binding {
        keys: &[KeyPress::new(Key::Home), KeyPress::new(Key::End)],
        action: "line start/end",
    },
    Binding {
        keys: &[KeyPress::new(Key::Escape)],
        action: "leave the editor",
    },
];

/// Every binding, grouped as the palette lists them.
pub const CATALOGUE: &[Group] = &[
    Group {
        heading: "Global",
        bindings: GLOBAL,
    },
    Group {
        heading: "Motion — focused panel",
        bindings: MOTION,
    },
    Group {
        heading: "Tables",
        bindings: TABLES,
    },
    Group {
        heading: "Search",
        bindings: SEARCH,
    },
    Group {
        heading: "Table DDL",
        bindings: DDL,
    },
    Group {
        heading: "Table contents",
        bindings: CONTENTS,
    },
    Group {
        heading: "Queries",
        bindings: QUERIES,
    },
];

/// One line of the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRow {
    Heading(&'static str),
    Command { keys: String, action: &'static str },
}

/// The catalogue as a flat list, headings included.
///
/// Headings are rows so that the cursor and the renderer count in the same
/// scheme; [`CommandsPanel`] is what keeps the cursor off them.
#[must_use]
pub fn rows() -> Vec<CommandRow> {
    CATALOGUE
        .iter()
        .flat_map(|group| {
            std::iter::once(CommandRow::Heading(group.heading)).chain(group.bindings.iter().map(
                |binding| CommandRow::Command {
                    keys: label(binding.keys),
                    action: binding.action,
                },
            ))
        })
        .collect()
}

/// How many rows [`rows`] produces, without building them.
#[must_use]
pub fn row_count() -> usize {
    CATALOGUE.iter().map(|group| 1 + group.bindings.len()).sum()
}

fn is_command(index: usize) -> bool {
    let mut at = 0;

    for group in CATALOGUE {
        if index == at {
            return false;
        }
        at += 1 + group.bindings.len();
        if index < at {
            return true;
        }
    }

    false
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

    /// The widest row `MIN_W` in the renderer is derived from
    /// (docs/superpowers/specs/2026-08-09-command-palette-design.md §5): a `ctrl-e/ctrl-y`
    /// gutter, two columns of gap, and the longest action.
    const WIDEST_KEYS: usize = 13;
    const WIDEST_ACTION: usize = 20;

    fn commands() -> Vec<&'static Binding> {
        CATALOGUE.iter().flat_map(|group| group.bindings).collect()
    }

    #[test]
    fn no_command_row_is_wider_than_the_narrowest_palette() {
        for binding in commands() {
            let keys = label(binding.keys);

            assert!(
                keys.chars().count() <= WIDEST_KEYS,
                "{keys:?} is wider than the gutter MIN_W is derived from"
            );
            assert!(
                binding.action.chars().count() <= WIDEST_ACTION,
                "{:?} is longer than the action column MIN_W is derived from",
                binding.action
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
                !group.bindings.is_empty(),
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

    #[test]
    fn the_catalogue_lists_search() {
        assert!(
            commands()
                .iter()
                .any(|binding| binding.keys.contains(&KeyPress::new(Key::Char('/')))),
            "no row binds /"
        );
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
}
