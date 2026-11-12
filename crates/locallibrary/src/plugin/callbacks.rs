//! What plugins read: requests named after the sidecar's read methods, answered by the app's
//! (see [`Reads`]) once the host has checked them, but for the reads of rekordbox's files a backup
//! needs, which the host answers itself (see [`super::collection`]). A plugin may call only the
//! reads its capabilities grant, with only the params listed for each ([`READS`]), naming drives
//! the host finds itself or gave its hooks under way (never by path) and files only in the
//! request's scratch folder, in pages of a bounded size, a few calls at a time; what it gets back, errors
//! too, says nothing of where things are on this computer (see [`answer_for_plugin`]), but where
//! rekordbox's files and its tracks' audio are, which `collection.locations` and
//! `collection.tracks` are for. A plugin
//! with the `net` feature calls `net/fetch` here too (see [`super::net`]), and one with the
//! `accounts` feature `accounts/ensure` (see [`super::accounts::ensure`]).

use std::cmp::Reverse;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Weak};

use loclib_plugin::{ErrorData, Incoming, Manifest, Param, Peer, READS, RpcError, code, method};
use memchr::memmem::Finder;
use serde_json::Value;

use super::collection;
use super::process::{Live, Plugin, Slot};
use crate::Result;
use crate::api::{self, RpcFailure};
use crate::progress::{Ctx, NoProgress};

