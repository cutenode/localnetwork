//! Drive writes from a launchd job, on macOS, so macOS adds no `._` files to FAT and exFAT drives.
//!
//! A process whose executable carries a `com.apple.provenance` attribute gives one to every file
//! it creates, and every installed locallibrary carries it (it marks files a process started from
//! a terminal wrote: Homebrew, `cargo install`, the installer). FAT and exFAT can't hold the
//! attribute, so macOS writes a 4 KiB `._` file beside each file a write makes. A launchd job
//! whose executable carries none writes none, so a command that writes drives, run from a
//! terminal, runs as a one-shot launchd job of a copy of locallibrary that a launchd job wrote
//! with `/bin/cat`. The job runs at Interactive priority: at launchd's Background default the
//! same write took four times as long.
//!
//! That copy has no provenance by design: macOS doesn't attribute what it writes to the app that
//! installed locallibrary. It relies on behaviour Apple doesn't document. The copy runs only while
//! its hash is the installed binary's, and when anything here fails (no launchd, a copy that can't
//! be made or checked) the command runs in this process as before, saying so, and `drive tidy`
//! removes the `._` files.

use std::ffi::{CString, OsStr, OsString};
use std::io::IsTerminal;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use locallibrary::Hash;

/// In a job's environment: the command runs where it is, never as another job.
const IN_JOB: &str = "LOCALLIBRARY_LAUNCHD_JOB";

/// The environment a job gets from the terminal's, besides locallibrary's own variables.
const PASSED: [&str; 12] =
    ["HOME", "PATH", "USER", "LOGNAME", "SHELL", "TMPDIR", "LANG", "TERM", "COLORTERM", "NO_COLOR", "TZ", "EDITOR"];

/// How long making the copy may take before the command runs here instead.
const COPY_TIMEOUT: Duration = Duration::from_secs(60);

fn launchctl(args: &[&OsStr]) -> Result<String, String> {
    let out = Command::new("launchctl").args(args).output().map_err(|e| format!("running launchctl: {e}"))?;
    match out.status.success() {
        true => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        false => Err(format!("launchctl {}", String::from_utf8_lossy(&out.stderr).trim())),
    }
}

/// Where the copy and the jobs' definitions are kept: beside the store's default folder, not in
/// it, as a store is only made in a folder that holds nothing else.
fn home() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "locallibrary-launchd").map(|d| d.data_dir().to_path_buf())
}

/// Whether `path` carries a `com.apple.provenance` attribute.
fn has_provenance(path: &Path) -> bool {
    let Ok(p) = CString::new(path.as_os_str().as_bytes()) else { return true };
    // SAFETY: both strings are NUL-terminated; a null buffer of size 0 asks only for the length.
    unsafe { libc::getxattr(p.as_ptr(), c"com.apple.provenance".as_ptr(), std::ptr::null_mut(), 0, 0, 0) >= 0 }
}

/// Whether `copy` holds the bytes whose hash is `want`.
fn holds(copy: &Path, want: &Hash) -> bool {
    std::fs::read(copy).is_ok_and(|b| Hash::of(&b) == *want)
}

/// Whether `copy` is `want`'s bytes and carries no provenance: the only copy that's run.
fn is_current(copy: &Path, want: &Hash) -> bool {
    holds(copy, want) && !has_provenance(copy)
}

/// A launchd job's definition: `args` run in `cwd` with `env`, at Interactive priority, once,
/// with the terminal `tty` (if any) as stdin, stdout and stderr.
fn plist(label: &str, args: &[OsString], env: &[(String, String)], cwd: &Path, tty: Option<&Path>) -> String {
    let xml = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let string = |s: &str| format!("<string>{}</string>", xml(s));
    let arguments: String = args.iter().map(|a| format!("    {}\n", string(&a.to_string_lossy()))).collect();
    let variables: String = env.iter().map(|(k, v)| format!("    <key>{}</key>{}\n", xml(k), string(v))).collect();
    let stdio: String = tty
        .map(|t| {
            ["StandardInPath", "StandardOutPath", "StandardErrorPath"]
                .map(|k| format!("  <key>{k}</key>{}\n", string(&t.to_string_lossy())))
                .concat()
        })
        .unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>{}
  <key>ProgramArguments</key>
  <array>
{arguments}  </array>
  <key>EnvironmentVariables</key>
  <dict>
{variables}  </dict>
  <key>WorkingDirectory</key>{}
{stdio}  <key>ProcessType</key><string>Interactive</string>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
        string(label),
        string(&cwd.to_string_lossy()),
    )
}

