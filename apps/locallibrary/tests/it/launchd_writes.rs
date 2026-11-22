//! On macOS, a command that writes drives, run from a terminal, runs as a launchd job of a copy of
//! locallibrary that launchd wrote, so macOS adds no `._` files (`src/launchd.rs`). These run the
//! real thing in a terminal (a pty). The two that write a FAT32 disk image through launchd need a
//! launchd GUI session (GitHub's macOS runners log their user in) and
//! `LOCALLIBRARY_DISK_IMAGE_TESTS=1`, as every test that attaches disk images does, and say why
//! when they're skipped. The one where launchd loses track of a job needs the session only, and
//! the one where launchd can't take the write runs on every Mac.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::support::{Env, PDB, demo_drive, path};

/// Why these tests can't run here, if they can't.
fn skipped() -> bool {
    let why = if std::env::var_os("LOCALLIBRARY_DISK_IMAGE_TESTS").is_none_or(|v| v != "1") {
        Some("set LOCALLIBRARY_DISK_IMAGE_TESTS=1 to attach disk images")
    } else {
        // SAFETY: getuid has no preconditions and can't fail.
        let session = format!("gui/{}", unsafe { libc::getuid() });
        let there = Command::new("launchctl").args(["print", &session]).output().is_ok_and(|o| o.status.success());
        (!there).then_some("no launchd GUI session to run jobs in")
    };
    if let Some(why) = why {
        eprintln!("skipped: {why}");
    }
    why.is_some()
}

/// A FAT32 disk image attached at `mount`, detached (and its file removed) when dropped.
struct Image {
    mount: PathBuf,
    file: PathBuf,
}

impl Image {
    fn attach(mount: &Path) -> Image {
        let file = mount.with_extension("sparseimage");
        let made = Command::new("hdiutil")
            .args(["create", "-quiet", "-size", "2g", "-type", "SPARSE", "-fs", "MS-DOS FAT32", "-volname", "LLTEST"])
            .arg(file.with_extension(""))
            .status()
            .unwrap();
        assert!(made.success(), "hdiutil create");
        std::fs::create_dir_all(mount).unwrap();
        let attached = Command::new("hdiutil")
            .args(["attach", "-quiet", "-nobrowse", "-mountpoint"])
            .arg(mount)
            .arg(&file)
            .status();
        assert!(attached.unwrap().success(), "hdiutil attach");
        Image { mount: mount.to_path_buf(), file }
    }

    /// The AppleDouble files on it.
    fn apple_double(&self) -> usize {
        walkdir::WalkDir::new(&self.mount)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("._"))
            .count()
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&self.mount).status();
        let _ = std::fs::remove_file(&self.file);
    }
}

/// `c` run in a terminal of its own: its exit code and all it printed. `interrupt` is a line
/// to wait for before pressing Ctrl-C.
fn in_terminal(c: &Command, interrupt: Option<&str>) -> (u32, u32, String) {
    let pair = native_pty_system().openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 }).unwrap();
    let mut b = CommandBuilder::new(c.get_program());
    b.args(c.get_args());
    for (k, v) in c.get_envs() {
        match v {
            Some(v) => b.env(k, v),
            None => b.env_remove(k),
        }
    }
    // As a terminal says what it is (the tests' commands have none): progress is drawn only
    // for one that does.
    b.env("TERM", "xterm-256color");
    if let Some(d) = c.get_current_dir() {
        b.cwd(d);
    }
    let mut child = pair.slave.spawn_command(b).unwrap();
    drop(pair.slave);
    let pid = child.process_id().unwrap();
    let out = Arc::new(Mutex::new(String::new()));
    let mut reader = pair.master.try_clone_reader().unwrap();
    let read = {
        let out = out.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                out.lock().unwrap().push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        })
    };
    let mut writer = pair.master.take_writer().unwrap();
    if let Some(line) = interrupt {
        let start = Instant::now();
        while !out.lock().unwrap().contains(line) {
            assert!(start.elapsed() < Duration::from_secs(120), "no {line:?} in:\n{}", out.lock().unwrap());
            std::thread::sleep(Duration::from_millis(20));
        }
        writer.write_all(b"\x03").unwrap();
    }
    let code = child.wait().unwrap().exit_code();
    drop(writer);
    drop(pair.master);
    let _ = read.join();
    let out = out.lock().unwrap().clone();
    (code, pid, out)
}

/// `out` without the terminal's escape sequences (colours, cursor moves).
fn plain(out: &str) -> String {
    let mut text = String::new();
    let mut chars = out.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                // `ESC [`, numbers, then one letter.
                if chars.next() == Some('[') {
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            c => text.push(c),
        }
    }
    text
}

/// What `launchctl list` has of the jobs a run started.
fn jobs_of(pid: u32) -> Vec<String> {
    let list = Command::new("launchctl").arg("list").output().unwrap();
    let label = format!("local.locallibrary.write.{pid}");
    String::from_utf8_lossy(&list.stdout).lines().filter(|l| l.contains(&label)).map(String::from).collect()
}

