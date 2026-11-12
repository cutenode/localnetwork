//! The hook end to end, with stand-ins for FFmpeg (shell scripts, so not on Windows) and for
//! YouTube through the host.

mod tools;
mod youtube;

use std::time::{Duration, Instant};

use hegel::TestCase;
use hegel::generators as gs;

use loclib_plugin::{Published, PublishedSet, TracklistEntry, method};
use loclib_progress::{Event, Phase};
use serde_json::json;

use super::set;
use crate::*;
use youtube::{YouTube, start};

fn entry(artist: Option<&str>, title: Option<&str>, start: Option<u32>) -> TracklistEntry {
    TracklistEntry { artist: artist.map(Into::into), title: title.map(Into::into), start_seconds: start }
}

/// Served as a real one is, it says it's its manifest, publishing sets through fetches with an
/// account.
#[test]
fn it_serves_its_manifest() {
    // `start` checks that it describes itself as its manifest.
    let s = start(YouTube::new());
    let has = |f: &&str| s.host.hello().features.iter().any(|had| had == f);
    assert!(["destinations", "net", "blobs", "accounts"].iter().all(has), "{:?}", s.host.hello().features);
    s.stop();
}

/// An error's kind and message.
fn kind(e: &RpcError) -> (&str, &str) {
    (e.kind().unwrap_or_default(), e.message.as_str())
}

/// From a host that gives no scratch folder (one without `blobs`), nothing is made or sent: the
/// video is made there.
#[test]
fn without_a_scratch_folder_nothing_goes() {
    let dir = tempfile::tempdir().unwrap();
    let path = tools::file(dir.path(), "set.m4a", 10);
    tools::working(tools::AAC, b"video");
    let s = start(YouTube::new());
    let req = json!({ "destination": "video", "set": set(), "recording": path, "tracklist": [],
        "context": { "scratch": null } });
    let e = s.host.request::<Published>(method::SET_PUBLISH, &req).unwrap_err();
    assert_eq!(e.message, "invalid params: no scratch folder to make the video in");
    assert_eq!(tools::args("ffmpeg"), None);
    assert!(s.stop().requests.is_empty());
}

/// A recording goes up as FFmpeg's video of a black frame over its audio (copied, being AAC
/// already), its title and tracklist as YouTube takes them, unlisted, in YouTube's chunks; the
/// answer is the video's page, named YouTube, and the video is gone from the scratch folder.
#[test]
fn a_set_goes_up_as_a_video_and_answers_its_page() {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "Closing Set.m4a", 4000);
    let made = std::fs::read(tools::file(dir.path(), "made.mp4", 3 * upload::CHUNK + 5)).unwrap();
    // An hour and a bit, which ffmpeg says it's halfway through, then done.
    let probed = r#"{"streams": [{"codec_name": "aac"}], "format": {"duration": "4000.000000"}}"#;
    tools::answer("ffprobe", None, probed, "", 0);
    let progress = "out_time_us=2000000000\nprogress=continue\nout_time_us=4000000000\nprogress=end\n";
    tools::answer("ffmpeg", Some(&made), progress, "", 0);
    let tracklist = [
        entry(Some("Young Fathers"), Some("In My View"), Some(0)),
        entry(None, None, Some(330)),
        entry(Some("Someone"), Some("Unplaced"), None),
        entry(Some("Chris Lake"), Some("Turn Off <the> Lights"), Some(3723)),
    ];
    let s = start(YouTube::new());
    let (answer, events) = s.publish(&set(), &recording, &tracklist, json!({ "privacy": "unlisted" }), false);
    let mp4 = s.scratch.path().join("video.mp4");
    let r = s.stop();

    let published = answer.unwrap();
    assert_eq!(published.url.as_deref(), Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
    assert_eq!(published.name.as_deref(), Some("YouTube"));
    assert_eq!(published.warnings, [PROCESSING]);
    assert!(!mp4.exists(), "the video is gone once it's sent");

    let ffprobe = format!(
        "-v\nerror\n-select_streams\na:0\n-show_entries\nstream=codec_name:format=duration\n-of\njson\n{}\n",
        recording.display()
    );
    assert_eq!(tools::args("ffprobe"), Some(ffprobe));
    let ffmpeg = [
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-t",
        "4000.000000",
        "-i",
        "color=c=black:s=1920x1080:r=1",
        "-i",
        &recording.display().to_string(),
        "-map",
        "0:v",
        "-map",
        "1:a:0",
        "-vf",
        "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,format=yuv420p",
        "-c:v",
        "libx264",
        "-tune",
        "stillimage",
        "-bf",
        "0",
        "-c:a",
        "copy",
        "-use_editlist",
        "0",
        "-movflags",
        "+faststart",
        "-progress",
        "pipe:1",
        "-nostats",
        &mp4.display().to_string(),
    ];
    assert_eq!(tools::args("ffmpeg"), Some(ffmpeg.map(|a| format!("{a}\n")).concat()));

    // What ffmpeg wrote is what went up, in chunks of YouTube's size but the last.
    assert_eq!(r.video, made);
    let c = upload::CHUNK;
    assert_eq!(r.chunks, [(0, c), (c, c), (2 * c, c), (3 * c, 5)]);
    let started: Vec<(&str, &str)> = r.started.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let size = made.len().to_string();
    assert_eq!(
        started,
        [
            ("Content-Type", "application/json; charset=UTF-8"),
            ("X-Upload-Content-Length", size.as_str()),
            ("X-Upload-Content-Type", "video/mp4")
        ]
    );
    let description =
        "00:00 Young Fathers - In My View\n05:30 ID - ID\nSomeone - Unplaced\n1:02:03 Chris Lake - Turn Off the Lights";
    let resource = json!({
        "snippet": { "title": "Closing set", "description": description, "categoryId": "10" },
        "status": { "privacyStatus": "unlisted" },
    });
    assert_eq!(r.resource, Some(resource));
    assert_eq!((r.count("start"), r.count("chunk"), r.count("status")), (1, 4, 0));

    // The recording's making, then the video's sending: both as much as there is.
    let phases: Vec<&Event> = events.iter().filter(|e| matches!(e, Event::PhaseStart { .. })).collect();
    assert!(
        matches!(phases[..], [
            Event::PhaseStart { phase: Phase::Write, bytes: 4000, .. },
            Event::PhaseStart { phase: Phase::Upload, bytes, .. }
        ] if *bytes == made.len() as u64),
        "{events:?}"
    );
    let written: u64 = events
        .iter()
        .filter_map(|e| match e {
            Event::Bytes { phase: Phase::Write, bytes } => Some(*bytes),
            _ => None,
        })
        .sum();
    assert_eq!(written, 4000);
    assert!(events.iter().any(|e| matches!(e, Event::PhaseEnd { phase: Phase::Upload })), "{events:?}");
}

