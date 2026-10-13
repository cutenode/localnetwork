//! Sets through the API: declared from a kept mix report or typed in, edited, listed, written out,
//! asked about, and turned into a playlist on a drive. (Building from sessions is covered by
//! `sets::build`'s own tests: these never gather sessions.)

mod sources;

use crate::common::{self, path};

use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate, Utc};
use locallibrary::api::{
    self, EditSet, Insert, Move, NewSet, SetExport, SetFields, SetFormat, SetPlaylist, SetRef, SetsQuery,
    TrackPlaysQuery,
};
use locallibrary::error::Reason;
use locallibrary::mix::{MixInfo, MixReport, Play, report};
use locallibrary::progress::Ctx;
use locallibrary::sets::{EntryOrigin, MusicBrainzIds, SETS_FILE, Set};
use locallibrary::{Device, Store, Track};
use uuid::Uuid;

/// A store, and a drive in the only folder drives are looked for in.
struct Setup {
    _dir: tempfile::TempDir,
    store: Store,
    drive: PathBuf,
    tracks: Vec<Track>,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    let volumes = dir.path().join("volumes");
    let drive = common::bigger_drive(&volumes, "Main");
    locallibrary::testing::volumes_on_this_thread(Some(vec![volumes]));
    let tracks = Device::open(&drive).unwrap().library().unwrap().tracks;
    Setup { _dir: dir, store, drive, tracks }
}

/// A report of a recording that plays `tracks` from these seconds, at this confidence.
pub(crate) fn report(plays: &[(&Track, f64, f64)], date: Option<&str>) -> MixReport {
    let play = |&(t, start, confidence): &(&Track, f64, f64)| Play {
        track: t.clone(),
        start,
        end: start + 200.0,
        track_start: 0.0,
        track_end: 200.0,
        tempo: 1.0,
        played_bpm: None,
        key_lock: None,
        semitones: 0.0,
        key: None,
        played_key: None,
        confidence,
        votes: 100,
        presence: Vec::new(),
        presence_step: 2.0,
        jumps: 0,
        alternates: Vec::new(),
    };
    MixReport {
        mix: MixInfo {
            path: "/mixes/Friday/set.wav".into(),
            kind: "audio",
            title: "Friday".into(),
            artist: None,
            genre: None,
            date: date.map(String::from),
            recorder: None,
            duration_secs: 3600.0,
            sample_rate: 44100,
            channels: 2,
            labels: Vec::new(),
            cover: None,
        },
        drive: "Main".into(),
        drive_id: None,
        library: Default::default(),
        plays: plays.iter().map(play).collect(),
        transitions: Vec::new(),
        unidentified: Vec::new(),
        coverage: 0.9,
        waveform: Default::default(),
        tempo_range: 0.16,
        analysed_at: Utc::now(),
        elapsed_secs: 1.0,
    }
}

fn fields(title: &str, date: &str) -> SetFields {
    SetFields { title: Some(title.into()), date: Some(date.into()), ..Default::default() }
}

fn new(s: &Setup, req: NewSet) -> Set {
    api::set_new(&s.store, &req, &Ctx::silent()).unwrap().set
}

fn edit(s: &Setup, req: EditSet) -> locallibrary::Result<api::SetChange> {
    api::set_edit(&s.store, &req, &Ctx::silent())
}

fn names(set: &Set) -> Vec<String> {
    set.tracks.iter().map(|e| e.name()).collect()
}

fn day(d: &str) -> NaiveDate {
    NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
}

