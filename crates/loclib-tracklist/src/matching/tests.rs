use super::*;
use crate::parse;
use loclib_model::Track;
use loclib_model::test_util::track;

fn lib(tracks: Vec<Track>) -> Library {
    Library { tracks, ..Default::default() }
}

fn with(mut t: Track, f: impl FnOnce(&mut Track)) -> Track {
    f(&mut t);
    t
}

fn entry(artist: &str, title: &str) -> Entry {
    Entry { title: title.into(), artist: artist.into(), ..Default::default() }
}

fn status(lib: &Library, e: Entry) -> Status {
    Matcher::new(lib).find(&e)
}

fn found(id: u32, how: MatchKind) -> Status {
    Status::Found { track_id: id, how }
}

fn sample() -> Library {
    lib(vec![
        track(1, "Daft Punk", "One More Time", "/Contents/Daft Punk/Discovery/01 One More Time.mp3"),
        with(track(2, "deadmau5", "Strobe", "/Contents/deadmau5/For Lack/Strobe.mp3"), |t| {
            t.mix_name = "Original Mix".into();
            t.duration_secs = 637;
        }),
        with(track(3, "deadmau5", "Strobe (Radio Edit)", "/Contents/deadmau5/Radio/Strobe.mp3"), |t| {
            t.duration_secs = 214;
        }),
        with(track(4, "Avicii", "Levels (Skrillex Remix)", "/Contents/Avicii/Levels/Levels Skrillex.mp3"), |t| {
            t.isrc = "SE-AT1-11-00123".into();
        }),
        track(5, "Kölsch", "Grey", "/Contents/Kolsch/1983/Grey.mp3"),
        track(6, "Daft Punk feat. Pharrell Williams", "Get Lucky", "/Contents/Daft Punk/RAM/Get Lucky.mp3"),
        track(7, "Above & Beyond", "Sun & Moon", "/Contents/Above & Beyond/Group Therapy/Sun & Moon.mp3"),
        track(8, "Eric Prydz", "Opus", "/Contents/Eric Prydz/Opus/01 Intro.mp3"),
        track(9, "Bicep", "Glue", "/Contents/Bicep/Bicep/01 Intro.mp3"),
    ])
}

#[test]
fn matches_by_path_suffix() {
    let l = sample();
    let mut e = entry("", "");
    e.path = Some("/Users/me/Music/deadmau5/For Lack/strobe.MP3".into());
    assert_eq!(status(&l, e.clone()), found(2, MatchKind::Path));
    // Windows path, case and slashes differ, NFD vs NFC.
    e.path = Some("D:\\Music\\DAFT PUNK\\Discovery\\01 One More Time.mp3".into());
    assert_eq!(status(&l, e.clone()), found(1, MatchKind::Path));
    e.path = Some("/Contents/Kolsch/1983/Grey.mp3".into());
    assert_eq!(status(&l, e.clone()), found(5, MatchKind::Path));
    // Two "01 Intro.mp3": the folder decides...
    e.path = Some("/Music/Bicep/01 Intro.mp3".into());
    assert_eq!(status(&l, e.clone()), found(9, MatchKind::Path));
    // ...and without one it's ambiguous.
    e.path = Some("01 Intro.mp3".into());
    assert_eq!(status(&l, e.clone()), Status::Ambiguous { candidates: vec![8, 9] });
    // A name-only match must share a word with the entry: this one goes on to metadata.
    e.path = Some("01 Intro.mp3".into());
    e.artist = "Eric Prydz".into();
    e.title = "Opus".into();
    assert_eq!(status(&l, e), found(8, MatchKind::Path));
    let mut e = entry("Daft Punk", "Get Lucky");
    e.path = Some("/elsewhere/Strobe.mp3".into());
    assert_eq!(status(&l, e), found(6, MatchKind::ArtistTitle));
}

#[test]
fn matches_by_isrc_then_metadata() {
    let l = sample();
    let mut e = entry("Someone", "Something Else");
    e.isrc = Some("seat11100123".into());
    assert_eq!(status(&l, e), found(4, MatchKind::Isrc));
    // Path beats ISRC.
    let mut e = entry("", "");
    e.isrc = Some("SEAT11100123".into());
    e.path = Some("/x/Bicep/01 Intro.mp3".into());
    assert_eq!(status(&l, e), found(9, MatchKind::Path));
}

