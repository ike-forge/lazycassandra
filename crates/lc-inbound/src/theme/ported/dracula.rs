//! Dracula. Upstream defines one foreground and one comment colour, so `dim`
//! is derived between them. Upstream palette: <https://github.com/dracula/dracula-theme>.

use crate::theme::{Palette, rgb};

/// Keywords pink, strings yellow, types cyan: upstream's own syntax mapping.
#[expect(
    clippy::unreadable_literal,
    reason = "colour codes transcribed verbatim from upstream Dracula, not magnitudes"
)]
pub(crate) const DRACULA: Palette = Palette {
    base: rgb(0x282a36),
    text: rgb(0xf8f8f2),
    // Not upstream's: the midpoint of foreground and comment.
    dim: rgb(0xadb5cb),
    faint: rgb(0x6272a4),
    border: rgb(0x6272a4),
    focus: rgb(0xbd93f9),
    // Upstream's selection and current line, one colour.
    selection_bg: rgb(0x44475a),
    // Not upstream's: a fifth of the way from background to selection.
    crosshair: rgb(0x2e303d),
    // Not upstream's: the midpoint of crosshair and selection.
    ranged: rgb(0x393c4c),
    selection_fg: rgb(0xf8f8f2),
    selection_bar: rgb(0xf1fa8c),
    shortcut: rgb(0xf1fa8c),
    keyspace: rgb(0xff79c6),
    table: rgb(0x8be9fd),
    partition: rgb(0xffb86c),
    clustering: rgb(0x50fa7b),
    column: rgb(0xf8f8f2),
    label: rgb(0xadb5cb),
    section: rgb(0xff79c6),
    accent: rgb(0xffb86c),
    match_: rgb(0xf1fa8c),
    success: rgb(0x50fa7b),
    warning: rgb(0xffb86c),
    danger: rgb(0xff5555),
    info: rgb(0x8be9fd),
    keyword: rgb(0xff79c6),
    reference: rgb(0x8be9fd),
    literal: rgb(0xf1fa8c),
    comment: rgb(0x6272a4),
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast::ratio;

    /// Upstream's comment is its lowest, about 3.0; this catches a mistyped
    /// hex, not taste.
    #[test]
    fn every_role_reads_against_the_background() {
        for (role, color) in crate::theme::tests::foregrounds(&DRACULA) {
            let ratio = ratio(color, DRACULA.base);

            assert!(ratio >= 2.3, "{role} is {ratio:.2} on base");
        }
    }
}
