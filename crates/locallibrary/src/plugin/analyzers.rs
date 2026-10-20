//! Plugins' analyzers (`track/analyze`): each track's audio file is handed to the plugin (read
//! only), a few at a time, and the fields it answers, once they're of the types the analyzer
//! declares, are kept in the [`crate::fields`] store for the analyzer's version.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use loclib_plugin::{Analysis, AnalyzeTrack, Context, Track, method};
use schemars::JsonSchema;
use serde::Serialize;

use super::Plugins;
use crate::audio_cache::TrackFailure;
use crate::fields::{Analyzer, FieldStore};
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
        let slot = self.slot(plugin)?;
        let declared = slot.declared("analyzer", analyzer, &slot.manifest.contributes.analyzers, |a| &a.name)?;
        let (context, device) =
            slot.context(Context { drive: Some(drive.into()), ..Context::default() }, Some(store))?;
        let device = device.expect("a drive was named");
        let library = device.library()?;
        let selected = match tracks {
            Some(ids) => {
                ids.iter().map(|&id| library.track(id).ok_or_else(|| Error::no_track(id))).collect::<Result<_>>()?
            }
            None => library.tracks.iter().collect::<Vec<_>>(),
        };
        let fields = FieldStore::in_store(store);
        let key = Analyzer { plugin: plugin.into(), analyzer: analyzer.into(), version: declared.version.clone() };
        let mut done = Analyzed::default();
        let mut todo = Vec::new();
        for t in selected {
            let audio = device.checked_path(&paths::key(&t.file_path)).ok().filter(|a| a.is_file());
            match audio {
                None => {
                    ctx.warn(format!("{}: its audio file isn't on the drive", t.file_path));
                    done.failed += 1;
                }
                Some(audio) if !fields.get(&audio, std::slice::from_ref(&key)).is_empty() => done.kept += 1,
                Some(audio) => {
                    let analyze_path = device.checked_path(&paths::key(&t.analyze_path)).ok().filter(|a| a.is_file());
                    let track = Track { id: t.id, file_path: t.file_path.clone(), audio_path: audio, analyze_path };
                    todo.push(track);
                }
            }
        }
        if !todo.is_empty() {
            // One that can't start fails here, not on every track.
            slot.start(Some(store))?;
        }
        ctx.phase_start(Phase::Analyze, todo.len() as u64, 0);
        let (next, analyzed) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let failures = Mutex::new(Vec::new());
        // The plugin's own progress about a track is left out (each is an item here); cancelling
        // still reaches it.
        let quiet = Ctx::new(&NoProgress, ctx.cancel.clone());
        let work = || -> Result<()> {
            loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(track) = todo.get(i) else { break };
                ctx.emit(Event::Item { phase: Phase::Analyze, path: track.file_path.clone(), bytes: 0 });
                let req = AnalyzeTrack { analyzer: analyzer.into(), track: track.clone(), context: context.clone() };
                let answer = slot.call::<Analysis>(method::ANALYZE_TRACK, &req, Some(store), &quiet).and_then(|a| {
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
                        return Err(slot.misbehaved(wrong));
                    }
                    // In the order the analyzer declares them (they're columns), whatever the
                    // answer's.
                    let mut fields = a.fields;
                    fields.sort_by_cached_key(|name, _| declared.fields.get_index_of(name));
                    Ok(fields)
                });
                match answer {
                    Ok(answer) => {
                        fields.put(&track.audio_path, &key, &answer)?;
                        analyzed.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(e) => {
                        // The track by its path on the drive, once, whether or not the plugin
                        // named its audio file.
                        let error = e.to_string().replace(&track.audio_path.display().to_string(), &track.file_path);
                        ctx.warn(match error.contains(&track.file_path) {
                            true => error.clone(),
                            false => format!("{}: {error}", track.file_path),
                        });
                        let t = library.track(track.id).expect("one of the library's");
                        crate::lock(&failures).push((i, TrackFailure { track: t.into(), error }));
                    }
                }
            }
            Ok(())
        };
        let workers = slot.host.options.analyses.clamp(1, todo.len().max(1));
        let results: Vec<Result<()>> = std::thread::scope(|s| {
            let running: Vec<_> = (0..workers).map(|_| s.spawn(work)).collect();
            running.into_iter().map(|w| w.join().expect("a worker doesn't panic")).collect()
        });
        ctx.phase_end(Phase::Analyze);
        results.into_iter().collect::<Result<()>>()?;
        done.analyzed += analyzed.into_inner();
        // In the tracks' order, whichever worker finished first.
        let mut failures = failures.into_inner().unwrap_or_else(PoisonError::into_inner);
        failures.sort_by_key(|(i, _)| *i);
        done.failures = failures.into_iter().map(|(_, f)| f).collect();
        done.failed += done.failures.len();
        Ok(done)
    }
}
