//! Merges that leave something behind and say so: names the target's export.pdb can't hold or
//! playlists nested too deep (refused when planning), source rows outside the library, artwork
//! and analysis files missing or unreadable, My Tags beyond rekordbox's limits, target databases
//! that can't be read, a matched track the target's OneLibrary lacks, and a merge too big for
//! the target.

use crate::common::{self, Warnings, media};

use std::fs;
use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions, PlaylistMode};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{Error, Library, Playlist, paths, pdb};
use rekordbox_pdb::{ExtDatabase, ExtEditor, NewTrack, PdbEditor};

/// A OneLibrary-only drive (which takes names and details of any length, and playlists of any
/// depth) holding the demo export's tracks, the second with `comment`, with `playlists` (a folder
/// chain, the last one a playlist holding the first track) and history lists `history`.
fn onelibrary_source(dir: &Path, name: &str, chain: &[String], history: &[String], comment: &str) -> Device {
    let root = common::demo_drive(dir, name);
    let mut lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    lib.tracks[1].comment = comment.to_string();
    let first = lib.tracks[0].id;
    let node = |i: usize, name: &String, folder: bool| Playlist {
        id: i as u32 + 1,
        parent_id: i.checked_sub(1).map(|p| p as u32 + 1),
        name: name.clone(),
        is_folder: folder,
        sort_order: 0,
        track_ids: if folder { Vec::new() } else { vec![first] },
    };
    lib.playlists = chain.iter().enumerate().map(|(i, n)| node(i, n, i + 1 < chain.len())).collect();
    lib.history = history.iter().enumerate().map(|(i, n)| Playlist { parent_id: None, ..node(i, n, false) }).collect();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), common::onelibrary_for(&lib).to_encrypted().unwrap()).unwrap();
    for key in [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB] {
        fs::remove_file(root.join(key)).unwrap();
    }
    Device::open_export(root).unwrap()
}

/// What the target's export.pdb can't take is refused when planning, the target left alone: a
/// playlist named past what a row holds when merging by name (ASCII names fit up to 4,024
/// characters), a history list past 4,040, and a playlist the merge folder would put deeper than
/// playlists nest.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn names_and_depths_the_target_cant_hold_are_refused_when_planning(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let main = common::bigger_drive(dir.path(), "Main");
    let before = common::Before::take(&main);
    let dst = Device::open_export(&main).unwrap();
    let past = |limit: usize| "y".repeat(limit + tc.draw(gs::integers::<usize>().min_value(1).max_value(300)));
    let folders = tc.draw(gs::integers::<usize>().max_value(2));
    let mut by_name: Vec<String> = (0..folders).map(|i| format!("Folder {i}")).collect();
    by_name.push(past(4024));
    let deep: Vec<String> = (0..locallibrary::model::MAX_DEPTH).map(|i| format!("Level {i}")).collect();
    let long = past(5000);
    let cases = [
        (onelibrary_source(dir.path(), "Names", &by_name, &[], ""), PlaylistMode::ByName, false, "a name in "),
        (
            onelibrary_source(dir.path(), "History", &[], &[past(4040)], &long),
            PlaylistMode::Folder,
            true,
            "history list",
        ),
        (
            onelibrary_source(dir.path(), "Deep", &deep, &[], ""),
            PlaylistMode::Folder,
            false,
            "levels deep in the merge",
        ),
    ];
    for (n, (src, playlists, history, why)) in cases.into_iter().enumerate() {
        let opts = MergeOptions { playlists, history, ..Default::default() };
        let err = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains(why), "{why}: {err}");
        // Without what it can't hold, the merge goes ahead: but for a track whose details are
        // too long, or that no playlist chosen holds.
        let only_playlists = if n == 2 { vec!["Level 0".to_string()] } else { Vec::new() };
        let lighter =
            MergeOptions { playlists: PlaylistMode::None, history: false, only_playlists, ..Default::default() };
        let plan = merge::plan(&src, &dst, &lighter, &Ctx::silent()).unwrap();
        assert_eq!(plan.new_tracks.len(), if n == 0 { 2 } else { 1 }, "{:?}", plan.warnings);
        let too_long =
            plan.warnings.iter().any(|w| w.ends_with("its details are too long for the target's export.pdb; skipped"));
        assert_eq!(too_long, n == 1, "{:?}", plan.warnings);
    }
    before.untouched(&main).unwrap();
}

