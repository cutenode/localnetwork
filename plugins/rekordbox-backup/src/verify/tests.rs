use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::method;
use loclib_plugin::test_util::FakeHost;
use zip::write::SimpleFileOptions;

use super::*;
use crate::tests::{DB_ID, Fixture, MASTER_DB, as_view, fixture, host, run, stamped};

#[test]
fn only_places_in_the_music_folder_are_looked_for() {
    assert!(in_music_folder("/OliverTree/UnknownAlbum/026EE51D_Lizard (Once).mp3"));
    for outside in
        ["/../../etc/x", "/a/../b", "/a/b", "/a/b/c/d", "a/b/c", "/C:/b/c", r"/a\..\b/c/d", "/a//c", "/./b/c"]
    {
        assert!(!in_music_folder(outside), "{outside}");
    }
}

/// A `contents.xml` with every entry a backup needs, for the fixture's library, and `more`.
fn contents_xml(more: &[(&str, &str)]) -> Vec<u8> {
    let needed = [("DB_ID", DB_ID), ("CONTENT_NUM", "5"), ("BACKUP_RB_VER", "70214"), ("BACKUP_FILE_VER", "4")];
    let values = needed.into_iter().chain([("DIRECTORY_ARRAY", "/")]).chain(more.iter().copied());
    contents::write(&values.map(|(n, v)| (n.to_string(), v.to_string())).collect::<Vec<_>>())
}

/// A zip at `path` holding `entries` in order: a file with its bytes, or a folder (`None`).
fn zip_of(path: &Path, entries: &[(&str, Option<&[u8]>)]) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        match bytes {
            Some(bytes) => {
                zip.start_file(*name, SimpleFileOptions::default()).unwrap();
                zip.write_all(bytes).unwrap();
            }
            None => zip.add_directory(*name, SimpleFileOptions::default()).unwrap(),
        }
    }
    zip.finish().unwrap();
}

/// What `verify` says is wrong with the backup at `zip`.
fn problems(host: &FakeHost, f: &Fixture, zip: &Path) -> Vec<String> {
    let e = run(host, f, "verify", json!({ "file": zip }), json!({})).unwrap_err();
    assert_eq!(e.kind(), Some("invalid"), "{e:?}");
    let (_, said) = e.message.split_once(" isn't whole: ").unwrap();
    said.split("; ").map(String::from).collect()
}

/// A zip laid out otherwise than rekordbox's: each way it differs is said.
#[test]
fn verify_says_how_a_zip_is_laid_out_wrong() {
    stamped();
    let f = fixture();
    let host = host(&f);
    let zip = f.dest.join("rekordbox_bak_20260101.zip");
    let xml = contents_xml(&[]);
    zip_of(&zip, &[("contents.xml", Some(&xml)), ("share/", None), ("master.db", Some(MASTER_DB))]);
    let expected = [
        "it has a folder entry, share/, which rekordbox's backups don't",
        "master.db isn't its first entry",
        "contents.xml isn't its last entry",
        "it has no master.db",
    ];
    assert_eq!(problems(&host, &f, &zip), expected);
    host.stop();
}

/// A zip with two entries of one name is said to be. The zip crate won't write one, so the second
/// entry's name is changed in the zip's bytes (its local header and central directory) after.
#[test]
fn verify_says_when_a_zip_has_a_name_twice() {
    stamped();
    let f = fixture();
    let host = host(&f);
    let zip = f.dest.join("rekordbox_bak_20260101.zip");
    let xml = contents_xml(&[]);
    zip_of(&zip, &[("master.db", Some(MASTER_DB)), ("master.dX", Some(MASTER_DB)), ("contents.xml", Some(&xml))]);
    let mut bytes = std::fs::read(&zip).unwrap();
    let at: Vec<usize> = bytes.windows(9).enumerate().filter(|(_, w)| w == b"master.dX").map(|(i, _)| i).collect();
    assert_eq!(at.len(), 2, "the local header and the central directory");
    for i in at {
        bytes[i + 8] = b'b';
    }
    std::fs::write(&zip, bytes).unwrap();
    assert_eq!(problems(&host, &f, &zip), ["it has two entries of the same name"]);
    host.stop();
}

/// Each way a `contents.xml` can let the backup down is said: one that isn't XML, a value that
/// can't be read, none at all, entries missing, music out of its folder or misnamed, and another
/// library's id.
#[test]
fn verify_says_what_contents_xml_gets_wrong() {
    stamped();
    let f = fixture();
    let host = host(&f);
    let zip = f.dest.join("rekordbox_bak_20260101.zip");
    let with = |xml: &[u8]| zip_of(&zip, &[("master.db", Some(MASTER_DB)), ("contents.xml", Some(xml))]);
    with(b"<PROPERTIES><VALUE name=\"a\" val=\"b\"/></WRONG>");
    let [said] = &problems(&host, &f, &zip)[..] else { panic!() };
    assert!(said.starts_with("contents.xml isn't XML: "), "{said}");
    with(b"<PROPERTIES><VALUE name=\"a\" val=\"&nothing;\"/></PROPERTIES>");
    let [said] = &problems(&host, &f, &zip)[..] else { panic!() };
    assert!(said.starts_with("contents.xml has a value it can't read: "), "{said}");
    zip_of(&zip, &[("master.db", Some(MASTER_DB))]);
    assert_eq!(problems(&host, &f, &zip), ["contents.xml isn't its last entry", "it has no contents.xml that reads"]);
    let values = [
        ("DB_ID", "999"),
        ("CONTENT_NUM", "5"),
        ("0000000A", "/../../etc/passwd"),
        ("0000000B", "/Artist/Album/0000000C_x.mp3"),
    ];
    let xml = contents::write(&values.map(|(n, v)| (n.to_string(), v.to_string())));
    with(&xml);
    let expected = [
        "contents.xml has no BACKUP_RB_VER, BACKUP_FILE_VER, DIRECTORY_ARRAY",
        "contents.xml names 1 music files outside the music folder",
        "contents.xml names 1 music files that aren't named after their track",
        "its master.db is another library than contents.xml says (DB_ID)",
    ];
    assert_eq!(problems(&host, &f, &zip), expected);
    host.stop();
}

