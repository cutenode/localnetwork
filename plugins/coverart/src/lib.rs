//! The coverart plugin: cover art found online for the tracks locallibrary asks about (an artwork
//! provider). Each album is looked up once on MusicBrainz (see `musicbrainz`) and the front cover
//! of each release group it could be downloaded from the Cover Art Archive (see `covers`), a
//! candidate for each of the album's tracks, which locallibrary offers, sets once chosen, and
//! journals. The host makes its requests (`net/fetch`), a second apart for MusicBrainz as the
//! manifest asks, and the covers go to it as files in the request's scratch folder, which it
//! removes once it has read them: nothing downloaded is kept, as it's cheap to fetch again.

mod covers;
mod musicbrainz;

use std::fmt;
use std::io::{Read, Write};
use std::path::Path;

use indexmap::IndexMap;
use loclib_model::model::normalize;
use loclib_plugin::{Blob, Candidate, Candidates, Hooks, Host, ProvideCandidates, RpcError, TrackTags};
use loclib_progress::{Ctx, Event, Phase};

use musicbrainz::{Found, ReleaseGroup};

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(provide), input, output);
}

/// Where the covers are from, as candidates say.
const SOURCE: &str = "Cover Art Archive";

/// A cover of the one release group an album's ISRC is on: an ISRC names the recording, so it's
/// surer than a name, which another album can have too.
const BY_ISRC: f64 = 0.95;
/// A cover of the one release group named as the album is.
const BY_NAME: f64 = 0.9;
/// A cover of one of several release groups named as the album is: the user says which.
const ONE_OF_SEVERAL: f64 = 0.5;
/// The most release groups an album's covers are offered from: each is a download.
const MAX_GROUPS: usize = 5;

/// Tracks looked up together: an album's, or a track without one, by its title (a single).
pub(crate) struct Album {
    /// The album's artist, else the track's.
    pub(crate) artist: String,
    /// The album's name, else the track's title.
    pub(crate) name: String,
    /// The first of its tracks' ISRCs.
    pub(crate) isrc: Option<String>,
    /// Its tracks' ids.
    tracks: Vec<u32>,
}

impl fmt::Display for Album {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.artist.is_empty() {
            true => f.write_str(&self.name),
            false => write!(f, "{} - {}", self.artist, self.name),
        }
    }
}

/// `provide/candidates`: for each album of the tracks asked about, a candidate for each of its
/// tracks of each release group's cover found (of at most [`MAX_GROUPS`]), and a warning for each
/// album with none.
fn provide(req: ProvideCandidates, host: &Host) -> Result<Candidates, RpcError> {
    let scratch = req.context.scratch.as_deref().ok_or_else(|| {
        RpcError::invalid_params("the host gives the plugin no scratch folder for the covers (the blobs feature)")
    })?;
    let mut warnings = Vec::new();
    let albums = albums(&req.tracks, &mut warnings);
    let ctx = host.ctx();
    ctx.phase_start(Phase::Scan, albums.len() as u64, 0);
    let candidates = candidates(host, scratch, &albums, &ctx, &mut warnings)?;
    ctx.phase_end(Phase::Scan);
    Ok(Candidates { candidates, warnings })
}

/// The candidates for `albums`' tracks, each cover a file in `scratch` downloaded once for each
/// release group. Why an album has none, or a release group's cover can't be had, is a warning.
fn candidates(
    host: &Host,
    scratch: &Path,
    albums: &[Album],
    ctx: &Ctx<'_>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Candidate>, RpcError> {
    // Each release group's cover, by its id: its file, or why there's none.
    let mut covers: IndexMap<String, Result<String, String>> = IndexMap::new();
    let mut found = Vec::new();
    // Each album's requests notice the hook being cancelled.
    for album in albums {
        ctx.emit(Event::Item { phase: Phase::Scan, path: album.to_string(), bytes: 0 });
        let (groups, by_isrc) = match musicbrainz::release_group(host, album)? {
            Found::Groups { groups, .. } if groups.is_empty() => {
                warnings.push(format!("{album}: not on MusicBrainz"));
                continue;
            }
            Found::Groups { groups, by_isrc } => (groups, by_isrc),
            Found::Refused(status) => {
                warnings.push(format!("{album}: left out, as MusicBrainz answered {status}"));
                continue;
            }
        };
        let score = match (groups.len(), by_isrc) {
            (1, true) => BY_ISRC,
            (1, false) => BY_NAME,
            _ => ONE_OF_SEVERAL,
        };
        let several = groups.len() > 1;
        for group in groups.into_iter().take(MAX_GROUPS) {
            let label = label(&group);
            let cover = match covers.get(&group.id) {
                Some(cover) => cover.clone(),
                None => {
                    let cover = covers::front(host, scratch, &group.id)?;
                    covers.insert(group.id, cover.clone());
                    cover
                }
            };
            match cover {
                Ok(blob) => found.extend(album.tracks.iter().map(|&track| Candidate {
                    track,
                    score,
                    source: SOURCE.into(),
                    label: label.clone(),
                    artwork: Some(Blob { blob: blob.clone() }),
                    metadata: None,
                })),
                Err(why) if several => warnings.push(format!("{album}, {label}: {why}")),
                Err(why) => warnings.push(format!("{album}: {why}")),
            }
        }
    }
    Ok(found)
}

/// What a release group's cover is shown as: its title and its kind (`Discovery, album`).
fn label(group: &ReleaseGroup) -> String {
    let kind = group.primary_type.as_deref().unwrap_or("release").to_lowercase();
    format!("{}, {kind}", group.title)
}

/// `tracks` by album (by artist and name, ignoring letter case and spacing), in the order they
/// come. A track with neither album nor title is warned about.
fn albums(tracks: &[TrackTags], warnings: &mut Vec<String>) -> Vec<Album> {
    let mut albums: IndexMap<(String, String), Album> = IndexMap::new();
    for t in tracks {
        let (artist, name) = (first([&t.album_artist, &t.artist]), first([&t.album, &t.title]));
        if name.is_empty() {
            warnings.push(format!("track #{} has no album or title to look up", t.id));
            continue;
        }
        let album = albums.entry((normalize(artist), normalize(name))).or_insert_with(|| Album {
            artist: artist.into(),
            name: name.into(),
            isrc: None,
            tracks: Vec::new(),
        });
        album.isrc = album.isrc.take().or_else(|| isrc(&t.isrc));
        album.tracks.push(t.id);
    }
    albums.into_values().collect()
}

/// The first of `tags` that isn't blank, trimmed.
fn first(tags: [&str; 2]) -> &str {
    tags.into_iter().map(str::trim).find(|s| !s.is_empty()).unwrap_or_default()
}

/// `tag` as an ISRC: twelve letters and digits, without the hyphens and spaces some tags have.
fn isrc(tag: &str) -> Option<String> {
    let isrc: String = tag.chars().filter(|c| !matches!(c, '-' | ' ')).collect::<String>().to_ascii_uppercase();
    (isrc.len() == 12 && isrc.chars().all(|c| c.is_ascii_alphanumeric())).then_some(isrc)
}

#[cfg(test)]
mod tests;
