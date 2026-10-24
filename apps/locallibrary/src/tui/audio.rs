//! Playback through an output device (the default one unless another is picked), with a tap that
//! copies what is being played into a lock-free ring buffer for the visualiser. Each track keeps
//! its own playhead, and plays a loop on the audio thread so it repeats on the sample (see
//! [`Looper`]). Seeks are asked of the audio thread through the playhead too, and the UI never
//! waits for one: an output that has gone away never answers. The next track can be queued behind
//! the playing one, to start the moment it ends.
//!
//! Output devices come with the `playback` feature (cpal; ALSA on Linux). Without it no device
//! opens, so the player plays nothing, but everything else here is the same. The tests use no
//! device at all (see [`Outputs`]): they play into a mixer they read themselves.

use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result};
use locallibrary::probe;
#[cfg(feature = "playback")]
use rodio::MixerDeviceSink;
use rodio::mixer::Mixer;
use rodio::source::SeekError;
use rodio::{ChannelCount, Decoder, Player, Sample, SampleRate, Source};

/// Mono samples (power of two, ~186 ms at 44.1 kHz).
const RING: usize = 8192;

/// The most recently played samples, mixed to mono. Written by the audio thread, read by the UI.
pub struct Tap {
    ring: Box<[AtomicU32]>,
    written: AtomicUsize,
    sample_rate: AtomicU32,
}

impl Default for Tap {
    fn default() -> Tap {
        Tap {
            ring: (0..RING).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicUsize::new(0),
            sample_rate: AtomicU32::new(44_100),
        }
    }
}

impl Tap {
    fn push(&self, s: f32) {
        // Single writer: the audio thread.
        let i = self.written.load(Ordering::Relaxed);
        self.ring[i & (RING - 1)].store(s.to_bits(), Ordering::Relaxed);
        self.written.store(i.wrapping_add(1), Ordering::Release);
    }

