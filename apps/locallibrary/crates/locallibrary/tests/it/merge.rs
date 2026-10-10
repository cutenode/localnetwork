mod files;
mod matching;
mod playlist_trees;
mod unreadable_databases;

use crate::common;

use std::io::Cursor;

use binrw::BinRead;
use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions, PlaylistMode};
use locallibrary::progress::Ctx;
use locallibrary::{anlz, backup, paths};

/// Track rows as seen by rekordcrate, an independent PDB parser (Debug-formatted: its fields
/// are private).
fn rekordcrate_tracks(pdb: &[u8]) -> Vec<String> {
    use rekordcrate::pdb::{Header, PageType, Row};
    let mut reader = Cursor::new(pdb);
    let header = Header::read(&mut reader).unwrap();
    let table = header.tables.iter().find(|t| t.page_type == PageType::Tracks).unwrap();
    let pages = header.read_pages(&mut reader, binrw::Endian::NATIVE, (&table.first_page, &table.last_page)).unwrap();
    pages
        .into_iter()
        .flat_map(|p| p.row_groups.into_iter())
        .flat_map(|g| g.present_rows().collect::<Vec<_>>())
        .filter_map(|r| match r {
            Row::Track(t) => Some(format!("{t:?}")),
            _ => None,
        })
        .collect()
}

#[test]
fn merge_into_folder_copies_files_and_rewrites_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(common::demo_drive(dir.path(), "Guest")).unwrap();
    let mut dst = Device::open_export(common::bigger_drive(dir.path(), "Main")).unwrap();

    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(plan.new_tracks.len(), 2);
    assert_eq!(plan.matched_tracks, 0);
    assert!(plan.fits);

    let report = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(report.tracks_added, 2);
    let lib = dst.library().unwrap();
    assert_eq!(lib.tracks.len(), 14);

    // Folder named after the source with no playlists inside (the demo export has none).
    let folder = lib.find_playlist("Guest").unwrap();
    assert!(folder.is_folder);
    // The original playlist is intact.
    assert_eq!(lib.find_playlist("aab").unwrap().track_ids.len(), 12);

    for t in lib.tracks.iter().filter(|t| t.artist == "Loopmasters") {
        let audio = dst.path(&paths::key(&t.file_path));
        assert!(audio.is_file(), "{} copied", t.file_path);
        let dat = std::fs::read(dst.path(&paths::key(&t.analyze_path))).unwrap();
        assert_eq!(anlz::read_path(&dat).unwrap().as_deref(), Some(t.file_path.as_str()));
        // Real analysis data (beat grid) survived the copy.
        assert!(!anlz::beat_grid(&dat).unwrap().is_empty());
        // .EXT/.2EX siblings came along.
        assert!(dst.path(&paths::key(&t.analyze_path.replace(".DAT", ".EXT"))).is_file());
    }

    // An independent parser agrees.
    let rows = rekordcrate_tracks(&dst.pdb_bytes().unwrap());
    assert_eq!(rows.len(), 14);
    assert!(rows.iter().any(|r| r.contains("Demo Track 2")));

    // Merging again finds everything already present.
    let again =
        merge::plan(&src, &dst, &MergeOptions { playlists: PlaylistMode::None, ..Default::default() }, &Ctx::silent())
            .unwrap();
    assert!(again.new_tracks.is_empty());
    assert_eq!(again.matched_tracks, 2);

    // And it can be undone.
    backup::restore(&mut dst, Some(report.backup.as_deref().unwrap()), false).unwrap();
    assert_eq!(dst.library().unwrap().tracks.len(), 12);
}

