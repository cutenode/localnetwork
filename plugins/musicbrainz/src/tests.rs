use std::io::pipe;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use loclib_model::Track;
use loclib_plugin::{Fetch, Fetched, HelloReply, Incoming, Manifest, NetLimit, PROTOCOL_VERSION, Peer, Role, method};
use loclib_progress::{CancelToken, Ctx, Event};
use serde_json::{Value, json};

use super::*;

/// MusicBrainz's answers to ISRC lookups, as it gave them in October 2026: Young Fathers' "In My
/// View" (one recording, on its single, five editions of "Cocoa Sugar" released the same day and
/// two compilations) and Chris Lake ft. Alexis Roberts' "Turn off the Lights". The third is made
/// in their shape: an ISRC that's three recordings, two of them titled alike.
const IN_MY_VIEW_XML: &str = include_str!("../fixtures/GBCFB1700586.xml");
const LIGHTS_XML: &str = include_str!("../fixtures/QMDA61831832.xml");
const SEVERAL_XML: &str = include_str!("../fixtures/several-recordings.xml");
const IN_MY_VIEW: &str = "GBCFB1700586";
const LIGHTS: &str = "QMDA61831832";
const SEVERAL: &str = "GBCFB1700587";

const RECORDING: &str = "8dfae998-efd4-4ebe-a482-1523ff687362";
const COCOA_SUGAR: &str = "11ce37ca-3894-4269-a609-5bb916bfcedf";
const YOUNG_FATHERS: &str = "24aa5c20-a275-468a-93a9-b3d96b344b6c";
const CHRIS_LAKE: &str = "fca94d64-9ada-4264-9373-4158e2f7c7e7";
const ALEXIS_ROBERTS: &str = "7689466c-d9c4-49a0-a998-be5698c08793";

/// Where the plugin looks ISRCs up.
const LOOKUP: &str = "https://musicbrainz.org/ws/2/isrc/";

/// How MusicBrainz answers a lookup, through the host: a status, its `Retry-After` and a body, or
/// the host's failure to get an answer.
enum Reply {
    Answer(u16, Option<&'static str>, String),
    Fails(RpcError),
    /// No answer yet: the fetch is under way until the plugin's hook is cancelled.
    Held,
}

fn ok(xml: impl Into<String>) -> Reply {
    Reply::Answer(200, None, xml.into())
}

fn status(status: u16) -> Reply {
    Reply::Answer(status, None, "<html>no</html>".into())
}

fn busy(retry_after: &'static str) -> Reply {
    Reply::Answer(503, Some(retry_after), String::new())
}

/// MusicBrainz as it is: the fixtures' ISRCs, and no other.
fn as_it_is(isrc: &str, _asked: usize) -> Reply {
    match isrc {
        IN_MY_VIEW => ok(IN_MY_VIEW_XML),
        LIGHTS => ok(LIGHTS_XML),
        SEVERAL => ok(SEVERAL_XML),
        _ => status(404),
    }
}

/// A read the plugin called back for: its method and params.
type Read = (String, Value);

/// The plugin served on a thread over pipes, as [`serve`] serves it, past `hello`, with a host
/// whose library has `tracks` and whose fetches MusicBrainz answers as the route does (given the
/// ISRC asked about, and how many times it was asked before).
struct Session {
    host: Peer,
    served: JoinHandle<()>,
    answering: JoinHandle<()>,
    /// The reads the plugin called back for.
    reads: Arc<Mutex<Vec<Read>>>,
    /// The URLs it fetched.
    fetched: Arc<Mutex<Vec<String>>>,
    /// Told of each fetch once it's asked.
    heard: Receiver<()>,
    /// What the plugin answered `hello`.
    hello: HelloReply,
    /// The library's tracks.
    tracks: Vec<Track>,
}

fn start(tracks: Vec<Track>, route: impl Fn(&str, usize) -> Reply + Send + 'static) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let reads: Arc<Mutex<Vec<Read>>> = Arc::default();
    let fetched: Arc<Mutex<Vec<String>>> = Arc::default();
    let (tell, heard) = channel();
    let (to, read, fetches, library) = (host.clone(), reads.clone(), fetched.clone(), tracks.clone());
    let answering = std::thread::spawn(move || {
        for msg in incoming {
            let Incoming::Request(req) = msg else { continue };
            if req.method != method::NET_FETCH {
                read.lock().unwrap().push((req.method.clone(), req.params.clone()));
                let ids: Vec<u32> = serde_json::from_value(req.params["ids"].clone()).unwrap_or_default();
                let held = |id: &u32| library.iter().find(|t| t.id == *id);
                let (found, missing): (Vec<u32>, Vec<u32>) = ids.into_iter().partition(|id| held(id).is_some());
                let found: Vec<&Track> = found.iter().filter_map(held).collect();
                to.respond(&req.id, Ok(json!({ "tracks": found, "missing": missing })));
                continue;
            }
            let fetch: Fetch = serde_json::from_value(req.params).unwrap();
            let asked = {
                let mut fetched = fetches.lock().unwrap();
                fetched.push(fetch.url.clone());
                fetched.iter().filter(|u| **u == fetch.url).count() - 1
            };
            let _ = tell.send(());
            let isrc = fetch.url.strip_prefix(LOOKUP).and_then(|rest| rest.split_once('?')).map_or("", |(i, _)| i);
            let answer = match route(isrc, asked) {
                Reply::Answer(status, wait, body) => {
                    let headers: IndexMap<String, String> =
                        wait.map(|w| ("retry-after".to_string(), w.to_string())).into_iter().collect();
                    Ok(serde_json::to_value(Fetched { status, headers, body: Some(body), blob: None }).unwrap())
                }
                Reply::Fails(e) => Err(e),
                Reply::Held => continue,
            };
            to.respond(&req.id, answer);
        }
    });
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    let hello = host.request(method::HELLO, &hello).unwrap();
    Session { host, served, answering, reads, fetched, heard, hello, tracks }
}

impl Session {
    /// `track/analyze` with `ids` for track `id`, as the host sends it, in `context`.
    fn send(&self, id: u32, context: Value) -> loclib_plugin::Pending {
        let audio = "/Volumes/USB/Contents/track.mp3";
        let track = json!({ "id": id, "file_path": "/Contents/track.mp3", "audio_path": audio });
        let req = json!({ "analyzer": "ids", "track": track, "context": context });
        self.host.send_request(method::ANALYZE_TRACK, &req).unwrap()
    }

