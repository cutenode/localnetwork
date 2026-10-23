//! Libraries to export: tracks with the tags people (and broken imports) give them, playlist trees
//! with repeats and missing tracks, and analysis with every kind of cue.

use hegel::generators::{self as gs, PrintableGenerator};
use hegel::{Generator, TestCase};
use loclib_model::analysis::{Beat, Cue, TrackAnalysis};
use loclib_model::test_util::{self, tracks_like};
use loclib_model::{Library, Playlist, Track, TrackColor};
use rustc_hash::FxHashMap;

/// Key texts in each notation rekordbox shows, with Traktor's `MUSICAL_KEY` (0-11 C to B major,
/// 12-23 minor) and Engine's key number (round the Camelot wheel from C major 0 and A minor 1),
/// worked out by hand from the formats' descriptions.
pub const KEYS: &[(&str, Option<u8>, Option<i64>)] = &[
    ("", None, None),
    ("C", Some(0), Some(0)),
    ("Am", Some(21), Some(1)),
    ("8A", Some(21), Some(1)),
    ("1m", Some(21), Some(1)),
    ("G", Some(7), Some(2)),
    ("Em", Some(16), Some(3)),
    ("F#m", Some(18), Some(7)),
    ("Fm", Some(17), Some(17)),
    ("Bb", Some(10), Some(20)),
    ("7B", Some(5), Some(22)),
    ("Dm", Some(14), Some(23)),
    ("H", None, None),
    ("13A", None, None),
];

pub const COLOURS: [TrackColor; 8] = [
    TrackColor::Pink,
    TrackColor::Red,
    TrackColor::Orange,
    TrackColor::Yellow,
    TrackColor::Green,
    TrackColor::Aqua,
    TrackColor::Blue,
    TrackColor::Purple,
];

/// Text a tag might hold: XML's specials, characters XML can't hold, NULs, Serato's `%%`.
pub fn text() -> impl PrintableGenerator<String> {
    hegel::one_of!(
        gs::text().max_size(10),
        gs::text().alphabet("aB <>&\"'\t\n\r\u{1}\u{0}\u{fffe}\u{ffff}é%.").max_size(10),
        gs::sampled_from(vec![
            String::new(),
            "Daft Punk".to_string(),
            "Set".to_string(),
            "set".to_string(),
            "50%% off".to_string(),
            "Tom & Jerry's \"Mix\"".to_string(),
            "日本語".to_string(),
        ]),
    )
}

/// A number that's often 0, which the formats take for "not set".
fn unset_or<T: Default + Clone + Send + Sync + std::fmt::Debug + 'static>(
    numbers: impl PrintableGenerator<T>,
) -> impl PrintableGenerator<T> {
    hegel::one_of!(gs::just(T::default()), numbers)
}

/// A date as rekordbox stores it: `2026-09-30` (any numbers, maybe with a time after it), or
/// something else.
#[hegel::composite]
fn date(tc: &TestCase) -> String {
    if tc.draw(gs::booleans()) {
        return tc.draw(gs::sampled_from(vec!["", "soon", "2024", "2024/03/01", "30-09-2026"])).to_string();
    }
    let y = tc.draw(gs::integers::<i32>().min_value(0).max_value(2999));
    let m = tc.draw(gs::integers::<u32>().max_value(13));
    let d = tc.draw(gs::integers::<u32>().max_value(32));
    let time = tc.draw(gs::sampled_from(vec!["", " 10:00:00", "T23:59:59Z"]));
    format!("{y:04}-{m:02}-{d:02}{time}")
}

/// The year, month and day of a date written `2026-09-30`.
pub fn ymd(s: &str) -> Option<(i32, u32, u32)> {
    let b = s.as_bytes();
    let shaped = b.len() >= 10
        && b[4] == b'-'
        && b[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9].iter().all(|&i| b[i].is_ascii_digit());
    shaped.then(|| (s[..4].parse().unwrap(), s[5..7].parse().unwrap(), s[8..10].parse().unwrap()))
}

