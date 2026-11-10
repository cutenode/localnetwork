//! Copy-on-write clones of files on this computer, not to be taken for a library's clones, its
//! drives: a clone of a file costs no bytes and no writing until one side is written, and a write
//! to either leaves the other as it was (APFS `clonefile`, `FICLONE` on Btrfs and XFS).

use std::path::Path;

use tempfile::NamedTempFile;

/// The temp file `tmp` (empty, with the permissions the copy should have) as a clone of `src`, or
/// `tmp` as it was when `src` can't be cloned there: another volume, a filesystem without clones,
/// or a `src` that can't be read, which the byte copy that follows reports.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn reflink_into(src: &Path, tmp: NamedTempFile) -> Result<NamedTempFile, NamedTempFile> {
    let mut beside = tmp.path().as_os_str().to_owned();
    beside.push(".clone");
    let beside = std::path::PathBuf::from(beside);
    if reflink_copy::reflink(src, &beside).is_err() {
        return Err(tmp);
    }
    let placed = (|| {
        std::fs::set_permissions(&beside, tmp.as_file().metadata()?.permissions())?;
        let file = std::fs::File::options().read(true).write(true).open(&beside)?;
        std::fs::rename(&beside, tmp.path())?;
        Ok::<_, std::io::Error>(file)
    })();
    match placed {
        Ok(file) => Ok(NamedTempFile::from_parts(file, tmp.into_temp_path())),
        Err(_) => {
            let _ = std::fs::remove_file(&beside);
            Err(tmp)
        }
    }
}

/// `tmp` as it was: nothing here clones.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn reflink_into(_: &Path, tmp: NamedTempFile) -> Result<NamedTempFile, NamedTempFile> {
    Err(tmp)
}

#[cfg(test)]
mod tests;
