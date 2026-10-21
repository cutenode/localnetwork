//! What plugins hear of: what the player, the sidecar and the `watch` agent notice, as the
//! sidecar's `event` notifications say it, and plans applied (by any of them, or a command run
//! once), each told to the plugins listening for it (see [`locallibrary::plugin::Event`]). And
//! where what they tell the user goes, while a front end shows it.

use std::sync::{Arc, Mutex, PoisonError};

use locallibrary::api::{self, Notice};
use locallibrary::plugin::{Event, Log, Notices};
use locallibrary::{Store, op};

/// Where what plugins tell the user goes, instead of their log, while a front end shows it.
static NOTICES: Mutex<Option<Notices>> = Mutex::new(None);

/// Sends what plugins tell the user to `notices` (the player's messages, the sidecar's events,
/// the `watch` agent's output), or with `None` back to their log.
pub fn notices_to(notices: Option<Notices>) {
    *NOTICES.lock().unwrap_or_else(PoisonError::into_inner) = notices;
}

/// What the host tells the user with: the front end's [`notices_to`], else `log`.
pub(super) fn notices(log: Log) -> Notices {
    Arc::new(move |notice| {
        let to = NOTICES.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match to {
            Some(to) => to(notice),
            None => log(&notice.plugin, &notice.message),
        }
    })
}

/// Has every plan applied from now on told to the plugins listening (`plan.applied`).
pub(super) fn tell_applied() {
    op::on_applied(|applied, store| {
        if let Some(plugins) = super::checked() {
            plugins.deliver(&Event::applied(applied), store);
        }
    });
}

/// Tells the plugins listening of what `notice` says happened, if anything they hear of: in the
/// store the command line names. Never waits for them, but loads them if nothing has yet (what
/// a front end notices first isn't lost): never call it on the player's frame path.
pub fn deliver(notice: &Notice) {
    let events = Event::of(notice);
    let Ok(plugins) = super::host() else { return };
    if !events.iter().any(|e| plugins.listens(e.name)) {
        return;
    }
    let store = api::store_root().and_then(|root| Store::open(root).ok());
    for event in &events {
        plugins.deliver(event, store.as_ref());
    }
}
