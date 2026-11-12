use hegel::TestCase;
use hegel::generators as gs;

use super::*;
use crate::tests::{SETTINGS_STRIPPED, SETTINGS_XML, fixture, put};

fn folders(at: &CollectionLocations) -> Folders<'_> {
    Folders::of(at).unwrap()
}

/// The referenced analysis set alone, a deleted track's left out, artwork shared by two tracks
/// once, and a path leading out of `share` refused with a line saying so.
#[test]
fn share_files_are_the_live_tracks_referenced_ones() {
    let f = fixture();
    let (files, warnings) = share_files(f.at.share.as_deref().unwrap(), &f.tracks);
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "share/PIONEER/USBANLZ/46b/3baea/ANLZ0001.DAT",
            "share/PIONEER/USBANLZ/46b/3baea/ANLZ0001.EXT",
            "share/PIONEER/USBANLZ/46b/3baea/ANLZ0001.2EX",
            "share/PIONEER/USBANLZ/46b/3baea/ANLZ0001.3EX",
            "share/PIONEER/USBANLZ/111/222/ANLZ0000.DAT",
            "share/PIONEER/USBANLZ/111/222/ANLZ0000.EXT",
            "share/PIONEER/Artwork/46b/3baea/artwork.jpg",
            "share/PIONEER/Artwork/46b/3baea/artwork_s.jpg",
            "share/PIONEER/Artwork/46b/3baea/artwork_m.jpg",
        ]
    );
    for (name, file) in &files {
        assert!(file.ends_with(name.strip_prefix("share/").unwrap()), "{name}");
    }
    assert_eq!(warnings, ["track 6: /PIONEER/../../escape.DAT isn't in rekordbox's share folder, so it's left out"]);
}

#[test]
fn only_paths_inside_share_are_taken() {
    for inside in ["/PIONEER/USBANLZ/1/2/ANLZ0000.DAT", r"\PIONEER\Artwork\1\a.jpg"] {
        assert!(within_share(inside).is_some(), "{inside}");
    }
    for outside in ["/PIONEER/../x", "PIONEER/x", "/", "", "/a/./b", "C:/PIONEER/x"] {
        assert_eq!(within_share(outside), None, "{outside}");
    }
}

/// Stripping changes `rekordbox3.settings` and `activations.xml` alone; everything else is the
/// file as it is.
#[test]
fn gathered_settings_are_stripped_only_when_asked() {
    let f = fixture();
    let db = f.path("copy.db");
    let (kept, _) = gather(&folders(&f.at), &db, &f.tracks, false).unwrap();
    let (stripped, _) = gather(&folders(&f.at), &db, &f.tracks, true).unwrap();
    assert_eq!(kept.len(), stripped.len());
    for (k, s) in kept.iter().zip(&stripped) {
        assert_eq!(k.name, s.name);
        match s.name.as_str() {
            "rekordbox3.settings" => {
                assert_eq!(k.body, Body::File(f.path("settings/rekordbox3.settings")));
                assert_eq!(s.body, Body::Changed(f.path("settings/rekordbox3.settings"), SETTINGS_STRIPPED.into()));
            }
            "activations.xml" => assert!(matches!(s.body, Body::Changed(..))),
            _ => assert_eq!(k.body, s.body),
        }
    }
    assert_eq!(kept[0], Entry { name: "master.db".into(), body: Body::File(db.clone()) });
    // A settings file that doesn't parse isn't backed up with what it may hold.
    put(&f.path("settings"), "rekordbox3.settings", &SETTINGS_XML.as_bytes()[..60]);
    let e = gather(&folders(&f.at), &db, &f.tracks, true).unwrap_err();
    assert!(e.starts_with("rekordbox3.settings: "), "{e}");
    assert!(gather(&folders(&f.at), &db, &f.tracks, false).is_ok());
}

