use super::*;

fn lines(l: Vec<(&'static str, &'static str, String)>) -> Vec<String> {
    l.into_iter().map(|(_, title, text)| format!("{title}: {text}")).collect()
}

/// Every section a profile record can hold reads as one line; empty ones are left out.
#[test]
fn a_profile_record_reads_as_a_line_per_section() {
    let record = json!({
        "headline": "Techno warm-ups",
        "tags": [{ "name": "Mixes in key" }, { "name": "Plays long sets" }],
        "sets": { "total": { "atLeast": 50 }, "since": 2019, "medianTracks": 18 },
        "tempo": { "low": 122, "high": 128, "median": 125 },
        "arc": { "shape": "builds", "builds": 60, "steady": 30, "windsDown": 10 },
        "mixing": { "harmonic": 70, "genreChanges": 20 },
        "keys": { "top": [{ "key": "8A" }, { "key": "9A" }], "minor": 80 },
        "energy": { "high": 50, "mid": 30, "low": 20 },
        "genres": [{ "name": "Techno", "share": 80 }, { "name": "House", "share": 20 }],
        "labels": [],
        "artists": [{ "name": "Floorplan", "share": 5 }],
        "era": { "medianYear": 2019 },
        "signature": [{ "artist": "Floorplan", "title": "Never Grow Old" }, { "title": "Untitled" }],
        "prep": { "hotCues": 90, "memoryCues": 50, "loops": 10 },
        "collection": { "tracks": { "count": 1200 }, "playlists": {} },
        "style": { "quantize": "off", "madeUp": "x" },
    });
    assert_eq!(
        lines(profile_lines(&record)),
        [
            "Headline: Techno warm-ups",
            "Traits: Mixes in key, Plays long sets",
            "Sets: 50+ since 2019, 18 tracks in a typical set",
            "Tempo: 122-128 BPM, median 125",
            "Set shape: builds (builds 60%, steady 30%, winds down 10%)",
            "Mixing: 70% in key, 20% change genre",
            "Keys: 8A, 9A; 80% minor",
            "Energy: high 50%, mid 30%, low 20%",
            "Genres: Techno 80%, House 20%",
            "Artists: Floorplan 5%",
            "Era: half released by 2019",
            "Signature tracks: Floorplan - Never Grow Old; Untitled",
            "Prep: hot cues 90%, memory cues 50%, loops 10%",
            "Collection: 1200 tracks, ? playlists",
            "Player style: Quantize Off, made_up x",
        ]
    );
    assert!(profile_lines(&json!({})).is_empty());
}

#[test]
fn a_set_record_reads_as_its_details_and_tracks() {
    let record = json!({
        "title": "Friday",
        "date": "2026-10-03",
        "kind": "club",
        "venue": "Warehouse",
        "location": { "locality": "London", "country": "GB" },
        "playedWith": [{ "name": "DJ Two" }, { "name": "Partner", "did": "did:plc:abc" }],
        "event": "at://did:plc:abc/x.event/1",
        "links": [
            { "uri": "https://www.mixcloud.com/dj/friday/" },
            { "uri": "https://radio.example/archive/friday", "name": "Archive" },
        ],
        "sources": [{ "kind": "history" }, { "kind": "mix" }],
    });
    assert_eq!(
        lines(set_lines(&record)),
        [
            "Title: Friday",
            "Date: 2026-10-03",
            "Kind: club",
            "Venue: Warehouse",
            "Where: London, GB",
            "With: DJ Two, Partner (did:plc:abc)",
            "Event: at://did:plc:abc/x.event/1",
            "Listen: https://www.mixcloud.com/dj/friday/, Archive (https://radio.example/archive/friday)",
            "From: history, mix",
        ]
    );
    assert_eq!(lines(set_lines(&json!({ "date": "2026-10-03", "playedWith": [] }))), ["Date: 2026-10-03"]);

    // Plain text, whether or not the tests run in a terminal.
    console::set_colors_enabled(false);
    let timed = json!({ "startSeconds": 295, "artist": "Floorplan", "title": "Never Grow Old", "bpmCentis": 12650, "key": "8A" });
    assert_eq!(set_track(&timed), "4:55  Floorplan - Never Grow Old  126.50 BPM · 8A");
    assert_eq!(set_track(&json!({ "startSeconds": 0, "unidentified": true })), "0:00  ID - ID");
    assert_eq!(set_track(&json!({ "title": "Dubplate" })), "Dubplate");
}
