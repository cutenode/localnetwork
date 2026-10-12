//! Streaming sample-rate conversion through rubato's sinc resampler. rubato's output lags its
//! input by the filter's delay and stops short of the input's end; rekordbox keeps cues and beat
//! grids in milliseconds from the start, so the delay is dropped from the front and the tail
//! flushed, keeping sample 0 at time 0 and the length the source's at the new rate.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Indexing, Resampler as _, SincInterpolationParameters};

/// Input frames per call.
const CHUNK: usize = 1024;

pub(crate) struct Resampler {
    inner: Async<f64>,
    channels: usize,
    from: u32,
    to: u32,
    /// Interleaved input not yet resampled, under a chunk.
    pending: Vec<f64>,
    out: Vec<f64>,
    /// Output frames still to drop: the delay.
    skip: usize,
    frames_in: u64,
    frames_out: u64,
}

impl Resampler {
    pub(crate) fn new(from: u32, to: u32, channels: usize) -> Resampler {
        let ratio = f64::from(to) / f64::from(from);
        let params = SincInterpolationParameters::default();
        let inner = Async::new_sinc(ratio, 1.0, &params, CHUNK, channels, FixedAsync::Input)
            .expect("decode_native hands on no 0 Hz or channelless audio, and recipes have a rate");
        // rubato's output lags its input by half its filter at the new rate, less one frame: its
        // first output is a step past where it starts, which `output_delay` leaves out
        // (`clicks_stay_in_time` holds us to this). With the rates in the ratio q:p that's
        // half * p / q - 1 frames, a fraction unless q divides `half`, so `lead` frames of
        // silence go in first to make it whole, and dropping `skip` frames puts the source's
        // sample 0 on the copy's.
        let half = params.sinc_len / 2;
        let g = gcd(from, to);
        let (p, q) = ((to / g) as usize, (from / g) as usize);
        let lead = (q - half % q) % q;
        let skip = (half + lead) / q * p - 1;
        let out = vec![0.0; inner.output_frames_max() * channels];
        Resampler {
            inner,
            channels,
            from,
            to,
            pending: vec![0.0; lead * channels],
            out,
            skip,
            frames_in: 0,
            frames_out: 0,
        }
    }

    /// Resample interleaved `input`, handing what's ready to `sink`.
    pub(crate) fn push<E>(&mut self, input: &[f64], sink: &mut impl FnMut(&[f64]) -> Result<(), E>) -> Result<(), E> {
        self.frames_in += (input.len() / self.channels) as u64;
        self.pending.extend_from_slice(input);
        let mut offset = 0;
        while self.pending.len() / self.channels - offset >= CHUNK {
            self.process(offset, None, u64::MAX, sink)?;
            offset += CHUNK;
        }
        self.pending.drain(..offset * self.channels);
        Ok(())
    }

    /// Flush the rest: the input's last partial chunk, then silence until the output is as long
    /// as the input, to the nearest frame at the new rate.
    pub(crate) fn finish<E>(mut self, sink: &mut impl FnMut(&[f64]) -> Result<(), E>) -> Result<(), E> {
        let expected = (self.frames_in * u64::from(self.to) + u64::from(self.from) / 2) / u64::from(self.from);
        let mut left = self.pending.len() / self.channels;
        while self.frames_out < expected {
            self.process(0, Some(left), expected, sink)?;
            left = 0;
        }
        Ok(())
    }

    /// Resample a chunk of `pending` from frame `offset` (`partial`: only so many frames of it
    /// are input, the rest silence), handing on output beyond the delay up to `limit` frames.
    fn process<E>(
        &mut self,
        offset: usize,
        partial: Option<usize>,
        limit: u64,
        sink: &mut impl FnMut(&[f64]) -> Result<(), E>,
    ) -> Result<(), E> {
        let ch = self.channels;
        let input = InterleavedSlice::new(&self.pending, ch, self.pending.len() / ch).expect("whole frames");
        let frames = self.out.len() / ch;
        let mut output = InterleavedSlice::new_mut(&mut self.out, ch, frames).expect("whole frames");
        let indexing = Indexing { input_offset: offset, partial_len: partial, ..Indexing::default() };
        let (_, produced) =
            self.inner.process_into_buffer(&input, &mut output, Some(&indexing)).expect("buffers are big enough");
        let skipped = self.skip.min(produced);
        self.skip -= skipped;
        let kept = ((produced - skipped) as u64).min(limit - self.frames_out) as usize;
        self.frames_out += kept as u64;
        sink(&self.out[skipped * ch..(skipped + kept) * ch])
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[cfg(test)]
mod tests;