#[test]
fn music_is_named_as_rekordbox_names_it() {
    let name = |id, artist, album, path| music_name(id, artist, album, path);
    let lizard = "/Users/dj/Music/TikTok/UnknownAlbum/Lizard (Once).mp3";
    assert_eq!(
        name("40822045", Some("TikTok"), Some("Unknown Album"), lizard).unwrap(),
        "/TikTok/UnknownAlbum/026EE51D_Lizard (Once).mp3"
    );
    let long = "/m/Dirt Off Your Shoulder (Know Good Remix).mp3";
    assert_eq!(
        name("106675563", Some("Disciples, Calvin Harris"), None, long).unwrap(),
        "/Disciples,CalvinHarris/UnknownAlbum/065BBD6B_Dirt Off Your Shoulder (Know Good R.mp3"
    );
    assert_eq!(
        name("1", None, Some(" "), r"C:\Music\a.b.wav").unwrap(),
        "/UnknownArtist/UnknownAlbum/00000001_a.b.wav"
    );
    assert_eq!(name("1", Some(".."), Some("AC/DC: Live?"), "/x/.hidden").unwrap(), "/._/AC_DC_Live_/00000001_.hidden");
    assert_eq!(name("not a number", None, None, "/a.mp3"), None);
    assert_eq!(name("4294967296", None, None, "/a.mp3"), None, "more than 32 bits");
    // A device's name on Windows.
    assert_eq!(name("1", Some("Con"), Some("nul.mix"), "/a.mp3").unwrap(), "/_Con/_nul.mix/00000001_a.mp3");
}

/// The cut to 35 characters leaves no whitespace at the stem's end.
#[test]
fn music_stems_lose_the_whitespace_their_cut_leaves() {
    let path = "/m/01 How Deep Is Your Love (DJ Snake Remix).mp3";
    assert_eq!(
        music_name("210303334", Some("Calvin Harris"), None, path).unwrap(),
        "/CalvinHarris/UnknownAlbum/0C88F966_01 How Deep Is Your Love (DJ Snake.mp3"
    );
}

/// Folders are cut to 48 characters once their whitespace is gone, counted in characters, not
/// bytes.
#[test]
fn music_folders_are_cut_to_48_characters() {
    let album = "Variations In Production Vol. 1 (Matroda Drum N Bass VIPs)";
    let path = "/m/03 Saturday Love (Matroda VIP).mp3";
    assert_eq!(
        music_name("39174117", Some("Matroda"), Some(album), path).unwrap(),
        "/Matroda/VariationsInProductionVol.1(MatrodaDrumNBassVIPs/0255BFE5_03 Saturday Love (Matroda VIP).mp3"
    );
    let artist = "Camo & Krooked, P Money, Whiney, Sophie Lindinger, Mefjus";
    let path = "/m/No Tomorrow (P Money X Whiney Remix).mp3";
    assert_eq!(
        music_name("67147437", Some(artist), None, path).unwrap(),
        "/Camo&Krooked,PMoney,Whiney,SophieLindinger,Mefju/UnknownAlbum/040096AD_No Tomorrow (P Money X Whiney Remix.mp3"
    );
    let long = music_name("1", Some(&"東".repeat(90)), None, "/a.mp3").unwrap();
    assert_eq!(long, format!("/{}/UnknownAlbum/00000001_a.mp3", "東".repeat(48)));
}

/// A folder's last `.` becomes `_`, and only its last.
#[test]
fn music_folders_end_in_no_dot() {
    let name = |artist, album| music_name("17288646", Some(artist), Some(album), "/m/01 Bloodhound.mp3").unwrap();
    assert_eq!(name("fred again..", "OMRI."), "/fredagain._/OMRI_/0107CDC6_01 Bloodhound.mp3");
    assert_eq!(name("DJ POLO", "If The Glove Fits E.P."), "/DJPOLO/IfTheGloveFitsE.P_/0107CDC6_01 Bloodhound.mp3");
}

/// `~` in a folder becomes `_`. In a file's stem it stays, which no backup seen has shown either
/// way.
#[test]
fn music_folders_have_no_tilde() {
    let album = "The Legend of Kòrá Remixes & Edits ~Book Three~";
    let path = "/m/02 Stayin Alivinho w_ Paul Mond (Lefty Remix).mp3";
    assert_eq!(
        music_name("64662554", Some("Tera Kòrá"), Some(album), path).unwrap(),
        "/TeraKòrá/TheLegendofKòráRemixes&Edits_BookThree_/03DAAC1A_02 Stayin Alivinho w_ Paul Mond (Le.mp3"
    );
    assert_eq!(music_name("1", None, None, "/m/a~b.mp3").unwrap(), "/UnknownArtist/UnknownAlbum/00000001_a~b.mp3");
}

