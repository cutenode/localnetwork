//! Pointing tracks at other audio files on the drive (`Edit::SetPath`), on drawn drives with one
//! database or both: the track keeps its row, entries and details, both databases and its
//! analysis files name the new file, a file that isn't audio on the drive (or is another track's)
//! is refused, undo takes it back, and replaying the journal after a new export does it again.

use crate::{common, drawn};

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, AudioFile, Edit, EditOptions};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Library, anlz, backup, paths, pdb};

/// Each list's entries, by the tracks' ids.
fn entries(lib: &Library) -> Vec<(String, Vec<u32>)> {
    let mut v: Vec<_> =
        lib.playlists.iter().chain(&lib.history).map(|p| (lib.playlist_path(p.id), p.track_ids.clone())).collect();
    v.sort();
    v
}

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_track_pointed_at_another_file_keeps_everything_else(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, lib) = drawn::drive(&tc, dir.path(), "A");
    let plus = tc.draw(gs::booleans());
    if plus {
        common::add_onelibrary(&root);
    }
    let pristine = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let t = lib.tracks[tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1))].clone();
    // Where it goes: a copy of its file elsewhere, another track's file, or nothing at all.
    let target = tc.draw(gs::sampled_from(vec!["moved", "renamed", "other track", "missing", "analysis"]));
    let other = lib.tracks.iter().find(|o| o.id != t.id && o.file_path != t.file_path).unwrap();
    let new_path = match target {
        "moved" => format!("/Contents/Moved/{}", t.file_path.rsplit('/').next().unwrap()),
        "renamed" => format!("{} (2).mp3", t.file_path.rsplit_once('.').unwrap().0),
        "other track" => other.file_path.clone(),
        "missing" => "/Contents/Nowhere/gone.mp3".into(),
        _ => "/PIONEER/rekordbox/export.pdb".into(),
    };
    let audio = fs::read(paths::to_fs(&root, &paths::key(&t.file_path))).unwrap();
    if matches!(target, "moved" | "renamed") {
        let p = paths::to_fs(&root, &paths::key(&new_path));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, &audio).unwrap();
    }
    let file = tc.draw(gs::booleans()).then_some(AudioFile {
        size: audio.len() as u64,
        sample_rate: 96_000,
        bit_depth: 24,
        bitrate: 4608,
    });
    let e =
        Edit::SetPath { track: TrackSelector::paths([t.file_path.clone()]), path: new_path.clone(), file, shift_ms: 0 };
    let mut d = Device::open_export(&root).unwrap();
    let plan = edit::plan(&d, std::slice::from_ref(&e), &EditOptions::default(), &Ctx::silent());
    let shared = lib.tracks.iter().filter(|o| o.file_path.eq_ignore_ascii_case(&t.file_path)).count() > 1;
    let fine = matches!(target, "moved" | "renamed") && !shared;
    let Ok(plan) = plan else {
        assert!(!fine, "{target} refused");
        return;
    };
    assert!(fine, "{target} accepted");
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();

    let check = |lib: &Library| {
        let now = lib.tracks.iter().find(|x| x.file_path == new_path).expect("the track at its new file");
        assert_eq!((now.title.as_str(), now.artist.as_str()), (t.title.as_str(), t.artist.as_str()));
        if let Some(f) = file {
            assert_eq!((now.sample_rate, now.sample_depth), (f.sample_rate, f.bit_depth));
        }
        assert!(!lib.tracks.iter().any(|x| x.file_path == t.file_path), "nothing points at the old file");
        now.id
    };
    let after = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    assert_eq!(check(&after), t.id, "the same row");
    assert_eq!(entries(&after), entries(&lib), "entries stay");
    if plus {
        check(&common::plus_library(&root));
    }
    // (The twelve-track export keeps some analysis files under `.PIONEER/`, which isn't
    // rekordbox's folder: those aren't the drive's analysis files, and aren't edited.)
    let dat_key = paths::key(&t.analyze_path);
    if paths::classify(&dat_key) == Some(paths::FileClass::Analysis) {
        let dat = fs::read(paths::to_fs(&root, &dat_key)).unwrap();
        assert_eq!(anlz::read_path(&dat).unwrap().as_deref(), Some(new_path.as_str()), "the analysis names it");
    }
    let journal = edit::journal::edits(&d).unwrap();
    assert!(journal.iter().any(|j| matches!(j, Edit::SetPath { path, .. } if *path == new_path)), "{journal:?}");

    // rekordbox exports the old row again: replaying the journal points it at the file again.
    let backup_id = report.backup.clone();
    fs::write(root.join(paths::EXPORT_PDB), &pristine).unwrap();
    let (_, replayed) = common::replay(&root);
    check(&replayed);
    // Undone (from the edit's backup), the track is back on its own file.
    if let Some(id) = backup_id {
        let mut d = Device::open_export(&root).unwrap();
        backup::restore(&mut d, Some(&id), true).unwrap();
        let back = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
        assert!(back.tracks.iter().any(|x| x.id == t.id && x.file_path == t.file_path));
    }
}
