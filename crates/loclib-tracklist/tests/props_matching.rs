//! Properties of finding a tracklist's entries in a library.

use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::model::{Library, Track};
use loclib_model::test_util::track;
use loclib_tracklist::{Entry, MatchKind, Matched, Matcher, Report, Status, Summary, match_entries};

// Two vocabularies, each word 6-8 letters and at least three edits from every other word (and
// from the version and stop words), so a one-letter typo can only be read as the word it came
// from, and an artist can't be taken for a title.
const TITLE_WORDS: &[&str] = &[
    "strobe", "glacier", "harbour", "velvet", "meadow", "copper", "lantern", "orchid", "thunder", "silence", "marble",
    "falcon", "canyon", "juniper", "saffron", "quartz", "embers", "tundra", "nectar", "pebble",
];
const ARTIST_WORDS: &[&str] = &[
    "aurora", "basalt", "cobalt", "delphi", "kestrel", "lumen", "nomad", "oberon", "pixie", "rhombus", "wicker",
    "topaz", "umbra", "vesper", "wyvern", "zephyr",
];

/// 1 to `max` distinct words of `vocab`, capitalised, in a fixed order (so the same words always
/// make the same name, and two names never differ only in word order).
#[hegel::test_helper]
fn words(tc: &TestCase, vocab: &[&'static str], max: usize) -> String {
    let mut w: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(vocab.to_vec())).min_size(1).max_size(max).unique(true));
    w.sort_unstable();
    w.iter().map(|w| format!("{}{}", w[..1].to_uppercase(), &w[1..])).collect::<Vec<_>>().join(" ")
}

/// `s` as another list might spell it: any case, an accent on a vowel, doubled spaces.
#[hegel::test_helper]
fn respelled(tc: &TestCase, s: &str) -> String {
    let s = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => s.to_string(),
        1 => s.to_uppercase(),
        _ => s.to_lowercase(),
    };
    let s = if tc.draw(gs::booleans()) { s.replacen('e', "é", 1).replacen('A', "Á", 1) } else { s };
    if tc.draw(gs::booleans()) { s.replace(' ', "  ") } else { s }
}

/// `title` with a one-letter typo (a letter changed, added or dropped) in one of its words.
#[hegel::test_helper]
fn typo(tc: &TestCase, title: &str) -> String {
    let mut words: Vec<String> = title.split(' ').map(String::from).collect();
    let k = tc.draw(gs::integers::<usize>().max_value(words.len() - 1));
    let w: Vec<char> = words[k].chars().collect();
    let i = tc.draw(gs::integers::<usize>().min_value(1).max_value(w.len() - 1));
    let mut out = w.clone();
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => out[i] = if w[i] == 'x' { 'y' } else { 'x' },
        1 => out.insert(i, 'x'),
        _ => {
            out.remove(i);
        }
    }
    words[k] = out.into_iter().collect();
    words.join(" ")
}

/// A track by `artist` called `title` in its own file, with a unique ISRC.
fn track_of(id: u32, artist: &str, title: &str, secs: u32) -> Track {
    let mut t = track(id, artist, title, &format!("/Contents/{artist}/{title}.mp3"));
    t.isrc = format!("QZ-ES1-{id:07}");
    t.duration_secs = secs;
    t
}

/// `min` to `max` tracks (fewer when a draw repeats one), no two by the same artist with the
/// same title.
#[hegel::test_helper]
fn library(tc: &TestCase, min: usize, max: usize) -> Library {
    let n = tc.draw(gs::integers::<usize>().min_value(min).max_value(max));
    let mut tracks: Vec<Track> = Vec::new();
    for _ in 0..n {
        let (artist, title) = (words(tc, ARTIST_WORDS, 2), words(tc, TITLE_WORDS, 3));
        if tracks.iter().any(|t| t.artist == artist && t.title == title) {
            continue;
        }
        let secs = tc.draw(gs::integers::<u32>().min_value(60).max_value(600));
        tracks.push(track_of(tracks.len() as u32 + 1, &artist, &title, secs));
    }
    Library { tracks, ..Default::default() }
}

fn entry(artist: &str, title: &str) -> Entry {
    Entry { artist: artist.into(), title: title.into(), ..Default::default() }
}