/// A `master.db` the host can't open is said; one cancelled while the host reads it cancels.
#[test]
fn verify_says_when_master_db_doesnt_open() {
    stamped();
    let f = fixture();
    let host = host(&f);
    let zip = f.dest.join("rekordbox_bak_20260101.zip");
    zip_of(&zip, &[("master.db", Some(b"not a database")), ("contents.xml", Some(&contents_xml(&[])))]);
    host.on_read("collection.tracks", |_| Err(RpcError::failed("invalid", "file is not a database")));
    assert_eq!(problems(&host, &f, &zip), ["its master.db doesn't open (file is not a database)"]);
    host.cancel_on("collection.tracks");
    let e = run(&host, &f, "verify", json!({ "file": zip }), json!({})).unwrap_err();
    assert_eq!(e.kind(), Some("cancelled"), "{e:?}");
    host.stop();
}

/// A backup that isn't there fails, naming it.
#[test]
fn verify_says_when_the_backup_isnt_there() {
    let f = fixture();
    let host = host(&f);
    let zip = f.dest.join("rekordbox_bak_20260101.zip");
    let e = run(&host, &f, "verify", json!({ "file": zip }), json!({})).unwrap_err();
    assert_eq!(e.kind(), Some("io"), "{e:?}");
    assert!(e.message.starts_with(&zip.display().to_string()), "{e:?}");
    host.stop();
}

/// A host with no scratch folder to unpack `master.db` into can't verify.
#[test]
fn verify_needs_a_scratch_folder() {
    let f = fixture();
    let host = host(&f);
    let zip = f.dest.join("rekordbox_bak_20260101.zip");
    let context = json!({ "settings": { "destination": f.dest }, "scratch": null });
    let req = json!({ "command": "verify", "params": { "file": zip }, "context": context });
    let e = host.request::<Value>(method::RUN_COMMAND, &req).unwrap_err();
    assert_eq!(e.kind(), Some("incompatible"), "{e:?}");
    host.stop();
}

/// A backup whose music folder isn't beside it any more is still whole, and says where it looked.
#[test]
fn verify_says_when_the_music_folder_isnt_beside_the_zip() {
    stamped();
    let f = fixture();
    let host = host(&f);
    run(&host, &f, "backup", json!({}), json!({ "music": true })).unwrap();
    let folder = f.dest.join(FOLDER);
    std::fs::rename(&folder, f.path("moved")).unwrap();
    let checked = as_view(run(&host, &f, "verify", json!({ "file": f.zip() }), json!({})).unwrap());
    let View::Sections { items, .. } = checked else { panic!("{checked:?}") };
    let View::Kv { pairs, .. } = &items[0].view else { panic!("{items:?}") };
    let music = pairs.iter().find(|p| p.label == "Music").unwrap();
    assert_eq!(music.value, json!(format!("3 files, but {} isn't beside the zip", folder.display())));
    host.stop();
}

/// However a zip's entries are laid out (some of `master.db`, `contents.xml`, a library file and a
/// folder, in any order), `verify` says each way the layout differs from rekordbox's, in the order
/// it reads them, and nothing else: one laid out as rekordbox lays it out is whole.
#[hegel::test(test_cases = 40, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_verify_says_each_way_a_layout_is_wrong(tc: TestCase) {
    stamped();
    let f = fixture();
    let host = host(&f);
    let xml = contents_xml(&[]);
    let pool: [(&str, Option<&[u8]>); 4] = [
        ("master.db", Some(MASTER_DB)),
        ("contents.xml", Some(&xml)),
        ("masterPlaylists6.xml", Some(b"x")),
        ("share/", None),
    ];
    let mut entries: Vec<(&str, Option<&[u8]>)> = pool.into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    for i in (1..entries.len()).rev() {
        entries.swap(i, tc.draw(gs::integers::<usize>().max_value(i)));
    }
    let zip = f.dest.join("rekordbox_bak_20260101.zip");
    zip_of(&zip, &entries);

    let names: Vec<&str> = entries.iter().map(|(n, _)| *n).collect();
    let mut expected: Vec<String> = names
        .iter()
        .filter(|n| n.ends_with('/'))
        .map(|n| format!("it has a folder entry, {n}, which rekordbox's backups don't"))
        .collect();
    if names.first() != Some(&"master.db") {
        expected.push("master.db isn't its first entry".into());
    }
    if names.last() != Some(&"contents.xml") {
        expected.push("contents.xml isn't its last entry".into());
    }
    if !names.contains(&"contents.xml") {
        expected.push("it has no contents.xml that reads".into());
    } else if names.first() != Some(&"master.db") {
        // Only a first master.db is unpacked for the host to open.
        expected.push("it has no master.db".into());
    }
    if expected.is_empty() {
        run(&host, &f, "verify", json!({ "file": zip }), json!({})).unwrap();
    } else {
        assert_eq!(problems(&host, &f, &zip), expected, "{names:?}");
    }
    host.stop();
}
