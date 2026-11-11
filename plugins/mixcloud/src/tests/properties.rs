//! Properties of the hook end to end, through the stand-in for Mixcloud.

use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use super::any_set;
use super::mixcloud::{Mixcloud, PAGE, Part, Reply, error, file, rate_limited, start};
use crate::show;
use crate::upload::{MAX_RETRY_AFTER, TRIES};
use crate::*;

/// What Mixcloud does with a try: makes the show, fails for now (`408`, a server error, the host
/// not reaching it, its rate limit for a few seconds), or refuses it for good, with the kind of
/// error that's said.
#[derive(Clone, Debug)]
enum Try {
    Made,
    Failed(Reply),
    Refused(Reply, &'static str),
}

/// A try Mixcloud fails for now.
#[hegel::test_helper]
fn failed(tc: &TestCase) -> Try {
    Try::Failed(match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => Reply::Unreachable,
        1 => error(tc.draw(gs::sampled_from(vec![408, 500, 502, 503, 504])), "", ""),
        2 => rate_limited(tc.draw(gs::optional(gs::integers::<u64>().max_value(20))), None),
        _ => Reply::Status(429, Vec::new(), String::new()),
    })
}

/// A try that ends the upload: Mixcloud makes the show or refuses it.
#[hegel::test_helper]
fn ending(tc: &TestCase) -> Try {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => Try::Made,
        1 => Try::Refused(error(401, "OAuthException", "An invalid access token was provided."), "permission"),
        _ => {
            let status = tc.draw(gs::sampled_from(vec![400, 404, 413, 422]));
            Try::Refused(error(status, "PostValidationError", "No."), "invalid")
        }
    }
}

