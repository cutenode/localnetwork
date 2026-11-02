mod soundcloud;

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use chrono::TimeDelta;
use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::{
    AccountType, Capability, DestinationKind, Fetched, Manifest, PublishedSet, SettingType, TokenPlace, TracklistEntry,
    method,
};
use loclib_progress::{Event, Phase};
use serde_json::{Value, json};

use self::soundcloud::{PAGE, Reply, SIGNED_OUT, SoundCloud, TOKEN, refusal, start};
use super::track::{MAX_DESCRIPTION, MAX_TITLE};
use super::upload::{AUTHORIZATION, LONGEST_WAIT, NEWEST, TRACKS, TRIES, retry_after};
use super::*;

/// The account type the plugin asks for, as the SoundCloud account plugin provides it.
fn account_type() -> AccountType {
    let manifest = Manifest::parse(include_str!("../../accounts/soundcloud-account/locallibrary-plugin.toml"));
    let [account_type] = &manifest.unwrap().contributes.account_types[..] else { panic!("one account type") };
    account_type.clone()
}

fn set() -> PublishedSet {
    PublishedSet {
        id: "s1".into(),
        date: "2026-10-03".into(),
        title: Some("Closing set".into()),
        venue: Some("Warehouse".into()),
        ..PublishedSet::default()
    }
}

fn entry(artist: Option<&str>, title: Option<&str>, start: Option<u32>) -> TracklistEntry {
    TracklistEntry { artist: artist.map(Into::into), title: title.map(Into::into), start_seconds: start }
}

/// A file `name` of `size` bytes in `dir`, none of its stretches alike.
fn file(dir: &Path, name: &str, size: u64) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, (0..size).map(|i| (i * 31 % 251) as u8).collect::<Vec<u8>>()).unwrap();
    path
}

/// An error's kind and message.
fn kind(e: &RpcError) -> (&str, &str) {
    (e.kind().unwrap_or_default(), e.message.as_str())
}

/// The bytes each upload phase started with, a phase for each time the form was sent.
fn phases(events: &[Event]) -> Vec<u64> {
    let mut started = Vec::new();
    let mut open = false;
    for event in events {
        match event {
            Event::PhaseStart { phase: Phase::Upload, items: 1, bytes } if !open => {
                started.push(*bytes);
                open = true;
            }
            Event::PhaseEnd { phase: Phase::Upload } if open => open = false,
            other => panic!("{other:?} in {events:?}"),
        }
    }
    assert!(!open, "every phase ends: {events:?}");
    started
}

/// It reaches SoundCloud's API alone, publishes sets at one destination, asks for a SoundCloud
/// account to upload with, naming it where the SoundCloud account plugin puts its token (after
/// `OAuth`), and keeps who can listen, the genre and the artwork as settings.
#[test]
fn the_manifest_is_a_built_in_set_destination_on_soundcloud() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    assert_eq!(manifest.capabilities, [Capability::Net("api.soundcloud.com".into())]);
    assert!([TRACKS, NEWEST].iter().all(|url| url.starts_with("https://api.soundcloud.com/")));
    let [destination] = &manifest.contributes.destinations[..] else { panic!("{manifest:?}") };
    assert_eq!((manifest.id.as_str(), destination.name.as_str()), ("soundcloud", "track"));
    assert_eq!(destination.kind, DestinationKind::Set);
    let [need] = &manifest.accounts[..] else { panic!("{manifest:?}") };
    assert_eq!((need.name.as_str(), need.scopes.as_slice()), ("soundcloud", ["upload".to_string()].as_slice()));
    assert!(need.optional.is_empty(), "{need:?}");
    let account_type = account_type();
    assert_eq!(format!("{{account:{}}}", account_type.name), AUTHORIZATION);
    assert_eq!(account_type.token, TokenPlace::Scheme("OAuth".into()));
    let settings: Vec<(&str, SettingType, Option<&Value>)> =
        manifest.contributes.settings.iter().map(|s| (s.key.as_str(), s.kind, s.default.as_ref())).collect();
    assert_eq!(
        settings,
        [
            ("sharing", SettingType::Enum, Some(&json!("private"))),
            ("genre", SettingType::String, None),
            ("picture", SettingType::File, None)
        ]
    );
    assert_eq!(manifest.contributes.settings[0].choices, ["private", "public"]);

    // Served as a real one is: it says it's this manifest, publishing sets through fetches with
    // an account.
    let s = start(SoundCloud::new());
    let has = |f: &&str| s.hello.features.iter().any(|had| had == f);
    assert!(["destinations", "net", "blobs", "accounts"].iter().all(has), "{:?}", s.hello.features);
    let described: Manifest = s.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    s.stop();
}

