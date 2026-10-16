use super::*;

#[test]
fn slugs_and_urls() {
    assert_eq!(slug("one shot: i promise it's techno"), "one-shot-i-promise-it-s-techno");
    assert_eq!(slug("!!!"), "mix");
    let dir = tempfile::tempdir().unwrap();
    let mixes = dir.path().join("mixes/tachy cardia");
    std::fs::create_dir_all(&mixes).unwrap();
    let audio = mixes.join("Set #1.mp3");
    std::fs::write(&audio, b"x").unwrap();
    let page = dir.path().join("reports/set.html");
    std::fs::create_dir_all(page.parent().unwrap()).unwrap();
    assert_eq!(relative_url(&page, &audio), "../mixes/tachy%20cardia/Set%20%231.mp3");
}

/// The report's link to its audio leads there from the page, whichever folders they're in
/// and before the page's folder exists. (Bombadil found the link climbing to `/` when only
/// the audio's side had macOS's /var resolved to /private/var.)
#[hegel::test(test_cases = 200)]
fn audio_links_lead_to_the_audio(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let folder = || gs::vecs(gs::sampled_from(vec!["a", "b", ".", "tachy cardia", "Set #1", "日本"])).max_size(4);
    let page_dirs: Vec<&str> = tc.draw(folder());
    let audio_dirs: Vec<&str> = tc.draw(folder());
    let name = tc.draw(gs::sampled_from(vec!["set.wav", "Set #1.mp3", "ünï cödé.m4a"]));
    let made = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let audio = audio_dirs.iter().fold(dir.path().to_path_buf(), |p, s| p.join(s)).join(name);
    std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
    std::fs::write(&audio, b"x").unwrap();
    let page = page_dirs.iter().fold(dir.path().to_path_buf(), |p, s| p.join(s)).join("report.html");
    if made {
        std::fs::create_dir_all(page.parent().unwrap()).unwrap();
    }
    let url = relative_url(&page, &audio);
    assert!(!url.starts_with("file:") && !url.starts_with('/'), "{url}");

    // Follow it the way a browser would, from the page's folder.
    let mut bytes = Vec::new();
    let mut rest = url.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'%' {
            bytes.push(u8::from_str_radix(std::str::from_utf8(&tail[..2]).unwrap(), 16).unwrap());
            rest = &tail[2..];
        } else {
            bytes.push(b);
            rest = tail;
        }
    }
    std::fs::create_dir_all(page.parent().unwrap()).unwrap();
    let followed = page.parent().unwrap().join(String::from_utf8(bytes).unwrap());
    assert_eq!(std::fs::canonicalize(followed).unwrap(), std::fs::canonicalize(&audio).unwrap(), "{url}");
}

pub(crate) fn report(title: &str) -> MixReport {
    MixReport {
        mix: super::super::MixInfo {
            path: PathBuf::from("/mixes/set.wav"),
            kind: "audio",
            title: title.into(),
            artist: Some("Example DJ".into()),
            genre: None,
            date: None,
            recorder: None,
            duration_secs: 3600.0,
            sample_rate: 44100,
            channels: 2,
            labels: Vec::new(),
            cover: None,
        },
        drive: "DNA".into(),
        drive_id: None,
        library: Default::default(),
        plays: Vec::new(),
        transitions: Vec::new(),
        unidentified: Vec::new(),
        coverage: 0.9,
        waveform: Default::default(),
        tempo_range: 0.16,
        analysed_at: Utc::now(),
        elapsed_secs: 12.0,
    }
}

pub(crate) fn play(track: Track) -> Play {
    Play {
        track,
        start: 10.0,
        end: 200.0,
        track_start: 0.0,
        track_end: 190.0,
        tempo: 1.0,
        played_bpm: Some(128.0),
        key_lock: None,
        semitones: 0.0,
        key: None,
        played_key: None,
        confidence: 0.9,
        votes: 100,
        presence: vec![1.0],
        presence_step: 2.0,
        jumps: 0,
        alternates: Vec::new(),
    }
}

