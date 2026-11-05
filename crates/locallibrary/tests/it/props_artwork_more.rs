//! Artwork the main properties leave out: an image read from the art an audio file embeds (or
//! refused when it embeds none), any image made a PNG of its size, art found embedded in a drive's
//! audio, art that won't decode left off, tracks OneLibrary doesn't list said, and nothing to
//! take back when no art was ever embedded.

use crate::common::{self, media};

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::artwork::{self, Artwork};
use locallibrary::device::Device;
use locallibrary::progress::{CancelToken, Ctx, NoProgress};
use locallibrary::store::Store;
use locallibrary::{Error, paths};
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;

/// The medium copy of an image sits beside it, `_m` before its extension (or at the end without
/// one); any image becomes a PNG of its own size, and bytes that aren't one are refused.
#[hegel::test(test_cases = 100)]
fn medium_copies_and_pngs(tc: TestCase) {
    let dir = tc.draw(gs::sampled_from(vec!["PIONEER/Artwork/00001", "PIONEER/Art.work", ""]));
    let name = tc.draw(gs::sampled_from(vec!["a1.jpg", "b12.png", "noext"]));
    let key = if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") };
    let medium = artwork::medium_variant(&key);
    match name.rsplit_once('.') {
        Some((stem, ext)) => assert_eq!(medium, key.replace(name, &format!("{stem}_m.{ext}"))),
        None => assert_eq!(medium, format!("{key}_m")),
    }
    let (w, h) = (
        tc.draw(gs::integers::<u32>().min_value(1).max_value(40)),
        tc.draw(gs::integers::<u32>().min_value(1).max_value(40)),
    );
    let png = artwork::to_png(&media::png(w, h, tc.draw(gs::integers::<u8>()))).unwrap();
    let back = image::load_from_memory(&png).unwrap();
    assert_eq!((back.width(), back.height()), (w, h));
    assert!(matches!(artwork::to_png(b"not an image"), Err(Error::Image(_))));
}

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn art_comes_from_what_audio_embeds_and_what_decodes(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    assert!(artwork::restore_embedded(&store, None).is_err_and(|e| e.to_string().contains("no embedded art")));

    // An audio file read as an image: the art it embeds, if any.
    let audio = dir.path().join("song.wav");
    fs::write(&audio, media::wav()).unwrap();
    assert!(matches!(Artwork::from_path(&audio), Err(Error::NotFound(_))));
    let shade = tc.draw(gs::integers::<u8>());
    let art = Artwork::from_bytes(&media::png(40, 40, shade)).unwrap();
    artwork::embed_in_file(&audio, &art).unwrap();
    assert!(Artwork::from_path(&audio).is_ok());

    // A drive whose tracks embed art, some of it broken, some tracks' audio gone.
    let root = common::bigger_drive(dir.path(), "A");
    let lib = common::library(&root);
    let mut embeds = Vec::new();
    let mut broken = Vec::new();
    let mut gone = Vec::new();
    for t in lib.tracks.iter().take(4) {
        let path = paths::to_fs(&root, &paths::key(&t.file_path));
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => {
                fs::write(&path, media::mp3()).unwrap();
                artwork::embed_in_file(&path, &art).unwrap();
                embeds.push(t.id);
            }
            1 => {
                // A front cover whose bytes aren't an image: the track gets no art from it.
                fs::write(&path, media::mp3()).unwrap();
                artwork::embed_in_file(&path, &art).unwrap();
                let mut tagged = lofty::read_from_path(&path).unwrap();
                let tag = tagged.primary_tag_mut().unwrap();
                tag.remove_picture_type(PictureType::CoverFront);
                tag.push_picture(
                    Picture::unchecked(b"not a png at all".to_vec())
                        .pic_type(PictureType::CoverFront)
                        .mime_type(MimeType::Png)
                        .build(),
                );
                tag.save_to_path(&path, WriteOptions::default()).unwrap();
                broken.push(t.id);
            }
            2 => {
                fs::remove_file(&path).unwrap();
                gone.push(t.id);
            }
            _ => {}
        }
    }
    let mut d = Device::open_export(&root).unwrap();
    let plan = artwork::plan_auto(&d, &lib);
    for item in &plan.found {
        if embeds.contains(&item.track.id) || broken.contains(&item.track.id) {
            assert_eq!(item.source, "embedded", "{item:?}");
        }
    }
    let found: Vec<u32> = plan.found.iter().map(|i| i.track.id).collect();
    for id in &embeds {
        assert!(found.contains(id), "#{id} embeds art");
    }
    for id in &gone {
        assert!(plan.not_found.iter().any(|t| t.id == *id), "#{id}'s audio is gone");
    }
    let r = artwork::apply_auto(&mut d, &plan, &Ctx::silent()).unwrap();
    let after = d.library().unwrap();
    for id in &embeds {
        assert!(after.track(*id).unwrap().artwork_path.is_some(), "#{id} got its art");
    }
    for id in &broken {
        assert!(after.track(*id).unwrap().artwork_path.is_none(), "#{id}'s art doesn't decode");
    }
    assert_eq!(r.tracks_updated, embeds.len());
}

