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
