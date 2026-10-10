//! Property tests for OneLibrary's `exportLibrary.db`: the SQLCipher page cipher, replaying the
//! WAL, and a track's `cue` rows (with the hot cue bank lists that point at them) as a model-based
//! state machine.
//!
//! Key derivation costs one 256,000-round PBKDF2 per salt, so everything uses the fixtures'
//! salts.

use crate::common;

use indexmap::IndexMap;
use std::sync::OnceLock;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use locallibrary::onelibrary::cipher::{Codec, PAGE_SIZE, RESERVE, SALT_LEN, SQLITE_MAGIC};
use locallibrary::onelibrary::{self, CueRow, OneLibrary, wal};
use locallibrary::{paths, pdb};

fn fixture(name: &str) -> &'static [u8] {
    static FILES: OnceLock<IndexMap<&'static str, Vec<u8>>> = OnceLock::new();
    &FILES.get_or_init(|| {
        ["empty.db", "wal.db", "wal.db-wal"]
            .into_iter()
            .map(|n| (n, std::fs::read(common::fixtures().join("onelibrary").join(n)).unwrap()))
            .collect()
    })[name]
}

/// The codec of the empty fixture database (its keys are derived once per process).
fn codec() -> &'static Codec {
    static CODEC: OnceLock<Codec> = OnceLock::new();
    CODEC.get_or_init(|| Codec::for_file(fixture("empty.db")).unwrap())
}

/// The empty fixture database, decrypted.
fn empty_plain() -> &'static [u8] {
    static PLAIN: OnceLock<Vec<u8>> = OnceLock::new();
    PLAIN.get_or_init(|| codec().decrypt(fixture("empty.db"), |_| true).unwrap())
}

const USABLE: usize = PAGE_SIZE - RESERVE;

/// A page image: a pattern with some bytes drawn.
#[hegel::test_helper]
fn page_image(tc: &TestCase, size: usize) -> Vec<u8> {
    let fill = tc.draw(gs::integers::<u8>());
    let mut page = vec![fill; size];
    let edits =
        tc.draw(gs::vecs(gs::tuples!(gs::integers::<usize>().max_value(size - 1), gs::integers::<u8>())).max_size(8));
    for (at, b) in edits {
        page[at] = b;
    }
    page
}

/// A page encrypts and decrypts back to itself, but for the 80 reserved bytes (IV and HMAC) and,
/// on page 1, the salt (which comes back as SQLite's magic). Encrypting twice uses fresh IVs.
#[hegel::test]
fn pages_decrypt_to_what_was_encrypted(tc: TestCase) {
    let pgno = tc.draw(hegel::one_of!(gs::integers::<u32>().min_value(1).max_value(4), gs::integers::<u32>()));
    let plain = page_image(&tc, PAGE_SIZE);
    let enc = codec().encrypt_page(pgno, &plain).unwrap();
    let dec = codec().decrypt_page(pgno, &enc).unwrap();
    let from = if pgno == 1 { SALT_LEN } else { 0 };
    assert_eq!(&dec[from..USABLE], &plain[from..USABLE]);
    if pgno == 1 {
        assert_eq!(&enc[..SALT_LEN], &fixture("empty.db")[..SALT_LEN]);
        assert_eq!(&dec[..SALT_LEN], SQLITE_MAGIC);
    }
    assert_ne!(codec().encrypt_page(pgno, &plain).unwrap(), enc);
}

/// Changing any authenticated byte of an encrypted page (everything but page 1's salt: data,
/// IV and HMAC), or decrypting it as another page, is detected.
#[hegel::test]
fn tampered_pages_are_refused(tc: TestCase) {
    let pgno = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
    let plain = page_image(&tc, PAGE_SIZE);
    let mut enc = codec().encrypt_page(pgno, &plain).unwrap();
    let wrong = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
    if wrong != pgno {
        assert!(codec().decrypt_page(wrong, &enc).is_err(), "page {pgno} decrypted as page {wrong}");
    }
    let from = if pgno == 1 { SALT_LEN } else { 0 };
    let at = tc.draw(gs::integers::<usize>().min_value(from).max_value(PAGE_SIZE - 1));
    let mask = tc.draw(gs::integers::<u8>().min_value(1));
    enc[at] ^= mask;
    assert!(codec().decrypt_page(pgno, &enc).is_err(), "byte {at} ^ {mask:#x} went unnoticed");
}

