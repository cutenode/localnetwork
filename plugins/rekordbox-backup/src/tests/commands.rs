//! `backup`, `backups` and `verify` over the protocol, against the fixture's library.

use std::path::Path;

use loclib_plugin::{Column, ExportedFile, Pair, Section};

use super::*;
use crate::contents::{Contents, Folders};
use crate::entries::{Body, Entry};
use crate::list::read_contents;

/// The zip's entries a backup of the fixture has, in order, and the files they're from (but
/// `master.db`, the host's copy, and `contents.xml`, made).
fn expected(f: &Fixture) -> Vec<(String, Option<PathBuf>)> {
    let data = |n: &str| (n.to_string(), Some(f.path("rekordbox").join(n)));
    let settings = |n: &str| (n.to_string(), Some(f.path("settings").join(n)));
    let share = |n: &str| (format!("share/{n}"), Some(f.path("rekordbox/share").join(n)));
    vec![
        ("master.db".into(), None),
        data("masterPlaylists6.xml"),
        data("automixPlaylist6.xml"),
        data("networkAnalyze6.db"),
        data("playlists3.sync"),
        settings("rekordbox3.settings"),
        settings("PlaySettings.xml"),
        settings("activations.xml"),
        settings("LightingDB/user.db3"),
        settings("MYSETTING.DAT"),
        share("PIONEER/USBANLZ/46b/3baea/ANLZ0001.DAT"),
        share("PIONEER/USBANLZ/46b/3baea/ANLZ0001.EXT"),
        share("PIONEER/USBANLZ/46b/3baea/ANLZ0001.2EX"),
        share("PIONEER/USBANLZ/46b/3baea/ANLZ0001.3EX"),
        share("PIONEER/USBANLZ/111/222/ANLZ0000.DAT"),
        share("PIONEER/USBANLZ/111/222/ANLZ0000.EXT"),
        share("PIONEER/Artwork/46b/3baea/artwork.jpg"),
        share("PIONEER/Artwork/46b/3baea/artwork_s.jpg"),
        share("PIONEER/Artwork/46b/3baea/artwork_m.jpg"),
        settings("KeyMappings/rekordbox_0000000000030.mappings"),
        ("contents.xml".into(), None),
    ]
}

/// The music a backup of the fixture copies: the three tracks with a file.
fn music(f: &Fixture) -> Vec<(String, String, PathBuf)> {
    let copy = |hex: &str, name: &str, from: &str| (hex.to_string(), name.to_string(), f.path("music").join(from));
    vec![
        copy("026EE51D", "/OliverTree/UnknownAlbum/026EE51D_Lizard (Once).mp3", "TikTok/Lizard (Once).mp3"),
        copy(
            "00000002",
            "/Łukasz&Mø💀/Über_Alles_/00000002_A file name far longer than thirty-.flac",
            "A file name far longer than thirty-five characters.flac",
        ),
        copy("00000006", "/UnknownArtist/UnknownAlbum/00000006_six.wav", "six.wav"),
    ]
}

/// The `contents.xml` a backup of the fixture has, with `roots` copied from an earlier one and
/// the `music` it copied.
fn contents(roots: Vec<(String, String)>, music: &[(String, String, PathBuf)]) -> Vec<u8> {
    let folders = Folders::here().unwrap();
    // The fixture's music is in a temporary folder, in none of the user's own.
    let directories = vec!["/".to_string()];
    let tracks = music.iter().map(|(hex, name, _)| (hex.clone(), name.clone())).collect();
    let c = Contents { roots, tracks, db_id: DB_ID.into(), content_num: 5, rb_version: 70214, directories, folders };
    c.to_xml()
}

fn kv(view: &View) -> Vec<(String, Value)> {
    let View::Sections { items, .. } = view else { panic!("sections: {view:?}") };
    let View::Kv { pairs, .. } = &items[0].view else { panic!("pairs: {view:?}") };
    pairs.iter().map(|Pair { label, value }| (label.clone(), value.clone())).collect()
}

