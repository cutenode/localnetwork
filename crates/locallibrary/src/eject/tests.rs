use super::*;

#[cfg(target_os = "macos")]
#[test]
fn macos_ejects_with_diskutil() {
    let cmds = commands(Path::new("/Volumes/REKORDBOX")).unwrap();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].display(), "diskutil eject /Volumes/REKORDBOX");
}

/// Every mounted partition goes, then the power; a volume that isn't on the disk is refused.
#[test]
fn linux_unmounts_the_whole_disk_then_powers_it_off() {
    let mut disk = crate::format::tests::sample_disk();
    (disk.id, disk.device) = ("/dev/sdb".into(), "/dev/sdb".into());
    disk.volumes[0].id = "/dev/sdb1".into();
    disk.volumes[0].mount = Some("/media/sam/DNA".into());
    let mut other = disk.volumes[0].clone();
    (other.id, other.mount) = ("/dev/sdb2".into(), Some("/media/sam/STORE".into()));
    disk.volumes.push(other);
    let shown: Vec<String> =
        linux_commands(&disk, Path::new("/media/sam/DNA")).unwrap().iter().map(Cmd::display).collect();
    assert_eq!(
        shown,
        [
            "udisksctl unmount --no-user-interaction -b /dev/sdb1",
            "udisksctl unmount --no-user-interaction -b /dev/sdb2",
            "udisksctl power-off --no-user-interaction -b /dev/sdb",
        ]
    );
    assert!(linux_commands(&disk, Path::new("/media/sam/OTHER")).is_err());
}

#[test]
fn windows_ejects_the_drive_letter_through_explorer() {
    for root in [r"E:\", "E:", "E:/"] {
        let cmd = windows_command(Path::new(root)).unwrap();
        assert_eq!(cmd.program, "powershell");
        assert_eq!(
            cmd.args.last().unwrap(),
            "(New-Object -ComObject Shell.Application).Namespace(17).ParseName('E:').InvokeVerb('Eject')"
        );
    }
    assert!(windows_command(Path::new(r"E:\Music")).is_err(), "only a drive's root");
}

/// Ejecting doesn't wait for a running operation, and asks nothing of the system meanwhile.
#[test]
fn a_drive_in_use_is_not_ejected() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    let _syncing = d.lock().unwrap();
    assert!(matches!(eject(&d), Err(Error::Locked(_))));
}