/// A track with id `id` whose audio is somewhere below `Contents/`: a few folders and names, so
/// some tracks share a file, sometimes in another case.
fn track(id: u32) -> impl PrintableGenerator<Track> {
    let dir = gs::sampled_from(vec!["Artist", "artist", "A/B", "Ünïcödé", "x & y", "Prince:Live"]);
    let name = hegel::one_of!(
        gs::sampled_from(vec!["One".to_string(), "one".to_string(), "Two".to_string()]),
        gs::text().alphabet("ab .&'é%").min_size(1).max_size(6),
    );
    let ext = gs::sampled_from(vec!["mp3", "MP3", "wav", "aiff", "aif", "flac", "m4a"]);
    tracks_like(Track { id, ..Default::default() })
        .title(text())
        .artist(text())
        .album(text())
        .genre(text())
        .label(text())
        .key(gs::sampled_from(KEYS.iter().map(|k| k.0.to_string()).collect::<Vec<_>>()))
        .remixer(text())
        .composer(text())
        .comment(text())
        .mix_name(text())
        .bpm(hegel::one_of!(gs::just(0.0), gs::floats::<f64>().min_value(40.0).max_value(220.0)))
        .duration_secs(unset_or(gs::integers::<u32>().max_value(20_000)))
        .year(unset_or(gs::integers::<u16>().max_value(2100)))
        .track_number(unset_or(gs::integers::<u32>().max_value(30)))
        .rating(gs::integers::<u8>().max_value(9))
        .color(gs::optional(gs::sampled_from(COLOURS.to_vec())))
        .play_count(gs::integers::<u16>())
        .bitrate(unset_or(gs::integers::<u32>().max_value(3000)))
        .file_size(unset_or(gs::integers::<u64>().max_value(1 << 34)))
        .file_path(hegel::tuples!(dir, name, ext).map(|(dir, name, ext)| format!("/Contents/{dir}/{name}.{ext}")))
        .date_added(date())
        .release_date(date())
}

/// A playlist tree over `tracks`: folders hold what comes after them, names repeat, playlists
/// list tracks twice and tracks the library doesn't have (id 999).
#[hegel::test_helper]
pub fn playlists(tc: &TestCase, tracks: &[Track]) -> Vec<Playlist> {
    let mut ids: Vec<u32> = tracks.iter().map(|t| t.id).collect();
    ids.push(999);
    let n = tc.draw(gs::integers::<usize>().max_value(7));
    // Sometimes every node has one name, so siblings have to be told apart.
    let one_name = tc.draw(gs::optional(gs::sampled_from(vec!["Set", "set"])));
    let mut out: Vec<Playlist> = Vec::new();
    for id in 1..=n as u32 {
        let folders: Vec<u32> = out.iter().filter(|p| p.is_folder).map(|p| p.id).collect();
        let parent = if folders.is_empty() { None } else { tc.draw(gs::optional(gs::sampled_from(folders))) };
        let is_folder = tc.draw(gs::booleans());
        let track_ids =
            if is_folder { Vec::new() } else { tc.draw(gs::vecs(gs::sampled_from(ids.clone())).max_size(6)) };
        out.push(Playlist {
            id,
            parent_id: parent,
            // Long enough, with the folders above it, to be cut short in a file name.
            name: match one_name {
                Some(name) => name.to_string(),
                None => tc.draw(hegel::one_of!(text(), gs::text().alphabet("aé%日").min_size(60).max_size(120))),
            },
            is_folder,
            sort_order: tc.draw(gs::integers::<u32>().max_value(3)),
            track_ids,
        });
    }
    out
}

/// A library of up to `max_tracks` tracks (ids 1 up) and a playlist tree over them.
#[hegel::test_helper]
pub fn library(tc: &TestCase, max_tracks: u32) -> Library {
    let Library { mut tracks, .. } = tc.draw(test_util::libraries(0, max_tracks, track));
    // Rows sharing a file, as is or in another case.
    for i in 1..tracks.len() {
        if let Some((j, upper)) =
            tc.draw(gs::optional(hegel::tuples!(gs::integers::<usize>().max_value(i - 1), gs::booleans())))
        {
            let path = &tracks[j].file_path;
            tracks[i].file_path = if upper { path.to_uppercase() } else { path.clone() };
        }
    }
    let playlists = playlists(tc, &tracks);
    Library { tracks, playlists, ..Default::default() }
}

/// A cue of any kind: memory or hot (past the eight pads too), a point or a loop (ending before it
/// starts, even), named or not, coloured or not.
pub fn cue() -> impl PrintableGenerator<Cue> {
    hegel::tuples!(
        gs::optional(gs::integers::<u8>().max_value(10)),
        gs::integers::<u32>().max_value(600_000),
        gs::optional(gs::integers::<u32>().max_value(600_000)),
        text(),
        gs::optional(hegel::one_of!(
            gs::arrays::<_, u8, 3>(gs::integers::<u8>()),
            // A grey, which has no hue.
            gs::integers::<u8>().max_value(240).map(|v| [v, v + 15, v]),
        )),
    )
    .map(|(hot_cue, start_ms, end_ms, comment, color)| Cue {
        hot_cue,
        start_ms,
        end_ms,
        comment,
        color,
        color_id: 0,
        loop_beats: None,
    })
}

