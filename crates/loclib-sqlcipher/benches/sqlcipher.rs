//! Decrypting a database, alone and with a WAL to replay, on a synthetic file of 300 pages
//! (1.2 MB): `cargo bench -p loclib-sqlcipher --features test-util`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use loclib_sqlcipher::{Codec, PAGE_SIZE, RESERVE, SALT_LEN, SQLITE_MAGIC, decrypt, wal};

const PAGES: usize = 300;
/// Pages the WAL's transactions rewrite, and the pages they add past the main file.
const REWRITTEN: u32 = 60;
const ADDED: u32 = 4;

/// A plain database of `PAGES` pages in SQLCipher's layout.
fn plain() -> Vec<u8> {
    let mut db: Vec<u8> = (0..PAGES * PAGE_SIZE).map(|i| (i * 7 % 251) as u8).collect();
    db[..16].copy_from_slice(SQLITE_MAGIC);
    db[16..18].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
    db[20] = RESERVE as u8;
    db
}

/// A WAL of eight transactions of eight frames, rewriting every fifth page and then adding pages
/// past the main file.
fn wal_for(codec: &Codec) -> Vec<u8> {
    let pgnos = (1..=REWRITTEN).map(|i| i * 5).chain(PAGES as u32 + 1..=PAGES as u32 + ADDED);
    let frames: Vec<(u32, u32, Vec<u8>)> = pgnos
        .enumerate()
        .map(|(i, pgno)| {
            let commit = if (i + 1) % 8 == 0 { pgno.max(PAGES as u32) } else { 0 };
            (pgno, commit, codec.encrypt_page(pgno, &[i as u8; PAGE_SIZE]).unwrap())
        })
        .collect();
    wal::tests::build(PAGE_SIZE, (1, 2), &frames)
}

fn bench(c: &mut Criterion) {
    // A random salt, as a real file has: CodeQL flags a literal one.
    let mut salt = [0; SALT_LEN];
    getrandom::fill(&mut salt).unwrap();
    let codec = Codec::with_key(b"a key", salt);
    let db = codec.encrypt(&plain()).unwrap();
    let wal = wal_for(&codec);
    let mut group = c.benchmark_group("sqlcipher");
    group.bench_function("decrypt/300_pages", |b| b.iter(|| decrypt(&codec, black_box(&db), None).unwrap()));
    group.bench_function("decrypt/300_pages_wal", |b| b.iter(|| decrypt(&codec, black_box(&db), Some(&wal)).unwrap()));
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
