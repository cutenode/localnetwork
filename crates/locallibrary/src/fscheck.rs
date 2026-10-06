//! Checking a drive's file system, and repairing it.
//!
//! Pulling a drive out without ejecting it, from a computer or from a player whose USB
//! indicator is still blinking, can leave its file system inconsistent. The check is the
//! operating system's own:
//!
//! * macOS: `diskutil verifyVolume` / `repairVolume`. No administrator rights needed; the volume
//!   is unmounted for the duration and mounted again.
//! * Linux: `fsck.vfat` / `fsck.exfat` (`-n` to check, `-a` to repair) on the partition, as
//!   root. Neither may run on a mounted file system, so the partition is unmounted with
//!   `udisksctl unmount` first (the check is refused when that fails) and mounted again with
//!   `udisksctl mount` afterwards.
//! * Windows: `chkdsk` (`/f /x` to repair, which dismounts the volume first), from an elevated
//!   prompt.
//!
//! FAT and exFAT also keep a "dirty" flag, but it isn't a useful signal for a mounted drive:
//! macOS clears FAT's clean-shutdown bit as soon as it mounts a volume writable, Windows and
//! Linux set the boot sector's dirty bit while a volume is mounted, and macOS runs `fsck -y` on
//! a dirty volume before mounting it anyway. So the flag is never read here; the check reports
//! what is actually inconsistent.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use schemars::JsonSchema;
use serde::Serialize;

use crate::device::Device;
use crate::format::Cmd;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FsCheckReport {
    pub drive: String,
    pub root: PathBuf,
    /// The volume's device (`disk4s1`, `/dev/sdb1`, `E:`).
    pub volume: Option<String>,
    pub filesystem: Option<String>,
    pub command: String,
    /// Whether the file system is consistent (after the repair, when repairing).
    pub ok: bool,
    pub repaired: bool,
    /// The tool's report, last lines.
    pub output: Vec<String>,
}

impl FsCheckReport {
    /// One line for the drive's logbook.
    pub fn summary(&self) -> String {
        match (self.ok, self.repaired) {
            (true, true) => "file system repaired".into(),
            (true, false) => "file system consistent".into(),
            (false, true) => "file system errors the repair couldn't fix".into(),
            (false, false) => "file system errors found".into(),
        }
    }
}

/// What a check runs: the OS tool and, where the tool can't unmount the volume itself, the
/// commands that unmount it before and mount it again after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Steps {
    /// Run first. When it fails, nothing else runs.
    pub unmount: Option<Cmd>,
    pub check: Cmd,
    /// Run after the check, whatever it found.
    pub mount: Option<Cmd>,
}

impl Steps {
    fn only(check: Cmd) -> Steps {
        Steps { unmount: None, check, mount: None }
    }

    fn display(&self) -> String {
        [self.unmount.as_ref(), Some(&self.check), self.mount.as_ref()]
            .into_iter()
            .flatten()
            .map(Cmd::display)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// How to check (or with `repair`, fix) the volume mounted at `root`.
pub(crate) fn steps(root: &Path, volume: Option<&str>, fs: Option<&str>, repair: bool) -> Result<Steps> {
    let _ = (volume, fs);
    // One `let` per OS: on Linux a `return` here would end the function's last block, which
    // clippy's needless_return rejects.
    #[cfg(target_os = "macos")]
    let steps = Steps::only(Cmd::new(
        "diskutil",
        &[if repair { "repairVolume" } else { "verifyVolume" }, &root.to_string_lossy()],
    ));
    #[cfg(all(unix, not(target_os = "macos")))]
    let steps = {
        let volume = volume.ok_or_else(|| Error::NotFound(format!("the partition mounted at {}", root.display())))?;
        let sudo_user = std::env::var("SUDO_USER").ok();
        linux_steps(volume, fs, repair, sudo_user.as_deref())
    };
    #[cfg(windows)]
    let steps = windows_steps(root, repair)?;
    Ok(steps)
}

/// Windows: `chkdsk` on the drive letter of `root` (`E:\`); `/f /x` to repair, which dismounts
/// the volume first.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_steps(root: &Path, repair: bool) -> Result<Steps> {
    let drive = &format!("{}:", crate::format::windows::root_letter(root)?);
    Ok(Steps::only(if repair { Cmd::new("chkdsk", &[drive, "/f", "/x"]) } else { Cmd::new("chkdsk", &[drive]) }))
}

/// Linux: `fsck.vfat` and `fsck.exfat` must not run on a mounted file system, so the partition
/// is unmounted with udisks first and mounted again afterwards. Run through `sudo`, it is
/// mounted again for the user who ran sudo (`sudo_user`), where udisks had it, not for root.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn linux_steps(volume: &str, fs: Option<&str>, repair: bool, sudo_user: Option<&str>) -> Steps {
    let tool = if fs.is_some_and(|f| f.to_lowercase().contains("exfat")) { "fsck.exfat" } else { "fsck.vfat" };
    let udisks = |verb| Cmd::new("udisksctl", &[verb, "--no-user-interaction", "-b", volume]);
    let mut mount = udisks("mount");
    if let Some(user) = sudo_user.filter(|u| !u.is_empty() && *u != "root") {
        let mut args = vec!["-u".to_string(), user.to_string(), mount.program];
        args.append(&mut mount.args);
        mount = Cmd { program: "sudo".into(), args, input: None };
    }
    Steps {
        unmount: Some(udisks("unmount")),
        check: Cmd::new(tool, &[if repair { "-a" } else { "-n" }, volume]),
        mount: Some(mount),
    }
}

