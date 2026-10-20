use super::*;

#[test]
fn genres_are_split_and_spelled_one_way() {
    let none = IndexMap::new();
    assert_eq!(genres("deep house / Tech-House", &none), ["Deep House", "Tech House"]);
    assert_eq!(genres("DnB", &none), ["Drum & Bass"]);
    assert_eq!(genres("Drum & Bass", &none), ["Drum & Bass"]);
    assert_eq!(genres("UK garage; ukg", &none), ["UK Garage"]);
    assert_eq!(genres("Unknown", &none), Vec::<String>::new());
    let mine = IndexMap::from([("minimal".to_string(), "Minimal / Deep Tech".to_string())]);
    assert_eq!(genres("MINIMAL", &mine), ["Minimal / Deep Tech"]);
}

#[test]
fn artists_are_credited_once_each() {
    assert_eq!(artist_credits("Bicep FEAT. Clara La San"), ["Bicep", "Clara La San"]);
    assert_eq!(artist_credits("Chris Lake, Amber Mark"), ["Chris Lake", "Amber Mark"]);
    assert_eq!(artist_credits("Simon & Garfunkel"), ["Simon & Garfunkel"]);
    assert_eq!(artist_credits("Röyksopp (feat. Robyn)"), ["Röyksopp", "Robyn"]);
    assert_eq!(artist_credits("İzel, Ölgü"), ["İzel", "Ölgü"]);
    assert_eq!(artist_credits(""), Vec::<String>::new());
    assert_eq!(credited_artists("Bicep feat. Clara La San"), ["bicep", "clara la san"]);
    assert_eq!(credited_artists("Fred again.. x Four Tet"), ["fred again..", "four tet"]);
}

#[test]
fn percentiles_interpolate() {
    let v = [120.0, 122.0, 124.0, 126.0, 128.0];
    assert_eq!(percentile(&v, 0.5), Some(124.0));
    assert_eq!(percentile(&v, 0.25), Some(122.0));
    assert_eq!(percentile(&v, 0.1), Some(120.8));
    assert_eq!(percentile(&[], 0.5), None);
}

#[test]
fn tempo_histogram_covers_the_core() {
    let mut v: Vec<f64> = (0..100).map(|i| 120.0 + (i % 10) as f64).collect();
    v.sort_by(f64::total_cmp);
    let t = tempo_section(&v);
    assert_eq!(t.bin_width, 2.0);
    assert_eq!(t.histogram.first().unwrap().bpm, 120.0);
    assert_eq!(t.histogram.iter().map(|b| b.count).sum::<usize>(), 100);
    let BpmRange { low: lo, high: hi } = t.core.unwrap();
    assert!((lo - 120.9).abs() < 1e-9 && (hi - 128.1).abs() < 1e-9, "{lo} {hi}");
}

#[test]
fn deciles_spread_a_set() {
    assert_eq!(decile(0, 20), 0);
    assert_eq!(decile(19, 20), 9);
    assert_eq!(decile(2, 3), 6);
}

// -----------------------------------------------------------------------------------------
// Properties: a report worked out from random libraries and sets, against a model of them
// -----------------------------------------------------------------------------------------

use crate::model::TrackColor;
use crate::sets::EntryOrigin;
use hegel::TestCase;
use hegel::generators as gs;

const ARTISTS: [&str; 6] = ["Bicep", "Four Tet", "bicep", "Bicep feat. Clara La San", "Floorplan, Robert Hood", ""];
// Mostly keys a wheel step apart, so sets mix in key as often as not.
const KEYS: [&str; 10] = ["8A", "8A", "9A", "8B", "7A", "3A", "10B", "1m", "", "junk"];
const GENRES: [&str; 6] = ["Techno", "techno / Deep House", "DnB", "", "Unknown", "UKG; House"];
const LABELS: [&str; 4] = ["Hessle Audio", "hessle  audio", "", "Ninja Tune"];
// Mostly tempos within 3% of each other, and some only at half or double time.
const BPMS: [f64; 11] = [0.0, 30.0, 124.0, 124.0, 125.0, 126.0, 122.5, 128.0, 174.0, 64.0, 62.0];
const PLAYLISTS: [&str; 4] = ["Warm-up", "warm-up", "Peak", "Old"];

/// A track as the test makes it: what the profile reads of it.
#[derive(Debug, Clone)]
struct GenTrack {
    artist: &'static str,
    bpm: f64,
    key: &'static str,
    genre: &'static str,
    label: &'static str,
    year: u16,
    rating: u8,
    coloured: bool,
    in_collection: bool,
    mood: Option<Mood>,
    cues: Option<(usize, usize, usize)>,
}

/// A set (or session) of the test's tracks, by index.
#[derive(Debug, Clone)]
struct GenSet {
    tracks: Vec<usize>,
    /// Days before today it was played (a declared set) or first archived (a session: `None`
    /// is one still only on a drive).
    age: Option<i64>,
    /// Positions entries gone from the library came before (sessions only).
    missing_at: Vec<usize>,
    unnamed: Vec<bool>,
}

