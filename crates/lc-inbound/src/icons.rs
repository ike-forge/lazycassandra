//! The glyphs the panels draw in front of a name.
//!
//! Nerd Font codepoints, taken from `ryanoasis/nerd-fonts`' `glyphnames.json`
//! and drawn unconditionally: a terminal never says what font it draws with,
//! and the glyphs sit in the Private Use Area, where a font that lacks one
//! still advances the cursor a column for its replacement box — so nothing a
//! probe could measure tells a drawn icon from a missing one
//! (docs/superpowers/specs/2026-08-12-nerd-font-icons-design.md §2).
//!
//! Here rather than in the view model: the domain names things, and a
//! codepoint chosen for a font is not a name (design §4).

use lc_domain::style::Token;
use lc_domain::view::RowKind;

/// `nf-md-key`, in front of a keyspace.
pub(crate) const KEYSPACE: &str = "\u{f0306} ";
/// `nf-fa-table`, in front of a table.
pub(crate) const TABLE: &str = "\u{f0ce} ";
/// `nf-oct-key`, in front of the heading of a key section.
pub(crate) const KEY: &str = "\u{f43d} ";

pub(crate) const fn tree_row(kind: RowKind) -> &'static str {
    match kind {
        RowKind::Keyspace => KEYSPACE,
        RowKind::Table => TABLE,
    }
}

/// The icon for a Ddl heading naming a table or a keyspace, and nothing for a
/// heading that names neither.
pub(crate) fn heading(token: Token) -> &'static str {
    match token {
        Token::Keyspace => KEYSPACE,
        Token::Table => TABLE,
        _ => "",
    }
}

/// The icon for a section heading, which only the two key sections get: the
/// Options heading names a list of settings, and the Table columns tab already
/// marks each row with the part of the key it plays (design §3).
pub(crate) fn section(token: Option<Token>) -> &'static str {
    match token {
        Some(Token::PartitionKey | Token::ClusteringKey) => KEY,
        _ => "",
    }
}
