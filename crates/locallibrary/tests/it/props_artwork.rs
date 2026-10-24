//! Cover art against a model: random batches of art set on a drive's tracks (in both databases
//! when it has both), art found beside the audio and in it, and art embedded in the computer's
//! files and taken back out.

use crate::common::{self, media};
use crate::drawn;

use indexmap::IndexMap;
use rustc_hash::FxHashMap;
use std::path::PathBuf;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::artwork::{self, Artwork};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::{Library, Store, paths};

/// The picture of shade `n`, small so encoding stays quick.
fn picture(n: u8) -> Vec<u8> {
    media::png(24, 24, n.wrapping_mul(40))
}

fn art(n: u8) -> Artwork {
    Artwork::from_bytes(&picture(n)).unwrap()
}

/// What players show for a track: the medium image read from the drive.
fn shown(d: &Device, lib: &Library, id: u32) -> Option<Vec<u8>> {
    artwork::read_from_drive(d, lib, id).unwrap()
}

/// Each batch of art set leaves every track of it showing that picture and every other track
/// what it showed before. A batch that is exactly the tracks of one picture replaces its files
/// in place; any other adds a row, so tracks that shared the old picture keep it.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn art_set_on_tracks_shows_on_exactly_those(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let plus = tc.draw(gs::booleans());
    if plus {
        common::add_onelibrary(&root);
    }
    let mut d = Device::open_export(&root).unwrap();
    let ids: Vec<u32> = d.library().unwrap().tracks.iter().map(|t| t.id).collect();
    // The model: each track's picture (by shade), and which tracks share each row.
    let mut shows: FxHashMap<u32, u8> = FxHashMap::default();
    let mut row: FxHashMap<u32, usize> = FxHashMap::default();
    let mut rows = 0;
    let mut medium: IndexMap<u8, Vec<u8>> = IndexMap::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(2).max_value(4)) {
        let mut batch: Vec<u32> = tc.draw(gs::subsequences(&ids).min_size(1));
        // Often exactly the tracks of one picture, which is then replaced in place.
        if !row.is_empty() && tc.draw(gs::weighted_booleans(0.5)) {
            let mut existing: Vec<usize> = row.values().copied().collect();
            existing.sort_unstable();
            existing.dedup();
            let r = tc.draw(gs::sampled_from(existing));
            let users: Vec<u32> = ids.iter().copied().filter(|t| row.get(t) == Some(&r)).collect();
            if !users.is_empty() {
                batch = users;
            }
        }
        let shade = tc.draw(gs::integers::<u8>().max_value(5));
        medium.entry(shade).or_insert_with(|| art(shade).jpeg(artwork::MEDIUM_SIZE).unwrap());
        let databases = |d: &Device| {
            let mut libs = vec![d.library().unwrap()];
            libs.extend(plus.then(|| common::plus_library(&root)));
            libs
        };
        let before = databases(&d);
        media::set_artwork(&mut d, &batch, &picture(shade), &Ctx::silent()).unwrap();

        let shared = row.get(&batch[0]).copied();
        let in_place = shared.is_some_and(|s| {
            let users: Vec<u32> = ids.iter().copied().filter(|t| row.get(t) == Some(&s)).collect();
            batch.iter().all(|t| row.get(t) == Some(&s)) && users.len() == batch.len()
        });
        // In each database the batch keeps its row in place, or shares a new one.
        let files: Vec<String> = batch.iter().map(|t| before[0].track(*t).unwrap().file_path.clone()).collect();
        for (was, now) in before.iter().zip(databases(&d)) {
            let rows = |lib: &Library| -> Vec<Option<u32>> {
                files.iter().map(|f| lib.tracks.iter().find(|t| t.file_path == *f).unwrap().artwork_id).collect()
            };
            let (old, new) = (rows(was), rows(&now));
            match in_place {
                true => assert_eq!(new, old, "{batch:?} {row:?}"),
                false => assert!(
                    new.iter().all(|r| r.is_some() && *r == new[0])
                        && !was.tracks.iter().any(|t| t.artwork_id == new[0]),
                    "{batch:?} {row:?}: {old:?} -> {new:?}"
                ),
            }
        }
        if !in_place {
            rows += 1;
            for t in &batch {
                row.insert(*t, rows);
            }
        }
        for t in &batch {
            shows.insert(*t, shade);
        }

        let lib = d.library().unwrap();
        for t in &lib.tracks {
            assert_eq!(shown(&d, &lib, t.id).as_ref(), shows.get(&t.id).map(|s| &medium[s]), "track {}", t.id);
        }
        let missing: Vec<u32> = artwork::missing(&d, &lib).iter().map(|t| t.id).collect();
        assert_eq!(missing, ids.iter().copied().filter(|t| !shows.contains_key(t)).collect::<Vec<_>>());
        // Tracks share a row exactly when the model says they do.
        for a in &lib.tracks {
            for b in &lib.tracks {
                let same = a.artwork_id.is_some() && a.artwork_id == b.artwork_id;
                assert_eq!(same, row.contains_key(&a.id) && row.get(&a.id) == row.get(&b.id));
            }
        }
        if plus {
            // OneLibrary players see the same picture under OneLibrary's own image.
            let one = common::plus_library(&root);
            for t in &one.tracks {
                let pdb_track = lib.tracks.iter().find(|x| x.file_path == t.file_path).unwrap();
                assert_eq!(shown(&d, &one, t.id), shown(&d, &lib, pdb_track.id));
            }
        }
    }
}

