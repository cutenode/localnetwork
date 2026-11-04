//! Stand-ins for FFmpeg's `ffmpeg` and `ffprobe`, first on `PATH`: shell scripts (so not on
//! Windows) that note their arguments and answer as a test says.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

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
    // The tools it runs by their paths: [`unrunnable`] leaves `PATH` without them.
    for tool in ["ffmpeg", "ffprobe"] {
        let script = format!(
            "#!/bin/sh\nd='{}'\nprintf '%s\\n' \"$@\" >\"$d/{tool}-args\"\nfor last; do :; done\n\
             if [ -f \"$d/{tool}-writes\" ]; then /bin/cp \"$d/{tool}-writes\" \"$last\"; fi\n\
             /bin/cat \"$d/{tool}-out\"\n/bin/cat \"$d/{tool}-err\" >&2\n\
             if [ -f \"$d/{tool}-after\" ]; then ( exec 2>/dev/null; /bin/sleep 0.3; /bin/cat \"$d/{tool}-after\"; \
             : >\"$d/{tool}-wrote\" ) & fi\n\
             if [ -f \"$d/{tool}-stalls\" ]; then exec /bin/sleep 60; fi\nexit \"$(/bin/cat \"$d/{tool}-status\")\"\n",
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
    unsafe { std::env::set_var("PATH", &path) };
    STANDING_IN.set(path).unwrap();
    dir
});

/// `PATH` as [`DIR`] set it, the stand-ins first.
static STANDING_IN: OnceLock<OsString> = OnceLock::new();

/// Both stand-ins runnable and first on `PATH` again, after [`unrunnable`] or [`uninstalled`].
pub(super) fn restore() {
    for tool in ["ffmpeg", "ffprobe"] {
        std::fs::set_permissions(DIR.path().join(tool), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // SAFETY: as above, while no hook runs.
    unsafe { std::env::set_var("PATH", STANDING_IN.get().unwrap()) };
}

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
    for what in ["args", "stalls", "after", "wrote"] {
        let _ = std::fs::remove_file(at(what));
    }
}

/// The stand-in `tool`, once it has said what it says, waiting a minute rather than exiting
/// (itself, not a shell waiting on it), until [`answer`] is called again.
pub(super) fn stall(tool: &str) {
    std::fs::write(DIR.path().join(format!("{tool}-stalls")), "").unwrap();
}

/// The stand-in `tool` leaving behind a process of its own that says `out` on its standard output
/// a moment later, after the tool itself is gone, until [`answer`] is called again; and
/// [`written`] once it has.
pub(super) fn after(tool: &str, out: &str) {
    std::fs::write(DIR.path().join(format!("{tool}-after")), out).unwrap();
}

/// Whether what [`after`] has `tool` say has been said.
pub(super) fn written(tool: &str) -> bool {
    DIR.path().join(format!("{tool}-wrote")).exists()
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
