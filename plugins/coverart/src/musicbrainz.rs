//! Which MusicBrainz release groups an album could be: found by one of its tracks' ISRC when it
//! has one (a recording search, which names the releases it's on), else by a release search for
//! the album's name and artist. Only releases named as the album is count; their release groups
//! are what the album could be, one alone being surely it. The Cover Art Archive has their covers;
//! the releases say the album's year and label, the recording an ISRC names its title and artist,
//! and a release group looked up on its own its genres.

use loclib_model::model::normalize;
use loclib_plugin::{Fetch, Host, RpcError};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::Album;

/// Where MusicBrainz is.
pub(crate) const MUSICBRAINZ: &str = "https://musicbrainz.org";

/// The label MusicBrainz credits a release with when it came out on none.
const NO_LABEL: &str = "[no label]";

/// What a recording search answers, as far as this plugin reads it.
#[derive(Deserialize)]
struct Recordings {
    recordings: Vec<Recording>,
}

#[derive(Deserialize)]
struct Recording {
    title: String,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<Credit>,
    #[serde(default)]
    releases: Vec<Release>,
}

/// An artist credited, as the credit names them, and what joins them to the next (` & `,
/// ` feat. `).
#[derive(Deserialize)]
struct Credit {
    name: String,
    #[serde(default)]
    joinphrase: String,
}

/// What a release search answers.
#[derive(Deserialize)]
struct Releases {
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    title: String,
    /// `2018-03-09`, `2018-03` or `2018`.
    #[serde(default)]
    date: String,
    /// Only a release search's releases have it.
    #[serde(default, rename = "label-info")]
    label_info: Vec<LabelInfo>,
    #[serde(rename = "release-group")]
    release_group: Option<ReleaseGroup>,
}