/// Whatever the set and its settings and however Mixcloud answers each try, a recording Mixcloud
/// takes goes up as one form: the whole file as `mp3`, the show's fields, then the picture, the
/// same form each try, the upload's progress as much as it sends. It answers the show's page once
/// a try makes it, fails at once on a refusal, and after the last try otherwise; and a dry run
/// (saying the tracklist and tags that would go, when there are any), or a recording or picture
/// Mixcloud wouldn't take, sends nothing.
#[hegel::test(test_cases = 50)]
fn the_form_goes_until_mixcloud_makes_or_refuses_it(tc: TestCase) {
    let (set, tags, tracklist) = any_set(&tc, 12);
    let unlisted = tc.draw(gs::booleans());
    // Mostly a recording Mixcloud takes, published.
    let dry_run = tc.draw(gs::integers::<u8>().max_value(4)) == 0;
    let extension = tc.draw(gs::sampled_from(vec!["mp3", "FLAC", "wma", "m4a", "WAV", "mp4"]));
    let size = tc.draw(gs::integers::<u64>().max_value(2000));
    // Some tries failing, as many as there are at most, then one ending it if there's a try left.
    let failures = tc.draw(gs::integers::<u32>().max_value(TRIES));
    let mut tries: Vec<Try> = (0..failures).map(|_| failed(&tc)).collect();
    if failures < TRIES {
        tries.push(ending(&tc));
    }
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), &format!("set.{extension}"), size);
    // Now and then one too big, sparse so it takes no room.
    let size = match tc.draw(gs::integers::<u8>().max_value(9)) {
        0 => {
            std::fs::File::options().write(true).open(&recording).unwrap().set_len(MAX_BYTES + 1).unwrap();
            MAX_BYTES + 1
        }
        _ => size,
    };
    // No picture, one Mixcloud takes, or one that isn't there, isn't absolute or is too big.
    let (picture, sends_picture) = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 | 1 => (None, true),
        2 => (Some(file(dir.path(), "cover.png", 30)), true),
        3 => (Some(dir.path().join("gone.png")), false),
        4 => (Some("Cargo.toml".into()), false),
        _ => {
            let big = dir.path().join("big.png");
            std::fs::File::create(&big).unwrap().set_len(MAX_PICTURE + 1).unwrap();
            (Some(big), false)
        }
    };
    let mut settings = json!({ "tags": tags, "unlisted": unlisted });
    if let Some(picture) = &picture {
        settings["picture"] = json!(picture);
    }

    let replies = tries.clone();
    let s = start(Mixcloud::scripted(move |n| match replies.get(n) {
        Some(Try::Failed(reply) | Try::Refused(reply, _)) => Some(reply.clone()),
        // A try past the script is made too, and `sent` says it went.
        Some(Try::Made) | None => None,
    }));
    let (answer, started, ended) = s.publish(&set, &recording, &tracklist, settings, dry_run);
    let sent = s.stop();

    let takes = extension != "mp4" && (1..=MAX_BYTES).contains(&size) && sends_picture;
    if !takes {
        assert_eq!(answer.unwrap_err().kind(), Some("invalid"));
        assert!(sent.is_empty() && started.is_empty());
        return;
    }
    let made = show::of(&set, &tracklist, &tags, unlisted);
    if dry_run {
        let warnings = answer.unwrap().warnings;
        assert!(warnings[0].starts_with(&format!("set.{extension} (")), "{warnings:?}");
        // The tracklist and the tags that go, said when there are any.
        let timed = made.sections.iter().filter(|s| s.start.is_some()).count();
        let listed = format!(", its {} tracks as the tracklist ({timed} timed)", made.sections.len());
        assert_eq!(warnings[0].contains(&listed), !made.sections.is_empty(), "{warnings:?}");
        let tagged = format!(", tagged {}", made.tags.join(", "));
        assert_eq!(warnings[0].contains(&tagged), !made.tags.is_empty(), "{warnings:?}");
        assert_eq!(warnings[1..], [made.notes, vec![MIXCLOUD.into()]].concat());
        assert!(sent.is_empty() && started.is_empty());
        return;
    }
    // Every try is sent: up to the one ending it, else as many as there are.
    let last = tries.len() - 1;
    match (&tries[last], answer) {
        (Try::Made, Ok(published)) => {
            assert_eq!(
                published,
                Published {
                    url: Some(PAGE.into()),
                    name: Some("Mixcloud".into()),
                    warnings: made.notes.clone(),
                    ..Published::default()
                }
            );
        }
        (Try::Refused(_, kind), Err(e)) => assert_eq!(e.kind(), Some(*kind), "{e:?}"),
        (Try::Failed(_), Err(e)) => assert_eq!(e.kind(), Some("external"), "{e:?}"),
        (t, answer) => panic!("{t:?} answered {answer:?}"),
    }
    let mut form =
        vec![("mp3".to_string(), Part::File(format!("set.{extension}"), std::fs::read(&recording).unwrap()))];
    form.extend(made.fields().into_iter().map(|f| (f.name, Part::Text(f.value.unwrap()))));
    let pictured = picture.map(|p| std::fs::read(p).unwrap());
    form.extend(pictured.iter().map(|bytes| ("picture".to_string(), Part::File("cover.png".into(), bytes.clone()))));
    assert_eq!(sent, vec![form; last + 1]);
    let bytes = size + pictured.map_or(0, |p| p.len() as u64);
    assert_eq!((started, ended), (vec![bytes; last + 1], last + 1));
}

