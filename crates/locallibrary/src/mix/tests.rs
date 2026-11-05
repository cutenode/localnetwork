use super::*;

/// A mix given by a path relative to the current folder is kept by its absolute path: its
/// folder is where a cover and a project's audio are looked for, and a kept report finds it
/// again from anywhere.
#[cfg(unix)]
#[test]
fn mixes_are_kept_by_their_absolute_path() {
    let dir = tempfile::tempdir().unwrap();
    let set = dir.path().join("set.wav");
    std::fs::write(&set, crate::probe::tests::wav(8_000, 16, 0.5)).unwrap();
    let up = std::env::current_dir().unwrap().components().count() - 1;
    let relative = std::iter::repeat_n(Path::new(".."), up).collect::<PathBuf>().join(set.strip_prefix("/").unwrap());
    let (info, _) = listen(&relative, &Ctx::silent()).unwrap();
    assert!(info.path.is_absolute(), "{}", info.path.display());
}

/// A recording that's on the drive too (a set imported into rekordbox) isn't one of the
/// tracks heard in it: it would match the whole mix and swallow them.
#[test]
fn a_recording_on_the_drive_is_left_out_of_its_own_tracks() {
    use crate::mix::fingerprint::RATE;
    use crate::probe::tests::wav_of;
    use loclib_mixid::test_util::synth;
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    let (a, b) = (synth(40.0, 7919), synth(40.0, 3 * 7919));
    let set = [&a[..], &b[..]].concat();
    for (name, x) in [("a.wav", &a), ("b.wav", &b), ("set.wav", &set)] {
        let pcm = wav_of(RATE, 16, x.iter().map(|s| (s * 12_000.0) as i32));
        std::fs::write(root.join("Contents").join(name), pcm).unwrap();
    }
    let tracks = vec![
        track(1, "A", "One", "/Contents/a.wav"),
        track(2, "B", "Two", "/Contents/b.wav"),
        track(3, "DJ", "Live", "/Contents/set.wav"),
    ];
    let library = Arc::new(Library { tracks, ..Default::default() });
    let device = Device::open(&root).unwrap();
    let cache = Cache::new(dir.path().join("cache"));
    let id = Identifier::with_library(&device, library, &cache, &MixPrefs::default(), &Ctx::silent()).unwrap();
    let r = id.identify(&root.join("Contents/set.wav"), &Options { tempo_range: 0.02 }, &Ctx::silent()).unwrap();
    assert_eq!(r.plays.iter().map(|p| p.track.id).collect::<Vec<_>>(), [1, 2]);
}

/// A track the pitch fader was ridden through (+0% for 45 s, then +4%, pitch and all) gives the
/// slowest and fastest speeds it was played at; one played at +2% throughout gives none.
#[test]
fn a_ridden_pitch_fader_gives_the_speeds_it_went_between() {
    use crate::mix::fingerprint::RATE;
    use crate::probe::tests::wav_of;
    use loclib_mixid::test_util::{resample, synth};
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    let (a, b) = (synth(90.0, 7919), synth(90.0, 3 * 7919));
    let at = |x: &[f32], speed: f64| resample(x, (f64::from(RATE) * speed).round() as u32, RATE);
    let half = a.len() / 2;
    let set = [at(&a[..half], 1.0), at(&a[half..], 1.04), at(&b, 1.02)].concat();
    let pcm = |x: &[f32]| wav_of(RATE, 16, x.iter().map(|s| (s * 12_000.0) as i32));
    for (name, x) in [("a.wav", &a), ("b.wav", &b)] {
        std::fs::write(root.join("Contents").join(name), pcm(x)).unwrap();
    }
    std::fs::write(dir.path().join("set.wav"), pcm(&set)).unwrap();
    let mut tracks = vec![track(1, "A", "One", "/Contents/a.wav"), track(2, "B", "Two", "/Contents/b.wav")];
    tracks.iter_mut().for_each(|t| t.bpm = 120.0);
    let library = Arc::new(Library { tracks, ..Default::default() });
    let device = Device::open(&root).unwrap();
    let id = Identifier::with_library(
        &device,
        library,
        &Cache::new(dir.path().join("cache")),
        &MixPrefs::default(),
        &Ctx::silent(),
    )
    .unwrap();
    let r = id.identify(&dir.path().join("set.wav"), &Options { tempo_range: 0.08 }, &Ctx::silent()).unwrap();
    let plays: Vec<(u32, Option<(f64, f64)>)> = r.plays.iter().map(|p| (p.track.id, p.speed_range)).collect();
    let [(1, Some((slowest, fastest))), (2, None)] = plays[..] else { panic!("{plays:?}") };
    assert!((slowest - 1.0).abs() < 0.002 && (fastest - 1.04).abs() < 0.002, "{plays:?}");
    assert!((slowest..=fastest).contains(&r.plays[0].tempo), "{plays:?}");
    // Along the ride: 45 s into the track when the fader moved, played at +0% before it and +4%
    // after. At the average speed it would be most of a second out there.
    let p = &r.plays[0];
    assert!((p.track_secs(45.0) - 45.0).abs() < 0.25, "{:?}", p.lines);
    let bpm = |t: f64| p.bpm_at(t).unwrap();
    assert!((bpm(20.0) - 120.0).abs() < 0.25 && (bpm(70.0) - 124.8).abs() < 0.25, "{} then {}", bpm(20.0), bpm(70.0));
    // The next track comes in at +2%, down from +4% then.
    let change = r.transitions[0].tempo_change.unwrap();
    assert!((change - (1.02 / 1.04 - 1.0) * 100.0).abs() < 0.25, "{change}");
}

/// A play's matcher [`Found`] from `start` (mix seconds), along `fits`.
fn found(start: f64, fits: Vec<matcher::Fit>) -> Found {
    Found {
        slot: 0,
        start,
        end: start,
        track_start: 0.0,
        track_end: 0.0,
        tempo: 1.0,
        key_lock: None,
        votes: 0,
        strongest: 0,
        heard: 0,
        spanned: 1,
        presence: Vec::new(),
        jumps: 0,
        alternates: Vec::new(),
        fits,
    }
}

