//! Uploading a set's recording to the account (`sets publish SET --upload`), against the pretend
//! data server: the file goes up as a DPoP-bound blob, read from disk as it goes, its record goes
//! under the set's key, and the set's record points at it. Encoding with ffmpeg is the unit
//! tests' (`publish::recording`), and the command's (`apps/locallibrary`).

use std::path::Path;

use chrono::{NaiveDate, Utc};
use locallibrary::api::{self, SetPublishRequest, SetUploadRequest};
use locallibrary::atmosphere::lexicon::RECORDING;
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::publish::recording::Encoding;
use locallibrary::sets::{Recording, Set, SetBook, SetEntry};
use locallibrary::store::Store;
use lofty::config::ParseOptions;
use lofty::id3::v2::Frame;
use lofty::mpeg::MpegFile;
use lofty::prelude::*;
use serde_json::json;
use sha2::Digest;

use super::{DID, LATER, PUBLISHING, base64url, network, sign_in};

const UPLOAD: &str = "com.atproto.repo.uploadBlob";
const ID: &str = "3m2abcdefgh22";

/// Ten silent MP3 frames at a time: `frames` of them play for about 26 ms each.
fn mp3(frames: usize) -> Vec<u8> {
    [&[0xff, 0xfb, 0x90, 0x64][..], &[0; 413]].concat().repeat(frames)
}

/// A set in the store at `root` whose recording is `recording` (written there, as `name`, when it's
/// given).
fn save_set(root: &Path, name: &str, recording: Option<&[u8]>) -> std::path::PathBuf {
    // Beside the store, which takes nothing else in its folder.
    let file = root.with_extension("recordings").join(name);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    if let Some(bytes) = recording {
        std::fs::write(&file, bytes).unwrap();
    }
    let at = Utc::now();
    let set = Set {
        id: ID.into(),
        title: Some("Friday".into()),
        date: NaiveDate::from_ymd_opt(2026, 10, 3).unwrap(),
        recording: recording.map(|_| Recording { path: file.clone(), report: None }),
        tracks: vec![SetEntry::added("Bicep - Glue")],
        created_at: at,
        updated_at: at,
        ..Default::default()
    };
    let store = Store::open_or_init(root).unwrap();
    let mut book = SetBook::load(&store).unwrap();
    book.sets.push(set);
    book.save(&store).unwrap();
    file
}

fn upload_request() -> SetUploadRequest {
    SetUploadRequest { set: ID.into(), max_bytes: None, hide: vec![] }
}

/// An MP3 goes up with the set's timed tracks as ID3 chapters, named as the set's record names
/// them (`--hide` too), each ending where the next starts, and the last where the recording does.
/// The plan lists them, and says which tracks get none.
#[test]
fn an_mp3_goes_up_with_its_tracks_as_chapters() {
    let pds = network();
    pds.set_account(DID, "dj.example.com");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    let audio = mp3(200);
    let file = save_set(&root, "friday.mp3", Some(&audio));
    let store = Store::open(&root).unwrap();
    let mut book = SetBook::load(&store).unwrap();
    let timed = |name: &str, start: Option<u32>| SetEntry { start_seconds: start, ..SetEntry::added(name) };
    book.sets[0].tracks = vec![
        timed("Bicep - Glue", Some(0)),
        timed("Floorplan - Never Grow Old", Some(3)),
        timed("Late - Track", Some(60)),
        timed("Untimed - Track", None),
    ];
    book.save(&store).unwrap();

    let plan = api::set_upload_plan(&root, &SetUploadRequest { hide: vec![2], ..upload_request() }).unwrap();
    let titles: Vec<(u32, &str)> = plan.chapters.iter().map(|c| (c.start, c.title.as_str())).collect();
    assert_eq!(titles, [(0, "Bicep - Glue"), (3, "ID - ID")]);
    assert_eq!(
        plan.notes[..2],
        [
            "track 4 has no start time, so no chapter",
            "track 3 starts at 0:01:00, after the recording ends, so no chapter"
        ]
    );
    plan.apply(None, &Ctx::silent()).unwrap();

    let (record, _) = pds.record(DID, RECORDING, ID).unwrap();
    let (mime, sent) = pds.blob(record["audio"]["ref"]["$link"].as_str().unwrap()).unwrap();
    assert_eq!(mime, "audio/mpeg");
    assert!(sent.ends_with(&audio) && sent.len() > audio.len(), "the same audio, after a tag");
    assert_eq!(record["audio"]["size"].as_u64(), Some(sent.len() as u64));
    let read = MpegFile::read_from(&mut std::io::Cursor::new(&sent), ParseOptions::new()).unwrap();
    let chapters: Vec<_> = read
        .id3v2()
        .unwrap()
        .iter()
        .filter_map(|f| match f {
            Frame::Chapter(c) => Some((c.times.clone(), c.title().unwrap().to_string())),
            _ => None,
        })
        .collect();
    assert_eq!(chapters, [(0..3000, "Bicep - Glue".to_string()), (3000..5000, "ID - ID".to_string())]);
    assert_eq!(std::fs::read(&file).unwrap(), audio, "the recording itself is left alone");
}