/// A plain database of SQLCipher's layout encrypts and decrypts back (but for the reserved
/// bytes); one without room for the cipher, or not whole pages, is refused.
#[hegel::test(test_cases = 50)]
fn databases_round_trip_through_the_cipher(tc: TestCase) {
    let pages = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let mut plain: Vec<u8> = (0..pages).flat_map(|_| page_image(&tc, PAGE_SIZE)).collect();
    plain[..16].copy_from_slice(SQLITE_MAGIC);
    plain[16..18].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
    plain[20] = RESERVE as u8;
    let enc = codec().encrypt(&plain).unwrap();
    assert_eq!(enc.len(), plain.len());
    let dec = Codec::for_file(&enc).unwrap().decrypt(&enc, |_| true).unwrap();
    for (a, b) in dec.chunks(PAGE_SIZE).zip(plain.chunks(PAGE_SIZE)) {
        assert_eq!(&a[..USABLE], &b[..USABLE]);
    }
    let damage = tc.draw(gs::integers::<u8>().max_value(2));
    let mut bad = plain.clone();
    match damage {
        0 => bad[20] = tc.draw(gs::integers::<u8>().filter(|r| usize::from(*r) != RESERVE)),
        1 => bad[16] ^= tc.draw(gs::integers::<u8>().min_value(1)),
        _ => bad.truncate(bad.len() - tc.draw(gs::integers::<usize>().min_value(1).max_value(PAGE_SIZE - 1))),
    }
    assert!(codec().encrypt(&bad).is_err());
}

// ---------------------------------------------------------------------------------------------
// The write-ahead log
// ---------------------------------------------------------------------------------------------

const WAL_HEADER: usize = 32;
const FRAME_HEADER: usize = 24;

/// SQLite's WAL checksum (sqlite.org/fileformat2.html#walformat), words in either byte order.
fn wal_checksum(big_endian: bool, data: &[u8], seed: (u32, u32)) -> (u32, u32) {
    let (mut s1, mut s2) = seed;
    for pair in data.chunks_exact(8) {
        let word = |b: &[u8]| {
            let a: [u8; 4] = b.try_into().unwrap();
            if big_endian { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) }
        };
        s1 = s1.wrapping_add(word(&pair[..4])).wrapping_add(s2);
        s2 = s2.wrapping_add(word(&pair[4..])).wrapping_add(s1);
    }
    (s1, s2)
}

/// One frame: page number, database size in pages for a commit frame (else 0), page image.
type Frame = (u32, u32, Vec<u8>);

/// A WAL as SQLite writes one.
fn build_wal(big_endian: bool, page_size: usize, salts: (u32, u32), frames: &[Frame]) -> Vec<u8> {
    let mut wal = Vec::new();
    wal.extend(if big_endian { 0x377f_0683u32 } else { 0x377f_0682 }.to_be_bytes());
    wal.extend(3_007_000u32.to_be_bytes());
    wal.extend((page_size as u32).to_be_bytes());
    wal.extend(0u32.to_be_bytes());
    wal.extend(salts.0.to_be_bytes());
    wal.extend(salts.1.to_be_bytes());
    let mut sum = wal_checksum(big_endian, &wal, (0, 0));
    wal.extend(sum.0.to_be_bytes());
    wal.extend(sum.1.to_be_bytes());
    for (pgno, commit, page) in frames {
        let mut head = Vec::new();
        head.extend(pgno.to_be_bytes());
        head.extend(commit.to_be_bytes());
        head.extend(salts.0.to_be_bytes());
        head.extend(salts.1.to_be_bytes());
        sum = wal_checksum(big_endian, page, wal_checksum(big_endian, &head[..8], sum));
        head.extend(sum.0.to_be_bytes());
        head.extend(sum.1.to_be_bytes());
        wal.extend(head);
        wal.extend(page);
    }
    wal
}