#[test]
fn reports_are_kept_in_the_store_and_listed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(list(root).unwrap().is_empty(), "no reports folder yet");
    let store = Store::open_or_init(root).unwrap();
    let id = save(&store, &report("Live at the Warehouse")).unwrap();
    assert!(id.starts_with("live-at-the-warehouse_"), "{id}");
    // A page an older build wrote, on its own.
    std::fs::write(reports_dir(root).join("old-set.html"), "<html>").unwrap();

    let listed = list(root).unwrap();
    let ids: Vec<&str> = listed.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&id.as_str()) && ids.contains(&"old-set"), "{ids:?}");
    let kept = listed.iter().find(|r| r.id == id).unwrap();
    assert_eq!(
        (kept.title.as_str(), kept.tracks, kept.drive.as_deref()),
        ("Live at the Warehouse", Some(0), Some("DNA"))
    );
    let old = listed.iter().find(|r| r.id == "old-set").unwrap();
    assert!(old.report.is_none() && old.html.is_some() && old.title == "old set");

    let back = load(root, &id).unwrap();
    assert_eq!((back.mix.title.as_str(), back.mix.kind), ("Live at the Warehouse", "audio"));
    assert!(matches!(load(root, "nope").unwrap_err().inner(), Error::NotFound(_)));
    assert!(matches!(load(root, "../store").unwrap_err().inner(), Error::Invalid(_)));

    // Without the drive, the page has the report and no tracks' extras.
    let html = page(&back, None, None, &reports_dir(root).join("x.html"), &ProfilePrefs::default(), false);
    assert!(html.contains("<title>") && html.contains("Live at the Warehouse") && html.contains("\"extras\":[]"));
}

/// A reports folder that can't be read is an error, not an empty list.
#[test]
fn a_reports_folder_that_cant_be_read_says_so() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(reports_dir(dir.path()), b"a file where the folder goes").unwrap();
    assert!(matches!(list(dir.path()).unwrap_err().inner(), Error::Io { .. }));
}

/// Two recordings with one title (a recorder's sets take their folder's name) are kept
/// apart; the same recording identified again replaces its own report.
#[test]
fn reports_of_mixes_with_one_title_are_kept_apart() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let (mut a, mut b) = (report("Friday"), report("friday!"));
    a.mix.path = "/mixes/club/REC001.WAV".into();
    b.mix.path = "/mixes/club/REC002.WAV".into();
    let (ia, ib) = (save(&store, &a).unwrap(), save(&store, &b).unwrap());
    assert_ne!(ia, ib);
    assert_eq!(save(&store, &a).unwrap(), ia);
    assert_eq!(list(dir.path()).unwrap().len(), 2);
    assert_eq!(load(dir.path(), &ia).unwrap().mix.path, a.mix.path);
    assert_eq!(list(dir.path()).unwrap().iter().find(|r| r.id == ib).unwrap().title, "friday!");
}

/// Ids stay short enough for a file name, and only ids made the way [`id`] makes them load:
/// a Windows drive-relative `C:x` would lead out of the reports folder.
#[test]
fn ids_are_bounded_and_checked() {
    assert_eq!(slug(&"long title ".repeat(40)).len(), SLUG_BYTES - 1, "no dash at the end");
    // Four bytes each (mathematical bold, as some titles are styled).
    assert_eq!(slug(&"\u{1d40b}".repeat(100)).len(), SLUG_BYTES);
    let dir = tempfile::tempdir().unwrap();
    for bad in ["C:x", "a/b", "..", ".hidden", ""] {
        assert!(matches!(load(dir.path(), bad).unwrap_err().inner(), Error::Invalid(_)), "{bad}");
    }
    assert!(matches!(load(dir.path(), "日本-set_0a1b2c3d").unwrap_err().inner(), Error::NotFound(_)));
}

