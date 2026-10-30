//! Plugins' events (`event/notify`): what the hosts already say happens (drives coming and going
//! and saved into the store, the sidecar's requests finishing, plans applied), told to each
//! plugin listening for it whose capabilities grant it. Each plugin's waiting events are a queue
//! of their own, sent on a thread of its own (starting the plugin when it isn't running): a front
//! end only adds to it, never waiting, and once it's full the oldest is dropped. What a plugin
//! tells the user back (`host/notify`, with the `events` or `services` feature) goes to the
//! front end's [`super::Options::notices`], without what it suggests that the user already
//! reviewed or dismissed, or that's waiting for them (see [`forget_suggested`]).

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use loclib_plugin::{EventName, EventNotify, HostAction, HostNotify, Suggested, Suggestion, method};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Plugins;
use super::callbacks::{answer_for_plugin, known_roots};
use super::process::{Live, Plugin, Slot, printable};
use crate::api::{JobEvent, Notice, WatchAction};
use crate::error::IoContext;
use crate::op::Applied;
use crate::store::Store;
use crate::watch::{MountedDrive, WatchEvent};
use crate::{Result, fsutil, lock};

/// How long a plugin's thread sending its events waits for another before it ends.
const IDLE: Duration = Duration::from_secs(5);

/// The longest message a plugin tells the user, in characters: the rest is cut.
const MAX_MESSAGE: usize = 500;

/// The most suggestions kept for the player to offer (see [`keep_suggested`]): the oldest go.
pub const MAX_SUGGESTED: usize = 20;

/// Something that happened, as a plugin is told of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: EventName,
    /// The drive it's about, by id, else name.
    pub drive: Option<String>,
    /// As the sidecar says it, paths and all: they're left out as it's sent.
    pub payload: Value,
}

/// A drive as a plugin names it: by id, else by name.
fn named(d: &MountedDrive) -> String {
    d.id.map_or_else(|| d.name.clone(), |id| id.to_string())
}

impl Event {
    /// The events plugins may be told of in what the sidecar sends as `event` (none in most).
    pub fn of(notice: &Notice) -> Vec<Event> {
        let (names, drive): (&[EventName], _) = match notice {
            Notice::Watch(WatchEvent::Attached(d)) => (&[EventName::DriveMounted], Some(d)),
            Notice::Watch(WatchEvent::Detached(d)) => (&[EventName::DriveUnmounted], Some(d)),
            Notice::Watch(WatchEvent::Changed { drive, .. }) => (&[EventName::DriveChanged], Some(drive)),
            Notice::Watched(WatchAction::Pulled { drive, beside: true, .. }) => {
                (&[EventName::DrivePulled, EventName::SetReturned], Some(drive))
            }
            Notice::Watched(WatchAction::Pulled { drive, .. }) => (&[EventName::DrivePulled], Some(drive)),
            Notice::Job(JobEvent::Finished { .. }) => (&[EventName::JobFinished], None),
            _ => return Vec::new(),
        };
        let mut payload = serde_json::to_value(notice).unwrap_or_default();
        if let Some(fields) = payload.as_object_mut() {
            fields.remove("event");
        }
        let drive = drive.map(named);
        names.iter().map(|&name| Event { name, drive: drive.clone(), payload: payload.clone() }).collect()
    }

    /// `plan.applied`: what it did, on the first drive it wrote.
    pub fn applied(plan: &Applied<'_>) -> Event {
        let written: Vec<&str> = plan.drives.iter().filter(|t| t.drive && t.writes).map(|t| t.name.as_str()).collect();
        let payload = json!({
            "operation": plan.operation,
            "summary": plan.summary,
            "destructive": plan.destructive,
            "drives": written,
        });
        Event { name: EventName::PlanApplied, drive: written.first().map(|d| d.to_string()), payload }
    }
}

/// A line a plugin tells the user (`host/notify`), and what it suggests, with its params: one of
/// its operations (`operation`) or one of the host's actions (`action`) on its own contributions.
/// The front end offers to start it, and the user confirms it as when they start it themselves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginNotice {
    /// The plugin's id.
    pub plugin: String,
    pub message: String,
    #[serde(flatten)]
    #[schemars(schema_with = "suggests_schema")]
    pub suggests: Option<Suggestion>,
}

/// [`PluginNotice::suggests`] as the sidecar's contract has it: `operation` or `action`, each
/// optional, as front ends read them.
fn suggests_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let (operation, action) = (g.subschema_for::<Suggested>(), g.subschema_for::<HostAction>());
    schemars::json_schema!({ "type": "object", "properties": { "operation": operation, "action": action } })
}

