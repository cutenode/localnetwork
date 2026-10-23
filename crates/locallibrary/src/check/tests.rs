use super::*;

fn models(players: &[&str]) -> Vec<(String, Option<&'static Model>)> {
    players.iter().map(|p| (p.to_string(), equipment::find(p))).collect()
}

type Items = Vec<(Level, String)>;

/// The `players` items, and the `formats` items.
fn verdicts(players: &[&str], layout: &Layout, audio: &DriveAudio) -> (Items, Items) {
    let (mut players_items, mut formats) = (Vec::new(), Vec::new());
    player_items(&models(players), layout, audio, &mut |check, level, message| match check {
        "players" => players_items.push((level, message)),
        _ => formats.push((level, message)),
    });
    (players_items, formats)
}

fn levels(v: &[(Level, String)]) -> Vec<Level> {
    v.iter().map(|(l, _)| *l).collect()
}

#[test]
fn players_against_file_systems_and_databases() {
    let none = DriveAudio::default();
    let drive = |fs, has_plus| Layout { has_pdb: true, has_plus, volume: Some((Some(Scheme::Mbr), Some(fs))) };
    let players = |p: &[&str], layout: &Layout| verdicts(p, layout, &none).0;
    let all = ["CDJ-2000NXS2", "cdj-3000", "CDJ-3000X", "DJM-V10"];
    assert_eq!(levels(&players(&all, &drive(FsKind::Fat32, true))), [Level::Info; 4]);
    let exfat = players(&all, &drive(FsKind::Exfat, true));
    assert_eq!(levels(&exfat), [Level::Problem, Level::Warning, Level::Info, Level::Problem]);
    assert!(exfat[1].1.contains("firmware 1.20+"), "{exfat:?}");
    assert_eq!(
        levels(&players(&all, &drive(FsKind::Hfsplus, false))),
        [Level::Info, Level::Info, Level::Problem, Level::Problem]
    );
    let gpt = Layout { has_pdb: true, has_plus: true, volume: Some((Some(Scheme::Gpt), Some(FsKind::Fat32))) };
    assert_eq!(levels(&players(&["CDJ-3000"], &gpt)), [Level::Problem]);
    let unknown = Layout { has_pdb: true, has_plus: false, volume: None };
    assert_eq!(
        levels(&players(&["CDJ-2000NXS2", "OPUS-QUAD", "XYZ-9"], &unknown)),
        [Level::Info, Level::Problem, Level::Warning]
    );
    // A drive the CDJ-3000 can't read but players reading only OneLibrary can.
    let only_plus = Layout { has_pdb: false, has_plus: true, volume: None };
    assert_eq!(levels(&players(&["CDJ-3000", "OMNIS-DUO"], &only_plus)), [Level::Problem, Level::Info]);
}

/// Without models, a drive with one database warns (the audit's D8: a drive holding only
/// export.pdb used to pass without a word about the CDJ-3000X).
#[test]
fn no_models_and_one_database() {
    let none = DriveAudio::default();
    let pdb_only = Layout { has_pdb: true, has_plus: false, volume: None };
    let v = verdicts(&[], &pdb_only, &none).0;
    assert_eq!(levels(&v), [Level::Warning]);
    assert!(v[0].1.contains("CDJ-3000X") && v[0].1.contains("XDJ-AN"), "{v:?}");
    let plus_only = Layout { has_pdb: false, has_plus: true, volume: None };
    let v = verdicts(&[], &plus_only, &none).0;
    assert!(v[0].1.contains("CDJ-2000NXS2") && !v[0].1.contains("CDJ-3000X"), "{v:?}");
    let both = Layout { has_pdb: true, has_plus: true, volume: None };
    assert!(verdicts(&[], &both, &none).0.is_empty());
}

fn row(path: &str, bitrate: u32, rate: u32, depth: u16) -> Track {
    Track { bitrate, sample_rate: rate, sample_depth: depth, ..loclib_model::test_util::track(1, "", path, path) }
}

#[test]
fn audio_from_rows() {
    let a = track_audio(&row("/Contents/a.m4a", 256, 44_100, 16), None);
    assert_eq!(a.codec, Some(Ok(Codec::Aac)));
    let a = track_audio(&row("/Contents/a.m4a", 900, 96_000, 24), None);
    assert_eq!((a.codec, a.rate, a.depth), (Some(Ok(Codec::Alac)), 96_000, 24));
    // The file has the last word on an .m4a, and fills in what the row leaves out.
    let probed = probe::Format {
        codec: Some(Codec::Alac),
        sample_rate: 48_000,
        bit_depth: 24,
        channels: 2,
        duration_secs: None,
    };
    let a = track_audio(&row("/Contents/a.m4a", 0, 0, 0), Some(&probed));
    assert_eq!((a.codec, a.rate, a.depth), (Some(Ok(Codec::Alac)), 48_000, 24));
    assert_eq!(track_audio(&row("/Contents/a.m4a", 360, 44_100, 16), None).codec, None);
    assert_eq!(track_audio(&row("/Contents/a.ogg", 0, 0, 0), None).codec, Some(Err(".ogg".into())));
    assert!(needs_probe(&row("/Contents/a.flac", 0, 0, 0)));
    assert!(needs_probe(&row("/Contents/a.flac", 0, 44_100, 0)));
    assert!(!needs_probe(&row("/Contents/a.mp3", 320, 44_100, 0)));
    assert!(needs_probe(&row("/Contents/a.m4a", 360, 44_100, 16)));
}

