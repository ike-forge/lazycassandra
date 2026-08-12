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

/// `nf-fa-key`, in front of a keyspace. The solid cut rather than
/// `nf-md-key`'s: a filled shape reads as the larger glyph at a size no
/// terminal lets us change (design §7).
pub(crate) const KEYSPACE: &str = "\u{f084} ";
/// `nf-fa-table`, in front of a table. The filled `nf-md-table_large` was
/// tried and reads as noise at one cell: a glyph this small carries by its
/// outline, not by how much ink it spends.
pub(crate) const TABLE: &str = "\u{f0ce} ";
/// `nf-md-key_variant`, in front of the heading of a key section. Not the
/// keyspace's key: two different things wearing one glyph would say they are
/// the same thing.
pub(crate) const KEY: &str = "\u{f030b} ";

pub(crate) const fn tree_row(kind: RowKind) -> &'static str {
    match kind {
        RowKind::Keyspace => KEYSPACE,
        RowKind::Table { .. } => TABLE,
    }
}

/// The colour an icon carries: the one its own kind is drawn in, whatever the
/// row around it is drawn in.
pub(crate) const fn token(kind: RowKind) -> Token {
    match kind {
        RowKind::Keyspace => Token::Keyspace,
        RowKind::Table { .. } => Token::Table,
    }
}

/// The icon a Ddl identity row wears, and nothing for a token that names
/// neither a keyspace nor a table.
pub(crate) fn identity(token: Token) -> &'static str {
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
