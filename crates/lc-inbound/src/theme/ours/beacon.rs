//! Vivid, high-contrast and the loudest. `focus` and `shortcut` share an amber,
//! hence underlined hot keys.

use crate::theme::{Palette, rgb};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes, not magnitudes: digit pairs would not clarify one"
)]
pub(crate) const DARK: Palette = Palette {
    base: rgb(0x0e131d),
    text: rgb(0xebeef5),
    dim: rgb(0xb6bcc8),
    faint: rgb(0xa2a7b5),
    border: rgb(0x4a505b),
    focus: rgb(0xfebb45),
    selection_bg: rgb(0x343c4c),
    crosshair: rgb(0x191f2b),
    ranged: rgb(0x262d3b),
    selection_fg: rgb(0xebeef5),
    selection_bar: rgb(0xfebb45),
    shortcut: rgb(0xfebb45),
    keyspace: rgb(0xdab6ff),
    table: rgb(0x9aceff),
    partition: rgb(0xfebb45),
    clustering: rgb(0x00e7de),
    column: rgb(0xebeef5),
    label: rgb(0xb6bcc8),
    section: rgb(0xffa3e6),
    accent: rgb(0xffb28d),
    match_: rgb(0xa4dd72),
    success: rgb(0x7ae494),
    warning: rgb(0xf1c240),
    danger: rgb(0xffb0a6),
    info: rgb(0x57daff),
    keyword: rgb(0xdab6ff),
    reference: rgb(0x9aceff),
    literal: rgb(0x7ae494),
    comment: rgb(0xa2a7b5),
};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes, not magnitudes: digit pairs would not clarify one"
)]
pub(crate) const LIGHT: Palette = Palette {
    base: rgb(0xffffff),
    text: rgb(0x1c1f25),
    dim: rgb(0x4b505a),
    faint: rgb(0x616876),
    border: rgb(0xb6bcc8),
    focus: rgb(0x734d00),
    selection_bg: rgb(0xe1e8f7),
    crosshair: rgb(0xf6f8fd),
    ranged: rgb(0xebf0fa),
    selection_fg: rgb(0x1c1f25),
    selection_bar: rgb(0x734d00),
    shortcut: rgb(0x734d00),
    keyspace: rgb(0x6c329f),
    table: rgb(0x0056a4),
    partition: rgb(0x734d00),
    clustering: rgb(0x006460),
    column: rgb(0x1c1f25),
    label: rgb(0x4b505a),
    section: rgb(0x8d1e72),
    accent: rgb(0x952d00),
    match_: rgb(0x376300),
    success: rgb(0x006927),
    warning: rgb(0x6c5100),
    danger: rgb(0x9e1618),
    info: rgb(0x005f7c),
    keyword: rgb(0x6c329f),
    reference: rgb(0x0056a4),
    literal: rgb(0x006927),
    comment: rgb(0x616876),
};