/// What SQLite applies from `frames`: the last image of each page up to the last commit frame,
/// and that commit's database size; nothing without a commit.
fn committed_model(frames: &[Frame]) -> Option<(IndexMap<u32, Vec<u8>>, u32)> {
    let (mut pages, mut pending, mut db_pages) = (IndexMap::new(), IndexMap::new(), 0);
    for (pgno, commit, page) in frames {
        pending.insert(*pgno, page.clone());
        if *commit != 0 {
            pages.append(&mut pending);
            db_pages = *commit;
        }
    }
    (db_pages > 0).then_some((pages, db_pages))
}

fn committed_owned(wal: &[u8], page_size: usize) -> Option<(IndexMap<u32, Vec<u8>>, u32)> {
    wal::committed(wal, page_size)
        .unwrap()
        .map(|c| (c.pages.into_iter().map(|(n, p)| (n, p.to_vec())).collect(), c.db_pages))
}

#[hegel::test_helper]
fn frames(tc: &TestCase, page_size: usize, max_pgno: u32) -> Vec<Frame> {
    let n = tc.draw(gs::integers::<usize>().max_value(8));
    (0..n)
        .map(|_| {
            let pgno = tc.draw(gs::integers::<u32>().min_value(1).max_value(max_pgno));
            let commits = tc.draw(gs::weighted_booleans(0.3));
            let commit = if commits { tc.draw(gs::integers::<u32>().min_value(1).max_value(max_pgno + 2)) } else { 0 };
            (pgno, commit, page_image(tc, page_size))
        })
        .collect()
}

/// The committed part of a WAL is the last image of each page up to the last commit, in either
/// checksum byte order.
#[hegel::test]
fn wal_replays_up_to_the_last_commit(tc: TestCase) {
    let page_size = tc.draw(gs::sampled_from(vec![512usize, 1024, 4096]));
    let big_endian = tc.draw(gs::booleans());
    let salts = (tc.draw(gs::integers::<u32>()), tc.draw(gs::integers::<u32>()));
    let frames = frames(&tc, page_size, 6);
    let wal = build_wal(big_endian, page_size, salts, &frames);
    assert_eq!(committed_owned(&wal, page_size), committed_model(&frames));
    let other_size = if page_size == 4096 { 512 } else { 4096 };
    assert!(wal::committed(&wal, other_size).is_err(), "a WAL of another page size was accepted");
}

/// A frame whose checksum, salts or page number is wrong ends the log there, and so does the
/// end of the file (a torn last frame is ignored).
#[hegel::test]
fn a_broken_frame_ends_the_log(tc: TestCase) {
    let page_size = 512;
    let salts = (tc.draw(gs::integers::<u32>()), tc.draw(gs::integers::<u32>()));
    let frames = frames(&tc, page_size, 6);
    tc.assume(!frames.is_empty());
    let mut wal = build_wal(false, page_size, salts, &frames);
    let k = tc.draw(gs::integers::<usize>().max_value(frames.len() - 1));
    let head = WAL_HEADER + k * (FRAME_HEADER + page_size);
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {
            let bit = tc.draw(gs::integers::<usize>().max_value(63));
            wal[head + 16 + bit / 8] ^= 1 << (bit % 8);
        }
        1 => {
            let at = head + tc.draw(gs::sampled_from(vec![8usize, 12]));
            let x = tc.draw(gs::integers::<u32>().min_value(1));
            let v = u32::from_be_bytes(wal[at..at + 4].try_into().unwrap()) ^ x;
            wal[at..at + 4].copy_from_slice(&v.to_be_bytes());
        }
        2 => wal[head..head + 4].fill(0),
        _ => {
            let cut = tc.draw(gs::integers::<usize>().max_value(FRAME_HEADER + page_size - 1));
            wal.truncate(head + cut);
        }
    }
    assert_eq!(committed_owned(&wal, page_size), committed_model(&frames[..k]));
}