    /// Total samples written so far (wraps). Unchanged between two reads means nothing played.
    pub fn written(&self) -> usize {
        self.written.load(Ordering::Acquire)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    /// Fill `out` with the latest `out.len()` samples, oldest first.
    pub fn latest(&self, out: &mut [f32]) {
        let n = out.len().min(RING);
        let end = self.written();
        for (k, o) in out.iter_mut().enumerate().take(n) {
            let i = end.wrapping_sub(n).wrapping_add(k);
            *o = f32::from_bits(self.ring[i & (RING - 1)].load(Ordering::Relaxed));
        }
    }

    fn clear(&self) {
        for s in self.ring.iter() {
            s.store(0, Ordering::Relaxed);
        }
    }
}

/// Passes samples through unchanged, copying a mono mix of each frame into the [`Tap`].
struct Tapped<S> {
    inner: S,
    tap: Arc<Tap>,
    channels: u16,
    channel: u16,
    acc: f32,
}

impl<S: Source> Tapped<S> {
    fn new(inner: S, tap: Arc<Tap>) -> Self {
        tap.sample_rate.store(inner.sample_rate().get(), Ordering::Relaxed);
        let channels = inner.channels().get();
        Tapped { inner, tap, channels, channel: 0, acc: 0.0 }
    }
}

impl<S: Source> Iterator for Tapped<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.channel == 0 {
            // Channel count and rate may change between spans; spans end on frame boundaries.
            self.channels = self.inner.channels().get().max(1);
            self.tap.sample_rate.store(self.inner.sample_rate().get(), Ordering::Relaxed);
        }
        let s = self.inner.next()?;
        self.acc += s;
        self.channel += 1;
        if self.channel >= self.channels {
            self.tap.push(self.acc / f32::from(self.channels));
            self.acc = 0.0;
            self.channel = 0;
        }
        Some(s)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for Tapped<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

/// Where a track's playhead is, and the loop it plays: shared by the UI and the audio thread.
#[derive(Debug)]
pub struct Playhead {
    /// Frames played from the start of the track, as the audio thread last counted them.
    frame: AtomicU64,
    rate: AtomicU32,
    /// The loop, in microseconds into the track.
    loop_in: AtomicU64,
    loop_out: AtomicU64,
    /// Playing round the loop; off, playback carries on through its end.
    looping: AtomicBool,
    /// Counts the loops set, so the audio thread notices a new one.
    loop_set: AtomicU64,
    /// A seek asked for, in nanoseconds into the track (`NO_SEEK`: none). The audio thread makes
    /// it before its next frame, so it always lands on a frame's first channel.
    seek: AtomicU64,
    /// For a seek that keeps time: the frame the playhead was at when its target was worked
    /// out, so what has played since is added (`u64::MAX`: none).
    since: AtomicU64,
    /// The last seek asked for couldn't be made.
    seek_failed: AtomicBool,
    /// The player has moved on to this track (its first sample was asked for).
    started: AtomicBool,
    /// A sample of the track has played (a file with no audio in it ends without one).
    played: AtomicBool,
    /// Why it ended part-way: playing it panicked (its decoder tripping over a bad frame, say).
    broke: Mutex<Option<String>>,
    /// Its last sample was played (or it was dropped).
    ended: AtomicBool,
    /// Queued but not wanted after all: it ends as soon as it starts.
    dropped: AtomicBool,
    /// Channels and sample rate the track started with.
    format: (u16, u32),
}

impl Default for Playhead {
    fn default() -> Playhead {
        Playhead {
            frame: AtomicU64::new(0),
            rate: AtomicU32::new(0),
            loop_in: AtomicU64::new(0),
            loop_out: AtomicU64::new(0),
            looping: AtomicBool::new(false),
            loop_set: AtomicU64::new(0),
            seek: AtomicU64::new(NO_SEEK),
            since: AtomicU64::new(u64::MAX),
            seek_failed: AtomicBool::new(false),
            started: AtomicBool::new(false),
            played: AtomicBool::new(false),
            broke: Mutex::new(None),
            ended: AtomicBool::new(false),
            dropped: AtomicBool::new(false),
            format: (0, 0),
        }
    }
}

/// [`Playhead::seek`] when no seek is asked for.
const NO_SEEK: u64 = u64::MAX;

impl Playhead {
    /// Where the track is: where a seek asked for lands, until the audio thread has made it.
    pub fn position(&self) -> Duration {
        let asked = self.seek.load(Ordering::Acquire);
        if asked != NO_SEEK {
            return Duration::from_nanos(asked);
        }
        match self.rate.load(Ordering::Relaxed) {
            0 => Duration::ZERO,
            rate => {
                let frame = self.frame.load(Ordering::Relaxed);
                Duration::from_nanos((frame as u128 * 1_000_000_000 / rate as u128) as u64)
            }
        }
    }

    /// Loop from `from` to `to` (a new loop, or the same one shorter or longer), or (`None`) let
    /// playback carry on through the end of the loop.
    pub fn set_loop(&self, bounds: Option<(Duration, Duration)>) {
        match bounds {
            Some((from, to)) => {
                self.loop_in.store(from.as_micros() as u64, Ordering::Relaxed);
                self.loop_out.store(to.as_micros() as u64, Ordering::Relaxed);
                self.looping.store(true, Ordering::Relaxed);
                self.loop_set.fetch_add(1, Ordering::Release);
            }
            None => self.looping.store(false, Ordering::Relaxed),
        }
    }

    pub fn is_looping(&self) -> bool {
        self.looping.load(Ordering::Relaxed)
    }