/// A sync from a terminal writes the drive from a launchd job of the copy, and leaves no `._`
/// file on it.
#[test]
fn a_drive_write_from_a_terminal_leaves_no_apple_double_files() {
    if skipped() {
        return;
    }
    let env = Env::new();
    let source = demo_drive(&env.volumes, "A");
    let image = Image::attach(&env.volumes.join("T"));
    let home = env.outside("home");
    std::fs::create_dir(&home).unwrap();
    let mut c = env.command();
    c.env("HOME", &home).args(["-y", "sync", "--from", path(&source), path(&image.mount)]);
    let (code, pid, out) = in_terminal(&c, None);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("writing from here"), "{out}");
    let copy = home.join("Library/Application Support/locallibrary-launchd/bin/locallibrary");
    assert!(copy.is_file(), "the write ran from the copy:\n{out}");
    assert!(image.mount.join(PDB).is_file(), "{out}");
    assert_eq!(image.apple_double(), 0, "{out}");
    // The command ran once, as the job: not again here after it (which would plan the sync again).
    assert_eq!(plain(&out).matches("A → T").count(), 1, "{out}");
    assert!(jobs_of(pid).is_empty());
}

/// Ctrl-C in the terminal stops the job's write as it would stop one here: it says it's stopping,
/// exits 130, and leaves no job behind.
#[test]
fn ctrl_c_stops_a_drive_write_s_job() {
    if skipped() {
        return;
    }
    let env = Env::new();
    let source = demo_drive(&env.volumes, "A");
    // A track big enough that the write is still copying when Ctrl-C comes.
    let audio = walkdir::WalkDir::new(source.join("Contents"))
        .into_iter()
        .filter_map(Result::ok)
        .find(|e| e.file_type().is_file())
        .unwrap();
    std::fs::OpenOptions::new().write(true).open(audio.path()).unwrap().set_len(1 << 30).unwrap();
    let image = Image::attach(&env.volumes.join("T"));
    let home = env.outside("home");
    std::fs::create_dir(&home).unwrap();
    let mut c = env.command();
    c.env("HOME", &home).args(["-y", "sync", "--from", path(&source), path(&image.mount)]);
    let (code, pid, out) = in_terminal(&c, Some("Copying"));
    assert_eq!(code, 130, "{out}");
    assert!(out.contains("stopping safely"), "{out}");
    assert!(jobs_of(pid).is_empty(), "{:?}", jobs_of(pid));
}

/// A write launchd can't take (here a `launchctl` that refuses everything) runs from the terminal
/// as before, and says so. It needs neither a disk image nor a launchd session, so it runs on
/// every Mac.
#[test]
fn a_write_launchd_can_t_take_runs_here_and_says_so() {
    let env = Env::new();
    let source = demo_drive(&env.volumes, "A");
    let target = crate::support::blank_drive(&env.volumes, "T");
    let home = env.outside("home");
    std::fs::create_dir(&home).unwrap();
    let tools = env.outside("tools");
    std::fs::create_dir(&tools).unwrap();
    let refusing = tools.join("launchctl");
    std::fs::write(&refusing, "#!/bin/sh\necho refused >&2\nexit 1\n").unwrap();
    std::fs::set_permissions(&refusing, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let search =
        std::env::join_paths(std::iter::once(tools).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())));
    let mut c = env.command();
    c.env("HOME", &home).env("PATH", search.unwrap()).args(["-y", "sync", "--from", path(&source), path(&target)]);
    let (code, _, out) = in_terminal(&c, None);
    assert_eq!(code, 0, "{out}");
    assert!(plain(&out).contains("writing from here, not a launchd job"), "{out}");
    assert!(target.join(PDB).is_file(), "{out}");
    assert!(!home.join("Library/Application Support/locallibrary-launchd/bin/locallibrary").exists());
}

/// A job launchd started but then can't say anything of fails the command, saying the write may
/// still be going, rather than waiting forever. The copy is made first, by the real launchd; then a
/// `launchctl` that starts nothing and knows of nothing stands in for it.
#[test]
fn a_job_launchd_loses_track_of_fails_and_says_so() {
    // SAFETY: getuid has no preconditions and can't fail.
    let session = format!("gui/{}", unsafe { libc::getuid() });
    if !Command::new("launchctl").args(["print", &session]).output().is_ok_and(|o| o.status.success()) {
        eprintln!("skipped: no launchd GUI session to run jobs in");
        return;
    }
    let env = Env::new();
    let source = demo_drive(&env.volumes, "A");
    let target = crate::support::blank_drive(&env.volumes, "T");
    let home = env.outside("home");
    std::fs::create_dir(&home).unwrap();
    let args = ["-y", "sync", "--from", path(&source), path(&target), "--dry-run"];
    let (code, _, out) = in_terminal(env.command().env("HOME", &home).args(args), None);
    assert_eq!(code, 0, "the copy is made: {out}");
    let tools = env.outside("tools");
    std::fs::create_dir(&tools).unwrap();
    let forgetful = tools.join("launchctl");
    std::fs::write(&forgetful, "#!/bin/sh\ncase \"$1\" in print) exit 1 ;; *) exit 0 ;; esac\n").unwrap();
    std::fs::set_permissions(&forgetful, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let search =
        std::env::join_paths(std::iter::once(tools).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())));
    let (code, _, out) = in_terminal(env.command().env("HOME", &home).env("PATH", search.unwrap()).args(args), None);
    assert_eq!(code, 1, "{out}");
    assert!(plain(&out).contains("lost track of the launchd job"), "{out}");
}