/// A label a release came out on (a catalog number alone has none).
#[derive(Deserialize)]
struct LabelInfo {
    label: Option<Label>,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
pub(crate) struct ReleaseGroup {
    pub(crate) id: String,
    pub(crate) title: String,
    /// `Album`, `Single`, `EP`...
    #[serde(rename = "primary-type")]
    pub(crate) primary_type: Option<String>,
}

/// A release group an album could be, with what its releases named as the album say of it.
pub(crate) struct Group {
    pub(crate) group: ReleaseGroup,
    /// The first release's title: the album as MusicBrainz spells it.
    pub(crate) album: String,
    /// The year the earliest release came out: a reissue's isn't the album's.
    pub(crate) year: Option<u16>,
    /// The first record label a release names.
    pub(crate) label: Option<String>,
    /// The recording the album's ISRC names, when it was found by one: its title and artist.
    pub(crate) recording: Option<(String, String)>,
}

/// What looking an album up found.
pub(crate) enum Found {
    /// The release groups of releases named as the album is, as MusicBrainz lists them (none, or
    /// several), and whether they were found by an ISRC.
    Groups { groups: Vec<Group>, by_isrc: bool },
    /// MusicBrainz didn't answer the search (`503`, each time: it's busy).
    Refused(u16),
}

/// The release groups `album` could be. Fails when MusicBrainz can't be reached at all, or when
/// the hook is cancelled.
pub(crate) fn release_group(host: &Host, album: &Album) -> Result<Found, RpcError> {
    if let Some(isrc) = &album.isrc {
        let groups = match search(host, "recording", &format!("isrc:{isrc}"))? {
            Ok(Recordings { recordings }) => {
                let releases = recordings.into_iter().flat_map(|r| {
                    let artist: String = r.artist_credit.iter().flat_map(|c| [&*c.name, &*c.joinphrase]).collect();
                    let recording = (r.title, artist);
                    r.releases.into_iter().map(move |release| (release, Some(recording.clone())))
                });
                named(releases, &album.name)
            }
            Err(status) => return Ok(Found::Refused(status)),
        };
        // Not on MusicBrainz by its ISRC, or not on a release of that name: by name, then.
        if !groups.is_empty() {
            return Ok(Found::Groups { groups, by_isrc: true });
        }
    }
    let mut query = phrase("release", &album.name);
    if !album.artist.is_empty() {
        query = format!("{query} AND {}", phrase("artist", &album.artist));
    }
    Ok(match search(host, "release", &query)? {
        Ok(Releases { releases }) => {
            let groups = named(releases.into_iter().map(|release| (release, None)), &album.name);
            Found::Groups { groups, by_isrc: false }
        }
        Err(status) => Found::Refused(status),
    })
}

/// What a release group's lookup answers with its genres, as far as this plugin reads it.
#[derive(Deserialize)]
struct Genres {
    #[serde(default)]
    genres: Vec<Genre>,
}

#[derive(Deserialize)]
struct Genre {
    name: String,
    /// How many voted for it.
    count: u32,
}

/// Release group `id`'s genre, the one most voted for (of equals, the first: MusicBrainz lists
/// them by name), or the status MusicBrainz refused it with. Fails as a search does.
pub(crate) fn genre(host: &Host, id: &str) -> Result<Result<Option<String>, u16>, RpcError> {
    let path = format!("release-group/{id}?inc=genres&fmt=json");
    let genres = get::<Genres>(host, &path, &format!("release group {id}"), "release group")?;
    // `max_by_key` gives the last of equals: reversed, that's the first.
    Ok(genres.map(|g| g.genres.into_iter().rev().max_by_key(|g| g.count).map(|g| g.name)))
}

/// How many times a request is sent while MusicBrainz answers 503, which it does when too many
/// requests come at once (from everyone, at busy times), each waiting its turn at the host.
const TRIES: usize = 3;

/// Searches MusicBrainz for `entity` (`release`, `recording`) with the Lucene `query`: the answer,
/// or the status it was refused with.
fn search<T: DeserializeOwned>(host: &Host, entity: &str, query: &str) -> Result<Result<T, u16>, RpcError> {
    let params =
        form_urlencoded::Serializer::new(String::new()).append_pair("query", query).append_pair("fmt", "json").finish();
    get(host, &format!("{entity}/?{params}"), &format!("{entity} search {query:?}"), "search result")
}

/// Asks MusicBrainz's API for `path` (`release/?query=...`): its answer, a `what` (`asked`, as
/// the log says), or the status it was refused with.
fn get<T: DeserializeOwned>(host: &Host, path: &str, asked: &str, what: &str) -> Result<Result<T, u16>, RpcError> {
    let fetch = Fetch::get(format!("{MUSICBRAINZ}/ws/2/{path}"));
    let mut tries = 1;
    loop {
        let fetched = host.fetch(&fetch).map_err(|e| match e.kind() {
            Some("cancelled") => e,
            _ => RpcError { message: format!("couldn't reach MusicBrainz: {}", e.message), ..e },
        })?;
        eprintln!("coverart: MusicBrainz {asked}: {}", fetched.status);
        match fetched.status {
            503 if tries < TRIES => tries += 1,
            200 => {
                let answer = serde_json::from_str(fetched.body.as_deref().unwrap_or_default()).map_err(|e| {
                    RpcError::failed("external", format!("MusicBrainz answered what isn't a {what}: {e}"))
                });
                return answer.map(Ok);
            }
            status => return Ok(Err(status)),
        }
    }
}

/// `value` as a Lucene phrase in `field`.
fn phrase(field: &str, value: &str) -> String {
    format!("{field}:\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The release groups of `releases` (each with the recording it was found by, if one) named as
/// `name` is, each once, in their order, with what those releases say of it.
fn named(releases: impl Iterator<Item = (Release, Option<(String, String)>)>, name: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for (release, recording) in releases.filter(|(r, _)| same(&r.title, name)) {
        let Some(group) = release.release_group else { continue };
        // An id goes in the Cover Art Archive's URL: only one shaped as MusicBrainz's are.
        let is_mbid = group.id.len() == 36 && group.id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if !is_mbid {
            continue;
        }
        let year = release.date.get(..4).and_then(|y| y.parse().ok());
        let label = release.label_info.into_iter().filter_map(|l| l.label).map(|l| l.name).find(|l| l != NO_LABEL);
        match groups.iter_mut().find(|g| g.group.id == group.id) {
            Some(had) => {
                had.year = had.year.into_iter().chain(year).min();
                had.label = had.label.take().or(label);
            }
            None => groups.push(Group { group, album: release.title, year, label, recording }),
        }
    }
    groups
}

/// Whether two titles are the same but for letter case, spacing, Unicode forms, and the curly
/// quotes and hyphens MusicBrainz writes where tags have straight ones.
fn same(a: &str, b: &str) -> bool {
    let plain = |s: &str| normalize(s).replace(['\u{2018}', '\u{2019}'], "'").replace(['\u{2010}', '\u{2011}'], "-");
    plain(a) == plain(b)
}
