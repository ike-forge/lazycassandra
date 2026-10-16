//! The four Catppuccin flavours: one recipe over four colour tables, so they
//! cannot drift apart. Upstream palette: <https://github.com/catppuccin/palette>.

use ratatui::style::Color;

use crate::theme::{Palette, rgb};

/// Only the upstream colours some role spends.
struct Flavour {
    base: Color,
    surface1: Color,
    /// Not upstream's: a fifth of the way from `base` to `surface1`, a wash
    /// upstream has no role for.
    crosshair: Color,
    /// Not upstream's: the midpoint of `crosshair` and `surface1`.
    ranged: Color,
    overlay1: Color,
    subtext1: Color,
    subtext0: Color,
    text: Color,
    yellow: Color,
    teal: Color,
    mauve: Color,
    blue: Color,
    sky: Color,
    pink: Color,
    peach: Color,
    flamingo: Color,
    green: Color,
    red: Color,
}

impl Flavour {
    /// `selection_fg` equals `text` and is never spent, so key colours survive
    /// on the cursor row.
    const fn palette(&self) -> Palette {
        Palette {
            base: self.base,
            text: self.text,
            dim: self.subtext1,
            faint: self.subtext0,
            border: self.surface1,
            focus: self.blue,
            selection_bg: self.surface1,
            crosshair: self.crosshair,
            ranged: self.ranged,
            selection_fg: self.text,
            selection_bar: self.yellow,
            shortcut: self.yellow,
            keyspace: self.mauve,
            table: self.blue,
            partition: self.yellow,
            clustering: self.teal,
            column: self.text,
            label: self.subtext1,
            section: self.pink,
            accent: self.peach,
            match_: self.flamingo,
            success: self.green,
            // Same yellow as `partition` and `shortcut`; the three never share
            // a surface.
            warning: self.yellow,
            danger: self.red,
            info: self.sky,
            keyword: self.mauve,
            reference: self.blue,
            literal: self.green,
            comment: self.overlay1,
        }
    }
}