/// A play is where in the track the matcher's nearest line to each moment puts it (the first of
/// two as near), as the matcher traced it: across a gap between lines, an overlap, a line inside
/// another and the fades either side.
#[hegel::test(test_cases = 300)]
fn a_play_follows_the_nearest_of_its_lines(tc: TestCase) {
    use loclib_model::test_util::track;
    let secs = || gs::floats::<f64>().min_value(0.0).max_value(600.0);
    let mut fits: Vec<matcher::Fit> = tc
        .draw(
            gs::vecs(gs::tuples!(secs(), gs::floats::<f64>().min_value(0.0).max_value(120.0), secs()))
                .min_size(1)
                .max_size(5),
        )
        .into_iter()
        .map(|(start, long, at)| {
            let ratio = tc.draw(gs::floats::<f64>().min_value(0.84).max_value(1.16));
            let (start, end) = (start * FRAMES_PER_SEC, (start + long) * FRAMES_PER_SEC);
            matcher::Fit { ratio, intercept: at * FRAMES_PER_SEC - ratio * start, start, end, measured: true }
        })
        .collect();
    fits.sort_by(|a, b| a.start.total_cmp(&b.start));
    let start = fits[0].start / FRAMES_PER_SEC - tc.draw(gs::floats::<f64>().min_value(0.0).max_value(10.0));
    let f = found(start, fits);
    let p = Play { lines: play_lines(&f), ..report::tests::play(track(1, "A", "One", "/Contents/one.wav")) };
    let t = tc.draw(gs::floats::<f64>().min_value(start).max_value(1400.0));
    let frame = t * FRAMES_PER_SEC;
    let off = |l: &matcher::Fit| (l.start - frame).max(frame - l.end).max(0.0);
    let mut near: Vec<&matcher::Fit> = f.fits.iter().collect();
    near.sort_by(|a, b| off(a).total_cmp(&off(b)));
    // Two as near (to rounding) may be told apart either way.
    let tied = near.get(1).is_some_and(|l| off(l) - off(near[0]) < 1e-6);
    let expect = near[0].track_frame(frame) / FRAMES_PER_SEC;
    assert!(tied || (p.track_secs(t) - expect).abs() < 1e-6, "{t}: {} not {expect}: {:?}", p.track_secs(t), p.lines);
    assert!(
        p.lines.windows(2).all(|w| w[0].start <= w[1].start) && (p.lines[0].start - start).abs() < 1e-9,
        "{:?}",
        p.lines
    );
}

#[test]
fn recorder_tags_are_not_a_title() {
    let mut info = MixInfo {
        path: PathBuf::from("/mixes/half-savage and hardy, and free/dubstep mix one original.WAV"),
        kind: "audio",
        title: String::new(),
        artist: None,
        genre: None,
        date: None,
        recorder: None,
        duration_secs: 0.0,
        sample_rate: 0,
        channels: 0,
        labels: Vec::new(),
        cover: None,
    };
    let tags = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    describe(&mut info, &tags(&[("TITLE", "REC046"), ("ARTIST", "PIONEER DJ REC"), ("GENRE", "XDJ-RX2")]));
    assert_eq!(info.title, "half-savage and hardy, and free");
    assert_eq!(info.recorder.as_deref(), Some("Pioneer DJ XDJ-RX2"));
    assert_eq!(info.artist, None);

    describe(&mut info, &tags(&[("TITLE", "Live at the Warehouse"), ("ARTIST", "Example DJ")]));
    assert_eq!(info.title, "Live at the Warehouse");
    assert_eq!(info.artist.as_deref(), Some("Example DJ"));
}

/// Half a second of a recording, at `path`.
fn recording(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, crate::probe::tests::wav(8_000, 16, 0.5)).unwrap();
}

/// Tags an editor wrote and an embedded cover describe the mix, and the cover is one to
/// share.
#[test]
fn tags_and_an_embedded_cover_describe_the_mix() {
    use lofty::config::WriteOptions;
    let dir = tempfile::tempdir().unwrap();
    let set = dir.path().join("REC001.wav");
    recording(&set);
    let png = dir.path().join("art.png");
    image::RgbImage::from_fn(64, 64, |x, _| image::Rgb([x as u8 * 4, 0, 0])).save(&png).unwrap();
    crate::artwork::embed_in_file(&set, &crate::artwork::Artwork::from_path(&png).unwrap()).unwrap();
    let mut tagged = lofty::read_from_path(&set).unwrap();
    let tag = tagged.primary_tag_mut().unwrap();
    tag.set_title("Friday".into());
    tag.set_artist("Example DJ".into());
    tag.set_genre("Techno".into());
    tag.insert_text(ItemKey::RecordingDate, "2026-10-03".into());
    tag.save_to_path(&set, WriteOptions::default()).unwrap();

    let (info, _) = listen(&set, &Ctx::silent()).unwrap();
    let described = (info.title.as_str(), info.artist.as_deref(), info.genre.as_deref(), info.date.as_deref());
    assert_eq!(described, ("Friday", Some("Example DJ"), Some("Techno"), Some("2026-10-03")));
    assert!(matches!(&info.cover, Some(CoverSource::Embedded(p)) if *p == set), "{:?}", info.cover);
    let shared = info.shared_cover().unwrap();
    assert_eq!(image::load_from_memory(&shared).unwrap().width(), crate::artwork::THUMB_SIZE);
}

/// An image next to the mix is its cover when it's named like the mix, then like its folder,
/// then a generic cover name; it's a guess, so a shared report leaves it out.
#[test]
fn a_cover_next_to_the_mix_is_found_by_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Live at Fabric");
    let set = folder.join("set.wav");
    recording(&set);
    let cover = || match listen(&set, &Ctx::silent()).unwrap().0.cover {
        Some(CoverSource::File(p)) => p.file_name().unwrap().to_string_lossy().into_owned(),
        other => panic!("{other:?}"),
    };
    for name in ["notes.png", "cover.JPG", "set.txt"] {
        std::fs::write(folder.join(name), name).unwrap();
    }
    assert_eq!(cover(), "cover.JPG");
    std::fs::write(folder.join("fabric.png"), b"x").unwrap();
    assert_eq!(cover(), "fabric.png");
    std::fs::write(folder.join("set.webp"), b"x").unwrap();
    assert_eq!(cover(), "set.webp");

    let (info, _) = listen(&set, &Ctx::silent()).unwrap();
    assert_eq!(info.shared_cover(), None);
    assert_eq!(info.cover.unwrap().read().unwrap(), b"x");
    assert!(CoverSource::File(folder.join("gone.png")).read().is_err());
    let err = CoverSource::Embedded(set.clone()).read().unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err}");
    assert!(matches!(listen(&folder.join("gone.wav"), &Ctx::silent()), Err(Error::NotFound(_))));
}

/// An Audacity project is listened to as the mix it renders, with its labels and tags.
#[test]
fn a_project_is_listened_to_as_its_mix() {
    use super::aup::tests::{Xml, floats, project};
    let dir = tempfile::tempdir().unwrap();
    let mut x = Xml::new(4);
    x.start("project").double("rate", 8_000.0);
    x.start("tags").start("tag").string("name", "TITLE").string("value", "Warehouse").end("tag").end("tags");
    x.start("labeltrack").start("label").double("t", 1.0).string("title", "drop").end("label").end("labeltrack");
    x.start("wavetrack").clip(0.0, 8_000, &[(0, 1)]);
    let tone: Vec<f32> = (0..8_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    // 0x0004_000F: Audacity's 32-bit float samples.
    let path = project(dir.path(), "project", Some(&x), &[(1, 0x0004_000F, floats(&tone))]);
    let (info, heard) = listen(&path, &Ctx::silent()).unwrap();
    assert_eq!((info.kind, info.title.as_str(), info.duration_secs), ("audacity", "Warehouse", 1.0));
    assert_eq!(info.labels, [Label { start: 1.0, end: 1.0, title: "drop".into() }]);
    assert!(heard.frames > 0);
}

/// Recordings are found by their names alone, in every folder but hidden ones.
#[test]
fn recordings_are_found_in_every_folder_but_hidden_ones() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join(".mixes");
    for f in ["b.MP3", "a.wav", "notes.txt", "old/set.aup3", ".hidden/d.wav", "old/.e.flac"] {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"not audio, but only the name is read").unwrap();
    }
    let found: Vec<PathBuf> = recordings_in(&root).iter().map(|p| p.strip_prefix(&root).unwrap().into()).collect();
    assert_eq!(found, [PathBuf::from("a.wav"), "b.MP3".into(), Path::new("old").join("set.aup3")]);
}

