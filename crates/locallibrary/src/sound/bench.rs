//! Scoring rekordbox's stored keys and tempos, and what the analyzers chosen for the `key` and
//! `bpm` roles hear (see [`crate::fields::roles`]), against the GiantSteps datasets' annotations
//! (see [`loclib_analysis::benchmark`] for their formats).
//!
//! Tracks are matched to annotations by the start of their file name, up to the first dot:
//! GiantSteps names its previews `<id>.LOFI.mp3` and its annotations `<id>.LOFI.key` (or lists
//! `<id>` in the MTG dataset's table), and rekordbox keeps a file's name when it exports it.
//!
//! The analyzers measure the audio every run (see [`crate::plugin::Plugins::measure`]): the
//! benchmark scores them as they are now, and keeps nothing.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use loclib_analysis::benchmark::{self, Report, Row, TempoOutcome};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{KeyOutcome, heard_key, number};
use crate::fields::roles::{self, RoleField, RoleValues};
use crate::fields::{Analyzer, Fields, Role};
use crate::harmony::Camelot;
use crate::model::Library;
use crate::plugin::Audio;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

/// The name rekordbox's stored values are scored under. The analyzers' are their own,
/// `<plugin>.<analyzer>`.
pub const REKORDBOX: &str = "rekordbox";

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
    /// Key reports by analyzer (`rekordbox`, `sound.tempo-key-loudness`): `score` is MIREX's
    /// weighted score.
    pub keys: IndexMap<String, Report<KeyOutcome>>,
    /// Tempo reports by analyzer: `score` is Accuracy1.
    pub tempos: IndexMap<String, Report<TempoOutcome>>,
    /// Tempo Accuracy2 (percent) by analyzer.
    pub accuracy2: IndexMap<String, f64>,
    /// Annotated tracks the analysis had no audio for: not on the drive, missing from it, or not in
    /// the audio folder.
    pub unmatched: usize,
    /// Audio files that couldn't be analysed: `[name, error]`, once for each analyzer that failed
    /// on it.
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

/// `analyzer`'s rows, in `ids`' order: one for each track `annotations` annotate that `estimate`
/// scores (`None`: left out), with what it estimated.
fn rows<T: Copy>(
    analyzer: &str,
    ids: &[&String],
    annotations: &FxHashMap<String, T>,
    estimate: impl Fn(&String) -> Option<Option<T>>,
) -> Vec<Row<T>> {
    let row = |id: &&String| {
        let reference = *annotations.get(*id)?;
        Some(Row { analyzer: analyzer.into(), track: (*id).clone(), reference, estimate: estimate(id)? })
    };
    ids.iter().filter_map(row).collect()
}

