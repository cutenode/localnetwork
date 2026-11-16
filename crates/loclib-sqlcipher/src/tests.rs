use hegel::TestCase;
use hegel::generators as gs;

use super::*;
use crate::cipher::tests::plain_db;

/// Every test uses one key and salt, so the keys are derived once a process.
fn codec() -> Codec {
    Codec::with_key(b"a key", [7; SALT_LEN])
}

/// A database's pages without the reserve, which keeps each page's own IV and HMAC.
fn usable(db: &[u8]) -> Vec<u8> {
    db.chunks(PAGE_SIZE).flat_map(|p| p[..PAGE_SIZE - RESERVE].to_vec()).collect()
}

/// SQLite reads a page from the WAL when a committed frame holds it, so the main file's copy
/// may be anything (zeros an interrupted checkpoint left, say). Anywhere else, zeros are damage.
#[test]
fn only_pages_sqlite_reads_must_authenticate() {
    let codec = codec();
    let plain = plain_db(4);
    let bytes = codec.encrypt(&plain).unwrap();
    let frame = codec.encrypt_page(2, &plain[PAGE_SIZE..2 * PAGE_SIZE]).unwrap();
    let wal = wal::tests::build(PAGE_SIZE, (1, 2), &[(2, 4, frame)]);
    let mut holed = bytes.clone();
    holed[PAGE_SIZE..2 * PAGE_SIZE].fill(0);
    assert_eq!(usable(&decrypt(&codec, &holed, Some(&wal)).unwrap()), usable(&plain));
    let err = decrypt(&codec, &holed, None).unwrap_err();
    assert_eq!(err.to_string(), "page 2 fails authentication (the wrong key, or damaged)");

    let committed = wal::committed(&wal, PAGE_SIZE).unwrap().unwrap();
    assert!(!reads_main_page(2, Some(&committed)));
    assert!(!reads_main_page(5, Some(&committed)), "past the last commit");
    assert!(reads_main_page(3, Some(&committed)));
    assert!(!reads_main_page(LOCK_PAGE, None));
    assert!(reads_main_page(1, None));
}

/// A database grows only by writing its new pages: a commit claiming more than the main file and
/// the WAL hold between them (and the lock-byte page SQLite skips) is refused, and pages past a
/// commit's size are left out.
#[test]
fn a_wal_claiming_pages_it_does_not_hold_is_refused() {
    let codec = codec();
    let page = |n| codec.encrypt_page(n, &[7; PAGE_SIZE]).unwrap();
    let wal = wal::tests::build(PAGE_SIZE, (1, 2), &[(5, 0, page(5)), (3, 6, page(3))]);
    let c = wal::committed(&wal, PAGE_SIZE).unwrap().unwrap();
    assert_eq!(wal_pages(&codec, &c, 4).unwrap().len(), 2);
    let err = wal_pages(&codec, &c, 3).unwrap_err().to_string();
    assert_eq!(err, "the WAL's last commit says the database has 6 pages, but only 4 exist");

    let wal = wal::tests::build(PAGE_SIZE, (1, 2), &[(5, 0, page(5)), (3, 3, page(3))]);
    let c = wal::committed(&wal, PAGE_SIZE).unwrap().unwrap();
    let kept: Vec<u32> = wal_pages(&codec, &c, 4).unwrap().into_iter().map(|(n, _)| n).collect();
    assert_eq!(kept, [3]);
}

/// A database with a WAL decrypts to its main file with the committed frames' pages laid over
/// it (a page's last frame winning), grown to the last commit's size; frames after the last
/// commit change nothing.
#[hegel::test(test_cases = 50)]
fn decrypting_replays_the_committed_frames(tc: TestCase) {
    let codec = codec();
    let main = tc.draw(gs::integers::<u32>().min_value(1).max_value(4));
    let plain = plain_db(main as usize);
    let frames =
        gs::tuples!(gs::integers::<u32>().min_value(2).max_value(main + 2), gs::booleans(), gs::integers::<u8>());
    let mut expected = plain.clone();
    let (mut size, mut pending, mut wal) = (main, Vec::new(), Vec::new());
    for (pgno, commit, fill) in tc.draw(gs::vecs(frames).max_size(6)) {
        let page = vec![fill; PAGE_SIZE];
        wal.push((pgno, 0, codec.encrypt_page(pgno, &page).unwrap()));
        pending.push((pgno, page));
        if commit {
            size = pending.iter().map(|(n, _)| *n).fold(size, u32::max);
            wal.last_mut().unwrap().1 = size;
            expected.resize(size as usize * PAGE_SIZE, 0);
            for (n, page) in pending.drain(..) {
                let at = (n as usize - 1) * PAGE_SIZE;
                expected[at..at + PAGE_SIZE].copy_from_slice(&page);
            }
        }
    }
    let db = codec.encrypt(&plain).unwrap();
    let wal = wal::tests::build(PAGE_SIZE, (1, 2), &wal);
    assert_eq!(usable(&decrypt(&codec, &db, Some(&wal)).unwrap()), usable(&expected));
}

/// Whatever damage a WAL takes (a byte changed or the file cut short anywhere), reading it never
/// panics, and gives at most the transactions before the damage.
#[hegel::test(test_cases = 100)]
fn a_damaged_wal_keeps_the_transactions_before_the_damage(tc: TestCase) {
    let page = |b: u8| vec![b; 512];
    let mut wal = wal::tests::build(512, (1, 2), &[(2, 0, page(1)), (3, 3, page(2)), (2, 4, page(3))]);
    let at = tc.draw(gs::integers::<usize>().max_value(wal.len() - 1));
    match tc.draw(gs::booleans()) {
        true => wal[at] ^= tc.draw(gs::integers::<u8>().min_value(1)),
        false => wal.truncate(at),
    }
    if let Ok(Some(c)) = wal::committed(&wal, 512) {
        assert_eq!(c.db_pages, 3, "byte {at}");
        assert_eq!(c.pages.into_iter().collect::<Vec<_>>(), [(2, &page(1)[..]), (3, &page(2)[..])]);
    }
}

#[test]
fn errors_say_what_went_wrong() {
    let err = Error::NoRandomness(getrandom::Error::UNSUPPORTED).to_string();
    assert!(err.starts_with("no randomness available: "), "{err}");
    assert_eq!(Error::Malformed("short".into()).to_string(), "short");
}