/// A recording that isn't AAC is encoded as AAC at the 48 kHz and 384 kbit/s YouTube recommends,
/// under the picture setting's picture, as public as the setting says; YouTube making it less so
/// is said.
#[test]
fn a_picture_goes_over_audio_encoded_as_aac() {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.wav", 100);
    let picture = tools::file(dir.path(), "cover.jpg", 10);
    tools::working(tools::WAV, b"a video");
    let s = start(YouTube::scripted(|kind, _| {
        let made = json!({ "id": "abc", "status": { "privacyStatus": "private" } });
        (kind == "chunk").then(|| youtube::Reply::Status(201, made.to_string()))
    }));
    let settings = json!({ "privacy": "public", "picture": picture });
    let (answer, _) = s.publish(&set(), &recording, &[], settings, false);
    let r = s.stop();
    let published = answer.unwrap();
    assert_eq!(published.url.as_deref(), Some("https://www.youtube.com/watch?v=abc"));
    let warning = "YouTube made the video private rather than public: change it in YouTube Studio";
    assert_eq!(published.warnings, [warning, PROCESSING]);
    assert_eq!(r.resource.unwrap()["status"]["privacyStatus"], "public");

    let args = tools::args("ffmpeg").unwrap();
    let still =
        format!("-y\n-loop\n1\n-framerate\n1\n-t\n25.000000\n-i\n{}\n-i\n{}\n", picture.display(), recording.display());
    assert!(args.contains(&still), "{args}");
    assert!(args.contains("\n-c:a\naac\n-b:a\n384k\n-ar\n48000\n-use_editlist\n"), "{args}");
}

/// Cancelled while FFmpeg makes the video, the hook stops ffmpeg and answers as cancelled at
/// once, sending nothing; and an ffmpeg that can't be run says why.
#[test]
fn cancelling_stops_ffmpeg() {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", 1000);
    tools::working(tools::AAC, b"video");
    tools::stall("ffmpeg");
    let s = start(YouTube::new());
    let pending = s.send(&set(), &recording, &[], json!({}), false);
    // Once ffmpeg has said how far it has got, it's running.
    let made = |e: &Event| matches!(e, Event::Bytes { phase: Phase::Write, .. });
    while !youtube::events(&pending).iter().any(made) {
        assert!(pending.wait_timeout::<Published>(Duration::from_millis(20)).is_none(), "it waits for ffmpeg");
    }
    let asked = Instant::now();
    pending.cancel();
    let answer = pending.wait_timeout::<Published>(Duration::from_secs(30)).expect("an answer");
    assert!(asked.elapsed() < Duration::from_secs(10), "{:?}", asked.elapsed());
    assert_eq!(answer.unwrap_err().kind(), Some("cancelled"));

    tools::working(tools::AAC, b"video");
    tools::unrunnable("ffprobe");
    let e = s.publish(&set(), &recording, &[], json!({}), true).0.unwrap_err();
    assert_eq!(e.kind(), Some("external"));
    assert!(e.message.starts_with("ffprobe couldn't be started: "), "{e:?}");
    assert!(s.stop().requests.is_empty());
}

/// A picture setting that isn't a file, or isn't absolute, is refused before FFmpeg runs.
#[test]
fn a_picture_that_isnt_there_is_refused_first() {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", 10);
    tools::working(tools::AAC, b"video");
    let s = start(YouTube::new());
    for picture in [dir.path().join("gone.png"), "cover.png".into(), dir.path().to_path_buf()] {
        let (answer, _) = s.publish(&set(), &recording, &[], json!({ "picture": picture }), false);
        let said = format!(
            "the picture setting, {}, isn't a file (locallibrary plugin settings youtube picture)",
            picture.display()
        );
        assert_eq!(kind(&answer.unwrap_err()), ("invalid", said.as_str()));
    }
    assert_eq!(tools::args("ffprobe"), None);
    assert!(s.stop().requests.is_empty());
}

