//! A mix's report for people: the interactive HTML page (the mix's waveform with each track's
//! lane, tempo, keys, and every track's own waveform, cues and phrases, playing the mix in sync
//! when its audio can be found), what each play adds to it from the drive, and the reports kept
//! in the store's `reports` folder.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf, Prefix};

use base64::prelude::*;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::export::{UNNAMED, may_name};
use super::{MixReport, Play};
use crate::anlz::{self, TrackAnalysis};
use crate::artwork::{self, Artwork};
use crate::cue_edit::HotCue;
use crate::device::Device;
use crate::error::IoContext;
use crate::model::{Library, Track};
use crate::profile::ProfilePrefs;
use crate::store::Store;
use crate::{Error, Result, fsutil};

const TEMPLATE: &str = include_str!("report.html");

/// Most bytes a report's id takes from its title: file names are limited in bytes, and a
/// character can take four.
const SLUG_BYTES: usize = 60;

/// The name of the store's folder of reports.
pub(crate) const REPORTS_DIR: &str = "reports";

/// The store's folder of reports.
pub fn reports_dir(store_root: &Path) -> PathBuf {
    store_root.join(REPORTS_DIR)
}

/// A report's id, which names its files: the title's slug and a hash of the recording's path.
/// Two recordings with one title (a recorder's sets all take their folder's name) are kept
/// apart, and the same recording identified again replaces its own.
pub fn id(r: &MixReport) -> String {
    let hash = blake3::hash(r.mix.path.as_os_str().as_encoded_bytes()).to_hex();
    format!("{}_{}", slug(&r.mix.title), &hash[..8])
}

/// Where the store keeps `r`'s file `ext` (`html`, `json`, `mix.json`).
pub fn report_file(store: &Store, r: &MixReport, ext: &str) -> PathBuf {
    reports_dir(store.root()).join(format!("{}.{ext}", id(r)))
}

/// A file name for a title: lowercase letters and digits, words joined by `-`, in at most 60
/// bytes.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let mut bytes = 0;
    let out: String = out
        .chars()
        .take_while(|c| {
            bytes += c.len_utf8();
            bytes <= SLUG_BYTES
        })
        .collect();
    let out = out.trim_end_matches('-');
    if out.is_empty() { "mix".into() } else { out.into() }
}

/// What browsers play, by extension: its rank, most compact first.
fn browser_format(p: &Path) -> Option<usize> {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    ["mp3", "m4a", "aac", "wav", "flac", "ogg"].iter().position(|e| *e == ext)
}

/// Audio a browser can play for the mix: the file itself, or for an Audacity project, its
/// export ([`project_audio`]).
pub fn playable_audio(mix: &Path) -> Option<PathBuf> {
    match browser_format(mix) {
        Some(_) => Some(mix.to_path_buf()),
        None => project_audio(mix),
    }
}

/// The audio an Audacity project was exported to: the file a browser plays next to it with the
/// longest name it shares (four characters at least). `None` for a recording, which is its own
/// audio.
pub fn project_audio(project: &Path) -> Option<PathBuf> {
    if !super::aup::is_project(project) {
        return None;
    }
    let stem = project.file_stem()?.to_string_lossy().to_lowercase();
    let shared = |p: &Path| {
        let other = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        stem.chars().zip(other.chars()).take_while(|(a, b)| a == b).count()
    };
    let mut candidates: Vec<(usize, usize, PathBuf)> = std::fs::read_dir(project.parent()?)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter_map(|p| Some((shared(&p), browser_format(&p)?, p)))
        .filter(|(n, ..)| *n >= 4)
        .collect();
    // Longest shared name first, then the most compact format.
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    candidates.into_iter().next().map(|(.., p)| p)
}

/// The drive a mix was matched against, for what it adds to each play: cover art, the overview
/// waveform, hot cues and phrases.
#[derive(Clone, Copy)]
pub struct Drive<'a> {
    pub device: &'a Device,
    pub library: &'a Library,
    /// Each play's analysis files, in play order, when they've been read already.
    pub analyses: Option<&'a [TrackAnalysis]>,
}

