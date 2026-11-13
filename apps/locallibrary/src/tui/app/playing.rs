//! The track playing and the queue, and opening what plays next.

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use locallibrary::anlz::Cue;
use locallibrary::anlz::{PhraseKind, TrackAnalysis, TrackWaves};
use locallibrary::{Library, Track};
use loclib_waveform::{Art, Wave};

use crate::tui::audio::{self};
use crate::tui::deck::BandMode;
use crate::tui::keys::Camelot;

use super::*;

pub struct NowPlaying {
    /// Matches the background read of the track's files to this play.
    pub id: u64,
    pub track: Track,
    pub root: PathBuf,
    pub analysis: TrackAnalysis,
    pub waves: DriveWaves,
    /// What of the analysis is a plugin's, not the drive's: a line to show.
    pub from_plugins: Option<String>,
    pub art: Option<Art>,
    pub duration: Duration,
    pub key: Option<Camelot>,
    /// rekordbox's phrases, placed in time with the beat grid.
    pub phrases: Vec<PhraseSpan>,
    /// Tracks on the same drive that mix into this one (best first). `None` when the drive's
    /// library wasn't open.
    pub matches: Option<Matches>,
    /// Playlists on the drive that contain the track.
    pub playlists: Vec<String>,
    /// The analysis, artwork and matches are still being read.
    pub loading: bool,
    /// The audio file is still being opened.
    pub opening: bool,
    /// Started by moving through the queue: if it won't play, the queue moves on past it.
    pub advancing: bool,
}

/// What the drive says of how a track's waveforms are drawn.
pub struct DriveWaves {
    /// Every kind it has.
    pub kinds: TrackWaves,
    /// What its My Settings colour them with, when it has them.
    pub bands: Option<BandMode>,
    /// What the overview's and the zoomed view's 3-band waveforms are turned up by.
    pub gains: (f32, f32),
}

impl Default for DriveWaves {
    fn default() -> DriveWaves {
        DriveWaves { kinds: TrackWaves::default(), bands: None, gains: (1.0, 1.0) }
    }
}

pub struct PhraseSpan {
    pub kind: PhraseKind,
    pub start_ms: f64,
    pub end_ms: f64,
    /// Length in bars of four beats.
    pub bars: u16,
}

/// Phrases with times: a phrase's beats are 1-based indexes into the grid; one that runs past the
/// grid ends with the track. One that doesn't end after it starts isn't drawn (the reader drops
/// those, but made-up data can hold them).
pub(super) fn phrase_spans(analysis: &TrackAnalysis, duration: Duration) -> Vec<PhraseSpan> {
    let Some(st) = &analysis.structure else { return Vec::new() };
    let at = |beat: u16| analysis.beats.get(beat as usize - 1).map(|b| b.time_ms as f64);
    let end_of_track = duration.as_secs_f64() * 1000.0;
    st.phrases
        .iter()
        .filter(|p| p.start_beat > 0 && p.end_beat > p.start_beat)
        .filter_map(|p| {
            Some(PhraseSpan {
                kind: p.kind,
                start_ms: at(p.start_beat)?,
                end_ms: at(p.end_beat).unwrap_or(end_of_track),
                bars: (p.end_beat - p.start_beat).div_ceil(4),
            })
        })
        .collect()
}

/// What's tried for `mode`, when it isn't there: the others, best first.
fn tried(mode: BandMode) -> [BandMode; 3] {
    match mode {
        BandMode::Blue => [BandMode::Blue, BandMode::Rgb, BandMode::ThreeBand],
        BandMode::Rgb => [BandMode::Rgb, BandMode::ThreeBand, BandMode::Blue],
        BandMode::ThreeBand => [BandMode::ThreeBand, BandMode::Rgb, BandMode::Blue],
    }
}