#[test]
fn a_set_is_built_from_a_kept_report() {
    let s = setup();
    let t = &s.tracks;
    let r = report(&[(&t[2], 610.0, 0.9), (&t[0], 4.7, 0.9), (&t[1], 300.0, 0.3)], Some("2026-09-12T23:00:00"));
    let id = report::save(&s.store, &r).unwrap();
    let req = NewSet {
        fields: SetFields {
            title: Some("Friday".into()),
            venue: Some("The Warehouse".into()),
            with: Some(vec!["Someone".into(), "did:plc:abc".into()]),
            ..Default::default()
        },
        report: Some(id.clone()),
        ..Default::default()
    };
    let made = api::set_new(&s.store, &req, &Ctx::silent()).unwrap();
    let set = &made.set;
    assert_eq!(set.date, day("2026-09-12"), "the recording's own date");
    assert_eq!(set.id.len(), 13);
    assert_eq!(names(set), [t[0].display_name(), t[1].display_name(), t[2].display_name()]);
    assert_eq!(set.tracks.iter().map(|e| e.start_seconds).collect::<Vec<_>>(), [Some(4), Some(300), Some(610)]);
    assert!(set.tracks.iter().all(|e| e.origin == EntryOrigin::Recording));
    let rec = set.recording.as_ref().unwrap();
    assert_eq!((rec.path.as_path(), rec.report.as_deref()), (Path::new("/mixes/Friday/set.wav"), Some(id.as_str())));
    assert_eq!(set.with[1].did.as_deref(), Some("did:plc:abc"));
    assert_eq!(made.notes, [format!("2. {}: a tentative match (30%), check it", t[1].display_name())]);
    assert_eq!(api::sets_list(&s.store, &SetsQuery::default()).unwrap(), [made.set], "kept in the store");

    // Typed in from nothing, it's dated today unless told.
    let made = api::set_new(&s.store, &NewSet::default(), &Ctx::silent()).unwrap();
    let today = Local::now().date_naive();
    assert_eq!(
        (made.set.date, made.notes),
        (today, vec![format!("dated today ({today}): give the day it was played")])
    );
    let made =
        api::set_new(&s.store, &NewSet { fields: fields("Radio", "2026-10-01"), ..Default::default() }, &Ctx::silent());
    assert!(made.unwrap().notes.is_empty());

    let both = NewSet { report: Some(id), recording: Some("/mixes/set.wav".into()), ..Default::default() };
    assert!(api::set_new(&s.store, &both, &Ctx::silent()).is_err());
    let missing = NewSet { report: Some("nope".into()), ..Default::default() };
    assert_eq!(api::set_new(&s.store, &missing, &Ctx::silent()).unwrap_err().code(), "not_found");
    assert_eq!(api::sets_list(&s.store, &SetsQuery::default()).unwrap().len(), 3, "refused sets aren't kept");
}

/// Tag `track`'s file on the drive at `root` with MusicBrainz IDs, as Picard would (its release's
/// the same as its recording's).
fn tag_musicbrainz(root: &Path, track: &Track, recording: Uuid, artists: &[Uuid]) {
    use lofty::config::WriteOptions;
    use lofty::tag::{ItemKey, Tag, TagExt, TagType};
    let file = root.join(track.file_path.trim_start_matches('/'));
    // The drive's files are MP3s by name.
    std::fs::write(&file, common::media::mp3()).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::MusicBrainzRecordingId, recording.to_string());
    tag.insert_text(ItemKey::MusicBrainzReleaseId, recording.to_string());
    for a in artists {
        tag.push(lofty::tag::TagItem::new(ItemKey::MusicBrainzArtistId, lofty::tag::ItemValue::Text(a.to_string())));
    }
    tag.save_to_path(&file, WriteOptions::default()).unwrap();
}

/// A set's tracks keep the MusicBrainz IDs their files on a mounted drive are tagged with: read
/// when they go in, and, for tracks without, again whenever the set is edited.
#[test]
fn tracks_keep_the_musicbrainz_ids_their_files_are_tagged_with() {
    let s = setup();
    let t = &s.tracks;
    let (recording, artist) = (Uuid::from_u128(1), Uuid::from_u128(2));
    tag_musicbrainz(&s.drive, &t[0], recording, &[artist]);
    let id = report::save(&s.store, &report(&[(&t[0], 0.0, 0.9), (&t[1], 200.0, 0.9)], None)).unwrap();
    let set = new(&s, NewSet { fields: fields("Friday", "2026-10-03"), report: Some(id), ..Default::default() });
    let ids = |set: &Set| set.tracks.iter().map(|e| e.musicbrainz.clone()).collect::<Vec<_>>();
    let tagged = MusicBrainzIds { recording: Some(recording), release: Some(recording), artists: vec![artist] };
    assert_eq!(ids(&set), [tagged.clone(), MusicBrainzIds::default()]);

    tag_musicbrainz(&s.drive, &t[1], Uuid::from_u128(3), &[]);
    let typed = Insert { at: 3, track: "Someone - Dubplate".into() };
    let set = edit(&s, EditSet { set: set.id, insert: vec![typed], ..Default::default() }).unwrap().set;
    let later =
        MusicBrainzIds { recording: Some(Uuid::from_u128(3)), release: Some(Uuid::from_u128(3)), artists: vec![] };
    assert_eq!(ids(&set), [tagged, later, MusicBrainzIds::default()]);
}