/// rekordcrate's demo export with: its tracks' artwork gone from the drive, My Tags on them in a
/// fifth category (beyond rekordbox's four) as well as one of the four, in exportExt.pdb and in
/// OneLibrary (which also has a cue on the first track and a hot cue bank list holding none), a
/// third track only export.pdb lists whose analysis isn't rekordbox's, and a row pointing outside
/// the library (`outside`).
fn guest(dir: &Path, outside: &str) -> PathBuf {
    let root = common::demo_drive(dir, "Guest");
    media::set_art(&root, &[1, 2]);
    fs::remove_dir_all(root.join("PIONEER/Artwork")).unwrap();
    let ext_path = root.join(paths::EXPORT_EXT_PDB);
    let mut ed = ExtEditor::from_file(&ext_path).unwrap();
    ed.add_category(5, "Energy").unwrap();
    ed.add_tag(5, 0x7000_0005, "Peak").unwrap();
    let techno = ed.tags(1).iter().find(|(_, n)| *n == "Techno").unwrap().0;
    for track in [1, 2] {
        ed.link(0x7000_0005, track).unwrap();
        ed.link(techno, track).unwrap();
    }
    ed.save(&ext_path).unwrap();
    common::add_onelibrary(&root);
    let mut one = OneLibrary::open_root(&root).unwrap();
    let ext = ExtDatabase::from_file(&ext_path).unwrap();
    for c in ext.categories() {
        one.add_my_tag(c.id.into(), 0, &c.name, true).unwrap();
        for t in ext.tags_in(c.id) {
            one.add_my_tag(t.id.into(), c.id.into(), &t.name, false).unwrap();
        }
    }
    let demo: Vec<u32> = one.library().unwrap().tracks.iter().map(|t| t.id).collect();
    for tag in [0x7000_0005, i64::from(techno)] {
        one.link_my_tag(tag, &demo).unwrap();
    }
    one.connection()
        .execute("INSERT INTO cue (cue_id, content_id, kind, inUsec) VALUES (900, ?1, 0, 2000000)", [demo[0]])
        .unwrap();
    one.create_hot_cue_bank_list("Empty", 0, false).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    // Only in export.pdb, so a merge makes its OneLibrary row (and cues) from the analysis.
    let junk = common::export_new_track(&root, "Junk", "Analysis");
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let dat = &lib.track(junk).unwrap().analyze_path;
    fs::write(paths::to_fs(&root, &paths::key(dat)), b"not an analysis file").unwrap();
    let mut ed = PdbEditor::from_file(root.join(paths::EXPORT_PDB)).unwrap();
    ed.add_track(&NewTrack { file_size: 9, ..NewTrack::new("Outside", outside) }).unwrap();
    ed.save(root.join(paths::EXPORT_PDB)).unwrap();
    root
}

