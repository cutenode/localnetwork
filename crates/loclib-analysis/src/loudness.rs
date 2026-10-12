//! Loudness (EBU R 128 through [ebur128](https://github.com/sdroege/ebur128)), peaks, and clipping.

use ebur128::{EbuR128, Mode};
use serde::{Deserialize, Serialize};

use crate::Result;

/// Samples at or above this are at full scale: 32767/32768, the most a 16-bit file holds, as its
/// decoded floats count. A 24-bit or float master clipped on export lands here or above it.
const FULL_SCALE: f32 = 32_767.0 / 32_768.0;

/// How many full-scale samples in a row make a clipped run. One can be a peak that just touches
/// 0 dBFS; three in a row is the default Audacity's Find Clipping starts a clip at.
const CLIPPED_RUN: u32 = 3;

/// How loud a track is, and how close to clipping.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Loudness {
    /// Integrated loudness (ITU-R BS.1770, as EBU R 128 measures it), in LUFS. `None` when
    /// nothing is above the -70 LUFS gate: silence.
    pub integrated_lufs: Option<f64>,
    /// Loudness range (EBU Tech 3342), in LU: how much the loudness moves between quiet and loud
    /// passages.
    pub range_lu: f64,
    /// The highest sample in any channel, 1.0 at full scale.
    pub sample_peak: f64,
    /// The highest true peak in any channel (oversampled 4x below 96 kHz), 1.0 at full scale.
    /// Above 1.0 means a converter or lossy encoder can clip it even where no sample does.
    pub true_peak: f64,
    /// Runs of three or more full-scale samples in a row in one channel.
    pub clipped_runs: u64,
    /// The samples in those runs.
    pub clipped_samples: u64,
}

impl Loudness {
    /// [`Loudness::sample_peak`] in dBFS (minus infinity for silence).
    pub fn sample_peak_dbfs(&self) -> f64 {
        20.0 * self.sample_peak.log10()
    }

    /// [`Loudness::true_peak`] in dBTP (minus infinity for silence).
    pub fn true_peak_dbtp(&self) -> f64 {
        20.0 * self.true_peak.log10()
    }
}

/// Measures [`Loudness`] as packets stream in, at the rate and channel count of the first.
#[derive(Default)]
pub(crate) struct Meter {
    ebu: Option<EbuR128>,
    /// Each channel's run of full-scale samples so far.
    runs: Vec<u32>,
    clipped_runs: u64,
    clipped_samples: u64,
}

impl Meter {
    /// `samples` interleaved, `channels` to a frame, at `rate` Hz.
    pub(crate) fn push(&mut self, samples: &[f32], rate: u32, channels: usize) -> Result<(), ebur128::Error> {
        let ebu = match &mut self.ebu {
            Some(ebu) => ebu,
            None => {
                self.runs = vec![0; channels];
                self.ebu.insert(EbuR128::new(channels as u32, rate, Mode::I | Mode::LRA | Mode::TRUE_PEAK)?)
            }
        };
        ebu.add_frames_f32(samples)?;
        for frame in samples.chunks_exact(self.runs.len()) {
            for (run, s) in self.runs.iter_mut().zip(frame) {
                if s.abs() < FULL_SCALE {
                    *run = 0;
                    continue;
                }
                *run += 1;
                // A run is counted once it's long enough, then grows sample by sample.
                if *run == CLIPPED_RUN {
                    self.clipped_runs += 1;
                    self.clipped_samples += u64::from(CLIPPED_RUN);
                } else if *run > CLIPPED_RUN {
                    self.clipped_samples += 1;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<Loudness, ebur128::Error> {
        let Some(ebu) = self.ebu else {
            return Ok(Loudness {
                integrated_lufs: None,
                range_lu: 0.0,
                sample_peak: 0.0,
                true_peak: 0.0,
                clipped_runs: 0,
                clipped_samples: 0,
            });
        };
        let peak = |f: fn(&EbuR128, u32) -> Result<f64, ebur128::Error>| {
            (0..ebu.channels()).try_fold(0.0f64, |max, c| Ok(max.max(f(&ebu, c)?)))
        };
        Ok(Loudness {
            integrated_lufs: Some(ebu.loudness_global()?).filter(|l| l.is_finite()),
            range_lu: ebu.loudness_range()?,
            sample_peak: peak(EbuR128::sample_peak)?,
            true_peak: peak(EbuR128::true_peak)?,
            clipped_runs: self.clipped_runs,
            clipped_samples: self.clipped_samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Single full-scale samples and pairs aren't clipping; runs of three or more are, counted
    /// once each whatever their length, across packets, in each channel apart.
    #[test]
    fn clipped_runs_need_three_samples_in_a_row() {
        let full = 1.0;
        let left = [0.0, full, 0.0, full, -full, 0.0, full, full, full, full, -full, 0.0];
        let right = [0.0, 0.0, -full, -full, -full, 0.0, 0.0, 0.0, full, 0.0, 0.0, 0.0];
        let interleaved: Vec<f32> = left.iter().zip(&right).flat_map(|(l, r)| [*l, *r]).collect();
        let mut meter = Meter::default();
        // Split inside the left channel's run of five.
        for packet in [&interleaved[..16], &interleaved[16..]] {
            meter.push(packet, 44_100, 2).unwrap();
        }
        let l = meter.finish().unwrap();
        assert_eq!((l.clipped_runs, l.clipped_samples), (2, 8));
        assert_eq!(l.sample_peak, 1.0);
    }

    #[test]
    fn nothing_decoded_measures_nothing() {
        let l = Meter::default().finish().unwrap();
        assert_eq!((l.integrated_lufs, l.true_peak, l.clipped_runs), (None, 0.0, 0));
    }

    use hegel::TestCase;
    use hegel::generators as gs;

    /// However a signal is split into packets, in any number of channels: each channel's runs of
    /// three or more full-scale samples are counted once each with all their samples, shorter
    /// ones not at all, and the peak is the loudest sample.
    #[hegel::test(test_cases = 200)]
    fn clipping_is_counted_run_by_run(tc: TestCase) {
        let channels = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
        let levels = gs::sampled_from(vec![0.0f32, 0.5, -0.25, 1.0, -1.0, FULL_SCALE, 0.999]);
        let frames = tc.draw(gs::vecs(gs::vecs(levels).min_size(channels).max_size(channels)).max_size(200));
        let interleaved: Vec<f32> = frames.concat();
        let mut meter = Meter::default();
        let mut rest = &interleaved[..];
        while !rest.is_empty() {
            let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(64)).min(rest.len() / channels) * channels;
            meter.push(&rest[..n], 44_100, channels).unwrap();
            rest = &rest[n..];
        }
        let l = meter.finish().unwrap();
        let (mut runs, mut samples) = (0, 0);
        for c in 0..channels {
            let column: Vec<bool> = frames.iter().map(|f| f[c].abs() >= FULL_SCALE).collect();
            for run in column.split(|clipped| !clipped).map(<[bool]>::len).filter(|n| *n >= CLIPPED_RUN as usize) {
                runs += 1;
                samples += run as u64;
            }
        }
        assert_eq!((l.clipped_runs, l.clipped_samples), (runs, samples));
        let loudest = interleaved.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert_eq!(l.sample_peak, f64::from(loudest));
        assert_eq!(l.sample_peak_dbfs(), 20.0 * f64::from(loudest).log10());
    }
}
