//! Upgrading lossy tracks to lossless: each MP3 or AAC track whose recording is among the
//! lossless files in some folders (a FLAC, WAV, AIFF or Apple Lossless bought since) gets that
//! file on the drive and is pointed at it, keeping its row and so its playlists, history, My
//! Tags, rating and play count.
//!
//! Files are matched by their sound, as `doctor`'s duplicates by sound are ([`doctor`]): the
//! store's fingerprint cache, lengths within `doctor::SAME_LENGTH_SECS` (so an edit never
//! stands in for a mix) and prints that line up over `doctor::SAME_RECORDING_COVERAGE` of each.
//! A lossless file's audio starts at another point than the MP3 made from it (the encoder's
//! delay), so the cues and beat grid move by the offset between the two, measured to the sample
//! (`doctor::measured_offset`); a track whose offset can't be measured is left as it is.
//!
//! The lossless file is copied next to the old one and the edit is one [`Edit::SetPath`] per
//! track, written through one backup (so `undo` takes it back) and recorded, so `edits replay`
//! points the track at it again after rekordbox exports the old file's row. The old file stays
//! unless asked to go (backed up too). The detail waveforms move with the cues; the rest of the
//! analysis stays as it was.
//!
//! The offset is between the two files as rekordbox times them: an MP3 from its first decoded
//! frame, untrimmed, or for some MP3s a frame before it, told apart by the track's waveform, and
//! AAC measured the same way. An AAC track without a waveform to measure against is left as it is.
//!
//! A lossless file the drive's players can't play (the players given, or remembered for its
//! library) isn't used: the track would stop loading.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::audio_cache::{self, Audio};
use crate::device::Device;
use crate::doctor::{self, SAME_LENGTH_SECS, SAME_RECORDING_COVERAGE};
use crate::edit::{self, AudioFile, Edit, EditOptions, EditPlan, EditReport};
use crate::equipment::{self, Codec, FromPath, Model};
use crate::incoming::{Incoming, Space};
use crate::mix::index::{Cache, fingerprint_library};
use crate::model::{Library, Track, TrackRef};
use crate::paths::{self, FileClass};
use crate::progress::{Ctx, Phase};
use crate::query::TrackSelector;
use crate::{Error, Result, probe, timeline};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct UpgradeOptions {
    /// Folders of lossless files (on this computer or another drive), with everything below them.
    pub folders: Vec<PathBuf>,
    /// Delete the lossy files the tracks used (they're backed up first).
    #[serde(default)]
    pub remove_old: bool,
    /// The players the drive is for: a lossless file one of them can't play isn't used. None: the
    /// ones remembered for the drive's library (none remembered: not checked).
    #[serde(default)]
    pub players: Vec<String>,
}

/// A track pointed at a lossless file of its recording.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Upgrade {
    pub track: TrackRef,
    /// The lossless file found, on this computer.
    pub file: PathBuf,
    /// Where it goes on the drive (a key): beside the track's file.
    pub to: String,
    pub size: u64,
    /// How much later its cues and beat grid fall in the new file (ms; earlier when negative).
    pub shift_ms: i32,
    /// The file is on the drive already (left by an upgrade since undone): it isn't copied.
    pub on_drive: bool,
}

/// A track with a lossless file of its recording that isn't upgraded, and why.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct NotUpgraded {
    pub track: TrackRef,
    pub file: PathBuf,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UpgradePlan {
    pub drive: String,
    pub root: PathBuf,
    pub upgrades: Vec<Upgrade>,
    pub not_upgraded: Vec<NotUpgraded>,
    /// Lossy tracks with no lossless file of their recording in the folders.
    pub unmatched: usize,
    /// Lossy files deleted (with `remove_old`), as keys.
    pub removed: Vec<String>,
    pub copy_bytes: u64,
    /// Space for the copies, the backup and the new databases.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
    #[serde(skip)]
    edit: EditPlan,
}

impl UpgradePlan {
    pub fn is_noop(&self) -> bool {
        self.upgrades.is_empty()
    }