/// Whether a check's output and exit status say the file system is consistent.
pub(crate) fn verdict(success: bool, output: &str) -> bool {
    // diskutil passes on the file system check's own exit code in its output.
    output
        .lines()
        .find_map(|l| l.split("exit code is ").nth(1).and_then(|n| n.trim().trim_end_matches('.').parse::<i32>().ok()))
        .map_or(success, |c| c == 0)
}

fn needs_admin(cmd: &Cmd) -> Error {
    Error::Permission(format!("{} needs administrator rights", cmd.display()))
}

/// Run a check tool, keeping its exit status (non-zero means "errors found", not a failure to
/// run).
fn run_status(cmd: &Cmd) -> Result<(bool, String)> {
    let out = Command::new(&cmd.program)
        .args(&cmd.args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::External { command: cmd.program.clone(), message: e.to_string() })?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let lower = text.to_lowercase();
    if !out.status.success()
        && (lower.contains("permission denied") || lower.contains("access denied") || lower.contains("must be root"))
    {
        return Err(needs_admin(cmd));
    }
    Ok((verdict(out.status.success(), &text), text))
}

/// Unmount the volume for the check, or say why the check can't run.
fn unmount(unmount: &Cmd, check: &Cmd, partition: Option<&str>) -> Result<()> {
    // The tool reads the partition itself, which takes administrator rights: find that out before
    // the drive disappears from the desktop.
    if let Some(Err(e)) = partition.map(std::fs::File::open)
        && e.kind() == std::io::ErrorKind::PermissionDenied
    {
        return Err(needs_admin(check));
    }
    unmount.run().map(drop).map_err(|e| match e {
        Error::External { command, message } => {
            let why = if message.starts_with("not found") {
                "install udisks2, which unmounts the drive for the check"
            } else {
                "close any window or program using the drive, then try again"
            };
            Error::External {
                command,
                message: format!("{message} (the check needs the drive unmounted, so it didn't run: {why})"),
            }
        }
        other => other,
    })
}

