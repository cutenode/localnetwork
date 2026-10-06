//! Ejecting a drive: unmount it and power it off, so it can be pulled out safely.
//!
//! A drive pulled out without ejecting is the usual way a FAT file system ends up marked dirty
//! (see [`crate::fscheck`]). Ejecting is refused at once while a locallibrary operation is
//! writing to the drive (it takes the drive lock without waiting for it); otherwise it asks the
//! operating system:
//!
//! * macOS: `diskutil eject`, which unmounts every volume of the disk and powers it off;
//! * Linux: `udisksctl unmount` and `power-off` (no administrator rights needed for removable
//!   drives), falling back to `umount`;
//! * Windows: Explorer's own Eject, through PowerShell.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::Serialize;

use crate::device::Device;
use crate::format::{Cmd, Disk};
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EjectReport {
    pub name: String,
    pub root: PathBuf,
    /// What was run.
    pub commands: Vec<String>,
    /// Whether the disk was also powered off (otherwise only unmounted).
    pub powered_off: bool,
}

/// The commands that eject the volume mounted at `root` (for plans and tests).
pub(crate) fn commands(root: &Path) -> Result<Vec<Cmd>> {
    // One `let` per OS: on Linux a `return` here would end the function's last block, which
    // clippy's needless_return rejects.
    #[cfg(target_os = "macos")]
    let cmds = vec![Cmd::new("diskutil", &["eject", &root.to_string_lossy()])];
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmds = linux_commands(&crate::format::disk_at(root)?, root)?;
    #[cfg(windows)]
    let cmds = vec![windows_command(root)?];
    Ok(cmds)
}

/// Linux: udisks unmounts every mounted partition of `disk` (which holds the volume mounted at
/// `root`), then powers the disk off.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn linux_commands(disk: &Disk, root: &Path) -> Result<Vec<Cmd>> {
    let volume = disk
        .volumes
        .iter()
        .find(|v| v.mount.as_deref() == Some(root))
        .ok_or_else(|| Error::NotFound(format!("the partition mounted at {}", root.display())))?;
    let unmount = |id: &str| Cmd::new("udisksctl", &["unmount", "--no-user-interaction", "-b", id]);
    let mut out: Vec<Cmd> = disk.volumes.iter().filter(|v| v.mount.is_some()).map(|v| unmount(&v.id)).collect();
    if out.is_empty() {
        out.push(unmount(&volume.id));
    }
    out.push(Cmd::new("udisksctl", &["power-off", "--no-user-interaction", "-b", &disk.device]));
    Ok(out)
}

/// Windows: Explorer's own Eject, for the drive whose root is `root` (`E:\`).
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_command(root: &Path) -> Result<Cmd> {
    use crate::format::windows::{quote, root_letter};
    let letter = root_letter(root)?;
    let script = format!(
        "(New-Object -ComObject Shell.Application).Namespace(17).ParseName({}).InvokeVerb('Eject')",
        quote(&format!("{letter}:"))
    );
    Ok(Cmd::new("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script]))
}

/// Eject a drive. Fails with [`Error::Locked`] while a locallibrary operation is writing to it.
pub fn eject(device: &Device) -> Result<EjectReport> {
    // Wait for nothing: a running sync must finish (or be cancelled) first.
    drop(device.lock()?);
    let root = device.root().to_path_buf();
    let cmds = commands(&root)?;
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
    if cfg!(windows) && !wait_gone(&root, Duration::from_secs(10)) {
        return Err(Error::External {
            command: ran.join("; "),
            message: format!("{} is still there: a program may be using a file on it", root.display()),
        });
    }
    Ok(EjectReport { name: device.name(), root, commands: ran, powered_off })
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
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_ejects_with_diskutil() {
        let cmds = commands(Path::new("/Volumes/REKORDBOX")).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].display(), "diskutil eject /Volumes/REKORDBOX");
    }

    /// Every mounted partition goes, then the power; a volume that isn't on the disk is refused.
    #[test]
    fn linux_unmounts_the_whole_disk_then_powers_it_off() {
        let mut disk = crate::format::tests::sample_disk();
        (disk.id, disk.device) = ("/dev/sdb".into(), "/dev/sdb".into());
        disk.volumes[0].id = "/dev/sdb1".into();
        disk.volumes[0].mount = Some("/media/sam/DNA".into());
        let mut other = disk.volumes[0].clone();
        (other.id, other.mount) = ("/dev/sdb2".into(), Some("/media/sam/STORE".into()));
        disk.volumes.push(other);
        let shown: Vec<String> =
            linux_commands(&disk, Path::new("/media/sam/DNA")).unwrap().iter().map(Cmd::display).collect();
        assert_eq!(
            shown,
            [
                "udisksctl unmount --no-user-interaction -b /dev/sdb1",
                "udisksctl unmount --no-user-interaction -b /dev/sdb2",
                "udisksctl power-off --no-user-interaction -b /dev/sdb",
            ]
        );
        assert!(linux_commands(&disk, Path::new("/media/sam/OTHER")).is_err());
    }

    #[test]
    fn windows_ejects_the_drive_letter_through_explorer() {
        for root in [r"E:\", "E:", "E:/"] {
            let cmd = windows_command(Path::new(root)).unwrap();
            assert_eq!(cmd.program, "powershell");
            assert_eq!(
                cmd.args.last().unwrap(),
                "(New-Object -ComObject Shell.Application).Namespace(17).ParseName('E:').InvokeVerb('Eject')"
            );
        }
        assert!(windows_command(Path::new(r"E:\Music")).is_err(), "only a drive's root");
    }
}
