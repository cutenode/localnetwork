use crate::common::{self, path};

use std::fs;
use std::path::Path;

use locallibrary::api::{self, DiffRequest, LabelRequest, RestoreBackup, SnapshotLibraryRequest};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, paths};
use rekordbox_pdb::{NewTrack, PdbEditor};

fn aab(root: &Path) -> u32 {
    locallibrary::pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().find_playlist("aab").unwrap().id
}

fn set_entries(root: &Path, playlist: u32, tracks: &[u32]) {
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    ed.set_playlist_entries(playlist, tracks).unwrap();
    ed.save(&pdb).unwrap();
}

fn req(left: String, right: String, playlist: Option<&str>) -> DiffRequest {
    DiffRequest { left, right, playlist: playlist.map(Into::into) }
}

#[test]
fn entries_added_removed_and_moved_between_drives() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let b = dir.path().join("B");
    common::copy_dir(&a, &b);
    let new = common::export_new_track(&b, "New", "Track");
    // Track 1 moves to third, 5 goes, the new track joins at the end.
    set_entries(&b, aab(&b), &[2, 3, 1, 4, 6, 7, 8, 9, 10, 11, 12, new]);
    let pdb = b.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    ed.create_playlist("Fresh", 0, false).unwrap();
    ed.save(&pdb).unwrap();

    let d = api::diff(None, &req(path(&a), path(&b), None)).unwrap();
    assert_eq!((d.left.as_str(), d.right.as_str()), ("A", "B"));
    assert_eq!(d.added_tracks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["New - Track"]);
    assert!(d.removed_tracks.is_empty());
    assert_eq!(d.added_playlists, ["Fresh"]);
    assert_eq!(d.changed_playlists.len(), 1);
    let c = &d.changed_playlists[0];
    assert_eq!(c.path, "aab");
    assert_eq!(c.added.iter().map(|e| (e.track.id, e.position)).collect::<Vec<_>>(), [(new, 12)]);
    assert_eq!(c.removed.iter().map(|e| (e.track.id, e.position)).collect::<Vec<_>>(), [(5, 5)]);
    assert_eq!(c.moved.iter().map(|m| (m.track.id, m.from, m.to)).collect::<Vec<_>>(), [(1, 1, 3)]);

    // One playlist only: no collection-level changes.
    let d = api::diff(None, &req(path(&a), path(&b), Some("Fresh"))).unwrap();
    assert_eq!(d.added_playlists, ["Fresh"]);
    assert!(d.added_tracks.is_empty() && d.changed_playlists.is_empty());

    let d = api::diff(None, &req(path(&a), path(&a), None)).unwrap();
    assert!(d.is_empty());
    assert_eq!(d.unchanged_playlists, 1);
}

#[test]
fn a_drive_against_a_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let mut d = Device::open_export(&a).unwrap();
    let opts = PullOptions { library: Some("Club library".into()), ..Default::default() };
    let snap = sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap().snapshot.unwrap();
    set_entries(&a, aab(&a), &(1..=12).rev().collect::<Vec<_>>());

    let spec = format!("snapshot:{}", &snap.short_id[..6]);
    let d = api::diff(Some(&store), &req(spec.clone(), path(&a), None)).unwrap();
    assert_eq!(d.left, format!("snapshot {}", snap.short_id));
    // Reversed: one entry keeps its place, the other eleven moved.
    assert_eq!(d.changed_playlists[0].moved.len(), 11);
    // A library name means its latest snapshot.
    let by_name = api::diff(Some(&store), &req("Club library".into(), path(&a), None)).unwrap();
    assert_eq!(by_name.left, d.left);
    assert!(matches!(api::diff(None, &req(spec, path(&a), None)), Err(Error::Invalid(_))));
}

/// A snapshot's library is the drive's as it was then, whatever the drive has since: the whole
/// library, or its tracks a page at a time as `drive.library` pages them, the snapshot named by an
/// id prefix (with or without `snapshot:`) or a label.
#[test]
fn a_snapshot_s_library_is_the_drive_s_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let then = locallibrary::pdb::read_library(&a.join(paths::EXPORT_PDB)).unwrap();
    let mut d = Device::open_export(&a).unwrap();
    let snap = sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap().snapshot.unwrap();
    set_entries(&a, aab(&a), &[1]);
    let label = LabelRequest { label: "gig".into(), snapshot: Some(snap.short_id.clone()), remove: false };
    api::snapshot_label(&store, &label).unwrap();
    let read = |snapshot: &str, offset, limit| {
        api::snapshot_library(&store, &SnapshotLibraryRequest { snapshot: snapshot.into(), offset, limit })
    };

    let ids = |tracks: &[locallibrary::model::Track]| tracks.iter().map(|t| t.id).collect::<Vec<_>>();
    for spec in [&snap.short_id[..6], &format!("snapshot:{}", snap.short_id), "GIG"] {
        let whole = read(spec, None, None).unwrap();
        assert!(whole.page.is_none(), "{spec}");
        assert_eq!(ids(&whole.library.tracks), ids(&then.tracks), "{spec}");
        let playlist = whole.library.find_playlist("aab").unwrap();
        assert_eq!(playlist.track_ids, then.find_playlist("aab").unwrap().track_ids, "{spec}: not the drive's now");
    }
    let page = read(&snap.short_id, Some(2), Some(3)).unwrap();
    let p = page.page.unwrap();
    assert_eq!((p.offset, p.count, p.total), (2, 3, then.tracks.len()));
    assert_eq!(ids(&page.library.tracks), ids(&then.tracks[2..5]));
    assert_eq!(page.summary.tracks, then.tracks.len());
    assert!(matches!(read("ffffffff", None, None), Err(Error::NotFound(_))));
}

