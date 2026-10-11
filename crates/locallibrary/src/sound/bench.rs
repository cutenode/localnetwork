//! Scoring rekordbox's stored keys and tempos, and loclib-analysis's estimates, against the
//! GiantSteps datasets' annotations (see [`loclib_analysis::benchmark`] for their formats).
//!
//! Tracks are matched to annotations by the start of their file name, up to the first dot:
//! GiantSteps names its previews `<id>.LOFI.mp3` and its annotations `<id>.LOFI.key` (or lists
//! `<id>` in the MTG dataset's table), and rekordbox keeps a file's name when it exports it.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use loclib_analysis::benchmark::{self, Report, Row, TempoOutcome};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Analyses, Cache, KeyOutcome};
use crate::audio_cache::{self, Audio};
use crate::harmony::Camelot;
use crate::model::Library;
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// The name rekordbox's stored values are scored under.
pub const REKORDBOX: &str = "rekordbox";
/// The name loclib-analysis's estimates are scored under: the analyzer it runs.
pub const ESTIMATE: &str = "stratum-dsp";

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct BenchOptions {
    /// Key annotations: GiantSteps Key's `annotations/key` folder (`<id>.LOFI.key` files), or the
    /// GiantSteps MTG Key dataset's `annotations.txt` (its tracks with one key at confidence 2).
    #[serde(default)]
    pub keys: Option<PathBuf>,
    /// Tempo annotations: GiantSteps Tempo's `annotations/tempo` or `annotations_v2/tempo` folder
    /// (`<id>.LOFI.bpm` files).
    #[serde(default)]
    pub tempos: Option<PathBuf>,
    /// The datasets' audio to analyse. Without it, the drive's tracks are.
    #[serde(default)]
    pub audio: Option<PathBuf>,
}

/// Each analyzer's scores.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct BenchReport {
    /// Key reports by analyzer (`rekordbox`, `stratum-dsp`): `score` is MIREX's weighted score.
    pub keys: IndexMap<String, Report<KeyOutcome>>,
    /// Tempo reports by analyzer: `score` is Accuracy1.
    pub tempos: IndexMap<String, Report<TempoOutcome>>,
    /// Tempo Accuracy2 (percent) by analyzer.
    pub accuracy2: IndexMap<String, f64>,
    /// Annotated tracks the analysis had no audio for: not on the drive, missing from it, or not in
    /// the audio folder.
    pub unmatched: usize,
    /// Audio files that couldn't be analysed: `[name, error]`.
    pub failed: Vec<(String, String)>,
}

/// A file's id in the datasets: its name up to the first dot.
fn id_of(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?;
    let id = name.split('.').next()?;
    (!id.is_empty()).then(|| id.to_string())
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| Error::Io { path: path.to_path_buf(), source: e })
}

/// The files in `dir` with extension `ext`, by id, parsed with `parse` (`None`: not annotated).
fn annotations<T>(
    dir: &Path,
    ext: &str,
    parse: impl Fn(&str) -> loclib_analysis::Result<Option<T>>,
) -> Result<FxHashMap<String, T>> {
    let mut out = FxHashMap::default();
    for entry in std::fs::read_dir(dir).map_err(|e| Error::Io { path: dir.to_path_buf(), source: e })? {
        let path = entry.map_err(|e| Error::Io { path: dir.to_path_buf(), source: e })?.path();
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        let Some(id) = path.file_name().and_then(|n| id_of(&n.to_string_lossy())) else { continue };
        let parsed = parse(&read(&path)?).map_err(|e| Error::parse(&path, e))?;
        out.extend(parsed.map(|v| (id, v)));
    }
    Ok(out)
}

fn key_annotations(path: &Path) -> Result<FxHashMap<String, Camelot>> {
    if path.is_dir() {
        return annotations(path, "key", |text| benchmark::parse_key(text).map(Some));
    }
    let rows = benchmark::parse_mtg_keys(&read(path)?).map_err(|e| Error::parse(path, e))?;
    Ok(rows.into_iter().filter(|r| r.confidence == 2 && r.keys.len() == 1).map(|r| (r.id, r.keys[0])).collect())
}

