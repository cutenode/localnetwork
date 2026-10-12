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