/// Without FFmpeg, publishing (or its dry run) says to install it, and nothing goes; ffmpeg or
/// ffprobe failing says why in its words, ffmpeg making no video says so, and a recording ffprobe
/// can't read the audio or length of is refused.
#[test]
fn ffmpeg_going_wrong_says_why_and_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.wav", 10);
    let s = start(YouTube::new());
    let publish = |dry_run| s.publish(&set(), &recording, &[], json!({}), dry_run).0.unwrap_err();

    tools::answer("ffprobe", None, tools::WAV, "", 0);
    tools::answer("ffmpeg", None, "", "set.wav: Invalid data found when processing input\n\n", 1);
    let e = publish(false);
    assert_eq!(e.kind(), Some("external"));
    assert!(e.message.starts_with("ffmpeg failed (exit status: 1"), "{e:?}");
    assert!(e.message.ends_with("): set.wav: Invalid data found when processing input"), "{e:?}");
    assert!(!s.scratch.path().join("video.mp4").exists());

    // Saying it went well, but having written nothing.
    for writes in [None, Some(&b""[..])] {
        tools::answer("ffmpeg", writes, tools::PROGRESS, "", 0);
        assert_eq!(kind(&publish(false)), ("external", "ffmpeg made no video"));
    }

    tools::answer("ffprobe", None, "", "set.wav: Invalid data found when processing input", 1);
    assert!(publish(true).message.ends_with("): set.wav: Invalid data found when processing input"));
    let refusals = [
        (r#"{"streams": [], "format": {"duration": "25.0"}}"#, "set.wav has no audio FFmpeg can read"),
        (
            r#"{"streams": [{"codec_name": "mp3"}], "format": {"duration": "N/A"}}"#,
            "FFmpeg can't tell how long set.wav is",
        ),
        (r#"{"streams": [{"codec_name": "mp3"}], "format": {}}"#, "FFmpeg can't tell how long set.wav is"),
    ];
    for (probed, said) in refusals {
        tools::answer("ffprobe", None, probed, "", 0);
        assert_eq!(kind(&publish(true)), ("invalid", said));
    }
    tools::answer("ffprobe", None, "{", "", 0);
    assert!(publish(true).message.starts_with("ffprobe's answer can't be read: "));

    tools::uninstalled();
    for dry_run in [true, false] {
        let e = publish(dry_run);
        let said = "ffprobe isn't installed: YouTube takes a video, which FFmpeg makes from the recording. Install it \
                    (https://ffmpeg.org/download.html) and publish again";
        assert_eq!(kind(&e), ("not_found", said));
    }
    assert!(s.stop().requests.is_empty());
}

/// FFmpeg's tools ending in a way that can't be waited for (here because the process ignores
/// SIGCHLD, so the system reaps them itself) fails the probe and the render, saying so.
#[test]
fn a_tool_that_cant_be_waited_for_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.wav", 10);
    tools::working(tools::WAV, b"video");
    let audio = crate::render::probe(&recording).unwrap();
    // SAFETY: nextest gives each test a process of its own, and nothing else in it runs a child.
    unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
    let e = crate::render::probe(&recording).unwrap_err();
    assert!(e.message.starts_with("ffprobe: "), "{e:?}");
    let ctx = loclib_progress::Ctx::new(&loclib_progress::NoProgress, loclib_progress::CancelToken::new());
    let to = dir.path().join("video.mp4");
    let e = crate::render::render(&recording, &audio, 10, None, &to, &ctx).unwrap_err();
    assert!(e.message.starts_with("ffmpeg: "), "{e:?}");
    // SAFETY: as above.
    unsafe { libc::signal(libc::SIGCHLD, libc::SIG_DFL) };
}

/// A dry run reads how long the recording is, says what would go and what YouTube does with it,
/// and neither makes the video nor sends anything.
#[test]
fn a_dry_run_makes_and_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", 10);
    let picture = tools::file(dir.path(), "cover.jpg", 10);
    tools::working(tools::AAC, b"video");
    let s = start(YouTube::new());
    let untitled = PublishedSet { title: None, ..set() };
    let tracklist = [entry(Some("A"), Some("B"), Some(1)), entry(None, None, Some(12)), entry(None, None, Some(20))];
    let (answer, events) = s.publish(&untitled, &recording, &tracklist, json!({ "privacy": "unlisted" }), true);
    let lines = [
        "set.m4a (00:25) would go to YouTube as the unlisted video \"Warehouse, 2026-10-03\", a black frame over the \
         audio, its 3 tracks in the description (2 chapters)",
        "left the time off 1 of the tracks: YouTube's chapters are in order, each at least 10 seconds long",
        "YouTube shows chapters once there are 3, and the description has 2",
        CONTENT_ID,
    ];
    assert_eq!(answer.unwrap(), Published { warnings: lines.map(String::from).to_vec(), ..Published::default() });
    assert!(events.is_empty(), "{events:?}");

    let long = r#"{"streams": [{"codec_name": "aac"}], "format": {"duration": "3723.5"}}"#;
    tools::answer("ffprobe", None, long, "", 0);
    let settings = json!({ "privacy": "private", "picture": picture });
    let untimed = [entry(Some("A"), Some("B"), None), entry(Some("C"), Some("D"), None)];
    let (answer, _) = s.publish(&set(), &recording, &untimed, settings, true);
    let lines = [
        "set.m4a (1:02:03) would go to YouTube as the private video \"Closing set\", cover.jpg over the audio, \
         its 2 tracks in the description",
        "YouTube takes videos over 15 minutes from verified accounts only (youtube.com/verify)",
        CONTENT_ID,
    ];
    assert_eq!(answer.unwrap().warnings, lines);
    assert_eq!(tools::args("ffmpeg"), None);
    assert!(!s.scratch.path().join("video.mp4").exists());
    assert!(s.stop().requests.is_empty());
}

