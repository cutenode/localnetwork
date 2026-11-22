use loclib_sqlcipher::{Codec, PAGE_SIZE};
use rusqlite::Connection;

/// A database whose b-tree for `table` claims more cells than a page holds, re-encrypted.
pub fn with_broken_table(conn: &Connection, codec: &Codec, mut plain: Vec<u8>, table: &str) -> Vec<u8> {
    let root: u32 =
        conn.query_row("SELECT rootpage FROM sqlite_master WHERE name = ?1", [table], |r| r.get(0)).unwrap();
    let at = (root as usize - 1) * PAGE_SIZE;
    plain[at + 3..at + 5].copy_from_slice(&u16::MAX.to_be_bytes());
    codec.encrypt(&plain).unwrap()
}