/// A drive is matched by its identity, or by its name when it has none.
#[test]
fn a_report_knows_the_drive_it_was_matched_against() {
    let dir = tempfile::tempdir().unwrap();
    let mut device = Device::open(dir.path()).unwrap();
    let mut r = crate::mix::report::tests::report("Friday");
    r.drive = device.name();
    assert!(r.matched_against(&device));
    r.drive = "Other".into();
    assert!(!r.matched_against(&device));
    let id = device.adopt(Some("Other")).unwrap().id;
    r.drive_id = Some(Uuid::new_v4());
    assert!(!r.matched_against(&device), "the same name, another identity");
    r.drive_id = Some(id);
    assert!(r.matched_against(&device));
}

/// What the matcher found becomes a play: the key heard moves with the speed unless key lock
/// was on, and the other tracks matching the same stretch are named.
#[test]
fn plays_hear_the_key_moved_by_the_speed() {
    use loclib_model::test_util::track;
    let mut t = track(1, "A", "One", "/Contents/one.wav");
    (t.key, t.bpm) = ("Am".into(), 125.0);
    // Unanalysed: no key or tempo.
    let mut copy = track(2, "A", "One (copy)", "/Contents/copy.wav");
    copy.bpm = 0.0;
    let tracks = vec![t, copy];
    let print = || index::TrackPrint { duration_secs: 60.0, peaks: Vec::new() };
    let id = Identifier {
        drive: "DNA".into(),
        drive_id: None,
        library: Arc::new(Library { tracks, ..Default::default() }),
        report: IndexReport::default(),
        index: LibraryIndex::build(vec![(1, print()), (2, print())]),
        slots: FxHashMap::default(),
        cache: Default::default(),
    };
    let semitone_up = 2f64.powf(1.0 / 12.0);
    let found = |slot, key_lock| Found {
        slot,
        start: 10.0,
        end: 70.0,
        track_start: 0.0,
        track_end: 60.0,
        tempo: semitone_up,
        key_lock,
        votes: 600,
        strongest: 400,
        heard: 30,
        spanned: 30,
        presence: vec![1.0],
        jumps: 0,
        alternates: vec![(1, true), (7, false)],
        fits: Vec::new(),
    };
    let p = id.play(&found(0, Some(false))).unwrap();
    assert_eq!((p.key.as_deref(), p.played_key.as_deref()), (Some("8A"), Some("3A")));
    assert!((p.semitones - 1.0).abs() < 1e-9 && (p.played_bpm.unwrap() - 125.0 * semitone_up).abs() < 1e-9);
    let alternates: Vec<(u32, &str, bool)> =
        p.alternates.iter().map(|a| (a.track_id, a.file_path.as_str(), a.same_audio)).collect();
    assert_eq!(alternates, [(2, "/Contents/copy.wav", true)], "slot 7 isn't in the index");
    let locked = id.play(&found(0, Some(true))).unwrap();
    assert_eq!((locked.semitones, locked.played_key.as_deref()), (0.0, Some("8A")));
    let keyless = id.play(&found(1, None)).unwrap();
    assert_eq!((keyless.key, keyless.played_bpm), (None, None));
    assert!(id.play(&found(5, None)).is_none());
}

/// Each transition says how the keys heard relate and how the tempo moved.
#[test]
fn transitions_say_how_keys_and_tempos_move() {
    use loclib_model::test_util::track;
    let play = |start: f64, end: f64, key: Option<&str>, bpm: Option<f64>| Play {
        start,
        end,
        track_end: end - start,
        played_bpm: bpm,
        key: key.map(str::to_string),
        played_key: key.map(str::to_string),
        ..report::tests::play(track(1, "A", "One", "/Contents/one.wav"))
    };
    let plays = [
        play(0.0, 130.0, Some("8A"), Some(125.0)),
        play(120.0, 250.0, Some("10A"), Some(128.0)),
        play(260.0, 400.0, Some("3A"), None),
        play(390.0, 500.0, None, Some(126.0)),
    ];
    let t = transitions(&plays);
    let said: Vec<(f64, f64, Option<&str>, Option<f64>)> =
        t.iter().map(|t| (t.at, t.overlap, t.harmony.as_deref(), t.tempo_change)).collect();
    assert_eq!(
        said,
        [
            (120.0, 10.0, Some("energy boost"), Some(2.4)),
            (260.0, -10.0, Some("clash"), None),
            (390.0, 10.0, None, None),
        ]
    );
    assert_eq!(t.iter().map(|t| (t.from, t.to)).collect::<Vec<_>>(), [(0, 1), (1, 2), (2, 3)]);
}

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_model::test_util::keys;

/// The mix's waveform has a column every quarter second: loclib-mixid's columns of its levels
/// at that rate (whose pooling and scaling its tests cover).
#[hegel::test(test_cases = 50)]
fn the_mix_waveform_has_a_column_every_quarter_second(tc: TestCase) {
    let level = || gs::floats::<f32>().min_value(0.0).max_value(1.0);
    let frames: Vec<(f32, [f32; 3])> =
        tc.draw(gs::vecs(gs::tuples!(level(), gs::arrays::<_, _, 3>(level()))).max_size(400));
    let levels: Vec<FrameLevels> = frames.iter().map(|&(peak, bands)| FrameLevels { peak, bands }).collect();
    let w = waveform(&levels);
    let c = fingerprint::columns(&levels, 4.0);
    assert_eq!(w.columns_per_sec, 4.0);
    assert_eq!((w.peak, [w.low, w.mid, w.high]), (c.peak, c.bands));
}