    /// Its fields for track `id` in `context` but for the ISRC and credits they're for, which are
    /// checked to be the track's: the ISRC as loclib-model spells it (or none), and exactly the
    /// names loclib-model's `artist_credits` gives, however the lookup went.
    fn analyze_in(&self, id: u32, context: Value) -> Result<Analysis, RpcError> {
        let mut a: Analysis = self.send(id, context).wait()?;
        let t = self.tracks.iter().find(|t| t.id == id).expect("a track of the library");
        let fields: Vec<&str> = a.fields.keys().map(String::as_str).collect();
        assert_eq!(fields, ["recording", "release", "artists", "isrc", "credits"]);
        let isrc = loclib_model::model::isrc(&t.isrc).map_or(FieldValue::Null, FieldValue::String);
        let credits = FieldValue::Strings(loclib_model::model::artist_credits(&t.artist));
        let given = (a.fields.shift_remove("isrc").unwrap(), a.fields.shift_remove("credits").unwrap());
        assert_eq!(given, (isrc, credits), "track #{id}");
        Ok(a)
    }

    fn analyze(&self, id: u32) -> Result<Analysis, RpcError> {
        self.analyze_in(id, json!({ "drive": "USB" }))
    }

    fn fetched(&self) -> Vec<String> {
        self.fetched.lock().unwrap().clone()
    }

    /// Closes the plugin's input: it stops. The reads it called back for.
    fn stop(self) -> Vec<Read> {
        self.host.close();
        self.served.join().unwrap();
        self.answering.join().unwrap();
        Arc::try_unwrap(self.reads).unwrap().into_inner().unwrap()
    }
}

/// Track `id`, `Artist - Title`, `secs` long, from `album`, with the ISRC tag `isrc`.
fn track(id: u32, name: &str, album: &str, secs: u32, isrc: &str) -> Track {
    let (artist, title) = name.split_once(" - ").unwrap_or(("", name));
    Track {
        id,
        artist: artist.into(),
        title: title.into(),
        album: album.into(),
        duration_secs: secs,
        isrc: isrc.into(),
        ..Track::default()
    }
}

/// The fields the analyzer answers for these IDs.
fn answer(recording: Option<&str>, release: Option<&str>, artists: &[&str]) -> Analysis {
    let id = |id: Option<&str>| id.map_or(FieldValue::Null, |id| FieldValue::String(id.into()));
    let artists = match artists.is_empty() {
        true => FieldValue::Null,
        false => FieldValue::Strings(artists.iter().map(|a| a.to_string()).collect()),
    };
    let fields = [("recording", id(recording)), ("release", id(release)), ("artists", artists)];
    Analysis { fields: fields.into_iter().map(|(n, v)| (n.into(), v)).collect() }
}

fn nothing() -> Analysis {
    answer(None, None, &[])
}

fn uuid(n: u128) -> String {
    Uuid::from_u128(n).to_string()
}

fn kind(e: &RpcError) -> (&str, &str) {
    (e.kind().unwrap_or_default(), e.message.as_str())
}

type Answers = Vec<Result<Analysis, RpcError>>;

/// Each of `tracks` analysed in turn in one session, MusicBrainz answering as `route` does: the
/// answers, and the URLs fetched.
fn analyze_all(tracks: Vec<Track>, route: impl Fn(&str, usize) -> Reply + Send + 'static) -> (Answers, Vec<String>) {
    let ids: Vec<u32> = tracks.iter().map(|t| t.id).collect();
    let session = start(tracks, route);
    let answers = ids.iter().map(|&id| session.analyze(id)).collect();
    let fetched = session.fetched();
    session.stop();
    (answers, fetched)
}

/// MusicBrainz answering `xml` about any ISRC.
fn answering(xml: String) -> impl Fn(&str, usize) -> Reply + Send + 'static {
    move |_, _| ok(xml.clone())
}

