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
mod widgets;

use std::io::stdout;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    let signalled = on_signals();
    let result = event_loop(&mut terminal, &mut app, &signalled);
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
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

/// A flag raised when the terminal closes (SIGHUP) or something asks the player to stop
/// (SIGTERM), instead of the process ending on the spot: a drive write under way gets to finish
/// and the state is saved, as when quitting with `q`.
fn on_signals() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    for signal in [signal_hook::consts::SIGHUP, signal_hook::consts::SIGTERM] {
        let _ = signal_hook::flag::register(signal, flag.clone());
    }
    flag
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App, signalled: &AtomicBool) -> Result<()> {
    while !app.quit {
        if signalled.swap(false, Ordering::Relaxed) {
            app.request_quit();
        }
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
}