#[test]
fn a_set_is_edited_in_one_go_or_not_at_all() {
    let s = setup();
    let t = &s.tracks;
    let set = new(&s, NewSet { fields: fields("Friday", "2026-10-03"), ..Default::default() });
    // Looked up on the only drive there is, or typed.
    let insert = |at: usize, track: &str| Insert { at, track: track.into() };
    let changed = edit(
        &s,
        EditSet {
            set: "friday".into(),
            insert: vec![insert(1, &t[0].title), insert(1, "Someone - Dubplate"), insert(1, &t[1].file_path)],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(names(&changed.set), [t[0].display_name(), "Someone - Dubplate".into(), t[1].display_name()]);
    assert_eq!(changed.set.tracks[0].file.as_deref(), Some(t[0].file_path.as_str()));
    assert_eq!(
        changed.notes,
        [
            format!("{} went in from the library", t[0].display_name()),
            "\"Someone - Dubplate\" went in as typed: no track matches it".to_string(),
            format!("{} went in from the library", t[1].display_name()),
        ]
    );
    assert!(changed.set.updated_at > set.updated_at);

    let changed = edit(
        &s,
        EditSet {
            set: "friday".into(),
            fields: SetFields { venue: Some("Warehouse".into()), title: Some(String::new()), ..Default::default() },
            drop: vec![2],
            moves: vec![Move { from: 3, to: 1 }],
            insert: vec![insert(4, "Late - One")],
            unnamed: vec![1],
            ..Default::default()
        },
    )
    .unwrap()
    .set;
    assert_eq!(names(&changed), [t[1].display_name(), t[0].display_name(), "Late - One".into()]);
    assert_eq!(changed.tracks.iter().map(|e| e.unnamed).collect::<Vec<_>>(), [false, true, false]);
    assert_eq!((changed.title, changed.venue.as_deref()), (None, Some("Warehouse")));

    // A refused edit leaves the store as it was.
    let kept = std::fs::read(s.store.root().join(SETS_FILE)).unwrap();
    let refused = [
        EditSet { set: changed.id.clone(), drop: vec![4], ..Default::default() },
        EditSet {
            set: changed.id.clone(),
            fields: SetFields { venue: Some(String::new()), date: Some("Friday".into()), ..Default::default() },
            ..Default::default()
        },
        EditSet {
            set: changed.id.clone(),
            insert: vec![insert(1, "x")],
            report: Some("any".into()),
            ..Default::default()
        },
        EditSet { set: "nope".into(), ..Default::default() },
    ];
    for req in refused {
        assert!(edit(&s, req).is_err());
    }
    assert_eq!(std::fs::read(s.store.root().join(SETS_FILE)).unwrap(), kept);

    // A recording rebuilds the tracklist and keeps the details; an unnamed track stays unnamed.
    let id = report::save(&s.store, &report(&[(&t[0], 0.0, 0.9), (&t[2], 200.0, 0.9)], None)).unwrap();
    let changed = edit(&s, EditSet { set: changed.id.clone(), report: Some(id), ..Default::default() }).unwrap();
    assert_eq!(names(&changed.set), [t[0].display_name(), t[2].display_name()]);
    assert!(changed.set.tracks[0].unnamed);
    assert_eq!(changed.set.venue.as_deref(), Some("Warehouse"));
    assert_eq!(changed.set.date, day("2026-10-03"));
    assert_eq!(
        changed.notes,
        [
            format!("{}: not heard in the recording", t[1].display_name()),
            "Late - One: not heard in the recording".into()
        ]
    );
}

#[test]
fn sets_are_listed_found_and_deleted() {
    let s = setup();
    assert!(api::sets_list(&s.store, &SetsQuery::default()).unwrap().is_empty(), "no sets.json is no sets");
    let at = |title: &str, date: &str, venue: &str, kind: &str| {
        let f = SetFields { venue: Some(venue.into()), kind: Some(kind.into()), ..fields(title, date) };
        new(&s, NewSet { fields: f, ..Default::default() })
    };
    at("Warm-up", "2026-09-12", "The Warehouse", "club");
    at("Friday", "2026-10-03", "Warehouse Project", "club");
    at("Show", "2026-10-02", "NTS", "radio");
    let titles = |q: SetsQuery| -> Vec<String> {
        api::sets_list(&s.store, &q).unwrap().into_iter().map(|s| s.title.unwrap()).collect()
    };
    assert_eq!(titles(SetsQuery::default()), ["Friday", "Show", "Warm-up"], "newest first");
    assert_eq!(titles(SetsQuery { venue: Some("warehouse".into()), ..Default::default() }), ["Friday", "Warm-up"]);
    assert_eq!(titles(SetsQuery { kind: Some("Radio".into()), ..Default::default() }), ["Show"]);
    assert_eq!(titles(SetsQuery { since: Some("2026-10".into()), ..Default::default() }), ["Friday", "Show"]);
    assert_eq!(titles(SetsQuery { until: Some("2026-09".into()), ..Default::default() }), ["Warm-up"]);
    assert!(api::sets_list(&s.store, &SetsQuery { since: Some("soon".into()), ..Default::default() }).is_err());

    let get = |set: Option<&str>, venue: Option<&str>| {
        api::set_get(&s.store, &SetRef { set: set.map(String::from), venue: venue.map(String::from) })
    };
    assert_eq!(get(None, Some("WAREHOUSE")).unwrap().title.as_deref(), Some("Friday"), "the latest there");
    assert_eq!(get(None, Some("the warehouse")).unwrap().title.as_deref(), Some("Warm-up"));
    assert_eq!(get(Some("2026-10-02"), None).unwrap().title.as_deref(), Some("Show"));
    assert_eq!(get(None, Some("fabric")).unwrap_err().code(), "not_found");
    assert!(get(None, None).is_err());

    let gone = api::set_delete(&s.store, &SetRef { set: Some("show".into()), venue: None }).unwrap();
    assert_eq!(gone.title.as_deref(), Some("Show"));
    assert_eq!(titles(SetsQuery::default()), ["Friday", "Warm-up"]);
}

#[test]
fn sets_are_written_out_and_files_kept() {
    let s = setup();
    let t = &s.tracks;
    let id = report::save(&s.store, &report(&[(&t[0], 0.0, 0.9), (&t[1], 312.0, 0.9)], None)).unwrap();
    let set = new(&s, NewSet { fields: fields("Friday", "2026-10-03"), report: Some(id), ..Default::default() });
    edit(
        &s,
        EditSet {
            set: set.id.clone(),
            insert: vec![Insert { at: 3, track: "Someone - Dubplate".into() }],
            ..Default::default()
        },
    )
    .unwrap();
    let export = |format: SetFormat, output: Option<PathBuf>, replace: bool| {
        let req = SetExport { set: "friday".into(), format, drive: None, output, replace };
        api::set_export(&s.store, &req)
    };
    let text = export(SetFormat::Text, None, false).unwrap();
    let expected = format!("01. {}\n02. {}\n03. Someone - Dubplate\n", t[0].display_name(), t[1].display_name());
    assert_eq!((text.text.as_deref(), text.tracks), (Some(expected.as_str()), 3));

    let file = s.store.root().join("friday.txt");
    let written = export(SetFormat::Text, Some(file.clone()), false).unwrap();
    assert_eq!((written.text, written.path.as_deref()), (None, Some(file.as_path())));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), expected);
    let e = export(SetFormat::Csv, Some(file.clone()), false).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("conflict", Some(Reason::Exists)), "{e}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), expected);
    export(SetFormat::Csv, Some(file.clone()), true).unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().starts_with("#,Start,Artist"));

    // The tracks found on the drive (the only one there is), paths relative to the playlist.
    let list = s.drive.join("friday.m3u8");
    let m3u8 = export(SetFormat::M3u8, Some(list.clone()), false).unwrap();
    assert_eq!(m3u8.tracks, 2);
    assert_eq!(m3u8.warnings, ["3. Someone - Dubplate isn't on the drive: left out"]);
    let paths: Vec<String> =
        std::fs::read_to_string(&list).unwrap().lines().filter(|l| !l.starts_with('#')).map(String::from).collect();
    assert_eq!(paths, [&t[0].file_path[1..], &t[1].file_path[1..]]);

    let e = export(SetFormat::Youtube, None, false).unwrap_err();
    assert!(e.to_string().starts_with("track 3 (Someone - Dubplate) has no start time"), "{e}");
}