#[test]
fn merge_by_name_extends_existing_playlists_and_renames_collisions() {
    let dir = tempfile::tempdir().unwrap();
    // Source: the big library plus one track; target: the big library with a *different* file
    // at one of the source's paths.
    let src_root = common::bigger_drive(dir.path(), "Src");
    let new_id = common::export_new_track(&src_root, "Extra", "Bonus");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(src_root.join(paths::EXPORT_PDB)).unwrap();
    ed.add_to_playlist(1, new_id, None).unwrap();
    ed.save(src_root.join(paths::EXPORT_PDB)).unwrap();

    let dst_root = common::bigger_drive(dir.path(), "Dst");
    let src = Device::open_export(&src_root).unwrap();
    let mut dst = Device::open_export(&dst_root).unwrap();
    // Occupy the new track's path on the target with an unrelated file.
    let clash = dst.path("Contents/Extra/UnknownAlbum/Bonus.mp3");
    std::fs::create_dir_all(clash.parent().unwrap()).unwrap();
    std::fs::write(&clash, b"someone else's file").unwrap();

    let opts = MergeOptions { playlists: PlaylistMode::ByName, ..Default::default() };
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    assert_eq!(plan.matched_tracks, 12);
    assert_eq!(plan.new_tracks.len(), 1);
    assert!(plan.new_tracks[0].renamed);
    assert_eq!(plan.new_tracks[0].audio, "Contents/Extra/UnknownAlbum/Bonus (2).mp3");
    assert_eq!(plan.playlists.len(), 1);
    assert!(!plan.playlists[0].create);
    assert_eq!(plan.playlists[0].tracks_added, 1);

    merge::apply(&plan, &src, &mut dst, &opts, &Ctx::silent()).unwrap();
    let lib = dst.library().unwrap();
    assert_eq!(lib.tracks.len(), 13);
    let aab = lib.find_playlist("aab").unwrap();
    assert_eq!(aab.track_ids.len(), 13);
    let bonus = lib.tracks.iter().find(|t| t.title == "Bonus").unwrap();
    assert_eq!(bonus.file_path, "/Contents/Extra/UnknownAlbum/Bonus (2).mp3");
    assert_eq!(std::fs::read(&clash).unwrap(), b"someone else's file");
    let dat = std::fs::read(dst.path(&paths::key(&bonus.analyze_path))).unwrap();
    assert_eq!(anlz::read_path(&dat).unwrap().as_deref(), Some(bonus.file_path.as_str()));
}

/// A source playlist merges into the target's playlist of exactly its name, not into a neighbour
/// whose name only differs in case.
#[test]
fn merge_by_name_tells_names_that_differ_in_case_apart() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::bigger_drive(dir.path(), "Src");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(src_root.join(paths::EXPORT_PDB)).unwrap();
    ed.rename_playlist(1, "AAB").unwrap();
    ed.save(src_root.join(paths::EXPORT_PDB)).unwrap();
    // The target has the same 12 tracks in "aab", next to an empty "AAB".
    let dst_root = common::bigger_drive(dir.path(), "Dst");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(dst_root.join(paths::EXPORT_PDB)).unwrap();
    ed.create_playlist("AAB", 0, false).unwrap();
    ed.save(dst_root.join(paths::EXPORT_PDB)).unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let mut dst = Device::open_export(&dst_root).unwrap();

    let opts = MergeOptions { playlists: PlaylistMode::ByName, ..Default::default() };
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    assert_eq!(plan.playlists.len(), 1);
    assert!(!plan.playlists[0].create);
    assert_eq!(plan.playlists[0].tracks_added, 12);

    merge::apply(&plan, &src, &mut dst, &opts, &Ctx::silent()).unwrap();
    let lib = dst.library().unwrap();
    let entries = |name: &str| lib.playlists.iter().find(|p| p.name == name).unwrap().track_ids.len();
    assert_eq!((entries("aab"), entries("AAB")), (12, 12));
}

/// Regression: a playlist new to the target went into the last of the target's folders whose
/// path matched its folder's ignoring case. It goes into the folder of exactly that path, also
/// when a playlist of that name is shown before it.
#[test]
fn merge_by_name_files_playlists_in_the_folder_of_exactly_their_name() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::bigger_drive(dir.path(), "Src");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(src_root.join(paths::EXPORT_PDB)).unwrap();
    let folder = ed.create_playlist("Shows", 0, true).unwrap();
    let new = ed.create_playlist("New", folder, false).unwrap();
    ed.set_playlist_entries(new, &[1, 2]).unwrap();
    ed.save(src_root.join(paths::EXPORT_PDB)).unwrap();
    let dst_root = common::bigger_drive(dir.path(), "Dst");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(dst_root.join(paths::EXPORT_PDB)).unwrap();
    ed.create_playlist("Shows", 0, false).unwrap();
    let shows = ed.create_playlist("Shows", 0, true).unwrap();
    ed.create_playlist("SHOWS", 0, true).unwrap();
    ed.save(dst_root.join(paths::EXPORT_PDB)).unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let mut dst = Device::open_export(&dst_root).unwrap();

    let opts = MergeOptions { playlists: PlaylistMode::ByName, ..Default::default() };
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    merge::apply(&plan, &src, &mut dst, &opts, &Ctx::silent()).unwrap();
    let lib = dst.library().unwrap();
    let folders: Vec<u32> = lib.playlists.iter().filter(|p| p.is_folder && p.name == "Shows").map(|p| p.id).collect();
    assert_eq!(folders, [shows]);
    let new = lib.playlists.iter().find(|p| p.name == "New").unwrap();
    assert_eq!((new.parent_id, new.track_ids.len()), (Some(shows), 2));
}