#[test]
fn a_busy_drive_says_what_to_do() {
    let busy = |message: &str| {
        let e = busy_hint(Error::External { command: "diskutil eject /Volumes/DNA".into(), message: message.into() });
        match e {
            Error::External { message, .. } => message,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(
        busy("Volume DNA on disk4s1 failed to eject: Resource busy"),
        "Volume DNA on disk4s1 failed to eject: Resource busy (close any window or program using the drive, then try again)"
    );
    assert!(busy("The volume is in use by Music").ends_with("then try again)"));
    assert_eq!(busy("Unable to find disk for /Volumes/DNA"), "Unable to find disk for /Volumes/DNA");
    assert!(matches!(busy_hint(Error::Cancelled), Error::Cancelled));
}

#[test]
fn a_volume_that_is_gone_is_ejected() {
    let dir = tempfile::tempdir().unwrap();
    assert!(wait_gone(&dir.path().join("E"), Duration::ZERO));
    assert!(!wait_gone(dir.path(), Duration::ZERO), "still there");
}

/// Whatever partitions a disk has, ejecting one of its mounted volumes unmounts each mounted
/// one, in order, then powers the disk off; a root none of them is mounted at is refused.
#[hegel::test(test_cases = 200)]
fn linux_ejects_every_mounted_partition_of_the_disk(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut disk = crate::format::tests::sample_disk();
    (disk.id, disk.device) = ("/dev/sdc".into(), "/dev/sdc".into());
    let template = disk.volumes[0].clone();
    disk.volumes.clear();
    for n in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let mut v = template.clone();
        v.id = format!("/dev/sdc{}", n + 1);
        v.mount = tc.draw(gs::booleans()).then(|| PathBuf::from(format!("/media/sam/P{n}")));
        disk.volumes.push(v);
    }
    let root = PathBuf::from(format!("/media/sam/P{}", tc.draw(gs::integers::<usize>().max_value(4))));
    let mounted: Vec<&str> = disk.volumes.iter().filter(|v| v.mount.is_some()).map(|v| v.id.as_str()).collect();
    match linux_commands(&disk, &root) {
        Err(_) => assert!(!disk.volumes.iter().any(|v| v.mount.as_deref() == Some(root.as_path()))),
        Ok(cmds) => {
            let shown: Vec<String> = cmds.iter().map(Cmd::display).collect();
            let mut want: Vec<String> =
                mounted.iter().map(|id| format!("udisksctl unmount --no-user-interaction -b {id}")).collect();
            want.push("udisksctl power-off --no-user-interaction -b /dev/sdc".into());
            assert_eq!(shown, want);
        }
    }
    let letter = tc.draw(gs::sampled_from(vec!['E', 'f', '7']));
    let windows = windows_command(Path::new(&format!("{letter}:\\")));
    assert_eq!(windows.is_ok(), letter.is_ascii_alphabetic());
    if let Ok(cmd) = windows {
        assert!(cmd.args.last().unwrap().contains(&format!("'{}:'", letter.to_ascii_uppercase())), "{cmd:?}");
    }
}

/// Ejecting, everywhere: on Linux every mounted partition of the disk is unmounted and then the
/// disk powered off (a mount point not on the disk is refused); on Windows Explorer's Eject takes
/// the drive's letter, and only a drive's root.
#[hegel::test(test_cases = 100)]
fn every_platform_ejects_the_whole_drive(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut disk = crate::format::tests::sample_disk();
    (disk.id, disk.device) = ("/dev/sdb".into(), "/dev/sdb".into());
    let template = disk.volumes[0].clone();
    disk.volumes = (1..=tc.draw(gs::integers::<usize>().min_value(1).max_value(4)))
        .map(|n| {
            let mut v = template.clone();
            v.id = format!("/dev/sdb{n}");
            v.mount = tc.draw(gs::booleans()).then(|| PathBuf::from(format!("/media/sam/P{n}")));
            v
        })
        .collect();
    let mounted: Vec<String> = disk.volumes.iter().filter(|v| v.mount.is_some()).map(|v| v.id.clone()).collect();
    let root = PathBuf::from(format!("/media/sam/P{}", tc.draw(gs::integers::<usize>().min_value(1).max_value(5))));
    match linux_commands(&disk, &root) {
        Ok(cmds) => {
            assert!(disk.volumes.iter().any(|v| v.mount.as_deref() == Some(root.as_path())));
            let mut want: Vec<String> =
                mounted.iter().map(|id| format!("udisksctl unmount --no-user-interaction -b {id}")).collect();
            want.push("udisksctl power-off --no-user-interaction -b /dev/sdb".into());
            assert_eq!(cmds.iter().map(Cmd::display).collect::<Vec<_>>(), want);
        }
        Err(e) => assert!(disk.volumes.iter().all(|v| v.mount.as_deref() != Some(root.as_path())), "{e}"),
    }
    let letter = tc.draw(gs::sampled_from(vec!['e', 'F', 'Z']));
    let cmd = windows_command(Path::new(&format!("{letter}:\\"))).unwrap();
    assert_eq!(cmd.program, "powershell");
    assert!(cmd.args.last().unwrap().contains(&format!("'{}:'", letter.to_ascii_uppercase())), "{cmd:?}");
    assert!(windows_command(Path::new(&format!("{letter}:\\Music"))).is_err());
    #[cfg(target_os = "macos")]
    assert_eq!(commands(&root).unwrap()[0].display(), format!("diskutil eject {}", root.display()));
}

/// A tool's failure gets the hint to close what holds the drive exactly when it says the drive is
/// busy or in use; the command and the tool's words are kept as they were, and other errors pass
/// through untouched.
#[hegel::test(test_cases = 60)]
#[hegel::explicit_test_case(word = "Resource BUSY", external = true)]
#[hegel::explicit_test_case(word = "in use", external = true)]
#[hegel::explicit_test_case(word = "In Use", external = true)]
#[hegel::explicit_test_case(word = "busy", external = false)]
fn only_a_busy_drive_gets_the_hint(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let word = tc.draw(gs::sampled_from(vec!["busy", "Resource BUSY", "in use", "In Use", "unmounted", ""]));
    let external = tc.draw(gs::booleans());
    let message = format!("Volume DNA failed to eject: {word}");
    let command = "diskutil eject /Volumes/DNA".to_string();
    let e = match external {
        true => Error::External { command: command.clone(), message: message.clone() },
        false => Error::Locked(PathBuf::from(&message)),
    };
    let busy = message.to_lowercase().contains("busy") || message.contains("in use");
    match busy_hint(e) {
        Error::External { command: c, message: m } => {
            assert!(external);
            assert_eq!(c, command);
            let hint = " (close any window or program using the drive, then try again)";
            assert_eq!(m, if busy { format!("{message}{hint}") } else { message.clone() });
        }
        Error::Locked(p) => assert!(!external && p == Path::new(&message)),
        other => panic!("{other:?}"),
    }
}

/// Waiting for a volume to go answers whether it's gone when the wait ends: at once when it never
/// was there, after it disappears partway, never while it stays.
#[hegel::test(test_cases = 8)]
#[hegel::explicit_test_case(there = false, goes = false)]
#[hegel::explicit_test_case(there = true, goes = true)]
#[hegel::explicit_test_case(there = true, goes = false)]
fn a_volume_is_gone_once_it_disappears(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let there = tc.draw(gs::booleans());
    let goes = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("DNA");
    if there {
        std::fs::create_dir(&root).unwrap();
    }
    let remover = (there && goes).then(|| {
        let root = root.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            std::fs::remove_dir(&root).unwrap();
        })
    });
    let start = Instant::now();
    let gone = wait_gone(&root, Duration::from_millis(if there && !goes { 300 } else { 2_000 }));
    if let Some(r) = remover {
        r.join().unwrap();
    }
    assert_eq!(gone, !there || goes);
    if !there {
        assert!(start.elapsed() < Duration::from_millis(250), "no wait for a volume never there");
    }
    if there && !goes {
        assert!(start.elapsed() >= Duration::from_millis(300), "waited the whole time");
    }
}

