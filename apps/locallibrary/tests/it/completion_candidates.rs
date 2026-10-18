//! What Tab offers for each kind of argument (`COMPLETE=fish locallibrary -- ...`): what's on
//! the mounted drives, what's in the store, and what locallibrary itself knows.

use std::process::Command;

use crate::support::{Env, play};

/// The demo drive as `A`, with a history list a player recorded, a My Tag and a playlist.
fn with_a_drive() -> Env {
    let env = Env::new();
    let a = env.demo("A");
    play(&a, "HISTORY 001", &[1, 2]);
    env.ok(&["--yes", "tags", "add", "A", "Genre/Deep", "--track", "2"]);
    env.ok(&["--yes", "playlist", "new", "A", "Friday", "--track", "1"]);
    env
}

/// What Tab offers after `words`: each candidate and its description.
fn complete(env: &Env, words: &[&str]) -> Vec<(String, String)> {
    let out = env.command().env("COMPLETE", "fish").arg("--").arg("locallibrary").args(words).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    text.lines()
        .map(|l| {
            let (value, help) = l.split_once('\t').unwrap_or((l, ""));
            (value.to_string(), help.to_string())
        })
        .collect()
}

/// The candidate `value`'s description, if it's offered.
fn offered<'a>(candidates: &'a [(String, String)], value: &str) -> Option<&'a str> {
    candidates.iter().find(|(v, _)| v == value).map(|(_, h)| h.as_str())
}

#[test]
fn what_is_on_the_drives_completes() {
    let env = with_a_drive();
    // Nothing on the command line names a drive: every mounted one's.
    let playlists = complete(&env, &["pack", "--playlist", ""]);
    assert_eq!(offered(&playlists, "Friday"), Some("1 tracks"), "{playlists:?}");
    let tags = complete(&env, &["tags", "add", "A", ""]);
    assert_eq!(offered(&tags, "Genre"), Some("category"), "{tags:?}");
    assert_eq!(offered(&tags, "Genre/Deep"), Some("1 tracks"), "{tags:?}");
    let lists = complete(&env, &["plays", "export", "A", ""]);
    assert_eq!(offered(&lists, "HISTORY 001"), Some("2 tracks"), "{lists:?}");
}

#[test]
fn what_is_in_the_store_completes() {
    let env = with_a_drive();
    assert!(complete(&env, &["pull", "--library", ""]).is_empty(), "no store yet");
    env.ok(&["--yes", "drive", "adopt", "A", "--name", "Main"]);
    env.ok(&["pull", "Main", "--library", "Club", "--new-library"]);
    assert_eq!(offered(&complete(&env, &["pull", "--library", ""]), "Club"), Some(""));

    // Sessions by their short id, then the set made of one.
    let sessions = complete(&env, &["sets", "new", "--from", ""]);
    let (id, about) = sessions.first().unwrap_or_else(|| panic!("no sessions"));
    assert!(about.starts_with("HISTORY 001, 2 tracks, "), "{sessions:?}");
    let set = env.json(&["sets", "new", "--from", id, "--title", "Friday"]);
    let set_id = set["report"]["set"]["id"].as_str().unwrap_or_else(|| panic!("{set}"));
    let sets = complete(&env, &["sets", "edit", ""]);
    assert!(offered(&sets, set_id).is_some_and(|d| d.contains("Friday")), "{sets:?}");

    // A drive the store knows that isn't plugged in.
    std::fs::rename(env.volumes.join("A"), env.outside("A")).unwrap();
    let drives = complete(&env, &["sync", ""]);
    assert_eq!(offered(&drives, "Main"), Some("not mounted"), "{drives:?}");
}

#[test]
fn what_locallibrary_knows_completes() {
    let env = Env::new();
    let models = complete(&env, &["players", "set", ""]);
    assert!(offered(&models, "CDJ-3000").is_some(), "{models:?}");
    let steps = complete(&env, &["refresh", "--skip", ""]);
    let ids: Vec<&str> = steps.iter().map(|(v, _)| v.as_str()).collect();
    assert!(ids.contains(&"pull") && !ids.contains(&"check") && !ids.contains(&"eject"), "{steps:?}");
}

/// The script has the shell run this binary on Tab: by name when it was found on the PATH, by its
/// full path otherwise.
#[test]
fn the_script_runs_the_binary_the_shell_found() {
    let env = Env::new();
    let script = |c: &mut Command| String::from_utf8(c.args(["completions", "bash"]).output().unwrap().stdout).unwrap();
    // Looked up when the test runs, not built in, so it holds when nextest runs the tests from an
    // archive somewhere else.
    let exe = assert_cmd::cargo::cargo_bin("locallibrary");
    // The script quotes the path for its shell, which escapes a Windows path's backslashes.
    #[cfg(unix)]
    assert!(script(&mut env.command()).contains(exe.to_str().unwrap()));
    let mut by_name = Command::new("locallibrary");
    by_name
        .env("PATH", exe.parent().unwrap())
        .env("LOCALLIBRARY_STORE", &env.store)
        .env("LOCALLIBRARY_VOLUMES", &env.volumes);
    let found = script(&mut by_name);
    assert!(found.contains("locallibrary") && !found.contains(exe.to_str().unwrap()), "{found}");
}

/// Where a library's name is taken, Tab offers the store's libraries beside the drives; an
/// argument about one particular drive offers drives only.
#[test]
fn libraries_complete_where_a_library_is_taken() {
    let env = with_a_drive();
    env.ok(&["--yes", "drive", "adopt", "A", "--name", "Main"]);
    env.ok(&["pull", "Main", "--library", "Club", "--new-library"]);
    for words in [&["playlist", "new", ""][..], &["sync", ""], &["mix", "--drive", ""]] {
        let offers = complete(&env, words);
        assert_eq!(offered(&offers, "Club"), Some("library"), "{words:?}: {offers:?}");
        assert!(offered(&offers, "Main").is_some(), "{words:?}: {offers:?}");
    }
    for words in [&["drive", "rename", ""][..], &["settings", "set", "quantize=off", "--drive", ""]] {
        let offers = complete(&env, words);
        assert!(offered(&offers, "Main").is_some() && offered(&offers, "Club").is_none(), "{words:?}: {offers:?}");
    }
}