/// The SoundCloud account plugin's `upload` scope covers every request the plugin makes, the
/// upload and reading the account's newest tracks, so it needs no `read`; and not the rest of
/// SoundCloud's API.
#[test]
fn soundclouds_upload_scope_covers_every_request() {
    let account_type = account_type();
    let scope = &account_type.scopes["upload"];
    let covers = |method: &str, url: &str| scope.covers(method, &url::Url::parse(url).unwrap());
    assert!(covers("POST", TRACKS));
    assert!(covers("GET", NEWEST));
    assert!(!covers("GET", "https://api.soundcloud.com/tracks/123"));
    assert!(!covers("DELETE", "https://api.soundcloud.com/tracks/123"));
}

/// The recording goes up whole in one form with the track's title, who can listen (private
/// unless the setting says), not downloadable, the tag and the tracklist, with the account's
/// token as `OAuth`; the answer is the track's page, saying SoundCloud is still transcoding it.
#[test]
fn a_set_goes_up_in_one_form_and_answers_its_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), "Closing Set.FLAC", 2500);
    let tracklist = [
        entry(Some("Young Fathers"), Some("In My View"), Some(0)),
        entry(None, None, Some(330)),
        entry(Some("Chris Lake"), Some("Turn Off\nthe Lights"), Some(3723)),
        entry(Some("Someone"), Some("Unplaced"), None),
    ];
    let s = start(SoundCloud::new());
    let (answer, events) = s.publish(&set(), &path, &tracklist, json!({ "sharing": "private" }), false);
    let r = s.stop();
    assert_eq!(
        answer.unwrap(),
        Published {
            url: Some(PAGE.into()),
            name: Some("SoundCloud".into()),
            warnings: vec![TRANSCODING.into()],
            ..Published::default()
        }
    );
    let [form] = &r.forms[..] else { panic!("{r:?}") };
    assert_eq!(form.authorization, format!("OAuth {TOKEN}"));
    let description =
        "00:00 Young Fathers - In My View\n05:30 ID - ID\n1:02:03 Chris Lake - Turn Off the Lights\nSomeone - Unplaced";
    let fields = [
        ("title", "Closing set"),
        ("sharing", "private"),
        ("downloadable", "false"),
        ("tag_list", "\"dj mix\""),
        ("description", description),
    ];
    assert_eq!(form.fields, fields.map(|(k, v)| (format!("track[{k}]"), v.to_string())));
    let audio = std::fs::read(&path).unwrap();
    assert_eq!(form.files, [("track[asset_data]".to_string(), "Closing Set.FLAC".to_string(), audio)]);
    assert_eq!(phases(&events), [2500]);
}

/// The artwork setting's picture goes as the track's artwork, copied into the scratch folder (the
/// host sends no other file), with the genre and public as the settings say; a set without a
/// title is named after its venue and date, and without a tracklist has no description.
#[test]
fn the_settings_say_the_artwork_genre_and_sharing() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), "set.mp3", 300);
    let picture = file(dir.path(), "Cover.JPG", 40);
    let settings = json!({ "sharing": "public", "genre": "Techno", "picture": picture });
    let untitled = PublishedSet { title: None, ..set() };
    let s = start(SoundCloud::new());
    let (answer, events) = s.publish(&untitled, &path, &[], settings, false);
    let scratch = s.scratch.path().to_path_buf();
    let r = s.stop();
    assert_eq!(answer.unwrap().url.as_deref(), Some(PAGE));
    let [form] = &r.forms[..] else { panic!("{r:?}") };
    let fields = [
        ("title", "Warehouse, 2026-10-03"),
        ("sharing", "public"),
        ("downloadable", "false"),
        ("tag_list", "\"dj mix\""),
        ("genre", "Techno"),
    ];
    assert_eq!(form.fields, fields.map(|(k, v)| (format!("track[{k}]"), v.to_string())));
    let names: Vec<(&str, &str)> = form.files.iter().map(|(n, f, _)| (n.as_str(), f.as_str())).collect();
    assert_eq!(names, [("track[asset_data]", "set.mp3"), ("track[artwork_data]", "artwork.jpg")]);
    assert_eq!(form.files[1].2, std::fs::read(&picture).unwrap());
    assert_eq!(form.paths[1], scratch.join("artwork.jpg"));
    assert_eq!(phases(&events), [340]);
}

