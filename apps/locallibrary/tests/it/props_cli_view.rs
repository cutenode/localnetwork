//! `store browse`, `store views` and `store unbrowse` as commands: a view opens where it says, is
//! listed until it's gone, and removes itself on Ctrl-C, and a run killed outright is still found.

use std::io::{BufRead, BufReader, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{ChildStdout, Stdio};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, Running, line, path};

/// The newest snapshot of drive A, by the id `log` shows.
fn snapshot(env: &Env) -> String {
    env.ok(&["log", "A"]).lines().last().unwrap().split_whitespace().next().unwrap().to_string()
}

/// The file tree under `dir`, each file with its size through its link.
fn tree(dir: &Path) -> Vec<(String, u64)> {
    let mut found: Vec<(String, u64)> = walkdir::WalkDir::new(dir)
        .into_iter()
        .map(Result::unwrap)
        .filter(|e| !e.file_type().is_dir())
        .map(|e| {
            let key = e.path().strip_prefix(dir).unwrap().to_str().unwrap().to_string();
            (key, std::fs::metadata(e.path()).unwrap().len())
        })
        .collect();
    found.sort();
    found
}

/// The store's manifest of `snapshot`.
fn manifest(env: &Env, snapshot: &str) -> PathBuf {
    std::fs::read_dir(env.store.join("snapshots"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap().to_str().unwrap().starts_with(snapshot))
        .unwrap()
}

/// What the store's manifest of `snapshot` lists, as [`tree`] does.
fn listed(env: &Env, snapshot: &str) -> Vec<(String, u64)> {
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest(env, snapshot)).unwrap()).unwrap();
    let files = manifest["files"].as_array().unwrap().iter();
    let mut files: Vec<(String, u64)> =
        files.map(|f| (f["path"].as_str().unwrap().to_string(), f["size"].as_u64().unwrap())).collect();
    files.sort();
    files
}

/// `store browse` with a terminal's output gone to a pipe, until its first line of `--json`.
fn browsing(env: &Env, args: &[&str]) -> (Running, BufReader<ChildStdout>, PathBuf) {
    let temp = env.outside("temp");
    std::fs::create_dir_all(&temp).unwrap();
    let mut command = env.command();
    command.args(["--json", "store", "browse"]).args(args).env("TMPDIR", &temp).stdout(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    out.read_line(&mut first).unwrap();
    let opened: Value = serde_json::from_str(&first).unwrap();
    (Running(child), out, PathBuf::from(opened["path"].as_str().unwrap()))
}

fn signal(child: &Running, signal: i32) {
    // SAFETY: a signal to the child this test started, which hasn't been waited for yet.
    assert_eq!(unsafe { libc::kill(i32::try_from(child.id()).unwrap(), signal) }, 0);
}

/// `--keep` leaves a view that says where it is and how it goes. It's the snapshot's tree, `store
/// views` and `store info` say it's there, and `store unbrowse` removes it, by name or with the
/// rest. A file in the view can't be written.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn a_kept_view_is_said_listed_and_removed(tc: TestCase) {
    let env = Env::from_template();
    let id = snapshot(&env);
    let to = env.outside("view");
    let chosen: bool = tc.draw(gs::booleans());
    let mut args = vec!["store", "browse", &id, "--keep"];
    if chosen {
        args.extend(["--to", path(&to)]);
    }
    let out = env.ok(&args);
    assert!(out.contains(&format!("view of snapshot {id}")) && out.contains("nothing copied"), "{out}");
    let at = PathBuf::from(line(&out, "/").trim());
    assert_eq!(at == to, chosen, "{out}");
    let run = env.run(&["store", "browse", &id, "--keep", "--to", path(&env.outside("second"))]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.contains("Remove it with `locallibrary store unbrowse`."), "{}", run.out);
    assert!(run.out.contains("pruning this snapshot leaves its space in use"), "{}", run.out);
    assert!(run.out.contains(&format!("a view of snapshot {id} is open at {}", at.display())), "{}", run.out);
    assert_eq!(tree(&at), listed(&env, &id));

    let views = env.ok(&["store", "views"]);
    assert!(views.contains(path(&at)) && views.contains(&format!("snapshot {id}, opened ")), "{views}");
    let views: Value = serde_json::from_str(&env.ok(&["--json", "store", "views"])).unwrap();
    assert_eq!(views.as_array().unwrap().len(), 2);
    assert!(env.ok(&["store", "info"]).contains("is open at"));

    let by_name: bool = tc.draw(gs::booleans());
    if by_name {
        let out = env.ok(&["store", "unbrowse", path(&at)]);
        assert!(out.contains(&format!("removed the view at {}", at.display())), "{out}");
        assert!(!at.exists());
    }
    assert!(env.ok(&["store", "unbrowse"]).contains("removed the view at"));
    assert!(!at.exists() && !env.outside("second").exists());
    assert!(env.ok(&["store", "views"]).contains("No views open."));
    assert!(env.ok(&["store", "unbrowse"]).contains("No views open."));
    let err = env.fails(1, &["store", "unbrowse", path(&at)]);
    assert!(err.contains(&format!("a view at {}", at.display())), "{err}");
    assert!(!env.ok(&["store", "info"]).contains("is open at"));
}

