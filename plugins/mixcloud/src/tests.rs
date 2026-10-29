mod mixcloud;
mod properties;
mod publishing;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::{Capability, DestinationKind, Manifest, PublishedSet, SettingType, TokenPlace, TracklistEntry};
use serde_json::{Value, json};

use super::show::{self, MAX_DESCRIPTION, MAX_TAGS, Section};
use super::*;

fn set() -> PublishedSet {
    PublishedSet {
        id: "s1".into(),
        date: "2026-10-03".into(),
        title: Some("Closing set".into()),
        venue: Some("Warehouse".into()),
        city: Some("Leeds".into()),
        country: Some("GB".into()),
        ..PublishedSet::default()
    }
}

fn entry(artist: Option<&str>, title: Option<&str>, start: Option<u32>) -> TracklistEntry {
    TracklistEntry { artist: artist.map(Into::into), title: title.map(Into::into), start_seconds: start }
}

/// It reaches Mixcloud's API alone, publishes sets at one destination, signs in to Mixcloud
/// through Local Network's token exchange with the token in the query, and keeps the tags, the
/// picture and whether a show is unlisted as settings.
#[test]
fn the_manifest_is_a_first_party_set_destination_on_mixcloud() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    assert_eq!(manifest.capabilities, [Capability::Net("api.mixcloud.com".into())]);
    let [destination] = &manifest.contributes.destinations[..] else { panic!("{manifest:?}") };
    assert_eq!((manifest.id.as_str(), destination.name.as_str()), ("mixcloud", "show"));
    assert_eq!(destination.kind, DestinationKind::Set);
    let [account] = &manifest.contributes.accounts[..] else { panic!("{manifest:?}") };
    assert_eq!(account.token, TokenPlace::Query("access_token".into()));
    let named = format!("?{}={{account:{}}}", "access_token", account.id);
    assert!(upload::UPLOAD.ends_with(&named), "{}", upload::UPLOAD);
    assert_eq!(account.authorize_url, "https://www.mixcloud.com/oauth/authorize");
    assert_eq!(account.token_url, None);
    assert_eq!(account.token_exchange.as_deref(), Some("https://localnetwork.computer/api/token-exchange/mixcloud"));
    // Mixcloud has no scopes, no revoking and no registered redirect URI to match.
    assert!(account.scopes.is_empty());
    assert_eq!((account.revoke_url.as_deref(), account.redirect_port), (None, None));
    assert!(account.client_id.starts_with("PLACEHOLDER"), "{}", account.client_id);
    let settings: Vec<(&str, SettingType, Option<&Value>)> =
        manifest.contributes.settings.iter().map(|s| (s.key.as_str(), s.kind, s.default.as_ref())).collect();
    assert_eq!(
        settings,
        [
            ("tags", SettingType::String, None),
            ("picture", SettingType::File, None),
            ("unlisted", SettingType::Boolean, Some(&json!(false)))
        ]
    );
}

/// A set without a title is named for where and when it was played, the description says where
/// it was, an unnamed track is `ID`, and a time before the one before it is left off.
#[test]
fn a_set_becomes_a_show() {
    let untitled = PublishedSet { title: None, ..set() };
    let tracklist = [
        entry(Some(" Young Fathers "), Some("In My View"), Some(0)),
        entry(None, Some(" "), Some(330)),
        entry(Some("Back"), Some("In Time"), Some(200)),
        entry(Some("Someone"), Some("Unplaced"), None),
        entry(Some("Chris Lake"), Some("Turn Off the Lights"), Some(3723)),
    ];
    let s = show::of(&untitled, &tracklist, " House, techno ,,HOUSE ", false);
    assert_eq!(s.name, "Warehouse, 2026-10-03");
    assert_eq!(s.description, "Played at Warehouse, Leeds, GB on 2026-10-03");
    assert_eq!(s.tags, ["House", "techno"]);
    let section = |artist: &str, song: &str, start| Section { artist: artist.into(), song: song.into(), start };
    assert_eq!(
        s.sections,
        [
            section("Young Fathers", "In My View", Some(0)),
            section("ID", "ID", Some(330)),
            section("Back", "In Time", None),
            section("Someone", "Unplaced", None),
            section("Chris Lake", "Turn Off the Lights", Some(3723)),
        ]
    );
    assert_eq!(s.notes, ["left the time off 1 of the tracks, each no later than a time before it"]);

    // Cut at 1,000 characters (each of these two bytes), and only past them.
    let sized = |n: usize| PublishedSet { venue: Some("\u{e9}".repeat(n)), city: None, country: None, ..set() };
    let fits = show::of(&sized(MAX_DESCRIPTION - "Played at  on 2026-10-03".len()), &[], "", false);
    assert_eq!((fits.description.chars().count(), fits.notes.len()), (MAX_DESCRIPTION, 0));
    let over = show::of(&sized(MAX_DESCRIPTION), &[], "", false);
    assert_eq!(over.description.chars().count(), MAX_DESCRIPTION);
    assert_eq!(over.notes, ["the description is cut to Mixcloud's 1000 characters"]);

    let nowhere = PublishedSet { title: None, venue: None, city: Some(" ".into()), country: None, ..set() };
    let s = show::of(&nowhere, &[], "", true);
    assert_eq!((s.name.as_str(), s.description.as_str()), ("DJ set, 2026-10-03", "Played on 2026-10-03"));
    assert!(s.sections.is_empty() && s.tags.is_empty() && s.notes.is_empty() && s.unlisted);
}