/// Arbitrary bytes, and real or built WALs with damage, never make the reader panic.
#[hegel::test(test_cases = 300)]
fn wal_reader_survives_any_bytes(tc: TestCase) {
    let page_size = tc.draw(gs::sampled_from(vec![512usize, 4096]));
    let source = tc.draw(gs::integers::<u8>().max_value(2));
    let mut wal = match source {
        0 => tc.draw(gs::binary().max_size(2000)),
        1 => fixture("wal.db-wal").to_vec(),
        _ => {
            let frames = frames(&tc, page_size, 6);
            build_wal(tc.draw(gs::booleans()), page_size, (1, 2), &frames)
        }
    };
    let damage = tc.draw(gs::vecs(gs::tuples!(gs::integers::<usize>(), gs::integers::<u8>())).max_size(6));
    for (at, b) in damage {
        if !wal.is_empty() {
            let i = at % wal.len();
            wal[i] = b;
        }
    }
    let cut = tc.draw(gs::booleans());
    if cut {
        let len = tc.draw(gs::integers::<usize>().max_value(wal.len()));
        wal.truncate(len);
    }
    if let Ok(Some(c)) = wal::committed(&wal, page_size) {
        assert!(c.db_pages > 0);
        assert!(c.pages.values().all(|p| p.len() == page_size));
    }
}

/// `onelibrary::decrypt` lays the committed WAL pages (encrypted like the main file's) over the
/// decrypted database, grown or truncated to the last commit's size; a commit claiming more pages
/// than the file and the WAL's pages past it hold (and the lock-byte page SQLite skips) is
/// refused rather than allocated. A committed frame of zeros fails authentication like any other
/// damaged page.
#[hegel::test(test_cases = 50)]
fn wal_frames_are_laid_over_the_database(tc: TestCase) {
    let db = fixture("empty.db");
    let plain = empty_plain();
    let n = (plain.len() / PAGE_SIZE) as u32;
    let mut frames = frames(&tc, PAGE_SIZE, n + 3);
    for f in &mut frames {
        if f.0 == 1 {
            // Page 1 keeps its database header.
            f.2[..100].copy_from_slice(&plain[..100]);
        }
    }
    let absurd = tc.draw(gs::weighted_booleans(0.1));
    if absurd && let Some(last) = frames.last_mut() {
        last.1 = tc.draw(gs::integers::<u32>().min_value(n + 4));
    }
    let mut encrypted: Vec<Frame> =
        frames.iter().map(|(p, c, img)| (*p, *c, codec().encrypt_page(*p, img).unwrap())).collect();
    let zeros = tc.draw(gs::weighted_booleans(0.2));
    if zeros {
        let pgno = tc.draw(gs::integers::<u32>().min_value(2));
        let commit = tc.draw(hegel::one_of!(gs::just(pgno), gs::integers::<u32>()));
        encrypted.push((pgno, commit, vec![0; PAGE_SIZE]));
    }
    let salts = (tc.draw(gs::integers::<u32>()), tc.draw(gs::integers::<u32>()));
    let wal = build_wal(tc.draw(gs::booleans()), PAGE_SIZE, salts, &encrypted);
    let result = onelibrary::decrypt(db, Some(&wal));
    let Some((pages, db_pages)) = committed_model(&encrypted) else {
        assert_eq!(result.unwrap().1, plain);
        return;
    };
    if pages.values().any(|p| p.iter().all(|&b| b == 0)) {
        assert!(result.is_err(), "a committed frame of zeros was accepted");
        return;
    }
    let known = n + pages.keys().filter(|p| **p > n).count() as u32 + 1;
    if db_pages > known {
        assert!(result.is_err(), "a commit of {db_pages} pages was accepted (only {known} exist)");
        return;
    }
    let mut expect = plain.to_vec();
    expect.resize(db_pages as usize * PAGE_SIZE, 0);
    for (pgno, page) in pages.iter().filter(|(p, _)| **p <= db_pages) {
        let at = (*pgno as usize - 1) * PAGE_SIZE;
        expect[at..at + PAGE_SIZE].copy_from_slice(&codec().decrypt_page(*pgno, page).unwrap());
    }
    assert_eq!(result.unwrap().1, expect);
}