/// A recording SoundCloud wouldn't take (another format, empty, over 4 GB, or gone), or artwork it
/// wouldn't (not a file, not absolute, another format, over 2 MB), is refused before anything goes,
/// saying why, in a dry run too.
#[test]
fn what_soundcloud_wouldnt_take_is_refused_first() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("long.wav");
    std::fs::File::create(&big).unwrap().set_len(MAX_BYTES + 1).unwrap();
    let takes = "SoundCloud takes WAV, FLAC, AIFF, MP3, AAC, Ogg, MP4, MP2, M4A, 3GP, 3G2, MJ2, WMA and AMR recordings";
    let recordings = [
        (file(dir.path(), "set.opus", 10), "invalid", format!("{takes}, which set.opus isn't")),
        (file(dir.path(), "set", 10), "invalid", format!("{takes}, which set isn't")),
        (file(dir.path(), "set.aif", 0), "invalid", "set.aif is empty".into()),
        (big, "invalid", "SoundCloud takes recordings of up to 4000.0 MB, and long.wav is 4000.0 MB".into()),
    ];
    let s = start(SoundCloud::new());
    for dry_run in [false, true] {
        for (path, expected_kind, expected) in &recordings {
            let e = s.publish(&set(), path, &[], json!({}), dry_run).0.unwrap_err();
            assert_eq!(kind(&e), (*expected_kind, expected.as_str()), "{}", path.display());
        }
    }
    let gone = dir.path().join("gone.mp3");
    let e = s.publish(&set(), &gone, &[], json!({}), false).0.unwrap_err();
    assert_eq!(e.kind(), Some("io"), "{e:?}");
    assert!(e.message.starts_with(&gone.display().to_string()), "{e:?}");

    let path = file(dir.path(), "set.wav", 10);
    let huge = dir.path().join("huge.png");
    std::fs::File::create(&huge).unwrap().set_len(MAX_PICTURE + 1).unwrap();
    let hint = "(locallibrary plugin settings soundcloud picture)";
    let pictures = [
        (
            dir.path().join("nothing.jpg"),
            format!("the picture setting, {}, isn't a file", dir.path().join("nothing.jpg").display()),
        ),
        (PathBuf::from("cover.jpg"), "the picture setting, cover.jpg, isn't a file".into()),
        (dir.path().to_path_buf(), format!("the picture setting, {}, isn't a file", dir.path().display())),
        (
            file(dir.path(), "cover.gif", 10),
            format!("SoundCloud takes JPEG and PNG artwork, which {} isn't", dir.path().join("cover.gif").display()),
        ),
        (huge.clone(), format!("SoundCloud takes artwork of up to 2.0 MB, and {} is 2.0 MB", huge.display())),
    ];
    for dry_run in [false, true] {
        for (picture, expected) in &pictures {
            let e = s.publish(&set(), &path, &[], json!({ "picture": picture }), dry_run).0.unwrap_err();
            assert_eq!(kind(&e), ("invalid", format!("{expected} {hint}").as_str()));
        }
    }
    assert!(s.stop().forms.is_empty());
}

/// A dry run says what would go and what SoundCloud does with a mix, and sends nothing, signed in
/// or not.
#[test]
fn a_dry_run_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), "set.mp3", 3 << 19);
    let picture = file(dir.path(), "cover.png", 10);
    let tracklist = [entry(Some("A"), Some("B"), Some(1)), entry(None, None, None)];
    let s = start(SoundCloud::signed_out());
    let settings = json!({ "sharing": "public", "picture": picture });
    let (answer, events) = s.publish(&set(), &path, &tracklist, settings, true);
    let line = "set.mp3 (1.6 MB) would go to SoundCloud as the public track \"Closing set\" with cover.png as its \
                artwork, its 2 tracks in the description";
    assert_eq!(
        answer.unwrap(),
        Published { warnings: vec![line.into(), RIGHTS.into(), FREE_TIER.into()], ..Published::default() }
    );
    assert!(events.is_empty(), "{events:?}");
    let long = PublishedSet { title: Some("x".repeat(MAX_TITLE + 1)), ..set() };
    let (answer, _) = s.publish(&long, &path, &[], json!({}), true);
    let line = format!("set.mp3 (1.6 MB) would go to SoundCloud as the private track \"{}\"", "x".repeat(MAX_TITLE));
    let cut = format!("the title is cut to SoundCloud's {MAX_TITLE} characters");
    assert_eq!(answer.unwrap().warnings, [line, cut, RIGHTS.into(), FREE_TIER.into()]);
    assert!(s.stop().forms.is_empty());
}

