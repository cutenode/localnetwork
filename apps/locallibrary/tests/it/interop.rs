//! The interop plugin locallibrary ships, end to end: `export --format traktor|serato|engine`
//! through the plugin host on real drives, and the sidecar's `plugins.export.*`. What the files
//! hold is loclib-interop's to test (its tests/props), and what the plugin makes of the host's
//! answers is plugins/interop's; here is what only the real host and the command line show: the
//! reads (a drive's own analysis, the tracks only OneLibrary has), the options, asking before
//! overwriting, and the exit statuses.

use std::path::Path;

use locallibrary::anlz::{self, CueFile, CuePoint};
use locallibrary::device::Device;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::{convert, paths};
use rekordbox_pdb::PdbEditor;
use serde_json::{Value, json};

use crate::support::{Env, PDB, Rpc, path};

const TRACK_1: &str = "Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3";

/// Every file below `dir` but locallibrary's own state, with its bytes.
fn drive_files(dir: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut files: Vec<_> = walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file() && !e.path().components().any(|c| c.as_os_str() == ".locallibrary"))
        .map(|e| (e.path().to_path_buf(), std::fs::read(e.path()).unwrap()))
        .collect();
    files.sort();
    files
}

/// A collection.nml's `ENTRY` for the file named `file`, up to its end.
fn entry<'a>(nml: &'a str, file: &str) -> &'a str {
    let at = nml.find(&format!("FILE=\"{file}\"")).unwrap_or_else(|| panic!("no entry for {file}:\n{nml}"));
    let start = nml[..at].rfind("<ENTRY ").unwrap();
    &nml[start..start + nml[start..].find("</ENTRY>").unwrap()]
}

/// An entry's `CUE_V2` elements, as written.
fn cues(entry: &str) -> Vec<&str> {
    entry.match_indices("<CUE_V2 ").map(|(at, _)| &entry[at..at + entry[at..].find('>').unwrap()]).collect()
}

/// An element's attribute `name`.
fn attr<'a>(element: &'a str, name: &str) -> &'a str {
    let at = element.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
    &element[at..at + element[at..].find('"').unwrap()]
}

/// A Traktor collection is planned (writing nothing), written, and asked about before it's
/// written over, unless replacing; never onto a drive, and the drive is only read. The list
/// formats' flags and options the format hasn't are refused.
#[test]
fn a_traktor_collection_is_planned_written_and_overwritten_when_asked() {
    let env = Env::new();
    let main = env.drive("Main");
    let before = drive_files(&main);
    let (d, out) = (path(&main), env.outside("traktor"));
    let o = path(&out);
    let v = env.json(&["export", d, "--format", "traktor", "-o", o, "--dry-run"]);
    assert_eq!(
        (&v["applied"], &v["plan"]["counts"]["tracks"], &v["plan"]["counts"]["playlists"]),
        (&json!(false), &json!(2), &json!(1))
    );
    assert!(!out.exists(), "a dry run writes nothing");

    let v = env.json(&["export", d, "Set", "--format", "traktor", "-o", o]);
    assert_eq!(v["outcome"], "applied", "{v}");
    let nml = std::fs::read_to_string(out.join("collection.nml")).unwrap();
    assert!(nml.contains("<NODE TYPE=\"PLAYLIST\" NAME=\"Set\">"), "{nml}");
    // Stamped at `SOURCE_DATE_EPOCH`, in local time.
    let stamped = chrono::DateTime::from_timestamp(crate::STAMPED, 0).unwrap().with_timezone(&chrono::Local);
    let date = stamped.format("MODIFIED_DATE=\"%Y/%-m/%-d\" MODIFIED_TIME=\"").to_string();
    let time = chrono::Timelike::num_seconds_from_midnight(&stamped);
    assert!(nml.contains(&format!("{date}{time}\"")), "{nml}");
    // Refused, planned or not, when it isn't seconds since 1970.
    for dry_run in [true, false] {
        let mut c = env.command();
        c.env("SOURCE_DATE_EPOCH", "soon").args(["export", d, "--format", "traktor", "-o", o, "--replace"]);
        let r = c.args(dry_run.then_some("--dry-run")).output().unwrap();
        let err = String::from_utf8(r.stderr).unwrap();
        assert_eq!(r.status.code(), Some(1), "{err}");
        assert!(err.contains("SOURCE_DATE_EPOCH isn't a number of seconds since 1970"), "{err}");
    }
    assert_eq!(std::fs::read_to_string(out.join("collection.nml")).unwrap(), nml, "left as it was");
    // Not again without asking (JSON never prompts: exit status 3), and with --replace.
    let r = env.run(&["--json", "export", d, "--format", "traktor", "-o", o]);
    assert_eq!(r.code, 3, "{}", r.out);
    assert_eq!(serde_json::from_str::<Value>(&r.out).unwrap()["outcome"], "not_confirmed");
    assert_eq!(env.json(&["export", d, "--format", "traktor", "-o", o, "--replace"])["outcome"], "applied");

    let on_drive = env.fails(1, &["export", d, "--format", "traktor", "-o", path(&main.join("exports"))]);
    assert!(on_drive.contains("isn't written to the drive at"), "{on_drive}");
    assert!(
        env.fails(1, &["export", d, "--format", "traktor", "-o", o, "--split"])
            .contains("are for locallibrary's own formats")
    );
    let list = env.fails(1, &["export", d, "-o", path(&env.outside("x.m3u8")), "--dry-run"]);
    assert!(list.contains("--dry-run is for plugins' formats"), "{list}");
    let tags = env.fails(2, &["export", d, "--format", "traktor", "-o", o, "--option", "write_tags=true"]);
    assert!(tags.contains("this format has no option \"write_tags\""), "{tags}");
    assert_eq!(drive_files(&main), before, "the drive is only read");
}

