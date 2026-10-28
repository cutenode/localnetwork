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
/// a try makes it, fails at once on a refusal, and after the last try otherwise; and a dry run, or
/// a recording or picture Mixcloud wouldn't take, sends nothing.
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
        assert_eq!(warnings[1..], [made.notes, vec![MIXCLOUD.into()]].concat());
        assert!(sent.is_empty() && started.is_empty());
        return;
    }
    // Every try is sent: up to the one ending it, else as many as there are.
    let last = tries.len() - 1;
    match (&tries[last], answer) {
        (Try::Made, Ok(published)) => {
            assert_eq!(published, Published { url: Some(PAGE.into()), warnings: made.notes.clone() });
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
        401 => ("permission", "sign in again: locallibrary plugin account mixcloud mixcloud sign-in"),
        _ if limit => ("invalid", "move one to your drafts, or go Pro"),
        403 => ("permission", ")"),
        _ => ("invalid", ")"),
    };
    assert_eq!(e.kind(), Some(expected), "{e:?}");
    assert!(e.message.ends_with(then), "{e:?}");
}