/// Regression: a folder in the target shown before a playlist of the same name hid the playlist,
/// so the source's playlist of that name was made again instead of extending it.
#[test]
fn merge_by_name_extends_a_playlist_shown_after_a_folder_of_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::bigger_drive(dir.path(), "Src");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(src_root.join(paths::EXPORT_PDB)).unwrap();
    ed.rename_playlist(1, "Mix").unwrap();
    ed.save(src_root.join(paths::EXPORT_PDB)).unwrap();
    let dst_root = common::bigger_drive(dir.path(), "Dst");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(dst_root.join(paths::EXPORT_PDB)).unwrap();
    ed.create_playlist("Mix", 0, true).unwrap();
    let mix = ed.create_playlist("Mix", 0, false).unwrap();
    ed.save(dst_root.join(paths::EXPORT_PDB)).unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let mut dst = Device::open_export(&dst_root).unwrap();

    let opts = MergeOptions { playlists: PlaylistMode::ByName, ..Default::default() };
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    assert_eq!(plan.playlists.len(), 1);
    assert!(!plan.playlists[0].create);
    assert_eq!(plan.playlists[0].tracks_added, 12);

    merge::apply(&plan, &src, &mut dst, &opts, &Ctx::silent()).unwrap();
    let lib = dst.library().unwrap();
    let mixes: Vec<(u32, usize)> =
        lib.playlists.iter().filter(|p| !p.is_folder && p.name == "Mix").map(|p| (p.id, p.track_ids.len())).collect();
    assert_eq!(mixes, [(mix, 12)]);
}

#[test]
fn merge_selected_playlist_into_large_library() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(common::bigger_drive(dir.path(), "Src")).unwrap();
    let mut dst = Device::open_export(common::large_drive(dir.path(), "Large", true)).unwrap();
    let before = dst.library().unwrap();

    let opts =
        MergeOptions { only_playlists: vec!["aab".into()], folder_name: Some("From Src".into()), ..Default::default() };
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    let start = std::time::Instant::now();
    merge::apply(&plan, &src, &mut dst, &opts, &Ctx::silent()).unwrap();
    let elapsed = start.elapsed();

    let after = dst.library().unwrap();
    assert_eq!(after.tracks.len(), before.tracks.len() + plan.new_tracks.len());
    assert_eq!(after.playlists.len(), before.playlists.len() + 2);
    let merged = after.find_playlist("From Src/aab").unwrap();
    assert_eq!(merged.track_ids.len(), 12);
    // Existing playlists are untouched.
    for p in &before.playlists {
        assert_eq!(after.playlist(p.id).unwrap().track_ids, p.track_ids);
    }
    assert!(elapsed.as_secs() < 60, "merge into a 3.9k track library took {elapsed:?}");
}

#[test]
fn unknown_playlist_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(common::bigger_drive(dir.path(), "Src")).unwrap();
    let dst = Device::open_export(common::demo_drive(dir.path(), "Dst")).unwrap();
    let opts = MergeOptions { only_playlists: vec!["nope".into()], ..Default::default() };
    assert!(merge::plan(&src, &dst, &opts, &Ctx::silent()).is_err());
}

/// Regression: merge made its backup and interrupted marker and copied the new tracks' files
/// before editing the databases in memory, so a merge that failed there (here the target's
/// export.pdb has no playlist table, so its editor can't add the merge folder) said "drive left
/// unchanged" when it wasn't. Now it is.
#[test]
fn a_merge_that_fails_leaves_the_target_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(common::demo_drive(dir.path(), "Guest")).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    // The table directory: the number of tables at 8, then 16 bytes each from 0x1c, type first.
    let path = root.join(paths::EXPORT_PDB);
    let mut bytes = std::fs::read(&path).unwrap();
    let tables = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let tree = (0..tables).map(|i| 0x1c + i * 16).find(|&o| bytes[o..o + 4] == [7, 0, 0, 0]).unwrap();
    bytes[tree] = 0xff;
    std::fs::write(&path, bytes).unwrap();
    let before = common::Before::take(&root);
    let mut dst = Device::open_export(&root).unwrap();
    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(plan.new_tracks.len(), 2);
    merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap_err();
    before.untouched(&root).unwrap();
}

