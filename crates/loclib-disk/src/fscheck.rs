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
//! what is inconsistent.
//!
//! [`steps`] says what runs for a volume, and [`run`] runs it.

use std::path::Path;
use std::process::{Command, Stdio};

use loclib_format::{Cmd, Error, Result};

/// What a check runs: the OS tool and, where the tool can't unmount the volume itself, the
/// commands that unmount it before and mount it again after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Steps {
    /// Run first. When it fails, nothing else runs.
    pub unmount: Option<Cmd>,
    /// The check (or repair) itself.
    pub check: Cmd,
    /// Run after the check, whatever it found.
    pub mount: Option<Cmd>,
}

impl Steps {
    fn only(check: Cmd) -> Steps {
        Steps { unmount: None, check, mount: None }
    }

    /// The commands, shell-style, in the order they run.
    pub fn display(&self) -> String {
        [self.unmount.as_ref(), Some(&self.check), self.mount.as_ref()]
            .into_iter()
            .flatten()
            .map(Cmd::display)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// How to check (or with `repair`, fix) the volume mounted at `root`: its partition is `volume`
/// (`disk4s1`, `/dev/sdb1`), holding the file system the OS calls `fs`. Linux needs `volume`;
/// macOS and Windows go by `root`.
pub fn steps(root: &Path, volume: Option<&str>, fs: Option<&str>, repair: bool) -> Result<Steps> {
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
    let drive = &format!("{}:", loclib_format::windows::root_letter(root)?);
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
fn verdict(success: bool, output: &str) -> bool {
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

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Whether the file system is consistent (after the repair, when repairing).
    pub ok: bool,
    /// The tool's last 15 lines, then what mounting the volume again said.
    pub output: Vec<String>,
}

/// Run `steps`. Nothing may be using the volume: it is unmounted for the duration and mounted
/// again (by diskutil on macOS, by udisks on Linux), and on Windows a repair dismounts it.
/// `partition` is the device the check reads, which needs administrator rights: that's checked
/// before the volume is unmounted. A volume that can't be mounted again is said in the output,
/// not an error.
pub fn run(steps: &Steps, partition: Option<&str>) -> Result<Outcome> {
    if let Some(cmd) = &steps.unmount {
        unmount(cmd, &steps.check, partition)?;
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
    Ok(Outcome { ok, output })
}

#[cfg(test)]
mod tests;
