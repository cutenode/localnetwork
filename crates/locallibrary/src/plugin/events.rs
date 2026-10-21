//! Plugins' events (`event/notify`): what the hosts already say happens (drives coming and going
//! and saved into the store, the sidecar's requests finishing, plans applied), told to each
//! plugin listening for it whose capabilities grant it. Each plugin's waiting events are a queue
//! of their own, sent on a thread of its own (starting the plugin when it isn't running): a front
//! end only adds to it, never waiting, and once it's full the oldest is dropped. What a plugin
//! tells the user back (`host/notify`, with the `events` or `services` feature) goes to the
//! front end's [`super::Options::notices`].

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use loclib_plugin::{EventName, EventNotify, HostNotify, Suggested, method};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Plugins;
use super::callbacks::{answer_for_plugin, known_roots};
use super::process::{Live, Slot, printable};
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

/// The longest params an operation a plugin suggests may have, as JSON: they're kept to plan it.
const MAX_PARAMS: usize = 64 << 10;

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

/// A line a plugin tells the user (`host/notify`), and the operation of its it suggests, with
/// its params: the front end offers to plan it, and the user confirms it as any plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginNotice {
    /// The plugin's id.
    pub plugin: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Suggested>,
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

    /// A notification from the plugin: `host/notify`, from one with a feature that sends it,
    /// told to the front end once it's text the user can read, without the secrets the host
    /// filled in for it, and suggesting one of its own operations (with params neither too long
    /// nor holding those secrets), if any. Anything else (the progress of a request nobody waits
    /// for any more) is dropped.
    pub(crate) fn notified(&self, live: &Live, name: &str, params: Value) {
        if name != method::HOST_NOTIFY {
            return;
        }
        let id = &self.manifest.id;
        if !live.sends(method::HOST_NOTIFY) {
            return self.host.log(id, "it tells the user something without the events or services feature");
        }
        let notice: HostNotify = match serde_json::from_value(params) {
            Ok(notice) => notice,
            Err(e) => return self.host.log(id, &format!("what it tells the user isn't host/notify's: {e}")),
        };
        let said = printable(&self.net.scrub(&notice.message));
        let message: String = said.trim().chars().take(MAX_MESSAGE).collect();
        if message.is_empty() {
            return self.host.log(id, "it tells the user nothing");
        }
        let operation = notice.operation.filter(|o| {
            let ours = self.manifest.contributes.operations.iter().any(|x| x.name == o.name);
            let refused = if !ours {
                format!("it suggests {:?}, which isn't one of its operations", o.name)
            } else if !(o.params.is_null() || o.params.is_object()) {
                format!("it suggests {:?} with params that aren't an object", o.name)
            } else if o.params.to_string().len() > MAX_PARAMS {
                format!("it suggests {:?} with params longer than {MAX_PARAMS} bytes", o.name)
            } else {
                return true;
            };
            self.host.log(id, &refused);
            false
        });
        // Kept and shown (planned) as the message is.
        let operation = operation.map(|mut o| {
            self.net.scrub_value(&mut o.params);
            o
        });
        (self.host.options.notices)(PluginNotice { plugin: id.clone(), message, operation });
    }
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

/// The suggestions kept for the player to offer, oldest first: none when none are kept, and an
/// error when the file they're kept in can't be read (it's left as it is).
pub fn suggested(store: &Store) -> Result<Vec<PluginNotice>> {
    let _keeping = lock(&KEEPING);
    Ok(fsutil::read_json_opt(&suggested_file(store))?.unwrap_or_default())
}

/// `change`s the suggestions kept, and answers them as they're kept now. The read and the write
/// happen under a lock of their own (`suggested.lock` beside them, waited for), so the player and
/// the `watch` agent keeping and forgetting them at once don't drop each other's changes. (Read
/// alone, they needn't wait: they're written whole, in place at once.)
fn change_kept(store: &Store, change: impl FnOnce(&mut Vec<PluginNotice>)) -> Result<Vec<PluginNotice>> {
    let _keeping = lock(&KEEPING);
    let file = suggested_file(store);
    let dir = file.parent().expect("in a folder");
    std::fs::create_dir_all(dir).at(dir)?;
    let _changing = fsutil::FileLock::wait(&dir.join("suggested.lock"))?;
    let mut kept = fsutil::read_json_opt(&file)?.unwrap_or_default();
    change(&mut kept);
    fsutil::write_json_atomic(&file, &kept)?;
    Ok(kept)
}

/// Keeps a plugin's suggestion (one with an operation) for the player to offer until it's
/// reviewed or dismissed ([`forget_suggested`]), as the player and the `watch` agent do with what
/// they're told: the newest [`MAX_SUGGESTED`]. Answers those kept now.
pub fn keep_suggested(store: &Store, notice: &PluginNotice) -> Result<Vec<PluginNotice>> {
    change_kept(store, |kept| {
        kept.push(notice.clone());
        kept.drain(..kept.len().saturating_sub(MAX_SUGGESTED));
    })
}

/// No longer keeps a suggestion (the oldest the same as `notice`): it was reviewed or dismissed.
/// Answers those kept now.
pub fn forget_suggested(store: &Store, notice: &PluginNotice) -> Result<Vec<PluginNotice>> {
    change_kept(store, |kept| {
        if let Some(i) = kept.iter().position(|k| k == notice) {
            kept.remove(i);
        }
    })
}