/// Art set on tracks of a drive whose OneLibrary lacks some of them: they get it in export.pdb,
/// and the drive says OneLibrary's players won't show it.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn art_for_tracks_onelibrary_lacks_is_said(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let late = common::export_new_track(&root, "Late", "Export");
    let lib = common::library(&root);
    let mut ids: Vec<u32> = crate::drawn::some(&tc, &lib.tracks.iter().map(|t| t.id).collect::<Vec<_>>());
    ids.push(late);
    let warnings = common::Warnings::default();
    let mut d = Device::open_export(&root).unwrap();
    media::set_artwork(&mut d, &ids, &media::png(30, 30, 7), &warnings.ctx()).unwrap();
    assert!(warnings.said().iter().any(|w| w.contains("aren't in exportLibrary.db")), "{:?}", warnings.said());
    assert!(d.library().unwrap().track(late).unwrap().artwork_path.is_some());
}

/// What applying found art does to a plan that went stale.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stale {
    No,
    /// It names a track the drive doesn't have: passed over.
    UnknownTrack,
    /// Its image is outside the drive, or gone: refused.
    Escaped,
    Deleted,
    /// Cancelled before writing: refused.
    Cancelled,
}

/// Only cover, folder, front, album or artwork images in a picture format are found beside the
/// audio (and a track the drive lacks has none to read); applying the plan sets them on the tracks
/// it names that the drive has, and a plan whose image left the drive or went, or one cancelled,
/// fails with the drive as it was. Each way a plan goes stale is tried on a drive of its own.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_automatic_art_applies_only_what_it_can_still_read(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let pictured = tc.draw(gs::booleans());
    let name = tc.draw(gs::sampled_from(vec!["Cover.JPG", "folder.png", "front.jpeg"]));
    let ways = match pictured {
        true => vec![Stale::No, Stale::UnknownTrack, Stale::Escaped, Stale::Deleted, Stale::Cancelled],
        false => vec![Stale::No],
    };
    for stale in ways {
        let root = common::demo_drive(&dir.path().join(format!("{stale:?}")), "DNA");
        let lib = common::library(&root);
        let folder = paths::to_fs(&root, &paths::key(&lib.tracks[0].file_path)).parent().unwrap().to_path_buf();
        fs::write(folder.join("cover.txt"), b"liner notes").unwrap();
        let cover = folder.join(name);
        if pictured {
            fs::write(&cover, media::png(32, 32, 3)).unwrap();
        }
        let mut d = Device::open_export(&root).unwrap();
        assert!(matches!(artwork::read_from_drive(&d, &lib, 9999), Err(Error::NotFound(_))));
        let mut plan = artwork::plan_auto(&d, &lib);
        assert_eq!(plan.found.is_empty(), !pictured, "{:?}", plan.found);
        assert_eq!(plan.found.len() + plan.not_found.len(), lib.tracks.len());
        let cancel = CancelToken::new();
        match stale {
            Stale::No => {}
            Stale::UnknownTrack => {
                let mut item = plan.found[0].clone();
                item.track.id = 9999;
                plan.found.push(item);
            }
            Stale::Escaped => {
                fs::write(dir.path().join("Escaped/outside.png"), media::png(32, 32, 4)).unwrap();
                plan.found[0].source = "../outside.png".into();
            }
            Stale::Deleted => fs::remove_file(&cover).unwrap(),
            Stale::Cancelled => cancel.cancel(),
        }
        let set = plan.found.iter().filter(|i| i.track.id != 9999).count();
        let before = common::Before::take(&root);
        let r = artwork::apply_auto(&mut d, &plan, &Ctx::new(&NoProgress, cancel));
        match stale {
            Stale::No | Stale::UnknownTrack => {
                let r = r.unwrap();
                assert_eq!((r.tracks_updated, r.artwork_rows_added), (set, usize::from(set > 0)));
                let after = d.library().unwrap();
                for item in plan.found.iter().filter(|i| i.track.id != 9999) {
                    assert!(after.track(item.track.id).unwrap().artwork_path.is_some(), "{item:?}");
                }
            }
            _ => {
                assert!(r.is_err(), "{stale:?}: {r:?}");
                before.untouched(&root).unwrap();
            }
        }
    }
}
