//! Handing a web page to the default browser, or a file to the app the system opens it with.
//!
//! Only public `https:` addresses and files that exist are handed over, as a single argument and
//! never through a shell, so nothing else can be started this way: no other URL scheme's handler,
//! no command-line option, no shell command, nothing on this computer or its local network.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::IoContext;
use crate::export::one_line;
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

/// `url` as the browser may be given it: a public `https:` address (`net::public`)
/// without a user name or password, written out as parsed. One with a space, a control character
/// or a quote is refused rather than tidied, as the parser would drop some of those silently.
pub fn web_address(url: &str) -> Result<String> {
    let refused = |why: String| {
        Error::invalid(format!("only https:// addresses on the internet are opened in the browser: {why}"))
    };
    if url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"') {
        return Err(refused(format!("{} has a space, a control character or a quote", one_line(url))));
    }
    let parsed = crate::net::public(url).map_err(refused)?;
    // Not shown: it may be a password.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(refused("the address has a user name or password".into()));
    }
    Ok(parsed.into())
}

/// Open `url`, a public `https:` address, in the default browser.
pub fn open_url(url: &str) -> Result<()> {
    launch(OsStr::new(&web_address(url)?))
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
