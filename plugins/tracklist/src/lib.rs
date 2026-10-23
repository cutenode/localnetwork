//! The tracklist plugin: makes a playlist on a drive from a tracklist (M3U, CSV, rekordbox text or
//! XML, or "Artist - Title" lines), finding its tracks with loclib-tracklist. It reads the list
//! itself, as it's the user's own file, and the drive's library through the host; the playlist it
//! answers is an edit locallibrary plans, confirms and applies. The entries that could be several
//! tracks it asks the user about first, all in one round (the `choose` feature, the first as the
//! question and the rest as its `more`, at most `Choice::MAX_QUESTIONS`): the host plans it again
//! with the answers, each entry's number (`"3"`) set to the track's id, or to `null` to leave it
//! out. A host that asks only the first is asked about the rest in the rounds after.

use std::io::{Read, Write};
use std::path::PathBuf;

use indexmap::IndexMap;
use loclib_model::model::{self, Library, Playlist, Track};
use loclib_plugin::{Choice, ChoiceItem, Context, Hooks, Host, OpAnswer, OpPlan, PlanOp, RpcError, params};
use loclib_tracklist::{self as tracklist, Format, MatchKind, Report, Status, Summary};
use serde::Deserialize;
use serde_json::{Value, json};

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(plan_op), input, output);
}

/// The `import` operation's params, but for the answers to its questions (see [`answers`]). The
/// drive is the one the user is on (`context.drive`).
#[derive(Deserialize)]
struct Import {
    file: PathBuf,
    into: String,
    #[serde(default)]
    playlist: Option<String>,
    #[serde(default)]
    append: bool,
}

/// What the user chose for entries, by index (from 0): a track, or none (left out).
type Answers = IndexMap<usize, Option<u32>>;

/// The params naming entries (by number, from 1) and what was chosen for each: a track's id, or
/// `null` to leave it out.
fn answers(params: &Value) -> Result<Answers, RpcError> {
    let numbered = params.as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.parse::<usize>().ok()?, v)));
    numbered
        .map(|(n, v)| {
            let i = n.checked_sub(1).ok_or_else(|| RpcError::invalid_params("entries count from 1"))?;
            if v.is_null() {
                return Ok((i, None));
            }
            let id = v.as_u64().and_then(|id| u32::try_from(id).ok());
            let id = id.ok_or_else(|| RpcError::invalid_params(format!("entry {n} is a track's id, not {v}")))?;
            Ok((i, Some(id)))
        })
        .collect()
}

/// `import`: the playlist filled with the tracks found, in the list's order. While entries that
/// could be several tracks aren't answered, the questions which, all in one round (as many as
/// one may ask: the rest in the next). With none found, a plan that changes nothing, saying so.
fn plan_op(req: PlanOp, host: &Host) -> Result<OpAnswer, RpcError> {
    let import: Import = params(req.params.clone())?;
    let answers = answers(&req.params)?;
    let (report, lib) = matched(&import, &answers, &req.context, host)?;
    let undecided = report.entries.iter().enumerate().filter(|(i, _)| !answers.contains_key(i));
    let questions = undecided.filter_map(|(i, m)| match &m.status {
        Status::Ambiguous { candidates } => Some(question(i, m.entry.raw.trim(), candidates, &lib)),
        _ => None,
    });
    if let Some(asked) = Choice::together(questions) {
        return Ok(asked.into());
    }
    let files = |ids: &[u32]| lib.tracks_by_id(ids).into_iter().map(|t| t.file_path.clone()).collect::<Vec<_>>();
    // As entries, by file: a set list that played a track twice keeps both. Those left out
    // aren't.
    let kept = report.entries.iter().enumerate().filter(|(i, _)| answers.get(i) != Some(&None));
    let mut paths = files(&kept.filter_map(|(_, m)| m.status.track_id()).collect::<Vec<_>>());
    let mut warnings = warnings(&report, &answers);
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
        import.file.display(),
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
    Ok(OpPlan { summary, warnings, edits, inputs: vec![import.file] }.into())
}

/// Which of `candidates` (best first) entry `i` (`raw`) is, or none: the entry's number (from 1)
/// set to the track's id, or to `null` to leave it out.
fn question(i: usize, raw: &str, candidates: &[u32], lib: &Library) -> Choice {
    let n = i + 1;
    let items = lib.tracks_by_id(candidates).into_iter().map(|t| ChoiceItem {
        label: format!("#{} {}", t.id, t.display_name()),
        detail: detail(t),
        value: t.id.into(),
    });
    let none = ChoiceItem { label: "leave it out".into(), detail: None, value: Value::Null };
    let items = items.chain([none]).collect();
    Choice { title: format!("{n}. {raw}"), param: n.to_string(), multi: false, items, more: Vec::new() }
}

/// What tells a candidate from its namesakes: its album and length.
fn detail(t: &Track) -> Option<String> {
    let length = (t.duration_secs > 0).then(|| format!("{}:{:02}", t.duration_secs / 60, t.duration_secs % 60));
    let said: Vec<String> = [(!t.album.is_empty()).then(|| t.album.clone()), length].into_iter().flatten().collect();
    (!said.is_empty()).then(|| said.join(", "))
}

/// The tracklist `req` names, found in the library of the drive the user is on, with the tracks
/// the user chose (`answers`) found; and that library.
fn matched(req: &Import, answers: &Answers, context: &Context, host: &Host) -> Result<(Report, Library), RpcError> {
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
    for (&i, &track) in answers {
        let m = report
            .entries
            .get_mut(i)
            .ok_or_else(|| RpcError::failed("invalid", format!("no entry {} in the list", i + 1)))?;
        let Some(track) = track else { continue };
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

/// The entries that won't be in the playlist, numbered from 1 as people count: those the user
/// left out, and those the drive doesn't have.
fn warnings(report: &Report, answers: &Answers) -> Vec<String> {
    let numbered = report.entries.iter().enumerate().map(|(i, m)| (i + 1, m.entry.raw.trim(), &m.status));
    numbered
        .filter_map(|(n, raw, status)| match status {
            _ if answers.get(&(n - 1)) == Some(&None) => Some(format!("{n}. {raw}: left out")),
            Status::Missing => Some(format!("{n}. {raw} isn't on the drive")),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
