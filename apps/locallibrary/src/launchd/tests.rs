use super::*;

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// A write's job runs the copy with the command's arguments, in the terminal's folder and
/// device, at Interactive priority, once.
#[test]
fn a_job_runs_the_command_in_the_terminal_at_interactive_priority() {
    let env = vec![("HOME".to_string(), "/Users/dj".to_string())];
    let tty = Path::new("/dev/ttys004");
    let xml = plist(
        "local.locallibrary.write.7",
        &os(&["/copy/locallibrary", "sync", "A & B"]),
        &env,
        Path::new("/work"),
        Some(tty),
    );
    for part in [
        "<key>Label</key><string>local.locallibrary.write.7</string>",
        "    <string>/copy/locallibrary</string>\n    <string>sync</string>\n    <string>A &amp; B</string>\n",
        "<key>HOME</key><string>/Users/dj</string>",
        "<key>WorkingDirectory</key><string>/work</string>",
        "<key>StandardInPath</key><string>/dev/ttys004</string>",
        "<key>StandardOutPath</key><string>/dev/ttys004</string>",
        "<key>StandardErrorPath</key><string>/dev/ttys004</string>",
        "<key>ProcessType</key><string>Interactive</string>",
        "<key>RunAtLoad</key><true/>",
    ] {
        assert!(xml.contains(part), "{part} missing from\n{xml}");
    }
    assert!(!xml.contains("KeepAlive"), "it runs once");
    assert!(!plist("l", &os(&["/bin/true"]), &[], Path::new("/"), None).contains("StandardInPath"));
}

/// A job gets what it needs of the terminal's environment and locallibrary's own variables,
/// nothing else, with paging off and its own mark set.
#[test]
fn a_job_s_environment_is_what_it_needs() {
    let vars = [
        ("HOME", "/Users/dj"),
        ("LOCALLIBRARY_STORE", "/store"),
        ("LOCALLIBRARY_LAUNCHD_JOB", "stale"),
        ("GITHUB_TOKEN", "secret"),
        ("PAGER", "less"),
        ("TERM", "xterm-256color"),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string()));
    let env = job_env(vars.into_iter());
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(
        pairs,
        [
            ("HOME", "/Users/dj"),
            ("LOCALLIBRARY_LAUNCHD_JOB", "1"),
            ("LOCALLIBRARY_STORE", "/store"),
            ("PAGER", ""),
            ("TERM", "xterm-256color"),
        ]
    );
}

/// What `launchctl print` says of a job: still running, an exit code, or a signal.
#[test]
fn a_finished_job_s_exit_code_is_read_from_launchd() {
    let print = |state: &str, last: &str| {
        format!("gui/501/x = {{\n\tstate = {state}\n\truns = 1\n\t{last}\n\t\tstate = active\n}}\n")
    };
    assert_eq!(finished(&print("running", "last exit code = (never exited)")), None);
    assert_eq!(finished(&print("not running", "last exit code = 3")), Some(3));
    assert_eq!(finished(&print("not running", "last terminating signal = Interrupt: 2")), Some(130));
    assert_eq!(finished(&print("not running", "last exit code = (odd)")), Some(1));
    assert_eq!(finished("nothing launchd would say"), None);
}

/// Only a command that writes drives, run from a terminal, and not already in a job, goes to
/// one.
#[test]
fn only_drive_writes_from_a_terminal_go_to_a_job() {
    assert!(wanted(true, true, false));
    assert!(!wanted(false, true, false), "a read");
    assert!(!wanted(true, false, false), "piped, or in a script");
    assert!(!wanted(true, true, true), "the job itself");
}

/// A copy runs only while it's the executable's bytes, and carries no provenance.
#[test]
fn only_a_copy_with_the_executable_s_bytes_runs() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("locallibrary");
    let want = Hash::of(b"this build");
    assert!(!holds(&copy, &want) && !is_current(&copy, &want), "no copy");
    std::fs::write(&copy, b"an older build").unwrap();
    assert!(!holds(&copy, &want) && !is_current(&copy, &want), "a stale or changed copy");
    std::fs::write(&copy, b"this build").unwrap();
    assert!(holds(&copy, &want));
    // A file this test writes carries provenance when the tests run from a terminal app.
    assert_eq!(is_current(&copy, &want), !has_provenance(&copy));
}