#[test]
fn formats_players_wont_load() {
    let mut tracks: Vec<TrackAudio> = Vec::new();
    for i in 0..12 {
        tracks.push(track_audio(&row(&format!("/Contents/hi{i}.flac"), 0, 96_000, 24), None));
    }
    tracks.push(track_audio(&row("/Contents/a.mp3", 320, 44_100, 16), None));
    tracks.push(track_audio(&row("/Contents/b.wav", 0, 44_100, 32), None));
    tracks.push(track_audio(&row("/Contents/c.ogg", 0, 44_100, 16), None));
    let audio = DriveAudio { pdb: Some(tracks.clone()), plus: None };
    let layout = Layout { has_pdb: true, has_plus: false, volume: None };

    let (_, formats) = verdicts(&["CDJ-2000NXS"], &layout, &audio);
    assert_eq!(levels(&formats), [Level::Problem; 3]);
    assert_eq!(formats[0].1, "12 FLAC files won't load on the CDJ-2000NXS: it doesn't play FLAC");
    assert!(formats.iter().any(|(_, m)| m == "1 .ogg file won't load on the CDJ-2000NXS: it doesn't play .ogg"));
    let wav = "1 WAV file at 32-bit won't load on the CDJ-2000NXS: it plays WAV at 16 and 24-bit";
    assert!(formats.iter().any(|(_, m)| m == wav), "{formats:?}");

    let (_, formats) = verdicts(&["CDJ-2000NXS2"], &layout, &audio);
    assert_eq!(formats.len(), 2, "FLAC at 96 kHz plays: {formats:?}");

    let audio_plus = DriveAudio { pdb: None, plus: Some(tracks) };
    let layout_plus = Layout { has_pdb: false, has_plus: true, volume: None };
    let (_, formats) = verdicts(&["OMNIS-DUO"], &layout_plus, &audio_plus);
    assert_eq!(formats[0].1, "12 FLAC files at 96 kHz won't load on the OMNIS-DUO: it plays FLAC at 44.1 and 48 kHz");

    // Inferred specs say "probably", as warnings.
    let (_, formats) = verdicts(&["XDJ-RX2"], &layout, &audio);
    let flac = "12 FLAC files probably won't load on the XDJ-RX2: it probably doesn't play FLAC";
    assert!(formats.contains(&(Level::Warning, flac.to_string())), "{formats:?}");

    let fine = DriveAudio { pdb: Some(vec![track_audio(&row("/Contents/a.mp3", 320, 44_100, 16), None)]), plus: None };
    assert_eq!(
        verdicts(&["CDJ-3000"], &layout, &fine).1,
        [(Level::Info, "CDJ-3000: plays all 1 track(s) (format, sample rate and bit depth)".to_string())]
    );
    // Mixers have no tracks to play.
    assert!(verdicts(&["DJM-A9"], &layout, &fine).1.is_empty());
}

#[test]
fn players_refuse_other_partition_tables_and_file_systems() {
    let none = DriveAudio::default();
    let on = |scheme, fs| Layout { has_pdb: true, has_plus: true, volume: Some((scheme, fs)) };
    let said = |player: &str, layout: &Layout| verdicts(&[player], layout, &none).0.remove(0);
    let mbr = Some(Scheme::Mbr);
    for (fs, name) in
        [(FsKind::Fat12, "FAT12"), (FsKind::Ntfs, "NTFS"), (FsKind::Apfs, "APFS"), (FsKind::Other, "this file system")]
    {
        assert_eq!(said("CDJ-3000", &on(mbr, Some(fs))), (Level::Problem, format!("CDJ-3000: can't read {name}")));
    }
    assert_eq!(
        said("DJM-A9", &on(mbr, Some(FsKind::Hfsplus))),
        (Level::Problem, "DJM-A9: settings and firmware updates need FAT32".into())
    );
    assert_eq!(
        said("CDJ-3000", &on(Some(Scheme::Apm), Some(FsKind::Fat32))),
        (Level::Problem, "CDJ-3000: players need an MBR partition table".into())
    );
    // Neither a partition table nor a file system that can be told: warnings, both said.
    assert_eq!(
        said("CDJ-3000", &on(None, None)),
        (
            Level::Warning,
            "CDJ-3000: the drive has no partition table, which players aren't documented to read; the drive's \
                 file system isn't known"
                .into()
        )
    );
}