/// Serato's crates are written as they are; copying the audio for its cue tag isn't asked about
/// (a dry run lists each copy and its size first).
#[test]
fn serato_crates_and_copies_are_written_unasked() {
    let env = Env::new();
    let main = env.drive("Main");
    let d = path(&main);
    let out = env.outside("serato");
    let v = env.json(&["export", d, "--format", "serato", "-o", path(&out)]);
    assert_eq!(v["report"]["files"][0]["what"], "crate Set (2 tracks)", "{v}");
    assert!(out.join("_Serato_/Subcrates/Set.crate").is_file());
    assert!(!out.join("Contents").exists(), "no copies unless asked");

    let tags = env.outside("serato tags");
    let copy = ["export", d, "--format", "serato", "-o", path(&tags), "--option", "write_tags=true"];
    let planned = env.ok(&[&copy[..], &["--dry-run"]].concat());
    let size = std::fs::metadata(main.join(TRACK_1)).unwrap().len();
    // Shown as the platform writes paths (`\` between folders on Windows).
    let copy_path = TRACK_1.split('/').fold(tags.clone(), |p, part| p.join(part));
    let listed = format!("{}  copy of the audio, {}", copy_path.display(), locallibrary::op::bytes(size));
    assert!(planned.contains(&listed), "{listed:?}:\n{planned}");
    assert!(!tags.exists());
    let v = env.json(&copy);
    assert_eq!(v["outcome"], "applied", "{v}");
    assert_eq!(std::fs::read(tags.join(TRACK_1)).unwrap(), std::fs::read(main.join(TRACK_1)).unwrap());
}

/// An Engine DJ library takes its paths relative to where it's written or to the drive; Traktor
/// has no such option, and Engine no absolute paths.
#[test]
fn an_engine_library_takes_its_paths_relative() {
    let env = Env::new();
    let main = env.drive("Main");
    let d = path(&main);
    let out = env.outside("engine");
    let o = path(&out);
    let r = env.run(&["export", d, "--format", "engine", "-o", o, "--option", "paths=absolute"]);
    assert_ne!(r.code, 0, "Engine's paths are relative: {}", r.out);
    env.fails(2, &["export", d, "--format", "traktor", "-o", o, "--option", "paths=drive"]);
    let v = env.json(&["export", d, "--format", "engine", "-o", o, "--option", "paths=drive"]);
    assert_eq!((&v["report"]["counts"]["tracks"], &v["report"]["counts"]["playlists"]), (&json!(2), &json!(1)), "{v}");
    let db = out.join("Engine Library/Database2/m.db");
    let db = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let first: String = db.query_row("SELECT path FROM Track ORDER BY id", [], |r| r.get(0)).unwrap();
    assert_eq!(first, format!("../{TRACK_1}"));
}

/// The sidecar plans an export with the plugin, writing nothing.
#[test]
fn the_sidecar_plans_an_export() {
    let env = Env::new();
    let main = env.drive("Main");
    let out = env.outside("traktor");
    let mut s = Rpc::start(env.command());
    let params = json!({ "plugin": "interop", "exporter": "traktor", "drive": path(&main), "output": path(&out), "options": { "skip_analysis": true } });
    let planned = s.call(1, "plugins.export.plan", params)["result"].clone();
    assert_eq!(
        (&planned["operation"], &planned["counts"]["beat grids"]),
        (&json!("interop.export.traktor"), &json!(0)),
        "{planned}"
    );
    assert!(!out.exists());
}

