//! Calm and low-chroma. A warm neutral ground with muted hues, for long
//! sessions; nothing shouts.

use crate::theme::{Palette, rgb};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes, not magnitudes: digit pairs would not clarify one"
)]
pub(crate) const DARK: Palette = Palette {
    base: rgb(0x1b1812),
    surface: rgb(0x28231b),
    text: rgb(0xece5da),
    dim: rgb(0xc5bbaa),
    faint: rgb(0xb0a593),
    border: rgb(0x564f44),
    focus: rgb(0x98c0eb),
    selection_bg: rgb(0x443a2a),
    crosshair: rgb(0x272219),
    ranged: rgb(0x352e21),
    selection_fg: rgb(0xece5da),
    selection_bar: rgb(0xd7b785),
    shortcut: rgb(0xd7b785),
    keyspace: rgb(0xc7b0e3),
    table: rgb(0x98c0eb),
    partition: rgb(0xd7b785),
    clustering: rgb(0x81ccc7),
    column: rgb(0xece5da),
    label: rgb(0xc5bbaa),
    section: rgb(0xdeaacb),
    accent: rgb(0xe6ad94),
    match_: rgb(0xaac795),
    success: rgb(0x9acaa3),
    warning: rgb(0xd0ba84),
    danger: rgb(0xe9aaa2),
    info: rgb(0x86c7e1),
    keyword: rgb(0xc7b0e3),
    reference: rgb(0x98c0eb),
    literal: rgb(0x9acaa3),
    comment: rgb(0xb0a593),
};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes, not magnitudes: digit pairs would not clarify one"
)]
pub(crate) const LIGHT: Palette = Palette {
    base: rgb(0xfdfaf4),
    surface: rgb(0xf6f1e9),
    text: rgb(0x302b22),
    dim: rgb(0x574e40),
    faint: rgb(0x726653),
    border: rgb(0xc2bbb0),
    focus: rgb(0x315f8d),
    selection_bg: rgb(0xf0e7d8),
    crosshair: rgb(0xf9f4ec),
    ranged: rgb(0xf4ede2),
    selection_fg: rgb(0x302b22),
    selection_bar: rgb(0x785514),
    shortcut: rgb(0x785514),
    keyspace: rgb(0x684e85),
    table: rgb(0x315f8d),
    partition: rgb(0x785514),
    clustering: rgb(0x006c67),
    column: rgb(0x302b22),
    label: rgb(0x574e40),
    section: rgb(0x7d476c),
    accent: rgb(0x854a2f),
    match_: rgb(0x49672e),
    success: rgb(0x326a40),
    warning: rgb(0x715911),
    danger: rgb(0x884740),
    info: rgb(0x006682),
    keyword: rgb(0x684e85),
    reference: rgb(0x315f8d),
    literal: rgb(0x326a40),
    comment: rgb(0x726653),
};
