//! Art embedded in the computer's audio files and taken back out: what's kept to take it back,
//! for how long, and what happens to the files that can't take art or have gone since.

use crate::common::media::{png, wav};

use std::path::{Path, PathBuf};

use locallibrary::api;
use locallibrary::artwork::{self, Artwork};
use locallibrary::progress::Ctx;
use locallibrary::{Error, Store};

fn art() -> Artwork {
    Artwork::from_bytes(&png(200, 200, 7)).unwrap()
}

/// A store, and `names` written in the folder next to it (`.wav` ones as audio).
fn setup(dir: &Path, names: &[&str]) -> (Store, Vec<PathBuf>) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let files = names
        .iter()
        .map(|n| {
            let p = dir.join(n);
            std::fs::write(&p, if n.ends_with(".wav") { wav() } else { b"not audio".to_vec() }).unwrap();
            p
        })
        .collect();
    (store, files)
}

/// A file that can't take art is reported and the others go on; a run where none could keeps
/// no backup at all.
#[test]
fn files_that_cant_take_art_are_reported_and_leave_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let (store, files) = setup(dir.path(), &["a.wav", "notes.mp3"]);
    let image = dir.path().join("cover.png");
    let r = artwork::embed_with_backup(&store, &files, &art(), &image, &Ctx::silent()).unwrap();
    assert_eq!(r.embedded, [files[0].clone()]);
    assert_eq!(r.failed.len(), 1);
    let kept = artwork::embed_backups(&store).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].files.iter().map(|f| &f.path).collect::<Vec<_>>(), [&files[0]]);
    assert_eq!(std::fs::read(&files[1]).unwrap(), b"not audio");

    let r = artwork::embed_with_backup(&store, &files[1..], &art(), &image, &Ctx::silent()).unwrap();
    assert!(r.embedded.is_empty() && r.backup.is_none());
    assert_eq!(artwork::embed_backups(&store).unwrap().len(), 1, "no backup of nothing");
}

/// Only the latest five embeds are kept: each holds whole copies of the files it rewrote.
#[test]
fn only_the_latest_embeds_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (store, files) = setup(dir.path(), &["a.wav"]);
    let image = dir.path().join("cover.png");
    let ids: Vec<String> = (0..7)
        .map(|_| artwork::embed_with_backup(&store, &files, &art(), &image, &Ctx::silent()).unwrap().backup.unwrap())
        .collect();
    let kept: Vec<String> = artwork::embed_backups(&store).unwrap().into_iter().map(|b| b.id).collect();
    let newest: Vec<String> = ids.iter().rev().take(5).cloned().collect();
    assert_eq!(kept, newest);
}

/// Taking art back out needs an embed to take back; a file gone since is left out, and its
/// backup kept.
#[test]
fn taking_art_back_out_skips_files_gone_since() {
    let dir = tempfile::tempdir().unwrap();
    let (store, files) = setup(dir.path(), &["a.wav", "b.wav"]);
    assert!(matches!(artwork::restore_embedded(&store, None), Err(Error::NotFound(_))));
    let image = dir.path().join("cover.png");
    let id = artwork::embed_with_backup(&store, &files, &art(), &image, &Ctx::silent()).unwrap().backup.unwrap();
    assert!(matches!(artwork::restore_embedded(&store, Some("nope")), Err(Error::NotFound(_))));
    std::fs::remove_file(&files[1]).unwrap();
    let r = artwork::restore_embedded(&store, Some(&id)).unwrap();
    assert_eq!(r.restored, [files[0].clone()]);
    assert_eq!(r.skipped.iter().map(|s| s.reason.as_str()).collect::<Vec<_>>(), ["gone"]);
    assert_eq!(std::fs::read(&files[0]).unwrap(), wav());
    assert_eq!(artwork::embed_backups(&store).unwrap()[0].id, id, "kept for the file that's gone");
}

/// Embedding leaves out what isn't a file it can rewrite, and taking it back out leaves a file
/// that's gone since; with nothing embedded, or an embed that isn't there, it's refused.
#[test]
fn embedding_skips_what_it_cant_rewrite_and_unembedding_what_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let image = dir.path().join("cover.png");
    std::fs::write(&image, png(300, 300, 40)).unwrap();
    let unembed = |backup: Option<&str>| {
        api::artwork_unembed_plan(&store, &api::UnembedRequest { backup: backup.map(Into::into) })
    };
    assert!(matches!(unembed(None), Err(Error::NotFound(m)) if m.contains("no embedded art")));

    let track = dir.path().join("track.wav");
    std::fs::write(&track, wav()).unwrap();
    let nowhere = dir.path().join("nowhere.mp3");
    let files = vec![track.clone(), dir.path().to_path_buf(), nowhere.clone()];
    let req = api::EmbedArtwork { files, image };
    let plan = api::artwork_embed_plan(&req).unwrap();
    assert_eq!(plan.files.iter().map(|f| &f.path).collect::<Vec<_>>(), [&track]);
    let skipped: Vec<(&str, &str)> = plan.skipped.iter().map(|s| (s.path.as_str(), s.reason.as_str())).collect();
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    assert_eq!(skipped[0], (dir.path().display().to_string().as_str(), "not a file"));
    assert_eq!(skipped[1].0, nowhere.display().to_string(), "it isn't there");

    let report = api::artwork_embed(&store, &req).unwrap();
    assert_eq!(report.failed.len(), 2, "the skipped files are reported as not done: {:?}", report.failed);
    let backup = report.backup.expect("kept");
    assert!(matches!(unembed(Some("nope")), Err(Error::NotFound(_))));
    std::fs::remove_file(&track).unwrap();
    let plan = unembed(Some(&backup)).unwrap();
    assert!(plan.restored.is_empty());
    assert_eq!(plan.skipped.iter().map(|s| s.reason.as_str()).collect::<Vec<_>>(), ["gone"]);
}
