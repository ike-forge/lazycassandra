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
        keys: &[KeyPress::ctrl(Key::Char('t'))],
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
        keys: &[KeyPress::new(Key::Char('1'))],
        action: "focus Tables",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('2'))],
        action: "focus Table DDL",
    },
    // The one panel with a second tab names it, because the pairing is what
    // teaches the rule: two rows over the same key, and a reader who never
    // presses `]` still learns that `2` again reaches the Columns tab.
    Binding {
        keys: &[KeyPress::new(Key::Char('2'))],
        action: "focus Table columns",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('3'))],
        action: "focus Table contents",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('4'))],
        action: "focus Queries",
    },
    // The rule itself — a number pressed again steps one tab further in —
    // lives here rather than under all five numbers: only the DDL panel has a
    // tab to reach today, and only that press is spelled out above.
    Binding {
        keys: &[KeyPress::new(Key::Char(']')), KeyPress::new(Key::Char('['))],
        action: "next / prev tab",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('y'))],
        action: "yank",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('v'))],
        action: "select rows",
    },
];

/// `/` joins the focused panel's own group, not `GLOBAL`: it does nothing in
/// Queries, where the editor claims it as text before this binding is ever
/// consulted (panel search design §5.3, §8). `GLOBAL` would assert it works
/// everywhere, which is false there.
const SEARCH: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Char('/'))],
        action: "search this panel",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('n'))],
        action: "next match",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('N'))],
        action: "previous match",
    },
];

const MOTION: &[Binding] = &[
    Binding {
        keys: &[KeyPress::new(Key::Char('j')), KeyPress::new(Key::Down)],
        action: "down one row",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('k')), KeyPress::new(Key::Up)],
        action: "up one row",
    },
    // Worded for the movement rather than for what a panel makes of it: the
    // four horizontal keys mean move left / move right, and each panel
    // resolves them in its own vocabulary (docs/shortcuts.md §2). A row per
    // panel repeated the pair under every heading that answers it, which is
    // three rows saying "these keys go sideways" in three wordings.
    Binding {
        keys: &[KeyPress::new(Key::Char('h')), KeyPress::new(Key::Left)],
        action: "left, or fold",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char('l')), KeyPress::new(Key::Right)],
        action: "right, or unfold",
    },
    Binding {
        keys: &[KeyPress::new(Key::PageDown), KeyPress::ctrl(Key::Char('d'))],
        action: "down one screen",
    },
    Binding {
        keys: &[KeyPress::new(Key::PageUp), KeyPress::ctrl(Key::Char('u'))],
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
//
// The horizontal keys are not here: they are `Navigate`'s, and a panel group
// lists only what that panel alone answers.
const TABLES: &[Binding] = &[Binding {
    keys: &[KeyPress::new(Key::Enter)],
    action: "fold/unfold, or read",
}];

const CONTENTS: &[Binding] = &[Binding {
    keys: &[KeyPress::new(Key::Char('m'))],
    action: "cycle selection mode",
}];

/// The form's own keys. `Tab` and `Esc` also appear under `Global` meaning
/// something else, exactly as `l`/`h` appear under two panels: the palette may
/// not omit what a key does somewhere
/// (docs/superpowers/specs/2026-08-09-command-palette-design.md §2).
const EXPORT: &[Binding] = &[
    Binding {
        keys: &[KeyPress::ctrl(Key::Char('e'))],
        action: "export these rows",
    },
    Binding {
        keys: &[KeyPress::new(Key::Tab), KeyPress::new(Key::BackTab)],
        action: "next/previous field",
    },
    Binding {
        keys: &[KeyPress::new(Key::Char(' '))],
        action: "select, or export",
    },
    Binding {
        keys: &[KeyPress::new(Key::Escape)],
        action: "close the form",
    },
];

// The editor's printable keys are not rows: every one of them types itself,
// which is the panel's whole rule rather than a list of bindings
// (docs/shortcuts.md §2). What is listed is what a key does *not* do elsewhere.
const QUERIES: &[Binding] = &[
    Binding {
        keys: &[KeyPress::ctrl(Key::Char('r'))],
        action: "run the buffer",
    },
    Binding {
        keys: &[KeyPress::ctrl(Key::Char('e'))],
        action: "open in $EDITOR",
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
        heading: "Navigate",
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
        heading: "Table contents",
        bindings: CONTENTS,
    },
    Group {
        heading: "Export",
        bindings: EXPORT,
    },
    Group {
        heading: "Queries",
        bindings: QUERIES,
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
                .chain(group.bindings.iter().map(|binding| CommandRow::Command {
                    keys: label(binding.keys),
                    action: binding.action,
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
        .map(|(index, group)| usize::from(index > 0) + 1 + group.bindings.len())
        .sum()
}

fn is_command(index: usize) -> bool {
    let mut at = 0;

    for (group_index, group) in CATALOGUE.iter().enumerate() {
        // The separator, then the heading: neither is selectable, and both
        // sit below this group's first command.
        at += usize::from(group_index > 0);
        if index <= at {
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
    use crate::state::Panel;

    /// The widest row `MIN_W` in the renderer is derived from
    /// (docs/superpowers/specs/2026-08-09-command-palette-design.md §5): a
    /// thirteen-column key gutter, two columns of gap, and the longest action.
    /// No row fills the gutter today — `pgdn/ctrl-d` is the widest — and the
    /// renderer's `MIN_W` is derived from the constant rather than from the
    /// catalogue, so the two stay pinned to each other.
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
                }
            })
            .expect("no row focuses the DDL panel");

        assert_eq!(
            rows[ddl + 1],
            CommandRow::Command {
                keys: "2".to_owned(),
                action: "focus Table columns",
            }
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
    fn the_catalogue_lists_the_selection_mode_key() {
        assert!(
            commands()
                .iter()
                .any(|binding| binding.keys.contains(&KeyPress::new(Key::Char('m')))),
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
                            .any(|binding| binding.keys.contains(press)),
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
            for binding in group.bindings {
                let horizontal = binding.keys.contains(&KeyPress::new(Key::Char('h')))
                    || binding.keys.contains(&KeyPress::new(Key::Char('l')));

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
}