#[test]
fn restore_preview_shows_what_deleting_a_track_would_bring_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let file = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/House1.wav";
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let house = ed.add_track(&NewTrack { artist: Some("rekordbox".into()), ..NewTrack::new("House 1", file) }).unwrap();
    ed.add_to_playlist(aab(&root), house, None).unwrap();
    ed.save(&pdb).unwrap();
    let audio = root.join(file.trim_start_matches('/'));
    fs::create_dir_all(audio.parent().unwrap()).unwrap();
    fs::write(&audio, "demo sample").unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let edits = [Edit::DeleteTracks { tracks: TrackSelector::paths([file.to_string()]), keep_files: false }];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let before = common::library_files(&root);

    let p = api::restore_preview(&RestoreBackup { drive: path(&root), backup: None, force: false }).unwrap();
    assert_eq!(p.backup.reason, "before editing");
    assert_eq!(p.diff.added_tracks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["rekordbox - House 1"]);
    let entries = &p.diff.changed_playlists[0];
    assert_eq!((entries.path.as_str(), entries.added.len(), entries.added[0].position), ("aab", 1, 13));
    assert!(p.missing_audio.is_empty(), "the backup holds the deleted audio");
    assert_eq!(common::library_files(&root), before, "a preview changes nothing");
    assert!(matches!(
        api::restore_preview(&RestoreBackup { drive: path(&root), backup: Some("nope".into()), force: false }),
        Err(Error::NotFound(_))
    ));
}

/// A CDJ-3000X writes only to OneLibrary: diffs between drives, and what restoring a backup
/// would change, show its playlists too.
#[test]
fn diffs_show_what_changed_in_onelibrary() {
    use locallibrary::edit::Edit;
    use locallibrary::onelibrary::OneLibrary;
    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&a);
    let b = dir.path().join("B");
    common::copy_dir(&a, &b);
    common::apply(&b, &[Edit::CreatePlaylist { path: "Mine".into(), folder: false }]);
    let mut one = OneLibrary::open_root(&b).unwrap();
    one.create_playlist("Booth", 0, false).unwrap();
    fs::write(b.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    let d = api::diff(None, &req(path(&a), path(&b), None)).unwrap();
    let mut added = d.added_playlists.clone();
    added.sort();
    assert_eq!(added, ["Booth", "Mine"], "{d:?}");
    let p = api::restore_preview(&RestoreBackup { drive: path(&b), backup: None, force: false }).unwrap();
    let mut removed = p.diff.removed_playlists.clone();
    removed.sort();
    assert_eq!(removed, ["Booth", "Mine"], "{:?}", p.diff);
}

/// `diff --playlist` finds a playlist only OneLibrary has, and a playlist whose entries changed
/// only in OneLibrary isn't counted as the same.
#[test]
fn a_playlist_only_onelibrary_has_or_changed_is_diffed() {
    use locallibrary::onelibrary::OneLibrary;
    let dir = tempfile::tempdir().unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&a);
    let b = dir.path().join("B");
    common::copy_dir(&a, &b);
    let mut one = OneLibrary::open_root(&b).unwrap();
    let booth = one.create_playlist("Booth", 0, false).unwrap();
    one.add_to_playlist(booth, 1).unwrap();
    let aab = one.library().unwrap().find_playlist("aab").unwrap().id;
    one.add_to_playlist(aab, 1).unwrap();
    fs::write(b.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    let d = api::diff(None, &req(path(&a), path(&b), Some("Booth"))).unwrap();
    assert_eq!(d.added_playlists, ["Booth"], "{d:?}");
    let d = api::diff(None, &req(path(&a), path(&b), None)).unwrap();
    assert_eq!(d.changed_playlists.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["aab"], "{d:?}");
    assert_eq!(d.unchanged_playlists, 0, "{d:?}");
}
