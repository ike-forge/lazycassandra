//! Dropping a keyspace or a table: what is aimed at, and the confirmation
//! that has to be answered before the statement runs
//! (docs/superpowers/specs/2026-08-31-drop-design.md).

use crate::contents::TableRef;
use crate::line::Line;
use crate::model::{KeyspaceKind, Statement};
use crate::posture::Posture;
use crate::style::{Segment, Token};

/// What a drop is aimed at.
///
/// The two nodes the tree draws, and nothing else: a drop is opened from a
/// cursor sitting on one of them, so a target that names neither cannot be
/// written down (design §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Keyspace(String),
    Table(TableRef),
}

impl Target {
    /// The statement answering the confirmation runs.
    #[must_use]
    pub fn statement(&self) -> Statement {
        match self {
            Self::Keyspace(keyspace) => Statement::drop_keyspace(keyspace),
            Self::Table(table) => Statement::drop_table(&table.keyspace, &table.table),
        }
    }

    /// What a typed confirmation has to match, and what the block asks for.
    ///
    /// Verbatim rather than folded: `system_schema` reports names in their
    /// true form, so typing what is on screen is the whole of the rule
    /// (design §2.1).
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Keyspace(keyspace) => keyspace,
            Self::Table(table) => &table.table,
        }
    }

    /// `keyspace` or `keyspace.table`, as the trail and a message name it.
    #[must_use]
    pub fn qualified(&self) -> String {
        match self {
            Self::Keyspace(keyspace) => keyspace.clone(),
            Self::Table(table) => table.qualified(),
        }
    }

    /// What the block's border says it is about, in the colours the tree
    /// gives the same names ([`TableRef::segments`]).
    #[must_use]
    pub fn title(&self) -> Vec<Segment> {
        match self {
            Self::Keyspace(keyspace) => {
                let token = match KeyspaceKind::of(keyspace) {
                    KeyspaceKind::System => Token::Faint,
                    KeyspaceKind::User => Token::Keyspace,
                };

                vec![
                    Segment::plain("Drop keyspace "),
                    Segment::styled(keyspace.clone(), token),
                ]
            }
            Self::Table(table) => {
                let mut segments = vec![Segment::plain("Drop table ")];
                segments.extend(table.segments());
                segments
            }
        }
    }
}

/// How the confirmation has to be answered before the statement runs.
///
/// An enum rather than an `Option<Line>` beside a flag: "typed, and also
/// answerable with `y`" is not a state, and the renderer reads which of the
/// two questions is being asked rather than inferring it (design §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// `y` runs it, `n` and `Esc` abandon it.
    YesNo,
    /// The target's name has to be typed back, then `⏎`.
    Typed(Line),
}

/// The open drop confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropPrompt {
    target: Target,
    answer: Answer,
}

impl DropPrompt {
    /// Opens over `target`, asking whichever question `posture` and the
    /// target between them call for (design §2).
    ///
    /// A keyspace is always typed because of what it takes with it, and a
    /// protected connection raises a table to the same bar — this is
    /// [`Posture::requires_confirmation`]'s first caller.
    #[must_use]
    pub fn opened(target: Target, posture: Posture) -> Self {
        let typed = matches!(target, Target::Keyspace(_)) || posture.requires_confirmation();
        let answer = if typed {
            Answer::Typed(Line::default())
        } else {
            Answer::YesNo
        };

        Self { target, answer }
    }

    #[must_use]
    pub const fn target(&self) -> &Target {
        &self.target
    }

    #[must_use]
    pub const fn answer(&self) -> &Answer {
        &self.answer
    }

    /// The line a typed confirmation is being typed into, if this is one.
    #[must_use]
    pub const fn typed(&self) -> Option<&Line> {
        match &self.answer {
            Answer::Typed(line) => Some(line),
            Answer::YesNo => None,
        }
    }

    pub const fn typed_mut(&mut self) -> Option<&mut Line> {
        match &mut self.answer {
            Answer::Typed(line) => Some(line),
            Answer::YesNo => None,
        }
    }

    /// Whether `⏎` would run it: whether what has been typed is the name
    /// that was asked for. Always `false` on a [`Answer::YesNo`]
    /// confirmation, which `⏎` does not answer at all.
    #[must_use]
    pub fn confirmed(&self) -> bool {
        self.typed()
            .is_some_and(|line| line.text() == self.target.name())
    }

    /// Whether this confirmation claims printable keys as text (design §2.1).
    #[must_use]
    pub const fn takes_text(&self) -> bool {
        matches!(self.answer, Answer::Typed(_))
    }

    #[must_use]
    pub fn statement(&self) -> Statement {
        self.target.statement()
    }

    #[must_use]
    pub fn title(&self) -> Vec<Segment> {
        self.target.title()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style;

    fn keyspace() -> Target {
        Target::Keyspace("shop".to_owned())
    }

    fn table() -> Target {
        Target::Table(TableRef::new("shop", "orders"))
    }

    fn typing(prompt: &mut DropPrompt, text: &str) {
        let line = prompt.typed_mut().expect("a typed confirmation");
        for character in text.chars() {
            line.insert(character);
        }
    }

    #[test]
    fn a_keyspace_is_always_typed_back() {
        for posture in [Posture::Writable, Posture::Protected] {
            assert!(DropPrompt::opened(keyspace(), posture).takes_text());
        }
    }

    #[test]
    fn a_table_is_yes_or_no_until_the_posture_is_protected() {
        assert_eq!(
            DropPrompt::opened(table(), Posture::Writable).answer(),
            &Answer::YesNo
        );
        assert!(DropPrompt::opened(table(), Posture::Protected).takes_text());
    }

    #[test]
    fn a_typed_confirmation_runs_only_on_the_name_it_asked_for() {
        let mut prompt = DropPrompt::opened(keyspace(), Posture::Writable);
        assert!(!prompt.confirmed(), "an empty field confirms nothing");

        typing(&mut prompt, "sho");
        assert!(!prompt.confirmed());

        typing(&mut prompt, "p");
        assert!(prompt.confirmed());
    }

    #[test]
    fn a_name_is_matched_verbatim_rather_than_folded() {
        // CQL folds an unquoted identifier and `system_schema` does not, so
        // the field asks for what the tree draws (design §2.1).
        let mut prompt = DropPrompt::opened(Target::Keyspace("MyKs".to_owned()), Posture::Writable);
        typing(&mut prompt, "myks");

        assert!(!prompt.confirmed());
    }

    #[test]
    fn a_yes_no_confirmation_is_never_confirmed_by_a_line_it_does_not_have() {
        assert!(!DropPrompt::opened(table(), Posture::Writable).confirmed());
    }

    #[test]
    fn a_table_drops_only_the_table_and_a_keyspace_the_whole_keyspace() {
        assert_eq!(table().statement().cql, "DROP TABLE \"shop\".\"orders\"");
        assert_eq!(keyspace().statement().cql, "DROP KEYSPACE \"shop\"");
    }

    #[test]
    fn the_title_names_the_node_the_tree_was_standing_on() {
        assert_eq!(style::text(&table().title()), "Drop table shop.orders");
        assert_eq!(style::text(&keyspace().title()), "Drop keyspace shop");
    }
}
