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
mod form;
mod keymap;
mod keys;
mod mix;
mod mytags;
mod palette;
mod plugins;
mod profile;
#[cfg(test)]
mod props;
#[cfg(test)]
mod screens;
mod state;
mod stderr;
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
    // Plugins' logs would draw over the screen: they go to the messages (`!`) while it runs.
    crate::plugins::log_to(Some(app.plugin_log()));
    crate::plugins::events::notices_to(Some(app.plugin_notices()));
    crate::plugins::ask_to(Some(app.plugin_asker()));
    app.start_plugins();
    app.warn_signing_out();
    // (Not `ratatui::init`, whose panic hook would put the screen back for a worker's panic.)
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;
    // What's written to stderr (alsa-lib's, with no sound card) would draw over the screen too.
    let log = app.plugin_log();
    stderr::capture(move |line| log("stderr", line));
    let _ = execute!(stdout(), EnableMouseCapture);
    SIGNAL.store(RUNNING, Ordering::Relaxed);
    let result = event_loop(&mut terminal, &mut app);
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    stderr::restore();
    crate::plugins::log_to(None);
    crate::plugins::events::notices_to(None);
    crate::plugins::ask_to(None);
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
            stderr::restore();
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
mod tests;