// Kept in upstream's six-digit form, to diff against it.
#[expect(
    clippy::unreadable_literal,
    reason = "colour codes transcribed verbatim from upstream Catppuccin, not magnitudes"
)]
const LATTE_FLAVOUR: Flavour = Flavour {
    base: rgb(0xeff1f5),
    surface1: rgb(0xbcc0cc),
    crosshair: rgb(0xe5e7ed),
    ranged: rgb(0xd0d3dc),
    overlay1: rgb(0x8c8fa1),
    subtext1: rgb(0x5c5f77),
    subtext0: rgb(0x6c6f85),
    text: rgb(0x4c4f69),
    yellow: rgb(0xdf8e1d),
    teal: rgb(0x179299),
    mauve: rgb(0x8839ef),
    blue: rgb(0x1e66f5),
    sky: rgb(0x04a5e5),
    pink: rgb(0xea76cb),
    peach: rgb(0xfe640b),
    flamingo: rgb(0xdd7878),
    green: rgb(0x40a02b),
    red: rgb(0xd20f39),
};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes transcribed verbatim from upstream Catppuccin, not magnitudes"
)]
const FRAPPE_FLAVOUR: Flavour = Flavour {
    base: rgb(0x303446),
    surface1: rgb(0x51576d),
    crosshair: rgb(0x373b4e),
    ranged: rgb(0x44495d),
    overlay1: rgb(0x838ba7),
    subtext1: rgb(0xb5bfe2),
    subtext0: rgb(0xa5adce),
    text: rgb(0xc6d0f5),
    yellow: rgb(0xe5c890),
    teal: rgb(0x81c8be),
    mauve: rgb(0xca9ee6),
    blue: rgb(0x8caaee),
    sky: rgb(0x99d1db),
    pink: rgb(0xf4b8e4),
    peach: rgb(0xef9f76),
    flamingo: rgb(0xeebebe),
    green: rgb(0xa6d189),
    red: rgb(0xe78284),
};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes transcribed verbatim from upstream Catppuccin, not magnitudes"
)]
const MACCHIATO_FLAVOUR: Flavour = Flavour {
    base: rgb(0x24273a),
    surface1: rgb(0x494d64),
    crosshair: rgb(0x2b2f42),
    ranged: rgb(0x3a3e53),
    overlay1: rgb(0x8087a2),
    subtext1: rgb(0xb8c0e0),
    subtext0: rgb(0xa5adcb),
    text: rgb(0xcad3f5),
    yellow: rgb(0xeed49f),
    teal: rgb(0x8bd5ca),
    mauve: rgb(0xc6a0f6),
    blue: rgb(0x8aadf4),
    sky: rgb(0x91d7e3),
    pink: rgb(0xf5bde6),
    peach: rgb(0xf5a97f),
    flamingo: rgb(0xf0c6c6),
    green: rgb(0xa6da95),
    red: rgb(0xed8796),
};

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes transcribed verbatim from upstream Catppuccin, not magnitudes"
)]
const MOCHA_FLAVOUR: Flavour = Flavour {
    base: rgb(0x1e1e2e),
    surface1: rgb(0x45475a),
    crosshair: rgb(0x262637),
    ranged: rgb(0x353648),
    overlay1: rgb(0x7f849c),
    subtext1: rgb(0xbac2de),
    subtext0: rgb(0xa6adc8),
    text: rgb(0xcdd6f4),
    yellow: rgb(0xf9e2af),
    teal: rgb(0x94e2d5),
    mauve: rgb(0xcba6f7),
    blue: rgb(0x89b4fa),
    sky: rgb(0x89dceb),
    pink: rgb(0xf5c2e7),
    peach: rgb(0xfab387),
    flamingo: rgb(0xf2cdcd),
    green: rgb(0xa6e3a1),
    red: rgb(0xf38ba8),
};

pub(crate) const LATTE: Palette = LATTE_FLAVOUR.palette();
pub(crate) const FRAPPE: Palette = FRAPPE_FLAVOUR.palette();
pub(crate) const MACCHIATO: Palette = MACCHIATO_FLAVOUR.palette();
pub(crate) const MOCHA: Palette = MOCHA_FLAVOUR.palette();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast::{luminance, ratio};

    fn flavours() -> [(&'static str, Palette); 4] {
        [
            ("latte", LATTE),
            ("frappe", FRAPPE),
            ("macchiato", MACCHIATO),
            ("mocha", MOCHA),
        ]
    }

    /// 2.3 is upstream's own floor (Latte's accents run 2.31–2.96); it catches
    /// a mistyped hex. `comment` is meant to recede, so it has a lower floor.
    #[test]
    fn every_flavour_reads_against_its_own_base() {
        for (name, palette) in flavours() {
            let base = palette.base;

            for (role, color) in crate::theme::tests::foregrounds(&palette) {
                let floor = if role == "comment" { 2.0 } else { 2.3 };
                let ratio = ratio(color, base);

                assert!(ratio >= floor, "{name}: {role} is {ratio:.2} on base");
            }
        }
    }

    /// Not per role: upstream Latte's blue on surface1 is about 2.4, and this
    /// tests transcription, not taste.
    #[test]
    fn a_highlight_bar_is_distinct_from_its_field() {
        for (name, palette) in flavours() {
            let delta = (luminance(palette.selection_bg) - luminance(palette.base)).abs();

            assert!(delta >= 0.02, "{name}: the cursor row is invisible");
        }
    }

    #[test]
    fn latte_is_the_light_one_and_mocha_the_darkest() {
        let base = |palette: Palette| luminance(palette.base);

        assert!(base(LATTE) > 0.5);
        assert!(base(FRAPPE) > base(MACCHIATO));
        assert!(base(MACCHIATO) > base(MOCHA));
    }
}
