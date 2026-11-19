//! Plugins' analyzers (`track/analyze`): each track's audio file is handed to the plugin (read
//! only), a few at a time, and the fields it answers, once they're of the types the analyzer
//! declares, are kept in the [`crate::fields`] store for the analyzer's version; its beat grid,
//! phrases and waveform (feature `analysis`) in the [`crate::fields::overlay`] store beside it,
//! the waveform read from the file its answer names in the request's scratch folder. Measured
//! for a report instead ([`Plugins::measure`]), nothing is kept, and the audio files may be on no
//! drive.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use std::io::Read;

use indexmap::IndexMap;

use loclib_plugin::{Analysis, AnalyzeTrack, Context, FieldType, Output, Track, method};
use schemars::JsonSchema;
use serde::Serialize;

use super::Plugins;
use super::blobs::Scratch;
use super::process::Slot;
use crate::anlz::{self, Bands};
use crate::audio_cache::TrackFailure;
use crate::device::Device;
use crate::fields::{Analyzer, FieldStore, Fields, Overlay, OverlayStore};
use crate::progress::{Ctx, Event, NoProgress, Phase};
use crate::store::Store;
use crate::{Error, Result, paths};

/// What analysing a drive's tracks did.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct Analyzed {
    /// Tracks the analyzer worked out fields for, now kept.
    pub analyzed: usize,
    /// Tracks with fields kept already, for their audio file as it is.
    pub kept: usize,
    /// Tracks left out, each with a warning: their audio file isn't on the drive, or the plugin
    /// failed on them.
    pub failed: usize,
    /// The tracks the plugin failed on, and why (as their warnings say it): for a report of its
    /// own, as doctor's analysis has.
    #[serde(skip)]
    #[schemars(skip)]
    pub failures: Vec<TrackFailure>,
}

/// What [`Plugins::measure`] hands an analyzer.
#[derive(Debug, Clone, Copy)]
pub enum Audio<'a> {
    /// The tracks with ids `tracks` of the drive `drive` names (by id, name or path).
    Tracks { drive: &'a str, tracks: &'a [u32] },
    /// Audio files on no drive. Each is handed as a track with id 0, its file name for its path
    /// on a drive and no analysis file, and the context names no drive: the plugin's reads of a
    /// library are refused while it has them.
    Files(&'a [PathBuf]),
}

impl Plugins {
    /// Runs one of a plugin's analyzers on the tracks of the drive `drive` names (by id, name or
    /// path), those with ids `tracks` or every one, keeping their fields in `store`. Tracks it
    /// worked out fields for already are left as they are. Progress is an `analyze` phase with
    /// an item for each track.
    pub fn analyze(
        &self,
        plugin: &str,
        analyzer: &str,
        drive: &str,
        tracks: Option<&[u32]>,
        store: &Store,
        ctx: &Ctx<'_>,
    ) -> Result<Analyzed> {
        self.analyzing(plugin, analyzer, drive, tracks, false, store, ctx)
    }

    /// [`Plugins::analyze`] on the tracks with ids `tracks`, over what it kept for them: for
    /// values worked out from what the library said of a track then, which it has changed since
    /// (its audio file, which they're kept by, unchanged).
    pub fn analyze_again(
        &self,
        plugin: &str,
        analyzer: &str,
        drive: &str,
        tracks: &[u32],
        store: &Store,
        ctx: &Ctx<'_>,
    ) -> Result<Analyzed> {
        self.analyzing(plugin, analyzer, drive, Some(tracks), true, store, ctx)
    }