/// Without `--keep` the view is there until Ctrl-C, SIGTERM, a closed terminal, or a `store
/// unbrowse` elsewhere, and goes with it, also when that leaves a file someone put there. It's made
/// in the temporary folder unless told otherwise.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_view_goes_when_the_command_stops(tc: TestCase) {
    let env = Env::from_template();
    let id = snapshot(&env);
    for how in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, 0] {
        let (mut child, mut out, at) = browsing(&env, &[&id]);
        assert!(at.starts_with(env.outside("temp")), "{}", at.display());
        assert_eq!(tree(&at), listed(&env, &id));
        let theirs = at.join("mine.txt");
        let left: bool = how == 0 && tc.draw(gs::booleans());
        if left {
            std::fs::set_permissions(&at, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
            std::fs::write(&theirs, "mine").unwrap();
        }
        match how {
            0 => _ = env.ok(&["store", "unbrowse", path(&at)]),
            how => signal(&child, how),
        }
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut out, &mut rest).unwrap();
        assert!(child.wait().unwrap().success(), "{rest}");
        assert_eq!(theirs.exists(), left, "{}", at.display());
        assert_eq!(at.exists(), left, "{}", at.display());
        let closed: Value = serde_json::from_str(&rest).unwrap();
        assert_eq!(closed["report"]["path"], path(&at), "{rest}");
        assert!(env.ok(&["store", "views"]).contains("No views open."));
    }
}

/// A run killed outright can't tidy up, so its view is said until it's removed, and pruning its
/// snapshot says what that leaves.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_view_left_by_a_killed_run_is_still_found(_tc: TestCase) {
    let env = Env::from_template();
    let id = snapshot(&env);
    let listed_when_opened = listed(&env, &id);
    let (mut child, _out, at) = browsing(&env, &[&id, "--to", path(&env.outside("left"))]);
    signal(&child, libc::SIGKILL);
    child.wait().unwrap();
    assert!(at.exists());
    let info = env.ok(&["store", "info"]);
    assert!(info.contains(&format!("is open at {}", at.display())), "{info}");

    env.ok(&["--yes", "playlist", "new", "A", "Later"]);
    env.ok(&["pull", "A"]);
    let plan = env.ok(&["store", "prune", "--keep-last", "1", "--dry-run"]);
    let said = format!("the view at {} shows a snapshot this removes: the space its files take", at.display());
    assert!(plan.contains(&said), "{plan}");
    env.ok(&["--yes", "store", "prune", "--keep-last", "1"]);
    assert_eq!(tree(&at), listed_when_opened, "a hard link keeps its file");
    assert!(!env.ok(&["store", "views"]).contains("broken link(s)"));
    assert!(env.ok(&["store", "unbrowse"]).contains("removed the view at"));
    assert!(!at.exists());
}

