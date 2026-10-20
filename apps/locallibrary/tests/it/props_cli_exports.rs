//! Property tests of what's written out of a library: `export` (playlists, folders and history
//! lists as M3U8, CSV or text, and libraries for Traktor, Serato and Engine DJ, the interop
//! plugin's), rekordbox's own
//! collection (`collection`: what's in it, what a drive lacks against it, exporting it), and
//! `analysis bench`. Printed for people, each says what its `--json` does, and what it writes is
//! what it said it would.

use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, path};

/// Every file under `dir`.
fn files_under(dir: &Path) -> Vec<std::path::PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect()
}

/// A playlist, folder, history list or whole library of drive A or Club, written in any list
/// format, as one file or a file per playlist, with paths any way: each file the JSON reports is
/// there with its tracks (one line each, as text), and printed, the run names each; and the
/// flags only plugins' formats take are refused.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn playlists_are_written_as_reported(tc: TestCase) {
    let fx = Env::from_template();
    let drive = tc.draw(gs::sampled_from(&["A", "Club"][..]));
    let mut args: Vec<String> = vec!["export".into(), drive.into()];
    let what = tc.draw(gs::integers::<u8>().max_value(2));
    match what {
        0 => {}
        1 => args.push(if drive == "A" { "Set" } else { "aab" }.into()),
        _ => args.extend(["--history".into(), "HISTORY 001".into()]),
    }
    let format = tc.draw(gs::sampled_from(&[None, Some("m3u8"), Some("csv"), Some("txt"), Some("text")][..]));
    let ext = tc.draw(gs::sampled_from(&["m3u8", "csv", "txt", "list"][..]));
    let split = tc.draw(gs::weighted_booleans(0.3));
    let out = if split { fx.home("lists") } else { fx.home(&format!("out.{ext}")) };
    args.extend(["-o".into(), path(&out).into()]);
    if let Some(f) = format {
        args.extend(["--format".into(), f.into()]);
    }
    if split {
        args.push("--split".into());
    }
    args.extend(["--paths".into(), tc.draw(gs::sampled_from(&["relative", "drive", "absolute"][..])).into()]);
    let foreign: Option<&[&str]> =
        tc.draw(gs::sampled_from(&[None, None, Some(&["--dry-run"][..]), Some(&["--option", "a=b"][..])][..]));
    if let Some(f) = foreign {
        args.extend(f.iter().map(|a| a.to_string()));
    }
    let json = tc.draw(gs::booleans());
    let mut all: Vec<&str> = if json { vec!["--json"] } else { vec![] };
    all.extend(args.iter().map(String::as_str));
    let r = fx.run(&all);
    let said = format!("{all:?} exited {}\n{}\n{}", r.code, r.out, r.err);
    if foreign.is_some() {
        assert_eq!(r.code, 1, "{said}");
        assert!(!out.exists(), "{said}");
        return;
    }
    if r.code != 0 {
        // A history list Club doesn't have, or a playlist that isn't a folder split.
        assert!(!out.exists() || split, "{said}");
        return;
    }
    let files: Vec<(String, String, u64)> = if json {
        let v: Value = serde_json::from_str(&r.out).unwrap();
        v["report"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (f["path"].as_str().unwrap().into(), f["name"].as_str().unwrap().into(), f["tracks"].as_u64().unwrap())
            })
            .collect()
    } else {
        r.out
            .lines()
            .filter_map(|l| l.strip_prefix("✓ wrote "))
            .map(|l| {
                let (p, rest) = l.rsplit_once(" (").unwrap();
                let (name, n) = rest.trim_end_matches(" tracks)").rsplit_once(", ").unwrap();
                (p.to_string(), name.to_string(), n.parse().unwrap())
            })
            .collect()
    };
    if files.is_empty() {
        assert!(r.out.contains("no playlists there; nothing written") || json, "{said}");
    }
    let written = if split { files_under(&out).len() } else { usize::from(out.exists()) };
    assert_eq!(written, files.len(), "{said}");
    for (p, _, tracks) in &files {
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{p}: {e}\n{said}"));
        let as_text = format.is_some_and(|f| f == "txt" || f == "text") || (format.is_none() && p.ends_with(".txt"));
        if as_text {
            assert_eq!(text.lines().filter(|l| !l.is_empty()).count() as u64, *tracks, "{p}:\n{text}");
        }
        if format == Some("m3u8") || (format.is_none() && !p.ends_with(".csv") && !p.ends_with(".txt")) {
            assert_eq!(text.lines().filter(|l| l.starts_with("#EXTINF")).count() as u64, *tracks, "{p}:\n{text}");
        }
    }
    // Written again: refused over what's there, unless replacing.
    let again = fx.run(&all);
    assert_eq!(again.code, 1, "{}\n{}", again.out, again.err);
    let replace = [&all[..], &["--replace"]].concat();
    assert_eq!(fx.run(&replace).code, 0);
}