/// An entry naming a track's artist and title, however it's spelled, with a generic version
/// ("Original Mix", "Radio Edit") or a guest credited, is that track: by artist, title and
/// duration when its length is within two seconds, otherwise by artist and title.
#[hegel::test(test_cases = 150)]
fn named_tracks_are_found(tc: TestCase) {
    let lib = library(&tc, 1, 30);
    let matcher = Matcher::new(&lib);
    let t = &lib.tracks[tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1))];
    let version = tc.draw(gs::sampled_from(vec!["", " (Original Mix)", " - Radio Edit", " [Extended Mix]"]));
    // "Mystery Guest" isn't anyone in the library.
    let guest = tc.draw(gs::sampled_from(vec!["", " feat. Mystery Guest", ", Mystery Guest", " & Mystery Guest"]));
    let mut e =
        entry(&format!("{}{guest}", respelled(&tc, &t.artist)), &format!("{}{version}", respelled(&tc, &t.title)));
    let off = tc.draw(gs::optional(gs::integers::<i64>().min_value(-59).max_value(120)));
    e.duration_secs = off.map(|o| (i64::from(t.duration_secs) + o) as u32);
    let near = off.is_some_and(|o| o.abs() <= 2);
    let how = if near { MatchKind::ArtistTitleDuration } else { MatchKind::ArtistTitle };
    assert_eq!(matcher.find(&e), Status::Found { track_id: t.id, how }, "{e:?}");
}

/// Of several files of one artist's title, lengths at least five seconds apart, an entry whose
/// length is within two seconds of one of them is that one, by artist, title and duration.
#[hegel::test(test_cases = 100)]
fn lengths_tell_copies_of_a_track_apart(tc: TestCase) {
    let (artist, title) = (words(&tc, ARTIST_WORDS, 2), words(&tc, TITLE_WORDS, 3));
    let gaps: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().min_value(5).max_value(90)).min_size(2).max_size(4));
    let mut secs = 60;
    let tracks: Vec<Track> = (1..)
        .zip(&gaps)
        .map(|(id, gap)| {
            secs += gap;
            let mut t = track(id, &artist, &title, &format!("/Contents/{artist}/{title} {id}.mp3"));
            t.duration_secs = secs;
            t
        })
        .collect();
    let lib = Library { tracks, ..Default::default() };
    let t = &lib.tracks[tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1))];
    let mut e = entry(&respelled(&tc, &artist), &respelled(&tc, &title));
    e.duration_secs =
        Some((i64::from(t.duration_secs) + tc.draw(gs::integers::<i64>().min_value(-2).max_value(2))) as u32);
    let found = Matcher::new(&lib).find(&e);
    assert_eq!(found, Status::Found { track_id: t.id, how: MatchKind::ArtistTitleDuration }, "{e:?}");
}

/// An entry naming only a title is the one track with that title, unless its length is more
/// than two seconds off (then the track is only offered). When several have the title, the
/// length picks one, or the entry is ambiguous between exactly those it can't tell apart, in
/// library order.
#[hegel::test(test_cases = 150)]
fn titles_alone_find_the_tracks_with_that_title(tc: TestCase) {
    let lib = library(&tc, 1, 30);
    let t = &lib.tracks[tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1))];
    let off = tc.draw(gs::optional(gs::integers::<i64>().min_value(-5).max_value(5)));
    let secs = off.map(|o| (i64::from(t.duration_secs) + o) as u32);
    let e = Entry { duration_secs: secs, ..entry("", &respelled(&tc, &t.title)) };
    let same: Vec<&Track> = lib.tracks.iter().filter(|u| u.title == t.title).collect();
    let ids = |v: &[&Track]| v.iter().map(|u| u.id).collect::<Vec<_>>();
    let near: Vec<&Track> =
        same.iter().copied().filter(|u| secs.is_some_and(|s| u.duration_secs.abs_diff(s) <= 2)).collect();
    let want = match (same.as_slice(), near.as_slice()) {
        ([u], _) if secs.is_some() && near.is_empty() => Status::Ambiguous { candidates: vec![u.id] },
        ([u], _) | (_, [u]) => Status::Found { track_id: u.id, how: MatchKind::Title },
        (_, []) => Status::Ambiguous { candidates: ids(&same) },
        _ => Status::Ambiguous { candidates: ids(&near) },
    };
    assert_eq!(Matcher::new(&lib).find(&e), want);
}

