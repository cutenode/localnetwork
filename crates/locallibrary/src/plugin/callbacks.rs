//! What plugins read: requests named after the sidecar's read methods, answered by the app's
//! (see [`Reads`]) once the host has checked them. A plugin may call only the reads its
//! capabilities grant, with only the params listed for each, naming drives the host finds itself
//! (never by path), in pages of a bounded size, a few calls at a time; the errors it gets back say
//! nothing of where files are on this computer.

use std::cmp::Reverse;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;

use loclib_plugin::{Capability, ErrorData, Incoming, Manifest, Peer, RpcError, code};
use serde_json::Value;

use super::process::Host;
use crate::Result;
use crate::api::{self, RpcFailure};
use crate::progress::{Ctx, NoProgress};

/// How the host reads for plugins: the app's read methods, by the sidecar's names
/// (`tracks.query`), with its params and results.
pub trait Reads: Send + Sync {
    /// Runs the read method `method` with `params`, as the RPC sidecar does.
    fn read(&self, method: &str, params: Value, ctx: &Ctx<'_>) -> Result<Value, RpcFailure>;

    /// The root of the drive a plugin names: one known here (by identity, adopted name or
    /// volume name) or a library's lead clone, never a path or a folder
    /// ([`api::resolve_known_drive`]). The host gives the app that root.
    fn drive(&self, spec: &str) -> Result<PathBuf> {
        Ok(api::resolve_known_drive(spec)?.root().to_path_buf())
    }
}

/// The most tracks (or snapshots, or matches, or most played) a page holds: a plugin asks for
/// the next.
const MAX_PAGE: u64 = 500;
/// The most tracks asked for by id at once.
const MAX_IDS: usize = 1000;
/// The most tracks asked for at once by reads that answer each in full (details, cues).
const MAX_TRACKS: usize = 100;
/// The most tracks' artwork asked for at once (base64 JPEGs).
const MAX_IMAGES: usize = 50;

/// A param a plugin may give a read.
#[derive(Debug, Clone, Copy)]
enum Param {
    /// Passed on as it is.
    Plain,
    /// A drive (or library) the host finds itself: the app is given its root.
    Drive,
    /// A list of them.
    Drives,
    /// A list of at most this many.
    AtMost(usize),
    /// A page size of at most this many, this many when not given.
    Limit(u64),
    /// This when not given.
    Or(&'static str),
}

/// Params a plugin may give a read, by name.
type Params = &'static [(&'static str, Param)];

/// What a read of a drive's library takes.
const LIBRARY: Params = &[("drive", Param::Drive), ("combined", Param::Plain)];

/// The capability a read needs, whether it reads a library (and takes [`LIBRARY`]'s params), and
/// the other params a plugin may give it. `None` for anything else, which no plugin may call:
/// writes, reads naming files on this computer, and reads whose answers can't be bounded. A
/// param added to a read is refused until it's listed here.
fn allowed(method: &str) -> Option<(Capability, bool, Params)> {
    use Capability::*;
    use Param::*;
    Some(match method {
        "drive.library" => (LibraryRead, true, &[("offset", Plain), ("limit", Limit(MAX_PAGE))]),
        "playlists.tree" => (LibraryRead, true, &[]),
        "tracks.get" => (LibraryRead, true, &[("ids", AtMost(MAX_IDS))]),
        "tracks.query" => (
            LibraryRead,
            true,
            &[("filter", Plain), ("playlist", Plain), ("sort", Plain), ("offset", Plain), ("limit", Limit(MAX_PAGE))],
        ),
        "track.show" | "cues.list" => {
            (LibraryRead, true, &[("ids", AtMost(MAX_TRACKS)), ("paths", AtMost(MAX_TRACKS))])
        }
        "track.matches" => (LibraryRead, true, &[("track", Plain), ("limit", Limit(MAX_PAGE))]),
        "tags.list" => (LibraryRead, false, &[("drive", Drive)]),
        "track.analysis" => (AnalysisRead, true, &[("track", Plain), ("waveform", Or("none")), ("encoding", Plain)]),
        "tracks.analysis" => {
            (AnalysisRead, true, &[("ids", AtMost(api::MAX_ANALYSES)), ("waveform", Plain), ("encoding", Plain)])
        }
        "track.fields" => (AnalysisRead, true, &[("tracks", AtMost(MAX_IDS)), ("plugin", Plain)]),
        "art.get" => (ArtworkRead, true, &[("track", Plain), ("tracks", AtMost(MAX_IMAGES))]),
        "drive.info" | "drive.backups" | "drive.space" => (DriveInfo, false, &[("drive", Drive)]),
        "drives.list" => (DriveInfo, false, &[("all", Plain)]),
        "drives.mounted" | "drive.groups" => (DriveInfo, false, &[]),
        "fleet.status" => (DriveInfo, false, &[("all", Plain), ("retired", Plain)]),
        "doctor" => (
            HealthRead,
            false,
            &[
                ("drive", Drive),
                ("hash_audio", Plain),
                ("by_sound", Plain),
                ("only", Plain),
                ("audio", Plain),
                ("analyze", Plain),
                ("sure_only", Plain),
            ],
        ),
        "check" => (HealthRead, false, &[("drives", Drives), ("players", Plain), ("audio", Plain)]),
        "drive.verify" => (HealthRead, false, &[("drive", Drive), ("snapshot", Plain)]),
        "store.info" | "store.copies" | "library.list" | "smart.list" => (StoreRead, false, &[]),
        "store.history" => (StoreRead, false, &[("library", Plain), ("limit", Limit(MAX_PAGE))]),
        "settings.schema" | "settings.profile" | "players.models" | "players.show" => (SettingsRead, false, &[]),
        "settings.show" | "player.show" => (SettingsRead, false, &[("drive", Drive)]),
        "plays.stats" => (PlaysRead, false, &[("source", Drive), ("since", Plain), ("limit", Limit(MAX_PAGE))]),
        "sets.played" => (PlaysRead, false, &[("track", Plain)]),
        "sets.list" => (SetsRead, false, &[("venue", Plain), ("kind", Plain), ("since", Plain), ("until", Plain)]),
        "sets.get" => (SetsRead, false, &[("set", Plain), ("venue", Plain)]),
        "sessions.list" => (SetsRead, false, &[("drive", Drive), ("unassigned", Plain)]),
        "account.status" => (AccountStatus, false, &[]),
        "collection.info" => (CollectionRead, false, &[]),
        "collection.diff" => (CollectionRead, false, &[("drive", Drive), ("playlist", Plain)]),
        _ => return None,
    })
}

