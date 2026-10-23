use std::process::Command;

use super::*;

/// A file that crashes the decoder, opened on a worker as the player opens tracks, is that
/// file's error and leaves the screen to the player, even with the library's quiet panic hook
/// installed under the player's. Panic hooks are global, so this runs in a process of its own.
#[test]
fn a_decoder_crash_on_a_worker_leaves_the_screen_alone() {
    const ALONE: &str = "LOCALLIBRARY_TEST_PANIC_HOOKS";
    if std::env::var_os(ALONE).is_none() {
        let name = "tui::tests::a_decoder_crash_on_a_worker_leaves_the_screen_alone";
        let out = Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact", "--nocapture"])
            .env(ALONE, "1")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && said.contains("1 passed"), "{said}{}", String::from_utf8_lossy(&out.stderr));
        // What putting the screen back writes: leaving the alternate screen.
        assert!(!said.contains("\x1b[?1049l"), "the screen was put back: {said:?}");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zero.wav");
    std::fs::write(&path, audio::tests::wav(0, 100, 1)).unwrap();
    // The library's quiet hook first and the player's over it: the worse order, where only
    // the player's own check (the UI thread or not) keeps the screen.
    let _ = locallibrary::probe::guarded(&path, || Ok::<_, locallibrary::Error>(()));
    on_panic();
    let opened = std::thread::spawn(move || audio::open_file(&path).map(|_| ())).join().unwrap();
    assert!(format!("{:#}", opened.unwrap_err()).contains("crashed"));
}

/// What's written straight to stderr while the player runs (alsa-lib's complaints with no sound
/// card) goes to the messages, not over the screen. Once stderr is put back, or the UI thread
/// panics, it's the terminal again, so the panic's message is seen. stderr is the process's, so
/// this runs in a process of its own.
#[cfg(unix)]
#[test]
fn stderr_goes_to_the_messages_while_the_player_runs() {
    const ALONE: &str = "LOCALLIBRARY_TEST_STDERR";
    if std::env::var_os(ALONE).is_none() {
        let name = "tui::tests::stderr_goes_to_the_messages_while_the_player_runs";
        let out = Command::new(std::env::current_exe().unwrap())
            .args([name, "--exact", "--nocapture"])
            .env(ALONE, "1")
            .output()
            .unwrap();
        let (said, err) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && said.contains("1 passed"), "{said}{err}");
        assert!(!err.contains("cannot find card"), "written over the screen: {err}");
        assert!(err.contains("after the player") && err.contains("the player broke"), "{err}");
        return;
    }
    // As alsa-lib writes: to the file descriptor, past Rust's stderr.
    // SAFETY: writing a live buffer to a descriptor this process owns.
    let write = |s: &str| unsafe { libc::write(2, s.as_ptr().cast(), s.len()) };
    let (tx, rx) = std::sync::mpsc::channel();
    let logged = || rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let sender = tx.clone();
    stderr::capture(move |line| sender.send(line.to_string()).unwrap());
    write("ALSA lib confmisc.c:855:(parse_card) cannot find card '0'\n");
    assert_eq!(logged(), "ALSA lib confmisc.c:855:(parse_card) cannot find card '0'");
    stderr::restore();
    write("after the player\n");

    on_panic();
    stderr::capture(move |line| tx.send(line.to_string()).unwrap());
    write("cannot find card '1'\n");
    assert_eq!(logged(), "cannot find card '1'");
    assert!(std::panic::catch_unwind(|| panic!("the player broke")).is_err());
}

/// A signal while the player runs is the player's, and quits it as `q` does; before it runs
/// (and after), it's the CLI's handler's.
#[test]
fn signals_quit_the_running_player() {
    assert!(!signal(), "not running yet");
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    SIGNAL.store(RUNNING, Ordering::Relaxed);
    on_signal(&mut app);
    assert!(!app.quit);
    assert!(signal() && signal(), "every signal while it runs");
    on_signal(&mut app);
    assert!(app.quit);
    SIGNAL.store(IDLE, Ordering::Relaxed);
    assert!(!signal());
}

/// Signals in any order, as the player runs and stops: while it runs each is the player's, which
/// quits as q does once it looks; before and after, they're the CLI handler's.
#[hegel::test(test_cases = 200)]
fn signals_go_to_the_player_only_while_it_runs(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    SIGNAL.store(IDLE, Ordering::Relaxed);
    let (mut running, mut pending) = (false, false);
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => {
                SIGNAL.store(if running { IDLE } else { RUNNING }, Ordering::Relaxed);
                (running, pending) = (!running, false);
            }
            1 | 2 => {
                assert_eq!(signal(), running, "running {running}");
                pending |= running;
            }
            _ => {
                let quit = app.quit;
                on_signal(&mut app);
                assert_eq!(app.quit, quit || pending, "pending {pending}");
                pending = false;
            }
        }
    }
    SIGNAL.store(IDLE, Ordering::Relaxed);
}
