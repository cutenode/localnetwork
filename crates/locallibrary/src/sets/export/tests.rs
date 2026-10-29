use super::*;
use crate::export::PathStyle;
use crate::sets::query::tests::set;
use crate::sets::{EntryOrigin, Recording};
use loclib_model::test_util::track;

fn friday() -> Set {
    let mut s = set("3m2abcdefgh22", "2026-10-03", Some("Friday"));
    s.recording = Some(Recording { path: "/mixes/Friday/set.wav".into(), report: None });
    let mut t = track(1, "Floorplan", "Never Grow Old", "/Contents/f.mp3");
    (t.album, t.label, t.genre, t.key, t.isrc, t.year) =
        ("Paradise".into(), "M-Plant".into(), "Techno".into(), "8A".into(), "GBXXX1300001".into(), 2013);
    let mut a = SetEntry::from_track(&t, EntryOrigin::Recording, None);
    a.start_seconds = Some(4);
    let mut b = SetEntry::added("Someone, Other feat. X - Secret");
    (b.file, b.start_seconds, b.unnamed) = (Some("/Contents/secret.mp3".into()), Some(312), true);
    let mut c = SetEntry::added("Daft Punk - Around the World");
    (c.file, c.start_seconds) = (Some("/Contents/atw.mp3".into()), Some(3725));
    s.tracks = vec![a, b, c];
    s
}

fn text(set: &Set, format: SetFormat) -> String {
    render(set, format, &ProfilePrefs::default(), None).unwrap().text
}

#[test]
fn public_formats_hide_unnamed_tracks() {
    let s = friday();
    assert_eq!(
        text(&s, SetFormat::Text),
        "01. Floorplan - Never Grow Old\n02. ID - ID\n03. Daft Punk - Around the World\n"
    );
    let r = render(&s, SetFormat::Mixcloud, &ProfilePrefs::default(), None).unwrap();
    assert_eq!(
        r.text,
        "Floorplan - Never Grow Old - 00:00:04\nID - ID - 00:05:12\nDaft Punk - Around the World - 01:02:05\n"
    );
    assert_eq!((r.tracks, r.warnings.len()), (3, 0));
    assert_eq!(
        text(&s, SetFormat::Tracklists),
        "[0:00:04] Floorplan - Never Grow Old\n[0:05:12] ID - ID\n[1:02:05] Daft Punk - Around the World\n"
    );
    assert_eq!(
        text(&s, SetFormat::Youtube),
        "00:00 Floorplan - Never Grow Old\n05:12 ID - ID\n1:02:05 Daft Punk - Around the World\n"
    );
    assert_eq!(
        text(&s, SetFormat::Cue),
        "TITLE \"Friday\"\nFILE \"set.wav\" WAVE\n\
             \x20 TRACK 01 AUDIO\n    TITLE \"Never Grow Old\"\n    PERFORMER \"Floorplan\"\n    INDEX 01 00:04:00\n\
             \x20 TRACK 02 AUDIO\n    TITLE \"ID\"\n    PERFORMER \"ID\"\n    INDEX 01 05:12:00\n\
             \x20 TRACK 03 AUDIO\n    TITLE \"Around the World\"\n    PERFORMER \"Daft Punk\"\n    INDEX 01 62:05:00\n"
    );

    // Without times, the formats that can do without them leave them out.
    let mut untimed = s.clone();
    untimed.tracks.iter_mut().for_each(|e| e.start_seconds = None);
    assert_eq!(text(&untimed, SetFormat::Mixcloud).lines().next(), Some("Floorplan - Never Grow Old"));
    assert_eq!(text(&untimed, SetFormat::Tracklists).lines().nth(1), Some("ID - ID"));
}

#[test]
fn csv_names_every_track_and_marks_the_unnamed() {
    assert_eq!(
        text(&friday(), SetFormat::Csv),
        "#,Start,Artist,Title,Album,Label,Genre,BPM,Key,ISRC,Year,Unnamed\r\n\
             1,0:00:04,Floorplan,Never Grow Old,Paradise,M-Plant,Techno,128.00,8A,GBXXX1300001,2013,\r\n\
             2,0:05:12,\"Someone, Other feat. X\",Secret,,,,,,,,yes\r\n\
             3,1:02:05,Daft Punk,Around the World,,,,,,,,\r\n"
    );
}

