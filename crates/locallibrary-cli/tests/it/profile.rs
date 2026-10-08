//! `locallibrary profile`: the report, the document, and the choices that shape it.

use std::path::PathBuf;

use locallibrary::sets::{EntryOrigin, Set, SetBook, SetEntry};
use serde_json::Value;

use crate::support::{PDB, club_drive, play};

struct Env {
    dir: tempfile::TempDir,
    drive: PathBuf,
}

impl Env {
    /// The club drive, with two sets and a two-track sound check.
    fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let drive = club_drive(dir.path(), "Main");
        play(&drive, "HISTORY 001", &[1, 2, 3, 4, 5, 6]);
        play(&drive, "HISTORY 002", &[9, 10, 1, 2, 12, 7]);
        play(&drive, "HISTORY 003", &[3, 4]);
        Env { dir, drive }
    }

    fn drive(&self) -> &str {
        self.drive.to_str().unwrap()
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        crate::locallibrary(&self.dir.path().join("store"), &[]).args(args).output().unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = self.run(&[&["--json"], args].concat());
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{args:?}: {e}\n{text}"))
    }

    fn fails(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(!out.status.success(), "{args:?} should fail");
        String::from_utf8_lossy(&out.stderr).into_owned()
    }
}

#[test]
fn the_report_and_the_document() {
    let env = Env::new();
    let v = env.json(&["profile", env.drive()]);
    assert_eq!(v["report"]["sets"]["count"], 2);
    assert_eq!(v["report"]["sets"]["too_short"], 1);
    assert_eq!(v["report"]["basis"], "sessions");
    assert_eq!(v["document"]["version"], 1);
    assert!(v["document"].get("signature").is_none());

    // Fewer tracks make the sound check a set.
    let v = env.json(&["profile", env.drive(), "--min-tracks", "2", "--no-analysis"]);
    assert_eq!(v["report"]["sets"]["count"], 3);

    // The human report: only the sections asked for.
    let out = env.run(&["profile", env.drive(), "-s", "tempo,signature"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("Tempo") && text.contains("Signature tracks"), "{text}");
    assert!(!text.contains("Player style"), "{text}");
    assert!(env.fails(&["profile", "-s", "nope"]).contains("sections: summary"));

    // The document alone, printed or written.
    let out = env.run(&["profile", "document", env.drive()]);
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["basis"], "sessions");
    assert!(doc.get("sets").is_none(), "sessions are never counted in public");
    let file = env.dir.path().join("profile.json");
    let v = env.json(&["profile", "document", env.drive(), "-o", file.to_str().unwrap()]);
    assert_eq!(v["report"]["document"]["version"], 1);
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written["tempo"], doc["tempo"]);
    // A file that's there is replaced only with --replace.
    std::fs::write(&file, "mine").unwrap();
    let err = env.fails(&["profile", "document", env.drive(), "-o", file.to_str().unwrap()]);
    assert!(err.contains("already there") && err.contains("--replace"), "{err}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    env.json(&["profile", "document", env.drive(), "-o", file.to_str().unwrap(), "--replace"]);
    assert!(std::fs::read_to_string(&file).unwrap().contains("\"tempo\""));
}

#[test]
fn choices_shape_the_document() {
    let env = Env::new();
    let p = env.json(&["profile", "include", "signature,artists"]);
    assert!(p["report"]["sections"].as_array().unwrap().contains(&"signature".into()));

    // "In Charge" names two tracks; the CLI asks for one.
    let err = env.fails(&["profile", "pin", "in charge", env.drive()]);
    assert!(err.contains("matches 2 tracks"), "{err}");
    let p = env.json(&["profile", "pin", "a2 - in charge", env.drive()]);
    assert_eq!(p["report"]["pinned"][0], "[a]pendics.shuffle - A2 - In Charge");
    assert!(env.fails(&["profile", "pin", "not played", env.drive()]).contains("--exact"));
    env.json(&["profile", "hide", "[a]pendics.shuffle - A1 - It's In The Eyes", env.drive()]);
    env.json(&["profile", "hide", "ID - ID", "--exact"]);
    env.json(&["profile", "alias", "electronic", "Electronica"]);
    env.json(&["profile", "headline", "Techno warm-ups"]);

    let doc = env.json(&["profile", "document", env.drive()]);
    assert_eq!(doc["headline"], "Techno warm-ups");
    assert_eq!(doc["signature"], serde_json::json!([{ "artist": "[a]pendics.shuffle", "title": "A2 - In Charge" }]));
    assert!(doc["genres"].as_array().unwrap().iter().any(|g| g["name"] == "Electronica"));

    let p = env.json(&["profile", "prefs"]);
    assert_eq!(p["hidden"].as_array().unwrap().len(), 2);
    env.json(&["profile", "unhide", "id - id"]);
    env.json(&["profile", "unpin", "in charge"]);
    env.json(&["profile", "alias", "electronic"]);
    env.json(&["profile", "headline"]);
    let p = &env.json(&["profile", "precision", "exact"])["report"];
    assert_eq!(p["precision"], "exact");
    assert!(p["pinned"].as_array().unwrap().is_empty());
    assert!(p.get("headline").is_none());

    // Reset asks first; --yes skips the question.
    env.json(&["--yes", "profile", "reset"]);
    let p = env.json(&["profile", "prefs"]);
    assert!(p.get("sections").is_none() && p["hidden"].as_array().unwrap().is_empty());
}

