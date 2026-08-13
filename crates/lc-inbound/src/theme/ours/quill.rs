//! Two hues only — warm amber and cool blue — over a neutral ramp. Green and
//! red are reserved strictly for success and danger, so a colour that is not
//! one of those two never means a verdict. Structure is carried by weight and
//! glyph rather than by hue, which is why `clustering` is warm here and cool
//! everywhere else (design §3.1, rule 4).

use crate::theme::{Palette, rgb};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes, not magnitudes: digit pairs would not clarify one"
)]
pub(crate) const DARK: Palette = Palette {
    base: rgb(0x15191e),
    surface: rgb(0x1f252c),
    text: rgb(0xe1e7ee),
    dim: rgb(0xb5bdc8),
    faint: rgb(0xa0a8b4),
    border: rgb(0x49515b),
    border_focus: rgb(0xfcbf76),
    focus: rgb(0xfcbf76),
    selection_bg: rgb(0x313d4c),
    selection_fg: rgb(0xe1e7ee),
    selection_bar: rgb(0xfcbf76),
    shortcut: rgb(0xfcbf76),
    keyspace: rgb(0xe1e7ee),
    table: rgb(0x84cfec),
    partition: rgb(0xfcbf76),
    clustering: rgb(0xc2a585),
    column: rgb(0xe1e7ee),
    label: rgb(0xb5bdc8),
    section: rgb(0xb5bdc8),
    accent: rgb(0xfcbf76),
    match_: rgb(0xc2a585),
    success: rgb(0x7fd091),
    warning: rgb(0xfcbf76),
    danger: rgb(0xff877b),
    info: rgb(0x91b3c1),
    keyword: rgb(0x91b3c1),
    reference: rgb(0x84cfec),
    literal: rgb(0xe1e7ee),
    comment: rgb(0xa0a8b4),
};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes, not magnitudes: digit pairs would not clarify one"
)]
pub(crate) const LIGHT: Palette = Palette {
    base: rgb(0xf7faff),
    surface: rgb(0xedf2fa),
    text: rgb(0x272c31),
    dim: rgb(0x49505a),
    faint: rgb(0x5f6875),
    border: rgb(0xb5bdc8),
    border_focus: rgb(0x8c5500),
    focus: rgb(0x8c5500),
    selection_bg: rgb(0xdee9f7),
    selection_fg: rgb(0x272c31),
    selection_bar: rgb(0x8c5500),
    shortcut: rgb(0x8c5500),
    keyspace: rgb(0x272c31),
    table: rgb(0x006888),
    partition: rgb(0x8c5500),
    clustering: rgb(0x81613a),
    column: rgb(0x272c31),
    label: rgb(0x49505a),
    section: rgb(0x49505a),
    accent: rgb(0x8c5500),
    match_: rgb(0x81613a),
    success: rgb(0x00722e),
    warning: rgb(0x8c5500),
    danger: rgb(0xb7191c),
    info: rgb(0x3c6a7c),
    keyword: rgb(0x3c6a7c),
    reference: rgb(0x006888),
    literal: rgb(0x272c31),
    comment: rgb(0x5f6875),
};