#[test]
fn formats_out_of_range_are_problems_and_unknown_ones_are_left_open() {
    let tracks = vec![
        track_audio(&row("/Contents/hi.wav", 0, 192_000, 32), None),
        // An .m4a neither its row nor its file tells apart: not held against any player.
        track_audio(&row("/Contents/a.m4a", 360, 44_100, 16), None),
    ];
    let audio = DriveAudio { pdb: Some(tracks), plus: None };
    let layout = Layout { has_pdb: true, has_plus: false, volume: None };
    let (_, formats) = verdicts(&["CDJ-2000NXS2"], &layout, &audio);
    assert_eq!(formats.len(), 1, "{formats:?}");
    let (level, message) = &formats[0];
    assert_eq!(*level, Level::Problem);
    let start = "1 WAV file at 192 kHz/32-bit won't load on the CDJ-2000NXS2: it plays WAV at 44.1, 48";
    assert!(message.starts_with(start) && message.ends_with(" kHz, 16 and 24-bit"), "{message}");

    // The CDJ-2000's specs are inferred from the CDJ-900's.
    let mp3 = DriveAudio { pdb: Some(vec![track_audio(&row("/Contents/a.mp3", 320, 44_100, 16), None)]), plus: None };
    let message = "CDJ-2000: probably plays all 1 track(s) (format, sample rate and bit depth; some of its specs \
                       are inferred)";
    assert_eq!(verdicts(&["CDJ-2000"], &layout, &mp3).1, [(Level::Info, message.to_string())]);
}

/// Each model given gets one `players` item on any drive: a model nobody knows a warning; a known
/// one is fine on an MBR drive formatted FAT32 with the database it reads, refused for the
/// database it lacks (unless it's a mixer, which reads none), on any other partition table or a
/// file system it can't read, and warned about where it can't be told; an item is fine exactly
/// when nothing stands in the way.
#[hegel::test(test_cases = 300)]
fn every_player_is_checked_against_the_drives_layout(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut given: Vec<String> =
        MODELS.iter().filter(|_| tc.draw(gs::weighted_booleans(0.2))).map(|m| m.name.to_string()).collect();
    if tc.draw(gs::weighted_booleans(0.2)) {
        given.push("XYZ-9".into());
    }
    let schemes = [None, Some(Scheme::Mbr), Some(Scheme::Gpt), Some(Scheme::Apm), Some(Scheme::Other)];
    let systems = vec![
        None,
        Some(FsKind::Fat12),
        Some(FsKind::Fat16),
        Some(FsKind::Fat32),
        Some(FsKind::Exfat),
        Some(FsKind::Hfsplus),
        Some(FsKind::Apfs),
        Some(FsKind::Ntfs),
        Some(FsKind::Other),
    ];
    let volume = match tc.draw(gs::booleans()) {
        true => Some((tc.draw(gs::sampled_from(&schemes)), tc.draw(gs::sampled_from(&systems)))),
        false => None,
    };
    let (has_pdb, has_plus) = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => (true, false),
        1 => (false, true),
        _ => (true, true),
    };
    let layout = Layout { has_pdb, has_plus, volume };
    let names: Vec<&str> = given.iter().map(String::as_str).collect();
    let (players, _) = verdicts(&names, &layout, &DriveAudio::default());
    if given.is_empty() {
        return;
    }
    assert_eq!(players.len(), given.len(), "{players:?}");
    let best = Layout { has_pdb: true, has_plus: true, volume: Some((Some(Scheme::Mbr), Some(FsKind::Fat32))) };
    let (on_best, _) = verdicts(&names, &best, &DriveAudio::default());
    for ((g, (level, message)), (best_level, _)) in given.iter().zip(&players).zip(&on_best) {
        assert!(message.starts_with(&format!("{g}:")) || equipment::find(g).is_some(), "{message}");
        let Some(m) = equipment::find(g) else {
            assert_eq!(*level, Level::Warning, "{message}");
            assert!(message.contains("not a model"), "{message}");
            continue;
        };
        assert_eq!(*best_level, Level::Info, "{g} on an MBR FAT32 drive with both databases");
        assert!(*level >= *best_level);
        let reads_its_database = (m.export_pdb.value && has_pdb) || (m.onelibrary.value && has_plus);
        let lacks = !(m.is_mixer() || reads_its_database);
        if lacks {
            assert!(*level > Level::Info && message.contains("which this drive doesn't have"), "{message}");
        }
        if let Some((scheme, fs)) = volume {
            match scheme {
                Some(Scheme::Mbr) => {}
                None => assert!(*level >= Level::Warning && message.contains("no partition table"), "{message}"),
                Some(_) => assert!(*level == Level::Problem && message.contains("partition table"), "{message}"),
            }
            match fs.map(|fs| (fs, m.reads(fs))) {
                None => assert!(message.contains("file system isn't known"), "{message}"),
                Some((_, Some(true))) => {}
                Some((_, None)) => assert!(message.contains("reads exFAT only with firmware"), "{message}"),
                Some((_, Some(false))) => assert_eq!(*level, Level::Problem, "{message}"),
            }
        }
        let fine = !lacks
            && volume.is_none_or(|(s, fs)| s == Some(Scheme::Mbr) && fs.is_some_and(|fs| m.reads(fs) == Some(true)));
        assert_eq!(*level == Level::Info, fine, "{message}");
    }
}