impl NowPlaying {
    /// The overview in `mode`, or the nearest there is, and what that came to. Older analysis has
    /// only blue, and what a plugin worked out is only in the analysis.
    pub fn overview_wave(&self, mode: BandMode) -> Option<(Wave<'_>, Option<BandMode>)> {
        let (w, gain) = (&self.waves.kinds, self.waves.gains.0);
        let of = |m: BandMode| match m {
            BandMode::Blue => w.blue_preview.as_ref().map(Wave::Layers),
            BandMode::Rgb => w.rgb_preview.as_ref().map(Wave::Layers),
            BandMode::ThreeBand => {
                w.band_preview.as_ref().map(|bands| Wave::Bands { bands, columns_per_sec: None, gain })
            }
        };
        (tried(mode).into_iter().find_map(|m| Some((of(m)?, Some(m)))))
            .or_else(|| Some((Wave::Layers(self.analysis.preview.as_ref()?), None)))
    }

    /// The zoomed waveform in `mode`, or the nearest there is, and what that came to.
    pub fn zoom_wave(&self, mode: BandMode) -> Option<(Wave<'_>, Option<BandMode>)> {
        let (w, gain) = (&self.waves.kinds, self.waves.gains.1);
        let of = |m: BandMode| match m {
            BandMode::Blue => w.blue_detail.as_ref().map(Wave::Layers),
            BandMode::Rgb => w.rgb_detail.as_ref().map(Wave::Layers),
            BandMode::ThreeBand => {
                w.band_detail.as_ref().map(|(bands, rate)| Wave::Bands { bands, columns_per_sec: Some(*rate), gain })
            }
        };
        (tried(mode).into_iter().find_map(|m| Some((of(m)?, Some(m)))))
            .or_else(|| Some((Wave::Layers(self.analysis.detail.as_ref()?), None)))
    }

    /// The tempo at `ms`: the beat grid's there, else the track's.
    pub fn bpm_at(&self, ms: f64) -> f64 {
        let at_beat = self.beat_at(ms).map(|(i, _, _)| self.analysis.beats[i].bpm);
        at_beat.filter(|b| *b > 0.0).unwrap_or(self.track.bpm)
    }

    /// Whole bars and beats on the grid from `ms` to `to_ms`, as a CDJ counts down to a cue:
    /// `None` without a grid there.
    pub fn bars_to(&self, ms: f64, to_ms: f64) -> Option<(usize, usize)> {
        let beats = &self.analysis.beats;
        let beat = beats.partition_point(|b| f64::from(b.time_ms) <= ms).checked_sub(1)?;
        let target = beats.partition_point(|b| f64::from(b.time_ms) < to_ms);
        let left = target.saturating_sub(beat);
        Some((left / 4, left % 4))
    }

    /// The next cue after `ms`, a hot cue over a memory cue at the same spot.
    pub fn next_cue(&self, ms: f64) -> Option<&Cue> {
        let later = self.analysis.cues.iter().filter(|c| f64::from(c.start_ms) > ms + 50.0);
        later.min_by_key(|c| (c.start_ms, c.hot_cue.is_none()))
    }

    /// The phrase playing at `ms`, its index, and the bar within it (from 1).
    pub fn phrase_at(&self, ms: f64) -> Option<(usize, &PhraseSpan, u16)> {
        let i = self.phrases.iter().position(|p| p.start_ms <= ms && ms < p.end_ms)?;
        let p = &self.phrases[i];
        let beats = self.analysis.beats.iter().filter(|b| b.time_ms as f64 >= p.start_ms && b.time_ms as f64 <= ms);
        Some((i, p, (beats.count().saturating_sub(1) / 4 + 1) as u16))
    }

    /// Hot cues and memory cues, in time order.
    pub fn cue_times(&self) -> Vec<u32> {
        let mut t: Vec<u32> = self.analysis.cues.iter().map(|c| c.start_ms).collect();
        t.sort_unstable();
        t.dedup();
        t
    }

    /// The beat at `ms`: (index into the grid, beat in bar 1-4, bar number from 1).
    pub fn beat_at(&self, ms: f64) -> Option<(usize, u16, usize)> {
        let beats = &self.analysis.beats;
        let i = beats.partition_point(|b| b.time_ms as f64 <= ms).checked_sub(1)?;
        let bars = beats[..=i].iter().filter(|b| b.beat_number == 1).count().max(1);
        Some((i, beats[i].beat_number, bars))
    }
}

/// What plays next: tracks of one library, by index, so queueing a list of thousands copies
/// none of them.
#[derive(Default)]
pub struct Queue {
    pub library: Arc<Library>,
    /// The tracks, as indexes into `library`, in the order of the list they came from.
    pub items: Vec<usize>,
    pub root: PathBuf,
    pub drive: String,
    /// Play order (indexes into `items`), shuffled or not.
    pub order: Vec<usize>,
    /// Position in `order`.
    pub pos: usize,
}

