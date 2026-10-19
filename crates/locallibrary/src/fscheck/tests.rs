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
    assert!(matches!(&refused, Err(Error::Invalid(m)) if m.starts_with("LOCALLIBRARY_VOLUMES is set")), "{refused:?}");
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

/// What a check runs, everywhere: diskutil verifies or repairs the volume itself; on Linux the
/// partition is unmounted, fsck'd with its file system's tool (`-n` to check, `-a` to repair) and
/// mounted again (for the user who ran sudo, when that's someone); on Windows chkdsk takes the
/// drive's letter, and `/f /x` to repair.
#[hegel::test(test_cases = 100)]
fn every_platform_checks_the_volume_with_its_own_tool(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let repair = tc.draw(gs::booleans());
    #[cfg(target_os = "macos")]
    {
        let root = tc.draw(gs::sampled_from(vec!["/Volumes/USB", "/Volumes/Club Stick"]));
        let s = steps(Path::new(root), None, None, repair).unwrap();
        assert_eq!(s, Steps::only(Cmd::new("diskutil", &[if repair { "repairVolume" } else { "verifyVolume" }, root])));
    }
    let volume = tc.draw(gs::sampled_from(vec!["/dev/sdb1", "/dev/mmcblk0p1"]));
    let fs = tc.draw(gs::optional(gs::sampled_from(vec!["vfat", "exfat", "ExFAT"])));
    let user = tc.draw(gs::optional(gs::sampled_from(vec!["sam", "root", ""])));
    let s = linux_steps(volume, fs, repair, user);
    let tool = if fs.is_some_and(|f| f.eq_ignore_ascii_case("exfat")) { "fsck.exfat" } else { "fsck.vfat" };
    assert_eq!(s.check.display(), format!("{tool} {} {volume}", if repair { "-a" } else { "-n" }));
    assert_eq!(s.unmount.as_ref().unwrap().display(), format!("udisksctl unmount --no-user-interaction -b {volume}"));
    let mount = format!("udisksctl mount --no-user-interaction -b {volume}");
    let as_user = user.filter(|u| !u.is_empty() && *u != "root");
    assert_eq!(s.mount.as_ref().unwrap().display(), as_user.map_or(mount.clone(), |u| format!("sudo -u {u} {mount}")));
    let letter = tc.draw(gs::sampled_from(vec!['D', 'E', 'z']));
    let root = tc.draw(gs::sampled_from(vec![format!("{letter}:\\"), format!("{letter}:"), format!("{letter}:/")]));
    let w = windows_steps(Path::new(&root), repair).unwrap();
    let drive = format!("{}:", letter.to_ascii_uppercase());
    assert_eq!(w.display(), format!("chkdsk {drive}{}", if repair { " /f /x" } else { "" }));
    assert!(windows_steps(Path::new(&format!("{letter}:\\Music")), repair).is_err());
}

/// The logbook line says "repaired" only after a repair that left the file system consistent,
/// "errors" exactly when it isn't, and mentions the repair whenever one ran.
#[hegel::test(test_cases = 20)]
#[hegel::explicit_test_case(ok = true, repaired = true)]
#[hegel::explicit_test_case(ok = true, repaired = false)]
#[hegel::explicit_test_case(ok = false, repaired = true)]
#[hegel::explicit_test_case(ok = false, repaired = false)]
fn the_logbook_line_follows_the_verdict_and_the_repair(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let ok = tc.draw(gs::booleans());
    let repaired = tc.draw(gs::booleans());
    let report = FsCheckReport {
        drive: "Main".into(),
        root: "/Volumes/MAIN".into(),
        volume: None,
        filesystem: None,
        command: String::new(),
        ok,
        repaired,
        output: Vec::new(),
        warnings: Vec::new(),
    };
    let line = report.summary();
    assert!(line.starts_with("file system "), "{line}");
    assert_eq!(line.contains("errors"), !ok, "{line}");
    assert_eq!(line.contains("repair"), repaired, "{line}");
    assert_eq!(line == "file system consistent", ok && !repaired);
}