/// Every file below `dir` but locallibrary's own state, with its bytes.
fn drive_files(dir: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut files: Vec<_> = files_under(dir)
        .into_iter()
        .filter(|f| !f.components().any(|c| c.as_os_str() == ".locallibrary"))
        .map(|f| {
            let bytes = std::fs::read(&f).unwrap();
            (f, bytes)
        })
        .collect();
    files.sort();
    files
}

/// Drive A (on OneLibrary too, or not; some of its audio taken away) or Club (no audio at all),
/// whole or the playlist holding every track, for Traktor, Serato or Engine DJ through the
/// interop plugin, with options drawn (some its format refuses): the plan printed, as a dry run
/// writing nothing, is the JSON's; it counts every track, warns of those without audio, and
/// carries no analysis when told not to. Applied, every file it lists is written at its size (a
/// tagged copy at no more), the drive is only read, and again it asks before overwriting unless
/// replacing.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn other_dj_software_gets_the_files_planned(tc: TestCase) {
    let fx = Env::from_template();
    let drive = tc.draw(gs::sampled_from(&["A", "Club"][..]));
    let root = fx.volumes.join(drive);
    let tracks = if drive == "A" { 2 } else { 12 };
    let mut gone = 0;
    if drive == "A" {
        if tc.draw(gs::weighted_booleans(0.3)) {
            fx.ok(&["--yes", "convert", "A", "--to", "onelibrary"]);
        }
        gone = tc.draw(gs::integers::<usize>().max_value(2));
        for n in 1..=gone {
            std::fs::remove_file(root.join(format!("Contents/Loopmasters/UnknownAlbum/Demo Track {n}.mp3"))).unwrap();
        }
    }
    let software = tc.draw(gs::sampled_from(&["traktor", "serato", "engine"][..]));
    let out = fx.home("for-dj");
    let music = path(&fx.home("music")).to_string();
    let mut args: Vec<String> = vec!["export".into(), drive.into()];
    if tc.draw(gs::booleans()) {
        args.push(if drive == "A" { "Set" } else { "aab" }.into());
    }
    args.extend(["--format".into(), software.into(), "-o".into(), path(&out).into()]);
    let mut options: Vec<String> = Vec::new();
    for (option, p) in [("skip_analysis=true", 0.3), ("write_tags=true", 0.3), ("paths=drive", 0.2)] {
        if tc.draw(gs::weighted_booleans(p)) {
            options.push(option.into());
        }
    }
    let elsewhere = tc.draw(gs::weighted_booleans(0.3));
    if elsewhere {
        options.push(format!("location_root={music}"));
    }
    for o in &options {
        args.extend(["--option".into(), o.clone()]);
    }
    if tc.draw(gs::weighted_booleans(0.2)) {
        args.extend(["--history".into(), "HISTORY 001".into()]);
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let dry = [&args[..], &["--dry-run"]].concat();
    let (planned, printed) = (fx.run(&[&["--json"][..], &dry].concat()), fx.run(&dry));
    let said = format!("{dry:?}\n{}\n{}\n{}", printed.out, printed.err, planned.out);
    assert_eq!(planned.code, printed.code, "{said}");
    assert!(!out.exists(), "a dry run writes nothing: {said}");
    if planned.code != 0 {
        return;
    }
    let v: Value = serde_json::from_str(&planned.out).unwrap();
    let p = &v["plan"];
    for line in p["summary"].as_array().unwrap() {
        assert!(printed.out.contains(&format!("• {}\n", line.as_str().unwrap())), "{line}: {said}");
    }
    let files = p["files"].as_array().unwrap();
    for f in files.iter().take(20) {
        assert!(printed.out.contains(f["path"].as_str().unwrap()), "{f}: {said}");
    }
    assert_eq!(
        printed.out.contains(&format!("... and {} more", files.len().saturating_sub(20))),
        files.len() > 20,
        "{said}"
    );
    for n in p["notes"].as_array().unwrap().iter().chain(p["warnings"].as_array().unwrap()) {
        assert!(printed.out.contains(n.as_str().unwrap()), "{n}: {said}");
    }
    // Every track, those without audio where the files say warned of, analysis left out as told.
    let counts = &p["counts"];
    assert_eq!(counts["tracks"], tracks, "{said}");
    let missing = if elsewhere || drive == "Club" { tracks } else { gone };
    let warned =
        p["warnings"].as_array().unwrap().iter().filter_map(Value::as_str).find(|w| w.contains("have no audio"));
    assert_eq!(warned.map(|w| w.starts_with(&format!("{missing} track(s)"))), (missing > 0).then_some(true), "{said}");
    if options.iter().any(|o| o == "skip_analysis=true") {
        assert_eq!((&counts["beat grids"], &counts["cues"]), (&Value::from(0), &Value::from(0)), "{said}");
    }
    if p["noop"] == true {
        return;
    }

    let before = drive_files(&root);
    let json = tc.draw(gs::booleans());
    let flags: &[&str] = if json { &["--json"] } else { &[] };
    let r = fx.run(&[flags, &args[..]].concat());
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    for f in files {
        let len = std::fs::metadata(f["path"].as_str().unwrap()).unwrap_or_else(|e| panic!("{f}: {e}")).len();
        let (tagged, bytes) =
            (f["what"].as_str().unwrap().ends_with("with Serato's cue tag"), f["bytes"].as_u64().unwrap());
        assert!(if tagged { len <= bytes } else { len == bytes }, "{f}: {len}");
    }
    assert_eq!(drive_files(&root), before, "the drive is only read");
    if !json {
        assert!(r.out.contains(&format!("wrote {} file(s) to {}", files.len(), out.display())), "{}", r.out);
    }
    // Again: asked before overwriting (JSON never prompts), unless replacing.
    assert_eq!(fx.run(&[&["--json"], &args[..]].concat()).code, 3);
    assert_eq!(fx.run(&[&args[..], &["--replace"]].concat()).code, 0);
}

