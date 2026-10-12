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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Device;
    use hegel::TestCase;
    use hegel::generators as gs;
    use loclib_model::test_util::track;

    /// What the datasets name a key: `C minor`.
    fn spelled(key: Camelot) -> String {
        let tonic = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][key.pitch() as usize];
        format!("{tonic} {}", if key.major { "major" } else { "minor" })
    }

    /// Ids are file names up to the first dot, in any folder: a name made of an id and dotted
    /// suffixes reads as that id, and one with nothing before its first dot has none.
    #[hegel::test(test_cases = 300)]
    fn ids_are_names_up_to_the_first_dot(tc: TestCase) {
        let named = [
            ("audio/1030011.LOFI.mp3", Some("1030011")),
            (r"C:\gs\5.LOFI.key", Some("5")),
            ("a.b/c", Some("c")),
            ("key/", None),
            (".LOFI.key", None),
        ];
        for (path, id) in named {
            assert_eq!(id_of(path).as_deref(), id, "{path:?}");
        }
        let folders: Vec<String> = tc.draw(gs::vecs(gs::text().alphabet("ab.c ").max_size(5)).max_size(3));
        let id = tc.draw(gs::text().alphabet("0123ab ").max_size(8));
        let suffixes: Vec<String> = tc.draw(gs::vecs(gs::text().alphabet("LOFImp3.").max_size(5)).max_size(3));
        let mut path = String::new();
        for f in &folders {
            path += f;
            path += tc.draw(gs::sampled_from(vec!["/", "\\"]));
        }
        path += &id;
        for s in &suffixes {
            path += &format!(".{s}");
        }
        assert_eq!(id_of(&path), (!id.is_empty()).then_some(id), "{path:?}");
    }

    /// Any annotations, drive and audio: rekordbox is scored on every annotated track the drive
    /// has, and the estimates on every annotated track there's audio for (the ones that fail to
    /// decode as misses); the rest are unmatched. Without annotations, or anything to score,
    /// there's nothing to do.
    #[hegel::test(test_cases = 8)]
    fn scores_count_every_annotated_track_once(tc: TestCase) {
        let dir = tempfile::tempdir().unwrap();
        let (root, folder, keys_dir, tempos_dir) =
            (dir.path().join("drive"), dir.path().join("audio"), dir.path().join("key"), dir.path().join("tempo"));
        for d in [root.join("Contents"), folder.clone(), keys_dir.clone(), tempos_dir.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        let key =
            |tc: &TestCase| Camelot::from_pitch(tc.draw(gs::integers::<u32>().max_value(11)), tc.draw(gs::booleans()));
        let in_folder = tc.draw(gs::booleans());
        let (mtg, with_tempos) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
        // Files that aren't annotations are passed over.
        std::fs::write(keys_dir.join("README.md"), "GiantSteps").unwrap();
        let (mut tracks, mut table) = (Vec::new(), vec!["ID\tMANUAL KEY\tC".to_string()]);
        let (mut keyed, mut timed, mut audible, mut on_drive, mut broken) = (vec![], vec![], vec![], vec![], vec![]);
        for i in 0..tc.draw(gs::integers::<u32>().min_value(1).max_value(5)) {
            let id = format!("10{i}");
            let annotated_key = tc.draw(gs::optional(gs::just(()))).map(|_| key(&tc));
            if let Some(k) = annotated_key {
                keyed.push(id.clone());
                match mtg {
                    true => table.push(format!("{id}\t{}\t2", spelled(k))),
                    false => std::fs::write(keys_dir.join(format!("{id}.LOFI.key")), spelled(k)).unwrap(),
                }
            } else if mtg {
                // Annotated with too little confidence to count.
                table.push(format!("{id}\t{}\t1", spelled(key(&tc))));
            }
            if with_tempos && tc.draw(gs::booleans()) {
                timed.push(id.clone());
                std::fs::write(tempos_dir.join(format!("{id}.LOFI.bpm")), "128.0\n").unwrap();
            }
            let file = format!("{id}.LOFI.wav");
            if tc.draw(gs::booleans()) {
                on_drive.push(id.clone());
                let mut t = track(i, "A", &id, &format!("/Contents/{file}"));
                t.key = tc.draw(gs::optional(gs::just(()))).map_or(String::new(), |_| key(&tc).note());
                tracks.push(t);
            }
            let has_audio = tc.draw(gs::weighted_booleans(0.3)) && (in_folder || on_drive.contains(&id));
            if has_audio {
                let at = if in_folder { folder.join(&file) } else { root.join("Contents").join(&file) };
                let bad = tc.draw(gs::weighted_booleans(0.3));
                let wav = if bad {
                    loclib_mixid::test_util::wav(0, 16, 0.0)
                } else {
                    loclib_mixid::test_util::wav(44_100, 16, 0.2)
                };
                std::fs::write(at, wav).unwrap();
                audible.push(id.clone());
                if bad {
                    broken.push(id.clone());
                }
            }
        }
        if mtg {
            std::fs::write(keys_dir.join("annotations.txt"), table.join("\n")).unwrap();
        }
        let opts = BenchOptions {
            keys: Some(if mtg { keys_dir.join("annotations.txt") } else { keys_dir }),
            tempos: with_tempos.then_some(tempos_dir),
            audio: in_folder.then_some(folder),
        };
        let device = Device::open(&root).unwrap();
        let library = Library { tracks, ..Default::default() };
        let cache = Cache::new(dir.path().join("cache"));
        let result = bench(&opts, Some((&device, &library)), &cache, &Ctx::silent());
        if keyed.is_empty() && timed.is_empty() {
            let err = result.unwrap_err();
            return assert!(err.to_string().contains("no key or tempo annotations"), "{err}");
        }
        let r = result.unwrap();
        let both = |a: &[String], b: &[String]| a.iter().filter(|id| b.contains(id)).count();
        let tracks = |reports: &IndexMap<String, Report<KeyOutcome>>, name| reports.get(name).map_or(0, |r| r.tracks);
        assert_eq!(tracks(&r.keys, REKORDBOX), both(&keyed, &on_drive));
        assert_eq!(tracks(&r.keys, ESTIMATE), both(&keyed, &audible));
        let tempo_tracks = |name| r.tempos.get(name).map_or(0, |r| r.tracks);
        assert_eq!(
            (tempo_tracks(REKORDBOX), tempo_tracks(ESTIMATE)),
            (both(&timed, &on_drive), both(&timed, &audible))
        );
        assert!(r.accuracy2.keys().eq(r.tempos.keys()));
        let annotated: Vec<String> =
            keyed.iter().chain(&timed).cloned().collect::<FxHashSet<_>>().into_iter().collect();
        assert_eq!(r.unmatched, annotated.len() - both(&annotated, &audible));
        let mut failed: Vec<String> = r.failed.iter().filter_map(|(name, _)| id_of(name)).collect();
        failed.sort();
        let mut expected: Vec<String> = broken.into_iter().filter(|id| annotated.contains(id)).collect();
        expected.sort();
        assert_eq!(failed, expected);
        // The stored tempo, 128, agrees with the annotation wherever there's one to score.
        assert!(r.tempos.get(REKORDBOX).is_none_or(|t| t.count(TempoOutcome::Correct) == t.tracks));

        let nowhere = BenchOptions { audio: None, ..opts.clone() };
        assert!(bench(&nowhere, None, &cache, &Ctx::silent()).unwrap_err().to_string().contains("give a drive"));
    }
}