    #[allow(clippy::too_many_arguments)]
    fn analyzing(
        &self,
        plugin: &str,
        analyzer: &str,
        drive: &str,
        tracks: Option<&[u32]>,
        again: bool,
        store: &Store,
        ctx: &Ctx<'_>,
    ) -> Result<Analyzed> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared("analyzer", analyzer, &slot.manifest.contributes.analyzers, |a| &a.name)?;
        let (context, device, _given) =
            slot.context(Context { drive: Some(drive.into()), ..Context::default() }, Some(store))?;
        let device = device.expect("a drive was named");
        let library = device.library()?;
        let selected = match tracks {
            Some(ids) => super::tracks_named(&library, ids)?,
            None => library.tracks.iter().collect::<Vec<_>>(),
        };
        let (fields, overlays) = (FieldStore::in_store(store), OverlayStore::in_store(store));
        let key = Analyzer { plugin: plugin.into(), analyzer: analyzer.into(), version: declared.version.clone() };
        let mut done = Analyzed::default();
        let mut todo = Vec::new();
        for t in selected {
            match on_drive(&device, t) {
                None => {
                    ctx.warn(format!("{}: its audio file isn't on the drive", t.file_path));
                    done.failed += 1;
                }
                Some(track) if !again && !fields.get(&track.audio_path, std::slice::from_ref(&key)).is_empty() => {
                    done.kept += 1
                }
                Some(track) => todo.push(track),
            }
        }
        let analyzed = AtomicUsize::new(0);
        let failures = Mutex::new(Vec::new());
        let keep = |i: usize, answer: Result<(Fields, Overlay)>| -> Result<()> {
            let track = &todo[i];
            match answer {
                Ok((columns, overlay)) => {
                    fields.put(&track.audio_path, &key, &columns)?;
                    if !overlay.is_empty() {
                        overlays.put(&track.audio_path, &key, &overlay)?;
                    }
                    analyzed.fetch_add(1, Ordering::SeqCst);
                }
                Err(e) => {
                    // The track by its path on the drive, once, whether or not the plugin named
                    // its audio file.
                    let error = e.to_string().replace(&track.audio_path.display().to_string(), &track.file_path);
                    ctx.warn(match error.contains(&track.file_path) {
                        true => error.clone(),
                        false => format!("{}: {error}", track.file_path),
                    });
                    let t = library.track(track.id).expect("one of the library's");
                    crate::lock(&failures).push((i, TrackFailure { track: t.into(), error }));
                }
            }
            Ok(())
        };
        slot.analyze_each(analyzer, declared, &context, &todo, store, ctx, &keep)?;
        done.analyzed += analyzed.into_inner();
        // In the tracks' order, whichever worker finished first.
        let mut failures = failures.into_inner().unwrap_or_else(PoisonError::into_inner);
        failures.sort_by_key(|(i, _)| *i);
        done.failures = failures.into_iter().map(|(_, f)| f).collect();
        done.failed += done.failures.len();
        Ok(done)
    }

    /// Runs one of a plugin's analyzers on `audio`, keeping nothing (for a report on the analyzer
    /// itself): each one's fields that aren't structured (its columns, in the order it declares
    /// them), or why it failed, in `audio`'s order. A track whose audio file isn't on the drive
    /// fails. Progress is an `analyze` phase with an item for each one handed to it.
    pub fn measure(
        &self,
        plugin: &str,
        analyzer: &str,
        audio: Audio<'_>,
        store: &Store,
        ctx: &Ctx<'_>,
    ) -> Result<Vec<Result<Fields>>> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared("analyzer", analyzer, &slot.manifest.contributes.analyzers, |a| &a.name)?;
        let drive = match audio {
            Audio::Tracks { drive, .. } => Some(drive.to_string()),
            Audio::Files(_) => None,
        };
        let (context, device, _given) = slot.context(Context { drive, ..Context::default() }, Some(store))?;
        let (handed, _loose): (Vec<Result<Track>>, _) = match audio {
            Audio::Tracks { tracks, .. } => {
                let device = device.expect("a drive was named");
                let library = device.library()?;
                let handed = super::tracks_named(&library, tracks)?.into_iter().map(|t| {
                    let missing = || Error::NotFound(format!("{}: its audio file isn't on the drive", t.file_path));
                    on_drive(&device, t).ok_or_else(missing)
                });
                (handed.collect(), None)
            }
            Audio::Files(files) => {
                let loose = |f: &PathBuf| {
                    let name = f.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    Ok(Track { id: 0, file_path: name, audio_path: f.clone(), analyze_path: None })
                };
                (files.iter().map(loose).collect(), Some(slot.plugin.hand_loose()))
            }
        };
        let todo: Vec<Track> = handed.iter().flatten().cloned().collect();
        let places: Vec<usize> = handed.iter().enumerate().filter_map(|(i, h)| h.is_ok().then_some(i)).collect();
        let measured = Mutex::new(handed.into_iter().map(|h| h.map(|_| Fields::default())).collect::<Vec<_>>());
        let put = |i: usize, answer: Result<(Fields, Overlay)>| -> Result<()> {
            crate::lock(&measured)[places[i]] = answer.map(|(columns, _)| columns);
            Ok(())
        };
        slot.analyze_each(analyzer, declared, &context, &todo, store, ctx, &put)?;
        Ok(measured.into_inner().unwrap_or_else(PoisonError::into_inner))
    }
}

/// The track `t` as an analyzer is handed it, if its audio file is on `device`.
fn on_drive(device: &Device, t: &crate::model::Track) -> Option<Track> {
    let audio_path = device.checked_path(&paths::key(&t.file_path)).ok().filter(|a| a.is_file())?;
    let analyze_path = device.checked_path(&paths::key(&t.analyze_path)).ok().filter(|a| a.is_file());
    Some(Track { id: t.id, file_path: t.file_path.clone(), audio_path, analyze_path })
}

