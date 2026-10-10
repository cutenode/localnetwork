//! Synthetic audio for tests, behind the `test-util` feature. Not part of the stable API: it
//! changes with the tests that use it.

use crate::fingerprint::RATE;

/// Deterministic noise-like test signal with structure: chirps and clicks, at [`RATE`].
pub fn synth(secs: f64, seed: u32) -> Vec<f32> {
    let n = (secs * RATE as f64) as usize;
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    let mut rand = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32
    };
    let mut out = vec![0.0f32; n];
    // A new note every 120 ms, two partials, decaying.
    let note = (0.12 * RATE as f64) as usize;
    for start in (0..n).step_by(note) {
        let f = 150.0 + rand() * 2500.0;
        let g = 0.2 + rand() * 0.3;
        for i in 0..(note * 3).min(n - start) {
            let t = i as f32 / RATE as f32;
            let env = (-t * 12.0).exp() * g;
            out[start + i] += env
                * ((2.0 * std::f32::consts::PI * f * t).sin() + 0.5 * (2.0 * std::f32::consts::PI * f * 1.5 * t).sin());
        }
    }
    out
}

/// A WAV file of `secs` seconds of a quiet tone.
pub fn wav(rate: u32, bits: u16, secs: f64) -> Vec<u8> {
    let frames = (rate as f64 * secs) as u32;
    wav_of(rate, bits, (0..frames).map(|i| ((i as f64 * 0.05).sin() * 1000.0) as i32))
}

/// A stereo WAV file holding `samples` in both channels.
pub fn wav_of(rate: u32, bits: u16, samples: impl ExactSizeIterator<Item = i32>) -> Vec<u8> {
    let bytes = bits as u32 / 8;
    let data_len = samples.len() as u32 * bytes * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * bytes * 2).to_le_bytes());
    out.extend_from_slice(&((bytes * 2) as u16).to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for v in samples {
        for _ in 0..2 {
            out.extend_from_slice(&v.to_le_bytes()[..bytes as usize]);
        }
    }
    out
}
