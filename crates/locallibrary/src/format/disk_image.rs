//! Disk images for the tests that format and eject a real disk. [`TestImage`] creates an empty
//! image, attaches it, and detaches it when it goes, also when the test fails.
//!
//! Only the scheduled workflow runs these tests, with `LOCALLIBRARY_DISK_IMAGE_TESTS=1` on each
//! system's runner: without it they return at once, so an ordinary `cargo test` never touches a
//! device. Linux and Windows need administrator rights (`losetup`, `Mount-DiskImage`).
//!
//! The format code refuses every disk image. [`TestImage::admit`] lets through the one a test
//! attached, by its exact identifier, on that test's thread only ([`super::TEST_IMAGE`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::{Cmd, Disk};

/// Whether this run may attach and format disk images.
fn enabled() -> bool {
    std::env::var_os("LOCALLIBRARY_DISK_IMAGE_TESTS").is_some_and(|v| v == "1")
}

/// True, and says so, when this run may not attach disk images: the test then returns.
pub(super) fn skipped() -> bool {
    if enabled() {
        return false;
    }
    eprintln!("skipped: set LOCALLIBRARY_DISK_IMAGE_TESTS=1 to attach and format disk images");
    true
}

/// A volume label no other test running on this machine uses (the OS mounts volumes by label,
/// `/Volumes/<label>` on macOS): `LT`, `tag`, this process and a count, at most 11 characters
/// (FAT32's limit).
pub(super) fn unique_label(tag: char) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed) & 0xFF;
    format!("LT{tag}{:05X}{n:X}", std::process::id() & 0xF_FFFF)
}

/// An empty disk image this test created and attached. Dropping it detaches it, looked up by its
/// file, so it never detaches anything else.
pub(super) struct TestImage {
    /// The image file. The folder holding it goes after the image is detached.
    file: PathBuf,
    _dir: tempfile::TempDir,
    /// The whole disk as the format code names it: `disk4`, `/dev/loop7`, `PhysicalDrive3`.
    pub id: String,
    /// Its device path, as attaching it printed it.
    pub device: String,
}

impl TestImage {
    /// Create an empty image of `mib` MiB and attach it, mounting nothing.
    pub fn attach(mib: u64) -> TestImage {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("locallibrary-{}.{}", std::process::id(), os::EXTENSION));
        // Made before attaching: if attaching panics part way, dropping it detaches the image.
        let mut image = TestImage { file, _dir: dir, id: String::new(), device: String::new() };
        os::create(&image.file, mib);
        image.device = os::attach(&image.file);
        image.id = os::id(&image.device);
        assert_eq!(image.attached().as_deref(), Some(image.device.as_str()), "{}", image.file.display());
        eprintln!("attached {} at {}", image.file.display(), image.device);
        image
    }

    /// The device the system has this image attached at now, if it has.
    pub fn attached(&self) -> Option<String> {
        os::attached(&self.file)
    }

    /// Let the format code format this image, on this thread.
    pub fn admit(&self) {
        super::TEST_IMAGE.set(Some(self.id.clone()));
    }

    /// Panic unless `disk` is this image, still attached where attaching it said. Called before
    /// anything that writes to a disk.
    pub fn assert_mine(&self, disk: &Disk) {
        assert_eq!((disk.id.as_str(), disk.device.as_str()), (self.id.as_str(), self.device.as_str()));
        assert!(disk.virtual_disk, "{disk:?}");
        assert_eq!(self.attached().as_deref(), Some(self.device.as_str()), "{} isn't attached", self.file.display());
    }
}

impl Drop for TestImage {
    fn drop(&mut self) {
        super::TEST_IMAGE.with_borrow_mut(|t| {
            if t.as_deref() == Some(self.id.as_str()) {
                *t = None;
            }
        });
        if let Some(device) = self.attached()
            && let Err(e) = os::detach(&self.file, &device)
        {
            eprintln!("detaching {device} ({}): {e}", self.file.display());
        }
        let left = self.attached();
        if !std::thread::panicking() {
            assert_eq!(left, None, "{} is still attached", self.file.display());
        } else if let Some(device) = left {
            eprintln!("{} is still attached at {device}", self.file.display());
        }
    }
}

/// Whether `a` and `b` name the same file, symbolic links resolved: the system's tools may report
/// the image's path either way (`/var` is `/private/var` on macOS).
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canonical(a) == canonical(b)
}

/// `hdiutil`.
#[cfg(target_os = "macos")]
mod os {
    use super::*;
    use crate::format::json;
    use serde_json::Value;

    pub const EXTENSION: &str = "dmg";

    /// `hdiutil <args>` (with `-plist`), as JSON.
    fn hdiutil(args: &[&str]) -> Value {
        let xml = Cmd::new("hdiutil", args).run().unwrap();
        let text = Cmd::new("plutil", &["-convert", "json", "-o", "-", "--", "-"]).input(xml).run().unwrap();
        serde_json::from_str(&text).unwrap()
    }

