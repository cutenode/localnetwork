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
