//! Handing a web page to the default browser, or a file to the app the system opens it with.
//!
//! Only `https:` addresses and files that exist are handed over, as a single argument and never
//! through a shell, so nothing else can be started this way: no other URL scheme's handler, no
//! command-line option, no shell command.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::IoContext;
use crate::{Error, Result};

/// Set by the CLI's tests: nothing is opened, as if no browser or app were there, so a sign-in
/// prints its link instead of sending the browser of whoever runs them to a real service. Only in
/// builds with debug assertions.
#[cfg(debug_assertions)]
pub const NO_OPEN: &str = "LOCALLIBRARY_NO_OPEN";

/// [`NO_OPEN`] for this process, without the environment: see [`never_open`].
#[cfg(debug_assertions)]
static NEVER: AtomicBool = AtomicBool::new(false);

/// Nothing is opened in this process from now on, as with [`NO_OPEN`]: for tests that run the app
/// in their own process. Only in builds with debug assertions.
#[cfg(debug_assertions)]
pub fn never_open() {
    NEVER.store(true, Ordering::Relaxed);
}

/// `url`, when it's an `https:` address the browser may be given.
pub fn web_address(url: &str) -> Result<&str> {
    let https = url.get(..8).is_some_and(|s| s.eq_ignore_ascii_case("https://")) && url.len() > 8;
    if !https || url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"') {
        return Err(Error::invalid(format!("only https:// addresses are opened in the browser, not {url:?}")));
    }
    Ok(url)
}

/// Open `url`, an `https:` address, in the default browser.
pub fn open_url(url: &str) -> Result<()> {
    launch(OsStr::new(web_address(url)?))
}

/// Open the file at `path` with the app the system opens it with.
pub fn open_file(path: &Path) -> Result<()> {
    // An absolute path can't be taken for an option or an address.
    let path = std::path::absolute(path).at(path)?;
    if !path.is_file() {
        return Err(Error::NotFound(format!("{} to open", path.display())));
    }
    launch(path.as_os_str())
}

fn launch(target: &OsStr) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(windows)]
    let mut cmd = {
        // `cmd /C start` would parse the target again (`&`, `^`, `%`); the URL protocol handler
        // takes it whole, and opens files with their app too.
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = Command::new("xdg-open");
    let program = cmd.get_program().to_string_lossy().into_owned();
    let failed = |message: String| Error::External { command: program.clone(), message };
    #[cfg(debug_assertions)]
    if NEVER.load(Ordering::Relaxed) || std::env::var_os(NO_OPEN).is_some() {
        return Err(failed(format!("not run: {NO_OPEN} is set")));
    }
    // Quietly: what it prints would land on a TUI's screen.
    match cmd.arg(target).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(failed(status.to_string())),
        Err(e) => Err(failed(e.to_string())),
    }
}

#[cfg(test)]
mod tests;