/// Signed out, nothing goes: the host's error is the answer. From a host that gives no scratch
/// folder (one without `blobs`), or one the artwork can't be copied to, a set with artwork doesn't
/// go either.
#[test]
fn signed_out_or_without_a_scratch_folder_nothing_goes() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), "set.flac", 10);
    let s = start(SoundCloud::signed_out());
    let e = s.publish(&set(), &path, &[], json!({}), false).0.unwrap_err();
    assert_eq!((e.kind(), e.message.as_str()), (Some("access"), SIGNED_OUT));
    assert!(s.stop().forms.is_empty());

    let picture = file(dir.path(), "cover.png", 10);
    let s = start(SoundCloud::new());
    let req = json!({ "destination": "track", "set": set(), "recording": path, "tracklist": [],
        "context": { "settings": { "picture": picture } } });
    let e = s.host.request::<_, Published>(method::SET_PUBLISH, &req).unwrap_err();
    assert_eq!(e.message, "invalid params: no scratch folder to send the artwork from");
    // Nor when the artwork can't be copied there.
    std::fs::remove_dir(s.scratch.path()).unwrap();
    let e = s.publish(&set(), &path, &[], json!({ "picture": picture }), false).0.unwrap_err();
    assert_eq!(e.kind(), Some("io"), "{e:?}");
    let copy = s.scratch.path().join("artwork.png");
    assert!(e.message.starts_with(&format!("{} to {}: ", picture.display(), copy.display())), "{e:?}");
    assert!(s.stop().forms.is_empty());
}

/// A form that doesn't arrive, or that SoundCloud fails or finds too many, is sent again whole
/// (there's no resuming one), after as long as `Retry-After` says when it says; one that fails
/// every time stops after the last try, saying why, and one asked to wait longer than the plugin
/// waits stops at once, saying how long. Before sending it again after anything but too many
/// requests, the account's newest tracks are asked for, in case SoundCloud made it anyway.
#[test]
fn a_form_that_fails_is_sent_again_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), "set.wav", 300);
    let audio = std::fs::read(&path).unwrap();
    let busy = SoundCloud::scripted(|route, tried| match (route, tried) {
        ("upload", 0) => Some(Reply::Unreachable),
        ("upload", 1) => Some(Reply::Status(429, vec![("retry-after", "3".into())], refusal(429, "Too Many Requests"))),
        _ => None,
    });
    let s = start(busy);
    let (answer, events) = s.publish(&set(), &path, &[], json!({}), false);
    let r = s.stop();
    assert_eq!(answer.unwrap().url.as_deref(), Some(PAGE));
    assert_eq!((r.forms.len(), r.listed), (3, 1));
    assert!(r.forms.iter().all(|f| f.files[0].2 == audio), "each try sends it all");
    assert_eq!(phases(&events), [300, 300, 300]);

    let failing = |route: &str, _| (route == "upload").then(|| Reply::Status(502, Vec::new(), "<html>".into()));
    let s = start(SoundCloud::scripted(failing));
    let (answer, events) = s.publish(&set(), &path, &[], json!({}), false);
    let r = s.stop();
    assert_eq!((r.forms.len(), r.listed), (TRIES as usize, TRIES as usize));
    assert_eq!(kind(&answer.unwrap_err()), ("external", "SoundCloud failed the set (502)"));
    assert_eq!(phases(&events).len(), TRIES as usize);

    let wait = (LONGEST_WAIT + 1).to_string();
    let s = start(SoundCloud::scripted(move |_, _| {
        Some(Reply::Status(429, vec![("Retry-After", wait.clone())], refusal(429, "Too Many Requests")))
    }));
    let e = s.publish(&set(), &path, &[], json!({}), false).0.unwrap_err();
    let r = s.stop();
    assert_eq!((r.forms.len(), r.listed), (1, 0));
    let said = "SoundCloud refused the set (429: Too Many Requests): SoundCloud is taking too many requests: publish \
                again in 6 minutes";
    assert_eq!(kind(&e), ("external", said));
}