/// A plugin's events waiting to be sent, and whether a thread is sending them.
#[derive(Default)]
pub(crate) struct Queue {
    state: Mutex<Waiting>,
    changed: Condvar,
}

#[derive(Default)]
struct Waiting {
    events: VecDeque<(Event, Option<Store>)>,
    /// A thread is sending them.
    worker: bool,
    /// It's sending one now.
    sending: bool,
}

impl Queue {
    fn lock(&self) -> std::sync::MutexGuard<'_, Waiting> {
        lock(&self.state)
    }
}

impl Plugins {
    /// Tells every plugin listening for `event` (its capabilities granting it) of it, starting
    /// it (in `store`) if it isn't running. Never waits: each plugin's events are sent on a
    /// thread of their own, in order, and once [`super::Options::events`] wait the oldest is
    /// dropped (and logged).
    pub fn deliver(&self, event: &Event, store: Option<&Store>) {
        for slot in self.plugins.values().filter(|s| s.listens(event.name)) {
            slot.queue(event.clone(), store);
        }
    }

    /// Whether any plugin listens for `name`: when none does, a front end needn't look for it.
    pub fn listens(&self, name: EventName) -> bool {
        self.plugins.values().any(|s| s.listens(name))
    }
}

impl Slot {
    fn listens(&self, name: EventName) -> bool {
        let m = &self.manifest;
        m.capabilities.contains(&name.capability()) && m.contributes.events.iter().any(|l| l.on.contains(&name))
    }

    /// Adds `event` to the plugin's queue, starting its sending thread if none is.
    fn queue(self: &Arc<Self>, event: Event, store: Option<&Store>) {
        let mut waiting = self.events.lock();
        waiting.events.push_back((event, store.cloned()));
        let dropped = match waiting.events.len() > self.host.options.events {
            true => waiting.events.pop_front(),
            false => None,
        };
        let start = !std::mem::replace(&mut waiting.worker, true);
        drop(waiting);
        self.events.changed.notify_all();
        if let Some((e, _)) = dropped {
            let most = self.host.options.events;
            self.host.log(&self.manifest.id, &format!("dropped a {} event: {most} were waiting", e.name.name()));
        }
        if start {
            let (slot, queue) = (Arc::downgrade(self), self.events.clone());
            std::thread::spawn(move || send_queued(&slot, &queue));
        }
    }

    /// `event/notify`, its payload saying nothing of where things are on this computer (as a
    /// read's answer doesn't, see [`answer_for_plugin`]). One that can't be sent is logged.
    fn send_event(self: &Arc<Self>, event: Event, store: Option<&Store>) {
        let payload = answer_for_plugin("", event.payload, &known_roots(store));
        let notify = EventNotify { event: event.name, drive: event.drive, payload };
        if let Err(e) = self.notify("events", method::EVENT_NOTIFY, &notify, store) {
            let said = format!("the {} event wasn't sent: {e}", event.name.name());
            self.host.log(&self.manifest.id, &said);
        }
    }