/// Shrunk counterexample of `wal_frames_are_laid_over_the_database`: a commit frame claiming
/// 2^32 - 1 pages made `decrypt` try to allocate 16 TiB (an abort, not an error).
#[test]
fn a_wal_claiming_a_huge_database_is_refused() {
    let page = codec().encrypt_page(2, &empty_plain()[PAGE_SIZE..2 * PAGE_SIZE]).unwrap();
    let wal = build_wal(false, PAGE_SIZE, (1, 2), &[(2, u32::MAX, page)]);
    assert!(onelibrary::decrypt(fixture("empty.db"), Some(&wal)).is_err());
}

/// The key is public, so anyone can write a valid frame: one numbered 100,000 with a commit of as
/// many pages made `decrypt` allocate 400 MB before, the bound taking its page number on trust.
#[test]
fn a_wal_claiming_pages_it_does_not_hold_is_refused() {
    let page = codec().encrypt_page(100_000, &[7; PAGE_SIZE]).unwrap();
    let wal = build_wal(false, PAGE_SIZE, (1, 2), &[(100_000, 100_000, page)]);
    let err = onelibrary::decrypt(fixture("empty.db"), Some(&wal)).map(|_| ()).unwrap_err().to_string();
    assert!(err.contains("says the database has 100000 pages"), "{err}");
    // A database a page longer than its main file holds the page in its WAL.
    let n = (empty_plain().len() / PAGE_SIZE) as u32;
    let page = codec().encrypt_page(n + 1, &[7; PAGE_SIZE]).unwrap();
    let wal = build_wal(false, PAGE_SIZE, (1, 2), &[(n + 1, n + 1, page)]);
    assert_eq!(onelibrary::decrypt(fixture("empty.db"), Some(&wal)).unwrap().1.len(), (n as usize + 1) * PAGE_SIZE);
}

/// The real WAL fixture (four transactions, made by the sqlcipher CLI) cut short anywhere opens
/// as of its last complete transaction; with a byte changed it opens or fails cleanly.
#[hegel::test(test_cases = 40)]
fn real_wal_damage_is_survived(tc: TestCase) {
    let (db, full) = (fixture("wal.db"), fixture("wal.db-wal"));
    let mut wal = full.to_vec();
    let cut = tc.draw(gs::booleans());
    if cut {
        let len = tc.draw(gs::integers::<usize>().max_value(wal.len()));
        wal.truncate(len);
        let frame = FRAME_HEADER + PAGE_SIZE;
        let complete = len.saturating_sub(WAL_HEADER) / frame;
        let commit_at = |i: usize| WAL_HEADER + i * frame + 4;
        let last_commit = (0..complete).rfind(|&i| full[commit_at(i)..commit_at(i) + 4] != [0; 4]);
        let expect = match last_commit {
            Some(i) => onelibrary::decrypt(db, Some(&full[..WAL_HEADER + (i + 1) * frame])),
            None => onelibrary::decrypt(db, None),
        };
        assert_eq!(onelibrary::decrypt(db, Some(&wal)).unwrap().1, expect.unwrap().1);
    } else {
        let at = tc.draw(gs::integers::<usize>().max_value(wal.len() - 1));
        wal[at] ^= tc.draw(gs::integers::<u8>().min_value(1));
        let _ = onelibrary::decrypt(db, Some(&wal));
    }
}

