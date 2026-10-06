//! Handing a web page to the default browser, or a file to the app the system opens it with.
//!
//! Only `https:` addresses and files that exist are handed over, as a single argument and never
//! through a shell, so nothing else can be started this way: no other URL scheme's handler, no
//! command-line option, no shell command.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::IoContext;
use crate::{Error, Result};

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
    // Quietly: what it prints would land on a TUI's screen.
    match cmd.arg(target).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(failed(status.to_string())),
        Err(e) => Err(failed(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_addresses_and_files_that_exist_are_opened() {
        assert!(web_address("https://bsky.social/oauth/authorize?client_id=a&request_uri=b").is_ok());
        assert!(web_address("HTTPS://bsky.social/").is_ok());
        for refused in [
            "http://bsky.social/",
            "file:///etc/passwd",
            "ms-msdt:/id PCWDiagnostic",
            "search-ms:query=x",
            "-a Calculator",
            "https://",
            "https://a.example/ x",
            "https://a.example/\u{1b}]8;;x",
            "https://a.example/\"&calc",
        ] {
            assert!(web_address(refused).is_err(), "{refused}");
            assert!(open_url(refused).is_err(), "{refused}");
        }
        let dir = tempfile::tempdir().unwrap();
        let err = open_file(&dir.path().join("missing.html")).unwrap_err();
        assert_eq!(err.code(), "not_found", "{err}");
        assert!(open_file(dir.path()).is_err(), "a folder isn't a file");
    }
}