/// An MP3 goes up as it is: the server is asked who's signed in first (which refreshes the session
/// and learns its DPoP nonce), then gets the file's bytes as audio/mpeg, DPoP-bound. Its record goes
/// under the set's key, linking the blob, and the set keeps it, so the set's record points at it.
/// Uploading it again writes nothing new.
#[test]
fn a_recording_goes_up_as_a_blob_and_the_sets_record_points_at_it() {
    let pds = network();
    pds.set_account(DID, "dj.example.com");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let key = sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    let audio = mp3(40);
    save_set(&root, "friday.mp3", Some(&audio));

    let plan = api::set_upload_plan(&root, &upload_request()).unwrap();
    let uri = format!("at://{DID}/{RECORDING}/{ID}");
    assert_eq!(plan.uri.as_deref(), Some(uri.as_str()));
    assert_eq!((plan.encoding, plan.audio.playable.as_deref()), (Encoding::AsIs, Some("audio/mpeg")));
    assert!(plan.current.is_none() && plan.missing_scope.is_none());
    assert!(
        plan.summary[0].starts_with(&format!("upload friday.mp3 as it is (16.7 KB) as {DID}")),
        "{:?}",
        plan.summary
    );
    assert!(pds.calls(UPLOAD).is_empty(), "planning uploads nothing");

    let published = plan.apply(None, &Ctx::silent()).unwrap();
    assert_eq!((published.uri.as_str(), published.written), (uri.as_str(), true));
    // Asked who's signed in first, then told the nonce once, then uploaded with it.
    let names: Vec<String> = pds.requests().iter().map(|r| r.path.clone()).collect();
    let first_upload = names.iter().position(|p| p.ends_with(UPLOAD)).unwrap();
    assert!(names[..first_upload].iter().any(|p| p.ends_with("com.atproto.server.getSession")), "{names:?}");
    let uploads = pds.calls(UPLOAD);
    assert_eq!(uploads.len(), 2, "told to use the server's nonce, sent again");
    let up = &uploads[1];
    assert_eq!((up.header("content-type"), up.body.as_slice()), (Some("audio/mpeg"), audio.as_slice()));
    assert_eq!(up.header("content-length"), Some(audio.len().to_string().as_str()));
    assert_eq!(up.header("authorization"), Some("DPoP access-token"));
    let (header, claims) = up.dpop().unwrap();
    assert_eq!((&header["jwk"]["x"], &header["jwk"]["y"]), (&key["x"], &key["y"]), "bound to the session's key");
    assert_eq!(
        (claims["htm"].as_str(), claims["htu"].as_str()),
        (Some("POST"), Some(&*format!("{}/xrpc/{UPLOAD}", pds.url)))
    );
    assert_eq!(claims["ath"], base64url(&sha2::Sha256::digest(b"access-token")));
    assert_eq!(claims["nonce"], "nonce-1");

    let (record, cid) = pds.record(DID, RECORDING, ID).unwrap();
    assert_eq!(published.cid, cid);
    let link = record["audio"]["ref"]["$link"].as_str().unwrap();
    assert_eq!(pds.blob(link), Some(("audio/mpeg".to_string(), audio.clone())));
    assert_eq!((record["audio"]["size"].as_u64(), &record["durationSeconds"]), (Some(audio.len() as u64), &json!(1)));

    // The set keeps the record, and its record points at it.
    let set_plan = api::set_publish_plan(&root, &SetPublishRequest { set: ID.into(), hide: vec![] }).unwrap();
    assert_eq!(set_plan.record["recording"], json!({ "uri": uri, "cid": cid }));

    // The same file again: uploaded, but the record says what it said, so it isn't written.
    let again = api::set_upload_plan(&root, &upload_request()).unwrap();
    assert!(again.current.is_some() && again.summary[0].ends_with("replacing the recording there"));
    let published = again.apply(None, &Ctx::silent()).unwrap();
    assert_eq!((published.written, published.cid.as_str()), (false, cid.as_str()));
}

