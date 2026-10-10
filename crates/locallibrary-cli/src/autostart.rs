//! `watch --install`: run `locallibrary watch --pull --sync-group` whenever you log in, as a
//! launchd agent on macOS or a systemd user service on Linux. The names match what docs/store.md
//! used to have people write by hand, so installing replaces one made that way.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

const LAUNCHD_LABEL: &str = "local.locallibrary.watch";
const SYSTEMD_UNIT: &str = "locallibrary-watch.service";

/// Where the agent's definition lives on this system, or `None` where installing isn't supported.
pub fn path() -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    if cfg!(target_os = "macos") {
        return Some(home.join("Library/LaunchAgents").join(format!("{LAUNCHD_LABEL}.plist")));
    }
    if cfg!(target_os = "linux") {
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        return Some(config.join("systemd/user").join(SYSTEMD_UNIT));
    }
    None
}

pub fn installed() -> bool {
    path().is_some_and(|p| p.exists())
}

/// Where `watch` logs to when run at log in, as something to tell people.
pub fn log_location() -> String {
    match log_file() {
        Some(f) => f.display().to_string(),
        None => format!("journalctl --user -u {SYSTEMD_UNIT}"),
    }
}

fn log_file() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    std::env::home_dir().map(|h| h.join("Library/Logs/locallibrary-watch.log"))
}

/// Writes the agent running this executable with `args`, and starts it now (restarting one that
/// was running, so new arguments take effect). Where it was written.
pub fn install(args: &[String]) -> Result<PathBuf> {
    let Some(path) = path() else {
        bail!(
            "`watch --install` works on macOS and Linux; on Windows, make a Task Scheduler task that runs \
             `locallibrary watch --pull --sync-group` at log on"
        );
    };
    let exe = std::env::current_exe().context("finding the locallibrary executable")?;
    let definition = match log_file() {
        Some(log) => plist(&exe, args, &log),
        None => unit(&exe, args),
    };
    let dir = path.parent().expect("a file in a folder");
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::write(&path, definition).with_context(|| format!("writing {}", path.display()))?;
    if cfg!(target_os = "macos") {
        let _ = launchctl(&["bootout", &launchd_target()]);
        launchctl(&["bootstrap", &format!("gui/{}", uid()), &path.display().to_string()])?;
    } else {
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", SYSTEMD_UNIT])?;
        systemctl(&["restart", SYSTEMD_UNIT])?;
    }
    Ok(path)
}

/// Stops the agent and removes it. Where it was, or `None` when none was installed.
pub fn uninstall() -> Result<Option<PathBuf>> {
    let Some(path) = path().filter(|p| p.exists()) else { return Ok(None) };
    if cfg!(target_os = "macos") {
        // Not loaded (stopped by hand, or never since a restart) is fine: it's removed either way.
        let _ = launchctl(&["bootout", &launchd_target()]);
    } else {
        systemctl(&["disable", "--now", SYSTEMD_UNIT])?;
    }
    fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    if !cfg!(target_os = "macos") {
        systemctl(&["daemon-reload"])?;
    }
    Ok(Some(path))
}

fn launchd_target() -> String {
    format!("gui/{}/{LAUNCHD_LABEL}", uid())
}

#[cfg(unix)]
fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and can't fail.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn uid() -> u32 {
    unreachable!("launchd is macOS only")
}

fn launchctl(args: &[&str]) -> Result<()> {
    run("launchctl", args)
}

fn systemctl(args: &[&str]) -> Result<()> {
    run("systemctl", &[&["--user"], args].concat())
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(program).args(args).output().with_context(|| format!("running {program}"))?;
    if !out.status.success() {
        bail!("`{program} {}` failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// A launchd agent that runs at log in, is started again if it stops, and logs to `log`.
fn plist(exe: &Path, args: &[String], log: &Path) -> String {
    let xml = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let arguments: String = std::iter::once(exe.display().to_string())
        .chain(args.iter().cloned())
        .map(|a| format!("    <string>{}</string>\n", xml(&a)))
        .collect();
    let log = xml(&log.display().to_string());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
{arguments}  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
    )
}

/// A systemd user service that starts with the user's session and again if it fails.
fn unit(exe: &Path, args: &[String]) -> String {
    // systemd expands % specifiers and $ variables even inside quotes.
    let quote =
        |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%").replace('$', "$$"));
    let command: Vec<String> =
        std::iter::once(exe.display().to_string()).chain(args.iter().cloned()).map(|a| quote(&a)).collect();
    format!(
        "[Unit]\nDescription=Save rekordbox drives into the locallibrary store as they're plugged in\n\n\
         [Service]\nExecStart={}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
        command.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plist_runs_watch_and_escapes_paths() {
        let p = plist(
            Path::new("/Users/r&b/.cargo/bin/locallibrary"),
            &args(&["--store", "/Volumes/<NAS>", "watch", "--pull"]),
            Path::new("/Users/r&b/Library/Logs/locallibrary-watch.log"),
        );
        assert!(p.contains("<string>local.locallibrary.watch</string>"));
        assert!(p.contains(
            "    <string>/Users/r&amp;b/.cargo/bin/locallibrary</string>\n    <string>--store</string>\n    \
             <string>/Volumes/&lt;NAS&gt;</string>\n    <string>watch</string>\n    <string>--pull</string>\n  </array>"
        ));
        assert!(
            p.contains("<key>StandardErrorPath</key>\n  <string>/Users/r&amp;b/Library/Logs/locallibrary-watch.log")
        );
    }

    #[test]
    fn unit_quotes_each_argument() {
        let u = unit(
            Path::new("/home/dj/.cargo/bin/locallibrary"),
            &args(&["--store", "/mnt/100% \"mine\" $HOME", "watch", "--pull"]),
        );
        assert!(u.contains(
            r#"ExecStart="/home/dj/.cargo/bin/locallibrary" "--store" "/mnt/100%% \"mine\" $$HOME" "watch" "--pull""#
        ));
        assert!(u.contains("WantedBy=default.target"));
    }
}
