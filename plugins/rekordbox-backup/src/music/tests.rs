use std::time::Duration;

use loclib_progress::CancelToken;

use super::*;
use crate::tests::fixture;

/// The live tracks with a file, in order; a missing file said, a streaming track not.
#[test]
fn copies_are_the_tracks_with_a_file() {
    let f = fixture();
    let (copies, left_out) = copies(&f.tracks);
    let names: Vec<(&str, &str)> = copies.iter().map(|c| (c.hex.as_str(), c.name.as_str())).collect();
    assert_eq!(
        names,
        [
            ("026EE51D", "/OliverTree/UnknownAlbum/026EE51D_Lizard (Once).mp3"),
            ("00000002", "/Łukasz&Mø💀/Über_Alles_/00000002_A file name far longer than thirty-.flac"),
            ("00000006", "/UnknownArtist/UnknownAlbum/00000006_six.wav"),
        ]
    );
    let gone = f.path("music/gone.mp3");
    assert_eq!(left_out, [format!("track 4: {} isn't there, so its music isn't copied", gone.display())]);
}

/// Copied with its time; copied again only once it changed.
#[test]
fn a_copy_there_already_is_kept() {
    let f = fixture();
    let (copies, _) = copies(&f.tracks);
    let root = f.path("backups/rekordbox_bak");
    let ctx = Ctx::silent();
    assert_eq!(copy_all(&copies, &root, &ctx).unwrap(), Copied { copied: 3, kept: 0, failed: Vec::new() });
    for c in &copies {
        assert!(c.there(&c.to(&root)), "{}", c.name);
    }
    assert_eq!(copy_all(&copies, &root, &ctx).unwrap(), Copied { copied: 0, kept: 3, failed: Vec::new() });
    // Touched since: copied again, back to the source's time.
    let lizard = copies[0].to(&root);
    let later = SystemTime::now() + Duration::from_secs(60);
    File::options().write(true).open(&lizard).unwrap().set_modified(later).unwrap();
    assert!(!copies[0].there(&lizard));
    assert_eq!(copy_all(&copies, &root, &ctx).unwrap(), Copied { copied: 1, kept: 2, failed: Vec::new() });
    assert!(copies[0].there(&lizard));
    // A time a drive keeps coarser (FAT's two seconds) is the same time.
    let modified = copies[0].modified.unwrap();
    let rounded = modified - Duration::from_millis(1500);
    File::options().write(true).open(&lizard).unwrap().set_modified(rounded).unwrap();
    assert!(copies[0].there(&lizard));
    File::options().write(true).open(&lizard).unwrap().set_modified(modified - Duration::from_secs(3)).unwrap();
    assert!(!copies[0].there(&lizard));
}

/// A copy that fails is said and skipped; the rest are made.
#[test]
fn a_failed_copy_is_skipped() {
    let f = fixture();
    let (copies, _) = copies(&f.tracks);
    std::fs::remove_file(&copies[1].from).unwrap();
    let root = f.path("backups/rekordbox_bak");
    let done = copy_all(&copies, &root, &Ctx::silent()).unwrap();
    assert_eq!((done.copied, done.kept), (2, 0));
    let [(hex, why)] = &done.failed[..] else { panic!("{done:?}") };
    assert_eq!(hex, "00000002");
    assert!(why.starts_with(&copies[1].from.display().to_string()), "{why}");
    assert!(!copies[1].to(&root).exists());
    assert!(copies[0].there(&copies[0].to(&root)) && copies[2].there(&copies[2].to(&root)));
}

/// A track whose id isn't a number has no name in the music folder, so it's said and not copied.
#[test]
fn a_track_whose_id_isnt_a_number_isnt_copied() {
    let f = fixture();
    let path = f.path("music/six.wav").to_string_lossy().into_owned();
    let tracks = [CollectionTrack { id: "abc".into(), path, ..CollectionTrack::default() }];
    let (copies, left_out) = copies(&tracks);
    assert!(copies.is_empty());
    assert_eq!(left_out, ["track abc: its id isn't a number, so its music isn't copied"]);
}

