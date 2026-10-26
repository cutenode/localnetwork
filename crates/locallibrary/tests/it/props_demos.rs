//! The demos plugin's `clean` on drawn drives, in one database or both: exactly the demo tracks
//! go (however their names are spaced or cased, OneLibrary's own too), with their entries and
//! their files; a demo track sharing its file with another track stays, and so does every other
//! track, entry and file; cleaning again does nothing, and undoing gives the drive back.

use crate::{common, demos, drawn};

use std::collections::BTreeSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::{Library, backup, edit, paths, pdb};
use rekordbox_pdb::{NewTrack, PdbEditor};

/// Each playlist's entries as file keys, by path.
fn entries(lib: &Library) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = lib
        .playlists
        .iter()
        .chain(&lib.history)
        .filter(|p| !p.is_folder)
        .map(|p| {
            let files = p.track_ids.iter().map(|id| paths::key(&lib.track(*id).unwrap().file_path)).collect();
            (lib.playlist_path(p.id), files)
        })
        .collect();
    out.sort();
    out
}

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn cleaning_removes_exactly_the_demo_tracks(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, lib) = drawn::drive(&tc, dir.path(), "A");
    // Demo samples, as rekordbox spells them or not, some sharing a kept track's file.
    let kept_files: Vec<String> = lib.tracks.iter().map(|t| t.file_path.clone()).collect();
    let mut samples: BTreeSet<String> = BTreeSet::new();
    let mut shared: BTreeSet<String> = BTreeSet::new();
    let pdb_path = root.join(paths::EXPORT_PDB);
    for n in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let (artist, title) = tc.draw(gs::sampled_from(vec![
            ("rekordbox", "House 1"),
            ("REKORDBOX", "house  2"),
            (" rekordbox ", "Techno 1"),
            ("rekordbox", "Breaks 3"),
        ]));
        let file = match tc.draw(gs::weighted_booleans(0.25)) {
            true => {
                let f = tc.draw(gs::sampled_from(kept_files.clone()));
                shared.insert(paths::key(&f));
                f
            }
            false => format!("/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/{title}-{n}.wav"),
        };
        let track = NewTrack {
            artist: Some(artist.into()),
            album: Some("GROOVE CIRCUIT FACTORY SAMPLES".into()),
            analyze_path: format!("/PIONEER/USBANLZ/P0AA/{n:08X}/ANLZ0000.DAT"),
            ..NewTrack::new(title, file.clone())
        };
        let audio =
            fs::read(paths::to_fs(&root, &paths::key(&file))).unwrap_or_else(|_| common::placeholder_audio(&file));
        let id = common::add_track(&root, track, &audio);
        samples.insert(paths::key(&file));
        // In a playlist, or a history list, at times.
        if tc.draw(gs::booleans()) {
            let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
            let p = ed.create_playlist("Samples", 0, false).unwrap();
            ed.add_to_playlist(p, id, None).unwrap();
            if let Some(t) = lib.tracks.first() {
                ed.add_to_playlist(p, t.id, None).unwrap();
            }
            ed.save(&pdb_path).unwrap();
        }
    }
    // At times one only OneLibrary lists, as rekordbox exports some there alone.
    let plus = tc.draw(gs::booleans());
    let plus_only = plus && tc.draw(gs::booleans());
    let plus_only = plus_only.then(|| {
        let file = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/Breaks2.wav";
        let track = NewTrack { artist: Some("rekordbox".into()), ..NewTrack::new("Breaks 2", file) };
        samples.insert(paths::key(file));
        (common::add_track(&root, track, &common::placeholder_audio(file)), file)
    });
    if plus {
        common::add_onelibrary(&root);
    }
    if let Some((id, _)) = plus_only {
        let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
        ed.remove_tracks(&[id]).unwrap();
        ed.save(&pdb_path).unwrap();
    }
    let before_lib = pdb::read_library(&pdb_path).unwrap();
    // A sample on a kept track's file stays: deleting its file's rows would delete that track.
    let sample_ids: BTreeSet<u32> = (before_lib.tracks.iter())
        .filter(|t| t.album == "GROOVE CIRCUIT FACTORY SAMPLES" && !shared.contains(&paths::key(&t.file_path)))
        .map(|t| t.id)
        .collect();
    let files_before = common::drive_files(&root);

    let plan = demos::clean_plan(&root);
    let removed: BTreeSet<String> = plan.tracks_removed.iter().map(|t| t.file_path.clone()).collect();
    let going: BTreeSet<String> = (before_lib.tracks.iter().filter(|t| sample_ids.contains(&t.id)))
        .map(|t| t.file_path.clone())
        .chain(plus_only.map(|(_, file)| file.to_string()))
        .collect();
    assert_eq!(removed, going);
    // A file a kept track uses stays; the samples' own files go.
    let planned: BTreeSet<String> = plan.files.iter().map(|f| f.to_lowercase()).collect();
    for f in &shared {
        assert!(!planned.contains(&f.to_lowercase()), "{f} is still used");
    }
    for f in samples.difference(&shared) {
        assert!(planned.contains(&f.to_lowercase()), "{f} should go: {planned:?}");
    }
    assert_eq!(plan.bytes, plan.files.iter().map(|k| fs::metadata(paths::to_fs(&root, k)).unwrap().len()).sum::<u64>());

    let mut d = Device::open_export(&root).unwrap();
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(report.files_deleted, plan.files.len());
    let after = pdb::read_library(&pdb_path).unwrap();
    let kept: Vec<String> =
        before_lib.tracks.iter().filter(|t| !sample_ids.contains(&t.id)).map(|t| t.file_path.clone()).collect();
    assert_eq!(after.tracks.iter().map(|t| t.file_path.clone()).collect::<Vec<_>>(), kept);
    // Every list keeps its other entries, in order.
    let mut want: Vec<(String, Vec<String>)> = before_lib
        .playlists
        .iter()
        .chain(&before_lib.history)
        .filter(|p| !p.is_folder)
        .map(|p| {
            let files = p
                .track_ids
                .iter()
                .filter(|id| !sample_ids.contains(id))
                .map(|id| paths::key(&before_lib.track(*id).unwrap().file_path))
                .collect();
            (before_lib.playlist_path(p.id), files)
        })
        .collect();
    want.sort();
    assert_eq!(entries(&after), want);
    for f in &planned {
        assert!(!paths::to_fs(&root, f).exists(), "{f} is gone");
    }
    for t in &after.tracks {
        assert!(paths::to_fs(&root, &paths::key(&t.file_path)).exists(), "{} keeps its audio", t.file_path);
    }
    if plus {
        let one = common::plus_library(&root);
        let files = |l: &Library| {
            let mut files: Vec<String> = l.tracks.iter().map(|t| t.file_path.clone()).collect();
            files.sort();
            files
        };
        assert_eq!(files(&one), files(&after), "OneLibrary loses the same tracks, and its own");
    }

    // Again: nothing to clean. Undone: the drive as it was.
    assert!(demos::clean_plan(&root).is_noop());
    if let Some(id) = report.backup {
        backup::restore(&mut d, Some(&id), false).unwrap();
        assert!(common::changed_files(&files_before, &common::drive_files(&root)).is_empty());
    }
}
