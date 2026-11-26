use std::path::{Path, PathBuf};

use loclib_equipment::{FsKind, MBR_MAX};

use super::*;
use crate::cmd::install_hint;
use crate::test_util::{TEST_IMAGE, name, sample_disk};

#[test]
fn refuses_system_internal_virtual_and_protected_disks() {
    let ok = sample_disk();
    assert_eq!(ok.problem(&[]), None);
    assert!(Disk { system: true, ..ok.clone() }.problem(&[]).unwrap().contains("operating system"));
    assert!(Disk { internal: true, ..ok.clone() }.problem(&[]).unwrap().contains("internal"));
    assert!(Disk { removable: false, ..ok.clone() }.problem(&[]).unwrap().contains("internal"));
    assert!(Disk { read_only: true, ..ok.clone() }.problem(&[]).unwrap().contains("write-protected"));
    let store = PathBuf::from("/Volumes/DNA/store");
    assert!(ok.problem(&[store]).unwrap().contains("/Volumes/DNA/store"));
    assert_eq!(ok.problem(&[PathBuf::from("/Volumes/DNA2/x")]), None);
}

#[test]
fn holding_finds_a_path_on_any_volume() {
    let mut disk = sample_disk();
    let mut store = disk.volumes[0].clone();
    store.mount = Some("/Volumes/STORE".into());
    disk.volumes.push(store);
    let paths = [PathBuf::from("/Users/x/store"), PathBuf::from("/Volumes/STORE/locallibrary")];
    assert_eq!(disk.holding(&paths), Some((Path::new("/Volumes/STORE"), &paths[1])));
    assert_eq!(disk.holding(&paths[..1]), None);
}

#[test]
fn identity_tracks_contents_not_mounts() {
    let a = sample_disk();
    let mut b = a.clone();
    b.volumes[0].mount = None;
    assert_eq!(a.identity(), b.identity());
    b.volumes[0].label = Some("OTHER".into());
    assert_ne!(a.identity(), b.identity());
    let c = Disk { size: a.size + 1, ..a.clone() };
    assert_ne!(a.identity(), c.identity());
}

#[test]
fn labels_per_filesystem() {
    assert_eq!(Filesystem::Fat32.label("Club drive"), "CLUB DRIVE");
    assert_eq!(Filesystem::Exfat.label("Club drive: Tōkyō"), "Club drive_");
    assert_eq!(Filesystem::Hfsplus.label("Back:up"), "Back_up");
    assert_eq!(Filesystem::Exfat.label("  "), "REKORDBOX");
}

#[test]
fn cluster_size_rules() {
    assert!(Filesystem::Fat32.check_cluster_size(32768, 512).is_ok());
    assert!(Filesystem::Fat32.check_cluster_size(65536, 512).is_err());
    assert!(Filesystem::Exfat.check_cluster_size(131072, 512).is_ok());
    assert!(Filesystem::Exfat.check_cluster_size(1000, 512).is_err());
    assert!(Filesystem::Hfsplus.check_cluster_size(8192, 512).is_err());
    assert_eq!(Filesystem::Exfat.default_cluster_size(64_000_000_000), 131072);
    assert_eq!(Filesystem::Exfat.default_cluster_size(16_000_000_000), 32768);
}