/// rekordbox's collection (a `master.db` as plain SQLite with rekordbox 7's tables) of drawn
/// tracks, some deleted, with a playlist in a folder: `collection` printed says what its JSON
/// does, what drive A lacks against it is printed as listed, and exporting it to a folder plans
/// (and printed, shows) the copies it then makes.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn the_collection_prints_what_its_json_says(tc: TestCase) {
    let fx = Env::from_template();
    let db = fx.home("master.db");
    let music = fx.home("Music");
    std::fs::create_dir_all(&music).unwrap();
    let titles = ["Demo Track 1", "Never Grow Old", "Sanctified", "Bootleg"];
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TABLE djmdProperty (DBID TEXT, DBVersion TEXT);
         INSERT INTO djmdProperty VALUES ('1234567890', '7000');
         CREATE TABLE djmdArtist (ID TEXT PRIMARY KEY, Name TEXT);
         INSERT INTO djmdArtist VALUES ('1', 'Loopmasters'), ('2', 'Floorplan');
         CREATE TABLE djmdContent (ID TEXT PRIMARY KEY, FolderPath TEXT, FileNameL TEXT, Title TEXT,
             ArtistID TEXT, BPM INTEGER, Length INTEGER, FileSize INTEGER, rb_local_deleted INTEGER DEFAULT 0);
         CREATE TABLE djmdPlaylist (ID TEXT PRIMARY KEY, Seq INTEGER, Name TEXT, Attribute INTEGER, ParentID TEXT);
         INSERT INTO djmdPlaylist VALUES ('10', 1, 'Shows', 1, 'root'), ('11', 1, 'Friday', 0, '10');
         CREATE TABLE djmdSongPlaylist (ID TEXT PRIMARY KEY, PlaylistID TEXT, ContentID TEXT, TrackNo INTEGER);",
    )
    .unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(5));
    for id in 1..=n {
        let title = tc.draw(gs::sampled_from(&titles[..]));
        let artist = tc.draw(gs::sampled_from(&["1", "2"][..]));
        let file = music.join(format!("{id}.mp3"));
        std::fs::write(&file, format!("audio {id}").repeat(50)).unwrap();
        let bpm = tc.draw(gs::integers::<u32>().min_value(9000).max_value(15000));
        let deleted = tc.draw(gs::weighted_booleans(0.2)) as i32;
        conn.execute(
            "INSERT INTO djmdContent VALUES (?1, ?2, ?3, ?4, ?5, ?6, 300, 500, ?7)",
            (id.to_string(), path(&file), format!("{id}.mp3"), title, artist, bpm, deleted),
        )
        .unwrap();
        if tc.draw(gs::booleans()) {
            conn.execute("INSERT INTO djmdSongPlaylist VALUES (?1, '11', ?2, ?1)", (id as i64, id.to_string()))
                .unwrap();
        }
    }
    // An intelligent playlist (which isn't evaluated), and a track with no artist whose title is
    // one of drive A's: it only loosely matches it.
    if tc.draw(gs::booleans()) {
        conn.execute_batch("INSERT INTO djmdPlaylist VALUES ('12', 2, 'Recent', 4, 'root');").unwrap();
    }
    if tc.draw(gs::booleans()) {
        conn.execute(
            "INSERT INTO djmdContent VALUES ('99', ?1, 'Bootleg.mp3', 'Demo Track 2', NULL, 12000, 200, 100, 0)",
            [path(&music.join("Bootleg.mp3"))],
        )
        .unwrap();
        std::fs::write(music.join("Bootleg.mp3"), "bootleg".repeat(20)).unwrap();
    }
    drop(conn);
    let db = path(&db).to_string();
    let with = |more: &[&str]| -> Vec<String> {
        ["collection", "--db", &db].iter().chain(more).map(|s| s.to_string()).collect()
    };
    let both = |more: &[&str]| -> (Value, String) {
        let args = with(more);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        (fx.json(&args), fx.ok(&args))
    };

    let (info, out) = both(&[]);
    let s = &info["summary"];
    assert!(out.starts_with("rekordbox collection\n"), "{out}");
    assert!(out.contains(&format!("{} tracks by {} artists, ", s["tracks"], s["artists"])), "{out}\n{info}");
    assert!(
        out.contains(&format!(
            "  {} playlists in {} folders, {} history lists",
            s["playlists"], s["folders"], info["history_lists"]
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "  {} hot cues, {} memory cues, {} loops",
            info["hot_cues"], info["memory_cues"], info["loops"]
        )),
        "{out}"
    );
    let smart = info["smart_playlists"].as_array().unwrap();
    assert_eq!(out.contains("intelligent playlist(s) aren't evaluated and show empty"), !smart.is_empty(), "{out}");

    let limit = tc.draw(gs::integers::<usize>().min_value(1).max_value(5)).to_string();
    let (tracks, out) = both(&["tracks", "-n", &limit, "--sort", "-bpm"]);
    let tracks = tracks.as_array().unwrap();
    assert!(tracks.len() <= limit.parse().unwrap());
    for t in tracks {
        assert!(out.contains(t["title"].as_str().unwrap()), "{t}: {out}");
    }
    let (lists, out) = both(&["playlists"]);
    for l in lists.as_array().unwrap() {
        assert!(out.contains(l["name"].as_str().unwrap()), "{l}: {out}");
    }

    let (diff, out) = both(&["diff", "A"]);
    assert!(out.contains(&format!("{} track(s) found on the drive by file", diff["matched_by_file"])), "{out}\n{diff}");
    let uncertain = diff["uncertain"].as_array().unwrap();
    assert_eq!(
        out.contains("only loosely match a track on the drive and count as missing"),
        !uncertain.is_empty(),
        "{out}"
    );
    for t in diff["diff"]["added_tracks"].as_array().unwrap() {
        assert!(out.contains(t["name"].as_str().unwrap_or_else(|| t["file_path"].as_str().unwrap())), "{t}: {out}");
    }

    let folder = fx.home("export");
    let export = with(&["export", path(&folder), "--dry-run"]);
    let export: Vec<&str> = export.iter().map(String::as_str).collect();
    let (planned, printed) = (fx.run(&[&["--json"][..], &export].concat()), fx.run(&export));
    let said = format!("{}\n{}\n{}", printed.out, printed.err, planned.out);
    assert_eq!(planned.code, printed.code, "{said}");
    if planned.code != 0 {
        return;
    }
    assert!(!folder.exists(), "a dry run made {folder:?}");
    let p = &serde_json::from_str::<Value>(&planned.out).unwrap()["plan"];
    let copies = p["copies"].as_array().unwrap().len();
    assert!(printed.out.contains(&format!("  copy {copies} file(s), ")), "{said}");
    let head = format!(
        " -> {}: {} track(s), {} playlist(s), ",
        p["target"].as_str().unwrap(),
        p["tracks"],
        p["playlist_count"]
    );
    assert!(printed.out.lines().next().is_some_and(|l| l.contains(&head)), "{head:?}: {said}");
    for w in p["warnings"].as_array().unwrap() {
        assert!(printed.out.contains(w.as_str().unwrap()), "{w}: {said}");
    }
    let json = tc.draw(gs::booleans());
    let apply = with(&["export", path(&folder)]);
    let mut apply: Vec<&str> = apply.iter().map(String::as_str).collect();
    apply.insert(0, "--yes");
    if json {
        apply.insert(0, "--json");
    }
    let r = fx.run(&apply);
    assert_eq!(r.code, 0, "{}\n{}", r.out, r.err);
    let copied = files_under(&folder.join("Contents")).len();
    assert_eq!(copied, p["copies"].as_array().unwrap().len(), "{}", r.out);
    if !json {
        assert!(r.out.contains(&format!("exported {} track(s) to ", p["tracks"])), "{}", r.out);
        assert!(r.out.contains(&format!("{copied} file(s) copied")), "{}", r.out);
    }
}

