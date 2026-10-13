use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::fleet::Role;
use locallibrary::format::{self, Disk, Filesystem, FormatOptions, FsKind};
use locallibrary::progress::Ctx;
use std::path::PathBuf;

use super::*;

/// What the terminal shows of a styled string: the text, without colour codes.
fn plain(s: impl std::fmt::Display) -> String {
    console::strip_ansi_codes(&s.to_string()).into_owned()
}

#[hegel::test_helper]
fn maybe<T>(tc: &TestCase, value: impl FnOnce() -> T) -> Option<T> {
    tc.draw(gs::booleans()).then(value)
}

#[hegel::test_helper]
fn name(tc: &TestCase) -> String {
    tc.draw(gs::sampled_from(&["DJ", "Ünïcödé 日本", "", "BACKUP", "a b"][..])).to_string()
}

#[hegel::test_helper]
fn volume(tc: &TestCase, i: usize) -> Volume {
    let kinds = [
        FsKind::Fat12,
        FsKind::Fat16,
        FsKind::Fat32,
        FsKind::Exfat,
        FsKind::Hfsplus,
        FsKind::Apfs,
        FsKind::Ntfs,
        FsKind::Other,
    ];
    Volume {
        id: format!("disk4s{}", i + 1),
        size: tc.draw(gs::integers::<u64>().max_value(1 << 40)),
        filesystem: maybe(tc, || kinds[tc.draw(gs::integers::<usize>().max_value(kinds.len() - 1))]),
        filesystem_name: maybe(tc, || name(tc)),
        label: maybe(tc, || name(tc)),
        mount: maybe(tc, || PathBuf::from(format!("/Volumes/{}", name(tc)))),
        cluster_size: maybe(tc, || 512 << tc.draw(gs::integers::<u32>().max_value(8))),
        uuid: None,
    }
}

#[hegel::test_helper]
fn disk(tc: &TestCase, size: u64) -> Disk {
    let schemes = [Scheme::Mbr, Scheme::Gpt, Scheme::Apm, Scheme::Other];
    let volumes = (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|i| volume(tc, i)).collect();
    Disk {
        id: "disk4".into(),
        device: "/dev/disk4".into(),
        model: tc.draw(gs::sampled_from(&["", "SanDisk Extreme"][..])).into(),
        serial: None,
        size,
        sector_size: 512,
        bus: maybe(tc, || "USB".to_string()),
        removable: true,
        internal: false,
        system: false,
        virtual_disk: false,
        read_only: false,
        scheme: maybe(tc, || schemes[tc.draw(gs::integers::<usize>().max_value(3))]),
        volumes,
    }
}

#[hegel::test_helper]
fn compatibility_drawn(tc: &TestCase) -> Compatibility {
    let verdicts = [Verdict::AllPlayers, Verdict::SomePlayers, Verdict::Unknown, Verdict::NoPlayers];
    Compatibility {
        verdict: verdicts[tc.draw(gs::integers::<usize>().max_value(3))],
        summary: name(tc),
        unsupported: tc.draw(gs::subsequences(vec!["CDJ-900".to_string(), "XDJ-700".to_string()])),
        notes: tc.draw(gs::subsequences(vec!["needs rekordbox 6.6.2".to_string()])),
    }
}