/// One transition between each play and the next: when the next comes in, how long both
/// are heard, how the keys heard relate and by how much the tempo moves then (a ridden track's
/// at the speed it was at, not its average).
#[hegel::test(test_cases = 200)]
fn transitions_follow_the_plays(tc: TestCase) {
    use loclib_model::test_util::track;
    let n = tc.draw(gs::integers::<usize>().max_value(6));
    let plays: Vec<Play> = (0..n)
        .map(|i| {
            let start = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(3600.0));
            let end = start + tc.draw(gs::floats::<f64>().min_value(1.0).max_value(600.0));
            let mut p = Play {
                start,
                end,
                played_key: tc.draw(gs::optional(keys().map(Camelot::code))),
                played_bpm: tc.draw(gs::optional(gs::floats::<f64>().min_value(60.0).max_value(200.0))),
                ..report::tests::play(track(i as u32, "A", "One", "/Contents/one.wav"))
            };
            // A ride: lines at speeds within it, and one searched at, outside it.
            if let Some(bpm) = p.played_bpm.filter(|_| tc.draw(gs::booleans())) {
                let speed = || gs::floats::<f64>().min_value(0.9).max_value(1.1);
                let (a, b) = (tc.draw(speed()), tc.draw(speed()));
                let lines = tc.draw(
                    gs::vecs(gs::tuples!(gs::floats::<f64>().min_value(start).max_value(end), speed()))
                        .min_size(1)
                        .max_size(4),
                );
                let mut lines: Vec<PlayLine> =
                    lines.into_iter().map(|(start, tempo)| PlayLine { start, track_start: 0.0, tempo }).collect();
                lines.sort_by(|x, y| x.start.total_cmp(&y.start));
                lines[0].start = start;
                (p.speed_range, p.lines, p.track.bpm) = (Some((a.min(b), a.max(b))), lines, bpm);
            }
            p
        })
        .collect();
    // The BPM `t` into the mix: a ride's line's then (the last to start by then), within the
    // ride's speeds.
    let bpm_at = |p: &Play, t: f64| match p.speed_range {
        Some((slowest, fastest)) => {
            let line = p.lines.iter().rfind(|l| l.start <= t).unwrap_or(&p.lines[0]);
            p.played_bpm.map(|_| p.track.bpm * line.tempo.clamp(slowest, fastest))
        }
        None => p.played_bpm,
    };
    let t = transitions(&plays);
    assert_eq!(t.len(), n.saturating_sub(1));
    for (i, t) in t.iter().enumerate() {
        let (a, b) = (&plays[i], &plays[i + 1]);
        assert_eq!((t.from, t.to, t.at, t.overlap), (i, i + 1, b.start, a.end - b.start));
        let keys =
            a.played_key.as_deref().and_then(Camelot::parse).zip(b.played_key.as_deref().and_then(Camelot::parse));
        match (keys, t.harmony.as_deref()) {
            (None, None) => {}
            (Some((x, y)), Some(h)) => assert_eq!(h, x.relation_with_boost(y).map_or("clash", |r| r.describe())),
            other => panic!("{other:?}"),
        }
        let ride = |p: &Play| p.bpm_range().zip(bpm_at(p, b.start));
        for p in [a, b] {
            let in_range = ride(p).is_none_or(|((lo, hi), bpm)| (lo..=hi).contains(&bpm));
            assert!(in_range && p.bpm_at(b.start) == bpm_at(p, b.start), "{p:?}");
        }
        match (bpm_at(a, b.start).zip(bpm_at(b, b.start)), t.tempo_change) {
            (None, None) => {}
            (Some((x, y)), Some(change)) => {
                assert!((change - (y / x - 1.0) * 100.0).abs() <= 0.05 + 1e-9, "{x} to {y}: {change}");
            }
            other => panic!("{other:?}"),
        }
    }
}

/// What the matcher found becomes a play of the library's track in that slot (none past the
/// index): played at its speed, its key heard a semitone up or down per semitone of speed
/// without key lock and as it is with it, and only the alternates the index has named.
#[hegel::test(test_cases = 30)]
fn plays_carry_the_track_at_its_speed(tc: TestCase) {
    use loclib_model::test_util::track;
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let tracks: Vec<Track> = (0..n)
        .map(|i| {
            let mut t = track(i as u32 + 1, "A", &format!("Track {i}"), &format!("/Contents/{i}.wav"));
            t.key = if tc.draw(gs::weighted_booleans(0.8)) { tc.draw(keys()).note() } else { String::new() };
            t.bpm = tc.draw(hegel::one_of!(gs::just(0.0), gs::floats::<f64>().min_value(60.0).max_value(180.0)));
            t
        })
        .collect();
    let print = || index::TrackPrint { duration_secs: 60.0, peaks: Vec::new() };
    let id = Identifier {
        drive: "DNA".into(),
        drive_id: None,
        library: Arc::new(Library { tracks: tracks.clone(), ..Default::default() }),
        report: IndexReport::default(),
        index: LibraryIndex::build(tracks.iter().map(|t| (t.id, print())).collect()),
        slots: FxHashMap::default(),
        cache: Default::default(),
    };
    let slot = tc.draw(gs::integers::<u32>().max_value(n as u32 + 1));
    let tempo = tc.draw(gs::floats::<f64>().min_value(0.84).max_value(1.16));
    let key_lock = tc.draw(gs::optional(gs::booleans()));
    let spanned = tc.draw(gs::integers::<u32>().min_value(1).max_value(100));
    let f = Found {
        slot,
        start: 10.0,
        end: 70.0,
        track_start: 0.0,
        track_end: 60.0,
        tempo,
        key_lock,
        votes: tc.draw(gs::integers::<u32>().max_value(5000)),
        strongest: 0,
        heard: tc.draw(gs::integers::<u32>().max_value(spanned)),
        spanned,
        presence: Vec::new(),
        jumps: 0,
        alternates: tc
            .draw(gs::vecs(gs::tuples!(gs::integers::<u32>().max_value(n as u32 + 1), gs::booleans())).max_size(3)),
        fits: tc
            .draw(
                gs::vecs(gs::tuples!(gs::floats::<f64>().min_value(0.84).max_value(1.16), gs::booleans())).max_size(4),
            )
            .into_iter()
            .map(|(ratio, measured)| matcher::Fit { ratio, intercept: 0.0, start: 0.0, end: 1.0, measured })
            .collect(),
    };
    let Some(p) = id.play(&f) else { return assert!(slot as usize >= n) };
    let t = &tracks[slot as usize];
    assert_eq!(p.track.id, t.id);
    assert_eq!(p.played_bpm, (t.bpm > 0.0).then_some(t.bpm * tempo));
    assert_eq!(p.semitones, f.semitones());
    assert!((0.0..=1.0).contains(&p.confidence) && (p.confidence - f.confidence()).abs() <= 0.005);
    match Camelot::parse(&t.key) {
        None => assert_eq!((p.key, p.played_key), (None, None)),
        Some(key) => {
            assert_eq!(p.key, Some(key.code()));
            let heard = Camelot::parse(p.played_key.as_deref().unwrap()).unwrap();
            let moved = if key_lock == Some(false) { p.semitones.round() as i32 } else { 0 };
            assert_eq!(heard.major, key.major);
            assert_eq!((heard.pitch() as i32 - key.pitch() as i32).rem_euclid(12), moved.rem_euclid(12), "{p:?}");
        }
    }
    let named: Vec<(u32, bool)> = f.alternates.iter().filter(|a| (a.0 as usize) < n).map(|a| (a.0 + 1, a.1)).collect();
    assert_eq!(p.alternates.iter().map(|a| (a.track_id, a.same_audio)).collect::<Vec<_>>(), named);
    // The speeds a ridden pitch fader went between: of measured lines only, and only when they
    // differ by a ride's worth.
    let measured: Vec<f64> = f.fits.iter().filter(|f| f.measured).map(|f| f.ratio).collect();
    match p.speed_range {
        None => {
            assert!(measured.iter().all(|r| measured.iter().all(|s| (r - s).abs() < matcher::RIDE)), "{measured:?}")
        }
        Some((slowest, fastest)) => {
            assert!(fastest - slowest >= matcher::RIDE && measured.contains(&slowest) && measured.contains(&fastest));
            assert!(measured.iter().all(|r| (slowest..=fastest).contains(r)), "{measured:?}");
        }
    }
}

