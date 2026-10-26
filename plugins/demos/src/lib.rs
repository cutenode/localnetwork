//! The demos plugin: the demo tracks rekordbox ships in its collection ("GROOVE CIRCUIT FACTORY
//! SAMPLES"), which every export puts back on a drive, so a drive collects `House1.wav`,
//! `House1-1.wav`, `House1-2.wav`... in `Contents/rekordbox/`. Its check finds them, and its
//! operation removes them.
//!
//! A track is one when its artist and title are an entry of [`DEMO_TRACKS`] (ignoring case and
//! spacing), in either of the drive's databases (the library read `combined`). Removing them is
//! one `delete_tracks` edit naming their audio files, which locallibrary plans, confirms and
//! applies: every row of those files goes from both databases with its playlist and history
//! entries, and so do the audio, analysis files and artwork no other track uses, and the artists,
//! albums, genres, labels and keys only they used. A demo track sharing its file with another
//! track stays, as deleting the file's rows would delete that track too.

use std::io::{Read, Write};

use loclib_model::model::{Track, normalize};
use loclib_model::paths::file_key;
use loclib_plugin::{
    CheckFinding, Context, Finding, Findings, Fix, Hooks, Host, Level, OpPlan, PlanOp, RpcError, RunCheck,
};
use rustc_hash::FxHashSet;
use serde_json::{Value, json};

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// `(artist, title)` of rekordbox's demo tracks.
pub const DEMO_TRACKS: &[(&str, &str)] = &[
    ("rekordbox", "House 1"),
    ("rekordbox", "House 2"),
    ("rekordbox", "House 3"),
    ("rekordbox", "House 4"),
    ("rekordbox", "Techno 1"),
    ("rekordbox", "Breaks 1"),
    ("rekordbox", "Breaks 2"),
    ("rekordbox", "Breaks 3"),
];

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(run_check).on(plan_op), input, output);
}

fn is_demo(t: &Track) -> bool {
    let (artist, title) = (normalize(&t.artist), normalize(&t.title));
    DEMO_TRACKS.iter().any(|(a, t)| normalize(a) == artist && normalize(t) == title)
}

/// The demo tracks on the drive the user is on that can go, and how many share their file with
/// another track (and stay).
fn demos(context: &Context, host: &Host) -> Result<(Vec<Track>, usize), RpcError> {
    let drive = context.drive.as_deref().ok_or_else(|| RpcError::invalid_params("say which drive"))?;
    let lib = host.drive(drive).combined().library()?;
    let others: FxHashSet<String> = lib.tracks.iter().filter(|t| !is_demo(t)).map(|t| file_key(&t.file_path)).collect();
    let (shared, demos): (Vec<Track>, Vec<Track>) =
        lib.tracks.into_iter().filter(is_demo).partition(|t| others.contains(&file_key(&t.file_path)));
    Ok((demos, shared.len()))
}

/// `tracks`: the demo tracks `clean` removes, as one finding fixed by it; nothing on a drive
/// without any.
fn run_check(req: RunCheck, host: &Host) -> Result<Findings, RpcError> {
    let (demos, _) = demos(&req.context, host)?;
    if demos.is_empty() {
        return Ok(Findings::default());
    }
    let message = format!("{} of rekordbox's demo track(s), which its exports put back", demos.len());
    let finding =
        Finding { tracks: demos.iter().map(|t| t.id).collect(), ..Finding::new(Level::Info, req.check, message) };
    let fix = Fix { operation: "clean".into(), params: Value::Null };
    Ok(Findings { findings: vec![CheckFinding { finding, fix: Some(fix) }] })
}

/// `clean`: the demo tracks deleted, by file; with none, a plan that changes nothing.
fn plan_op(req: PlanOp, host: &Host) -> Result<OpPlan, RpcError> {
    let (demos, shared) = demos(&req.context, host)?;
    let mut warnings = Vec::new();
    if shared > 0 {
        warnings.push(format!(
            "{shared} demo track(s) share their audio file with another track and stay: deleting them would delete it too"
        ));
    }
    let mut seen = FxHashSet::default();
    let paths: Vec<&str> =
        demos.iter().filter(|t| seen.insert(file_key(&t.file_path))).map(|t| t.file_path.as_str()).collect();
    let summary = match demos.len() {
        0 => "no demo tracks from rekordbox on the drive".to_string(),
        n => format!("remove {n} of rekordbox's demo track(s) and their files"),
    };
    let edits = match paths.is_empty() {
        true => Vec::new(),
        false => vec![json!({ "op": "delete_tracks", "tracks": { "paths": paths } })],
    };
    Ok(OpPlan { summary, warnings, edits, inputs: Vec::new() })
}

#[cfg(test)]
mod tests;
