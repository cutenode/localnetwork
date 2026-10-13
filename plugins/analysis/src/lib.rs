//! locallibrary's sound analysis plugin: one analyzer, `sound`, measuring a track's tempo, key
//! and loudness from its audio with loclib-analysis, as doctor's `--analyze` does, for the host
//! to keep and show as track-list columns. It reads nothing but the audio file it's handed, so it
//! asks for no capabilities.

use std::io::{Read, Write};

use loclib_analysis::{Want, analyze};
use loclib_plugin::{Analysis, AnalyzeTrack, FieldValue, Host, Manifest, Plugin, RpcError};

/// `locallibrary-plugin.toml`.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

struct Sound;

impl Plugin for Sound {
    fn analyze_track(&self, req: AnalyzeTrack, host: &Host) -> Result<Analysis, RpcError> {
        // Everything doctor measures, from one decode. Cancelling stops the decode; the tempo and
        // key analysis after it runs to its end (a few seconds).
        let analysis = analyze(&req.track.audio_path, Want::ALL, &host.ctx()).map_err(|e| match &e {
            loclib_analysis::Error::Cancelled(_) => RpcError::cancelled(),
            loclib_analysis::Error::Audio(loclib_mixid::Error::Io { .. }) => RpcError::failed("io", e.to_string()),
            // Audio no decoder plays, or that the meter can't measure.
            _ => RpcError::failed("parse", e.to_string()),
        })?;
        let estimate = analysis.estimate.as_ref();
        // A tempo of 0 is none found (a blip too short for one).
        let tempo = estimate.filter(|e| e.bpm > 0.0);
        let loudness = analysis.loudness.as_ref();
        let fields = [
            ("bpm", tempo.map(|e| FieldValue::Number(e.bpm.into()))),
            ("bpm_confidence", tempo.map(|e| FieldValue::Number(e.bpm_confidence.into()))),
            ("key", estimate.map(|e| FieldValue::String(e.key.code()))),
            ("key_confidence", estimate.map(|e| FieldValue::Number(e.key_confidence.into()))),
            ("integrated_lufs", loudness.and_then(|l| l.integrated_lufs).map(FieldValue::Number)),
            // Minus infinity for silence, which JSON has no number for.
            (
                "true_peak_dbtp",
                loudness.map(|l| l.true_peak_dbtp()).filter(|db| db.is_finite()).map(FieldValue::Number),
            ),
            ("clipped_runs", loudness.map(|l| FieldValue::Integer(l.clipped_runs.try_into().unwrap_or(i64::MAX)))),
        ];
        Ok(Analysis { fields: fields.into_iter().filter_map(|(name, v)| Some((name.into(), v?))).collect() })
    }
}

/// Serves the plugin over `input` and `output` (the process's stdin and stdout) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    let manifest = Manifest::parse(MANIFEST).expect("the manifest parses");
    loclib_plugin::serve(&manifest, Sound, input, output);
}

#[cfg(test)]
mod tests;
