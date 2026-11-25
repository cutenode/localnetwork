//! The output device's stream, and when what it's handed is heard.
//!
//! A track's playhead counts the frames handed to the output, which plays them a buffer and the
//! device's own latency later (tens of milliseconds through speakers, a few hundred through
//! Bluetooth). [`OutputClock`] keeps what each buffer's callback says of that, so the player can
//! draw, and place cues and loops, where the track is heard rather than where it's been read to.
//! rodio's own sink hides cpal's timestamps, so the stream is built here, on rodio's mixer.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// When the last buffer handed to the output will have been heard, against the wall clock.
#[derive(Debug)]
pub struct OutputClock {
    epoch: Instant,
    /// When the last buffer was handed over, in nanoseconds since `epoch` (0: none yet).
    at_ns: AtomicU64,
    /// From then until its last frame is heard, in nanoseconds: the device's latency and the
    /// buffer itself.
    ahead_ns: AtomicU64,
}

impl Default for OutputClock {
    fn default() -> OutputClock {
        OutputClock { epoch: Instant::now(), at_ns: AtomicU64::new(0), ahead_ns: AtomicU64::new(0) }
    }
}

impl OutputClock {
    /// A buffer of `frames` at `rate` was handed over just now, its first frame to be heard
    /// `delay` from now.
    pub fn handed(&self, frames: usize, rate: u32, delay: Duration) {
        let buffer = Duration::from_secs_f64(frames as f64 / f64::from(rate.max(1)));
        let ahead = u64::try_from((delay + buffer).as_nanos()).unwrap_or(u64::MAX);
        let at = u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX).max(1);
        self.ahead_ns.store(ahead, Ordering::Relaxed);
        self.at_ns.store(at, Ordering::Release);
    }

    /// How far what's heard now is behind what's been handed over.
    pub fn lag(&self) -> Duration {
        let at = self.at_ns.load(Ordering::Acquire);
        if at == 0 {
            return Duration::ZERO;
        }
        let since = Duration::from_nanos(u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX))
            .saturating_sub(Duration::from_nanos(at));
        lag_at(Duration::from_nanos(self.ahead_ns.load(Ordering::Relaxed)), since)
    }
}

/// What's heard lags what's been handed over by `ahead` as a buffer is handed over, less the time
/// `since` then (the device plays on meanwhile), and never by less than nothing.
pub fn lag_at(ahead: Duration, since: Duration) -> Duration {
    ahead.saturating_sub(since)
}

/// Where a playhead counted at `counted` is heard: `lag` behind, but never before `floor`, where it
/// last jumped to (a seek, a loop going round, playing on after a pause), whose sound is on its way.
pub fn heard(counted: Duration, lag: Duration, floor: Duration) -> Duration {
    counted.saturating_sub(lag).max(floor.min(counted))
}

/// What's heard, given what was last shown since the last jump (`None`: it jumped since): never a
/// little behind it, as the output's uneven buffers would make it, but a jump back (more than
/// [`JITTER`]) is taken as it is.
pub fn steady(heard: Duration, after: Option<Duration>) -> Duration {
    match after {
        Some(last) if heard < last && last - heard <= JITTER => last,
        _ => heard,
    }
}

/// The most the output's uneven buffers move what's heard back by.
pub const JITTER: Duration = Duration::from_millis(50);

#[cfg(feature = "playback")]
pub use stream::open;

#[cfg(feature = "playback")]
mod stream {
    use std::sync::Arc;

    use anyhow::{Context, Result};
    use rodio::Source;
    use rodio::cpal::traits::{DeviceTrait, StreamTrait};
    use rodio::cpal::{self, BufferSize, FromSample, SampleFormat, StreamConfig, SupportedStreamConfig};
    use rodio::mixer::{Mixer, mixer};
    use rodio::{ChannelCount, SampleRate};

    use super::OutputClock;

    /// The most delay a device is believed to have.
    const MOST_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

    /// A stream playing a mixer on `device`, with its clock: the device's default configuration
    /// with a buffer of about 50 ms (as rodio's sink asks for), else the first of the others the
    /// device supports that opens.
    pub fn open(
        device: &cpal::Device,
        on_error: impl FnMut(cpal::StreamError) + Send + Clone + 'static,
    ) -> Result<(cpal::Stream, Mixer, Arc<OutputClock>)> {
        let default = device.default_output_config().context("the output has no default configuration")?;
        let mut tried: Vec<SupportedStreamConfig> = vec![default];
        if let Ok(others) = device.supported_output_configs() {
            tried.extend(others.map(|c| c.with_max_sample_rate()));
        }
        let mut last = None;
        for config in tried {
            match open_with(device, &config, on_error.clone()) {
                Ok(opened) => return Ok(opened),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("the output supports no configuration")))
    }

    fn open_with(
        device: &cpal::Device,
        supported: &SupportedStreamConfig,
        on_error: impl FnMut(cpal::StreamError) + Send + 'static,
    ) -> Result<(cpal::Stream, Mixer, Arc<OutputClock>)> {
        let rate = supported.sample_rate();
        let channels = supported.channels();
        let frames = (rate / 20).next_power_of_two() / 2;
        let config = StreamConfig { channels, sample_rate: rate, buffer_size: BufferSize::Fixed(frames) };
        let (controller, source) = mixer(
            ChannelCount::new(channels).context("an output with no channels")?,
            SampleRate::new(rate).context("an output at 0 Hz")?,
        );
        let clock = Arc::new(OutputClock::default());
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32, _>(device, &config, source, clock.clone(), on_error),
            SampleFormat::F64 => build::<f64, _>(device, &config, source, clock.clone(), on_error),
            SampleFormat::I16 => build::<i16, _>(device, &config, source, clock.clone(), on_error),
            SampleFormat::I32 => build::<i32, _>(device, &config, source, clock.clone(), on_error),
            SampleFormat::U16 => build::<u16, _>(device, &config, source, clock.clone(), on_error),
            SampleFormat::U8 => build::<u8, _>(device, &config, source, clock.clone(), on_error),
            SampleFormat::I8 => build::<i8, _>(device, &config, source, clock.clone(), on_error),
            other => anyhow::bail!("unsupported sample format {other}"),
        }?;
        stream.play().context("the output wouldn't start")?;
        Ok((stream, controller, clock))
    }

    fn build<T, S>(
        device: &cpal::Device,
        config: &StreamConfig,
        mut source: S,
        clock: Arc<OutputClock>,
        on_error: impl FnMut(cpal::StreamError) + Send + 'static,
    ) -> Result<cpal::Stream>
    where
        T: cpal::SizedSample + FromSample<f32> + Send + 'static,
        S: Source + Send + 'static,
    {
        let (channels, rate) = (usize::from(config.channels.max(1)), config.sample_rate);
        let stream = device.build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                for d in data.iter_mut() {
                    *d = source.next().map(T::from_sample).unwrap_or(T::EQUILIBRIUM);
                }
                let ts = info.timestamp();
                // A driver that says more than a second is taken at its buffer alone.
                let delay = ts.playback.duration_since(&ts.callback).filter(|d| *d <= MOST_DELAY).unwrap_or_default();
                clock.handed(data.len() / channels, rate, delay);
            },
            on_error,
            None,
        )?;
        Ok(stream)
    }
}

#[cfg(test)]
mod tests;
