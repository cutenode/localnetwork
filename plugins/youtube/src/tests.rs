#[cfg(unix)]
mod publishing;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::{Capability, DestinationKind, Manifest, PublishedSet, SettingType, TracklistEntry};
use serde_json::{Value, json};

use super::video::{self, MAX_DESCRIPTION, MAX_TITLE, MIN_CHAPTER, MIN_CHAPTERS};
use super::*;

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

/// It reaches YouTube's API alone, publishes sets at one destination, signs in to Google through
/// Local Network's token exchange asking to upload and nothing else, and keeps who can watch and
/// the picture as settings.
#[test]
fn the_manifest_is_a_built_in_set_destination_on_youtube() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    assert_eq!(manifest.capabilities, [Capability::Net("www.googleapis.com".into())]);
    let [destination] = &manifest.contributes.destinations[..] else { panic!("{manifest:?}") };
    assert_eq!((manifest.id.as_str(), destination.name.as_str()), ("youtube", "video"));
    assert_eq!(destination.kind, DestinationKind::Set);
    let [account] = &manifest.contributes.accounts[..] else { panic!("{manifest:?}") };
    assert_eq!(format!("{{account:{}}}", account.id), upload::AUTHORIZATION);
    assert_eq!(account.authorize_url, "https://accounts.google.com/o/oauth2/v2/auth");
    assert_eq!(account.token_url, None);
    assert_eq!(account.token_exchange.as_deref(), Some("https://localnetwork.computer/api/token-exchange/google"));
    assert_eq!(account.scopes, ["https://www.googleapis.com/auth/youtube.upload"]);
    assert_eq!(account.revoke_url.as_deref(), Some("https://oauth2.googleapis.com/revoke"));
    // Google's client ids are `<number>-<id>.apps.googleusercontent.com`; until Local Network has
    // one, a placeholder that says so.
    assert!(account.client_id.ends_with(".apps.googleusercontent.com"), "{}", account.client_id);
    let settings: Vec<(&str, SettingType, Option<&Value>)> =
        manifest.contributes.settings.iter().map(|s| (s.key.as_str(), s.kind, s.default.as_ref())).collect();
    assert_eq!(
        settings,
        [("privacy", SettingType::Enum, Some(&json!("unlisted"))), ("picture", SettingType::File, None)]
    );
    assert_eq!(manifest.contributes.settings[0].choices, ["private", "unlisted", "public"]);
}

/// The Google account plugin's `youtube-upload` scope covers the requests an upload makes:
/// starting its session, and sending the video to the address YouTube answers with (as Google's
/// guide to the resumable upload shows one), and not YouTube's other API.
#[test]
fn googles_youtube_upload_scope_covers_the_upload() {
    let google = Manifest::parse(include_str!("../../accounts/google-account/locallibrary-plugin.toml")).unwrap();
    let scope = &google.contributes.account_types[0].scopes["youtube-upload"];
    let covers = |method: &str, url: &str| scope.covers(method, &url::Url::parse(url).unwrap());
    let session = "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&upload_id=xa298sd_f&part=snippet,status,contentDetails";
    assert!(covers("POST", upload::START));
    assert!(covers("PUT", session));
    assert!(!covers("PUT", "https://www.googleapis.com/youtube/v3/videos?part=snippet"));
}