/// Tags describe the mix, but a Pioneer DJ recorder's (`REC046`, `PIONEER DJ REC`, its
/// model) only say what recorded it: the title comes from the folder it was filed in, or
/// from the file's own name when a title is missing or a recorder's.
#[hegel::test(test_cases = 300)]
fn tags_describe_the_mix_unless_a_recorder_wrote_them(tc: TestCase) {
    let recorder = tc.draw(gs::booleans());
    let artist = match recorder {
        true => tc.draw(gs::sampled_from(vec!["PIONEER DJ REC", "Pioneer DJ Rec", "pioneer dj rec"])).to_string(),
        false => tc.draw(gs::sampled_from(vec!["Example DJ", "DJ REC"])).to_string(),
    };
    let title = tc.draw(gs::optional(gs::sampled_from(vec!["REC046", "REC1", "Friday", "RECORD", " "])));
    let genre = tc.draw(gs::optional(gs::sampled_from(vec!["XDJ-RX2", "Techno"])));
    let mut tags = vec![("ARTIST".to_string(), artist)];
    tags.extend(title.map(|t| ("TITLE".to_string(), t.to_string())));
    tags.extend(genre.map(|g| ("genre".to_string(), g.to_string())));
    tags.push(("YEAR".into(), "2024".into()));
    let mut info = MixInfo {
        path: PathBuf::from("/mixes/Club Night/set one.wav"),
        kind: "audio",
        title: String::new(),
        artist: None,
        genre: None,
        date: None,
        recorder: None,
        duration_secs: 0.0,
        sample_rate: 0,
        channels: 0,
        labels: Vec::new(),
        cover: None,
    };
    describe(&mut info, &tags);
    assert_eq!(info.date.as_deref(), Some("2024"));
    let generic = title.is_none_or(|t| t.trim().is_empty() || matches!(t, "REC046" | "REC1"));
    let expected = match (generic, recorder) {
        (false, _) => title.unwrap(),
        (true, true) => "Club Night",
        (true, false) => "set one",
    };
    assert_eq!(info.title, expected);
    if recorder {
        assert_eq!((info.artist.as_deref(), info.genre.as_deref()), (None, None));
        assert_eq!(info.recorder, genre.map(|g| format!("Pioneer DJ {g}")));
    } else {
        assert_eq!(
            (info.artist.as_deref(), info.genre.as_deref(), info.recorder.as_deref()),
            (Some(tags[0].1.as_str()), genre, None)
        );
    }
}

/// Recordings are found by name in every folder but hidden ones, in name order; the files
/// found are the ones a mix can be read from.
#[hegel::test(test_cases = 40)]
fn recordings_are_every_mix_file_outside_hidden_folders(tc: TestCase) {
    let names = vec!["a.wav", "B.MP3", "set.aup3", "notes.txt", ".c.flac", "d.m4a", "e.AUP4", "f.jpg"];
    let dirs = vec!["", "old", ".hidden", "old/2024", "old/.trash"];
    let files: Vec<(&str, &str)> =
        tc.draw(gs::vecs(gs::tuples!(gs::sampled_from(dirs), gs::sampled_from(names))).max_size(10));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("mixes");
    std::fs::create_dir_all(&root).unwrap();
    let mut expected = Vec::new();
    for (folder, name) in &files {
        let rel = Path::new(folder).join(name);
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::write(root.join(&rel), b"only the name is read").unwrap();
        let hidden = rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
        if !hidden && is_mix_file(&rel) && !expected.contains(&rel) {
            expected.push(rel);
        }
    }
    expected.sort();
    let found: Vec<PathBuf> =
        recordings_in(&root).iter().map(|p| p.strip_prefix(&root).unwrap().to_path_buf()).collect();
    assert_eq!(found, expected);
}

/// A set stitched from any of a drive's tracks, each from any point, at any level, after any
/// silence, is identified as those tracks in that order, from where they were taken, with
/// one transition between each and the next and nothing left unidentified that long; the
/// fingerprints are kept, so identifying against the drive again decodes none of them.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_set_on_a_drive_is_identified_as_its_tracks(tc: TestCase) {
    use crate::probe::tests::wav_of;
    use loclib_mixid::test_util::synth;
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    let pcm = |x: &[f32]| wav_of(RATE, 16, x.iter().map(|s| (s * 20_000.0) as i32));
    let secs = |s: f64| (s * RATE as f64) as usize;
    let audio: Vec<Vec<f32>> = (1..=3).map(|k| synth(32.0, k * 7919)).collect();
    let mut tracks = Vec::new();
    for (i, x) in audio.iter().enumerate() {
        let path = format!("/Contents/{i}.wav");
        std::fs::write(root.join(&path[1..]), pcm(x)).unwrap();
        let mut t = track(i as u32 + 1, "A", &format!("Track {i}"), &path);
        t.key = tc.draw(keys()).code();
        tracks.push(t);
    }
    // A file the decoder can't read, and a recorded set too long to be a track: neither is
    // indexed.
    std::fs::write(root.join("Contents/broken.wav"), crate::probe::tests::wav(0, 16, 0.0)).unwrap();
    tracks.push(track(8, "B", "Broken", "/Contents/broken.wav"));
    let mut long = track(9, "DJ", "Live", "/Contents/0.wav");
    long.duration_secs = index::LONGEST_TRACK_SECS + 1;
    tracks.push(long);
    let order = tc.draw(gs::samples(vec![0usize, 1, 2]).without_replacement().min_size(2).max_size(3));
    let mut set = vec![0.0f32; secs(tc.draw(gs::floats::<f64>().min_value(0.0).max_value(4.0)))];
    let mut truth = Vec::new();
    for &i in &order {
        let from = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(4.0));
        let gain = tc.draw(gs::floats::<f32>().min_value(0.4).max_value(1.0));
        truth.push((tracks[i].id, set.len() as f64 / RATE as f64, from));
        set.extend(audio[i][secs(from)..secs(from + 27.0)].iter().map(|s| s * gain));
    }
    let mixes = dir.path().join("mixes");
    std::fs::create_dir_all(&mixes).unwrap();
    let recording = mixes.join("set.wav");
    std::fs::write(&recording, pcm(&set)).unwrap();
    let device = Device::open(&root).unwrap();
    let library = Arc::new(Library { tracks, ..Default::default() });
    let cache = Cache::new(dir.path().join("cache"));
    let id = Identifier::with_library(&device, library.clone(), &cache, &MixPrefs::default(), &Ctx::silent()).unwrap();
    assert_eq!((id.report.tracks, id.report.indexed, id.report.fingerprinted), (5, 3, 3));
    assert_eq!((id.report.failed.len(), id.report.long.iter().map(|t| t.id).collect::<Vec<_>>()), (1, vec![9]));

    assert_eq!(Options::default().tempo_range, DEFAULT_TEMPO_RANGE);
    // A tempo range that isn't a number is refused before the mix is even looked for.
    let gone = mixes.join("gone.wav");
    let refused = id.identify(&gone, &Options { tempo_range: f64::NAN }, &Ctx::silent()).unwrap_err();
    assert!(matches!(refused, Error::Invalid(_)), "{refused}");
    let r = id.identify(&recording, &Options { tempo_range: 0.02 }, &Ctx::silent()).unwrap();
    assert!(r.matched_against(&device));
    let heard: Vec<u32> = r.plays.iter().map(|p| p.track.id).collect();
    assert_eq!(heard, truth.iter().map(|t| t.0).collect::<Vec<_>>(), "{:#?}", r.plays);
    for (p, (_, start, from)) in r.plays.iter().zip(&truth) {
        assert!((p.start - start).abs() < 2.0 && (p.track_start - from).abs() < 2.0, "{start}, {from}: {p:#?}");
        assert_eq!(
            (p.key.as_deref(), p.played_key.as_deref()),
            (Some(p.track.key.as_str()), Some(p.track.key.as_str()))
        );
    }
    assert_eq!(r.transitions.len(), r.plays.len() - 1);
    assert!(r.transitions.iter().zip(&r.plays[1..]).all(|(t, p)| t.at == p.start));
    assert!(r.unidentified.iter().all(|s| s.end - s.start >= UNIDENTIFIED_SECS));
    assert!((0.0..=1.0).contains(&r.coverage) && r.coverage > 0.8, "{}", r.coverage);
    assert!((r.mix.duration_secs - set.len() as f64 / RATE as f64).abs() < 0.01);
    assert_eq!((r.mix.title.as_str(), r.mix.sample_rate, r.mix.channels), ("set", RATE, 2));
    assert!((r.waveform.peak.len() as f64 - r.mix.duration_secs * WAVEFORM_RATE).abs() <= 1.0);

    let again = Identifier::with_library(&device, library, &cache, &MixPrefs::default(), &Ctx::silent()).unwrap();
    assert_eq!((again.report.fingerprinted, again.report.hashes), (0, id.report.hashes));
}