/// Generic versions of one recording, labelled and spelt the ways lists do.
const GENERIC_VERSIONS: &[(&str, &[&str])] = &[
    ("original", &["", " (Original Mix)", " - Original Mix"]),
    ("extended", &[" (Extended Mix)", " [Extended Mix]", " - Extended"]),
    ("radio", &[" (Radio Edit)", " - Radio Edit", " [Radio Edit]"]),
    ("club", &[" (Club Mix)", " - Club Mix"]),
];

/// When the library has a track in several generic versions (extended, radio edit...), the one
/// the entry names is found; a version the library lacks leaves the choice open, unless
/// there's only one.
#[hegel::test(test_cases = 150)]
fn generic_versions_pick_their_own(tc: TestCase) {
    let (artist, title) = (words(&tc, ARTIST_WORDS, 2), words(&tc, TITLE_WORDS, 2));
    let spelt = |tc: &TestCase, k: usize| tc.draw(gs::sampled_from(GENERIC_VERSIONS[k].1.to_vec()));
    let have: Vec<usize> = tc.draw(
        gs::vecs(gs::integers::<usize>().max_value(GENERIC_VERSIONS.len() - 1)).min_size(1).max_size(4).unique(true),
    );
    let tracks: Vec<Track> = have
        .iter()
        .enumerate()
        .map(|(i, &k)| {
            let mut t = track_of(i as u32 + 1, &artist, &format!("{title}{}", spelt(&tc, k)), 300);
            t.file_path = format!("/Contents/{i}.mp3");
            t
        })
        .collect();
    let lib = Library { tracks, ..Default::default() };
    let asked = tc.draw(gs::integers::<usize>().max_value(GENERIC_VERSIONS.len() - 1));
    let e = entry(&respelled(&tc, &artist), &format!("{}{}", respelled(&tc, &title), spelt(&tc, asked)));
    let want = match (have.iter().position(|&k| k == asked), lib.tracks.as_slice()) {
        (Some(i), _) => Status::Found { track_id: i as u32 + 1, how: MatchKind::ArtistTitle },
        (None, [only]) => Status::Found { track_id: only.id, how: MatchKind::ArtistTitle },
        (None, _) => Status::Ambiguous { candidates: lib.tracks.iter().map(|t| t.id).collect() },
    };
    assert_eq!(Matcher::new(&lib).find(&e), want, "{e:?}");
}

/// A one-letter typo in the title still finds the track, as a fuzzy match.
#[hegel::test(test_cases = 150)]
fn typos_still_find_the_track(tc: TestCase) {
    let lib = library(&tc, 1, 30);
    let t = &lib.tracks[tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1))];
    let e = entry(&respelled(&tc, &t.artist), &typo(&tc, &t.title));
    assert_eq!(Matcher::new(&lib).find(&e), Status::Found { track_id: t.id, how: MatchKind::Fuzzy }, "{e:?}");
}

/// A file is found by the trailing folders and name of its path, however the list spells the
/// path. By the file name alone, an entry with nothing else to go on is whichever tracks have
/// that name.
#[hegel::test(test_cases = 150)]
fn files_are_found_by_their_path(tc: TestCase) {
    let lib = library(&tc, 1, 30);
    let t = &lib.tracks[tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1))];
    let matcher = Matcher::new(&lib);
    let windows = format!("D:\\Music\\{}\\{}.MP3", t.artist.to_uppercase(), t.title);
    let posix = format!("/Users/me/Music/{}/{}.mp3", t.artist.to_lowercase(), t.title);
    let path = tc.draw(gs::sampled_from(vec![windows, posix]));
    let e = Entry { path: Some(path), ..entry("", "") };
    assert_eq!(matcher.find(&e), Status::Found { track_id: t.id, how: MatchKind::Path });

    let same: Vec<u32> = lib.tracks.iter().filter(|u| u.title == t.title).map(|u| u.id).collect();
    let e = Entry { path: Some(format!("{}.mp3", t.title)), ..entry("", "") };
    let want = match same.as_slice() {
        [id] => Status::Found { track_id: *id, how: MatchKind::Path },
        _ => Status::Ambiguous { candidates: same.clone() },
    };
    assert_eq!(matcher.find(&e), want);
}

