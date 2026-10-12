//! Cleaning rekordbox's demo samples off drives laid out every way (both databases, export.pdb
//! alone, OneLibrary alone), with something changed between the plan and applying it: a plan
//! applies only to the drive it was made for, a database that can't be read is reported rather
//! than written, and audio outside `Contents/` is never deleted.

use crate::common::{self, Before, Warnings};

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::clean;
use locallibrary::device::Device;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{Error, paths};
use rekordbox_pdb::{NewTrack, PdbEditor};

/// Titles on a drive, in each database it has that can be read (an unreadable OneLibrary is
/// left out: the property checks it's left as it was).
fn titles(root: &std::path::Path) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    if root.join(paths::EXPORT_PDB).is_file() {
        out.push(common::library(root).tracks.into_iter().map(|t| t.title).collect());
    }
    if let Ok(lib) = OneLibrary::open_root(root).and_then(|one| one.library()) {
        out.push(lib.tracks.into_iter().map(|t| t.title).collect());
    }
    out
}

#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(layout = 2u8, samples = 1usize, outside = true, fault = 0u8, art = 1u8)]
#[hegel::explicit_test_case(layout = 2u8, samples = 2usize, outside = false, fault = 2u8, art = 1u8)]
#[hegel::explicit_test_case(layout = 2u8, samples = 1usize, outside = false, fault = 4u8, art = 0u8)]
#[hegel::explicit_test_case(layout = 0u8, samples = 1usize, outside = true, fault = 1u8, art = 2u8)]
#[hegel::explicit_test_case(layout = 1u8, samples = 2usize, outside = false, fault = 3u8, art = 0u8)]
#[hegel::explicit_test_case(layout = 0u8, samples = 1usize, outside = false, fault = 2u8, art = 2u8)]
#[hegel::explicit_test_case(layout = 0u8, samples = 1usize, outside = false, fault = 4u8, art = 0u8)]
fn a_clean_plan_holds_only_for_the_drive_it_was_made_for(tc: TestCase) {
    // Both databases, export.pdb alone, OneLibrary alone.
    let layout = tc.draw(gs::integers::<u8>().max_value(2));
    let samples = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
    // One more sample whose audio is outside Contents/.
    let outside = tc.draw(gs::booleans());
    // Between plan and apply: nothing, export.pdb exported again, OneLibrary exported again,
    // exportExt.pdb damaged; or before planning, OneLibrary damaged.
    let fault = tc.draw(gs::integers::<u8>().max_value(4));
    // In OneLibrary, a sample's image: none, its own, or one a playlist shows too.
    let art = tc.draw(gs::integers::<u8>().max_value(2));
    let (has_pdb, has_plus) = (layout != 2, layout != 1);

    let dir = tempfile::tempdir().unwrap();
    let blank = common::blank(dir.path(), "Blank");
    assert!(matches!(clean::plan(&Device::open(&blank).unwrap(), &Ctx::silent()), Err(Error::NotAnExport(_))));
    let root = common::bigger_drive(dir.path(), "Main");
    let pdb_path = root.join(paths::EXPORT_PDB);
    for n in 0..samples {
        common::export_new_track(&root, "rekordbox", &format!("House {}", n + 1));
    }
    if outside {
        let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
        ed.add_track(&NewTrack {
            artist: Some("rekordbox".into()),
            file_size: 5,
            ..NewTrack::new("Techno 1", "/Music/Techno1.wav")
        })
        .unwrap();
        ed.save(&pdb_path).unwrap();
        fs::create_dir_all(root.join("Music")).unwrap();
        fs::write(root.join("Music/Techno1.wav"), b"audio").unwrap();
    }
    match layout {
        0 => common::add_onelibrary(&root),
        2 => common::onelibrary_only(&root),
        _ => {}
    }
    let image = "PIONEER/Artwork/00001/b900.jpg";
    if art > 0 && has_plus {
        fs::create_dir_all(root.join("PIONEER/Artwork/00001")).unwrap();
        fs::write(root.join(image), b"jpeg").unwrap();
        let mut one = OneLibrary::open_root(&root).unwrap();
        let id = i64::from(one.add_image(&format!("/{image}")).unwrap());
        let house = one.library().unwrap().tracks.iter().find(|t| t.title == "House 1").unwrap().id;
        let conn = one.connection();
        conn.execute("UPDATE content SET image_id = ?1 WHERE content_id = ?2", [id, i64::from(house)]).unwrap();
        if art == 2 {
            conn.execute(
                "UPDATE playlist SET image_id = ?1 WHERE playlist_id = (SELECT min(playlist_id) FROM playlist)",
                [id],
            )
            .unwrap();
        }
        fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    }
    // rekordbox syncs playlists to the drive automatically: its next sync may undo the clean.
    let synced = outside;
    if synced {
        let key = if has_pdb { paths::PLAYLIST_SYNC } else { paths::PLAYLIST_SYNC_PLUS };
        fs::write(
            root.join(key),
            r#"<Sync DBID="7" AutomaticSync="1"><Playlists><NODE Id="0"/><NODE Id="1"/></Playlists></Sync>"#,
        )
        .unwrap();
    }
    let tracks_before = titles(&root);
    if fault == 4 && has_plus {
        fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
    }

    let plan = clean::plan(&Device::open_export(&root).unwrap(), &Ctx::silent());
    let plan = match plan {
        Err(e) => {
            // OneLibrary alone, and unreadable: nothing to clean it with.
            assert!(fault == 4 && !has_pdb, "{e}");
            assert!(
                matches!(e, Error::OneLibrary(_) | Error::Io { .. }) || e.to_string().contains("exportLibrary.db"),
                "{e}"
            );
            return;
        }
        Ok(p) => p,
    };
    let unreadable_plus = fault == 4 && has_plus;
    let removed = samples + usize::from(outside);
    assert_eq!(plan.tracks.len(), removed, "{:?}", plan.tracks);
    let outside_warning =
        plan.warnings.iter().any(|w| w.contains("outside Contents/ are left on the drive (e.g. /Music/Techno1.wav)"));
    assert_eq!(outside_warning, outside, "{:?}", plan.warnings);
    let sync_warning = plan
        .warnings
        .iter()
        .any(|w| w.starts_with("rekordbox syncs 2 playlist(s) to this drive automatically (from collection 7); "));
    assert_eq!(sync_warning, synced, "{:?}", plan.warnings);
    assert_eq!(plan.warnings.iter().any(|w| w.contains(". Only export.pdb will be changed")), unreadable_plus);
    assert!(plan.files.iter().all(|f| f.starts_with("Contents/") || f.starts_with("PIONEER/")), "{:?}", plan.files);
    // An image goes with the sample only when nothing else in OneLibrary shows it.
    let image_goes = art == 1 && has_plus && !unreadable_plus;
    assert_eq!(plan.files.iter().any(|f| f == image), image_goes, "{:?}", plan.files);

    // Exported again: the planned samples' rows are gone, another in their place.
    let stale = match fault {
        1 if has_pdb => {
            let first = common::library(&root).tracks.iter().find(|t| t.title == "House 1").unwrap().id;
            let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
            ed.remove_tracks(&[first]).unwrap();
            ed.save(&pdb_path).unwrap();
            common::export_new_track(&root, "rekordbox", "Breaks 9");
            true
        }
        2 if has_plus => {
            let mut one = OneLibrary::open_root(&root).unwrap();
            let first = one.library().unwrap().tracks.iter().find(|t| t.title == "House 1").unwrap().id;
            one.remove_tracks(&[first]).unwrap();
            fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
            true
        }
        3 if has_pdb => {
            fs::write(root.join(paths::EXPORT_EXT_PDB), b"not a database").unwrap();
            false
        }
        _ => false,
    };
    let before = Before::take(&root);
    let warnings = Warnings::default();
    let mut d = Device::open_export(&root).unwrap();
    match clean::apply(&plan, &mut d, &warnings.ctx()) {
        Err(e) => {
            assert!(stale, "{e}");
            assert_eq!(e.code(), "stale_plan", "{e}");
            before.untouched(&root).unwrap();
            return;
        }
        Ok(report) => {
            assert!(!stale, "applied to a drive that changed");
            assert_eq!(report.tracks_removed, removed);
        }
    }
    let ext_warning = warnings.said().iter().any(|w| w.starts_with("My Tags in exportExt.pdb weren't updated: "));
    assert_eq!(ext_warning, fault == 3 && has_pdb, "{:?}", warnings.said());
    if outside {
        assert_eq!(fs::read(root.join("Music/Techno1.wav")).unwrap(), b"audio", "never deleted");
    }
    if art > 0 && has_plus {
        assert_eq!(root.join(image).exists(), !image_goes);
    }
    let sample = |t: &String| t.starts_with("House ") || t == "Techno 1";
    for (n, (before, after)) in tracks_before.iter().zip(titles(&root)).enumerate() {
        let left_alone = unreadable_plus && n == 1;
        let want: Vec<String> = before.iter().filter(|t| left_alone || !sample(t)).cloned().collect();
        assert_eq!(after, want, "database {n}");
    }
    if unreadable_plus {
        assert_eq!(fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap(), b"not a database");
    }
    if layout == 2 {
        assert!(!pdb_path.exists(), "no export.pdb appears");
    }
}