#[test]
fn public_formats_hide_tracks_the_profile_hides() {
    let s = friday();
    let prefs = ProfilePrefs { hidden: vec!["Daft Punk - Around the World".into()], ..ProfilePrefs::default() };
    let hidden = |f: SetFormat| render(&s, f, &prefs, None).unwrap().text;
    assert_eq!(hidden(SetFormat::Text), "01. Floorplan - Never Grow Old\n02. ID - ID\n03. ID - ID\n");
    assert_eq!(hidden(SetFormat::Mixcloud).lines().nth(2), Some("ID - ID - 01:02:05"));
    assert_eq!(hidden(SetFormat::Tracklists).lines().nth(2), Some("[1:02:05] ID - ID"));
    assert_eq!(hidden(SetFormat::Youtube).lines().nth(2), Some("1:02:05 ID - ID"));
    assert!(
        hidden(SetFormat::Cue)
            .ends_with("TRACK 03 AUDIO\n    TITLE \"ID\"\n    PERFORMER \"ID\"\n    INDEX 01 62:05:00\n")
    );
    // CSV is the DJ's own: it names every track, and marks only what the set marks.
    assert_eq!(hidden(SetFormat::Csv), text(&s, SetFormat::Csv));
}

/// Podcasting 2.0 chapters: the set's name, and a chapter for each track with a start time, in
/// the order they play, not naming what a published tracklist doesn't. Tracks without one are left
/// out, saying so.
#[test]
fn chapters_are_podcasting_json() {
    let mut s = friday();
    s.tracks[0].start_seconds = None;
    s.tracks.swap(1, 2);
    let r = render(&s, SetFormat::Chapters, &ProfilePrefs::default(), None).unwrap();
    assert_eq!(
        r.text,
        r#"{
  "version": "1.2.0",
  "title": "Friday",
  "chapters": [
    {
      "startTime": 312,
      "title": "ID - ID"
    },
    {
      "startTime": 3725,
      "title": "Daft Punk - Around the World"
    }
  ]
}
"#
    );
    assert_eq!((r.tracks, r.warnings.as_slice()), (2, &["track 1 has no start time, so no chapter".to_string()][..]));
    let empty = set("3m2abcdefgh22", "2026-10-03", None);
    let r = render(&empty, SetFormat::Chapters, &ProfilePrefs::default(), None).unwrap();
    assert_eq!(r.text, "{\n  \"version\": \"1.2.0\",\n  \"title\": \"2026-10-03\",\n  \"chapters\": []\n}\n");
    assert_eq!((r.tracks, r.warnings.len()), (0, 0));
}

#[test]
fn m3u8_lists_the_tracks_on_the_drive() {
    let lib = Library {
        tracks: vec![
            track(1, "Floorplan", "Never Grow Old", "/Contents/f.mp3"),
            track(2, "Daft Punk", "Around the World", "/Contents/ATW.mp3"),
        ],
        ..Default::default()
    };
    let locator = Locator { style: PathStyle::Drive, drive_root: "/Volumes/USB".into(), base: "/x".into() };
    let r = render(&friday(), SetFormat::M3u8, &ProfilePrefs::default(), Some((&lib, &locator))).unwrap();
    assert_eq!(
        r.text,
        "#EXTM3U\n#PLAYLIST:Friday\n#EXTINF:300,Floorplan - Never Grow Old\n/Contents/f.mp3\n\
             #EXTINF:300,Daft Punk - Around the World\n/Contents/ATW.mp3\n"
    );
    assert_eq!(r.tracks, 2);
    assert_eq!(r.warnings, ["2. Someone, Other feat. X - Secret isn't on the drive: left out"]);
    assert!(render(&friday(), SetFormat::M3u8, &ProfilePrefs::default(), None).is_err());
}

#[test]
fn timed_formats_need_every_start() {
    let mut s = friday();
    s.tracks[1].start_seconds = None;
    for format in [SetFormat::Cue, SetFormat::Youtube] {
        let err = render(&s, format, &ProfilePrefs::default(), None).unwrap_err().to_string();
        assert!(err.starts_with("track 2 (Someone, Other feat. X - Secret) has no start time"), "{err}");
    }
    let mut s = friday();
    s.recording = None;
    let err = render(&s, SetFormat::Cue, &ProfilePrefs::default(), None).unwrap_err().to_string();
    assert_eq!(err, "a CUE sheet indexes the recording: attach it to the set first");
}