/// An ISRC finds its track however it's written (case, dashes, spaces), whatever the entry's
/// artist and title say.
#[hegel::test(test_cases = 100)]
fn isrcs_find_their_track(tc: TestCase) {
    let lib = library(&tc, 1, 30);
    let t = &lib.tracks[tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1))];
    let isrc = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => t.isrc.clone(),
        1 => t.isrc.replace('-', "").to_lowercase(),
        _ => t.isrc.replace('-', " "),
    };
    let e = Entry { isrc: Some(isrc), ..entry(&tc.draw(gs::text().max_size(10)), &tc.draw(gs::text().max_size(10))) };
    assert_eq!(Matcher::new(&lib).find(&e), Status::Found { track_id: t.id, how: MatchKind::Isrc });
}

/// A version of a track, written as a mix name, in brackets or after a dash ("Spotify style").
#[hegel::test_helper]
fn versioned(tc: &TestCase, title: &str, remixer: Option<&str>) -> (String, Option<String>) {
    let Some(r) = remixer else { return (title.to_string(), None) };
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => (format!("{title} ({r} Remix)"), None),
        1 => (format!("{title} [{r} Remix]"), None),
        2 => (format!("{title} - {r} Remix"), None),
        _ => (title.to_string(), Some(format!("{r} Remix"))),
    }
}

/// Versions aren't confused: asking for the original never finds a remix, asking for a remix
/// never finds the original or another remixer's, and the version asked for is found when the
/// library has it.
#[hegel::test(test_cases = 200)]
fn versions_are_never_confused(tc: TestCase) {
    let remixers = || gs::optional(gs::sampled_from(ARTIST_WORDS.to_vec()));
    let artist = words(&tc, ARTIST_WORDS, 1);
    let title = words(&tc, TITLE_WORDS, 2);
    let versions: Vec<Option<&str>> = tc.draw(gs::vecs(remixers()).min_size(1).max_size(4).unique(true));
    let tracks: Vec<Track> = versions
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let (t, mix) = versioned(&tc, &title, *v);
            let mut t = track_of(i as u32 + 1, &artist, &t, 300);
            t.file_path = format!("/Contents/{i}.mp3");
            t.mix_name = mix.unwrap_or_default();
            t
        })
        .collect();
    let lib = Library { tracks, ..Default::default() };
    let asked = tc.draw(remixers());
    let (t, mix) = versioned(&tc, &title, asked);
    let e = Entry { mix, ..entry(&artist, &t) };
    let got = Matcher::new(&lib).find(&e);
    tc.note(&format!("{versions:?} asked {asked:?}: {got:?}"));
    match versions.iter().position(|v| *v == asked) {
        Some(i) => assert_eq!(got, Status::Found { track_id: i as u32 + 1, how: MatchKind::ArtistTitle }),
        None => assert!(!matches!(got, Status::Found { .. }), "{got:?}"),
    }
}

/// An entry that may name a track of `lib`, the way lists get them wrong: exact, misspelt,
/// artist and title swapped or run together, no artist, only a path or an ISRC, or nothing in
/// the library at all.
#[hegel::test_helper]
fn entry_for(tc: &TestCase, lib: &Library) -> Entry {
    let i = tc.draw(gs::integers::<usize>().max_value(lib.tracks.len()));
    let Some(t) = lib.tracks.get(i) else {
        return entry(&words(tc, ARTIST_WORDS, 2), &words(tc, TITLE_WORDS, 3));
    };
    let mut e = match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => entry(&t.artist, &t.title),
        1 => entry(&t.artist, &typo(tc, &t.title)),
        2 => entry(&t.title, &t.artist),
        3 => entry("", &format!("{} {}", t.artist, t.title)),
        4 => entry("", &t.title),
        5 => Entry { path: Some(format!("{}.mp3", t.title)), ..entry("", "") },
        _ => Entry { isrc: Some(t.isrc.clone()), ..entry("", "") },
    };
    e.duration_secs = tc.draw(gs::optional(gs::integers::<u32>().min_value(60).max_value(600)));
    e
}