/// The data block parses whatever the strings hold: `<!--` once left the page blank.
#[test]
fn the_page_reads_back_any_title() {
    let title = "a <!-- b </script> c";
    let html = page(&report(title), None, None, Path::new("/r/x.html"), &ProfilePrefs::default(), true);
    let data = html.split_once(r#"<script id="data" type="application/json">"#).unwrap().1;
    let data = data.split_once("</script>").unwrap().0;
    let v: serde_json::Value = serde_json::from_str(data).unwrap_or_else(|e| panic!("{e}: {data}"));
    assert_eq!(v["report"]["mix"]["title"], title);
}

/// The page draws in the colours the player does: the bands and phrases, and good, warning
/// and critical for the light page and both ways it goes dark.
#[test]
fn the_page_shares_the_players_colours() {
    use crate::tokens::{Rgb, bands, phrases, status};
    let html = page(&report("x"), None, None, Path::new("/r/x.html"), &ProfilePrefs::default(), true);
    let prop = |name: &str, [r, g, b]: Rgb| format!("--{name}: #{r:02x}{g:02x}{b:02x};");
    assert!(!html.contains("{{TOKENS}}"));
    for (name, rgb) in [("band-low", bands::LOW), ("phrase-verse", phrases::VERSE), ("good", status::light::GOOD)] {
        assert_eq!(html.matches(&prop(name, rgb)).count(), 1, "{name}");
    }
    assert!(html.contains("--band-scale-mid: 0.66;"));
    assert_eq!(html.matches(&prop("critical", status::dark::CRITICAL)).count(), 2, "both dark blocks");
}

/// A page made to be shared holds what it shows and nothing that only makes sense here (the
/// recording by its file name alone), and the tracks the profile hides are "ID - ID".
#[test]
fn the_page_holds_only_what_it_shows() {
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let screenshot = dir.path().join("screenshot.png");
    image::RgbImage::new(32, 32).save(&screenshot).unwrap();
    let mut r = report("Friday");
    r.mix.path = "/Users/someone/Mixes/friday.wav".into();
    r.mix.cover = Some(super::super::CoverSource::File(screenshot));
    r.library.missing.push((&track(9, "Gone", "Track", "/Contents/gone.mp3")).into());
    let mut shown = track(1, "Bicep", "Glue", "/Contents/Bicep/glue.mp3");
    shown.comment = "promo from a friend".into();
    shown.rating = 5;
    let mut p = play(shown);
    p.alternates.push(super::super::Alternate {
        track_id: 3,
        name: "Bicep - Glue (dub)".into(),
        file_path: "/Contents/Bicep/glue dub.mp3".into(),
        same_audio: false,
    });
    r.plays = vec![p, play(track(2, "Dubplate", "Unreleased", "/Contents/dub.wav"))];
    r.plays[1].confidence = 0.3;
    let mut prefs = ProfilePrefs::default();
    prefs.hidden_artists.push("Dubplate".into());

    let audio = Some(Path::new("/Users/someone/Mixes/friday.wav"));
    let html = page(&r, None, audio, Path::new("/r/x.html"), &prefs, true);
    for leak in
        ["/Users/someone", "Mixes", "/Contents/", "promo from a friend", "DNA", "screenshot", "Gone", "Unreleased"]
    {
        assert!(!html.contains(leak), "{leak} is in the page");
    }
    assert!(!html.contains("\"rating\"") && !html.contains("\"library\""));
    assert!(html.contains("Bicep - Glue (dub)") && html.contains("\"title\":\"ID\""));
    assert!(html.contains("\"tentative\":true") && html.contains("\"cover\":null"));
    // Played once the recording is put next to it.
    assert!(html.contains(r#""audio":"friday.wav","audioName":"friday.wav""#), "{html}");

    // The store's own page, opened here, plays the recording where it is.
    let kept = page(&r, None, audio, Path::new("/r/x.html"), &prefs, false);
    assert!(kept.contains(r#""audio":"../Users/someone/Mixes/friday.wav""#));
}

/// What the drive adds is found by the track's file, as a kept report's ids may name other
/// tracks there by now; a hot cue past Z is a `?`, and the page leaves cue names out.
#[test]
fn extras_come_from_the_drive_by_file() {
    use crate::anlz::Cue;
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let library = Library { tracks: vec![track(7, "A", "One", "/Contents/one.mp3")], ..Default::default() };
    let cue = |hot: u8| Cue {
        hot_cue: Some(hot),
        start_ms: 1000,
        end_ms: None,
        comment: "drop".into(),
        color: None,
        color_id: 0,
        loop_beats: None,
    };
    let analyses = [TrackAnalysis { cues: vec![cue(1), cue(200)], ..Default::default() }];
    let drive = Drive { device: &device, library: &library, analyses: Some(&analyses) };
    let labels = |e: PlayExtras| e.cues.into_iter().map(|c| c.label).collect::<Vec<_>>();
    let p = play(track(1, "A", "One", "/Contents/one.mp3"));
    assert_eq!(labels(extras(drive, &p, 0, false)), ["B · drop", "? · drop"]);
    assert_eq!(labels(extras(drive, &p, 0, true)), ["B", "?"]);
    let elsewhere = play(track(7, "B", "Two", "/Contents/two.mp3"));
    assert_eq!(extras(drive, &elsewhere, 0, false), PlayExtras::default());
}

#[test]
fn finds_the_export_of_a_project() {
    let dir = tempfile::tempdir().unwrap();
    for f in ["set one audacity save.aup4", "set one export.wav", "set one export.mp3", "other.mp3"] {
        std::fs::write(dir.path().join(f), b"x").unwrap();
    }
    let got = playable_audio(&dir.path().join("set one audacity save.aup4")).unwrap();
    assert_eq!(got.file_name().unwrap(), "set one export.mp3");
}

/// Only a project stands for audio exported next to it: a recording browsers can't play
/// has none, rather than another recording that shares its first letters.
#[test]
fn a_recording_is_never_swapped_for_its_neighbour() {
    let dir = tempfile::tempdir().unwrap();
    for f in ["friday 2024-05-03.aiff", "friday 2024-04-26.mp3"] {
        std::fs::write(dir.path().join(f), b"x").unwrap();
    }
    let aiff = dir.path().join("friday 2024-05-03.aiff");
    assert_eq!(playable_audio(&aiff), None);
    assert_eq!(project_audio(&aiff), None);
}

#[test]
fn file_urls() {
    assert_eq!(file_url(Path::new("/Users/dj/Set #1.wav")), "file:///Users/dj/Set%20%231.wav");
}

/// The root is a URL of its own, not `file://` with nothing after it.
#[test]
fn the_root_is_a_file_url() {
    assert_eq!(file_url(Path::new("/")), "file:///");
    assert_eq!(file_url(Path::new("/.")), "file:///");
}

/// The page on C:, the recording on the USB drive: no folder in common.
#[cfg(windows)]
#[test]
fn audio_on_another_drive_letter() {
    assert_eq!(
        relative_url(Path::new(r"C:\Users\dj\reports\x.html"), Path::new(r"E:\PIONEER REC\x.wav")),
        "file:///E:/PIONEER%20REC/x.wav"
    );
    assert_eq!(file_url(Path::new(r"\\nas\music\set.wav")), "file://nas/music/set.wav");
}

use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// `%XX` escapes decoded back to the bytes they stand for.
fn percent_decoded(url: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = url.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'%' {
            out.push(u8::from_str_radix(std::str::from_utf8(&tail[..2]).unwrap(), 16).unwrap());
            rest = &tail[2..];
        } else {
            out.push(b);
            rest = tail;
        }
    }
    out
}

/// Any title makes a slug fit to name a file: letters and digits, each its own lowercase,
/// words joined by single dashes, none at either end, within 60 bytes, never empty; and a
/// slug is its own.
#[hegel::test(test_cases = 500)]
fn any_title_slugs_to_a_file_name(tc: TestCase) {
    let title = tc.draw(gs::text().max_size(80));
    let s = slug(&title);
    assert!(!s.is_empty() && s.len() <= SLUG_BYTES, "{s:?}");
    assert!(!s.starts_with('-') && !s.ends_with('-') && !s.contains("--"), "{s:?}");
    assert!(
        s.chars().all(|c| c == '-' || (c.is_alphanumeric() && c.to_lowercase().eq([c]))),
        "{title:?} slugs to {s:?}"
    );
    assert_eq!(slug(&s), s);
}

/// Any URL path piece holds only what URLs take as it is, and reads back as what it stood
/// for; a `file:` URL leads to its path.
#[hegel::test(test_cases = 300)]
fn urls_read_back_as_their_paths(tc: TestCase) {
    // One piece each: on Windows a backslash separates too, and a colon makes a drive.
    let separators = if cfg!(windows) { "/\\:\0" } else { "/\0" };
    let parts: Vec<String> =
        tc.draw(gs::vecs(gs::text().min_size(1).max_size(10).exclude_characters(separators)).min_size(1).max_size(4));
    for part in &parts {
        let encoded = url_encode(part);
        assert!(encoded.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~%".contains(&b)), "{encoded}");
        assert_eq!(percent_decoded(&encoded), part.as_bytes());
    }
    let path: PathBuf = std::iter::once("/".to_string()).chain(parts).collect();
    let url = file_url(&path);
    let decoded = String::from_utf8(percent_decoded(url.strip_prefix("file://").unwrap())).unwrap();
    assert_eq!(Path::new(&decoded), path.components().collect::<PathBuf>(), "{url}");
}

/// A project's audio is the browser-playable file next to it sharing the most of its name
/// (four characters at least), the most compact of those; a recording stands for no other.
#[hegel::test(test_cases = 25)]
fn a_project_plays_the_export_sharing_most_of_its_name(tc: TestCase) {
    let stems = vec!["set one", "set one export", "set", "set two", "other", "SET ONE final"];
    let exts = vec!["mp3", "m4a", "wav", "flac", "ogg", "aiff", "txt", "aup3"];
    let files: Vec<(&str, &str)> =
        tc.draw(gs::vecs(gs::tuples!(gs::sampled_from(stems), gs::sampled_from(exts))).max_size(8));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("set one session.aup3");
    std::fs::write(&project, b"x").unwrap();
    for (stem, ext) in &files {
        std::fs::write(dir.path().join(format!("{stem}.{ext}")), b"x").unwrap();
    }
    let shared = |p: &Path| {
        let stem = p.file_stem().unwrap().to_string_lossy().to_lowercase();
        "set one session".chars().zip(stem.chars()).take_while(|(a, b)| a == b).count()
    };
    let playable: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| browser_format(p).is_some() && shared(p) >= 4)
        .collect();
    match playable_audio(&project) {
        None => assert!(playable.is_empty(), "{playable:?}"),
        Some(chosen) => {
            let most = playable.iter().map(|p| shared(p)).max().unwrap();
            assert_eq!(shared(&chosen), most, "{chosen:?} of {playable:?}");
            let best = playable.iter().filter(|p| shared(p) == most).filter_map(|p| browser_format(p)).min();
            assert_eq!(browser_format(&chosen), best, "{chosen:?} of {playable:?}");
        }
    }
    let recording = dir.path().join("set one session.aiff");
    assert_eq!(project_audio(&recording), None);
    // A recording a browser plays is its own audio.
    let mp3 = dir.path().join("set one session.MP3");
    assert_eq!(playable_audio(&mp3).as_deref(), Some(mp3.as_path()));
}

/// Any report kept in the store is listed with what it found and reads back as it was,
/// by an id made only of what ids are made of; nothing else is an id.
#[hegel::test(test_cases = 20)]
fn kept_reports_read_back_and_list_what_they_found(tc: TestCase) {
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path()).unwrap();
    let mut r = report(&tc.draw(gs::text().max_size(30)));
    r.mix.path = PathBuf::from(format!("/mixes/{}.wav", tc.draw(gs::text().alphabet("ab ï#").max_size(8))));
    r.plays = (0..tc.draw(gs::integers::<u32>().max_value(4))).map(|i| play(track(i, "A", "B", "/C/d.mp3"))).collect();
    r.coverage = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(1.0));
    let id = save(&store, &r).unwrap();
    assert_eq!(id, super::id(&r));
    assert!(id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'), "{id}");
    let back = load(dir.path(), &id).unwrap();
    assert_eq!(
        (back.mix.title, back.mix.path, back.plays.len()),
        (r.mix.title.clone(), r.mix.path.clone(), r.plays.len())
    );
    let listed = list(dir.path()).unwrap();
    assert_eq!(listed.len(), 1);
    let kept = &listed[0];
    assert_eq!((kept.id.as_str(), kept.tracks), (id.as_str(), Some(r.plays.len())));
    assert_eq!(kept.coverage, Some(r.coverage), "{kept:?}");
    assert_eq!((kept.title.as_str(), kept.mix.as_deref()), (r.mix.title.as_str(), Some(r.mix.path.as_path())));

    let other = tc.draw(gs::text().max_size(12));
    let err = load(dir.path(), &other).unwrap_err();
    let well_formed = !other.is_empty() && other.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    match err.inner() {
        Error::NotFound(_) => assert!(well_formed && other != id, "{other:?}"),
        Error::Invalid(_) => assert!(!well_formed, "{other:?}"),
        e => panic!("{other:?}: {e}"),
    }
}

/// The page reads back any title, holds one entry for each play (and with the drive, what
/// it adds to each), names the tracks the profile hides as "ID", and puts in its title
/// element only what HTML takes as text.
#[hegel::test(test_cases = 100)]
fn the_page_holds_the_report_whatever_it_says(tc: TestCase) {
    use loclib_model::test_util::track;
    let title = tc.draw(gs::text().alphabet("ab <!-/script>&\"'é").max_size(30));
    let mut r = report(&title);
    r.plays =
        (0..tc.draw(gs::integers::<u32>().max_value(4))).map(|i| play(track(i, "A", &title, "/C/d.mp3"))).collect();
    for p in &mut r.plays {
        let artist = tc.draw(gs::sampled_from(vec!["A", "B"]));
        let name = format!("{artist} - {title} (dub)");
        p.alternates.push(super::super::Alternate {
            track_id: 99,
            name,
            file_path: "/C/e.mp3".into(),
            same_audio: false,
        });
    }
    let shared = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let library = Library { tracks: r.plays.iter().map(|p| p.track.clone()).collect(), ..Default::default() };
    let drive = tc.draw(gs::booleans()).then(|| Drive::new(&device, &library));
    let mut prefs = ProfilePrefs::default();
    let hidden = tc.draw(gs::booleans());
    if hidden {
        prefs.hidden_artists.push("A".into());
    }
    let audio = Some(Path::new("/mixes/set one.wav"));
    let html = page(&r, drive, audio, Path::new("/r/x.html"), &prefs, shared);
    let data = html.split_once(r#"<script id="data" type="application/json">"#).unwrap().1;
    let data = data.split_once("</script>").unwrap().0;
    let v: serde_json::Value = serde_json::from_str(data).unwrap_or_else(|e| panic!("{e}: {data}"));
    assert_eq!(v["report"]["mix"]["title"], title.as_str());
    let plays = v["report"]["plays"].as_array().unwrap();
    assert_eq!(plays.len(), r.plays.len());
    assert_eq!(v["extras"].as_array().unwrap().len(), if drive.is_some() { r.plays.len() } else { 0 });
    let named = if hidden { UNNAMED } else { title.as_str() };
    assert!(plays.iter().all(|p| p["track"]["title"] == named), "{plays:?}");
    for (p, play) in plays.iter().zip(&r.plays) {
        let alternate = &play.alternates[0];
        let hidden_alternate = hidden && alternate.name.starts_with("A - ");
        let name = if hidden_alternate { format!("{UNNAMED} - {UNNAMED}") } else { alternate.name.clone() };
        assert_eq!(p["alternates"][0]["name"], name.as_str());
    }
    let audio = if shared { "set%20one.wav" } else { "../mixes/set%20one.wav" };
    assert_eq!((v["audio"].as_str(), v["audioName"].as_str()), (Some(audio), Some("set one.wav")));
    let head = html.split_once("<title>").unwrap().1.split_once("</title>").unwrap().0;
    assert!(!head.contains('<') && !head.contains('>'), "{head}");
}

/// What the drive adds to a play: each hot cue (only those) at its time, labelled by its letter,
/// with its name after it except on the page or when it has none, in its colour; and on the page
/// only, the cover from the drive, as a JPEG `data:` URL of the file's own bytes.
#[hegel::test(test_cases = 100)]
fn extras_carry_each_hot_cue_and_on_the_page_the_cover(tc: TestCase) {
    use crate::anlz::Cue;
    use loclib_model::test_util::track;
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    let mut t = track(7, "A", "One", "/Contents/one.mp3");
    let cover: Option<Vec<u8>> = tc.draw(gs::optional(gs::vecs(gs::integers::<u8>()).max_size(40)));
    if let Some(bytes) = &cover {
        t.artwork_path = Some("/PIONEER/Artwork/00001/a7.jpg".into());
        let file = dir.path().join("PIONEER/Artwork/00001/a7.jpg");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
    let cues: Vec<Cue> = tc
        .draw(
            gs::vecs(gs::tuples!(
                gs::optional(gs::integers::<u8>()),
                gs::integers::<u32>().max_value(600_000),
                gs::sampled_from(vec!["", "drop", "vox in"]),
                gs::optional(gs::tuples!(gs::integers::<u8>(), gs::integers::<u8>(), gs::integers::<u8>())),
            ))
            .max_size(5),
        )
        .into_iter()
        .map(|(hot_cue, start_ms, comment, color)| Cue {
            hot_cue,
            start_ms,
            end_ms: None,
            comment: comment.into(),
            color: color.map(|(r, g, b)| [r, g, b]),
            color_id: 0,
            loop_beats: None,
        })
        .collect();
    let library = Library { tracks: vec![t.clone()], ..Default::default() };
    let analyses = [TrackAnalysis { cues: cues.clone(), ..Default::default() }];
    let drive = Drive { device: &device, library: &library, analyses: Some(&analyses) };
    let page = tc.draw(gs::booleans());
    let e = extras(drive, &play(t), 0, page);

    let hot: Vec<&Cue> = cues.iter().filter(|c| c.hot_cue.is_some()).collect();
    assert_eq!(e.cues.len(), hot.len());
    for (mark, c) in e.cues.iter().zip(hot) {
        let letter = HotCue(c.hot_cue.unwrap()).letter().to_string();
        assert_eq!(mark.t, f64::from(c.start_ms) / 1000.0);
        let (named, rest) = match mark.label.split_once(" · ") {
            Some((l, name)) => (l, Some(name)),
            None => (mark.label.as_str(), None),
        };
        assert_eq!(named, letter);
        assert_eq!(rest, (!page && !c.comment.is_empty()).then_some(c.comment.as_str()), "{}", mark.label);
        let hex = mark.color.as_deref().map(|h| {
            let n = u32::from_str_radix(h.strip_prefix('#').unwrap(), 16).unwrap();
            assert_eq!(h.len(), 7, "{h}");
            [(n >> 16) as u8, (n >> 8) as u8, n as u8]
        });
        assert_eq!(hex, c.color);
    }
    let art = e.art.map(|url| BASE64_STANDARD.decode(url.strip_prefix("data:image/jpeg;base64,").unwrap()).unwrap());
    assert_eq!(art, cover.filter(|_| page));
}

/// A file URL leads to its path, the root included: it ends in a slash exactly when the path is
/// the root, which `file://` alone wouldn't name.
#[hegel::test(test_cases = 200)]
fn file_urls_end_in_a_slash_only_at_the_root(tc: TestCase) {
    let parts: Vec<String> =
        tc.draw(gs::vecs(gs::sampled_from(vec![".", "Mixes", "set #1.wav", "日本"]).map(String::from)).max_size(4));
    let path: PathBuf = std::iter::once("/".to_string()).chain(parts).collect();
    let url = file_url(&path);
    let root = path.components().all(|c| matches!(c, Component::RootDir | Component::CurDir));
    assert_eq!(url.ends_with('/'), root, "{url}");
    let decoded = String::from_utf8(percent_decoded(url.strip_prefix("file://").unwrap())).unwrap();
    assert_eq!(Path::new(&decoded), path.components().collect::<PathBuf>(), "{url}");
}

/// The reports folder lists each report once, newest first, by any of its files (the data, the
/// page, the export), and nothing else in it: one with its data reads back its title from it,
/// one without takes it from its id. No folder is no reports; a folder that can't be read is an
/// error, not an empty list.
#[hegel::test(test_cases = 40)]
fn the_reports_folder_lists_each_report_once(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let folder = reports_dir(dir.path());
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => return assert!(list(dir.path()).unwrap().is_empty()),
        1 => {
            std::fs::write(&folder, b"a file where the folder goes").unwrap();
            let e = list(dir.path()).unwrap_err();
            return assert!(matches!(e.inner(), Error::Io { path, .. } if *path == folder), "{e}");
        }
        _ => {}
    }
    std::fs::create_dir_all(&folder).unwrap();
    let stray = tc.draw(gs::vecs(gs::sampled_from(vec!["notes.txt", "x.json.bak", ".DS_Store"])).max_size(2));
    for s in &stray {
        std::fs::write(folder.join(s), b"x").unwrap();
    }
    // Each report: a title, and which of its three files are there.
    let kept: Vec<(String, [bool; 3])> = tc.draw(
        gs::vecs(gs::tuples!(
            gs::sampled_from(vec!["friday-night", "warm-up", "b2b-set"]).map(String::from),
            gs::tuples!(gs::booleans(), gs::booleans(), gs::booleans()).map(|(a, b, c)| [a, b, c]),
        ))
        .max_size(4),
    );
    let mut want = rustc_hash::FxHashMap::default();
    for (i, (title, files)) in kept.iter().enumerate() {
        let id = format!("{title}_{i:04}");
        for (on, ext) in files.iter().zip(["json", "html", "mix.json"]) {
            let file = folder.join(format!("{id}.{ext}"));
            match (on, ext) {
                (false, _) => {}
                (true, "json") => {
                    let r = report(&format!("{title} live"));
                    std::fs::write(&file, serde_json::to_vec(&r).unwrap()).unwrap();
                }
                (true, _) => std::fs::write(&file, b"{}").unwrap(),
            }
        }
        if files.contains(&true) {
            let shown = if files[0] { format!("{title} live") } else { title.replace('-', " ") };
            want.insert(id, (shown, files[0]));
        }
    }
    let listed = list(dir.path()).unwrap();
    let got: rustc_hash::FxHashMap<String, (String, bool)> =
        listed.iter().map(|s| (s.id.clone(), (s.title.clone(), s.report.is_some()))).collect();
    assert_eq!(got, want);
    assert_eq!(listed.len(), want.len(), "each once");
    assert!(listed.windows(2).all(|w| w[0].modified >= w[1].modified), "newest first");
}
