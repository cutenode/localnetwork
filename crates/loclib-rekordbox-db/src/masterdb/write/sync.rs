//! Whether Cloud Library Sync is on, told conservatively: anything that hints at it counts, and so
//! does not being able to tell.
//!
//! In a library sync was never on (rekordbox 7.2.18, 15,814 rows), `usn` is NULL on every row,
//! `rb_local_synced` is 0, `cloudAgentRegistry` is empty, `agentRegistry` has no
//! `lastUpdateCount` (DJCrate #229 sees it once sync is on), and the agent's log says
//! `sync mode: standalone => standalone` each time it starts. `djmdCloudProperty` names a storage
//! service (`SyncService` `Dropbox`) even then, so it says nothing either way.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rusqlite::Connection;

use super::tables_with;
use crate::{Error, Result};

/// How much of the end of the agent's log is read first (it grows to tens of MB).
const LOG_TAIL: u64 = 4 << 20;

/// Why Cloud Library Sync looks to be on in the database `conn` has open, with the agent's log at
/// `agent_log`; `None` when everything says it's off. A log that can't be read, or that never says
/// which mode the agent is in, counts as on.
pub fn sync_on(conn: &Connection, agent_log: Option<&Path>) -> Result<Option<String>> {
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
    if tables_with(conn, "registry_id").map_err(sql)?.iter().any(|t| t == "agentRegistry")
        && any(conn, "SELECT 1 FROM agentRegistry WHERE registry_id = 'lastUpdateCount'").map_err(sql)?
    {
        return Ok(Some("agentRegistry has a lastUpdateCount, which only a synced library has".into()));
    }
    let Some(log) = agent_log else {
        return Ok(Some("rekordbox's agent's log, which says, wasn't found".into()));
    };
    Ok(match last_sync_mode(log) {
        Ok(Some(mode)) if mode == "standalone" => None,
        Ok(Some(mode)) => Some(format!("rekordbox's agent last said its sync mode is {mode}")),
        Ok(None) => Some(format!("{} never says which sync mode the agent is in", log.display())),
        Err(e) => Some(format!("{} can't be read: {e}", log.display())),
    })
}

fn has_table(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1")?.exists([name])
}

fn any(conn: &Connection, sql: &str) -> rusqlite::Result<bool> {
    conn.prepare(sql)?.exists([])
}

/// The sync mode the agent's log at `path` last records: its lines say
/// `[261126T012213.824-0600] I/sync mode: standalone => standalone` (what it was, what it is now)
/// each time it starts. The end of the log is read first, then the whole of it.
fn last_sync_mode(path: &Path) -> std::io::Result<Option<String>> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let from = len.saturating_sub(LOG_TAIL);
    file.seek(SeekFrom::Start(from))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    if let Some(mode) = sync_mode(&tail) {
        return Ok(Some(mode));
    }
    if from == 0 {
        return Ok(None);
    }
    Ok(sync_mode(&std::fs::read(path)?))
}

/// The mode the last `sync mode:` line in `log` ends at.
fn sync_mode(log: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(log);
    let line = text.lines().rev().find_map(|l| l.split_once("sync mode:").map(|(_, rest)| rest))?;
    let now = line.rsplit("=>").next().unwrap_or(line).trim();
    Some(now.to_string())
}

#[cfg(test)]
mod tests;