#[test]
fn commands_render_like_a_shell() {
    let c = Cmd::new("diskutil", &["eraseDisk", "MS-DOS FAT32", "DNA", "MBR", "disk4"]);
    assert_eq!(c.display(), r#"diskutil eraseDisk "MS-DOS FAT32" DNA MBR disk4"#);
}

#[test]
fn lenient_json() {
    let v: serde_json::Value = serde_json::json!({"a": "1", "b": true, "c": "12", "d": 7, "e": {"value": [1, 2], "Count": 2}, "f": {"x": 1}, "g": "\u{0}"});
    assert!(json::bool(&v, "a") && json::bool(&v, "b") && !json::bool(&v, "zz"));
    assert_eq!(json::u64(&v, "c"), Some(12));
    assert_eq!(json::u64(&v, "d"), Some(7));
    assert_eq!(json::array(v.get("e")).len(), 2);
    assert_eq!(json::array(v.get("f")).len(), 1);
    assert_eq!(json::str(&v, "g"), None);
}

/// What the tools put where a string or a count belongs: numbers as text, flags as 0 and 1.
#[test]
fn lenient_json_takes_numbers_and_flags_in_any_form() {
    let v = serde_json::json!({"n": 3, "flag": 1, "off": 0, "t": "True", "yes": true, "list": [1]});
    assert_eq!(json::str(&v, "n").as_deref(), Some("3"));
    assert_eq!((json::str(&v, "yes"), json::u64(&v, "yes"), json::u64(&v, "list")), (None, None, None));
    assert!(json::bool(&v, "flag") && json::bool(&v, "t") && !json::bool(&v, "off") && !json::bool(&v, "list"));
    assert!(json::array(v.get("n")).is_empty());
}

#[test]
fn file_systems_are_named_and_small_volumes_get_small_clusters() {
    let names: Vec<&str> = [Filesystem::Fat32, Filesystem::Exfat, Filesystem::Hfsplus].map(Filesystem::name).into();
    assert_eq!(names, ["FAT32", "exFAT", "HFS+"]);
    assert_eq!(Filesystem::Exfat.default_cluster_size(256 << 20), 4096);
    assert_eq!(Filesystem::Hfsplus.default_cluster_size(1 << 40), 4096);
    assert!(Filesystem::Hfsplus.check_cluster_size(4096, 512).is_ok());
    assert_eq!(Filesystem::Hfsplus.label(":::"), "___");
    assert_eq!(Filesystem::Hfsplus.label(""), "REKORDBOX");
}

/// A disk image is refused, and so is a disk holding `/` whatever it says about itself.
#[test]
fn refuses_disk_images_and_the_root_volume() {
    let image = Disk { virtual_disk: true, ..sample_disk() };
    assert_eq!(image.problem(&[]).as_deref(), Some("it's a disk image or virtual disk"));
    let mut root = sample_disk();
    root.volumes[0].mount = Some("/".into());
    assert_eq!(root.problem(&[]).as_deref(), Some("it holds the running operating system"));
}

/// The disk image a test attached is let through, and only that one: a disk image under another
/// identifier is refused, and so is a disk that isn't an image under the image's identifier.
#[test]
fn only_the_tests_own_image_is_let_through() {
    let image = Disk { virtual_disk: true, internal: true, removable: false, ..sample_disk() };
    TEST_IMAGE.set(Some(image.id.clone()));
    assert_eq!(image.problem(&[]), None);
    let other = Disk { id: "disk9".into(), ..image.clone() };
    assert_eq!(
        other.problem(&[]).as_deref(),
        Some("it's an internal disk (only external and removable drives can be formatted)")
    );
    let real = Disk { virtual_disk: false, ..image.clone() };
    assert!(real.problem(&[]).unwrap().contains("internal"));
    // Read-only, holding the system or a protected path: refused all the same.
    assert_eq!(Disk { read_only: true, ..image.clone() }.problem(&[]).as_deref(), Some("it's write-protected"));
    assert!(Disk { system: true, ..image.clone() }.problem(&[]).unwrap().contains("operating system"));
    assert!(image.problem(&[PathBuf::from("/Volumes/DNA/store")]).unwrap().contains("holds"));
    TEST_IMAGE.set(None);
    assert!(image.problem(&[]).unwrap().contains("internal"));
}

/// A drive's name isn't taken for a disk identifier on any system, so nothing is asked of the
/// system's tools.
#[test]
fn a_drive_name_is_not_a_disk() {
    assert!(by_id("Backup").unwrap().is_none());
}

#[test]
fn missing_tools_say_what_to_install() {
    let err = Cmd::new("locallibrary-no-such-tool", &[]).run().unwrap_err();
    assert!(matches!(&err, Error::External { message, .. } if message == "not found"), "{err}");
    assert_eq!(install_hint("mkfs.exfat"), " (install exfatprogs)");
    assert_eq!(install_hint("lsblk"), " (install util-linux)");
    assert_eq!(install_hint("diskutil"), "");
}

/// A tool's own message is the error when it fails, and its input reaches it on stdin.
#[cfg(unix)]
#[test]
fn tools_run_without_a_shell() {
    let failing = Cmd::new("sh", &["-c", "echo 'Unable to unmount disk4' >&2; exit 1"]);
    match failing.run() {
        Err(Error::External { command, message }) => {
            assert_eq!(command, r#"sh -c "echo 'Unable to unmount disk4' >&2; exit 1""#);
            assert_eq!(message, "Unable to unmount disk4");
        }
        other => panic!("{other:?}"),
    }
    let silent = Cmd::new("sh", &["-c", "exit 2"]).run().unwrap_err();
    assert!(matches!(silent, Error::External { message, .. } if message == "no output"));
    assert_eq!(Cmd::new("cat", &[]).input("label: dos\n").run().unwrap(), "label: dos\n");
}

#[test]
fn write_access_to_a_device_is_checked() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("rdisk4");
    std::fs::write(&file, b"").unwrap();
    assert!(require_write_access(&file.to_string_lossy(), "run it as an administrator").is_ok());
    let gone = dir.path().join("rdisk5");
    let err = require_write_access(&gone.to_string_lossy(), "run it as an administrator").unwrap_err();
    assert!(matches!(err, Error::Io { .. }), "{err}");
}

// -----------------------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------------------

use hegel::generators as gs;
use hegel::{Generator, TestCase};

const FILESYSTEMS: [Filesystem; 3] = [Filesystem::Fat32, Filesystem::Exfat, Filesystem::Hfsplus];

/// Every label is one its file system takes: never empty, without the characters it refuses,
/// within its length (FAT32: 11 bytes of upper case ASCII; exFAT: 11 UTF-16 units; HFS+: 27
/// characters), and labelling a label changes nothing.
#[hegel::test(test_cases = 500)]
fn prop_labels_suit_their_file_system(tc: TestCase) {
    let name = name(&tc);
    for fs in FILESYSTEMS {
        let l = fs.label(&name);
        assert!(!l.is_empty() && l.trim() == l, "{fs:?} {name:?} -> {l:?}");
        assert!(!l.contains(char::is_control), "{fs:?} {l:?}");
        match fs {
            Filesystem::Fat32 => {
                assert!(l.len() <= 11 && l.bytes().all(|b| b.is_ascii_graphic() || b == b' '), "{l:?}");
                assert!(!l.contains(|c: char| c.is_ascii_lowercase() || "\"*+,./:;<=>?[\\]|".contains(c)));
            }
            Filesystem::Exfat => {
                assert!(l.encode_utf16().count() <= 11, "{name:?} -> {l:?} is too long for exFAT");
                assert!(!l.contains(|c| "\"*/:<>?\\|".contains(c)), "{l:?}");
            }
            Filesystem::Hfsplus => assert!(l.chars().count() <= 27 && !l.contains(':'), "{l:?}"),
        }
        assert_eq!(fs.label(&l), l, "{fs:?} relabels {l:?}");
    }
}

/// A cluster size is taken exactly when it is a power of two, at least a sector, and within
/// the file system's limit, and the default one is always taken.
#[hegel::test(test_cases = 500)]
fn prop_cluster_sizes_follow_each_file_systems_rules(tc: TestCase) {
    let fs = tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()));
    let sector = tc.draw(gs::sampled_from(vec![512u32, 1024, 2048, 4096]));
    let size = tc.draw(hegel::one_of!(gs::integers::<u32>().max_value(31).map(|s| 1u32 << s), gs::integers::<u32>(),));
    let limit = match fs {
        Filesystem::Fat32 => 32 << 10,
        Filesystem::Exfat => 32 << 20,
        Filesystem::Hfsplus => 4096,
    };
    let valid =
        size.is_power_of_two() && size >= sector && size <= limit && (fs != Filesystem::Hfsplus || size == 4096);
    assert_eq!(fs.check_cluster_size(size, sector).is_ok(), valid, "{fs:?} {size} on {sector}-byte sectors");
    let volume = tc.draw(gs::integers::<u64>().max_value(MBR_MAX));
    let default = fs.default_cluster_size(volume);
    assert!(fs.check_cluster_size(default, sector).is_ok(), "{fs:?} default {default} for {volume}");
    assert!(FsKind::from(fs) != FsKind::Other && !fs.name().is_empty());
}