/// How the host reads for plugins: the app's read methods, by the sidecar's names
/// (`tracks.query`), with its params and results.
pub trait Reads: Send + Sync {
    /// Runs the read method `method` with `params`, as the RPC sidecar does: on a thread whose
    /// store ([`api::store_root`]) is the one the host started the plugin in, when it had one.
    fn read(&self, method: &str, params: Value, ctx: &Ctx<'_>) -> Result<Value, RpcFailure>;

    /// The root of the drive a plugin names: one known here (by identity, adopted name or
    /// volume name) or a library's lead clone, never a path or a folder
    /// ([`api::resolve_known_drive`]). The host gives the app that root. A drive given to one of
    /// the plugin's hooks under way is found before this is asked (see `Plugin::drive`).
    fn drive(&self, spec: &str) -> Result<PathBuf> {
        Ok(api::resolve_known_drive(spec)?.root().to_path_buf())
    }
}

pub(super) fn invalid_params(message: impl std::fmt::Display) -> RpcFailure {
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
/// at a time. Every one is answered, one whose read panics too, and one that comes once the host
/// has let go of its `slot` (which stops it). Nothing here keeps the plugin hosted: a callback
/// holds the [`Plugin`], never its slot. Its notifications are handled here, in order (see
/// [`Plugin::notified`]).
pub(crate) fn answer(
    slot: &Weak<Slot>,
    plugin: &Arc<Plugin>,
    live: &Arc<Live>,
    peer: &Peer,
    incoming: Receiver<Incoming>,
) {
    let busy = Arc::new(AtomicUsize::new(0));
    for msg in incoming {
        let hosted = slot.strong_count() > 0;
        let req = match msg {
            Incoming::Request(req) => req,
            Incoming::Notification { method, params } => {
                if hosted {
                    plugin.notified(live, &method, params);
                }
                continue;
            }
        };
        if !hosted {
            peer.respond(&req.id, Err(RpcError::closed()));
            continue;
        }
        let most = plugin.host.options.callbacks;
        let under_way = busy.fetch_add(1, Ordering::SeqCst);
        let quota = Quota(busy.clone());
        if under_way >= most {
            drop(quota);
            let e = RpcError::failed("busy", format!("a plugin has at most {most} calls under way at once"));
            peer.respond(&req.id, Err(e));
            continue;
        }
        let (plugin, live, peer) = (plugin.clone(), live.clone(), peer.clone());
        std::thread::spawn(move || {
            // The app reads (and finds drives) in the store of the host that started the plugin,
            // whichever store the process would default to.
            let store = live.store.as_ref().map(|s| s.root().to_path_buf());
            crate::api::THREAD_STORE_ROOT.with(|r| *r.borrow_mut() = store);
            let reads = &*plugin.host.reads;
            let run = || match req.method.as_str() {
                method::NET_FETCH if live.sends(method::NET_FETCH) => plugin.fetch(&live, req.params, &req.cancel),
                method::ACCOUNTS_ENSURE if live.sends(method::ACCOUNTS_ENSURE) => {
                    plugin.ensure(&live, req.params, &Ctx::new(&NoProgress, req.cancel.clone()))
                }
                _ => {
                    let params = check(&plugin.manifest, &|spec| plugin.drive(spec), &req.method, req.params);
                    let read = params.and_then(|p| match collection::answers(&req.method) {
                        true => collection::answer(&live, plugin.host.options.max_blobs, &req.method, p),
                        false => reads.read(&req.method, p, &Ctx::new(&NoProgress, req.cancel.clone())),
                    });
                    let roots = known_roots(live.store.as_ref(), &plugin.given());
                    read.map(|answer| answer_for_plugin(&req.method, answer, &roots)).map_err(|e| for_plugin(e, &roots))
                }
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
                .unwrap_or_else(|_| Err(RpcError::internal(format!("internal error while handling {:?}", req.method))));
            // Before the answer: a call the plugin makes once it has it finds room.
            drop(quota);
            peer.respond(&req.id, result);
        });
    }
}

/// `params` for the read `method`, once the plugin may call it with them: only params listed
/// for it, each as its [`Param`] says (drives found by `drive`, lists and pages bounded), or
/// refused.
pub(crate) fn check(manifest: &Manifest, drive: Drive<'_>, method: &str, params: Value) -> Result<Value, RpcFailure> {
    let Some(read) = READS.iter().find(|r| r.method == method) else {
        let e = format!("{method:?} isn't a read a plugin can call");
        return Err(RpcFailure::new(code::METHOD_NOT_FOUND, "method_not_found", e));
    };
    let capability = &read.capability;
    if !manifest.capabilities.contains(capability) {
        let e = format!("{method} needs the {capability} capability, which the {} plugin doesn't have", manifest.id);
        return Err(RpcFailure::new(code::FAILED, "denied", e));
    }
    let Value::Object(mut params) = params else { return Err(invalid_params("expected an object")) };
    let listed: Vec<(&str, Param)> = read.params().collect();
    if let Some(name) = params.keys().find(|k| !listed.iter().any(|(l, _)| l == k)) {
        return Err(invalid_params(format!("a plugin doesn't give {method} `{name}`")));
    }
    for (name, param) in listed {
        match (param, params.get_mut(name)) {
            (Param::Drive, Some(v)) => *v = root(drive, name, v)?,
            (Param::Drives, Some(Value::Array(list))) => {
                for v in list {
                    *v = root(drive, name, v)?;
                }
            }
            (Param::Drives, Some(v)) => *v = root(drive, name, v)?,
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
            (Param::Scratch, Some(v)) if !(v.is_string() || v.is_null()) => {
                return Err(invalid_params(format!("`{name}` names a file in the scratch folder, which {v} doesn't")));
            }
            _ => {}
        }
    }
    Ok(Value::Object(params))
}

/// Keys left out of every read's answer: a drive's mount point, and where one was last mounted.
const MOUNTS: [&str; 2] = ["root", "last_mount"];

/// A read's answer as a plugin gets it, saying nothing of where things are on this computer:
/// without a drive's mount point ([`MOUNTS`]) or the place of a file outside any drive (a store
/// copy's, a set's or a mix's recording and its cover, rekordbox's collection), and with any other
/// path under one of `roots` said from there (see [`known_roots`]), as errors' are. But for
/// `collection.locations` and `collection.tracks`, whose answers are where rekordbox's files and
/// its tracks' audio are.
pub(crate) fn answer_for_plugin(method: &str, mut answer: Value, roots: &[(String, String)]) -> Value {
    // Where rekordbox's files are (and its tracks' audio files), as they are: telling a plugin with
    // `collection:files` that is the read's whole point, and a path said from `~` or `<store>`
    // names no file it can open.
    if method == "collection.locations" || method == "collection.tracks" {
        return answer;
    }
    // Each key, at any depth, in an object that's the value of the first key when one is given.
    let places: &[(Option<&str>, &str)] = match method {
        "store.copies" => &[(None, "path"), (None, "from")],
        "store.info" => &[(Some("copy_of"), "path"), (Some("copy_of"), "from")],
        "sets.list" | "sets.get" => &[(Some("recording"), "path")],
        "mix.reports.get" => &[(Some("mix"), "path"), (Some("mix"), "cover"), (None, "audio")],
        "collection.info" => &[(None, "path")],
        _ => &[],
    };
    without(&mut answer, None, places);
    scrub_value(&mut answer, roots);
    answer
}

/// `v` without [`MOUNTS`] and `places` (see [`answer_for_plugin`]), in an object under `parent`.
fn without(v: &mut Value, parent: Option<&str>, places: &[(Option<&str>, &str)]) {
    match v {
        Value::Array(list) => list.iter_mut().for_each(|v| without(v, parent, places)),
        Value::Object(map) => {
            let place =
                |k: &str| places.iter().any(|(under, key)| *key == k && under.is_none_or(|u| Some(u) == parent));
            map.retain(|k, _| !MOUNTS.contains(&k.as_str()) && !place(k));
            map.iter_mut().for_each(|(k, v)| without(v, Some(k), places));
        }
        _ => {}
    }
}

/// How a callback finds the root of the drive a plugin names ([`super::process::Plugin::drive`]).
pub(crate) type Drive<'a> = &'a dyn Fn(&str) -> Result<PathBuf>;

/// The root of the drive the param `name` names, as the app is given it.
fn root(drive: Drive<'_>, name: &str, v: &Value) -> Result<Value, RpcFailure> {
    match v {
        Value::Null => Ok(Value::Null),
        Value::String(spec) => Ok(Value::from(drive(spec)?.to_string_lossy().into_owned())),
        other => Err(invalid_params(format!("`{name}` names a drive, which {other} doesn't"))),
    }
}

/// What absolute paths here start with, longest first, each with what it's said as instead:
/// the mounted drives' roots and those `given` to the plugin's hooks (their names), the store's and `store`'s (`<store>`), the temporary
/// folder's (`<temp>`) and the home folder's (`~`), each as given and resolved (a drive in the
/// temporary folder through either of its spellings too), with either separator.
pub(crate) fn known_roots(store: Option<&crate::Store>, given: &[(String, PathBuf)]) -> Vec<(String, String)> {
    let (temp, home) = (std::env::temp_dir(), directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()));
    // Folders spelt otherwise once resolved (macOS's `/var` is `/private/var`).
    let aliases: Vec<(PathBuf, PathBuf)> = std::iter::once(temp.clone())
        .chain(home.clone())
        .filter_map(|p| Some((p.canonicalize().ok()?, p)))
        .filter(|(resolved, given)| resolved != given)
        .collect();
    let mut roots: Vec<(PathBuf, String)> =
        api::known_drives(true).into_iter().map(|d| (d.root().to_path_buf(), d.name())).collect();
    roots.extend(given.iter().map(|(name, root)| (root.clone(), name.clone())));
    let stores = api::registry().map(|s| s.root().to_path_buf()).into_iter().chain(store.map(|s| s.root().into()));
    roots.extend(stores.map(|root| (root, "<store>".into())));
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

/// [`scrub`] for every string in `v`. A read's answer holds thousands, nearly all without a root,
/// so each root's searcher is made once, and only a string with a root in it is rewritten.
fn scrub_value(v: &mut Value, roots: &[(String, String)]) {
    fn walk(v: &mut Value, finders: &[Finder<'_>], roots: &[(String, String)]) {
        match v {
            Value::String(s) if finders.iter().any(|f| f.find(s.as_bytes()).is_some()) => *s = scrub(s, roots),
            Value::Array(list) => list.iter_mut().for_each(|v| walk(v, finders, roots)),
            Value::Object(map) => map.values_mut().for_each(|v| walk(v, finders, roots)),
            _ => {}
        }
    }
    let finders: Vec<Finder<'_>> = roots.iter().map(|(root, _)| Finder::new(root)).collect();
    walk(v, &finders, roots);
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
