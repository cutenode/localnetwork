//! `tracks`, `track show` and `playlists`: listings laid out for reading, and kept whole where
//! programs read them (TSV, CSV).

use crate::support::{Env, PDB, line};

/// A table lines its columns up (numbers on the right) and cuts long values short; TSV and CSV
/// keep every value whole and quote what needs it.
#[test]
fn track_listings_line_up_and_keep_values_whole_where_programs_read_them() {
    let env = Env::new();
    env.demo("A");
    let title = "A Title Long Enough to Be Cut Short, With \"Quotes\" in It";
    env.ok(&["--yes", "track", "set", "A", "--track", "1", "--title", title, "--comment", "tab\there"]);

    let table = env.ok(&["tracks", "A"]);
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(lines.len(), 4, "a header, two tracks and the count: {table}");
    let width = |l: &str| l.chars().count();
    assert!(lines[..3].iter().all(|l| width(l) == width(lines[0])), "the time column ends every line:\n{table}");
    assert!(line(&table, "A Title").starts_with(" 1  Loopmasters - A Title Long"), "{table}");
    assert!(line(&table, "A Title").contains("Cut Short…  128.00"), "{table}");
    assert!(!table.contains("in It"), "cut at 50 columns: {table}");
    assert_eq!(lines[3], "2 track(s), 5m 00s");
    let ascii = env.ok(&["--ascii", "tracks", "A"]);
    assert!(ascii.contains("...") && !ascii.contains('…'), "{ascii}");
    assert_eq!(env.ok(&["tracks", "A", "-n", "1"]).lines().last(), Some("1 of 2 track(s)"));

    let csv = env.ok(&["tracks", "A", "--format", "csv", "--columns", "id,title"]);
    assert_eq!(line(&csv, "1,"), "1,\"A Title Long Enough to Be Cut Short, With \"\"Quotes\"\" in It\"");
    let tsv = env.ok(&["tracks", "A", "--format", "tsv", "--columns", "id,comment"]);
    assert_eq!(line(&tsv, "1\t"), "1\ttab here", "a tab in a value would start a new column");

    // Several drives: one listing, each row naming its drive.
    env.demo("B");
    let both = env.ok(&["tracks", "--format", "tsv", "--columns", "id"]);
    let mut rows: Vec<&str> = both.lines().collect();
    assert_eq!(rows.remove(0), "drive\tid");
    rows.sort();
    assert_eq!(rows, ["A\t1", "A\t2", "B\t1", "B\t2"]);
}

/// Every column a listing can show, under its own header.
#[test]
fn every_column_shows_its_value() {
    let env = Env::new();
    env.demo("A");
    let set = ["--yes", "track", "set", "A", "--track", "2", "--rating", "4", "--color", "red", "--genre", "House"];
    env.ok(&[&set[..], &["--album-name", "Night", "--year", "2020", "--play-count", "3"]].concat());
    let columns = [
        "id", "name", "title", "artist", "album", "genre", "label", "key", "bpm", "rating", "color", "year", "plays",
        "added", "time", "bitrate", "path", "comment",
    ];
    let tsv = env.ok(&["tracks", "A", "--format", "tsv", "--columns", &columns.join(",")]);
    let rows: Vec<Vec<&str>> = tsv.lines().map(|l| l.split('\t').collect()).collect();
    assert_eq!(rows[0], columns);
    let row: rustc_hash::FxHashMap<&str, &str> = columns.iter().copied().zip(rows[2].iter().copied()).collect();
    let expected = [
        ("id", "2"),
        ("name", "Loopmasters - Demo Track 2"),
        ("title", "Demo Track 2"),
        ("artist", "Loopmasters"),
        ("album", "Night"),
        ("genre", "House"),
        ("label", "Loopmasters"),
        ("key", "Fm"),
        ("bpm", "120.00"),
        ("rating", "4"),
        ("color", "red"),
        ("year", "2020"),
        ("plays", "3"),
        ("added", "2018-05-25"),
        ("time", "2:08"),
        ("bitrate", "320"),
        ("path", "/Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3"),
        ("comment", "Tracks by www.loopmasters.com"),
    ];
    for (column, value) in expected {
        assert_eq!(row[column], value, "{column}");
    }
    // What's not set is left blank, not 0.
    let first: Vec<&str> = rows[1].clone();
    assert_eq!((first[9], first[10], first[11]), ("", "", ""), "{tsv}");
}

