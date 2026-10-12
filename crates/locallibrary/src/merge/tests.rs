use super::*;

#[test]
fn unique_keys() {
    let mut taken: FxHashSet<String> = ["contents/a/x.mp3".to_string()].into_iter().collect();
    let nothing = |_: &str| false;
    assert_eq!(unique_key("Contents/A/x.mp3", &mut taken, &nothing), "Contents/A/x (2).mp3");
    assert_eq!(unique_key("Contents/A/x.mp3", &mut taken, &nothing), "Contents/A/x (3).mp3");
    assert_eq!(unique_key("Contents/B/y.mp3", &mut taken, &nothing), "Contents/B/y.mp3");
    // A file already on the drive that the library doesn't list (the DJ's own) is taken too.
    let mine = |k: &str| k == "Music/z.mp3";
    assert_eq!(unique_key("Music/z.mp3", &mut taken, &mine), "Music/z (2).mp3");
}

#[test]
fn unique_keys_without_an_extension_are_numbered_at_the_end() {
    let mut taken: FxHashSet<String> =
        ["contents/a/notes".to_string(), "contents/a.b/c".to_string()].into_iter().collect();
    let nothing = |_: &str| false;
    assert_eq!(unique_key("Contents/A/notes", &mut taken, &nothing), "Contents/A/notes (2)");
    // A dot in a folder's name isn't an extension.
    assert_eq!(unique_key("Contents/A.b/c", &mut taken, &nothing), "Contents/A.b/c (2)");
}
