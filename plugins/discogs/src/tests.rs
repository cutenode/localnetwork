use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hegel::TestCase;
use hegel::generators as gs;
use image::RgbImage;
use image::codecs::jpeg::JpegEncoder;
use loclib_model::Track;
use loclib_model::test_util::track;
use loclib_plugin::test_util::{FakeHost, Sent};
use loclib_plugin::{ACCOUNT_PLACEHOLDER, AccountType, Fetch, Fetched, Manifest, NetLimit, Provider, SignIn, method};
use serde_json::{Value, json};

use super::discogs::{API, AUTHORIZATION, IMAGES, LONGEST_WAIT, PER_PAGE, SECOND, TRIES, WINDOW};
use super::*;

/// Discogs' answers to searches, shaped as its database search answers them (trimmed; the ids,
/// catalogue numbers and image addresses are made up): Bicep's album "Bicep" by its name and
/// artist (three editions of one master, the last without a cover, and a remix album of another
/// name), "Isles" by its label (a label numbered as Discogs tells apart labels of one name, and no
/// style), and the releases with a track "Glue" (the album, and a single without a master that
/// came out on no label but a sublabel).
const BICEP: &str = include_str!("../fixtures/search-bicep.json");
const ISLES: &str = include_str!("../fixtures/search-isles.json");
const GLUE: &str = include_str!("../fixtures/search-glue.json");
/// A search that found nothing.
const NOTHING: &str =
    r#"{"pagination": {"page": 1, "pages": 0, "per_page": 25, "items": 0, "urls": {}}, "results": []}"#;

/// The covers of those releases.
const BICEP_COVER: &str = "https://i.discogs.com/aW1hZ2Ut/rs:fit/g:sm/q:90/h:600/w:600/UjEwNjI1NTQ1.jpeg";
const ISLES_COVER: &str = "https://i.discogs.com/aXNsZXM/rs:fit/g:sm/q:90/h:600/w:600/UjE3MDAxMDAx.jpeg";
const GLUE_COVER: &str = "https://i.discogs.com/Z2x1ZQ/rs:fit/g:sm/q:90/h:600/w:600/UjExMDUwMDAx.jpeg";

/// The account plugin's manifest, which provides the account type the plugin asks for.
const ACCOUNT: &str = include_str!("../../accounts/discogs-account/locallibrary-plugin.toml");

/// As the host fails a request naming an account the plugin has no access to.
const NO_ACCESS: &str = "it has no access to a discogs account";

fn account_type() -> AccountType {
    let [account_type] = &Manifest::parse(ACCOUNT).unwrap().contributes.account_types[..] else {
        panic!("one account type")
    };
    account_type.clone()
}