/// Regression: a relative output folder that didn't exist yet (`-o trak`) was always refused at
/// apply, as the folder above it the plan measured was the empty path.
#[test]
fn a_relative_output_folder_is_written() {
    let env = Env::new();
    let main = env.drive("Main");
    let dir = env.outside("here");
    std::fs::create_dir(&dir).unwrap();
    let out = env
        .command()
        .current_dir(&dir)
        .args(["--json", "export", path(&main), "--format", "traktor", "-o", "trak"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["outcome"], "applied", "{v}");
    assert!(dir.join("trak/collection.nml").is_file());
}

/// Regression: copies for Serato's tags were all kept open until every one was written, so a
/// library of more tracks than a process may have files open (256 for a macOS app) failed with
/// "Too many open files". Here the limit is 64 and there are 100 copies.
#[cfg(unix)]
#[test]
fn copies_for_serato_outnumber_the_open_file_limit() {
    use std::process::{Command, Stdio};

    use rekordbox_pdb::NewTrack;
    let env = Env::new();
    let root = env.drive("Main");
    let mut ed = PdbEditor::from_file(root.join(PDB)).unwrap();
    std::fs::create_dir_all(root.join("Contents/Many")).unwrap();
    for n in 0..100 {
        let file = format!("Contents/Many/{n}.mp3");
        ed.add_track(&NewTrack::new(format!("Track {n}"), format!("/{file}"))).unwrap();
        std::fs::write(root.join(file), "audio").unwrap();
    }
    ed.save(root.join(PDB)).unwrap();
    let to = env.outside("serato");
    // The CLI as the tests run it, its plugin too, under a lower open-file limit.
    let cli = env.command();
    let mut sh = Command::new("sh");
    sh.args(["-c", "ulimit -n 64 && exec \"$@\"", "sh"])
        .arg(cli.get_program())
        .args(["--json", "export", path(&root), "--format", "serato", "--option", "write_tags=true"])
        .args(["--option", "skip_analysis=true", "-o", path(&to)])
        .stdin(Stdio::null());
    for (k, v) in cli.get_envs() {
        match v {
            Some(v) => sh.env(k, v),
            None => sh.env_remove(k),
        };
    }
    let out = sh.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(std::fs::read_dir(to.join("Contents/Many")).unwrap().count(), 100);
}

/// The plugin reads the drive's own analysis through the host: each track's grid starts at its
/// first downbeat, and its cues follow. A track only OneLibrary has (its id the host's, with the
/// top bit set) has its analysis read the same way, and a playlist only OneLibrary has lists it.
#[test]
fn analysis_and_what_only_onelibrary_has_are_read() {
    let env = Env::new();
    let main = env.drive("Main");
    let d = path(&main);
    let device = Device::open(&main).unwrap();
    let first = device.library().unwrap().tracks.into_iter().find(|t| t.file_path == format!("/{TRACK_1}")).unwrap();
    let dat = device.path(&paths::key(&first.analyze_path));
    let marked = [
        CuePoint { start_ms: 25, comment: "Intro".into(), color_id: 6, ..Default::default() },
        CuePoint { start_ms: 30_025, end_ms: Some(33_775), loop_beats: Some((8, 1)), ..Default::default() },
        CuePoint { hot_cue: Some(0), start_ms: 15_025, comment: "Vocal".into(), ..Default::default() },
    ];
    for (file, kind) in [(dat.with_extension("EXT"), CueFile::Ext), (dat, CueFile::Dat)] {
        let bytes = std::fs::read(&file).unwrap();
        std::fs::write(&file, anlz::write_cue_points(&bytes, kind, &marked).unwrap()).unwrap();
    }
    let export = |to: &str| {
        let out = env.outside(to);
        env.json(&["export", d, "--format", "traktor", "-o", path(&out)]);
        std::fs::read_to_string(out.join("collection.nml")).unwrap()
    };
    let nml = export("read");
    let read = cues(entry(&nml, "Demo Track 1.mp3")).into_iter().map(String::from).collect::<Vec<_>>();
    let downbeat = anlz::load_grid(&device, &first).beats.into_iter().find(|b| b.beat_number == 1).unwrap();
    assert_eq!(
        (attr(&read[0], "TYPE"), attr(&read[0], "START").parse::<f64>().unwrap()),
        ("4", f64::from(downbeat.time_ms))
    );
    // The drive's cues as written (memory cues, then hot cues), the hot cue on its pad.
    let marked: Vec<(&str, &str)> = read[1..].iter().map(|c| (attr(c, "START"), attr(c, "HOTCUE"))).collect();
    assert_eq!(marked, [("25.000000", "-1"), ("30025.000000", "-1"), ("15025.000000", "0")], "{read:?}");

    // Track 1 goes from export.pdb, leaving OneLibrary's, and a playlist only OneLibrary has.
    env.ok(&["--yes", "convert", d, "--to", "onelibrary"]);
    let mut one = OneLibrary::open_root(&main).unwrap();
    let own = one.library().unwrap().tracks.into_iter().find(|t| t.file_path == first.file_path).unwrap();
    let booth = one.create_playlist("Booth", 0, false).unwrap();
    one.add_to_playlist(booth, own.id).unwrap();
    std::fs::write(main.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let mut ed = PdbEditor::from_file(main.join(PDB)).unwrap();
    ed.remove_tracks(&[first.id]).unwrap();
    ed.save(main.join(PDB)).unwrap();
    let combined = convert::combined_library(&Device::open(&main).unwrap()).unwrap();
    let only = combined.tracks.iter().find(|t| t.file_path == first.file_path).unwrap();
    assert!(convert::is_onelibrary_only(only.id), "{}", only.id);

    let nml = export("only onelibrary");
    let only = entry(&nml, "Demo Track 1.mp3");
    assert_eq!(cues(only), read, "OneLibrary's own track's grid and cues, read as before");
    let listed = nml.split("NAME=\"Booth\"").nth(1).unwrap();
    assert!(listed[..listed.find("</NODE>").unwrap()].contains("Demo Track 1.mp3"), "{nml}");
}