/// In words, a view says where it is and how it goes, and names a file it left out: here one whose
/// path would lead out of the view, as a damaged or crafted manifest could.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn a_view_says_where_it_is_and_what_it_left_out(_tc: TestCase) {
    let env = Env::from_template();
    let id = snapshot(&env);
    let file = manifest(&env, &id);
    let mut snapshot: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let mut outside = snapshot["files"][0].clone();
    outside["path"] = "../outside.mp3".into();
    snapshot["files"].as_array_mut().unwrap().push(outside);
    std::fs::write(&file, snapshot.to_string()).unwrap();

    let temp = env.outside("temp");
    std::fs::create_dir_all(&temp).unwrap();
    let mut command = env.command();
    command.args(["store", "browse", &id]).env("TMPDIR", &temp).stdout(Stdio::piped());
    let mut child = Running(command.spawn().unwrap());
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut said = String::new();
    while !said.contains("It's removed on Ctrl-C.") {
        assert_ne!(out.read_line(&mut said).unwrap(), 0, "{said}");
    }
    assert!(said.contains(&format!("view of snapshot {id}")), "{said}");
    assert!(said.contains("../outside.mp3 was left out: an unsafe path"), "{said}");
    assert!(!temp.join("outside.mp3").exists());

    signal(&child, libc::SIGINT);
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut out, &mut rest).unwrap();
    assert!(child.wait().unwrap().success(), "{rest}");
    assert!(rest.contains("removed the view at"), "{rest}");
    assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0, "nothing is left in the temporary folder");
}

/// A file the store has lost is said and shows as a broken symlink. A symlink leads to the store by
/// its full path, whatever path the store was named by, so it works from anywhere once the file is
/// back, and the view warns that it breaks when pruned.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn a_lost_file_is_a_symlink_that_leads_to_the_store(_tc: TestCase) {
    let env = Env::from_template();
    let id = snapshot(&env);
    let shard = std::fs::read_dir(env.store.join("objects")).unwrap().flatten().find(|e| e.path().is_dir()).unwrap();
    let object = std::fs::read_dir(shard.path()).unwrap().flatten().next().unwrap().path();
    let bytes = std::fs::read(&object).unwrap();
    std::fs::remove_file(&object).unwrap();

    let to = env.outside("view");
    let mut command = env.command();
    command.current_dir(env.store.parent().unwrap()).env("LOCALLIBRARY_STORE", "store");
    let run = command.args(["store", "browse", &id, "--keep", "--to", path(&to)]).output().unwrap();
    assert!(run.status.success());
    let out = String::from_utf8(run.stdout).unwrap();
    assert!(out.contains("isn't in the store, so its link is broken"), "{out}");
    assert!(out.contains("they break if this snapshot is pruned"), "{out}");
    let views = env.ok(&["store", "views"]);
    assert!(views.contains("broken link(s): the store no longer has their files"), "{views}");

    std::fs::write(&object, bytes).unwrap();
    assert_eq!(tree(&to), listed(&env, &id));
    env.ok(&["store", "unbrowse"]);
}

/// Hard links are real files, so a view reads like a drive, but no write gets in: not a command
/// pointed at it, nor a write to one of its files. The store's files stay as they were.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn nothing_writes_into_a_view(tc: TestCase) {
    let env = Env::from_template();
    let id = snapshot(&env);
    let view = env.outside("view");
    env.ok(&["store", "browse", &id, "--keep", "--to", path(&view)]);
    let stored = || {
        let objects = walkdir::WalkDir::new(env.store.join("objects")).sort_by_file_name();
        let files = objects.into_iter().map(Result::unwrap).filter(|e| e.file_type().is_file());
        files.map(|e| std::fs::read(e.path()).unwrap()).collect::<Vec<_>>()
    };
    let before = stored();

    let args: &[&str] = match tc.draw(gs::booleans()) {
        true => &["--yes", "adopt", path(&view)],
        false => &["--yes", "playlist", "new", path(&view), "Later"],
    };
    let run = env.run(args);
    assert_ne!(run.code, 0, "{args:?}\n{}", run.out);
    assert!(run.err.to_lowercase().contains("permission denied"), "{args:?}\n{}", run.err);

    let file = view.join(&listed(&env, &id)[0].0);
    assert_eq!(std::fs::write(&file, b"edited").unwrap_err().kind(), ErrorKind::PermissionDenied);
    assert_eq!(stored(), before);
    env.ok(&["store", "unbrowse"]);
}

/// What a view can't be made of says so, and a chosen folder with something in it is refused.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_view_that_cant_be_made_says_why(_tc: TestCase) {
    let env = Env::from_template();
    let err = env.fails(1, &["store", "browse", "feedface"]);
    assert!(err.contains("snapshot feedface"), "{err}");
    let taken = env.outside("taken");
    std::fs::create_dir(&taken).unwrap();
    std::fs::write(taken.join("mine.txt"), "mine").unwrap();
    let err = env.run(&["store", "browse", &snapshot(&env), "--keep", "--to", path(&taken)]).err;
    assert!(err.contains("isn't empty; choose another folder"), "{err}");
}
