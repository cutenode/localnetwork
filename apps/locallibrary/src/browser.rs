//! The browser's half of signing in, as the command line does it: an Atmosphere account's
//! (`account login`), an account's through an account plugin (`account TYPE sign-in`) and a
//! plugin's account's (`plugin account ... sign-in`).

use std::io::{BufRead, IsTerminal, Write};
use std::time::Duration;

use anyhow::{Result, bail};
use clap::Args;
use locallibrary::callback::{LONGEST_LOGIN_WAIT, span};

use crate::output::{Output, dim};

/// How the browser round trip goes.
#[derive(Args, Clone, Copy)]
pub struct BrowserArgs {
    /// Print links instead of opening the browser
    #[arg(long)]
    pub(crate) no_browser: bool,
    /// The browser runs on another computer: paste the address it ends up on after approving
    #[arg(long)]
    pub(crate) paste: bool,
    /// Seconds to wait for the browser (an hour at most)
    #[arg(long, value_name = "SECS", default_value_t = 300)]
    pub(crate) timeout: u64,
}

impl BrowserArgs {
    /// Whether to open links in the browser here: not with `--no-browser`, nor `--paste`, whose
    /// browser runs on another computer.
    pub fn opens(&self) -> bool {
        !self.no_browser && !self.paste
    }

    /// How long the browser is waited for: `--timeout`, up to the longest sign-in waits.
    pub fn wait(&self) -> Duration {
        Duration::from_secs(self.timeout).min(LONGEST_LOGIN_WAIT)
    }
}

impl Default for BrowserArgs {
    /// As the flags are when none is given.
    fn default() -> BrowserArgs {
        BrowserArgs { no_browser: false, paste: false, timeout: 300 }
    }
}

/// The browser round trip of a sign-in begun at `url`: opened (or printed), then `finish`ed
/// once the browser comes back (waiting as long as `browser` says), or `paste`d the address it
/// ended up on, read from the terminal; `cancel`led when there's no terminal to paste into. The
/// link goes to stderr, so stdout stays the result (JSON with `--json`).
pub fn round_trip<T>(
    url: &str,
    browser: BrowserArgs,
    out: Output,
    finish: impl FnOnce(Duration) -> locallibrary::Result<T>,
    paste: impl FnOnce(&str) -> locallibrary::Result<T>,
    cancel: impl FnOnce() -> bool,
) -> Result<T> {
    let opened = browser.opens() && locallibrary::opener::open_url(url).is_ok();
    match opened {
        true => {
            eprintln!("Opening your browser to sign in…");
            eprintln!("{}", dim(format!("If it didn't open: {url}")));
        }
        false => eprintln!("Sign in: open this link\n  {url}"),
    }
    if !browser.paste {
        eprintln!("{}", dim(format!("Waiting for you to approve (up to {}; Ctrl-C stops)…", span(browser.wait()))));
        return Ok(finish(browser.wait())?);
    }
    if out.json || !std::io::stdin().is_terminal() {
        cancel();
        bail!("--paste needs a terminal to paste into");
    }
    eprint!("After approving, paste the address your browser ended up on: ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(paste(line.trim())?)
}

#[cfg(test)]
mod tests;
