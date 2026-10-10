use crate::common::{self, path};

use std::fs;
use std::path::Path;

use locallibrary::api::{self, DiffRequest, RestoreBackup};
use locallibrary::clean;
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
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

#[test]
fn restore_preview_shows_what_a_clean_would_bring_back() {
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
    let plan = clean::plan(&d, &Ctx::silent()).unwrap();
    clean::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let before = common::library_files(&root);

    let p = api::restore_preview(&RestoreBackup { drive: path(&root), backup: None, force: false }).unwrap();
    assert_eq!(p.backup.reason, "before cleaning");
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