#[test]
fn a_tracks_details_say_what_a_dj_looks_for() {
    let env = Env::new();
    let a = env.demo("A");
    let set =
        ["--yes", "track", "set", "A", "--track", "2", "--rating", "4", "--color", "red", "--album-name", "Night"];
    env.ok(&[&set[..], &["--year", "2020", "--remixer", "Someone", "--mix", "Extended", "--play-count", "3"]].concat());
    env.ok(&["--yes", "playlist", "new", "A", "Sets/Friday", "--track", "2"]);
    env.ok(&["--yes", "playlist", "new", "A", "Warmup", "--track", "2"]);
    env.ok(&["--yes", "tags", "add", "A", "Genre/Deep", "--track", "2"]);
    // A player played it; and a track with a key that isn't one and no analysis.
    let pdb = a.join(PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    let list = ed.create_history_playlist("HISTORY 001").unwrap();
    ed.add_to_history(list, 2).unwrap();
    let odd =
        rekordbox_pdb::NewTrack { key: Some("Q".into()), ..rekordbox_pdb::NewTrack::new("Odd", "/Contents/odd.mp3") };
    let odd = ed.add_track(&odd).unwrap().to_string();
    ed.save(&pdb).unwrap();
    std::fs::write(a.join("Contents/odd.mp3"), "audio").unwrap();

    let shown = env.ok(&["track", "show", "A", "--track", "2"]);
    assert!(shown.starts_with("#2  Loopmasters - Demo Track 2\n"), "{shown}");
    for fact in [
        "album       Night · Someone · Extended · 2020",
        "tempo       120.00 BPM · 4A Fm · 2:08",
        "rating      ★★★★☆",
        "colour      red",
        "format      MP3 · 320 kbps · 44.1 kHz · 16-bit · 5.1 MB",
        "plays       played 3× · added 2018-05-25",
        "playlists   Sets/Friday\n              Warmup",
        "history     HISTORY 001",
        "my tags     Genre/Deep",
        "beat grid   257 beats · 65 bars · steady tempo",
        "cues        0 hot · 0 memory",
        "phrases     mid mood: Intro 8 · ",
    ] {
        assert!(shown.contains(fact), "{fact:?} in:\n{shown}");
    }
    // Phrases wrap under their label, a phrase never split, and end with the unit.
    let phrases: Vec<&str> = shown.lines().skip_while(|l| !l.contains("phrases")).collect();
    assert!(phrases.len() > 1 && phrases.iter().all(|l| l.chars().count() <= 14 + 64 + 8), "{shown}");
    assert!(phrases[1].starts_with(&" ".repeat(14)) && phrases.last().unwrap().ends_with("(bars)"), "{shown}");
    let ascii = env.ok(&["--ascii", "track", "show", "A", "--track", "2"]);
    assert!(ascii.contains("rating      ****.") && ascii.contains("played 3x"), "{ascii}");

    let odd = env.ok(&["track", "show", "A", "--track", &odd]);
    assert!(odd.contains("tempo       Q · 0:00\n") && odd.ends_with("analysis    none on the drive\n"), "{odd}");

    // Two tracks, a blank line between them; one with no audio says so first.
    std::fs::remove_file(a.join("Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3")).unwrap();
    let both = env.ok(&["track", "show", "A", "--track", "1", "--track", "2"]);
    let first = both.split("\n\n").next().unwrap();
    assert!(first.lines().nth(1).unwrap().contains("the audio file isn't on the drive"), "{both}");
    assert!(first.contains("never played") && first.contains("playlists   none (unfiled)"), "{both}");
    assert!(both.contains("\n\n#2  "), "{both}");
}

#[test]
fn playlists_are_a_tree() {
    let env = Env::new();
    env.demo("A");
    assert_eq!(env.ok(&["playlists", "A"]).trim(), "no playlists");
    env.ok(&["--yes", "playlist", "new", "A", "Sets/Friday", "--track", "1", "--track", "2"]);
    env.ok(&["--yes", "playlist", "new", "A", "Warmup", "--track", "1"]);
    let tree = env.ok(&["playlists", "A"]);
    assert_eq!(tree.lines().collect::<Vec<_>>(), ["Sets/", "  Friday (2)", "Warmup (1)"]);
}
