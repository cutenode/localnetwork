//! The tracklist plugin locallibrary ships, on real drives: `playlist import` through the plugin
//! host, asking about an entry that could be several tracks (`--choose N=ID` answering). These
//! were the library's tests of its own import, before the plugin took it over.

use std::path::Path;

use locallibrary::Library;
use locallibrary::onelibrary::OneLibrary;

use crate::support::{Env, PDB, path};

fn pdb_library(root: &Path) -> Library {
    locallibrary::pdb::read_library(&root.join(PDB)).unwrap()
}

/// Track file paths of a playlist, in order.
fn files(lib: &Library, playlist: &str) -> Vec<String> {
    let p = lib.find_playlist(playlist).unwrap_or_else(|| panic!("no playlist {playlist}"));
    p.track_ids.iter().map(|id| lib.track(*id).unwrap().file_path.clone()).collect()
}

/// Two tracks of a Spotify export (via Exportify) and one the drive doesn't have: the playlist
/// lands in both databases.
#[test]
fn a_spotify_csv_makes_a_playlist_in_both_databases() {
    let env = Env::new();
    let root = env.club("Club");
    let d = path(&root);
    env.ok(&["--yes", "convert", d, "--to", "onelibrary"]);
    let lib = pdb_library(&root);
    let (a, b) = (&lib.tracks[5], &lib.tracks[7]);
    let csv = format!(
        "\"Track Name\",\"Artist Name(s)\",\"Duration (ms)\"\n\"{}\",\"{}\",{}\n\"{}\",\"{}\",{}\n\"Not on the drive\",\"Nobody\",1000\n",
        a.title,
        a.artist,
        a.duration_secs * 1000,
        b.title,
        b.artist,
        b.duration_secs * 1000
    );
    let file = env.outside("spotify.csv");
    std::fs::write(&file, csv).unwrap();
    let import = ["playlist", "import", d, path(&file), "--into", "From Spotify"];
    let planned = env.json(&[&import[..], &["--dry-run"]].concat());
    let summary = planned["plan"]["summary"][0].as_str().unwrap();
    assert!(summary.ends_with(": 3 entr(ies) as Csv: 2 found, 0 ambiguous, 1 missing"), "{planned}");
    env.ok(&[&["--yes"], &import[..]].concat());
    let plus = OneLibrary::open_root(&root).unwrap().library().unwrap();
    for l in [pdb_library(&root), plus] {
        assert_eq!(files(&l, "From Spotify"), [a.file_path.clone(), b.file_path.clone()]);
    }
}

/// A list that played a track twice keeps both, an entry whose length is off is asked about (and
/// counts as found once chosen, left out with `null`), and `--into` fills exactly its path, never
/// a playlist of that name in a folder.
#[test]
fn an_import_keeps_repeats_and_fills_exactly_its_path() {
    let env = Env::new();
    let root = env.club("Club");
    let d = path(&root);
    let lib = pdb_library(&root);
    let (a, b) = (&lib.tracks[5], &lib.tracks[7]);
    env.ok(&["--yes", "playlist", "new", d, "Shows", "--folder"]);
    env.ok(&["--yes", "playlist", "new", d, "Shows/Friday", "--track", &b.id.to_string()]);
    let m3u = format!(
        "#EXTM3U\n/Users/me/Music{a}\n/Users/me/Music{b}\n/Users/me/Music{a}\n#EXTINF:5000,{title}\n/nowhere/x.mp3\n",
        a = a.file_path,
        b = b.file_path,
        title = b.title
    );
    let file = env.outside("set.m3u8");
    std::fs::write(&file, m3u).unwrap();
    let choose = format!("4={}", b.id);
    let import = ["--yes", "playlist", "import", d, path(&file), "--into", "Friday"];
    // Without a terminal, the question is how to answer it.
    let asked = env.fails(2, &import);
    assert!(asked.contains("answer with --choose 4=VALUE, VALUE one of:"), "{asked}");
    let candidate = format!("  {:<4}  #{} {}", b.id, b.id, b.display_name());
    for item in [candidate.as_str(), "  null  leave it out"] {
        assert!(asked.contains(item), "{item}: {asked}");
    }
    let planned = env.json(&[&import[1..], &["--choose", &choose, "--dry-run"]].concat());
    assert!(planned["plan"]["summary"][0].as_str().unwrap().ends_with("4 found, 0 ambiguous, 0 missing"), "{planned}");
    env.ok(&[&import[..], &["--choose", &choose]].concat());
    let (fa, fb) = (a.file_path.as_str(), b.file_path.as_str());
    let lib = pdb_library(&root);
    assert_eq!(files(&lib, "Shows/Friday"), [fb], "the nested namesake is left alone");
    let top = lib.children(None).into_iter().find(|p| p.name == "Friday").unwrap();
    assert_eq!(lib.playlist_path(top.id), "Friday");
    assert_eq!(files(&lib, "Friday"), [fa, fb, fa, fb]);
    let out = env.ok(&[&import[..], &["--append", "--choose", "4=null"]].concat());
    assert!(out.contains(": left out"), "{out}");
    assert_eq!(files(&pdb_library(&root), "Friday"), [fa, fb, fa, fb, fa, fb, fa]);
}

