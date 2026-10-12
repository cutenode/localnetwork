use super::*;

pub(crate) fn sample_disk() -> Disk {
    Disk {
        id: "disk4".into(),
        device: "/dev/disk4".into(),
        model: "DJ TechTools".into(),
        serial: None,
        size: 128_000_000_000,
        sector_size: 512,
        bus: Some("USB".into()),
        removable: true,
        internal: false,
        system: false,
        virtual_disk: false,
        read_only: false,
        scheme: Some(Scheme::Mbr),
        volumes: vec![Volume {
            id: "disk4s1".into(),
            size: 127_999_000_000,
            filesystem: Some(FsKind::Fat32),
            filesystem_name: Some("MS-DOS FAT32".into()),
            label: Some("DNA".into()),
            mount: Some("/Volumes/DNA".into()),
            cluster_size: Some(32768),
            uuid: Some("12345678".into()),
        }],
    }
}

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

/// Regression: the disk was compared with the plan once, before planning again and the
/// capacity test, and erased by its identifier without looking again.
#[test]
fn the_disk_is_checked_again_just_before_it_is_erased() {
    let planned = sample_disk();
    assert!(still_the_disk(&planned, Some(&planned.clone()), true).is_ok());
    let mut relabelled = planned.clone();
    relabelled.volumes[0].label = Some("OTHER".into());
    assert!(matches!(still_the_disk(&planned, Some(&relabelled), true), Err(Error::StalePlan(_))));
    // After the capacity test, which rewrote what was on it, only the disk itself counts.
    relabelled.volumes.clear();
    assert!(still_the_disk(&planned, Some(&relabelled), false).is_ok());
    let swapped = Disk { serial: Some("4C530002".into()), ..planned.clone() };
    assert!(still_the_disk(&planned, Some(&swapped), false).is_err());
    assert!(still_the_disk(&planned, None, false).is_err(), "gone");
}

/// Regression: the CLI asked for a volume label when the disk held no adopted drive, the very
/// word that may have found an unrelated disk.
#[test]
fn erasing_is_confirmed_by_the_drives_name_or_the_disks_identifier() {
    let disk = sample_disk();
    let plan = FormatPlan {
        identity: disk.identity(),
        filesystem: Filesystem::Fat32,
        scheme: Scheme::Mbr,
        label: "DNA".into(),
        cluster_size: 32768,
        partition_size: disk.partition_size(),
        erases: Vec::new(),
        keeps_name: None,
        compatibility: players::assess_disk(&disk),
        steps: Vec::new(),
        warnings: Vec::new(),
        protected: Vec::new(),
        disk,
    };
    assert_eq!(plan.disk.volumes[0].label.as_deref(), Some("DNA"));
    assert_eq!(plan.confirmation(), "disk4", "not the label");
    let named = FormatPlan { keeps_name: Some("Backup".into()), ..plan };
    assert_eq!(named.confirmation(), "Backup");
}

