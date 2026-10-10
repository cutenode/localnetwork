//! `locallibrary export --format traktor`, end to end through the binary and the RPC sidecar.

use std::path::Path;
use std::process::{Command, Stdio};

use rekordbox_pdb::{NewTrack, PdbEditor};
use serde_json::Value;

use crate::support::dj_drive;

fn locallibrary(root: &Path) -> Command {
    crate::locallibrary(&root.join("store"), &[])
}

fn run(root: &Path, args: &[&str]) -> (Value, bool) {
    let out = locallibrary(root).arg("--json").args(args).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let v = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{args:?}: {e}\n{text}\n{}", String::from_utf8_lossy(&out.stderr)));
    (v, out.status.success())
}

#[test]
fn export_for_traktor_plans_then_writes() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dj_drive(dir.path(), "Main");
    let (drive, out) = (drive.to_str().unwrap(), dir.path().join("traktor"));
    let out_s = out.to_str().unwrap();

    let (v, ok) = run(dir.path(), &["export", drive, "--format", "traktor", "-o", out_s, "--dry-run"]);
    assert!(ok, "{v}");
    assert_eq!(v["applied"], false);
    assert_eq!((v["plan"]["tracks"].as_u64(), v["plan"]["playlists"].as_u64()), (Some(2), Some(1)));
    assert!(!out.exists(), "a dry run writes nothing");

    let (v, ok) = run(dir.path(), &["export", drive, "Set", "--format", "traktor", "-o", out_s]);
    assert!(ok, "{v}");
    assert_eq!(v["applied"], true);
    let nml = std::fs::read_to_string(out.join("collection.nml")).unwrap();
    assert!(nml.contains("<NODE TYPE=\"PLAYLIST\" NAME=\"Set\">"), "{nml}");

    // Not again without --replace, and the list formats' options don't mix in.
    let (v, ok) = run(dir.path(), &["export", drive, "--format", "traktor", "-o", out_s]);
    assert!(!ok);
    assert_eq!(v["error"]["code"], "conflict", "{v}");
    let (_, ok) = run(dir.path(), &["export", drive, "--format", "traktor", "-o", out_s, "--replace"]);
    assert!(ok);
    let (_, ok) = run(dir.path(), &["export", drive, "--format", "traktor", "-o", out_s, "--split"]);
    assert!(!ok);
    let (_, ok) = run(dir.path(), &["export", drive, "-o", dir.path().join("x.m3u8").to_str().unwrap(), "--dry-run"]);
    assert!(!ok, "--dry-run is for other DJ software");
    let (_, ok) = run(dir.path(), &["export", drive, "--format", "traktor", "-o", out_s, "--write-tags"]);
    assert!(!ok, "Traktor's cues aren't tags");
}

#[test]
fn export_for_serato_writes_crates_and_asks_before_copying() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dj_drive(dir.path(), "Main");
    let (drive, out) = (drive.to_str().unwrap(), dir.path().join("serato"));
    let out_s = out.to_str().unwrap();

    // Copying the audio for tags needs --yes (JSON never prompts): not applied, exit status 3.
    let (v, ok) = run(dir.path(), &["export", drive, "--format", "serato", "-o", out_s, "--write-tags"]);
    assert!(!ok, "{v}");
    assert_eq!((&v["applied"], &v["outcome"]), (&Value::Bool(false), &Value::from("not_confirmed")));
    assert!(v["plan"]["copy_bytes"].as_u64().unwrap() > 0);
    assert!(!out.exists());

    let (v, ok) = run(dir.path(), &["export", drive, "--format", "serato", "-o", out_s]);
    assert!(ok, "{v}");
    assert_eq!(v["report"]["files"][0]["what"], "crate Set (2 tracks)");
    assert!(out.join("_Serato_/Subcrates/Set.crate").is_file());
    assert!(!out.join("Contents").exists());
}

#[test]
fn export_for_engine_dj() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dj_drive(dir.path(), "Main");
    let (drive, out) = (drive.to_str().unwrap(), dir.path().join("engine"));
    let out_s = out.to_str().unwrap();
    let (_, ok) = run(dir.path(), &["export", drive, "--format", "engine", "-o", out_s, "--paths", "absolute"]);
    assert!(!ok, "Engine's paths are relative");
    let (_, ok) = run(dir.path(), &["export", drive, "--format", "traktor", "-o", out_s, "--paths", "drive"]);
    assert!(!ok, "Traktor's are full paths");
    let (v, ok) = run(dir.path(), &["export", drive, "--format", "engine", "-o", out_s, "--paths", "drive"]);
    assert!(ok, "{v}");
    assert_eq!((v["report"]["tracks"].as_u64(), v["report"]["playlists"].as_u64()), (Some(2), Some(1)));
    assert!(out.join("Engine Library/Database2/m.db").is_file());
}

#[test]
fn rpc_plans_an_export_for_traktor() {
    let dir = tempfile::tempdir().unwrap();
    let drive = dj_drive(dir.path(), "Main");
    let out = dir.path().join("traktor");
    let mut child = locallibrary(dir.path()).arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        let params = serde_json::json!({ "drive": drive, "software": "traktor", "output": out, "skip_analysis": true });
        writeln!(
            stdin,
            "{}",
            serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "interop.plan", "params": params })
        )
        .unwrap();
    }
    drop(child.stdin.take());
    let out_text = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
    // The response, not the progress notifications about it.
    let v: Value =
        out_text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).find(|v| v["id"] == 1).expect("a response");
    assert_eq!(v["result"]["software"], "traktor", "{v}");
    assert_eq!(v["result"]["beat_grids"], 0);
    assert!(!out.exists());
}

/// Regression: a relative output folder that didn't exist yet (`-o trak`) was always refused at
/// apply, as the folder above it the plan measured was the empty path.
#[test]
fn a_relative_output_folder_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let root = dj_drive(dir.path(), "Main");
    let out = locallibrary(dir.path())
        .current_dir(dir.path())
        .args(["--json", "--yes", "export", root.to_str().unwrap(), "--format", "traktor", "-o", "trak"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["outcome"], "applied", "{v}");
    assert!(dir.path().join("trak/collection.nml").is_file());
}

/// Regression: `--write-tags` kept every staged copy open until all were written, so a library of
/// more tracks than a process may have files open (256 for a macOS app) failed with "Too many
/// open files". Here the limit is 64 and there are 100 copies.
#[cfg(unix)]
#[test]
fn copies_for_serato_outnumber_the_open_file_limit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dj_drive(dir.path(), "Main");
    let pdb = root.join("PIONEER/rekordbox/export.pdb");
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    std::fs::create_dir_all(root.join("Contents/Many")).unwrap();
    for n in 0..100 {
        let file = format!("Contents/Many/{n}.mp3");
        ed.add_track(&NewTrack::new(format!("Track {n}"), format!("/{file}"))).unwrap();
        std::fs::write(root.join(file), "audio").unwrap();
    }
    ed.save(&pdb).unwrap();
    let to = dir.path().join("serato");
    // The CLI as the tests run it, under a lower open-file limit.
    let cli = locallibrary(dir.path());
    let mut sh = Command::new("sh");
    sh.args(["-c", "ulimit -n 64 && exec \"$@\"", "sh"])
        .arg(cli.get_program())
        .args(["--json", "--yes", "export", root.to_str().unwrap(), "--format", "serato", "--write-tags"])
        .args(["--no-analysis", "-o", to.to_str().unwrap()])
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
