//! Finding the audio of tracks whose file is missing from a drive, and putting a track right only
//! when exactly one file fits it.
//!
//! A missing file is looked for by its name, folded as paths are compared (Unicode NFC, any letter
//! case): among the drive's audio files no track uses (moved or renamed on the drive, see
//! [`crate::doctor`]'s orphans) and in folders named for it, with everything below them. Of
//! several, those of the size the database recorded are kept, then of those, the ones of its
//! length (when a step would keep none, it's skipped: a tag editor changes a file's size). A track
//! with more than one file left gets them all listed and isn't changed: rekordbox's Relocate All
//! takes the first match, which can be another album's `Intro.mp3`. Nor does one with a single file
//! left whose length is known to differ from the track's: that's another recording of the name.
//!
//! A file found on the drive is used where it is: the track is pointed at it in both databases
//! and in its analysis files ([`Edit::SetPath`]), and no audio is copied or moved. A file found
//! elsewhere is copied to the path the track has, which leaves the databases as rekordbox wrote
//! them: rekordbox's next export to the drive writes the paths its collection has again, so the
//! copy is where that export expects it, while a track pointed at a moved file has its old path
//! again after it. Copies go only under `Contents/`. Everything is written through one backup, so
//! `undo` takes it back, and none of it is recorded for `edits replay`: an export puts back what
//! rekordbox knows.

use std::path::PathBuf;

use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::device::Device;
use crate::doctor::{self, Databases};
use crate::edit::{self, Edit, EditOptions, EditPlan, EditReport};
use crate::incoming::{Incoming, Space};
use crate::model::{Track, TrackRef};
use crate::paths::{self, FileClass, fold};
use crate::progress::Ctx;
use crate::query::TrackSelector;

/// A file's length must be less than this (seconds) from a track's to count as the track's:
/// databases keep whole seconds, rounded or cut off.
const LENGTH_TOLERANCE_SECS: f64 = 1.0;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RelocateOptions {
    /// Folders to look in besides the drive (on this computer or another drive), with
    /// everything below them.
    #[serde(default)]
    pub folders: Vec<PathBuf>,
}