// ---------------------------------------------------------------------------------------------
// Cue rows and hot cue bank lists
// ---------------------------------------------------------------------------------------------

/// The demo export as a OneLibrary database, encrypted (built once).
fn demo_library() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let root = common::fixtures().join("rekordcrate/demo_tracks");
        let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
        common::onelibrary_for(&lib).to_encrypted().unwrap()
    })
}

/// A position in microseconds: usually within a track, sometimes anything.
#[hegel::test_helper]
fn usec(tc: &TestCase) -> i64 {
    let realistic = tc.draw(gs::weighted_booleans(0.85));
    if realistic {
        tc.draw(gs::integers::<i64>().min_value(0).max_value(1_200_000_000))
    } else {
        tc.draw(gs::integers::<i64>())
    }
}

/// Sets one modelled field of `row` (all of them with `all`). Loops end after they start (and
/// at or after 0, since -1 means "no loop"), and quantised lengths are non-zero.
#[hegel::test_helper]
fn edit_row(tc: &TestCase, row: &mut CueRow, all: bool) {
    let field = if all { None } else { Some(tc.draw(gs::integers::<u8>().max_value(5))) };
    let wants = |f: u8| field.is_none_or(|x| x == f);
    if wants(0) {
        let hot = tc.draw(gs::optional(gs::integers::<u8>()));
        row.hot_cue = hot;
    }
    if wants(1) {
        row.in_usec = usec(tc);
        row.out_usec = row.out_usec.filter(|o| *o > row.in_usec);
    }
    if wants(2) && row.in_usec < i64::MAX {
        let is_loop = tc.draw(gs::booleans());
        row.out_usec = if is_loop {
            Some(tc.draw(gs::integers::<i64>().min_value(row.in_usec.saturating_add(1).max(0))))
        } else {
            None
        };
    }
    if wants(3) {
        row.comment = tc.draw(gs::text().max_size(12));
    }
    if wants(4) {
        row.color = tc.draw(gs::integers::<u8>());
        row.active_loop = tc.draw(gs::booleans());
    }
    if wants(5) {
        let quantised = tc.draw(gs::booleans());
        row.loop_beats = if quantised {
            let n = tc.draw(gs::integers::<u16>().min_value(1));
            let d = tc.draw(gs::integers::<u16>().min_value(1));
            Some((n, d))
        } else {
            None
        };
    }
}