#[test]
fn youtube_rules_are_warned_about() {
    let mut s = friday();
    s.tracks[2].start_seconds = Some(100);
    s.tracks[1].start_seconds = Some(105);
    let r = render(&s, SetFormat::Youtube, &ProfilePrefs::default(), None).unwrap();
    assert_eq!(r.warnings, ["track 3 (0:01:40) doesn't start after track 2 (0:01:45)"]);
    s.tracks.pop();
    let r = render(&s, SetFormat::Youtube, &ProfilePrefs::default(), None).unwrap();
    assert_eq!(r.text.lines().next(), Some("00:00 Floorplan - Never Grow Old"));
    assert_eq!(r.warnings, ["YouTube needs at least 3 chapters; this has 2"]);
    s.tracks[1].start_seconds = Some(9);
    let r = render(&s, SetFormat::Youtube, &ProfilePrefs::default(), None).unwrap();
    assert_eq!(r.warnings[1], "chapter 1 is 9 seconds long; YouTube needs 10");
    s.tracks[1].start_seconds = Some(10);
    assert_eq!(render(&s, SetFormat::Youtube, &ProfilePrefs::default(), None).unwrap().warnings.len(), 1);
}

/// A set of 5-minute tracks by these artists, from these releases.
fn show(tracks: &[(&str, &str)]) -> Set {
    let mut s = set("3m2abcdefgh22", "2026-10-03", None);
    s.tracks = tracks
        .iter()
        .map(|(artist, album)| SetEntry {
            album: album.to_string(),
            duration_secs: 300,
            ..SetEntry::added(&format!("{artist} - Track"))
        })
        .collect();
    s
}

const LIMITED: &str = "before it limits the show to subscribers";

#[test]
fn mixcloud_allows_three_from_a_release_two_in_a_row() {
    assert!(mixcloud_check(&show(&[("A", "X"), ("A", "X"), ("B", "Y"), ("A", "X")])).is_empty());
    assert_eq!(
        mixcloud_check(&show(&[("A", "X"), ("A", "X"), ("B", "Y"), ("A", "X"), ("C", "x")])),
        [format!("4 tracks from \"X\": Mixcloud allows 3 {LIMITED}")]
    );
    assert_eq!(
        mixcloud_check(&show(&[("A", "X"), ("B", "X"), ("C", "X"), ("D", "Y")])),
        [format!("tracks 1-3 are from \"X\": Mixcloud allows 2 in a row {LIMITED}")]
    );
    assert!(mixcloud_check(&show(&[("A", ""), ("B", ""), ("C", ""), ("D", ""), ("E", "")])).is_empty());
}

#[test]
fn mixcloud_allows_four_by_an_artist_three_in_a_row() {
    assert!(mixcloud_check(&show(&[("A", ""), ("A", ""), ("A", ""), ("B", ""), ("A feat. C", "")])).is_empty());
    assert_eq!(
        mixcloud_check(&show(&[("A", ""), ("a", ""), ("A", ""), ("A feat. C", ""), ("B", "")])),
        [format!("tracks 1-4 are by A: Mixcloud allows 3 in a row {LIMITED}")]
    );
    let spread = [("A", ""), ("B", ""), ("A", ""), ("C", ""), ("A", ""), ("D", ""), ("C, A", ""), ("E", "")];
    assert!(mixcloud_check(&show(&spread)).is_empty());
    assert_eq!(
        mixcloud_check(&show(&[&spread[..], &[("A", "")]].concat())),
        [format!("5 tracks by A: Mixcloud allows 4 {LIMITED}")]
    );
}

