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
mod tests {
    use super::*;

    /// Feeds `input` in uneven pieces, as decoded packets come.
    fn resample(input: &[f64], from: u32, to: u32, channels: usize) -> Vec<f64> {
        let mut r = Resampler::new(from, to, channels);
        let mut out = Vec::new();
        let mut sink = |s: &[f64]| -> Result<(), ()> {
            out.extend_from_slice(s);
            Ok(())
        };
        for piece in input.chunks(577 * channels) {
            r.push(piece, &mut sink).unwrap();
        }
        r.finish(&mut sink).unwrap();
        out
    }

    /// A click comes out at the same time it went in, to the nearest frame: on the frame itself
    /// when one rate is a multiple of the other.
    #[test]
    fn clicks_stay_in_time() {
        for (from, to) in [(96_000, 48_000), (88_200, 44_100), (44_100, 88_200), (44_100, 48_000), (48_000, 44_100)] {
            let mut click = vec![0.0; from as usize / 5];
            click[from as usize / 10] = 1.0;
            let out = resample(&click, from, to, 1);
            let peak = (0..out.len()).max_by(|a, b| out[*a].total_cmp(&out[*b])).unwrap();
            assert_eq!(peak, to as usize / 10, "{from} -> {to}");
            // rubato's kernel sits 1/128 of an input sample early (its table step), which tips
            // these by about 0.02; a frame or half a frame out would by far more.
            if from % to == 0 || to % from == 0 {
                let (before, after) = (out[peak - 1], out[peak + 1]);
                assert!((before - after).abs() < 0.05, "{from} -> {to}: {before} and {after} around the click");
            }
        }
    }

    /// Lengths follow the input to the nearest frame, and a short clip (under one chunk) or none
    /// at all comes out right too.
    #[test]
    fn length_follows_the_input() {
        for (frames, from, to) in
            [(96_000, 96_000, 48_000), (44_101, 44_100, 48_000), (300, 88_200, 44_100), (0, 48_000, 44_100)]
        {
            let out = resample(&vec![0.1; frames * 2], from, to, 2);
            let expected = (frames as u64 * u64::from(to) + u64::from(from) / 2) / u64::from(from);
            assert_eq!(out.len() as u64, expected * 2, "{frames} frames {from} -> {to}");
        }
    }

    use hegel::TestCase;
    use hegel::generators as gs;

    /// Between any two of the rates players take, in any number of channels, however the input
    /// comes: the output is the input's length at the new rate to the nearest frame, the same
    /// whatever the pieces, and a click comes out within a frame of when it went in.
    #[hegel::test(test_cases = 5)]
    fn clicks_keep_their_time_between_any_rates(tc: TestCase) {
        let rates = vec![32_000, 44_100, 48_000, 88_200, 96_000];
        let (from, to) = (tc.draw(gs::sampled_from(rates.clone())), tc.draw(gs::sampled_from(rates)));
        let channels = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
        let frames = tc.draw(gs::integers::<usize>().min_value(800).max_value(2500));
        let at = tc.draw(gs::integers::<usize>().min_value(frames / 4).max_value(3 * frames / 4));
        let mut input = vec![0.0; frames * channels];
        input[at * channels..(at + 1) * channels].fill(0.9);
        let piece = tc.draw(gs::integers::<usize>().min_value(1).max_value(3000));
        let mut r = Resampler::new(from, to, channels);
        let mut out = Vec::new();
        let mut sink = |s: &[f64]| -> Result<(), ()> {
            out.extend_from_slice(s);
            Ok(())
        };
        for p in input.chunks(piece * channels) {
            r.push(p, &mut sink).unwrap();
        }
        r.finish(&mut sink).unwrap();
        let expected = (frames as u64 * u64::from(to) + u64::from(from) / 2) / u64::from(from);
        assert_eq!(out.len() as u64, expected * channels as u64);
        assert_eq!(out, resample(&input, from, to, channels), "pieces of {piece} change it");
        for c in 0..channels {
            let channel: Vec<f64> = out.iter().skip(c).step_by(channels).copied().collect();
            let peak = (0..channel.len()).max_by(|a, b| channel[*a].total_cmp(&channel[*b])).unwrap();
            let truth = at as f64 * f64::from(to) / f64::from(from);
            assert!((peak as f64 - truth).abs() <= 1.0, "{from} -> {to}: the click at {at} came out at {peak}");
        }
    }
}
