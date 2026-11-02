//! Plugins' services (`service/start`): each held open on a thread of its own until it's stopped
//! (`$/cancel`), which keeps its plugin from being stopped as idle. Only a host that stays open
//! runs them (the player, the sidecar, the `watch` agent), and only those the user turned on
//! (`services.<name>`, among the plugin's settings). One that stops or fails is started again
//! after a while, longer each time, and left off once it has failed
//! [`super::Options::restarts`] times in a row, saying so.

use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use loclib_plugin::{Context, StartService, method};
use serde_json::Value;

use super::events::PluginNotice;
use super::process::Slot;
use super::{Plugins, SERVICES};
use crate::lock;
use crate::progress::{CancelToken, Ctx, NoProgress};
use crate::store::Store;

/// A service that ran this long before it stopped was working: its failures are counted afresh.
const HEALTHY: Duration = Duration::from_secs(60);

/// The longest a service waits to be started again.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// A service running, on its thread.
pub(crate) struct Running {
    cancel: CancelToken,
    thread: JoinHandle<()>,
}

impl Running {
    fn stop(self) {
        self.cancel.cancel();
        let _ = self.thread.join();
    }
}

impl Plugins {
    /// Starts each service the user turned on (in `store`) that isn't running, and stops each
    /// running one they turned off. For a host that stays open: never the command line's
    /// commands.
    pub fn run_services(&self, store: Option<&Store>) {
        let mut stopping = Vec::new();
        for slot in self.plugins.values() {
            let mut running = lock(&slot.services);
            for service in &slot.manifest.contributes.services {
                let on = store.is_some_and(|s| slot.enabled(s, &service.name));
                let alive = running.get(&service.name).is_some_and(|r| !r.thread.is_finished());
                match (on, alive) {
                    (true, true) => {}
                    (true, false) => {
                        let cancel = CancelToken::new();
                        let (weak, name, store, stop) =
                            (Arc::downgrade(slot), service.name.clone(), store.cloned(), cancel.clone());
                        let thread = std::thread::spawn(move || supervise(&weak, &name, store.as_ref(), &stop));
                        running.insert(service.name.clone(), Running { cancel, thread });
                    }
                    (false, _) => stopping.extend(running.shift_remove(&service.name)),
                }
            }
        }
        // Each given its grace, all at once.
        stopping.iter().for_each(|r| r.cancel.cancel());
        stopping.into_iter().for_each(Running::stop);
    }

    /// Whether the plugin's service is running now (or waiting to be started again).
    pub fn service_running(&self, plugin: &str, service: &str) -> bool {
        let Ok(slot) = self.slot(plugin) else { return false };
        lock(&slot.services).get(service).is_some_and(|r| !r.thread.is_finished())
    }
}

impl Slot {
    /// Whether the user turned the service on: its setting `services.<name>`.
    fn enabled(&self, store: &Store, service: &str) -> bool {
        let saved = self.saved(store).unwrap_or_default();
        saved.get(&format!("{SERVICES}{service}")).and_then(Value::as_bool) == Some(true)
    }

    /// Stops the plugin's services, each given its grace, all at once, and waits for them.
    pub(crate) fn stop_services(&self) {
        let running: Vec<Running> = lock(&self.services).drain(..).map(|(_, r)| r).collect();
        running.iter().for_each(|r| r.cancel.cancel());
        running.into_iter().for_each(Running::stop);
    }
}

/// Runs the plugin's service until `cancel`, starting it again (after a while, longer each time)
/// whenever it stops or fails, until it has failed [`super::Options::restarts`] times in a row:
/// then it's left off, and the user is told.
fn supervise(slot: &Weak<Slot>, service: &str, store: Option<&Store>, cancel: &CancelToken) {
    let mut failed: u32 = 0;
    loop {
        let Some(s) = slot.upgrade() else { return };
        let started = Instant::now();
        let ctx = Ctx::new(&NoProgress, cancel.clone());
        let ran = s.context(Context::default(), store).and_then(|(context, ..)| {
            let start = StartService { service: service.into(), context };
            s.call::<Value>(method::SERVICE_START, &start, store, &ctx)
        });
        if cancel.is_cancelled() {
            return;
        }
        let why = match ran {
            Ok(_) => "it stopped by itself".to_string(),
            Err(e) => e.to_string(),
        };
        failed = if started.elapsed() >= HEALTHY { 1 } else { failed + 1 };
        let (id, options) = (&s.manifest.id, &s.host.options);
        if failed > options.restarts {
            let message = format!("its {service} service is off: it failed {failed} times in a row ({why})");
            s.host.log(id, &message);
            (options.notices)(PluginNotice { plugin: id.clone(), message, suggests: None });
            return;
        }
        let wait = options.backoff.saturating_mul(1u32 << (failed - 1).min(16)).min(MAX_BACKOFF);
        s.host.log(id, &format!("its {service} service stopped ({why}): starting it again in {wait:?}"));
        drop(s);
        let until = Instant::now() + wait;
        while let Some(left) = until.checked_duration_since(Instant::now()).filter(|l| !l.is_zero()) {
            if cancel.is_cancelled() {
                return;
            }
            std::thread::sleep(left.min(Duration::from_millis(50)));
        }
    }
}