/// Merged into a drive with both databases (rekordbox's four categories in each, and a track
/// both drives exported that its OneLibrary lacks) or one with only OneLibrary, the guest's
/// sound tracks come, and the merge says what stays behind: the row outside the library, the
/// missing artwork, the unreadable analysis (copied as it is, without cues in OneLibrary), the
/// tags beyond rekordbox's limits (the others carried, and none left on a reused id), the matched
/// track's playlist entries in OneLibrary. No hot cue bank list comes: none holds a cue that does.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
fn a_merge_says_what_it_leaves_behind(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let outside = tc.draw(gs::sampled_from(vec!["/../escape.mp3", "/PIONEER/rekordbox/export.pdb"]));
    let src_root = guest(dir.path(), outside);
    common::export_new_track(&src_root, "Both", "Drives");
    let both = common::bigger_drive(dir.path(), "Both");
    fs::copy(
        common::fixtures().join("rekordcrate/demo_tracks/PIONEER/rekordbox/exportExt.pdb"),
        both.join(paths::EXPORT_EXT_PDB),
    )
    .unwrap();
    common::add_onelibrary(&both);
    let ext = ExtDatabase::from_file(both.join(paths::EXPORT_EXT_PDB)).unwrap();
    let mut one = OneLibrary::open_root(&both).unwrap();
    for c in ext.categories() {
        one.add_my_tag(c.id.into(), 0, &c.name, true).unwrap();
    }
    fs::write(both.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    common::export_new_track(&both, "Both", "Drives");
    // A link left by a track removed from export.pdb, on the id the next track gets.
    let next = pdb::read_library(&both.join(paths::EXPORT_PDB)).unwrap().tracks.iter().map(|t| t.id).max().unwrap() + 1;
    let mut ed = ExtEditor::from_file(both.join(paths::EXPORT_EXT_PDB)).unwrap();
    let trap = ed.tags(ed.categories()[0].0).iter().find(|(_, n)| *n == "Trap").unwrap().0;
    ed.link(trap, next).unwrap();
    ed.save(both.join(paths::EXPORT_EXT_PDB)).unwrap();
    let plus_only = common::bigger_drive(dir.path(), "Plus");
    common::onelibrary_only(&plus_only);

    let src = Device::open_export(&src_root).unwrap();
    for (target, plus_only) in [(&both, false), (&plus_only, true)] {
        let mut dst = Device::open_export(target).unwrap();
        let warnings = Warnings::default();
        let opts = MergeOptions::default();
        let plan = merge::plan(&src, &dst, &opts, &warnings.ctx()).unwrap();
        assert_eq!(plan.warnings.iter().filter(|w| w.contains("unsafe file path")).count(), 1, "{:?}", plan.warnings);
        let report = merge::apply(&plan, &src, &mut dst, &opts, &warnings.ctx()).unwrap();
        let said = warnings.said();
        tc.note(&format!("{}: {:?} {said:?}", target.display(), plan.warnings));
        let says = |what: &str| said.iter().any(|w| w.contains(what));
        assert!(says("missing on the source"), "{said:?}");
        assert_eq!(report.artwork_added, 0);
        assert_eq!(report.hot_cue_bank_lists_added, 0);
        let new = if plus_only { 4 } else { 3 };
        assert_eq!((report.tracks_added, plan.new_tracks.len()), (new, new));
        let plus = common::plus_library(target);
        for t in plus.tracks.iter().filter(|t| t.title == "Analysis") {
            let copied = fs::read(paths::to_fs(target, &paths::key(&t.analyze_path))).unwrap();
            assert_eq!(copied, b"not an analysis file", "copied as it is");
        }
        assert!(says("unrecognised analysis format, copied unchanged"), "{said:?}");
        if !plus_only {
            assert!(says("1 matched track(s) are in the target's export.pdb but not its exportLibrary.db"), "{said:?}");
            assert!(
                says("Junk - Analysis: its analysis files can't be read, so it has no cues in OneLibrary"),
                "{said:?}"
            );
            for file in ["exportExt.pdb", "exportLibrary.db"] {
                assert!(says(&format!("My Tag(s) weren't added to {file} (e.g. Energy/Peak)")), "{said:?}");
            }
            let ext = ExtDatabase::from_file(target.join(paths::EXPORT_EXT_PDB)).unwrap();
            assert!(ext.categories().iter().all(|c| c.name != "Energy"));
            assert_eq!(ext.tag_tracks.len(), 2, "Techno on the two demo tracks, and no stale link");
        }
        assert!(plus.tracks.iter().all(|t| t.artwork_path.is_none()), "no artwork row for a missing image");
    }
}

/// A target whose exportExt.pdb is a folder or isn't a database, and whose OneLibrary can't be
/// read, still takes the source's tracks (with their artwork, but for a larger copy the source
/// lacks, and history): the merge says the My Tags of its databases, and of a source OneLibrary
/// without them, are left out (and, without an exportExt.pdb, that players reading export.pdb
/// won't show them), and leaves those files as they were. Merged again, it has nothing to do. A
/// drive with only OneLibrary takes the tracks with their image and tags. A merge whose files are
/// bigger than the target holds is refused when applied, the target left alone.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
fn a_target_whose_tag_databases_cant_be_read_still_takes_the_tracks(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    // A guest whose artwork has no larger copy, with a history list, and a OneLibrary without
    // the table of its tracks' My Tags.
    let src_root = common::demo_drive(dir.path(), "Guest");
    media::set_art(&src_root, &[1, 2]);
    for e in walkdir::WalkDir::new(src_root.join("PIONEER/Artwork")).into_iter().flatten() {
        if e.file_name().to_string_lossy().contains("_m.") {
            fs::remove_file(e.path()).unwrap();
        }
    }
    common::add_history(&src_root, "HISTORY 001", &[1]);
    let mut ed = ExtEditor::from_file(src_root.join(paths::EXPORT_EXT_PDB)).unwrap();
    ed.link(ed.tags(ed.categories()[0].0)[0].0, 1).unwrap();
    ed.save(src_root.join(paths::EXPORT_EXT_PDB)).unwrap();
    common::add_onelibrary(&src_root);
    let one = OneLibrary::open_root(&src_root).unwrap();
    one.connection().execute_batch("DROP TABLE myTag_content").unwrap();
    fs::write(src_root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let opts = MergeOptions { history: true, ..Default::default() };
    for folder in [true, false] {
        let root = common::bigger_drive(dir.path(), &format!("Main {folder}"));
        let ext_path = root.join(paths::EXPORT_EXT_PDB);
        let junk = tc.draw(gs::sampled_from(vec![b"not a database".to_vec(), vec![0; 4096]]));
        match folder {
            true => fs::create_dir_all(ext_path.join("inside")).unwrap(),
            false => fs::write(&ext_path, &junk).unwrap(),
        }
        fs::write(root.join(paths::LIBRARY_PLUS_DB), &junk).unwrap();
        let no_ext = "1 new track(s) have My Tags, but the target has no exportExt.pdb";
        let mut dst = Device::open_export(&root).unwrap();
        let warnings = Warnings::default();
        let plan = merge::plan(&src, &dst, &opts, &warnings.ctx()).unwrap();
        let left_out = plan.warnings.iter().filter(|w| w.ends_with("; My Tags there are left out of the merge"));
        assert_eq!(left_out.count(), 2, "the guest's OneLibrary and the target's exportExt.pdb: {:?}", plan.warnings);
        assert_eq!(plan.warnings.iter().any(|w| w.starts_with(no_ext)), folder, "{:?}", plan.warnings);
        let report = merge::apply(&plan, &src, &mut dst, &opts, &warnings.ctx()).unwrap();
        assert_eq!((report.tracks_added, report.artwork_added, report.history_playlists_added), (2, 1, 1));
        let said = warnings.said();
        assert!(said.iter().any(|w| w.contains("Only export.pdb will be changed")), "{said:?}");
        assert_eq!(
            said.iter().any(|w| w.ends_with("the merged tracks' My Tags aren't added to it")),
            !folder,
            "{said:?}"
        );
        assert_eq!(fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap(), junk);
        assert_eq!(ext_path.is_dir(), folder);
        let lib: Library = dst.library().unwrap();
        assert_eq!(lib.tracks.len(), 14);
        let art = lib.tracks.iter().rev().find_map(|t| t.artwork_path.clone()).unwrap();
        assert!(paths::to_fs(&root, &paths::key(&art)).is_file());
        assert!(!paths::to_fs(&root, &paths::key(&locallibrary::artwork::medium_variant(&art))).exists());
        // Merged again: every track and the history list are there already.
        let again = MergeOptions { playlists: PlaylistMode::None, ..opts.clone() };
        let plan = merge::plan(&src, &dst, &again, &Ctx::silent()).unwrap();
        assert!(plan.is_noop() && plan.history.is_empty(), "{plan:?}");
    }
    // Into a drive with OneLibrary alone, from the guest without its OneLibrary (whose rows it
    // would bring), the image comes as OneLibrary's own, without the copy.
    fs::remove_file(src_root.join(paths::LIBRARY_PLUS_DB)).unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let plus = common::bigger_drive(dir.path(), "Plus");
    common::onelibrary_only(&plus);
    let mut dst = Device::open_export(&plus).unwrap();
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, &opts, &Ctx::silent()).unwrap();
    assert_eq!((report.tracks_added, report.artwork_added, report.tagged_tracks), (2, 1, 1));
    let images: Vec<String> =
        common::plus_library(&plus).tracks.iter().filter_map(|t| t.artwork_path.clone()).collect();
    assert_eq!(images.len(), 2, "both tracks show the one image");
    assert!(paths::to_fs(&plus, &paths::key(&images[0])).is_file());

    // Audio far bigger than the drive (sparse, and under ext4's 16 TiB, the most a file holds):
    // planned with a warning, refused when applied.
    let main = common::bigger_drive(dir.path(), "Small");
    let lib = pdb::read_library(&src_root.join(paths::EXPORT_PDB)).unwrap();
    let audio = paths::to_fs(&src_root, &paths::key(&lib.tracks[0].file_path));
    let big = 2 * locallibrary::fsutil::available_space(&main).unwrap() + (1 << 30);
    fs::OpenOptions::new().write(true).open(&audio).unwrap().set_len(big).unwrap();
    let before = common::Before::take(&main);
    let mut dst = Device::open_export(&main).unwrap();
    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert!(!plan.fits && plan.needed_bytes > big, "{plan:?}");
    assert!(plan.warnings.iter().any(|w| w.starts_with(&format!("needs about {} bytes", plan.needed_bytes))));
    let err = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::NoSpace { .. }), "{err}");
    before.untouched(&main).unwrap();
}