/// Score the drive's stored values (`library`, when given) and the analyses of the audio (in
/// `opts.audio`, or the drive's tracks on `device`) against the annotations.
pub fn bench(
    opts: &BenchOptions,
    drive: Option<(&crate::Device, &Library)>,
    cache: &Cache,
    ctx: &Ctx<'_>,
) -> Result<BenchReport> {
    let keys = opts.keys.as_deref().map(key_annotations).transpose()?.unwrap_or_default();
    let tempos = match &opts.tempos {
        Some(dir) => annotations(dir, "bpm", benchmark::parse_tempo)?,
        None => FxHashMap::default(),
    };
    if keys.is_empty() && tempos.is_empty() {
        return Err(Error::invalid("no key or tempo annotations were found"));
    }
    let annotated: FxHashSet<&String> = keys.keys().chain(tempos.keys()).collect();

    // Each annotated track's stored values on the drive, and its audio.
    let stored: FxHashMap<String, &crate::Track> = drive
        .map(|(_, l)| l.tracks.iter().filter_map(|t| Some((id_of(&t.file_path)?, t))).collect())
        .unwrap_or_default();
    let audio: Vec<(String, PathBuf)> = match (&opts.audio, drive) {
        (Some(dir), _) => walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && loclib_mixid::audio::is_decodable(e.path()))
            .filter_map(|e| Some((id_of(&e.file_name().to_string_lossy())?, e.into_path())))
            .collect(),
        (None, Some((device, _))) => stored
            .iter()
            .filter_map(|(id, t)| Some((id.clone(), device.checked_path(&crate::paths::key(&t.file_path)).ok()?)))
            .collect(),
        (None, None) => return Err(Error::invalid("give a drive or a folder of audio to score")),
    };
    let audio: Vec<(String, PathBuf)> = audio.into_iter().filter(|(id, _)| annotated.contains(id)).collect();
    let files: Vec<Audio> = audio
        .iter()
        .enumerate()
        .map(|(i, (_, path))| Audio { id: i as u32, path: path.clone(), name: path.display().to_string(), bytes: 0 })
        .collect();
    let run = audio_cache::for_files::<Analyses>(&files, cache, Phase::Analyze, super::analyze_file, ctx)?;
    let estimates: FxHashMap<&String, &loclib_analysis::Estimate> =
        run.done.iter().filter_map(|d| Some((&audio[d.id as usize].0, d.value.estimate.as_ref()?))).collect();
    // Tracks the analysis had audio for (it may have failed on some): those whose file is missing
    // are left out of its scores rather than counted as its misses.
    let scored: FxHashSet<&String> = run
        .done
        .iter()
        .map(|d| d.id)
        .chain(run.failed.iter().map(|(id, _)| *id))
        .map(|i| &audio[i as usize].0)
        .collect();

    // Rows in the annotations' id order, so reports list misses the same way every run.
    let mut ids: Vec<&String> = annotated.iter().copied().collect();
    ids.sort();
    let rows = |analyzer: &str, have: &dyn Fn(&String) -> bool| -> Vec<(String, String)> {
        ids.iter().filter(|id| have(id)).map(|id| (analyzer.to_string(), (*id).clone())).collect()
    };
    let mut key_rows = Vec::new();
    let mut tempo_rows = Vec::new();
    let has_drive = |id: &String| stored.contains_key(id);
    let has_audio = |id: &String| scored.contains(id);
    for (analyzer, id) in rows(REKORDBOX, &has_drive).into_iter().chain(rows(ESTIMATE, &has_audio)) {
        let rekordbox = analyzer == REKORDBOX;
        if let Some(reference) = keys.get(&id) {
            let estimate = match rekordbox {
                true => Camelot::parse(&stored[&id].key),
                false => estimates.get(&id).map(|e| e.key),
            };
            key_rows.push(Row { analyzer: analyzer.clone(), track: id.clone(), reference: *reference, estimate });
        }
        if let Some(reference) = tempos.get(&id) {
            let estimate = match rekordbox {
                true => Some(stored[&id].bpm).filter(|b| *b > 0.0),
                false => estimates.get(&id).map(|e| f64::from(e.bpm)),
            };
            tempo_rows.push(Row { analyzer, track: id, reference: *reference, estimate });
        }
    }
    let tempos = benchmark::score::<TempoOutcome>(tempo_rows);
    Ok(BenchReport {
        keys: benchmark::score::<KeyOutcome>(key_rows),
        accuracy2: tempos.iter().map(|(a, r)| (a.clone(), r.accuracy2())).collect(),
        tempos,
        unmatched: annotated.iter().filter(|id| !has_audio(id)).count(),
        failed: run.failed.into_iter().map(|(i, e)| (audio[i as usize].1.display().to_string(), e)).collect(),
    })
}
