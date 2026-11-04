//! The player run from a shell in a terminal of its own, as a terminal window or ttyd runs it: it
//! quits on `q` and on SIGTERM, and exits once the terminal goes away (a window closed, or ttyd or
//! VHS done with it), instead of spinning on a terminal that only says it has ended.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crate::support::{Env, Running, terminal};

/// Mouse reporting switched on: the player has the screen, and draws next.
const STARTED: &[u8] = b"\x1b[?1000h";

/// An interactive shell in a terminal of its own, as in a terminal window or ttyd, and the job
/// typed into it (the player, and the `sh` that runs it). The job is killed when this is dropped,
/// so a failing test leaves nothing running.
struct Session {
    /// The job's process group, once its leader has said its process ID.
    job: Option<i32>,
    /// The end of the terminal the test holds.
    terminal: std::fs::File,
    shell: Running,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(group) = self.job {
            // SAFETY: a signal to the job the test typed, by its process group.
            unsafe { libc::kill(-group, libc::SIGKILL) };
        }
    }
}

impl Session {
    /// A shell leading its own session, with what the terminal shows read on another thread:
    /// what's seen is said on the channel ("prompt", then "drawn", then "shown" for each write).
    fn new(fx: &Env) -> (Session, Receiver<&'static str>) {
        let (screen, tty) = terminal();
        let size = libc::winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
        // SAFETY: TIOCSWINSZ only reads the size given, for a terminal this opened.
        assert_eq!(unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCSWINSZ, &size) }, 0);
        let mut shell = Command::new("bash");
        for (key, value) in fx.command().get_envs() {
            match value {
                Some(value) => shell.env(key, value),
                None => shell.env_remove(key),
            };
        }
        shell
            .args(["--norc", "--noprofile", "-i"])
            .env("LOCALLIBRARY_AUDIO", "silent")
            // The prompt the test waits for, whoever runs it (root's ends `# `).
            .env("PS1", "$ ")
            .stdin(Stdio::from(tty.try_clone().unwrap()))
            .stdout(Stdio::from(tty.try_clone().unwrap()))
            .stderr(Stdio::from(tty));
        // SAFETY: setsid and ioctl are async-signal-safe, and touch only the child. (TIOCSCTTY's
        // type differs from ioctl's request type on macOS, and matches it on Linux.)
        unsafe {
            shell.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let session = Session { job: None, terminal: screen, shell: Running(shell.spawn().unwrap()) };
        // The command goes with this scope, and its copies of the terminal with it.
        let mut reading = session.terminal.try_clone().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        // To the end, as a terminal reads what's shown until it closes: a shell exiting while its
        // terminal is open waits for what it wrote to be read.
        std::thread::spawn(move || {
            let prompted = read_until(&mut reading, |shown| end_of(shown, b"$ ").is_some());
            if prompted && tx.send("prompt").is_ok() {
                // Something after the mouse switch: a draw has begun.
                if read_until(&mut reading, |shown| end_of(shown, STARTED).is_some_and(|end| shown.len() > end)) {
                    let _ = tx.send("drawn");
                }
            }
            while let Ok(1..) = reading.read(&mut [0; 4096]) {
                let _ = tx.send("shown");
            }
        });
        (session, rx)
    }
}

/// Reads what the terminal shows until `done` with it (true), or the terminal closes (false).
fn read_until(terminal: &mut std::fs::File, done: impl Fn(&[u8]) -> bool) -> bool {
    let (mut shown, mut buf) = (Vec::new(), [0; 4096]);
    while !done(&shown) {
        match terminal.read(&mut buf) {
            Ok(n @ 1..) => shown.extend_from_slice(&buf[..n]),
            _ => return false,
        }
    }
    true
}

/// Where `text` first ends in `shown`.
fn end_of(shown: &[u8], text: &[u8]) -> Option<usize> {
    shown.windows(text.len()).position(|w| w == text).map(|i| i + text.len())
}

/// `locallibrary tui` typed into a [`Session`] as a job of its own, once it waits for input
/// between frames: the session, and where the job's `sh` (which outlives a hangup or SIGTERM)
/// writes how the player exited.
fn idle_player(fx: &Env) -> (Session, std::path::PathBuf) {
    let (mut session, seen) = Session::new(fx);
    assert_eq!(seen.recv_timeout(Duration::from_secs(30)), Ok("prompt"));
    // The `sh` says its process ID, the job's process group, first.
    let (job_file, status_file) = (fx.outside("job"), fx.outside("status"));
    let typed = format!(
        "sh -c 'trap : HUP TERM; echo $$ > \"$0\"; \"$1\" tui; echo $? > \"$2\"' '{}' '{}' '{}'\n",
        job_file.display(),
        std::path::Path::new(fx.command().get_program()).display(),
        status_file.display()
    );
    (&session.terminal).write_all(typed.as_bytes()).unwrap();
    assert_eq!(seen.recv_timeout(Duration::from_secs(30)), Ok("drawn"));
    session.job = Some(std::fs::read_to_string(&job_file).unwrap().trim().parse().unwrap());
    // An idle player draws four frames a second: once one is written, it waits for input, as it
    // mostly does, and isn't part way through writing.
    let started = Instant::now();
    while seen.recv_timeout(Duration::from_millis(100)) == Ok("shown") {
        assert!(started.elapsed() < Duration::from_secs(10), "the player draws without pause");
    }
    (session, status_file)
}

/// How the player exited, once it has (within a few seconds).
fn exit_status(session: &Session, status_file: &std::path::Path) -> u8 {
    let job = session.job.unwrap();
    // SAFETY: signal 0 only asks whether any of the job is there.
    let running = || unsafe { libc::kill(-job, 0) == 0 };
    let started = Instant::now();
    while running() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!running(), "the player still runs");
    std::fs::read_to_string(status_file).unwrap().trim().parse().unwrap()
}

/// Regression: a player whose terminal window closed (or whose ttyd went away) spun at full CPU
/// for hours, deaf to SIGHUP and SIGTERM, until killed. Then, ending, it aborted.
#[test]
fn the_player_exits_when_its_terminal_closes() {
    let fx = Env::new();
    let (session, status_file) = idle_player(&fx);
    // As ttyd does when its page closes (and a terminal window, as it closes): the shell is hung
    // up, which hangs up its jobs and exits. The system then takes the terminal from the player.
    // SAFETY: a signal to the shell the test started, which hasn't been waited for yet.
    assert_eq!(unsafe { libc::kill(i32::try_from(session.shell.id()).unwrap(), libc::SIGHUP) }, 0);
    // Exited, not ended by a signal (an abort as it put the terminal back, say).
    let status = exit_status(&session, &status_file);
    assert!(status < 128, "the player ended with status {status}");
}

/// `q` quits, read from the terminal as keys are.
#[test]
fn the_player_quits_on_q() {
    let fx = Env::new();
    let (session, status_file) = idle_player(&fx);
    (&session.terminal).write_all(b"q").unwrap();
    assert_eq!(exit_status(&session, &status_file), 0);
}

/// SIGTERM, with the terminal still there, quits as `q` does.
#[test]
fn the_player_quits_on_sigterm() {
    let fx = Env::new();
    let (session, status_file) = idle_player(&fx);
    // SAFETY: a signal to the job the test typed, by its process group.
    assert_eq!(unsafe { libc::kill(-session.job.unwrap(), libc::SIGTERM) }, 0);
    assert_eq!(exit_status(&session, &status_file), 0);
}
