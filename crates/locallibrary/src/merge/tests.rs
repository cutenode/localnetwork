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

/// A key for a new file is the one wanted when nothing has it, else the first ` (n)`, n from 2,
/// before the extension (a dot in a folder's name isn't one) that neither the library nor the
/// drive has, ignoring case; it's taken from then on.
#[hegel::test(test_cases = 100)]
fn prop_a_new_files_key_is_the_first_free_one(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let folder = tc.draw(gs::sampled_from(vec!["Contents/A", "Contents/A.b", "Music"]));
    let stem = tc.draw(gs::sampled_from(vec!["x", "Track 1", "Ünï (2)"]));
    // With an extension and without, each case.
    for ext in [tc.draw(gs::sampled_from(vec![".mp3", ".FLAC", ".m4a"])), ""] {
        let key = format!("{folder}/{stem}{ext}");
        let numbered = |n: u32| match n {
            1 => key.clone(),
            n => format!("{folder}/{stem} ({n}){ext}"),
        };
        let mut taken: FxHashSet<String> = FxHashSet::default();
        let mut on_disk: FxHashSet<String> = FxHashSet::default();
        for n in 1..=4 {
            match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => {}
                1 => {
                    taken.insert(fold(&numbered(n)));
                }
                _ => {
                    on_disk.insert(numbered(n).to_uppercase());
                }
            }
        }
        let before = taken.clone();
        let disk = |k: &str| on_disk.contains(&k.to_uppercase());
        let got = unique_key(&key, &mut taken, &disk);
        let free = (1..).map(numbered).find(|k| !before.contains(&fold(k)) && !disk(k)).unwrap();
        assert_eq!(got, free);
        assert!(taken.contains(&fold(&got)) && taken.len() == before.len() + 1);
        assert_ne!(unique_key(&key, &mut taken, &disk), got, "taken now");
    }
}

/// A track's analysis files go where rekordbox puts them on the target, each keeping its own
/// extension, even when their names differ from the row's in case and their lower case is longer
/// (`İ` is two bytes, its lower case three).
#[test]
fn analysis_files_named_in_another_case_keep_their_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("PIONEER/USBANLZ/P001/00000001");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("ANLZ\u{130}.DAT"), b"d").unwrap();
    std::fs::write(folder.join("anlzi\u{307}.EXT"), b"e").unwrap();
    let tree = crate::tree::scan(dir.path(), &crate::tree::ScanOptions::default(), &Ctx::silent()).unwrap();
    let t = Track { analyze_path: "/PIONEER/USBANLZ/P001/00000001/ANLZ\u{130}.DAT".into(), ..Track::default() };
    let (pairs, analyze_path) =
        plan_analysis(&t, &tree, "Contents/a.mp3", &mut FxHashSet::default(), &mut String::new());
    let stem = paths::analysis_stem(crate::anlz::AnalysisFolder::of("/Contents/a.mp3"), |_| false);
    let to: Vec<&str> = pairs.iter().map(|(_, d)| d.as_str()).collect();
    assert_eq!(to, [format!("{stem}.DAT"), format!("{stem}.EXT")]);
    assert_eq!(analyze_path, format!("{stem}.DAT"));
}

/// A place on the target is taken by any of a track's file names, one whose extension isn't
/// ASCII included: the track then goes to the next `ANLZ000n`.
#[test]
fn a_place_taken_by_any_file_name_goes_to_the_next_number() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("PIONEER/USBANLZ/P001/00000001");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("ANLZ0000.DAT"), b"d").unwrap();
    std::fs::write(folder.join("ANLZ0000.\u{c9}XT"), b"e").unwrap();
    let tree = crate::tree::scan(dir.path(), &crate::tree::ScanOptions::default(), &Ctx::silent()).unwrap();
    let t = Track { analyze_path: "/PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT".into(), ..Track::default() };
    let first = paths::analysis_stem(crate::anlz::AnalysisFolder::of("/Contents/a.mp3"), |_| false);
    let mut taken: FxHashSet<String> = [fold(&format!("{first}.\u{c9}XT"))].into_iter().collect();
    let (_, analyze_path) = plan_analysis(&t, &tree, "Contents/a.mp3", &mut taken, &mut String::new());
    assert_eq!(analyze_path, format!("{}/ANLZ0001.DAT", first.folder));
}

/// A track whose analysis files aren't on the source brings none, and gets no analysis path.
#[test]
fn a_track_whose_analysis_is_missing_brings_none() {
    let dir = tempfile::tempdir().unwrap();
    let tree = crate::tree::scan(dir.path(), &crate::tree::ScanOptions::default(), &Ctx::silent()).unwrap();
    let t = Track { analyze_path: "/PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT".into(), ..Track::default() };
    let (pairs, analyze_path) =
        plan_analysis(&t, &tree, "Contents/a.mp3", &mut FxHashSet::default(), &mut String::new());
    assert!(pairs.is_empty());
    assert_eq!(analyze_path, "");
}
