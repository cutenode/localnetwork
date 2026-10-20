//! The coverart plugin: cover art found online for tracks on a drive that have none (or, asked to,
//! for any). Each album is looked up once on MusicBrainz (see `musicbrainz`) and its front cover
//! downloaded from the Cover Art Archive (see `covers`); the plan sets it on the album's tracks
//! with `set_artwork` edits, which locallibrary plans, confirms, journals and applies. The host
//! makes its requests (`net/fetch`), a second apart for MusicBrainz as the manifest asks, and the
//! covers go to it as files in the request's scratch folder, which it removes once it has read
//! them: nothing downloaded is kept, as it's cheap to fetch again.

mod covers;
mod musicbrainz;

use std::fmt;
use std::io::{Read, Write};
use std::path::Path;

use indexmap::IndexMap;
use indexmap::map::Entry;
use loclib_model::model::{Track, normalize};
use loclib_plugin::{Hooks, Host, OpPlan, PlanOp, RpcError, TracksById, TracksQuery, params};
use loclib_progress::{Ctx, Event, Phase};
use serde::Deserialize;
use serde_json::json;

use musicbrainz::Found;

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(plan_op), input, output);
}

/// The `fetch` operation's params.
#[derive(Default, Deserialize)]
struct Fetch {
    #[serde(default)]
    tracks: Vec<u32>,
    #[serde(default)]
    playlist: Option<String>,
    #[serde(default)]
    replace: bool,
}

/// Tracks looked up together: an album's, or a track without one, by its title (a single).
pub(crate) struct Album {
    /// The album's artist, else the track's.
    pub(crate) artist: String,
    /// The album's name, else the track's title.
    pub(crate) name: String,
    /// The first of its tracks' ISRCs.
    pub(crate) isrc: Option<String>,
    /// Its tracks' audio files.
    paths: Vec<String>,
}

impl fmt::Display for Album {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.artist.is_empty() {
            true => f.write_str(&self.name),
            false => write!(f, "{} - {}", self.artist, self.name),
        }
    }
}

/// `fetch`: a `set_artwork` edit for each album whose cover was found, giving it to the album's
/// tracks that lack artwork (all of them, with `replace`). Which tracks: `tracks`, else
/// `playlist`, else the ones selected, else the playlist open, else the whole drive.
fn plan_op(req: PlanOp, host: &Host) -> Result<OpPlan, RpcError> {
    let Fetch { tracks, playlist, replace } = params::<Option<Fetch>>(req.params)?.unwrap_or_default();
    let context = req.context;
    let drive = context.drive.as_deref().ok_or_else(|| RpcError::invalid_params("say which drive"))?;
    let scratch = context.scratch.as_deref().ok_or_else(|| {
        RpcError::invalid_params("the host gives the plugin no scratch folder for the covers (the blobs feature)")
    })?;
    // What the params name wins over what's selected: a form fills `tracks` in with the
    // selection, so a playlist typed in with `tracks` emptied is what's wanted.
    let (ids, playlist) = match tracks.is_empty() && playlist.is_none() {
        true => (context.tracks.clone(), context.playlist.clone()),
        false => (tracks, playlist),
    };
    let mut warnings = Vec::new();
    let chosen = chosen(host, drive, &ids, playlist.as_deref(), &mut warnings)?;
    // Tracks without artwork, as locallibrary's `art auto` picks them. It also takes tracks
    // whose image file is gone from the drive, which a plugin can't see.
    let albums = albums(chosen.into_iter().filter(|t| replace || t.artwork_path.is_none()), &mut warnings);

    let ctx = host.ctx();
    ctx.phase_start(Phase::Scan, albums.len() as u64, 0);
    let covers = covers(host, scratch, &albums, &ctx, &mut warnings)?;
    ctx.phase_end(Phase::Scan);

    let tracks: usize = covers.iter().map(|(_, paths)| paths.len()).sum();
    let summary = match (covers.len(), albums.len()) {
        (_, 0) => "no track chosen needs cover art".to_string(),
        (0, n) => format!("found no cover art for the {n} album(s) looked up"),
        (found, _) => format!("set cover art from the Cover Art Archive on {tracks} track(s) of {found} album(s)"),
    };
    let edits = covers
        .into_iter()
        .map(|(blob, paths)| json!({ "op": "set_artwork", "tracks": { "paths": paths }, "image": { "blob": blob } }));
    Ok(OpPlan { summary, warnings, edits: edits.collect(), inputs: Vec::new() })
}

/// The covers of `albums` found, each a file in `scratch` (by its name there) with the tracks it
/// goes on: one for each release group, downloaded once. Why an album has none is a warning.
fn covers(
    host: &Host,
    scratch: &Path,
    albums: &[Album],
    ctx: &Ctx<'_>,
    warnings: &mut Vec<String>,
) -> Result<Vec<(String, Vec<String>)>, RpcError> {
    let mut covers: IndexMap<String, Result<(String, Vec<String>), String>> = IndexMap::new();
    // Each album's requests notice the operation being cancelled.
    for album in albums {
        ctx.emit(Event::Item { phase: Phase::Scan, path: album.to_string(), bytes: 0 });
        let group = match musicbrainz::release_group(host, album)? {
            Found::One(group) => group,
            Found::None => {
                warnings.push(format!("{album}: not on MusicBrainz"));
                continue;
            }
            Found::Several(groups) => {
                let named = groups.iter().map(|g| {
                    let kind = g.primary_type.as_deref().unwrap_or("release");
                    format!("{} {kind} {}/release-group/{}", g.title, musicbrainz::MUSICBRAINZ, g.id)
                });
                let named = named.collect::<Vec<_>>().join(", ");
                warnings.push(format!("{album}: left out, as MusicBrainz has several of that name: {named}"));
                continue;
            }
            Found::Refused(status) => {
                warnings.push(format!("{album}: left out, as MusicBrainz answered {status}"));
                continue;
            }
        };
        let cover = match covers.entry(group.id) {
            Entry::Occupied(had) => had.into_mut(),
            Entry::Vacant(new) => {
                let front = covers::front(host, scratch, new.key())?;
                new.insert(front.map(|blob| (blob, Vec::new())))
            }
        };
        match cover {
            Ok((_, paths)) => paths.extend(album.paths.iter().cloned()),
            Err(why) => warnings.push(format!("{album}: {why}")),
        }
    }
    Ok(covers.into_values().flatten().collect())
}

/// The drive's tracks the user chose: `ids`, else `playlist`'s, else every one. Ids the drive
/// doesn't have are warned about. Not `combined`: an edit can't name a track only OneLibrary has.
fn chosen(
    host: &Host,
    drive: &str,
    ids: &[u32],
    playlist: Option<&str>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Track>, RpcError> {
    let drive = host.drive(drive);
    if ids.is_empty() {
        return drive.query(&TracksQuery { playlist: playlist.map(String::from), ..TracksQuery::default() });
    }
    let TracksById { tracks, missing } = drive.tracks(ids)?;
    warnings.extend(missing.iter().map(|id| format!("track #{id} isn't on the drive")));
    Ok(tracks)
}

/// `tracks` by album (by artist and name, ignoring letter case and spacing), in the order they
/// come. A track with neither album nor title is warned about.
fn albums(tracks: impl Iterator<Item = Track>, warnings: &mut Vec<String>) -> Vec<Album> {
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
            paths: Vec::new(),
        });
        album.isrc = album.isrc.take().or_else(|| isrc(&t.isrc));
        album.paths.push(t.file_path);
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