/// An Audacity project is heard as the mix it renders: its tags and labels describe it, and
/// it's as long as its last clip. One that isn't there isn't found.
#[hegel::test(test_cases = 3)]
fn a_project_is_heard_as_the_mix_it_renders(tc: TestCase) {
    use super::aup::tests::{Xml, floats, project};
    let rate = tc.draw(gs::sampled_from(vec![4_000.0, 8_000.0]));
    let title = tc.draw(gs::optional(gs::text().alphabet("Warehouse ").min_size(1).max_size(10)));
    let labels: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(9)).max_size(3));
    let offset = tc.draw(gs::integers::<u32>().max_value(2));
    let mut x = Xml::new(tc.draw(gs::sampled_from(vec![1u8, 2, 4])));
    x.start("project").double("rate", rate);
    if let Some(t) = &title {
        x.start("tags").start("tag").string("name", "TITLE").string("value", t).end("tag").end("tags");
    }
    x.start("labeltrack");
    for t in &labels {
        x.start("label").double("t", f64::from(*t)).string("title", "here").end("label");
    }
    x.end("labeltrack");
    let n = rate as usize;
    x.start("wavetrack").clip(f64::from(offset), n as i64, &[(0, 1)]).end("waveclip").end("wavetrack").end("project");
    let tone: Vec<f32> = (0..n).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let dir = tempfile::tempdir().unwrap();
    let path = project(dir.path(), "project", Some(&x), &[(1, 0x0004_000F, floats(&tone))]);
    let (info, heard) = listen(&path, &Ctx::silent()).unwrap();
    assert_eq!((info.kind, info.sample_rate, info.channels), ("audacity", rate as u32, 1));
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    let expected = title.filter(|t| !t.trim().is_empty()).unwrap_or(stem);
    assert_eq!(info.title, expected);
    assert_eq!(info.labels.iter().map(|l| l.start as u8).collect::<Vec<_>>(), labels);
    assert_eq!(info.duration_secs, f64::from(offset) + 1.0);
    let frames = info.duration_secs * FRAMES_PER_SEC;
    assert!((heard.frames as f64 - frames).abs() < 2.0, "{} frames for {frames}", heard.frames);
    assert!(matches!(listen(&dir.path().join("gone.aup3"), &Ctx::silent()), Err(Error::NotFound(_))));
}

/// A recording is described by its tags (a recorder's left out) and pictured by the cover
/// embedded in it, which a shared report carries, else by the picture next to it named most
/// like it: as the file, as its folder, then cover, folder, front or artwork. That one is a
/// guess, shown but not shared.
#[hegel::test(test_cases = 25)]
fn recordings_are_described_by_their_tags_and_pictures(tc: TestCase) {
    use lofty::config::WriteOptions;
    use lofty::tag::{Tag, TagType};
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Live Mixes");
    std::fs::create_dir_all(&folder).unwrap();
    let set = folder.join("set.wav");
    std::fs::write(&set, crate::probe::tests::wav(8_000, 16, 0.25)).unwrap();
    let title = tc.draw(gs::optional(gs::sampled_from(vec!["Friday", "REC046", " "])));
    let artist = tc.draw(gs::optional(gs::sampled_from(vec!["Example DJ", "PIONEER DJ REC"])));
    let genre = tc.draw(gs::optional(gs::sampled_from(vec!["Techno", "XDJ-RX2"])));
    let mut tag = Tag::new(TagType::Id3v2);
    title.inspect(|t| tag.set_title(t.to_string()));
    artist.inspect(|a| tag.set_artist(a.to_string()));
    genre.inspect(|g| tag.set_genre(g.to_string()));
    if title.is_some() || artist.is_some() || genre.is_some() {
        tag.save_to_path(&set, WriteOptions::default()).unwrap();
    }
    let embedded = tc.draw(gs::booleans());
    if embedded {
        let png = dir.path().join("art.png");
        image::RgbImage::from_pixel(16, 16, image::Rgb([200, 30, 30])).save(&png).unwrap();
        crate::artwork::embed_in_file(&set, &crate::artwork::Artwork::from_path(&png).unwrap()).unwrap();
    }
    let pool = vec![
        "set.jpg",
        "mixes.png",
        "live mixes.webp",
        "cover.jpg",
        "front.png",
        "artwork.jpeg",
        "notes.png",
        "set.txt",
    ];
    let pictures: Vec<&str> = tc.draw(gs::samples(pool).without_replacement().max_size(4));
    for name in &pictures {
        std::fs::write(folder.join(name), name).unwrap();
    }

    let (info, _) = listen(&set, &Ctx::silent()).unwrap();
    let recorder = artist == Some("PIONEER DJ REC");
    let expected = match title.filter(|t| !matches!(*t, "REC046" | " ")) {
        Some(t) => t,
        None if recorder => "Live Mixes",
        None => "set",
    };
    assert_eq!(info.title, expected);
    assert_eq!(info.artist.as_deref(), artist.filter(|_| !recorder));
    assert_eq!(info.recorder, genre.filter(|_| recorder).map(|g| format!("Pioneer DJ {g}")));
    let rank = |name: &str| {
        let stem = Path::new(name).file_stem().unwrap().to_string_lossy().to_lowercase();
        let image = ["jpg", "jpeg", "png", "webp"].iter().any(|e| name.ends_with(e));
        match stem.as_str() {
            _ if !image => None,
            "set" => Some(0),
            "live mixes" | "mixes" => Some(1),
            "cover" | "folder" | "front" | "artwork" => Some(2),
            _ => None,
        }
    };
    let guess = pictures.iter().filter_map(|n| Some((rank(n)?, folder.join(n)))).min().map(|(_, p)| p);
    match &info.cover {
        Some(CoverSource::Embedded(p)) => {
            assert!(embedded && *p == set);
            assert!(info.shared_cover().is_some_and(|b| !b.is_empty()));
        }
        Some(CoverSource::File(p)) => {
            assert!(!embedded && Some(p) == guess.as_ref(), "{p:?} for {guess:?}");
            assert_eq!(
                (info.shared_cover(), info.cover.as_ref().unwrap().read().unwrap()),
                (None, std::fs::read(p).unwrap())
            );
        }
        None => assert!(!embedded && guess.is_none(), "{guess:?}"),
        Some(CoverSource::Given(p)) => panic!("only `mix --cover` gives a cover: {p:?}"),
    }
}

