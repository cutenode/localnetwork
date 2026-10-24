//! Edits on drives that test what export.pdb and its companions can hold: names too long for
//! export.pdb, an edit OneLibrary lacks that export.pdb has, tracks deleted whose audio is outside
//! `Contents/`, and My Tags following replaced and deleted tracks, whatever exportExt.pdb holds.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, TrackFields};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{paths, pdb, tags};
use rekordbox_pdb::NewTrack;

fn ids(ids: &[u32]) -> TrackSelector {
    TrackSelector { ids: ids.to_vec(), ..Default::default() }
}

/// A name export.pdb can hold is set and reads back as given (an album cleared, a key set
/// alongside); one too long for it is refused, said, before anything on the drive changes.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(len = 12usize, album = Some(""), key = Some("8A"))]
#[hegel::explicit_test_case(len = 5000usize, album = None::<&str>, key = None::<&str>)]
fn names_export_pdb_can_hold_are_set_and_longer_ones_refused(tc: TestCase) {
    let len = tc.draw(gs::integers::<usize>().min_value(1).max_value(6000));
    let album = tc.draw(gs::optional(gs::sampled_from(vec!["", " ", "Album"])));
    let key = tc.draw(gs::optional(gs::sampled_from(vec!["8A", "Am", ""])));
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let t = common::library(&root).tracks[3].clone();
    let artist = "x".repeat(len);
    let fields = TrackFields {
        artist: Some(artist.clone()),
        album: album.map(String::from),
        key: key.map(String::from),
        ..Default::default()
    };
    let before = common::Before::take(&root);
    let mut d = Device::open_export(&root).unwrap();
    let plan =
        edit::plan(&d, &[Edit::EditTracks { tracks: ids(&[t.id]), fields }], &EditOptions::default(), &Ctx::silent());
    let plan = match plan {
        Ok(plan) => plan,
        Err(e) => {
            assert!(e.to_string().contains("the artist name is too long for export.pdb"), "{e}");
            assert!(len > 1000, "{len} characters refused");
            before.untouched(&root).unwrap();
            return;
        }
    };
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let now = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().track(t.id).unwrap().clone();
    assert_eq!(now.artist, artist);
    if let Some(a) = album {
        assert_eq!(now.album, a.trim());
    }
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        assert_eq!(now.key, k);
    }
}

/// An edit a drive's export.pdb has already but its OneLibrary database lacks (rekordbox
/// exported over that alone) is made in OneLibrary alone, and said so.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn an_edit_onelibrary_lacks_is_made_there_alone(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let plus = fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap();
    let lib = common::library(&root);
    let picked: Vec<u32> =
        tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(12)).min_size(1).max_size(3).unique(true));
    let rating = tc.draw(gs::integers::<u8>().min_value(1).max_value(5));
    let rated =
        Edit::EditTracks { tracks: ids(&picked), fields: TrackFields { rating: Some(rating), ..Default::default() } };
    common::apply(&root, std::slice::from_ref(&rated));
    let pdb_after = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    // rekordbox exports OneLibrary's database again, without it.
    fs::write(root.join(paths::LIBRARY_PLUS_DB), &plus).unwrap();
    for key in [paths::LIBRARY_PLUS_WAL, paths::LIBRARY_PLUS_SHM] {
        let _ = fs::remove_file(root.join(key));
    }
    let plan = common::apply(&root, &[rated]);
    assert!(
        plan.changes.iter().any(|c| c.contains("make them in exportLibrary.db, which lacks them")),
        "{:?}",
        plan.changes
    );
    let files: Vec<&str> = picked.iter().map(|id| lib.track(*id).unwrap().file_path.as_str()).collect();
    for t in &common::plus_library(&root).tracks {
        let was = lib.tracks.iter().find(|o| o.file_path == t.file_path).unwrap().rating;
        assert_eq!(t.rating, if files.contains(&t.file_path.as_str()) { rating } else { was }, "{}", t.file_path);
    }
    assert_eq!(pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks, pdb_after.tracks);
}

