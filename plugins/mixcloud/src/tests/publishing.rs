//! The hook end to end, with a stand-in for Mixcloud through the host.

use std::path::Path;
use std::time::{Duration, Instant};

use loclib_plugin::{Cancel, Manifest, Published, PublishedSet, method};
use serde_json::json;

use super::mixcloud::{Mixcloud, PAGE, Part, Reply, SIGNED_OUT, error, file, rate_limited, start};
use super::{entry, set};
use crate::upload::{MAX_RETRY_AFTER, TRIES};
use crate::*;

fn kind(e: &RpcError) -> (&str, &str) {
    (e.kind().unwrap_or_default(), e.message.as_str())
}

fn text(name: &str, value: &str) -> (String, Part) {
    (name.into(), Part::Text(value.into()))
}

/// Served as a real one is, it says it's its manifest, publishing sets through fetches with an
/// account.
#[test]
fn it_serves_its_manifest() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    let s = start(Mixcloud::new());
    let has = |f: &&str| s.hello.features.iter().any(|had| had == f);
    assert!(["destinations", "net", "blobs", "accounts"].iter().all(has), "{:?}", s.hello.features);
    let described: Manifest = s.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    s.stop();
}

/// The recording goes up whole in one form with the token in the query: the file as `mp3`, then
/// the show's name, description, tags and sections in order (an unnamed track as `ID`, a track
/// without a time without `start_time`), unlisted as the setting says, and the picture last. The
/// answer is the show's page, and the upload's progress is all that's sent.
#[test]
fn a_set_goes_up_as_one_form_and_answers_its_page() {
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "Closing Set.FLAC", 3000);
    let picture = file(dir.path(), "cover.jpg", 200);
    let tracklist = [
        entry(Some("Young Fathers"), Some("In My View"), Some(0)),
        entry(None, None, Some(330)),
        entry(Some("Someone"), Some("Unplaced"), None),
        entry(Some("Back"), Some("In Time"), Some(100)),
        entry(Some("Chris Lake"), Some("Turn Off the Lights"), Some(3723)),
    ];
    let settings = json!({ "tags": "House, techno, house", "picture": picture, "unlisted": true });
    let s = start(Mixcloud::new());
    let (answer, started, ended) = s.publish(&set(), &recording, &tracklist, settings, false);
    let r = s.stop();

    let left_off = "left the time off 1 of the tracks, each no later than a time before it";
    assert_eq!(
        answer.unwrap(),
        Published {
            url: Some(PAGE.into()),
            name: Some("Mixcloud".into()),
            warnings: vec![left_off.into()],
            ..Published::default()
        }
    );
    let [form] = &r[..] else { panic!("one upload: {r:?}") };
    let expected = vec![
        ("mp3".into(), Part::File("Closing Set.FLAC".into(), std::fs::read(&recording).unwrap())),
        text("name", "Closing set"),
        text("description", "Played at Warehouse, Leeds, GB on 2026-10-03"),
        text("tags-0-tag", "House"),
        text("tags-1-tag", "techno"),
        text("sections-0-artist", "Young Fathers"),
        text("sections-0-song", "In My View"),
        text("sections-0-start_time", "0"),
        text("sections-1-artist", "ID"),
        text("sections-1-song", "ID"),
        text("sections-1-start_time", "330"),
        text("sections-2-artist", "Someone"),
        text("sections-2-song", "Unplaced"),
        text("sections-3-artist", "Back"),
        text("sections-3-song", "In Time"),
        text("sections-4-artist", "Chris Lake"),
        text("sections-4-song", "Turn Off the Lights"),
        text("sections-4-start_time", "3723"),
        text("unlisted", "1"),
        ("picture".into(), Part::File("cover.jpg".into(), std::fs::read(&picture).unwrap())),
    ];
    assert_eq!(*form, expected);
    assert_eq!((started, ended), (vec![3200], 1));
}

/// Without a picture, tags or tracklist, and listed, the form has the recording, name and
/// description alone, and a host without a scratch folder sends it: only a picture needs one.
#[test]
fn a_bare_set_needs_no_scratch_folder() {
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "set.mp3", 10);
    let s = start(Mixcloud::new());
    let untitled = PublishedSet { title: None, venue: None, city: None, country: None, ..set() };
    let pending = s.send(&untitled, &recording, &[], json!({ "unlisted": false, "tags": " , " }), false, false);
    let answer = pending.wait_timeout::<Published>(Duration::from_secs(60)).expect("an answer");
    assert_eq!(
        answer.unwrap(),
        Published { url: Some(PAGE.into()), name: Some("Mixcloud".into()), ..Published::default() }
    );
    let r = s.stop();
    let expected = vec![
        ("mp3".into(), Part::File("set.mp3".into(), std::fs::read(&recording).unwrap())),
        text("name", "DJ set, 2026-10-03"),
        text("description", "Played on 2026-10-03"),
    ];
    assert_eq!(r, [expected]);
}