/// Statuses with ambiguous candidates as a set, which is all library order may change.
fn unordered(mut s: Status) -> Status {
    if let Status::Ambiguous { candidates } = &mut s {
        candidates.sort_unstable();
    }
    s
}

/// What an entry is found as doesn't depend on the order of the library's tracks.
#[hegel::test(test_cases = 150)]
fn matches_ignore_library_order(tc: TestCase) {
    let mut lib = library(&tc, 0, 10);
    // Copies of a track (another folder, another length) make ties to break.
    let copies: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(9)).max_size(3));
    let originals = lib.tracks.len();
    for k in copies.into_iter().filter(|&k| k < originals) {
        let mut t = lib.tracks[k].clone();
        t.id = lib.tracks.len() as u32 + 1;
        t.file_path = format!("/Contents/Copies/{}/{}.mp3", t.id, t.title);
        t.isrc.clear();
        t.duration_secs = tc.draw(gs::integers::<u32>().min_value(60).max_value(600));
        lib.tracks.push(t);
    }
    let entries: Vec<Entry> =
        (0..tc.draw(gs::integers::<usize>().max_value(6))).map(|_| entry_for(&tc, &lib)).collect();
    let keys: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>()).min_size(lib.tracks.len()).max_size(lib.tracks.len()));
    let mut shuffled = lib.clone();
    let mut order: Vec<usize> = (0..lib.tracks.len()).collect();
    order.sort_by_key(|&i| keys[i]);
    shuffled.tracks = order.iter().map(|&i| lib.tracks[i].clone()).collect();
    let a: Vec<Status> = match_entries(&lib, &entries).into_iter().map(|m| unordered(m.status)).collect();
    let b: Vec<Status> = match_entries(&shuffled, &entries).into_iter().map(|m| unordered(m.status)).collect();
    assert_eq!(a, b);
}

/// An empty library finds nothing, and its report says every entry is missing.
#[hegel::test(test_cases = 100)]
fn empty_libraries_find_nothing(tc: TestCase) {
    let lib = Library::default();
    let field = || gs::text().max_size(12);
    let entries: Vec<Entry> = tc
        .draw(
            gs::vecs(gs::tuples5(
                field(),
                field(),
                gs::optional(field()),
                gs::optional(field()),
                gs::optional(gs::integers::<u32>()),
            ))
            .max_size(5),
        )
        .into_iter()
        .map(|(artist, title, path, isrc, duration_secs)| Entry {
            artist,
            title,
            path,
            isrc,
            duration_secs,
            ..Default::default()
        })
        .collect();
    let report = Report::new(loclib_tracklist::Format::Text, match_entries(&lib, &entries));
    assert!(report.entries.iter().all(|m| m.status == Status::Missing));
    assert_eq!(report.summary, Summary { entries: entries.len(), found: 0, ambiguous: 0, missing: entries.len() });
}

/// A report counts exactly what it lists, the found entries name tracks of the library, and it
/// survives a JSON round trip unchanged.
#[hegel::test(test_cases = 100)]
fn reports_count_what_they_list(tc: TestCase) {
    let lib = library(&tc, 0, 10);
    let entries: Vec<Entry> =
        (0..tc.draw(gs::integers::<usize>().max_value(8))).map(|_| entry_for(&tc, &lib)).collect();
    let matched: Vec<Matched> = match_entries(&lib, &entries);
    let report = Report::new(loclib_tracklist::Format::Csv, matched.clone());
    let count = |f: fn(&Status) -> bool| matched.iter().filter(|m| f(&m.status)).count();
    let s = report.summary;
    assert_eq!(s.entries, entries.len());
    assert_eq!(s.found, count(|s| matches!(s, Status::Found { .. })));
    assert_eq!(s.ambiguous, count(|s| matches!(s, Status::Ambiguous { .. })));
    assert_eq!(s.found + s.ambiguous + s.missing, s.entries);
    let ids: Vec<u32> = report.entries.iter().filter_map(|m| m.status.track_id()).collect();
    // Every id it names is a track of the library.
    assert!(ids.iter().all(|id| lib.tracks.iter().any(|t| t.id == *id)));
    let json = serde_json::to_string(&report).unwrap();
    assert_eq!(serde_json::from_str::<Report>(&json).unwrap(), report);
}