#[derive(Debug, Clone)]
struct Scenario {
    tracks: Vec<GenTrack>,
    sets: Vec<GenSet>,
    /// Declared sets, else sessions standing in for them.
    declared: bool,
    /// Collection tracks with a second file (`Track-1.mp3`).
    dupes: Vec<usize>,
    playlists: Vec<(bool, &'static str)>,
    top: usize,
    min_set_tracks: usize,
    /// Days before today.
    since: Option<i64>,
    until: Option<i64>,
    /// Player settings (of [`STYLE_SETTINGS`] and [`STYLE_VALUES`]), each key once.
    style: Vec<(usize, usize)>,
}

const STYLE_VALUES: [&str; 5] = ["on", "off", "16", "wide", "10"];

fn scenario() -> impl hegel::PrintableGenerator<Scenario> {
    hegel::compose!(|tc| {
        let years = vec![0u16, 1987, 1999, 2012, 2024, Utc::now().year() as u16, 2099];
        let track = hegel::compose!(|tc| {
            GenTrack {
                artist: tc.draw(gs::sampled_from(ARTISTS.to_vec())),
                bpm: tc.draw(gs::sampled_from(BPMS.to_vec())),
                key: tc.draw(gs::sampled_from(KEYS.to_vec())),
                genre: tc.draw(gs::sampled_from(GENRES.to_vec())),
                label: tc.draw(gs::sampled_from(LABELS.to_vec())),
                year: tc.draw(gs::sampled_from(years.clone())),
                rating: tc.draw(gs::integers::<u8>().max_value(5)),
                coloured: tc.draw(gs::booleans()),
                in_collection: tc.draw(gs::weighted_booleans(0.8)),
                mood: tc.draw(gs::optional(gs::sampled_from(vec![Mood::High, Mood::Mid, Mood::Low]))),
                cues: tc.draw(gs::optional(gs::tuples!(
                    gs::integers::<usize>().max_value(3),
                    gs::integers::<usize>().max_value(2),
                    gs::integers::<usize>().max_value(1)
                ))),
            }
        });
        let tracks: Vec<GenTrack> = tc.draw(gs::vecs(track).min_size(1).max_size(24));
        let n = tracks.len();
        let set = hegel::compose!(|tc| {
            let tracks: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().max_value(n - 1)).max_size(14));
            let len = tracks.len();
            let flags = |p: f64| gs::vecs(gs::weighted_booleans(p)).min_size(len).max_size(len);
            let missing: Vec<bool> = tc.draw(flags(0.15));
            GenSet {
                age: tc.draw(gs::optional(gs::integers::<i64>().min_value(0).max_value(400))),
                missing_at: (1..len).filter(|&p| missing[p]).collect(),
                unnamed: tc.draw(flags(0.1)),
                tracks,
            }
        });
        let days = || gs::optional(gs::integers::<i64>().min_value(0).max_value(400));
        Scenario {
            sets: tc.draw(gs::vecs(set).max_size(8)),
            declared: tc.draw(gs::booleans()),
            dupes: tc.draw(gs::vecs(gs::integers::<usize>().max_value(n - 1)).max_size(3)),
            playlists: tc.draw(gs::vecs(gs::tuples!(gs::booleans(), gs::sampled_from(PLAYLISTS.to_vec()))).max_size(5)),
            top: tc.draw(gs::integers::<usize>().min_value(1).max_value(6)),
            min_set_tracks: tc.draw(gs::integers::<usize>().min_value(2).max_value(6)),
            since: tc.draw(days()),
            until: tc.draw(days()),
            style: tc.draw(
                gs::vecs(gs::tuples!(
                    gs::integers::<usize>().max_value(STYLE_SETTINGS.len() - 1),
                    gs::integers::<usize>().max_value(STYLE_VALUES.len() - 1)
                ))
                .max_size(3),
            ),
            tracks,
        }
    })
}

fn gen_track(i: usize, g: &GenTrack) -> Track {
    Track {
        id: i as u32 + 1,
        title: format!("T{i}"),
        artist: g.artist.into(),
        bpm: g.bpm,
        key: g.key.into(),
        genre: g.genre.into(),
        label: g.label.into(),
        year: g.year,
        rating: g.rating,
        color: g.coloured.then_some(TrackColor::Red),
        duration_secs: 200,
        file_path: format!("/Contents/T{i}.mp3"),
        ..Track::default()
    }
}

fn builder(options: &ProfileOptions) -> Builder<'_> {
    Builder {
        options,
        files: Files { drives: &[], store: None, snapshots: Vec::new(), by_id: FxHashMap::default() },
        recs: Vec::new(),
        by_key: FxHashMap::default(),
        by_file: FxHashMap::default(),
        lists: Vec::new(),
        sets: Vec::new(),
        playlists: FxHashSet::default(),
        duplicate_files: 0,
        sources: Vec::new(),
        warnings: Vec::new(),
    }
}

/// A set the report should count: its tracks (by index), its day, and where entries gone
/// from the library were.
struct Played {
    tracks: Vec<usize>,
    date: Option<NaiveDate>,
    missing_at: Vec<usize>,
}