/// A picture to send from a host without a scratch folder is refused, and nothing goes.
#[test]
fn a_picture_needs_a_scratch_folder() {
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "set.mp3", 10);
    let picture = file(dir.path(), "cover.png", 10);
    let s = start(Mixcloud::new());
    let pending = s.send(&set(), &recording, &[], json!({ "picture": picture }), false, false);
    let e = pending.wait_timeout::<Published>(Duration::from_secs(60)).expect("an answer").unwrap_err();
    assert_eq!(e.message, "invalid params: no scratch folder to send the picture from");
    assert!(s.stop().is_empty());
}

/// A dry run says what would go, what was cut or left out, and what Mixcloud does with a set,
/// sending nothing.
#[test]
fn a_dry_run_says_what_would_go_and_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "set.wav", 2_500_000);
    let picture = file(dir.path(), "cover.jpg", 10);
    let tracklist = [entry(Some("A"), Some("B"), Some(60)), entry(Some("C"), Some("D"), None)];
    let s = start(Mixcloud::new());
    let settings = json!({ "tags": "a, b, c, d, e, f, g", "picture": picture, "unlisted": true });
    let (answer, started, _) = s.publish(&set(), &recording, &tracklist, settings, true);
    assert_eq!(
        answer.unwrap(),
        Published {
            url: None,
            warnings: vec![
                "set.wav (2.5 MB) would go to Mixcloud as the unlisted show \"Closing set\", its 2 tracks as the \
                 tracklist (1 timed), tagged a, b, c, d, e, cover.jpg as its picture"
                    .into(),
                "Mixcloud takes 5 tags: left out f, g".into(),
                MIXCLOUD.into(),
            ],
            ..Published::default()
        }
    );
    let (answer, ..) = s.publish(&set(), &recording, &[], json!({}), true);
    let what = "set.wav (2.5 MB) would go to Mixcloud as the show \"Closing set\"";
    assert_eq!(answer.unwrap().warnings, [what, MIXCLOUD]);
    assert!(started.is_empty());
    assert!(s.stop().is_empty());
}

/// A recording Mixcloud doesn't take (its format, empty, over 4 GB) or a picture it can't send
/// (not a file, relative, over 10 MB) is refused before anything goes, a dry run too.
#[test]
fn what_mixcloud_wouldnt_take_is_refused_first() {
    let dir = tempfile::tempdir().unwrap();
    let mp3 = file(dir.path(), "set.mp3", 10);
    let s = start(Mixcloud::new());
    let refused = |recording: &Path, settings: serde_json::Value| {
        let (dry, ..) = s.publish(&set(), recording, &[], settings.clone(), true);
        let (real, ..) = s.publish(&set(), recording, &[], settings, false);
        let (dry, real) = (dry.unwrap_err(), real.unwrap_err());
        assert_eq!(kind(&dry), kind(&real));
        real
    };

    let video = file(dir.path(), "set.mp4", 10);
    let takes = "Mixcloud takes MP3, M4A, AAC, WAV, AIFF, FLAC, OGG, MP2 and WMA recordings, which set.mp4 isn't";
    assert_eq!(kind(&refused(&video, json!({}))), ("invalid", takes));
    let bare = file(dir.path(), "set", 10);
    assert_eq!(refused(&bare, json!({})).message, takes.replace("set.mp4", "set"));
    let empty = file(dir.path(), "empty.m4a", 0);
    assert_eq!(kind(&refused(&empty, json!({}))), ("invalid", "empty.m4a is empty"));
    // Sparse: it takes no room. Every limit is the most Mixcloud takes.
    let huge = dir.path().join("huge.wav");
    std::fs::File::create(&huge).unwrap().set_len(MAX_BYTES).unwrap();
    assert!(s.publish(&set(), &huge, &[], json!({}), true).0.is_ok());
    std::fs::File::create(&huge).unwrap().set_len(MAX_BYTES + 1).unwrap();
    assert_eq!(refused(&huge, json!({})).kind(), Some("invalid"));
    std::fs::File::create(&huge).unwrap().set_len(MAX_BYTES + 50_000_000).unwrap();
    let why = "Mixcloud takes recordings of up to 4.00 GB, and huge.wav is 4.05 GB";
    assert_eq!(kind(&refused(&huge, json!({}))), ("invalid", why));
    assert_eq!(refused(&dir.path().join("gone.mp3"), json!({})).kind(), Some("io"));

    // The crate's Cargo.toml is a file from where tests run, but not an absolute path.
    for picture in [dir.path().join("gone.png"), "Cargo.toml".into(), dir.path().to_path_buf()] {
        let said = format!(
            "the picture setting, {}, isn't a file (locallibrary plugin settings mixcloud picture)",
            picture.display()
        );
        assert_eq!(kind(&refused(&mp3, json!({ "picture": picture }))), ("invalid", said.as_str()));
    }
    let big = dir.path().join("big.jpg");
    std::fs::File::create(&big).unwrap().set_len(MAX_PICTURE + 100_000).unwrap();
    let why = "Mixcloud takes pictures of up to 10.0 MB, and big.jpg is 10.1 MB";
    assert_eq!(kind(&refused(&mp3, json!({ "picture": big }))), ("invalid", why));
    std::fs::File::create(&big).unwrap().set_len(MAX_PICTURE + 1).unwrap();
    assert_eq!(refused(&mp3, json!({ "picture": big })).kind(), Some("invalid"));
    std::fs::File::create(&big).unwrap().set_len(MAX_PICTURE).unwrap();
    assert!(s.publish(&set(), &mp3, &[], json!({ "picture": big }), true).0.is_ok());
    assert!(s.stop().is_empty());
}