/// It reads tracks and asks MusicBrainz, a request a second; its one analyzer answers the roles
/// `sets publish` reads, the recording standing for the track's identity too.
#[test]
fn the_manifest_is_a_built_in_analyzer_of_musicbrainz_ids() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    let capabilities: Vec<String> = manifest.capabilities.iter().map(ToString::to_string).collect();
    assert_eq!(capabilities, ["library:read", "net:musicbrainz.org"]);
    assert_eq!(manifest.net, [NetLimit { host: "musicbrainz.org".into(), per_second: 1.0 }]);
    let [analyzer] = &manifest.contributes.analyzers[..] else { panic!("{manifest:?}") };
    assert_eq!((manifest.id.as_str(), analyzer.name.as_str()), ("musicbrainz", "ids"));
    // Version 1 kept IDs for an only recording of another length, and no ISRC or credits.
    assert_eq!(analyzer.version, "2");
    let fields: Vec<&str> = analyzer.fields.keys().map(String::as_str).collect();
    assert_eq!(fields, ["recording", "release", "artists", "isrc", "credits"]);
    let roles: Vec<(Role, &str)> = analyzer.roles.iter().map(|(r, f)| (*r, f.as_str())).collect();
    let expected = [
        (Role::Recording, "recording"),
        (Role::Release, "release"),
        (Role::Artists, "artists"),
        (Role::Isrc, "isrc"),
        (Role::Credits, "credits"),
        (Role::Identity, "recording"),
    ];
    assert_eq!(roles, expected);
    let settings: Vec<&str> = manifest.contributes.settings.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(settings, ["contact"]);

    // Served as a real one is: it says it's this manifest, analysing and sending fetches.
    let session = start(Vec::new(), as_it_is);
    let has = |f: &&str| session.hello.features.iter().any(|had| had == f);
    assert!(["analyzers", "net"].iter().all(has), "{:?}", session.hello.features);
    let described: Manifest = session.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    session.stop();
}

/// A track whose ISRC is one recording gets it when it's as long (or, without a length, titled
/// alike), the release named as its album (the first of the editions released first) and its
/// artist, reading the track from the drive and looking its ISRC up once, however the tag writes
/// it. A track of another length (an extended mix with the original's ISRC), or of another title
/// and no length, gets nothing.
#[test]
fn one_recording_is_the_tracks_if_it_fits() {
    let tracks = vec![
        track(1, "Young Fathers - In My View", "Cocoa Sugar", 195, "gb-cfb-17-00586"),
        track(2, "Young Fathers - In My View", "in my  VIEW", 0, IN_MY_VIEW),
        track(3, "Young Fathers - In My View", "Cocoa Sugar (Deluxe)", 195, IN_MY_VIEW),
        // As long, though titled otherwise; the artists still go by name.
        track(4, "Someone Else - In My View (Radio Edit)", "", 196, IN_MY_VIEW),
        track(5, "Young Fathers - In My View (Extended Mix)", "Cocoa Sugar", 302, IN_MY_VIEW),
        track(6, "Young Fathers - In My View", "Cocoa Sugar", 198, IN_MY_VIEW),
        track(7, "Someone Else - Something Else", "", 0, IN_MY_VIEW),
    ];
    let session = start(tracks, as_it_is);
    let answers: Vec<Analysis> = (1..=7).map(|id| session.analyze(id).unwrap()).collect();
    let single = "9fcb6929-c8e2-4ff0-b47b-d0d7f9a4fdf5";
    assert_eq!(
        answers,
        [
            answer(Some(RECORDING), Some(COCOA_SUGAR), &[YOUNG_FATHERS]),
            answer(Some(RECORDING), Some(single), &[YOUNG_FATHERS]),
            answer(Some(RECORDING), None, &[YOUNG_FATHERS]),
            answer(Some(RECORDING), None, &[]),
            nothing(),
            nothing(),
            nothing(),
        ]
    );
    let url = format!("{LOOKUP}{IN_MY_VIEW}?inc=artist-credits+releases");
    assert_eq!(session.fetched(), vec![url; 7]);
    let reads = session.stop();
    let expected: Vec<Read> =
        (1..=7).map(|id| ("tracks.get".to_string(), json!({ "ids": [id], "drive": "USB" }))).collect();
    assert_eq!(reads, expected);
}

/// Of several recordings, the track is the only one with its title and length (to two seconds);
/// with none, or more than one, it gets no IDs at all.
#[test]
fn several_recordings_go_by_title_and_length() {
    let other = "3f4c2a7e-91d5-4c1b-a1e2-6b0d4c9e8f10";
    let instrumental = "a7d93e02-5f6b-4c8e-b1a4-9e2c0d7f3b56";
    let cases = [
        ("Young Fathers - In My View", 197, answer(Some(RECORDING), None, &[YOUNG_FATHERS])),
        ("Young Fathers - In My View", 228, answer(Some(other), None, &[YOUNG_FATHERS])),
        ("Young Fathers - In My View (Instrumental)", 195, answer(Some(instrumental), None, &[YOUNG_FATHERS])),
        ("Young Fathers - In My View", 210, nothing()),
        // No length to tell them apart.
        ("Young Fathers - In My View", 0, nothing()),
        ("Young Fathers - Toy", 195, nothing()),
    ];
    let tracks = (1..).zip(&cases).map(|(id, (name, secs, _))| track(id, name, "", *secs, SEVERAL)).collect();
    let (answers, fetched) = analyze_all(tracks, as_it_is);
    let expected: Answers = cases.into_iter().map(|(_, _, a)| Ok(a)).collect();
    assert_eq!(answers, expected);
    assert_eq!(fetched.len(), 6);

    // Two recordings with its title and length: it could be either.
    let twins = xml(&[
        Rec { id: 1, title: "In My View".into(), length: Some(195_000), ..Rec::default() },
        Rec { id: 2, title: "in my view".into(), length: Some(196_400), ..Rec::default() },
    ]);
    let (answers, _) = analyze_all(vec![track(1, "Young Fathers - In My View", "", 195, IN_MY_VIEW)], answering(twins));
    assert_eq!(answers, [Ok(nothing())]);
}