    /// The whole disk among an image's `system-entities` (its partitions are `/dev/disk4s1`...).
    fn whole_disk(image: &Value) -> Option<String> {
        json::array(image.get("system-entities"))
            .into_iter()
            .filter_map(|e| json::str(e, "dev-entry"))
            .find(|d| d.strip_prefix("/dev/disk").is_some_and(|n| n.chars().all(|c| c.is_ascii_digit())))
    }

    pub fn create(file: &Path, mib: u64) {
        let size = format!("{mib}m");
        let file = file.to_string_lossy();
        Cmd::new("hdiutil", &["create", "-quiet", "-size", &size, "-layout", "NONE", "-type", "UDIF", &file])
            .run()
            .unwrap();
    }

    pub fn attach(file: &Path) -> String {
        let file = file.to_string_lossy();
        let out = hdiutil(&["attach", "-plist", "-nomount", "-noverify", "-noautofsck", "-noautoopen", &file]);
        whole_disk(&out).unwrap_or_else(|| panic!("no disk in hdiutil's answer: {out}"))
    }

    pub fn attached(file: &Path) -> Option<String> {
        let info = hdiutil(&["info", "-plist"]);
        json::array(info.get("images"))
            .into_iter()
            .find(|i| json::str(i, "image-path").is_some_and(|p| same_file(Path::new(&p), file)))
            .and_then(whole_disk)
    }

    pub fn detach(_: &Path, device: &str) -> crate::Result<()> {
        Cmd::new("hdiutil", &["detach", device])
            .run()
            .or_else(|_| Cmd::new("hdiutil", &["detach", "-force", device]).run())?;
        Ok(())
    }

    pub fn id(device: &str) -> String {
        device.trim_start_matches("/dev/").to_string()
    }
}

/// `losetup`, with partition scanning, so the format's partition shows up as `/dev/loop7p1`.
#[cfg(all(unix, not(target_os = "macos")))]
mod os {
    use super::*;
    use crate::format::json;
    use serde_json::Value;

    pub const EXTENSION: &str = "img";

    pub fn create(file: &Path, mib: u64) {
        std::fs::File::create(file).unwrap().set_len(mib << 20).unwrap();
    }

    pub fn attach(file: &Path) -> String {
        Cmd::new("losetup", &["--find", "--show", "--partscan", &file.to_string_lossy()])
            .run()
            .unwrap()
            .trim()
            .to_string()
    }

    pub fn attached(file: &Path) -> Option<String> {
        // Nothing at all when no loop device is set up.
        let text = Cmd::new("losetup", &["--list", "--json", "--output", "NAME,BACK-FILE"]).run().unwrap();
        let list: Value = if text.trim().is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap() };
        json::array(list.get("loopdevices"))
            .into_iter()
            .find(|d| json::str(d, "back-file").is_some_and(|p| same_file(Path::new(&p), file)))
            .and_then(|d| json::str(d, "name"))
    }

    pub fn detach(_: &Path, device: &str) -> crate::Result<()> {
        Cmd::new("losetup", &["--detach", device]).run()?;
        Ok(())
    }

    pub fn id(device: &str) -> String {
        device.to_string()
    }
}

/// A VHD: `diskpart` creates it (`New-VHD` needs Hyper-V), the Storage cmdlets attach it.
#[cfg(windows)]
mod os {
    use super::*;
    use crate::format::windows::{powershell, quote};

    pub const EXTENSION: &str = "vhd";

    pub fn create(file: &Path, mib: u64) {
        let script = file.with_extension("diskpart");
        std::fs::write(&script, format!("create vdisk file=\"{}\" maximum={mib} type=expandable\r\n", file.display()))
            .unwrap();
        Cmd::new("diskpart", &["/s", &script.to_string_lossy()]).run().unwrap();
    }

    pub fn attach(file: &Path) -> String {
        let path = quote(&file.to_string_lossy());
        let n = powershell(&format!(
            "$ErrorActionPreference = 'Stop'\n$null = Mount-DiskImage -ImagePath {path} -StorageType VHD\n(Get-DiskImage -ImagePath {path}).Number"
        ))
        .run()
        .unwrap();
        format!(r"\\.\PhysicalDrive{}", n.trim())
    }

    pub fn attached(file: &Path) -> Option<String> {
        let path = quote(&file.to_string_lossy());
        let n = powershell(&format!(
            "$i = Get-DiskImage -ImagePath {path} -ErrorAction SilentlyContinue\nif ($i.Attached) {{ $i.Number }}"
        ))
        .run()
        .unwrap();
        let n = n.trim();
        (!n.is_empty()).then(|| format!(r"\\.\PhysicalDrive{n}"))
    }

    pub fn detach(file: &Path, _: &str) -> crate::Result<()> {
        let path = quote(&file.to_string_lossy());
        powershell(&format!("$ErrorActionPreference = 'Stop'\n$null = Dismount-DiskImage -ImagePath {path}")).run()?;
        Ok(())
    }

    pub fn id(device: &str) -> String {
        device.trim_start_matches(r"\\.\").to_string()
    }
}

mod tests;