/// A report is matched against a drive by the drive's identity when it was made with one,
/// whatever either is called now, and only by name when it wasn't.
#[hegel::test(test_cases = 50)]
fn reports_know_their_drive_by_identity_before_name(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut device = Device::open(dir.path()).unwrap();
    let names = vec!["DNA".to_string(), "Backup".into(), "USB".into()];
    let adopted = tc.draw(gs::optional(gs::sampled_from(names.clone())));
    let id = adopted.as_deref().map(|n| device.adopt(Some(n)).unwrap().id);
    let mut r = crate::mix::report::tests::report("Friday");
    r.drive = tc.draw(hegel::one_of!(gs::sampled_from(names), gs::just(device.name())));
    let made_with = tc.draw(gs::integers::<u8>().max_value(2));
    r.drive_id = match made_with {
        0 => None,
        1 => id,
        _ => Some(Uuid::new_v4()),
    };
    let same_name = r.drive == device.name();
    let want = match r.drive_id {
        None => same_name,
        Some(i) => Some(i) == id,
    };
    assert_eq!(r.matched_against(&device), want, "{:?} {:?} {}", r.drive_id, id, r.drive);
}

/// A mix with nothing in it to hear (an Audacity project whose clips hold no samples, wherever
/// they sit) is identified as what it is: no plays, no transitions, nothing covered, and as long
/// as what was heard of it, nothing.
#[hegel::test(test_cases = 5)]
fn a_mix_with_nothing_to_hear_is_identified_as_nothing(tc: TestCase) {
    use super::aup::tests::{Xml, project};
    let dir = tempfile::tempdir().unwrap();
    let mut x = Xml::new(1);
    x.start("project").double("rate", tc.draw(gs::sampled_from(vec![8_000.0, 44_100.0])));
    let clips = tc.draw(gs::integers::<u8>().min_value(1).max_value(3));
    x.start("wavetrack");
    for _ in 0..clips {
        x.clip(0.0, 0, &[]).end("waveclip");
    }
    x.end("wavetrack").end("project");
    let path = project(dir.path(), "project", Some(&x), &[]);
    let device = Device::open(dir.path()).unwrap();
    let cache = Cache::new(dir.path().join("cache"));
    let id =
        Identifier::with_library(&device, Arc::new(Library::default()), &cache, &MixPrefs::default(), &Ctx::silent())
            .unwrap();
    let r = id.identify(&path, &Options::default(), &Ctx::silent()).unwrap();
    assert!(r.plays.is_empty() && r.transitions.is_empty(), "{:?}", r.plays);
    assert_eq!(r.mix.duration_secs, 0.0);
    assert_eq!(r.coverage, 0.0);
}

/// A mix's cover reads back as the picture embedded in the recording, or as the bytes of the
/// image beside it; with that gone, reading it says so (not found in the recording, or the
/// file named), and only an embedded cover is one to share.
#[hegel::test(test_cases = 20)]
fn a_cover_reads_back_while_it_is_there(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let set = dir.path().join("set.wav");
    recording(&set);
    let side = tc.draw(gs::integers::<u32>().min_value(16).max_value(64));
    let png = dir.path().join("set.png");
    image::RgbImage::from_fn(side, side, |x, y| image::Rgb([x as u8, y as u8, 7])).save(&png).unwrap();
    let embedded = tc.draw(gs::booleans());
    let there = tc.draw(gs::booleans());
    let cover = if embedded {
        if there {
            crate::artwork::embed_in_file(&set, &crate::artwork::Artwork::from_path(&png).unwrap()).unwrap();
        }
        CoverSource::Embedded(set.clone())
    } else {
        if !there {
            std::fs::remove_file(&png).unwrap();
        }
        CoverSource::File(png.clone())
    };
    let info = MixInfo { cover: Some(cover.clone()), ..listen(&set, &Ctx::silent()).unwrap().0 };
    match (cover.read(), embedded, there) {
        (Ok(bytes), true, true) => {
            assert_eq!(image::load_from_memory(&bytes).unwrap().width(), side.max(crate::artwork::THUMB_SIZE));
            assert_eq!(info.shared_cover(), Some(bytes));
        }
        (Ok(bytes), false, true) => {
            assert_eq!(bytes, std::fs::read(&png).unwrap());
            assert_eq!(info.shared_cover(), None);
        }
        (Err(Error::NotFound(m)), true, false) => {
            assert!(m.contains(&set.display().to_string()), "{m}");
            assert_eq!(info.shared_cover(), None);
        }
        (Err(Error::Io { path, .. }), false, false) => assert_eq!(path, png),
        (other, ..) => panic!("embedded {embedded}, there {there}: {other:?}"),
    }
}

/// A mix of only silence, with nothing in the library to hear in it, is unidentified from its
/// start to its end once that's half a minute or more, and has no gap marked when it's shorter.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_mix_nothing_is_heard_in_is_unidentified_throughout(tc: TestCase) {
    use super::aup::tests::{Xml, project};
    let secs = tc.draw(gs::sampled_from(vec![5i64, 29, 31, 40]));
    let rate = 8_000i64;
    let dir = tempfile::tempdir().unwrap();
    let mut x = Xml::new(1);
    // A negative block id is that many samples of silence.
    x.start("project").double("rate", rate as f64).start("wavetrack");
    x.clip(0.0, secs * rate, &[(0, -secs * rate)]).end("waveclip").end("wavetrack").end("project");
    let path = project(dir.path(), "project", Some(&x), &[]);
    let device = Device::open(dir.path()).unwrap();
    let cache = Cache::new(dir.path().join("cache"));
    let id =
        Identifier::with_library(&device, Arc::new(Library::default()), &cache, &MixPrefs::default(), &Ctx::silent())
            .unwrap();
    let r = id.identify(&path, &Options::default(), &Ctx::silent()).unwrap();
    assert!(r.plays.is_empty(), "{:?}", r.plays);
    assert_eq!(r.mix.duration_secs, secs as f64);
    let gaps: Vec<(f64, f64)> = r.unidentified.iter().map(|s| (s.start, s.end)).collect();
    let want = if secs >= 30 { vec![(0.0, secs as f64)] } else { Vec::new() };
    assert_eq!(gaps, want);
}