/// How Discogs answers a request through the host: a status with its headers and body, or the
/// host failing it (`external`, `denied`...).
#[derive(Clone, Debug)]
enum Reply {
    Answer(u16, Vec<(&'static str, String)>, Vec<u8>),
    Fails(&'static str),
}

fn ok(body: impl Into<Vec<u8>>) -> Reply {
    Reply::Answer(200, Vec::new(), body.into())
}

fn status(status: u16) -> Reply {
    Reply::Answer(status, Vec::new(), br#"{"message": "no"}"#.to_vec())
}

/// Over the limit: 429, saying how long to wait when `wait` does.
fn too_many(wait: Option<&str>) -> Reply {
    let headers = wait.map(|w| ("retry-after", w.to_string())).into_iter().collect();
    Reply::Answer(429, headers, br#"{"message": "You are making requests too quickly."}"#.to_vec())
}

/// A JPEG of `size` x `size`.
fn jpeg(size: u32) -> Vec<u8> {
    let image = RgbImage::from_fn(size, size, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
    let mut out = Vec::new();
    image.write_with_encoder(JpegEncoder::new_with_quality(&mut out, 90)).unwrap();
    out
}

/// A search's answer with these releases, each its id, `Artist - Title`, year and cover.
fn found(releases: &[(u64, &str, &str, &str)]) -> String {
    let results: Vec<Value> = releases
        .iter()
        .map(|&(id, title, year, cover)| json!({ "id": id, "title": title, "year": year, "type": "release", "cover_image": cover }))
        .collect();
    json!({ "pagination": { "page": 1, "pages": 1, "per_page": 25, "items": results.len() }, "results": results })
        .to_string()
}

/// A search's params, but the `type` and `per_page` every one has (which it checks).
fn params_of(url: &str) -> Vec<(String, String)> {
    let query = url.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let mut params: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    assert_eq!(params.remove(0), ("type".into(), "release".into()), "{url}");
    assert_eq!(params.pop(), Some(("per_page".into(), PER_PAGE.into())), "{url}");
    params
}

fn param<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Discogs as it has these releases: Bicep's "Bicep" by name, "Isles" by name or as Ninja Tune's,
/// the releases with "Glue" on them, and a cover for each.
fn as_they_are(url: &str, _: usize) -> Reply {
    if url.starts_with(IMAGES) {
        return ok(jpeg(600));
    }
    let params = params_of(url);
    let (album, label, track) = (param(&params, "release_title"), param(&params, "label"), param(&params, "track"));
    match (param(&params, "artist"), album, label, track) {
        (Some("Bicep"), Some("Bicep"), None, None) => ok(BICEP),
        (Some("Bicep"), Some("Isles"), None | Some("Ninja Tune"), None) => ok(ISLES),
        (Some("Bicep"), None, None, Some("Glue")) => ok(GLUE),
        _ => ok(NOTHING),
    }
}

/// A request that reached the fake Discogs through the host.
#[derive(Debug, Clone)]
struct Asked {
    url: String,
    at: Instant,
}

/// Discogs' answer to a request, by its URL and how many times it was asked before.
type Route = Box<dyn Fn(&str, usize) -> Reply + Send + Sync>;

/// What answers the plugin's `net/fetch` as the host and Discogs would: `route` answers each
/// request (by its URL, and how many times it was asked before), filling in the account where
/// it's named while it's signed in, and writing an answer asked `into_blob` to that file.
struct Fake {
    signed_in: bool,
    route: Route,
    asked: Mutex<Vec<Asked>>,
    /// Told of each request once it's asked.
    tell: Mutex<Sender<()>>,
}

impl Fake {
    fn answer(&self, params: Value) -> Result<Value, RpcError> {
        let fetch: Fetch = serde_json::from_value(params).unwrap();
        let before = {
            let mut asked = self.asked.lock().unwrap();
            let before = asked.iter().filter(|a| a.url == fetch.url).count();
            asked.push(Asked { url: fetch.url.clone(), at: Instant::now() });
            before
        };
        let _ = self.tell.lock().unwrap().send(());
        // A search names the account, as the whole Authorization header, in a request its scope
        // covers; an image doesn't.
        let named: Vec<(&String, &String)> =
            fetch.headers.iter().filter(|(_, v)| v.contains(ACCOUNT_PLACEHOLDER)).collect();
        match fetch.url.starts_with(&format!("{API}/")) {
            true => {
                let authorization = ("Authorization".to_string(), AUTHORIZATION.to_string());
                assert_eq!(named, [(&authorization.0, &authorization.1)], "{}", fetch.url);
                let url = url::Url::parse(&fetch.url).unwrap();
                assert!(account_type().scopes["database"].covers(&fetch.method, &url), "{}", fetch.url);
                if !self.signed_in {
                    return Err(RpcError::failed("access", NO_ACCESS));
                }
            }
            false => {
                assert!(fetch.url.starts_with(IMAGES) && named.is_empty(), "{fetch:?}");
                assert!(fetch.into_blob.is_some(), "an image goes to a file: {fetch:?}");
            }
        }
        let (status, headers, body) = match (self.route)(&fetch.url, before) {
            Reply::Fails(kind) => return Err(RpcError::failed(kind, format!("no answer ({kind})"))),
            Reply::Answer(status, headers, body) => (status, headers, body),
        };
        let headers = headers.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let fetched = match fetch.into_blob {
            Some(file) => {
                std::fs::write(&file, body).unwrap();
                Fetched { status, headers, body: None, blob: Some(file) }
            }
            None => Fetched { status, headers, body: Some(String::from_utf8(body).unwrap()), blob: None },
        };
        Ok(serde_json::to_value(fetched).unwrap())
    }
}

/// The plugin served to a fake host whose library has these record `labels` (by track id) and
/// whose Discogs answers as `route` says, with the account signed in or not.
struct Session {
    host: FakeHost,
    fake: Arc<Fake>,
    heard: Receiver<()>,
}

fn start(
    signed_in: bool,
    labels: &[(u32, &str)],
    route: impl Fn(&str, usize) -> Reply + Send + Sync + 'static,
) -> Session {
    let host = FakeHost::start(MANIFEST, serve);
    let (tell, heard) = channel();
    let fake = Arc::new(Fake { signed_in, route: Box::new(route), asked: Mutex::default(), tell: Mutex::new(tell) });
    let answering = fake.clone();
    host.on_read(method::NET_FETCH, move |params| answering.answer(params));
    let tracks: Vec<Track> = labels
        .iter()
        .map(|&(id, label)| Track { label: label.into(), ..track(id, "Bicep", "Song", "/Song.mp3") })
        .collect();
    host.read("tracks.get", json!({ "tracks": tracks, "missing": [] }));
    Session { host, fake, heard }
}

impl Session {
    /// Asks for covers (or tags, as `kind` says) for `tracks` on the drive USB.
    fn send(&self, kind: ProviderKind, tracks: &[TrackTags]) -> Sent {
        let provider = match kind {
            ProviderKind::Artwork => "covers",
            ProviderKind::Metadata => "tags",
        };
        let req = json!({ "provider": provider, "kind": kind, "tracks": tracks, "context": { "drive": "USB" } });
        self.host.send(method::PROVIDE, &req)
    }

    fn asked(&self) -> Vec<Asked> {
        self.fake.asked.lock().unwrap().clone()
    }

    /// The searches asked, each as its params, in order.
    fn searches(&self) -> Vec<Vec<(String, String)>> {
        self.asked().iter().filter(|a| a.url.starts_with(API)).map(|a| params_of(&a.url)).collect()
    }

    /// The covers asked for, in order.
    fn covers(&self) -> Vec<String> {
        self.asked().into_iter().filter(|a| a.url.starts_with(IMAGES)).map(|a| a.url).collect()
    }

    /// The reads the plugin called back for: each one's method and params.
    fn reads(&self) -> Vec<(String, Value)> {
        self.host.calls().into_iter().filter(|(m, _)| m != method::NET_FETCH).collect()
    }
}

/// Track `id`'s tags, of `album` (by `artist`).
fn tags(id: u32, artist: &str, album: &str, title: &str) -> TrackTags {
    TrackTags { id, artist: artist.into(), album: album.into(), title: title.into(), ..TrackTags::default() }
}

/// A search's params, as [`Session::searches`] has them.
fn search(params: &[(&str, &str)]) -> Vec<(String, String)> {
    params.iter().map(|&(k, v)| (k.into(), v.into())).collect()
}

/// A candidate's track, score, label and file.
type Seen = (u32, f64, String, String);

/// Each candidate's track, score, label and file, checking it's a cover from Discogs as
/// locallibrary takes one: an image given as a file in the request's `scratch` folder, by its
/// name. The folder holds those files and no others.
fn seen(c: &Candidates, scratch: &std::path::Path) -> Vec<Seen> {
    let mut files = Vec::new();
    let seen = c.candidates.iter().map(|c| {
        assert_eq!((c.source.as_str(), &c.metadata), (SOURCE, &None), "{c:?}");
        let blob = c.artwork.as_ref().expect("an image").blob.clone();
        assert!(!blob.contains(['/', '\\']), "a name in the folder: {blob}");
        let image = image::load_from_memory(&std::fs::read(scratch.join(&blob)).unwrap()).unwrap();
        assert_eq!((image.width(), image.height()), (600, 600));
        if !files.contains(&blob) {
            files.push(blob.clone());
        }
        (c.track, c.score, c.label.clone(), blob)
    });
    let seen: Vec<Seen> = seen.collect();
    let there = std::fs::read_dir(scratch).unwrap().count();
    assert_eq!(there, files.len(), "{:?}", std::fs::read_dir(scratch).unwrap().collect::<Vec<_>>());
    seen
}

/// Each candidate's track, score, label and tags, checking they're from Discogs, without an image.
fn tagged(c: &Candidates) -> Vec<(u32, f64, String, TrackMetadata)> {
    let tagged = c.candidates.iter().map(|c| {
        assert_eq!((c.source.as_str(), &c.artwork), (SOURCE, &None), "{c:?}");
        (c.track, c.score, c.label.clone(), c.metadata.clone().expect("tags"))
    });
    tagged.collect()
}

fn album_tags(album: &str, year: u16, genre: &str, label: &str) -> TrackMetadata {
    TrackMetadata {
        album: Some(album.into()),
        year: Some(year),
        genre: Some(genre.into()),
        label: Some(label.into()),
        ..TrackMetadata::default()
    }
}

/// What the releases in the fixtures are shown as.
const BICEP_ALBUM: &str = "Bicep, Ninja Tune ZEN242, 2016";
const BICEP_ON_GLUE: &str = "Bicep, Ninja Tune ZEN242, 2017";
const GLUE_SINGLE: &str = "Glue, Feel My Bicep FMB012, 2017";
const ISLES_ALBUM: &str = "Isles, Ninja Tune ZEN275, 2021";

/// It reads the tracks it's given and their labels, provides artwork and metadata, says which
/// servers it talks to and asks the host to keep to Discogs' limit, and asks for a Discogs
/// account to search with. Served, it says it's this manifest (as `FakeHost::start` checks),
/// providing, sending fetches, answering blobs and naming accounts.
#[test]
fn the_manifest_is_a_built_in_provider_with_a_discogs_account() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    manifest.validate(true).unwrap();
    let capabilities: Vec<String> = manifest.capabilities.iter().map(ToString::to_string).collect();
    assert_eq!(capabilities, ["library:read", "net:api.discogs.com", "net:i.discogs.com"]);
    assert_eq!(manifest.net, [NetLimit { host: "api.discogs.com".into(), per_second: 1.0 }]);
    let covers = Provider { name: "covers".into(), title: SOURCE.into(), kind: ProviderKind::Artwork };
    let tags = Provider { name: "tags".into(), title: SOURCE.into(), kind: ProviderKind::Metadata };
    assert_eq!(manifest.contributes.providers, [covers, tags]);
    let [need] = &manifest.accounts[..] else { panic!("one account type") };
    assert_eq!((need.name.as_str(), &need.scopes[..]), ("discogs", &["database".to_string()][..]));
    assert!(account_type().scopes.contains_key("database"));
    assert!(manifest.contributes.operations.is_empty() && manifest.contributes.commands.is_empty());
    let contact = &manifest.contributes.settings[0];
    assert_eq!(
        (contact.key.as_str(), &contact.default),
        ("contact", &Some(json!("https://github.com/cutenode/localnetwork")))
    );

    let host = FakeHost::start(MANIFEST, serve);
    let has = |f: &&str| host.hello().features.iter().any(|had| had == f);
    assert!(["providers", "blobs", "net", "accounts"].iter().all(has), "{:?}", host.hello().features);
    host.stop();
}

/// The account type is signed in to with a personal access token, sent as Discogs takes it, and
/// Discogs says whose it is; its scope covers searches of the database and nothing else.
#[test]
fn the_account_type_takes_a_personal_access_token() {
    let manifest = Manifest::parse(ACCOUNT).unwrap();
    manifest.validate(true).unwrap();
    assert!(manifest.is_account_plugin());
    let t = account_type();
    assert_eq!((t.name.as_str(), t.kind), ("discogs", SignIn::Keys));
    let place = t.place();
    assert_eq!(place.header.as_deref(), Some("Authorization"));
    let filled = place.filled(|key| match key {
        "token" => "abc123",
        other => panic!("discogs-account has no key {other}"),
    });
    assert_eq!(filled, "Discogs token=abc123");
    let whoami = t.whoami.as_ref().unwrap();
    let said = (whoami.url.as_str(), whoami.subject.as_str(), whoami.name.as_str());
    assert_eq!(said, ("https://api.discogs.com/oauth/identity", "/id", "/username"));
    let scope = &t.scopes["database"];
    let covers = |method: &str, url: &str| scope.covers(method, &url::Url::parse(url).unwrap());
    assert!(covers("GET", "https://api.discogs.com/database/search?type=release&artist=Bicep&per_page=25"));
    assert!(!covers("POST", "https://api.discogs.com/database/search?type=release"));
    assert!(!covers("GET", "https://api.discogs.com/oauth/identity"));
    assert!(!covers("GET", "https://api.discogs.com/users/dj/collection"));
    assert!(!covers("GET", BICEP_COVER));
    assert!(!covers("GET", "https://api.discogs.com.evil.example/database/search?q=x"));
}

/// Each album is looked up once by its name and artist, a track without one by its title; each
/// release's cover is downloaded once, into the scratch folder, and offered for each of its
/// tracks: surer of the one album named so (its editions are one) than of the several releases a
/// track is on. Searches name the account; covers don't.
#[test]
fn albums_get_their_covers_one_search_each() {
    let tracks = [
        tags(1, "Bicep", "Bicep", "Glue"),
        // Letter case and spacing don't make another album.
        tags(2, "BICEP", " bicep", "Opal"),
        tags(3, "Bicep", "", "Glue"),
    ];
    let session = start(true, &[], as_they_are);
    let sent = session.send(ProviderKind::Artwork, &tracks);
    let candidates: Candidates = sent.wait().unwrap();
    assert_eq!(candidates.warnings, Vec::<String>::new());
    let cover = |id: &str| format!("cover-{id}");
    assert_eq!(
        seen(&candidates, sent.scratch().unwrap()),
        [
            (1, BY_NAME, BICEP_ALBUM.into(), cover("10625545")),
            (2, BY_NAME, BICEP_ALBUM.into(), cover("10625545")),
            (3, ONE_OF_SEVERAL, BICEP_ON_GLUE.into(), cover("10625545")),
            (3, ONE_OF_SEVERAL, GLUE_SINGLE.into(), cover("11050001")),
        ]
    );
    assert_eq!(
        session.searches(),
        [search(&[("artist", "Bicep"), ("release_title", "Bicep")]), search(&[("artist", "Bicep"), ("track", "Glue")])]
    );
    assert_eq!(session.covers(), [BICEP_COVER, GLUE_COVER]);
    assert_eq!(
        session.asked()[0].url,
        "https://api.discogs.com/database/search?type=release&artist=Bicep&release_title=Bicep&per_page=25"
    );
    // The labels of the tracks with an album, which have none here.
    assert_eq!(session.reads(), [("tracks.get".to_string(), json!({ "drive": "USB", "ids": [1, 2] }))]);
}

/// An album a track of has a record label is looked up by it first, surest; by its name and
/// artist when that finds nothing (tags spell labels many ways), and a track without an album by
/// its title. Each release's album, earliest year, first label (without Discogs' number) and
/// style (else genre) are its tags.
#[test]
fn lookups_go_by_label_then_name_then_title() {
    let tracks =
        [tags(1, "Bicep", "Isles", "Apricots"), tags(2, "Bicep", "Bicep", "Glue"), tags(3, "Bicep", "", "Glue")];
    let session = start(true, &[(1, "Ninja Tune"), (2, " Ninja Tune Ltd. ")], as_they_are);
    let candidates: Candidates = session.send(ProviderKind::Metadata, &tracks).wait().unwrap();
    assert_eq!(candidates.warnings, Vec::<String>::new());
    let bicep = album_tags("Bicep", 2016, "Techno", "Ninja Tune");
    assert_eq!(
        tagged(&candidates),
        [
            (1, BY_LABEL, ISLES_ALBUM.into(), album_tags("Isles", 2021, "Electronic", "Ninja Tune")),
            (2, BY_NAME, BICEP_ALBUM.into(), bicep),
            (3, ONE_OF_SEVERAL, BICEP_ON_GLUE.into(), album_tags("Bicep", 2017, "Techno", "Ninja Tune")),
            (3, ONE_OF_SEVERAL, GLUE_SINGLE.into(), album_tags("Glue", 2017, "Breakbeat", "Feel My Bicep")),
        ]
    );
    assert_eq!(
        session.searches(),
        [
            search(&[("artist", "Bicep"), ("release_title", "Isles"), ("label", "Ninja Tune")]),
            search(&[("artist", "Bicep"), ("release_title", "Bicep"), ("label", "Ninja Tune Ltd.")]),
            search(&[("artist", "Bicep"), ("release_title", "Bicep")]),
            search(&[("artist", "Bicep"), ("track", "Glue")]),
        ]
    );
    assert_eq!(session.covers(), Vec::<String>::new());
}

/// The one release found by a track's title is offered less surely than an album found by name,
/// below what's picked for the user; an album's artist goes before the track's, and a track with
/// no artist is searched without one.
#[test]
fn a_tracks_one_release_and_artists() {
    let single = found(&[(11050001, "Bicep - Glue", "2017", GLUE_COVER)]);
    let session = start(true, &[], move |url, _| {
        let params = params_of(url);
        match (param(&params, "artist"), param(&params, "track")) {
            (Some("Bicep"), Some("Glue")) => ok(single.clone()),
            _ => ok(NOTHING),
        }
    });
    let tracks = [
        TrackTags { album_artist: "Bicep".into(), ..tags(1, "Bicep, Clara La San", "", "Glue") },
        tags(2, " ", "Isles", "Atlas"),
    ];
    let candidates: Candidates = session.send(ProviderKind::Metadata, &tracks).wait().unwrap();
    let glue = TrackMetadata { album: Some("Glue".into()), year: Some(2017), ..TrackMetadata::default() };
    assert_eq!(tagged(&candidates), [(1, BY_TITLE, "Glue, 2017".into(), glue)]);
    assert_eq!(candidates.warnings, ["Isles: not on Discogs"]);
    assert_eq!(
        session.searches(),
        [search(&[("artist", "Bicep"), ("track", "Glue")]), search(&[("release_title", "Isles")])]
    );
}

/// The note when there's no account to search with: how to sign in.
fn sign_in_note() -> String {
    format!(
        "Discogs answers searches only for a signed-in account ({NO_ACCESS}): make a personal access token at \
         https://www.discogs.com/settings/developers and sign in with `locallibrary account discogs sign-in --for discogs`"
    )
}

/// Without an account it may use, either provider finds nothing, looks up nothing past the first
/// search, and says how to sign in.
#[test]
fn without_an_account_it_says_how_to_sign_in() {
    for kind in [ProviderKind::Artwork, ProviderKind::Metadata] {
        let tracks = [tags(1, "Bicep", "Bicep", "Glue"), tags(2, "Bicep", "Isles", "Atlas"), tags(9, "Bicep", "", "")];
        let session = start(false, &[], as_they_are);
        let candidates: Candidates = session.send(kind, &tracks).wait().unwrap();
        let nameless = "track #9 has no album or title to look up".to_string();
        assert_eq!(candidates, Candidates { candidates: Vec::new(), warnings: vec![nameless, sign_in_note()] });
        assert_eq!(session.searches().len(), 1);
        assert_eq!(session.covers(), Vec::<String>::new());
    }
}

/// A token Discogs refuses (it was revoked) finds nothing either, and says to sign in with another.
#[test]
fn a_refused_token_says_to_sign_in_again() {
    let session = start(true, &[], |_, _| status(401));
    let tracks = [tags(1, "Bicep", "Bicep", "Glue"), tags(2, "Bicep", "Isles", "Atlas")];
    let candidates: Candidates = session.send(ProviderKind::Metadata, &tracks).wait().unwrap();
    let said = "Discogs refused the account's personal access token (401): make another at \
                https://www.discogs.com/settings/developers and sign in again with \
                `locallibrary account discogs sign-in --for discogs`";
    assert_eq!(candidates, Candidates { candidates: Vec::new(), warnings: vec![said.into()] });
    assert_eq!(session.searches().len(), 1);
}

/// How long after the request before it each request of `asked` to `url` came.
fn gaps(asked: &[Asked], url: &str) -> Vec<Duration> {
    let at: Vec<Instant> = asked.iter().filter(|a| a.url.contains(url)).map(|a| a.at).collect();
    at.windows(2).map(|w| w[1] - w[0]).collect()
}

/// Over Discogs' limit (429), a search waits as long as Discogs asks, else a whole window, and is
/// sent again, a few times at most; asked to wait too long, it isn't. An album left out is a
/// warning, and the others are still looked up.
#[test]
fn over_the_limit_a_search_waits_then_goes_again() {
    let session = start(true, &[], |url, before| {
        let params = params_of(url);
        match (param(&params, "release_title"), param(&params, "track"), before) {
            (Some("Bicep"), _, 0) => too_many(Some("30")),
            (Some("Isles"), _, _) => too_many(None),
            (_, Some("Glue"), _) => too_many(Some(&(LONGEST_WAIT + 1).to_string())),
            _ => as_they_are(url, before),
        }
    });
    let tracks = [tags(1, "Bicep", "Bicep", "Glue"), tags(2, "Bicep", "Isles", "Atlas"), tags(3, "Bicep", "", "Glue")];
    let candidates: Candidates = session.send(ProviderKind::Metadata, &tracks).wait().unwrap();
    let found: Vec<(u32, String)> = tagged(&candidates).into_iter().map(|(t, _, label, _)| (t, label)).collect();
    assert_eq!(found, [(1, BICEP_ALBUM.to_string())]);
    assert_eq!(
        candidates.warnings,
        ["Bicep - Isles: left out, as Discogs answered 429", "Bicep - Glue: left out, as Discogs answered 429"]
    );
    let asked = session.asked();
    let bicep = gaps(&asked, "release_title=Bicep");
    assert_eq!(bicep.len(), 1);
    assert!(bicep[0] >= SECOND * 30, "{bicep:?}");
    let isles = gaps(&asked, "release_title=Isles");
    assert_eq!(isles.len(), TRIES - 1);
    assert!(isles.iter().all(|&gap| gap >= SECOND * WINDOW), "{isles:?}");
    assert_eq!(gaps(&asked, "track=Glue").len(), 0, "asked once");
}

/// An answer saying the account has no requests left this minute holds the next search back a
/// window from then.
#[test]
fn no_requests_left_holds_the_next_search_back() {
    let session = start(true, &[], |url, before| match as_they_are(url, before) {
        Reply::Answer(200, _, body) if url.contains("release_title=Bicep") => {
            Reply::Answer(200, vec![("x-discogs-ratelimit-remaining", "0".into())], body)
        }
        reply => reply,
    });
    let tracks = [tags(1, "Bicep", "Bicep", "Glue"), tags(2, "Bicep", "Isles", "Atlas")];
    let candidates: Candidates = session.send(ProviderKind::Metadata, &tracks).wait().unwrap();
    assert_eq!(candidates.candidates.len(), 2);
    let asked = session.asked();
    assert_eq!(asked.len(), 2);
    assert!(asked[1].at - asked[0].at >= SECOND * WINDOW, "{asked:?}");
}

/// A cover that can't be had (Discogs has none, it doesn't download, it isn't an image, or is too
/// small to be artwork) is a warning, naming the album and the release, and no file is left; each
/// release's cover is asked for once.
#[test]
fn covers_that_cant_be_had_are_warnings() {
    let opal = found(&[(30001, "Bicep - Opal", "2017", "https://st.discogs.com/images/default-release.png")]);
    let atlas_cover = "https://i.discogs.com/YXRsYXM/rs:fit/g:sm/q:90/h:600/w:600/UjQwMDAx.jpeg";
    let atlas = found(&[(40001, "Bicep - Atlas", "2020", atlas_cover)]);
    let session = start(true, &[], move |url, before| match url {
        BICEP_COVER => status(404),
        ISLES_COVER => ok("<html>not an image</html>"),
        GLUE_COVER => ok(jpeg(8)),
        _ if url == atlas_cover => Reply::Fails("external"),
        _ if url.contains("release_title=Opal") => ok(opal.clone()),
        _ if url.contains("release_title=Atlas") => ok(atlas.clone()),
        _ => as_they_are(url, before),
    });
    let tracks = [
        tags(1, "Bicep", "Bicep", "Glue"),
        tags(2, "Bicep", "Isles", "Atlas"),
        tags(3, "Bicep", "", "Glue"),
        tags(4, "Bicep", "Opal", "Opal"),
        tags(5, "Bicep", "Atlas", "Atlas"),
    ];
    let sent = session.send(ProviderKind::Artwork, &tracks);
    let candidates: Candidates = sent.wait().unwrap();
    assert_eq!(candidates.candidates, Vec::new());
    let warnings = &candidates.warnings;
    assert_eq!(warnings.len(), 6, "{warnings:?}");
    assert_eq!(warnings[0], format!("Bicep - Bicep, {BICEP_ALBUM}: its cover didn't download: Discogs answered 404"));
    assert!(
        warnings[1].starts_with(&format!("Bicep - Isles, {ISLES_ALBUM}: its cover isn't an image: ")),
        "{warnings:?}"
    );
    assert_eq!(warnings[2], format!("Bicep - Glue, {BICEP_ON_GLUE}: its cover didn't download: Discogs answered 404"));
    assert_eq!(warnings[3], format!("Bicep - Glue, {GLUE_SINGLE}: its cover is too small (8 x 8 px)"));
    assert_eq!(warnings[4], "Bicep - Opal, Opal, 2017: Discogs has no cover for it");
    assert_eq!(warnings[5], "Bicep - Atlas, Atlas, 2020: its cover didn't download: no answer (external)");
    assert_eq!(session.covers(), [BICEP_COVER, ISLES_COVER, GLUE_COVER, atlas_cover]);
    assert_eq!(std::fs::read_dir(sent.scratch().unwrap()).unwrap().count(), 0);
}

/// Discogs out of reach, or answering what isn't a search result, fails the request: every album
/// would.
#[test]
fn discogs_out_of_reach_fails_it() {
    let tracks = [tags(1, "Bicep", "Bicep", "Glue")];
    let session = start(true, &[], |_, _| Reply::Fails("external"));
    let e = session.send(ProviderKind::Metadata, &tracks).wait::<Candidates>().unwrap_err();
    assert_eq!((e.kind(), e.message.as_str()), (Some("external"), "couldn't reach Discogs: no answer (external)"));
    let session = start(true, &[], |_, _| ok("<html>busy</html>"));
    let e = session.send(ProviderKind::Metadata, &tracks).wait::<Candidates>().unwrap_err();
    assert_eq!(e.kind(), Some("external"));
    assert!(e.message.starts_with("Discogs answered what isn't a search result: "), "{e:?}");
}

/// Cancelled while it waits out Discogs' limit, it stops there, and searches no more.
#[test]
fn cancelling_while_it_waits_stops_it() {
    let session = start(true, &[], |_, _| too_many(Some(&LONGEST_WAIT.to_string())));
    let sent = session.send(ProviderKind::Metadata, &[tags(1, "Bicep", "Bicep", "Glue")]);
    session.heard.recv().unwrap();
    sent.cancel();
    assert_eq!(sent.wait::<Candidates>().unwrap_err(), RpcError::cancelled());
    assert_eq!(session.searches().len(), 1);
}

/// Text drawn for a tag: blank, Unicode, or with what a search's query has to encode.
#[hegel::test_helper]
fn drawn_tag(tc: &TestCase) -> String {
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => String::new(),
        1 => tc
            .draw(gs::sampled_from(vec![" ", "Bicep", "BICEP ", "Isles", "Glue", "Röyksopp & Robyn", "a=b?c#d"]))
            .into(),
        _ => tc.draw(gs::text().max_size(12)),
    }
}

/// Whatever tracks' tags and labels (any text), and whatever Discogs answers (releases, nothing,
/// over its limit, refusing, or no account), the provider never fails nor panics: its candidates
/// are only for tracks asked about that have an album or a title, each scored as a lookup can be,
/// a cover a file in the scratch folder or tags with an album; each track with neither is a
/// warning; and nothing is searched for a track with nothing to look up.
#[hegel::test(test_cases = 40)]
fn prop_any_tracks_get_candidates_only_for_what_they_asked(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(6));
    let tracks: Vec<TrackTags> = (1..=n)
        .map(|id| TrackTags {
            album_artist: drawn_tag(&tc),
            isrc: drawn_tag(&tc),
            ..tags(id, &drawn_tag(&tc), &drawn_tag(&tc), &drawn_tag(&tc))
        })
        .collect();
    let labels: Vec<(u32, String)> = (1..=n).map(|id| (id, drawn_tag(&tc))).collect();
    let answer = tc.draw(gs::sampled_from(vec![
        ok(BICEP),
        ok(GLUE),
        ok(ISLES),
        ok(NOTHING),
        too_many(Some("0")),
        status(500),
        status(401),
    ]));
    let image = tc.draw(gs::sampled_from(vec![ok(jpeg(600)), status(404)]));
    let signed_in = tc.draw(gs::weighted_booleans(0.9));
    let kind = tc.draw(gs::sampled_from(vec![ProviderKind::Artwork, ProviderKind::Metadata]));

    let labelled: Vec<(u32, &str)> = labels.iter().map(|(id, l)| (*id, l.as_str())).collect();
    let session = start(signed_in, &labelled, move |url, _| match url.starts_with(IMAGES) {
        true => image.clone(),
        false => answer.clone(),
    });
    let sent = session.send(kind, &tracks);
    let candidates: Candidates = sent.wait().unwrap();
    let blank = |t: &TrackTags| t.album.trim().is_empty() && t.title.trim().is_empty();
    for c in &candidates.candidates {
        let t = tracks.iter().find(|t| t.id == c.track).expect("a track asked about");
        assert!(!blank(t), "{c:?}");
        assert!([BY_LABEL, BY_NAME, BY_TITLE, ONE_OF_SEVERAL].contains(&c.score), "{c:?}");
        match kind {
            ProviderKind::Artwork => assert!(sent.scratch().unwrap().join(&c.artwork.as_ref().unwrap().blob).is_file()),
            ProviderKind::Metadata => assert!(c.metadata.as_ref().unwrap().album.is_some(), "{c:?}"),
        }
    }
    for t in tracks.iter().filter(|t| blank(t)) {
        assert!(candidates.warnings.contains(&format!("track #{} has no album or title to look up", t.id)));
    }
    let lookups = tracks.iter().filter(|t| !blank(t)).count();
    assert!(session.searches().len() <= 2 * TRIES * lookups, "{:?}", session.searches());
}

/// Whichever albums (with or without a record label) and tracks without one are asked about, when
/// Discogs finds nothing: each album is searched by its label (its first track's that has one),
/// then by name, else by name alone; each track without an album by its title; in the order they
/// first come, each once; and each is a warning.
#[hegel::test(test_cases = 40)]
fn prop_lookups_search_by_label_then_name_or_by_title(tc: TestCase) {
    let n = tc.draw(gs::integers::<u32>().max_value(6));
    // Each track: an album (or none, a title), and a label.
    let drawn: Vec<(Option<&str>, &str, &str)> = (1..=n)
        .map(|_| {
            let album = tc.draw(gs::sampled_from(vec![Some("Bicep"), Some("Isles"), None]));
            let title = tc.draw(gs::sampled_from(vec!["Glue", "Atlas"]));
            (album, title, tc.draw(gs::sampled_from(vec!["", "Ninja Tune", "XL"])))
        })
        .collect();
    let tracks: Vec<TrackTags> = drawn
        .iter()
        .zip(1..)
        .map(|(&(album, title, _), id)| tags(id, "Bicep", album.unwrap_or_default(), title))
        .collect();
    let labels: Vec<(u32, &str)> = drawn.iter().zip(1..).map(|(&(_, _, label), id)| (id, label)).collect();
    let session = start(true, &labels, |_, _| ok(NOTHING));
    let candidates: Candidates = session.send(ProviderKind::Metadata, &tracks).wait().unwrap();

    // What a lookup is: an album's name or a track's title, whether it's a title, and a label.
    let mut lookups: Vec<(&str, bool, Option<&str>)> = Vec::new();
    for &(album, title, label) in &drawn {
        let (name, single) = album.map_or((title, true), |album| (album, false));
        let label = Some(label).filter(|l| !l.is_empty() && !single);
        match lookups.iter_mut().find(|(n, s, _)| (*n, *s) == (name, single)) {
            Some(had) => had.2 = had.2.or(label),
            None => lookups.push((name, single, label)),
        }
    }
    let mut expected = Vec::new();
    for &(name, single, label) in &lookups {
        if single {
            expected.push(search(&[("artist", "Bicep"), ("track", name)]));
            continue;
        }
        if let Some(label) = label {
            expected.push(search(&[("artist", "Bicep"), ("release_title", name), ("label", label)]));
        }
        expected.push(search(&[("artist", "Bicep"), ("release_title", name)]));
    }
    assert_eq!(session.searches(), expected);
    let not_found: Vec<String> = lookups.iter().map(|(name, _, _)| format!("Bicep - {name}: not on Discogs")).collect();
    assert_eq!(candidates, Candidates { candidates: Vec::new(), warnings: not_found });
}

/// However many times Discogs answers 429 before it answers a search, the search is sent again
/// until it's found, [`TRIES`] times at most, after which the album is a warning.
#[hegel::test(test_cases = 15)]
fn prop_a_search_over_the_limit_is_sent_again_a_few_times(tc: TestCase) {
    let busy = tc.draw(gs::integers::<usize>().max_value(TRIES + 1));
    let session = start(true, &[], move |url, before| match before < busy {
        true => too_many(Some("0")),
        false => as_they_are(url, before),
    });
    let candidates: Candidates =
        session.send(ProviderKind::Metadata, &[tags(1, "Bicep", "Bicep", "Glue")]).wait().unwrap();
    assert_eq!(session.searches().len(), (busy + 1).min(TRIES));
    match busy < TRIES {
        true => assert_eq!(candidates.candidates.len(), 1, "{candidates:?}"),
        false => assert_eq!(candidates.warnings, ["Bicep - Bicep: left out, as Discogs answered 429"]),
    }
}
