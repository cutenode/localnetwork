use super::*;

#[test]
fn verdicts() {
    let mbr = Some(Scheme::Mbr);
    assert_eq!(assess(mbr, Some(FsKind::Fat32), 1 << 40).verdict, Verdict::AllPlayers);
    let exfat = assess(mbr, Some(FsKind::Exfat), 1 << 40);
    assert_eq!(exfat.verdict, Verdict::SomePlayers);
    assert!(exfat.unsupported.iter().any(|p| p == "CDJ-2000NXS2"));
    assert_eq!(assess(Some(Scheme::Gpt), Some(FsKind::Fat32), 1 << 30).verdict, Verdict::NoPlayers);
    assert_eq!(assess(mbr, Some(FsKind::Ntfs), 1 << 30).verdict, Verdict::NoPlayers);
    assert_eq!(assess(None, Some(FsKind::Fat32), 1 << 30).verdict, Verdict::Unknown);
}

#[test]
fn players_read_the_first_partition() {
    let mut d = crate::format::tests::sample_disk();
    let mut second = d.volumes[0].clone();
    second.filesystem = Some(FsKind::Ntfs);
    d.volumes.push(second);
    let c = assess_disk(&d);
    assert_eq!(c.verdict, Verdict::AllPlayers);
    assert!(c.notes.iter().any(|n| n.contains("first partition")));
    d.volumes.reverse();
    assert_eq!(assess_disk(&d).verdict, Verdict::NoPlayers);
}

#[test]
fn each_file_system_gets_its_verdict() {
    let mbr = Some(Scheme::Mbr);
    let fat16 = assess(mbr, Some(FsKind::Fat16), 1 << 30);
    assert_eq!(fat16.verdict, Verdict::AllPlayers);
    assert!(fat16.notes.iter().any(|n| n.contains("need FAT32")), "{:?}", fat16.notes);
    let hfs = assess(mbr, Some(FsKind::Hfsplus), 1 << 30);
    assert_eq!(hfs.verdict, Verdict::AllPlayers);
    assert!(hfs.notes.iter().any(|n| n.contains("rekordbox for Windows")), "{:?}", hfs.notes);
    for fs in [None, Some(FsKind::Fat12), Some(FsKind::Apfs), Some(FsKind::Other)] {
        assert_eq!(assess(mbr, fs, 1 << 30).verdict, Verdict::NoPlayers, "{fs:?}");
    }
}

/// The partition table only matters once the file system is one players read.
#[test]
fn partition_tables_and_size_matter_once_the_file_system_is_read() {
    for scheme in [Scheme::Apm, Scheme::Other] {
        let c = assess(Some(scheme), Some(FsKind::Fat32), 1 << 30);
        assert_eq!((c.verdict, c.summary.as_str()), (Verdict::NoPlayers, "players need an MBR partition table"));
    }
    let bare = assess(None, Some(FsKind::Exfat), 1 << 30);
    assert!(
        bare.summary.starts_with("exFAT: only CDJ-3000")
            && bare.summary.ends_with("without a partition table, which players aren't documented to read")
    );
    let big = assess(Some(Scheme::Mbr), Some(FsKind::Fat32), MBR_MAX + 1);
    assert_eq!(big.notes, ["players can't use more than 2 TiB"]);
}

#[test]
fn disks_without_partitions_or_with_case_sensitive_hfs_are_doubted() {
    let mut d = crate::format::tests::sample_disk();
    d.volumes[0].filesystem = Some(FsKind::Hfsplus);
    d.volumes[0].filesystem_name = Some("Mac OS Extended (Case-sensitive)".into());
    let c = assess_disk(&d);
    assert_eq!(c.verdict, Verdict::Unknown);
    assert!(c.notes.iter().any(|n| n == "case-sensitive HFS+ may not be recognised"));
    d.volumes[0].filesystem_name = Some("Mac OS Extended".into());
    assert_eq!(assess_disk(&d).verdict, Verdict::AllPlayers);
    d.volumes.clear();
    let none = assess_disk(&d);
    assert_eq!((none.verdict, none.summary.as_str()), (Verdict::NoPlayers, "no partitions"));
}

// -----------------------------------------------------------------------------------------
// Properties
// -----------------------------------------------------------------------------------------

use hegel::TestCase;
use hegel::generators as gs;

const SCHEMES: [Option<Scheme>; 5] =
    [None, Some(Scheme::Mbr), Some(Scheme::Gpt), Some(Scheme::Apm), Some(Scheme::Other)];