impl Slot {
    /// Hands each of `todo` to the analyzer `analyzer` (as the manifest `declared` it) with
    /// `context`, as many at once as the host's `analyses` option says, with progress: an
    /// `analyze` phase with an item for each, by its path. `done` takes each one's answer as it
    /// comes, with its place in `todo`: its fields that aren't structured, in the order the
    /// analyzer declares them, and its overlay; or why it failed. Cancelling stops them all, as
    /// does an error `done` answers.
    #[allow(clippy::too_many_arguments)]
    fn analyze_each(
        self: &Arc<Self>,
        analyzer: &str,
        declared: &loclib_plugin::Analyzer,
        context: &Context,
        todo: &[Track],
        store: &Store,
        ctx: &Ctx<'_>,
        done: &(dyn Fn(usize, Result<(Fields, Overlay)>) -> Result<()> + Sync),
    ) -> Result<()> {
        if !todo.is_empty() {
            // One that can't start fails here, not on every track.
            self.start(Some(store))?;
        }
        ctx.phase_start(Phase::Analyze, todo.len() as u64, 0);
        let next = AtomicUsize::new(0);
        // The plugin's own progress about a track is left out (each is an item here); cancelling
        // still reaches it.
        let quiet = Ctx::new(&NoProgress, ctx.cancel.clone());
        let work = || -> Result<()> {
            loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(track) = todo.get(i) else { break };
                ctx.emit(Event::Item { phase: Phase::Analyze, path: track.file_path.clone(), bytes: 0 });
                let req = AnalyzeTrack { analyzer: analyzer.into(), track: track.clone(), context: context.clone() };
                let read = |a: Analysis, scratch: &Scratch, _: &_| {
                    let wrong = a.fields.iter().find_map(|(name, v)| match declared.fields.get(name) {
                        None => Some(format!("its field {name} isn't one {analyzer} declares")),
                        Some(t) if !t.accepts(v) => {
                            let t = serde_json::to_value(t).unwrap_or_default();
                            Some(format!(
                                "its field {name} isn't the {} {analyzer} declares",
                                t.as_str().unwrap_or("?")
                            ))
                        }
                        Some(_) => None,
                    });
                    if let Some(wrong) = wrong {
                        return Err(self.misbehaved(wrong));
                    }
                    // In the order the analyzer declares them (they're columns), whatever the
                    // answer's; the structured ones apart.
                    let (structured, mut columns): (Fields, Fields) =
                        a.fields.into_iter().partition(|(name, _)| declared.fields[name].is_structured());
                    columns.sort_by_cached_key(|name, _| declared.fields.get_index_of(name));
                    let overlay = self.overlay(&structured, &declared.fields, scratch)?;
                    Ok((columns, overlay))
                };
                match self.call_with(method::ANALYZE_TRACK, &req, Some(store), &quiet, &|_| {}, read) {
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    answer => done(i, answer)?,
                }
            }
            Ok(())
        };
        let workers = self.host.options.analyses.clamp(1, todo.len().max(1));
        let results: Vec<Result<()>> = std::thread::scope(|s| {
            let running: Vec<_> = (0..workers).map(|_| s.spawn(work)).collect();
            running.into_iter().map(|w| w.join().expect("a worker doesn't panic")).collect()
        });
        ctx.phase_end(Phase::Analyze);
        results.into_iter().collect()
    }

    /// An analyzer's structured fields as the overlay store keeps them (of the types `declared`
    /// says, checked already): its waveform read from `scratch` and put at 150 columns a second,
    /// its beat grid one the analysis files can hold. Null ones are left out.
    fn overlay(
        &self,
        structured: &Fields,
        declared: &IndexMap<String, FieldType>,
        scratch: &Scratch,
    ) -> Result<Overlay> {
        let mut o = Overlay::default();
        for (name, value) in structured {
            let output = Output::of(declared[name], value).expect("accepts checked it");
            match output {
                None => {}
                Some(Output::Beats(beats)) if beats.is_empty() => {}
                Some(Output::Beats(beats)) => {
                    let checked =
                        anlz::checked_grid(&beats).map_err(|e| self.misbehaved(format!("its {name}: {e}")))?;
                    o.beats = Some(checked);
                }
                Some(Output::Phrases(phrases)) => o.phrases = Some(phrases),
                Some(Output::Waveform(w)) => {
                    let mut bytes = Vec::new();
                    let read =
                        scratch.blob(&w.blob).and_then(|mut f| f.read_to_end(&mut bytes).map_err(|e| e.to_string()));
                    read.map_err(|why| self.misbehaved(format!("its {name}: {why}")))?;
                    let bands = Bands::unpack(&bytes).filter(|b| !b.0.is_empty()).ok_or_else(|| {
                        self.misbehaved(format!("its {name} isn't three bytes a column, at least one column"))
                    })?;
                    let bands = anlz::at_detail_rate(&bands, w.columns_per_sec)
                        .map_err(|e| self.misbehaved(format!("its {name}: {e}")))?;
                    o.waveform = Some(bands);
                }
            }
        }
        Ok(o)
    }
}
