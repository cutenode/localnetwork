#![doc = include_str!("../README.md")]

mod cipher;
mod error;
pub mod wal;

#[cfg(feature = "rusqlite")]
pub mod untrusted;

pub use cipher::{Codec, Keys, LOCK_PAGE, PAGE_SIZE, RESERVE, SALT_LEN, SQLITE_MAGIC, check_plain_header, derive_keys};
pub use error::{Error, Result};

/// Decrypt a database and replay the committed part of its WAL: the database as SQLite opening
/// it would see it, as a plain SQLite file.
pub fn decrypt(codec: &Codec, db: &[u8], wal: Option<&[u8]>) -> Result<Vec<u8>> {
    let committed = wal.map(|w| wal::committed(w, PAGE_SIZE)).transpose()?.flatten();
    let mut plain = codec.decrypt(db, |pgno| reads_main_page(pgno, committed.as_ref()))?;
    check_plain_header(&plain)?;
    if let Some(committed) = committed {
        let replayed = wal_pages(codec, &committed, plain.len() / PAGE_SIZE)?;
        plain.resize(committed.db_pages as usize * PAGE_SIZE, 0);
        for (pgno, page) in replayed {
            let at = (pgno as usize - 1) * PAGE_SIZE;
            plain[at..at + PAGE_SIZE].copy_from_slice(&page);
        }
        check_plain_header(&plain)?;
    }
    Ok(plain)
}

/// Whether SQLite, opening a database whose WAL holds `committed`, reads page `pgno` from the
/// main file. It doesn't read the lock-byte page, a page a committed frame replaces, or a page
/// past the last commit's size, so those may hold anything (a hole an interrupted checkpoint
/// left, say) and aren't checked. Page 1 always is: its salt keys every other page.
pub fn reads_main_page(pgno: u32, committed: Option<&wal::Committed<'_>>) -> bool {
    let replaced = committed.is_some_and(|c| c.pages.contains_key(&pgno) || pgno > c.db_pages);
    pgno == 1 || (pgno != LOCK_PAGE && !replaced)
}

/// The pages of a WAL's `committed` transactions over a main file of `main_pages`, decrypted,
/// that lie within the database size the last commit gives. Every page is decrypted first,
/// which authenticates it against its page number. A database grows only by writing its new
/// pages, so a size past the main file plus the WAL's pages beyond it (and one more: SQLite never
/// writes the lock-byte page at 1 GiB) is corrupt or crafted, and is refused instead of
/// allocated.
pub fn wal_pages(codec: &Codec, committed: &wal::Committed<'_>, main_pages: usize) -> Result<Vec<(u32, Vec<u8>)>> {
    let pages: Vec<(u32, Vec<u8>)> =
        committed.pages.iter().map(|(&n, p)| Ok((n, codec.decrypt_page(n, p)?))).collect::<Result<_>>()?;
    let known = main_pages + pages.iter().filter(|(n, _)| *n as usize > main_pages).count();
    let size = committed.db_pages as usize;
    if size > known + 1 {
        return Err(Error::Malformed(format!(
            "the WAL's last commit says the database has {size} pages, but only {known} exist"
        )));
    }
    Ok(pages.into_iter().filter(|(n, _)| *n as usize <= size).collect())
}

#[cfg(test)]
mod tests;