/// A file a missing track could be.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[schemars(rename = "RelocateCandidate")]
pub struct Candidate {
    /// A key on the drive, or a path on this computer.
    pub path: String,
    pub on_drive: bool,
    pub size: u64,
    /// Seconds, when its headers say.
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Relocation {
    pub track: TrackRef,
    pub file: Candidate,
}

/// A track and every file of its name found for it, none picked.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Ambiguous {
    pub track: TrackRef,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RelocatePlan {
    pub drive: String,
    pub root: PathBuf,
    /// Tracks pointed at their file where it is on the drive.
    pub repointed: Vec<Relocation>,
    /// Tracks whose file is copied to the path they have.
    pub copied: Vec<Relocation>,
    pub ambiguous: Vec<Ambiguous>,
    /// Tracks whose files of their name are none of them as long as the track: none picked.
    pub unfit: Vec<Ambiguous>,
    /// Missing tracks with no file of their name.
    pub not_found: Vec<TrackRef>,
    /// Audio copied onto the drive.
    pub copy_bytes: u64,
    /// Space needed for that, the backup and the new databases.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
    #[serde(skip)]
    edit: EditPlan,
}

impl RelocatePlan {
    pub fn is_noop(&self) -> bool {
        self.repointed.is_empty() && self.copied.is_empty()
    }
}

/// Of `candidates`, those `fits` keeps, or all of them when it keeps none.
fn narrow(candidates: Vec<Candidate>, fits: impl Fn(&Candidate) -> bool) -> Vec<Candidate> {
    let kept: Vec<Candidate> = candidates.iter().filter(|c| fits(c)).cloned().collect();
    if kept.is_empty() { candidates } else { kept }
}

/// Where each of the drive's tracks whose audio is missing has its file, when exactly one fits.
pub fn plan(device: &Device, opts: &RelocateOptions, ctx: &Ctx<'_>) -> Result<RelocatePlan> {
    let dbs = Databases::read(device)?;
    let tree = device.scan(Some(&dbs.everything()), ctx)?;
    let mut warnings = Vec::new();
    // Tracks of the main database (edits are planned on it; the other follows by path).
    let missing: Vec<&Track> = dbs
        .primary()
        .tracks
        .iter()
        .filter(|t| {
            let key = paths::key(&t.file_path);
            paths::is_safe_key(&key) && !doctor::present(device, &tree, &key)
        })
        .collect();

    let name = |path: &str| fold(path.rsplit('/').next().unwrap_or(path));
    let mut found: FxHashMap<String, Vec<Candidate>> = FxHashMap::default();
    if !missing.is_empty() {
        match &dbs.plus_error {
            Some(_) => warnings
                .push("the drive's own files weren't looked through: what OneLibrary references is unknown".into()),
            None => {
                for o in doctor::orphans(&dbs, &tree).into_iter().filter(|o| o.class == FileClass::Audio) {
                    let c = Candidate { path: o.path, on_drive: true, size: o.size, duration_secs: None };
                    found.entry(name(&c.path)).or_default().push(c);
                }
            }
        }
        for folder in &opts.folders {
            for f in crate::incoming::audio_files(folder, device, ctx)? {
                let c = Candidate {
                    path: f.path.display().to_string(),
                    on_drive: false,
                    size: f.size,
                    duration_secs: None,
                };
                found.entry(paths::file_key(&f.name)).or_default().push(c);
            }
        }
    }

    // Each track's one file, or every file it could be.
    let mut picks: Vec<(&Track, Candidate)> = Vec::new();
    let mut ambiguous = Vec::new();
    let mut unfit = Vec::new();
    let mut not_found = Vec::new();
    for t in missing {
        ctx.check()?;
        let Some(candidates) = found.get(&name(&paths::key(&t.file_path))) else {
            not_found.push(TrackRef::from(t));
            continue;
        };
        let candidates: Vec<Candidate> = candidates
            .iter()
            .map(|c| {
                let file = if c.on_drive { device.checked_path(&c.path) } else { Ok(PathBuf::from(&c.path)) };
                let duration_secs = file.ok().and_then(|f| crate::probe::format(&f).ok()).and_then(|f| f.duration_secs);
                Candidate { duration_secs, ..c.clone() }
            })
            .collect();
        // export.pdb keeps sizes modulo 2^32.
        let same_size = |c: &Candidate| c.size % (1 << 32) == t.file_size % (1 << 32);
        let same_length = |c: &Candidate| {
            c.duration_secs.is_some_and(|d| (d - f64::from(t.duration_secs)).abs() < LENGTH_TOLERANCE_SECS)
        };
        match narrow(narrow(candidates.clone(), same_size), same_length).as_slice() {
            // A file whose length is known to differ from the track's is another recording.
            [one] if one.duration_secs.is_some() && !same_length(one) => {
                unfit.push(Ambiguous { track: t.into(), candidates })
            }
            [one] => picks.push((t, one.clone())),
            _ => ambiguous.push(Ambiguous { track: t.into(), candidates }),
        }
    }
    // A file two tracks would each take is neither's.
    let mut takers: FxHashMap<String, usize> = FxHashMap::default();
    for (_, c) in &picks {
        *takers.entry(c.path.clone()).or_default() += 1;
    }
    let (picks, shared): (Vec<_>, Vec<_>) = picks.into_iter().partition(|(_, c)| takers[&c.path] == 1);
    for (t, c) in shared {
        warnings.push(format!("{} could be the file of several tracks; none of them is relocated", c.path));
        ambiguous.push(Ambiguous { track: t.into(), candidates: vec![c] });
    }

    let mut repointed = Vec::new();
    let mut copied = Vec::new();
    let mut edits = Vec::new();
    for (t, c) in picks {
        let at = Relocation { track: t.into(), file: c.clone() };
        if c.on_drive {
            let track = TrackSelector::paths([t.file_path.clone()]);
            edits.push(Edit::SetPath { track, path: paths::device_path(&c.path), file: None, shift_ms: 0 });
            repointed.push(at);
        } else if paths::classify(&paths::key(&t.file_path)) == Some(FileClass::Audio) {
            copied.push(at);
        } else {
            warnings.push(format!(
                "{}'s file belongs outside Contents/, where locallibrary never writes: copy {} there yourself",
                t.display_name(),
                c.path
            ));
        }
    }
    let mut edit = edit::plan(device, &edits, &EditOptions { lenient: false, no_journal: true }, ctx)?;
    edit.changes.extend(copied.iter().map(|r| format!("copy {} to {}", r.file.path, r.track.file_path)));
    warnings.extend(edit.warnings.iter().cloned());

    let copy_bytes: u64 = copied.iter().map(|r| r.file.size).sum();
    let Space { needed_bytes, free_space, fits } =
        crate::incoming::space(device, &incoming(&copied), &edit, 0, &mut warnings);
    Ok(RelocatePlan {
        drive: device.name(),
        root: device.root().to_path_buf(),
        repointed,
        copied,
        ambiguous,
        unfit,
        not_found,
        copy_bytes,
        needed_bytes,
        free_space,
        fits,
        warnings,
        edit,
    })
}

/// Each copied track's file, copied to the path the track has.
fn incoming(copied: &[Relocation]) -> Vec<Incoming> {
    copied
        .iter()
        .map(|r| Incoming { from: PathBuf::from(&r.file.path), to: paths::key(&r.track.file_path), size: r.file.size })
        .collect()
}

/// Make the planned relocations: the drive's databases and analysis files, and the copies.
pub fn apply(plan: &RelocatePlan, device: &mut Device, ctx: &Ctx<'_>) -> Result<EditReport> {
    let space = Space { needed_bytes: plan.needed_bytes, free_space: plan.free_space, fits: plan.fits };
    crate::incoming::apply(&plan.edit, space, &incoming(&plan.copied), device, ctx)
}
