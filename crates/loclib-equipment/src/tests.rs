use super::*;

#[test]
fn names_are_found_however_they_are_spelled() {
    assert_eq!(find("cdj-2000nxs2").unwrap().name, "CDJ-2000NXS2");
    assert_eq!(find("CDJ 2000 NEXUS2").unwrap().name, "CDJ-2000NXS2");
    assert_eq!(find("opus quad").unwrap().name, "OPUS-QUAD");
    assert_eq!(find("CDJ-2000").unwrap().name, "CDJ-2000");
    assert!(find("CDJ-2000NXS3").is_none());
    let mut seen = std::collections::HashSet::new();
    assert!(MODELS.iter().all(|m| seen.insert(normalise(m.name))), "no two models normalise alike");
}

#[test]
fn every_player_reads_a_database_and_plays_mp3() {
    for m in MODELS.iter().filter(|m| !m.is_mixer()) {
        assert!(m.export_pdb.value || m.onelibrary.value, "{}", m.name);
        assert!(m.audio(Codec::Mp3).is_some(), "{}", m.name);
        let mut codecs: Vec<Codec> = m.audio.iter().map(|a| a.codec).collect();
        codecs.dedup();
        assert_eq!(codecs.len(), m.audio.len(), "{} lists a format twice", m.name);
    }
}

#[test]
fn what_plays_where() {
    let nxs2 = find("CDJ-2000NXS2").unwrap();
    assert_eq!(nxs2.plays(Codec::Flac, 96_000, 24).result, Ok(()));
    assert_eq!(nxs2.plays(Codec::Flac, 192_000, 24).result, Err(Unplayable::SampleRate));
    assert_eq!(nxs2.plays(Codec::Wav, 44_100, 32).result, Err(Unplayable::BitDepth));
    assert_eq!(nxs2.plays(Codec::Wav, 192_000, 32).result, Err(Unplayable::SampleRateAndBitDepth));
    // MP3s at 22.05 kHz are MPEG-2 Layer 3, which the NXS2 lists and the CDJ-3000 doesn't.
    assert_eq!(nxs2.plays(Codec::Mp3, 22_050, 16).result, Ok(()));
    let cdj3000 = find("CDJ-3000").unwrap();
    assert_eq!(cdj3000.plays(Codec::Mp3, 22_050, 16).result, Err(Unplayable::SampleRate));
    // Lossy formats' depths aren't held against them, and unknown values aren't either.
    assert_eq!(cdj3000.plays(Codec::Mp3, 44_100, 32).result, Ok(()));
    assert_eq!(cdj3000.plays(Codec::Flac, 0, 0).result, Ok(()));
    let nxs = find("CDJ-2000NXS").unwrap();
    assert_eq!(nxs.plays(Codec::Flac, 44_100, 16), Playback { result: Err(Unplayable::Format), inferred: false });
    assert_eq!(nxs.plays(Codec::Wav, 96_000, 24).result, Err(Unplayable::SampleRate));
    let omnis = find("OMNIS-DUO").unwrap();
    assert_eq!(omnis.plays(Codec::Flac, 96_000, 24).result, Err(Unplayable::SampleRate));
    let rx2 = find("XDJ-RX2").unwrap();
    assert_eq!(rx2.plays(Codec::Flac, 44_100, 16), Playback { result: Err(Unplayable::Format), inferred: true });
    assert!(find("XDJ-RX3").unwrap().plays(Codec::Aac, 32_000, 16).inferred);
}

#[test]
fn file_systems() {
    let reads = |name: &str, fs| find(name).unwrap().reads(fs);
    assert_eq!(reads("CDJ-2000NXS2", FsKind::Exfat), Some(false));
    assert_eq!(reads("CDJ-3000", FsKind::Exfat), None);
    assert_eq!(reads("CDJ-3000X", FsKind::Exfat), Some(true));
    assert_eq!(reads("CDJ-3000X", FsKind::Ntfs), Some(false));
    assert_eq!(reads("DJM-V10", FsKind::Hfsplus), Some(false));
    assert_eq!(reads("XDJ-700", FsKind::Hfsplus), Some(true));
    assert_eq!(reads("DJM-V10", FsKind::Fat16), Some(false), "settings from FAT32 only");
    assert_eq!(reads("DJM-A9", FsKind::Fat32), Some(true));
    assert_eq!(reads("CDJ-2000NXS2", FsKind::Fat16), Some(true));
}

#[test]
fn canonical_names() {
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        canonical(&names(&["cdj-3000x", "CDJ 2000 NEXUS2", "CDJ-3000X"])).unwrap(),
        ["CDJ-3000X", "CDJ-2000NXS2"]
    );
    let e = canonical(&names(&["CDJ-3000", "XYZ-9", " ", "ABC-1"])).unwrap_err();
    assert_eq!(e, UnknownModels(names(&["XYZ-9", "ABC-1"])));
    let e = e.to_string();
    assert!(
        e.starts_with("not a known model: XYZ-9, ABC-1 (known models: CDJ-3000X, ") && e.contains("OPUS-QUAD"),
        "{e}"
    );
}

#[test]
fn formats_from_file_names() {
    assert_eq!(Codec::from_path("/Contents/a/b.MP3"), FromPath::Codec(Codec::Mp3));
    assert_eq!(Codec::from_path("/Contents/a/b.m4a"), FromPath::AacOrAlac);
    assert_eq!(Codec::from_path("/Contents/a/b.fla"), FromPath::Codec(Codec::Flac));
    assert_eq!(Codec::from_path("/Contents/a/b.ogg"), FromPath::Unknown);
    assert_eq!(Codec::from_path("/Contents/a/b"), FromPath::Unknown);
}
