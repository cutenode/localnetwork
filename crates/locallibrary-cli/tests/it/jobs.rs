//! `refresh`, `set`, `setup`, and the drive a command means when it's given none, through the
//! command line (and `refresh` over RPC). Every run sees only the volumes under its own
//! `LOCALLIBRARY_VOLUMES`, never the drives plugged into the machine.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use locallibrary::device::dunce_canonicalize;
use serde_json::{Value, json};

const SAMPLE: &str = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/House1.wav";
const PDB: &str = "PIONEER/rekordbox/export.pdb";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../locallibrary/tests/fixtures")
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

/// The demo export with placeholder audio.
fn demo_drive(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    copy_dir(&fixtures().join("rekordcrate/demo_tracks"), &root);
    let db = rekordbox_pdb::Database::from_file(root.join(PDB)).unwrap();
    for t in &db.tracks {
        let p = root.join(t.file_path().trim_start_matches('/'));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, format!("audio {}", t.title()).repeat(100)).unwrap();
    }
    root
}

/// rekordbox exports to the drive again: `pdb` (its own database), with a demo sample, a minute
/// from now.
fn export_again(root: &Path, pdb: &[u8]) {
    std::fs::write(root.join(PDB), pdb).unwrap();
    let mut ed = rekordbox_pdb::PdbEditor::from_file(root.join(PDB)).unwrap();
    ed.add_track(&rekordbox_pdb::NewTrack {
        artist: Some("rekordbox".into()),
        ..rekordbox_pdb::NewTrack::new("House 1", SAMPLE)
    })
    .unwrap();
    ed.add_track(&rekordbox_pdb::NewTrack::new("Fresh", "/Contents/Fresh.mp3")).unwrap();
    ed.save(root.join(PDB)).unwrap();
    for (file, audio) in [(SAMPLE, "demo sample"), ("/Contents/Fresh.mp3", "fresh audio")] {
        let p = root.join(file.trim_start_matches('/'));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, audio).unwrap();
    }
    let later = SystemTime::now() + Duration::from_secs(60);
    std::fs::File::options().write(true).open(root.join(PDB)).unwrap().set_modified(later).unwrap();
}

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    volumes: PathBuf,
}