/// The release is the recording's named as the track's album: the earliest dated when several
/// are (a year before a day in it), the undated last, the first listed of those released the same
/// day; none for a track without an album.
#[test]
fn the_release_is_the_earliest_named_as_the_album() {
    let release = |id: u128, title: &str, date: Option<&str>| (id, title.to_string(), date.map(String::from));
    let rec = Rec {
        id: 1,
        title: "Glue".into(),
        releases: vec![
            release(10, "Bicep", None),
            release(11, "Bicep", Some("2019-01-01")),
            release(12, "BICEP", Some("2018")),
            release(13, "Bicep", Some("2018-03-09")),
            release(14, "Bicep (Deluxe)", Some("2017")),
            release(15, "Bicep", Some("2018")),
            // Untitled, as no album is.
            release(18, "...", Some("2000")),
        ],
        ..Rec::default()
    };
    let only_undated =
        Rec { releases: vec![release(16, "Isles", Some("")), release(17, "Isles", None)], ..rec.clone() };
    let releases = |rec: Rec, albums: &[&str]| -> Vec<FieldValue> {
        let tracks = (1..).zip(albums).map(|(id, album)| track(id, "Bicep - Glue", album, 0, IN_MY_VIEW)).collect();
        let (answers, _) = analyze_all(tracks, answering(xml(&[rec])));
        answers.into_iter().map(|a| a.unwrap().fields["release"].clone()).collect()
    };
    let id = |n: u128| FieldValue::String(uuid(n));
    assert_eq!(releases(rec, &["bicep", "  ", "Isles"]), [id(12), FieldValue::Null, FieldValue::Null]);
    assert_eq!(releases(only_undated, &["Isles"]), [id(16)]);
}

/// Each name the track's artist credits is the one recording artist named so, or credited under
/// that name; unless every name is a different artist, there are none. `&` and `b2b` don't split
/// a credit, so such a name is no one artist.
#[test]
fn artists_pair_with_the_names_the_track_credits() {
    let cases: [(&str, &[&str]); 8] = [
        ("Chris Lake feat. Alexis Roberts", &[CHRIS_LAKE, ALEXIS_ROBERTS]),
        ("Alexis Roberts, Chris Lake", &[ALEXIS_ROBERTS, CHRIS_LAKE]),
        ("chris lake", &[CHRIS_LAKE]),
        ("Chris Lake ft. Alexis Roberts; Chris Lake", &[CHRIS_LAKE, ALEXIS_ROBERTS]),
        ("Chris Lake & Alexis Roberts", &[]),
        ("Chris Lake b2b Alexis Roberts", &[]),
        ("Chris Lake feat. Amber Mark", &[]),
        ("", &[]),
    ];
    let named = |(id, (artist, _)): (u32, &(&str, &[&str]))| {
        track(id, &format!("{artist} - Turn Off the Lights"), "", 0, LIGHTS)
    };
    let (answers, _) = analyze_all((1..).zip(&cases).map(named).collect(), as_it_is);
    let lights = "48133fac-424c-4599-8647-6501386b6d6e";
    let expected: Answers = cases.iter().map(|(_, ids)| Ok(answer(Some(lights), None, ids))).collect();
    assert_eq!(answers, expected);

    // Credited under another name; two artists of one name, which can't be told apart; and two
    // names for one artist, which can't each have it.
    let credit = |id: u128, credited: Option<&str>, name: &str| (id, credited.map(String::from), name.to_string());
    let body = xml(&[Rec {
        id: 1,
        title: "Song".into(),
        credits: vec![
            credit(1, Some("DJ Koze"), "Stefan Kozalla"),
            credit(2, None, "Floorplan"),
            credit(3, None, "floorplan"),
        ],
        releases: vec![(5, "Song".into(), None)],
        ..Rec::default()
    }]);
    let tracks = vec![
        track(1, "DJ KOZE - Song", "Song", 0, LIGHTS),
        track(2, "DJ Koze feat. Floorplan - Song", "", 0, LIGHTS),
        track(3, "DJ Koze, Stefan Kozalla - Song", "", 0, LIGHTS),
    ];
    let (answers, _) = analyze_all(tracks, answering(body));
    let (one, five) = (uuid(1), uuid(5));
    let nameless = Ok(answer(Some(&one), None, &[]));
    assert_eq!(answers, [Ok(answer(Some(&one), Some(&five), &[&one])), nameless.clone(), nameless]);
}

/// A track without an ISRC (or a tag that isn't one) gets no IDs, and MusicBrainz isn't asked.
#[test]
fn without_an_isrc_nothing_is_looked_up() {
    let tags = ["", "GBCFB17005", "none", "GBCFB17005 86", "GBCFB17005866", "1BCFB1700586", "GBCFB17005X6"];
    let tracks = (1..).zip(tags).map(|(id, isrc)| track(id, "Young Fathers - In My View", "Cocoa Sugar", 195, isrc));
    let (answers, fetched) = analyze_all(tracks.collect(), as_it_is);
    assert_eq!(answers, vec![Ok(nothing()); 7]);
    assert_eq!(fetched, Vec::<String>::new());
}

