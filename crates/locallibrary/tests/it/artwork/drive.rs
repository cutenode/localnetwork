//! Cover art on a drive: found in the tracks' own audio or next to it, previewed, read back
//! through the service layer, and kept in step with OneLibrary's database where it can be.

use crate::common::media::{mp3, png, set_artwork};
use crate::common::{self, Warnings, path};

use locallibrary::api::{self, DriveRef, TrackSelector};
use locallibrary::artwork::{self, Artwork};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::{Error, paths};

#[test]
fn art_embedded_in_a_tracks_audio_goes_on_the_drive() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open_export(common::demo_drive(dir.path(), "DNA")).unwrap();
    let lib = d.library().unwrap();
    let audio = d.path(&paths::key(&lib.track(1).unwrap().file_path));
    std::fs::write(&audio, mp3()).unwrap();
    artwork::embed_in_file(&audio, &Artwork::from_bytes(&png(300, 300, 42)).unwrap()).unwrap();

    let plan = artwork::plan_auto(&d, &lib);
    let found: Vec<(u32, &str)> = plan.found.iter().map(|f| (f.track.id, f.source.as_str())).collect();
    assert_eq!(found, [(1, "embedded")]);
    assert_eq!(plan.not_found.iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
    let r = artwork::apply_auto(&mut d, &plan, &Ctx::silent()).unwrap();
    assert_eq!((r.tracks_updated, r.artwork_rows_added), (1, 1));
    let shown = artwork::read_from_drive(&d, &d.library().unwrap(), 1).unwrap().unwrap();
    assert_eq!(image::load_from_memory(&shown).unwrap().width(), artwork::MEDIUM_SIZE);
}

/// A cover image that doesn't decode is skipped with a warning, and nothing to apply changes
/// nothing.
#[test]
fn art_that_cant_be_used_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let mut d = Device::open_export(&root).unwrap();
    let plan = artwork::plan_auto(&d, &d.library().unwrap());
    let r = artwork::apply_auto(&mut d, &plan, &Ctx::silent()).unwrap();
    assert_eq!((r.tracks_updated, r.backup), (0, None));

    let album = d.path("Contents/[a]pendics.shuffle/Framed And Defamed");
    std::fs::write(album.join("cover.png"), b"not a picture").unwrap();
    let plan = artwork::plan_auto(&d, &d.library().unwrap());
    assert_eq!(plan.found.len(), 4);
    let warnings = Warnings::default();
    let before = common::Before::take(&root);
    let r = artwork::apply_auto(&mut d, &plan, &warnings.ctx()).unwrap();
    assert_eq!(r.artwork_rows_added, 0);
    let said = warnings.said();
    assert!(said.iter().any(|w| w.starts_with("skipping undecodable artwork for 4 track(s)")), "{said:?}");
    before.untouched(&root).unwrap();
}

/// A preview is the medium-sized image, else the thumbnail, else nothing.
#[test]
fn previews_fall_back_to_the_thumbnail() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open_export(common::demo_drive(dir.path(), "DNA")).unwrap();
    assert!(matches!(set_artwork(&mut d, &[], &png(100, 100, 42), &Ctx::silent()), Err(Error::Invalid(_))));
    set_artwork(&mut d, &[1], &png(300, 300, 42), &Ctx::silent()).unwrap();
    let lib = d.library().unwrap();
    let preview =
        || artwork::read_from_drive(&d, &lib, 1).unwrap().map(|b| image::load_from_memory(&b).unwrap().width());
    assert_eq!(preview(), Some(artwork::MEDIUM_SIZE));
    let key = paths::key(lib.track(1).unwrap().artwork_path.as_ref().unwrap());
    std::fs::remove_file(d.path(&artwork::medium_variant(&key))).unwrap();
    assert_eq!(preview(), Some(artwork::THUMB_SIZE));
    std::fs::remove_file(d.path(&key)).unwrap();
    assert_eq!(preview(), None);
    assert_eq!(artwork::read_from_drive(&d, &lib, 2).unwrap(), None, "no art");
    assert!(matches!(artwork::read_from_drive(&d, &lib, 99), Err(Error::NotFound(_))));
}