/// Splits a [`Cmd::display`] rendering back into words, undoing the quoting of `{:?}`.
fn split_display(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = s.chars().peekable();
    while chars.peek().is_some() {
        let mut w = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next().unwrap() {
                        'n' => w.push('\n'),
                        'r' => w.push('\r'),
                        't' => w.push('\t'),
                        '0' => w.push('\0'),
                        'u' => {
                            let hex: String = chars.by_ref().skip(1).take_while(|&c| c != '}').collect();
                            w.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                        }
                        other => w.push(other),
                    },
                    c => w.push(c),
                }
            }
        } else {
            w.extend(chars.by_ref().take_while(|&c| c != ' '));
            words.push(w);
            continue;
        }
        words.push(w);
        chars.next(); // the space after a quoted word
    }
    words
}

/// A command's shell-style rendering is unambiguous: it splits back into the program and
/// exactly its arguments, and says when a script goes to its input.
#[hegel::test(test_cases = 500)]
fn prop_commands_display_unambiguously(tc: TestCase) {
    let program = tc.draw(gs::sampled_from(vec!["diskutil", "mkfs.vfat", "sfdisk"]));
    let args: Vec<String> = tc.draw(
        gs::vecs(hegel::one_of!(gs::text().max_size(12), gs::text().alphabet("ab $'\"\\\t\n").max_size(6),))
            .max_size(5),
    );
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut cmd = Cmd::new(program, &arg_refs);
    let script = tc.draw(gs::booleans());
    if script {
        cmd = cmd.input("label: dos\n");
    }
    let shown = cmd.display();
    let shown = shown.strip_suffix(" <<< (script)").filter(|_| script).unwrap_or(&shown);
    let mut words = split_display(shown);
    assert_eq!(words.remove(0), program);
    assert_eq!(words, args, "{shown}");
}

