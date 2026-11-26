//! The output device's stream, and when what it's handed is heard.
//!
//! A track's playhead counts the frames handed to the output, which plays them a buffer and the
//! device's own latency later (tens of milliseconds through speakers, a few hundred through
//! Bluetooth). [`OutputClock`] keeps what each buffer's callback says of that, so the player can
//! draw, and place cues and loops, where the track is heard rather than where it's been read to.
//! The device's stream that tells it is in `device.rs`.

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

#[cfg(test)]
mod tests;
