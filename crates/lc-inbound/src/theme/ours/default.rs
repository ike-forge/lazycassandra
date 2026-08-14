//! The original `LazyCassandra` identity, corrected.
//!
//! Hue angles are unchanged from the palette this replaces — amber partition,
//! teal clustering, mauve keyspace, blue table. Only lightness and chroma
//! moved, and `base` is the same `#282a36` the old theme implicitly assumed.

use crate::theme::{Palette, rgb};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes, not magnitudes: digit pairs would not clarify one"
)]
pub(crate) const DARK: Palette = Palette {
    base: rgb(0x282a36),
    surface: rgb(0x30323f),
    text: rgb(0xe8eaf2),
    dim: rgb(0xbabdc9),
    faint: rgb(0xa3a7b5),
    border: rgb(0x4c5160),
    focus: rgb(0x9ec6ff),
    selection_bg: rgb(0x373c4e),
    crosshair: rgb(0x2c2f3d),
    selection_fg: rgb(0xe8eaf2),
    selection_bar: rgb(0xf1b877),
    shortcut: rgb(0xf1b877),
    keyspace: rgb(0xcfb5fe),
    table: rgb(0x9ec6ff),
    partition: rgb(0xf1b877),
    clustering: rgb(0x69dad0),
    column: rgb(0xe8eaf2),
    label: rgb(0xbabdc9),
    section: rgb(0xf2a9da),
    accent: rgb(0xfdae8b),
    match_: rgb(0xaed289),
    success: rgb(0x90d8a2),
    warning: rgb(0xe3bf72),
    danger: rgb(0xffaaa0),
    info: rgb(0x72d3f7),
    keyword: rgb(0xf2a9da),
    reference: rgb(0x9ec6ff),
    literal: rgb(0x90d8a2),
    comment: rgb(0xa3a7b5),
};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes, not magnitudes: digit pairs would not clarify one"
)]
pub(crate) const LIGHT: Palette = Palette {
    base: rgb(0xf8f8fb),
    surface: rgb(0xf0f0f5),
    text: rgb(0x23252c),
    dim: rgb(0x4c4f5b),
    faint: rgb(0x626776),
    border: rgb(0xb5baca),
    focus: rgb(0x2c59a0),
    selection_bg: rgb(0xe2e7f7),
    crosshair: rgb(0xf1f3fa),
    selection_fg: rgb(0x23252c),
    selection_bar: rgb(0x824d00),
    shortcut: rgb(0x824d00),
    keyspace: rgb(0x674795),
    table: rgb(0x2c59a0),
    partition: rgb(0x824d00),
    clustering: rgb(0x006a63),
    column: rgb(0x23252c),
    label: rgb(0x4c4f5b),
    section: rgb(0x863a71),
    accent: rgb(0x913f0f),
    match_: rgb(0x426803),
    success: rgb(0x006e34),
    warning: rgb(0x755500),
    danger: rgb(0x943932),
    info: rgb(0x006584),
    keyword: rgb(0x863a71),
    reference: rgb(0x2c59a0),
    literal: rgb(0x006e34),
    comment: rgb(0x626776),
};
