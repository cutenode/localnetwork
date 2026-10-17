//! Artwork the other properties leave out: embedded art taken back when some of the files were
//! changed or deleted since (those are skipped and the backup kept for them), art set on a drive
//! exported for OneLibrary players only, batches that name no track or one the drive lacks, and
//! art found automatically for tracks whose audio can't be reached or no longer holds it.

use crate::common::{self, media};
use crate::drawn;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::artwork::{self, Artwork};
use locallibrary::device::Device;
use locallibrary::progress::{CancelToken, Ctx, Event, Progress};
use locallibrary::store::Store;
use locallibrary::{Error, paths};
use rekordbox_pdb::NewTrack;

/// Taking embedded art back puts back exactly the files untouched since, skips the rest (said
/// as gone or changed), and keeps the backup while any were skipped.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn art_is_taken_back_only_from_files_untouched_since(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut files = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let p = dir.path().join(format!("{i}.mp3"));
        fs::write(&p, media::mp3()).unwrap();
        files.push(p);
    }
    let art = Artwork::from_bytes(&media::png(24, 24, 3)).unwrap();
    let r = artwork::embed_with_backup(&store, &files, &art, &dir.path().join("cover.png"), &Ctx::silent()).unwrap();
    assert_eq!(r.embedded, files);
    let backup = r.backup.unwrap();
    let (mut gone, mut changed, mut kept) = (BTreeSet::new(), BTreeSet::new(), Vec::new());
    for f in &files {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => kept.push(f.clone()),
            1 => {
                fs::remove_file(f).unwrap();
                gone.insert(f.display().to_string());
            }
            _ => {
                fs::write(f, b"changed since").unwrap();
                changed.insert(f.display().to_string());
            }
        }
    }
    let r = artwork::restore_embedded(&store, Some(&backup)).unwrap();
    assert_eq!(r.restored, kept);
    for f in &kept {
        assert_eq!(fs::read(f).unwrap(), media::mp3(), "{}", f.display());
    }
    let skipped = |why: &str| -> BTreeSet<String> {
        r.skipped.iter().filter(|s| s.reason.contains(why)).map(|s| s.path.clone()).collect()
    };
    assert_eq!(skipped("gone"), gone);
    assert_eq!(skipped("changed"), changed);
    let backups: Vec<String> = artwork::embed_backups(&store).unwrap().into_iter().map(|b| b.id).collect();
    assert_eq!(backups.contains(&backup), !r.skipped.is_empty());
}

/// Art set on a drive with only OneLibrary shows on exactly the tracks chosen; a batch naming no
/// track, or one the drive doesn't have, is refused without writing anything.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn art_on_a_onelibrary_only_drive_shows_on_exactly_those_tracks(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::onelibrary_only(&root);
    let mut d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    let art = media::png(24, 24, 9);

    let before = common::drive_files(&root);
    assert!(matches!(media::set_artwork(&mut d, &[], &art, &Ctx::silent()), Err(Error::Invalid(_))));
    let stranger = ids.iter().max().unwrap() + 1;
    assert!(matches!(media::set_artwork(&mut d, &[ids[0], stranger], &art, &Ctx::silent()), Err(Error::NotFound(_))));
    assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty());

    let mut chosen: Vec<u32> = drawn::some(&tc, &ids);
    if chosen.is_empty() {
        chosen.push(ids[0]);
    }
    let r = media::set_artwork(&mut d, &chosen, &art, &Ctx::silent()).unwrap();
    assert_eq!(r.changes, 1);
    let after = d.library().unwrap();
    for id in &ids {
        let shown = artwork::read_from_drive(&d, &after, *id).unwrap();
        assert_eq!(shown.is_some(), chosen.contains(id), "#{id}");
    }
}

/// Art found automatically: a track whose audio path leaves the drive isn't looked for, and art a
/// file held when planned but not when applied isn't set (nothing is written for it).
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn automatic_art_skips_audio_out_of_reach_or_emptied_since(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let escaped = common::add_track(&root, NewTrack::new("Escaped", "/../outside.mp3"), &media::mp3());
    let lib = common::library(&root);
    let with_art: Vec<u32> =
        drawn::some(&tc, &lib.tracks.iter().map(|t| t.id).filter(|id| *id != escaped).collect::<Vec<_>>());
    let art = Artwork::from_bytes(&media::png(24, 24, 5)).unwrap();
    let mut audio: Vec<PathBuf> = Vec::new();
    for id in &with_art {
        let p = paths::to_fs(&root, &paths::key(&lib.track(*id).unwrap().file_path));
        fs::write(&p, media::mp3()).unwrap();
        artwork::embed_in_file(&p, &art).unwrap();
        audio.push(p);
    }
    let mut d = Device::open_export(&root).unwrap();
    let plan = artwork::plan_auto(&d, &lib);
    assert!(plan.not_found.iter().any(|t| t.id == escaped));
    let found: BTreeSet<u32> = plan.found.iter().map(|i| i.track.id).collect();
    assert_eq!(found, with_art.iter().copied().collect());
    // Each file loses its art before the plan is applied.
    for p in &audio {
        fs::write(p, media::mp3()).unwrap();
    }
    let before = common::drive_files(&root);
    let r = artwork::apply_auto(&mut d, &plan, &Ctx::silent()).unwrap();
    assert_eq!((r.tracks_updated, r.backup), (0, None));
    assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty());
}

/// Cancels once the operation has started on `at` files.
struct StopAfter {
    seen: AtomicUsize,
    at: usize,
    cancel: CancelToken,
}

impl Progress for StopAfter {
    fn on_event(&self, event: &Event) {
        if matches!(event, Event::Item { .. }) && self.seen.fetch_add(1, Ordering::SeqCst) + 1 >= self.at {
            self.cancel.cancel();
        }
    }
}

/// Embedding cancelled part way stops before the next file: the files it finished hold the art,
/// the rest are as they were, and taking it back puts back exactly the ones it changed. Cancelled
/// before any, nothing is kept; cancelled during the last, it's done.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn art_embedded_until_cancelled_is_taken_back_exactly(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let files: Vec<PathBuf> = (0..n).map(|i| dir.path().join(format!("{i}.mp3"))).collect();
    for f in &files {
        fs::write(f, media::mp3()).unwrap();
    }
    // Cancelled once `done` files were started (0: before any).
    let done = tc.draw(gs::integers::<usize>().max_value(n));
    let cancel = CancelToken::new();
    if done == 0 {
        cancel.cancel();
    }
    let stop = StopAfter { seen: AtomicUsize::new(0), at: done, cancel: cancel.clone() };
    let art = Artwork::from_bytes(&media::png(24, 24, 1)).unwrap();
    let image = dir.path().join("cover.png");
    let r = artwork::embed_with_backup(&store, &files, &art, &image, &Ctx::new(&stop, cancel));
    match done == n {
        true => assert_eq!(r.unwrap().embedded, files),
        false => assert!(matches!(r, Err(Error::Cancelled)), "{r:?}"),
    }
    for (i, f) in files.iter().enumerate() {
        assert_eq!(artwork::read_embedded(f).unwrap().is_some(), i < done, "{i} of {done}");
    }
    let backups = artwork::embed_backups(&store).unwrap();
    assert_eq!(backups.len(), usize::from(done > 0));
    if let Some(b) = backups.first() {
        let r = artwork::restore_embedded(&store, Some(&b.id)).unwrap();
        assert_eq!(r.restored, files[..done].to_vec());
        assert!(files.iter().all(|f| fs::read(f).unwrap() == media::mp3()));
    }
}