/// `art auto` finds the art of tracks that have none, in their audio file or a cover image in
/// its folder; tracks whose source is the same picture share one row. With a OneLibrary that
/// can't be read, the art goes into export.pdb only, and that's said.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn art_found_beside_the_audio_goes_on_exactly_the_tracks_without(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let mut folders: Vec<PathBuf> = lib
        .tracks
        .iter()
        .map(|t| paths::to_fs(&root, &paths::key(&t.file_path)).parent().unwrap().to_path_buf())
        .collect();
    folders.sort();
    folders.dedup();
    // Some folders get a cover image (a shade each, two may share one), and some tracks art first.
    let mut cover: FxHashMap<PathBuf, u8> = FxHashMap::default();
    for f in drawn::some(&tc, &folders) {
        let shade = tc.draw(gs::integers::<u8>().max_value(1));
        let name = tc.draw(gs::sampled_from(&["cover.png", "Folder.PNG", "front.jpg"][..]));
        let bytes = if name.ends_with("jpg") { art(shade).jpeg(64).unwrap() } else { media::png(24, 24, shade) };
        std::fs::write(f.join(name), bytes).unwrap();
        cover.insert(f, shade);
    }
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    let had: Vec<u32> = drawn::some(&tc, &ids).into_iter().take(3).collect();
    if !had.is_empty() {
        media::set_artwork(&mut d, &had, &picture(9), &Ctx::silent()).unwrap();
    }
    let lib = d.library().unwrap();
    let folder_of = |id: u32| {
        let t = lib.track(id).unwrap();
        paths::to_fs(&root, &paths::key(&t.file_path)).parent().unwrap().to_path_buf()
    };

    let plan = artwork::plan_auto(&d, &lib);
    let found: Vec<u32> = plan.found.iter().map(|i| i.track.id).collect();
    let want: Vec<u32> =
        ids.iter().copied().filter(|t| !had.contains(t) && cover.contains_key(&folder_of(*t))).collect();
    assert_eq!(found, want);
    assert_eq!(plan.found.len() + plan.not_found.len() + had.len(), ids.len());

    // A OneLibrary that can't be read is left alone, and the art goes into export.pdb, saying so.
    let broken_plus = tc.draw(gs::booleans());
    if broken_plus {
        common::add_onelibrary(&root);
        std::fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
        d = Device::open_export(&root).unwrap();
    }
    let warnings = common::Warnings::default();
    let r = artwork::apply_auto(&mut d, &plan, &warnings.ctx()).unwrap();
    assert_eq!(r.tracks_updated, want.len());
    let only_pdb = warnings.said().iter().any(|w| w.contains("Only export.pdb will be changed"));
    assert_eq!(only_pdb, broken_plus && !want.is_empty(), "{:?}", warnings.said());
    let after = d.library().unwrap();
    let missing: Vec<u32> = artwork::missing(&d, &after).iter().map(|t| t.id).collect();
    assert_eq!(missing, plan.not_found.iter().map(|t| t.id).collect::<Vec<_>>());
    // The same source file, the same row; a track that had art keeps it.
    for a in &want {
        for b in &want {
            let same_file = folder_of(*a) == folder_of(*b);
            if same_file {
                assert_eq!(after.track(*a).unwrap().artwork_id, after.track(*b).unwrap().artwork_id);
            }
        }
    }
    for t in &had {
        assert_eq!(after.track(*t).unwrap().artwork_id, lib.track(*t).unwrap().artwork_id);
    }
    // Run again, there's nothing left to find.
    let again = artwork::plan_auto(&d, &after);
    assert!(again.found.is_empty(), "{:?}", again.found);
}