/// Ejecting never waits for a running operation, nor asks the system anything meanwhile, whoever
/// holds the drive's lock.
#[hegel::test(test_cases = 6)]
fn a_drive_being_written_is_never_ejected(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let adopted = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    if adopted {
        d.adopt(Some("DNA")).unwrap();
    }
    // Held throughout: were it not, the system's eject would run on this folder's volume.
    let _writing = d.lock().unwrap();
    let again = Device::open(dir.path()).unwrap();
    match eject(&again) {
        Err(Error::Locked(root)) => assert_eq!(root, d.root()),
        other => panic!("{other:?}"),
    }
}

#[cfg(unix)]
use crate::format::tests::fake_tools;

/// Ejecting runs the platform's commands for the drive's root, in order, and reports them as
/// run; the first that fails stops it in the tool's own words, with what to do when the drive is
/// busy.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn ejecting_runs_its_commands_in_order_until_one_fails(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    std::fs::create_dir(fake.dir().join("DNA")).unwrap();
    let d = Device::open(fake.dir().join("DNA")).unwrap();
    fake.volume_at(d.root(), "vfat", true);
    let cmds = commands(d.root()).unwrap();
    let fails_at =
        tc.draw(gs::weighted_booleans(0.4)).then(|| tc.draw(gs::integers::<usize>().max_value(cmds.len() - 1)));
    let why = tc.draw(gs::sampled_from(vec!["Device or resource busy", "Not authorized to perform operation"]));
    for (i, c) in cmds.iter().enumerate() {
        let words: Vec<&str> = c.args.iter().map(String::as_str).collect();
        if fails_at == Some(i) {
            fake.refuse(&c.program, &words, why);
        } else {
            fake.answer(&c.program, &words, "");
        }
    }

    let ejected = eject(&d);
    let ran: Vec<Cmd> = fake.calls().into_iter().filter(|c| c.program != "lsblk").collect();
    assert_eq!(ran, cmds[..fails_at.map_or(cmds.len(), |i| i + 1)]);
    match (ejected, fails_at) {
        (Ok(report), None) => {
            assert_eq!(report.commands, cmds.iter().map(Cmd::display).collect::<Vec<_>>());
            assert_eq!((report.root.as_path(), report.powered_off), (d.root(), true));
        }
        (Err(Error::External { command, message }), Some(i)) => {
            assert_eq!(command, cmds[i].display());
            let hint = " (close any window or program using the drive, then try again)";
            assert_eq!(message, format!("{why}{}", if why.contains("busy") { hint } else { "" }));
        }
        (other, _) => panic!("{other:?}"),
    }
}