impl<'a> Drive<'a> {
    pub fn new(device: &'a Device, library: &'a Library) -> Drive<'a> {
        Drive { device, library, analyses: None }
    }

    /// The drive's track `play` is of, by its file: a kept report's track ids may name other
    /// tracks by now, or on another drive with the same name.
    pub(crate) fn track(&self, play: &Play) -> Option<&'a Track> {
        self.library.tracks.iter().find(|t| t.file_path == play.track.file_path)
    }

    /// The analysis files of play `i`, whose track is `track`.
    pub(crate) fn analysis(&self, i: usize, track: &Track) -> Cow<'a, TrackAnalysis> {
        match self.analyses.and_then(|a| a.get(i)) {
            Some(a) => Cow::Borrowed(a),
            None => Cow::Owned(anlz::load(self.device, track)),
        }
    }
}

/// What the drive adds to a play for following along: the track's overview waveform, hot cues
/// and phrases (and, in the HTML report, its cover art), so a front end needn't ask for each
/// track's analysis.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayExtras {
    /// The track's cover art, a JPEG `data:` URL (the HTML report only; `art.get` and `art.path`
    /// have it otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art: Option<String>,
    /// The track's overview waveform: up to 400 columns of four bytes (height 0-255, then red,
    /// green and blue), in base64.
    pub wave: Option<String>,
    /// Its phrases, in seconds into the track.
    pub phrases: Vec<PhraseSpan>,
    /// Its hot cues, in seconds into the track.
    pub cues: Vec<CueMark>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PhraseSpan {
    pub start: f64,
    pub end: f64,
    /// `Intro`, `Up`, `Chorus`...
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CueMark {
    /// Seconds into the track.
    pub t: f64,
    /// The hot cue's letter and name: `A`, `B · Drop` (the HTML report gives only the letter).
    pub label: String,
    /// `#rrggbb`.
    pub color: Option<String>,
}

/// What `drive` says about play `i`: its track's overview waveform, hot cues and phrases. For
/// the HTML report (`page`), also its cover, and its cues without their names (the page doesn't
/// show them). Nothing when the drive no longer has the track's file.
pub fn extras(drive: Drive<'_>, play: &Play, i: usize, page: bool) -> PlayExtras {
    let Some(track) = drive.track(play) else { return PlayExtras::default() };
    let art = page
        .then(|| artwork::read_from_drive(drive.device, drive.library, track.id).ok().flatten())
        .flatten()
        .map(|b| data_url("image/jpeg", &b));
    let analysis = drive.analysis(i, track);
    // Overview waveform: up to 400 columns of (height, r, g, b).
    let wave = analysis.preview.as_ref().filter(|w| !w.columns.is_empty()).map(|w| {
        let n = w.columns.len().min(400);
        let mut bytes = Vec::with_capacity(n * 4);
        for i in 0..n {
            let lo = i * w.columns.len() / n;
            let hi = ((i + 1) * w.columns.len() / n).max(lo + 1);
            let cols = &w.columns[lo..hi];
            let c = cols.iter().max_by(|a, b| a.back.height.total_cmp(&b.back.height)).expect("non-empty");
            bytes.push((c.back.height.clamp(0.0, 1.0) * 255.0) as u8);
            bytes.extend(c.back.color);
        }
        BASE64_STANDARD.encode(&bytes)
    });
    let beat_secs =
        |beat: u16| analysis.beats.get((beat as usize).saturating_sub(1)).map(|b| b.time_ms as f64 / 1000.0);
    let phrases = analysis
        .structure
        .iter()
        .flat_map(|s| &s.phrases)
        .filter_map(|ph| {
            let start = beat_secs(ph.start_beat)?;
            let end = beat_secs(ph.end_beat).unwrap_or(track.duration_secs as f64);
            Some(PhraseSpan { start, end, kind: ph.kind.name().to_string() })
        })
        .collect();
    let cues = analysis
        .cues
        .iter()
        .filter_map(|c| {
            let letter = HotCue(c.hot_cue?).letter();
            Some(CueMark {
                t: c.start_ms as f64 / 1000.0,
                label: if page || c.comment.is_empty() {
                    letter.to_string()
                } else {
                    format!("{letter} · {}", c.comment)
                },
                color: c.color.map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}")),
            })
        })
        .collect();
    PlayExtras { art, wave, phrases, cues }
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", BASE64_STANDARD.encode(bytes))
}