/// The website reads the blend threshold from packages/tokens/mix.json; the player, the HTML
/// report and published sets use [`BLEND_SECS`]. A packaged crate has no repository around it.
#[test]
fn blends_are_called_as_the_website_calls_them() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/tokens/mix.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: {} isn't here", path.display());
        return;
    };
    let terms: serde_json::Value = serde_json::from_str(&text).expect("mix.json is JSON");
    assert_eq!(terms["blendSecs"].as_f64(), Some(BLEND_SECS));
    let at = |overlap| Transition { from: 0, to: 1, at: 0.0, overlap, harmony: None, tempo_change: None };
    assert!(at(BLEND_SECS).is_blend() && !at(BLEND_SECS - 0.1).is_blend());
}

/// Only an image can be given as a recording's cover: one is kept by its absolute path (given
/// relative to the current folder or not) and reads back as its bytes; anything else is refused
/// as no cover, and a file that isn't there as the file it is.
#[hegel::test(test_cases = 30)]
fn prop_only_an_image_can_be_given_as_a_cover(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cover.png");
    // 0: an image, 1: text, 2: nothing there.
    let kind = tc.draw(gs::integers::<u8>().max_value(2));
    match kind {
        0 => {
            let side = tc.draw(gs::integers::<u32>().min_value(16).max_value(40));
            image::RgbImage::from_fn(side, side, |x, y| image::Rgb([x as u8, y as u8, 9])).save(&file).unwrap();
        }
        1 => std::fs::write(&file, format!("not an image {}", tc.draw(gs::text().max_size(40)))).unwrap(),
        _ => {}
    }
    let relative = tc.draw(gs::booleans());
    let given = if relative {
        let up = std::env::current_dir().unwrap().components().count() - 1;
        std::iter::repeat_n(Path::new(".."), up).collect::<PathBuf>().join(file.strip_prefix("/").unwrap())
    } else {
        file.clone()
    };
    match (CoverSource::given(&given), kind) {
        (Ok(c @ CoverSource::Given(_)), 0) => {
            let CoverSource::Given(kept) = &c else { unreachable!() };
            assert!(kept.is_absolute() && kept.canonicalize().unwrap() == file.canonicalize().unwrap(), "{kept:?}");
            assert_eq!(c.read().unwrap(), std::fs::read(&file).unwrap());
        }
        (Err(Error::Invalid(m)), 1) => {
            assert!(m.starts_with(&format!("{} can't be a cover: ", given.display())), "{m}")
        }
        (Err(Error::Io { path, .. }), 2) => assert_eq!(path, given),
        (other, _) => panic!("{kind}: {other:?}"),
    }
}

/// A play's speed is its tempo as a percentage, or, when the pitch fader was ridden, the slowest
/// and fastest it went between: each to a tenth of a percent, signed.
#[hegel::test(test_cases = 200)]
fn prop_a_plays_speed_reads_as_the_percentages_it_was_played_at(tc: TestCase) {
    use loclib_model::test_util::track;
    let speed = || gs::floats::<f64>().min_value(0.5).max_value(1.5);
    let mut p = crate::mix::report::tests::play(track(1, "A", "B", "/C/d.mp3"));
    p.tempo = tc.draw(speed());
    p.speed_range = tc.draw(gs::optional(gs::tuples!(speed(), speed())));
    let said = p.speed();
    let read = |s: &str| -> f64 {
        assert!(
            s.starts_with(['+', '-']) && s.ends_with('%') && s.split('.').nth(1).is_some_and(|d| d.len() == 2),
            "{s}"
        );
        s.trim_end_matches('%').parse().unwrap()
    };
    let near = |s: &str, speed: f64| assert!((read(s) - (speed - 1.0) * 100.0).abs() <= 0.05 + 1e-9, "{said}");
    match p.speed_range {
        Some((slowest, fastest)) => {
            let (a, b) = said.split_once(" to ").unwrap();
            near(a, slowest);
            near(b, fastest);
        }
        None => near(&said, p.tempo),
    }
}

/// Files beside a mix named like a cover but with no extension saying they're an image aren't
/// its cover, and nothing is found beside a mix whose folder is gone.
#[test]
fn only_images_beside_a_mix_are_its_cover() {
    let dir = tempfile::tempdir().unwrap();
    let set = dir.path().join("set.wav");
    for name in ["set", "cover", "folder.txt"] {
        std::fs::write(dir.path().join(name), b"x").unwrap();
    }
    assert_eq!(cover_next_to(&set), None);
    std::fs::write(dir.path().join("front.PNG"), b"x").unwrap();
    assert_eq!(cover_next_to(&set), Some(dir.path().join("front.PNG")));
    assert_eq!(cover_next_to(&dir.path().join("gone/set.wav")), None);
}

/// A play kept without its lines (a report made before they were) follows its tempo from where it
/// starts in the track; it starts at the whole second it was first heard, never before 0. Two
/// plays blend when they're heard together for [`BLEND_SECS`] or more.
#[hegel::test(test_cases = 200)]
fn prop_a_play_without_lines_follows_its_tempo(tc: TestCase) {
    use loclib_model::test_util::track;
    let secs = || gs::floats::<f64>().min_value(-5.0).max_value(600.0);
    let mut p = crate::mix::report::tests::play(track(1, "A", "B", "/C/d.mp3"));
    (p.start, p.track_start) = (tc.draw(secs()), tc.draw(secs()));
    p.tempo = tc.draw(gs::floats::<f64>().min_value(0.84).max_value(1.16));
    let at = tc.draw(secs());
    assert_eq!(p.track_secs(at), p.track_start + (at - p.start) * p.tempo);
    let second = p.start_second();
    assert!(f64::from(second) <= p.start.max(0.0) && p.start.max(0.0) < f64::from(second) + 1.0, "{}", p.start);
    let overlap = tc.draw(gs::floats::<f64>().min_value(-30.0).max_value(30.0));
    let t = Transition { from: 0, to: 1, at, overlap, harmony: None, tempo_change: None };
    assert_eq!(t.is_blend(), overlap >= BLEND_SECS);
}

/// A drive with no library has nothing to identify a mix against: it says it isn't an export.
#[hegel::test(test_cases = 5)]
fn prop_a_drive_without_a_library_identifies_nothing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let name = tc.draw(gs::sampled_from(vec!["Main", "Spare"]));
    std::fs::create_dir(dir.path().join(name)).unwrap();
    let device = Device::open(dir.path().join(name)).unwrap();
    let cache = Cache::new(dir.path().join("cache"));
    let made = Identifier::new(&device, &cache, &MixPrefs::default(), &Ctx::silent());
    assert!(matches!(made, Err(Error::NotAnExport(_))), "{:?}", made.err());
}