/// A decomposed artist is composed (NFC), as rekordbox's folders were for artists `master.db`
/// stores decomposed.
#[test]
fn music_folders_are_composed() {
    let name = |artist| music_name("1", Some(artist), None, "/a.mp3").unwrap();
    assert_eq!(name("Beyonce\u{301}"), "/Beyonc\u{e9}/UnknownAlbum/00000001_a.mp3");
    assert_eq!(name("Juste\u{307}"), "/Just\u{117}/UnknownAlbum/00000001_a.mp3");
    assert_eq!(name("dualite\u{301}"), "/dualit\u{e9}/UnknownAlbum/00000001_a.mp3");
}

/// C1 controls stay in a name, as rekordbox keeps them; C0 controls, which Windows refuses, don't.
#[test]
fn music_names_keep_c1_controls() {
    let path = "/m/Paint The Town Red (Jo\u{cc}\u{88}rgen & YAMB Remix).mp3";
    assert_eq!(
        music_name("199407894", Some("Doja Cat"), None, path).unwrap(),
        "/DojaCat/UnknownAlbum/0BE2B916_Paint The Town Red (Jo\u{cc}\u{88}rgen & YAMB.mp3"
    );
    assert_eq!(
        music_name("1", Some("a\u{86}\u{1}"), None, "/m/b\u{9f}\u{1f}\u{7f}.mp3").unwrap(),
        "/a\u{86}_/UnknownAlbum/00000001_b\u{9f}_\u{7f}.mp3"
    );
}