impl Env {
    fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let volumes = root.join("volumes");
        std::fs::create_dir_all(&volumes).unwrap();
        Env { _dir: dir, root, volumes }
    }

    fn command(&self) -> Command {
        let mut c = crate::locallibrary(&self.root.join("store"), &[&self.volumes]);
        c.env_remove("NO_COLOR").env("CLICOLOR", "0");
        c
    }

    /// stdout, stderr and the exit status.
    fn run(&self, args: &[&str]) -> (String, String, i32) {
        let out = self.command().args(args).output().unwrap();
        (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap(), out.status.code().unwrap())
    }

    /// `--json` output and the exit status.
    fn json(&self, args: &[&str]) -> (Value, i32) {
        let (out, err, code) = self.run(&[&["--json"], args].concat());
        let v = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{args:?}: {e}\n{out}\n{err}"));
        (v, code)
    }
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn running(plan: &Value) -> Vec<&str> {
    plan["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["runs"] == true)
        .map(|s| s["step"].as_str().unwrap())
        .collect()
}

#[test]
fn refresh_puts_back_what_rekordbox_exported_over() {
    let env = Env::new();
    let main = demo_drive(&env.volumes, "MAIN");
    let export = std::fs::read(main.join(PDB)).unwrap();

    // The first run: no store yet, so a hint; `pull` with no drive starts the library.
    let (_, err, code) = env.run(&["--yes", "adopt", path(&main), "--name", "Main"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("`locallibrary setup`"), "{err}");
    let (v, code) = env.json(&["pull"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!((&v["schema"], &v["plan"], &v["outcome"]), (&json!(1), &json!(null), &json!("applied")), "{v}");
    assert!(v["report"]["steps"][0]["done"][0].as_str().unwrap().starts_with("saved Main in a new library"), "{v}");
    let (_, err, _) = env.run(&["drives"]);
    assert!(!err.contains("setup"), "{err}");

    // An edit, recorded; then rekordbox exports over it.
    let (_, err, code) = env.run(&["--yes", "playlist", "new", "Main", "Shows/Friday"]);
    assert_eq!(code, 0, "{err}");
    export_again(&main, &export);
    let (v, _) = env.json(&["status"]);
    assert_eq!(v["mounted"][0]["exported_over_edits"], 1, "{v}");
    let (out, _, _) = env.run(&["status"]);
    assert!(out.contains("rekordbox exported over 1 recorded edit(s): run `locallibrary refresh \"Main\"`"), "{out}");

    // The plan, without --yes: not applied.
    let (v, code) = env.json(&["refresh"]);
    assert_eq!((code, &v["applied"], &v["outcome"]), (3, &json!(false), &json!("not_confirmed")), "{v}");
    assert_eq!(running(&v["plan"]), ["clean", "replay", "pull"]);
    assert_eq!(v["plan"]["steps"][2]["reason"], "no rule-based playlists for this drive");
    let (out, _, code) = env.run(&["refresh", "--dry-run"]);
    assert_eq!(code, 0);
    assert!(out.contains("Refresh Main ("), "{out}");
    assert!(out.contains("1. Remove rekordbox's demo samples"), "{out}");
    assert!(out.contains("2. Make the recorded edits again"), "{out}");
    assert!(out.contains("-  Write My Settings: the My Settings profile is empty"), "{out}");
    let (v, code) = env.json(&["refresh", "--skip", "clean,pull", "--dry-run"]);
    assert_eq!((code, running(&v["plan"])), (0, vec!["replay"]));
    let (_, err, code) = env.run(&["refresh", "--skip", "everything"]);
    assert_eq!(code, 2, "{err}");

    // Applied: the playlist is back, the sample gone, the drive saved.
    let (out, err, code) = env.run(&["--yes", "refresh"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(err.contains("[1/3] Remove rekordbox's demo samples"), "{err}");
    assert!(out.contains("✓ remove 1 track(s)"), "{out}");
    assert!(!main.join(SAMPLE.trim_start_matches('/')).exists());
    let (v, _) = env.json(&["playlists", "Main"]);
    assert!(v.to_string().contains("Friday"), "{v}");
    let (v, _) = env.json(&["status"]);
    let main_status = &v["mounted"][0];
    assert_eq!(
        (&main_status["relation"], &main_status["exported_over_edits"]),
        (&json!("identical"), &json!(0)),
        "{v}"
    );
    let (out, _, code) = env.run(&["refresh"]);
    assert_eq!(code, 0);
    assert!(out.contains("nothing to do"), "{out}");

    // Over RPC: plan, then apply by plan_id.
    export_again(&main, &export);
    let mut rpc = env.command().arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = rpc.stdin.take().unwrap();
    let mut lines = BufReader::new(rpc.stdout.take().unwrap()).lines();
    let mut call = |id: u32, method: &str, params: Value| -> Value {
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(stdin, "{msg}").unwrap();
        loop {
            let reply: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
            if reply["id"] == id {
                return reply;
            }
        }
    };
    let plan = call(1, "refresh.plan", json!({ "drive": "Main" }));
    assert_eq!(running(&plan["result"]), ["clean", "replay", "pull"], "{plan}");
    let id = plan["result"]["plan_id"].clone();
    let applied = call(2, "refresh.apply", json!({ "plan_id": id }));
    assert!(applied["result"]["stopped"].is_null(), "{applied}");
    assert_eq!(applied["result"]["steps"].as_array().unwrap().len(), 3, "{applied}");
    let again = call(3, "refresh.apply", json!({ "plan_id": id }));
    assert_eq!(again["error"]["data"]["kind"], "stale_plan", "a plan is applied once");
    let set = call(4, "set.plan", json!({ "eject": false }));
    assert_eq!(running(&set["result"]), ["check"], "{set}");
    drop(stdin);
    rpc.wait().unwrap();
}

/// The commands that, given no drive, read every mounted one.
const READS: [&[&str]; 11] = [
    &["info"],
    &["tracks"],
    &["playlists"],
    &["backups"],
    &["doctor"],
    &["space"],
    &["settings", "show"],
    &["art"],
    &["tags"],
    &["edits"],
    &["plays"],
];

#[test]
fn a_read_given_no_drive_reads_every_mounted_drive() {
    let env = Env::new();
    let (v, code) = env.json(&["tracks"]);
    assert_eq!((code, &v["error"]["kind"]), (2, &json!("usage")), "no drive mounted: {v}");
    let (a, b) = (demo_drive(&env.volumes, "A"), demo_drive(&env.volumes, "B"));

    // With --json, a list: each drive's name, mount path and what was read.
    for args in READS {
        let (v, code) = env.json(args);
        assert!(code == 0 || code == 4, "{args:?}: {code}\n{v}");
        let reads = v.as_array().unwrap_or_else(|| panic!("{args:?}: {v}"));
        let drives: Vec<(&Value, &Value)> = reads.iter().map(|r| (&r["drive"], &r["root"])).collect();
        let roots = [&a, &b].map(|d| json!(dunce_canonicalize(d)));
        assert_eq!(drives, [(&json!("A"), &roots[0]), (&json!("B"), &roots[1])], "{args:?}");
        assert!(reads.iter().all(|r| r["error"].is_null() && !r["result"].is_null()), "{args:?}: {v}");
    }

    // In a terminal, each drive under its name.
    let (out, _, code) = env.run(&["tracks", "--columns", "title"]);
    assert_eq!(code, 0, "{out}");
    let (at_a, at_b) = (out.find("A  ").unwrap(), out.find("B  ").unwrap());
    assert!(at_a < at_b && out[at_a..at_b].contains("track(s)") && out[at_b..].contains("track(s)"), "{out}");

    // TSV and CSV: one listing, the drive in a column of its own.
    let (out, _, code) = env.run(&["tracks", "--format", "csv", "--columns", "id,title"]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.iter().filter(|l| l.starts_with("drive,")).collect::<Vec<_>>(), [&"drive,id,title"], "{out}");
    assert!(lines[1].starts_with("A,") && lines.last().unwrap().starts_with("B,"), "{out}");
    assert_eq!(
        lines.iter().filter(|l| l.starts_with("A,")).count(),
        lines.iter().filter(|l| l.starts_with("B,")).count()
    );

    // A drive that can't be read is left out, and said so; the others are still read.
    std::fs::write(b.join(PDB), "not a database").unwrap();
    let (out, err, code) = env.run(&["tracks"]);
    assert_eq!(code, 5, "{out}\n{err}");
    assert!(err.contains("B left out: "), "{err}");
    assert!(out.contains("A  ") && out.contains("track(s)") && !out.contains("B  "), "{out}");
    let (v, code) = env.json(&["playlists"]);
    assert_eq!(code, 5, "{v}");
    assert!(v[0]["error"].is_null() && v[0]["result"].is_array(), "{v}");
    assert!(v[1]["result"].is_null() && v[1]["error"]["kind"].is_string(), "{v}");
}

#[test]
fn art_auto_given_no_drive_fills_in_every_mounted_drive() {
    let env = Env::new();
    let drives = [demo_drive(&env.volumes, "A"), demo_drive(&env.volumes, "B")];
    let cover = image::RgbImage::from_fn(300, 300, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 40]));
    for d in &drives {
        cover.save(d.join("Contents/Loopmasters/UnknownAlbum/cover.png")).unwrap();
    }

    // Every drive's plan, and one question.
    let (v, code) = env.json(&["art", "auto", "--dry-run"]);
    assert_eq!((code, &v["outcome"]), (0, &json!("dry_run")), "{v}");
    let found: Vec<usize> =
        v["plan"].as_array().unwrap().iter().map(|p| p["found"].as_array().unwrap().len()).collect();
    assert_eq!(found, [2, 2], "{v}");

    let (out, err, code) = env.run(&["--yes", "art", "auto"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert_eq!(out.matches("artwork set on 2 track(s)").count(), 2, "{out}");
    let (v, _) = env.json(&["art", "missing"]);
    assert!(v.as_array().unwrap().iter().all(|r| r["result"] == json!([])), "{v}");
}

#[test]
fn which_drive_a_command_means() {
    let env = Env::new();
    let (a, b) = (demo_drive(&env.volumes, "A"), demo_drive(&env.volumes, "B"));

    // Two drives, neither in a library: a read takes both (see
    // `a_read_given_no_drive_reads_every_mounted_drive`), a write says which.
    let xml = env.root.join("rekordbox.xml");
    let (_, err, code) = env.run(&["xml", "-o", path(&xml)]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("say which drive to export"), "{err}");
    for d in [&a, &b] {
        assert!(err.contains(&dunce_canonicalize(d).display().to_string()), "{err}");
    }
    let (v, code) = env.json(&["xml", "-o", path(&xml)]);
    assert_eq!((code, &v["error"]["kind"], &v["outcome"]), (2, &json!("usage"), &json!("error")), "{v}");
    assert_eq!((&v["schema"], &v["error"]["code"]), (&json!(1), &json!("usage")), "{v}");
    let (v, code) = env.json(&["info", "A"]);
    assert_eq!((code, &v["volume_name"]), (0, &json!("A")));
    let (_, err, code) = env.run(&["pull"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("none belongs to a library"), "{err}");

    // A name nobody has, and a known drive that isn't plugged in.
    let (_, err, code) = env.run(&["info", "Nobody"]);
    assert_eq!(code, 1);
    assert!(err.contains("no mounted drive matches \"Nobody\"") && err.contains("mounted: A ("), "{err}");
    env.run(&["--yes", "adopt", path(&b), "--name", "Club drive"]);
    env.run(&["pull", "Club drive"]);
    std::fs::rename(&b, env.root.join("B")).unwrap();
    let (_, err, code) = env.run(&["info", "club drive"]);
    assert_eq!(code, 1);
    assert!(err.contains("Club drive is known but not mounted; last seen"), "{err}");

    // One drive mounted: that's the one, for every command that reads one.
    for args in READS {
        let (_, err, code) = env.run(args);
        assert!(code == 0 || code == 4, "{args:?}: {code}\n{err}");
    }
    let (v, code) = env.json(&["check"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["drives"], json!(["A"]));
}

#[test]
fn set_saves_syncs_checks_and_keeps_problems_mounted() {
    let env = Env::new();
    let main = demo_drive(&env.volumes, "MAIN");
    let backup = env.volumes.join("BACKUP");
    std::fs::create_dir(&backup).unwrap();
    env.run(&["--yes", "adopt", path(&main), "--name", "Main"]);
    env.run(&["pull", "Main", "--library", "Home", "--new-library"]);
    let (_, err, code) = env.run(&["--yes", "sync", path(&main), path(&backup)]);
    assert_eq!(code, 0, "{err}");
    env.run(&["--yes", "adopt", path(&backup), "--name", "Backup"]);
    env.run(&["--yes", "drive", "role", "Main", "primary"]);

    // Main has a newer export: the plan saves it, copies it onto Backup, checks both.
    let mut ed = rekordbox_pdb::PdbEditor::from_file(main.join(PDB)).unwrap();
    ed.add_track(&rekordbox_pdb::NewTrack::new("New", "/Contents/New/new.mp3")).unwrap();
    ed.save(main.join(PDB)).unwrap();
    std::fs::create_dir_all(main.join("Contents/New")).unwrap();
    std::fs::write(main.join("Contents/New/new.mp3"), "new audio").unwrap();
    let (out, _, code) = env.run(&["set", "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Get Main, Backup ready for a set"), "{out}");
    assert!(out.contains("2. Copy the newest library onto the other drives"), "{out}");
    assert!(out.contains("Main holds the newest library"), "{out}");
    assert!(out.contains("4. Eject the drives"), "{out}");
    let (v, code) = env.json(&["--yes", "set", "--no-eject"]);
    assert_eq!((code, &v["outcome"]), (0, &json!("applied")), "{v}");
    assert_eq!(running(&v["plan"]), ["pull", "sync", "check"]);
    let (c, _) = env.json(&["compare", "Main", "Backup"]);
    assert_eq!(c["relation"], "identical");

    // A check that finds problems (placeholder audio doesn't decode) leaves the drives mounted.
    let (v, code) = env.json(&["--yes", "set", "--audio"]);
    assert_eq!((code, &v["outcome"]), (4, &json!("problems")), "{v}");
    assert_eq!(running(&v["plan"]), ["check", "eject"]);
    assert_eq!(v["report"]["problems"], true);
    assert!(main.exists() && backup.exists());
    let (out, _, code) = env.run(&["--yes", "set", "--audio"]);
    assert_eq!(code, 4);
    assert!(out.contains("The drives stay mounted"), "{out}");

    // `check` and `eject` take the group too.
    let (v, code) = env.json(&["check"]);
    assert_eq!((code, &v["drives"]), (0, &json!(["Main", "Backup"])), "{v}");
}

#[test]
fn setup_names_drives_and_starts_the_library() {
    let env = Env::new();
    let main = demo_drive(&env.volumes, "REKORDBOX");
    let backup = env.volumes.join("BACKUP");
    copy_dir(&main, &backup);

    // No terminal: it needs --yes and the library's name.
    let (_, err, code) = env.run(&["setup"]);
    assert_eq!(code, 2);
    assert!(err.contains("needs a terminal"), "{err}");
    let (_, err, code) = env.run(&["--yes", "setup"]);
    assert_eq!(code, 2);
    assert!(err.contains("--library NAME"), "{err}");
    let (_, err, code) = env.run(&["--yes", "setup", "--library", "Home", "--role", "NOPE=backup"]);
    assert_eq!(code, 2, "{err}");

    let (out, err, code) = env.run(&[
        "--yes",
        "setup",
        "--library",
        "Home",
        "--primary",
        path(&main),
        "--name",
        &format!("{}=Main", path(&main)),
        "--name",
        "BACKUP=Backup",
        "--role",
        "BACKUP=set",
        "--players",
        "CDJ-3000,CDJ-2000NXS2",
        "--contact",
        "sam@example.com",
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    for d in [&main, &backup] {
        let found = std::fs::read_to_string(d.join("IF FOUND.txt")).unwrap();
        assert!(found.contains("sam@example.com"), "{found}");
    }
    assert!(out.contains("You're set up"), "{out}");
    assert!(out.contains("locallibrary refresh"), "{out}");
    let (libs, _) = env.json(&["library", "ls"]);
    assert_eq!(libs[0]["name"], "Home", "{libs}");
    let (drives, _) = env.json(&["drive", "list"]);
    let role = |name: &str| drives.as_array().unwrap().iter().find(|d| d["name"] == name).map(|d| d["role"].clone());
    assert_eq!(role("Main"), Some(json!("primary")), "{drives}");
    assert_eq!(role("Backup"), Some(json!("set")), "{drives}");
    let (players, _) = env.json(&["players"]);
    assert_eq!(players["default"], json!(["CDJ-3000", "CDJ-2000NXS2"]), "{players}");
    let (profile, _) = env.json(&["settings", "profile"]);
    assert!(!profile["base"].as_object().unwrap().is_empty(), "My Settings imported from Main: {profile}");
    let (group, code) = env.json(&["check", "--players", "CDJ-3000"]);
    assert_eq!(code, 0, "{group}");
    assert_eq!(group["drives"], json!(["Main", "Backup"]), "both drives are in Home");
}

/// Regression: setup took the first drive a name matched, so with two drives answering to
/// REKORDBOX it set up either. It refuses the name, as every other command does.
#[test]
fn setup_refuses_a_name_two_drives_answer_to() {
    let env = Env::new();
    let main = demo_drive(&env.volumes, "REKORDBOX");
    let other = demo_drive(&env.volumes, "OTHER");
    let (_, err, code) = env.run(&["--yes", "adopt", path(&other), "--name", "REKORDBOX"]);
    assert_eq!(code, 0, "{err}");
    for named in [&["REKORDBOX"][..], &[path(&main), "--name", "REKORDBOX=Main"]] {
        let (_, err, code) = env.run(&[&["--yes", "setup", "--library", "Home"], named].concat());
        assert_ne!(code, 0, "{named:?}: {err}");
        assert!(err.contains("\"REKORDBOX\" matches 2 drives"), "{err}");
    }
    assert!(!main.join(".locallibrary").exists(), "nothing written");
}

/// Regression: setup took every mounted drive with no way to leave one out, started a second
/// library over drives that were in one already, and stopped part way on an archive drive given
/// contact details.
#[test]
fn setup_takes_the_drives_named_and_joins_their_library() {
    let env = Env::new();
    let main = demo_drive(&env.volumes, "REKORDBOX");
    let backup = env.volumes.join("BACKUP");
    copy_dir(&main, &backup);
    let friend = env.volumes.join("FRIEND");
    copy_dir(&main, &friend);

    // Only the drives named; the archive drive gets no contact details, and the last step runs.
    let (out, err, code) = env.run(&[
        "--yes",
        "setup",
        path(&main),
        "BACKUP",
        "--library",
        "Home",
        "--primary",
        path(&main),
        "--role",
        "BACKUP=archive",
        "--contact",
        "sam@example.com",
        "--players",
        "CDJ-3000",
    ]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(main.join("IF FOUND.txt").exists());
    assert!(!backup.join("IF FOUND.txt").exists(), "kept as it is");
    assert!(out.contains("BACKUP is an archive drive, kept as it is"), "{out}");
    assert!(!friend.join(".locallibrary").exists() && !friend.join("IF FOUND.txt").exists(), "left alone");
    let (players, _) = env.json(&["players"]);
    assert_eq!(players["default"], json!(["CDJ-3000"]), "{players}");

    // FRIEND holds another library in the store. Setting up again joins Home and leaves FRIEND
    // out.
    let (_, err, code) = env.run(&["--yes", "pull", path(&friend), "--library", "Theirs", "--new-library"]);
    assert_eq!(code, 0, "{err}");
    let (out, err, code) = env.run(&["--yes", "setup", "--library", "home", "--primary", path(&main)]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("the library it's in already"), "{out}");
    assert!(out.contains("left out FRIEND, a drive of library \"Theirs\""), "{out}");
    let libraries = || {
        let (libs, _) = env.json(&["library", "ls"]);
        libs.as_array().unwrap().iter().map(|l| (l["name"].clone(), l["drives"].clone())).collect::<Vec<_>>()
    };
    let both = vec![(json!("Home"), json!(["REKORDBOX"])), (json!("Theirs"), json!(["FRIEND"]))];
    assert_eq!(libraries(), both);

    // Its primary in a new library would leave Home without it: refused.
    let (_, err, code) = env.run(&["--yes", "setup", "REKORDBOX", "--library", "Mine"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("REKORDBOX is a drive of library \"Home\""), "{err}");
    assert_eq!(libraries(), both);

    // Regression: the refusal above looked at the first drive named, while the first one mounted
    // (REKORDBOX, of Home) became the primary. The first named is the primary.
    let zed = demo_drive(&env.volumes, "ZED");
    let (out, err, code) = env.run(&["--yes", "setup", path(&zed), "REKORDBOX", "--library", "Mine", "--no-settings"]);
    assert_eq!(code, 0, "{out}\n{err}");
    let (drives, _) = env.json(&["drive", "list"]);
    let role = |name: &str| drives.as_array().unwrap().iter().find(|d| d["name"] == name).map(|d| d["role"].clone());
    assert_eq!((role("ZED"), role("REKORDBOX")), (Some(json!("primary")), Some(json!("backup"))), "{drives}");
}