/// A volume's line names it (its label, else its identifier), where it's mounted (or that it
/// isn't), its file system (or that it has none) and its size, and its clusters when they're
/// known; each partition table has a name of its own; and a verdict's mark says how players get
/// on with it, before its summary. Disks are listed with every part of them.
#[hegel::test(test_cases = 100)]
fn disks_are_listed_with_what_players_make_of_them(tc: TestCase) {
    let views: Vec<DiskView> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|_| {
            let disk = disk(&tc, tc.draw(gs::integers::<u64>().min_value(1 << 26).max_value(1 << 42)));
            DiskView {
                identity: disk.identity(),
                compatibility: compatibility_drawn(&tc),
                problem: maybe(&tc, || "it's write-protected".to_string()),
                disk,
            }
        })
        .collect();
    for v in views.iter().flat_map(|d| &d.disk.volumes) {
        let line = volume_line(v);
        assert!(line.starts_with(v.label.as_deref().unwrap_or(&v.id)), "{line}");
        match &v.mount {
            Some(m) => assert!(line.contains(&m.display().to_string()), "{line}"),
            None => assert!(line.contains("not mounted"), "{line}"),
        }
        assert!(line.contains(v.filesystem_name.as_deref().unwrap_or("no file system")), "{line}");
        assert_eq!(line.contains(" KB clusters"), v.cluster_size.is_some(), "{line}");
        assert!(line.ends_with(&bytes(v.size)), "{line}");
    }
    let schemes = [None, Some(Scheme::Mbr), Some(Scheme::Gpt), Some(Scheme::Apm), Some(Scheme::Other)];
    let mut names: Vec<&str> = schemes.iter().map(|s| scheme_name(*s)).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), schemes.len());
    for c in views.iter().map(|v| &v.compatibility) {
        let mark = plain(verdict_mark(c));
        let expected = match c.verdict {
            Verdict::AllPlayers => '✓',
            Verdict::SomePlayers | Verdict::Unknown => '~',
            Verdict::NoPlayers => '✗',
        };
        assert_eq!(mark, format!("{expected} {}", c.summary));
    }
    disks(&views);
}

/// A plan to format a disk is made for any disk players can use (an MBR disk under 2 TiB),
/// erasing each of its volumes, and is shown with every part of it: the volumes it erases (an
/// adopted drive's name and role, a library and whether the store has it), what it creates, how
/// players get on with that, the name the drive keeps, the steps and the warnings. Its report
/// says where the new volume is, or that it isn't mounted yet.
#[hegel::test(test_cases = 100)]
#[cfg_attr(
    target_os = "linux",
    ignore = "a plan on Linux asks lsblk what's mounted on the disk, and the generated disk isn't there"
)]
fn format_plans_and_reports_show_every_part(tc: TestCase) {
    // From the smallest disk FAT32 takes with the default clusters.
    let size = tc.draw(gs::integers::<u64>().min_value(1 << 29).max_value(1 << 42));
    let d = disk(&tc, size);
    let filesystems = [Filesystem::Fat32, Filesystem::Exfat, Filesystem::Hfsplus];
    let filesystem = filesystems[tc.draw(gs::integers::<usize>().max_value(2))];
    let options = FormatOptions { filesystem: Some(filesystem), ..FormatOptions::default() };
    let progress = locallibrary::progress::NoProgress;
    let planned = format::plan(d.clone(), &options, None, &[], &Ctx::new(&progress, Default::default()));
    let mut plan = match planned {
        Ok(p) => p,
        Err(e) => {
            assert!(size >= 1 << 41, "{} refused: {e}", d.describe());
            return;
        }
    };
    assert!(size < 1 << 41, "a disk larger than an MBR addresses, planned");
    assert_eq!(plan.erases.len(), d.volumes.len());
    assert!(plan.partition_size <= size && plan.filesystem == filesystem);
    assert!(!plan.confirmation().is_empty());
    for e in &mut plan.erases {
        e.drive_name = maybe(&tc, || name(&tc));
        e.role = maybe(&tc, || Role::ALL[tc.draw(gs::integers::<usize>().max_value(3))]);
        e.retired = tc.draw(gs::booleans());
        e.tracks = maybe(&tc, || tc.draw(gs::integers::<usize>().max_value(5000)));
        e.in_store = maybe(&tc, || tc.draw(gs::booleans()));
    }
    plan.keeps_name = maybe(&tc, || name(&tc));
    plan.compatibility = compatibility_drawn(&tc);
    plan.steps = tc.draw(gs::subsequences(vec!["diskutil eraseDisk".to_string(), "newfs_msdos".to_string()]));
    plan.warnings = tc.draw(gs::subsequences(vec!["untested here".to_string()]));
    format_plan(&plan);

    let report = FormatReport {
        disk: d.id.clone(),
        filesystem,
        label: plan.label.clone(),
        mount: maybe(&tc, || PathBuf::from("/Volumes/DJ")),
        cluster_size: maybe(&tc, || u64::from(plan.cluster_size)),
        capacity: None,
        drive_name: maybe(&tc, || name(&tc)),
    };
    format_report(&report);
}