/// rekordbox's groups in order, each entry once and none a folder, the library's and the
/// settings' files only when they're there, whichever are.
#[hegel::test(test_cases = 40)]
fn prop_entries_are_in_rekordbox_order_and_unique(tc: TestCase) {
    let f = fixture();
    let present = |tc: &TestCase, dir: &Path, names: &[&str]| -> Vec<String> {
        let mut kept = Vec::new();
        for name in names {
            let path = dir.join(name);
            if tc.draw(gs::booleans()) {
                put(dir, name, b"there");
                kept.push(name.to_string());
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
        kept
    };
    let library = present(&tc, &f.path("rekordbox"), &LIBRARY);
    let settings = present(&tc, &f.path("settings"), &SETTINGS);
    // Tracks drawn from the fixture's, in any order, some more than once.
    let picks = tc.draw(gs::vecs(gs::sampled_from(f.tracks.clone())).max_size(8));
    let (entries, _) = gather(&folders(&f.at), &f.path("copy.db"), &picks, false).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let mut seen = FxHashSet::default();
    assert!(names.iter().all(|n| seen.insert(*n)), "unique: {names:?}");
    assert!(names.iter().all(|n| !n.ends_with('/')), "no folders: {names:?}");
    let group = |n: &str| match n {
        "master.db" => 0,
        n if LIBRARY.contains(&n) => 1,
        n if SETTINGS.contains(&n) => 2,
        n if n.starts_with("share/PIONEER/USBANLZ/") => 3,
        n if n.starts_with("share/PIONEER/Artwork/") => 4,
        n if n.starts_with("KeyMappings/") => 5,
        n => panic!("{n} isn't one rekordbox backs up"),
    };
    let groups: Vec<u8> = names.iter().map(|n| group(n)).collect();
    assert!(groups.is_sorted(), "{names:?}");
    let in_order = |list: &[&str], kept: &[String]| {
        let got: Vec<&str> = names.iter().copied().filter(|n| list.contains(n)).collect();
        assert_eq!(got, kept);
    };
    in_order(&LIBRARY, &library);
    in_order(&SETTINGS, &settings);
    for name in names.iter().filter(|n| n.starts_with("share/PIONEER/USBANLZ/")) {
        let (stem, ext) = name.rsplit_once('.').unwrap();
        assert!(ANALYSIS.contains(&ext), "{name}");
        let named = picks.iter().filter(|t| !t.deleted).filter_map(|t| t.analysis_path.as_deref());
        assert!(named.into_iter().any(|p| format!("share{}", p.rsplit_once('.').unwrap().0) == stem), "{name}");
    }
}

/// Whatever the tags and file name, a copy's name is three levels deep, the folders free of
/// spaces and separators, the file the id in 8 hex digits, `_` and the stem's first 35
/// characters without trailing whitespace, with its extension.
#[hegel::test(test_cases = 300)]
fn prop_music_names_follow_the_hexid_rule(tc: TestCase) {
    let id = tc.draw(gs::integers::<u32>());
    let artist = tc.draw(gs::optional(gs::text().max_size(80)));
    let album = tc.draw(gs::optional(gs::text().max_size(30)));
    let stem = tc.draw(gs::text().min_size(1).max_size(60)).replace(['/', '\\', '.'], "x");
    let ext = tc.draw(gs::from_regex("[a-z0-9]{1,4}").fullmatch(true));
    let dir = tc.draw(gs::sampled_from(vec!["/Users/dj/Music/", r"C:\Music\", "/Volumes/USB/a/b/"]));
    let name = music_name(&id.to_string(), artist.as_deref(), album.as_deref(), &format!("{dir}{stem}.{ext}")).unwrap();
    let parts: Vec<&str> = name.split('/').collect();
    let [empty, artist_dir, album_dir, file] = parts[..] else { panic!("three levels: {name}") };
    assert_eq!(empty, "");
    for folder in [artist_dir, album_dir] {
        assert!(!folder.is_empty() && !folder.chars().any(char::is_whitespace), "{name}");
        assert!(folder.chars().any(|c| c != '.'), "{name}");
        assert!(!folder.contains(['\\', ':', '*', '?', '"', '<', '>', '|']), "{name}");
        let base = folder.split('.').next().unwrap();
        assert!(!DEVICES.iter().any(|d| d.eq_ignore_ascii_case(base)), "{name}");
    }
    let hex = format!("{id:08X}_");
    let rest = file.strip_prefix(&hex).unwrap_or_else(|| panic!("{name}"));
    let (kept, file_ext) = rest.rsplit_once('.').unwrap();
    assert_eq!(file_ext, ext);
    let cut: String = stem.chars().take(35).collect();
    let safe: String =
        cut.trim_end().chars().map(|c| if c < ' ' || r#":*?"<>|"#.contains(c) { '_' } else { c }).collect();
    assert_eq!(kept, safe);
}

/// Whatever the tags and file name, a copy's name keeps rekordbox's limits: no whitespace at the
/// stem's end, and folders of at most 48 characters, ending in no `.`, with no `~`.
#[hegel::test(test_cases = 300)]
fn prop_music_names_keep_rekordboxs_limits(tc: TestCase) {
    let ends = || gs::sampled_from(vec!["", ".", "..", "~", " ", " .", ". "]);
    // Padded, so a tag often has more than 48 characters left once its whitespace is gone.
    let pad = |tc: &TestCase| "x".repeat(tc.draw(gs::integers::<usize>().max_value(60)));
    let tag = |tc: &TestCase| tc.draw(gs::optional(gs::text().max_size(80))).map(|t| pad(tc) + &t + tc.draw(ends()));
    let (artist, album) = (tag(&tc), tag(&tc));
    let stem = tc.draw(gs::text().min_size(1).max_size(60)).replace(['/', '\\', '.'], "x") + tc.draw(ends());
    let path = format!("/m/{stem}.mp3");
    let name = music_name("1", artist.as_deref(), album.as_deref(), &path).unwrap();
    let parts: Vec<&str> = name.split('/').collect();
    let ["", artist_dir, album_dir, file] = parts[..] else { panic!("three levels: {name:?}") };
    for folder in [artist_dir, album_dir] {
        assert!(folder.chars().count() <= 48, "{name:?}");
        assert!(!folder.ends_with('.') && !folder.contains('~'), "{name:?}");
    }
    let (kept, _) = file.rsplit_once('.').unwrap();
    assert_eq!(kept, kept.trim_end(), "{name:?}");
}