/// A check tool that fails saying it may not read the partition is a permission error naming
/// the command; any other failure is the tool's verdict, and a tool that can't be started is an
/// external error.
#[cfg(unix)]
#[hegel::test(test_cases = 30)]
#[hegel::explicit_test_case(phrase = "Permission denied", status = 6, missing = false, to_stderr = true)]
#[hegel::explicit_test_case(phrase = "MUST BE ROOT", status = 0, missing = false, to_stderr = false)]
#[hegel::explicit_test_case(phrase = "", status = 0, missing = true, to_stderr = false)]
fn a_tool_refused_the_partition_needs_an_administrator(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let phrase = tc.draw(gs::sampled_from(vec![
        "Permission denied",
        "PERMISSION DENIED",
        "Access is denied",
        "access denied",
        "fsck.vfat: must be root",
        "MUST BE ROOT",
        "Dirty bit is set",
        "",
    ]));
    let status = tc.draw(gs::integers::<i32>().min_value(0).max_value(8));
    let missing = tc.draw(gs::booleans());
    let to_stderr = tc.draw(gs::booleans());
    let program = if missing { "locallibrary-no-such-check-tool" } else { "sh" };
    let script = format!("echo '{phrase}'{}; exit {status}", if to_stderr { " >&2" } else { "" });
    let cmd = Cmd::new(program, &["-c", &script]);
    let lower = phrase.to_lowercase();
    let refused = ["permission denied", "access denied", "must be root"].iter().any(|p| lower.contains(p));
    match run_status(&cmd) {
        Ok((ok, text)) => {
            assert!(!missing && (!refused || status == 0), "{phrase:?} {status}");
            assert_eq!(ok, status == 0, "{text:?}");
            assert_eq!(text.trim_end(), phrase);
        }
        Err(Error::Permission(m)) => {
            assert!(!missing && refused && status != 0, "{phrase:?} {status}: {m}");
            assert_eq!(m, format!("{} needs administrator rights", cmd.display()));
        }
        Err(Error::External { command, .. }) => {
            assert!(missing);
            assert_eq!(command, program);
        }
        Err(e) => panic!("{e}"),
    }
}