/// An ISRC MusicBrainz doesn't know (`404`) is worked out as nothing found: the fields are null,
/// kept so it isn't asked again.
#[test]
fn an_isrc_musicbrainz_doesnt_know_has_no_ids() {
    let (answers, fetched) = analyze_all(vec![track(1, "A - B", "C", 100, "USRC17607839")], |_, _| status(404));
    assert_eq!(answers, [Ok(nothing())]);
    assert_eq!(fetched.len(), 1);
}

/// A busy MusicBrainz (`503`) is asked again once its `Retry-After` has passed (seconds, here
/// [`lookup::SECOND`]s, or an HTTP-date, at once when it's past).
#[test]
fn a_busy_musicbrainz_is_asked_again_after_its_wait() {
    let tracks = || vec![track(1, "Young Fathers - In My View", "Cocoa Sugar", 195, IN_MY_VIEW)];
    for (wait, waited) in [("25", lookup::SECOND * 25), ("Sun, 06 Nov 1994 08:49:37 GMT", Duration::ZERO)] {
        let started = Instant::now();
        let (answers, fetched) = analyze_all(tracks(), move |isrc, asked| match asked {
            0 => busy(wait),
            _ => as_it_is(isrc, asked),
        });
        assert!(started.elapsed() >= waited, "{:?}", started.elapsed());
        assert_eq!(answers, [Ok(answer(Some(RECORDING), Some(COCOA_SUGAR), &[YOUNG_FATHERS]))]);
        assert_eq!(fetched.len(), 2);
    }
}

/// `Retry-After` is a number of seconds (one too big to count longer than any wait) or an
/// HTTP-date, counted from now and rounded up; one past, missing or unreadable is no wait.
#[test]
fn retry_after_says_seconds_or_a_date() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(784_111_770_500);
    let said = |name: &str, value: &str| {
        let headers: IndexMap<String, String> = [(name.to_string(), value.to_string())].into_iter().collect();
        lookup::retry_after(&Fetched { status: 503, headers, body: None, blob: None }, now)
    };
    assert_eq!(said("retry-after", " 12 "), 12);
    assert_eq!(said("Retry-After", "0"), 0);
    assert_eq!(said("retry-after", "99999999999999999999"), u32::MAX);
    // 784111777 is Sun, 06 Nov 1994 08:49:37 GMT: 6.5 seconds after `now`.
    assert_eq!(said("retry-after", "Sun, 06 Nov 1994 08:49:37 GMT"), 7);
    assert_eq!(said("retry-after", "Sun, 06 Nov 1994 08:49:30 GMT"), 0, "past");
    assert_eq!(said("retry-after", "Fri, 31 Dec 9999 23:59:59 GMT"), u32::MAX);
    assert_eq!((said("retry-after", "soon"), said("retry-after", "-5"), said("x-wait", "9")), (0, 0, 0));
}

/// A wait stops as soon as it's cancelled (here, before it starts), and otherwise lasts as long
/// as it's asked to.
#[test]
fn a_wait_ends_when_its_cancelled() {
    let token = CancelToken::new();
    token.cancel();
    let started = Instant::now();
    let cancelled = lookup::pause(&Ctx::new(&|_: &Event| {}, token), Duration::from_secs(30));
    assert_eq!((cancelled, started.elapsed() < Duration::from_secs(5)), (Err(RpcError::cancelled()), true));
    let started = Instant::now();
    assert_eq!(lookup::pause(&Ctx::new(&|_: &Event| {}, CancelToken::new()), lookup::SECOND * 5), Ok(()));
    assert!(started.elapsed() >= lookup::SECOND * 5, "{:?}", started.elapsed());
}