/// A chunk that doesn't arrive, or that YouTube fails, is followed by asking YouTube how much it
/// has, and the upload goes on from there, also when YouTube kept only part of a chunk, or none.
#[test]
fn an_upload_goes_on_from_where_youtube_got_to() {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", 10);
    let made = std::fs::read(tools::file(dir.path(), "made.mp4", 3 * upload::CHUNK)).unwrap();
    tools::working(tools::AAC, &made);
    let half = upload::CHUNK / 2;
    let s = start(YouTube::scripted(move |kind, before| match (kind, before) {
        ("chunk", 1) => Some(youtube::Reply::Unreachable),
        ("chunk", 2) => Some(youtube::Reply::Keeps(half)),
        ("chunk", 3) => Some(youtube::Reply::Keeps(0)),
        ("status", 0) => Some(youtube::refusal(503, "backendError", "try again")),
        _ => None,
    }));
    let (answer, _) = s.publish(&set(), &recording, &[], json!({}), false);
    let r = s.stop();
    assert_eq!(answer.unwrap().url.as_deref(), Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
    assert_eq!(r.video, made);
    let c = upload::CHUNK;
    // The second didn't arrive, and of the second's next try YouTube kept half: the chunk after
    // starts where it got to, YouTube having said so in its 308. Of that one it kept nothing, so
    // it's asked how much it has, and the chunk goes again.
    assert_eq!(r.chunks, [(0, c), (c, c), (c + half, c), (c + half, c), (2 * c + half, half)]);
    let kinds: Vec<&str> = r.requests.iter().map(|(k, ..)| k.as_str()).collect();
    let asked = ["start", "chunk", "chunk", "status", "status", "chunk", "chunk", "status", "chunk", "chunk"];
    assert_eq!(kinds, asked);
}

/// The second chunk failing, then YouTube failing to say how much it has twice more, is four
/// failures in a row: the upload stops there, saying why, and the video is gone.
#[test]
fn an_upload_failing_again_and_again_stops() {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", 10);
    let made = std::fs::read(tools::file(dir.path(), "made.mp4", 2 * upload::CHUNK)).unwrap();
    tools::working(tools::AAC, &made);
    let s = start(YouTube::scripted(|kind, before| match (kind, before) {
        ("chunk", 0) => None,
        ("chunk" | "status", _) => Some(youtube::refusal(503, "backendError", "Backend Error")),
        _ => None,
    }));
    let (answer, _) = s.publish(&set(), &recording, &[], json!({}), false);
    let mp4 = s.scratch.path().join("video.mp4");
    let r = s.stop();
    let e = answer.unwrap_err();
    assert_eq!(kind(&e), ("external", "YouTube failed the video (503 backendError: Backend Error)"));
    let kinds: Vec<&str> = r.requests.iter().map(|(k, ..)| k.as_str()).collect();
    assert_eq!(kinds, ["start", "chunk", "chunk", "status", "status", "status"]);
    assert_eq!(upload::TRIES, 4);
    assert!(!mp4.exists());
}

/// What YouTube answers a request with in the property below, in place of going on as it would:
/// nothing (going on), not being reached, a server error, or keeping only some of a chunk (none
/// of it fails too).
#[hegel::test_helper]
fn reply(tc: &TestCase, chunk: bool) -> Option<youtube::Reply> {
    use youtube::Reply;
    match tc.draw(gs::integers::<u8>().max_value(if chunk { 5 } else { 4 })) {
        0 => Some(Reply::Unreachable),
        1 => Some(youtube::refusal(503, "backendError", "Backend Error")),
        5 => Some(Reply::Keeps(tc.draw(gs::integers::<u64>().max_value(upload::CHUNK)))),
        _ => None,
    }
}

/// How YouTube may refuse to start a session in the property below, and what the publish fails
/// as then.
const REFUSALS: [(u16, &str, &str); 4] = [
    (401, "authError", "permission"),
    (403, "quotaExceeded", "external"),
    (400, "invalidTitle", "invalid"),
    (503, "backendError", "external"),
];

/// Whatever the set, recording, picture and privacy, a dry run makes and sends nothing, and
/// YouTube refusing to start fails the publish as what it is. Otherwise, whatever the video's
/// size, and however its chunks and YouTube's answers about how much it has fail along the way,
/// the video FFmpeg made of the recording arrives whole, with the set's details, each chunk going
/// on from where YouTube got to, and the publish answers its page; unless requests fail four times
/// in a row with no chunk arriving between, which stops it there, saying why.
// Each case runs both stand-ins, a tenth of a second on macOS (a shell and the programs it runs
// for each): 50 keep it well inside the plugin tests' minute and a half under coverage.
#[hegel::test(test_cases = 50)]
fn any_publish_arrives_whole_or_stops_saying_why(tc: TestCase) {
    use std::sync::{LazyLock, Mutex, PoisonError};
    let size = tc.draw(gs::integers::<u64>().min_value(1).max_value(6 * upload::CHUNK));
    let chunks: Vec<Option<youtube::Reply>> =
        (0..tc.draw(gs::integers::<usize>().max_value(24))).map(|_| reply(&tc, true)).collect();
    let checks: Vec<Option<youtube::Reply>> =
        (0..tc.draw(gs::integers::<usize>().max_value(8))).map(|_| reply(&tc, false)).collect();
    let dry_run = tc.draw(gs::weighted_booleans(0.1));
    let refused = tc.draw(gs::weighted_booleans(0.2)).then(|| tc.draw(gs::sampled_from(REFUSALS.to_vec())));
    let aac = tc.draw(gs::booleans());
    let privacy = tc.draw(gs::sampled_from(vec!["private", "unlisted", "public"]));
    let tracklist = super::tracklist(&tc, 6);
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", 10);
    let picture = tc.draw(gs::booleans()).then(|| tools::file(dir.path(), "cover.png", 10));
    let made = std::fs::read(tools::file(dir.path(), "made.mp4", size)).unwrap();
    tools::working(if aac { tools::AAC } else { tools::WAV }, &made);
    let (by_chunk, by_check) = (chunks.clone(), checks.clone());
    let youtube = YouTube::scripted(move |kind, before| match kind {
        "start" => refused.map(|(code, reason, _)| youtube::refusal(code, reason, "no")),
        "chunk" => by_chunk.get(before).cloned().flatten(),
        _ => by_check.get(before).cloned().flatten(),
    });
    // One session for every case: stopping one waits out the SDK's progress flusher, and a failing
    // case shrinks through hundreds of them.
    static SESSION: LazyLock<Mutex<youtube::Session>> = LazyLock::new(|| Mutex::new(start(YouTube::new())));
    let s = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    s.script(youtube);
    let settings = json!({ "privacy": privacy, "picture": picture });
    let (answer, _) = s.publish(&set(), &recording, &tracklist, settings, dry_run);
    let r = s.received();
    drop(s);

    let ffmpeg = tools::args("ffmpeg");
    if dry_run {
        assert_eq!(answer.unwrap().url, None);
        assert_eq!((ffmpeg, r.requests.len()), (None, 0));
        return;
    }
    let args = ffmpeg.expect("ffmpeg made the video");
    assert_eq!(args.contains("\n-c:a\ncopy\n"), aac, "{args}");
    let shown = picture.map_or("color=c=black:s=1920x1080:r=1".into(), |p| p.display().to_string());
    assert!(args.contains(&format!("\n-i\n{shown}\n-i\n{}\n", recording.display())), "{args}");
    if let Some((code, _, kind)) = refused {
        assert_eq!(answer.unwrap_err().kind(), Some(kind));
        // A server error is asked again.
        let tries = if code == 503 { upload::TRIES as usize } else { 1 };
        assert_eq!((r.count("start"), r.requests.len()), (tries, tries));
        return;
    }
    let resource = r.resource.as_ref().unwrap();
    assert_eq!(resource["status"]["privacyStatus"], privacy);
    assert_eq!(resource["snippet"]["description"], video::of(&set(), &tracklist, 25).description);

    // Which requests failed, in order: four in a row, with no chunk arriving between, stop it.
    let (mut seen, mut in_a_row, mut stopped) = ([0, 0], 0, false);
    for (kind, ..) in r.requests.iter().filter(|(k, ..)| k != "start") {
        let (replies, n) = if kind == "chunk" { (&chunks, &mut seen[0]) } else { (&checks, &mut seen[1]) };
        let failed = matches!(
            replies.get(*n),
            Some(Some(youtube::Reply::Unreachable | youtube::Reply::Status(..) | youtube::Reply::Keeps(0)))
        );
        *n += 1;
        assert!(!stopped, "nothing is sent once it has stopped: {:?}", r.requests);
        match (failed, kind.as_str()) {
            (true, _) => in_a_row += 1,
            (false, "chunk") => in_a_row = 0,
            _ => {}
        }
        stopped = in_a_row == upload::TRIES;
    }
    match answer {
        Ok(published) => {
            assert!(!stopped);
            assert_eq!(published.url.as_deref(), Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
            assert_eq!(r.video, made);
        }
        Err(e) => {
            assert!(stopped, "{e:?} {:?}", r.requests);
            assert_eq!(e.kind(), Some("external"), "{e:?}");
            assert!(made.starts_with(&r.video));
        }
    }
}

/// One session for each case of the properties below (stopping one waits out the SDK's progress
/// flusher), scripted afresh by each.
fn shared() -> std::sync::MutexGuard<'static, youtube::Session> {
    use std::sync::{LazyLock, Mutex, PoisonError};
    static SESSION: LazyLock<Mutex<youtube::Session>> = LazyLock::new(|| Mutex::new(start(YouTube::new())));
    SESSION.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whatever the set and settings, a publish that can't make its video says why and sends
/// nothing, leaving no video behind: a picture setting that isn't a file, no scratch folder, a
/// recording that isn't there, FFmpeg not installed or not runnable, ffprobe failing (in its last
/// words) or answering what can't be read, no audio or no length, ffmpeg failing or making no
/// video.
#[hegel::test(test_cases = 1)]
fn prop_a_publish_that_cant_make_its_video_says_why(tc: TestCase) {
    for case in 0..=17 {
        cant_make_its_video(&tc, case);
    }
}

/// [`prop_a_publish_that_cant_make_its_video_says_why`]'s `case` of what goes wrong.
#[hegel::test_helper]
fn cant_make_its_video(tc: &TestCase, case: u8) {
    let dir = tempfile::tempdir().unwrap();
    let mut recording = tools::file(dir.path(), "set.wav", 10);
    let mut dry_run = tc.draw(gs::booleans());
    let mut settings = json!({ "privacy": tc.draw(gs::sampled_from(vec!["private", "unlisted", "public"])) });
    tools::working(tools::WAV, b"video");
    let said_last = |words: &str| format!("): {words}");
    let invalid_data = "set.wav: Invalid data found when processing input";
    // What it fails as: its kind, and its message (or how it starts and ends).
    let (kind_said, starts, ends) = match case {
        0 | 10 | 11 => {
            let picture = match case {
                0 => "cover.png".into(),
                10 => dir.path().join("gone.png"),
                _ => dir.path().to_path_buf(),
            };
            settings["picture"] = json!(picture);
            let said = format!(
                "the picture setting, {}, isn't a file (locallibrary plugin settings youtube picture)",
                picture.display()
            );
            ("invalid", said, String::new())
        }
        1 => {
            recording = dir.path().join("gone.wav");
            dry_run = false;
            ("io", format!("{}: ", recording.display()), String::new())
        }
        2 | 12 | 13 => {
            let (tool, unrunnable) = match case {
                2 => ("ffprobe", false),
                12 => ("ffprobe", true),
                _ => ("ffmpeg", true),
            };
            dry_run &= tool == "ffprobe";
            match unrunnable {
                true => tools::unrunnable(tool),
                false => tools::uninstalled(),
            }
            match unrunnable {
                true => ("external", format!("{tool} couldn't be started: "), String::new()),
                false => {
                    let said = "ffprobe isn't installed: YouTube takes a video, which FFmpeg makes from the recording. \
                                Install it (https://ffmpeg.org/download.html) and publish again";
                    ("not_found", said.into(), String::new())
                }
            }
        }
        3 | 14 => {
            let (err, last) = match case {
                3 => (format!("{invalid_data}\n\n"), invalid_data),
                _ => (" \n".into(), "it said nothing more"),
            };
            tools::answer("ffprobe", None, "", &err, 1);
            ("external", "ffprobe failed (exit status: 1".into(), said_last(last))
        }
        4 => {
            tools::answer("ffprobe", None, "{", "", 0);
            ("external", "ffprobe's answer can't be read: ".into(), String::new())
        }
        5 => {
            tools::answer("ffprobe", None, r#"{"streams": [], "format": {"duration": "25.0"}}"#, "", 0);
            ("invalid", "set.wav has no audio FFmpeg can read".into(), String::new())
        }
        6 | 15 | 16 => {
            let probed = match case {
                6 => r#"{"streams": [{"codec_name": "mp3"}], "format": {"duration": "N/A"}}"#,
                15 => r#"{"streams": [{"codec_name": "mp3"}], "format": {"duration": "0"}}"#,
                _ => r#"{"streams": [{"codec_name": "mp3"}]}"#,
            };
            tools::answer("ffprobe", None, probed, "", 0);
            ("invalid", "FFmpeg can't tell how long set.wav is".into(), String::new())
        }
        7 => {
            dry_run = false;
            tools::answer("ffmpeg", None, "", &format!("{invalid_data}\n"), 1);
            ("external", "ffmpeg failed (exit status: 1".into(), said_last(invalid_data))
        }
        8 | 17 => {
            dry_run = false;
            let writes = (case == 17).then_some(&b""[..]);
            // Its progress not going forward, which isn't reported twice.
            tools::answer("ffmpeg", writes, "out_time_us=0\nout_time_us=0\nprogress=end\n", "", 0);
            ("external", "ffmpeg made no video".into(), String::new())
        }
        _ => {
            let s = shared();
            let req = json!({ "destination": "video", "set": set(), "recording": recording, "tracklist": [],
                "context": { "scratch": null } });
            let e = s.host.request::<Published>(method::SET_PUBLISH, &req).unwrap_err();
            assert_eq!(e.message, "invalid params: no scratch folder to make the video in");
            assert_eq!((tools::args("ffmpeg"), s.received().requests.len()), (None, 0));
            return;
        }
    };
    let s = shared();
    s.script(YouTube::new());
    let (answer, _) = s.publish(&set(), &recording, &[], settings, dry_run);
    tools::restore();
    let e = answer.unwrap_err();
    assert_eq!(e.kind(), Some(kind_said), "{e:?}");
    assert!(e.message.starts_with(&starts) && e.message.ends_with(&ends), "{e:?}");
    assert_eq!(s.received().requests.len(), 0, "nothing is sent");
    assert!(!s.scratch.path().join("video.mp4").exists());
}

/// YouTube refusing says why in its words, and what to do where there's something to do: an
/// account without a channel, a quota or limit used up, details it won't take, an upload it
/// dropped. A token it doesn't take (which locallibrary refreshes or signs out of before it gets
/// here) is refused as it says, and so is an account the host has signed out of, sending
/// nothing. What may go through another time is asked again, up to a few times: a server error,
/// YouTube out of reach, and a chunk of which it keeps nothing; nothing else is.
#[hegel::test(test_cases = 1)]
fn prop_youtube_refusing_says_why(tc: TestCase) {
    use youtube::{Reply, refusal};
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", 10);
    let details = "YouTube refused the video's details";
    let cases = [
        (
            "start",
            refusal(401, "authError", "Invalid Credentials"),
            "permission",
            format!("{details} (401 authError: Invalid Credentials)"),
        ),
        (
            "start",
            refusal(401, "youtubeSignupRequired", "Unauthorized"),
            "permission",
            format!(
                "{details} (401 youtubeSignupRequired: Unauthorized): the Google account has no YouTube channel: \
                 make one and publish again"
            ),
        ),
        (
            "start",
            refusal(403, "quotaExceeded", "The request cannot be completed because you have exceeded your quota."),
            "external",
            format!(
                "{details} (403 quotaExceeded: The request cannot be completed because you have exceeded your \
                 quota.): Local Network's share of the YouTube API is used up for today: publish again tomorrow"
            ),
        ),
        (
            "start",
            refusal(400, "uploadLimitExceeded", "The channel's daily video upload limit has been reached."),
            "external",
            format!(
                "{details} (400 uploadLimitExceeded: The channel's daily video upload limit has been reached.): the \
                 channel has uploaded as many videos as it may today"
            ),
        ),
        (
            "start",
            refusal(403, "rateLimitExceeded", "Too many requests"),
            "external",
            format!(
                "{details} (403 rateLimitExceeded: Too many requests): YouTube is taking too many requests: publish \
                 again in a while"
            ),
        ),
        (
            "start",
            refusal(400, "invalidTitle", "The request metadata specifies an invalid or empty video title."),
            "invalid",
            format!("{details} (400 invalidTitle: The request metadata specifies an invalid or empty video title.)"),
        ),
        (
            "start",
            refusal(403, "forbidden", "Access forbidden."),
            "permission",
            format!("{details} (403 forbidden: Access forbidden.)"),
        ),
        ("start", Reply::Status(500, "<html>".into()), "external", "YouTube failed the video's details (500)".into()),
        ("start", Reply::Status(200, String::new()), "external", "YouTube didn't say where to send the video".into()),
        (
            "start",
            Reply::Status(400, json!({ "error": { "message": "Bad Request" } }).to_string()),
            "invalid",
            format!("{details} (400: Bad Request)"),
        ),
        (
            "start",
            Reply::Status(429, "<html>".into()),
            "external",
            format!("{details} (429): YouTube is taking too many requests: publish again in a while"),
        ),
        ("start", Reply::Unreachable, "external", "connection reset".into()),
        ("start", Reply::Fails("access"), "access", "the host failed it".into()),
        (
            "chunk",
            refusal(404, "notFound", "Not Found"),
            "external",
            "YouTube refused the video (404 notFound: Not Found): the upload's session expired: publish again".into(),
        ),
        (
            "chunk",
            Reply::Status(201, "{}".into()),
            "external",
            "YouTube's answer can't be read: missing field `id` at line 1 column 2".into(),
        ),
        (
            "chunk",
            refusal(500, "backendError", "Backend Error"),
            "external",
            "YouTube failed the video (500 backendError: Backend Error)".into(),
        ),
        ("chunk", Reply::Keeps(0), "external", "YouTube kept no more than 0 bytes of the video".into()),
        ("chunk", Reply::Fails("access"), "access", "the host failed it".into()),
    ];
    let size = tc.draw(gs::integers::<u64>().min_value(1).max_value(100));
    let video = std::fs::read(tools::file(dir.path(), "made.mp4", size)).unwrap();
    let s = start(YouTube::new());
    for (at, reply, kind_said, said) in cases {
        let tries = matches!(reply, Reply::Status(500..=599, _) | Reply::Unreachable | Reply::Keeps(0));
        tools::working(tools::AAC, &video);
        let replying = reply.clone();
        s.script(YouTube::scripted(move |kind, _| (kind == at).then(|| replying.clone())));
        let e = s.publish(&set(), &recording, &[], json!({}), false).0.unwrap_err();
        let r = s.received();
        assert_eq!(kind(&e), (kind_said, said.as_str()), "{reply:?}");
        assert_eq!(r.count(at), if tries { upload::TRIES as usize } else { 1 }, "{reply:?}");
        assert!(!s.scratch.path().join("video.mp4").exists());
    }

    // Signed out, the host's error is the answer, and nothing goes.
    tools::working(tools::AAC, &video);
    s.script(YouTube::signed_out());
    let e = s.publish(&set(), &recording, &[], json!({}), false).0.unwrap_err();
    assert_eq!(kind(&e), ("access", youtube::SIGNED_OUT));
    assert!(s.stop().requests.is_empty());
}

/// Cancelled wherever it is, making the video, between chunks, or waiting to ask YouTube again
/// (to start the upload, or how much it has), the publish answers `cancelled` and sends nothing
/// more. What ffmpeg says once the publish has answered (a process it left behind) goes nowhere,
/// and the next publish goes as it would.
#[hegel::test(test_cases = 2)]
fn prop_a_cancel_stops_the_publish_wherever_it_is(tc: TestCase) {
    use youtube::Reply;
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", 1000);
    let made = std::fs::read(tools::file(dir.path(), "made.mp4", 3 * upload::CHUNK)).unwrap();
    for case in 0..=4 {
        tools::working(tools::AAC, &made);
        // A session of its own: what the plugin sent before the cancel reached it goes to it,
        // not to the next.
        let s = start(YouTube::new());
        let (at, n, then): (&str, usize, _) = match case {
            0 | 4 => {
                cancel_while_ffmpeg_makes_the_video(&s, &recording, &made, case == 4);
                s.stop();
                continue;
            }
            // Answered as YouTube would: between chunks.
            1 => ("chunk", tc.draw(gs::integers::<usize>().max_value(1)), None),
            // Out of reach: waiting to ask again.
            2 => ("chunk", tc.draw(gs::integers::<usize>().max_value(1)), Some(Box::new(Reply::Unreachable))),
            _ => ("start", 0, Some(Box::new(Reply::Unreachable))),
        };
        let cancels = Reply::Cancels(then);
        s.script(YouTube::scripted(move |kind, before| (kind == at && before == n).then(|| cancels.clone())));
        let (answer, _) = s.publish(&set(), &recording, &[], json!({}), false);
        assert_eq!(answer.unwrap_err(), RpcError::cancelled());
        // The plugin may have sent one more before the cancel reached it, but nothing after that.
        let r = s.stop();
        assert!((n + 1..=n + 2).contains(&r.count(at)), "{:?}", r.requests);
    }
}

/// [`prop_a_cancel_stops_the_publish_wherever_it_is`] while ffmpeg makes the video of
/// `recording` (`made`), on `s`: ffmpeg stopped, and `lingering`, a process it left behind
/// saying how far it got once the publish has answered.
fn cancel_while_ffmpeg_makes_the_video(s: &youtube::Session, recording: &Path, made: &[u8], lingering: bool) {
    tools::stall("ffmpeg");
    if lingering {
        tools::after("ffmpeg", "out_time_us=20000000\n");
    }
    let pending = s.send(&set(), recording, &[], json!({}), false);
    // Once ffmpeg has said how far it has got, it's running.
    let making = |e: &Event| matches!(e, Event::Bytes { phase: Phase::Write, .. });
    let deadline = Instant::now() + Duration::from_secs(30);
    while !youtube::events(&pending).iter().any(making) {
        assert!(Instant::now() < deadline, "ffmpeg never said how far it got");
        assert!(pending.wait_timeout::<Published>(Duration::from_millis(20)).is_none(), "it waits for ffmpeg");
    }
    pending.cancel();
    let answer = pending.wait_timeout::<Published>(Duration::from_secs(30)).expect("an answer");
    assert_eq!(answer.unwrap_err(), RpcError::cancelled());
    // What it reported up to its answer, which came after it.
    youtube::events(&pending);
    if lingering {
        let said = Instant::now();
        while !tools::written("ffmpeg") {
            assert!(said.elapsed() < Duration::from_secs(10), "what ffmpeg left behind never said anything");
            std::thread::sleep(Duration::from_millis(10));
        }
        let late = youtube::events(&pending);
        assert!(late.is_empty(), "no progress once it has answered: {late:?}");
    }
    assert_eq!(s.received().requests.len(), 0);
    tools::working(tools::AAC, made);
    let (answer, _) = s.publish(&set(), recording, &[], json!({}), false);
    assert_eq!(answer.unwrap().url.as_deref(), Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
}

/// Whoever may watch the video as the setting says, YouTube making it otherwise (or saying
/// nothing of it) is said only when it did, with where to change it; the answer is the video's
/// page either way.
#[hegel::test(test_cases = 1)]
fn prop_youtube_making_the_video_otherwise_is_said(tc: TestCase) {
    let privacies = ["private", "unlisted", "public"];
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", tc.draw(gs::integers::<u64>().min_value(1).max_value(100)));
    let s = start(YouTube::new());
    for asked in privacies {
        for made in privacies.map(Some).into_iter().chain([None]) {
            tools::working(tools::AAC, b"video");
            s.script(YouTube::scripted(move |kind, _| {
                let video = json!({ "id": "abc", "status": { "privacyStatus": made } });
                (kind == "chunk").then(|| youtube::Reply::Status(201, video.to_string()))
            }));
            let published = s.publish(&set(), &recording, &[], json!({ "privacy": asked }), false).0.unwrap();
            assert_eq!(published.url.as_deref(), Some("https://www.youtube.com/watch?v=abc"));
            let mut warnings = Vec::new();
            if let Some(made) = made.filter(|m| *m != asked) {
                warnings
                    .push(format!("YouTube made the video {made} rather than {asked}: change it in YouTube Studio"));
            }
            warnings.push(PROCESSING.to_string());
            assert_eq!(published.warnings, warnings);
        }
    }
    s.stop();
}

/// Whatever the tracklist, recording and picture, a dry run says what would go: the recording,
/// its length, the title, what's shown over the audio, and how many tracks (and chapters, when
/// there are any) the description has; what was cut or left out; that YouTube wants an account
/// verified for a video over 15 minutes; and what Content ID does.
#[hegel::test(test_cases = 40)]
fn prop_a_dry_run_says_what_would_go(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let recording = tools::file(dir.path(), "set.m4a", 10);
    let picture = tc.draw(gs::booleans()).then(|| tools::file(dir.path(), "cover.jpg", 10));
    let seconds = tc.draw(gs::sampled_from(vec![25u32, 900, 901, 3723]));
    tools::answer(
        "ffprobe",
        None,
        &format!(r#"{{"streams": [{{"codec_name": "aac"}}], "format": {{"duration": "{seconds}"}}}}"#),
        "",
        0,
    );
    let tracklist = super::tracklist(&tc, 5);
    let s = shared();
    s.script(YouTube::new());
    let (answer, _) = s.publish(&set(), &recording, &tracklist, json!({ "picture": picture }), true);
    assert_eq!(s.received().requests.len(), 0);
    let d = video::of(&set(), &tracklist, seconds);
    let shown = if picture.is_some() { "cover.jpg" } else { "a black frame" };
    let mut what = format!(
        "set.m4a ({}) would go to YouTube as the unlisted video \"Closing set\", {shown} over the audio",
        loclib_plugin::clock(seconds)
    );
    match (tracklist.len(), d.chapters) {
        (0, _) => {}
        (n, 0) => what += &format!(", its {n} tracks in the description"),
        (n, c) => what += &format!(", its {n} tracks in the description ({c} chapters)"),
    }
    let mut lines = vec![what];
    lines.extend(d.notes);
    if seconds > 900 {
        lines.push("YouTube takes videos over 15 minutes from verified accounts only (youtube.com/verify)".into());
    }
    lines.push(CONTENT_ID.into());
    assert_eq!(answer.unwrap(), Published { warnings: lines, ..Published::default() });
}