impl Queue {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Track `i` of the list (not of the play order).
    pub fn track(&self, i: usize) -> &Track {
        &self.library.tracks[self.items[i]]
    }
}

/// The track after the playing one, opened ahead so the player goes straight on to it.
pub(super) struct Upcoming {
    pub(super) now: NowPlaying,
    /// Its place in the queue's play order.
    pub(super) pos: usize,
    /// Queued behind the playing track (else it's opening, or wouldn't open).
    pub(super) queued: bool,
    /// Opened, but not to be queued (another sample rate): started as the playing one ends.
    pub(super) file: Option<audio::OpenedFile>,
}

/// How long before the end of a track the next one is opened, to follow it without a gap.
pub(super) const AHEAD: Duration = Duration::from_secs(30);

/// Opening a track, on the [`Opener`]'s worker.
type OpenJob = Box<dyn FnOnce(&Opening) + Send>;

/// Opens the tracks played (their audio files, then their analysis, artwork and matches) on a
/// worker of its own, one at a time. Starting a track drops what was asked for before it and
/// hasn't been done (holding `n` asks for dozens a second), and a file opened for a track no
/// longer wanted is closed rather than handed over: an eject waits for the worker (`idle`), so
/// no file on the drive is left open.
#[derive(Default)]
pub(super) struct Opener {
    jobs: Option<Sender<(u64, u64, OpenJob)>>,
    /// Bumped by each track started and each stop: work asked for before then is dropped.
    current: Arc<Mutex<u64>>,
    /// Held while a track is being opened.
    pub(super) idle: Arc<Mutex<()>>,
}

/// What an open on the [`Opener`]'s worker is for, and where its results go.
pub(super) struct Opening {
    generation: u64,
    current: Arc<Mutex<u64>>,
    tx: Sender<Msg>,
}

impl Opening {
    /// Whether what it's for is still wanted.
    fn wanted(&self) -> bool {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) == self.generation
    }

    /// Send `msg` while what it's for is still wanted (else it's dropped here, closing any file
    /// in it). Returns whether it was sent.
    pub(super) fn send(&self, msg: Msg) -> bool {
        let current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        *current == self.generation && self.tx.send(msg).is_ok()
    }
}

impl Opener {
    /// Open track `id` with `job`; `starts`: it's the track to play now, so whatever was asked
    /// for before it is dropped (the next track, opened ahead, isn't).
    pub(super) fn open(&mut self, tx: &Sender<Msg>, id: u64, starts: bool, job: OpenJob) {
        let generation = {
            let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
            *current += u64::from(starts);
            *current
        };
        let jobs = self.jobs.get_or_insert_with(|| {
            let (jobs, rx) = channel::<(u64, u64, OpenJob)>();
            let (current, idle, tx) = (self.current.clone(), self.idle.clone(), tx.clone());
            std::thread::spawn(move || {
                for (generation, id, job) in rx {
                    let _busy = idle.lock().unwrap_or_else(PoisonError::into_inner);
                    let opening = Opening { generation, current: current.clone(), tx: tx.clone() };
                    if !opening.wanted() {
                        continue;
                    }
                    if let Err(e) = caught(|| job(&opening)) {
                        opening.send(Msg::Opened(id, Err(e)));
                        opening.send(Msg::TrackInfo(id, Box::default()));
                    }
                }
            });
            jobs
        });
        let _ = jobs.send((generation, id, job));
    }

    /// Drop every track asked for that isn't open yet.
    pub(super) fn drop_all(&self) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) += 1;
    }
}

/// How long an opened file plays: what the decoder says, unless that's far from what rekordbox
/// says (some decoders guess from the start of a VBR file).
pub(super) fn file_duration(track: &Track, file: &audio::OpenedFile) -> Duration {
    let rekordbox = Duration::from_secs(track.duration_secs as u64);
    match file.duration {
        Some(d) if track.duration_secs == 0 || d.abs_diff(rekordbox) < Duration::from_secs(5) => d,
        _ => rekordbox,
    }
}