    /// Ask the audio thread to seek to `pos` (in place of a seek asked for before it).
    fn ask_seek(&self, pos: Duration) {
        self.seek.store((pos.as_nanos() as u64).min(NO_SEEK - 1), Ordering::Release);
    }
}

/// The most of a loop kept in memory, in samples: 30 seconds of 48 kHz stereo. A longer loop
/// goes back by seeking the file each time round.
const KEPT: usize = 30 * 48_000 * 2;

/// Plays a track, counting its frames into a [`Playhead`] and playing the playhead's loop: at the
/// loop's end it goes back to the start. The first time round, the loop's samples are kept, so
/// after that it repeats from memory, without seeking the file (no reading from the drive on the
/// audio thread, and no gap). Changes to the loop take effect between frames.
pub struct Looper<S> {
    inner: S,
    head: Arc<Playhead>,
    channels: u16,
    rate: u32,
    /// The channel the next sample is for.
    channel: u16,
    frame: u64,
    /// The loop, in frames, as last read from the playhead (and which one it was).
    bounds: Option<(u64, u64)>,
    loop_set: u64,
    /// The loop's samples, from its start, as they played the first time round.
    kept: Vec<Sample>,
    keeping: bool,
    /// Playing from `kept` (the next sample's index), while the track waits at the loop's end.
    replay: Option<usize>,
    /// A sample has been asked for.
    started: bool,
    /// Playing it panicked: it has ended (see [`Playhead::broke`]).
    broken: bool,
}

impl<S: Source> Looper<S> {
    pub fn new(inner: S, head: Arc<Playhead>) -> Self {
        let (channels, rate) = (inner.channels().get().max(1), inner.sample_rate().get());
        head.rate.store(rate, Ordering::Relaxed);
        Looper {
            inner,
            head,
            channels,
            rate,
            channel: 0,
            frame: 0,
            bounds: None,
            loop_set: 0,
            kept: Vec::new(),
            keeping: false,
            replay: None,
            started: false,
            broken: false,
        }
    }

    fn frames(&self, micros: u64) -> u64 {
        (micros as u128 * self.rate as u128 / 1_000_000) as u64
    }

    /// Seek the track to `frame` (a quarter frame on, so rounding either way lands on it).
    fn seek_track(&mut self, frame: u64) -> Result<(), SeekError> {
        let nanos = (frame as f64 + 0.25) * 1e9 / self.rate.max(1) as f64;
        self.inner.try_seek(Duration::from_nanos(nanos as u64))
    }

    /// Before each frame: make the seek asked for, take up a new loop, and go round the one
    /// playing.
    fn frame_start(&mut self) {
        let asked = self.head.seek.swap(NO_SEEK, Ordering::Acquire);
        if asked != NO_SEEK {
            self.seek(Duration::from_nanos(asked));
        }
        if self.replay.is_none() {
            // Channel count and rate may change between spans, which end on frame boundaries.
            self.channels = self.inner.channels().get().max(1);
            let rate = self.inner.sample_rate().get();
            if rate != self.rate {
                self.rate = rate;
                self.head.rate.store(rate, Ordering::Relaxed);
            }
        }
        let set = self.head.loop_set.load(Ordering::Acquire);
        if set != self.loop_set {
            self.loop_set = set;
            self.take_loop();
        }
        let Some((a, b)) = self.bounds else { return };
        if self.frame >= b {
            self.keeping = false; // all of it, or played on past it
        }
        let looping = self.head.is_looping();
        if looping && self.frame == b {
            self.wrap(a, b);
        } else if self.replay.is_some_and(|i| i >= self.kept.len()) {
            // Out of the loop: the track carries on from its end, where it waited.
            self.replay = None;
        }
        if looping && self.replay.is_none() && self.frame == a && !self.keeping {
            self.keep(a, b);
        }
    }

    /// A loop was set: from here on it's the one played.
    fn take_loop(&mut self) {
        let (a, b) = (
            self.frames(self.head.loop_in.load(Ordering::Relaxed)),
            self.frames(self.head.loop_out.load(Ordering::Relaxed)),
        );
        let new = (a < b).then_some((a, b));
        if new == self.bounds {
            return; // the same loop again: what's kept still is it
        }
        let resized = self.bounds.is_some_and(|(old, _)| Some(old) == new.map(|(a, _)| a));
        if self.replay.take().is_some() {
            // The track waited at the old loop's end: bring it to where playback is.
            let _ = self.seek_track(self.frame);
        }
        self.kept.clear();
        self.keeping = false;
        self.bounds = new;
        // Made shorter with the playhead past its new end: back into it, as far round as before.
        if let Some((a, b)) = new
            && resized
            && self.frame >= b
        {
            let to = a + (self.frame - a) % (b - a);
            if self.seek_track(to).is_ok() {
                self.frame = to;
                self.head.frame.store(to, Ordering::Relaxed);
            }
        }
    }

