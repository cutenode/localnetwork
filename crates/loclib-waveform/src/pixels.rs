use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::color::{mix, scale};
use crate::{Palette, Rgb};

/// Half-block pixels, row-major, two rows per cell. `None` is transparent.
pub type PixelBuf = Vec<Option<Rgb>>;

/// An image in half blocks: two pixels per cell, top in the foreground, bottom in the background.
/// `None` pixels are transparent.
pub struct Pixels<'a> {
    pub width: u16,
    pub pixels: &'a [Option<Rgb>],
    pub palette: Palette,
}

impl Widget for Pixels<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (p, image) = (&self.palette, self.palette.image);
        let w = self.width as usize;
        // Pictures need colours; and they're drawn as they are, on a light terminal too.
        if w == 0 || p.plain {
            return;
        }
        // A cell's two rows of pixels at a time.
        for (cy, rows) in (0..area.height).zip(self.pixels.chunks_exact(2 * w)) {
            let (top, bottom) = rows.split_at(w);
            for (cx, (&top, &bottom)) in (0..area.width).zip(top.iter().zip(bottom)) {
                let (x, y) = (area.x + cx, area.y + cy);
                match (top, bottom) {
                    (Some(t), Some(b)) => p.put(buf, x, y, "▀", image(t), Some(image(b))),
                    (Some(t), None) => p.put(buf, x, y, "▀", image(t), None),
                    (None, Some(b)) => p.put(buf, x, y, "▄", image(b), None),
                    (None, None) => {}
                }
            }
        }
    }
}

/// Cover art, decoded once and resampled to whatever cell size it is drawn at.
#[cfg(feature = "image")]
pub struct Art {
    image: image::RgbImage,
    cache: Option<(u16, u16, PixelBuf)>,
}

#[cfg(feature = "image")]
impl Art {
    /// `None` when the bytes aren't an image in a format it reads.
    pub fn decode(bytes: &[u8]) -> Option<Art> {
        let image = image::load_from_memory(bytes).ok()?.to_rgb8();
        Some(Art { image, cache: None })
    }

    /// Pixels for a `cols` x `rows` cell area (two pixels per cell vertically).
    pub fn pixels(&mut self, cols: u16, rows: u16) -> &[Option<Rgb>] {
        if self.cache.as_ref().is_none_or(|(c, r, _)| (*c, *r) != (cols, rows)) {
            let resized = image::imageops::resize(
                &self.image,
                cols as u32,
                rows as u32 * 2,
                image::imageops::FilterType::Lanczos3,
            );
            let px = resized.pixels().map(|p| Some(p.0)).collect();
            self.cache = Some((cols, rows, px));
        }
        &self.cache.as_ref().unwrap().2
    }
}

/// A spinning record in `record`'s colour with a `label` in the middle, for tracks without
/// artwork. `angle` turns the light streak (radians).
pub fn vinyl(cols: u16, rows: u16, angle: f32, label: Rgb, record: Rgb) -> PixelBuf {
    let (w, h) = (cols as usize, rows as usize * 2);
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let radius = cx.min(cy);
    let mut out = vec![None; w * h];
    // `max(1)`: with no columns there are no pixels, and no rows to fill.
    for (y, row) in out.chunks_exact_mut(w.max(1)).enumerate() {
        for (x, px) in row.iter_mut().enumerate() {
            let (dx, dy) = ((x as f32 + 0.5 - cx) / radius, (y as f32 + 0.5 - cy) / radius);
            let r = (dx * dx + dy * dy).sqrt();
            *px = if !(0.06..=1.0).contains(&r) {
                None // outside, or the spindle hole
            } else if r < 0.36 {
                // Label, lit from the top left.
                Some(scale(label, 1.0 - 0.35 * ((dy - dx) * 0.5 + 0.5).clamp(0.0, 1.0)))
            } else if r > 0.95 {
                Some([74, 74, 90]) // rim
            } else {
                // Grooves, plus a highlight that sweeps round as the record turns.
                let groove = if ((r * 40.0) as i32) % 2 == 0 { 1.0 } else { 0.78 };
                let theta = dy.atan2(dx) - angle;
                let streak = theta.cos().abs().powi(10) * (1.1 - r) * 1.4;
                Some(mix(scale(record, groove), [170, 170, 196], streak))
            };
        }
    }
    out
}
