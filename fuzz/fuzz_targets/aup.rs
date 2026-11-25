//! Audacity projects (`.aup3`, `.aup4`): the binary XML reader and the project it builds from
//! the document, then, for a project that builds, the same document in a project file, opened and
//! mixed down from sample blocks of each format.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use loclib_aup::Project;
use loclib_mixid::fingerprint::RATE;
use loclib_progress::Ctx;

/// The most samples a project is mixed down for, at its rate or at `RATE`: a few chunks of
/// the mix, so a project that says it's hours long doesn't stall the run.
const SAMPLES: f64 = 200_000.0;

/// Sample blocks 1 to 4, `(format, samples)`: 16-bit, 24-bit in 32, float, and a format Audacity
/// doesn't have, each a sample and a few odd bytes long.
const BLOCKS: [(i64, &[u8]); 4] = [
    (0x0002_0001, &[0x00, 0x40, 0x01]),
    (0x0004_0001, &[0x00, 0x00, 0x40, 0x00, 0x01]),
    (0x0004_000F, &[0x00, 0x00, 0x00, 0x3F, 0x01]),
    (0x7FFF, &[0x00]),
];

fuzz_target!(|data: &[u8]| {
    // The dictionary and the document go through one reader, so names can be in either: all of
    // it as the document covers both.
    let Ok(project) = Project::from_document(Path::new("fuzz.aup3"), &[], data) else { return };
    let duration = project.duration_secs();
    assert!(duration >= 0.0, "{duration} s");
    if duration * project.rate().max(f64::from(RATE)) > SAMPLES {
        return;
    }

    let dir = tempfile::tempdir().expect("a temporary folder");
    let path = dir.path().join("fuzz.aup3");
    let db = rusqlite::Connection::open(&path).expect("a new project file");
    db.execute_batch(
        "CREATE TABLE project(id INTEGER PRIMARY KEY, dict BLOB, doc BLOB);
         CREATE TABLE sampleblocks(blockid INTEGER PRIMARY KEY AUTOINCREMENT, sampleformat INTEGER, samples BLOB);",
    )
    .expect("Audacity's tables");
    db.execute("INSERT INTO project VALUES (1, x'', ?1)", [data]).expect("the document");
    for (format, samples) in BLOCKS {
        db.execute("INSERT INTO sampleblocks (sampleformat, samples) VALUES (?1, ?2)", (format, samples))
            .expect("a sample block");
    }
    drop(db);

    let project = Project::open(&path).expect("the document built a project once already");
    let _ = project.render(RATE, &Ctx::silent(), &mut |_| {});
});