/// A tool gets its input on stdin and its output is returned; when it fails, its own message
/// (standard error first, else its output) is the error, with the command line that ran. A
/// missing tool says which package has it, for the ones Linux distributions leave out.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn prop_tools_get_their_input_and_report_their_failures(tc: TestCase) {
    let input = tc.draw(gs::text().max_size(40).exclude_characters("\0"));
    assert_eq!(Cmd::new("cat", &[]).input(input.clone()).run().unwrap(), input);
    let tool = tc.draw(gs::sampled_from(vec![
        "mkfs.vfat",
        "mkfs.exfat",
        "mkfs.hfsplus",
        "sfdisk",
        "wipefs",
        "lsblk",
        "diskutil",
    ]));
    assert_eq!(install_hint(tool).is_empty(), tool == "diskutil");
    let missing = Cmd::new(&format!("locallibrary-missing-{tool}"), &[]).run().unwrap_err();
    assert!(matches!(&missing, Error::External { message, .. } if message == "not found"), "{missing}");
    let (err, out) = (tc.draw(gs::text().max_size(20)), tc.draw(gs::text().max_size(20)));
    let (err, out) = (err.replace('\0', ""), out.replace('\0', ""));
    let script = "printf %s \"$1\" >&2; printf %s \"$2\"; exit 3";
    let cmd = Cmd::new("sh", &["-c", script, "sh", &err, &out]);
    match cmd.run() {
        Err(Error::External { command, message }) => {
            assert_eq!(command, cmd.display());
            let want = [err.trim(), out.trim()].into_iter().find(|s| !s.is_empty()).unwrap_or("no output");
            assert_eq!(message, want);
        }
        other => panic!("{other:?}"),
    }
}

