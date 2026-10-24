//! A set or a mix for other DJ software: what the DJ played, as one playlist. The files point at
//! the audio on the drive the user names, so the drive's library is read as a drive's export
//! reads it, then narrowed to that playlist.

use loclib_model::{Library, Playlist, paths};
use loclib_plugin::{ExportSource, Host, RpcError};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use serde_json::json;

/// What was played, in order, and the playlist's name.
pub(crate) struct Played {
    /// `set` or `mix`, for messages.
    what: &'static str,
    name: String,
    tracks: Vec<Heard>,
}

/// A track played, as a set's entry and a mix's play name it.
#[derive(Deserialize)]
struct Heard {
    #[serde(default)]
    artist: String,
    #[serde(default)]
    title: String,
    /// Its audio on the drive (`/Contents/...`): a set entry's `file`, a mix play's track's
    /// `file_path`. A set's track typed in by hand has none.
    #[serde(default, alias = "file_path")]
    file: Option<String>,
}

impl Heard {
    /// `Artist - Title`, or whichever of them it has, as locallibrary's sets name a track.
    fn name(&self) -> String {
        let parts = [&self.artist, &self.title];
        parts.into_iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join(" - ")
    }
}

/// `sets.get`'s answer, as far as it's read.
#[derive(Deserialize)]
struct Set {
    date: String,
    title: Option<String>,
    venue: Option<String>,
    tracks: Vec<Heard>,
}

/// `mix.reports.get`'s answer, as far as it's read.
#[derive(Deserialize)]
struct Mix {
    mix: MixInfo,
    plays: Vec<Play>,
}

#[derive(Deserialize)]
struct MixInfo {
    title: String,
    date: Option<String>,
}

#[derive(Deserialize)]
struct Play {
    track: Heard,
}

/// The set or the mix `source` names, read through the host; `None` for a drive's export. One
/// without a drive is refused first: the files point at its tracks' audio there.
pub(crate) fn read(source: &ExportSource, host: &Host) -> Result<Option<Played>, RpcError> {
    let played = match (&source.set, &source.mix) {
        (None, None) => return Ok(None),
        _ if source.drive.is_empty() => {
            let what = if source.set.is_some() { "set" } else { "mix" };
            return Err(crate::invalid(format!(
                "a {what}'s export points at its tracks' audio: name the drive they're on"
            )));
        }
        (Some(set), _) => {
            let set: Set = host.call("sets.get", &json!({ "set": set }))?;
            // As `sets playlist` names one: its title, else its venue.
            let name = named([Some(set.date), set.title.or(set.venue)]);
            Played { what: "set", name, tracks: set.tracks }
        }
        (None, Some(mix)) => {
            let mix: Mix = host.call("mix.reports.get", &json!({ "id": mix }))?;
            let name = named([mix.mix.date, Some(mix.mix.title)]);
            // Neither a day nor a title: a playlist still needs a name to show.
            let name = if name.is_empty() { "Mix".into() } else { name };
            Played { what: "mix", name, tracks: mix.plays.into_iter().map(|p| p.track).collect() }
        }
    };
    Ok(Some(played))
}

/// The parts given, with a space between.
fn named(parts: [Option<String>; 2]) -> String {
    parts.into_iter().flatten().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

impl Played {
    /// `library` with only one playlist, named after what was played, holding the tracks found
    /// on the drive in play order (one played twice is there twice), and only those tracks; and
    /// a warning for each play that isn't on the drive. Found by file, as `sets playlist` and
    /// `sets export --format m3u8` find a set's tracks. None found is refused.
    pub(crate) fn narrow(self, mut library: Library) -> Result<(Library, Vec<String>), RpcError> {
        let by_file: FxHashMap<String, u32> =
            library.tracks.iter().map(|t| (paths::file_key(&t.file_path), t.id)).collect();
        let (mut ids, mut missing) = (Vec::new(), Vec::new());
        for (i, heard) in self.tracks.iter().enumerate() {
            match heard.file.as_deref().and_then(|f| by_file.get(&paths::file_key(f))) {
                Some(id) => ids.push(*id),
                None => missing.push(format!("{}. {} isn't on the drive: left out", i + 1, heard.name())),
            }
        }
        if ids.is_empty() {
            return Err(RpcError::failed("not_found", format!("none of the {}'s tracks are on the drive", self.what)));
        }
        let played: FxHashSet<u32> = ids.iter().copied().collect();
        library.tracks.retain(|t| played.contains(&t.id));
        let playlist =
            Playlist { id: 1, parent_id: None, name: self.name, is_folder: false, sort_order: 0, track_ids: ids };
        library.playlists = vec![playlist];
        library.history.clear();
        library.history_playlists = 0;
        Ok((library, missing))
    }
}
