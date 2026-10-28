//! Stand-ins for FFmpeg's `ffmpeg` and `ffprobe`, first on `PATH`: shell scripts (so not on
//! Windows) that note their arguments and answer as a test says.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// What the stand-in ffprobe says of a 25-second AAC recording.
pub(super) const AAC: &str = r#"{"streams": [{"codec_name": "aac"}], "format": {"duration": "25.000000"}}"#;
/// And of a 25-second WAV.
pub(super) const WAV: &str = r#"{"streams": [{"codec_name": "pcm_s16le"}], "format": {"duration": "25.000000"}}"#;
/// What ffmpeg's `-progress` says making a 25-second video: halfway, then done.
pub(super) const PROGRESS: &str = "out_time_us=N/A\nprogress=continue\nout_time_us=12500000\nprogress=continue\n\
                                   out_time_us=25000000\nprogress=end\n";

/// The folder the stand-ins are in and keep what they're told and said, one for the test's
/// process (nextest runs each test in one of its own), first on its `PATH` from the first use.
static DIR: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().unwrap();
    for tool in ["ffmpeg", "ffprobe"] {
        let script = format!(
            "#!/bin/sh\nd='{}'\nprintf '%s\\n' \"$@\" >\"$d/{tool}-args\"\nfor last; do :; done\n\
             if [ -f \"$d/{tool}-writes\" ]; then cp \"$d/{tool}-writes\" \"$last\"; fi\n\
             cat \"$d/{tool}-out\"\ncat \"$d/{tool}-err\" >&2\n\
             if [ -f \"$d/{tool}-stalls\" ]; then exec sleep 60; fi\nexit \"$(cat \"$d/{tool}-status\")\"\n",
            dir.path().display()
        );
        let path = dir.path().join(tool);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::join_paths(
        std::iter::once(dir.path().to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .unwrap();
    // SAFETY: nextest gives each test a process of its own, and nothing else in it reads the
    // environment while this runs (before the plugin is served).
    unsafe { std::env::set_var("PATH", path) };
    dir
});

/// The stand-in `tool` from now on: what it writes to the file it's given last (none when
/// `None`), says on standard output and standard error, and exits with.
pub(super) fn answer(tool: &str, writes: Option<&[u8]>, out: &str, err: &str, status: u8) {
    let dir = DIR.path();
    let at = |what: &str| dir.join(format!("{tool}-{what}"));
    match writes {
        Some(bytes) => std::fs::write(at("writes"), bytes).unwrap(),
        None => drop(std::fs::remove_file(at("writes"))),
    }
    std::fs::write(at("out"), out).unwrap();
    std::fs::write(at("err"), err).unwrap();
    std::fs::write(at("status"), status.to_string()).unwrap();
    let _ = std::fs::remove_file(at("args"));
    let _ = std::fs::remove_file(at("stalls"));
}

/// The stand-in `tool`, once it has said what it says, waiting a minute rather than exiting
/// (itself, not a shell waiting on it), until [`answer`] is called again.
pub(super) fn stall(tool: &str) {
    std::fs::write(DIR.path().join(format!("{tool}-stalls")), "").unwrap();
}

/// The stand-in `tool` can't be run, and `PATH` has no other.
pub(super) fn unrunnable(tool: &str) {
    std::fs::set_permissions(DIR.path().join(tool), std::fs::Permissions::from_mode(0o644)).unwrap();
    // SAFETY: as above.
    unsafe { std::env::set_var("PATH", DIR.path()) };
}

/// The arguments `tool` was last run with, a line each, if it ran since [`answer`].
pub(super) fn args(tool: &str) -> Option<String> {
    std::fs::read_to_string(DIR.path().join(format!("{tool}-args"))).ok()
}

/// Both stand-ins as a publish that goes well finds them: ffprobe reading `probed`, and ffmpeg
/// writing `video` and its progress.
pub(super) fn working(probed: &str, video: &[u8]) {
    answer("ffprobe", None, probed, "", 0);
    answer("ffmpeg", Some(video), PROGRESS, "", 0);
}

/// `PATH` without either stand-in nor anything else: FFmpeg isn't installed.
pub(super) fn uninstalled() {
    // SAFETY: as above.
    unsafe { std::env::set_var("PATH", DIR.path().join("nothing")) };
}

/// A file `name` of `size` bytes in `dir`, none of its stretches alike.
pub(super) fn file(dir: &Path, name: &str, size: u64) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, (0..size).map(|i| (i * 31 % 251) as u8).collect::<Vec<u8>>()).unwrap();
    path
}