/// Lenient JSON reads a value in every form the tools write it in.
#[hegel::test(test_cases = 300)]
fn prop_lenient_json_reads_every_spelling(tc: TestCase) {
    use serde_json::{Value, json};
    let n = tc.draw(gs::integers::<u64>());
    let pad = tc.draw(gs::sampled_from(vec!["", " ", "\t", " \n"]));
    let v = json!({ "num": n, "text": format!("{pad}{n}{pad}") });
    assert_eq!((json::u64(&v, "num"), json::u64(&v, "text")), (Some(n), Some(n)));
    assert_eq!(json::str(&v, "num"), Some(n.to_string()));
    assert_eq!(json::str(&v, "text"), Some(n.to_string()));

    let s = tc.draw(gs::text().max_size(10));
    let v = json!({ "s": s });
    let trimmed = s.trim();
    let want = (!trimmed.is_empty() && trimmed != "\0").then(|| trimmed.to_string());
    assert_eq!(json::str(&v, "s"), want);
    assert_eq!(json::str(&v, "missing"), None);

    let flag = tc.draw(gs::sampled_from(vec![
        (json!(true), true),
        (json!(false), false),
        (json!("1"), true),
        (json!(" true"), true),
        (json!("True "), true),
        (json!("yes"), false),
        (json!("0"), false),
        (json!(1), true),
        (json!(2), false),
        (json!(null), false),
        (json!([true]), false),
    ]));
    assert_eq!(json::bool(&json!({ "f": flag.0 }), "f"), flag.1, "{}", flag.0);

    let items: Vec<u64> = tc.draw(gs::vecs(gs::integers::<u64>()).max_size(4));
    let items: Vec<Value> = items.into_iter().map(|i| json!({ "n": i })).collect();
    let array = Value::Array(items.clone());
    let wrapped = json!({ "value": items.clone(), "Count": items.len() });
    assert_eq!(json::array(Some(&array)), items.iter().collect::<Vec<_>>());
    assert_eq!(json::array(Some(&wrapped)), items.iter().collect::<Vec<_>>());
    if let Some(one) = items.first() {
        assert_eq!(json::array(Some(one)), [one]);
    }
    assert!(json::array(Some(&json!(n))).is_empty() && json::array(None).is_empty());
}

/// Formatting a device needs write access to it: a writable one passes, a read-only one asks
/// for administrator rights naming the device and how to get them, and a missing one is an
/// I/O error.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn prop_write_access_is_checked_before_formatting(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let dev = dir.path().join(format!("rdisk{}", tc.draw(gs::integers::<u8>())));
    let how = tc.draw(gs::sampled_from(vec!["run it as an administrator", "use sudo"]));
    let state = tc.draw(gs::integers::<u8>().max_value(2));
    if state < 2 {
        std::fs::write(&dev, b"").unwrap();
        std::fs::set_permissions(&dev, std::fs::Permissions::from_mode(if state == 0 { 0o600 } else { 0o400 }))
            .unwrap();
    }
    let got = require_write_access(&dev.to_string_lossy(), how);
    match state {
        0 => got.unwrap(),
        // Root may write anyway.
        1 if got.is_ok() => {}
        1 => {
            let e = got.unwrap_err();
            assert!(
                matches!(&e, Error::Permission(m) if m.contains(&*dev.to_string_lossy()) && m.ends_with(how)),
                "{e}"
            );
        }
        _ => assert!(matches!(got, Err(Error::Io { .. })), "{got:?}"),
    }
}

