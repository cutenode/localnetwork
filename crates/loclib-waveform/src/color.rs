//! Colour arithmetic the widgets shade with, on 8-bit sRGB triples.

use std::sync::LazyLock;

use crate::Rgb;

/// Mix two colours: `t` = 0 gives `a`, 1 gives `b`.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// `c` taken `t` of the way to the grey of the same lightness: 0 gives `c`, 1 the grey.
pub fn mute(c: Rgb, t: f32) -> Rgb {
    let [r, g, b] = c.map(f32::from);
    let grey = (0.2126 * r + 0.7152 * g + 0.0722 * b).round() as u8;
    mix(c, [grey; 3], t)
}

/// Each channel times `f`, clamped: below 1 darkens, above 1 brightens.
pub fn scale(c: Rgb, f: f32) -> Rgb {
    c.map(|v| (v as f32 * f).clamp(0.0, 255.0) as u8)
}

/// `c` faded towards `background`: `f` = 1 gives `c`, 0 the background. On black it's [`scale`].
pub fn fade(background: Rgb, c: Rgb, f: f32) -> Rgb {
    [0, 1, 2].map(|i| (background[i] as f32 + (c[i] as f32 - background[i] as f32) * f) as u8)
}

/// Colour at `t` (0-1) along a multi-stop gradient (two stops or more).
pub fn gradient(stops: &[Rgb], t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (t.floor() as usize).min(stops.len() - 2);
    mix(stops[i], stops[i + 1], t - i as f32)
}

/// sRGB's channel values as linear light, worked out once: a frame asks for them for every cue.
static LINEAR: LazyLock<[f32; 256]> = LazyLock::new(|| {
    std::array::from_fn(|v| {
        let v = v as f32 / 255.0;
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    })
});

/// How bright a colour looks (WCAG relative luminance, 0 to 1).
pub fn luminance(c: Rgb) -> f32 {
    let [r, g, b] = c.map(|v| LINEAR[usize::from(v)]);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG contrast ratio of two colours, 1 to 21.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Whichever of two inks reads better on `paper`.
pub fn better_ink(a: Rgb, b: Rgb, paper: Rgb) -> Rgb {
    if contrast(a, paper) >= contrast(b, paper) { a } else { b }
}

#[cfg(test)]
mod tests;
