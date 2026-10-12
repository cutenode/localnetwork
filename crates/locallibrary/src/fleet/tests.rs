use super::*;

fn snap(fp: u8, id: &str) -> SnapshotInfo {
    SnapshotInfo {
        id: Hash::of(id.as_bytes()),
        short_id: id.into(),
        library_id: Uuid::nil(),
        parents: vec![],
        created: Utc::now(),
        message: None,
        source: None,
        fingerprint: Hash::of(&[fp]),
        summary: Default::default(),
        files: 0,
        bytes: 0,
        labels: Vec::new(),
        beside: false,
    }
}

#[test]
fn standing_counts_newer_snapshots() {
    let history = vec![snap(3, "c"), snap(2, "b"), snap(1, "a")];
    assert!(matches!(standing(&history, &Hash::of(&[3])), Standing::UpToDate));
    assert!(matches!(standing(&history, &Hash::of(&[1])), Standing::Behind { snapshots: 2, .. }));
    assert!(matches!(standing(&history, &Hash::of(&[9])), Standing::Unsaved));
}

#[test]
fn roles_parse_by_name() {
    assert_eq!(Role::parse("Archive").unwrap(), Role::Archive);
    assert!(Role::parse("spare").is_err());
    // `gig` is the old name of `set`: read in requests and the registry, written as `set`.
    assert_eq!(Role::parse("GIG").unwrap(), Role::Set);
    assert_eq!(serde_json::from_str::<Role>("\"gig\"").unwrap(), Role::Set);
    assert_eq!(serde_json::to_string(&Role::Set).unwrap(), "\"set\"");
}

#[test]
fn windows_labels_the_drive_letter_with_set_volume() {
    let cmd = windows_label_command(Path::new(r"E:\"), &volume_label("DJ's USB")).unwrap();
    assert_eq!(cmd.args.last().unwrap(), "Set-Volume -DriveLetter E -NewFileSystemLabel 'DJ''S USB'");
    assert!(windows_label_command(Path::new(r"E:\Music"), "X").is_err(), "only a drive's root");
}

/// Under the test switch the volume holding a drive folder is this machine's own: its label
/// stays, and the drive keeps its name too.
#[test]
fn the_test_switch_leaves_volume_labels_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    d.adopt(Some("Main")).unwrap();
    crate::testing::volumes_on_this_thread(Some(Vec::new()));
    let renamed = rename(None, &mut d, "Club", true);
    // A path that isn't there: were the switch ignored, no tool could find a volume for it.
    let labelled = set_volume_label(&dir.path().join("gone"), "Club");
    crate::testing::volumes_on_this_thread(None);
    for refused in [renamed.map(|r| r.name), labelled] {
        assert!(
            matches!(&refused, Err(Error::Invalid(m)) if m.starts_with("LOCALLIBRARY_VOLUMES is set")),
            "{refused:?}"
        );
    }
    assert_eq!(Device::open(dir.path()).unwrap().name(), "Main");
}

#[test]
fn same_disk_needs_a_serial() {
    let a = Hardware { model: "Cruzer".into(), serial: None, volume_uuid: Some("A".into()), size: 64 };
    assert!(!a.same_disk(&a.clone()));
    let b = Hardware { serial: Some("4C530001".into()), ..a.clone() };
    assert!(b.same_disk(&b.clone()));
    assert!(!b.same_disk(&Hardware { size: 32, ..b.clone() }));
    assert!(a.same_volume(&Hardware { volume_uuid: Some("a".into()), ..a.clone() }));
    assert!(!plausible_serial("0000000000"));
    assert!(plausible_serial("4C530001123456789012"));
}

/// A serial number tells a disk apart unless it's one many controllers report for every unit:
/// too short, only zeros and Fs, or the counting placeholder; spacing around it doesn't matter.
#[hegel::test(test_cases = 300)]
fn placeholder_serials_tell_no_disk_apart(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let placeholder = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => "0".repeat(tc.draw(gs::integers::<usize>().max_value(20))),
        1 => tc.draw(gs::text().alphabet("0Ff").max_size(20)),
        _ => tc.draw(gs::sampled_from(vec!["0123456789ABCDEF", "0123456789abcdef", "abc", ""])).to_string(),
    };
    assert!(!plausible_serial(&placeholder), "{placeholder:?}");
    let real = format!(
        "{}{}",
        tc.draw(gs::text().alphabet("0F").max_size(4)),
        tc.draw(gs::text().alphabet("ABCDEXYZ123456789").min_size(4).max_size(12))
    );
    let spaced = format!(
        "{}{real}{}",
        " ".repeat(tc.draw(gs::integers::<usize>().max_value(2))),
        " ".repeat(tc.draw(gs::integers::<usize>().max_value(2)))
    );
    let all_placeholder =
        real.chars().all(|c| matches!(c, '0' | 'F' | 'f')) || real.eq_ignore_ascii_case("0123456789ABCDEF");
    assert_eq!(plausible_serial(&spaced), !all_placeholder, "{spaced:?}");
}