    fn incoming(&self) -> Vec<Incoming> {
        self.upgrades
            .iter()
            .filter(|u| !u.on_drive)
            .map(|u| Incoming { from: u.file.clone(), to: u.to.clone(), size: u.size })
            .collect()
    }
}

/// Lossless files in `folders` and below them, by extension (an `.m4a`: by its header), leaving
/// out the drive's own library.
fn lossless_files(folders: &[PathBuf], device: &Device, ctx: &Ctx<'_>) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for folder in folders {
        for f in crate::incoming::audio_files(folder, device, ctx)? {
            let lossless = match Codec::from_path(&f.name) {
                FromPath::Codec(c) => c.lossless(),
                FromPath::AacOrAlac => probe::format(&f.path).is_ok_and(|p| p.codec == Some(Codec::Alac)),
                FromPath::Unknown => false,
            };
            if lossless {
                out.push(f.path);
            }
        }
    }
    Ok(out)
}

/// What a track's rows say about `file`, a lossless file, and its format.
fn audio_file(file: &Path, size: u64) -> Result<(AudioFile, Option<Codec>)> {
    let f = probe::format(file)?;
    let secs = f.duration_secs.unwrap_or(0.0);
    let bitrate = match f.codec {
        // rekordbox gives uncompressed audio its nominal rate, and compressed its average.
        Some(Codec::Wav | Codec::Aiff) => f.sample_rate * u32::from(f.bit_depth) * u32::from(f.channels) / 1000,
        _ if secs > 0.0 => (size as f64 * 8.0 / secs / 1000.0).round() as u32,
        _ => 0,
    };
    Ok((AudioFile { size, sample_rate: f.sample_rate, bit_depth: f.bit_depth, bitrate }, f.codec))
}

/// `name` made a file name players and FAT/exFAT take (Unicode NFC, no `:` `?` and the rest).
fn file_name(name: &str) -> String {
    paths::sanitize_component(&paths::key(name))
}

/// The first of `dir/stem.ext`, `dir/stem (2).ext`... that no track uses and that isn't on the
/// drive, or that is and holds `file`'s audio (an upgrade undone left it): the key, and whether
/// it's on the drive already.
fn free_key(
    device: &Device,
    dir: &str,
    name: &str,
    file: &Path,
    size: u64,
    taken: &FxHashSet<String>,
    ctx: &Ctx<'_>,
) -> Result<(String, bool)> {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    for n in 1.. {
        let candidate = match n {
            1 => name.to_string(),
            n => format!("{stem} ({n}).{ext}"),
        };
        let key = format!("{dir}/{candidate}");
        if taken.contains(&key.to_lowercase()) {
            continue;
        }
        let at = device.checked_path(&key)?;
        let Ok(meta) = std::fs::metadata(&at) else { return Ok((key, false)) };
        let same = meta.len() == size && {
            let hash = |p: &Path| crate::hash::hash_file(p, ctx, Phase::Hash);
            hash(&at)? == hash(file)?
        };
        if same {
            return Ok((key, true));
        }
    }
    unreachable!("some name is free")
}

/// Why the players can't play `file`, if one of them can't.
fn unplayable(players: &[&Model], codec: Option<Codec>, file: &AudioFile) -> Option<String> {
    let codec = codec?;
    let not = players.iter().find(|m| m.plays(codec, file.sample_rate, file.bit_depth).result.is_err())?;
    Some(format!(
        "the {} doesn't play it ({} at {} Hz, {} bit)",
        not.name,
        codec.name(),
        file.sample_rate,
        file.bit_depth
    ))
}