/// A path's characters a URL takes as they are; the rest are percent-encoded.
fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|c| match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (c as char).to_string(),
            _ => format!("%{c:02X}"),
        })
        .collect()
}

/// A `file:` URL for the absolute path `target`: `file:///Users/…`, `file:///E:/…` for a
/// Windows drive letter, `file://server/share/…` for a network share.
fn file_url(target: &Path) -> String {
    let mut url = String::from("file://");
    // A path that is only its root (`/`, `C:\`) still needs the slash that names it.
    let mut ends_at_root = false;
    for c in target.components() {
        ends_at_root = matches!(c, Component::RootDir);
        match c {
            Component::Prefix(p) => match p.kind() {
                Prefix::Disk(d) | Prefix::VerbatimDisk(d) => url.push_str(&format!("/{}:", d as char)),
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    url.push_str(&url_encode(&server.to_string_lossy()));
                    url.push('/');
                    url.push_str(&url_encode(&share.to_string_lossy()));
                }
                _ => {}
            },
            Component::RootDir => {}
            c => {
                url.push('/');
                url.push_str(&url_encode(&c.as_os_str().to_string_lossy()));
            }
        }
    }
    if ends_at_root {
        url.push('/');
    }
    url
}

/// A URL for `target` relative to the page at `page` (so the folder can move as a whole), or a
/// `file:` URL when they share no folder at all (on different Windows drives).
pub fn relative_url(page: &Path, target: &Path) -> String {
    // Lexically, the way a browser resolves `..` against the page's address: resolving symlinks
    // would give the wrong link for a page opened through one (and the page's folder may not
    // exist yet, so only one side would be resolved: macOS's /var is /private/var).
    let from = fsutil::absolute(page.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")));
    let to = fsutil::absolute(target);
    let a: Vec<_> = from.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return file_url(&to);
    }
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), a.len() - common).collect();
    parts.extend(b[common..].iter().map(|c| url_encode(&c.as_os_str().to_string_lossy())));
    parts.join("/")
}