/// An upload that doesn't arrive, or that Mixcloud fails or times out, is sent again whole,
/// waiting longer each time, and the show it makes in the end is the answer; one that keeps
/// failing fails after the last try, saying why.
#[test]
fn transient_failures_send_the_whole_form_again() {
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "set.ogg", 500);
    let s = start(Mixcloud::scripted(|n| match n {
        0 => Some(Reply::Unreachable),
        1 => Some(error(503, "", "")),
        2 => Some(Reply::Status(408, Vec::new(), "<html>timeout</html>".into())),
        _ => None,
    }));
    let (answer, started, ended) = s.publish(&set(), &recording, &[], json!({}), false);
    assert_eq!(answer.unwrap().url.as_deref(), Some(PAGE));
    let r = s.received();
    assert_eq!(r.len(), 4);
    let audio = Part::File("set.ogg".into(), std::fs::read(&recording).unwrap());
    assert!(r.iter().all(|form| form[0] == ("mp3".into(), audio.clone())), "the whole recording each time");
    assert_eq!((started, ended), (vec![500; 4], 4));

    s.script(Mixcloud::scripted(|_| Some(error(500, "ServerError", "Something went wrong"))));
    let (answer, started, _) = s.publish(&set(), &recording, &[], json!({}), false);
    let failed = "Mixcloud failed the show (500 ServerError: Something went wrong)";
    assert_eq!(kind(&answer.unwrap_err()), ("external", failed));
    assert_eq!((s.received().len(), started.len()), (TRIES as usize, TRIES as usize));

    s.script(Mixcloud::scripted(|_| Some(Reply::Unreachable)));
    let e = s.publish(&set(), &recording, &[], json!({}), false).0.unwrap_err();
    assert_eq!(kind(&e), ("external", "couldn't send the show to Mixcloud: connection reset"));
    assert_eq!(s.stop().len(), TRIES as usize);
}

/// Over Mixcloud's rate limit, the upload waits as long as Mixcloud says (its `Retry-After`, else
/// the answer's `retry_after`) and goes again; when that's longer than publishing waits, it fails
/// at once saying when to publish again.
#[test]
fn the_rate_limit_is_waited_out_when_its_short() {
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "set.mp3", 10);
    // A test's second is a millisecond: these wait 0.4 s, where the first wait is 5 ms.
    for (header, body) in [(Some(400), None), (None, Some(400)), (Some(400), Some(1))] {
        let s = start(Mixcloud::scripted(move |n| (n == 0).then(|| rate_limited(header, body))));
        let started = Instant::now();
        let (answer, ..) = s.publish(&set(), &recording, &[], json!({}), false);
        assert_eq!(answer.unwrap().url.as_deref(), Some(PAGE));
        assert!(started.elapsed() >= Duration::from_millis(400), "{:?}", started.elapsed());
        assert_eq!(s.stop().len(), 2);
    }

    let s = start(Mixcloud::scripted(|_| Some(rate_limited(Some(MAX_RETRY_AFTER + 1), Some(MAX_RETRY_AFTER + 1)))));
    let e = s.publish(&set(), &recording, &[], json!({}), false).0.unwrap_err();
    let said = "Mixcloud refused the show (403 RateLimitException: You have hit your rate limit. Retry after 901 \
                seconds.): it's over its rate limit: publish again in 16 minutes";
    assert_eq!(kind(&e), ("external", said));
    assert_eq!(s.received().len(), 1);

    // A 429 is a rate limit too, waited out as an error is without a time.
    s.script(Mixcloud::scripted(|_| Some(Reply::Status(429, Vec::new(), String::new()))));
    let e = s.publish(&set(), &recording, &[], json!({}), false).0.unwrap_err();
    assert_eq!(kind(&e), ("external", "Mixcloud refused the show (429): it's over its rate limit"));
    assert_eq!(s.stop().len(), TRIES as usize);
}