#[test]
fn the_plan_lists_the_zip_and_the_music_with_what_to_know() {
    stamped();
    let f = fixture();
    let host = host(&f);
    let plan = super::plan(&host, &f, json!({})).unwrap();
    // Its size before compression: rekordbox's own master.db, as the copy isn't made yet.
    let live = f.at.master_db.clone();
    let files = expected(&f).into_iter().filter(|(n, _)| n != "contents.xml");
    let estimate: u64 = files.map(|(_, p)| std::fs::metadata(p.or(live.clone()).unwrap()).unwrap().len()).sum();
    let zip = ExportedFile {
        path: f.zip(),
        what: "rekordbox library backup (its size before compression)".into(),
        bytes: estimate,
    };
    assert_eq!(plan.files, std::slice::from_ref(&zip));
    assert_eq!(plan.summary, format!("Back up rekordbox's library (5 tracks) to {}", f.zip().display()));
    let tokens = "the backup holds your rekordbox and Dropbox sign-in tokens, as rekordbox's own does: keep it \
private, or set credentials to strip";
    let escape = "track 6: /PIONEER/../../escape.DAT isn't in rekordbox's share folder, so it's left out";
    assert_eq!(plan.warnings, [tokens, escape]);
    assert!(std::fs::read_dir(&f.dest).unwrap().next().is_none(), "a plan writes nothing");

    // With the music, stripped, over a backup of today's.
    std::fs::write(f.zip(), b"an older backup").unwrap();
    let plan = super::plan(&host, &f, json!({ "music": true, "credentials": "strip" })).unwrap();
    let root = f.dest.join("rekordbox_bak");
    let copies = music(&f).into_iter().map(|(_, name, from)| ExportedFile {
        path: name.split('/').skip(1).fold(root.clone(), |p, n| p.join(n)),
        what: "music".into(),
        bytes: std::fs::metadata(from).unwrap().len(),
    });
    let stripped =
        (SETTINGS_XML.len() - SETTINGS_STRIPPED.len()) + (ACTIVATIONS_XML.len() - ACTIVATIONS_STRIPPED.len());
    let zip = ExportedFile { bytes: estimate - stripped as u64, ..zip };
    assert_eq!(plan.files, std::iter::once(zip).chain(copies).collect::<Vec<_>>());
    let gone = f.path("music/gone.mp3");
    let warnings = [
        format!("{} is there already, and is replaced", f.zip().display()),
        escape.to_string(),
        format!("track 4: {} isn't there, so its music isn't copied", gone.display()),
    ];
    assert_eq!(plan.warnings, warnings);
    assert!(plan.summary.ends_with(&format!(", and copy 3 music files into {}", root.display())), "{}", plan.summary);
    host.stop();
}

