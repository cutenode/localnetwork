//! locallibrary's sound analysis plugin, `sound`: one analyzer, `tempo-key-loudness`, measuring a
//! track's tempo, key and loudness from its audio with loclib-analysis, for the host to keep and
//! show as track-list columns (`sound.bpm`) and to read by role (doctor's `--analyze`, the check
//! before a set, ordering a set). It reads nothing but the audio file it's handed, so it asks for
//! no capabilities.

use std::io::{Read, Write};

use loclib_analysis::{Want, analyze};
use loclib_plugin::{Analysis, AnalyzeTrack, FieldValue, Hooks, Host, RpcError};

/// `locallibrary-plugin.toml`.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// `tempo-key-loudness`.
fn analyze_track(req: AnalyzeTrack, host: &Host) -> Result<Analysis, RpcError> {
    // Everything doctor measures, from one decode. Cancelling stops the decode; the tempo and
    // key analysis after it runs to its end (a few seconds).
    let ctx = host.ctx();
    // Audio no decoder plays, or that the meter can't measure, is `parse`.
    let analysis = analyze(&req.track.audio_path, Want::ALL, &ctx).map_err(|e| RpcError::failure(&e, "parse", &ctx))?;
    let estimate = analysis.estimate.as_ref();
    // A tempo of 0 is none found (a blip too short for one).
    let tempo = estimate.filter(|e| e.bpm > 0.0);
    let loudness = analysis.loudness.as_ref();
    // Everything was worked out: what has nothing to show is null.
    let number = |n: Option<f64>| n.map_or(FieldValue::Null, FieldValue::Number);
    let fields = [
        ("bpm", number(tempo.map(|e| e.bpm.into()))),
        ("bpm_confidence", number(tempo.map(|e| e.bpm_confidence.into()))),
        ("key", estimate.and_then(|e| e.key).map_or(FieldValue::Null, |k| FieldValue::String(k.code()))),
        ("key_confidence", number(estimate.map(|e| e.key_confidence.into()))),
        ("integrated_lufs", number(loudness.and_then(|l| l.integrated_lufs))),
        // Minus infinity for silence, which JSON has no number for.
        ("true_peak_dbtp", number(loudness.map(|l| l.true_peak_dbtp()).filter(|db| db.is_finite()))),
        (
            "clipped_runs",
            loudness.map_or(FieldValue::Null, |l| FieldValue::Integer(l.clipped_runs.try_into().unwrap_or(i64::MAX))),
        ),
    ];
    Ok(Analysis { fields: fields.into_iter().map(|(name, v)| (name.into(), v)).collect() })
}

/// Serves the plugin over `input` and `output` (the process's stdin and stdout) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(analyze_track), input, output);
}

#[cfg(test)]
mod tests;