#[test]
fn a_track_is_found_in_the_sets_it_was_played_in() {
    let s = setup();
    let t = &s.tracks;
    let a = new(&s, NewSet { fields: fields("A", "2026-09-12"), ..Default::default() });
    let b = new(&s, NewSet { fields: fields("B", "2026-10-03"), ..Default::default() });
    let insert = |set: &Set, tracks: &[&str]| {
        let insert = tracks.iter().map(|x| Insert { at: 1, track: x.to_string() }).collect();
        edit(&s, EditSet { set: set.id.clone(), insert, ..Default::default() }).unwrap();
    };
    insert(&a, &[&t[0].file_path, &t[1].file_path]);
    insert(&b, &["Someone - Dubplate", &t[1].file_path]);
    let plays = |q: &str| api::track_plays(&s.store, &TrackPlaysQuery { track: q.into() });
    let found: Vec<(String, usize)> =
        plays(&t[1].title).unwrap().into_iter().map(|a| (a.title.unwrap(), a.position)).collect();
    assert_eq!(found, [("B".to_string(), 2), ("A".to_string(), 2)]);
    assert_eq!(plays(&t[0].file_path.to_uppercase()).unwrap().len(), 1, "by file");
    assert_eq!(plays("someone - dub").unwrap()[0].track, "Someone - Dubplate");
    assert!(plays(" ").is_err());
}

