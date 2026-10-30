use super::*;

/// Regression: on Linux, `/media/<user>`, where udisks mounts drives, was a blank volume in
/// `/media`.
#[test]
fn mount_folders_are_not_volumes() {
    let dir = tempfile::tempdir().unwrap();
    let (media, user) = (dir.path().join("media"), dir.path().join("media/sam"));
    for v in [user.join("STICK"), media.join("usb0")] {
        std::fs::create_dir_all(v).unwrap();
    }
    let mut found = in_mount_folders(&[user.clone(), media.clone()]);
    found.sort();
    assert_eq!(found, [user.join("STICK"), media.join("usb0")]);
}

fn entry(fp: u8) -> LineageEntry {
    LineageEntry {
        fingerprint: Hash::of(&[fp]),
        at: Utc::now(),
        op: LineageOp::Observed,
        snapshot: None,
        source: None,
        tracks: 0,
    }
}

#[test]
fn lineage_dedupes_head_and_caps() {
    let mut s = DeviceState::new("x");
    s.record(entry(1));
    s.record(entry(1));
    assert_eq!(s.lineage.len(), 1);
    for i in 0..=255u8 {
        s.record(entry(i));
    }
    // 1 + 256 entries recorded; the oldest one is dropped.
    assert_eq!(s.lineage.len(), MAX_LINEAGE);
    assert_eq!(s.lineage[0].fingerprint, Hash::of(&[0]));
    assert_eq!(s.head().unwrap().fingerprint, Hash::of(&[255]));
}

#[test]
fn parses_rekordbox_playlist_sync() {
    let xml = "<?xml version=\"1.0\"?>\r\n<Sync DBID=\"-1940252023\" AutomaticSync=\"1\" AllPlaylists=\"0\">\
                   <Playlists><NODE Id=\"0\" ParentId=\"0\"/><NODE Id=\"B181727E\" ParentId=\"0\"/></Playlists></Sync>";
    let s = RekordboxSync::parse("onelibrary", xml).unwrap();
    assert_eq!(
        s,
        RekordboxSync { library: "onelibrary", automatic: true, collection: Some(2_354_715_273), playlists: 2 }
    );
    assert!(RekordboxSync::parse("onelibrary", "<Other/>").is_none());
}

/// What a run that didn't finish says, and what finishes it.
#[test]
fn an_unfinished_run_says_what_it_was_and_what_to_do() {
    let pending = |op, description: &str, backup: Option<&str>| PendingOp {
        op,
        started: Utc::now(),
        description: description.into(),
        backup: backup.map(String::from),
        player: None,
    };
    let pack = pending(LineageOp::Pack, "pack from Main", None);
    assert!(pack.describe().starts_with("a `locallibrary pack` (pack from Main) didn't finish (started "));
    assert_eq!(pack.remedy(), "pack it again to finish it");
    let repair = pending(LineageOp::Repair, "", Some("20261005-213000"));
    assert!(repair.describe().starts_with("a `locallibrary drive verify --repair` didn't finish"));
    assert_eq!(repair.remedy(), "`locallibrary undo` puts back the backup it made");
    let edit = pending(LineageOp::Edit, "", None);
    assert!(edit.describe().starts_with("a locallibrary operation didn't finish"));
    assert_eq!(edit.remedy(), "run it again, or make any other change to the drive, to finish it");
    assert_eq!(pending(LineageOp::Push, "", None).remedy(), "sync onto it again to finish it");
    for op in [
        LineageOp::Sync,
        LineageOp::Pull,
        LineageOp::Merge,
        LineageOp::Clean,
        LineageOp::Convert,
        LineageOp::Artwork,
        LineageOp::Restore,
    ] {
        assert!(pending(op, "", None).describe().starts_with("a `locallibrary "), "{op:?}");
    }
}