/// What a job's environment is, from the terminal's `vars`: the ones it needs and locallibrary's
/// own, with paging off (a job has no terminal of its own for a pager to read keys from) and
/// [`IN_JOB`] set.
fn job_env(vars: impl Iterator<Item = (String, String)>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> =
        vars.filter(|(k, _)| PASSED.contains(&k.as_str()) || (k.starts_with("LOCALLIBRARY_") && k != IN_JOB)).collect();
    env.extend([("PAGER".into(), String::new()), (IN_JOB.into(), "1".into())]);
    env.sort();
    env
}

/// Whether `launchctl print` describes a job: one launchd knows has a state.
fn known(print: &str) -> bool {
    print.lines().any(|l| l.starts_with("\tstate = "))
}

/// A finished job's exit code from `launchctl print`, or `None` while it's running. A job a
/// signal stopped exits 128 and the signal's number, as a shell says.
fn finished(print: &str) -> Option<u8> {
    let field =
        |name: &str| print.lines().find_map(|l| l.strip_prefix('\t').and_then(|l| l.strip_prefix(name))).map(str::trim);
    if field("state = ")? != "not running" {
        return None;
    }
    if let Some(code) = field("last exit code = ").and_then(|c| c.parse::<u8>().ok()) {
        return Some(code);
    }
    let signal = field("last terminating signal = ").and_then(|s| s.rsplit(": ").next()?.parse::<u8>().ok());
    Some(signal.map_or(1, |s| 128 + s))
}

/// A one-shot job, taken out of launchd when dropped.
struct Job {
    target: String,
}

impl Job {
    /// Starts the job `definition` describes, its definition kept in `home` until it's started.
    fn start(home: &Path, label: &str, definition: &str) -> Result<Job, String> {
        let dir = home.join("jobs");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(format!("{label}.plist"));
        std::fs::write(&file, definition).map_err(|e| e.to_string())?;
        let domain = format!("gui/{}", crate::autostart::uid());
        // A job left by a run that was cut off goes first.
        let target = format!("{domain}/{label}");
        let _ = launchctl(&[OsStr::new("bootout"), OsStr::new(&target)]);
        let started = launchctl(&[OsStr::new("bootstrap"), OsStr::new(&domain), file.as_os_str()]);
        let _ = std::fs::remove_file(&file);
        started.map(|_| Job { target })
    }