/// Deleting tracks removes their audio under `Contents/` and never audio outside it, which is
/// left on the drive and said so.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn deleted_tracks_audio_outside_contents_is_left_and_said(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let outside = common::add_track(&root, NewTrack::new("Outside", "/Music/Outside.mp3"), b"ID3 outside");
    let inside = common::add_track(&root, NewTrack::new("Inside", "/Contents/Inside.mp3"), b"ID3 inside");
    let doomed: Vec<u32> = tc.draw(gs::subsequences(&[outside, inside]).min_size(1));
    let mut d = Device::open_export(&root).unwrap();
    let e = Edit::DeleteTracks { tracks: ids(&doomed), keep_files: false };
    let plan = edit::plan(&d, &[e], &EditOptions::default(), &Ctx::silent()).unwrap();
    let left = plan.warnings.iter().any(|w| w.contains("outside Contents/ are left on the drive"));
    assert_eq!(left, doomed.contains(&outside), "{:?}", plan.warnings);
    assert_eq!(plan.files.contains(&"Contents/Inside.mp3".to_string()), doomed.contains(&inside), "{:?}", plan.files);
    assert!(!plan.files.iter().any(|f| f.starts_with("Music/")));
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert!(root.join("Music/Outside.mp3").is_file());
    assert_eq!(root.join("Contents/Inside.mp3").is_file(), !doomed.contains(&inside));
}

/// My Tags follow the tracks of export.pdb: a replaced track's links move to its keeper, a
/// deleted one's go. An exportExt.pdb that can't be read (not a database, or a folder) doesn't
/// stop the edit: it's left as it was, and the edit says its My Tags weren't updated.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(ext = 0u8, replace = true, delete = true)]
#[hegel::explicit_test_case(ext = 1u8, replace = true, delete = false)]
#[hegel::explicit_test_case(ext = 2u8, replace = false, delete = true)]
fn my_tags_follow_replaced_and_deleted_tracks(tc: TestCase) {
    let ext = tc.draw(gs::integers::<u8>().max_value(2));
    let replace = tc.draw(gs::booleans());
    let delete = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let third = common::export_new_track(&root, "Artist", "Extra");
    let lib = common::library(&root);
    let [a, b, c] = [lib.tracks[0].id, lib.tracks[1].id, third];
    let mut d = Device::open_export(&root).unwrap();
    let tag = tags::Edit::Add { tag: "Genre/Deep".into(), tracks: vec![a, b] };
    tags::apply(&tags::plan(&d, &tag).unwrap(), &mut d, &Ctx::silent()).unwrap();
    let ext_path = root.join(paths::EXPORT_EXT_PDB);
    match ext {
        1 => fs::write(&ext_path, b"not exportExt.pdb").unwrap(),
        2 => {
            fs::remove_file(&ext_path).unwrap();
            fs::create_dir_all(ext_path.join("inside")).unwrap();
        }
        _ => {}
    }
    let ext_before = fs::read(&ext_path).ok();
    let mut edits = Vec::new();
    if replace {
        edits.push(Edit::ReplaceTrack { duplicate: ids(&[a]), keep: ids(&[c]), keep_duplicate: false });
    }
    if delete {
        edits.push(Edit::DeleteTracks { tracks: ids(&[b]), keep_files: true });
    }
    if edits.is_empty() {
        return;
    }
    let mut d = Device::open_export(&root).unwrap();
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let warned = report.warnings.iter().any(|w| w.contains("My Tags in exportExt.pdb weren't updated"));
    assert_eq!(warned, ext != 0, "{:?}", report.warnings);
    if ext != 0 {
        assert_eq!(fs::read(&ext_path).ok(), ext_before);
        return;
    }
    let ext_db = rekordbox_pdb::ExtDatabase::from_file(&ext_path).unwrap();
    let mut links: Vec<u32> = ext_db.tag_tracks.iter().map(|l| l.track_id).collect();
    links.sort_unstable();
    let mut want = vec![if replace { c } else { a }];
    if !delete {
        want.push(b);
    }
    want.sort_unstable();
    assert_eq!(links, want);
}

