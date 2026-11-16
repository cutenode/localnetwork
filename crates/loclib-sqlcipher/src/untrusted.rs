//! Opening a SQLite database someone else wrote (a decrypted one, or any other) with rusqlite, so
//! that reading it can't be turned against the reader. Behind the `rusqlite` feature.

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OptionalExtension};

/// Make `conn`, open on a database someone else wrote, safe to run SQL on: SQL in its schema may
/// only call functions without side effects, no SQL may corrupt the file on purpose (SQLite's
/// defensive mode), a malformed b-tree cell is an error rather than read out of bounds, and a
/// schema object of one of the `refused` kinds, which `maker` never makes, is refused: a view
/// standing in for a table could loop for ever when read, a trigger would run on every edit made
/// here. `Some` says which one the file has.
pub fn distrust(conn: &Connection, refused: &[&str], maker: &str) -> rusqlite::Result<Option<String>> {
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
    conn.execute_batch("PRAGMA trusted_schema = OFF; PRAGMA cell_size_check = ON")?;
    for kind in refused {
        let name: Option<String> =
            conn.query_row("SELECT name FROM sqlite_master WHERE type = ?1", [kind], |r| r.get(0)).optional()?;
        if let Some(name) = name {
            return Ok(Some(format!("it has a {kind} {name}, which {maker} never makes")));
        }
    }
    Ok(None)
}

/// SQLite's structural check of every b-tree in `conn`'s database, which SQLite advises before
/// reading a file someone else wrote. It reads the whole file, so it's for databases of a few
/// hundred MB at most (rekordbox's), not Audacity projects full of audio. `Some` says what's wrong.
pub fn damaged(conn: &Connection) -> rusqlite::Result<Option<String>> {
    let check: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    Ok((check != "ok").then(|| format!("SQLite finds it damaged: {check}")))
}

#[cfg(test)]
mod tests;