/// A track exported after the OneLibrary database was written gets its art in export.pdb, and
/// a warning that players reading OneLibrary won't show it.
#[test]
fn a_track_onelibrary_doesnt_have_gets_art_in_export_pdb_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "DNA");
    common::add_onelibrary(&root);
    let id = common::export_new_track(&root, "Newcomer", "Fresh");
    let mut d = Device::open_export(&root).unwrap();
    let warnings = Warnings::default();
    let ctx = warnings.ctx();
    let r = set_artwork(&mut d, &[id], &png(300, 300, 42), &ctx).unwrap();
    assert!(r.databases.iter().any(|d| d == "export.pdb"), "{:?}", r.databases);
    assert!(!r.databases.iter().any(|d| d == "exportLibrary.db"), "{:?}", r.databases);
    let said = warnings.said();
    assert!(said.iter().any(|w| w.contains("1 selected track(s) aren't in exportLibrary.db")), "{said:?}");
    assert!(artwork::read_from_drive(&d, &d.library().unwrap(), id).unwrap().is_some());
}

/// A track whose path leaves the drive isn't looked for art outside it.
#[test]
fn art_isnt_looked_for_outside_the_drive() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "DNA");
    std::fs::write(dir.path().join("cover.png"), png(64, 64, 42)).unwrap();
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    let id = ed.add_track(&rekordbox_pdb::NewTrack::new("Escape", "/Contents/../../outside.mp3")).unwrap();
    ed.save(&pdb).unwrap();
    let d = Device::open_export(&root).unwrap();
    let plan = artwork::plan_auto(&d, &d.library().unwrap());
    assert!(plan.found.is_empty(), "{:?}", plan.found.iter().map(|f| &f.source).collect::<Vec<_>>());
    assert!(plan.not_found.iter().any(|t| t.id == id));
}

/// Artwork set on one track is planned as an edit, then read back as a JPEG; the other track
/// still has none until an image next to its audio is found for it.
#[test]
fn art_is_set_as_planned_found_next_to_the_audio_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let drive = DriveRef { drive: path(&root) };
    let cover = dir.path().join("cover.png");
    std::fs::write(&cover, png(500, 400, 40)).unwrap();
    let ids: Vec<u32> = Device::open(&root).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    let get = |track| api::artwork_get(&api::GetArtwork { drive: path(&root), track }).unwrap();
    assert_eq!(api::artwork_missing(&drive).unwrap().len(), 2);
    assert!(get(ids[0]).is_none());

    let set = api::SetArtwork {
        drive: path(&root),
        image: cover,
        tracks: TrackSelector { ids: vec![ids[0]], ..Default::default() },
    };
    let plan = api::artwork_set_plan(&set, &Ctx::silent()).unwrap();
    assert_eq!(plan.changes, ["set the artwork of 1 track(s)"]);
    assert_eq!(api::artwork_set(&set, &Ctx::silent()).unwrap().changes, 1);
    let art = get(ids[0]).expect("set");
    assert_eq!(art.mime, "image/jpeg");
    assert!(art.base64.starts_with("/9j/"), "a JPEG's first bytes, in base64");
    let missing: Vec<u32> = api::artwork_missing(&drive).unwrap().into_iter().map(|t| t.id).collect();
    assert_eq!(missing, [ids[1]]);

    assert!(api::artwork_auto_plan(&drive).unwrap().noop, "no image next to the other track's audio");
    let audio = Device::open(&root).unwrap().library().unwrap().track(ids[1]).unwrap().file_path.clone();
    let folder = locallibrary::paths::to_fs(&root, &locallibrary::paths::key(&audio)).parent().unwrap().to_path_buf();
    std::fs::write(folder.join("cover.png"), png(64, 64, 40)).unwrap();
    let auto = api::artwork_auto_plan(&drive).unwrap();
    assert_eq!(auto.found.len(), 1);
    assert_eq!(api::artwork_auto_apply(&drive, &Ctx::silent()).unwrap().tracks_updated, 1);
    assert!(api::artwork_missing(&drive).unwrap().is_empty());
}
