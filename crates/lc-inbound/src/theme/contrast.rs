//! WCAG relative luminance and contrast, for the palette tests.

use ratatui::style::Color;

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

/// CIE76: Euclidean distance in L*a*b*, D65. About 2.3 is barely noticeable.
pub(crate) fn delta_e(a: Color, b: Color) -> f64 {
    let ([l1, a1, b1], [l2, a2, b2]) = (lab(a), lab(b));

    ((l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt()
}

fn lab(color: Color) -> [f64; 3] {
    let Color::Rgb(red, green, blue) = color else {
        panic!("every palette role is a Color::Rgb, got {color:?}");
    };
    let (red, green, blue) = (channel(red), channel(green), channel(blue));
    let x = pivot((0.4124 * red + 0.3576 * green + 0.1805 * blue) / 0.950_47);
    let y = pivot(0.2126 * red + 0.7152 * green + 0.0722 * blue);
    let z = pivot((0.0193 * red + 0.1192 * green + 0.9505 * blue) / 1.088_83);

    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}

fn pivot(t: f64) -> f64 {
    if t > 0.008_856 {
        t.cbrt()
    } else {
        7.787 * t + 16.0 / 116.0
    }
}

/// `1.0` for identical colours, `21.0` for black on white.
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

    #[test]
    fn black_and_white_are_a_hundred_apart_in_lightness_alone() {
        let black = Color::Rgb(0x00, 0x00, 0x00);

        assert!((delta_e(black, WHITE) - 100.0).abs() < 0.1);
    }

    /// The 4.5:1 boundary grey pins the sRGB transfer function; a linear one
    /// would give about 3.
    #[test]
    fn the_transfer_function_is_srgb_rather_than_linear() {
        let ratio = ratio(Color::Rgb(0x76, 0x76, 0x76), WHITE);

        assert!((4.4..=4.7).contains(&ratio), "{ratio}");
    }
}
