use super::*;
use crate::mix::index::Prints;
use crate::sound::Analyses;

/// Keys and paths stay what fingerprint caches were written with, so users' caches still read,
/// and each kind keeps to its own folder and version.
#[test]
fn caches_keep_their_layout() {
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("Track.MP3");
    fs::write(&audio, b"12345").unwrap();
    let mtime = fs::metadata(&audio).unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap();
    let recipe = format!("2\u{1f}track.mp3\u{1f}5\u{1f}{}", mtime.as_secs());
    let expected = blake3::hash(recipe.as_bytes()).to_hex()[..32].to_string();
    assert_eq!(Cache::<Prints>::key(&audio).as_deref(), Some(expected.as_str()));
    assert_eq!(Cache::<Prints>::key(&dir.path().join("gone.mp3")), None);

    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let prints = Cache::<Prints>::in_store(&store);
    assert_eq!(prints.root(), store.root().join("cache/fingerprints"));
    assert_eq!(prints.file("abcdef", "fp"), prints.root().join("ab/abcdef.fp"));
    let analyses = Cache::<Analyses>::in_store(&store).clone();
    assert_eq!(analyses.root(), store.root().join("cache/analysis"));
}

/// A cached value that can't be read back (cut short) counts as not cached.
#[test]
fn unreadable_values_are_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::<Analyses>::new(dir.path());
    fs::create_dir_all(dir.path().join("ab")).unwrap();
    fs::write(cache.file("abcdef", "json"), b"{\"version\":").unwrap();
    assert!(cache.get("abcdef").is_none());
}