/// Art embedded in the computer's audio files comes back out as it went in, and taking it back
/// puts back every file not changed since, byte for byte; only the latest embeds are kept.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn embedded_art_is_taken_back_exactly(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let image = dir.path().join("cover.png");
    let kinds = [("wav", media::wav()), ("mp3", media::mp3()), ("txt.mp3", b"not audio".to_vec())];
    let mut files = Vec::new();
    let mut originals = FxHashMap::default();
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let (ext, bytes) = tc.draw(gs::sampled_from(kinds.to_vec()));
        let p = dir.path().join(format!("{i}.{ext}"));
        std::fs::write(&p, &bytes).unwrap();
        originals.insert(p.clone(), bytes);
        files.push(p);
    }
    let embeds = tc.draw(gs::integers::<usize>().min_value(1).max_value(7));
    let mut ids = Vec::new();
    for n in 0..embeds {
        let a = art(n as u8);
        let r = artwork::embed_with_backup(&store, &files, &a, &image, &Ctx::silent()).unwrap();
        let takes: Vec<PathBuf> = files.iter().filter(|f| originals[*f] != b"not audio").cloned().collect();
        assert_eq!(r.embedded, takes);
        assert_eq!(r.failed.len(), files.len() - takes.len());
        for f in &takes {
            let pic = artwork::read_embedded(f).unwrap().expect("embedded");
            assert_eq!(image::load_from_memory(&pic).unwrap().width(), 24u32.max(artwork::THUMB_SIZE));
        }
        ids.extend(r.backup);
    }
    let kept: Vec<String> = artwork::embed_backups(&store).unwrap().into_iter().map(|b| b.id).collect();
    assert_eq!(kept, ids.iter().rev().take(5).cloned().collect::<Vec<_>>());
    assert_eq!(artwork::embed_backup_bytes(&store) > 0, !kept.is_empty());

    // One file changes again; undoing every embed kept, newest first, puts the others back.
    let touched = files.iter().find(|f| tc.draw(gs::booleans()) && originals[*f] != b"not audio").cloned();
    if let Some(f) = &touched {
        std::fs::write(f, b"changed since").unwrap();
    }
    for id in &kept {
        let r = artwork::restore_embedded(&store, Some(id)).unwrap();
        assert!(r.restored.iter().all(|p| Some(p) != touched.as_ref()));
    }
    for f in &files {
        let now = std::fs::read(f).unwrap();
        match (Some(f) == touched.as_ref(), kept.len() == ids.len()) {
            (true, _) => assert_eq!(now, b"changed since"),
            // Every embed was kept, so the first one's copies (the originals) went back.
            (false, true) => assert_eq!(now, originals[f], "{}", f.display()),
            (false, false) => {}
        }
    }
}

/// Image keys follow rekordbox's layout of 20 per folder, the medium copy sits beside each,
/// and a free key is never one already taken.
#[hegel::test(test_cases = 100)]
fn image_keys_follow_rekordboxs_layout(tc: TestCase) {
    let id = tc.draw(gs::integers::<u32>().min_value(1).max_value(5000));
    let prefix = tc.draw(gs::sampled_from(vec!['a', 'b']));
    let ext = tc.draw(gs::sampled_from(vec!["jpg", "png"]));
    let key = artwork::image_key(prefix, id, ext);
    assert_eq!(key, format!("PIONEER/Artwork/{:05}/{prefix}{id}.{ext}", (id - 1) / 20 + 1));
    let medium = artwork::medium_variant(&key);
    assert_eq!(medium, format!("{}_m.{ext}", key.strip_suffix(&format!(".{ext}")).unwrap()));
    assert_eq!(paths::classify(&key), Some(paths::FileClass::Artwork));

    let dir = tempfile::tempdir().unwrap();
    let taken = tc.draw(gs::integers::<u32>().max_value(2));
    for folder in 0..taken {
        let k = format!("PIONEER/Artwork/{:05}/{prefix}{id}.{ext}", (id - 1) / 20 + 1 + folder);
        let which = if tc.draw(gs::booleans()) { k } else { artwork::medium_variant(&k) };
        let p = paths::to_fs(dir.path(), &which);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"taken").unwrap();
    }
    let free = artwork::free_image_key(dir.path(), prefix, id, ext);
    assert_eq!(free, format!("PIONEER/Artwork/{:05}/{prefix}{id}.{ext}", (id - 1) / 20 + 1 + taken));
    assert!(!paths::to_fs(dir.path(), &free).exists());
}