fn invalid_params(message: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(code::INVALID_PARAMS, "invalid_params", format!("invalid params: {message}"))
}

/// A callback under way, counted against its plugin's quota until it's dropped.
struct Quota(Arc<AtomicUsize>);

impl Drop for Quota {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Answers the plugin's callbacks until its output closes, each on a thread of its own, a few
/// at a time. Every one is answered, one whose read panics too.
pub(crate) fn answer(manifest: &Manifest, host: &Arc<Host>, peer: &Peer, incoming: Receiver<Incoming>) {
    let busy = Arc::new(AtomicUsize::new(0));
    // Progress about a request nobody waits for any more is dropped.
    for req in incoming.into_iter().filter_map(|m| match m {
        Incoming::Request(req) => Some(req),
        Incoming::Notification { .. } => None,
    }) {
        let most = host.options.callbacks;
        let under_way = busy.fetch_add(1, Ordering::SeqCst);
        let quota = Quota(busy.clone());
        if under_way >= most {
            drop(quota);
            let e = RpcError::failed("busy", format!("a plugin has at most {most} calls under way at once"));
            peer.respond(&req.id, Err(e));
            continue;
        }
        let (manifest, host, peer) = (manifest.clone(), host.clone(), peer.clone());
        std::thread::spawn(move || {
            let reads = &*host.reads;
            let run = || {
                let params = check(&manifest, reads, &req.method, req.params)?;
                reads.read(&req.method, params, &Ctx::new(&NoProgress, req.cancel.clone()))
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| {
                let e = format!("internal error while handling {:?}", req.method);
                Err(RpcFailure::new(code::INTERNAL, "internal", e))
            });
            // Before the answer: a call the plugin makes once it has it finds room.
            drop(quota);
            peer.respond(&req.id, result.map_err(|e| for_plugin(e, &known_roots())));
        });
    }
}

/// `params` for the read `method`, once the plugin may call it with them: only params listed
/// for it, each as its [`Param`] says (drives found, lists and pages bounded), or refused.
pub(crate) fn check(manifest: &Manifest, reads: &dyn Reads, method: &str, params: Value) -> Result<Value, RpcFailure> {
    let Some((capability, library, others)) = allowed(method) else {
        let e = format!("{method:?} isn't a read a plugin can call");
        return Err(RpcFailure::new(code::METHOD_NOT_FOUND, "method_not_found", e));
    };
    if !manifest.capabilities.contains(&capability) {
        let e = format!("{method} needs the {capability} capability, which the {} plugin doesn't have", manifest.id);
        return Err(RpcFailure::new(code::FAILED, "denied", e));
    }
    let Value::Object(mut params) = params else { return Err(invalid_params("expected an object")) };
    let listed: Vec<(&str, Param)> = LIBRARY.iter().filter(|_| library).chain(others).copied().collect();
    if let Some(name) = params.keys().find(|k| !listed.iter().any(|(l, _)| l == k)) {
        return Err(invalid_params(format!("a plugin doesn't give {method} `{name}`")));
    }
    for (name, param) in listed {
        match (param, params.get_mut(name)) {
            (Param::Drive, Some(v)) => *v = drive(reads, name, v)?,
            (Param::Drives, Some(Value::Array(list))) => {
                for v in list {
                    *v = drive(reads, name, v)?;
                }
            }
            (Param::Drives, Some(v)) => *v = drive(reads, name, v)?,
            (Param::AtMost(most), Some(Value::Array(list))) if list.len() > most => {
                return Err(invalid_params(format!("`{name}` has {} items: at most {most} at a time", list.len())));
            }
            (Param::Limit(most), given) => {
                let limit = given.and_then(|v| v.as_u64()).map_or(most, |n| n.min(most));
                params.insert(name.into(), limit.into());
            }
            (Param::Or(default), None) => {
                params.insert(name.into(), default.into());
            }
            _ => {}
        }
    }
    Ok(Value::Object(params))
}

/// The root of the drive the param `name` names, as the app is given it.
fn drive(reads: &dyn Reads, name: &str, v: &Value) -> Result<Value, RpcFailure> {
    match v {
        Value::Null => Ok(Value::Null),
        Value::String(spec) => Ok(Value::from(reads.drive(spec)?.to_string_lossy().into_owned())),
        other => Err(invalid_params(format!("`{name}` names a drive, which {other} doesn't"))),
    }
}

/// What absolute paths here start with, longest first, each with what it's said as instead:
/// the mounted drives' roots (their names), the store's (`<store>`), the temporary folder's
/// (`<temp>`) and the home folder's (`~`), each as given and resolved (a drive in the temporary
/// folder through either of its spellings too), with either separator.
pub(crate) fn known_roots() -> Vec<(String, String)> {
    let (temp, home) = (std::env::temp_dir(), directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()));
    // Folders spelt otherwise once resolved (macOS's `/var` is `/private/var`).
    let aliases: Vec<(PathBuf, PathBuf)> = std::iter::once(temp.clone())
        .chain(home.clone())
        .filter_map(|p| Some((p.canonicalize().ok()?, p)))
        .filter(|(resolved, given)| resolved != given)
        .collect();
    let drives = api::list_drives(&api::ListDrives { all: true });
    let mut roots: Vec<(PathBuf, String)> =
        drives.into_iter().map(|d| (d.root, d.name.unwrap_or(d.volume_name))).collect();
    roots.extend(api::registry().map(|s| (s.root().to_path_buf(), "<store>".into())));
    roots.push((temp, "<temp>".into()));
    roots.extend(home.map(|h| (h, "~".into())));
    let mut out = Vec::new();
    for (path, said) in roots {
        let resolved = path.canonicalize().unwrap_or_else(|_| path.clone());
        let spellings = aliases.iter().filter_map(|(r, given)| Some(given.join(resolved.strip_prefix(r).ok()?)));
        for p in [resolved.clone(), path].into_iter().chain(spellings.collect::<Vec<_>>()) {
            let p = p.to_string_lossy().trim_end_matches(['/', '\\']).to_string();
            if !p.is_empty() {
                out.push((p.replace('\\', "/"), said.clone()));
                out.push((p, said.clone()));
            }
        }
    }
    out.sort_by_key(|(p, _)| Reverse(p.len()));
    out.dedup();
    out
}

