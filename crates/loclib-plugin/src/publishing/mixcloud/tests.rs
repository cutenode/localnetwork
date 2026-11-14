use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// A show of 5-minute tracks by these artists, from these releases.
fn show(tracks: &[(&str, &str)]) -> Vec<TracklistEntry> {
    tracks
        .iter()
        .map(|(artist, album)| TracklistEntry {
            artist: Some(artist.to_string()),
            title: Some("Track".into()),
            album: Some(album.to_string()),
            duration_seconds: Some(300),
            ..TracklistEntry::default()
        })
        .collect()
}

#[test]
fn mixcloud_allows_three_from_a_release_two_in_a_row() {
    assert!(warnings(&show(&[("A", "X"), ("A", "X"), ("B", "Y"), ("A", "X")])).is_empty());
    assert_eq!(
        warnings(&show(&[("A", "X"), ("A", "X"), ("B", "Y"), ("A", "X"), ("C", "x")])),
        [format!("4 tracks from \"X\": Mixcloud allows 3 {LIMITED}")]
    );
    assert_eq!(
        warnings(&show(&[("A", "X"), ("B", "X"), ("C", "X"), ("D", "Y")])),
        [format!("tracks 1-3 are from \"X\": Mixcloud allows 2 in a row {LIMITED}")]
    );
    assert!(warnings(&show(&[("A", ""), ("B", " "), ("C", ""), ("D", ""), ("E", "")])).is_empty());
}

#[test]
fn mixcloud_allows_four_by_an_artist_three_in_a_row() {
    assert!(warnings(&show(&[("A", ""), ("A", ""), ("A", ""), ("B", ""), ("A feat. C", "")])).is_empty());
    assert_eq!(
        warnings(&show(&[("A", ""), ("a", ""), ("A", ""), ("A feat. C", ""), ("B", "")])),
        [format!("tracks 1-4 are by A: Mixcloud allows 3 in a row {LIMITED}")]
    );
    let spread = [("A", ""), ("B", ""), ("A", ""), ("C", ""), ("A", ""), ("D", ""), ("C, A", ""), ("E", "")];
    assert!(warnings(&show(&spread)).is_empty());
    assert_eq!(
        warnings(&show(&[&spread[..], &[("A", "")]].concat())),
        [format!("5 tracks by A: Mixcloud allows 4 {LIMITED}")]
    );
}

/// A track published unnamed has no artist or album, so it counts towards neither limit.
#[test]
fn an_unnamed_track_counts_towards_no_limit() {
    let mut tracklist = show(&[("A", "X"); 5]);
    for e in &mut tracklist[1..4] {
        (e.artist, e.title, e.album) = (None, None, None);
    }
    assert!(warnings(&tracklist).is_empty(), "{:?}", warnings(&tracklist));
}

#[test]
fn mixcloud_wants_fifteen_minutes() {
    assert_eq!(
        warnings(&show(&[("A", "X"), ("A", "X")])),
        ["it's 10 minutes long: Mixcloud doesn't allow copyrighted music in uploads under 15 minutes"]
    );
    let mut tracklist = show(&[("A", "X"), ("A", "X"), ("A", "X")]);
    assert_eq!(warnings(&tracklist).len(), 1, "15 minutes is long enough for the rules to apply");
    tracklist[2].start_seconds = Some(500);
    assert!(warnings(&tracklist)[0].starts_with("it's 13 minutes long"), "a recording's length goes by its times");
    tracklist[2].start_seconds = None;
    tracklist[0].duration_seconds = None;
    assert!(!warnings(&tracklist)[0].starts_with("it's"), "a length not known is no reason to warn");
    assert!(warnings(&[]).is_empty());
}

/// Artists and albums as tracklists have them: with credits, in either case, blank, or left out
/// for a track published unnamed.
const ARTISTS: &[&str] = &["Floorplan", "floorplan", "Someone, Other", "A feat. B", "", "Tab\tArtist"];
const ALBUMS: &[&str] = &["", " ", "Paradise", "PARADISE", "Some, Album"];

/// Mixcloud's rules, as a model counts them: under 15 minutes (to the end of the last track when
/// it has a start, else the tracks' lengths added up when each is known), the one warning about
/// length; else a release on more than 3 tracks or more than 2 in a row, an artist credited on more
/// than 4 or more than 3 in a row, each named as first spelled (ignoring case), and no warning
/// when none is.
#[hegel::test(test_cases = 300)]
fn prop_mixcloud_rules_are_counted_as_the_model_does(tc: TestCase) {
    let tracklist: Vec<TracklistEntry> = (0..tc.draw(gs::integers::<usize>().max_value(12)))
        .map(|_| TracklistEntry {
            artist: tc.draw(gs::optional(gs::sampled_from(ARTISTS.to_vec()))).map(String::from),
            title: Some("Track".into()),
            album: tc.draw(gs::optional(gs::sampled_from(ALBUMS.to_vec()))).map(String::from),
            start_seconds: tc.draw(gs::optional(gs::integers::<u32>().max_value(20_000))),
            duration_seconds: tc.draw(gs::optional(gs::integers::<u32>().max_value(600))),
        })
        .collect();
    let length = match tracklist.last() {
        Some(e) if e.start_seconds.is_some() => Some(e.start_seconds.unwrap() + e.duration_seconds.unwrap_or(0)),
        _ => tracklist.iter().map(|e| e.duration_seconds).sum::<Option<u32>>(),
    };
    let got = warnings(&tracklist);
    if let Some(length) = length.filter(|&l| l > 0 && l < 900) {
        assert_eq!(
            got,
            [format!(
                "it's {} minutes long: Mixcloud doesn't allow copyrighted music in uploads under 15 minutes",
                length / 60
            )]
        );
        return;
    }
    let releases: Vec<Vec<String>> =
        tracklist.iter().map(|e| e.album.clone().filter(|a| !a.trim().is_empty()).into_iter().collect()).collect();
    let artists: Vec<Vec<String>> =
        tracklist.iter().map(|e| artist_credits(e.artist.as_deref().unwrap_or_default())).collect();
    let model = |tracks: &[Vec<String>], what: &dyn Fn(&str) -> String, most: usize, in_a_row: usize| {
        let mut names: Vec<(String, String)> = Vec::new();
        for c in tracks.iter().flatten() {
            if !names.iter().any(|n| n.0 == normalize(c)) {
                names.push((normalize(c), c.clone()));
            }
        }
        let has = |i: usize, key: &str| tracks[i].iter().any(|c| normalize(c) == key);
        let mut out: Vec<String> = names
            .iter()
            .map(|(key, name)| (name, (0..tracks.len()).filter(|&i| has(i, key)).count()))
            .filter(|(_, n)| *n > most)
            .map(|(name, n)| format!("{n} tracks {}: Mixcloud allows {most} {LIMITED}", what(name)))
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
                        "tracks {}-{i} are {}: Mixcloud allows {in_a_row} in a row {LIMITED}",
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
