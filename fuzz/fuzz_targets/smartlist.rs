// paths: fuzz/fuzz_targets/smartlist.rs
// paths: fuzz/seeds/smartlist/**
// paths: crates/loclib-rekordbox-db/src/masterdb/smartlist.rs
// paths: crates/loclib-model/src/model.rs
//! Intelligent playlists' rules (`djmdPlaylist.SmartList` in `master.db`): parsing the XML, then
//! evaluating the rules against tracks and writing them as a filter.

#![no_main]

use chrono::NaiveDate;
use libfuzzer_sys::fuzz_target;
use locallibrary::Track;
use loclib_rekordbox_db::masterdb::SmartList;

fuzz_target!(|data: &[u8]| {
    // The column is read as text, lossily (`masterdb`'s `text`).
    let xml = String::from_utf8_lossy(data);
    // Short ids name a My Tag, so tag conditions both match and miss.
    let Some(list) = SmartList::parse(&xml, |id| (id.len() < 4).then(|| format!("Genre/{id}"))) else { return };
    list.unevaluable();
    let _ = list.filter();
    let track = Track {
        artist: "Daft Punk".into(),
        bpm: 128.0,
        rating: 5,
        date_added: "2024-03-01".into(),
        release_date: "2000-11-30".into(),
        ..Default::default()
    };
    let tags = ["Genre/1".to_string()];
    // "In the last" counts back from today: the earliest and latest days too, where it can't.
    let today = NaiveDate::from_ymd_opt(2026, 11, 13).expect("a date");
    for t in [&Track::default(), &track] {
        for day in [today, NaiveDate::MIN, NaiveDate::MAX] {
            list.matches(t, &tags, day);
        }
    }
});