/// A job launchd runs reports its exit code. Skipped where there's no launchd session to run
/// it in (a CI runner without a login).
#[test]
fn a_job_reports_its_exit_code() {
    let label = format!("local.locallibrary.test.{}", std::process::id());
    let dir = tempfile::tempdir().unwrap();
    let Ok(job) =
        Job::start(dir.path(), &label, &plist(&label, &os(&["/bin/sh", "-c", "exit 3"]), &[], dir.path(), None))
    else {
        return;
    };
    assert_eq!(job.wait(Some(Duration::from_secs(30))), Some(3));
}

/// A job launchd doesn't know (a `launchctl` that started nothing says nothing of it) isn't
/// waited for.
#[test]
fn only_a_job_launchd_knows_is_waited_for() {
    assert!(known("gui/501/x = {\n\tstate = running\n}\n"));
    assert!(!known(""));
    assert!(!known("Could not find service \"x\" in domain for user gui: 501\n"));
}

/// The copy is written by a launchd job, carries no provenance, and is made again when the
/// executable changes. Skipped where there's no launchd session to run the job in.
#[test]
fn the_copy_is_launchd_s_and_follows_the_executable() {
    if !session() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (exe, home) = (dir.path().join("locallibrary"), dir.path().join("home"));
    std::fs::write(&exe, b"build 1").unwrap();
    let copy = copy_of(&exe, &home).unwrap();
    assert_eq!(copy, home.join("bin/locallibrary"));
    assert_eq!(std::fs::read(&copy).unwrap(), b"build 1");
    assert!(!has_provenance(&copy), "written by launchd");
    std::fs::write(&exe, b"build 2").unwrap();
    assert_eq!(std::fs::read(copy_of(&exe, &home).unwrap()).unwrap(), b"build 2");
    // A copy that's current is used as it is: no job, so no definition is left behind either.
    std::fs::remove_dir_all(home.join("jobs")).unwrap();
    assert_eq!(copy_of(&exe, &home).unwrap(), copy);
    assert!(!home.join("jobs").exists());
}

/// Whether there's a launchd session to run jobs in: a Mac someone's logged in to (GitHub's
/// macOS runners log their user in). Tests that need one say so and skip where there's none.
fn session() -> bool {
    let there = launchctl(&[OsStr::new("print"), OsStr::new(&format!("gui/{}", crate::autostart::uid()))]).is_ok();
    if !there {
        eprintln!("skipped: no launchd GUI session to run jobs in");
    }
    there
}

/// A job that isn't done by the time given isn't waited for longer, and is taken out of launchd
/// once dropped.
#[test]
fn a_job_that_isn_t_done_in_time_is_given_up_on() {
    if !session() {
        return;
    }
    let label = format!("local.locallibrary.test.slow.{}", std::process::id());
    let dir = tempfile::tempdir().unwrap();
    let job =
        Job::start(dir.path(), &label, &plist(&label, &os(&["/bin/sleep", "30"]), &[], dir.path(), None)).unwrap();
    let started = Instant::now();
    assert_eq!(job.wait(Some(Duration::from_millis(200))), None);
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    let target = job.target.clone();
    drop(job);
    assert!(launchctl(&[OsStr::new("print"), OsStr::new(&target)]).is_err(), "{target} is gone");
}

/// A copy the job made that isn't the executable's bytes, or a job that failed, is never run.
#[test]
fn a_copy_that_isn_t_the_executable_is_refused() {
    if !session() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (exe, home) = (dir.path().join("locallibrary"), dir.path().join("home"));
    std::fs::write(&exe, b"build 1").unwrap();
    let corrupt = copy_with(&exe, &home, r#"printf corrupt > "$2""#).unwrap_err();
    assert_eq!(corrupt, "the copy isn't this executable, or carries provenance");
    assert_eq!(copy_with(&exe, &home, "exit 3").unwrap_err(), "making the copy failed (3)");
}
