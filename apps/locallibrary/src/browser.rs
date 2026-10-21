//! The browser's half of signing in, as the command line does it: an Atmosphere account's
//! (`account login`) and a plugin's account's (`plugin account ... sign-in`).

use clap::Args;
use locallibrary::callback::LONGEST_LOGIN_WAIT;

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
    pub fn wait(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout).min(LONGEST_LOGIN_WAIT)
    }
}

#[cfg(test)]
mod tests;