/// A backup is rekordbox's layout to the entry: the files it backs up in its order, those it
/// leaves out left out, the settings as they are (tokens and all) and `contents.xml` as rekordbox
/// writes it, with the per-root number an earlier backup of the library had. The music is copied
/// beside it, keeping each file's time.
#[test]
fn a_backup_is_laid_out_as_rekordboxs() {
    stamped();
    let f = fixture();
    let host = host(&f);
    // An earlier backup of the library, whose number for the root `/` is copied.
    let earlier = f.dest.join("rekordbox_bak_20200101.zip");
    let values = [("/", "-1193679842"), ("DB_ID", DB_ID), ("CONTENT_NUM", "1")].map(|(n, v)| (n.into(), v.into()));
    let made = Entry { name: "contents.xml".into(), body: Body::Made(contents::write(&values)) };
    write::write(&[made], &earlier, &loclib_progress::Ctx::silent()).unwrap();

    let view = as_view(run(&host, &f, "backup", json!({}), json!({ "music": true })).unwrap());
    let zip = f.zip();
    let expected = expected(&f);
    let names: Vec<String> = expected.iter().map(|(n, _)| n.clone()).collect();
    assert_eq!(super::names(&zip), names);
    for (name, file) in &expected[1..expected.len() - 1] {
        assert_eq!(entry(&zip, name), std::fs::read(file.as_ref().unwrap()).unwrap(), "{name}");
    }
    assert_eq!(entry(&zip, "master.db"), MASTER_DB);
    assert_eq!(entry(&zip, "rekordbox3.settings"), SETTINGS_XML.as_bytes());
    let roots = vec![("/".to_string(), "-1193679842".to_string())];
    assert_eq!(
        String::from_utf8(entry(&zip, "contents.xml")).unwrap(),
        String::from_utf8(contents(roots, &music(&f))).unwrap()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&zip).unwrap().permissions().mode() & 0o777, 0o600, "it holds tokens");
    }
    for (_, name, from) in music(&f) {
        let copy = name.split('/').skip(1).fold(f.dest.join("rekordbox_bak"), |p, n| p.join(n));
        assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&from).unwrap(), "{name}");
        let time = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        assert_eq!(time(&copy), time(&from), "{name}");
    }
    // The tracks are the copy's, which the host was asked to make with the tokens.
    let calls = host.calls();
    let copy = calls.iter().find(|(m, _)| m == "collection.copy").unwrap();
    assert_eq!(copy.1["with_credentials"], json!(true));
    let read = calls.iter().find(|(m, _)| m == "collection.tracks").unwrap();
    assert_eq!(read.1["db"], copy.1["into"]);
    let View::Kv { pairs, .. } = view else { panic!("{view:?}") };
    assert_eq!(pairs[3], Pair { label: "Tracks".into(), value: json!(5) });
    assert_eq!(pairs[4], Pair { label: "Music".into(), value: json!("3 copied, 0 there already") });

    // A second backup copies no music again, and replaces the zip of the day.
    std::fs::write(&zip, b"replaced").unwrap();
    let again = as_view(run(&host, &f, "backup", json!({}), json!({ "music": true })).unwrap());
    let View::Kv { pairs, .. } = again else { panic!("{again:?}") };
    assert_eq!(pairs[4], Pair { label: "Music".into(), value: json!("0 copied, 3 there already") });
    assert_eq!(super::names(&zip), names);
    host.stop();
}

/// Stripped, the settings files lose the lines holding tokens and nothing else, and the host's
/// copy of `master.db` leaves out the cloud agent's sign-in.
#[test]
fn stripped_credentials_leave_the_rest_as_it_is() {
    stamped();
    let f = fixture();
    let host = host(&f);
    run(&host, &f, "backup", json!({}), json!({ "credentials": "strip" })).unwrap();
    let zip = f.zip();
    assert_eq!(entry(&zip, "rekordbox3.settings"), SETTINGS_STRIPPED.as_bytes());
    assert_eq!(entry(&zip, "activations.xml"), ACTIVATIONS_STRIPPED.as_bytes());
    let copy = host.calls().into_iter().find(|(m, _)| m == "collection.copy").unwrap();
    assert_eq!(copy.1["with_credentials"], json!(false));
    // Without the music, no track is mapped to a copy of its music.
    let values = read_contents(&zip).unwrap();
    assert!(!values.iter().any(|(n, _)| list::is_track(n)), "{values:?}");
    assert!(!f.dest.join("rekordbox_bak").exists());
    host.stop();
}

/// A track whose music can't be copied is said and left out of `contents.xml`; the rest of the
/// backup is made, its zip after the music, so it maps only copies that are there.
#[cfg(unix)]
#[test]
fn music_that_cant_be_copied_isnt_mapped() {
    use std::os::unix::fs::PermissionsExt;
    stamped();
    let f = fixture();
    let host = host(&f);
    let six = f.path("music/six.wav");
    std::fs::set_permissions(&six, std::fs::Permissions::from_mode(0o000)).unwrap();
    let view = as_view(run(&host, &f, "backup", json!({}), json!({ "music": true })).unwrap());
    let View::Kv { pairs, .. } = view else { panic!("{view:?}") };
    assert_eq!(pairs[4], Pair { label: "Music".into(), value: json!("2 copied, 0 there already, 1 failed") });
    let mapped: Vec<String> =
        read_contents(&f.zip()).unwrap().into_iter().filter(|(n, _)| list::is_track(n)).map(|(n, _)| n).collect();
    assert_eq!(mapped, ["026EE51D", "00000002"]);
    host.stop();
}