#[test]
fn matches_by_artist_title_and_duration() {
    let l = sample();
    // Both Strobes are "Strobe" by deadmau5; the duration picks one.
    let mut e = entry("deadmau5", "Strobe");
    e.duration_secs = Some(215);
    assert_eq!(status(&l, e.clone()), found(3, MatchKind::ArtistTitleDuration));
    e.duration_secs = Some(636);
    assert_eq!(status(&l, e.clone()), found(2, MatchKind::ArtistTitleDuration));
    // No duration: "Strobe" with no version is the original mix, not the radio edit.
    e.duration_secs = None;
    assert_eq!(status(&l, e.clone()), found(2, MatchKind::ArtistTitle));
    // Spotify's naming.
    let e = entry("deadmau5", "Strobe - Radio Edit");
    assert_eq!(status(&l, e), found(3, MatchKind::ArtistTitle));
    // A duration matching neither: still ambiguous between versions it can't tell apart.
    let mut e = entry("deadmau5", "Strobe (Club Mix)");
    e.duration_secs = Some(400);
    assert_eq!(status(&l, e), Status::Ambiguous { candidates: vec![2, 3] });
}

#[test]
fn normalised_matches() {
    let l = sample();
    // Accents, case, featured artists, "&" vs "and", Spotify credit lists.
    assert_eq!(status(&l, entry("KOLSCH", "grey")), found(5, MatchKind::ArtistTitle));
    assert_eq!(
        status(&l, entry("Daft Punk, Pharrell Williams, Nile Rodgers", "Get Lucky - Radio Edit")),
        found(6, MatchKind::ArtistTitle)
    );
    assert_eq!(status(&l, entry("Daft Punk", "Get Lucky (feat. Pharrell Williams)")), found(6, MatchKind::ArtistTitle));
    assert_eq!(status(&l, entry("Above and Beyond", "Sun and Moon (Original Mix)")), found(7, MatchKind::ArtistTitle));
    assert_eq!(status(&l, entry("Avicii", "Levels (Skrillex RMX)")), found(4, MatchKind::ArtistTitle));
    // Title only.
    assert_eq!(status(&l, entry("", "Glue")), found(9, MatchKind::Title));
    // The Beatport-style mix column counts as part of the title.
    let mut e = entry("Eric Prydz", "Opus");
    e.mix = Some("Original Mix".into());
    assert_eq!(status(&l, e), found(8, MatchKind::ArtistTitle));
}

#[test]
fn versions_are_never_confused() {
    let l = sample();
    // The original of Levels isn't in the library; the Skrillex remix is offered, not picked.
    assert_eq!(status(&l, entry("Avicii", "Levels")), Status::Ambiguous { candidates: vec![4] });
    // A remix that isn't there doesn't become the original.
    assert_eq!(status(&l, entry("Bicep", "Glue (Ewan McVicar Remix)")), Status::Ambiguous { candidates: vec![9] });
    assert_eq!(status(&l, entry("Bicep", "Glue - Live")), Status::Ambiguous { candidates: vec![9] });
}

/// One remix however it's written is found; another remixer's is only offered, even when
/// the two share every other word. Generic versions (original, extended, radio edit) are one
/// recording, so the only one there is found whichever is asked for.
#[test]
fn remixes_and_generic_versions() {
    let remix = with(track(1, "deadmau5", "Strobe (Aurora Remix)", "/Contents/a.mp3"), |t| t.duration_secs = 400);
    let l = lib(vec![remix]);
    let with_mix = |title: &str, mix: &str| Entry { mix: Some(mix.into()), ..entry("deadmau5", title) };
    for title in ["Strobe [AURORA remix]", "strobe - Auróra Rmx", "Strobe (Aurora Remix)"] {
        assert_eq!(status(&l, entry("Deadmau5", title)), found(1, MatchKind::ArtistTitle), "{title}");
    }
    assert_eq!(status(&l, with_mix("Strobe", "Aurora Remix")), found(1, MatchKind::ArtistTitle));
    // A typo in the remixer is still that remix.
    assert_eq!(status(&l, entry("deadmau5", "Strobe (Aurorra Remix)")), found(1, MatchKind::Fuzzy));
    // Another remixer: offered, never found, however the rest agrees.
    let other = Status::Ambiguous { candidates: vec![1] };
    assert_eq!(status(&l, entry("deadmau5", "Strobe (Basalt Remix)")), other);
    assert_eq!(status(&l, with_mix("Strobe", "Basalt Remix")), other);
    assert_eq!(status(&l, Entry { duration_secs: Some(400), ..entry("deadmau5", "Strobe (Basalt Remix)") }), other);
    // Part of a remixer's name could be that remix or another's: offered too.
    let l = lib(vec![track(1, "Bicep", "Glue (Ewan McVicar Remix)", "/Contents/g.mp3")]);
    assert_eq!(status(&l, entry("Bicep", "Glue (McVicar Remix)")), Status::Ambiguous { candidates: vec![1] });

    // "Original Mix" names no version, written in the title or the mix field.
    let plain = lib(vec![track(1, "deadmau5", "Strobe", "/Contents/s.mp3")]);
    let original = lib(vec![with(track(1, "deadmau5", "Strobe", "/Contents/s.mp3"), |t| {
        t.mix_name = "Original Mix".into();
    })]);
    for l in [&plain, &original] {
        assert_eq!(status(l, entry("deadmau5", "Strobe")), found(1, MatchKind::ArtistTitle));
        assert_eq!(status(l, entry("deadmau5", "Strobe (Original Mix)")), found(1, MatchKind::ArtistTitle));
        assert_eq!(status(l, with_mix("Strobe", "Original Mix")), found(1, MatchKind::ArtistTitle));
    }
    // An extended mix asked for, only the radio edit there (or the reverse): that's the one.
    let radio = lib(vec![track(1, "deadmau5", "Strobe (Radio Edit)", "/Contents/r.mp3")]);
    assert_eq!(status(&radio, entry("deadmau5", "Strobe (Extended Mix)")), found(1, MatchKind::ArtistTitle));
    let extended = lib(vec![track(1, "deadmau5", "Strobe - Extended Mix", "/Contents/e.mp3")]);
    assert_eq!(status(&extended, entry("deadmau5", "Strobe - Radio Edit")), found(1, MatchKind::ArtistTitle));
}

