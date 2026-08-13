//! WCAG relative luminance and contrast, for the palette tests.
//!
//! Test-only: a palette is a table of constants, and the thing worth asserting
//! about a table of constants is that someone can read it.

use ratatui::style::Color;

/// WCAG 2.x relative luminance.
pub(crate) fn luminance(color: Color) -> f64 {
    let Color::Rgb(red, green, blue) = color else {
        panic!("every palette role is a Color::Rgb, got {color:?}");
    };

    0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
}

fn channel(value: u8) -> f64 {
    let value = f64::from(value) / 255.0;

    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG 2.x contrast ratio, `1.0` for identical colours and `21.0` for black
/// against white.
pub(crate) fn ratio(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    let (lighter, darker) = if a > b { (a, b) } else { (b, a) };

    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Color = Color::Rgb(0xff, 0xff, 0xff);

    #[test]
    fn black_against_white_is_the_maximum_ratio() {
        let black = Color::Rgb(0x00, 0x00, 0x00);

        assert!((ratio(black, WHITE) - 21.0).abs() < 0.01);
    }

    #[test]
    fn a_colour_against_itself_is_the_minimum_ratio() {
        assert!((ratio(WHITE, WHITE) - 1.0).abs() < 0.001);
    }

    #[test]
    fn the_ratio_does_not_depend_on_which_way_round_it_is_asked() {
        let grey = Color::Rgb(0x7a, 0x7f, 0x8f);

        assert!((ratio(grey, WHITE) - ratio(WHITE, grey)).abs() < 0.001);
    }

    /// The mid-grey web accessibility guidance names as the 4.5:1 boundary
    /// against white. Pins the transfer function rather than the arithmetic
    /// around it — a linear channel would put this near 3.
    #[test]
    fn the_transfer_function_is_srgb_rather_than_linear() {
        let ratio = ratio(Color::Rgb(0x76, 0x76, 0x76), WHITE);

        assert!((4.4..=4.7).contains(&ratio), "{ratio}");
    }
}
