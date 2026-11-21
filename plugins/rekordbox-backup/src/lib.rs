//! The rekordbox-backup plugin: backups of rekordbox's library on this computer, made as
//! rekordbox's own File > Library > Backup Library makes them, so rekordbox's Restore Library
//! takes them back. It's written as anyone outside locallibrary would write one: it depends on
//! the SDK crates alone and reads rekordbox's files through the host (`collection:files`).
//!
//! - `backup` (planned first, long-running) writes `rekordbox_bak_YYYYMMDD.zip` into the backup
//!   folder, and with `music` copies the audio into `rekordbox_bak/` beside it.
//! - `backups` lists the backups there.
//! - `verify` checks that one is whole: the zip, its `contents.xml` and its `master.db`.
//! - The `schedule` service makes one (without music) every `every_days`, keeping the newest
//!   `keep` of its own.
//!
//! It never restores: rekordbox's Restore Library does that.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use loclib_plugin::{Context, Hooks, Host, RpcError, RunCommand, View};
use serde::Deserialize;
use serde_json::Value;

mod backup;
mod contents;
mod credentials;
mod entries;
mod list;
mod music;
mod schedule;
mod verify;
mod write;

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    let hooks = Hooks::new().on(run_command).on(schedule::run);
    loclib_plugin::serve(MANIFEST, hooks, input, output);
}

/// What rekordbox's Restore Library needs, said wherever a backup is.
pub(crate) const RESTORE: &str = "To restore one, use rekordbox's File > Library > Restore Library, with \
rekordbox_bak/ (when there is one) in the same folder as the zip.";

fn run_command(req: RunCommand, host: &Host) -> Result<Value, RpcError> {
    match req.command.as_str() {
        "backup" => backup::command(&req, host),
        "backups" => list::command(&req),
        // `verify`, the last of them: the SDK answers only the manifest's commands.
        _ => verify::command(&req, host),
    }
}

/// The plugin's settings, as the host gives them (each one the user set, else its default).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    /// The folder backups go in; the user's Documents folder when not set.
    destination: Option<PathBuf>,
    /// Copy the music too.
    pub music: bool,
    pub credentials: Credentials,
    /// Days between scheduled backups; 0 for none.
    every_days: f64,
    /// How many of its own scheduled backups the schedule keeps; 0 for all.
    keep: f64,
}

/// What a backup does with rekordbox's sign-in tokens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Credentials {
    /// As rekordbox's own backup does: a restore stays signed in.
    #[default]
    Keep,
    /// Out of `master.db`, `rekordbox3.settings` and `activations.xml`: a restore signs in again.
    Strip,
}

impl Settings {
    pub(crate) fn of(context: &Context) -> Result<Settings, RpcError> {
        let map: serde_json::Map<String, Value> = context.settings.clone().into_iter().collect();
        serde_json::from_value(Value::Object(map)).map_err(|e| RpcError::failed("invalid", format!("settings: {e}")))
    }

    /// The backup folder, which must be there: one on a drive that isn't plugged in mustn't be
    /// made on the computer's own disk instead.
    pub(crate) fn destination(&self) -> Result<PathBuf, RpcError> {
        let documents = || directories::UserDirs::new().and_then(|u| u.document_dir().map(Path::to_path_buf));
        let Some(dir) = self.destination.clone().or_else(documents) else {
            return Err(RpcError::failed("invalid", "there's no Documents folder: set destination to a folder"));
        };
        if !dir.is_absolute() {
            return Err(RpcError::failed("invalid", format!("destination must be absolute, not {}", dir.display())));
        }
        if !dir.is_dir() {
            return Err(RpcError::failed("not_found", format!("the backup folder {} isn't there", dir.display())));
        }
        Ok(dir)
    }

    /// The time between scheduled backups, when there are any.
    pub(crate) fn every(&self) -> Option<Duration> {
        let days = self.every_days;
        (days.is_finite() && days > 0.0).then(|| Duration::from_secs_f64(days.min(36_500.0) * 86_400.0))
    }

    /// How many of its own scheduled backups the schedule keeps, when it doesn't keep all.
    pub(crate) fn keep(&self) -> Option<usize> {
        let keep = self.keep;
        (keep.is_finite() && keep >= 1.0).then(|| keep.min(1e6) as usize)
    }
}

pub(crate) fn io(path: &Path, e: std::io::Error) -> RpcError {
    RpcError::failed("io", format!("{}: {e}", path.display()))
}

/// A size as people read one (`513.1 MiB`).
pub(crate) fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} bytes"),
        _ => format!("{value:.1} {}", UNITS[unit]),
    }
}

pub(crate) fn view(view: View) -> Value {
    serde_json::to_value(view).expect("a view is JSON")
}

#[cfg(test)]
mod tests;
