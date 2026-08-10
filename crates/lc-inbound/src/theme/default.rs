//! The theme that wears the terminal's clothes.
//!
//! `base`, `surface`, `text` and `border` are `None`: the background and the
//! body foreground are the user's, and this theme paints over neither
//! (vision.md §4, principle 5).
//!
//! Every colour below is drawn on top of a background this theme cannot see,
//! so each holds a WCAG contrast ratio of at least 3.0 against black *and*
//! against white. That is a narrow band — a relative luminance of roughly
//! 0.10 to 0.30 — and it is why these are deeper than a palette designed for
//! one background would be (design §4).

use ratatui::style::Color;

use super::Palette;

const AMBER: Color = Color::Rgb(0xc9, 0x82, 0x1f);
const TEAL: Color = Color::Rgb(0x10, 0x84, 0x7c);
const MAUVE: Color = Color::Rgb(0x8a, 0x4f, 0xd4);
const BLUE: Color = Color::Rgb(0x2b, 0x6f, 0xd8);
const MAGENTA: Color = Color::Rgb(0xb8, 0x48, 0x9a);
const RUST: Color = Color::Rgb(0xd1, 0x66, 0x2f);
const OLIVE: Color = Color::Rgb(0x5f, 0x8f, 0x16);
const GREY: Color = Color::Rgb(0x7a, 0x7f, 0x8f);
const GREEN: Color = Color::Rgb(0x2e, 0x8b, 0x4f);
const OCHRE: Color = Color::Rgb(0xa8, 0x78, 0x1a);
const RED: Color = Color::Rgb(0xc8, 0x32, 0x2f);

pub(crate) const DEFAULT: Palette = Palette {
    base: None,
    surface: None,
    text: None,
    border: None,
    highlight: Color::Rgb(0x45, 0x48, 0x5a),
    highlight_text: Some(Color::Rgb(0xe8, 0xea, 0xf2)),
    muted: GREY,
    partition: AMBER,
    clustering: TEAL,
    keyspace: MAUVE,
    table: BLUE,
    property: MAGENTA,
    accent: RUST,
    match_: OLIVE,
    success: GREEN,
    warning: OCHRE,
    danger: RED,
    keyword: MAUVE,
    reference: BLUE,
    literal: GREEN,
};
