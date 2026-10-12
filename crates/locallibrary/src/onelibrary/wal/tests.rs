use super::*;

/// Build a WAL the way SQLite writes one: `frames` are (page number, commit size, page).
pub(crate) fn build(page_size: usize, salts: (u32, u32), frames: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
    let mut wal = Vec::new();
    wal.extend(MAGIC_LE.to_be_bytes());
    wal.extend(3_007_000u32.to_be_bytes());
    wal.extend((page_size as u32).to_be_bytes());
    wal.extend(0u32.to_be_bytes());
    wal.extend(salts.0.to_be_bytes());
    wal.extend(salts.1.to_be_bytes());
    let mut sum = checksum(false, &wal, (0, 0));
    wal.extend(sum.0.to_be_bytes());
    wal.extend(sum.1.to_be_bytes());
    for (pgno, commit, page) in frames {
        let mut head = Vec::new();
        head.extend(pgno.to_be_bytes());
        head.extend(commit.to_be_bytes());
        head.extend(salts.0.to_be_bytes());
        head.extend(salts.1.to_be_bytes());
        sum = checksum(false, page, checksum(false, &head[..8], sum));
        head.extend(sum.0.to_be_bytes());
        head.extend(sum.1.to_be_bytes());
        wal.extend(head);
        wal.extend(page);
    }
    wal
}

#[test]
fn applies_only_committed_frames_with_a_valid_chain() {
    let page = |b: u8| vec![b; 512];
    let wal = build(512, (1, 2), &[(2, 0, page(1)), (3, 3, page(2)), (2, 4, page(3)), (5, 0, page(4))]);
    let c = committed(&wal, 512).unwrap().unwrap();
    assert_eq!(c.db_pages, 4);
    assert_eq!(c.pages.keys().copied().collect::<Vec<_>>(), vec![2, 3]);
    assert_eq!(c.pages[&2], &page(3)[..], "the later frame for a page wins");

    // A broken checksum ends the log at that frame.
    let mut bad = wal.clone();
    bad[HEADER + (FRAME_HEADER + 512) * 2 + FRAME_HEADER + 10] ^= 1;
    let c = committed(&bad, 512).unwrap().unwrap();
    assert_eq!((c.db_pages, c.pages.len()), (3, 2));

    // Frames left over from before a checkpoint carry old salts.
    let mut stale = build(512, (9, 9), &[]);
    stale.extend_from_slice(&wal[HEADER..]);
    assert!(committed(&stale, 512).unwrap().is_none());

    assert!(committed(&[], 512).unwrap().is_none());
    assert!(committed(&wal, 4096).is_err());
}