/// A track SoundCloud made though it failed the upload is the answer, not sent again; one of the
/// same title made before the upload began isn't. SoundCloud not saying what it has fails the
/// upload, saying to look before publishing again, and cancelled while it's asked, the publish is.
#[test]
fn a_track_made_anyway_is_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), "set.wav", 30);
    let made = |route: &str, tried| (route, tried) == ("upload", 0);
    let s = start(SoundCloud::scripted(move |route, tried| made(route, tried).then_some(Reply::MadeAnyway(500))));
    let (answer, _) = s.publish(&set(), &path, &[], json!({}), false);
    let r = s.stop();
    assert_eq!(answer.unwrap().url.as_deref(), Some(PAGE));
    assert_eq!((r.forms.len(), r.listed), (1, 1));

    let lost = SoundCloud::scripted(move |route, tried| made(route, tried).then_some(Reply::Unreachable));
    let s = start(lost.having("Closing set", TimeDelta::hours(1)).having("Other set", TimeDelta::zero()));
    let (answer, _) = s.publish(&set(), &path, &[], json!({}), false);
    let r = s.stop();
    assert_eq!(answer.unwrap().url.as_deref(), Some(PAGE));
    assert_eq!((r.forms.len(), r.listed), (2, 1));

    let unanswered = [
        (Reply::Unreachable, "connection reset"),
        (
            Reply::Status(401, Vec::new(), refusal(401, "Unauthorized")),
            "SoundCloud refused the account's tracks (401: Unauthorized)",
        ),
        (
            Reply::Status(200, Vec::new(), "[]".into()),
            "SoundCloud's answer can't be read: invalid length 0, expected struct Tracks with 1 element at line 1 column 2",
        ),
    ];
    for (listing, why) in unanswered {
        let s = start(SoundCloud::scripted(move |route, _| match route {
            "upload" => Some(Reply::Status(504, Vec::new(), refusal(504, "Gateway Timeout"))),
            _ => Some(listing.clone()),
        }));
        let e = s.publish(&set(), &path, &[], json!({}), false).0.unwrap_err();
        let r = s.stop();
        assert_eq!((r.forms.len(), r.listed), (1, 1));
        let said = format!(
            "SoundCloud failed the set (504: Gateway Timeout), and SoundCloud may have made the track anyway ({why}): \
             look at your tracks there before publishing again"
        );
        assert_eq!(kind(&e), ("external", said.as_str()));
    }

    // Cancelled while it asks, the publish is cancelled.
    let s = start(SoundCloud::scripted(|route, _| match route {
        "upload" => Some(Reply::Unreachable),
        _ => Some(Reply::Cancelled),
    }));
    let e = s.publish(&set(), &path, &[], json!({}), false).0.unwrap_err();
    assert_eq!((s.stop().forms.len(), e.kind()), (1, Some("cancelled")));
}

/// SoundCloud refusing says why, in its words, and what to do where there's something to do: a
/// track it won't take. A token it doesn't take (which locallibrary refreshes or signs out of
/// before it gets here) is refused as it says. None is sent again.
#[test]
fn soundcloud_refusing_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), "set.ogg", 10);
    let cases = [
        (401, refusal(401, "invalid_token"), "permission", "SoundCloud refused the set (401: invalid_token)"),
        (403, refusal(403, "Forbidden"), "permission", "SoundCloud refused the set (403: Forbidden)"),
        (
            422,
            refusal(422, "Upload limit reached"),
            "invalid",
            "SoundCloud refused the set (422: Upload limit reached)",
        ),
        (
            413,
            String::new(),
            "invalid",
            "SoundCloud refused the set (413): the recording is bigger than SoundCloud takes",
        ),
        (400, json!({ "message": "bad" }).to_string(), "invalid", "SoundCloud refused the set (400: bad)"),
    ];
    for (status, body, expected_kind, expected) in cases {
        let s = start(SoundCloud::scripted(move |_, _| Some(Reply::Status(status, Vec::new(), body.clone()))));
        let e = s.publish(&set(), &path, &[], json!({}), false).0.unwrap_err();
        let r = s.stop();
        assert_eq!((r.forms.len(), r.listed), (1, 0), "{status}");
        assert_eq!(kind(&e), (expected_kind, expected));
    }

    // An answer that can't be read fails.
    let s = start(SoundCloud::scripted(|_, _| Some(Reply::Status(201, Vec::new(), "{}".into()))));
    let e = s.publish(&set(), &path, &[], json!({}), false).0.unwrap_err();
    s.stop();
    assert!(e.message.starts_with("SoundCloud's answer can't be read: missing field `permalink_url`"), "{e:?}");
}

/// `Retry-After` is a number of seconds (one too big to count longer than any wait); none, or one
/// that isn't a number, says nothing.
#[test]
fn retry_after_says_seconds() {
    let said = |name: &str, value: &str| {
        let headers = [(name.to_string(), value.to_string())].into_iter().collect();
        retry_after(&Fetched { status: 429, headers, body: None, blob: None })
    };
    assert_eq!(said("retry-after", " 12 "), Some(12));
    assert_eq!(said("Retry-After", "0"), Some(0));
    assert_eq!(said("retry-after", "99999999999999999999"), Some(u64::MAX));
    assert_eq!(said("retry-after", "Sun, 06 Nov 1994 08:49:37 GMT"), None);
    assert_eq!(said("retry-after", ""), None);
    assert_eq!(said("x-other", "12"), None);
}