#[test]
fn fuzzy_matches() {
    let l = sample();
    // A typo.
    assert_eq!(status(&l, entry("Daft Punk", "One More Tme")), found(1, MatchKind::Fuzzy));
    assert_eq!(status(&l, entry("Daft Punkk", "One More Time")), found(1, MatchKind::Fuzzy));
    // Swapped.
    assert_eq!(status(&l, entry("One More Time", "Daft Punk")), found(1, MatchKind::Fuzzy));
    // Everything in the title.
    assert_eq!(status(&l, entry("", "Eric Prydz Opus")), found(8, MatchKind::Fuzzy));
    // A track tagged with the artist inside its title.
    let l2 = lib(vec![track(1, "", "Moby - Porcelain", "/Contents/x.mp3"), track(2, "Moby", "Go", "/Contents/y.mp3")]);
    assert_eq!(status(&l2, entry("Moby", "Porcelain")), found(1, MatchKind::Fuzzy));
    // Nothing like it.
    assert_eq!(status(&l, entry("Burial", "Archangel")), Status::Missing);
    assert_eq!(status(&l, entry("", "")), Status::Missing);
    // Same title, different artist: offered at most, never found.
    assert!(!matches!(status(&l, entry("Pink Floyd", "Opus")), Status::Found { .. }));
}

/// Regression: an entry with no artist took the only track of its title even when its
/// duration was far off, so a title-only list filled a playlist with unrelated tracks.
#[test]
fn weak_matches_need_the_duration_to_agree() {
    let l = sample();
    let timed = |artist: &str, title: &str, secs| Entry { duration_secs: Some(secs), ..entry(artist, title) };
    assert_eq!(status(&l, timed("", "Glue", 301)), found(9, MatchKind::Title));
    assert_eq!(status(&l, timed("", "Glue", 240)), Status::Ambiguous { candidates: vec![9] });
    assert_eq!(status(&l, timed("", "Eric Prydz Opus", 299)), found(8, MatchKind::Fuzzy));
    assert_eq!(status(&l, timed("", "Eric Prydz Opus", 1500)), Status::Ambiguous { candidates: vec![8] });
    // The artist and title together are enough.
    assert_eq!(status(&l, timed("Bicep", "Glue", 240)), found(9, MatchKind::ArtistTitle));
}

#[test]
fn duplicates_are_ambiguous() {
    let l = lib(vec![
        track(1, "Moby", "Porcelain", "/Contents/Moby/Play/Porcelain.mp3"),
        track(2, "Moby", "Porcelain", "/Contents/Moby/Best Of/Porcelain.mp3"),
        track(3, "Moby", "Go", "/Contents/Moby/Go.mp3"),
        track(4, "Moby", "Go (Woodtick Mix)", "/Contents/Moby/Go2.mp3"),
        with(track(5, "Bicep", "Glu", "/Contents/Bicep/a.mp3"), |t| t.isrc = "X1".into()),
        with(track(6, "Bicep", "Glu", "/Contents/Bicep/b.mp3"), |t| t.isrc = "X1".into()),
    ]);
    assert_eq!(status(&l, entry("Moby", "Porcelain")), Status::Ambiguous { candidates: vec![1, 2] });
    let mut e = entry("", "");
    e.path = Some("/Music/Porcelain.mp3".into());
    assert_eq!(status(&l, e), Status::Ambiguous { candidates: vec![1, 2] });
    let mut e = entry("Bicep", "Glu");
    e.isrc = Some("X1".into());
    assert_eq!(status(&l, e), Status::Ambiguous { candidates: vec![5, 6] });
    // Near-equal fuzzy candidates are never picked between.
    assert_eq!(status(&l, entry("Moby", "Porcelian")), Status::Ambiguous { candidates: vec![1, 2] });
    assert_eq!(status(&l, entry("Moby", "Go")), found(3, MatchKind::ArtistTitle));
    assert_eq!(status(&l, entry("Moby", "Go - Woodtick Mix")), found(4, MatchKind::ArtistTitle));
}