/// Cancelled while the host reads the tracks, a backup leaves nothing in the backup folder.
#[test]
fn a_cancelled_backup_leaves_nothing() {
    stamped();
    let f = fixture();
    let host = host(&f);
    host.cancel_on("collection.tracks");
    let e = run(&host, &f, "backup", json!({}), json!({ "music": true })).unwrap_err();
    assert_eq!(e.kind(), Some("cancelled"), "{e:?}");
    assert_eq!(std::fs::read_dir(&f.dest).unwrap().count(), 0);
    host.stop();
}

#[test]
fn a_library_that_isnt_there_isnt_backed_up() {
    let f = fixture();
    let host = host(&f);
    host.read("collection.locations", CollectionLocations { master_db_exists: false, ..f.at.clone() });
    let e = run(&host, &f, "backup", json!({}), json!({})).unwrap_err();
    assert_eq!(e.kind(), Some("not_found"), "{e:?}");
    host.read("collection.locations", f.at.clone());
    host.read("collection.status", CollectionStatus { running: true, version: None });
    let e = run(&host, &f, "backup", json!({}), json!({})).unwrap_err();
    assert!(e.message.contains("which version of rekordbox"), "{e:?}");
    // Nor is one to a folder that isn't there (a drive not plugged in).
    let req = json!({ "command": "backup", "context": { "settings": { "destination": f.path("unplugged") } } });
    let e = host.request::<Value>(method::RUN_COMMAND, &req).unwrap_err();
    assert_eq!(e.kind(), Some("not_found"), "{e:?}");
    assert!(!f.path("unplugged").exists());
    host.stop();
}

/// `backups` lists every backup with what its `contents.xml` says, and says how to restore one.
#[test]
fn backups_are_listed_with_what_they_hold() {
    stamped();
    let f = fixture();
    let host = host(&f);
    let none = as_view(run(&host, &f, "backups", json!({}), json!({})).unwrap());
    let View::Text { text, .. } = none else { panic!("{none:?}") };
    assert!(text.starts_with("No rekordbox library backups"), "{text}");
    run(&host, &f, "backup", json!({}), json!({ "music": true })).unwrap();
    std::fs::write(f.dest.join("rekordbox_bak_20200101.zip"), b"not a zip").unwrap();
    std::fs::write(f.dest.join("notes.zip"), b"not a backup").unwrap();
    let view = as_view(run(&host, &f, "backups", json!({}), json!({})).unwrap());
    let View::Sections { items, .. } = view else { panic!("{view:?}") };
    let [Section { view: View::Table { columns, rows, .. }, .. }, Section { view: restore, .. }] = &items[..] else {
        panic!("{items:?}")
    };
    let headings: Vec<&str> = columns.iter().map(Column::name).collect();
    assert_eq!(headings, ["Backup", "Date", "Size", "Tracks", "rekordbox", "Music"]);
    let size = crate::size(std::fs::metadata(f.zip()).unwrap().len());
    let mut rows = rows.clone();
    rows.sort_by_key(|r| r[0].as_str().unwrap().to_string());
    let today = f.zip().file_name().unwrap().to_string_lossy().into_owned();
    let unreadable = [json!("rekordbox_bak_20200101.zip"), json!("2020-01-01"), json!("9 bytes"), Value::Null];
    assert_eq!(rows[0][..4], unreadable);
    assert!(rows[0][4].as_str().unwrap().starts_with("can't read it: not a zip"), "{:?}", rows[0]);
    assert_eq!(rows[0][5], Value::Null);
    let made = [json!(today), json!("2026-09-21"), json!(size), json!(5), json!("7.2.14"), json!("yes")];
    assert_eq!(rows[1], made);
    assert_eq!(restore, &View::Text { title: None, text: RESTORE.into() });
    host.stop();
}

