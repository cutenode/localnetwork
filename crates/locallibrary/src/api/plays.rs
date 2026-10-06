//! History lists and play statistics.

use schemars::JsonSchema;
use serde::Deserialize;

use super::diff::{Endpoint, resolve_endpoint, snapshot};
use super::{DriveRef, resolve_drive};
use crate::error::Reason;
use crate::plays::{self, DriveHistory, HistoryArchive, PlayStats};
use crate::progress::Ctx;
use crate::store::{Snapshot, Store};
use crate::{Error, Result};

/// The history lists on a drive (both databases).
pub fn drive_history(req: &DriveRef) -> Result<DriveHistory> {
    plays::drive_history(&resolve_drive(&req.drive)?)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct HistoryArchiveRequest {
    /// Only this library (name or id).
    #[serde(default)]
    pub library: Option<String>,
    /// Only the library this drive is linked to (every library when it isn't linked).
    #[serde(default)]
    pub drive: Option<String>,
}

/// Every history list the store's snapshots hold, each once, oldest first: the lists rekordbox
/// deleted from drives when it imported them, but `pull` kept.
pub fn history_archive(store: &Store, req: &HistoryArchiveRequest, ctx: &Ctx<'_>) -> Result<HistoryArchive> {
    let library = match (&req.library, &req.drive) {
        (Some(name), _) => {
            Some(store.find_library(name)?.ok_or_else(|| Error::NotFound(format!("library {name:?}")))?.id)
        }
        (None, Some(drive)) => resolve_drive(drive)?.state().and_then(|s| s.library_id),
        (None, None) => None,
    };
    plays::archive(store, library, ctx)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlayStatsRequest {
    /// A drive, `snapshot:<id prefix>`, or a library (its latest snapshot).
    pub source: String,
    /// Also report what was played since this snapshot (a label, `snapshot:<id prefix>` or an id
    /// prefix), or since this date (`2026`, `2026-09` or `2026-09-30`: since the newest snapshot
    /// of the source's library from before it).
    #[serde(default)]
    pub since: Option<String>,
    /// How many of the most played tracks to list (default 20).
    #[serde(default)]
    pub limit: Option<usize>,
}

pub fn play_stats(store: Option<&Store>, req: &PlayStatsRequest) -> Result<PlayStats> {
    let source = resolve_endpoint(store, &req.source)?;
    let current = source.databases()?;
    let mut stats = plays::stats(source.label(), &current, req.limit.unwrap_or(20));
    if let Some(since) = &req.since {
        let store = store
            .ok_or_else(|| Error::invalid("play counts since a snapshot need the store").because(Reason::NoStore))?;
        let snap = since_snapshot(store, &source, since)?;
        let before = crate::doctor::Databases::from_snapshot(store, &snap)?;
        stats.since = Some(plays::played_since(&before, &snap, &current));
    }
    Ok(stats)
}

/// `since` as a snapshot: a label, a date (the newest snapshot of the source's library from
/// before it), or a snapshot id. A date wins over an id prefix of four digits (`snapshot:2026`
/// is the id).
fn since_snapshot(store: &Store, source: &Endpoint<'_>, since: &str) -> Result<Snapshot> {
    let is_date = since.trim().split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    if !is_date || store.find_label(since)?.is_some() {
        return snapshot(store, since);
    }
    let day = super::parse_day(since, false)?;
    let start = day
        .and_hms_opt(0, 0, 0)
        .and_then(|d| d.and_local_timezone(chrono::Local).earliest())
        .map(|d| d.with_timezone(&chrono::Utc))
        .ok_or_else(|| Error::invalid(format!("{since:?} isn't a date")))?;
    let library_id = match source {
        Endpoint::Drive(d) => d.state().and_then(|s| s.library_id).ok_or_else(|| {
            Error::NotFound(format!("{} isn't linked to a library in the store (pull it first)", d.name()))
        })?,
        Endpoint::Snapshot(_, s) => s.library_id,
    };
    let lib = store.library_by_id(library_id)?.ok_or_else(|| Error::NotFound("the source's library".into()))?;
    let history = store.history(&lib, None)?;
    let Some(before) = history.iter().find(|s| s.created < start) else {
        let oldest =
            history.last().map(|s| format!("; the oldest is {} ({})", s.short_id, s.created.format("%Y-%m-%d")));
        return Err(Error::NotFound(format!(
            "a snapshot of {} from before {since}{}",
            lib.name,
            oldest.unwrap_or_default()
        )));
    };
    store.load_snapshot(&before.id)
}