    /// Waits, for at most `within`, until the plugin's events are all sent. Those still waiting
    /// then are dropped (and logged): sent later, they'd start it again once it has stopped. The one
    /// being sent then is waited for (one start of the plugin, at most): it could start the plugin
    /// after the stop that follows, which would leave it running.
    pub(crate) fn sent_events(&self, within: Duration) {
        let until = Instant::now() + within;
        let mut waiting = self.events.lock();
        while !waiting.events.is_empty() || waiting.sending {
            let Some(left) = until.checked_duration_since(Instant::now()).filter(|l| !l.is_zero()) else { break };
            waiting = self.events.changed.wait_timeout(waiting, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        let dropped = std::mem::take(&mut waiting.events).len();
        let waiting = self.events.changed.wait_while(waiting, |w| w.sending).unwrap_or_else(PoisonError::into_inner);
        drop(waiting);
        if dropped > 0 {
            let said = format!("it's stopping: {dropped} of its events waiting weren't sent");
            self.host.log(&self.manifest.id, &said);
        }
    }
}

impl Plugin {
    /// A notification from the plugin: `host/notify`, from one with a feature that sends it,
    /// told to the front end once it's text the user can read, without the secrets the host
    /// filled in for it, and suggesting one of its own operations or an action on its own
    /// contributions (with params it takes, neither too long nor holding those secrets), if any:
    /// one it may not suggest is left out, and logged. Anything else (the progress of a request
    /// nobody waits for any more) is dropped.
    pub(crate) fn notified(&self, live: &Live, name: &str, params: Value) {
        if name != method::HOST_NOTIFY {
            return;
        }
        let id = &self.manifest.id;
        if !live.sends(method::HOST_NOTIFY) {
            return self.host.log(id, "it tells the user something without the events or services feature");
        }
        let mut notice: HostNotify = match serde_json::from_value(params) {
            Ok(notice) => notice,
            Err(e) => return self.host.log(id, &format!("what it tells the user isn't host/notify's: {e}")),
        };
        let said = printable(&self.net.scrub(&notice.message));
        let message: String = said.trim().chars().take(MAX_MESSAGE).collect();
        if message.is_empty() {
            return self.host.log(id, "it tells the user nothing");
        }
        // Kept and shown (planned) as the message is.
        for s in notice.operation.iter_mut().chain(notice.action.iter_mut()) {
            self.net.scrub_value(&mut s.params);
        }
        let suggests = notice.suggestion(&self.manifest).and_then(|s| match s {
            Some(Suggestion::Action(a)) => self.on_known_drive(live, a).map(|a| Some(Suggestion::Action(a))),
            s => Ok(s),
        });
        let suggests = suggests.unwrap_or_else(|refused| {
            self.host.log(id, &refused);
            None
        });
        let suggests = match (suggests, &live.store) {
            (Some(s), Some(store)) => match unseen(store, id, s.clone()) {
                Ok(Some(new)) => Some(new),
                Ok(None) => {
                    return self.host.log(id, "it suggests only what was reviewed, dismissed or is waiting: left out");
                }
                Err(e) => {
                    self.host.log(id, &format!("what was suggested before can't be read: {e}"));
                    Some(s)
                }
            },
            (s, _) => s,
        };
        (self.host.options.notices)(PluginNotice { plugin: id.clone(), message, suggests });
    }

    /// `action` on the drive it names as this computer knows it, by id (else by name), so a front
    /// end never takes it for a folder; its tracks those the drive has, each once (the others left
    /// out, and logged). Why not, when the drive isn't one known here, or has none of its tracks.
    /// Found as its reads find drives: in the store the plugin was started in.
    fn on_known_drive(&self, live: &Live, mut action: HostAction) -> std::result::Result<HostAction, String> {
        let (Some(drive), tracks) = action.scope_mut() else { return Ok(action) };
        let store = live.store.as_ref().map(|s| s.root().to_path_buf());
        crate::api::THREAD_STORE_ROOT.with(|r| *r.borrow_mut() = store);
        let device = self.host.reads.drive(drive).and_then(|root| crate::Device::open(&root));
        let device =
            device.map_err(|e| format!("it suggests an action on {drive:?}, which isn't a drive known here: {e}"))?;
        let name = device.name();
        *drive = device.id().map_or_else(|| name.clone(), |id| id.to_string());
        if let Some(tracks) = tracks.filter(|t| !t.is_empty()) {
            let library = device.library().map_err(|e| format!("it suggests an action on {name}: {e}"))?;
            let (on, missing) = tracks_on(&library, tracks);
            if on.is_empty() {
                return Err(format!("it suggests an action on tracks {name} doesn't have"));
            }
            if missing > 0 {
                self.host
                    .log(&self.manifest.id, &format!("{missing} of the tracks it suggests aren't on {name}: left out"));
            }
            *tracks = on;
        }
        Ok(action)
    }
}

/// Which of `tracks` (by id) `library` has, each once and in their order, and how many others
/// there are.
pub fn tracks_on(library: &crate::Library, tracks: &[u32]) -> (Vec<u32>, usize) {
    let has: FxHashSet<u32> = library.tracks.iter().map(|t| t.id).collect();
    let mut seen = FxHashSet::default();
    let (on, missing): (Vec<u32>, Vec<u32>) = tracks.iter().filter(|t| seen.insert(**t)).partition(|t| has.contains(t));
    (on, missing.len())
}

/// Sends the plugin its events as they come, until none has come for [`IDLE`] or the host has
/// let go of it. Holding the plugin only while it sends one, it never keeps it running.
fn send_queued(slot: &Weak<Slot>, queue: &Queue) {
    loop {
        let mut waiting = queue.lock();
        let next = loop {
            if let Some(next) = waiting.events.pop_front() {
                waiting.sending = true;
                break next;
            }
            let (now, waited) = queue.changed.wait_timeout(waiting, IDLE).unwrap_or_else(PoisonError::into_inner);
            waiting = now;
            if waited.timed_out() && waiting.events.is_empty() {
                waiting.worker = false;
                return;
            }
        };
        drop(waiting);
        let sent = slot.upgrade().map(|s| s.send_event(next.0, next.1.as_ref()));
        queue.lock().sending = false;
        queue.changed.notify_all();
        if sent.is_none() {
            return;
        }
    }
}

/// Held while the suggestions kept are read and written: plugins tell the user things at once,
/// each on its own thread. (Another process changing them too waits on `suggested.lock`.)
static KEEPING: Mutex<()> = Mutex::new(());

/// Where suggestions are kept for the player: `<store>/plugins/suggested.json`.
fn suggested_file(store: &Store) -> std::path::PathBuf {
    store.root().join("plugins").join("suggested.json")
}

/// Where suggestions reviewed or dismissed are remembered: `<store>/plugins/seen.json`.
fn seen_file(store: &Store) -> std::path::PathBuf {
    store.root().join("plugins").join("seen.json")
}

/// The most suggestions remembered as reviewed or dismissed (see [`forget_suggested`]), each
/// plugin's action on a drive once with all its tracks: the oldest go.
const MAX_SEEN: usize = 200;

/// A suggestion reviewed or dismissed: its plugin, what it suggests (an action without its
/// tracks, as JSON, so one this version doesn't know is kept as it is), and its tracks, every one
/// reviewed or dismissed for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Seen {
    plugin: String,
    suggests: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tracks: Vec<u32>,
}

/// What `suggests` is without its tracks, as JSON, and its tracks (none for every one, or for a
/// suggestion that has none).
fn scope(suggests: &Suggestion) -> (Value, Vec<u32>) {
    let mut without = suggests.clone();
    let tracks = match &mut without {
        Suggestion::Action(a) => a.scope_mut().1.map(std::mem::take).unwrap_or_default(),
        Suggestion::Operation(_) => Vec::new(),
    };
    (serde_json::to_value(&without).expect("JSON"), tracks)
}

/// `suggests` without what plugin `plugin` suggested before, still waiting in `store` or
/// reviewed or dismissed since: none when that's all of it. An action on tracks keeps those not
/// suggested before; any other suggestion is all new or none.
fn unseen(store: &Store, plugin: &str, mut suggests: Suggestion) -> Result<Option<Suggestion>> {
    let (key, tracks) = scope(&suggests);
    let (kept, seen) = {
        let _keeping = lock(&KEEPING);
        (read_kept(&suggested_file(store))?.0, read_seen(store)?)
    };
    let waiting = kept.iter().filter_map(|n| Some((n.plugin.as_str(), scope(n.suggests.as_ref()?))));
    let before = seen.iter().map(|s| (s.plugin.as_str(), (s.suggests.clone(), s.tracks.clone())));
    let mut had = FxHashSet::default();
    for (_, (_, t)) in waiting.chain(before).filter(|(by, (k, _))| *by == plugin && *k == key) {
        // One on every track had them all.
        if t.is_empty() {
            return Ok(None);
        }
        had.extend(t);
    }
    if tracks.is_empty() {
        return Ok(Some(suggests));
    }
    let new: Vec<u32> = tracks.into_iter().filter(|t| !had.contains(t)).collect();
    if new.is_empty() {
        return Ok(None);
    }
    if let Suggestion::Action(a) = &mut suggests
        && let (_, Some(tracks)) = a.scope_mut()
    {
        *tracks = new;
    }
    Ok(Some(suggests))
}

/// The suggestions remembered as reviewed or dismissed.
fn read_seen(store: &Store) -> Result<Vec<Seen>> {
    Ok(fsutil::read_json_opt(&seen_file(store))?.unwrap_or_default())
}

/// The suggestions kept for the player.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Suggestions {
    /// Those this version can offer, oldest first.
    pub notices: Vec<PluginNotice>,
    /// How many it can't: another version's (a newer one suggesting what this one doesn't know).
    /// They're left as they are.
    pub unread: usize,
}