type Said = Vec<(&'static str, Level, String)>;

/// What `f` says through an item callback.
fn said(f: impl FnOnce(&mut dyn FnMut(&'static str, Level, String))) -> Said {
    let mut out = Vec::new();
    f(&mut |check, level, message| out.push((check, level, message)));
    out
}

/// A drive's `file_system` item is one, and as good as what players make of the disk: fine when
/// every player reads it, a problem when none does, a warning otherwise; it says what they make
/// of it, with any notes after.
#[hegel::test(test_cases = 60)]
#[hegel::explicit_test_case(scheme = Some(Scheme::Mbr), fs = Some(FsKind::Fat32), name = "MS-DOS FAT32", volumes = 1usize)]
#[hegel::explicit_test_case(scheme = Some(Scheme::Gpt), fs = Some(FsKind::Ntfs), name = "NTFS", volumes = 2usize)]
#[hegel::explicit_test_case(scheme = Some(Scheme::Mbr), fs = Some(FsKind::Hfsplus), name = "Mac OS Extended (Case-sensitive)", volumes = 2usize)]
#[hegel::explicit_test_case(scheme = None::<Scheme>, fs = Some(FsKind::Exfat), name = "exFAT", volumes = 0usize)]
fn a_drives_file_system_item_says_what_players_make_of_it(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let scheme = tc.draw(gs::sampled_from(vec![
        None,
        Some(Scheme::Mbr),
        Some(Scheme::Gpt),
        Some(Scheme::Apm),
        Some(Scheme::Other),
    ]));
    let fs = tc.draw(gs::sampled_from(vec![
        None,
        Some(FsKind::Fat16),
        Some(FsKind::Fat32),
        Some(FsKind::Exfat),
        Some(FsKind::Hfsplus),
        Some(FsKind::Apfs),
        Some(FsKind::Ntfs),
    ]));
    let name = tc.draw(gs::sampled_from(vec!["MS-DOS FAT32", "Mac OS Extended (Case-sensitive)", "exFAT", "NTFS"]));
    let volumes = tc.draw(gs::integers::<usize>().max_value(2));
    let mut disk = crate::format::tests::sample_disk();
    disk.scheme = scheme;
    let template = disk.volumes[0].clone();
    disk.volumes = (0..volumes)
        .map(|n| crate::format::Volume {
            id: format!("disk4s{}", n + 1),
            filesystem: fs,
            filesystem_name: Some(name.into()),
            label: (n == 0).then(|| "DNA".into()),
            ..template.clone()
        })
        .collect();
    let items = said(|mut item| disk_items(&disk, &mut item));
    let [(check, level, message)] = items.as_slice() else { panic!("{items:?}") };
    assert_eq!(*check, "file_system");
    let c = players::assess_disk(&disk);
    let want = match c.verdict {
        Verdict::AllPlayers => Level::Info,
        Verdict::NoPlayers => Level::Problem,
        Verdict::SomePlayers | Verdict::Unknown => Level::Warning,
    };
    assert_eq!(*level, want, "{message}");
    assert!(message.starts_with(&c.summary), "{message}");
    assert_eq!(message.ends_with(')'), !c.notes.is_empty(), "{message}");
    for note in &c.notes {
        assert!(message.contains(note.as_str()), "{message}");
    }
    if volumes == 0 {
        assert_eq!(*level, Level::Problem);
    }
}

/// Files of 4 GB or more: none is fine; on their own they're a warning (a FAT32 drive couldn't
/// take them); with another FAT32 drive in the set they're a problem naming that drive. Long
/// lists name three, then how many more.
#[hegel::test(test_cases = 60)]
#[hegel::explicit_test_case(files = 5usize, fat32 = vec![true, true, true, true, true])]
#[hegel::explicit_test_case(files = 2usize, fat32 = vec![true, false])]
#[hegel::explicit_test_case(files = 0usize, fat32 = vec![false, true])]
fn large_files_are_a_problem_only_beside_a_fat32_drive(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let files = tc.draw(gs::integers::<usize>().max_value(5));
    // Drive A, then the others; whether each is FAT32.
    let fat32 = tc.draw(gs::vecs(gs::booleans()).min_size(1).max_size(5));
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let names: Vec<String> = (0..fat32.len()).map(|n| char::from(b'A' + n as u8).to_string()).collect();
    let large: Vec<doctor::LargeFile> = (0..files)
        .map(|n| doctor::LargeFile { path: format!("Contents/Set {n}.wav"), size: 5 << 30, track: None })
        .collect();
    let d = Drive { device: &device, name: "A".into(), side: None, dbs: None, fs: None, large };
    let on_fat32: Vec<&String> = names.iter().zip(&fat32).filter(|(_, f)| **f).map(|(n, _)| n).collect();
    let items = said(|mut item| large_files_item(&d, &on_fat32, &mut item));
    let [(check, level, message)] = items.as_slice() else { panic!("{items:?}") };
    assert_eq!(*check, "large_files");
    let others: Vec<&String> = on_fat32.iter().copied().filter(|n| *n != "A").collect();
    match (files, others.is_empty()) {
        (0, _) => assert_eq!((*level, message.as_str()), (Level::Info, "no files of 4 GB or more")),
        (n, true) => {
            assert_eq!(*level, Level::Warning);
            assert!(
                message.starts_with(&format!("{n} file(s) of 4 GB or more won't fit on a FAT32 drive")),
                "{message}"
            );
        }
        (n, false) => {
            assert_eq!(*level, Level::Problem);
            assert!(
                message.starts_with(&format!("{n} file(s) of 4 GB or more can't be copied to {}", others[0])),
                "{message}"
            );
            assert!(!message.contains("to A") && message.contains("(FAT32)"), "{message}");
            assert_eq!(
                message.contains(&format!("{} more", others.len().saturating_sub(3))),
                others.len() > 3,
                "{message}"
            );
        }
    }
    if files > 0 {
        let more = files.saturating_sub(3);
        assert_eq!(message.ends_with(&format!(" and {more} more")), more > 0, "{message}");
        assert!(message.contains("Contents/Set 0.wav"));
    }
}

/// The `audio` items group the files with problems by kind (each a problem but a length that
/// differs, which is a warning), one item and one group per kind found, every file in exactly
/// one group; with none found, one item says how many files were checked.
#[hegel::test(test_cases = 60)]
#[hegel::explicit_test_case(kinds = Vec::<u8>::new(), checked = 7usize)]
#[hegel::explicit_test_case(kinds = vec![0u8, 1, 2, 3, 3], checked = 5usize)]
fn audio_problems_are_grouped_by_kind(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let kinds: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(3)).max_size(6));
    let checked = tc.draw(gs::integers::<usize>().min_value(kinds.len()).max_value(kinds.len() + 5));
    let findings: Vec<probe::AudioFinding> = kinds
        .iter()
        .enumerate()
        .map(|(n, k)| probe::AudioFinding {
            track: TrackRef { id: n as u32, name: format!("Track {n}"), file_path: format!("/Contents/{n}.mp3") },
            problem: match k {
                0 => probe::AudioProblem::Unreadable { error: "no decoder".into() },
                1 => probe::AudioProblem::Truncated { ends_secs: 10.0, expected_secs: 20.0 },
                2 => probe::AudioProblem::Undecodable { error: "bad frame".into() },
                _ => probe::AudioProblem::LengthDiffers { file_secs: 10.0, expected_secs: 20.0 },
            },
        })
        .collect();
    let mut groups = Vec::new();
    let items = said(|mut item| groups = audio_items(checked, findings.clone(), &mut item));
    assert!(items.iter().all(|(c, ..)| *c == "audio"));
    let kinds_found: FxHashSet<u8> = kinds.iter().copied().collect();
    if kinds.is_empty() {
        assert!(groups.is_empty());
        assert_eq!(items, [("audio", Level::Info, format!("all {checked} audio file(s) open and decode to the end"))]);
        return;
    }
    assert_eq!(items.len(), kinds_found.len());
    assert_eq!(groups.len(), kinds_found.len());
    let mut seen = FxHashSet::default();
    for (g, (_, level, message)) in groups.iter().zip(&items) {
        assert_eq!((g.level, &g.message), (*level, message));
        assert_eq!(g.level == Level::Warning, g.kind == "length_differs", "{g:?}");
        assert!(g.files.iter().all(|f| f.problem.kind() == g.kind));
        assert!(message.starts_with(&format!("{} audio file(s) ", g.files.len())), "{message}");
        for f in &g.files {
            assert!(seen.insert(f.track.id));
        }
    }
    assert_eq!(seen.len(), findings.len());
}

/// Track audio as players see it: a row's own rate and depth win over the file's, which fill in
/// what the row leaves at 0; an `.m4a` is AAC or Apple Lossless by its file when that says, else
/// by its bit rate (AAC up to 320 kbps, Apple Lossless above 400, open in between); a file in no
/// format players list goes by its extension, or `extensionless`. A row needs its file opened
/// exactly when something it decides is left open.
#[hegel::test(test_cases = 200)]
#[hegel::explicit_test_case(path = "/Contents/a.m4a", bitrate = 900u32, rate = 0u32, depth = 0u16, probed = Some(Codec::Aac))]
#[hegel::explicit_test_case(path = "/Contents/a.m4a", bitrate = 401u32, rate = 44_100u32, depth = 16u16, probed = None::<Codec>)]
#[hegel::explicit_test_case(path = "/Contents/Music/readme", bitrate = 0u32, rate = 0u32, depth = 0u16, probed = None::<Codec>)]
fn a_tracks_audio_comes_from_its_row_then_its_file(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let path = tc.draw(gs::sampled_from(vec![
        "/Contents/a.m4a",
        "/Contents/a.M4A",
        "/Contents/a.flac",
        "/Contents/a.mp3",
        "/Contents/a.ogg",
        "/Contents/Music/readme",
        "/Contents/v1.0/readme",
    ]));
    let bitrate = tc.draw(gs::sampled_from(vec![0u32, 128, 320, 321, 400, 401, 900]));
    let rate = tc.draw(gs::sampled_from(vec![0u32, 44_100, 96_000]));
    let depth = tc.draw(gs::sampled_from(vec![0u16, 16, 24]));
    let probed = tc.draw(gs::optional(gs::sampled_from(Codec::ALL.to_vec())));
    let file = probe::Format { codec: probed, sample_rate: 48_000, bit_depth: 32, channels: 2, duration_secs: None };
    let t = row(path, bitrate, rate, depth);
    let by_rate = match bitrate {
        0 | 321..=400 => None,
        1..=320 => Some(Codec::Aac),
        _ => Some(Codec::Alac),
    };
    for opened in [None, Some(&file)] {
        let a = track_audio(&t, opened);
        let lower = path.to_lowercase();
        let want = if lower.ends_with(".m4a") {
            match opened.and_then(|f| f.codec).filter(|c| matches!(c, Codec::Aac | Codec::Alac)) {
                Some(c) => Some(Ok(c)),
                None => by_rate.map(Ok),
            }
        } else if lower.ends_with(".flac") {
            Some(Ok(Codec::Flac))
        } else if lower.ends_with(".mp3") {
            Some(Ok(Codec::Mp3))
        } else if lower.ends_with(".ogg") {
            Some(Err(".ogg".to_string()))
        } else {
            Some(Err("extensionless".to_string()))
        };
        assert_eq!(a.codec, want, "{path} {opened:?}");
        assert_eq!(a.rate, if rate > 0 { rate } else { opened.map_or(0, |f| f.sample_rate) });
        assert_eq!(a.depth, if depth > 0 { depth } else { opened.map_or(0, |f| f.bit_depth) });
    }
    let open = match Codec::from_path(path) {
        FromPath::Unknown => false,
        FromPath::AacOrAlac => by_rate.is_none() || rate == 0,
        FromPath::Codec(c) => rate == 0 || (c.lossless() && depth == 0),
    };
    assert_eq!(needs_probe(&t), open, "{path} {bitrate} {rate} {depth}");
}

/// Each model's `formats` items over a drive's tracks: every track whose format is known is
/// either played or in exactly one group, with the tracks of one format failing for one reason
/// together; a group says the rate or depth the model refuses exactly when that's the reason and
/// what it plays instead, and is a warning when it rests on an inferred spec; without groups, one
/// item says it plays them all. Every model is checked on every run.
#[hegel::test(test_cases = 40)]
#[hegel::explicit_test_case(tracks = vec![(2usize, 44_100u32, 32u16), (4, 192_000, 24), (4, 44_100, 16), (6, 0, 0), (7, 44_100, 16)])]
fn every_model_says_which_tracks_it_wont_load(tc: hegel::TestCase) {
    use hegel::generators as gs;
    const PATHS: [&str; 8] = ["a.mp3", "a.m4a", "a.wav", "a.aiff", "a.flac", "a.ogg", "a.m4a", "readme"];
    let tracks: Vec<(usize, u32, u16)> = tc.draw(
        gs::vecs(gs::tuples!(
            gs::integers::<usize>().max_value(PATHS.len() - 1),
            gs::sampled_from(vec![0u32, 44_100, 48_000, 96_000, 192_000]),
            gs::sampled_from(vec![0u16, 16, 24, 32])
        ))
        .max_size(6),
    );
    let audio: Vec<TrackAudio> = tracks
        .iter()
        .enumerate()
        .map(|(n, (p, rate, depth))| {
            // The second `.m4a` is one whose bit rate leaves it open.
            let bitrate = if *p == 6 { 360 } else { 256 };
            let mut t = row(&format!("/Contents/{n}/{}", PATHS[*p]), bitrate, *rate, *depth);
            t.id = n as u32;
            track_audio(&t, None)
        })
        .collect();
    let known = audio.iter().filter(|a| a.codec.is_some()).count();
    for m in MODELS.iter().filter(|m| !m.is_mixer()) {
        let mut groups = Vec::new();
        let items = said(|mut item| groups = format_items(m, &audio, &mut item));
        assert!(items.iter().all(|(c, ..)| *c == "formats"));
        let mut grouped = FxHashSet::default();
        for (g, (_, level, message)) in groups.iter().zip(&items) {
            assert_eq!((g.level, &g.message), (*level, message));
            assert!(message.contains(&format!("won't load on the {}", m.name)), "{message}");
            assert_eq!(g.level == Level::Warning, message.contains("probably"), "{message}");
            let rate = matches!(g.reason, Unplayable::SampleRate | Unplayable::SampleRateAndBitDepth);
            let depth = matches!(g.reason, Unplayable::BitDepth | Unplayable::SampleRateAndBitDepth);
            assert_eq!(g.sample_rate.is_some(), rate, "{g:?}");
            assert_eq!(g.bit_depth.is_some(), depth, "{g:?}");
            // What it refuses, then what it plays instead.
            let (refused, why) = message.split_once(" won't load on the ").unwrap();
            let why = why.split_once(": ").unwrap().1;
            assert_eq!(refused.contains(" kHz") && why.contains(" kHz"), rate, "{message}");
            assert_eq!(refused.contains("-bit") && why.contains("-bit"), depth, "{message}");
            if let Some(r) = g.sample_rate {
                assert!(refused.contains(&format!(" at {} kHz", khz_number(r))), "{message}");
            }
            if let Some(d) = g.bit_depth {
                assert!(
                    refused.ends_with(&format!("{d}-bit")) || refused.ends_with(&format!("{d}-bit probably")),
                    "{message}"
                );
            }
            if g.reason == Unplayable::Format {
                assert!(message.ends_with(&format!("doesn't play {}", g.format)), "{message}");
            }
            for t in &g.tracks {
                assert!(grouped.insert(t.id), "{} in two groups", t.id);
                let a = &audio[t.id as usize];
                match &a.codec {
                    Some(Ok(c)) => {
                        assert_eq!(c.name(), g.format);
                        assert_eq!(m.plays(*c, a.rate, a.depth).result, Err(g.reason));
                    }
                    Some(Err(ext)) => assert_eq!((ext, g.reason), (&g.format, Unplayable::Format)),
                    None => panic!("an open format is never held against a model"),
                }
            }
        }
        assert!(groups.windows(2).all(|w| (w[0].level, w[0].tracks.len()) >= (w[1].level, w[1].tracks.len())));
        for (n, a) in audio.iter().enumerate() {
            let plays = matches!(&a.codec, Some(Ok(c)) if m.plays(*c, a.rate, a.depth).result.is_ok());
            assert_eq!(grouped.contains(&(n as u32)), a.codec.is_some() && !plays, "{} {:?}", m.name, a.codec);
        }
        if groups.is_empty() {
            let [(_, level, message)] = items.as_slice() else { panic!("{items:?}") };
            assert_eq!(*level, Level::Info);
            assert!(message.contains(&format!("plays all {known} track(s)")), "{message}");
        }
    }
}

/// Numbers and lists as the report words them: kHz without trailing zeros, `a, b and c`, and
/// each file system by its own name.
#[hegel::test(test_cases = 60)]
#[hegel::explicit_test_case(words = Vec::<String>::new(), rate = 44_100u32)]
#[hegel::explicit_test_case(words = vec!["FLAC".to_string()], rate = 96_000u32)]
fn rates_and_lists_read_as_written(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let words: Vec<String> =
        tc.draw(gs::vecs(gs::sampled_from(vec!["FLAC".to_string(), "WAV".into(), "AIFF".into()])).max_size(4));
    let rate = tc.draw(gs::sampled_from(vec![32_000u32, 44_100, 48_000, 88_200, 96_000, 192_000]));
    let listed = and_list(&words);
    match words.as_slice() {
        [] => assert_eq!(listed, ""),
        [one] => assert_eq!(&listed, one),
        [init @ .., last] => {
            let (head, tail) = listed.rsplit_once(" and ").unwrap();
            assert_eq!(tail, last);
            assert_eq!(head.split(", ").collect::<Vec<_>>(), init.iter().map(String::as_str).collect::<Vec<_>>());
        }
    }
    let khz = khz_number(rate);
    assert_eq!(khz.parse::<f64>().unwrap() * 1000.0, f64::from(rate));
    assert!(!khz.ends_with('0') || !khz.contains('.'), "{khz}");
    let all = [
        FsKind::Fat12,
        FsKind::Fat16,
        FsKind::Fat32,
        FsKind::Exfat,
        FsKind::Hfsplus,
        FsKind::Apfs,
        FsKind::Ntfs,
        FsKind::Other,
    ];
    let names: FxHashSet<&str> = all.iter().map(|f| fs_name(*f)).collect();
    assert_eq!(names.len(), all.len());
    assert!(fs_name(FsKind::Fat32).contains("FAT32") && fs_name(FsKind::Exfat) == "exFAT");
}

/// A drive compared with the first one: the same library is fine; anything else is a problem
/// naming the first drive, saying which way it differs.
#[hegel::test(test_cases = 40)]
#[hegel::explicit_test_case(same_fp = true, overlap = 4usize, extra = 0usize)]
#[hegel::explicit_test_case(same_fp = false, overlap = 4usize, extra = 0usize)]
#[hegel::explicit_test_case(same_fp = false, overlap = 0usize, extra = 3usize)]
#[hegel::explicit_test_case(same_fp = false, overlap = 4usize, extra = 2usize)]
fn a_drive_compared_with_the_first_says_how_they_differ(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let same_fp = tc.draw(gs::booleans());
    // Tracks both have, and tracks only the second has.
    let overlap = tc.draw(gs::integers::<usize>().max_value(4));
    let extra = tc.draw(gs::integers::<usize>().max_value(3));
    let tracks = |range: std::ops::Range<usize>| -> Vec<Track> {
        range
            .map(|n| {
                let mut t = row(&format!("/Contents/{n}.mp3"), 320, 44_100, 16);
                t.id = n as u32 + 1;
                t
            })
            .collect()
    };
    let side = |label: &str, tracks: Vec<Track>, fp: u8| crate::compare::Side {
        label: label.into(),
        id: None,
        library_id: None,
        fingerprint: Some(crate::Hash::of(&[fp])),
        aliases: Vec::new(),
        lineage: Vec::new(),
        library: Library { tracks, ..Default::default() },
        sequence: None,
        exported_at: None,
        pending: false,
        mid_copy: false,
        packed_from: None,
    };
    let first = side("Main", tracks(0..overlap), 1);
    let mut second = tracks(0..overlap);
    second.extend(tracks(10..10 + extra));
    let other = side("Backup", second, if same_fp { 1 } else { 2 });
    let items = said(|mut item| same_library_item("Main", &first, "Backup", &other, &mut item));
    let [(check, level, message)] = items.as_slice() else { panic!("{items:?}") };
    assert_eq!(*check, "same_library");
    let relation = compare::compare(&first, &other).relation;
    assert_eq!(*level == Level::Info, relation == Relation::Identical, "{message}");
    assert!(message.contains("Main"), "{message}");
    let phrase = match relation {
        Relation::Identical => "same library as Main",
        Relation::LeftNewer => "behind Main",
        Relation::RightNewer => "newer than Main",
        Relation::Diverged => "Backup and Main have both changed",
        Relation::Unrelated => "holds a different library from Main",
        Relation::Undetermined => "differs from Main",
    };
    assert!(message.starts_with(phrase), "{relation:?}: {message}");
    if !matches!(relation, Relation::Identical | Relation::Unrelated) {
        assert!(message.contains(" track(s) and ") && message.contains(" playlist(s) differ"), "{message}");
    }
}

/// On a drive whose file system wasn't checked, every model is fine exactly when the drive has
/// the database it reads (a mixer reads none); otherwise it's told which database to add, as a
/// problem, or a warning saying "probably" when that spec is inferred. A name that's no model is
/// a warning. Every model and every database layout is checked on every run.
#[hegel::test(test_cases = 20)]
#[hegel::explicit_test_case(unknown = vec!["XYZ-9".to_string()])]
fn each_model_is_told_the_database_it_lacks(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let unknown: Vec<String> =
        tc.draw(gs::vecs(gs::sampled_from(vec!["XYZ-9".to_string(), "CDJ-1".into(), "".into()])).max_size(2));
    for (has_pdb, has_plus) in [(true, false), (false, true), (true, true)] {
        let layout = Layout { has_pdb, has_plus, volume: None };
        let mut names: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
        names.extend(unknown.iter().map(String::as_str));
        let (players, _) = verdicts(&names, &layout, &DriveAudio::default());
        assert_eq!(players.len(), names.len());
        for (name, (level, message)) in names.iter().zip(&players) {
            let Some(m) = equipment::find(name) else {
                assert_eq!(*level, Level::Warning);
                assert!(message.starts_with(&format!("{name}: not a model")), "{message}");
                continue;
            };
            let reads = m.is_mixer() || (m.export_pdb.value && has_pdb) || (m.onelibrary.value && has_plus);
            if reads {
                assert_eq!(
                    (*level, message.as_str()),
                    (Level::Info, format!("{}: has the database it reads (file system not checked)", m.name).as_str())
                );
                continue;
            }
            let spec = if m.onelibrary_only() { m.onelibrary } else { m.export_pdb };
            assert_eq!(*level, if spec.inferred { Level::Warning } else { Level::Problem }, "{message}");
            assert_eq!(message.contains(": probably "), spec.inferred, "{message}");
            let add = if m.onelibrary_only() { "--to onelibrary" } else { "--to device-library" };
            assert!(message.ends_with(&format!("`locallibrary convert {add}` adds it")), "{message}");
        }
    }
}

#[cfg(unix)]
use crate::format::tests::fake_tools;

/// An export on a drive that is a volume of its own has its file system judged by the disk it is
/// on, as players get on with it: FAT32 on MBR by every player, exFAT by some, nothing on a GUID
/// partition table. A drive that is only a folder is warned it wasn't judged.
#[cfg(unix)]
#[hegel::test(test_cases = 8)]
fn a_drives_file_system_is_judged_by_its_disk(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let d = Device::open(fake.demo_drive("DNA")).unwrap();
    let on_a_disk = tc.draw(gs::weighted_booleans(0.8));
    let (fs, mbr) = (tc.draw(gs::sampled_from(vec!["vfat", "exfat"])), tc.draw(gs::booleans()));
    if on_a_disk {
        fake.volume_at(d.root(), fs, mbr);
    }

    let report = check(&[d], None, &CheckOptions::default(), &Ctx::silent()).unwrap();
    let judged: Vec<(Level, &str)> =
        report.items.iter().filter(|i| i.check == "file_system").map(|i| (i.level, i.message.as_str())).collect();
    let [(level, message)] = judged[..] else { panic!("{judged:?}") };
    match (on_a_disk, mbr, fs) {
        (false, ..) => assert_eq!(
            (level, message),
            (Level::Warning, "not the root of a mounted volume, so its file system wasn't checked")
        ),
        (true, false, _) => {
            assert!(level == Level::Problem && message.starts_with("GUID partition table"), "{message}")
        }
        (true, true, "vfat") => assert_eq!((level, message), (Level::Info, "every player")),
        _ => assert!(level == Level::Warning && message.starts_with("exFAT: only"), "{message}"),
    }
}