/// A beat grid: none, a steady tempo, or one that changes.
#[hegel::test_helper]
pub fn beats(tc: &TestCase) -> Vec<Beat> {
    let n = tc.draw(gs::integers::<usize>().max_value(5));
    let first = tc.draw(gs::integers::<u16>().min_value(1).max_value(4));
    let bpm = tc.draw(hegel::one_of!(gs::just(0.0), gs::floats::<f64>().min_value(60.0).max_value(180.0)));
    let varies = tc.draw(gs::booleans());
    (0..n)
        .map(|i| Beat {
            beat_number: (first - 1 + i as u16) % 4 + 1,
            bpm: if varies && i > 0 { bpm + i as f64 } else { bpm },
            time_ms: 40 + i as u32 * 500,
        })
        .collect()
}

/// Analysis for every track of `lib`, by id.
#[hegel::test_helper]
pub fn analysis(tc: &TestCase, lib: &Library) -> FxHashMap<u32, (Vec<Beat>, Vec<Cue>)> {
    lib.tracks.iter().map(|t| (t.id, (beats(tc), tc.draw(gs::vecs(cue()).max_size(12))))).collect()
}

/// [`analysis`] as an export reads it.
pub fn loader(by_id: &FxHashMap<u32, (Vec<Beat>, Vec<Cue>)>) -> impl Fn(&Track) -> TrackAnalysis + '_ {
    |t| {
        let (beats, cues) = by_id[&t.id].clone();
        TrackAnalysis { beats, cues, ..Default::default() }
    }
}

/// `s` without what XML 1.0 can't hold (control characters other than tab, LF and CR, U+FFFE and
/// U+FFFF).
pub fn xml_text(s: &str) -> String {
    s.chars().filter(|&c| matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')).collect()
}

/// The first track of each file, as an export that keeps one track per file sees them: `key` says
/// which file a track is.
pub fn first_per_file(lib: &Library, key: impl Fn(&Track) -> String) -> Vec<&Track> {
    let mut seen = rustc_hash::FxHashSet::default();
    lib.tracks.iter().filter(|t| seen.insert(key(t))).collect()
}

/// An XML element as quick-xml (an independent reader) sees it, attributes normalized as XML 1.0
/// says: a literal tab or line break in a value reads as a space.
#[derive(Debug)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<El>,
}

impl El {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The attribute `name`, which must be there.
    pub fn get(&self, name: &str) -> &str {
        self.attr(name).unwrap_or_else(|| panic!("{} has no {name}: {:?}", self.name, self.attrs))
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> {
        self.kids.iter().filter(move |k| k.name == name)
    }

    /// The only child named `name`.
    pub fn one(&self, name: &str) -> &El {
        let found: Vec<&El> = self.kids.iter().filter(|k| k.name == name).collect();
        assert_eq!(found.len(), 1, "{} holds one {name}", self.name);
        found[0]
    }
}

/// The root element of `xml`, which must be well-formed: matching tags, quoted attributes without
/// `<`, no repeated attributes, known entities, nothing but whitespace between elements.
pub fn parse_xml(xml: &str) -> El {
    use quick_xml::events::{BytesStart, Event};
    fn element(e: &BytesStart<'_>) -> El {
        let attrs = e
            .attributes()
            .map(|a| {
                let a = a.expect("a well-formed attribute");
                assert!(!a.value.contains('<'), "`<` in an attribute value: {:?}", a.value);
                let value = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).expect("known entities");
                (a.key.as_ref().to_string(), value.into_owned())
            })
            .collect();
        El { name: e.name().as_ref().to_string(), attrs, kids: Vec::new() }
    }
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack = vec![El { name: String::new(), attrs: Vec::new(), kids: Vec::new() }];
    loop {
        match reader.read_event().unwrap_or_else(|e| panic!("not well-formed: {e}")) {
            Event::Start(e) => stack.push(element(&e)),
            Event::Empty(e) => stack.last_mut().unwrap().kids.push(element(&e)),
            Event::End(_) => {
                let done = stack.pop().unwrap();
                stack.last_mut().expect("an end tag closes an element").kids.push(done);
            }
            Event::Text(t) => assert!(t.as_ref().trim().is_empty(), "text between elements: {t:?}"),
            Event::Decl(_) => {}
            Event::Eof => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    let mut doc = stack.pop().unwrap();
    assert!(stack.is_empty(), "every element is closed");
    assert_eq!(doc.kids.len(), 1, "one root element");
    doc.kids.pop().unwrap()
}