    /// Keep the loop's samples as it plays from its start, if it's short enough.
    fn keep(&mut self, a: u64, b: u64) {
        let n = (b - a) as usize * self.channels as usize;
        self.kept.clear();
        if n <= KEPT {
            self.kept.reserve_exact(n);
            self.keeping = true;
        }
    }

    /// At the loop's end: back to its start, from memory once it's all kept.
    fn wrap(&mut self, a: u64, b: u64) {
        // Kept whole, the track has played to the loop's end (or waits there, replaying).
        let whole = self.kept.len() as u64 == (b - a) * self.channels as u64;
        if whole {
            self.replay = Some(0);
            self.frame = a;
        } else if self.seek_track(a).is_ok() {
            self.replay = None;
            self.frame = a;
        } else {
            // The track can't go back: play on.
            self.head.looping.store(false, Ordering::Relaxed);
        }
        self.head.frame.store(self.frame, Ordering::Relaxed);
    }

    /// Seek to `pos`, between two frames: the track (which is between two frames too, playing
    /// or waiting at a kept loop's end) and the loop's memory both start the next one on its
    /// first channel.
    fn seek(&mut self, pos: Duration) {
        let mut to = (pos.as_secs_f64() * self.rate as f64 + 0.25) as u64;
        // A seek that keeps time lands as much further on as has played since it was asked for.
        let since = self.head.since.swap(u64::MAX, Ordering::Relaxed);
        if since != u64::MAX {
            to += self.frame.saturating_sub(since);
        }
        match (self.replay, self.bounds) {
            (Some(_), Some((a, b))) if (a..b).contains(&to) => {
                // Inside the loop playing from memory: stay there, the track waits at its end.
                self.replay = Some((to - a) as usize * self.channels as usize);
            }
            _ => {
                if self.seek_track(to).is_err() {
                    self.head.seek_failed.store(true, Ordering::Relaxed);
                    return;
                }
                self.replay = None;
                self.keeping = false;
                match self.bounds {
                    Some((a, b)) if self.head.is_looping() && to == a => self.keep(a, b),
                    _ => self.kept.clear(),
                }
            }
        }
        self.frame = to;
        self.head.frame.store(to, Ordering::Relaxed);
    }
}

impl<S: Source> Iterator for Looper<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.broken {
            return None;
        }
        // A panic here, on the audio thread, would stop the output's stream or end the player:
        // the track ends instead, and the UI says why (see `Audio::broken`).
        super::app::caught(|| self.sample()).unwrap_or_else(|e| {
            self.broken = true;
            *self.head.broke.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
            self.head.ended.store(true, Ordering::Release);
            None
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<S: Source> Looper<S> {
    /// The next sample: the track's, or the loop's from memory.
    fn sample(&mut self) -> Option<Sample> {
        if self.channel == 0 {
            if !self.started {
                self.started = true;
                self.head.started.store(true, Ordering::Release);
            }
            if self.head.dropped.load(Ordering::Relaxed) {
                self.head.ended.store(true, Ordering::Release);
                return None;
            }
            self.frame_start();
        }
        let s = match self.replay.and_then(|i| self.kept.get(i)) {
            Some(&s) => {
                self.replay = self.replay.map(|i| i + 1);
                s
            }
            None => {
                // (Run out of what's kept: the track waits at the loop's end, so on from there.)
                self.replay = None;
                let Some(s) = self.inner.next() else {
                    self.head.ended.store(true, Ordering::Release);
                    return None;
                };
                if !self.head.played.load(Ordering::Relaxed) {
                    self.head.played.store(true, Ordering::Relaxed);
                }
                if self.keeping {
                    self.kept.push(s);
                }
                s
            }
        };
        self.channel += 1;
        if self.channel >= self.channels {
            self.channel = 0;
            self.frame += 1;
            self.head.frame.store(self.frame, Ordering::Relaxed);
        }
        Some(s)
    }
}

impl<S: Source> Source for Looper<S> {
    fn current_span_len(&self) -> Option<usize> {
        if self.head.ended.load(Ordering::Relaxed) {
            return Some(0); // so a queue goes on to the next track's channels and rate
        }
        // Some(0) says the track is over, so never at the end of what's kept: round the loop
        // again from memory, or on with the track from where it waits.
        match self.replay {
            Some(i) if i < self.kept.len() => Some(self.kept.len() - i),
            Some(_) if self.head.is_looping() => Some(self.kept.len()),
            _ => self.inner.current_span_len(),
        }
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(self.channels).unwrap_or(ChannelCount::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    /// Seeking is asked of the playhead and made before the next frame (see [`Playhead::seek`]).
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.head.ask_seek(pos);
        Ok(())
    }
}

/// What went wrong with an output's stream, last: what it said, and whether the output is gone
/// (unplugged, or its stream can't go on), not just a glitch.
type StreamErrors = Arc<Mutex<Option<(String, bool)>>>;

/// Keep a stream's error for the UI (rodio would print it, over the TUI). One that says the
/// output is gone isn't replaced by a glitch after it.
#[cfg(feature = "playback")]
fn stream_error(errors: &StreamErrors, e: rodio::cpal::StreamError) {
    use rodio::cpal::StreamError;
    let gone = matches!(e, StreamError::DeviceNotAvailable | StreamError::StreamInvalidated);
    let mut last = errors.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if gone || !last.as_ref().is_some_and(|(_, g)| *g) {
        *last = Some((e.to_string(), gone));
    }
}

/// An audio file opened and ready to play. Opening reads the start of the file (from a USB
/// drive, which can be slow), so it happens on a worker thread; see [`open_file`].
pub struct OpenedFile {
    /// (Boxed: moved about while it waits to be played, and it's big.)
    decoder: Box<Decoder<BufReader<File>>>,
    /// Channels and sample rate.
    format: (u16, u32),
    /// The decoder's idea of the duration, when it has one.
    pub duration: Option<Duration>,
    /// Where it starts playing (auto cue), when not at the beginning.
    pub start: Option<Duration>,
}

/// Above this size an MP3 seeks coarsely (see [`open_file`]).
const LONG_MP3: u64 = 40_000_000;

/// Open `path` to play, on the worker and guarded ([`probe::guarded`]): symphonia's probe can
/// panic on a bad file (a WAV saying 0 Hz), and so can rodio asked for the channels or rate of a
/// stream that has none, which playing it would ask on the UI thread. A panic while decoding as
/// it plays is caught on the audio thread, in [`Looper`]'s `next`.
pub fn open_file(path: &Path) -> Result<OpenedFile> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let len = file.metadata().map(|m| m.len()).ok();
    let mut builder = Decoder::builder().with_data(BufReader::new(file)).with_seekable(true).with_gapless(true);
    if let Some(len) = len {
        builder = builder.with_byte_len(len);
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(&ext.to_ascii_lowercase());
    }
    // An accurate seek back in an MP3 reads the file again from the start, which in a recorded
    // set (anything past 40 MB, a quarter of an hour or more) takes seconds: those seek to where
    // the time should be in the file instead, close enough for the set's view.
    if len.is_some_and(|l| l > LONG_MP3) && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp3")) {
        builder = builder.with_coarse_seek(true);
    }
    probe::guarded(path, || {
        let decoder = builder.build().with_context(|| format!("can't decode {}", path.display()))?;
        let format = (decoder.channels().get(), decoder.sample_rate().get());
        let duration = decoder.total_duration();
        Ok(OpenedFile { decoder: Box::new(decoder), format, duration, start: None })
    })
}

/// The outputs' names, and which is the system's default.
pub type Listed = (Vec<String>, Option<String>);

/// The outputs the player can play through: the machine's devices (see [`Outputs::DEVICES`]), or
/// none (an app made for a test, which mustn't play through the speakers).
#[derive(Clone, Copy)]
pub struct Outputs {
    /// List the outputs. It can take a while (cpal asks every host), so the player does it in
    /// the background.
    pub list: fn() -> Result<Listed>,
    /// Open the output called `name` (the default for `None`).
    pub open: fn(Option<&str>) -> Result<Audio>,
}

impl Outputs {
    /// The machine's output devices.
    pub const DEVICES: Outputs = Outputs { list: device_names, open: open_device };

    /// None at all.
    pub const NONE: Outputs =
        Outputs { list: || Ok((Vec::new(), None)), open: |_| anyhow::bail!("no audio output device") };

    /// One that plays to no one, in real time (see [`Audio::silent`]).
    pub const SILENT: Outputs = Outputs { list: || Ok((Vec::new(), None)), open: |_| Ok(Audio::silent()) };

    /// The machine's devices, or [`Outputs::SILENT`] when `$LOCALLIBRARY_AUDIO` is `silent`: a
    /// test switch, for harnesses that play tracks in the player (the AT Store screenshots) and
    /// mustn't be heard.
    pub fn from_env() -> Outputs {
        match std::env::var_os("LOCALLIBRARY_AUDIO") {
            Some(v) if v == "silent" => Outputs::SILENT,
            _ => Outputs::DEVICES,
        }
    }

    /// Open the output called `name` (the default one for `None`), else the default. Also says
    /// why the one asked for didn't open.
    pub fn open_preferring(self, name: Option<&str>) -> (Result<Audio>, Option<String>) {
        match name.map(|n| (self.open)(Some(n))) {
            Some(Ok(a)) => (Ok(a), None),
            Some(Err(e)) => ((self.open)(None), Some(format!("{e:#}"))),
            None => ((self.open)(None), None),
        }
    }
}

/// The names of the output devices, and which is the system's default (failing when the host
/// can't list them at all).
#[cfg(feature = "playback")]
fn device_names() -> Result<Listed> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let host = rodio::cpal::default_host();
    let name = |d: &rodio::cpal::Device| d.description().ok().map(|d| d.name().to_string());
    let default = host.default_output_device().as_ref().and_then(name);
    let names = host.output_devices()?.filter_map(|d| name(&d)).collect();
    Ok((names, default))
}

/// Built without the `playback` feature: no output devices.
#[cfg(not(feature = "playback"))]
fn device_names() -> Result<Listed> {
    Ok((Vec::new(), None))
}

/// Open the output device called `name`, or the default one (failing that, as rodio does, the
/// first other that opens). Its stream's errors are kept for the UI (see [`Audio::stream_error`]).
#[cfg(feature = "playback")]
fn open_device(name: Option<&str>) -> Result<Audio> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let host = rodio::cpal::default_host();
    let errors = StreamErrors::default();
    let on_error = {
        let errors = errors.clone();
        move |e| stream_error(&errors, e)
    };
    let open = |d| {
        rodio::DeviceSinkBuilder::from_device(d)
            .and_then(|b| b.with_error_callback(on_error.clone()).open_sink_or_fallback())
    };
    let devices = || host.output_devices().ok().into_iter().flatten();
    let mut sink = match name {
        Some(name) => {
            let device = devices()
                .find(|d| d.description().is_ok_and(|d| d.name() == name))
                .with_context(|| format!("no output called {name}"))?;
            open(device).with_context(|| format!("can't open {name}"))?
        }
        None => {
            let default = host.default_output_device().context("no audio output device")?;
            match open(default) {
                Ok(sink) => sink,
                Err(e) => devices()
                    .filter(|d| d.description().is_ok_and(|d| d.driver().is_some_and(|driver| driver != "null")))
                    .find_map(|d| open(d).ok())
                    .ok_or(e)
                    .context("no audio output device")?,
            }
        }
    };
    sink.log_on_drop(false);
    let mixer = sink.mixer().clone();
    Ok(Audio { _sink: Some(sink), device: name.map(str::to_string), errors, ..Audio::with(mixer) })
}

