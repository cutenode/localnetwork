use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::symbols::Marker;
use ratatui::widgets::Widget;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};

use crate::color::{gradient, mix};
use crate::{LOWER, Palette, Rgb};

/// How many spectrum bands fit in `width` columns, and how wide each bar and gap is.
pub fn bar_layout(width: u16, mirror: bool) -> (usize, u16, u16) {
    let (bar, gap) = if width >= 48 { (2, 1) } else { (1, 0) };
    let mut n = ((width + gap) / (bar + gap)) as usize;
    if mirror {
        n /= 2;
    }
    (n.max(1), bar, gap)
}

/// Vertical bars with peak caps, coloured along a gradient by height. `mirror` puts the bass in
/// the middle and spreads the highs out to both sides.
pub struct Spectrum<'a> {
    /// Each band's level, 0-1, bass first: [`bar_layout`] says how many fit.
    pub bands: &'a [f32],
    /// Each band's peak cap: its height, 0-1, then a value of the caller's own (how long it has
    /// been held, say) that isn't drawn.
    pub peaks: &'a [(f32, f32)],
    pub mirror: bool,
    /// A colour washed over the bars (a cue just passed) and how strongly, 0-1.
    pub tint: Option<(Rgb, f32)>,
    pub palette: Palette<'a>,
}

impl Widget for Spectrum<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.bands.is_empty() {
            return;
        }
        let (p, paint) = (&self.palette, self.palette.paint);
        let (_, bar, gap) = bar_layout(area.width, self.mirror);
        let n = self.bands.len();
        // Bars in display order (index into bands).
        let order: Vec<usize> = if self.mirror { (0..n).rev().chain(0..n).collect() } else { (0..n).collect() };
        let used = order.len() as u16 * (bar + gap) - gap;
        let left = area.x + area.width.saturating_sub(used) / 2;
        let h = area.height as f32;
        for (slot, &band) in order.iter().enumerate() {
            let x0 = left + slot as u16 * (bar + gap);
            if x0 + bar > area.right() {
                break;
            }
            let level = self.bands[band] * h;
            let peak = self.peaks.get(band).map_or(0.0, |p| p.0) * h;
            for row in 0..area.height {
                let y = area.bottom() - 1 - row;
                let fill = ((level - row as f32) * 8.0).round().clamp(0.0, 8.0) as usize;
                let mut color = gradient(p.spectrum, (row as f32 + 0.5) / h);
                if let Some((tint, t)) = self.tint {
                    color = mix(color, tint, t * 0.7);
                }
                let (symbol, fg) = if fill > 0 {
                    (LOWER[fill], paint(color))
                } else if peak > level + 0.25 && peak.floor() as u16 == row && peak > 0.05 {
                    (if peak.fract() < 0.5 { "▁" } else { "▔" }, paint(mix(color, [255, 255, 255], 0.35)))
                } else {
                    continue;
                };
                for dx in 0..bar {
                    p.put(buf, x0 + dx, y, symbol, fg, None);
                }
            }
        }
    }
}

/// Oscilloscope trace in braille, scaled to fill the height.
pub struct Scope<'a> {
    /// Samples, -1 to 1.
    pub samples: &'a [f32],
    /// A colour washed over the trace (a cue just passed) and how strongly, 0-1.
    pub tint: Option<(Rgb, f32)>,
    pub palette: Palette<'a>,
}

impl Widget for Scope<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.samples.len() < 2 {
            return;
        }
        let (p, paint) = (&self.palette, self.palette.paint);
        let points = (area.width as usize * 2).min(self.samples.len());
        let step = self.samples.len() as f64 / points as f64;
        // Auto-gain, so quiet passages stay readable (up to 6x; silence stays flat).
        let peak = self.samples.iter().fold(0.0f32, |m, s| m.max(s.abs())) as f64;
        let gain = if peak > 1e-4 { (0.92 / peak).clamp(1.0, 6.0) } else { 1.0 };
        let at = |i: usize| self.samples[((i as f64 * step) as usize).min(self.samples.len() - 1)] as f64 * gain;
        Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([0.0, points as f64])
            .y_bounds([-1.0, 1.0])
            .paint(|ctx| {
                ctx.draw(&CanvasLine::new(0.0, 0.0, points as f64, 0.0, paint(p.faint)));
                for i in 1..points {
                    let (a, b) = (at(i - 1).clamp(-1.0, 1.0), at(i).clamp(-1.0, 1.0));
                    let t = (a.abs().max(b.abs()) as f32).powf(0.7);
                    let mut color = gradient(p.scope, t);
                    if let Some((tint, k)) = self.tint {
                        color = mix(color, tint, k * 0.7);
                    }
                    ctx.draw(&CanvasLine::new((i - 1) as f64, a, i as f64, b, paint(color)));
                }
            })
            .render(area, buf);
    }
}