/// Annotations of drive A's tracks, agreeing with what rekordbox stored or another related key
/// or tempo, plus one for a track the drive doesn't have: `analysis bench` printed gives each
/// analyzer's scores and outcomes as its JSON does (the analysis has none to give: the drive's
/// placeholder audio doesn't decode).
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn the_benchmark_prints_its_scores(tc: TestCase) {
    use locallibrary::Device;
    use locallibrary::harmony::Camelot;

    let fx = Env::from_template();
    let root = fx.volumes.join("A");
    let lib = Device::open(&root).unwrap().library().unwrap();
    let (keys, tempos) = (fx.home("keys"), fx.home("tempos"));
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::create_dir_all(&tempos).unwrap();
    for t in &lib.tracks {
        let key = Camelot::parse(&t.key).unwrap();
        let id = t.file_path.rsplit('/').next().unwrap().split('.').next().unwrap().to_string();
        let annotated_key = match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => key,
            1 => key.parallel(),
            2 => key.relative(),
            _ => Camelot::parse("1A").unwrap(),
        };
        std::fs::write(keys.join(format!("{id}.key")), annotated_key.note()).unwrap();
        let factor = tc.draw(gs::sampled_from(&[1.0, 2.0, 0.5, 3.0, 1.37][..]));
        std::fs::write(tempos.join(format!("{id}.bpm")), (t.bpm * factor).to_string()).unwrap();
    }
    let extra = tc.draw(gs::booleans());
    if extra {
        std::fs::write(keys.join("9999.LOFI.key"), "D major").unwrap();
    }
    // Keys alone, then tempos too.
    for with_tempos in [false, true] {
        let mut args = vec!["analysis", "bench", "A", "--keys", path(&keys)];
        if with_tempos {
            args.extend(["--tempos", path(&tempos)]);
        }
        let v = fx.json(&args);
        let out = fx.ok(&args);
        let said = format!("{out}\n{v}");
        let pct = |r: &Value, o: &str| {
            let tracks = r["tracks"].as_f64().unwrap();
            if tracks == 0.0 { 0.0 } else { 100.0 * r["counts"][o].as_f64().unwrap() / tracks }
        };
        for (analyzer, k) in v["keys"].as_object().unwrap() {
            let line = out
                .lines()
                .find(|l| l.trim_start().starts_with(analyzer.as_str()) && l.contains("weighted"))
                .unwrap_or_else(|| panic!("{analyzer}: {said}"));
            let scores =
                format!("weighted {:.1}%, correct {:.1}%", k["score"].as_f64().unwrap() * 100.0, pct(k, "correct"));
            assert!(line.contains(&scores), "{scores:?} in {line:?}: {said}");
            for (o, name) in [
                ("fifth", "fifth"),
                ("relative", "relative"),
                ("parallel", "parallel"),
                ("other", "other"),
                ("missing", "no key"),
            ] {
                let shown = format!("{name} {:.1}%", pct(k, o));
                assert_eq!(line.contains(&shown), k["counts"][o].as_u64().unwrap() > 0, "{shown:?} in {line:?}");
            }
            assert!(line.ends_with(&format!("({} tracks)", k["tracks"])), "{line:?}");
        }
        assert_eq!(out.contains("Tempos"), !v["tempos"].as_object().unwrap().is_empty(), "{said}");
        for (analyzer, t) in v["tempos"].as_object().unwrap() {
            let line = out
                .lines()
                .find(|l| l.trim_start().starts_with(analyzer.as_str()) && l.contains("Accuracy1"))
                .unwrap_or_else(|| panic!("{analyzer}: {said}"));
            let scores = format!(
                "Accuracy1 {:.1}%, Accuracy2 {:.1}%",
                t["score"].as_f64().unwrap() * 100.0,
                v["accuracy2"][analyzer].as_f64().unwrap_or(0.0)
            );
            assert!(line.contains(&scores), "{scores:?} in {line:?}");
            for (o, name) in [
                ("double", "double"),
                ("half", "half"),
                ("triple", "triple"),
                ("third", "third"),
                ("other", "other"),
                ("missing", "no tempo"),
            ] {
                let shown = format!("{name} {:.1}%", pct(t, o));
                assert_eq!(line.contains(&shown), t["counts"][o].as_u64().unwrap() > 0, "{shown:?} in {line:?}");
            }
        }
        let unmatched = v["unmatched"].as_u64().unwrap();
        assert_eq!(unmatched > 0, extra, "{said}");
        assert_eq!(
            out.contains(&format!("{unmatched} annotated track(s) had no audio to score")),
            unmatched > 0,
            "{said}"
        );
    }
}