/// On Windows a drive's label is set with `Set-Volume` for its letter, however its root is
/// written, with the label a name makes (11 characters at most, upper case) quoted for
/// PowerShell. A path below a drive's root names no volume.
#[hegel::test(test_cases = 200)]
fn windows_labels_a_drive_by_its_letter(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let letter = tc.draw(gs::sampled_from(('a'..='z').chain('A'..='Z').collect::<Vec<char>>()));
    let sep = tc.draw(gs::sampled_from(vec!["", "\\", "/", "\\\\"]));
    let prefix = tc.draw(gs::sampled_from(vec!["", r"\\?\"]));
    let name = tc.draw(gs::text().max_size(20));
    let label = volume_label(&name);
    assert!(!label.is_empty() && label.len() <= 11, "{label:?}");
    assert_eq!(label, label.to_ascii_uppercase());
    let root = format!("{prefix}{letter}:{sep}");
    let cmd = windows_label_command(Path::new(&root), &label).unwrap();
    let quoted = format!("'{}'", label.replace('\'', "''"));
    let want = format!("Set-Volume -DriveLetter {} -NewFileSystemLabel {quoted}", letter.to_ascii_uppercase());
    assert_eq!(cmd.args.last(), Some(&want));
    assert_eq!(cmd.args[..2], ["-NoProfile", "-Command"]);
    let below = format!("{letter}:\\{}", tc.draw(gs::sampled_from(vec!["Music", "PIONEER\\rekordbox"])));
    assert!(windows_label_command(Path::new(&below), &label).is_err(), "{below}");
}

/// Under the test switch a drive's volume is this machine's own: setting its label is refused
/// before anything changes, and so is a rename that would set it. Renamed without its label, the
/// drive takes the name trimmed; an empty name is refused either way.
#[hegel::test(test_cases = 30)]
fn under_the_test_switch_only_a_drives_name_changes(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    d.adopt(Some("Main")).unwrap();
    let name = tc.draw(gs::sampled_from(vec!["Club", "  Booth 2 ", "Ünï", "", "   "]));
    crate::testing::volumes_on_this_thread(Some(Vec::new()));
    let labelled = rename(None, &mut d, name, true).map(|r| r.name);
    let label = set_volume_label(dir.path(), name);
    let renamed = rename(None, &mut d, name, false);
    crate::testing::volumes_on_this_thread(None);
    let empty = name.trim().is_empty();
    let switch =
        |r: &Result<String>| matches!(r, Err(Error::Invalid(m)) if m.starts_with("LOCALLIBRARY_VOLUMES is set"));
    let no_name = |r: &Result<String>| matches!(r, Err(Error::Invalid(m)) if m == "the name is empty");
    assert!(if empty { no_name(&labelled) } else { switch(&labelled) }, "{labelled:?}");
    assert!(switch(&label), "{label:?}");
    match renamed {
        Ok(r) => {
            assert!(!empty);
            assert_eq!((r.old_name.as_str(), r.name.as_str(), r.label), ("Main", name.trim(), None));
            assert_eq!(Device::open(dir.path()).unwrap().name(), name.trim());
        }
        Err(e) => assert!(empty && no_name(&Err(e)), "{name:?}"),
    }
}