/// A drive that copied another's whole library takes over its history, capped like any other.
#[test]
fn inheriting_a_lineage_ends_with_the_new_state() {
    let source: Vec<LineageEntry> = (0..=255u8).map(entry).collect();
    let mut s = DeviceState::new("Backup");
    s.inherit_with(&source, LineageEntry { op: LineageOp::Sync, ..entry(255) });
    assert_eq!(s.lineage.len(), MAX_LINEAGE);
    assert_eq!(s.head().unwrap().op, LineageOp::Sync, "the source's head is replaced");
    s.inherit_with(&source, entry(7));
    assert_eq!((s.lineage.len(), s.lineage[0].fingerprint), (MAX_LINEAGE, Hash::of(&[1])));
    assert_eq!(s.head().unwrap().fingerprint, Hash::of(&[7]));
}

/// A formatted drive gets its identity back, but none of the states it held before.
#[test]
fn a_formatted_drive_gets_its_identity_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    assert!(matches!(Device::open_export(dir.path()), Err(Error::NotAnExport(_))));
    d.save_state().unwrap();
    assert!(!dir.path().join(paths::STATE_DIR).join("state.json").exists(), "nothing to save");
    let mut old = DeviceState::new("Main");
    old.record(entry(1));
    d.reset_identity(&old).unwrap();
    let d = Device::open(dir.path()).unwrap();
    assert_eq!((d.id(), d.name()), (Some(old.id), "Main".to_string()));
    assert!(d.state().unwrap().lineage.is_empty());
    assert!(format!("{d:?}").contains(&old.id.to_string()));
}

#[test]
fn adopt_persists_identity() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    assert!(!d.is_adopted());
    let id = d.adopt(Some("Backup A")).unwrap().id;
    let d2 = Device::open(dir.path()).unwrap();
    assert_eq!(d2.id(), Some(id));
    assert_eq!(d2.name(), "Backup A");
}

/// The lineage keeps each state once in a row and the newest `MAX_LINEAGE`, whether states are
/// recorded one by one or taken over from a drive copied onto this one.
#[hegel::test(test_cases = 100)]
fn the_lineage_keeps_the_newest_states_once_in_a_row(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut s = DeviceState::new("x");
    let mut model: Vec<u8> = Vec::new();
    let fps = |l: &[LineageEntry]| l.iter().map(|e| e.fingerprint).collect::<Vec<_>>();
    let keep = |m: &mut Vec<u8>| {
        if m.len() > MAX_LINEAGE {
            m.drain(..m.len() - MAX_LINEAGE);
        }
    };
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        match tc.draw(gs::booleans()) {
            true => {
                // Many states, some the same as the one before.
                for _ in 0..tc.draw(gs::integers::<usize>().max_value(300)) {
                    let fp = tc.draw(gs::integers::<u8>().max_value(3));
                    s.record(entry(fp));
                    if model.last() != Some(&fp) {
                        model.push(fp);
                    }
                    keep(&mut model);
                }
            }
            false => {
                let source: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(300));
                let head = tc.draw(gs::integers::<u8>());
                s.inherit_with(&source.iter().map(|f| entry(*f)).collect::<Vec<_>>(), entry(head));
                model = source;
                if model.last() == Some(&head) {
                    model.pop();
                }
                model.push(head);
                keep(&mut model);
            }
        }
        assert_eq!(fps(&s.lineage), model.iter().map(|f| Hash::of(&[*f])).collect::<Vec<_>>());
    }
}