/// Check a drive's file system (with `repair`, fix it). Nothing may be using the drive: it is
/// unmounted for the duration and mounted again (by diskutil on macOS, by udisks on Linux), and
/// on Windows a repair dismounts it.
pub fn check(device: &Device, repair: bool) -> Result<FsCheckReport> {
    // No locallibrary operation may be running on it (and the lock file, on the drive itself,
    // must be closed again before the volume can be unmounted).
    drop(device.lock()?);
    let root = device.root().to_path_buf();
    let disk = crate::format::disk_at(&root).ok();
    let volume =
        disk.as_ref().and_then(|d| d.volumes.iter().find(|v| v.mount.as_deref() == Some(root.as_path())).cloned());
    let steps = steps(
        &root,
        volume.as_ref().map(|v| v.id.as_str()),
        volume.as_ref().and_then(|v| v.filesystem_name.as_deref()),
        repair,
    )?;
    if let Some(cmd) = &steps.unmount {
        unmount(cmd, &steps.check, volume.as_ref().map(|v| v.id.as_str()))?;
    }
    let result = run_status(&steps.check);
    let mut mounted = Vec::new();
    if let Some(cmd) = &steps.mount {
        match cmd.run() {
            Ok(out) => mounted.extend(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from)),
            Err(e) => mounted.push(format!("couldn't mount it again ({e}): unplug the drive and plug it back in")),
        }
    }
    let (ok, text) = result?;
    let lines: Vec<String> = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    let mut output = lines[lines.len().saturating_sub(15)..].to_vec();
    output.extend(mounted);
    Ok(FsCheckReport {
        drive: device.name(),
        root,
        volume: volume.as_ref().map(|v| v.id.clone()),
        filesystem: volume.and_then(|v| v.filesystem_name),
        command: steps.display(),
        ok,
        repaired: repair,
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diskutil_reports_the_fsck_exit_code() {
        let ok = "** Phase 3 - Checking for Orphan Clusters\nFile system check exit code is 0\nFinished file system verification";
        assert!(verdict(true, ok));
        let bad = "** Phase 2\nFile system check exit code is 8\nError: -69845";
        assert!(!verdict(true, bad));
        assert!(!verdict(false, "fsck.fat: errors found"));
        assert!(verdict(true, "fsck.fat 4.2: no errors"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_checks_with_diskutil() {
        let s = steps(Path::new("/Volumes/USB"), Some("disk4s1"), Some("MS-DOS FAT32"), false).unwrap();
        assert_eq!(s, Steps::only(Cmd::new("diskutil", &["verifyVolume", "/Volumes/USB"])));
        let s = steps(Path::new("/Volumes/USB"), None, None, true).unwrap();
        assert_eq!(s.display(), "diskutil repairVolume /Volumes/USB");
    }

    /// Regression: on Linux, fsck ran on the mounted partition.
    #[test]
    fn linux_checks_the_unmounted_partition() {
        let s = linux_steps("/dev/sdb1", Some("vfat"), false, None);
        assert_eq!(s.unmount.unwrap().display(), "udisksctl unmount --no-user-interaction -b /dev/sdb1");
        assert_eq!(s.check.display(), "fsck.vfat -n /dev/sdb1");
        assert_eq!(s.mount.unwrap().display(), "udisksctl mount --no-user-interaction -b /dev/sdb1");

        let s = linux_steps("/dev/sdc1", Some("exfat"), true, Some("root"));
        assert_eq!(
            s.display(),
            "udisksctl unmount --no-user-interaction -b /dev/sdc1; fsck.exfat -a /dev/sdc1; \
             udisksctl mount --no-user-interaction -b /dev/sdc1"
        );
    }

    /// Under sudo, udisks would mount the drive again for root, where the user can't reach it.
    #[test]
    fn linux_mounts_again_for_the_user_who_ran_sudo() {
        let s = linux_steps("/dev/sdb1", None, true, Some("sam"));
        assert_eq!(s.check.display(), "fsck.vfat -a /dev/sdb1");
        assert_eq!(s.unmount.unwrap().display(), "udisksctl unmount --no-user-interaction -b /dev/sdb1");
        assert_eq!(s.mount.unwrap().display(), "sudo -u sam udisksctl mount --no-user-interaction -b /dev/sdb1");
        let s = linux_steps("/dev/sdb1", None, true, Some(""));
        assert_eq!(s.mount.unwrap().program, "udisksctl");
    }

    #[test]
    fn windows_checks_the_drive_letter_with_chkdsk() {
        assert_eq!(windows_steps(Path::new(r"E:\"), false).unwrap().display(), "chkdsk E:");
        assert_eq!(windows_steps(Path::new("E:"), true).unwrap(), Steps::only(Cmd::new("chkdsk", &["E:", "/f", "/x"])));
        assert!(windows_steps(Path::new(r"E:\Music"), false).is_err(), "only a drive's root");
    }

    #[test]
    fn a_failed_unmount_stops_the_check() {
        let check = Cmd::new("fsck.vfat", &["-n", "/dev/sdb1"]);
        let missing = Cmd::new("locallibrary-no-such-tool", &["unmount"]);
        match unmount(&missing, &check, None) {
            Err(Error::External { message, .. }) => {
                assert!(message.contains("didn't run") && message.contains("install udisks2"), "{message}")
            }
            other => panic!("{other:?}"),
        }
    }
}