/// Some text, maybe with spaces around it, maybe blank, maybe long: one of a few words in any
/// case, or a run of a letter two bytes long.
#[hegel::test_helper]
fn text(tc: &TestCase) -> String {
    let words = ["House", "Techno", " Leeds ", "", " ", "\u{d8}r\u{e9}sund", "Disco", "Jungle", "Dub"];
    let word = tc.draw(gs::sampled_from(words.map(String::from).to_vec()));
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => word.to_uppercase(),
        1 => "\u{e9}".repeat(tc.draw(gs::integers::<usize>().max_value(1100))),
        _ => word,
    }
}

/// A set with any of its details, a tags setting of up to nine tags, and a tracklist of up to
/// `most` tracks, each maybe named and maybe timed.
#[hegel::test_helper]
fn any_set(tc: &TestCase, most: usize) -> (PublishedSet, String, Vec<TracklistEntry>) {
    let maybe = |tc: &TestCase| tc.draw(gs::booleans()).then(|| text(tc));
    let set = PublishedSet { title: maybe(tc), venue: maybe(tc), city: maybe(tc), country: maybe(tc), ..set() };
    let tags: Vec<String> = (0..tc.draw(gs::integers::<usize>().max_value(9))).map(|_| text(tc)).collect();
    let tracklist = (0..tc.draw(gs::integers::<usize>().max_value(most)))
        .map(|_| TracklistEntry {
            artist: maybe(tc),
            title: maybe(tc),
            start_seconds: tc.draw(gs::optional(gs::integers::<u32>().max_value(4 * 3600))),
        })
        .collect();
    (set, tags.join(","), tracklist)
}

/// Whatever the set, the tags setting and the tracklist, the show is within what Mixcloud takes:
/// a description of 1,000 characters, saying so when it's cut; the setting's tags, each once
/// (whatever its case), in order, five at most, saying which were left out; and a section for each
/// track in order, named, its time kept when it's after every time kept before it. Its form has
/// exactly these fields, numbered from 0.
#[hegel::test(test_cases = 300)]
fn the_show_is_within_what_mixcloud_takes(tc: TestCase) {
    let (set, tags, tracklist) = any_set(&tc, 40);
    let unlisted = tc.draw(gs::booleans());
    let s = show::of(&set, &tracklist, &tags, unlisted);
    let has = |start: &str| s.notes.iter().any(|n| n.starts_with(start));

    assert!(!s.name.trim().is_empty() && s.name.trim() == s.name, "{:?}", s.name);
    assert!(s.description.chars().count() <= MAX_DESCRIPTION, "{}", s.description.chars().count());
    let place: Vec<&str> = [&set.venue, &set.city, &set.country]
        .into_iter()
        .flatten()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let whole = if place.is_empty() {
        "Played on 2026-10-03".to_string()
    } else {
        format!("Played at {} on 2026-10-03", place.join(", "))
    };
    assert!(whole.starts_with(&s.description));
    assert_eq!(has("the description is cut"), whole != s.description);

    let mut each: Vec<&str> = Vec::new();
    for t in tags.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if !each.iter().any(|e| e.eq_ignore_ascii_case(t)) {
            each.push(t);
        }
    }
    assert!(s.tags.len() <= MAX_TAGS);
    assert_eq!(s.tags, each[..each.len().min(MAX_TAGS)]);
    assert_eq!(has("Mixcloud takes 5 tags: left out "), each.len() > MAX_TAGS, "{:?}", s.notes);

    assert_eq!(s.sections.len(), tracklist.len());
    let mut last: Option<u32> = None;
    let mut left_off = 0;
    for (section, entry) in s.sections.iter().zip(&tracklist) {
        let named =
            |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("ID").to_string();
        assert_eq!((&section.artist, &section.song), (&named(&entry.artist), &named(&entry.title)));
        let kept = entry.start_seconds.filter(|&at| last.is_none_or(|l| at > l));
        assert_eq!(section.start, kept);
        left_off += usize::from(entry.start_seconds.is_some() && kept.is_none());
        last = kept.or(last);
    }
    let note = format!("left the time off {left_off} of the tracks, each no later than a time before it");
    assert_eq!(s.notes.contains(&note), left_off > 0, "{:?}", s.notes);

    let fields: Vec<(String, String)> =
        s.fields().into_iter().map(|f| (f.name, f.value.expect("a field is text"))).collect();
    let mut expected = vec![("name".to_string(), s.name.clone()), ("description".into(), s.description.clone())];
    expected.extend(s.tags.iter().enumerate().map(|(n, t)| (format!("tags-{n}-tag"), t.clone())));
    for (n, section) in s.sections.iter().enumerate() {
        expected.push((format!("sections-{n}-artist"), section.artist.clone()));
        expected.push((format!("sections-{n}-song"), section.song.clone()));
        if let Some(at) = section.start {
            expected.push((format!("sections-{n}-start_time"), at.to_string()));
        }
    }
    if unlisted {
        expected.push(("unlisted".into(), "1".into()));
    }
    assert_eq!(fields, expected);
}