/// A name a person gives a drive is never taken for a disk identifier: looking one up finds no
/// disk, on any system, without asking its tools. Identifiers are `disk<N>` (macOS),
/// `/dev/...` (Linux) and `PhysicalDrive<N>` (Windows), in any of their spellings.
#[hegel::test(test_cases = 300)]
#[hegel::explicit_test_case(spec = "Backup".to_string())]
#[hegel::explicit_test_case(spec = "Disco 1".to_string())]
fn prop_drive_names_are_not_disks(tc: TestCase) {
    let spec =
        tc.draw(hegel::one_of!(gs::text().max_size(30), gs::text().alphabet("rdisk0 /ev\\.PhysicalDrv").max_size(16)));
    let lower = spec.to_ascii_lowercase();
    let shaped_like_an_id = lower.contains("disk") || spec.starts_with("/dev/") || lower.contains("physicaldrive");
    tc.assume(!shaped_like_an_id);
    assert!(by_id(&spec).unwrap().is_none(), "{spec:?}");
}

/// A tool that can't be started says why: one that isn't there is "not found", and one that is
/// there but can't be run gives the system's reason. Either way the error names the program,
/// whatever its arguments, and nothing ran.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn prop_tools_that_cant_start_say_why(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(tc.draw(gs::sampled_from(vec!["mkfs.vfat", "diskutil", "sfdisk", "a tool"])));
    let args: Vec<String> = tc.draw(gs::vecs(gs::text().max_size(8).exclude_characters("\0")).max_size(3));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let installed = tc.draw(gs::booleans());
    if installed {
        // There, but not executable.
        std::fs::write(&file, "#!/bin/sh\necho ran\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let program = file.to_string_lossy().into_owned();
    match Cmd::new(&program, &args).run() {
        Err(Error::External { command, message }) => {
            assert_eq!(command, program, "the program, not the whole command line");
            if installed {
                // EACCES: exec refuses a file without its execute bit, even to root.
                assert_eq!(message, std::io::Error::from_raw_os_error(13).to_string());
            } else {
                assert_eq!(message, "not found");
            }
        }
        other => panic!("{other:?}"),
    }
}

/// A device that isn't there any more (unplugged since the plan, or a path that never was one)
/// is an I/O error naming it, never taken for a permission problem.
#[hegel::test(test_cases = 20)]
fn prop_write_access_to_a_device_that_is_gone_is_an_io_error(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let depth = tc.draw(gs::integers::<usize>().max_value(2));
    let mut dev = dir.path().to_path_buf();
    for i in 0..=depth {
        dev.push(format!("rdisk{i}"));
    }
    let device = dev.to_string_lossy().into_owned();
    match require_write_access(&device, "run it as an administrator") {
        Err(Error::Io { path, .. }) => assert_eq!(path, dev),
        other => panic!("{other:?}"),
    }
}

/// A disk unplugged since it was listed (its identifier names nothing now, its device is gone)
/// has no link speed, and formatting it fails on its device with an I/O error, as any file system
/// and cluster size would, before any tool runs.
#[hegel::test(test_cases = 10)]
fn prop_a_disk_that_is_gone_has_no_speed_and_cant_be_formatted(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let n = tc.draw(gs::integers::<u16>());
    let disk = Disk {
        id: format!("gone{n}"),
        device: dir.path().join(format!("gone{n}")).display().to_string(),
        bus: tc.draw(gs::sampled_from(vec![None, Some("USB".to_string())])),
        ..sample_disk()
    };
    assert_eq!(link_speed(&disk), None);
    let filesystem = tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()));
    let t = Target { filesystem, label: name(&tc), cluster_size: 32768, default_cluster: false };
    // macOS opens the raw device for a cluster size of its own; Linux and Windows the disk's.
    let device = if cfg!(target_os = "macos") { format!("/dev/r{}", disk.id) } else { disk.device.clone() };
    match format(&disk, &t, &Ctx::silent()) {
        Err(Error::Io { path, .. }) => assert_eq!(path, Path::new(&device)),
        other => panic!("{other:?}"),
    }
}