#[test]
fn mixcloud_wants_fifteen_minutes() {
    assert_eq!(
        mixcloud_check(&show(&[("A", "X"), ("A", "X"), ("A", "X")][..2])),
        ["it's 10 minutes long: Mixcloud doesn't allow copyrighted music in uploads under 15 minutes"]
    );
    let mut s = show(&[("A", "X"), ("A", "X"), ("A", "X")]);
    assert_eq!(mixcloud_check(&s).len(), 1, "15 minutes is long enough for the rules to apply");
    s.tracks[2].start_seconds = Some(500);
    assert!(mixcloud_check(&s)[0].starts_with("it's 13 minutes long"), "a recording's length goes by its times");
    assert!(mixcloud_check(&set("3m2abcdefgh22", "2026-10-03", None)).is_empty());
}

use hegel::TestCase;
use hegel::generators as gs;

/// Artists, titles and albums as tracklists get them: with commas, quotes, a tab, and
/// credits.
const ARTISTS: &[&str] = &["Floorplan", "floorplan", "Someone, Other", "A feat. B", "", "Tab\tArtist"];
const TITLES: &[&str] = &["Never Grow Old", "Say \"Yes\"", "ID", ""];
const ALBUMS: &[&str] = &["", "Paradise", "PARADISE", "Some, Album"];

#[hegel::test_helper]
fn some_set(tc: &TestCase, timed: bool) -> Set {
    let mut s = set("3m2abcdefgh22", "2026-10-03", tc.draw(gs::sampled_from(vec![None, Some("Friday")])));
    s.recording = tc
        .draw(gs::sampled_from(vec![
            None,
            Some("/mixes/set.wav"),
            Some("/mixes/Set.MP3"),
            Some("/m/a.aiff"),
            Some("/m/a.flac"),
        ]))
        .map(|p| Recording { path: p.into(), report: None });
    let mut at = 0;
    s.tracks = (0..tc.draw(gs::integers::<usize>().max_value(6)))
        .map(|_| SetEntry {
            album: tc.draw(gs::sampled_from(ALBUMS.to_vec())).into(),
            label: tc.draw(gs::sampled_from(vec!["", "M-Plant"])).into(),
            bpm: tc.draw(gs::sampled_from(vec![0.0, 128.0, 99.5])),
            year: tc.draw(gs::sampled_from(vec![0, 2013])),
            duration_secs: tc.draw(gs::integers::<u32>().max_value(600)),
            start_seconds: (timed || tc.draw(gs::booleans())).then(|| {
                // Mostly a few seconds after the track before (chapters around YouTube's 10),
                // now and then anywhere (out of order, or hours in).
                let anywhere = tc.draw(gs::integers::<u8>().max_value(3)) == 0;
                at = match anywhere {
                    true => tc.draw(gs::integers::<u32>().max_value(20_000)),
                    false => at + tc.draw(gs::integers::<u32>().max_value(15)),
                };
                at
            }),
            unnamed: tc.draw(gs::booleans()),
            ..SetEntry {
                artist: tc.draw(gs::sampled_from(ARTISTS.to_vec())).into(),
                title: tc.draw(gs::sampled_from(TITLES.to_vec())).into(),
                ..SetEntry::added("")
            }
        })
        .collect();
    s
}

/// A profile hiding, or not, Floorplan however it's spelt, B wherever they're credited, and one
/// track by its name.
#[hegel::test_helper]
fn some_prefs(tc: &TestCase) -> ProfilePrefs {
    let some = |names: &[&str]| -> Vec<String> {
        names.iter().filter(|_| tc.draw(gs::booleans())).map(|n| n.to_string()).collect()
    };
    ProfilePrefs {
        hidden_artists: some(&["Floorplan", "B"]),
        hidden: some(&["Someone, Other - Never Grow Old"]),
        ..ProfilePrefs::default()
    }
}

/// Whether a published tracklist names `e`: not when it's unnamed, or `prefs` (from
/// [`some_prefs`]) hides it.
fn named(e: &SetEntry, prefs: &ProfilePrefs) -> bool {
    let artist = |a: &str| prefs.hidden_artists.iter().any(|h| h == a);
    let hidden = (artist("Floorplan") && e.artist.eq_ignore_ascii_case("floorplan"))
        || (artist("B") && e.artist == "A feat. B")
        || (!prefs.hidden.is_empty() && e.name() == "Someone, Other - Never Grow Old");
    !e.unnamed && !hidden
}

