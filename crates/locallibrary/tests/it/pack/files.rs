//! The files a pack takes along with its tracks, the packs it refuses, and a pack through the
//! service layer.

use crate::common::{self, path};

use std::fs;

use locallibrary::api;
use locallibrary::artwork;
use locallibrary::device::Device;
use locallibrary::pack::{self, PackOptions};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{Error, paths};

#[test]
fn a_pack_brings_its_tracks_artwork_and_says_which_have_no_audio() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::media::set_art(&root, &[1]);
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let gig = ed.create_playlist("Gig", 0, false).unwrap();
    ed.set_playlist_entries(gig, &[1, 2]).unwrap();
    ed.save(&pdb_path).unwrap();
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let (art, gone) = (lib.track(1).unwrap().artwork_path.clone().unwrap(), lib.track(2).unwrap());
    fs::remove_file(paths::to_fs(&root, &paths::key(&gone.file_path))).unwrap();

    let src = Device::open_export(&root).unwrap();
    fs::create_dir(dir.path().join("Club")).unwrap();
    let mut club = Device::open(dir.path().join("Club")).unwrap();
    let plan =
        pack::plan(&src, &club, &PackOptions { playlists: vec!["Gig".into()], ..Default::default() }, &Ctx::silent())
            .unwrap();
    assert!(!plan.is_noop(), "the databases are always written");
    let warning = "1 track(s) have no audio file on Main; players won't load them";
    assert!(plan.warnings.iter().any(|w| w == warning), "{:?}", plan.warnings);
    let copied: Vec<&str> = plan.copies.iter().map(|c| c.path.as_str()).collect();
    for key in [paths::key(&art), artwork::medium_variant(&paths::key(&art))] {
        assert!(copied.contains(&key.as_str()), "{key} in {copied:?}");
    }

    let mut full = plan.clone();
    full.fits = false;
    let err = pack::apply(&full, &src, &mut club, None, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::NoSpace { .. }), "{err}");
    assert!(!club.has_export());
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    let packed = club.library().unwrap();
    assert_eq!(packed.tracks.len(), 2);
    let image = packed.tracks.iter().find_map(|t| t.artwork_path.clone()).unwrap();
    assert_eq!(fs::read(club.path(&paths::key(&image))).unwrap(), fs::read(root.join(paths::key(&art))).unwrap());
}

/// Packing copies the chosen playlists' tracks onto the target, as planned, and registers it.
#[test]
fn a_drive_is_packed_for_a_set() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let club = common::bigger_drive(dir.path(), "Club");
    let stick = dir.path().join("Stick");
    std::fs::create_dir(&stick).unwrap();
    let playlist =
        Device::open(&club).unwrap().library().unwrap().playlists.into_iter().find(|p| !p.is_folder).unwrap();
    let req = api::PackRequest {
        source: path(&club),
        target: path(&stick),
        options: PackOptions { playlists: vec![playlist.name.clone()], ..Default::default() },
    };

    let plan = api::pack_plan(&req, &Ctx::silent()).unwrap();
    let count = playlist.track_ids.len();
    assert_eq!(plan.summary[0], format!("pack {} from Club onto Stick ({count} track(s))", playlist.name));
    let report = api::pack_apply(Some(&store), &req, &Ctx::silent()).unwrap();
    assert!(report.copied > 0);
    assert_eq!(Device::open(&stick).unwrap().library().unwrap().tracks.len(), count);
    let written: Vec<u32> = store.drives().unwrap().into_values().map(|d| d.log.writes).collect();
    assert_eq!(written, [1], "the stick is registered with its write");
}

/// Regression: a pack (and a collection export, which writes the same way) that copied a file
/// moments after its source was written took the copy as current next time while size and mtime
/// matched, though the source could have been rewritten since in the same second or two. That
/// copy is copied again, and a rewrite at the same size and mtime reaches the target.
#[test]
fn a_file_packed_as_it_was_written_is_copied_again_next_time() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::bigger_drive(dir.path(), "Main");
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let audio = paths::key(&src.library().unwrap().tracks[0].file_path);
    let file = paths::to_fs(&src_root, &audio);
    // Ahead of the clock, so the copy is made within its window however slow the machine is.
    let written = chrono::Utc::now().timestamp() + 30;
    locallibrary::fsutil::set_mtime(&file, written).unwrap();
    let opts = PackOptions { playlists: vec!["aab".into()], ..Default::default() };
    pack::apply(&pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap(), &src, &mut club, None, &Ctx::silent())
        .unwrap();

    let mut bytes = fs::read(&file).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&file, &bytes).unwrap();
    locallibrary::fsutil::set_mtime(&file, written).unwrap();
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    assert!(plan.copies.iter().any(|c| c.path == audio), "{:?}", plan.copies);
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    assert_eq!(fs::read(paths::to_fs(&club_root, &audio)).unwrap(), bytes);
}
