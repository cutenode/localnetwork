//! Putting an export's files in place: each staged next to where it goes (a copy of the audio
//! tagged there) before any is put in place, so the files being replaced stay until the new ones
//! are all written, and a failure or a cancel leaves what was there.

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;

use loclib_interop::{Content, Export, serato};
use loclib_plugin::RpcError;
use loclib_progress::{Ctx, Event, Phase};

/// Bytes copied at a time, between checks for a cancel.
const CHUNK: usize = 1 << 20;

/// Writes `export`'s files below `output` (with its symlinks resolved, as the files' paths are),
/// and removes the files it says must go (an old database's WAL).
pub(crate) fn write(export: &Export, output: &Path, ctx: &Ctx<'_>) -> Result<(), RpcError> {
    let needed: u64 = export.files.iter().map(|f| f.bytes).sum();
    let existing_dir = output.ancestors().find(|a| a.exists()).unwrap_or(output);
    if let Ok(available) = fs4::available_space(existing_dir)
        && needed > available
    {
        let message = format!("{} needs {needed} bytes free, and has {available}", output.display());
        return Err(RpcError::failed("no_space", message));
    }
    let mut staged = Vec::with_capacity(export.files.len());
    for f in &export.files {
        ctx.check()?;
        let dir = f.path.parent().unwrap_or(output);
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        #[cfg_attr(not(unix), allow(unused_mut, reason = "only Unix sets its permissions"))]
        let mut builder = tempfile::Builder::new();
        // Made as any other file is (the umask decides), not readable by its owner alone.
        #[cfg(unix)]
        builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
        // Said of `dir`: tempfile adds the random name it tried (`... at path "<dir>/.tmpx1Yz"`).
        let mut tmp = builder.tempfile_in(dir).map_err(|e| {
            let cause = e.to_string().split(" at path ").next().unwrap_or_default().to_owned();
            io(dir, std::io::Error::new(e.kind(), cause))
        })?;
        match &f.content {
            Content::Bytes(bytes) => tmp.write_all(bytes).map_err(|e| io(tmp.path(), e))?,
            Content::TaggedCopy { from, markers } => {
                copy(from, tmp.as_file_mut(), &f.path, ctx)?;
                if let Some(m) = markers {
                    // An error names the drive's file: the copy's is temporary, and removed.
                    serato::write_markers(tmp.path(), from, m).map_err(crate::failure)?;
                }
            }
        }
        tmp.as_file().sync_all().map_err(|e| io(tmp.path(), e))?;
        // Closed until it's put in place: a copy of every track open at once would run out of
        // file handles (macOS gives an app 256).
        staged.push(tmp.into_temp_path());
    }
    // A failure or a cancel before here leaves an old Engine database its WAL (which holds its
    // latest changes).
    for stale in &export.remove {
        match std::fs::remove_file(stale) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(io(stale, e)),
            _ => {}
        }
    }
    for (f, tmp) in export.files.iter().zip(staged) {
        tmp.persist(&f.path).map_err(|e| io(&f.path, e.error))?;
    }
    Ok(())
}

/// Copies the audio file `from` into `to` (staged for `path`), saying how far it got and stopping
/// on a cancel.
fn copy(from: &Path, to: &mut File, path: &Path, ctx: &Ctx<'_>) -> Result<(), RpcError> {
    let mut file = File::open(from).map_err(|e| io(from, e))?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        ctx.check()?;
        let n = file.read(&mut buf).map_err(|e| io(from, e))?;
        if n == 0 {
            return Ok(());
        }
        to.write_all(&buf[..n]).map_err(|e| io(path, e))?;
        ctx.emit(Event::Bytes { phase: Phase::Copy, bytes: n as u64 });
    }
}

fn io(path: &Path, e: std::io::Error) -> RpcError {
    RpcError::failed("io", format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests;