/// With no drives named, the profile reads the mounted drives the store knows (those pulled):
/// a stick someone else plugged in doesn't count as the DJ's sets. A store that knows none
/// reads every mounted drive.
#[test]
fn the_profile_reads_the_djs_own_drives() {
    let env = Env::new();
    let partner = env.dir.path().join("Partner/PIONEER/rekordbox");
    std::fs::create_dir_all(&partner).unwrap();
    std::fs::copy(env.drive.join(PDB), partner.join("export.pdb")).unwrap();
    let run = |args: &[&str]| {
        let out = crate::locallibrary(&env.dir.path().join("store"), &[env.dir.path()])
            .arg("--json")
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    let drives = |v: &Value| {
        let sources = v["report"]["sources"].as_array().unwrap();
        sources
            .iter()
            .filter(|s| s["kind"] == "drive")
            .map(|s| s["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(drives(&run(&["profile", "--no-analysis"])), ["Main", "Partner"]);
    run(&["pull", env.drive()]);
    assert_eq!(drives(&run(&["profile", "--no-analysis"])), ["Main"]);
}

/// Until the DJ declares sets, the report counts sessions and calls them that; then the sets are
/// what it counts and lists.
#[test]
fn sessions_stand_in_until_sets_are_declared() {
    let env = Env::new();
    let text = |args: &[&str]| {
        let out = env.run(args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    };
    let out = text(&["profile", env.drive(), "--sets"]);
    assert!(out.contains("no sets declared: 12 plays in 2 sessions"), "{out}");
    assert!(out.contains("Techno and Electronic · 124–128 BPM · 2 sessions\n"), "{out}");
    assert!(out.contains("○ Sessions  only on this computer"), "{out}");
    assert!(out.contains("Session shape") && out.contains("2/2 sessions"), "{out}");
    assert!(out.contains("sets new --from ID"), "{out}");
    for set_words in ["2 sets", "Set shape", "Sets  ", "in a set", "2/2 sets"] {
        assert!(!out.contains(set_words), "{set_words:?} in\n{out}");
    }
    // Each session's id, to declare it a set by.
    let report = &env.json(&["profile", env.drive()])["report"];
    let id = report["sets"]["list"][0]["id"].as_str().unwrap();
    assert!(out.contains(&format!("{} Main", &id[..8])), "{out}");

    let store = locallibrary::Store::open_or_init(env.dir.path().join("store")).unwrap();
    let lib = locallibrary::pdb::read_library(&env.drive.join(PDB)).unwrap();
    let entries = |ids: &[u32]| -> Vec<SetEntry> {
        ids.iter()
            .map(|id| {
                SetEntry::from_track(lib.tracks.iter().find(|t| t.id == *id).unwrap(), EntryOrigin::Session, None)
            })
            .collect()
    };
    let at = chrono::Utc::now();
    let set = |title: &str, date: &str, tracks: Vec<SetEntry>| {
        let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
        Set {
            id: locallibrary::sets::new_id(date).unwrap(),
            title: Some(title.into()),
            date,
            venue: Some("Fabric".into()),
            city: Some("London".into()),
            tracks,
            created_at: at,
            updated_at: at,
            ..Default::default()
        }
    };
    let mut book = SetBook::default();
    book.sets.push(set("Friday", "2026-09-12", entries(&[1, 2, 3, 4, 5, 6])));
    book.sets.push(set("Warm-up", "2026-10-03", entries(&[9, 10, 1, 2, 12, 7])));
    book.save(&store).unwrap();
    let out = text(&["profile", env.drive(), "--sets", "-s", "sets"]);
    assert!(out.contains("12 plays in 2 sets"), "{out}");
    assert!(out.contains("● Sets  in the document"), "{out}");
    assert!(out.contains("played 2026-09-12 to 2026-10-03"), "{out}");
    let warm_up = out.lines().find(|l| l.contains("Warm-up")).unwrap_or_else(|| panic!("{out}"));
    assert!(warm_up.starts_with("    2026-10-03") && warm_up.ends_with("Fabric, London"), "{warm_up}");
    assert!(!out.contains("session"), "{out}");
    // --min-tracks is for sessions only.
    let v = env.json(&["profile", env.drive(), "--min-tracks", "7"]);
    assert_eq!((&v["report"]["basis"], &v["report"]["sets"]["count"]), (&"plays".into(), &2.into()));
    assert_eq!(v["document"]["sets"]["count"], 2);
}