/// The suggestions kept in `file`, as this version reads them, and those it can't.
fn read_kept(file: &std::path::Path) -> Result<(Vec<PluginNotice>, Vec<Value>)> {
    let all: Vec<Value> = fsutil::read_json_opt(file)?.unwrap_or_default();
    // One whose suggestion doesn't read would read without it (it's flattened in): that's not one
    // this version can offer.
    let read = |v: &Value| {
        let notice: PluginNotice = serde_json::from_value(v.clone()).ok()?;
        let suggests = v.get("operation").is_some() || v.get("action").is_some();
        (notice.suggests.is_some() == suggests).then_some(notice)
    };
    let (mut notices, mut unread) = (Vec::new(), Vec::new());
    for v in all {
        match read(&v) {
            Some(notice) => notices.push(notice),
            None => unread.push(v),
        }
    }
    Ok((notices, unread))
}

/// The suggestions kept for the player to offer: none when none are kept, and an error when the
/// file they're kept in can't be read (it's left as it is).
pub fn suggested(store: &Store) -> Result<Suggestions> {
    let _keeping = lock(&KEEPING);
    let (notices, unread) = read_kept(&suggested_file(store))?;
    Ok(Suggestions { notices, unread: unread.len() })
}

/// `change`s the suggestions kept, and answers them as they're kept now; those this version can't
/// read are kept as they are, first. The read and the write happen under a lock of their own
/// (`suggested.lock` beside them, waited for), so the player and the `watch` agent keeping and
/// forgetting them at once don't drop each other's changes. (Read alone, they needn't wait:
/// they're written whole, in place at once.) With `seen`, that suggestion is remembered as
/// reviewed or dismissed first, under the same locks.
fn change_kept(
    store: &Store,
    change: impl FnOnce(&mut Vec<PluginNotice>),
    seen: Option<&PluginNotice>,
) -> Result<Vec<PluginNotice>> {
    let _keeping = lock(&KEEPING);
    let file = suggested_file(store);
    let dir = file.parent().expect("in a folder");
    std::fs::create_dir_all(dir).at(dir)?;
    let _changing = fsutil::FileLock::wait(&dir.join("suggested.lock"))?;
    let (mut kept, unread) = read_kept(&file)?;
    if let Some(notice) = seen {
        remember(store, notice)?;
    }
    change(&mut kept);
    let all: Vec<Value> =
        unread.into_iter().chain(kept.iter().map(|n| serde_json::to_value(n).expect("JSON"))).collect();
    fsutil::write_json_atomic(&file, &all)?;
    Ok(kept)
}

