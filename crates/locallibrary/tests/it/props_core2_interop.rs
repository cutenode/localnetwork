//! A Serato export that writes cue tags, on drives whose audio can be tagged: every track's audio
//! is copied below the output folder, MP3 and WAV copies with Serato's tag added and others as
//! they are, the drive is only read, and a location root (the crates would list other files than
//! the tagged copies) is refused.

use crate::common::{self, media};

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use indexmap::IndexMap;
use locallibrary::device::Device;
use locallibrary::interop::{self, InteropOptions, Software};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use rekordbox_pdb::NewTrack;

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn serato_tags_go_into_copies_of_every_track(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let lib = common::library(&root);
    for t in &lib.tracks {
        fs::write(paths::to_fs(&root, &paths::key(&t.file_path)), media::mp3()).unwrap();
    }
    for n in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let (ext, audio) = match tc.draw(gs::sampled_from(vec!["wav", "flac", "mp3"])) {
            "wav" => ("wav", media::wav_of(44_100, 16, 2, 0.2)),
            "mp3" => ("mp3", media::mp3()),
            other => (other, common::placeholder_audio(other)),
        };
        common::add_track(&root, NewTrack::new(format!("Added {n}"), format!("/Contents/Added/{n}.{ext}")), &audio);
    }
    let d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    // Each track's audio once, by its key folded as the drive folds names.
    let audio: IndexMap<String, String> =
        lib.tracks.iter().map(|t| (paths::key(&t.file_path).to_lowercase(), paths::key(&t.file_path))).collect();
    let out = locallibrary::device::dunce_canonicalize(dir.path()).join("Serato");
    let opts = InteropOptions { write_tags: true, ..Default::default() };

    let elsewhere = InteropOptions { location_root: Some(dir.path().join("Music")), ..opts.clone() };
    assert_eq!(interop::plan(&d, Software::Serato, &out, &elsewhere, &Ctx::silent()).unwrap_err().code(), "invalid");

    let plan = interop::plan(&d, Software::Serato, &out, &opts, &Ctx::silent()).unwrap();
    let sizes: u64 = audio.values().map(|k| fs::metadata(paths::to_fs(&root, k)).unwrap().len()).sum();
    assert_eq!(plan.copy_bytes, sizes);
    let before = common::drive_files(&root);
    let done = interop::apply(&d, Software::Serato, &out, &opts, &Ctx::silent()).unwrap();
    assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty(), "the drive is only read");
    // A tagged copy is planned at its source's size and its whole new tag: no less than it takes.
    for f in &done.files {
        let len = fs::metadata(&f.path).unwrap().len();
        match f.what.ends_with("with Serato's cue tag") {
            true => assert!(len <= f.bytes, "{}", f.path.display()),
            false => assert_eq!(len, f.bytes, "{}", f.path.display()),
        }
    }
    for key in audio.values() {
        let original = fs::read(paths::to_fs(&root, key)).unwrap();
        let copy = fs::read(paths::to_fs(&out, key)).unwrap();
        let tagged = key.ends_with(".mp3") || key.ends_with(".wav");
        assert_eq!(copy != original, tagged, "{key}");
        if tagged {
            assert!(copy.len() > original.len(), "{key}: the tag is added");
        }
    }
}