/// The report `s` makes, through the builder as [`build`] drives it (without reading any
/// drive or store), and the sets it should count.
fn report_of(s: &Scenario, now: DateTime<Utc>) -> (ProfileReport, Vec<Played>) {
    let today = now.with_timezone(&Local).date_naive();
    let ago = |d: i64| today - chrono::Duration::days(d);
    let o = ProfileOptions {
        since: s.since.map(ago),
        until: s.until.map(ago),
        min_set_tracks: s.min_set_tracks,
        top: s.top,
        analysis: false,
        genre_aliases: IndexMap::new(),
    };
    let mut b = builder(&o);
    let tracks: Vec<Track> = s.tracks.iter().enumerate().map(|(i, g)| gen_track(i, g)).collect();
    let mut rows: Vec<Track> =
        (0..tracks.len()).filter(|&i| s.tracks[i].in_collection).map(|i| tracks[i].clone()).collect();
    for &d in s.dupes.iter().filter(|&&i| s.tracks[i].in_collection) {
        rows.push(Track { file_path: format!("/Contents/T{d}-1.mp3"), ..tracks[d].clone() });
    }
    let playlists = s
        .playlists
        .iter()
        .enumerate()
        .map(|(k, (is_folder, name))| crate::model::Playlist {
            id: k as u32 + 1,
            parent_id: None,
            name: name.to_string(),
            is_folder: *is_folder,
            sort_order: k as u32,
            track_ids: Vec::new(),
        })
        .collect();
    b.add_collection(&Library { tracks: rows, playlists, ..Library::default() }, Origin::Drive(0));

    let mut played = Vec::new();
    let too_short = if s.declared {
        for (k, set) in s.sets.iter().enumerate() {
            let date = ago(set.age.unwrap_or(0));
            let entries = set
                .tracks
                .iter()
                .zip(&set.unnamed)
                .map(|(&i, &unnamed)| {
                    let e = SetEntry { unnamed, ..SetEntry::from_track(&tracks[i], EntryOrigin::Session, None) };
                    let r = b.entry(&e);
                    b.recs[r].unnamed |= e.unnamed;
                    r
                })
                .collect();
            b.sets.push(SetData {
                id: format!("set{k}"),
                name: format!("Set {k}"),
                place: String::new(),
                date: Some(date),
                tracks: entries,
                missing_at: Vec::new(),
            });
            played.push(Played { tracks: set.tracks.clone(), date: Some(date), missing_at: Vec::new() });
        }
        0
    } else {
        let mut short = 0;
        for (k, set) in s.sets.iter().enumerate() {
            let list = HistoryList {
                name: format!("HISTORY {k:03}"),
                path: format!("HISTORY {k:03}"),
                database: "device_library",
                tracks: set.tracks.iter().map(|&i| tracks[i].clone()).collect(),
                missing: set.missing_at.len(),
                missing_at: set.missing_at.clone(),
            };
            let first_seen = set.age.map(|d| now - chrono::Duration::days(d));
            b.add_list(list, "Main".into(), first_seen, Origin::Drive(0));
            let date = first_seen.map(|t| t.with_timezone(&Local).date_naive());
            let in_range = match date {
                Some(d) => o.since.is_none_or(|x| d >= x) && o.until.is_none_or(|x| d <= x),
                None => o.until.is_none(),
            };
            if set.tracks.len() < s.min_set_tracks {
                short += 1;
            } else if in_range {
                played.push(Played { tracks: set.tracks.clone(), date, missing_at: set.missing_at.clone() });
            }
        }
        assert_eq!(b.sessions_as_sets(), short);
        short
    };
    for (i, g) in s.tracks.iter().enumerate() {
        if let Some(&r) = b.by_key.get(&rec_key(&tracks[i])) {
            b.recs[r].mood = g.mood;
            b.recs[r].cues = g.cues.map(|(hot, memory, loops)| CueCounts { hot, memory, loops });
        }
    }
    let counted = if s.declared { Basis::Plays } else { Basis::Sessions };
    let mut report = compute(&b, counted, too_short, now);
    let mut values: Vec<StyleValue> = Vec::new();
    for &(k, v) in &s.style {
        if !values.iter().any(|x| x.key == STYLE_SETTINGS[k]) {
            let (key, value) = (STYLE_SETTINGS[k].to_string(), STYLE_VALUES[v].to_string());
            values.push(StyleValue { label: key.clone(), value_label: value.clone(), key, value });
        }
    }
    report.style = (!values.is_empty()).then(|| StyleSection { source: "settings profile".into(), values });
    report.summary = summarise(&report);
    (report, played)
}

/// A track's sets, plays, sets opened and closed, and whether a set marks it unnamed.
type Counts = (usize, usize, usize, usize, bool);

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn valid_bpm(b: f64) -> bool {
    (40.0..=300.0).contains(&b)
}

fn name_of(i: usize, g: &GenTrack) -> String {
    gen_track(i, g).display_name()
}