/// Regression: merge planned names too long for a row of the target's export.pdb (from a source
/// with only OneLibrary, which takes any length, or as its folder name), and applying them failed.
/// The plan leaves out a track whose details don't fit, with a warning, and refuses a playlist or
/// folder name that doesn't.
#[test]
fn merge_plans_only_what_the_targets_export_pdb_can_hold() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::demo_drive(dir.path(), "Guest");
    let mut lib = locallibrary::pdb::read_library(&src_root.join(paths::EXPORT_PDB)).unwrap();
    lib.tracks[0].comment = "x".repeat(5000);
    let all = lib.tracks.iter().map(|t| t.id).collect();
    let list = |name: &str, track_ids| locallibrary::Playlist {
        id: 1,
        parent_id: None,
        name: name.into(),
        is_folder: false,
        sort_order: 0,
        track_ids,
    };
    let onelibrary = |lib: &locallibrary::Library| {
        std::fs::write(src_root.join(paths::LIBRARY_PLUS_DB), common::onelibrary_for(lib).to_encrypted().unwrap())
            .unwrap();
    };
    lib.playlists = vec![list("Warmup", all)];
    onelibrary(&lib);
    for key in [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB] {
        std::fs::remove_file(src_root.join(key)).unwrap();
    }
    let src = Device::open_export(&src_root).unwrap();
    let mut dst = Device::open_export(common::bigger_drive(dir.path(), "Main")).unwrap();

    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(plan.new_tracks.len(), 1);
    assert!(plan.warnings.iter().any(|w| w.contains("too long for the target's export.pdb")), "{:?}", plan.warnings);
    let long_folder = MergeOptions { folder_name: Some("é".repeat(2100)), ..Default::default() };
    let err = merge::plan(&src, &dst, &long_folder, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("merge folder's name is too long"), "{err}");
    merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(dst.library().unwrap().tracks.len(), 13);

    lib.playlists = vec![list(&"x".repeat(4100), vec![lib.tracks[1].id])];
    onelibrary(&lib);
    let src = Device::open_export(&src_root).unwrap();
    let by_name = MergeOptions { playlists: PlaylistMode::ByName, ..Default::default() };
    let err = merge::plan(&src, &dst, &by_name, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("too long for the target's export.pdb"), "{err}");
}

/// Regression: a merge from a drive with only export.pdb into one with OneLibrary made no `cue`
/// rows, so players that read only OneLibrary showed the merged tracks without cues. The rows are
/// made from the analysis files, as convert makes them.
#[test]
fn merging_from_export_pdb_brings_cues_into_onelibrary() {
    use locallibrary::cue_edit::{self, CueEdit, CuePosition, HotCue, SetCue};
    use locallibrary::onelibrary::OneLibrary;
    let dir = tempfile::tempdir().unwrap();
    let mut src = Device::open_export(common::demo_drive(dir.path(), "Guest")).unwrap();
    let t = src.library().unwrap().tracks[0].clone();
    for (hot_cue, at) in [(Some(HotCue(0)), CuePosition::Beat(9)), (None, CuePosition::Ms(20_000))] {
        let edit = CueEdit::Set(SetCue { hot_cue, at, snap: false, loop_beats: None, name: None, color: None });
        let plan = cue_edit::plan(&src, &[t.id], &edit, &Ctx::silent()).unwrap();
        cue_edit::apply(&plan, &mut src, &Ctx::silent()).unwrap();
    }
    assert!(!src.has_library_plus());
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst_root);
    let mut dst = Device::open_export(&dst_root).unwrap();

    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let one = OneLibrary::open(&dst).unwrap();
    let merged = one.library().unwrap().tracks.into_iter().find(|m| m.title == t.title).unwrap();
    let rows = one.cue_rows(merged.id).unwrap();
    let mut got: Vec<_> = rows.iter().map(|r| (r.hot_cue, r.in_usec / 1000)).collect();
    got.sort();
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got.iter().filter(|(hot, _)| hot.is_some()).count(), 1, "{got:?}");
    assert!(got.contains(&(None, 20_000)), "{got:?}");
}
