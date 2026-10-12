//! Rounding samples to a bit depth, with TPDF dither when the recipe asks for it: noise of ±1 LSB
//! with a triangular distribution, added before rounding, makes the rounding error a steady hiss
//! independent of the music instead of distortion that follows it.

use crate::Dither;

/// A fixed seed: the same source and recipe always make the same copy, byte for byte.
const SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// Rounds samples in [-1, 1) to integers of a bit depth.
pub(crate) struct Quantizer {
    /// Steps per unit: 2^(depth - 1).
    scale: f64,
    /// xorshift64* state, when dithering.
    rng: Option<u64>,
}

impl Quantizer {
    pub(crate) fn new(depth: u16, dither: Dither) -> Quantizer {
        let rng = match dither {
            Dither::None => None,
            Dither::Tpdf => Some(SEED),
        };
        Quantizer { scale: f64::from(1u32 << (depth - 1)), rng }
    }

    pub(crate) fn quantize(&mut self, sample: f64) -> i32 {
        let noise = match &mut self.rng {
            // Two uniform values of ±½ LSB sum to the triangle.
            Some(state) => uniform(state) + uniform(state),
            None => 0.0,
        };
        (sample * self.scale + noise).round().clamp(-self.scale, self.scale - 1.0) as i32
    }
}

/// Uniform in [-0.5, 0.5), from xorshift64*.
fn uniform(state: &mut u64) -> f64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    let bits = state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
    bits as f64 / (1u64 << 53) as f64 - 0.5
}

#[cfg(test)]
mod tests;
