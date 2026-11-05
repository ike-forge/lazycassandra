//! Solarized dark and light: one set of accents over two mirrored ramps, as
//! upstream defines it. Upstream palette: <https://ethanschoonover.com/solarized/>.

use ratatui::style::Color;

use crate::theme::{Palette, rgb};

/// The ramp roles for one mode; the accents are shared.
struct Mode {
    base: Color,
    /// Upstream's background highlight.
    highlight: Color,
    /// Not upstream's: two fifths of the way from `base` to `highlight`.
    /// Catppuccin's fifth would vanish: upstream's highlight is one step off.
    crosshair: Color,
    /// Not upstream's: the midpoint of `crosshair` and `highlight`.
    ranged: Color,
    /// Upstream's body text.
    body: Color,
    between: Color,
    /// Upstream's secondary content and comments.
    secondary: Color,
}

#[expect(
    clippy::unreadable_literal,
    reason = "colour codes transcribed verbatim from upstream Solarized, not magnitudes"
)]
mod accent {
    use crate::theme::rgb;
    use ratatui::style::Color;

    pub(super) const YELLOW: Color = rgb(0xb58900);
    pub(super) const ORANGE: Color = rgb(0xcb4b16);
    pub(super) const RED: Color = rgb(0xdc322f);
    pub(super) const MAGENTA: Color = rgb(0xd33682);
    pub(super) const VIOLET: Color = rgb(0x6c71c4);
    pub(super) const BLUE: Color = rgb(0x268bd2);
    pub(super) const CYAN: Color = rgb(0x2aa198);
    pub(super) const GREEN: Color = rgb(0x859900);
}

impl Mode {
    /// Keywords green, literals cyan, identifiers blue: upstream's own syntax
    /// mapping.
    const fn palette(&self) -> Palette {
        Palette {
            base: self.base,
            text: self.body,
            dim: self.between,
            faint: self.secondary,
            border: self.secondary,
            focus: accent::BLUE,
            selection_bg: self.highlight,
            crosshair: self.crosshair,
            ranged: self.ranged,
            selection_fg: self.body,
            selection_bar: accent::YELLOW,
            shortcut: accent::YELLOW,
            keyspace: accent::VIOLET,
            table: accent::BLUE,
            partition: accent::YELLOW,
            clustering: accent::CYAN,
            column: self.body,
            label: self.between,
            section: accent::MAGENTA,
            accent: accent::ORANGE,
            match_: accent::GREEN,
            success: accent::GREEN,
            warning: accent::YELLOW,
            danger: accent::RED,
            info: accent::CYAN,
            keyword: accent::GREEN,
            reference: accent::BLUE,
            literal: accent::CYAN,
            comment: self.secondary,
        }
    }
}

// base03 / base02 / base0 / base00 / base01.
#[expect(
    clippy::unreadable_literal,
    reason = "colour codes transcribed verbatim from upstream Solarized, not magnitudes"
)]
const DARK_MODE: Mode = Mode {
    base: rgb(0x002b36),
    highlight: rgb(0x073642),
    crosshair: rgb(0x032f3b),
    ranged: rgb(0x05323e),
    body: rgb(0x839496),
    between: rgb(0x657b83),
    secondary: rgb(0x586e75),
};

// base3 / base2 / base00 / base0 / base1.
#[expect(
    clippy::unreadable_literal,
    reason = "colour codes transcribed verbatim from upstream Solarized, not magnitudes"
)]
const LIGHT_MODE: Mode = Mode {
    base: rgb(0xfdf6e3),
    highlight: rgb(0xeee8d5),
    crosshair: rgb(0xf7f0dd),
    ranged: rgb(0xf2ecd9),
    body: rgb(0x657b83),
    between: rgb(0x839496),
    secondary: rgb(0x93a1a1),
};

pub(crate) const DARK: Palette = DARK_MODE.palette();
pub(crate) const LIGHT: Palette = LIGHT_MODE.palette();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast::{luminance, ratio};

    /// Upstream's own floor is base1 on base3, about 2.5; this catches a
    /// mistyped hex, not taste.
    #[test]
    fn both_modes_read_against_their_own_base() {
        for (name, palette) in [("dark", DARK), ("light", LIGHT)] {
            for (role, color) in crate::theme::tests::foregrounds(&palette) {
                let ratio = ratio(color, palette.base);

                assert!(ratio >= 2.3, "{name}: {role} is {ratio:.2} on base");
            }
        }
    }

    #[test]
    fn the_modes_mirror_each_other_around_the_same_accents() {
        assert!(luminance(LIGHT.base) > 0.5 && luminance(DARK.base) < 0.05);
        assert_eq!(DARK.focus, LIGHT.focus);
        assert_eq!(DARK.danger, LIGHT.danger);
    }
}