/// Keeps a plugin's suggestion (an operation or an action) for the player to offer until it's
/// reviewed or dismissed ([`forget_suggested`]), as the player and the `watch` agent do with what
/// they're told: the newest [`MAX_SUGGESTED`]. Answers those kept now.
pub fn keep_suggested(store: &Store, notice: &PluginNotice) -> Result<Vec<PluginNotice>> {
    change_kept(
        store,
        |kept| {
            kept.push(notice.clone());
            kept.drain(..kept.len().saturating_sub(MAX_SUGGESTED));
        },
        None,
    )
}

/// No longer keeps a suggestion (the oldest the same as `notice`): it was reviewed or dismissed.
/// It's remembered, so its plugin suggesting it again (or an action on the same tracks) is left
/// out (the newest [`MAX_SEEN`] remembered). Answers those kept now.
pub fn forget_suggested(store: &Store, notice: &PluginNotice) -> Result<Vec<PluginNotice>> {
    let forget = |kept: &mut Vec<PluginNotice>| {
        if let Some(i) = kept.iter().position(|k| k == notice) {
            kept.remove(i);
        }
    };
    change_kept(store, forget, Some(notice))
}

/// Remembers `notice`'s suggestion as reviewed or dismissed: with its tracks added to those of
/// the same action remembered before (every track, when either was on every one).
fn remember(store: &Store, notice: &PluginNotice) -> Result<()> {
    let Some(suggests) = &notice.suggests else { return Ok(()) };
    let (key, tracks) = scope(suggests);
    let mut seen = read_seen(store)?;
    let before = seen.iter().position(|s| s.plugin == notice.plugin && s.suggests == key);
    let tracks = match before.map(|i| seen.remove(i).tracks) {
        None => tracks,
        Some(had) if had.is_empty() || tracks.is_empty() => Vec::new(),
        Some(mut had) => {
            let mut known: FxHashSet<u32> = had.iter().copied().collect();
            had.extend(tracks.into_iter().filter(|t| known.insert(*t)));
            had
        }
    };
    seen.push(Seen { plugin: notice.plugin.clone(), suggests: key, tracks });
    seen.drain(..seen.len().saturating_sub(MAX_SEEN));
    fsutil::write_json_atomic(&seen_file(store), &seen)
}

#[cfg(test)]
mod tests;