/// The plan says formatting hasn't run on Linux or Windows, so the sidecar and the terminal
/// player show it too (only the CLI used to).
#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "a plan on Linux asks lsblk what's mounted on the disk, and the sample disk isn't there"
)]
fn plans_warn_where_formatting_is_untested() {
    let warnings = |os, check_capacity| {
        THREAD_OS.with(|o| o.set(os));
        let opts = FormatOptions { check_capacity, ..Default::default() };
        let w = plan(sample_disk(), &opts, None, &[], &Ctx::silent()).map(|p| p.warnings);
        THREAD_OS.with(|o| o.set(None));
        w.unwrap()
    };
    let untested = |w: Vec<String>| w.into_iter().filter(|w| w.contains("tested")).collect::<Vec<_>>();
    assert_eq!(
        untested(warnings(Some("linux"), false)),
        ["formatting hasn't been tested on Linux yet; check the result"]
    );
    assert_eq!(
        untested(warnings(Some("windows"), true)),
        ["formatting with a capacity check hasn't been tested on Windows yet; check the result"]
    );
    assert!(untested(warnings(Some("macos"), false)).is_empty());
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

/// A disk image is refused unless tests allow them, and so is a disk holding `/` whatever it
/// says about itself.
#[test]
fn refuses_disk_images_and_the_root_volume() {
    let image = Disk { virtual_disk: true, ..sample_disk() };
    assert_eq!(image.problem(&[]).as_deref(), Some("it's a disk image or virtual disk"));
    let mut root = sample_disk();
    root.volumes[0].mount = Some("/".into());
    assert_eq!(root.problem(&[]).as_deref(), Some("it holds the running operating system"));
}

/// Only external and removable physical disks are listed, each with why it can't be
/// formatted when it can't.
#[test]
fn lists_only_external_and_removable_disks() {
    let disks = vec![
        sample_disk(),
        Disk { id: "disk0".into(), internal: true, removable: false, ..sample_disk() },
        Disk { id: "disk1".into(), removable: false, ..sample_disk() },
        Disk { id: "disk9".into(), virtual_disk: true, ..sample_disk() },
        Disk { id: "disk5".into(), read_only: true, ..sample_disk() },
    ];
    let views = views(disks, &[PathBuf::from("/Volumes/DNA/store")]);
    let ids: Vec<&str> = views.iter().map(|v| v.disk.id.as_str()).collect();
    assert_eq!(ids, ["disk4", "disk5"]);
    assert_eq!(views[0].identity, views[0].disk.identity());
    assert_eq!(views[0].compatibility.verdict, Verdict::AllPlayers);
    assert_eq!(views[0].problem.as_deref(), Some("it holds /Volumes/DNA/store"));
    assert_eq!(views[1].problem.as_deref(), Some("it's write-protected"));
}

/// A drive's name isn't taken for a disk identifier on any system, so nothing is asked of the
/// system's tools.
#[test]
fn a_drive_name_is_not_a_disk() {
    assert!(disk_by_id("Backup").unwrap().is_none());
}

/// Refused before anything is asked of the system's tools: more than MBR can address, a
/// cluster size the file system can't take, a FAT32 volume too small for its clusters.
#[test]
fn plans_refuse_what_cant_be_formatted() {
    let plan_of = |disk: Disk, opts: FormatOptions| plan(disk, &opts, None, &[], &Ctx::silent()).unwrap_err();
    let huge = Disk { size: 3_000_000_000_000, ..sample_disk() };
    assert!(plan_of(huge, FormatOptions::default()).to_string().contains("2 TiB"));
    let odd = FormatOptions { cluster_size: Some(3000), ..Default::default() };
    assert!(plan_of(sample_disk(), odd).to_string().contains("power of two"));
    let small = Disk { size: 1_000_000_000, ..sample_disk() };
    let big_clusters = FormatOptions { cluster_size: Some(32768), ..Default::default() };
    assert!(plan_of(small, big_clusters).to_string().contains("too small for FAT32"));
    let system = Disk { system: true, ..sample_disk() };
    assert!(plan_of(system, FormatOptions::default()).to_string().contains("refusing to format"));
}

/// A plan made for one disk formats nothing on another.
#[test]
fn applying_needs_the_planned_disks_identity() {
    let disk = sample_disk();
    let err = apply(disk, "0123456789abcdef", &FormatOptions::default(), None, &[], &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
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
    assert_eq!(err.code(), "io", "{err}");
}

// -----------------------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------------------

use hegel::generators as gs;
use hegel::{Generator, TestCase};

pub(crate) const FS_KINDS: [FsKind; 8] = [
    FsKind::Fat12,
    FsKind::Fat16,
    FsKind::Fat32,
    FsKind::Exfat,
    FsKind::Hfsplus,
    FsKind::Apfs,
    FsKind::Ntfs,
    FsKind::Other,
];
const FILESYSTEMS: [Filesystem; 3] = [Filesystem::Fat32, Filesystem::Exfat, Filesystem::Hfsplus];

/// Names a person might give a drive, with the characters the file systems refuse.
#[hegel::test_helper]
fn name(tc: &TestCase) -> String {
    tc.draw(hegel::one_of!(
        gs::text().max_size(30),
        gs::text().alphabet("aZ 09._-:/\\\"*?<>|+,;=[]\t\u{0}éß日🎛").max_size(30),
    ))
}

/// A volume. Mount points are under a directory that doesn't exist, so nothing is read there.
#[hegel::test_helper]
fn volume(tc: &TestCase, disk: &str, n: usize) -> Volume {
    let opt = |tc: &TestCase| tc.draw(gs::booleans());
    Volume {
        id: format!("{disk}s{}", n + 1),
        size: tc.draw(gs::integers::<u64>().max_value(3 << 40)),
        filesystem: if opt(tc) { Some(tc.draw(gs::sampled_from(FS_KINDS.to_vec()).print_as_debug())) } else { None },
        filesystem_name: match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => None,
            1 => Some(name(tc)),
            _ => Some(tc.draw(gs::sampled_from(vec!["MS-DOS FAT32", "Mac OS Extended (Case-sensitive)"])).into()),
        },
        label: if opt(tc) { Some(tc.draw(gs::sampled_from(vec!["EFI", "DNA", "", "  "])).into()) } else { None },
        mount: if opt(tc) {
            // Now and then the system's own root, which no plan may erase.
            let m = tc.draw(gs::sampled_from(vec!["a", "b", "c", "/"]));
            Some(if m == "/" { PathBuf::from("/") } else { PathBuf::from(format!("/locallibrary-nowhere/{m}")) })
        } else {
            None
        },
        cluster_size: if opt(tc) { Some(tc.draw(gs::integers::<u64>())) } else { None },
        uuid: if opt(tc) { Some(name(tc)) } else { None },
    }
}

/// A disk, mostly one that may be formatted.
#[hegel::test_helper]
pub(crate) fn any_disk(tc: &TestCase) -> Disk {
    let id = format!("disk{}", tc.draw(gs::integers::<u8>().max_value(12)));
    let rare = |tc: &TestCase| tc.draw(gs::weighted_booleans(0.15));
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    let volumes = (0..n).map(|i| volume(tc, &id, i)).collect();
    Disk {
        device: format!("/dev/{id}"),
        model: name(tc),
        serial: if tc.draw(gs::booleans()) { Some(name(tc)) } else { None },
        size: tc.draw(hegel::one_of!(
            gs::integers::<u64>().max_value(4 << 40),
            gs::integers::<u64>().min_value(1 << 30).max_value(players::MBR_MAX - 1),
        )),
        sector_size: tc.draw(gs::sampled_from(vec![512u32, 1024, 2048, 4096])),
        bus: if tc.draw(gs::booleans()) { Some("USB".into()) } else { None },
        removable: !rare(tc),
        internal: rare(tc),
        system: rare(tc),
        virtual_disk: rare(tc),
        read_only: rare(tc),
        scheme: tc.draw(
            gs::sampled_from(vec![None, Some(Scheme::Mbr), Some(Scheme::Gpt), Some(Scheme::Apm)]).print_as_debug(),
        ),
        volumes,
        id,
    }
}

/// Every label is one its file system takes: never empty, without the characters it refuses,
/// within its length (FAT32: 11 bytes of upper case ASCII; exFAT: 11 UTF-16 units; HFS+: 27
/// characters), and labelling a label changes nothing.
#[hegel::test(test_cases = 500)]
fn labels_suit_their_file_system(tc: TestCase) {
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
fn cluster_sizes_follow_each_file_systems_rules(tc: TestCase) {
    let fs = tc.draw(gs::sampled_from(FILESYSTEMS.to_vec()).print_as_debug());
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
    let volume = tc.draw(gs::integers::<u64>().max_value(players::MBR_MAX));
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
fn commands_display_unambiguously(tc: TestCase) {
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
fn tools_get_their_input_and_report_their_failures(tc: TestCase) {
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
fn lenient_json_reads_every_spelling(tc: TestCase) {
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

    let flag = tc.draw(
        gs::sampled_from(vec![
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
        ])
        .print_as_debug(),
    );
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

/// A disk's identity changes with what is on it (its id, model, serial, size, and each
/// volume's id, uuid, label, size and file system) and not with where volumes are mounted; the
/// disk-only fingerprint ignores the volumes. The check before erasing agrees with it.
#[hegel::test(test_cases = 300)]
fn identity_follows_what_is_on_the_disk(tc: TestCase) {
    let disk = any_disk(&tc);
    let mut other = disk.clone();
    let kind = tc.draw(gs::integers::<u8>().max_value(9));
    let has_volumes = !other.volumes.is_empty();
    let i = if has_volumes { tc.draw(gs::integers::<usize>().max_value(other.volumes.len() - 1)) } else { 0 };
    // What each change leaves alone: (the identity, the disk-only fingerprint).
    let (same_identity, same_disk) = match (kind, has_volumes) {
        (0, _) => {
            for v in &mut other.volumes {
                v.mount = v.mount.take().xor(Some("/locallibrary-nowhere/elsewhere".into()));
            }
            (true, true)
        }
        (1, _) => {
            other.id.push('0');
            (false, false)
        }
        (2, _) => {
            other.model.push('x');
            (false, false)
        }
        (3, _) => {
            other.serial = Some(format!("{}x", other.serial.unwrap_or_default()));
            (false, false)
        }
        (4, _) => {
            other.size ^= 1;
            (false, false)
        }
        (5, true) => {
            other.volumes[i].id.push('0');
            (false, true)
        }
        (6, true) => {
            other.volumes[i].uuid = Some(format!("{}x", other.volumes[i].uuid.take().unwrap_or_default()));
            (false, true)
        }
        (7, true) => {
            other.volumes[i].label = Some(format!("{}x", other.volumes[i].label.take().unwrap_or_default()));
            (false, true)
        }
        (8, true) => {
            other.volumes[i].size ^= 1;
            (false, true)
        }
        (9, true) => {
            let was = other.volumes[i].filesystem;
            other.volumes[i].filesystem =
                Some(tc.draw(gs::sampled_from(FS_KINDS.to_vec()).filter(move |k| Some(*k) != was).print_as_debug()));
            (false, true)
        }
        _ => {
            other.volumes.push(volume(&tc, &disk.id, 9));
            (false, true)
        }
    };
    assert_eq!(disk.identity() == other.identity(), same_identity, "change {kind}");
    assert_eq!(disk.fingerprint(false) == other.fingerprint(false), same_disk, "change {kind}");
    assert_eq!(still_the_disk(&disk, Some(&other), true).is_ok(), same_identity);
    assert_eq!(still_the_disk(&disk, Some(&other), false).is_ok(), same_disk);
    assert!(matches!(still_the_disk(&disk, None, false), Err(Error::StalePlan(_))));
    assert_eq!(disk.identity().len(), 16);
}

/// A disk may be formatted exactly when it is external or removable, physical, writable, and
/// holds neither the system nor a protected path; when it holds one, that path is under one of
/// its mount points. Listing keeps exactly the external and removable physical disks, as they
/// are.
#[hegel::test(test_cases = 300)]
fn disks_are_refused_for_exactly_their_problems(tc: TestCase) {
    let disks: Vec<Disk> = (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| any_disk(&tc)).collect();
    let protected: Vec<PathBuf> = tc
        .draw(
            gs::vecs(gs::sampled_from(vec!["/locallibrary-nowhere/a/store", "/locallibrary-nowhere/b", "/Users/x"]))
                .max_size(2),
        )
        .into_iter()
        .map(PathBuf::from)
        .collect();
    for d in &disks {
        let held = d.holding(&protected);
        if let Some((mount, path)) = held {
            assert!(path.starts_with(mount) && protected.contains(path));
            assert!(d.volumes.iter().any(|v| v.mount.as_deref() == Some(mount)));
        } else {
            assert!(
                !d.volumes
                    .iter()
                    .filter_map(|v| v.mount.as_deref())
                    .any(|m| protected.iter().any(|p| p.starts_with(m)))
            );
        }
        let root = d.volumes.iter().any(|v| v.mount.as_deref() == Some(Path::new("/")));
        let fine =
            !d.system && !d.internal && d.removable && !d.virtual_disk && !d.read_only && !root && held.is_none();
        assert_eq!(d.problem(&protected).is_none(), fine, "{d:?}");
        assert!(d.describe().contains(&d.id));
        let usable = d.size.saturating_sub(PARTITION_START);
        let p = d.partition_size();
        assert!(p <= usable && usable - p < u64::from(d.sector_size) && p % u64::from(d.sector_size) == 0);
    }
    let shown = views(disks.clone(), &protected);
    let kept: Vec<&Disk> = disks.iter().filter(|d| d.removable && !d.internal && !d.virtual_disk).collect();
    assert_eq!(shown.len(), kept.len());
    for (v, d) in shown.iter().zip(kept) {
        assert_eq!((v.disk.identity(), &v.identity, &v.problem), (d.identity(), &d.identity(), &d.problem(&protected)));
        assert_eq!(v.compatibility.verdict, players::assess_disk(d).verdict);
    }
}

/// A plan is made exactly when the disk may be formatted, fits MBR, and the cluster size suits
/// the file system and the volume; it then describes that disk, its whole partition, a label
/// the file system takes, a step list, and a warning for each unmounted volume it destroys.
/// Applying it to any other identity changes nothing.
#[hegel::test(test_cases = 200)]
#[cfg_attr(
    target_os = "linux",
    ignore = "a plan on Linux asks lsblk what's mounted on the disk, and the generated disk isn't there"
)]
fn plans_follow_the_disk_and_the_options(tc: TestCase) {
    let disk = any_disk(&tc);
    let opts = FormatOptions {
        filesystem: tc.draw(
            gs::sampled_from(vec![None, Some(Filesystem::Fat32), Some(Filesystem::Exfat), Some(Filesystem::Hfsplus)])
                .print_as_debug(),
        ),
        label: if tc.draw(gs::booleans()) { Some(name(&tc)) } else { None },
        cluster_size: tc.draw(gs::sampled_from(vec![
            None,
            Some(512),
            Some(4096),
            Some(32768),
            Some(65536),
            Some(3000),
        ])),
        check_capacity: tc.draw(gs::booleans()),
    };
    let os = tc.draw(gs::sampled_from(vec![None, Some("linux"), Some("windows"), Some("macos")]));
    THREAD_OS.with(|o| o.set(os));
    let made = plan(disk.clone(), &opts, None, &[], &Ctx::silent());
    THREAD_OS.with(|o| o.set(None));

    let fs = opts.filesystem.unwrap_or(Filesystem::Fat32);
    let part = disk.partition_size();
    let cluster = opts.cluster_size.unwrap_or_else(|| fs.default_cluster_size(part));
    let fits = disk.problem(&[]).is_none()
        && disk.size < players::MBR_MAX
        && fs.check_cluster_size(cluster, disk.sector_size).is_ok()
        && (fs != Filesystem::Fat32 || fat32::Layout::new(part, disk.sector_size, cluster, 0).is_ok());
    let p = match made {
        Ok(p) => p,
        Err(e) => {
            assert!(!fits, "{e}");
            assert_eq!(e.code(), "invalid", "{e}");
            return;
        }
    };
    assert!(fits);
    assert_eq!((p.identity.as_str(), p.filesystem, p.scheme), (disk.identity().as_str(), fs, Scheme::Mbr));
    assert_eq!((p.cluster_size, p.partition_size, p.confirmation()), (cluster, part, disk.id.as_str()));
    assert_eq!(fs.label(&p.label), p.label);
    if let Some(l) = &opts.label {
        assert_eq!(p.label, fs.label(l));
    }
    assert_eq!(p.erases.len(), disk.volumes.len());
    assert!(p.erases.iter().all(|e| e.drive_name.is_none() && e.tracks.is_none() && e.in_store.is_none()));
    let unmounted =
        disk.volumes.iter().filter(|v| v.mount.is_none() && v.size > 0 && v.label.as_deref() != Some("EFI"));
    let warned = p.warnings.iter().filter(|w| w.contains("isn't mounted")).count();
    assert_eq!(warned, unmounted.count());
    let untested = p.warnings.iter().any(|w| w.contains("hasn't been tested on"));
    assert_eq!(untested, matches!(os, Some("linux" | "windows")));
    let all = p.compatibility.verdict == Verdict::AllPlayers;
    assert_eq!(p.warnings.iter().any(|w| w.contains("won't work on every player")), !all);
    assert_eq!(all, fs != Filesystem::Exfat);
    assert!(!p.steps.is_empty());
    assert_eq!(p.steps[0].starts_with("test the real capacity"), opts.check_capacity);
    let wrong = format!("{}x", p.identity);
    assert!(matches!(apply(disk, &wrong, &opts, None, &[], &Ctx::silent()), Err(Error::StalePlan(_))));
}

/// Formatting a device needs write access to it: a writable one passes, a read-only one asks
/// for administrator rights naming the device and how to get them, and a missing one is an
/// I/O error.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn write_access_is_checked_before_formatting(tc: TestCase) {
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
        _ => assert_eq!(got.unwrap_err().code(), "io"),
    }
}