#[test]
fn a_set_becomes_a_playlist_on_a_drive() {
    let s = setup();
    let t = &s.tracks;
    let set = new(&s, NewSet { fields: fields("Fri/day", "2026-10-03"), ..Default::default() });
    let paths = [&t[3].file_path, &t[0].file_path, "Someone - Dubplate", &t[3].file_path];
    let insert = paths.iter().map(|x| Insert { at: 1, track: x.to_string() }).collect();
    edit(&s, EditSet { set: set.id.clone(), insert, ..Default::default() }).unwrap();
    let req = SetPlaylist { set: set.id.clone(), drive: path(&s.drive), name: None };
    let before = common::Before::take(&s.drive);
    let planned = api::set_playlist_plan(&s.store, &req, &Ctx::silent()).unwrap();
    assert_eq!(planned.warnings, ["3. Someone - Dubplate isn't on the drive: left out"]);
    before.untouched(&s.drive).unwrap();
    planned.apply(Some(&s.store), &Ctx::silent()).unwrap();
    let lib = Device::open(&s.drive).unwrap().library().unwrap();
    let p = lib.find_playlist("Sets/2026-10-03 Fri-day").expect("the playlist");
    assert_eq!(p.track_ids, [t[3].id, t[0].id], "in set order, each once");
    // Made again, there's nothing to add.
    assert!(api::set_playlist_plan(&s.store, &req, &Ctx::silent()).unwrap().noop);

    let named = SetPlaylist { name: Some("Gigs/Friday".into()), ..req };
    api::set_playlist_apply(&s.store, &named, &Ctx::silent()).unwrap();
    assert!(Device::open(&s.drive).unwrap().library().unwrap().find_playlist("Gigs/Friday").is_some());

    let typed = new(&s, NewSet { fields: fields("Typed", "2026-10-04"), ..Default::default() });
    let one = vec![Insert { at: 1, track: "Someone - Dubplate".into() }];
    edit(&s, EditSet { set: typed.id.clone(), insert: one, ..Default::default() }).unwrap();
    let req = SetPlaylist { set: typed.id, drive: path(&s.drive), name: None };
    assert_eq!(api::set_playlist_plan(&s.store, &req, &Ctx::silent()).unwrap_err().code(), "not_found");
}