/// MusicBrainz busy every time it's asked (a few times), or asking for too long a wait, fails the
/// track, as does any other refusal, an answer that isn't a lookup's, or the host not reaching it:
/// none of that is kept as nothing found.
#[test]
fn musicbrainz_turning_it_away_fails_the_track() {
    let run = |route: fn(&str, usize) -> Reply| {
        let (mut answers, fetched) = analyze_all(vec![track(1, "A - In My View", "", 0, IN_MY_VIEW)], route);
        (answers.remove(0).unwrap_err(), fetched.len())
    };
    let busy_again = ("external", "MusicBrainz answered 503 about GBCFB1700586");
    let (e, asked) = run(|_, _| busy("0"));
    assert_eq!((kind(&e), asked), (busy_again, lookup::TRIES));
    let (e, asked) = run(|_, _| busy("31"));
    assert_eq!((kind(&e), asked), (busy_again, 1), "not worth waiting for");
    let (e, asked) = run(|_, _| busy("99999999999999999999"));
    assert_eq!((kind(&e), asked), (busy_again, 1), "longer than any wait");
    let (e, asked) = run(|_, _| status(500));
    assert_eq!((kind(&e), asked), (("external", "MusicBrainz answered 500 about GBCFB1700586"), 1));
    // Every ISRC asked about is one: MusicBrainz refusing it isn't nothing found.
    let (e, asked) = run(|_, _| status(400));
    assert_eq!((kind(&e), asked), (("external", "MusicBrainz answered 400 about GBCFB1700586"), 1));
    for garbled in [|_: &str, _| ok(r#"{"recordings": []}"#), |_: &str, _| ok(IN_MY_VIEW_XML.replace(RECORDING, "x"))] {
        let (e, _) = run(garbled);
        assert_eq!(e.kind(), Some("external"));
        assert!(e.message.starts_with("MusicBrainz's answer about GBCFB1700586 isn't readable: "), "{e}");
    }
    let (e, asked) = run(|_, _| Reply::Fails(RpcError::failed("too_big", "over 8 MiB")));
    assert_eq!((kind(&e), asked), (("too_big", "couldn't reach MusicBrainz: over 8 MiB"), 1));
}

/// Cancelled while it waits for MusicBrainz's answer, the hook stops at once, asking for no more.
/// (A wait for a `Retry-After` stops so too: [`a_wait_ends_when_its_cancelled`].)
#[test]
fn cancelling_while_it_waits_stops_it() {
    let session = start(vec![track(1, "A - B", "", 0, IN_MY_VIEW)], |_, _| Reply::Held);
    let pending = session.send(1, json!({ "drive": "USB" }));
    session.heard.recv().unwrap();
    let started = Instant::now();
    pending.cancel();
    let answer = pending.wait_timeout(Duration::from_secs(10)).expect("it stops");
    assert_eq!(answer.map(|_: Analysis| ()).unwrap_err(), RpcError::cancelled());
    assert_eq!(session.fetched().len(), 1);
    // Its hook has stopped too: the plugin stops once its input closes.
    session.stop();
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
}

/// It reads its track from the drive the host names: without one, or a track the library hasn't
/// got, it fails, asking MusicBrainz nothing.
#[test]
fn it_needs_the_drive_and_the_track() {
    let session = start(vec![track(1, "A - B", "", 0, IN_MY_VIEW)], as_it_is);
    let e = session.send(1, json!({})).wait::<Analysis>().unwrap_err();
    assert_eq!(kind(&e), ("invalid_params", "invalid params: no drive for the track"));
    let e = session.analyze(2).unwrap_err();
    assert_eq!(kind(&e), ("not_found", "the library has no track #2"));
    assert_eq!(session.fetched().len(), 0);
    assert_eq!(session.stop().len(), 1, "one read, of the track it hasn't got");
}

// Properties: drawn answers, the IDs foreseen from what was drawn.

/// A recording in an ISRC lookup's answer, as the tests write one.
#[derive(Debug, Clone, Default)]
struct Rec {
    id: u128,
    title: String,
    /// In milliseconds.
    length: Option<u64>,
    /// Each artist credited: its id, the name it's credited under (if not its own), its name.
    credits: Vec<(u128, Option<String>, String)>,
    /// Each release it's on: its id, title and date.
    releases: Vec<(u128, String, Option<String>)>,
}

/// `s` as XML text.
fn escaped(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// An ISRC lookup's answer naming `recordings`, in MusicBrainz's shape.
fn xml(recordings: &[Rec]) -> String {
    let mut out = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><metadata xmlns="http://musicbrainz.org/ns/mmd-2.0#"><isrc id="{IN_MY_VIEW}"><recording-list count="{}">"#,
        recordings.len()
    );
    for r in recordings {
        out += &format!(r#"<recording id="{}"><title>{}</title>"#, uuid(r.id), escaped(&r.title));
        if let Some(ms) = r.length {
            out += &format!("<length>{ms}</length>");
        }
        out += "<artist-credit>";
        for (id, credited, name) in &r.credits {
            out += "<name-credit>";
            if let Some(credited) = credited {
                out += &format!("<name>{}</name>", escaped(credited));
            }
            out += &format!(r#"<artist id="{}"><name>{}</name></artist></name-credit>"#, uuid(*id), escaped(name));
        }
        out += &format!(r#"</artist-credit><release-list count="{}">"#, r.releases.len());
        for (id, title, date) in &r.releases {
            out += &format!(r#"<release id="{}"><title>{}</title>"#, uuid(*id), escaped(title));
            if let Some(date) = date {
                out += &format!("<date>{date}</date>");
            }
            out += "</release>";
        }
        out += "</release-list></recording>";
    }
    out + "</recording-list></isrc></metadata>"
}

const TITLES: [&str; 4] = ["In My View", "Turn off the Lights", "Glue", "Never Grow Old"];

/// `title` written another way MusicBrainz might: case, spacing, accents, hyphens for spaces.
#[hegel::test_helper]
fn respelled(tc: &TestCase, title: &str) -> String {
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => title.to_uppercase(),
        1 => format!("  {}  ", title.replace(' ', "   ")),
        2 => title.replace('o', "ö"),
        3 => title.replace(' ', "-"),
        _ => title.to_string(),
    }
}

/// A recording titled `title`, `ms` long, on one release titled `Album`.
fn recording_of(n: u128, title: String, ms: Option<u64>) -> Rec {
    Rec { id: n, title, length: ms, releases: vec![(n, "Album".into(), None)], ..Rec::default() }
}

/// Recordings that aren't the track, among others: another title (but its length), a length more
/// than 2 seconds off, or none (but its title). Each with whether it would be the track alone:
/// all but the one of another length.
#[hegel::test_helper]
fn others(tc: &TestCase, title: &str, secs: u64) -> Vec<(Rec, bool)> {
    let count = tc.draw(gs::integers::<usize>().max_value(4));
    (0..count)
        .map(|i| {
            let kind = tc.draw(gs::integers::<u8>().max_value(2));
            let (title, length) = match kind {
                0 => {
                    let rest: Vec<&str> = TITLES.iter().filter(|t| **t != title).copied().collect();
                    (tc.draw(gs::sampled_from(rest)).to_string(), Some(secs * 1000))
                }
                1 => {
                    let off = tc.draw(gs::integers::<u64>().min_value(3).max_value(120));
                    let longer = tc.draw(gs::booleans()) || off > secs;
                    (title.to_string(), Some(if longer { secs + off } else { secs - off } * 1000))
                }
                _ => (title.to_string(), None),
            };
            (recording_of(100 + i as u128, title, length), kind != 1)
        })
        .collect()
}

/// `items` in a drawn order.
#[hegel::test_helper]
fn shuffled<T>(tc: &TestCase, mut items: Vec<T>) -> Vec<T> {
    let mut out = Vec::new();
    while !items.is_empty() {
        let i = tc.draw(gs::integers::<usize>().max_value(items.len() - 1));
        out.push(items.remove(i));
    }
    out
}

/// Several recordings an ISRC might be that a track's title and length don't tell apart give it
/// no IDs at all; when only one has its title and length, it's that one, however MusicBrainz
/// spells the title and whatever else the ISRC is. A recording alone is the track only when it's
/// as long, or without a length titled alike. A tag that isn't an ISRC asks nothing.
#[hegel::test(test_cases = 60)]
fn ambiguity_never_yields_ids(tc: TestCase) {
    let title = tc.draw(gs::sampled_from(TITLES.to_vec()));
    let secs = tc.draw(gs::integers::<u64>().min_value(60).max_value(600));
    let near = |tc: &TestCase| secs * 1000 + tc.draw(gs::integers::<u64>().max_value(2000)) - 1000;
    let mut fitting = vec![recording_of(1, respelled(&tc, title), Some(near(&tc)))];
    let ambiguous = tc.draw(gs::booleans());
    if ambiguous {
        fitting.push(recording_of(2, respelled(&tc, title), Some(near(&tc))));
    }
    let others = others(&tc, title, secs);
    // Alone, the track is the recording if it fits.
    let (all, the_one) = match others.first().filter(|_| tc.draw(gs::weighted_booleans(0.3))) {
        Some((alone, fits)) => (vec![alone.clone()], fits.then_some(alone.id)),
        None => {
            let unfit = others.into_iter().map(|(r, _)| r);
            (shuffled(&tc, fitting.into_iter().chain(unfit).collect()), (!ambiguous).then_some(1))
        }
    };
    let tags = vec![(IN_MY_VIEW, true), ("gb-cfb-17-00586", true), ("GBCFB17005", false)];
    let (tag, is_isrc) = tc.draw(gs::sampled_from(tags));
    let tracks = vec![track(1, &format!("Artist - {title}"), "Album", secs as u32, tag)];
    let (answers, fetched) = analyze_all(tracks, answering(xml(&all)));
    let got = answers.into_iter().next().unwrap().unwrap();
    match the_one.filter(|_| is_isrc) {
        Some(n) => assert_eq!(got, answer(Some(&uuid(n)), Some(&uuid(n)), &[]), "{all:?}"),
        None => assert_eq!(got, nothing(), "{all:?}"),
    }
    assert_eq!(fetched.len(), usize::from(is_isrc), "one lookup, with an ISRC");
}

const ARTISTS: [&str; 6] = ["Chris Lake", "Alexis Roberts", "Young Fathers", "Bicep", "Floorplan", "Amber Mark"];

/// However a track's credit names and orders its artists, and MusicBrainz orders and spells its
/// own, an artist ID only ever goes with the name of the artist it's for, all of them or none:
/// all when every name is credited.
#[hegel::test(test_cases = 60)]
fn artist_ids_line_up_with_names(tc: TestCase) {
    let id = |name: &str| ARTISTS.iter().position(|a| *a == name).unwrap() as u128 + 1;
    let some_artists = || gs::vecs(gs::sampled_from(ARTISTS.to_vec())).min_size(1).max_size(4).unique(true);
    let credited: Vec<&str> = tc.draw(some_artists());
    let credits = credited
        .iter()
        .map(|&name| match tc.draw(gs::booleans()) {
            // Credited under a name of its own, or as itself.
            true => (id(name), Some(respelled(&tc, name)), format!("{name} (the artist)")),
            false => (id(name), None, respelled(&tc, name)),
        })
        .collect();
    let body = xml(&[Rec { id: 1, title: "Title".into(), credits, ..Rec::default() }]);
    let named: Vec<&str> = tc.draw(some_artists());
    let artist = named.join(tc.draw(gs::sampled_from(vec![", ", "; ", " feat. ", " ft. ", " & ", " b2b "])));
    let (answers, _) = analyze_all(vec![track(1, &format!("{artist} - Title"), "", 0, IN_MY_VIEW)], answering(body));
    let got = match &answers[0].as_ref().unwrap().fields["artists"] {
        FieldValue::Strings(ids) => ids.clone(),
        FieldValue::Null => Vec::new(),
        other => panic!("{other:?}"),
    };
    let names = loclib_model::model::artist_credits(&artist);
    let same = |a: &str, b: &str| loclib_tracklist::fold(a) == loclib_tracklist::fold(b);
    let foreseen: Option<Vec<String>> =
        names.iter().map(|n| credited.iter().find(|c| same(c, n)).map(|c| uuid(id(c)))).collect();
    assert_eq!(got, foreseen.unwrap_or_default(), "{names:?} {credited:?}");
}

/// What a lookup's last answer is, after however many `503`s.
#[derive(Debug, Clone, Copy)]
enum Last {
    Found,
    Unknown(u16),
    Refused(u16),
    Garbage,
    Unreached,
}

/// However often MusicBrainz is busy (asking to wait some seconds, or not at all) and however it
/// answers in the end, a track with an ISRC is looked up once, again only for each `503` (a few
/// times at most): the fixture's IDs when it's found (the release only for a track with an album),
/// nothing when MusicBrainz doesn't know it, and a failure for anything else. The track is read
/// once; one the library hasn't got, or asked about without a drive, is never looked up.
#[hegel::test(test_cases = 60)]
fn each_track_takes_one_lookup_and_its_retries(tc: TestCase) {
    let busy_for = tc.draw(gs::integers::<usize>().max_value(lookup::TRIES + 1));
    let wait = tc.draw(gs::sampled_from(vec!["0", "0", "1", "3"]));
    let last = tc.draw(gs::sampled_from(vec![
        Last::Found,
        Last::Unknown(404),
        Last::Refused(400),
        Last::Refused(500),
        Last::Refused(429),
        Last::Garbage,
        Last::Unreached,
    ]));
    let album = tc.draw(gs::sampled_from(vec!["Cocoa Sugar", "cocoa  SUGAR", "", " "]));
    // Mostly the track, from its drive: the answers are what's drawn.
    let (id, drive) = match tc.draw(gs::integers::<u8>().max_value(9)) {
        8 => (2, true),
        9 => (1, false),
        _ => (1, true),
    };
    let route = move |isrc: &str, asked: usize| match (asked < busy_for, last) {
        (true, _) => busy(wait),
        (false, Last::Found) => as_it_is(isrc, asked),
        (false, Last::Unknown(code) | Last::Refused(code)) => status(code),
        (false, Last::Garbage) => ok("<html>maintenance</html>"),
        (false, Last::Unreached) => Reply::Fails(RpcError::failed("external", "no answer")),
    };
    let session = start(vec![track(1, "Young Fathers - In My View", album, 195, IN_MY_VIEW)], route);
    let context = if drive { json!({ "drive": "USB" }) } else { json!({}) };
    let started = Instant::now();
    let got = session.analyze_in(id, context);
    let fetched = session.fetched().len();
    // The seconds asked waited before each lookup sent again.
    let waited = lookup::SECOND * wait.parse::<u32>().unwrap() * fetched.saturating_sub(1) as u32;
    assert!(started.elapsed() >= waited, "{:?} for {fetched} lookups", started.elapsed());
    assert_eq!(session.stop().len(), usize::from(drive), "the track is read once, from its drive");
    match (id, drive) {
        (1, true) => {}
        (_, true) => return assert_eq!((got.unwrap_err().kind(), fetched), (Some("not_found"), 0)),
        _ => return assert_eq!((got.unwrap_err().kind(), fetched), (Some("invalid_params"), 0)),
    }
    if busy_for >= lookup::TRIES {
        assert_eq!(fetched, lookup::TRIES);
        assert_eq!(got.unwrap_err().message, "MusicBrainz answered 503 about GBCFB1700586");
        return;
    }
    assert_eq!(fetched, busy_for + 1);
    let release = (!album.trim().is_empty()).then_some(COCOA_SUGAR);
    match (last, got) {
        (Last::Found, Ok(a)) => assert_eq!(a, answer(Some(RECORDING), release, &[YOUNG_FATHERS])),
        (Last::Unknown(_), Ok(a)) => assert_eq!(a, nothing()),
        (Last::Refused(_) | Last::Garbage | Last::Unreached, Err(e)) => assert_eq!(e.kind(), Some("external"), "{e}"),
        (last, got) => panic!("{last:?}: {got:?}"),
    }
}

/// Any `Retry-After` comes to its seconds: a number as it is (one too big to count longer than
/// any wait), an HTTP-date as the seconds until then, rounded up (none once it's past), and
/// anything else as none.
#[hegel::test(test_cases = 100)]
fn any_retry_after_is_its_seconds(tc: TestCase) {
    let base = tc.draw(gs::integers::<u64>().min_value(1_000_000_000).max_value(4_000_000_000));
    let now = SystemTime::UNIX_EPOCH
        + Duration::from_secs(base)
        + Duration::from_millis(tc.draw(gs::integers::<u64>().max_value(999)));
    let n = tc.draw(gs::integers::<u64>().max_value(1_000_000_000));
    let date = |secs: u64| httpdate::fmt_http_date(SystemTime::UNIX_EPOCH + Duration::from_secs(secs));
    let big = tc.draw(gs::integers::<u64>().min_value(u64::from(u32::MAX) + 1));
    let (said, seconds) = tc.draw(gs::sampled_from(vec![
        (n.to_string(), u32::try_from(n).unwrap()),
        (big.to_string(), u32::MAX),
        (format!("{big}0"), u32::MAX),
        (date(base + n), u32::try_from(n).unwrap()),
        (date(base - 1 - n.min(base - 1)), 0),
        ("in a minute".to_string(), 0),
    ]));
    let headers: IndexMap<String, String> = [("retry-after".to_string(), said.clone())].into_iter().collect();
    assert_eq!(lookup::retry_after(&Fetched { status: 503, headers, body: None, blob: None }, now), seconds, "{said}");
}