/// A tracklist of up to `most` entries, each maybe named, maybe placed in the recording.
#[hegel::test_helper]
fn tracklist(tc: &TestCase, most: usize) -> Vec<TracklistEntry> {
    let names = ["Young Fathers", "\u{dc}n\u{ef}c\u{f8}d\u{e9} Art\u{ef}st", " ", "Two\nLines", &"\u{3a9}".repeat(300)]
        .map(String::from);
    let name = |tc: &TestCase| tc.draw(gs::optional(gs::sampled_from(names.to_vec())));
    let count = tc.draw(gs::integers::<usize>().max_value(most));
    (0..count)
        .map(|_| TracklistEntry {
            artist: name(tc),
            title: name(tc),
            start_seconds: tc.draw(gs::optional(gs::integers::<u32>().max_value(5 * 3600))),
        })
        .collect()
}

/// Whatever the set, its track has a title (the set's, else where and when) and the tracklist as
/// its description, a line each, both within SoundCloud's lengths in characters: a tracklist too
/// long keeps the lines that fit, in order, and says how many more there are and why.
#[hegel::test(test_cases = 200)]
fn the_track_is_the_set_within_soundclouds_lengths(tc: TestCase) {
    let words = gs::optional(gs::sampled_from(vec!["Closing set".to_string(), "  ".into(), "\u{3a9}".repeat(150)]));
    let set = PublishedSet {
        title: tc.draw(words),
        venue: tc.draw(gs::optional(gs::sampled_from(vec!["Warehouse".to_string(), String::new()]))),
        city: tc.draw(gs::optional(gs::just("Leeds".to_string()))),
        ..set()
    };
    let tracklist = tracklist(&tc, 40);
    let t = track::of(&set, &tracklist);

    let chars = |s: &str| s.chars().count();
    assert!(!t.title.is_empty() && chars(&t.title) <= MAX_TITLE, "{t:?}");
    let given = set.title.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match given {
        Some(title) => assert!(title.starts_with(&t.title), "{t:?}"),
        None => {
            let place = set.venue.as_deref().filter(|v| !v.is_empty()).or(set.city.as_deref()).unwrap_or("DJ set");
            assert_eq!(t.title, format!("{place}, 2026-10-03"));
        }
    }
    let cut = given.is_some_and(|g| chars(g) > MAX_TITLE);
    assert_eq!(t.notes.iter().any(|n| n.starts_with("the title is cut")), cut);

    assert!(chars(&t.description) <= MAX_DESCRIPTION, "{}", chars(&t.description));
    let lines: Vec<String> = tracklist.iter().map(track::line).collect();
    assert!(lines.iter().all(|l| !l.contains('\n')), "{lines:?}");
    let all = lines.join("\n");
    if chars(&all) <= MAX_DESCRIPTION {
        assert_eq!(t.description, all);
        assert!(!t.notes.iter().any(|n| n.starts_with("the description")), "{:?}", t.notes);
        return;
    }
    let (kept, more) = t.description.rsplit_once('\n').unwrap_or(("", &t.description));
    let kept: Vec<&str> = kept.lines().collect();
    assert_eq!(kept, lines[..kept.len()]);
    assert_eq!(more, format!("+ {} more", lines.len() - kept.len()));
    // The next line wouldn't have fit, with one fewer to say there are more of.
    let fewer = format!("+ {} more", lines.len() - kept.len() - 1);
    assert!(chars(&t.description) - more.len() + chars(&lines[kept.len()]) + 1 + fewer.len() > MAX_DESCRIPTION);
    let note = format!(
        "the description lists {} of the {} tracks, as many as SoundCloud's {MAX_DESCRIPTION} characters hold",
        kept.len(),
        lines.len()
    );
    assert!(t.notes.contains(&note), "{:?}", t.notes);
}

/// Where the count of the tracks left out loses a digit, the description keeps every line that
/// fits beside the shorter count.
#[test]
fn a_cut_tracklist_keeps_every_line_that_fits() {
    // 498 characters a line: eight of them, each with its newline, and "+ 9 more" make 4,000.
    let long = entry(Some(&"\u{3a9}".repeat(493)), None, None);
    let t = track::of(&set(), &vec![long; 17]);
    assert_eq!(t.description.chars().count(), MAX_DESCRIPTION);
    assert!(t.description.ends_with("\n+ 9 more"), "{:?}", t.description.chars().rev().take(20).collect::<String>());
}

