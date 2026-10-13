//! Plugins' analyzers (`track/analyze`): each track's audio file is handed to the plugin (read
//! only), a few at a time, and the fields it answers, once they're of the types the analyzer
//! declares, are kept in the [`crate::fields`] store for the plugin's version.

use std::sync::atomic::{AtomicUsize, Ordering};

use loclib_plugin::{Analysis, AnalyzeTrack, Context, Track, method};
use schemars::JsonSchema;
use serde::Serialize;

use super::Plugins;
use crate::fields::{Analyzer, FieldStore};
use crate::progress::{Ctx, Event, NoProgress, Phase};
use crate::store::Store;
use crate::{Error, Result, paths};

/// What analysing a drive's tracks did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Analyzed {
    /// Tracks the analyzer worked out fields for, now kept.
    pub analyzed: usize,
    /// Tracks with fields kept already, for their audio file as it is.
    pub kept: usize,
    /// Tracks left out, each with a warning: their audio file isn't on the drive, or the plugin
    /// failed on them.
    pub failed: usize,
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
        let key = Analyzer { plugin: plugin.into(), version: slot.manifest.version.clone(), analyzer: analyzer.into() };
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
            slot.start()?;
        }
        ctx.phase_start(Phase::Analyze, todo.len() as u64, 0);
        let (next, analyzed, failed) = (AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0));
        // The plugin's own progress about a track is left out (each is an item here); cancelling
        // still reaches it.
        let quiet = Ctx::new(&NoProgress, ctx.cancel.clone());
        let work = || -> Result<()> {
            while let Some(track) = todo.get(next.fetch_add(1, Ordering::SeqCst)) {
                ctx.emit(Event::Item { phase: Phase::Analyze, path: track.file_path.clone(), bytes: 0 });
                let req = AnalyzeTrack { analyzer: analyzer.into(), track: track.clone(), context: context.clone() };
                let answer = slot.call::<Analysis>(method::ANALYZE_TRACK, &req, &quiet).and_then(|a| {
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
                    match wrong {
                        Some(wrong) => Err(slot.misbehaved(wrong)),
                        None => Ok(a.fields),
                    }
                });
                match answer {
                    Ok(answer) => {
                        fields.put(&track.audio_path, &key, &answer)?;
                        analyzed.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(e) => {
                        ctx.warn(format!("{}: {e}", track.file_path));
                        failed.fetch_add(1, Ordering::SeqCst);
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
        done.failed += failed.into_inner();
        Ok(done)
    }
}