/// What stops an upload says why, and what to do: no recording on this computer, a session
/// without permission to upload (before anything's sent), and a data server that won't take a file
/// that big, naming its size.
#[test]
fn an_upload_that_cant_go_says_why() {
    let pds = network();
    pds.set_account(DID, "dj.example.com");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");

    // No recording on the set, or one that isn't there any more.
    save_set(&root, "friday.mp3", None);
    let err = api::set_upload_plan(&root, &upload_request()).unwrap_err();
    assert_eq!(err.code(), "not_found");
    assert!(err.to_string().contains(&format!("`sets edit {} --recording FILE`", &ID[..8])), "{err}");

    let other = dir.path().join("other");
    let file = save_set(&other, "friday.mp3", Some(&mp3(40)));
    // Signed out, it can be looked at.
    let plan = api::set_upload_plan(&other, &upload_request()).unwrap();
    assert!(plan.account.is_none() && plan.summary[0].starts_with("friday.mp3 would be uploaded as it is"));
    std::fs::remove_file(&file).unwrap();
    assert_eq!(api::set_upload_plan(&other, &upload_request()).unwrap_err().code(), "not_found");

    // Signed in allowing publishing, but from before uploads were asked for.
    let file = save_set(&dir.path().join("third"), "friday.mp3", Some(&mp3(40)));
    let root = dir.path().join("third");
    let without_uploads = PUBLISHING.replace(" blob:audio/*", "");
    sign_in(&root, &pds.url, &without_uploads, LATER, Some("refresh-1"));
    let plan = api::set_upload_plan(&root, &upload_request()).unwrap();
    assert_eq!(plan.missing_scope.as_deref(), Some("blob:audio/*"));
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::MissingScope));
    assert!(err.to_string().contains("sign in again, allowing publishing"), "{err}");
    assert!(pds.calls(UPLOAD).is_empty());

    // A data server that takes less than the file: the error names the size sent.
    sign_in(&root, &pds.url, PUBLISHING, LATER, Some("refresh-1"));
    pds.set_blob_limit(1000);
    let plan = api::set_upload_plan(&root, &upload_request()).unwrap();
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    let size = std::fs::metadata(&file).unwrap().len();
    assert_eq!(size, 16_680);
    assert!(err.to_string().contains("refused the recording: 16.7 KB is more than it takes"), "{err}");
    assert!(err.to_string().contains("`--max-mib`"), "{err}");
    assert!(pds.record(DID, RECORDING, ID).is_none(), "no record without the file");
    let store = Store::open(&root).unwrap();
    assert!(SetBook::load(&store).unwrap().sets[0].recording_record.is_none());
}

