//! Which MusicBrainz release group an album is: found by one of its tracks' ISRC when it has one
//! (a recording search, which names the releases it's on), else by a release search for the
//! album's name and artist. Only releases named as the album is count, and they must all be of one
//! release group: the album's, whose cover the Cover Art Archive has.

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
    One(ReleaseGroup),
    /// Nothing named as the album is.
    None,
    /// Releases of several release groups are named as the album is.
    Several(Vec<ReleaseGroup>),
    /// MusicBrainz didn't answer the search (`503`, each time: it's busy).
    Refused(u16),
}

/// The release group `album` is. Fails when MusicBrainz can't be reached at all, or when the
/// operation is cancelled.
pub(crate) fn release_group(host: &Host, album: &Album) -> Result<Found, RpcError> {
    if let Some(isrc) = &album.isrc {
        let found = match search(host, "recording", &format!("isrc:{isrc}"))? {
            Ok(Recordings { recordings }) => one(recordings.into_iter().flat_map(|r| r.releases), &album.name),
            Err(status) => return Ok(Found::Refused(status)),
        };
        // Not on MusicBrainz by its ISRC, or not on a release of that name: by name, then.
        if !matches!(found, Found::None) {
            return Ok(found);
        }
    }
    let mut query = phrase("release", &album.name);
    if !album.artist.is_empty() {
        query = format!("{query} AND {}", phrase("artist", &album.artist));
    }
    Ok(match search(host, "release", &query)? {
        Ok(Releases { releases }) => one(releases.into_iter(), &album.name),
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

/// The one release group of `releases` named as `name` is.
fn one(releases: impl Iterator<Item = Release>, name: &str) -> Found {
    let mut groups: Vec<ReleaseGroup> = Vec::new();
    for group in releases.filter(|r| same(&r.title, name)).filter_map(|r| r.release_group) {
        // An id goes in the Cover Art Archive's URL: only one shaped as MusicBrainz's are.
        let is_mbid = group.id.len() == 36 && group.id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if is_mbid && !groups.iter().any(|g| g.id == group.id) {
            groups.push(group);
        }
    }
    match groups.len() {
        0 => Found::None,
        1 => Found::One(groups.remove(0)),
        _ => Found::Several(groups),
    }
}

/// Whether two titles are the same but for letter case, spacing, Unicode forms, and the curly
/// quotes and hyphens MusicBrainz writes where tags have straight ones.
fn same(a: &str, b: &str) -> bool {
    let plain = |s: &str| normalize(s).replace(['\u{2018}', '\u{2019}'], "'").replace(['\u{2010}', '\u{2011}'], "-");
    plain(a) == plain(b)
}