/// A read's error as a plugin gets it: its kind, reason and fields, without the actions a front
/// end offers, and with each path under one of `roots` (see [`known_roots`]) said from there.
pub(crate) fn for_plugin(e: RpcFailure, roots: &[(String, String)]) -> RpcError {
    let mut fields = e.detail.fields;
    fields.values_mut().for_each(|v| scrub_value(v, roots));
    let reason = e.detail.reason.and_then(|r| serde_json::to_value(r).ok()).and_then(|r| r.as_str().map(String::from));
    let data = ErrorData { kind: e.kind.into(), reason, fields };
    RpcError { code: e.code, message: scrub(&e.message, roots), data: Some(Box::new(data)) }
}

fn scrub_value(v: &mut Value, roots: &[(String, String)]) {
    match v {
        Value::String(s) => *s = scrub(s, roots),
        Value::Array(list) => list.iter_mut().for_each(|v| scrub_value(v, roots)),
        Value::Object(map) => map.values_mut().for_each(|v| scrub_value(v, roots)),
        _ => {}
    }
}

/// `text` with each of `roots` said as it's to be where a path goes on past it or ends there
/// (not where another folder's name starts with it: `/Users/al` in `/Users/alex`).
fn scrub(text: &str, roots: &[(String, String)]) -> String {
    let mut text = text.to_string();
    for (root, said) in roots {
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(at) = rest.find(root.as_str()) {
            let after = &rest[at + root.len()..];
            let ends = after.chars().next().is_none_or(|c| !(c.is_alphanumeric() || "._-".contains(c)));
            out.push_str(&rest[..at]);
            out.push_str(if ends { said } else { root });
            rest = after;
        }
        out.push_str(rest);
        text = out;
    }
    text
}

#[cfg(test)]
mod tests;
