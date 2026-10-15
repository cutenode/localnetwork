//! Packing brings the images OneLibrary's playlists use (not only its tracks'), and a pack too big
//! for the drive says so before anything is written.

use crate::common::{self, media};

use std::fs;
use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::Edit;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::pack::{self, PackOptions};
use locallibrary::progress::Ctx;
use locallibrary::{artwork, paths};

/// The twelve-track export with a playlist "Set" of its first `n` tracks, the first with artwork.
fn source(dir: &Path, n: usize) -> PathBuf {
    let root = common::bigger_drive(dir, "Main");
    let lib = common::library(&root);
    let files: Vec<String> = lib.tracks.iter().take(n).map(|t| t.file_path.clone()).collect();
    common::apply(&root, &[Edit::SetEntries { playlist: "Set".into(), paths: files, create: true }]);
    media::set_art(&root, &[lib.tracks[0].id]);
    root
}

/// A pack brings every image OneLibrary's playlists use, with its smaller variant when the source
/// has one, whichever playlists it packs; its plan is never a no-op (the databases are rewritten).
/// One whose files outgrow the drive says how much it needs and how much is free.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_pack_brings_what_its_lists_use_and_says_when_it_wont_fit(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let root = source(dir.path(), n);
    common::add_onelibrary(&root);
    // Images on OneLibrary playlists: some drawn number, each perhaps with its smaller variant.
    let mut one = OneLibrary::open_root(&root).unwrap();
    let art =
        fs::read(paths::to_fs(&root, &paths::key(&common::library(&root).tracks[0].artwork_path.clone().unwrap())))
            .unwrap();
    let mut images = Vec::new();
    for k in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let key = format!("PIONEER/Artwork/00099/b{}.jpg", 900 + k);
        let medium = tc.draw(gs::booleans());
        for (path, wanted) in [(key.clone(), true), (artwork::medium_variant(&key), medium)] {
            if wanted {
                let p = paths::to_fs(&root, &path);
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(p, &art).unwrap();
            }
        }
        let id = one.add_image(&paths::device_path(&key)).unwrap();
        let list = one.create_playlist(&format!("Pictured {k}"), 0, false).unwrap();
        one.connection().execute("UPDATE playlist SET image_id = ?1 WHERE playlist_id = ?2", [id, list]).unwrap();
        images.push((key, medium));
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    let src = Device::open_export(&root).unwrap();
    let club = Device::open(common::blank(dir.path(), "Club")).unwrap();
    let set = PackOptions { playlists: vec!["Set".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &set, &Ctx::silent()).unwrap();
    assert!(!plan.is_noop());
    let copies: Vec<&str> = plan.copies.iter().map(|c| c.path.as_str()).collect();
    for (key, medium) in &images {
        assert!(copies.contains(&key.as_str()), "{key} in {copies:?}");
        assert_eq!(copies.contains(&artwork::medium_variant(key).as_str()), *medium, "{key}'s smaller variant");
    }
    assert!(plan.fits, "{:?}", plan.warnings);
    let mut club = Device::open(club.root()).unwrap();
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    for (key, _) in &images {
        assert_eq!(fs::read(paths::to_fs(club.root(), key)).unwrap(), art, "{key} came along");
    }

    // Sparse only on Unix: NTFS would have to find the room for it.
    #[cfg(unix)]
    {
        // A track of the set grows past the drive's free space (a sparse file: it takes no room).
        let lib = common::library(&root);
        let big =
            paths::to_fs(&root, &paths::key(&lib.tracks[tc.draw(gs::integers::<usize>().max_value(n - 1))].file_path));
        let free = locallibrary::fsutil::available_space(&root).unwrap();
        fs::File::options().write(true).open(&big).unwrap().set_len(2 * free + (1 << 30)).unwrap();
        let fresh = Device::open(common::blank(dir.path(), "Fresh")).unwrap();
        let plan = pack::plan(&Device::open_export(&root).unwrap(), &fresh, &set, &Ctx::silent()).unwrap();
        assert!(!plan.fits);
        let said = format!(
            "the pack needs about {} bytes (its files, copies, databases and backup) but only {} are free on Fresh",
            plan.needed_bytes,
            plan.free_space.unwrap()
        );
        assert!(plan.warnings.contains(&said), "{said:?} in {:?}", plan.warnings);
        assert!(plan.needed_bytes > 2 * free, "{} for a file of {} bytes", plan.needed_bytes, 2 * free);
    }
}