/// A ranked section agrees with `counts` (normalised name to count) out of `known`: every
/// name it lists has its count and share, most counted first, and none left out has more.
fn check_ranked(what: &str, sec: &RankedSection, counts: &FxHashMap<String, usize>, known: usize, top: usize) {
    assert_eq!((sec.known, sec.distinct), (known, counts.len()), "{what}");
    assert_eq!(sec.top.len(), counts.len().min(top), "{what}: {:?}", sec.top);
    assert!(sec.top.windows(2).all(|w| w[0].count >= w[1].count), "{what}: {:?}", sec.top);
    for s in &sec.top {
        assert_eq!(counts.get(&normalize(&s.name)), Some(&s.count), "{what}: {s:?}");
        assert!(close(s.share, s.count as f64 / known as f64) && (0.0..=1.0).contains(&s.share), "{what}: {s:?}");
    }
    let listed: FxHashSet<String> = sec.top.iter().map(|s| normalize(&s.name)).collect();
    if let Some(last) = sec.top.last() {
        assert!(counts.iter().all(|(k, c)| listed.contains(k) || *c <= last.count), "{what}: {counts:?}");
    }
}

/// Whatever the library and sets (declared, or sessions standing in for them), the report's
/// numbers are the ones a direct count of the inputs gives: the sets in range, plays and
/// distinct tracks, tempo and key tallies, transitions (not across a missing entry or a
/// repeat), ranked genres, labels and artists, release years, signature tracks, cues and the
/// collection; every share is in [0, 1] and the headline counts what the basis counts.
#[hegel::test(test_cases = 200)]
fn the_report_agrees_with_a_count_of_its_inputs(tc: TestCase) {
    let s = tc.draw(scenario());
    let now = Utc::now();
    let (r, played) = report_of(&s, now);
    let g = &s.tracks;
    let none = IndexMap::new();

    // The pool: every play, else the collection.
    let mut pool: Vec<(usize, i32)> = Vec::new();
    for p in &played {
        let year = p.date.map_or(now.year(), |d| d.year());
        pool.extend(p.tracks.iter().map(|&i| (i, year)));
    }
    let basis = match (pool.is_empty(), s.declared) {
        (true, _) => Basis::Collection,
        (false, true) => Basis::Plays,
        (false, false) => Basis::Sessions,
    };
    assert_eq!(r.basis, basis);
    if basis == Basis::Collection {
        pool = (0..g.len()).filter(|&i| g[i].in_collection).map(|i| (i, now.year())).collect();
    }

    // Sets.
    let distinct: FxHashSet<usize> = played.iter().flat_map(|p| p.tracks.iter().copied()).collect();
    let sets = &r.sets;
    assert_eq!(sets.count, played.len());
    assert_eq!(sets.plays, if basis == Basis::Collection { 0 } else { pool.len() });
    assert_eq!(sets.distinct_tracks, distinct.len());
    assert_eq!(sets.longest, played.iter().map(|p| p.tracks.len()).max().unwrap_or(0));
    assert_eq!(sets.oldest, played.iter().filter_map(|p| p.date).min());
    assert_eq!(sets.newest, played.iter().filter_map(|p| p.date).max());
    assert_eq!(sets.not_archived, played.iter().filter(|p| p.date.is_none()).count());
    assert!(sets.list.iter().all(|i| i.harmonic_share.is_none_or(|h| (0.0..=1.0).contains(&h))));
    let mut lengths: Vec<(usize, Option<NaiveDate>)> = sets.list.iter().map(|i| (i.tracks, i.date)).collect();
    let mut want: Vec<(usize, Option<NaiveDate>)> = played.iter().map(|p| (p.tracks.len(), p.date)).collect();
    lengths.sort();
    want.sort();
    assert_eq!(lengths, want);
    if !s.declared {
        // Sessions newest first: those still on a drive, then by the day first archived.
        let dates: Vec<Option<NaiveDate>> = sets.list.iter().map(|i| i.date).collect();
        let in_order = |w: &[Option<NaiveDate>]| match (w[0], w[1]) {
            (Some(_), None) => false,
            (Some(a), Some(b)) => a >= b,
            (None, _) => true,
        };
        assert!(dates.windows(2).all(in_order), "{dates:?}");
    }

    // Tempo.
    let mut bpms: Vec<f64> = pool.iter().map(|&(i, _)| g[i].bpm).filter(|b| valid_bpm(*b)).collect();
    bpms.sort_by(f64::total_cmp);
    let t = &r.tempo;
    assert_eq!((t.tracks, t.min, t.max), (bpms.len(), bpms.first().copied(), bpms.last().copied()));
    assert_eq!(t.histogram.iter().map(|b| b.count).sum::<usize>(), bpms.len());
    assert!(t.histogram.windows(2).all(|w| close(w[1].bpm - w[0].bpm, t.bin_width)));
    if let (Some(core), Some(median), Some(lo), Some(hi)) = (t.core, t.median, t.min, t.max) {
        let (p25, p75) = (t.p25.unwrap(), t.p75.unwrap());
        assert!(lo <= core.low && core.low <= p25 && p25 <= median && median <= p75, "{t:?}");
        assert!(p75 <= core.high && core.high <= hi, "{t:?}");
    } else {
        assert!(bpms.is_empty() && t.histogram.is_empty(), "{t:?}");
    }

    // Transitions.
    let (mut transitions, mut keyed, mut harmonic, mut timed, mut genred, mut changes) = (0, 0, 0, 0, 0, 0);
    let mut moves: FxHashMap<(usize, usize), usize> = FxHashMap::default();
    for p in &played {
        let mut here = FxHashSet::default();
        for k in 1..p.tracks.len() {
            let (x, y) = (p.tracks[k - 1], p.tracks[k]);
            if x == y || p.missing_at.contains(&k) {
                continue;
            }
            here.insert((x, y));
            transitions += 1;
            if let (Some(a), Some(c)) = (Camelot::parse(g[x].key), Camelot::parse(g[y].key)) {
                keyed += 1;
                harmonic += usize::from(a.relation_with_boost(c).is_some());
            }
            timed += usize::from(valid_bpm(g[x].bpm) && valid_bpm(g[y].bpm));
            if let (Some(a), Some(c)) = (genres(g[x].genre, &none).first(), genres(g[y].genre, &none).first()) {
                genred += 1;
                changes += usize::from(normalize(a) != normalize(c));
            }
        }
        for m in here {
            *moves.entry(m).or_default() += 1;
        }
    }
    let m = &r.mixing;
    assert_eq!((m.transitions, m.keyed, m.timed, m.genred), (transitions, keyed, timed, genred));
    assert_eq!(m.same_key + m.adjacent + m.relative + m.energy_boost + m.clash, m.keyed);
    assert_eq!(m.keyed - m.clash, harmonic);
    assert_eq!(m.harmonic_share, share(harmonic, keyed));
    assert_eq!(m.genre_change_share, share(changes, genred));
    assert!(m.within_3pct.zip(m.within_6pct).is_none_or(|(a, b)| a <= b && b <= 1.0), "{m:?}");
    assert!(m.half_double <= m.timed);

    // Keys.
    let mut wheel = [0usize; 24];
    for &(i, _) in &pool {
        if let Some(k) = Camelot::parse(g[i].key) {
            wheel[k.order() as usize] += 1;
        }
    }
    let k = &r.keys;
    assert_eq!(k.wheel.iter().map(|w| w.count).collect::<Vec<_>>(), wheel);
    assert_eq!(k.tracks, wheel.iter().sum::<usize>());
    let key_counts: FxHashMap<String, usize> =
        k.wheel.iter().filter(|w| w.count > 0).map(|w| (normalize(&w.key), w.count)).collect();
    check_ranked(
        "keys",
        &RankedSection { known: k.tracks, distinct: key_counts.len(), top: k.top.clone() },
        &key_counts,
        k.tracks,
        s.top,
    );
    assert!(k.minor_share.is_none_or(|x| (0.0..=1.0).contains(&x)));

    // Energy.
    let moods: Vec<Mood> = pool.iter().filter_map(|&(i, _)| g[i].mood).collect();
    match &r.energy {
        None => assert!(moods.is_empty()),
        Some(e) => {
            let part = |m: Mood| moods.iter().filter(|x| **x == m).count() as f64 / moods.len() as f64;
            assert_eq!(e.tracks, moods.len());
            assert!(close(e.high, part(Mood::High)) && close(e.mid, part(Mood::Mid)) && close(e.low, part(Mood::Low)));
            assert!(close(e.high + e.mid + e.low, 1.0));
        }
    }

    // Genres, labels and artists.
    let mut tallies: [(FxHashMap<String, usize>, usize); 3] = Default::default();
    for &(i, _) in &pool {
        let named: [Vec<String>; 3] = [
            genres(g[i].genre, &none),
            Some(g[i].label.trim().to_string()).filter(|l| !l.is_empty()).into_iter().collect(),
            artist_credits(g[i].artist),
        ];
        for (names, (counts, known)) in named.iter().zip(tallies.iter_mut()) {
            *known += usize::from(!names.is_empty());
            for n in names {
                *counts.entry(normalize(n)).or_default() += 1;
            }
        }
    }
    for ((what, sec), (counts, known)) in
        [("genres", &r.genres), ("labels", &r.labels), ("artists", &r.artists)].into_iter().zip(&tallies)
    {
        check_ranked(what, sec, counts, *known, s.top);
    }
    assert!(r.labels.top.iter().map(|l| l.share).sum::<f64>() <= 1.0 + 1e-9, "a track has one label");

    // Era.
    let years: Vec<(i32, i32)> = pool
        .iter()
        .map(|&(i, played)| (i32::from(g[i].year), played))
        .filter(|(y, _)| *y >= 1900 && *y <= now.year() + 1)
        .collect();
    let e = &r.era;
    assert_eq!(e.tracks, years.len());
    assert_eq!((e.oldest, e.newest), (years.iter().map(|y| y.0).min(), years.iter().map(|y| y.0).max()));
    assert_eq!(e.decades.iter().map(|d| d.count).sum::<usize>(), years.len());
    assert!(e.decades.windows(2).all(|w| w[0].decade < w[1].decade) && e.decades.iter().all(|d| d.decade % 10 == 0));
    assert_eq!(e.recent_share, share(years.iter().filter(|(y, p)| *y >= p - 1).count(), years.len()));
    assert!(e.median_year.is_none_or(|y| e.oldest.unwrap() <= y && y <= e.newest.unwrap()));

    // Arc.
    let shaped = played.iter().filter(|p| p.tracks.iter().filter(|&&i| valid_bpm(g[i].bpm)).count() >= 6).count();
    assert_eq!(r.arc.sets, shaped);
    assert_eq!(r.arc.curve.len(), 10);
    if shaped > 0 {
        assert!(close(r.arc.builds + r.arc.steady + r.arc.winds_down, 1.0), "{:?}", r.arc);
        let (lo, hi) = (t.min.unwrap(), t.max.unwrap());
        for b in [r.arc.opening_bpm.unwrap(), r.arc.closing_bpm.unwrap()] {
            assert!(lo <= b && b <= hi, "{:?}", r.arc);
        }
    } else {
        assert!(r.arc.opening_bpm.is_none() && r.arc.curve.iter().all(Option::is_none));
    }

    // Signature: every track played, counted per set.
    let mut per: FxHashMap<usize, Counts> = FxHashMap::default();
    for (k, p) in played.iter().enumerate() {
        // Sessions don't mark tracks unnamed; declared sets are all counted, in order.
        let set = s.declared.then(|| &s.sets[k]);
        for &i in p.tracks.iter().collect::<FxHashSet<_>>() {
            per.entry(i).or_default().0 += 1;
        }
        for (pos, &i) in p.tracks.iter().enumerate() {
            let e = per.entry(i).or_default();
            e.1 += 1;
            e.4 |= set.is_some_and(|set| set.unnamed[pos]);
        }
        if let (Some(&first), Some(&last)) = (p.tracks.first(), p.tracks.last()) {
            per.entry(first).or_default().2 += 1;
            per.entry(last).or_default().3 += 1;
        }
    }
    let sig = &r.signature;
    let mut got: Vec<(String, Counts)> =
        sig.played.iter().map(|p| (p.name.clone(), (p.sets, p.plays, p.opened, p.closed, p.unnamed))).collect();
    let mut want: Vec<(String, Counts)> = per.iter().map(|(&i, c)| (name_of(i, &g[i]), *c)).collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);
    assert!(sig.played.windows(2).all(|w| (w[0].sets, w[0].plays) >= (w[1].sets, w[1].plays)));
    let at_least_two = |f: fn(&Counts) -> usize| per.values().filter(|c| f(c) >= 2).count();
    assert_eq!(sig.tracks.len(), at_least_two(|c| c.0).min(s.top));
    assert_eq!(sig.openers.len(), at_least_two(|c| c.2).min(s.top));
    assert_eq!(sig.closers.len(), at_least_two(|c| c.3).min(s.top));
    assert!(sig.tracks.windows(2).all(|w| w[0].sets >= w[1].sets) && sig.tracks.iter().all(|t| t.sets >= 2));
    assert!(sig.openers.iter().all(|t| t.opened >= 2) && sig.closers.iter().all(|t| t.closed >= 2));
    assert_eq!(sig.transitions.len(), moves.values().filter(|n| **n >= 2).count().min(s.top));
    for st in &sig.transitions {
        let n = moves.iter().find(|((x, y), _)| name_of(*x, &g[*x]) == st.from && name_of(*y, &g[*y]) == st.to);
        assert_eq!(n.map(|(_, n)| *n), Some(st.sets), "{st:?}");
    }

    // Prep and the collection.
    let owned: Vec<usize> = (0..g.len()).filter(|&i| g[i].in_collection).collect();
    let cued: Vec<(usize, usize, usize)> = owned.iter().filter_map(|&i| g[i].cues).collect();
    match &r.prep {
        None => assert!(cued.is_empty()),
        Some(p) => {
            let part =
                |f: fn(&(usize, usize, usize)) -> bool| cued.iter().filter(|c| f(c)).count() as f64 / cued.len() as f64;
            assert_eq!(p.tracks, cued.len());
            assert!(close(p.hot_cue_share, part(|c| c.0 > 0)) && close(p.memory_cue_share, part(|c| c.1 > 0)));
            assert!(close(p.loop_share, part(|c| c.2 > 0)));
            assert!(p.hot_cues_per_track.is_none_or(|h| (1.0..=3.0).contains(&h)));
        }
    }
    let c = &r.collection;
    let n = owned.len();
    assert_eq!(c.tracks, n);
    assert_eq!(c.duplicate_files, s.dupes.iter().filter(|&&i| g[i].in_collection).count());
    let kinds = |folder: bool| {
        s.playlists.iter().filter(|p| p.0 == folder).map(|p| normalize(p.1)).collect::<FxHashSet<_>>().len()
    };
    assert_eq!((c.playlists, c.folders), (kinds(false), kinds(true)));
    assert_eq!(c.rated_share, share(owned.iter().filter(|&&i| g[i].rating > 0).count(), n));
    assert_eq!(c.coloured_share, share(owned.iter().filter(|&&i| g[i].coloured).count(), n));
    let played_owned = distinct.iter().filter(|&&i| g[i].in_collection).count();
    assert_eq!(c.played_share, if played.is_empty() { None } else { share(played_owned, n) });

    // The summary.
    let count = match basis {
        Basis::Collection => format!("{n} tracks"),
        _ => format!("{} {}{}", played.len(), basis.unit(), if played.len() == 1 { "" } else { "s" }),
    };
    assert!(r.summary.headline.ends_with(&count), "{:?} doesn't end with {count:?}", r.summary.headline);
    assert_eq!(r.summary.headline.contains("BPM"), t.core.is_some(), "{}", r.summary.headline);
    let tags: Vec<&str> = r.summary.tags.iter().map(|t| t.id).collect();
    assert!(tags.iter().all(|id| tag_section(id).is_some()), "{tags:?}");
    let harmonic_tag = m.keyed >= 20 && m.harmonic_share.is_some_and(|h| h >= 0.6);
    assert_eq!(tags.contains(&"harmonic"), harmonic_tag, "{m:?}");
    let repeats = sets.count >= 5 && sets.plays > 0 && sets.distinct_tracks as f64 / sets.plays as f64 >= 0.85;
    assert_eq!(tags.contains(&"rarely_repeats"), repeats);
    let setting = |key: &str| s.style.iter().find(|(k, _)| STYLE_SETTINGS[*k] == key).map(|(_, v)| STYLE_VALUES[*v]);
    assert_eq!(tags.contains(&"quantize_off"), setting("quantize") == Some("off"), "{:?}", s.style);
    assert_eq!(tags.contains(&"wide_tempo"), matches!(setting("tempo_range"), Some("16" | "wide")));
}

