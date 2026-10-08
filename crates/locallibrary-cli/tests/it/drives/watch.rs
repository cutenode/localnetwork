//! `watch` in words: what it says of each drive it looks at, and drives plugged in and out
//! while it runs. Its JSON events are tested in `cli`.

use crate::support::{Env, line, path, play};

#[test]
fn watch_says_what_each_drive_holds_and_syncs_the_group_after_a_pull() {
    let env = Env::new();
    let main = env.pulled("Main");
    let backup = env.blank("Backup");
    env.ok(&["--yes", "sync", "Main", path(&backup)]);
    play(&main, "HISTORY 002", &[]);
    play(&main, "HISTORY 003", &[]);
    let out = env.ok(&["watch", "--once", "--settle", "0"]);
    let said = line(&out, "Main: holds changes Main lacks");
    assert!(said.ends_with("(2 new history lists: HISTORY 002, HISTORY 003); --pull saves them"), "{out}");
    assert!(line(&out, "Backup: ").contains("the store holds it (Main)"), "{out}");

    let out = env.ok(&["watch", "--once", "--settle", "0", "--pull", "--sync-group"]);
    assert!(line(&out, "Main: pulled into Main as snapshot").contains("(2 new history lists"), "{out}");
    assert!(line(&out, "Main: synced to").ends_with("synced to Backup"), "{out}");
    assert!(env.ok(&["history", path(&backup)]).contains("HISTORY 003"));

    // A change that isn't a history list.
    env.ok(&["--yes", "playlist", "new", "Main", "New"]);
    let out = env.ok(&["watch", "--once", "--settle", "0"]);
    assert!(line(&out, "Main: holds changes").ends_with("Main lacks; --pull saves them"), "{out}");
}

/// A `watch` running until it's stopped: by the test, or killed when the test fails first.
#[cfg(unix)]
struct Watching(std::process::Child);

#[cfg(unix)]
impl Drop for Watching {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Runs until stopped, saying when drives come and go, in words or (`--json`) one event a line.
/// The test waits on what it says, never on a clock (the receive timeout only keeps a broken run
/// from hanging the suite).
#[cfg(unix)]
#[test]
fn watch_says_when_drives_are_plugged_in_and_out_until_stopped() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::support::copy_dir;

    let env = Env::new();
    let main = env.drive("Main");
    copy_dir(&main, &env.outside("USB"));
    let usb = env.volumes.join("USB");
    for json in [false, true] {
        let mut command = env.command();
        command.args(if json { &["--json"][..] } else { &[] }).args(["watch", "--settle", "0"]);
        let mut watching = Watching(command.stdout(Stdio::piped()).spawn().unwrap());
        let (tx, lines) = mpsc::channel();
        let stdout = watching.0.stdout.take().unwrap();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines() {
                let _ = tx.send(l.unwrap());
            }
        });
        let next = || lines.recv_timeout(Duration::from_secs(60)).expect("watch said nothing");
        // Waits for `name` to be said, in words, to have `text` happen; in JSON, the `event`.
        let wait_for = |name: &str, text: &str, event: &str| loop {
            let l = next();
            let said = match json {
                false => l.contains(&format!(" {name}{text}")),
                true => {
                    let v: serde_json::Value = serde_json::from_str(&l).unwrap();
                    v["name"] == name && v["event"] == event
                }
            };
            if said {
                break;
            }
        };
        if !json {
            assert_eq!(next(), "Watching for drives (--pull to save them in the store). Ctrl-C to stop.");
        }
        // Once it has looked at what's plugged in now, a drive plugged in all at once, as a
        // mount is, and pulled out.
        wait_for("Main", ": left alone", "watch.skipped");
        std::fs::rename(env.outside("USB"), &usb).unwrap();
        wait_for("USB", " plugged in", "drive.attached");
        std::fs::rename(&usb, env.outside("USB")).unwrap();
        wait_for("USB", " unplugged", "drive.detached");
        // SAFETY: a signal to the child this test started.
        unsafe { libc::kill(watching.0.id() as i32, libc::SIGINT) };
        assert!(watching.0.wait().unwrap().success(), "Ctrl-C stops it cleanly");
    }
}
