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
    assert_eq!(levels(&players(&all, &drive(FsKind::Fat32, true))), [Level::Ok; 4]);
    let exfat = players(&all, &drive(FsKind::Exfat, true));
    assert_eq!(levels(&exfat), [Level::Problem, Level::Warning, Level::Ok, Level::Problem]);
    assert!(exfat[1].1.contains("firmware 1.20+"), "{exfat:?}");
    assert_eq!(
        levels(&players(&all, &drive(FsKind::Hfsplus, false))),
        [Level::Ok, Level::Ok, Level::Problem, Level::Problem]
    );
    let gpt = Layout { has_pdb: true, has_plus: true, volume: Some((Some(Scheme::Gpt), Some(FsKind::Fat32))) };
    assert_eq!(levels(&players(&["CDJ-3000"], &gpt)), [Level::Problem]);
    let unknown = Layout { has_pdb: true, has_plus: false, volume: None };
    assert_eq!(
        levels(&players(&["CDJ-2000NXS2", "OPUS-QUAD", "XYZ-9"], &unknown)),
        [Level::Ok, Level::Problem, Level::Warning]
    );
    // A drive the CDJ-3000 can't read but players reading only OneLibrary can.
    let only_plus = Layout { has_pdb: false, has_plus: true, volume: None };
    assert_eq!(levels(&players(&["CDJ-3000", "OMNIS-DUO"], &only_plus)), [Level::Problem, Level::Ok]);
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
    Track {
        id: 1,
        title: path.into(),
        artist: String::new(),
        album: String::new(),
        album_artist: String::new(),
        genre: String::new(),
        label: String::new(),
        key: String::new(),
        remixer: String::new(),
        original_artist: String::new(),
        composer: String::new(),
        comment: String::new(),
        mix_name: String::new(),
        isrc: String::new(),
        bpm: 0.0,
        duration_secs: 0,
        year: 0,
        track_number: 0,
        disc_number: 0,
        rating: 0,
        color: None,
        play_count: 0,
        bitrate,
        sample_rate: rate,
        sample_depth: depth,
        file_size: 0,
        file_path: path.into(),
        analyze_path: String::new(),
        date_added: String::new(),
        release_date: String::new(),
        artwork_id: None,
        artwork_path: None,
    }
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
        [(Level::Ok, "CDJ-3000: plays all 1 track(s) (format, sample rate and bit depth)".to_string())]
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
    assert_eq!(verdicts(&["CDJ-2000"], &layout, &mp3).1, [(Level::Ok, message.to_string())]);
}