/// Before anything is unmounted: a partition this user may not read stops the check, naming it
/// and leaving the drive mounted; otherwise the unmount runs, and when it fails the check doesn't,
/// saying why and what to do (install udisks2 when it's missing, close what holds the drive
/// otherwise).
#[cfg(unix)]
#[hegel::test(test_cases = 30)]
#[hegel::explicit_test_case(partition = 2u8, unmount_fails = false, missing = false, message = "target is busy")]
#[hegel::explicit_test_case(partition = 0u8, unmount_fails = true, missing = false, message = "target is busy")]
#[hegel::explicit_test_case(partition = 1u8, unmount_fails = true, missing = true, message = "no such volume")]
#[hegel::explicit_test_case(partition = 3u8, unmount_fails = false, missing = false, message = "no such volume")]
fn the_check_runs_only_on_a_partition_unmounted_for_it(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::os::unix::fs::PermissionsExt;
    // None, missing, unreadable, readable.
    let partition = tc.draw(gs::integers::<u8>().max_value(3));
    let unmount_fails = tc.draw(gs::booleans());
    let missing = tc.draw(gs::booleans());
    let message = tc.draw(gs::sampled_from(vec!["target is busy", "Error unmounting: device busy", "no such volume"]));
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("unmounted");
    let part = dir.path().join("sdb1");
    if partition >= 2 {
        std::fs::write(&part, b"FAT32").unwrap();
    }
    if partition == 2 {
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    let part_str = part.to_string_lossy().into_owned();
    let script = format!(
        "touch '{}'; {}",
        marker.display(),
        if unmount_fails { format!("echo '{message}' >&2; exit 1") } else { "exit 0".into() }
    );
    let unmounting = Cmd::new(if missing { "locallibrary-no-such-udisksctl" } else { "sh" }, &["-c", &script]);
    let check = Cmd::new("fsck.vfat", &["-n", &part_str]);
    // Root reads anything: what the partition says is what this user may do.
    let denied =
        partition == 2 && std::fs::File::open(&part).is_err_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied);
    let result = unmount(&unmounting, &check, (partition > 0).then_some(part_str.as_str()));
    if partition == 2 {
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    match result {
        Ok(()) => {
            assert!(!denied && !missing && !unmount_fails);
            assert!(marker.exists(), "the unmount ran");
        }
        Err(Error::Permission(m)) => {
            assert!(denied, "{m}");
            assert!(m.starts_with(&check.display()) && m.contains("administrator"), "{m}");
            assert!(!marker.exists(), "nothing was unmounted");
        }
        Err(Error::External { message: m, .. }) => {
            assert!(!denied && (missing || unmount_fails), "{m}");
            assert!(m.contains("so it didn't run"), "{m}");
            match missing {
                true => assert!(
                    m.starts_with("not found")
                        && m.ends_with("install udisks2, which unmounts the drive for the check)"),
                    "{m}"
                ),
                false => {
                    assert!(marker.exists());
                    assert!(
                        m.starts_with(message)
                            && m.ends_with("close any window or program using the drive, then try again)"),
                        "{m}"
                    );
                }
            }
        }
        Err(e) => panic!("{e}"),
    }
}

/// The check asks nothing of the system while it can't run: under the test switch the volume is
/// the machine's own, and otherwise a drive locallibrary is writing is refused at once.
#[hegel::test(test_cases = 10)]
#[hegel::explicit_test_case(switch = true, repair = false)]
#[hegel::explicit_test_case(switch = false, repair = true)]
fn a_check_that_cant_run_asks_nothing_of_the_system(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let switch = tc.draw(gs::booleans());
    let repair = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    // Held throughout: without the switch the check must stop here, never reaching the OS tool.
    let _writing = d.lock().unwrap();
    crate::testing::volumes_on_this_thread(switch.then(|| vec![dir.path().to_path_buf()]));
    let refused = check(&d, repair);
    crate::testing::volumes_on_this_thread(None);
    match refused {
        Err(Error::Invalid(m)) => assert!(switch && m.starts_with("LOCALLIBRARY_VOLUMES is set"), "{m}"),
        Err(Error::Locked(root)) => assert!(!switch && root == d.root(), "{root:?}"),
        other => panic!("{other:?}"),
    }
}

#[cfg(unix)]
use crate::format::tests::fake_tools;

/// A check finds the volume at the drive's root through the platform's disk tools and runs that
/// platform's steps for it, in order, whatever the check finds: the report says the steps it
/// ran, is consistent exactly when the check tool succeeded, keeps the tool's last 15 lines and
/// adds what mounting again said (on Linux), or that it couldn't and to plug the drive in again.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
// More lines than the report keeps.
#[hegel::explicit_test_case(
    fs = "vfat",
    repair = false,
    lines = (1..=20).map(|n| format!("phase {n}")).collect::<Vec<String>>(),
    consistent = false,
    mounts_again = false
)]
fn a_check_runs_its_steps_and_reports_what_the_tool_found(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    std::fs::create_dir(fake.dir().join("DNA")).unwrap();
    let d = Device::open(fake.dir().join("DNA")).unwrap();
    let fs = tc.draw(gs::sampled_from(vec!["vfat", "exfat"]));
    let (volume, fs_name) = fake.volume_at(d.root(), fs, true);
    let repair = tc.draw(gs::booleans());
    let planned = steps(d.root(), Some(&volume), Some(&fs_name), repair).unwrap();
    let lines: Vec<String> = tc.draw(gs::vecs(gs::text().alphabet("abc 019:*").max_size(12)).max_size(25));
    let consistent = tc.draw(gs::booleans());
    fn words(c: &Cmd) -> Vec<&str> {
        c.args.iter().map(String::as_str).collect()
    }
    let check_words = words(&planned.check);
    if consistent {
        fake.answer(&planned.check.program, &check_words, &lines.join("\n"));
    } else {
        fake.refuse(&planned.check.program, &check_words, &lines.join("\n"));
    }
    if let Some(u) = &planned.unmount {
        fake.answer(&u.program, &words(u), "");
    }
    let mounts_again = tc.draw(gs::booleans());
    if let Some(m) = &planned.mount {
        if mounts_again {
            fake.answer(&m.program, &words(m), "Mounted /dev/sdb1 at /media/dj/DNA");
        } else {
            fake.refuse(&m.program, &words(m), "Error mounting: not authorized");
        }
    }

    let report = check(&d, repair).unwrap();
    let ran: Vec<Cmd> = fake
        .calls()
        .into_iter()
        .filter(|c| {
            !["diskutil", "plutil", "ioreg", "lsblk"].contains(&c.program.as_str())
                || c.args.first().is_some_and(|a| a.ends_with("Volume"))
        })
        .collect();
    let want: Vec<Cmd> =
        [planned.unmount.clone(), Some(planned.check.clone()), planned.mount.clone()].into_iter().flatten().collect();
    assert_eq!(ran, want);
    assert_eq!((report.command, report.ok, report.repaired), (planned.display(), consistent, repair));
    assert_eq!((report.volume, report.filesystem), (Some(volume), Some(fs_name)));
    let mut output: Vec<String> = lines.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    output.drain(..output.len().saturating_sub(15));
    match &planned.mount {
        Some(_) if mounts_again => output.push("Mounted /dev/sdb1 at /media/dj/DNA".into()),
        // A tool that fails says which command it was (Error::External).
        Some(m) => output.push(format!(
            "couldn't mount it again ({} failed: Error mounting: not authorized): unplug the drive and plug it back in",
            m.display()
        )),
        None => {}
    }
    assert_eq!(report.output, output);
}