/// Over recordings (an MP3 that may go up as it is, or a WAV that has to be encoded), limits,
/// sessions with and without permission to upload, what ffmpeg (a stand-in) makes of them and data
/// servers that take less than they're sent: the plan goes up as it is exactly when browsers play
/// the recording and it fits. A session that may not upload sends nothing; an encoding over the
/// limit isn't sent and a file over the server's is refused, each saying its size, with no record
/// and the set as it was. Otherwise the server has the file exactly as it went up, as its type,
/// its record is under the set's key, and the set points at it instead of what it had.
#[cfg(unix)]
#[hegel::test(test_cases = 25)]
fn an_upload_goes_as_planned_or_stops_saying_why(tc: hegel::TestCase) {
    use super::{PLYR_TRACK, PLYR_TRACK_CID};
    use hegel::generators as gs;
    use locallibrary::op::bytes;
    use locallibrary::publish::recording::MAX_BYTES;
    use loclib_plugin::StrongRef;
    use std::os::unix::fs::PermissionsExt;
    let pds = network();
    pds.set_account(DID, "dj.example.com");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let is_mp3 = tc.draw(gs::booleans());
    let audio = if is_mp3 {
        mp3(tc.draw(gs::integers::<usize>().min_value(40).max_value(400)))
    } else {
        let secs = tc.draw(gs::integers::<u32>().min_value(1).max_value(4));
        crate::common::media::wav_of(8000, 16, 1, f64::from(secs))
    };
    save_set(&root, if is_mp3 { "friday.mp3" } else { "friday.wav" }, Some(&audio));
    let may_upload = tc.draw(gs::weighted_booleans(0.8));
    let scopes = if may_upload { PUBLISHING.to_string() } else { PUBLISHING.replace(" blob:audio/*", "") };
    sign_in(&root, &pds.url, &scopes, LATER, Some("refresh-1"));
    // Published at a plugin first, the set points at that recording.
    let before = tc.draw(gs::booleans()).then(|| StrongRef { uri: PLYR_TRACK.into(), cid: PLYR_TRACK_CID.into() });
    let store = Store::open(&root).unwrap();
    let mut book = SetBook::load(&store).unwrap();
    book.sets[0].recording_record = before.clone();
    book.save(&store).unwrap();
    let pointed_at = || SetBook::load(&store).unwrap().sets[0].recording_record.clone();

    // ffmpeg copies what it's to make into the file it's told to.
    let encoded = vec![7u8; tc.draw(gs::integers::<usize>().min_value(1).max_value(40_000))];
    let (bin, made) = (dir.path().join("bin"), dir.path().join("encoded"));
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(&made, &encoded).unwrap();
    let script = format!("#!/bin/sh\nfor last; do :; done\n/bin/cp '{}' \"$last\"\n", made.display());
    std::fs::write(bin.join("ffmpeg"), script).unwrap();
    std::fs::set_permissions(bin.join("ffmpeg"), std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: nextest gives each test a process of its own, and nothing else in it reads the
    // environment meanwhile.
    unsafe { std::env::set_var("PATH", &bin) };
    let server_limit = tc.draw(gs::optional(gs::integers::<usize>().min_value(1).max_value(50_000)));
    if let Some(l) = server_limit {
        pds.set_blob_limit(l);
    }

    let max_bytes = tc.draw(gs::optional(gs::integers::<u64>().min_value(8_000).max_value(2 * audio.len() as u64)));
    let limit = max_bytes.map_or(MAX_BYTES, |m| m.min(MAX_BYTES));
    let as_is = is_mp3 && audio.len() as u64 <= limit;
    let plan = match api::set_upload_plan(&root, &SetUploadRequest { max_bytes, ..upload_request() }) {
        Ok(plan) => plan,
        Err(e) => {
            // Too long to fit the limit even at 64 kbit/s.
            assert!(!as_is && e.to_string().starts_with("the recording is too long to upload"), "{e}");
            return;
        }
    };
    assert_eq!(plan.encoding == Encoding::AsIs, as_is);
    assert_eq!(plan.missing_scope.is_some(), !may_upload);
    let sent = if as_is { &audio } else { &encoded };
    let result = plan.apply(None, &Ctx::silent());
    let uploaded = pds.calls(UPLOAD).len();
    if !may_upload {
        assert_eq!(result.unwrap_err().reason(), Some(Reason::MissingScope));
        assert_eq!(uploaded, 0);
    } else if !as_is && encoded.len() as u64 > limit {
        let err = result.unwrap_err().to_string();
        assert_eq!(
            err,
            format!(
                "ffmpeg's encoding of the recording came to {}, more than {}",
                bytes(encoded.len() as u64),
                bytes(limit)
            )
        );
        assert_eq!(uploaded, 0);
    } else if server_limit.is_some_and(|l| sent.len() > l) {
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains(&format!("refused the recording: {} is more than it takes", bytes(sent.len() as u64))),
            "{err}"
        );
    } else {
        let published = result.unwrap();
        let (record, cid) = pds.record(DID, RECORDING, ID).unwrap();
        assert_eq!(
            (published.uri.as_str(), published.cid.as_str()),
            (format!("at://{DID}/{RECORDING}/{ID}").as_str(), cid.as_str())
        );
        let mime = if as_is { "audio/mpeg" } else { "audio/mp4" };
        let link = record["audio"]["ref"]["$link"].as_str().unwrap();
        assert_eq!(pds.blob(link), Some((mime.to_string(), sent.clone())));
        assert_eq!(pointed_at(), Some(StrongRef { uri: published.uri, cid }));
        return;
    }
    assert!(pds.record(DID, RECORDING, ID).is_none(), "no record without the file");
    assert_eq!(pointed_at(), before);
}