/// As a published tracklist names it: `ID - ID` for a track it doesn't name, else its name on
/// one line.
fn published(e: &SetEntry, prefs: &ProfilePrefs) -> String {
    match named(e, prefs) {
        true => e.name().replace(|c: char| c.is_control(), " "),
        false => "ID - ID".into(),
    }
}

fn clock_of(secs: u32) -> String {
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Every format lists every track in set order: text numbered, Mixcloud and 1001Tracklists
/// with the times they have, CSV naming every track (read back as written); YouTube and CUE
/// need every track's start (CUE the recording too), refuse naming the first track without
/// one, and warn about tracks that don't start after the one before. The formats for publishing
/// name no track the set marks unnamed or the profile hides. An M3U8 needs a drive.
#[hegel::test(test_cases = 300)]
fn every_format_lists_every_track_in_order(tc: TestCase) {
    let s = some_set(&tc, tc.draw(gs::booleans()));
    let prefs = some_prefs(&tc);
    let published = |e: &SetEntry| published(e, &prefs);
    let lines = |f: SetFormat| -> Vec<String> {
        let r = render(&s, f, &prefs, None).unwrap();
        assert_eq!(r.tracks, s.tracks.len());
        r.text.lines().map(String::from).collect()
    };
    let want: Vec<String> =
        s.tracks.iter().enumerate().map(|(i, e)| format!("{:02}. {}", i + 1, published(e))).collect();
    assert_eq!(lines(SetFormat::Text), want);
    let timed = |e: &SetEntry, at: &dyn Fn(u32) -> String| match e.start_seconds {
        Some(t) => at(t),
        None => published(e),
    };
    let want: Vec<String> =
        s.tracks.iter().map(|e| timed(e, &|t| format!("{} - {:0>8}", published(e), clock_of(t)))).collect();
    assert_eq!(lines(SetFormat::Mixcloud), want);
    let want: Vec<String> =
        s.tracks.iter().map(|e| timed(e, &|t| format!("[{}] {}", clock_of(t), published(e)))).collect();
    assert_eq!(lines(SetFormat::Tracklists), want);

    // An M3U8 lists files on a drive, so with none named it's refused for that.
    let e = render(&s, SetFormat::M3u8, &prefs, None).unwrap_err();
    assert_eq!(e.to_string(), "an M3U8 lists files on a drive: name the drive");

    let csv = render(&s, SetFormat::Csv, &prefs, None).unwrap().text;
    let rows: Vec<Vec<String>> = crate::import::read_table(&csv, ',', true)
        .into_iter()
        .map(|r| r.fields.iter().map(|f| f.trim_end_matches('\r').to_string()).collect())
        .collect();
    assert_eq!(rows.len(), s.tracks.len() + 1);
    for (i, (row, e)) in rows[1..].iter().zip(&s.tracks).enumerate() {
        let start = e.start_seconds.map(clock_of).unwrap_or_default();
        let bpm = if e.bpm > 0.0 { format!("{:.2}", e.bpm) } else { String::new() };
        let year = if e.year > 0 { e.year.to_string() } else { String::new() };
        let unnamed = if e.unnamed { "yes" } else { "" };
        let want =
            [&(i + 1).to_string(), &start, &e.artist, &e.title, &e.album, &e.label, "", &bpm, "", "", &year, unnamed];
        assert_eq!(row, &want, "{csv}");
    }

    let missing = s.tracks.iter().position(|e| e.start_seconds.is_none());
    let starts: Vec<u32> = s.tracks.iter().filter_map(|e| e.start_seconds).collect();
    let late = |starts: &[u32]| -> Vec<String> {
        (1..starts.len())
            .filter(|&i| starts[i] <= starts[i - 1])
            .map(|i| {
                format!(
                    "track {} ({}) doesn't start after track {} ({})",
                    i + 1,
                    clock_of(starts[i]),
                    i,
                    clock_of(starts[i - 1])
                )
            })
            .collect()
    };
    let refused = |what: &str, i: usize| {
        format!(
            "track {} ({}) has no start time, and {what} needs one for every track: attach the recording",
            i + 1,
            s.tracks[i].name()
        )
    };
    match (missing, render(&s, SetFormat::Youtube, &prefs, None)) {
        (Some(i), Err(e)) => assert_eq!(e.to_string(), refused("YouTube", i)),
        (None, Ok(r)) => {
            let mut chapters = starts.clone();
            if let Some(first) = chapters.first_mut() {
                *first = 0;
            }
            let want: Vec<String> = s
                .tracks
                .iter()
                .zip(&chapters)
                .map(|(e, &t)| match t < 3600 {
                    true => format!("{:02}:{:02} {}", t / 60, t % 60, published(e)),
                    false => format!("{} {}", clock_of(t), published(e)),
                })
                .collect();
            assert_eq!(r.text.lines().collect::<Vec<_>>(), want);
            let mut warnings = Vec::new();
            if chapters.len() < 3 {
                warnings.push(format!("YouTube needs at least 3 chapters; this has {}", chapters.len()));
            }
            warnings.extend(late(&chapters));
            warnings.extend(
                (1..chapters.len())
                    .filter(|&i| chapters[i] > chapters[i - 1] && chapters[i] - chapters[i - 1] < 10)
                    .map(|i| {
                        format!("chapter {i} is {} seconds long; YouTube needs 10", chapters[i] - chapters[i - 1])
                    }),
            );
            assert_eq!(r.warnings, warnings);
        }
        (missing, got) => panic!("track {missing:?} has no start, and YouTube gave {got:?}"),
    }
    match (&s.recording, missing, render(&s, SetFormat::Cue, &prefs, None)) {
        (None, _, Err(e)) => {
            assert_eq!(e.to_string(), "a CUE sheet indexes the recording: attach it to the set first")
        }
        (Some(_), Some(i), Err(e)) => assert_eq!(e.to_string(), refused("a CUE sheet", i)),
        (Some(rec), None, Ok(r)) => {
            let name = rec.path.file_name().unwrap().to_string_lossy();
            let kind = match name.rsplit_once('.').unwrap().1.to_lowercase().as_str() {
                "mp3" => "MP3",
                "aiff" => "AIFF",
                _ => "WAVE",
            };
            assert_eq!(r.text.lines().nth(1).unwrap(), format!("FILE \"{name}\" {kind}"));
            let tracks: Vec<&str> = r.text.lines().filter(|l| l.starts_with("  TRACK ")).collect();
            assert_eq!(tracks.len(), s.tracks.len());
            let titles: Vec<&str> = r.text.lines().filter_map(|l| l.strip_prefix("    TITLE ")).collect();
            let want: Vec<String> = s
                .tracks
                .iter()
                .map(|e| match named(e, &prefs) {
                    true => format!("\"{}\"", e.title.replace('"', "'")),
                    false => "\"ID\"".into(),
                })
                .collect();
            assert_eq!(titles, want);
            let indexes: Vec<u32> = r
                .text
                .lines()
                .filter_map(|l| l.strip_prefix("    INDEX 01 "))
                .map(|t| {
                    let parts: Vec<u32> = t.split(':').map(|p| p.parse().unwrap()).collect();
                    assert_eq!(parts[2], 0, "frames");
                    parts[0] * 60 + parts[1]
                })
                .collect();
            assert_eq!(indexes, starts);
            assert_eq!(r.warnings, late(&starts));
        }
        (rec, missing, got) => panic!("recording {rec:?}, track {missing:?} without a start: CUE gave {got:?}"),
    }
}

/// Mixcloud's rules, as a model counts them: over 15 minutes, a release on more than 3 tracks
/// or more than 2 in a row, an artist credited on more than 4 or more than 3 in a row, each
/// named as first spelled (ignoring case); under 15, the one warning about length.
#[hegel::test(test_cases = 300)]
fn mixcloud_rules_are_counted_as_the_model_does(tc: TestCase) {
    let s = some_set(&tc, false);
    let length = match s.tracks.last() {
        Some(e) if e.start_seconds.is_some() => e.start_seconds.unwrap() + e.duration_secs,
        _ => s.tracks.iter().map(|e| e.duration_secs).sum(),
    };
    let got = mixcloud_check(&s);
    if length > 0 && length < 900 {
        assert_eq!(got.len(), 1);
        assert!(got[0].starts_with(&format!("it's {} minutes long", length / 60)), "{got:?}");
        return;
    }
    let releases: Vec<Vec<String>> =
        s.tracks.iter().map(|e| Some(e.album.clone()).filter(|a| !a.is_empty()).into_iter().collect()).collect();
    let artists: Vec<Vec<String>> = s.tracks.iter().map(|e| artist_credits(&e.artist)).collect();
    let model = |tracks: &[Vec<String>], what: &dyn Fn(&str) -> String, most: usize, in_a_row: usize| {
        let mut names: Vec<(String, String)> = Vec::new();
        for c in tracks.iter().flatten() {
            if !names.iter().any(|n| n.0 == normalize(c)) {
                names.push((normalize(c), c.clone()));
            }
        }
        let has = |i: usize, key: &str| tracks[i].iter().any(|c| normalize(c) == key);
        let limited = "before it limits the show to subscribers";
        let mut out: Vec<String> = names
            .iter()
            .map(|(key, name)| (name, (0..tracks.len()).filter(|&i| has(i, key)).count()))
            .filter(|(_, n)| *n > most)
            .map(|(name, n)| format!("{n} tracks {}: Mixcloud allows {most} {limited}", what(name)))
            .collect();
        // Runs, in the order they end, then the order names first appear.
        let mut runs: Vec<(usize, usize, String)> = Vec::new();
        for (k, (key, name)) in names.iter().enumerate() {
            let mut i = 0;
            while i < tracks.len() {
                if !has(i, key) {
                    i += 1;
                    continue;
                }
                let from = i;
                while i < tracks.len() && has(i, key) {
                    i += 1;
                }
                if i - from > in_a_row {
                    let msg = format!(
                        "tracks {}-{i} are {}: Mixcloud allows {in_a_row} in a row {limited}",
                        from + 1,
                        what(name)
                    );
                    runs.push((i, k, msg));
                }
            }
        }
        runs.sort_by_key(|r| (r.0, r.1));
        out.extend(runs.into_iter().map(|r| r.2));
        out
    };
    let mut want = model(&releases, &|r| format!("from {r:?}"), 3, 2);
    want.extend(model(&artists, &|a| format!("by {a}"), 4, 3));
    assert_eq!(got, want);
}

/// An M3U8 lists the set's tracks found on the drive by file (in any case), in set order, and
/// warns about each of the others by its place in the set.
#[hegel::test(test_cases = 300)]
fn m3u8_lists_the_tracks_found_on_the_drive(tc: TestCase) {
    let lib = Library {
        tracks: (1..=4).map(|n| track(n, "Artist", &format!("Track {n}"), &format!("/Contents/{n}.mp3"))).collect(),
        ..Default::default()
    };
    let mut s = set("3m2abcdefgh22", "2026-10-03", tc.draw(gs::sampled_from(vec![None, Some("Friday")])));
    let files: Vec<Option<u32>> =
        tc.draw(gs::vecs(gs::sampled_from(vec![None, Some(1), Some(2), Some(4), Some(7)])).max_size(6));
    let upper: Vec<bool> = files.iter().map(|_| tc.draw(gs::booleans())).collect();
    s.tracks = files
        .iter()
        .zip(&upper)
        .map(|(f, &up)| SetEntry {
            file: f.map(|n| if up { format!("/CONTENTS/{n}.MP3") } else { format!("/Contents/{n}.mp3") }),
            ..SetEntry::added("Someone - Dubplate")
        })
        .collect();
    let locator = Locator { style: PathStyle::Drive, drive_root: "/Volumes/USB".into(), base: "/x".into() };
    let r = render(&s, SetFormat::M3u8, &ProfilePrefs::default(), Some((&lib, &locator))).unwrap();
    let found: Vec<String> =
        files.iter().flatten().filter(|&&n| n <= 4).map(|n| format!("/Contents/{n}.mp3")).collect();
    let listed: Vec<&str> = r.text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!((r.tracks, listed), (found.len(), found.iter().map(String::as_str).collect()));
    let heading = format!("#PLAYLIST:{}", s.heading());
    assert_eq!(r.text.lines().take(2).collect::<Vec<_>>(), ["#EXTM3U", heading.as_str()]);
    let warnings: Vec<String> = files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.is_none_or(|n| n > 4))
        .map(|(i, _)| format!("{}. Someone - Dubplate isn't on the drive: left out", i + 1))
        .collect();
    assert_eq!(r.warnings, warnings);
}
