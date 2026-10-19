//! The tracklist plugin: makes a playlist on a drive from a tracklist (M3U, CSV, rekordbox text or
//! XML, or "Artist - Title" lines), finding its tracks with loclib-tracklist. It reads the list
//! itself, as it's the user's own file, and the drive's library through the host; the playlist it
//! answers is an edit locallibrary plans, confirms and applies.

use std::io::{Read, Write};
use std::path::PathBuf;

use indexmap::IndexMap;
use loclib_model::model::{self, Library, Playlist};
use loclib_plugin::{Context, Hooks, Host, OpPlan, PlanOp, RpcError, RunCommand, params};
use loclib_tracklist::{self as tracklist, Format, MatchKind, Report, Status, Summary};
use serde::Deserialize;
use serde_json::{Value, json};

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(run_command).on(plan_op), input, output);
}

/// The `match` command's params: which list, and the user's choices. The drive is the one the
/// user is on (`context.drive`).
#[derive(Deserialize)]
struct Match {
    file: PathBuf,
    #[serde(default)]
    playlist: Option<String>,
    /// Entry index (from 0) -> track id.
    #[serde(default)]
    choices: IndexMap<usize, u32>,
}

/// The `import` operation's besides [`Match`]'s: the playlist it fills. (Read apart from them:
/// `serde(flatten)` would take `choices`' keys as strings, not numbers.)
#[derive(Deserialize)]
struct Import {
    into: String,
    #[serde(default)]
    append: bool,
}

/// The most candidates a warning names for an ambiguous entry, best first.
const CANDIDATES_SHOWN: usize = 5;

/// `match`: the list's entries, each found (and how), ambiguous (with its candidates) or
/// missing.
fn run_command(req: RunCommand, host: &Host) -> Result<Value, RpcError> {
    let (report, _) = matched(&params(req.params)?, &req.context, host)?;
    Ok(serde_json::to_value(report).expect("a report is JSON"))
}

/// `import`: the playlist filled with the tracks found, in the list's order. With none found,
/// a plan that changes nothing, saying so.
fn plan_op(req: PlanOp, host: &Host) -> Result<OpPlan, RpcError> {
    let list: Match = params(req.params.clone())?;
    let import: Import = params(req.params)?;
    let (report, lib) = matched(&list, &req.context, host)?;
    let files = |ids: &[u32]| lib.tracks_by_id(ids).into_iter().map(|t| t.file_path.clone()).collect::<Vec<_>>();
    // As entries, by file: a set list that played a track twice keeps both.
    let mut paths = files(&report.track_ids());
    let mut warnings = warnings(&report, &lib);
    let found = !paths.is_empty();
    if !found {
        warnings.push("none of the list's tracks are on the drive".into());
    }
    if import.append
        && let Some(p) = exact(&lib, &import.into)
    {
        paths.splice(0..0, files(&p.track_ids));
    }
    let s = &report.summary;
    let summary = format!(
        "{}: {} entr(ies) as {:?}: {} found, {} ambiguous, {} missing",
        list.file.display(),
        s.entries,
        report.format,
        s.found,
        s.ambiguous,
        s.missing
    );
    let edits = match found {
        true => vec![json!({ "op": "set_entries", "playlist": import.into, "paths": paths, "create": true })],
        false => Vec::new(),
    };
    // The list it was matched from: the host won't apply the plan once it changes.
    Ok(OpPlan { summary, warnings, edits, inputs: vec![list.file] })
}

/// The tracklist `req` names, found in the library of the drive the user is on, with the user's
/// choices made; and that library.
fn matched(req: &Match, context: &Context, host: &Host) -> Result<(Report, Library), RpcError> {
    let drive = context.drive.as_deref().ok_or_else(|| RpcError::invalid_params("say which drive"))?;
    let file = req.file.display();
    let bytes = std::fs::read(&req.file).map_err(|e| RpcError::failed("io", format!("{file}: {e}")))?;
    let hint = req.file.file_name().map(|n| n.to_string_lossy().into_owned());
    let unreadable = |e: tracklist::Error| RpcError::failed("invalid", e.to_string());
    let (format, entries) = match &req.playlist {
        Some(p) => {
            let lists = tracklist::xml_playlists(&bytes).map_err(unreadable)?;
            let entries = model::pick(lists, p.trim().trim_matches('/'))
                .ok_or_else(|| RpcError::failed("not_found", format!("not found: playlist {p:?} in {file}")))?;
            (Format::RekordboxXml, entries)
        }
        None => {
            (tracklist::detect(&bytes, hint.as_deref()), tracklist::parse(&bytes, hint.as_deref()).map_err(unreadable)?)
        }
    };
    // Not `combined`: an edit can't name a track only OneLibrary has.
    let lib = host.drive(drive).library()?;
    let mut report = Report::new(format, tracklist::match_entries(&lib, &entries));
    for (&i, &track) in &req.choices {
        let m = report
            .entries
            .get_mut(i)
            .ok_or_else(|| RpcError::failed("invalid", format!("no entry {i} in the list")))?;
        if lib.track(track).is_none() {
            return Err(RpcError::failed("not_found", format!("not found: track #{track}")));
        }
        m.status = Status::Found { track_id: track, how: MatchKind::Chosen };
    }
    report.summary = Summary::of(&report.entries);
    Ok((report, lib))
}

/// The playlist at exactly the path `into`, as `set_entries` finds the one it fills: never one of
/// that name in another folder. When several match it ignoring case, the host's plan says so.
fn exact<'l>(lib: &'l Library, into: &str) -> Option<&'l Playlist> {
    let path = into.split('/').map(str::trim).filter(|n| !n.is_empty()).collect::<Vec<_>>().join("/");
    lib.display_order().into_iter().find(|(p, list)| !list.is_folder && *p == path).map(|(_, list)| list)
}

/// The entries that won't be in the playlist, numbered from 1 as people count: those the drive
/// doesn't have, and those that need a choice, with their best candidates.
fn warnings(report: &Report, lib: &Library) -> Vec<String> {
    let numbered = report.entries.iter().enumerate().map(|(i, m)| (i + 1, m.entry.raw.trim(), &m.status));
    numbered
        .filter_map(|(n, raw, status)| match status {
            Status::Found { .. } => None,
            Status::Ambiguous { candidates } => {
                let shown = &candidates[..candidates.len().min(CANDIDATES_SHOWN)];
                let named: Vec<String> =
                    lib.tracks_by_id(shown).iter().map(|t| format!("#{} {}", t.id, t.display_name())).collect();
                Some(format!("{n}. {raw} could be {}: left out unless one is chosen", named.join(", ")))
            }
            Status::Missing => Some(format!("{n}. {raw} isn't on the drive")),
        })
        .collect()
}

#[cfg(test)]
mod tests;