/// Every layout gets a verdict: no player reads a file system they don't support or anything
/// but an MBR partition table; FAT and HFS+ on MBR work everywhere, exFAT on some players (and
/// the rest are named); without a partition table it's unknown. Only a readable layout
/// warns about size past 2 TiB.
#[hegel::test(test_cases = 300)]
fn every_layout_gets_its_verdict(tc: TestCase) {
    let scheme = tc.draw(gs::sampled_from(SCHEMES.to_vec()));
    let all = crate::format::tests::FS_KINDS.map(Some);
    let fs = tc.draw(gs::sampled_from([&[None][..], &all[..]].concat()));
    let size = tc.draw(hegel::one_of!(
        gs::integers::<u64>(),
        gs::integers::<u64>().min_value(MBR_MAX - 2).max_value(MBR_MAX + 2)
    ));
    let c = assess(scheme, fs, size);
    let readable = matches!(fs, Some(FsKind::Fat32 | FsKind::Fat16 | FsKind::Hfsplus | FsKind::Exfat));
    let want = match (readable, scheme) {
        (false, _) | (true, Some(Scheme::Gpt | Scheme::Apm | Scheme::Other)) => Verdict::NoPlayers,
        (true, None) => Verdict::Unknown,
        (true, Some(Scheme::Mbr)) if fs == Some(FsKind::Exfat) => Verdict::SomePlayers,
        (true, Some(Scheme::Mbr)) => Verdict::AllPlayers,
    };
    assert_eq!(c.verdict, want, "{scheme:?} {fs:?}");
    assert!(!c.summary.is_empty());
    let exfat_named = c.unsupported == NO_EXFAT && c.summary.contains(EXFAT[0]);
    assert_eq!(exfat_named, fs == Some(FsKind::Exfat) && scheme.is_none_or(|s| s == Scheme::Mbr));
    if c.verdict != Verdict::SomePlayers {
        assert!(c.unsupported.is_empty() || exfat_named);
    }
    let size_note = c.notes.iter().any(|n| n.contains("2 TiB"));
    assert_eq!(size_note, readable && size > MBR_MAX);
    assert_eq!(c.summary.contains("without a partition table"), readable && scheme.is_none());
}

/// Players read a disk's first partition: the disk gets that partition's verdict, doubted for
/// case-sensitive HFS+, with a note naming it when there are others; a disk without
/// partitions works on none.
#[hegel::test(test_cases = 300)]
fn disks_are_judged_by_their_first_partition(tc: TestCase) {
    let disk = crate::format::tests::any_disk(&tc);
    let c = assess_disk(&disk);
    let Some(first) = disk.volumes.first() else {
        assert_eq!((c.verdict, c.summary.as_str()), (Verdict::NoPlayers, "no partitions"));
        return;
    };
    let alone = assess(disk.scheme, first.filesystem, first.size);
    let case_sensitive = first.filesystem == Some(FsKind::Hfsplus)
        && first.filesystem_name.as_deref().is_some_and(|n| n.to_lowercase().contains("case-sensitive"));
    let verdict = if case_sensitive { Verdict::Unknown } else { alone.verdict };
    assert_eq!(c.verdict, verdict);
    assert_eq!(c.summary, alone.summary);
    assert!(c.notes.starts_with(&alone.notes));
    let named = c.notes.iter().any(|n| n.starts_with("players read the first partition"));
    assert_eq!(named, disk.volumes.len() > 1 && verdict != Verdict::NoPlayers);
}

/// An HFS+ first partition is doubted exactly when its file system is the case-sensitive kind,
/// however the system spells that, and the doubt says why; any other one keeps its verdict.
#[hegel::test(test_cases = 100)]
fn case_sensitive_hfs_is_doubted_in_any_spelling(tc: TestCase) {
    let mut disk = crate::format::tests::sample_disk();
    let sensitive = tc.draw(gs::booleans());
    let name = if sensitive {
        let word: String = "case-sensitive"
            .chars()
            .map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c })
            .collect();
        format!("Mac OS Extended ({word}, Journaled)")
    } else {
        tc.draw(gs::sampled_from(vec!["Mac OS Extended (Journaled)", "Mac OS Extended", "hfsplus"])).to_string()
    };
    let first = &mut disk.volumes[0];
    (first.filesystem, first.filesystem_name) = (Some(FsKind::Hfsplus), Some(name.clone()));
    let alone = assess(disk.scheme, first.filesystem, first.size);
    let c = assess_disk(&disk);
    assert_eq!(c.verdict, if sensitive { Verdict::Unknown } else { alone.verdict }, "{name}");
    assert_eq!(c.notes.iter().any(|n| n == "case-sensitive HFS+ may not be recognised"), sensitive, "{name}");
}