/// Built without the `playback` feature, no output opens.
#[cfg(not(feature = "playback"))]
fn open_device(_: Option<&str>) -> Result<Audio> {
    anyhow::bail!("no audio output: this locallibrary was built without playback")
}

pub struct Audio {
    /// The output device (`None` in tests, which play into a mixer they read themselves).
    #[cfg(feature = "playback")]
    _sink: Option<MixerDeviceSink>,
    /// The device's name, when it was picked rather than the default.
    device: Option<String>,
    /// What went wrong with its stream, last.
    errors: StreamErrors,
    mixer: Mixer,
    player: Option<Player>,
    /// The playheads of the tracks handed to the player: the playing one, then the one queued
    /// after it, if any.
    heads: VecDeque<Arc<Playhead>>,
    volume: f32,
    pub tap: Arc<Tap>,
}

impl Audio {
    /// Playing into `mixer`, with no device.
    fn with(mixer: Mixer) -> Audio {
        Audio {
            #[cfg(feature = "playback")]
            _sink: None,
            device: None,
            errors: StreamErrors::default(),
            mixer,
            player: None,
            heads: VecDeque::new(),
            volume: 0.8,
            tap: Arc::new(Tap::default()),
        }
    }

    /// The output device's name, when it's one picked rather than the default.
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// No device: what plays is read from the mixer returned, mono at `rate`.
    pub fn offline(rate: u32) -> (Audio, rodio::mixer::MixerSource) {
        let (mixer, out) = rodio::mixer::mixer(ChannelCount::MIN, SampleRate::new(rate).unwrap());
        (Audio::with(mixer), out)
    }

