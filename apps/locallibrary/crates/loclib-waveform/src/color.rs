//! Colour arithmetic the widgets shade with, on 8-bit sRGB triples.

use crate::Rgb;

/// Mix two colours: `t` = 0 gives `a`, 1 gives `b`.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// Each channel times `f`, clamped: below 1 darkens, above 1 brightens.
pub fn scale(c: Rgb, f: f32) -> Rgb {
    c.map(|v| (v as f32 * f).clamp(0.0, 255.0) as u8)
}

/// Colour at `t` (0-1) along a multi-stop gradient (two stops or more).
pub fn gradient(stops: &[Rgb], t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (t.floor() as usize).min(stops.len() - 2);
    mix(stops[i], stops[i + 1], t - i as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradients() {
        let stops = crate::Palette::DARK.spectrum;
        assert_eq!(gradient(stops, 0.0), stops[0]);
        assert_eq!(gradient(stops, 1.0), [255, 196, 86]);
        assert_eq!(mix([0, 0, 0], [200, 100, 50], 0.5), [100, 50, 25]);
    }
}
