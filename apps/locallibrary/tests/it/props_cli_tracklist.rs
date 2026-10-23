//! Properties of `playlist import`, the tracklist plugin's, on the twelve-track drive: what's found
//! is what loclib-tracklist finds in the drive's library, with what's chosen (`--choose N=ID`) for
//! an entry that could be several, the playlist then holds it, and choices that name nothing are
//! refused. These were the library's properties of its own import, before the plugin took it over.

use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::Library;
use locallibrary::device::Device;
use locallibrary::import::{self, Status};
use serde_json::{Value, json};

use crate::support::{Env, path};

fn library(root: &Path) -> Library {
    Device::open(root).unwrap().library().unwrap()
}

/// The error kind `--json` gives for `args`, which must fail.
fn refused(env: &Env, args: &[&str]) -> Value {
    let r = env.run(&[&["--json"], args].concat());
    assert_eq!(r.code, 1, "{args:?}\n{}\n{}", r.out, r.err);
    serde_json::from_str::<Value>(&r.out).unwrap()["error"]["kind"].clone()
}

/// A list drawn from the drive's tracks and two it doesn't have, each entry that could be several
/// tracks answered with any of them or none (`--choose N=ID`, `N=null`): `playlist import` fills
/// the playlist with the tracks loclib-tracklist finds in the drive's library and those chosen, in
/// the list's order (after its entries, appending), or changes nothing when there are none. A
/// choice for an entry the list doesn't have, or of a track the drive lacks, is refused.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn imports_fill_the_playlist_with_what_is_found(tc: TestCase) {
    let env = Env::new();
    let root = env.club("Club");
    let d = path(&root);
    let lib = library(&root);
    let mut names: Vec<String> = lib.tracks.iter().map(|t| t.display_name()).collect();
    names.extend(["Nobody - Nothing".to_string(), "Unknown Artist - Untitled".to_string()]);
    let lines = tc.draw(gs::vecs(gs::sampled_from(names)).min_size(1).max_size(6));
    let file = env.outside("set.txt");
    std::fs::write(&file, lines.join("\n")).unwrap();
    let into = tc.draw(gs::sampled_from(vec![
        "Imported".to_string(),
        lib.playlist_path(lib.playlists.iter().find(|p| !p.is_folder).unwrap().id),
    ]));
    let append = tc.draw(gs::booleans());

    let entries = import::parse(lines.join("\n").as_bytes(), Some("set.txt")).unwrap();
    let matched = import::match_entries(&lib, &entries);
    let (mut found, mut answers) = (Vec::new(), Vec::new());
    for (i, m) in matched.iter().enumerate() {
        match &m.status {
            Status::Found { track_id, .. } => found.push(*track_id),
            Status::Ambiguous { candidates } => {
                let chosen = match candidates.is_empty() {
                    true => None,
                    false => tc.draw(gs::optional(gs::sampled_from(candidates.clone()))),
                };
                found.extend(chosen);
                let value = chosen.map_or("null".to_string(), |id| id.to_string());
                answers.extend(["--choose".to_string(), format!("{}={value}", i + 1)]);
            }
            Status::Missing => {}
        }
    }
    let existing: Vec<u32> =
        lib.find_playlist(&into).filter(|_| append).map(|p| p.track_ids.clone()).unwrap_or_default();
    let mut import = vec!["--yes", "playlist", "import", d, path(&file), "--into", &into];
    if append {
        import.push("--append");
    }
    import.extend(answers.iter().map(String::as_str));
    let out = env.ok(&import);
    let after = library(&root);
    match found.is_empty() {
        true => {
            assert!(out.contains("nothing to change"), "{out}");
            assert_eq!(
                after.find_playlist(&into).map(|p| &p.track_ids),
                lib.find_playlist(&into).map(|p| &p.track_ids)
            );
        }
        false => assert_eq!(after.find_playlist(&into).unwrap().track_ids, [existing, found].concat()),
    }
    let n = matched.len();
    let choose = |entry: usize, track: u32| {
        let choice = format!("{entry}={track}");
        refused(&env, &["playlist", "import", d, path(&file), "--into", &into, "--choose", &choice])
    };
    assert_eq!(choose(n + 1, lib.tracks[0].id), "invalid");
    assert_eq!(choose(1, 9999), "not_found");
}

/// The twelve-track drive's playlist, and the rekordbox XML of its library: importing that
/// playlist from the XML (into a new playlist, or onto the end of one, or replacing it) finds each
/// of its tracks, in order, or the track chosen for an entry (`--choose N=ID`, which wins over
/// what was found), and the playlist then holds those after what it kept; a playlist the XML lacks
/// isn't found. A list of tracks the drive doesn't
/// have changes nothing, saying so.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(onto_itself = true, append = true, choice = None::<usize>)]
fn an_xml_playlist_imports_as_the_tracks_it_names(tc: TestCase) {
    let onto_itself = tc.draw(gs::booleans());
    let append = tc.draw(gs::booleans());
    let choice = tc.draw(gs::optional(gs::integers::<usize>().max_value(20)));

    let env = Env::new();
    let root = env.club("Club");
    let d = path(&root);
    let lib = library(&root);
    let list = lib.playlists.iter().find(|p| !p.is_folder && !p.track_ids.is_empty()).expect("a playlist");
    let name = lib.playlist_path(list.id);
    let file = env.outside("collection.xml");
    env.ok(&["xml", d, "-o", path(&file)]);
    let into = if onto_itself { name.clone() } else { "Imported".to_string() };
    // A choice of another track for an entry the list has.
    let chosen = choice.filter(|i| *i < list.track_ids.len()).map(|i| {
        let other = lib.tracks.iter().find(|t| t.id != list.track_ids[i]).unwrap().id;
        (i, other)
    });
    let from = format!("/{name}/");
    let mut args = vec![path(&file).to_string(), "--from".into(), from.clone()];
    if let Some((i, t)) = chosen {
        args.extend(["--choose".into(), format!("{}={t}", i + 1)]);
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let mut want: Vec<u32> = list.track_ids.clone();
    if let Some((i, t)) = chosen {
        want[i] = t;
    }
    let planned = env.json(&[&["playlist", "import", d], &args[..], &["--into", &into, "--dry-run"]].concat());
    let n = list.track_ids.len();
    let said = format!(": {n} entr(ies) as RekordboxXml: {n} found, 0 ambiguous, 0 missing");
    assert!(planned["plan"]["summary"][0].as_str().unwrap().ends_with(&said), "{planned}");
    let kept: Vec<u32> = if append && onto_itself { list.track_ids.clone() } else { Vec::new() };
    let mut import = [&["--yes", "playlist", "import", d], &args[..], &["--into", &into]].concat();
    if append {
        import.push("--append");
    }
    env.ok(&import);
    assert_eq!(library(&root).find_playlist(&into).unwrap().track_ids, [kept, want].concat());

    let nowhere = ["playlist", "import", d, path(&file), "--from", "Nowhere", "--into", &into];
    assert_eq!(refused(&env, &nowhere), "not_found");
    let nothing = env.outside("nothing.txt");
    std::fs::write(&nothing, "Nobody - Nothing\nNo One - Nowhere\n").unwrap();
    let v = env.json(&["playlist", "import", d, path(&nothing), "--into", &into]);
    assert_eq!((&v["applied"], &v["outcome"]), (&json!(false), &json!("nothing_to_do")), "{v}");
    assert!(v["plan"]["warnings"].as_array().unwrap().contains(&json!("none of the list's tracks are on the drive")));
}
