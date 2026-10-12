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
pub(crate) mod tests {
    use super::*;

    #[test]
    fn slugs_and_urls() {
        assert_eq!(slug("one shot: i promise it's techno"), "one-shot-i-promise-it-s-techno");
        assert_eq!(slug("!!!"), "mix");
        let dir = tempfile::tempdir().unwrap();
        let mixes = dir.path().join("mixes/tachy cardia");
        std::fs::create_dir_all(&mixes).unwrap();
        let audio = mixes.join("Set #1.mp3");
        std::fs::write(&audio, b"x").unwrap();
        let page = dir.path().join("reports/set.html");
        std::fs::create_dir_all(page.parent().unwrap()).unwrap();
        assert_eq!(relative_url(&page, &audio), "../mixes/tachy%20cardia/Set%20%231.mp3");
    }

    /// The report's link to its audio leads there from the page, whichever folders they're in
    /// and before the page's folder exists. (Bombadil found the link climbing to `/` when only
    /// the audio's side had macOS's /var resolved to /private/var.)
    #[hegel::test(test_cases = 200)]
    fn audio_links_lead_to_the_audio(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let folder = || gs::vecs(gs::sampled_from(vec!["a", "b", ".", "tachy cardia", "Set #1", "日本"])).max_size(4);
        let page_dirs: Vec<&str> = tc.draw(folder());
        let audio_dirs: Vec<&str> = tc.draw(folder());
        let name = tc.draw(gs::sampled_from(vec!["set.wav", "Set #1.mp3", "ünï cödé.m4a"]));
        let made = tc.draw(gs::booleans());

        let dir = tempfile::tempdir().unwrap();
        let audio = audio_dirs.iter().fold(dir.path().to_path_buf(), |p, s| p.join(s)).join(name);
        std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
        std::fs::write(&audio, b"x").unwrap();
        let page = page_dirs.iter().fold(dir.path().to_path_buf(), |p, s| p.join(s)).join("report.html");
        if made {
            std::fs::create_dir_all(page.parent().unwrap()).unwrap();
        }
        let url = relative_url(&page, &audio);
        assert!(!url.starts_with("file:") && !url.starts_with('/'), "{url}");

        // Follow it the way a browser would, from the page's folder.
        let mut bytes = Vec::new();
        let mut rest = url.as_bytes();
        while let Some((&b, tail)) = rest.split_first() {
            if b == b'%' {
                bytes.push(u8::from_str_radix(std::str::from_utf8(&tail[..2]).unwrap(), 16).unwrap());
                rest = &tail[2..];
            } else {
                bytes.push(b);
                rest = tail;
            }
        }
        std::fs::create_dir_all(page.parent().unwrap()).unwrap();
        let followed = page.parent().unwrap().join(String::from_utf8(bytes).unwrap());
        assert_eq!(std::fs::canonicalize(followed).unwrap(), std::fs::canonicalize(&audio).unwrap(), "{url}");
    }

    pub(crate) fn report(title: &str) -> MixReport {
        MixReport {
            mix: super::super::MixInfo {
                path: PathBuf::from("/mixes/set.wav"),
                kind: "audio",
                title: title.into(),
                artist: Some("Example DJ".into()),
                genre: None,
                date: None,
                recorder: None,
                duration_secs: 3600.0,
                sample_rate: 44100,
                channels: 2,
                labels: Vec::new(),
                cover: None,
            },
            drive: "DNA".into(),
            drive_id: None,
            library: Default::default(),
            plays: Vec::new(),
            transitions: Vec::new(),
            unidentified: Vec::new(),
            coverage: 0.9,
            waveform: Default::default(),
            tempo_range: 0.16,
            analysed_at: Utc::now(),
            elapsed_secs: 12.0,
        }
    }

    pub(crate) fn play(track: Track) -> Play {
        Play {
            track,
            start: 10.0,
            end: 200.0,
            track_start: 0.0,
            track_end: 190.0,
            tempo: 1.0,
            played_bpm: Some(128.0),
            key_lock: None,
            semitones: 0.0,
            key: None,
            played_key: None,
            confidence: 0.9,
            votes: 100,
            presence: vec![1.0],
            presence_step: 2.0,
            jumps: 0,
            alternates: Vec::new(),
        }
    }

