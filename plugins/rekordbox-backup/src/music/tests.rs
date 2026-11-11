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
