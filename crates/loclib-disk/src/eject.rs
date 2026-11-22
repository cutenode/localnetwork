//! Ejecting a drive: unmount it and power it off, so it can be pulled out safely.
//!
//! A drive pulled out without ejecting is the usual way a FAT file system ends up marked dirty
//! (see [`crate::fscheck`]). [`commands`] says what ejecting asks of the operating system, and
//! [`run`] runs it:
//!
//! * macOS: `diskutil eject`, which unmounts every volume of the disk and powers it off;
//! * Linux: `udisksctl unmount` and `power-off` (no administrator rights needed for removable
//!   drives), falling back to `umount`;
//! * Windows: Explorer's own Eject, through PowerShell.

use std::path::Path;
use std::time::{Duration, Instant};

use loclib_format::{Cmd, Disk, Error, Result};

/// The commands that eject the volume mounted at `root`. Linux needs `disk`, the disk holding it
/// (`loclib_format::by_mount`), since it unmounts every partition of it; macOS and Windows go by
/// `root`.
pub fn commands(root: &Path, disk: Option<&Disk>) -> Result<Vec<Cmd>> {
    let _ = disk;
    // One `let` per OS: on Linux a `return` here would end the function's last block, which
    // clippy's needless_return rejects.
    #[cfg(target_os = "macos")]
    let cmds = vec![Cmd::new("diskutil", &["eject", &root.to_string_lossy()])];
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmds = {
        let disk = disk.ok_or_else(|| Error::NotFound(format!("the disk holding {}", root.display())))?;
        linux_commands(disk, root)?
    };
    #[cfg(windows)]
    let cmds = vec![windows_command(root)?];
    Ok(cmds)
}

/// Linux: udisks unmounts every mounted partition of `disk` (which holds the volume mounted at
/// `root`), then powers the disk off.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn linux_commands(disk: &Disk, root: &Path) -> Result<Vec<Cmd>> {
    if !disk.volumes.iter().any(|v| v.mount.as_deref() == Some(root)) {
        return Err(Error::NotFound(format!("the partition mounted at {}", root.display())));
    }
    let unmount = |id: &str| Cmd::new("udisksctl", &["unmount", "--no-user-interaction", "-b", id]);
    let mut out: Vec<Cmd> = disk.volumes.iter().filter(|v| v.mount.is_some()).map(|v| unmount(&v.id)).collect();
    out.push(Cmd::new("udisksctl", &["power-off", "--no-user-interaction", "-b", &disk.device]));
    Ok(out)
}

/// Windows: Explorer's own Eject, for the drive whose root is `root` (`E:\`).
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_command(root: &Path) -> Result<Cmd> {
    use loclib_format::windows::{quote, root_letter};
    let letter = root_letter(root)?;
    let script = format!(
        "(New-Object -ComObject Shell.Application).Namespace(17).ParseName({}).InvokeVerb('Eject')",
        quote(&format!("{letter}:"))
    );
    Ok(Cmd::new("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script]))
}

/// What ejecting did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ejected {
    /// What was run, shell-style.
    pub commands: Vec<String>,
    /// Whether the disk was also powered off (otherwise only unmounted).
    pub powered_off: bool,
}

/// Eject the volume mounted at `root` by running `cmds` ([`commands`]) in order, stopping at the
/// first that fails, with what to do when the drive is busy. On Linux without udisks, the volume
/// is unmounted with `umount` and the power left on. On Windows, waits up to 10 seconds for the
/// volume to go, since Explorer's Eject says nothing.
pub fn run(root: &Path, cmds: &[Cmd]) -> Result<Ejected> {
    let mut ran = Vec::new();
    #[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(unused_mut))]
    let mut powered_off = true;
    for (i, c) in cmds.iter().enumerate() {
        ran.push(c.display());
        match c.run() {
            Ok(_) => {}
            // udisks isn't everywhere: unmount with umount and leave the power on.
            #[cfg(all(unix, not(target_os = "macos")))]
            Err(Error::External { ref message, .. }) if i == 0 && message.starts_with("not found") => {
                let um = Cmd::new("umount", &[&root.to_string_lossy()]);
                ran.push(um.display());
                um.run().map_err(busy_hint)?;
                powered_off = false;
                break;
            }
            Err(e) => {
                let _ = i;
                return Err(busy_hint(e));
            }
        }
    }
    // Explorer's Eject reports nothing; the volume disappearing is the answer.
    if cfg!(windows) && !wait_gone(root, Duration::from_secs(10)) {
        return Err(Error::External {
            command: ran.join("; "),
            message: format!("{} is still there: a program may be using a file on it", root.display()),
        });
    }
    Ok(Ejected { commands: ran, powered_off })
}

fn wait_gone(root: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if !root.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    !root.exists()
}

fn busy_hint(e: Error) -> Error {
    match e {
        Error::External { command, message }
            if message.to_lowercase().contains("busy") || message.contains("in use") =>
        {
            Error::External {
                command,
                message: format!("{message} (close any window or program using the drive, then try again)"),
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests;