    #[test]
    fn reports_are_kept_in_the_store_and_listed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(list(root).unwrap().is_empty(), "no reports folder yet");
        let store = Store::open_or_init(root).unwrap();
        let id = save(&store, &report("Live at the Warehouse")).unwrap();
        assert!(id.starts_with("live-at-the-warehouse_"), "{id}");
        // A page an older build wrote, on its own.
        std::fs::write(reports_dir(root).join("old-set.html"), "<html>").unwrap();

        let listed = list(root).unwrap();
        let ids: Vec<&str> = listed.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&id.as_str()) && ids.contains(&"old-set"), "{ids:?}");
        let kept = listed.iter().find(|r| r.id == id).unwrap();
        assert_eq!(
            (kept.title.as_str(), kept.tracks, kept.drive.as_deref()),
            ("Live at the Warehouse", Some(0), Some("DNA"))
        );
        let old = listed.iter().find(|r| r.id == "old-set").unwrap();
        assert!(old.report.is_none() && old.html.is_some() && old.title == "old set");

        let back = load(root, &id).unwrap();
        assert_eq!((back.mix.title.as_str(), back.mix.kind), ("Live at the Warehouse", "audio"));
        assert!(matches!(load(root, "nope").unwrap_err().inner(), Error::NotFound(_)));
        assert!(matches!(load(root, "../store").unwrap_err().inner(), Error::Invalid(_)));

        // Without the drive, the page has the report and no tracks' extras.
        let html = page(&back, None, None, &reports_dir(root).join("x.html"), &ProfilePrefs::default(), false);
        assert!(html.contains("<title>") && html.contains("Live at the Warehouse") && html.contains("\"extras\":[]"));
    }

    /// A reports folder that can't be read is an error, not an empty list.
    #[test]
    fn a_reports_folder_that_cant_be_read_says_so() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(reports_dir(dir.path()), b"a file where the folder goes").unwrap();
        assert!(matches!(list(dir.path()).unwrap_err().inner(), Error::Io { .. }));
    }

    /// Two recordings with one title (a recorder's sets take their folder's name) are kept
    /// apart; the same recording identified again replaces its own report.
    #[test]
    fn reports_of_mixes_with_one_title_are_kept_apart() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path()).unwrap();
        let (mut a, mut b) = (report("Friday"), report("friday!"));
        a.mix.path = "/mixes/club/REC001.WAV".into();
        b.mix.path = "/mixes/club/REC002.WAV".into();
        let (ia, ib) = (save(&store, &a).unwrap(), save(&store, &b).unwrap());
        assert_ne!(ia, ib);
        assert_eq!(save(&store, &a).unwrap(), ia);
        assert_eq!(list(dir.path()).unwrap().len(), 2);
        assert_eq!(load(dir.path(), &ia).unwrap().mix.path, a.mix.path);
        assert_eq!(list(dir.path()).unwrap().iter().find(|r| r.id == ib).unwrap().title, "friday!");
    }

    /// Ids stay short enough for a file name, and only ids made the way [`id`] makes them load:
    /// a Windows drive-relative `C:x` would lead out of the reports folder.
    #[test]
    fn ids_are_bounded_and_checked() {
        assert_eq!(slug(&"long title ".repeat(40)).len(), SLUG_BYTES - 1, "no dash at the end");
        // Four bytes each (mathematical bold, as some titles are styled).
        assert_eq!(slug(&"\u{1d40b}".repeat(100)).len(), SLUG_BYTES);
        let dir = tempfile::tempdir().unwrap();
        for bad in ["C:x", "a/b", "..", ".hidden", ""] {
            assert!(matches!(load(dir.path(), bad).unwrap_err().inner(), Error::Invalid(_)), "{bad}");
        }
        assert!(matches!(load(dir.path(), "日本-set_0a1b2c3d").unwrap_err().inner(), Error::NotFound(_)));
    }

    /// The data block parses whatever the strings hold: `<!--` once left the page blank.
    #[test]
    fn the_page_reads_back_any_title() {
        let title = "a <!-- b </script> c";
        let html = page(&report(title), None, None, Path::new("/r/x.html"), &ProfilePrefs::default(), true);
        let data = html.split_once(r#"<script id="data" type="application/json">"#).unwrap().1;
        let data = data.split_once("</script>").unwrap().0;
        let v: serde_json::Value = serde_json::from_str(data).unwrap_or_else(|e| panic!("{e}: {data}"));
        assert_eq!(v["report"]["mix"]["title"], title);
    }

    /// The page draws in the colours the player does: the bands and phrases, and good, warning
    /// and critical for the light page and both ways it goes dark.
    #[test]
    fn the_page_shares_the_players_colours() {
        use crate::tokens::{Rgb, bands, phrases, status};
        let html = page(&report("x"), None, None, Path::new("/r/x.html"), &ProfilePrefs::default(), true);
        let prop = |name: &str, [r, g, b]: Rgb| format!("--{name}: #{r:02x}{g:02x}{b:02x};");
        assert!(!html.contains("{{TOKENS}}"));
        for (name, rgb) in [("band-low", bands::LOW), ("phrase-verse", phrases::VERSE), ("good", status::light::GOOD)] {
            assert_eq!(html.matches(&prop(name, rgb)).count(), 1, "{name}");
        }
        assert!(html.contains("--band-scale-mid: 0.66;"));
        assert_eq!(html.matches(&prop("critical", status::dark::CRITICAL)).count(), 2, "both dark blocks");
    }

    /// A page made to be shared holds what it shows and nothing that only makes sense here (the
    /// recording by its file name alone), and the tracks the profile hides are "ID - ID".
    #[test]
    fn the_page_holds_only_what_it_shows() {
        use loclib_model::test_util::track;
        let dir = tempfile::tempdir().unwrap();
        let screenshot = dir.path().join("screenshot.png");
        image::RgbImage::new(32, 32).save(&screenshot).unwrap();
        let mut r = report("Friday");
        r.mix.path = "/Users/someone/Mixes/friday.wav".into();
        r.mix.cover = Some(super::super::CoverSource::File(screenshot));
        r.library.missing.push((&track(9, "Gone", "Track", "/Contents/gone.mp3")).into());
        let mut shown = track(1, "Bicep", "Glue", "/Contents/Bicep/glue.mp3");
        shown.comment = "promo from a friend".into();
        shown.rating = 5;
        let mut p = play(shown);
        p.alternates.push(super::super::Alternate {
            track_id: 3,
            name: "Bicep - Glue (dub)".into(),
            file_path: "/Contents/Bicep/glue dub.mp3".into(),
            same_audio: false,
        });
        r.plays = vec![p, play(track(2, "Dubplate", "Unreleased", "/Contents/dub.wav"))];
        r.plays[1].confidence = 0.3;
        let mut prefs = ProfilePrefs::default();
        prefs.hidden_artists.push("Dubplate".into());

        let audio = Some(Path::new("/Users/someone/Mixes/friday.wav"));
        let html = page(&r, None, audio, Path::new("/r/x.html"), &prefs, true);
        for leak in
            ["/Users/someone", "Mixes", "/Contents/", "promo from a friend", "DNA", "screenshot", "Gone", "Unreleased"]
        {
            assert!(!html.contains(leak), "{leak} is in the page");
        }
        assert!(!html.contains("\"rating\"") && !html.contains("\"library\""));
        assert!(html.contains("Bicep - Glue (dub)") && html.contains("\"title\":\"ID\""));
        assert!(html.contains("\"tentative\":true") && html.contains("\"cover\":null"));
        // Played once the recording is put next to it.
        assert!(html.contains(r#""audio":"friday.wav","audioName":"friday.wav""#), "{html}");

        // The store's own page, opened here, plays the recording where it is.
        let kept = page(&r, None, audio, Path::new("/r/x.html"), &prefs, false);
        assert!(kept.contains(r#""audio":"../Users/someone/Mixes/friday.wav""#));
    }

    /// What the drive adds is found by the track's file, as a kept report's ids may name other
    /// tracks there by now; a hot cue past Z is a `?`, and the page leaves cue names out.
    #[test]
    fn extras_come_from_the_drive_by_file() {
        use crate::anlz::Cue;
        use loclib_model::test_util::track;
        let dir = tempfile::tempdir().unwrap();
        let device = Device::open(dir.path()).unwrap();
        let library = Library { tracks: vec![track(7, "A", "One", "/Contents/one.mp3")], ..Default::default() };
        let cue = |hot: u8| Cue {
            hot_cue: Some(hot),
            start_ms: 1000,
            end_ms: None,
            comment: "drop".into(),
            color: None,
            color_id: 0,
            loop_beats: None,
        };
        let analyses = [TrackAnalysis { cues: vec![cue(1), cue(200)], ..Default::default() }];
        let drive = Drive { device: &device, library: &library, analyses: Some(&analyses) };
        let labels = |e: PlayExtras| e.cues.into_iter().map(|c| c.label).collect::<Vec<_>>();
        let p = play(track(1, "A", "One", "/Contents/one.mp3"));
        assert_eq!(labels(extras(drive, &p, 0, false)), ["B · drop", "? · drop"]);
        assert_eq!(labels(extras(drive, &p, 0, true)), ["B", "?"]);
        let elsewhere = play(track(7, "B", "Two", "/Contents/two.mp3"));
        assert_eq!(extras(drive, &elsewhere, 0, false), PlayExtras::default());
    }

    #[test]
    fn finds_the_export_of_a_project() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["set one audacity save.aup4", "set one export.wav", "set one export.mp3", "other.mp3"] {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        let got = playable_audio(&dir.path().join("set one audacity save.aup4")).unwrap();
        assert_eq!(got.file_name().unwrap(), "set one export.mp3");
    }

    /// Only a project stands for audio exported next to it: a recording browsers can't play
    /// has none, rather than another recording that shares its first letters.
    #[test]
    fn a_recording_is_never_swapped_for_its_neighbour() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["friday 2024-05-03.aiff", "friday 2024-04-26.mp3"] {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        let aiff = dir.path().join("friday 2024-05-03.aiff");
        assert_eq!(playable_audio(&aiff), None);
        assert_eq!(project_audio(&aiff), None);
    }

    #[test]
    fn file_urls() {
        assert_eq!(file_url(Path::new("/Users/dj/Set #1.wav")), "file:///Users/dj/Set%20%231.wav");
    }

    /// The root is a URL of its own, not `file://` with nothing after it.
    #[test]
    fn the_root_is_a_file_url() {
        assert_eq!(file_url(Path::new("/")), "file:///");
        assert_eq!(file_url(Path::new("/.")), "file:///");
    }

    /// The page on C:, the recording on the USB drive: no folder in common.
    #[cfg(windows)]
    #[test]
    fn audio_on_another_drive_letter() {
        assert_eq!(
            relative_url(Path::new(r"C:\Users\dj\reports\x.html"), Path::new(r"E:\PIONEER REC\x.wav")),
            "file:///E:/PIONEER%20REC/x.wav"
        );
        assert_eq!(file_url(Path::new(r"\\nas\music\set.wav")), "file://nas/music/set.wav");
    }

    use hegel::TestCase;
    use hegel::generators as gs;

    /// `%XX` escapes decoded back to the bytes they stand for.
    fn percent_decoded(url: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut rest = url.as_bytes();
        while let Some((&b, tail)) = rest.split_first() {
            if b == b'%' {
                out.push(u8::from_str_radix(std::str::from_utf8(&tail[..2]).unwrap(), 16).unwrap());
                rest = &tail[2..];
            } else {
                out.push(b);
                rest = tail;
            }
        }
        out
    }

    /// Any title makes a slug fit to name a file: letters and digits, each its own lowercase,
    /// words joined by single dashes, none at either end, within 60 bytes, never empty; and a
    /// slug is its own.
    #[hegel::test(test_cases = 500)]
    fn any_title_slugs_to_a_file_name(tc: TestCase) {
        let title = tc.draw(gs::text().max_size(80));
        let s = slug(&title);
        assert!(!s.is_empty() && s.len() <= SLUG_BYTES, "{s:?}");
        assert!(!s.starts_with('-') && !s.ends_with('-') && !s.contains("--"), "{s:?}");
        assert!(
            s.chars().all(|c| c == '-' || (c.is_alphanumeric() && c.to_lowercase().eq([c]))),
            "{title:?} slugs to {s:?}"
        );
        assert_eq!(slug(&s), s);
    }

    /// Any URL path piece holds only what URLs take as it is, and reads back as what it stood
    /// for; a `file:` URL leads to its path.
    #[hegel::test(test_cases = 300)]
    fn urls_read_back_as_their_paths(tc: TestCase) {
        let parts: Vec<String> =
            tc.draw(gs::vecs(gs::text().min_size(1).max_size(10).exclude_characters("/\0")).min_size(1).max_size(4));
        for part in &parts {
            let encoded = url_encode(part);
            assert!(encoded.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.~%".contains(&b)), "{encoded}");
            assert_eq!(percent_decoded(&encoded), part.as_bytes());
        }
        let path: PathBuf = std::iter::once("/".to_string()).chain(parts).collect();
        let url = file_url(&path);
        let decoded = String::from_utf8(percent_decoded(url.strip_prefix("file://").unwrap())).unwrap();
        assert_eq!(Path::new(&decoded), path.components().collect::<PathBuf>(), "{url}");
    }

    /// A project's audio is the browser-playable file next to it sharing the most of its name
    /// (four characters at least), the most compact of those; a recording stands for no other.
    #[hegel::test(test_cases = 25)]
    fn a_project_plays_the_export_sharing_most_of_its_name(tc: TestCase) {
        let stems = vec!["set one", "set one export", "set", "set two", "other", "SET ONE final"];
        let exts = vec!["mp3", "m4a", "wav", "flac", "ogg", "aiff", "txt", "aup3"];
        let files: Vec<(&str, &str)> =
            tc.draw(gs::vecs(gs::tuples!(gs::sampled_from(stems), gs::sampled_from(exts))).max_size(8));
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("set one session.aup3");
        std::fs::write(&project, b"x").unwrap();
        for (stem, ext) in &files {
            std::fs::write(dir.path().join(format!("{stem}.{ext}")), b"x").unwrap();
        }
        let shared = |p: &Path| {
            let stem = p.file_stem().unwrap().to_string_lossy().to_lowercase();
            "set one session".chars().zip(stem.chars()).take_while(|(a, b)| a == b).count()
        };
        let playable: Vec<PathBuf> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| browser_format(p).is_some() && shared(p) >= 4)
            .collect();
        match playable_audio(&project) {
            None => assert!(playable.is_empty(), "{playable:?}"),
            Some(chosen) => {
                let most = playable.iter().map(|p| shared(p)).max().unwrap();
                assert_eq!(shared(&chosen), most, "{chosen:?} of {playable:?}");
                let best = playable.iter().filter(|p| shared(p) == most).filter_map(|p| browser_format(p)).min();
                assert_eq!(browser_format(&chosen), best, "{chosen:?} of {playable:?}");
            }
        }
        let recording = dir.path().join("set one session.aiff");
        assert_eq!(project_audio(&recording), None);
        // A recording a browser plays is its own audio.
        let mp3 = dir.path().join("set one session.MP3");
        assert_eq!(playable_audio(&mp3).as_deref(), Some(mp3.as_path()));
    }

    /// Any report kept in the store is listed with what it found and reads back as it was,
    /// by an id made only of what ids are made of; nothing else is an id.
    #[hegel::test(test_cases = 20)]
    fn kept_reports_read_back_and_list_what_they_found(tc: TestCase) {
        use loclib_model::test_util::track;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path()).unwrap();
        let mut r = report(&tc.draw(gs::text().max_size(30)));
        r.mix.path = PathBuf::from(format!("/mixes/{}.wav", tc.draw(gs::text().alphabet("ab ï#").max_size(8))));
        r.plays =
            (0..tc.draw(gs::integers::<u32>().max_value(4))).map(|i| play(track(i, "A", "B", "/C/d.mp3"))).collect();
        r.coverage = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(1.0));
        let id = save(&store, &r).unwrap();
        assert_eq!(id, super::id(&r));
        assert!(id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'), "{id}");
        let back = load(dir.path(), &id).unwrap();
        assert_eq!(
            (back.mix.title, back.mix.path, back.plays.len()),
            (r.mix.title.clone(), r.mix.path.clone(), r.plays.len())
        );
        let listed = list(dir.path()).unwrap();
        assert_eq!(listed.len(), 1);
        let kept = &listed[0];
        assert_eq!((kept.id.as_str(), kept.tracks), (id.as_str(), Some(r.plays.len())));
        assert_eq!(kept.coverage, Some(r.coverage), "{kept:?}");
        assert_eq!((kept.title.as_str(), kept.mix.as_deref()), (r.mix.title.as_str(), Some(r.mix.path.as_path())));

        let other = tc.draw(gs::text().max_size(12));
        let err = load(dir.path(), &other).unwrap_err();
        let well_formed = !other.is_empty() && other.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
        match err.inner() {
            Error::NotFound(_) => assert!(well_formed && other != id, "{other:?}"),
            Error::Invalid(_) => assert!(!well_formed, "{other:?}"),
            e => panic!("{other:?}: {e}"),
        }
    }

    /// The page reads back any title, holds one entry for each play (and with the drive, what
    /// it adds to each), names the tracks the profile hides as "ID", and puts in its title
    /// element only what HTML takes as text.
    #[hegel::test(test_cases = 100)]
    fn the_page_holds_the_report_whatever_it_says(tc: TestCase) {
        use loclib_model::test_util::track;
        let title = tc.draw(gs::text().alphabet("ab <!-/script>&\"'é").max_size(30));
        let mut r = report(&title);
        r.plays =
            (0..tc.draw(gs::integers::<u32>().max_value(4))).map(|i| play(track(i, "A", &title, "/C/d.mp3"))).collect();
        for p in &mut r.plays {
            let artist = tc.draw(gs::sampled_from(vec!["A", "B"]));
            let name = format!("{artist} - {title} (dub)");
            p.alternates.push(super::super::Alternate {
                track_id: 99,
                name,
                file_path: "/C/e.mp3".into(),
                same_audio: false,
            });
        }
        let shared = tc.draw(gs::booleans());
        let dir = tempfile::tempdir().unwrap();
        let device = Device::open(dir.path()).unwrap();
        let library = Library { tracks: r.plays.iter().map(|p| p.track.clone()).collect(), ..Default::default() };
        let drive = tc.draw(gs::booleans()).then(|| Drive::new(&device, &library));
        let mut prefs = ProfilePrefs::default();
        let hidden = tc.draw(gs::booleans());
        if hidden {
            prefs.hidden_artists.push("A".into());
        }
        let audio = Some(Path::new("/mixes/set one.wav"));
        let html = page(&r, drive, audio, Path::new("/r/x.html"), &prefs, shared);
        let data = html.split_once(r#"<script id="data" type="application/json">"#).unwrap().1;
        let data = data.split_once("</script>").unwrap().0;
        let v: serde_json::Value = serde_json::from_str(data).unwrap_or_else(|e| panic!("{e}: {data}"));
        assert_eq!(v["report"]["mix"]["title"], title.as_str());
        let plays = v["report"]["plays"].as_array().unwrap();
        assert_eq!(plays.len(), r.plays.len());
        assert_eq!(v["extras"].as_array().unwrap().len(), if drive.is_some() { r.plays.len() } else { 0 });
        let named = if hidden { UNNAMED } else { title.as_str() };
        assert!(plays.iter().all(|p| p["track"]["title"] == named), "{plays:?}");
        for (p, play) in plays.iter().zip(&r.plays) {
            let alternate = &play.alternates[0];
            let hidden_alternate = hidden && alternate.name.starts_with("A - ");
            let name = if hidden_alternate { format!("{UNNAMED} - {UNNAMED}") } else { alternate.name.clone() };
            assert_eq!(p["alternates"][0]["name"], name.as_str());
        }
        let audio = if shared { "set%20one.wav" } else { "../mixes/set%20one.wav" };
        assert_eq!((v["audio"].as_str(), v["audioName"].as_str()), (Some(audio), Some("set one.wav")));
        let head = html.split_once("<title>").unwrap().1.split_once("</title>").unwrap().0;
        assert!(!head.contains('<') && !head.contains('>'), "{head}");
    }
}
