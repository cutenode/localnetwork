//! Reading SQLite's write-ahead log (`exportLibrary.db-wal`).
//!
//! rekordbox leaves `exportLibrary.db` in WAL mode, and its last changes often live only in the
//! WAL. Reading the database means doing what SQLite does when it opens one: take the frames
//! of every committed transaction and lay their pages over the main file (a checkpoint).
//!
//! Format (sqlite.org/fileformat2.html#walformat): a 32-byte header (magic, version, page size,
//! checkpoint sequence, two salts, checksum) followed by frames of a 24-byte header (page number,
//! database size in pages for a commit frame or 0, the salts, a cumulative checksum) and one page.
//! A frame counts only if its salts match the header and the checksum chain holds up to it; only
//! frames up to the last commit among those are applied. With SQLCipher the page images are
//! encrypted like the main file's pages, and the checksums cover the encrypted bytes.

use indexmap::IndexMap;

use crate::{Error, Result};

const HEADER: usize = 32;
const FRAME_HEADER: usize = 24;
const MAGIC_LE: u32 = 0x377f_0682;
const MAGIC_BE: u32 = 0x377f_0683;

/// Pages of every committed transaction in a WAL, and the database size after the last one.
#[derive(Debug, Default)]
pub struct Committed<'a> {
    /// Latest committed image of each page, by 1-based page number.
    pub pages: IndexMap<u32, &'a [u8]>,
    pub db_pages: u32,
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

/// SQLite's WAL checksum: Fletcher-like sums over 32-bit words in the byte order the magic
/// number names, continuing from `seed`.
fn checksum(big_endian: bool, data: &[u8], seed: (u32, u32)) -> (u32, u32) {
    let (mut s1, mut s2) = seed;
    for pair in data.chunks_exact(8) {
        let word = |b: &[u8]| {
            let a: [u8; 4] = b.try_into().expect("4 bytes");
            if big_endian { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) }
        };
        s1 = s1.wrapping_add(word(&pair[..4])).wrapping_add(s2);
        s2 = s2.wrapping_add(word(&pair[4..])).wrapping_add(s1);
    }
    (s1, s2)
}

/// The committed frames of `wal`. `None` when it holds nothing to apply: empty, not a WAL, a
/// header that fails its checksum (SQLite ignores such a file), or no complete transaction.
pub fn committed(wal: &[u8], page_size: usize) -> Result<Option<Committed<'_>>> {
    if wal.len() < HEADER {
        return Ok(None);
    }
    let magic = be32(wal, 0);
    if magic != MAGIC_LE && magic != MAGIC_BE {
        return Ok(None);
    }
    let big_endian = magic == MAGIC_BE;
    let seed = checksum(big_endian, &wal[..24], (0, 0));
    if seed != (be32(wal, 24), be32(wal, 28)) {
        return Ok(None);
    }
    if be32(wal, 8) as usize != page_size {
        return Err(Error::OneLibrary(format!("the WAL uses {}-byte pages, the database {page_size}", be32(wal, 8))));
    }
    let salts = (be32(wal, 16), be32(wal, 20));

    let mut out = Committed::default();
    let mut pending: IndexMap<u32, &[u8]> = IndexMap::new();
    let mut sum = seed;
    for frame in wal[HEADER..].chunks_exact(FRAME_HEADER + page_size) {
        let (head, page) = frame.split_at(FRAME_HEADER);
        let pgno = be32(head, 0);
        if pgno == 0 || (be32(head, 8), be32(head, 12)) != salts {
            break;
        }
        sum = checksum(big_endian, page, checksum(big_endian, &head[..8], sum));
        if sum != (be32(head, 16), be32(head, 20)) {
            break;
        }
        pending.insert(pgno, page);
        let commit = be32(head, 4);
        if commit != 0 {
            out.pages.append(&mut pending);
            out.db_pages = commit;
        }
    }
    Ok((out.db_pages > 0).then_some(out))
}

#[cfg(test)]
pub(crate) mod tests;