/// A line's time, as `05:30` or past an hour `1:02:03`, reads back as the entry's.
#[hegel::test(test_cases = 100)]
fn a_lines_time_reads_back(tc: TestCase) {
    let secs = tc.draw(gs::integers::<u32>());
    let line = track::line(&entry(Some("A"), Some("B"), Some(secs)));
    let (time, name) = line.split_once(' ').unwrap();
    assert_eq!(name, "A - B");
    let fields: Vec<u64> = time.split(':').map(|f| f.parse().unwrap()).collect();
    assert!(time.split(':').skip(1).all(|f| f.len() == 2), "{line}");
    assert!(secs >= 3600 || time.len() == 5, "{line}");
    match fields[..] {
        [m, s] => assert_eq!((secs < 3600, m * 60 + s), (true, u64::from(secs)), "{line}"),
        [h, m, s] => assert_eq!((secs >= 3600, h * 3600 + m * 60 + s), (true, u64::from(secs)), "{line}"),
        _ => panic!("{line}"),
    }
}

/// How SoundCloud may answer a try in the property below, in place of taking the track: not being
/// reached, a server error, too many requests (saying how long to wait, maybe too long), refusing
/// the track or the sign-in, or making the track though it answers an error.
#[hegel::test_helper]
fn reply(tc: &TestCase) -> Reply {
    match tc.draw(gs::integers::<u8>().max_value(7)) {
        0 => Reply::Unreachable,
        1 => Reply::Status(503, Vec::new(), String::new()),
        2 => {
            let wait = tc.draw(gs::integers::<u64>().max_value(LONGEST_WAIT + 2));
            Reply::Status(429, vec![("retry-after", wait.to_string())], refusal(429, "Too Many Requests"))
        }
        3 => Reply::Status(429, Vec::new(), String::new()),
        4 => Reply::Status(422, Vec::new(), refusal(422, "Unprocessable Entity")),
        5 => Reply::Status(401, Vec::new(), refusal(401, "invalid_token")),
        6 => Reply::MadeAnyway(500),
        _ => Reply::Status(500, Vec::new(), "<html>".into()),
    }
}

/// What a try SoundCloud answers `reply` to fails as, and whether it's worth sending the form again
/// for, and whether SoundCloud may have made the track anyway.
fn failing(reply: &Reply) -> (&'static str, bool, bool) {
    match reply {
        Reply::Status(429, headers, _) => {
            let wait = headers.first().map_or(0, |(_, w)| w.parse::<u64>().unwrap());
            ("external", wait <= LONGEST_WAIT, false)
        }
        Reply::Status(401, ..) => ("permission", false, false),
        Reply::Status(422, ..) => ("invalid", false, false),
        _ => ("external", true, true),
    }
}

