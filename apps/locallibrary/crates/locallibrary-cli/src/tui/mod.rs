//! `locallibrary tui`: browse drives, play tracks with their rekordbox waveforms, and watch the
//! store, in the terminal.

// Without the `account` feature, a stand-in that says there's no account.
#[cfg_attr(not(feature = "account"), path = "account_off.rs")]
mod account;
mod analyzer;
mod app;
mod audio;
mod browse;
mod crate_list;
mod deck;
mod drives;
mod edits;
mod keymap;
mod keys;
mod mix;
mod mytags;
mod palette;
mod profile;
#[cfg(test)]
mod screens;
mod state;
mod theme;
mod ui;

use std::io::stdout;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};

use self::app::App;

/// Run the player. `store_root` is the store's directory, where the player also remembers what
/// it was showing (see `state.rs`).
pub fn run(store_root: PathBuf, drive: Option<String>) -> Result<()> {
    theme::init();
    on_panic();
    // Open the audio device before taking over the screen: some backends print while starting.
    let mut app = App::new(store_root, drive);
    // (Not `ratatui::init`, whose panic hook would put the screen back for a worker's panic.)
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
    let _ = execute!(stdout(), EnableMouseCapture);
    SIGNAL.store(RUNNING, Ordering::Relaxed);
    let result = event_loop(&mut terminal, &mut app);
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    SIGNAL.store(IDLE, Ordering::Relaxed);
    // The loop also ends when the terminal goes away mid-write: the write still finishes.
    app.finish_write();
    app.save_state();
    if let Some(m) = &app.exit_message {
        eprintln!("{m}");
    }
    result
}

/// A panic on the UI thread ends the player: the screen is put back before the message prints.
/// One on a worker leaves the screen to the UI, which keeps running and says what went wrong
/// (see `app::caught`). Installed before any worker starts, so the quiet hook the library sets
/// on its first PDB parse goes over this one, and the panics it catches stay quiet.
fn on_panic() {
    let ui = std::thread::current().id();
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == ui {
            let _ = execute!(stdout(), DisableMouseCapture);
            ratatui::restore();
            hook(info);
        } else {
            app::note_panic(info);
        }
    }));
}

/// Where a signal goes (Ctrl-C, SIGTERM, or SIGHUP as the terminal closes): to the CLI's handler,
/// or while the player runs, to the player (see [`signal`]).
static SIGNAL: AtomicU8 = AtomicU8::new(IDLE);
const IDLE: u8 = 0;
const RUNNING: u8 = 1;
const SIGNALLED: u8 = 2;

/// A signal, from the CLI's handler: true when it's the player's, which then quits as with `q`
/// (a drive write under way gets to finish and the state is saved) and puts the terminal back.
/// The handler's own way (a message, then exiting on a second signal) would write over the
/// screen and leave the terminal raw.
pub fn signal() -> bool {
    matches!(SIGNAL.compare_exchange(RUNNING, SIGNALLED, Ordering::Relaxed, Ordering::Relaxed), Ok(_) | Err(SIGNALLED))
}

/// Quit, once signalled.
fn on_signal(app: &mut App) {
    if SIGNAL.compare_exchange(SIGNALLED, RUNNING, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
        app.request_quit();
    }
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.quit {
        on_signal(app);
        app.tick();
        terminal.draw(|f| ui::draw(f, app))?;
        // ~60 fps while something moves (unless asked for less motion), otherwise just often
        // enough for clocks and hot-plugging.
        let fast = app.animating() && !theme::reduced_motion();
        let timeout = if fast { Duration::from_millis(16) } else { Duration::from_millis(250) };
        if event::poll(timeout)? {
            // Handle everything that queued up before drawing again.
            loop {
                match event::read()? {
                    Event::Key(k) if k.kind != KeyEventKind::Release => app.on_key(k),
                    Event::Mouse(m) => app.on_mouse(m),
                    _ => {}
                }
                if app.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
            assert!(
                out.status.success() && said.contains("1 passed"),
                "{said}{}",
                String::from_utf8_lossy(&out.stderr)
            );
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
}