/// Cancelled while it waits out the rate limit, the hook answers as cancelled at once.
#[test]
fn cancelling_stops_the_wait() {
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "set.mp3", 10);
    // The longest wait, 0.9 s of a test's milliseconds.
    let s = start(Mixcloud::scripted(|_| Some(rate_limited(Some(MAX_RETRY_AFTER), None))));
    let pending = s.send(&set(), &recording, &[], json!({}), false, true);
    while s.asked() == 0 {
        assert!(pending.wait_timeout::<Published>(Duration::from_millis(5)).is_none(), "it waits");
    }
    let asked = Instant::now();
    s.host.notify(method::CANCEL, &Cancel { id: pending.id().into() });
    let answer = pending.wait_timeout::<Published>(Duration::from_secs(30)).expect("an answer");
    assert!(asked.elapsed() < Duration::from_millis(400), "{:?}", asked.elapsed());
    assert_eq!(answer.unwrap_err().kind(), Some("cancelled"));
    assert_eq!(s.stop().len(), 1);
}

/// What Mixcloud refuses for good is said in its words, with what to do where there's something
/// to do, and not sent again: a token it doesn't take, a form it found wrong (each field's
/// problem), the free account's limit, anything else it forbids; and an answer naming no show.
#[test]
fn refusals_say_why_and_go_once() {
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "set.mp3", 10);
    let s = start(Mixcloud::new());
    let refused = |reply: Reply| {
        s.script(Mixcloud::scripted(move |_| Some(reply.clone())));
        let e = s.publish(&set(), &recording, &[], json!({}), false).0.unwrap_err();
        assert_eq!(s.received().len(), 1, "{e:?}");
        (e.kind().unwrap_or_default().to_string(), e.message)
    };
    let said = |kind: &str, message: &str| (kind.to_string(), message.to_string());

    let token = refused(error(401, "OAuthException", "An invalid access token was provided."));
    // One locallibrary didn't refresh, or sign out of, before it got here.
    let why = "Mixcloud refused the show (401 OAuthException: An invalid access token was provided.)";
    assert_eq!(token, said("permission", why));

    let details = json!({ "mp3": ["Unsupported audio.", "Try another file."], "name": "Too long." });
    let body = json!({ "error": { "type": "PostValidationError", "message": "Invalid fields." }, "details": details });
    let form = refused(Reply::Status(400, Vec::new(), body.to_string()));
    let why = "Mixcloud refused the show (400 PostValidationError: Invalid fields.; mp3: Unsupported audio. Try \
               another file.; name: Too long.)";
    assert_eq!(form, said("invalid", why));

    let limit = refused(error(403, "UploadLimitException", "You have reached your upload limit."));
    let why = "Mixcloud refused the show (403 UploadLimitException: You have reached your upload limit.): a free \
               Mixcloud account keeps 10 shows published: move one to your drafts, or go Pro";
    assert_eq!(limit, said("invalid", why));

    let forbidden = refused(error(403, "PermissionException", "Not allowed."));
    assert_eq!(forbidden, said("permission", "Mixcloud refused the show (403 PermissionException: Not allowed.)"));
    let bare = refused(Reply::Status(404, Vec::new(), "<html>gone</html>".into()));
    assert_eq!(bare, said("invalid", "Mixcloud refused the show (404)"));

    for (body, key) in [
        (json!({ "result": { "success": true } }), ""),
        (json!({ "result": { "success": true, "key": "/djkit/" } }), "/djkit/"),
        (json!({ "result": { "success": true, "key": "/djkit/a b/" } }), "/djkit/a b/"),
        (json!({ "result": { "success": true, "key": "djkit/set/" } }), "djkit/set/"),
    ] {
        let answer = refused(Reply::Status(200, Vec::new(), body.to_string()));
        assert_eq!(answer, said("external", &format!("Mixcloud's answer named no show: {key:?}")));
    }
    let unread = refused(Reply::Status(200, Vec::new(), "OK".into()));
    assert!(unread.1.starts_with("Mixcloud's answer can't be read: "), "{unread:?}");
    s.stop();
}

/// With the account signed out, publishing fails as the host's request did, and nothing goes.
#[test]
fn signed_out_fails_as_the_host_says() {
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "set.mp3", 10);
    let s = start(Mixcloud::signed_out());
    let (answer, ..) = s.publish(&set(), &recording, &[], json!({}), false);
    let e = answer.unwrap_err();
    assert_eq!(kind(&e), ("access", SIGNED_OUT));
    assert!(s.stop().is_empty());
}
