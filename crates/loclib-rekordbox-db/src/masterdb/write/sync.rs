//! Whether Cloud Library Sync is on, told conservatively: anything that hints at it counts, and so
//! does not being able to tell.
//!
//! In a library sync was never on (rekordbox 7.2.18, 15,814 rows), `usn` is NULL on every row,
//! `rb_local_synced` is 0, `cloudAgentRegistry` is empty, `agentRegistry` has no
//! `lastUpdateCount` (DJCrate #229 sees it once sync is on) nor a `lastSyncTime` holding a time,
//! and the agent's log says `sync mode: standalone => standalone` each time it starts.
//! `djmdCloudProperty` names a storage service (`SyncService` `Dropbox`) even then, so it says
//! nothing either way.
//!
//! The log speaks only for the agent's last run, and only while it's as new as the collection.
//! Each run starts the log with a banner (`rekordboxAgent/3.1.4 (darwin_arm64; 27.0.0)`) and then
//! says its sync mode; the agent logs while it runs, and closes `master.db` after rekordbox has,
//! as both quit. A log last written well before `master.db` is one the agent no longer writes (a
//! version that logs elsewhere, or a library brought from another computer).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use super::tables_with;
use crate::{Error, Result};

/// How much of the end of the agent's log is read first (it grows to tens of MB).
const LOG_TAIL: u64 = 4 << 20;

/// How much older than `master.db` the agent's log can be and still speak for it.
const LOG_LAG: Duration = Duration::from_secs(10 * 60);

/// Why Cloud Library Sync looks to be on in the database `conn` has open (a copy of `master`),
/// with the agent's log at `agent_log`; `None` when everything says it's off. A log that can't be
/// read, that doesn't say which mode the agent's last run is in, or that's older than `master`,
/// counts as on.
pub fn sync_on(conn: &Connection, agent_log: Option<&Path>, master: &Path) -> Result<Option<String>> {
    let sql = |e: rusqlite::Error| Error::OneLibrary(e.to_string());
    for table in tables_with(conn, "usn").map_err(sql)? {
        if any(conn, &format!("SELECT 1 FROM \"{table}\" WHERE usn IS NOT NULL LIMIT 1")).map_err(sql)? {
            return Ok(Some(format!("rows in {table} carry the cloud's update numbers (usn)")));
        }
    }
    for table in tables_with(conn, "rb_local_synced").map_err(sql)? {
        let synced = format!("SELECT 1 FROM \"{table}\" WHERE IFNULL(rb_local_synced, 0) != 0 LIMIT 1");
        if any(conn, &synced).map_err(sql)? {
            return Ok(Some(format!("rows in {table} are marked synced (rb_local_synced)")));
        }
    }
    if has_table(conn, "cloudAgentRegistry").map_err(sql)?
        && any(conn, "SELECT 1 FROM cloudAgentRegistry LIMIT 1").map_err(sql)?
    {
        return Ok(Some("the cloud agent keeps state in cloudAgentRegistry".into()));
    }
    if tables_with(conn, "registry_id").map_err(sql)?.iter().any(|t| t == "agentRegistry") {
        if any(conn, "SELECT 1 FROM agentRegistry WHERE registry_id = 'lastUpdateCount'").map_err(sql)? {
            return Ok(Some("agentRegistry has a lastUpdateCount, which only a synced library has".into()));
        }
        let values: Vec<String> = super::cues::columns(conn, "agentRegistry")
            .map_err(sql)?
            .into_iter()
            .filter(|c| !["registry_id", "created_at", "updated_at"].contains(&c.as_str()))
            .map(|c| format!("\"{c}\" IS NOT NULL"))
            .collect();
        let synced =
            format!("SELECT 1 FROM agentRegistry WHERE registry_id = 'lastSyncTime' AND ({})", values.join(" OR "));
        if !values.is_empty() && any(conn, &synced).map_err(sql)? {
            return Ok(Some("agentRegistry has a time it last synced (lastSyncTime)".into()));
        }
    }
    let Some(log) = agent_log else {
        return Ok(Some("rekordbox's agent's log, which says, wasn't found".into()));
    };
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if let (Some(logged), Some(written)) = (modified(log), modified(master))
        && logged + LOG_LAG < written
    {
        return Ok(Some(format!(
            "{} was last written well before master.db, so it isn't the agent's now",
            log.display()
        )));
    }
    Ok(match last_sync_mode(log) {
        Ok(Some(mode)) if mode == "standalone" => None,
        Ok(Some(mode)) => Some(format!("rekordbox's agent last said its sync mode is {mode}")),
        Ok(None) => Some(format!("{} doesn't say which sync mode the agent's last run is in", log.display())),
        Err(e) => Some(format!("{} can't be read: {e}", log.display())),
    })
}

fn has_table(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1")?.exists([name])
}

fn any(conn: &Connection, sql: &str) -> rusqlite::Result<bool> {
    conn.prepare(sql)?.exists([])
}

/// The sync mode the agent's last run records in its log at `path` (see [`last_run`]). The end of
/// the log is read first, then the whole of it. `None` when its last run says none, or no run's
/// start is in it.
fn last_sync_mode(path: &Path) -> std::io::Result<Option<String>> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let from = len.saturating_sub(LOG_TAIL);
    file.seek(SeekFrom::Start(from))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    if let Some(said) = last_run(&tail) {
        return Ok(said);
    }
    if from == 0 {
        return Ok(None);
    }
    Ok(last_run(&std::fs::read(path)?).flatten())
}

/// The mode the last run of the agent in `log` says it's in: its lines say
/// `[261126T012213.824-0600] I/sync mode: standalone => standalone` (what it was, what it is now)
/// after the banner the run starts with (`  rekordboxAgent/3.1.4 (darwin_arm64; 27.0.0)`).
/// `Some(None)` when that run says none; `None` when `log` holds no run's start.
fn last_run(log: &[u8]) -> Option<Option<String>> {
    let text = String::from_utf8_lossy(log);
    let mut mode = None;
    for line in text.lines().rev() {
        if mode.is_none()
            && let Some((_, said)) = line.split_once("sync mode:")
        {
            mode = Some(said.rsplit("=>").next().unwrap_or(said).trim().to_string());
        }
        if line.trim_start().starts_with("rekordboxAgent/") {
            return Some(mode);
        }
    }
    None
}

#[cfg(test)]
mod tests;