/// An operation cut off leaves its mark: a copy of a whole library (sync, push, pack, export) or
/// putting a backup back takes it over, and so does anything after a write that made no backup
/// and copied nothing; anything else is refused, saying what was cut off and what finishes it.
#[hegel::test(test_cases = 200)]
fn an_operation_cut_off_is_finished_before_anything_else(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let ops = vec![
        LineageOp::Sync,
        LineageOp::Push,
        LineageOp::Pull,
        LineageOp::Pack,
        LineageOp::Export,
        LineageOp::Merge,
        LineageOp::Clean,
        LineageOp::Convert,
        LineageOp::Artwork,
        LineageOp::Restore,
        LineageOp::Repair,
        LineageOp::Edit,
    ];
    let first = tc.draw(gs::sampled_from(ops.clone()));
    let next = tc.draw(gs::sampled_from(ops));
    let backup = tc.draw(gs::booleans()).then(|| "20261011T120000Z".to_string());
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(dir.path()).unwrap();
    // The root as Debug shows a path (on Windows its backslashes are escaped).
    let root = format!("{:?}", dunce_canonicalize(dir.path()));
    assert!(format!("{d:?}").contains(root.trim_matches('"')), "{d:?}");
    d.begin(first, "the first", backup.clone()).unwrap();
    let copies = |op| matches!(op, LineageOp::Sync | LineageOp::Push | LineageOp::Pack | LineageOp::Export);
    let allowed = copies(next) || next == LineageOp::Restore || (backup.is_none() && !copies(first));
    match d.begin(next, "the next", None) {
        Ok(()) => {
            assert!(allowed, "{next:?} after {first:?}");
            assert_eq!(d.state().unwrap().pending.as_ref().map(|p| p.op), Some(next));
        }
        Err(e) => {
            assert!(!allowed, "{e}");
            assert_eq!(e.reason(), Some(Reason::Interrupted));
            let text = e.to_string();
            assert!(text.contains("(the first) didn't finish"), "{text}");
            let remedy = match (first, &backup) {
                (LineageOp::Sync | LineageOp::Push, _) => "sync onto it again",
                (LineageOp::Pack, _) => "pack it again",
                (LineageOp::Export, _) => "export the collection to it again",
                (_, Some(_)) => "`locallibrary undo`",
                _ => "run it again",
            };
            assert!(text.contains(remedy), "{text}");
        }
    }
    // One write at a time: the lock is held until it's let go, and a read waits for it too.
    let held = d.lock().unwrap();
    assert!(matches!(d.lock(), Err(Error::Locked(_))));
    assert!(matches!(d.read_lock(), Err(Error::Locked(_))));
    drop(held);
    assert!(d.read_lock().unwrap().is_some());
    let fresh = tempfile::tempdir().unwrap();
    assert!(Device::open(fresh.path()).unwrap().read_lock().unwrap().is_none(), "a drive never written has no lock");
}

/// A drive that takes over another's history holds the source's newest states then the new
/// one (which stands in for the source's head when it's the same state), never more than
/// `MAX_LINEAGE`, however long the source's history was.
#[hegel::test(test_cases = 40)]
#[hegel::explicit_test_case(len = MAX_LINEAGE + 40, same_head = true)]
#[hegel::explicit_test_case(len = MAX_LINEAGE, same_head = false)]
#[hegel::explicit_test_case(len = 0usize, same_head = false)]
fn a_drive_taken_over_keeps_the_newest_states(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let len = tc.draw(gs::integers::<usize>().max_value(2 * MAX_LINEAGE));
    let same_head = tc.draw(gs::booleans());
    let source: Vec<LineageEntry> = (0..len).map(|i| entry((i % 251) as u8)).collect();
    let head = match (same_head, source.last()) {
        (true, Some(last)) => LineageEntry { op: LineageOp::Sync, ..last.clone() },
        _ => entry(252),
    };
    let mut s = DeviceState::new("x");
    s.record(entry(253));
    s.inherit_with(&source, head.clone());
    let mut want: Vec<Hash> = source.iter().map(|e| e.fingerprint).collect();
    if same_head && !source.is_empty() {
        want.pop();
    }
    want.push(head.fingerprint);
    let keep = want.len().min(MAX_LINEAGE);
    assert_eq!(s.lineage.iter().map(|e| e.fingerprint).collect::<Vec<_>>(), want[want.len() - keep..]);
    assert_eq!(s.head().map(|h| h.op), Some(head.op));
}