/// However Mixcloud refuses an upload (its status, its error's type and message, each field's
/// problems, how long its rate limit asks to wait), it's sent again when that's worth it (a
/// timeout, a server error, the rate limit for up to 15 minutes), and otherwise the error says
/// it in Mixcloud's words, its kind saying what's wrong, and what to do when there's something.
#[hegel::test(test_cases = 100)]
fn a_refusal_is_said_in_mixclouds_words(tc: TestCase) {
    let status = tc.draw(gs::sampled_from(vec![400, 401, 403, 404, 408, 413, 429, 500, 503]));
    let kind = tc.draw(gs::sampled_from(vec!["", "RateLimitException", "UploadLimitException", "OAuthException"]));
    let message = tc.draw(gs::sampled_from(vec!["", "No.", "You have reached your upload limit."]));
    let details = tc.draw(gs::optional(gs::sampled_from(vec![
        json!({ "mp3": ["Unsupported.", "Really."] }),
        json!({ "name": "Too long.", "picture": 3 }),
    ])));
    let retry_after = tc.draw(gs::optional(gs::sampled_from(vec![0, 3, MAX_RETRY_AFTER + 1])));
    let mut body = json!({ "error": { "type": kind, "message": message } });
    if let Some(details) = &details {
        body["details"] = details.clone();
    }
    let headers = retry_after.map(|r| ("retry-after", r.to_string())).into_iter().collect();
    let refusal = Reply::Status(status, headers, body.to_string());
    let s = start(Mixcloud::scripted(move |n| (n == 0).then(|| refusal.clone())));
    let dir = tempfile::tempdir().unwrap();
    let recording = file(dir.path(), "set.mp3", 10);
    let (answer, ..) = s.publish(&super::set(), &recording, &[], json!({}), false);
    let sent = s.stop().len();

    let rate_limited = kind == "RateLimitException" || status == 429;
    let waits = retry_after.is_none_or(|r| r <= MAX_RETRY_AFTER);
    if rate_limited && waits || !rate_limited && matches!(status, 408 | 500..) {
        assert_eq!(answer.unwrap().url.as_deref(), Some(PAGE));
        assert_eq!(sent, 2);
        return;
    }
    let e = answer.unwrap_err();
    assert_eq!(sent, 1);
    let verb = if status >= 500 { "failed" } else { "refused" };
    assert!(e.message.starts_with(&format!("Mixcloud {verb} the show ({status}")), "{e:?}");
    assert!(e.message.contains(message), "{e:?}");
    let fields = details.as_ref().and_then(Value::as_object).into_iter().flat_map(|d| d.keys());
    for field in fields {
        assert!(e.message.contains(&format!("; {field}: ")), "{e:?}");
    }
    let limit = format!("{kind} {message}").to_lowercase().contains("limit");
    let (expected, then) = match status {
        _ if rate_limited => ("external", "publish again in 16 minutes"),
        401 => ("permission", ")"),
        _ if limit => ("invalid", "move one to your drafts, or go Pro"),
        403 => ("permission", ")"),
        _ => ("invalid", ")"),
    };
    assert_eq!(e.kind(), Some(expected), "{e:?}");
    assert!(e.message.ends_with(then), "{e:?}");
}

/// Each way a publish can't go says why, sending nothing more: a recording that isn't there, or
/// is empty; a picture too big, without a scratch folder to send it from, or that can't be read;
/// the host failing the upload; Mixcloud failing it every try, answering what can't be read, or
/// refusing it as forbidden. A dry run without a tracklist says none, and names the picture when
/// there's one and the show unlisted when it is; with a picture and nothing in the way, the show
/// is made, the picture sent with it.
#[hegel::test(test_cases = 1)]
fn prop_each_way_a_publish_cant_go_says_why(tc: TestCase) {
    for case in [0, 1, 2, 3, 4, 6, 9, 10, 11, 12, 13, 14, 15] {
        publish_cant_go(&tc, case);
    }
}

/// However Mixcloud says it, an answer naming no show and a field's problems listed in any shape
/// fail the publish saying so, and cancelled while it waits to send it again after any failure,
/// it answers `cancelled`, sending nothing more.
#[hegel::test(test_cases = 4)]
fn prop_how_mixcloud_says_it_doesnt_change_how_a_publish_stops(tc: TestCase) {
    for case in [5, 7, 8] {
        publish_cant_go(&tc, case);
    }
}