/// Which of the drive's lossy tracks have a lossless file of their recording in the folders, and
/// the edit that points them at it. A track that can't be upgraded is in `not_upgraded`, with
/// why; only cancelling, or what stops the whole plan (the folders, the drive), is an error.
pub fn plan(device: &Device, opts: &UpgradeOptions, ctx: &Ctx<'_>) -> Result<UpgradePlan> {
    if opts.folders.is_empty() {
        return Err(Error::invalid("name the folders of lossless files to look in")
            .because(crate::error::Reason::MissingParam)
            .with("param", "folders"));
    }
    let players: Vec<&Model> = equipment::canonical(&opts.players)?.iter().filter_map(|n| equipment::find(n)).collect();
    let library = device.library()?;
    let key = |t: &Track| paths::key(&t.file_path).to_lowercase();
    let mut rows: rustc_hash::FxHashMap<String, usize> = Default::default();
    for t in &library.tracks {
        *rows.entry(key(t)).or_default() += 1;
    }
    let lossy: Vec<Track> = library
        .tracks
        .iter()
        .filter(|t| Codec::from_path(&t.file_path) != FromPath::Unknown && !doctor::lossless(device, t))
        .cloned()
        .collect();
    let mut warnings = Vec::new();
    let files = lossless_files(&opts.folders, device, ctx)?;

    // Prints, through the store's cache: the tracks', then the files'.
    let (cache, _temp) = Cache::of_default_store()?;
    let lossy_lib = Library { tracks: lossy, ..Default::default() };
    let (prints, _) = fingerprint_library(device, &lossy_lib, &cache, ctx)?;
    let audio: Vec<Audio> = files
        .iter()
        .enumerate()
        .map(|(i, f)| Audio {
            id: i as u32,
            path: f.clone(),
            name: f.display().to_string(),
            bytes: std::fs::metadata(f).map_or(0, |m| m.len()),
        })
        .collect();
    let run = audio_cache::for_files(
        &audio,
        &cache,
        Phase::Fingerprint,
        |f, ctx| loclib_mixid::fingerprint_file(f, ctx).map_err(Error::from),
        ctx,
    )?;
    if let Some((id, e)) = run.failed.first() {
        warnings.push(format!(
            "{} file(s) couldn't be decoded, so they weren't compared (e.g. {}: {e})",
            run.failed.len(),
            files[*id as usize].display()
        ));
    }

    let mut upgrades = Vec::new();
    let mut edits = Vec::new();
    let mut not_upgraded = Vec::new();
    let mut unmatched = 0;
    let mut taken: FxHashSet<String> = rows.keys().cloned().collect();
    for p in &prints {
        ctx.check()?;
        let t = lossy_lib.track(p.id).expect("printed from it");
        // The file of this recording whose prints line up over most of both.
        let best = run
            .done
            .iter()
            .filter(|f| (f.value.duration_secs - p.print.duration_secs).abs() <= SAME_LENGTH_SECS)
            .filter_map(|f| loclib_mixid::matcher::align(&p.print, &f.value).map(|al| (f, al)))
            .filter(|(_, al)| al.coverage.0 >= SAME_RECORDING_COVERAGE && al.coverage.1 >= SAME_RECORDING_COVERAGE)
            .max_by(|(_, a), (_, b)| a.coverage.0.min(a.coverage.1).total_cmp(&b.coverage.0.min(b.coverage.1)));
        let Some((f, al)) = best else {
            unmatched += 1;
            continue;
        };
        let file = files[f.id as usize].clone();
        let found = Found { track: t, file: &file, coarse: al, rows: rows[&key(t)] };
        match upgrade(device, &found, &players, &taken, ctx) {
            Ok(Ok((u, edit))) => {
                taken.insert(u.to.to_lowercase());
                upgrades.push(u);
                edits.push(edit);
            }
            Ok(Err(why)) => not_upgraded.push(NotUpgraded { track: t.into(), file, why }),
            Err(e) if matches!(e.inner(), Error::Cancelled) => return Err(e),
            Err(e) => not_upgraded.push(NotUpgraded { track: t.into(), file, why: e.to_string() }),
        }
    }

    let incoming: Vec<String> = upgrades.iter().map(|u| u.to.clone()).collect();
    let mut edit = edit::plan_writing(device, &edits, &EditOptions::default(), &incoming, ctx)?;
    let removed: Vec<String> = match opts.remove_old {
        true => upgrades.iter().map(|u| paths::key(&u.track.file_path)).collect(),
        false => Vec::new(),
    };
    edit.files.extend(removed.iter().cloned());
    edit.changes
        .extend(upgrades.iter().filter(|u| !u.on_drive).map(|u| format!("copy {} to {}", u.file.display(), u.to)));
    warnings.extend(edit.warnings.iter().cloned());
    let mut plan = UpgradePlan {
        drive: device.name(),
        root: device.root().to_path_buf(),
        upgrades,
        not_upgraded,
        unmatched,
        removed,
        copy_bytes: 0,
        needed_bytes: 0,
        free_space: None,
        fits: true,
        warnings,
        edit,
    };
    let incoming = plan.incoming();
    plan.copy_bytes = incoming.iter().map(|i| i.size).sum();
    // The old files go into the backup before they're deleted.
    let backed_up = plan.removed.iter().map(|k| std::fs::metadata(device.path(k)).map_or(0, |m| m.len())).sum();
    let Space { needed_bytes, free_space, fits } =
        crate::incoming::space(device, &incoming, &plan.edit, backed_up, &mut plan.warnings);
    (plan.needed_bytes, plan.free_space, plan.fits) = (needed_bytes, free_space, fits);
    Ok(plan)
}