/// Only a folder opens as a drive, and only one holding an export opens as an export; an identity
/// file that can't be read is an error, not a blank drive. A drive formatted and given its
/// identity back keeps its id, name and library but none of its states, nor an operation it was
/// in the middle of; with no identity, saving writes nothing.
#[hegel::test(test_cases = 20)]
#[hegel::explicit_test_case(states = 3u8, pending = true, export = false)]
#[hegel::explicit_test_case(states = 0u8, pending = false, export = true)]
fn a_formatted_drive_keeps_only_its_identity(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let states = tc.draw(gs::integers::<u8>().max_value(5));
    let pending = tc.draw(gs::booleans());
    let export = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("USB");
    std::fs::create_dir(&root).unwrap();
    if export {
        std::fs::create_dir_all(root.join("PIONEER/rekordbox")).unwrap();
        std::fs::write(root.join(paths::EXPORT_PDB), b"pdb").unwrap();
    }
    let mut d = Device::open(&root).unwrap();
    assert_eq!(Device::open_export(&root).is_ok(), export);
    if !export {
        assert!(matches!(Device::open_export(&root), Err(Error::NotAnExport(r)) if r == d.root()));
    }
    let file = dir.path().join("file");
    std::fs::write(&file, b"").unwrap();
    assert!(matches!(Device::open(&file), Err(Error::NotFound(_))));

    d.save_state().unwrap();
    assert!(!d.state_dir().exists(), "no identity, nothing saved");
    let mut old = DeviceState::new("Main");
    old.library_id = Some(Uuid::new_v4());
    for i in 0..states {
        old.record(entry(i));
    }
    if pending {
        old.pending = Some(PendingOp {
            op: LineageOp::Sync,
            started: Utc::now(),
            description: "sync".into(),
            backup: None,
            player: None,
        });
    }
    d.reset_identity(&old).unwrap();
    let again = Device::open(&root).unwrap();
    let s = again.state().unwrap();
    assert_eq!((s.id, s.name.as_str(), s.library_id), (old.id, "Main", old.library_id));
    assert!(s.lineage.is_empty() && s.pending.is_none() && s.alias.is_none());
    assert!(again.finished().is_ok());

    std::fs::write(again.state_dir().join("state.json"), b"{\"id\":").unwrap();
    assert!(matches!(Device::open(&root), Err(Error::Parse { .. })));
}

/// Under the test switch the machine's disks are hidden, and the drives are the folders in the
/// folders it names, each once however it's named; without it the disks are shown. Linux's mount
/// folders hold volumes, but aren't volumes themselves, even when one holds another.
#[hegel::test(test_cases = 30)]
#[hegel::explicit_test_case(names = vec!["STICK", "usb0"], user = true, twice = true)]
fn the_test_switch_shows_only_its_own_folders(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let names: Vec<&str> =
        tc.draw(gs::vecs(gs::sampled_from(vec!["STICK", "usb0", "Main", "B"])).max_size(3).unique(true));
    let user = tc.draw(gs::booleans());
    let twice = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let media = dunce_canonicalize(dir.path()).join("media");
    let sam = media.join("sam");
    std::fs::create_dir_all(&media).unwrap();
    if user {
        std::fs::create_dir_all(&sam).unwrap();
    }
    let mut want = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let v = if user && i % 2 == 0 { sam.join(n) } else { media.join(n) };
        std::fs::create_dir_all(&v).unwrap();
        std::fs::write(media.join(format!("{n}.txt")), b"not a volume").unwrap();
        want.push(v);
    }
    let mut folders = vec![media.clone()];
    if user {
        folders.insert(0, sam.clone());
    }
    let mut found = in_mount_folders(&folders);
    found.sort();
    let mut mounted = want.clone();
    mounted.sort();
    assert_eq!(found, mounted);

    assert!(disks_shown().is_ok());
    let mut switch = vec![media.clone()];
    if twice {
        switch.push(dir.path().join("media"));
    }
    crate::testing::volumes_on_this_thread(Some(switch));
    let hidden = disks_shown();
    let volumes = mounted_volumes();
    crate::testing::volumes_on_this_thread(None);
    assert!(matches!(hidden, Err(Error::Invalid(m)) if m.starts_with("LOCALLIBRARY_VOLUMES is set")));
    let mut subfolders: Vec<PathBuf> =
        names.iter().enumerate().filter(|(i, _)| !(user && i % 2 == 0)).map(|(_, n)| media.join(n)).collect();
    if user {
        subfolders.push(sam.clone());
    }
    subfolders.sort();
    assert_eq!(volumes, subfolders);
}