/// `(list, cue, sequence number)` of every hot cue bank list entry, in list order.
fn bank_entries(one: &OneLibrary) -> Vec<(i64, i64, i64)> {
    one.connection()
        .prepare("SELECT hotCueBankList_id, cue_id, sequenceNo FROM hotCueBankList_cue ORDER BY 1, 3, rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn max_cue_id(one: &OneLibrary) -> i64 {
    one.connection().query_row("SELECT coalesce(max(cue_id), 0) FROM cue", [], |r| r.get(0)).unwrap()
}

/// Whether two lists of rows mean the same cues (as multisets, by [`CueRow::same_as`]).
fn same_rows(a: &[&CueRow], b: &[&CueRow]) -> bool {
    let mut unused = b.to_vec();
    a.len() == b.len()
        && a.iter().all(|x| match unused.iter().position(|y| x.same_as(y)) {
            Some(i) => {
                unused.swap_remove(i);
                true
            }
            None => false,
        })
}

/// One track's cue rows edited again and again, with hot cue bank lists pointing at them and at
/// another track's cues, which must never change.
struct CueTable {
    one: OneLibrary,
    track: u32,
    other: u32,
    other_rows: Vec<CueRow>,
}

#[hegel::state_machine]
impl CueTable {
    /// Keep, change and drop the track's rows and add new ones (fresh, or copied from the other
    /// track): kept rows keep their ids and read back as asked, new rows read back as asked
    /// with ids no row had before, and bank list entries of removed rows go, their lists
    /// renumbered.
    #[rule]
    fn replace(&mut self, tc: TestCase) {
        let before = self.one.cue_rows(self.track).unwrap();
        let (max_before, bank_before) = (max_cue_id(&self.one), bank_entries(&self.one));
        let mut rows = Vec::new();
        for row in &before {
            match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => rows.push(row.clone()),
                1 => {
                    let mut r = row.clone();
                    edit_row(&tc, &mut r, false);
                    rows.push(r);
                }
                _ => {}
            }
        }
        let added = tc.draw(gs::integers::<usize>().max_value(3));
        for _ in 0..added {
            let copy = tc.draw(gs::integers::<usize>().max_value(self.other_rows.len()));
            let mut r = match self.other_rows.get(copy) {
                Some(o) => CueRow { cue_id: None, ..o.clone() },
                None => CueRow::default(),
            };
            let fresh = r.stored.is_empty();
            edit_row(&tc, &mut r, fresh);
            rows.push(r);
        }
        let order = tc.draw(gs::permutations((0..rows.len()).collect::<Vec<_>>()));
        let rows: Vec<CueRow> = order.into_iter().map(|i| rows[i].clone()).collect();

        let changes = self.one.replace_cues(self.track, &rows).unwrap();
        let after = self.one.cue_rows(self.track).unwrap();
        let kept: Vec<i64> = rows.iter().filter_map(|r| r.cue_id).collect();
        let removed: Vec<i64> = before.iter().filter_map(|r| r.cue_id).filter(|id| !kept.contains(id)).collect();
        let mut updated = 0;
        for r in rows.iter().filter(|r| r.cue_id.is_some()) {
            let a = after.iter().find(|a| a.cue_id == r.cue_id).expect("a kept row is still there");
            assert!(a.same_as(r), "row {:?}: asked {r:?}, read {a:?}", r.cue_id);
            updated += usize::from(!before.iter().find(|b| b.cue_id == r.cue_id).unwrap().same_as(r));
        }
        let new_rows: Vec<&CueRow> = after.iter().filter(|a| !kept.contains(&a.cue_id.unwrap())).collect();
        let wanted: Vec<&CueRow> = rows.iter().filter(|r| r.cue_id.is_none()).collect();
        assert!(same_rows(&new_rows, &wanted), "asked {wanted:?}, read {new_rows:?}");
        assert!(new_rows.iter().all(|r| r.cue_id.unwrap() > max_before), "an id was reused: {new_rows:?}");

        let bank_after = bank_entries(&self.one);
        let lost: Vec<i64> = bank_before.iter().filter(|e| removed.contains(&e.1)).map(|e| e.0).collect();
        let mut expect: Vec<(i64, i64, i64)> =
            bank_before.iter().filter(|e| !removed.contains(&e.1)).copied().collect();
        let mut seq = IndexMap::new();
        for e in expect.iter_mut().filter(|e| lost.contains(&e.0)) {
            let n = seq.entry(e.0).or_insert(0);
            *n += 1;
            e.2 = *n;
        }
        assert_eq!(bank_after, expect);
        let expected_changes = (updated, wanted.len(), removed.len(), bank_before.len() - bank_after.len());
        assert_eq!(
            (changes.updated, changes.added, changes.removed, changes.bank_list_entries_removed),
            expected_changes
        );
    }

    /// File a cue (of either track) in a hot cue bank list, new or existing.
    #[rule]
    fn file_in_bank_list(&mut self, tc: TestCase) {
        let cues: Vec<i64> = [self.track, self.other]
            .iter()
            .flat_map(|t| self.one.cue_rows(*t).unwrap())
            .filter_map(|r| r.cue_id)
            .collect();
        tc.assume(!cues.is_empty());
        let lists: Vec<u32> = self
            .one
            .connection()
            .prepare("SELECT hotCueBankList_id FROM hotCueBankList ORDER BY 1")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let pick = tc.draw(gs::integers::<usize>().max_value(lists.len()));
        let list = match lists.get(pick) {
            Some(l) => *l,
            None => self.one.create_hot_cue_bank_list(&format!("Bank {pick}"), 0, false).unwrap(),
        };
        let cue = tc.draw(gs::sampled_from(cues));
        self.one.add_to_hot_cue_bank_list(list, cue).unwrap();
    }

    /// Claiming another track's cue, or one cue twice, is refused and changes nothing.
    #[rule]
    fn foreign_or_repeated_ids_are_refused(&mut self, tc: TestCase) {
        let mut rows = self.one.cue_rows(self.track).unwrap();
        let foreign = tc.draw(gs::booleans());
        match (foreign, self.other_rows.first(), rows.first().cloned()) {
            (true, Some(o), _) => rows.push(o.clone()),
            (false, _, Some(r)) => rows.push(r),
            _ => tc.reject(),
        }
        let (before, bank) = (self.one.cue_rows(self.track).unwrap(), bank_entries(&self.one));
        assert!(self.one.replace_cues(self.track, &rows).is_err());
        assert_eq!(self.one.cue_rows(self.track).unwrap(), before);
        assert_eq!(bank_entries(&self.one), bank);
    }

    /// Saving (encrypted, with integrity check) and reopening keeps every row and entry.
    #[rule]
    fn save_and_reopen(&mut self, _: TestCase) {
        let bytes = self.one.to_encrypted().unwrap();
        let reopened = OneLibrary::from_encrypted(&bytes, None).unwrap();
        for t in [self.track, self.other] {
            assert_eq!(reopened.cue_rows(t).unwrap(), self.one.cue_rows(t).unwrap());
        }
        assert_eq!(bank_entries(&reopened), bank_entries(&self.one));
        self.one = reopened;
    }

    #[invariant]
    fn bank_lists_are_whole(&self, _: TestCase) {
        assert_eq!(self.one.dangling_bank_list_entries().unwrap(), 0);
        let mut seq: IndexMap<i64, i64> = IndexMap::new();
        for (list, _, n) in bank_entries(&self.one) {
            let next = seq.entry(list).or_insert(0);
            *next += 1;
            assert_eq!(n, *next, "list {list} isn't numbered 1..n");
        }
    }

    #[invariant]
    fn other_track_is_untouched(&self, _: TestCase) {
        assert_eq!(self.one.cue_rows(self.other).unwrap(), self.other_rows);
    }
}