/// A lossy track and the lossless file found for it.
struct Found<'a> {
    track: &'a Track,
    file: &'a Path,
    /// How their prints line up.
    coarse: loclib_mixid::matcher::Alignment,
    /// Rows the track's file has.
    rows: usize,
}

/// `found` as an upgrade and its edit, or why it isn't one.
fn upgrade(
    device: &Device,
    found: &Found<'_>,
    players: &[&Model],
    taken: &FxHashSet<String>,
    ctx: &Ctx<'_>,
) -> Result<std::result::Result<(Upgrade, Edit), String>> {
    let t = found.track;
    if found.rows > 1 {
        return Ok(Err("another row of the database has its file, and the two can't be told apart".into()));
    }
    if !found.coarse.is_precise() {
        return Ok(Err("the offset between them couldn't be estimated".into()));
    }
    let key = paths::key(&t.file_path);
    let dir = match key.rsplit_once('/') {
        Some((dir, _)) if paths::classify(&key) == Some(FileClass::Audio) => dir.to_string(),
        _ => return Ok(Err("its file isn't under Contents/, where locallibrary writes".into())),
    };
    let name = found.file.file_name().map(|n| file_name(&n.to_string_lossy())).unwrap_or_default();
    let size =
        std::fs::metadata(found.file).map_err(|e| Error::Io { path: found.file.to_path_buf(), source: e })?.len();
    let (facts, codec) = audio_file(found.file, size)?;
    if let Some(why) = unplayable(players, codec, &facts) {
        return Ok(Err(why));
    }
    let Some(lead) = timeline::lead(device, t, ctx)? else { return Ok(Err(timeline::UNTOLD.into())) };
    let from = device.checked_path(&key)?;
    let shift_ms = match doctor::measured_offset(&from, found.file, found.coarse.offset, ctx)? {
        Ok(secs) => ((secs + lead) * 1000.0).round() as i32,
        Err(why) => return Ok(Err(why.into())),
    };
    let (to, on_drive) = free_key(device, &dir, &name, found.file, size, taken, ctx)?;
    // Cues move in the analysis files and OneLibrary together, or not at all.
    let mut unreadable = Vec::new();
    edit::analysis_pointed_at(device, &t.analyze_path, &paths::device_path(&to), shift_ms, &mut unreadable)?;
    if !unreadable.is_empty() {
        return Ok(Err(format!("its analysis can't be rewritten ({})", unreadable.join("; "))));
    }
    let edit = Edit::SetPath {
        track: TrackSelector::paths([t.file_path.clone()]),
        path: paths::device_path(&to),
        file: Some(facts),
        shift_ms,
    };
    let upgrade = Upgrade { track: t.into(), file: found.file.to_path_buf(), to, size, shift_ms, on_drive };
    Ok(Ok((upgrade, edit)))
}

/// Copy the lossless files onto the drive and point the tracks at them, through one backup.
pub fn apply(plan: &UpgradePlan, device: &mut Device, ctx: &Ctx<'_>) -> Result<EditReport> {
    let space = Space { needed_bytes: plan.needed_bytes, free_space: plan.free_space, fits: plan.fits };
    crate::incoming::apply(&plan.edit, space, &plan.incoming(), device, ctx)
}