/// A rekordbox XML playlist is the one of exactly its path (`--from`): of two whose paths differ
/// only in case, asking for neither is refused.
#[test]
fn an_xml_playlist_is_the_one_of_exactly_its_path() {
    let env = Env::new();
    let root = env.club("Club");
    let d = path(&root);
    let lib = pdb_library(&root);
    let (a, b) = (&lib.tracks[5], &lib.tracks[7]);
    let location = |t: &locallibrary::Track| format!("file://localhost/Volumes/USB{}", t.file_path.replace(' ', "%20"));
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <COLLECTION Entries="2">
    <TRACK TrackID="1" Name="a" Location="{}"/>
    <TRACK TrackID="2" Name="b" Location="{}"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Name="Set" Type="1" KeyType="0" Entries="1"><TRACK Key="1"/></NODE>
      <NODE Name="SET" Type="1" KeyType="0" Entries="1"><TRACK Key="2"/></NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#,
        location(a),
        location(b)
    );
    let file = env.outside("rekordbox.xml");
    std::fs::write(&file, xml).unwrap();
    let import = ["--yes", "playlist", "import", d, path(&file), "--into", "Imported", "--from"];
    env.ok(&[&import[..], &["SET"]].concat());
    assert_eq!(files(&pdb_library(&root), "Imported"), [b.file_path.as_str()]);
    let err = env.fails(1, &[&import[..], &["set"]].concat());
    assert!(err.contains("not found: playlist \"set\""), "{err}");
}

/// A choice must name an entry the list has; refused, nothing is made. One naming a track the
/// drive hasn't leaves its entry out, saying so.
#[test]
fn a_bad_choice_is_refused() {
    let env = Env::new();
    let root = env.club("Club");
    let d = path(&root);
    let list = env.outside("list.txt");
    std::fs::write(&list, "Nobody - Nothing\n").unwrap();
    let import = ["--yes", "playlist", "import", d, path(&list), "--into", "Set", "--choose"];
    let r = env.run(&[&import[..], &["1=999"]].concat());
    let said = format!("{}{}", r.out, r.err);
    assert_eq!(r.code, 0, "{said}");
    assert!(said.contains("1. Nobody - Nothing isn't on the drive"), "{said}");
    let first = pdb_library(&root).tracks[0].id;
    let err = env.fails(1, &[&import[..], &[format!("6={first}").as_str()]].concat());
    assert!(err.contains("no entry 6 in the list"), "{err}");
    assert!(pdb_library(&root).find_playlist("Set").is_none(), "nothing was made");
}