fn prefs_for(tc: &TestCase, r: &ProfileReport) -> ProfilePrefs {
    let names: Vec<String> = r.signature.played.iter().map(|p| p.name.clone()).collect();
    let some = |v: Vec<String>, n: usize| {
        if v.is_empty() { Vec::new() } else { tc.draw(gs::vecs(gs::sampled_from(v)).max_size(n)) }
    };
    let mut pins = names.clone();
    pins.push("Nobody - Nothing".into());
    let artists: Vec<String> = ARTISTS.iter().flat_map(|a| artist_credits(a)).collect();
    ProfilePrefs {
        sections: tc.draw(gs::optional(gs::btree_sets(gs::sampled_from(Section::ALL.to_vec())))),
        precision: if tc.draw(gs::booleans()) { Precision::Exact } else { Precision::Rounded },
        pinned: some(pins, 3),
        hidden: some(names, 2),
        hidden_artists: some(artists, 1),
        genre_aliases: IndexMap::new(),
        headline: None,
    }
}

/// Whether `x` is a whole number of `1 / units`.
fn rounded_to(x: f64, units: f64) -> bool {
    ((x * units).round() - x * units).abs() < 1e-6
}

/// The document of any report holds exactly the sections the preferences publish that have
/// something to say (sessions never counted), rounded as asked: shares on the 5% (or 0.1%)
/// grid, in [0, 1] and adding up where they're parts of one whole, BPM whole (or to a tenth),
/// counts at most what they count. It names no hidden track or artist, pinned tracks first,
/// and its headline and traits say nothing a left-out section would.
#[hegel::test(test_cases = 200)]
fn the_document_publishes_what_the_preferences_allow(tc: TestCase) {
    let s = tc.draw(scenario());
    let (r, _) = report_of(&s, Utc::now());
    let prefs = prefs_for(&tc, &r);
    let d = document(&r, &prefs);
    let on = prefs.published();
    let shows = |x: Section| on.contains(&x) && r.basis.publishes(x);
    let (units, bpm_units) = match prefs.precision {
        Precision::Rounded => (20.0, 1.0),
        Precision::Exact => (1000.0, 10.0),
    };
    let share_ok = |x: f64| (0.0..=1.0).contains(&x) && rounded_to(x, units);
    let count_ok = |c: Count, n: usize| match (c, prefs.precision) {
        (Count::Exact { count }, Precision::Exact) => count == n as u64,
        (Count::Exact { count }, Precision::Rounded) => count == n as u64 && n < 10,
        (Count::AtLeast { at_least }, Precision::Rounded) => at_least <= n as u64 && n >= 10 && at_least >= 5,
        (Count::AtLeast { .. }, Precision::Exact) => false,
    };

    assert_eq!(d.sets.is_some(), shows(Section::Sets) && r.sets.count > 0);
    if let Some(sets) = &d.sets {
        assert!(count_ok(sets.count, r.sets.count), "{:?} for {}", sets.count, r.sets.count);
        assert_eq!(sets.since, r.sets.oldest.map(|d| d.year()));
    }
    assert_eq!(d.tempo.is_some(), on.contains(&Section::Tempo) && r.tempo.core.is_some());
    if let Some(t) = &d.tempo {
        assert!(t.low <= t.median && t.median <= t.high, "{t:?}");
        assert!([t.low, t.high, t.median].iter().all(|b| rounded_to(*b, bpm_units)), "{t:?}");
    }
    assert_eq!(d.arc.is_some(), on.contains(&Section::Arc) && r.arc.sets > 0);
    if let Some(a) = &d.arc {
        assert!([a.builds, a.steady, a.winds_down].iter().all(|x| share_ok(*x)), "{a:?}");
        assert!(close(a.builds + a.steady + a.winds_down, 1.0), "{a:?}");
        let shape = match (a.builds >= 0.5, a.steady >= 0.5, a.winds_down >= 0.5) {
            (true, _, _) => ArcShape::Builds,
            (_, true, _) => ArcShape::Steady,
            (_, _, true) => ArcShape::WindsDown,
            _ => ArcShape::Varies,
        };
        assert_eq!(a.shape, shape);
    }
    assert_eq!(d.mixing.is_some(), on.contains(&Section::Mixing) && r.mixing.transitions > 0);
    if let Some(m) = &d.mixing {
        assert!([m.harmonic, m.smooth_tempo, m.genre_changes].iter().flatten().all(|x| share_ok(*x)), "{m:?}");
    }
    assert_eq!(d.keys.is_some(), on.contains(&Section::Keys) && r.keys.tracks > 0);
    if let Some(k) = &d.keys {
        let top: Vec<&String> = r.keys.top.iter().take(3).map(|k| &k.name).collect();
        assert_eq!(k.top.iter().collect::<Vec<_>>(), top);
    }
    assert_eq!(d.energy.is_some(), on.contains(&Section::Energy) && r.energy.is_some());
    if let Some(e) = &d.energy {
        assert!([e.high, e.mid, e.low].iter().all(|x| share_ok(*x)) && close(e.high + e.mid + e.low, 1.0), "{e:?}");
    }
    for (section, shares, sec) in [(Section::Genres, &d.genres, &r.genres), (Section::Labels, &d.labels, &r.labels)] {
        assert_eq!(shares.is_some(), on.contains(&section) && !sec.top.is_empty());
        for x in shares.iter().flatten() {
            assert!(share_ok(x.share) && sec.top.iter().take(5).any(|t| t.name == x.name), "{x:?}");
        }
    }
    assert!(d.labels.iter().flatten().map(|l| l.share).sum::<f64>() <= 1.0 + 1e-9);
    for a in d.artists.iter().flatten() {
        assert!(on.contains(&Section::Artists) && !prefs.is_hidden_artist(&a.name) && share_ok(a.share), "{a:?}");
    }
    assert_eq!(d.era.is_some(), on.contains(&Section::Era) && r.era.tracks > 0);
    assert_eq!(d.prep.is_some(), on.contains(&Section::Prep) && r.prep.is_some());
    if let Some(p) = &d.prep {
        assert!([p.hot_cues, p.memory_cues, p.loops].iter().all(|x| share_ok(*x)), "{p:?}");
    }
    assert_eq!(d.collection.is_some(), on.contains(&Section::Collection) && r.collection.tracks > 0);
    if let Some(c) = &d.collection {
        assert!(count_ok(c.tracks, r.collection.tracks) && count_ok(c.playlists, r.collection.playlists));
    }
    let style: Option<IndexMap<String, String>> = r
        .style
        .as_ref()
        .filter(|_| on.contains(&Section::Style))
        .map(|s| s.values.iter().map(|v| (camel(&v.key), v.value.clone())).collect());
    assert_eq!(d.style, style);

    // Signature: named only when allowed, the pins that may be named first, in order.
    assert!(d.signature.is_none() || on.contains(&Section::Signature));
    let named = d.signature.clone().unwrap_or_default();
    let unnamed: Vec<String> = r.signature.played.iter().filter(|p| p.unnamed).map(|p| normalize(&p.name)).collect();
    for t in &named {
        let name = if t.artist.is_empty() { t.title.clone() } else { format!("{} - {}", t.artist, t.title) };
        assert!(prefs.may_name(&name, &t.artist) && !unnamed.contains(&normalize(&name)), "{t:?}");
    }
    assert!(named.len() <= r.options.top.max(prefs.pinned.len()));

    // The summary says no more than the sections do.
    if on.contains(&Section::Summary) {
        let headline = d.headline.clone().unwrap_or_default();
        assert_eq!(headline.contains("BPM"), shows(Section::Tempo) && r.tempo.core.is_some(), "{headline:?}");
        assert!(!headline.contains("session"), "{headline:?}");
        for tag in &d.tags {
            let t = r.summary.tags.iter().find(|t| t.label == *tag).expect("a tag of the report");
            assert!(tag_section(t.id).is_some_and(shows), "{tag:?}");
        }
    } else {
        assert!(d.headline.is_none() && d.tags.is_empty());
    }
}

/// A document says when it was made to the millisecond, whatever the clock's resolution: the
/// same report written twice is the same length (chrono's own format drops trailing zeros).
#[hegel::test(test_cases = 100)]
fn a_document_is_dated_to_the_millisecond(tc: TestCase) {
    let o = ProfileOptions {
        since: None,
        until: None,
        min_set_tracks: 1,
        top: 5,
        analysis: false,
        genre_aliases: IndexMap::new(),
    };
    let nanos = tc.draw(gs::integers::<u32>().max_value(999_999_999));
    let at = DateTime::from_timestamp(1_760_000_000, nanos).unwrap();
    let doc = document::document(&compute(&builder(&o), Basis::Sessions, 0, at), &ProfilePrefs::default());
    let json = serde_json::to_value(&doc).unwrap();
    let millis = at.timestamp_subsec_millis();
    assert_eq!(json["generatedAt"], format!("2025-10-09T08:53:20.{millis:03}Z"), "{nanos}");
}