#[hegel::test(test_cases = 30)]
fn cue_rows_follow_edits(tc: TestCase) {
    let mut one = OneLibrary::from_encrypted(demo_library(), None).unwrap();
    let ids: Vec<u32> = one
        .connection()
        .prepare("SELECT content_id FROM content ORDER BY 1")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let (track, other) = (ids[0], ids[1]);
    let seeded = [
        CueRow { hot_cue: Some(0), in_usec: 1_500_000, comment: "Intro".into(), color: 42, ..Default::default() },
        CueRow { in_usec: 30_000_000, out_usec: Some(34_000_000), loop_beats: Some((8, 1)), ..Default::default() },
    ];
    one.replace_cues(other, &seeded).unwrap();
    let other_rows = one.cue_rows(other).unwrap();
    hegel::stateful::machine(CueTable { one, track, other, other_rows }).steps(15).run(tc);
}

/// Shrunk counterexample of `cue_rows_follow_edits`: a cue at a position past about 6e16 µs
/// overflowed working out its 1/150 s frame number (a panic in debug builds).
#[test]
fn extreme_positions_are_stored() {
    let mut one = OneLibrary::from_encrypted(demo_library(), None).unwrap();
    let rows = [CueRow { in_usec: i64::MIN, out_usec: Some(i64::MAX), ..Default::default() }];
    one.replace_cues(1, &rows).unwrap();
    assert!(one.cue_rows(1).unwrap()[0].same_as(&rows[0]));
    let frames: (i64, i64) = one
        .connection()
        .query_row("SELECT in150FramePerSec, out150FramePerSec FROM cue", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(frames, (-1_383_505_805_528_216, 1_383_505_805_528_216));
}
