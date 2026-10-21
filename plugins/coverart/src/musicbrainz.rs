//! Which MusicBrainz release groups an album could be: found by one of its tracks' ISRC when it
//! has one (a recording search, which names the releases it's on), else by a release search for
//! the album's name and artist. Only releases named as the album is count; their release groups
//! are what the album could be, one alone being surely it. The Cover Art Archive has their covers.

use loclib_model::model::normalize;
use loclib_plugin::{Fetch, Host, RpcError};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::Album;

/// Where MusicBrainz is.
pub(crate) const MUSICBRAINZ: &str = "https://musicbrainz.org";

/// What a recording search answers, as far as this plugin reads it.
#[derive(Deserialize)]
struct Recordings {
    recordings: Vec<Recording>,
}

#[derive(Deserialize)]
struct Recording {
    #[serde(default)]
    releases: Vec<Release>,
}

/// What a release search answers.
#[derive(Deserialize)]
struct Releases {
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    title: String,
    #[serde(rename = "release-group")]
    release_group: Option<ReleaseGroup>,
}

#[derive(Deserialize)]
pub(crate) struct ReleaseGroup {
    pub(crate) id: String,
    pub(crate) title: String,
    /// `Album`, `Single`, `EP`...
    #[serde(rename = "primary-type")]
    pub(crate) primary_type: Option<String>,
}

/// What looking an album up found.
pub(crate) enum Found {
    /// The release groups of releases named as the album is, as MusicBrainz lists them (none, or
    /// several), and whether they were found by an ISRC.
    Groups { groups: Vec<ReleaseGroup>, by_isrc: bool },
    /// MusicBrainz didn't answer the search (`503`, each time: it's busy).
    Refused(u16),
}

/// The release groups `album` could be. Fails when MusicBrainz can't be reached at all, or when
/// the hook is cancelled.
pub(crate) fn release_group(host: &Host, album: &Album) -> Result<Found, RpcError> {
    if let Some(isrc) = &album.isrc {
        let groups = match search(host, "recording", &format!("isrc:{isrc}"))? {
            Ok(Recordings { recordings }) => named(recordings.into_iter().flat_map(|r| r.releases), &album.name),
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
        Ok(Releases { releases }) => Found::Groups { groups: named(releases.into_iter(), &album.name), by_isrc: false },
        Err(status) => Found::Refused(status),
    })
}

/// How many times a search is sent while MusicBrainz answers 503, which it does when too many
/// requests come at once (from everyone, at busy times), each waiting its turn at the host.
const TRIES: usize = 3;

/// Searches MusicBrainz for `entity` (`release`, `recording`) with the Lucene `query`: the answer,
/// or the status it was refused with.
fn search<T: DeserializeOwned>(host: &Host, entity: &str, query: &str) -> Result<Result<T, u16>, RpcError> {
    let params =
        form_urlencoded::Serializer::new(String::new()).append_pair("query", query).append_pair("fmt", "json").finish();
    let fetch = Fetch::get(format!("{MUSICBRAINZ}/ws/2/{entity}/?{params}"));
    let mut tries = 1;
    loop {
        let fetched = host.fetch(&fetch).map_err(|e| match e.kind() {
            Some("cancelled") => e,
            _ => RpcError { message: format!("couldn't reach MusicBrainz: {}", e.message), ..e },
        })?;
        eprintln!("coverart: MusicBrainz {entity} search {query:?}: {}", fetched.status);
        match fetched.status {
            503 if tries < TRIES => tries += 1,
            200 => {
                let answer = serde_json::from_str(fetched.body.as_deref().unwrap_or_default()).map_err(|e| {
                    RpcError::failed("external", format!("MusicBrainz answered what isn't a search result: {e}"))
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

/// The release groups of `releases` named as `name` is, each once, in their order.
fn named(releases: impl Iterator<Item = Release>, name: &str) -> Vec<ReleaseGroup> {
    let mut groups: Vec<ReleaseGroup> = Vec::new();
    for group in releases.filter(|r| same(&r.title, name)).filter_map(|r| r.release_group) {
        // An id goes in the Cover Art Archive's URL: only one shaped as MusicBrainz's are.
        let is_mbid = group.id.len() == 36 && group.id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if is_mbid && !groups.iter().any(|g| g.id == group.id) {
            groups.push(group);
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