/// The names in `dir` its staged copies have.
fn staged(dir: &Path) -> Vec<String> {
    let names = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned());
    names.filter(|n| n.starts_with(".rekordbox_bak_")).collect()
}

/// Copies that can't be put in place are each said, naming what stopped them, and leave nothing
/// staged: a file where a folder goes, a folder where the copy goes, a source that's a folder.
#[test]
fn copies_that_cant_be_made_are_said() {
    let f = fixture();
    let (mut copies, _) = copies(&f.tracks);
    let root = f.path("backups/rekordbox_bak");
    crate::tests::put(&root, "OliverTree", b"in the way");
    let taken = copies[1].to(&root);
    crate::tests::put(&taken, "inside", b"mine");
    copies[2].from = f.path("music/TikTok");
    let done = copy_all(&copies, &root, &Ctx::silent()).unwrap();
    assert_eq!((done.copied, done.kept), (0, 0));
    let first = root.join("OliverTree/UnknownAlbum");
    let said = [("026EE51D", first.as_path()), ("00000002", taken.as_path()), ("00000006", copies[2].from.as_path())];
    assert_eq!(done.failed.len(), said.len(), "{done:?}");
    for ((hex, why), (want, path)) in done.failed.iter().zip(said) {
        assert_eq!(hex, want);
        assert!(why.starts_with(&path.display().to_string()), "{why}");
    }
    assert_eq!(std::fs::read(taken.join("inside")).unwrap(), b"mine");
    assert!(staged(taken.parent().unwrap()).is_empty());
    assert!(staged(&root.join("UnknownArtist/UnknownAlbum")).is_empty());
}

/// A copy whose source has no time it could read is made again each time: nothing says the one
/// there is the same.
#[test]
fn a_copy_with_no_time_is_made_each_time() {
    let f = fixture();
    let (copies, _) = copies(&f.tracks);
    let untimed = [Copy { modified: None, ..copies[2].clone() }];
    let root = f.path("backups/rekordbox_bak");
    let once = Copied { copied: 1, kept: 0, failed: Vec::new() };
    assert_eq!(copy_all(&untimed, &root, &Ctx::silent()).unwrap(), once);
    assert_eq!(copy_all(&untimed, &root, &Ctx::silent()).unwrap(), once);
    assert_eq!(std::fs::read(untimed[0].to(&root)).unwrap(), b"six audio");
}

/// Cancelled while a file is copied: the copies before it stay, and none of it is left.
#[test]
fn cancelled_during_a_copy_leaves_no_part_of_it() {
    let f = fixture();
    let (copies, _) = copies(&f.tracks);
    let root = f.path("backups/rekordbox_bak");
    let cancel = CancelToken::new();
    let progress = |e: &Event| {
        if let Event::Item { path, .. } = e
            && *path == copies[1].name
        {
            cancel.cancel();
        }
    };
    let e = copy_all(&copies, &root, &Ctx::new(&progress, cancel.clone())).unwrap_err();
    assert_eq!(e.kind(), Some("cancelled"));
    assert!(copies[0].there(&copies[0].to(&root)));
    let second = copies[1].to(&root);
    assert!(!second.exists());
    assert!(staged(second.parent().unwrap()).is_empty());
}

#[test]
fn a_cancelled_copy_leaves_no_part_of_a_file() {
    let f = fixture();
    let (copies, _) = copies(&f.tracks);
    let root = f.path("backups/rekordbox_bak");
    let cancel = CancelToken::new();
    cancel.cancel();
    let e = copy_all(&copies, &root, &Ctx::new(&loclib_progress::NoProgress, cancel)).unwrap_err();
    assert_eq!(e.kind(), Some("cancelled"));
    assert!(!root.exists());
}
