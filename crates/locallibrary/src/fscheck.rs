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
    /// What to be wary of: the check hasn't run on this platform before.
    pub warnings: Vec<String>,
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
/// on Windows a repair dismounts it. Refused while `$LOCALLIBRARY_VOLUMES` hides the machine's
/// drives (`device::disks_shown`): the volume holding a drive folder is the machine's.
pub fn check(device: &Device, repair: bool) -> Result<FsCheckReport> {
    crate::device::disks_shown()?;
    // No locallibrary operation may be running on it (and the lock file, on the drive itself,
    // must be closed again before the volume can be unmounted). Checking makes none.
    drop(device.read_lock()?);
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
        warnings: crate::format::untested_here("the file-system check").into_iter().collect(),
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
    fn each_outcome_is_summed_up_for_the_logbook() {
        let report = |ok, repaired| FsCheckReport {
            drive: "Main".into(),
            root: "/Volumes/MAIN".into(),
            volume: Some("disk4s1".into()),
            filesystem: Some("MS-DOS FAT32".into()),
            command: "diskutil verifyVolume /Volumes/MAIN".into(),
            ok,
            repaired,
            output: Vec::new(),
            warnings: Vec::new(),
        };
        assert_eq!(report(true, false).summary(), "file system consistent");
        assert_eq!(report(true, true).summary(), "file system repaired");
        assert_eq!(report(false, false).summary(), "file system errors found");
        assert_eq!(report(false, true).summary(), "file system errors the repair couldn't fix");
    }

    /// Nothing is checked while a locallibrary operation is writing to the drive.
    #[test]
    fn a_drive_in_use_is_not_checked() {
        let dir = tempfile::tempdir().unwrap();
        let d = Device::open(dir.path()).unwrap();
        let _writing = d.lock().unwrap();
        assert!(matches!(check(&d, false), Err(Error::Locked(_))));
    }

    /// Under the test switch the volume holding a drive folder is this machine's own: it isn't
    /// checked, and nothing is run.
    #[test]
    fn the_test_switch_leaves_the_machines_volumes_alone() {
        let dir = tempfile::tempdir().unwrap();
        let d = Device::open(dir.path()).unwrap();
        // Were the switch ignored, the drive in use would still stop the check.
        let _writing = d.lock().unwrap();
        crate::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
        let refused = check(&d, false);
        crate::testing::volumes_on_this_thread(None);
        assert!(
            matches!(&refused, Err(Error::Invalid(m)) if m.starts_with("LOCALLIBRARY_VOLUMES is set")),
            "{refused:?}"
        );
    }

    /// A check tool's exit status and output, as fsck.vfat and diskutil give them. Each fake tool
    /// prints what the real one does.
    #[cfg(unix)]
    #[test]
    fn the_check_tools_status_and_output_give_the_verdict() {
        let tool = |script: &str| Cmd::new("sh", &["-c", script]);
        let (ok, text) =
            run_status(&tool("echo 'fsck.fat 4.2 (2021-01-31)'; echo '/dev/sdb1: 12 files, 3/1953 clusters'")).unwrap();
        assert!(ok && text.contains("12 files"));
        let dirty = tool("echo 'Dirty bit is set. Fs was not properly unmounted'; exit 1");
        assert!(!run_status(&dirty).unwrap().0);
        // diskutil exits 0 and says what the file system check found.
        let diskutil =
            tool("echo 'File system check exit code is 8.'; echo 'Error: -69845: File system verify or repair failed'");
        assert!(!run_status(&diskutil).unwrap().0);
        let not_root = tool("echo 'fsck.fat 4.2 (2021-01-31)'; echo 'open: Permission denied' >&2; exit 6");
        assert!(matches!(run_status(&not_root), Err(Error::Permission(m)) if m.contains("administrator")));
        assert!(matches!(run_status(&Cmd::new("locallibrary-no-such-tool", &[])), Err(Error::External { .. })));
    }

    /// When the drive can't be unmounted, the check doesn't run, and says what to do.
    #[cfg(unix)]
    #[test]
    fn a_busy_drive_stops_the_check() {
        let check = Cmd::new("fsck.vfat", &["-n", "/dev/sdb1"]);
        let busy = Cmd::new(
            "sh",
            &[
                "-c",
                "echo 'Error unmounting /dev/sdb1: GDBus.Error:org.freedesktop.UDisks2.Error.DeviceBusy: target is busy' >&2; exit 1",
            ],
        );
        let dir = tempfile::tempdir().unwrap();
        // A partition that isn't there is the tool's to report.
        let partition = dir.path().join("sdb1");
        match unmount(&busy, &check, Some(&partition.to_string_lossy())) {
            Err(Error::External { message, .. }) => assert!(
                message.contains("target is busy") && message.contains("close any window or program using the drive"),
                "{message}"
            ),
            other => panic!("{other:?}"),
        }
        assert!(unmount(&Cmd::new("true", &[]), &check, None).is_ok());
    }

    /// Whatever the tool prints around it, diskutil's `exit code is N` line decides the verdict;
    /// without one, the exit status does. Run as a real tool, the output reads the same.
    #[cfg(unix)]
    #[hegel::test(test_cases = 40)]
    fn the_verdict_follows_the_reported_exit_code(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let noise = || gs::sampled_from(vec!["** Phase 1", "Checking volume.", "12 files, 3/1953 clusters", ""]);
        let before: Vec<&str> = tc.draw(gs::vecs(noise()).max_size(3));
        let after: Vec<&str> = tc.draw(gs::vecs(noise()).max_size(2));
        let code: Option<i32> = tc.draw(gs::optional(gs::integers::<i32>().min_value(0).max_value(9)));
        let status = tc.draw(gs::integers::<i32>().min_value(0).max_value(3));
        let dot = if tc.draw(gs::booleans()) { "." } else { "" };
        let reported = code.map(|c| format!("File system check exit code is {c}{dot}"));
        let mut lines = before;
        lines.extend(reported.as_deref());
        lines.extend(after);
        let text = lines.join("\n");
        let consistent = code.map_or(status == 0, |c| c == 0);
        assert_eq!(verdict(status == 0, &text), consistent, "{text:?}");

        let quoted: Vec<String> = lines.iter().map(|l| format!("'{l}'")).collect();
        let script = format!("printf '%s\\n' {}; exit {status}", quoted.join(" "));
        let (ok, out) = run_status(&Cmd::new("sh", &["-c", &script])).unwrap();
        assert_eq!(ok, consistent, "{out:?}");
        assert_eq!(out.lines().filter(|l| !l.is_empty()).count(), lines.iter().filter(|l| !l.is_empty()).count());
    }

    /// Linux checks the partition it unmounted, with the tool for its file system, mounts that
    /// same partition again (for the user who ran sudo), and repairs only when asked.
    #[hegel::test(test_cases = 60)]
    fn linux_steps_unmount_check_and_mount_one_partition(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let volume = tc.draw(gs::sampled_from(vec!["/dev/sdb1", "/dev/sdc2", "/dev/mmcblk0p1"]));
        let fs = tc.draw(gs::optional(gs::sampled_from(vec!["vfat", "exfat", "ExFAT", "msdos", ""])));
        let repair = tc.draw(gs::booleans());
        let user = tc.draw(gs::optional(gs::sampled_from(vec!["", "root", "sam"])));
        let s = linux_steps(volume, fs, repair, user);
        let tool = if fs.is_some_and(|f| f.eq_ignore_ascii_case("exfat")) { "fsck.exfat" } else { "fsck.vfat" };
        assert_eq!(s.check, Cmd::new(tool, &[if repair { "-a" } else { "-n" }, volume]));
        let unmount = s.unmount.unwrap();
        let mount = s.mount.unwrap();
        assert_eq!(unmount.program, "udisksctl");
        assert_eq!(unmount.args.last().map(String::as_str), Some(volume));
        assert_eq!(mount.args.last().map(String::as_str), Some(volume));
        match user {
            Some("sam") => assert_eq!(mount.args[..3], ["-u", "sam", "udisksctl"], "{mount:?}"),
            _ => assert_eq!(mount.program, "udisksctl"),
        }
        assert!(mount.args.contains(&"mount".to_string()) && unmount.args.contains(&"unmount".to_string()));
    }

    /// chkdsk runs on the drive letter of a drive's root, and only there; `/f /x` only to repair.
    #[hegel::test(test_cases = 60)]
    fn windows_checks_only_a_drive_root(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let letter = tc.draw(gs::sampled_from(vec!['e', 'E', 'z', '1']));
        let tail = tc.draw(gs::sampled_from(vec!["", "\\", "/", "\\\\", "\\Music", "x"]));
        let prefix = tc.draw(gs::sampled_from(vec!["", r"\\?\"]));
        let repair = tc.draw(gs::booleans());
        let root = format!("{prefix}{letter}:{tail}");
        let is_root = letter.is_ascii_alphabetic() && tail.chars().all(|c| c == '\\' || c == '/');
        match windows_steps(Path::new(&root), repair) {
            Ok(s) => {
                assert!(is_root, "{root}");
                let drive = format!("{}:", letter.to_ascii_uppercase());
                let want = if repair { vec![drive.as_str(), "/f", "/x"] } else { vec![drive.as_str()] };
                assert_eq!(s, Steps::only(Cmd::new("chkdsk", &want)));
            }
            Err(e) => assert!(!is_root && e.to_string().contains("not the root"), "{root}: {e}"),
        }
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
