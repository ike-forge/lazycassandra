//! Nerd Font glyphs drawn in front of names, unconditionally: a terminal never
//! reports its font, and a missing glyph still advances one column, so nothing
//! can probe for it. Here rather than in the view model: a codepoint is not a
//! name.

use lc_domain::style::Token;
use lc_domain::view::{Group, RowKind};

/// The solid cut reads larger at one cell.
pub(crate) const KEYSPACE: &str = "\u{f084} ";
/// The outline cut: a filled table reads as noise at one cell.
pub(crate) const TABLE: &str = "\u{f0ce} ";
/// Not the keyspace's key: one glyph for two things would equate them.
pub(crate) const KEY: &str = "\u{f030b} ";
/// The outline cut: a filled cog is a blob at one cell.
pub(crate) const SETTINGS: &str = "\u{f013} ";
/// Not the key: an index is not part of the key, and the Indexes tab draws
/// both.
pub(crate) const INDEX: &str = "\u{f002} ";

pub(crate) const fn tree_row(kind: RowKind) -> &'static str {
    match kind {
        RowKind::Keyspace => KEYSPACE,
        RowKind::Table { .. } => TABLE,
    }
}

/// The icon keeps its kind's colour whatever the row wears.
pub(crate) const fn token(kind: RowKind) -> Token {
    match kind {
        RowKind::Keyspace => Token::Keyspace,
        RowKind::Table { .. } => Token::Table,
    }
}

pub(crate) fn identity(token: Token) -> &'static str {
    match token {
        Token::Keyspace => KEYSPACE,
        Token::Table => TABLE,
        _ => "",
    }
}

/// Decided by group: `Token::Section` also heads the Columns tab, which marks
/// its rows instead.
pub(crate) fn section(token: Token, group: Group) -> &'static str {
    match (token, group) {
        (Token::PartitionKey | Token::ClusteringKey, _) => KEY,
        (_, Group::Indexes) => INDEX,
        (_, Group::Settings) => SETTINGS,
        _ => "",
    }
}
