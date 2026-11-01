//! What analyzers work out for a track that isn't a column (feature `analysis`): a beat grid,
//! phrases and a waveform. Kept beside their fields and keyed the same way (see
//! [`super::FieldStore`]), in `<store>/cache/overlay/<plugin>/<analyzer>/<version>/`. Only
//! drawn, where a track's drive has none, unless they're written to its analysis files
//! (`plugins.apply_analysis`).

use std::path::Path;

use base64::prelude::*;
use serde::{Deserialize, Serialize};

use super::{Analyzer, is_name};
use crate::anlz::{Bands, Beat, DETAIL_RATE, SongStructure, TrackAnalysis};
use crate::audio_cache::{CACHE_DIR, Cache, Kind};
use crate::store::Store;
use crate::{Error, Result};

/// One analyzer's structured outputs for one audio file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    pub beats: Option<Vec<Beat>>,
    pub phrases: Option<SongStructure>,
    /// At [`crate::anlz::DETAIL_RATE`] columns a second.
    pub waveform: Option<Bands>,
}

impl Overlay {
    pub fn is_empty(&self) -> bool {
        self.beats.is_none() && self.phrases.is_none() && self.waveform.is_none()
    }
}

/// [`Overlay`] as its files hold it: the waveform packed, in base64.
#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    beats: Option<Vec<Beat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phrases: Option<SongStructure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    waveform: Option<String>,
}

impl Kind for Overlay {
    const DIR: &'static str = "overlay";
    const EXT: &'static str = "json";
    const WHAT: &'static str = "a plugin's analysis";
    /// The files' own format; the analyzer's version is in their folder.
    const VERSION: u32 = 1;
    type Value = Overlay;

    fn encode(o: &Overlay) -> Vec<u8> {
        let waveform = o.waveform.as_ref().map(|b| BASE64_STANDARD.encode(b.pack()));
        let stored = Stored { beats: o.beats.clone(), phrases: o.phrases.clone(), waveform };
        serde_json::to_vec(&stored).expect("an overlay serialises")
    }

    fn decode(bytes: &[u8]) -> Option<Overlay> {
        let s: Stored = serde_json::from_slice(bytes).ok()?;
        let waveform = match s.waveform {
            Some(text) => Some(Bands::unpack(&BASE64_STANDARD.decode(text).ok()?)?),
            None => None,
        };
        Some(Overlay { beats: s.beats, phrases: s.phrases, waveform })
    }
}

/// What every analyzer plugin worked out besides its columns, `<store>/cache/overlay`.
pub struct OverlayStore {
    root: std::path::PathBuf,
}

impl OverlayStore {
    pub fn in_store(store: &Store) -> OverlayStore {
        OverlayStore { root: store.root().join(CACHE_DIR).join(Overlay::DIR) }
    }

    fn of(&self, a: &Analyzer) -> Cache<Overlay> {
        Cache::new(self.root.join(&a.plugin).join(&a.analyzer).join(&a.version))
    }

    /// Keep what `a` worked out for the audio file at `audio`.
    pub fn put(&self, audio: &Path, a: &Analyzer, o: &Overlay) -> Result<()> {
        if let Some(bad) = [&a.plugin, &a.analyzer, &a.version].into_iter().find(|s| !is_name(s)) {
            return Err(Error::invalid(format!("{bad:?} isn't a plugin id, version or analyzer")));
        }
        let key = Cache::<Overlay>::key(audio).ok_or_else(|| Error::NotFound(audio.display().to_string()))?;
        self.of(a).put(&key, o)
    }

    /// What `a` worked out for the audio file at `audio` as it is now, if it has.
    pub fn get(&self, audio: &Path, a: &Analyzer) -> Option<Overlay> {
        self.of(a).get(&Cache::<Overlay>::key(audio)?)
    }

    /// For drawing: each part from the first of `analyzers` with one for the audio file at
    /// `audio`, and that analyzer.
    pub fn first(&self, audio: &Path, analyzers: &[Analyzer]) -> Shown {
        let mut shown = Shown::default();
        for a in analyzers {
            let Some(o) = self.get(audio, a) else { continue };
            let from = || Some(a.clone());
            if shown.overlay.beats.is_none() && o.beats.is_some() {
                (shown.overlay.beats, shown.beats_from) = (o.beats.clone(), from());
            }
            if shown.overlay.phrases.is_none() && o.phrases.is_some() {
                (shown.overlay.phrases, shown.phrases_from, shown.phrases_grid) = (o.phrases, from(), o.beats.clone());
            }
            if shown.overlay.waveform.is_none() && o.waveform.is_some() {
                (shown.overlay.waveform, shown.waveform_from) = (o.waveform, from());
            }
        }
        shown
    }
}

/// An overlay put together from several analyzers, with the one each part is from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    pub overlay: Overlay,
    pub beats_from: Option<Analyzer>,
    pub phrases_from: Option<Analyzer>,
    /// The grid of the analyzer the phrases are from, whose beats they count.
    pub phrases_grid: Option<Vec<Beat>>,
    pub waveform_from: Option<Analyzer>,
}

impl Shown {
    /// Fills in what a track's analysis from its drive (`a`) lacks: its grid, its phrases (only
    /// on the grid they count), its waveforms. What it filled in, as a line to show
    /// (`grid and waveform from echo.heard, not on the drive`), if anything.
    pub fn fill(self, a: &mut TrackAnalysis) -> Option<String> {
        let mut filled: Vec<(&str, Analyzer)> = Vec::new();
        if let (Some(beats), Some(from), true) = (self.overlay.beats, self.beats_from, a.beats.is_empty()) {
            a.beats = beats;
            filled.push(("grid", from));
        }
        let counts = self.phrases_grid.as_deref().is_none_or(|own| crate::plugin::same_grid(own, &a.beats));
        if let (Some(phrases), Some(from), true) =
            (self.overlay.phrases, self.phrases_from, a.structure.is_none() && counts)
        {
            a.structure = Some(phrases);
            filled.push(("phrases", from));
        }
        if let (Some(bands), Some(from)) = (self.overlay.waveform, self.waveform_from)
            && (a.preview.is_none() || a.detail.is_none())
        {
            a.preview = a.preview.take().or_else(|| Some(bands.pooled(1200).waveform(None)));
            a.detail = a.detail.take().or_else(|| Some(bands.waveform(Some(DETAIL_RATE))));
            filled.push(("waveform", from));
        }
        if filled.is_empty() {
            return None;
        }
        let (parts, mut from): (Vec<&str>, Vec<String>) =
            filled.into_iter().map(|(part, a)| (part, format!("{}.{}", a.plugin, a.analyzer))).unzip();
        from.dedup();
        let from: Vec<&str> = from.iter().map(String::as_str).collect();
        Some(format!("{} from {}, not on the drive", crate::op::and_list(&parts), crate::op::and_list(&from)))
    }
}