/// Whatever the recording, artwork and settings, a recording or artwork SoundCloud wouldn't take
/// is refused before anything goes, and a dry run sends nothing. Otherwise, however SoundCloud
/// answers each try and says what tracks the account has, every try sends the recording and
/// artwork whole with the set's details as the settings say them, with the account's token as
/// `OAuth`, and the publish answers the track's page once one is taken or found made anyway, or
/// stops at the first answer not worth trying again for, after the last try, or once SoundCloud
/// can't say whether it made the track, failing as what SoundCloud said (or cancelled, when the
/// publish is). The account's tracks are asked for after each try SoundCloud may have made the
/// track of.
#[hegel::test(test_cases = 100)]
fn any_publish_sends_the_set_whole_or_stops_saying_why(tc: TestCase) {
    let format = tc.draw(gs::sampled_from(vec!["flac", "WAV", "aiff", "mp3", "m4a", "opus"]));
    // 0: empty, 1: gone, 2: over SoundCloud's limit; else as long as it's drawn.
    let shape = tc.draw(gs::integers::<u8>().max_value(19));
    let size = tc.draw(gs::integers::<u64>().min_value(1).max_value(5000));
    // 0: none, 1: a GIF, 2: not there, 3: over SoundCloud's limit; else a PNG.
    let pictured = tc.draw(gs::integers::<u8>().max_value(9));
    let sharing = tc.draw(gs::sampled_from(vec!["private", "public"]));
    let genre = tc.draw(gs::optional(gs::just("Techno")));
    let tracklist = tracklist(&tc, 3);
    let dry_run = tc.draw(gs::weighted_booleans(0.1));
    // How SoundCloud answers each try before taking the track (at the try after the last one),
    // and answers when it's asked for the account's tracks (as it would when none).
    let failures = tc.draw(gs::integers::<usize>().max_value(TRIES as usize));
    let replies: Vec<Reply> = (0..failures).map(|_| reply(&tc)).collect();
    let listing = tc.draw(gs::sampled_from(vec![
        None,
        None,
        Some(Reply::Unreachable),
        Some(Reply::Cancelled),
        Some(Reply::Status(401, Vec::new(), refusal(401, "Unauthorized"))),
    ]));
    let dir = tempfile::tempdir().unwrap();
    let path = file(dir.path(), &format!("set.{format}"), if shape == 0 { 0 } else { size });
    match shape {
        1 => std::fs::remove_file(&path).unwrap(),
        2 => std::fs::File::options().write(true).open(&path).unwrap().set_len(MAX_BYTES + 1).unwrap(),
        _ => {}
    }
    let picture = match pictured {
        0 => None,
        1 => Some(file(dir.path(), "cover.gif", 50)),
        2 => Some(dir.path().join("nothing.png")),
        3 => {
            let huge = file(dir.path(), "huge.png", 0);
            std::fs::File::options().write(true).open(&huge).unwrap().set_len(MAX_PICTURE + 1).unwrap();
            Some(huge)
        }
        _ => Some(file(dir.path(), "cover.png", 50)),
    };
    let settings = json!({ "sharing": sharing, "genre": genre, "picture": picture });

    let (script, listed_as) = (replies.clone(), listing.clone());
    let soundcloud = SoundCloud::scripted(move |route, tried| match route {
        "upload" => script.get(tried).cloned(),
        _ => listed_as.clone(),
    });
    // One session for every case: stopping one waits out the SDK's progress flusher, and a failing
    // case shrinks through hundreds of them.
    static SESSION: LazyLock<Mutex<soundcloud::Session>> = LazyLock::new(|| Mutex::new(start(SoundCloud::new())));
    let s = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    s.script(soundcloud);
    let (answer, events) = s.publish(&set(), &path, &tracklist, settings, dry_run);
    let r = s.received();
    drop(s);

    let refused_first = match (format, shape, pictured) {
        ("opus", ..) | (_, 0 | 2, _) => Some("invalid"),
        (_, 1, _) => Some("io"),
        (.., 1..=3) => Some("invalid"),
        _ => None,
    };
    if let Some(kind) = refused_first {
        assert_eq!(answer.unwrap_err().kind(), Some(kind));
        assert!(r.forms.is_empty() && r.listed == 0 && events.is_empty());
        return;
    }
    if dry_run {
        let published = answer.unwrap();
        assert_eq!(published.url, None);
        assert!(published.warnings.ends_with(&[RIGHTS.into(), FREE_TIER.into()]), "{published:?}");
        assert!(r.forms.is_empty() && r.listed == 0 && events.is_empty());
        return;
    }
    let audio = std::fs::read(&path).unwrap();
    let artwork = picture.as_ref().filter(|_| pictured > 3).map(|p| std::fs::read(p).unwrap());
    let description = track::of(&set(), &tracklist).description;
    for form in &r.forms {
        assert_eq!(form.authorization, format!("OAuth {TOKEN}"));
        assert_eq!(form.field("track[title]"), Some("Closing set"));
        assert_eq!(form.field("track[sharing]"), Some(sharing));
        assert_eq!(form.field("track[downloadable]"), Some("false"));
        assert_eq!(form.field("track[genre]"), genre);
        assert_eq!(form.field("track[description]"), Some(description.as_str()).filter(|d| !d.is_empty()));
        assert_eq!(form.files[0].2, audio);
        assert_eq!(form.files.get(1).map(|f| &f.2), artwork.as_ref());
    }
    let sent = size + artwork.as_ref().map_or(0, |a| a.len() as u64);
    assert_eq!(phases(&events), vec![sent; r.forms.len()]);

    // Where it stops, how, and how often it asks for the account's tracks on the way.
    let listed_fails = match &listing {
        None => None,
        Some(Reply::Cancelled) => Some("cancelled"),
        Some(_) => Some("external"),
    };
    let (mut listed, mut stopped) = (0, None);
    for (at, reply) in replies.iter().enumerate() {
        if let Reply::MadeAnyway(_) = reply {
            listed += 1;
            stopped = Some((at, listed_fails));
            break;
        }
        let (kind, again, unknown) = failing(reply);
        let last = at + 1 == TRIES as usize;
        if !again || last && !unknown {
            stopped = Some((at, Some(kind)));
            break;
        }
        if unknown {
            listed += 1;
            if listed_fails.is_some() || last {
                stopped = Some((at, listed_fails.or(Some(kind))));
                break;
            }
        }
    }
    let (at, failed) = stopped.unwrap_or((failures, None));
    assert_eq!((r.forms.len(), r.listed), (at + 1, listed), "{replies:?}");
    match failed {
        Some(kind) => assert_eq!(answer.unwrap_err().kind(), Some(kind)),
        None => assert_eq!(answer.unwrap().url.as_deref(), Some(PAGE)),
    }
}
