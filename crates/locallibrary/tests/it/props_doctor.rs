//! The doctor's report against drives with problems drawn into them: files taken away, files
//! nothing references any more, and copies of tracks (another row for one file, a Sync Manager
//! copy, the same artist, title and length). It finds exactly the files missing and orphaned,
//! puts every copy with its original, and removing orphans takes exactly those away, undoably.

use crate::{common, drawn};

use std::collections::BTreeSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::doctor::{self, DoctorOptions, DuplicateReason, Problem};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::{backup, pdb};
use rekordbox_pdb::NewTrack;

fn folded(keys: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    keys.into_iter().map(|k| k.to_lowercase()).collect()
}

#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn the_doctor_finds_exactly_what_was_broken(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (root, lib) = drawn::drive(&tc, dir.path(), "A");

    // Copies: (the original's id, the copy's id, why they're one track).
    let mut copies: Vec<(u32, u32, DuplicateReason)> = Vec::new();
    let originals: Vec<u32> =
        drawn::some(&tc, &lib.tracks.iter().map(|t| t.id).collect::<Vec<_>>()).into_iter().take(3).collect();
    for (n, id) in originals.iter().enumerate() {
        let t = lib.track(*id).unwrap();
        let bytes = fs::read(paths::to_fs(&root, &paths::key(&t.file_path))).unwrap();
        let reason = tc.draw(gs::sampled_from(vec![
            DuplicateReason::SameFile,
            DuplicateReason::SyncManagerCopy,
            DuplicateReason::SameMetadata,
        ]));
        let (title, file_path, artist) = match reason {
            DuplicateReason::SameFile => (format!("Copy {n}"), t.file_path.clone(), format!("Someone {n}")),
            DuplicateReason::SyncManagerCopy => {
                let (stem, ext) = t.file_path.rsplit_once('.').unwrap();
                (format!("Copy {n}"), format!("{stem}-1.{ext}"), format!("Someone {n}"))
            }
            _ => (t.title.clone(), format!("/Contents/Copies/{n}.mp3"), t.artist.clone()),
        };
        let copy =
            NewTrack { artist: Some(artist), duration: t.duration_secs as u16, ..NewTrack::new(title, file_path) };
        copies.push((*id, common::add_track(&root, copy, &bytes), reason));
    }
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();

    // Missing: audio and analysis of tracks that aren't copies or originals.
    let plain: Vec<u32> =
        lib.tracks.iter().map(|t| t.id).filter(|id| !copies.iter().any(|(a, b, _)| a == id || b == id)).collect();
    let no_audio: Vec<u32> = drawn::some(&tc, &plain).into_iter().take(2).collect();
    let no_analysis: Vec<u32> =
        drawn::some(&tc, &plain).into_iter().filter(|id| !no_audio.contains(id)).take(2).collect();
    let mut want_missing = BTreeSet::new();
    for id in &no_audio {
        let t = lib.track(*id).unwrap();
        fs::remove_file(paths::to_fs(&root, &paths::key(&t.file_path))).unwrap();
        want_missing.insert((Problem::MissingAudio, *id));
    }
    for id in &no_analysis {
        let t = lib.track(*id).unwrap();
        if !t.analyze_path.is_empty() {
            fs::remove_file(paths::to_fs(&root, &paths::key(&t.analyze_path))).unwrap();
            want_missing.insert((Problem::MissingAnalysis, *id));
        }
    }

    // Orphans: audio, analysis and images nothing references.
    let candidates = [
        "Contents/Stray/left behind.mp3",
        "Contents/Stray/Left Behind.flac",
        "PIONEER/USBANLZ/P0FF/0000FFFF/ANLZ0000.DAT",
        "PIONEER/USBANLZ/P0FF/0000FFFF/ANLZ0000.EXT",
        "PIONEER/Artwork/00009/a999.jpg",
    ];
    let orphans: Vec<&str> = drawn::some(&tc, &candidates);
    for k in &orphans {
        let p = paths::to_fs(&root, k);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, common::placeholder_audio(k)).unwrap();
    }
    // Not orphans, whatever the database says: a cover beside the audio, notes.
    fs::create_dir_all(paths::to_fs(&root, "Contents/Stray")).unwrap();
    fs::write(paths::to_fs(&root, "Contents/Stray/cover.jpg"), b"jpeg").unwrap();

    let hash_audio = tc.draw(gs::booleans());
    let d = Device::open_export(&root).unwrap();
    let opts = DoctorOptions { hash_audio, ..Default::default() };
    let r = doctor::run(&d, &opts, &Ctx::silent()).unwrap();

    let missing: BTreeSet<(Problem, u32)> =
        r.missing.as_ref().unwrap().iter().map(|m| (m.problem, m.track.id)).collect();
    assert_eq!(missing, want_missing);
    let found: BTreeSet<String> = folded(r.orphans.as_ref().unwrap().iter().map(|o| o.path.clone()));
    assert_eq!(found, folded(orphans.iter().map(|s| s.to_string())));
    let unfiled: Vec<u32> = r.unfiled.as_ref().unwrap().iter().map(|t| t.id).collect();
    let filed: BTreeSet<u32> = lib.playlists.iter().flat_map(|p| p.track_ids.iter().copied()).collect();
    assert_eq!(unfiled, lib.tracks.iter().map(|t| t.id).filter(|id| !filed.contains(id)).collect::<Vec<_>>());
    assert!(r.large_files.as_ref().unwrap().is_empty());
    assert!(r.drift.is_none(), "export.pdb only");

    // Each copy is in a group with its original, for its reason; the keeper leads each group.
    let groups = r.duplicates.as_ref().unwrap();
    for (a, b, reason) in &copies {
        let g = groups
            .iter()
            .find(|g| g.tracks.iter().any(|t| t.track.id == *a))
            .unwrap_or_else(|| panic!("#{a} isn't in a group: {groups:#?}"));
        assert!(g.tracks.iter().any(|t| t.track.id == *b), "#{b} isn't with #{a}");
        assert!(g.reasons.contains(reason), "{reason:?} not in {:?}", g.reasons);
    }
    let mut seen = BTreeSet::new();
    for g in groups {
        assert!(g.tracks.len() > 1 && !g.reasons.is_empty());
        assert_eq!(g.keep, g.tracks[0].track.id);
        assert!(g.tracks.iter().all(|t| seen.insert(t.track.id)), "a track in two groups");
    }
    assert_eq!(r.has_problems(), !want_missing.is_empty() || !orphans.is_empty() || !groups.is_empty());

    // Removing orphans removes exactly those, and undoing it gives them back.
    let before = common::drive_files(&root);
    let mut d = Device::open_export(&root).unwrap();
    let plan = doctor::plan_orphans(&d, &Ctx::silent()).unwrap();
    assert_eq!(folded(plan.files.iter().map(|f| f.path.clone())), found);
    assert_eq!(plan.bytes, plan.files.iter().map(|f| f.size).sum::<u64>());
    let done = doctor::apply_orphans(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(done.files_deleted, orphans.len());
    let after = common::drive_files(&root);
    let gone: BTreeSet<String> = folded(common::changed_files(&before, &after));
    let databases: BTreeSet<String> = folded(paths::DATABASE_FILES.iter().map(|s| s.to_string()));
    assert_eq!(gone.difference(&databases).cloned().collect::<BTreeSet<_>>(), found);
    let again = doctor::run(&d, &DoctorOptions::default(), &Ctx::silent()).unwrap();
    assert!(again.orphans.unwrap().is_empty());
    if let Some(id) = done.backup {
        // Forced: the audio taken away above makes it look like the library was replaced.
        backup::restore(&mut d, Some(&id), true).unwrap();
        assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty());
    }
}