    /// The job's exit code once it's done, or `None` if launchd can't say (it doesn't know the
    /// job), or it isn't done by `timeout`.
    fn wait(&self, timeout: Option<Duration>) -> Option<u8> {
        let start = Instant::now();
        loop {
            let print = launchctl(&[OsStr::new("print"), OsStr::new(&self.target)]).ok()?;
            if !known(&print) {
                return None;
            }
            if let Some(code) = finished(&print) {
                return Some(code);
            }
            if timeout.is_some_and(|t| start.elapsed() > t) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn interrupt(target: &str) {
        let _ = launchctl(&[OsStr::new("kill"), OsStr::new("SIGINT"), OsStr::new(target)]);
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        let _ = launchctl(&[OsStr::new("bootout"), OsStr::new(&self.target)]);
    }
}

/// The copy of this executable a launchd job wrote, made again when it isn't this executable's
/// bytes or carries provenance.
pub(crate) fn clean_copy() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    copy_of(&exe, &home().ok_or("no folder for the copy")?)
}

/// How a launchd job writes the copy: `cat` writes the bytes alone, and as launchd started it,
/// the file carries no provenance (`cp` would copy the attribute). Each job writes a part of its
/// own and renames it into place, so two commands making the copy at once never share a file.
const COPY_SCRIPT: &str =
    r#"umask 077 && /bin/cat "$1" > "$2.$$.part" && /bin/chmod 700 "$2.$$.part" && /bin/mv -f "$2.$$.part" "$2""#;

/// [`clean_copy`] of `exe`, kept in `home`.
fn copy_of(exe: &Path, home: &Path) -> Result<PathBuf, String> {
    copy_with(exe, home, COPY_SCRIPT)
}

/// [`copy_of`], the copy written by `script` (`sh -c`, given `exe` and the copy's path).
fn copy_with(exe: &Path, home: &Path, script: &str) -> Result<PathBuf, String> {
    let copy = home.join("bin/locallibrary");
    let want = Hash::of(&std::fs::read(exe).map_err(|e| format!("reading {}: {e}", exe.display()))?);
    if is_current(&copy, &want) {
        return Ok(copy);
    }
    std::fs::create_dir_all(copy.parent().expect("a file in a folder")).map_err(|e| e.to_string())?;
    let args: Vec<OsString> = ["/bin/sh", "-c", script, "sh"]
        .map(OsString::from)
        .into_iter()
        .chain([exe.into(), copy.clone().into()])
        .collect();
    let cwd = copy.parent().expect("a file in a folder");
    // A label of this process's own: starting a job takes out any other of its label first.
    let label = format!("local.locallibrary.copy.{}", std::process::id());
    let job = Job::start(home, &label, &plist(&label, &args, &[], cwd, None))?;
    match job.wait(Some(COPY_TIMEOUT)) {
        Some(0) if is_current(&copy, &want) => Ok(copy),
        Some(0) => Err("the copy isn't this executable, or carries provenance".into()),
        Some(code) => Err(format!("making the copy failed ({code})")),
        None => Err("making the copy didn't finish".into()),
    }
}

/// Whether this run goes to a launchd job: a command that writes drives, from a terminal (stdin,
/// stdout and stderr all one), and not in a job already.
pub(crate) fn wanted(writes_drives: bool, terminal: bool, in_job: bool) -> bool {
    writes_drives && terminal && !in_job
}

/// Runs this command line as a launchd job of [`clean_copy`], in this terminal, and answers its
/// exit status; `None` when it should run here (not [`wanted`], or launchd or the copy failed,
/// which it says on stderr).
pub(crate) fn run_as_job(writes_drives: bool) -> Option<ExitCode> {
    let terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal() && std::io::stderr().is_terminal();
    if !wanted(writes_drives, terminal, std::env::var_os(IN_JOB).is_some()) {
        return None;
    }
    let started = clean_copy().and_then(|copy| {
        let tty = tty_path().ok_or("no terminal device")?;
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let args: Vec<OsString> = std::iter::once(copy.into_os_string()).chain(std::env::args_os().skip(1)).collect();
        let label = format!("local.locallibrary.write.{}", std::process::id());
        let env = job_env(std::env::vars());
        let home = home().ok_or("no folder for the job's definition")?;
        Job::start(&home, &label, &plist(&label, &args, &env, &cwd, Some(&tty)))
    });
    let job = match started {
        Ok(job) => job,
        Err(e) => {
            eprintln!(
                "locallibrary: writing from here, not a launchd job ({e}), so macOS adds ._ files: `drive tidy` removes them"
            );
            return None;
        }
    };
    // Ctrl-C reaches this process, not the job: pass it on, and the job stops as it would here.
    let target = job.target.clone();
    let _ = ctrlc::set_handler(move || Job::interrupt(&target));
    match job.wait(None) {
        Some(code) => Some(ExitCode::from(code)),
        None => {
            eprintln!("locallibrary: lost track of the launchd job ({}); its write may still be going", job.target);
            Some(ExitCode::FAILURE)
        }
    }
}

/// The terminal device this process's stdout is.
fn tty_path() -> Option<PathBuf> {
    // SAFETY: ttyname returns a pointer to a static buffer (or null), read at once.
    let name = unsafe { libc::ttyname(libc::STDOUT_FILENO) };
    (!name.is_null()).then(|| {
        // SAFETY: non-null, NUL-terminated, valid until the next ttyname call.
        let bytes = unsafe { std::ffi::CStr::from_ptr(name) }.to_bytes();
        PathBuf::from(OsStr::from_bytes(bytes))
    })
}

#[cfg(test)]
mod tests;