/// Score the drive's stored values (`library`, when given) and what the analyzers chosen in
/// `store` for the roles there are annotations of (`key`, `bpm`) hear in the audio (in
/// `opts.audio`, or the drive's tracks on `device` whose audio is there) against the annotations.
/// `measure` runs an analyzer on the audio, keeping nothing (as [`crate::plugin::Plugins::measure`]
/// does): each file's fields, or why it failed, in the order handed. Without an analyzer for those
/// roles, rekordbox is scored alone.
pub fn bench(
    opts: &BenchOptions,
    drive: Option<(&crate::Device, &Library)>,
    store: &Store,
    measure: impl Fn(&Analyzer, Audio<'_>) -> Result<Vec<Result<Fields>>>,
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
            .filter(|(_, path)| path.is_file())
            .collect(),
        (None, None) => return Err(Error::invalid("give a drive or a folder of audio to score")),
    };
    let audio: Vec<(String, PathBuf)> = audio.into_iter().filter(|(id, _)| annotated.contains(id)).collect();

    // The analyzers chosen for the roles there are annotations of.
    let annotates = [(Role::Key, !keys.is_empty()), (Role::Bpm, !tempos.is_empty())];
    let wanted: Vec<Role> = annotates.into_iter().filter(|(_, any)| *any).map(|(role, _)| role).collect();
    let chosen: Vec<(Role, RoleField)> =
        wanted.iter().filter_map(|&role| Some((role, roles::chosen(Some(store), role)?))).collect();
    if chosen.is_empty() {
        let named: Vec<&str> = wanted.iter().map(|r| r.name()).collect();
        let none = format!("no plugin works out {} (`locallibrary plugin` lists what they do)", named.join(", "));
        match drive {
            Some(_) => ctx.warn(format!("{none}: only rekordbox is scored")),
            None => return Err(Error::invalid(none)),
        }
    }
    let mut analyzers: Vec<&Analyzer> = Vec::new();
    for (_, f) in &chosen {
        if !analyzers.contains(&&f.analyzer) {
            analyzers.push(&f.analyzer);
        }
    }

    // Each analyzer measures the audio: the drive's tracks as they're on it, or the folder's files.
    let files: Vec<PathBuf> = audio.iter().map(|(_, path)| path.clone()).collect();
    let tracks: Vec<u32> = match &opts.audio {
        Some(_) => Vec::new(),
        None => audio.iter().map(|(id, _)| stored[id].id).collect(),
    };
    let root = drive.map(|(device, _)| device.root().display().to_string());
    let handed = match (&opts.audio, &root) {
        (None, Some(root)) => Audio::Tracks { drive: root, tracks: &tracks },
        _ => Audio::Files(&files),
    };
    // What they heard in each file, by id, as the roles read it.
    let mut heard: FxHashMap<&String, RoleValues> = FxHashMap::default();
    let mut failed = Vec::new();
    for analyzer in analyzers.into_iter().filter(|_| !audio.is_empty()) {
        let answers = measure(analyzer, handed)?;
        for ((id, path), answer) in audio.iter().zip(answers) {
            match answer {
                Ok(fields) => {
                    let values = heard.entry(id).or_default();
                    for (role, f) in chosen.iter().filter(|(_, f)| f.analyzer == *analyzer) {
                        values.extend(fields.get(&f.field).map(|v| (*role, v.clone())));
                    }
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => failed.push((path.display().to_string(), e.to_string())),
            }
        }
    }
    // Tracks there was audio for (an analyzer may have failed on some): those whose file is
    // missing are left out of the analyzers' scores rather than counted as their misses.
    let scored: FxHashSet<&String> = audio.iter().map(|(id, _)| id).collect();

    // Rows in the annotations' id order, so reports list misses the same way every run; rekordbox
    // first.
    let mut ids: Vec<&String> = annotated.iter().copied().collect();
    ids.sort();
    let by = |role| chosen.iter().find(|(r, _)| *r == role).map(|(_, f)| f.name());
    let heard_of = |id: &String| scored.contains(id).then(|| heard.get(id));
    let mut key_rows = rows(REKORDBOX, &ids, &keys, |id| Some(Camelot::parse(&stored.get(id)?.key)));
    if let Some(by) = by(Role::Key) {
        key_rows.extend(rows(&by, &ids, &keys, |id| heard_of(id).map(|h| h.and_then(heard_key))));
    }
    let mut tempo_rows = rows(REKORDBOX, &ids, &tempos, |id| Some(Some(stored.get(id)?.bpm).filter(|b| *b > 0.0)));
    if let Some(by) = by(Role::Bpm) {
        let tempo = |h: &RoleValues| number(h, Role::Bpm).filter(|b| *b > 0.0);
        tempo_rows.extend(rows(&by, &ids, &tempos, |id| heard_of(id).map(|h| h.and_then(tempo))));
    }
    let tempos = benchmark::score::<TempoOutcome>(tempo_rows);
    Ok(BenchReport {
        keys: benchmark::score::<KeyOutcome>(key_rows),
        accuracy2: tempos.iter().map(|(a, r)| (a.clone(), r.accuracy2())).collect(),
        tempos,
        unmatched: annotated.iter().filter(|id| !scored.contains(*id)).count(),
        failed,
    })
}

#[cfg(test)]
mod tests;