/// The interactive report for `r`, to be written at `path`: the template with the report, its
/// cover, the mix's `audio` for its player and each play's extras from `drive` (without it, the
/// tracks' waveforms, cues, phrases and art are left out).
///
/// A page holds only what it shows: no drive name or library counts, no comments or ratings, a
/// cover only when the recording embeds one, and the tracks `prefs` hides from the DJ's profile
/// as "ID - ID". The store's own pages, opened on this computer, link the audio relative to the
/// page. A `shared` page (`mix --html`, written to be moved or shared) holds no file paths at
/// all: it links the audio by its file name alone, so it plays once the recording is next to it.
pub fn page(
    r: &MixReport,
    drive: Option<Drive<'_>>,
    audio: Option<&Path>,
    path: &Path,
    prefs: &ProfilePrefs,
    shared: bool,
) -> String {
    let cover = r
        .mix
        .shared_cover()
        .and_then(|b| Artwork::from_bytes(&b).ok())
        .and_then(|a| a.jpeg(640).ok())
        .map(|j| data_url("image/jpeg", &j));
    let extras: Vec<PlayExtras> = match drive {
        Some(d) => r
            .plays
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let e = extras(d, p, i, true);
                // A hidden track's art would name it.
                if may_name(prefs, &p.track) { e } else { PlayExtras { art: None, ..e } }
            })
            .collect(),
        None => Vec::new(),
    };
    let plays: Vec<serde_json::Value> = r
        .plays
        .iter()
        .map(|p| {
            let t = &p.track;
            let track = if may_name(prefs, t) {
                json!({ "title": t.title, "artist": t.artist, "album": t.album, "label": t.label,
                    "bpm": t.bpm, "key": t.key, "duration_secs": t.duration_secs })
            } else {
                json!({ "title": UNNAMED, "artist": UNNAMED, "bpm": t.bpm, "key": t.key,
                    "duration_secs": t.duration_secs })
            };
            let alternates: Vec<serde_json::Value> = p
                .alternates
                .iter()
                .map(|a| {
                    let artist = a.name.split_once(" - ").map_or("", |(artist, _)| artist);
                    let name =
                        if prefs.may_name(&a.name, artist) { a.name.clone() } else { format!("{UNNAMED} - {UNNAMED}") };
                    json!({ "name": name, "same_audio": a.same_audio })
                })
                .collect();
            json!({
                "track": track, "start": p.start, "end": p.end, "track_start": p.track_start,
                "track_end": p.track_end, "tempo": p.tempo, "played_bpm": p.played_bpm,
                "key_lock": p.key_lock, "semitones": p.semitones, "key": p.key, "played_key": p.played_key,
                "confidence": p.confidence, "tentative": p.is_tentative(), "presence": p.presence,
                "presence_step": p.presence_step, "jumps": p.jumps, "alternates": alternates,
            })
        })
        .collect();
    let m = &r.mix;
    let name = audio.and_then(|a| a.file_name()).map(|n| n.to_string_lossy().into_owned());
    let link = match shared {
        true => name.as_deref().map(url_encode),
        false => audio.map(|a| relative_url(path, a)),
    };
    let data = json!({
        "report": {
            "mix": { "kind": m.kind, "title": m.title, "artist": m.artist, "genre": m.genre, "date": m.date,
                "recorder": m.recorder, "duration_secs": m.duration_secs },
            "plays": plays,
            "transitions": r.transitions,
            "unidentified": r.unidentified,
            "coverage": r.coverage,
            "waveform": r.waveform,
            "tempo_range": r.tempo_range,
            "analysed_at": r.analysed_at,
        },
        "cover": cover,
        "audio": link,
        "audioName": name,
        "extras": extras,
        "version": env!("CARGO_PKG_VERSION"),
    });
    // `</script>` or `<!--` in a title would end the data block early: every `<` is escaped the
    // way JSON allows.
    let json = serde_json::to_string(&data).expect("serialisable").replace('<', "\\u003c");
    let title = r.mix.title.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    TEMPLATE.replace("{{TITLE}}", &title).replace("/*{{TOKENS}}*/", &tokens_css()).replace("/*{{DATA}}*/null", &json)
}

/// The colours the page shares with the player and the website, as CSS custom properties: the
/// waveform's bands, phrases by kind, cue defaults and the playhead, and good, warning and
/// critical for the light page and the dark one (both ways it goes dark).
fn tokens_css() -> String {
    use crate::tokens::{PLAYHEAD, Rgb, bands, cues, phrases, status};
    let props = |vars: &[(&str, Rgb)]| {
        vars.iter().map(|(name, [r, g, b])| format!("  --{name}: #{r:02x}{g:02x}{b:02x};\n")).collect::<String>()
    };
    let status = |s: [Rgb; 3]| props(&[("good", s[0]), ("warning", s[1]), ("critical", s[2])]);
    let [low, mid, high] = bands::SCALES;
    let shared = props(&[
        ("band-low", bands::LOW),
        ("band-mid", bands::MID),
        ("band-high", bands::HIGH),
        ("phrase-intro", phrases::INTRO),
        ("phrase-up", phrases::UP),
        ("phrase-verse", phrases::VERSE),
        ("phrase-chorus", phrases::CHORUS),
        ("phrase-down", phrases::DOWN),
        ("phrase-bridge", phrases::BRIDGE),
        ("phrase-outro", phrases::OUTRO),
        ("phrase-unknown", phrases::UNKNOWN),
        ("cue-hot", cues::HOT),
        ("cue-memory", cues::MEMORY),
        ("cue-loop", cues::LOOP),
        ("playhead", PLAYHEAD),
    ]);
    let light = status([status::light::GOOD, status::light::WARNING, status::light::CRITICAL]);
    let dark = status([status::dark::GOOD, status::dark::WARNING, status::dark::CRITICAL]);
    format!(
        ":root {{\n{shared}  --band-scale-low: {low};\n  --band-scale-mid: {mid};\n  --band-scale-high: {high};\n{light}}}\n\
         @media (prefers-color-scheme: dark) {{\n  :root:where(:not([data-theme=\"light\"])) {{\n{dark}  }}\n}}\n\
         :root[data-theme=\"dark\"] {{\n{dark}}}"
    )
}