    /// No device, with a thread of its own reading what plays as fast as a device would and
    /// throwing it away, so the playhead and the visualisers move as they do through speakers.
    /// The thread lasts as long as the process: [`Outputs::SILENT`] lists no outputs to switch to,
    /// so it's opened once a run.
    fn silent() -> Audio {
        const RATE: u32 = 44_100;
        /// A hundredth of a second of samples, read at a time.
        const CHUNK: usize = RATE as usize / 100;
        let (audio, mut out) = Audio::offline(RATE);
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            for n in 1u32.. {
                out.by_ref().take(CHUNK).for_each(drop);
                std::thread::sleep(
                    (start + Duration::from_millis(10) * n).saturating_duration_since(std::time::Instant::now()),
                );
            }
        });
        audio
    }

    /// Start playing an opened file (from the beginning, or where auto cue put it), replacing
    /// whatever was playing; or have it ready, `paused`.
    pub fn play(&mut self, file: OpenedFile, paused: bool) {
        self.stop();
        self.tap.clear();
        // The track goes in before the player meets the mixer, which sets itself up for the
        // first sound's channels and rate as it's added.
        let (player, out) = Player::new();
        player.set_volume(self.volume);
        if paused {
            player.pause();
        }
        self.player = Some(player);
        self.append(file);
        self.mixer.add(out);
    }

    fn append(&mut self, file: OpenedFile) {
        let Some(p) = &self.player else { return };
        let head = Arc::new(Playhead { format: file.format, ..Playhead::default() });
        let mut looper = Looper::new(*file.decoder, head.clone());
        if let Some(at) = file.start {
            // Made by the audio thread before the first frame (from the start, if it can't be).
            let _ = looper.try_seek(at);
        }
        p.append(Tapped::new(looper, self.tap.clone()));
        self.heads.push_back(head);
    }

    /// Queue a track to start the moment the playing one ends, without a gap (in place of one
    /// queued before). Only one with the playing track's channels and sample rate: the mixer
    /// would play the start of another at the wrong rate, so it's handed back (as it is with
    /// nothing playing).
    pub fn queue(&mut self, file: OpenedFile) -> Result<(), OpenedFile> {
        match self.heads.front() {
            Some(h) if self.player.is_some() && h.format == file.format => {
                self.unqueue();
                self.append(file);
                Ok(())
            }
            _ => Err(file),
        }
    }

    /// Drop the track queued after the playing one.
    pub fn unqueue(&mut self) {
        while self.heads.len() > 1 {
            if let Some(h) = self.heads.pop_back() {
                h.dropped.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Whether the player has moved on to the queued track, which is now the one playing.
    pub fn advanced(&mut self) -> bool {
        if self.heads.get(1).is_some_and(|h| h.started.load(Ordering::Acquire)) {
            self.heads.pop_front();
            return true;
        }
        false
    }

    pub fn stop(&mut self) {
        if let Some(p) = self.player.take() {
            p.stop();
        }
        self.heads.clear();
    }

    pub fn is_loaded(&self) -> bool {
        self.player.is_some()
    }

    pub fn is_paused(&self) -> bool {
        self.player.as_ref().is_none_or(Player::is_paused)
    }

    pub fn toggle_pause(&self) {
        if let Some(p) = &self.player {
            if p.is_paused() { p.play() } else { p.pause() }
        }
    }

    /// The playing track played to its end, with nothing queued after it.
    pub fn finished(&self) -> bool {
        self.player.as_ref().is_some_and(Player::empty)
    }

    pub fn position(&self) -> Duration {
        self.heads.front().map(|h| h.position()).unwrap_or_default()
    }

    /// Seek to `pos`: asked of the audio thread, which makes it before the next frame it plays
    /// (paused, once it plays on); [`Self::position`] says `pos` meanwhile. Keeping time, `pos`
    /// was worked out from where the playhead is now, and what plays before the seek is made is
    /// added to it.
    pub fn seek(&self, pos: Duration, keep_time: bool) {
        if let Some(h) = self.heads.front() {
            let since = if keep_time { h.frame.load(Ordering::Relaxed) } else { u64::MAX };
            h.since.store(since, Ordering::Relaxed);
            h.ask_seek(pos);
            self.tap.clear();
        }
    }

    /// For tests: the output's stream says `e` (`gone`: the output went away).
    #[cfg(test)]
    pub fn stream_failed(&self, e: &str, gone: bool) {
        *self.errors.lock().unwrap() = Some((e.to_string(), gone));
    }

    /// Whether a seek in the playing track couldn't be made since this was last asked.
    pub fn seek_failed(&self) -> bool {
        self.heads.front().is_some_and(|h| h.seek_failed.swap(false, Ordering::Relaxed))
    }

    /// Why the playing track ended without playing through, if it did: there was no audio in
    /// the file, or playing it panicked.
    pub fn broken(&self) -> Option<String> {
        let h = self.heads.front().filter(|h| h.ended.load(Ordering::Acquire))?;
        let broke = h.broke.lock().unwrap_or_else(PoisonError::into_inner).clone();
        broke.or_else(|| (!h.played.load(Ordering::Relaxed)).then(|| "there's no audio in it".into()))
    }

    /// For tests: playing the track panics, saying `e`, at its next frame.
    #[cfg(test)]
    pub fn break_playing(&self, e: &str) {
        if let Some(h) = self.heads.front() {
            *h.broke.lock().unwrap() = Some(e.to_string());
            h.dropped.store(true, Ordering::Relaxed);
        }
    }

    /// What went wrong with the output's stream since this was last asked, and whether the
    /// output is gone (unplugged, say: nothing plays through it again).
    pub fn stream_error(&self) -> Option<(String, bool)> {
        self.errors.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    /// Loop the playing track between two points, or stop looping (`None`).
    pub fn set_loop(&self, bounds: Option<(Duration, Duration)>) {
        if let Some(h) = self.heads.front() {
            h.set_loop(bounds);
        }
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.5);
        if let Some(p) = &self.player {
            p.set_volume(self.volume);
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