/// An analysis file of one `PSSI` section saying a track's mood (1 high, 2 mid, 3 low).
fn mood_file(mood: u16) -> Vec<u8> {
    let mut body = vec![0u8; 14];
    body[0..2].copy_from_slice(&mood.to_be_bytes());
    body[8..10].copy_from_slice(&64u16.to_be_bytes());
    let mut entry = vec![0u8; 24];
    entry[0..2].copy_from_slice(&1u16.to_be_bytes());
    entry[2..4].copy_from_slice(&1u16.to_be_bytes());
    entry[4..6].copy_from_slice(&1u16.to_be_bytes());
    body.extend(entry);
    let mut section = b"PSSI".to_vec();
    section.extend(32u32.to_be_bytes());
    section.extend((18 + body.len() as u32).to_be_bytes());
    section.extend(24u32.to_be_bytes());
    section.extend(1u16.to_be_bytes());
    section.extend(body);
    let mut out = b"PMAI".to_vec();
    out.extend(28u32.to_be_bytes());
    out.extend(((28 + section.len()) as u32).to_be_bytes());
    out.extend([0; 16]);
    out.extend(section);
    out
}

/// Ordering a playlist for mixing, whatever moods its tracks' analysis gives them (high, mid or
/// low energy, or none), keeps exactly its entries, in an order of its own or into a new one.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn ordering_for_mixing_keeps_a_playlists_entries_whatever_their_moods(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let lib = common::library(&root);
    let p = lib.playlists.iter().find(|p| !p.is_folder && p.track_ids.len() > 1).expect("a playlist").clone();
    for id in &p.track_ids {
        let t = lib.track(*id).unwrap();
        let mood = tc.draw(gs::integers::<u16>().max_value(3));
        if mood > 0 && !t.analyze_path.is_empty() {
            let ext = paths::to_fs(&root, &paths::key(&t.analyze_path).replace(".DAT", ".EXT"));
            fs::create_dir_all(ext.parent().unwrap()).unwrap();
            fs::write(ext, mood_file(mood)).unwrap();
        }
    }
    let name = lib.playlist_path(p.id);
    let into = tc.draw(gs::booleans()).then(|| "Ordered".to_string());
    common::apply(&root, &[Edit::PlanSet { playlist: name.clone(), start: None, into: into.clone() }]);
    let after = common::library(&root);
    let ordered = after.find_playlist(into.as_deref().unwrap_or(&name)).unwrap();
    let (mut want, mut got) = (p.track_ids.clone(), ordered.track_ids.clone());
    want.sort_unstable();
    got.sort_unstable();
    assert_eq!(got, want);
}

/// An edit of any kind is refused on a drive without a rekordbox export, and leaves it as it was.
#[hegel::test(test_cases = 20)]
fn edits_need_an_export(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::blank(dir.path(), "Blank");
    let edit = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => Edit::CreatePlaylist { path: tc.draw(gs::text().alphabet("ab/").min_size(1).max_size(5)), folder: false },
        1 => Edit::DeleteTracks { tracks: ids(&[1]), keep_files: tc.draw(gs::booleans()) },
        _ => Edit::EditTracks { tracks: ids(&[1]), fields: TrackFields { rating: Some(3), ..Default::default() } },
    };
    let before = common::drive_files(&root);
    let d = Device::open(&root).unwrap();
    let err = edit::plan(&d, &[edit], &EditOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err, locallibrary::Error::NotAnExport(_)), "{err}");
    assert_eq!(common::drive_files(&root), before);
}

/// Packing playlists of a drive rekordbox exported for OneLibrary players only plans a library
/// of exactly those playlists' tracks, made from its OneLibrary database alone.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn packing_a_onelibrary_only_drive_takes_the_chosen_playlists_tracks(tc: TestCase) {
    use locallibrary::pack::{self, PackOptions};
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::onelibrary_only(&root);
    let lib = common::plus_library(&root);
    let lists: Vec<_> = lib.playlists.iter().filter(|p| !p.is_folder).collect();
    let chosen = tc.draw(gs::sampled_from(&lists));
    let history = tc.draw(gs::booleans());
    let target = common::blank(dir.path(), "Club");
    let opts = PackOptions { playlists: vec![lib.playlist_path(chosen.id)], history, ..Default::default() };
    let plan =
        pack::plan(&Device::open(&root).unwrap(), &Device::open(&target).unwrap(), &opts, &Ctx::silent()).unwrap();
    let mut used: Vec<u32> = chosen.track_ids.clone();
    if history {
        used.extend(lib.history.iter().flat_map(|h| h.track_ids.iter().copied()));
    }
    used.sort_unstable();
    used.dedup();
    assert_eq!(plan.tracks, used.len());
}
