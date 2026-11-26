//! The output device's stream (`playback` builds): rodio's mixer played through cpal, so each
//! buffer's callback can say when it will be heard (see `output.rs`). Only a real device runs
//! this, so CI's coverage leaves the file out.

use anyhow::{Context, Result};
use std::sync::Arc;

use rodio::Source;
use rodio::cpal::traits::{DeviceTrait, StreamTrait};
use rodio::cpal::{self, BufferSize, FromSample, SampleFormat, StreamConfig, SupportedStreamConfig};
use rodio::mixer::{Mixer, mixer};
use rodio::{ChannelCount, SampleRate};

use super::{Audio, OutputClock, StreamErrors, stream_error};

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

/// Open the output device called `name`, or the default one (failing that, as rodio does, the
/// first other that opens). Its stream's errors are kept for the UI (see [`Audio::stream_error`]).
pub(super) fn open_device(name: Option<&str>) -> Result<Audio> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let host = rodio::cpal::default_host();
    let errors = StreamErrors::default();
    let on_error = {
        let errors = errors.clone();
        move |e| stream_error(&errors, e)
    };
    let open = |d: rodio::cpal::Device| open(&d, on_error.clone());
    let devices = || host.output_devices().ok().into_iter().flatten();
    let (stream, mixer, clock) = match name {
        Some(name) => {
            let device = devices()
                .find(|d| d.description().is_ok_and(|d| d.name() == name))
                .with_context(|| format!("no output called {name}"))?;
            open(device).with_context(|| format!("can't open {name}"))?
        }
        None => {
            let default = host.default_output_device().context("no audio output device")?;
            match open(default) {
                Ok(opened) => opened,
                Err(e) => devices()
                    .filter(|d| d.description().is_ok_and(|d| d.driver().is_some_and(|driver| driver != "null")))
                    .find_map(|d| open(d).ok())
                    .ok_or(e)
                    .context("no audio output device")?,
            }
        }
    };
    Ok(Audio {
        _stream: Some(stream),
        clock: Some(clock),
        device: name.map(str::to_string),
        errors,
        ..Audio::with(mixer)
    })
}
