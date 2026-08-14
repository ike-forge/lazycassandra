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
use lc_domain::view::{Group, RowKind};

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
/// `nf-fa-cog`, in front of the heading of a settings section — a table's
/// Options, a keyspace's Replication. The outline cut: a cog is all teeth, and
/// filling it at one cell leaves a blob.
pub(crate) const SETTINGS: &str = "\u{f013} ";

/// `nf-ple-trapezoid_top_bottom` and its mirror, at either end of the cursor
/// row's band.
///
/// Drawn in the band's own background colour against the panel's ground, which
/// is the only way a cell grid softens a corner: the glyph is the band's end,
/// not a border around it. A chamfer rather than
/// `nf-ple-left_half_circle_thick`'s semicircle, because a glyph fills its
/// cell — the semicircle's radius is half a cell and there is no smaller one
/// to ask for, so the only way down from a pill is to cut the corners
/// straight instead of curving them.
pub(crate) const CAP_LEFT: char = '\u{e0d2}';
pub(crate) const CAP_RIGHT: char = '\u{e0d4}';

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

/// The icon for a section heading: the key glyph over a key section, the cog
/// over a settings one. The group and not the token alone decides, because
/// `Token::Section` also wears the Columns tab's count heading, and that tab
/// wears its glyphs on the rows rather than over them (design §3).
pub(crate) fn section(token: Token, group: Group) -> &'static str {
    match (token, group) {
        (Token::PartitionKey | Token::ClusteringKey, _) => KEY,
        (_, Group::Settings) => SETTINGS,
        _ => "",
    }
}