/// A tracklist of up to `most` entries, each maybe named (some with what YouTube doesn't take),
/// maybe timed, in any order.
#[hegel::test_helper]
fn tracklist(tc: &TestCase, most: usize) -> Vec<TracklistEntry> {
    let names = ["Young Fathers", "\u{dc}n\u{ef}c\u{f8}d\u{e9} <Art\u{ef}st>", " ", "Two\nLines", &"Long ".repeat(80)]
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

/// A line's time in seconds, if it starts with one (`05:30`, `1:02:03`), and the rest.
fn timed(line: &str) -> (Option<u32>, &str) {
    let Some((time, rest)) = line.split_once(' ') else { return (None, line) };
    let fields: Option<Vec<u32>> = time.split(':').map(|f| f.parse().ok()).collect();
    match fields.as_deref() {
        Some([m, s]) => (Some(m * 60 + s), rest),
        Some([h, m, s]) => (Some(h * 3600 + m * 60 + s), rest),
        _ => (None, line),
    }
}

/// Whatever the set, recording and tracklist, the video's title and description are within what
/// YouTube takes, and the description's times are chapters YouTube makes: the first line at
/// 00:00, then in order, each at least ten seconds after the one before and before the
/// recording's last ten seconds, each the track's own start. Every track that could have been one
/// is; a tracklist too long keeps the lines that fit, in order, and says how many more.
#[hegel::test(test_cases = 300)]
fn the_description_is_chapters_youtube_takes(tc: TestCase) {
    let words = gs::optional(gs::sampled_from(vec!["Closing set".to_string(), "<>".into(), "\u{3a9}".repeat(150)]));
    let set = PublishedSet { title: tc.draw(words), ..set() };
    let length = tc.draw(gs::integers::<u32>().max_value(6 * 3600));
    let tracklist = tracklist(&tc, 60);
    let d = video::of(&set, &tracklist, length);

    for text in [&d.title, &d.description] {
        assert!(!text.contains(['<', '>']), "{text:?}");
    }
    assert!(!d.title.is_empty() && d.title.chars().count() <= MAX_TITLE, "{:?}", d.title);
    let long = set.title.as_ref().is_some_and(|t| t.chars().count() > MAX_TITLE);
    assert_eq!(d.notes.iter().any(|n| n.starts_with("the title is cut to YouTube's")), long, "{:?}", d.notes);
    assert!(d.description.len() <= MAX_DESCRIPTION, "{}", d.description.len());

    let mut lines: Vec<&str> = d.description.lines().collect();
    let more = lines.last().and_then(|l| l.strip_prefix("+ ")).and_then(|l| l.strip_suffix(" more"));
    if let Some(more) = more {
        lines.pop();
        assert_eq!(more.parse::<usize>().unwrap(), tracklist.len() - lines.len());
        // The next line wouldn't have fit, with one fewer to say there are more of.
        let next = video::line(&tracklist[lines.len()], video::chapters(&tracklist, length)[lines.len()]).len();
        let fewer = format!("+ {} more", tracklist.len() - lines.len() - 1).len();
        assert!(d.description.len() - "+  more".len() - more.len() + next + 1 + fewer > MAX_DESCRIPTION);
    } else {
        assert_eq!(lines.len(), tracklist.len());
    }
    let any_timed = tracklist.iter().any(|e| e.start_seconds.is_some());
    let mut last: Option<u32> = None;
    for (i, (line, entry)) in lines.iter().zip(&tracklist).enumerate() {
        let (at, name) = timed(line);
        let plain = |s: &Option<String>| {
            let s: String = s.as_deref().unwrap_or_default().chars().filter(|c| !matches!(c, '<' | '>')).collect();
            let s = s.replace('\n', " ").trim().to_string();
            if s.is_empty() { "ID".into() } else { s }
        };
        assert_eq!(name, format!("{} - {}", plain(&entry.artist), plain(&entry.title)), "{line:?}");
        let start = if i == 0 && any_timed { Some(0) } else { entry.start_seconds };
        let could = start.filter(|&s| last.is_none_or(|l| s >= l + MIN_CHAPTER) && s + MIN_CHAPTER <= length);
        assert_eq!(at, could, "line {i}: {line:?}, after {last:?}, of {length} s");
        last = at.or(last);
    }
    let chapters = lines.iter().filter(|l| timed(l).0.is_some()).count();
    assert_eq!(chapters, d.chapters);
    let few = d.notes.iter().any(|n| n.starts_with("YouTube shows chapters once"));
    assert_eq!(few, (1..MIN_CHAPTERS).contains(&chapters), "{:?}", d.notes);
    let cut = d.notes.iter().any(|n| n.starts_with("the description lists"));
    assert_eq!(cut, more.is_some());
}

/// A chapter's time, as `05:30` or past an hour `1:02:03`, reads back as the second it's for.
#[hegel::test(test_cases = 100)]
fn a_chapters_time_reads_back(tc: TestCase) {
    let secs = tc.draw(gs::integers::<u32>());
    let line = video::line(&entry(Some("A"), Some("B"), None), Some(secs));
    let (time, name) = line.split_once(' ').unwrap();
    assert_eq!(name, "A - B");
    assert!(time.split(':').skip(1).all(|f| f.len() == 2), "{line}");
    assert_eq!(time.split(':').count(), if secs < 3600 { 2 } else { 3 }, "{line}");
    assert_eq!(timed(&line).0, Some(secs), "{line}");
}