/// A lock file this user may not open is an error naming it, not a drive no one is writing.
#[cfg(unix)]
#[hegel::test(test_cases = 6)]
fn a_lock_that_cant_be_opened_is_an_error(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use std::os::unix::fs::PermissionsExt;
    let mode = tc.draw(gs::sampled_from(vec![0o000u32, 0o200]));
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open(dir.path()).unwrap();
    drop(d.lock().unwrap());
    let lock = d.state_dir().join("lock");
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(mode)).unwrap();
    // Root opens anything.
    let denied = std::fs::File::open(&lock).is_err();
    let read = d.read_lock();
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o644)).unwrap();
    match read {
        Err(Error::Io { path, source }) => {
            assert!(denied);
            assert_eq!((path, source.kind()), (lock, std::io::ErrorKind::PermissionDenied));
        }
        Ok(Some(_)) => assert!(!denied),
        Ok(None) => panic!("the lock file is there"),
        Err(e) => panic!("{e}"),
    }
    assert!(d.read_lock().unwrap().is_some());
}

/// A key from a database is a path on the drive only when it stays there: one that climbs out
/// is refused, and so is one that leads out through a symlink on the drive; any other, there or
/// not yet, is the drive's path for it.
#[cfg(unix)]
#[hegel::test(test_cases = 30)]
#[hegel::explicit_test_case(key = "Contents/Out/a.mp3")]
#[hegel::explicit_test_case(key = "Contents/../../etc/passwd")]
fn a_key_from_a_database_stays_on_the_drive(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let key = tc.draw(gs::sampled_from(vec![
        "Contents/a.mp3",
        "Contents/New/b.mp3",
        "Contents/Out/a.mp3",
        "Contents/Out",
        "Contents/../../etc/passwd",
        "/etc/passwd",
    ]));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("USB");
    let outside = dir.path().join("Elsewhere");
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("a.mp3"), b"not the drive's").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("Contents/Out")).unwrap();
    let d = Device::open(&root).unwrap();
    let escapes = key.contains("..") || key.starts_with('/');
    let through_link = key.starts_with("Contents/Out");
    match d.checked_path(key) {
        Ok(p) => {
            assert!(!escapes && !through_link, "{key}");
            assert_eq!(p, d.path(key));
            assert!(p.starts_with(d.root()));
        }
        Err(Error::Invalid(m)) => {
            assert!(escapes || through_link, "{key}: {m}");
            assert!(m.contains(if escapes { "unsafe" } else { "resolves outside" }), "{m}");
        }
        Err(e) => panic!("{e}"),
    }
}

/// A scan that finds no file changed leaves the hash cache as it was; one after a database
/// changes rewrites it.
#[test]
fn a_scan_rewrites_the_hash_cache_only_when_a_file_changed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let write = |key: &str, bytes: &[u8]| {
        let p = root.join(key);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        filetime::set_file_mtime(&p, filetime::FileTime::from_unix_time(1_600_000_000, 0)).unwrap();
    };
    write(paths::EXPORT_PDB, b"pdb");
    write("PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT", b"anlz");
    let mut d = Device::open(root).unwrap();
    d.adopt(Some("T")).unwrap();
    let index = d.state_dir().join("index.json");
    let written = || {
        let modified = std::fs::metadata(&index).unwrap().modified().unwrap();
        (modified, std::fs::read(&index).unwrap())
    };
    let first = d.scan_fingerprinted(None, &Ctx::silent()).unwrap().fingerprint;
    let before = written();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert_eq!(d.scan_fingerprinted(None, &Ctx::silent()).unwrap().fingerprint, first);
    assert!(written() == before, "rewrote the cache with nothing changed");
    write(paths::EXPORT_PDB, b"pdb, edited");
    assert_ne!(d.scan_fingerprinted(None, &Ctx::silent()).unwrap().fingerprint, first);
    assert!(written().1 != before.1, "kept the cache of the old database");
}