/// [`prop_each_way_a_publish_cant_go_says_why`]'s `case`, on a session of its own.
#[hegel::test_helper]
fn publish_cant_go(tc: &TestCase, case: u8) {
    let dir = tempfile::tempdir().unwrap();
    let mut recording = file(dir.path(), "set.mp3", 10);
    let picture = file(dir.path(), "cover.png", 30);
    let (mut settings, mut scratch, mut dry_run, mut every) = (json!({}), true, false, false);
    // Mixcloud's answer to the first upload; what the publish fails as, how its message starts and
    // what it says after; how many uploads went (one more at most, after a cancel).
    let (reply, kind_said, said, then, sent) = match case {
        0 => {
            recording = dir.path().join("gone.mp3");
            (None, "io", format!("{}: ", recording.display()), String::new(), 0)
        }
        1 => {
            (settings, scratch) = (json!({ "picture": picture }), false);
            (
                None,
                "invalid_params",
                "invalid params: no scratch folder to send the picture from".into(),
                String::new(),
                0,
            )
        }
        2 if cfg!(unix) => {
            settings = json!({ "picture": picture });
            #[cfg(unix)]
            std::fs::set_permissions(&picture, std::os::unix::fs::PermissionsExt::from_mode(0o000)).unwrap();
            (None, "io", format!("{}: ", picture.display()), String::new(), 0)
        }
        3 => (Some(Reply::Fails("access")), "access", "the host failed it".into(), String::new(), 1),
        4 => {
            let html = Reply::Status(200, Vec::new(), "<html>".into());
            (Some(html), "external", "Mixcloud's answer can't be read: ".into(), String::new(), 1)
        }
        5 => {
            let key =
                tc.draw(gs::sampled_from(vec![None, Some(""), Some("/djkit/"), Some("/dj kit/set/"), Some("/a/b/c/")]));
            let body = json!({ "result": { "success": true, "key": key } }).to_string();
            let said = format!("Mixcloud's answer named no show: {:?}", key.unwrap_or_default());
            (Some(Reply::Status(200, Vec::new(), body)), "external", said, String::new(), 1)
        }
        6 => {
            let said = "Mixcloud refused the show (403 PermissionException: Forbidden)";
            (Some(error(403, "PermissionException", "Forbidden")), "permission", said.into(), String::new(), 1)
        }
        7 => {
            let problems = tc.draw(gs::sampled_from(vec![json!([1, "Really."]), json!([null]), json!([{ "a": 2 }])]));
            let listed = problems.as_array().unwrap().iter().map(|p| p.as_str().map_or(p.to_string(), String::from));
            let then = format!("; mp3: {})", listed.collect::<Vec<_>>().join(" "));
            let body =
                json!({ "error": { "type": "PostValidationError", "message": "No." }, "details": { "mp3": problems } });
            let refused = Reply::Status(400, Vec::new(), body.to_string());
            (Some(refused), "invalid", "Mixcloud refused the show (400 PostValidationError: No.".into(), then, 1)
        }
        // The one way it goes: the show made, its picture sent from the scratch folder.
        15 => {
            settings = json!({ "picture": picture });
            (None, "", String::new(), String::new(), 1)
        }
        14 => {
            // Sparse: it takes no room.
            std::fs::File::options().write(true).open(&picture).unwrap().set_len(10_000_001).unwrap();
            settings = json!({ "picture": picture });
            let said = "Mixcloud takes pictures of up to 10.0 MB, and cover.png is 10.0 MB";
            (None, "invalid", said.into(), String::new(), 0)
        }
        12 => {
            every = true;
            let failed = "Mixcloud failed the show (503)".to_string();
            (Some(error(503, "", "")), "external", failed, String::new(), TRIES as usize)
        }
        8 => {
            let then = tc.draw(gs::sampled_from(vec![Reply::Unreachable, error(503, "", "")]));
            (Some(Reply::Cancels(Box::new(then))), "cancelled", "operation cancelled".into(), String::new(), 1)
        }
        // A dry run of an unlisted show with a picture, and of a show without either.
        9 | 13 => {
            dry_run = true;
            settings = json!({ "unlisted": case == 9 });
            if case == 9 {
                settings["picture"] = json!(picture);
            }
            (None, "", String::new(), String::new(), 0)
        }
        _ => {
            recording = file(dir.path(), "empty.mp3", 0);
            (None, "invalid", "empty.mp3 is empty".into(), String::new(), 0)
        }
    };
    let s = start(Mixcloud::scripted(move |n| (n == 0 || every).then(|| reply.clone()).flatten()));
    let pending = s.send(&super::set(), &recording, &[], settings.clone(), dry_run, scratch);
    let answer = pending.wait_timeout::<Published>(std::time::Duration::from_secs(60)).expect("an answer");
    let uploads = s.stop().len();
    if dry_run {
        let warnings = answer.unwrap().warnings;
        assert!(!warnings[0].contains("tracklist"), "{warnings:?}");
        assert_eq!(warnings[0].ends_with(", cover.png as its picture"), settings.get("picture").is_some());
        assert_eq!(warnings[0].contains(" as the unlisted show "), settings["unlisted"] == true, "{warnings:?}");
        assert_eq!(uploads, 0);
        return;
    }
    if kind_said.is_empty() {
        assert_eq!((answer.unwrap().url.as_deref(), uploads), (Some(PAGE), sent));
        return;
    }
    let e = answer.unwrap_err();
    assert_eq!(e.kind(), Some(kind_said), "{e:?}");
    assert!(e.message.starts_with(&said) && e.message.ends_with(&then), "{e:?}, not {said:?} ... {then:?}");
    let cancelled = kind_said == "cancelled";
    assert!((sent..=sent + usize::from(cancelled)).contains(&uploads), "{uploads} uploads");
}
