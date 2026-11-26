#[cfg(unix)]
use loclib_format::Cmd;
#[cfg(unix)]
use loclib_format::test_util::FakeTools;

use super::*;
use crate::Error;

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

/// The logbook line says "repaired" only after a repair that left the file system consistent,
/// "errors" exactly when it isn't, and mentions the repair whenever one ran.
#[hegel::test(test_cases = 20)]
#[hegel::explicit_test_case(ok = true, repaired = true)]
#[hegel::explicit_test_case(ok = true, repaired = false)]
#[hegel::explicit_test_case(ok = false, repaired = true)]
#[hegel::explicit_test_case(ok = false, repaired = false)]
fn prop_the_logbook_line_follows_the_verdict_and_the_repair(tc: hegel::TestCase) {
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

/// The check asks nothing of the system while it can't run: under the test switch the volume is
/// the machine's own, and otherwise a drive locallibrary is writing is refused at once.
#[hegel::test(test_cases = 10)]
#[hegel::explicit_test_case(switch = true, repair = false)]
#[hegel::explicit_test_case(switch = false, repair = true)]
fn prop_a_check_that_cant_run_asks_nothing_of_the_system(tc: hegel::TestCase) {
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
fn prop_a_check_runs_its_steps_and_reports_what_the_tool_found(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = FakeTools::new();
    std::fs::create_dir(fake.dir().join("DNA")).unwrap();
    let d = Device::open(fake.dir().join("DNA")).unwrap();
    let fs = tc.draw(gs::sampled_from(vec!["vfat", "exfat"]));
    let (volume, fs_name) = fake.volume_at(d.root(), fs, true);
    let repair = tc.draw(gs::booleans());
    let planned = fscheck::steps(d.root(), Some(&volume), Some(&fs_name), repair).unwrap();
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