#[test]
fn asides_and_junk_mix_names() {
    let l = lib(vec![
        with(track(1, "Heaven & Earth", "Prescription", "/Contents/a.mp3"), |t| {
            t.mix_name = "exclusivesource4dj.com".into();
        }),
        track(2, "D'marc Cantu", "call home first", "/Contents/b.aif"),
        track(3, "D'marc Cantu", "call home first (DVSMMM)", "/Contents/c.aiff"),
    ]);
    // A mix name that names no version doesn't make a different recording.
    assert_eq!(status(&l, entry("Heaven and Earth", "Prescription")), found(1, MatchKind::ArtistTitle));
    // Exact titles pick their own version...
    assert_eq!(status(&l, entry("D'marc Cantu", "call home first")), found(2, MatchKind::ArtistTitle));
    assert_eq!(status(&l, entry("D'Marc Cantu", "Call Home First (DVSMMM)")), found(3, MatchKind::ArtistTitle));
    // ...and a mangled one still weighs the aside rather than preferring the bare title.
    assert_eq!(status(&l, entry("D'marc Cantu", "call home first (DVSMMM)x")), found(3, MatchKind::Fuzzy));
}

#[test]
fn whole_file_round_trip() {
    let l = sample();
    let m3u = "#EXTM3U\n#EXTINF:320,Daft Punk - One More Time\n/Users/me/Music/Daft Punk/Discovery/01 One More Time.mp3\n\
                   #EXTINF:214,deadmau5 - Strobe (Radio Edit)\n/Users/me/Music/strobe_radio.mp3\n\
                   #EXTINF:300,Burial - Archangel\n/Users/me/Music/Archangel.mp3\n";
    let entries = parse(m3u.as_bytes(), Some("x.m3u")).unwrap();
    let report = Report::new(Format::M3u, match_entries(&l, &entries));
    assert_eq!(report.summary, Summary { entries: 3, found: 2, ambiguous: 0, missing: 1 });
    let ids: Vec<u32> = report.entries.iter().filter_map(|m| m.status.track_id()).collect();
    assert_eq!(ids, [1, 3]);
    assert_eq!(report.entries[1].status, found(3, MatchKind::ArtistTitleDuration));

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["format"], "m3u");
    assert_eq!(json["entries"][0]["status"], "found");
    assert_eq!(json["entries"][0]["how"], "path");
    assert_eq!(json["entries"][0]["track_id"], 1);
    assert_eq!(json["entries"][0]["entry"]["line"], 3);
    assert_eq!(json["entries"][2]["status"], "missing");
    let back: Report = serde_json::from_value(json).unwrap();
    assert_eq!(back, report);
    let amb = serde_json::to_value(Status::Ambiguous { candidates: vec![1, 2] }).unwrap();
    assert_eq!(amb, serde_json::json!({ "status": "ambiguous", "candidates": [1, 2] }));
}

#[test]
fn large_libraries_stay_fast() {
    // 10,000 tracks x 1,000 entries, a mix of every strategy and of misses.
    let words = ["night", "light", "dream", "fire", "love", "deep", "sun", "moon", "city", "drive", "echo", "wave"];
    let tracks: Vec<Track> = (0..10_000u32)
        .map(|i| {
            let w = |k: u32| words[((i / k) % words.len() as u32) as usize];
            let artist = format!("Artist {} {}", w(7), i % 997);
            let title = format!("{} {} {}", w(1), w(3), i);
            with(track(i + 1, &artist, &title, &format!("/Contents/{artist}/{title}.mp3")), |t| {
                t.duration_secs = 180 + i % 300;
            })
        })
        .collect();
    let l = lib(tracks);
    let entries: Vec<Entry> = (0..1_000u32)
        .map(|k| {
            let t = &l.tracks[(k * 37 % 10_000) as usize];
            match k % 4 {
                0 => Entry { path: Some(format!("/Users/me/Music{}", &t.file_path[9..])), ..Default::default() },
                1 => entry(&t.artist, &t.title),
                2 => entry(&t.artist, &format!("{}x", t.title)),
                _ => entry("Nobody", &format!("Nothing {k}")),
            }
        })
        .collect();
    let start = std::time::Instant::now();
    let matched = match_entries(&l, &entries);
    let elapsed = start.elapsed();
    let s = Summary::of(&matched);
    assert_eq!(s.entries, 1_000);
    assert!(s.found >= 500, "{s:?}");
    // Generous for unoptimised builds on slow CI machines; release takes a fraction of this.
    assert!(elapsed.as_secs() < 20, "{elapsed:?}");
}
