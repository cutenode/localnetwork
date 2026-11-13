#[cfg(unix)]
mod publishing;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_plugin::{
    Capability, DestinationKind, Manifest, PublishedSet, SettingType, TokenPlace, TracklistEntry, youtube,
};
use serde_json::{Value, json};

use super::video::{self, MAX_TITLE};
use super::*;

/// The Google account plugin's manifest: the account type the plugin asks for.
const GOOGLE_ACCOUNT: &str = include_str!("../../accounts/google-account/locallibrary-plugin.toml");

fn set() -> PublishedSet {
    PublishedSet {
        id: "s1".into(),
        date: "2026-10-03".into(),
        title: Some("Closing set".into()),
        venue: Some("Warehouse".into()),
        ..PublishedSet::default()
    }
}

/// It reaches YouTube's API alone, publishes sets at one destination, asks for a Google account
/// to upload with and nothing else, naming it where the Google account plugin puts its token
/// (after `Bearer`), and keeps who can watch and the picture as settings.
#[test]
fn the_manifest_is_a_built_in_set_destination_on_youtube() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    assert_eq!(manifest.capabilities, [Capability::Net("www.googleapis.com".into())]);
    let [destination] = &manifest.contributes.destinations[..] else { panic!("{manifest:?}") };
    assert_eq!((manifest.id.as_str(), destination.name.as_str()), ("youtube", "video"));
    assert_eq!(destination.kind, DestinationKind::Set);
    let [need] = &manifest.accounts[..] else { panic!("{manifest:?}") };
    assert_eq!((need.name.as_str(), need.scopes.as_slice()), ("google", ["youtube-upload".to_string()].as_slice()));
    assert!(need.optional.is_empty(), "{need:?}");
    let google = Manifest::parse(GOOGLE_ACCOUNT).unwrap();
    let [account_type] = &google.contributes.account_types[..] else { panic!("{google:?}") };
    assert_eq!(format!("{{account:{}}}", account_type.name), upload::AUTHORIZATION);
    assert_eq!(account_type.token, TokenPlace::Scheme("Bearer".into()));
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
    let google = Manifest::parse(GOOGLE_ACCOUNT).unwrap();
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
            ..Default::default()
        })
        .collect()
}

/// Whatever the set, recording and tracklist, the video's title is within what YouTube takes,
/// saying so when it's cut, and its description, chapters and the rest of its notes are YouTube's
/// description of the tracklist (see `loclib_plugin::youtube`), which `sets export` writes too.
#[hegel::test(test_cases = 300)]
fn the_title_and_description_are_what_youtube_takes(tc: TestCase) {
    let words = gs::optional(gs::sampled_from(vec!["Closing set".to_string(), "<>".into(), "\u{3a9}".repeat(150)]));
    let set = PublishedSet { title: tc.draw(words), ..set() };
    let length = tc.draw(gs::integers::<u32>().max_value(6 * 3600));
    let tracklist = tracklist(&tc, 60);
    let d = video::of(&set, &tracklist, length);

    assert!(!d.title.contains(['<', '>']), "{:?}", d.title);
    assert!(!d.title.is_empty() && d.title.chars().count() <= MAX_TITLE, "{:?}", d.title);
    let long = set.title.as_ref().is_some_and(|t| t.chars().count() > MAX_TITLE);
    let cut = long.then(|| format!("the title is cut to YouTube's {MAX_TITLE} characters"));
    let description = youtube::description(&tracklist, length);
    let notes = [cut.into_iter().collect(), description.notes].concat();
    assert_eq!((d.description, d.chapters, d.notes), (description.text, description.chapters, notes));
}

/// A set without a title is named after where it was played (its venue, else its city, else "DJ
/// set") and when, each as YouTube takes it.
#[hegel::test(test_cases = 100)]
fn prop_an_untitled_set_is_named_after_where_and_when(tc: TestCase) {
    let place = || gs::optional(gs::sampled_from(vec!["Warehouse".to_string(), " ".into(), "<Club>".into()]));
    let (venue, city) = (tc.draw(place()), tc.draw(place()));
    let set = PublishedSet { title: None, venue: venue.clone(), city: city.clone(), ..set() };
    let given = |s: Option<String>| s.map(|s| s.replace(['<', '>'], "")).filter(|s| !s.trim().is_empty());
    let place = given(venue).or_else(|| given(city)).unwrap_or_else(|| "DJ set".into());
    assert_eq!(video::of(&set, &[], 60).title, format!("{place}, 2026-10-03"));
}