/// The byte of `name`'s data in the zip at `path` flipped, as a disk might.
fn damage(path: &Path, name: &str) {
    let header = {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        zip.by_name(name).unwrap().header_start()
    };
    let mut bytes = std::fs::read(path).unwrap();
    // The local header is 30 bytes and the name, with no extra field.
    let at = header as usize + 30 + name.len() + 2;
    bytes[at] ^= 0xff;
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn verify_says_a_whole_backup_is_whole_and_what_a_damaged_one_lacks() {
    stamped();
    let f = fixture();
    let host = host(&f);
    run(&host, &f, "backup", json!({}), json!({ "music": true })).unwrap();
    let zip = f.zip();
    let checked = as_view(run(&host, &f, "verify", json!({ "file": zip }), json!({})).unwrap());
    let rest = &kv(&checked)[1..];
    let music = format!("3 files, all in {}", f.dest.join("rekordbox_bak").display());
    let expected = [
        ("contents.xml", json!("rekordbox 7.2.14, format 4")),
        ("master.db", json!("opens, 5 tracks")),
        ("Music", json!(music)),
    ]
    .map(|(l, v)| (l.to_string(), v));
    assert_eq!(rest, expected);
    // The host read the backup's own master.db, unpacked into the request's scratch folder.
    let calls = host.calls();
    let (_, read) = calls.iter().rfind(|(m, _)| m == "collection.tracks").unwrap();
    assert!(read["db"].as_str().unwrap().ends_with("master.db"), "{read}");

    // A music file gone.
    let six = f.dest.join("rekordbox_bak/UnknownArtist/UnknownAlbum/00000006_six.wav");
    std::fs::remove_file(&six).unwrap();
    let e = run(&host, &f, "verify", json!({ "file": zip }), json!({})).unwrap_err();
    assert!(
        e.message.ends_with(&format!("1 of its 3 music files aren't in {}", f.dest.join("rekordbox_bak").display())),
        "{e:?}"
    );
    // A track count that disagrees with the database's.
    let wrong = f.dest.join("wrong.zip");
    let values: Vec<(String, String)> = read_contents(&zip)
        .unwrap()
        .into_iter()
        .map(|(n, v)| if n == "CONTENT_NUM" { (n, "9".into()) } else { (n, v) })
        .filter(|(n, _)| !list::is_track(n))
        .collect();
    let copy = f.dir.path().join("master.db copy");
    std::fs::write(&copy, MASTER_DB).unwrap();
    let entries = [
        Entry { name: "master.db".into(), body: Body::File(copy) },
        Entry { name: "contents.xml".into(), body: Body::Made(contents::write(&values)) },
    ];
    write::write(&entries, &wrong, &loclib_progress::Ctx::silent()).unwrap();
    let e = run(&host, &f, "verify", json!({ "file": wrong }), json!({})).unwrap_err();
    assert!(e.message.ends_with("its master.db has 5 tracks and contents.xml says 9"), "{e:?}");
    // A damaged entry.
    damage(&zip, "masterPlaylists6.xml");
    let e = run(&host, &f, "verify", json!({ "file": zip }), json!({})).unwrap_err();
    assert_eq!(e.kind(), Some("invalid"));
    assert!(e.message.contains("masterPlaylists6.xml is damaged"), "{e:?}");
    // Not a zip at all.
    std::fs::write(&wrong, b"PK not really").unwrap();
    let e = run(&host, &f, "verify", json!({ "file": wrong }), json!({})).unwrap_err();
    assert!(e.message.contains("isn't a zip that opens"), "{e:?}");
    host.stop();
}

/// Not told which, `verify` asks, offering the backups in the backup folder.
#[test]
fn verify_asks_which_backup() {
    stamped();
    let f = fixture();
    let host = host(&f);
    let e = run(&host, &f, "verify", json!({}), json!({})).unwrap_err();
    assert_eq!(e.kind(), Some("not_found"), "{e:?}");
    run(&host, &f, "backup", json!({}), json!({})).unwrap();
    let asked = as_view(run(&host, &f, "verify", json!({}), json!({})).unwrap());
    let View::Choose(choice) = asked else { panic!("{asked:?}") };
    assert_eq!((choice.param.as_str(), choice.items.len()), ("file", 1));
    assert_eq!(choice.items[0].value, json!(f.zip()));
    host.stop();
}