/// A report kept in the store's `reports` folder.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SavedReport {
    /// Names it in `mix.reports.get` and `mix.reports.html`: its files' name, the title's slug
    /// and a hash of the recording's path (see [`id`]).
    pub id: String,
    /// The mix's title (from its file name when only the HTML page is there).
    pub title: String,
    pub artist: Option<String>,
    /// The recording it was made from.
    pub mix: Option<PathBuf>,
    /// The drive whose library it was matched against.
    pub drive: Option<String>,
    pub duration_secs: Option<f64>,
    /// Tracks found.
    pub tracks: Option<usize>,
    /// Fraction of the mix identified.
    pub coverage: Option<f64>,
    pub analysed_at: Option<DateTime<Utc>>,
    /// The report as data (`<id>.json`): `mix.reports.get` reads it.
    pub report: Option<PathBuf>,
    /// The interactive page (`<id>.html`).
    pub html: Option<PathBuf>,
    /// The portable export (`<id>.mix.json`), for Local Network.
    pub export: Option<PathBuf>,
    /// When one of its files last changed.
    pub modified: DateTime<Utc>,
}

/// Keep `r` in the store's reports folder (`<id>.json`). Returns its id.
pub fn save(store: &Store, r: &MixReport) -> Result<String> {
    let json = serde_json::to_vec_pretty(r).map_err(|e| Error::invalid(e.to_string()))?;
    fsutil::write_atomic(&report_file(store, r, "json"), &json)?;
    Ok(id(r))
}

/// The reports in the store's reports folder, newest first.
pub fn list(store_root: &Path) -> Result<Vec<SavedReport>> {
    let dir = reports_dir(store_root);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::Io { path: dir, source: e }),
    };
    let mut ids = std::collections::BTreeSet::new();
    for e in entries {
        let name = e.at(&dir)?.file_name().to_string_lossy().into_owned();
        if let Some(id) = [".mix.json", ".json", ".html"].iter().find_map(|ext| name.strip_suffix(ext)) {
            ids.insert(id.to_string());
        }
    }
    let mut out: Vec<SavedReport> = ids.into_iter().filter_map(|id| describe(&dir, &id)).collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// What the store's reports folder has for `id`, if anything.
fn describe(dir: &Path, id: &str) -> Option<SavedReport> {
    let at = |ext: &str| Some(dir.join(format!("{id}.{ext}"))).filter(|p| p.is_file());
    let (report, html, export) = (at("json"), at("html"), at("mix.json"));
    let modified = [&report, &html, &export]
        .into_iter()
        .flatten()
        .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .max()?;
    let mut saved = SavedReport {
        id: id.to_string(),
        title: id.rsplit_once('_').map_or(id, |(slug, _)| slug).replace('-', " "),
        artist: None,
        mix: None,
        drive: None,
        duration_secs: None,
        tracks: None,
        coverage: None,
        analysed_at: None,
        report,
        html,
        export,
        modified: modified.into(),
    };
    if let Some(r) = saved.report.as_deref().and_then(|p| fsutil::read_json_opt::<MixReport>(p).ok().flatten()) {
        saved.title = r.mix.title;
        saved.artist = r.mix.artist;
        saved.mix = Some(r.mix.path);
        saved.drive = Some(r.drive);
        saved.duration_secs = Some(r.mix.duration_secs);
        saved.tracks = Some(r.plays.len());
        saved.coverage = Some(r.coverage);
        saved.analysed_at = Some(r.analysed_at);
    }
    Some(saved)
}

/// The report kept as `id`.
pub fn load(store_root: &Path, id: &str) -> Result<MixReport> {
    // Only what ids are made of: nothing that leads out of the folder (`..`, `/`, `C:`).
    if id.is_empty() || !id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err(Error::invalid(format!("{id:?} isn't a report's id")));
    }
    let path = reports_dir(store_root).join(format!("{id}.json"));
    fsutil::read_json_opt(&path)?.ok_or_else(|| Error::NotFound(format!("report {id:?} in the store")))
}

#[cfg(test)]
pub(crate) mod tests;
