//! Library sources (feature `sources`): a plugin reads a library from the folders it's granted, a
//! page at a time, and the host builds a drive from it as from rekordbox's collection, leaving out
//! what isn't an audio file in those folders, and making analysis files for the tracks that bring
//! cues, beat grids or waveforms.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::anlz::{self, Bands, Beat};
use locallibrary::collection_export::{self, CollectionExportOptions, CollectionExportPlan};
use locallibrary::op::Planned;
use locallibrary::paths;
use locallibrary::plugin::SourceExport;

use super::*;

/// The echo manifest granted `fs:read`, with the library source `folder` (any of the test plugin's
/// reads the files in its folders).
fn sourcing() -> String {
    let options = "{ type = \"object\", properties = { per_page = { type = \"integer\" }, as_blob = { type = \
                   \"boolean\" }, again = { type = \"boolean\" }, also = { type = \"array\", items = { type = \
                   \"string\" } } } }";
    let source =
        format!("\n[[contributes.sources]]\nname = \"folder\"\ntitle = \"Audio files\"\noptions = {options}\n");
    granting(&["fs:read"]) + &source
}

/// `secs` of silence, as a WAV file.
fn wav(secs: f64) -> Vec<u8> {
    common::media::wav_of(44_100, 16, 1, secs)
}

fn beats(n: usize) -> Vec<Beat> {
    (0..n).map(|i| Beat { beat_number: (i % 4) as u16 + 1, bpm: 120.0, time_ms: 100 + i as u32 * 500 }).collect()
}

impl Fixture {
    fn sourcing() -> Fixture {
        Fixture::new(|dir| vec![spec(dir, &sourcing(), &[])], Options::default())
    }

    /// A folder of its own, made.
    fn folder(&self, name: &str) -> PathBuf {
        let dir = self._dir.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The folder the drive is built in.
    fn stick(&self) -> PathBuf {
        self._dir.path().join("stick")
    }

    /// Building the stick from what the source reads in `grants`, with `options`.
    fn request(&self, grants: &[&Path], options: Value) -> SourceExport {
        SourceExport {
            plugin: "echo".into(),
            source: "folder".into(),
            options,
            grants: grants.iter().map(|g| g.to_path_buf()).collect(),
            drive: self.stick().display().to_string(),
            export: CollectionExportOptions::default(),
        }
    }

    /// Plans building the stick from what the source reads in `grants`, with `options`.
    fn build(
        &self,
        grants: &[&Path],
        options: Value,
    ) -> Result<Planned<CollectionExportPlan, CollectionExportOptions>> {
        self.plugins.plan_source_export(&self.request(grants, options), Some(&self.store), &Ctx::silent())
    }
}

/// Writes `bytes` at `path`, and what the source says of it besides (`told`) beside it.
fn put(path: &Path, bytes: &[u8], told: Option<Value>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    if let Some(told) = told {
        std::fs::write(format!("{}.json", path.display()), told.to_string()).unwrap();
    }
}

/// The library on the stick, by title: each track with its analysis there.
fn built(stick: &Path) -> Vec<(locallibrary::Track, anlz::TrackAnalysis)> {
    let device = locallibrary::Device::open(stick).unwrap();
    let mut tracks: Vec<_> = device
        .library()
        .unwrap()
        .tracks
        .into_iter()
        .map(|t| (anlz::load(&device, &t), t))
        .map(|(a, t)| (t, a))
        .collect();
    tracks.sort_by(|a, b| a.0.title.cmp(&b.0.title));
    tracks
}

#[test]
fn a_source_builds_a_drive_from_the_audio_files_it_was_granted() {
    let f = Fixture::sourcing();
    let music = f.folder("music");
    let bands = Bands((0..1500).map(|i| [(i % 256) as u8, 100, 200]).collect());
    let told = json!({
        "title": "A", "artist": "Art", "album": "Alb", "key": "8A", "bpm": 124.0, "rating": 9,
        "my_tags": ["Mood/Dark", "untagged"],
        "cues": [
            { "hot_cue": 0, "start_ms": 1000, "name": "drop", "color": "red" },
            { "start_ms": 2000 },
            { "start_ms": 3000, "end_ms": 4000 },
            { "hot_cue": 0, "start_ms": 5000 },
            { "hot_cue": 9, "start_ms": 6000 },
        ],
        "beats": beats(16),
        "waveform": { "blob": "wave.bands", "columns_per_sec": 150.0 },
        "blobs": { "wave.bands": bands.pack() },
    });
    put(&music.join("a.wav"), &wav(10.0), Some(told));
    put(&music.join("sub/b.wav"), &wav(2.0), None);
    put(&music.join("notes.txt"), b"not audio", None);
    put(&music.join("fake.mp3"), b"not audio either", None);
    let lists = json!({ "Sets/Friday": ["a.wav", "sub/b.wav", "gone.wav"], "Sets/Empty": [] });
    std::fs::write(music.join("playlists.json"), lists.to_string()).unwrap();
    let outside = f.folder("outside").join("c.wav");
    put(&outside, &wav(1.0), None);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, music.join("link.wav")).unwrap();
    let options = json!({ "also": [outside, "relative.wav"] });

    let planned = f.build(&[&music], options.clone()).unwrap();
    let plan = &planned.plan;
    assert_eq!((plan.tracks, plan.playlist_count, plan.source.as_deref()), (2, 2, Some("echo:folder")));
    let real = real(&music);
    assert!(planned.summary.iter().any(|s| *s == format!("read the files Audio files finds in {}", real.display())));
    let warned = |what: &str| planned.warnings.iter().find(|w| w.contains(what)).cloned().unwrap_or_default();
    // fake.mp3, link.wav (on Unix), notes.txt, c.wav and relative.wav, the first three shown.
    let tracks = warned("track(s) left out");
    let left_out = if cfg!(unix) { 5 } else { 4 };
    assert!(tracks.starts_with(&format!("{left_out} track(s) left out, e.g. ")), "{:?}", planned.warnings);
    let mut whys = vec!["fake.mp3: can't be read as audio", "notes.txt: isn't an audio file players play"];
    if cfg!(unix) {
        whys.push("link.wav: isn't in a folder granted (its links resolved)");
    }
    for why in whys {
        assert!(tracks.contains(why), "{why}: {tracks}");
    }
    assert!(warned("playlist entr(ies)").starts_with("1 playlist entr(ies) left out"), "{:?}", planned.warnings);
    assert!(warned("My Tag(s)").contains("\"untagged\" isn't Category/Tag"), "{:?}", planned.warnings);
    assert!(warned("cue(s)").starts_with("2 cue(s) left out"), "{:?}", planned.warnings);

    let report = planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    assert_eq!((report.tracks, report.playlists, report.tagged_tracks), (2, 2, 1));
    let tracks = built(&f.stick());
    let [(a, at), (b, bt)] = &tracks[..] else { panic!("two tracks: {tracks:?}") };
    assert_eq!((a.artist.as_str(), a.album.as_str(), a.key.as_str(), a.bpm, a.rating), ("Art", "Alb", "Am", 124.0, 5));
    assert_eq!((a.duration_secs, a.sample_rate, b.title.as_str(), b.duration_secs), (10, 44_100, "b", 2));
    assert!(a.file_path.starts_with("/Contents/Art/Alb/"), "{}", a.file_path);
    let copied = paths::to_fs(&f.stick(), &paths::key(&a.file_path));
    assert_eq!(std::fs::read(copied).unwrap(), std::fs::read(music.join("a.wav")).unwrap());
    let mut cues: Vec<(Option<u8>, u32, Option<u32>, &str)> =
        at.cues.iter().map(|c| (c.hot_cue, c.start_ms, c.end_ms, c.comment.as_str())).collect();
    cues.sort();
    assert_eq!(cues, [(None, 2000, None, ""), (None, 3000, Some(4000), ""), (Some(0), 1000, None, "drop")]);
    assert_eq!(at.beats, beats(16));
    assert!(at.preview.is_some() && at.detail.is_some(), "its waveforms");
    assert!(b.analyze_path.is_empty() && bt.beats.is_empty(), "nothing to make analysis files of");
    let stick = locallibrary::Device::open(f.stick()).unwrap();
    let library = stick.library().unwrap();
    let friday = library.find_playlist("Sets/Friday").unwrap();
    let titles: Vec<&str> = friday.track_ids.iter().map(|id| library.track(*id).unwrap().title.as_str()).collect();
    assert_eq!(titles, ["A", "b"]);
    assert_eq!(collection_export::record(&stick).unwrap().unwrap().source.as_deref(), Some("echo:folder"));

    // Built again, nothing has changed: nothing is copied.
    let again = f.build(&[&music], options).unwrap();
    assert!(again.plan.copies.is_empty(), "{:?}", again.plan.copies);
    assert!(!again.plan.replaces_library);
}

/// Pages, in a message or in a blob, add up to the library; a source that never stops paging is
/// refused.
#[test]
fn a_source_answers_in_pages_and_blobs() {
    let f = Fixture::sourcing();
    let music = f.folder("music");
    for name in ["a", "b", "c"] {
        put(&music.join(format!("{name}.wav")), &wav(1.0), Some(json!({ "title": name })));
    }
    for options in [json!({ "per_page": 1 }), json!({ "per_page": 2, "as_blob": true })] {
        let planned = f.build(&[&music], options.clone()).unwrap();
        assert_eq!(planned.plan.tracks, 3, "{options}");
    }
    let e = f.build(&[&music], json!({ "per_page": 2, "again": true })).unwrap_err();
    assert_eq!(failure(e), "the Echo plugin failed: it answered the page after \"3\" twice");
    let e = f.build(&[&music], json!({ "per_page": "two" })).unwrap_err();
    assert_eq!(e.code(), "invalid", "options are checked against the source's schema: {e}");
}

/// What a track brings that can't be written (a key that isn't one, a beat grid out of order, a
/// waveform that isn't one, a loop ending before it starts, a colour players don't have) is left
/// out with a warning, and the track is written without it.
#[test]
fn what_a_track_brings_that_cant_be_written_is_left_out() {
    let f = Fixture::sourcing();
    let music = f.folder("music");
    let mut grid = beats(4);
    grid.swap(1, 2);
    let told = json!({
        "title": "D", "key": "x",
        "beats": grid,
        "waveform": { "blob": "none.bands", "columns_per_sec": 150.0 },
        "cues": [{ "start_ms": 500, "end_ms": 100 }, { "start_ms": 600, "color": "nope" }],
    });
    put(&music.join("d.wav"), &wav(2.0), Some(told));
    let other = json!({ "title": "E", "waveform": { "blob": "e.bands", "columns_per_sec": 0.0 }, "blobs": { "e.bands": [1, 2, 3] } });
    put(&music.join("e.wav"), &wav(2.0), Some(other));
    let planned = f.build(&[&music], Value::Null).unwrap();
    let warned = |what: &str| planned.warnings.iter().find(|w| w.contains(what)).cloned().unwrap_or_default();
    assert!(warned("key(s) left out").contains("\"x\" isn't a key"), "{:?}", planned.warnings);
    assert!(warned("beat grid(s) left out").contains("isn't after the one before it"), "{:?}", planned.warnings);
    assert!(warned("2 waveform(s) left out").contains("aren't more than 0"), "{:?}", planned.warnings);
    assert!(warned("cue(s) left out").contains("its loop doesn't end after it starts"), "{:?}", planned.warnings);
    assert!(warned("cue colour(s) left out").contains("unknown colour \"nope\""), "{:?}", planned.warnings);
    planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    let tracks = built(&f.stick());
    let [(d, da), (e, ea)] = &tracks[..] else { panic!("two tracks: {tracks:?}") };
    assert_eq!((d.title.as_str(), d.key.as_str(), d.bpm, e.title.as_str()), ("D", "", 0.0, "E"));
    let cues: Vec<u32> = da.cues.iter().map(|c| c.start_ms).collect();
    assert_eq!((cues, da.beats.is_empty(), da.preview.is_none()), (vec![600], true, true));
    assert!(e.analyze_path.is_empty() && ea.preview.is_none(), "nothing left to write for it");
}

/// The folders granted are absolute folders off the drive written to, at least one; and a file
/// that changed after the export was planned isn't copied.
#[test]
fn grants_are_checked_and_files_copied_as_they_were_checked() {
    let f = Fixture::sourcing();
    let music = f.folder("music");
    put(&music.join("a.wav"), &wav(1.0), None);
    let refused = |grants: &[&Path]| f.build(grants, Value::Null).map(|_| ()).unwrap_err();
    assert_eq!(refused(&[]).code(), "invalid");
    assert_eq!(refused(&[Path::new("music")]).code(), "invalid");
    assert_eq!(refused(&[&f._dir.path().join("nowhere")]).code(), "not_found");
    std::fs::create_dir_all(f.stick().join("Contents")).unwrap();
    let e = refused(&[&f.stick().join("Contents")]);
    assert!(e.to_string().contains("the drive written to"), "{e}");
    std::fs::remove_dir_all(f.stick()).unwrap();

    let planned = f.build(&[&music], Value::Null).unwrap();
    let mut changed = wav(1.0);
    *changed.last_mut().unwrap() = 1;
    std::fs::write(music.join("a.wav"), changed).unwrap();
    let e = planned.apply(Some(&f.store), &Ctx::silent()).unwrap_err();
    assert!(matches!(e, Error::Corrupt { .. }), "{e}");
}

/// What a drawn track's file is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Placed {
    /// A WAV file in the folder granted.
    Granted,
    /// One outside it.
    Outside,
    /// A link in the folder granted to one outside it.
    LinkedOut,
    /// Text named as an MP3, in the folder granted.
    NotAudio,
}

/// For any tracks a source reads, the drive holds exactly the WAV files in the folder granted,
/// each with its title and the cues it was given (a hot cue per slot, A-H), read back from the
/// analysis files made for it, its other cues and files left out with a warning.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_drive_holds_exactly_the_granted_audio_files_and_their_cues(tc: TestCase) {
    let f = Fixture::sourcing();
    let (music, outside) = (f.folder("music"), f.folder("outside"));
    let mut places = vec![Placed::Granted, Placed::Outside, Placed::NotAudio];
    if cfg!(unix) {
        places.push(Placed::LinkedOut);
    }
    let drawn: Vec<Placed> = tc.draw(gs::vecs(gs::sampled_from(places)).max_size(5));
    let (mut also, mut wanted) = (Vec::new(), Vec::new());
    for (i, placed) in drawn.iter().enumerate() {
        let slots: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(9)).max_size(4));
        let cues: Vec<Value> =
            slots.iter().enumerate().map(|(j, s)| json!({ "hot_cue": s, "start_ms": 100 * j as u32 })).collect();
        let told = json!({ "title": format!("t{i}"), "cues": cues });
        match placed {
            Placed::Granted => {
                put(&music.join(format!("{i}.wav")), &wav(1.0), Some(told));
                let mut seen = Vec::new();
                let kept = slots.iter().enumerate().filter(|(_, s)| {
                    **s <= 7 && !seen.contains(*s) && {
                        seen.push(**s);
                        true
                    }
                });
                let mut cues: Vec<(u8, u32)> = kept.map(|(j, s)| (*s, 100 * j as u32)).collect();
                cues.sort();
                wanted.push((format!("t{i}"), cues));
            }
            Placed::Outside => {
                let file = outside.join(format!("{i}.wav"));
                put(&file, &wav(1.0), None);
                also.push(file);
            }
            Placed::NotAudio => put(&music.join(format!("{i}.mp3")), b"text", Some(told)),
            Placed::LinkedOut => {
                let file = outside.join(format!("{i}.wav"));
                put(&file, &wav(1.0), None);
                #[cfg(unix)]
                std::os::unix::fs::symlink(&file, music.join(format!("{i}-link.wav"))).unwrap();
            }
        }
    }
    let planned = f.build(&[&music], json!({ "also": also })).unwrap();
    let refused = drawn.iter().filter(|p| **p != Placed::Granted).count();
    let said = planned.warnings.iter().any(|w| w.starts_with(&format!("{refused} track(s) left out")));
    assert_eq!(said, refused > 0, "{:?}", planned.warnings);
    planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    let mut got: Vec<(String, Vec<(u8, u32)>)> = built(&f.stick())
        .into_iter()
        .map(|(t, a)| {
            let mut cues: Vec<(u8, u32)> = a.cues.iter().map(|c| (c.hot_cue.unwrap(), c.start_ms)).collect();
            cues.sort();
            (t.title, cues)
        })
        .collect();
    got.sort();
    wanted.sort();
    assert_eq!(got, wanted);
}

/// `path` with its links resolved, as the host gives it to a plugin.
fn real(path: &Path) -> PathBuf {
    locallibrary::device::without_verbatim_prefix(path.canonicalize().unwrap())
}

/// What a source gives for a drawn track, and why the host leaves it out (if it does).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Given {
    /// A WAV file in the folder granted: written.
    Audio,
    /// A WAV file in the folder granted, given again by its path: the second is left out.
    Twice,
    /// Text named as an MP3, in the folder granted.
    NotAudio,
    /// A text file in the folder granted.
    Text,
    /// An empty WAV file in the folder granted.
    Empty,
    /// A WAV file on the drive written to, which is in the folder granted.
    OnDrive,
    /// A WAV file outside the folder granted, by its path.
    Outside,
    /// A path in the folder granted to nothing.
    Gone,
    /// A relative path.
    Relative,
    /// A link in the folder granted to a WAV file outside it.
    LinkedOut,
}

impl Given {
    /// Why a track given so is left out.
    fn why(self) -> Option<&'static str> {
        match self {
            Given::Audio => None,
            Given::Twice => Some("is another track's audio file too"),
            Given::NotAudio => Some("can't be read as audio"),
            Given::Text => Some("isn't an audio file players play"),
            Given::Empty => Some("is empty"),
            Given::OnDrive => Some("is on the drive written to"),
            Given::Outside | Given::LinkedOut => Some("isn't in a folder granted (its links resolved)"),
            Given::Gone => Some("isn't there"),
            Given::Relative => Some("isn't an absolute path"),
        }
    }
}

/// For any tracks a source gives, in its folders or by their paths, the plan holds those whose
/// file is a WAV file in the folder granted, off the drive written to, each once; one warning
/// counts the rest, giving the first three as the source gave them, with why each is left out.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn every_track_left_out_is_counted_with_why(tc: TestCase) {
    let f = Fixture::sourcing();
    let (music, outside) = (f.folder("music"), f.folder("outside"));
    let (real, drive) = (real(&music), music.join("stick"));
    let mut kinds = vec![
        Given::Audio,
        Given::Twice,
        Given::NotAudio,
        Given::Text,
        Given::Empty,
        Given::OnDrive,
        Given::Outside,
        Given::Gone,
        Given::Relative,
    ];
    if cfg!(unix) {
        kinds.push(Given::LinkedOut);
    }
    let drawn: Vec<Given> = tc.draw(gs::vecs(gs::sampled_from(kinds)).max_size(6));
    // The files the source finds in the folder, as it gives them (sorted), then those it's told.
    let (mut found, mut told) = (Vec::new(), Vec::new());
    for (i, given) in drawn.iter().enumerate() {
        let name = format!("{i:02}.wav");
        let in_music = |name: &str, bytes: &[u8], found: &mut Vec<(PathBuf, Option<&str>)>| {
            put(&music.join(name), bytes, None);
            found.push((real.join(name), given.why()));
        };
        match given {
            Given::Audio => in_music(&name, &wav(1.0), &mut found),
            Given::Empty => in_music(&name, b"", &mut found),
            Given::Twice => {
                in_music(&name, &wav(1.0), &mut found);
                found.last_mut().unwrap().1 = None;
                told.push((music.join(&name).display().to_string(), given.why()));
            }
            Given::NotAudio => in_music(&format!("{i:02}.mp3"), b"text", &mut found),
            Given::Text => in_music(&format!("{i:02}.txt"), b"text", &mut found),
            Given::OnDrive => in_music(&format!("stick/{name}"), &wav(1.0), &mut found),
            Given::Outside => {
                put(&outside.join(&name), &wav(1.0), None);
                told.push((outside.join(&name).display().to_string(), given.why()));
            }
            Given::Gone => told.push((music.join(format!("{i:02}-gone.wav")).display().to_string(), given.why())),
            Given::Relative => told.push((name, given.why())),
            Given::LinkedOut => {
                put(&outside.join(&name), &wav(1.0), None);
                #[cfg(unix)]
                std::os::unix::fs::symlink(outside.join(&name), music.join(format!("{i:02}-link.wav"))).unwrap();
                found.push((real.join(format!("{i:02}-link.wav")), given.why()));
            }
        }
    }
    found.sort();
    let found = found.into_iter().map(|(p, why)| (p.display().to_string(), why));
    let given: Vec<(String, Option<&str>)> = found.chain(told.iter().cloned()).collect();

    let paths: Vec<&str> = told.iter().map(|(p, _)| p.as_str()).collect();
    let mut req = f.request(&[&music], json!({ "also": paths }));
    req.drive = drive.display().to_string();
    req.export.replace = true;
    let planned = f.plugins.plan_source_export(&req, Some(&f.store), &Ctx::silent()).unwrap();
    let left: Vec<String> = given.iter().filter_map(|(p, why)| why.map(|why| format!("{p}: {why}"))).collect();
    assert_eq!(planned.plan.tracks, given.len() - left.len(), "{:?}", planned.warnings);
    let said: Vec<&String> = planned.warnings.iter().filter(|w| w.contains("track(s) left out")).collect();
    let wanted = format!(
        "{} track(s) left out, e.g. {}",
        left.len(),
        left.iter().take(3).cloned().collect::<Vec<_>>().join("; ")
    );
    assert_eq!(said, Vec::from_iter((!left.is_empty()).then_some(&wanted)));
}

/// For any number of tracks read a page at a time (any page size, in messages or blobs), the
/// drive holds every one, and each playlist the source gives (folders made for its path) holds
/// those of its entries that are tracks read, in order, the rest counted in a warning. A source
/// whose every page says there's one after it is refused at the first page it gives twice, and
/// one that can't read what it's given fails.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn pages_and_blobs_add_up_to_the_library(tc: TestCase) {
    let f = Fixture::sourcing();
    let music = f.folder("music");
    let n = tc.draw(gs::integers::<usize>().max_value(5));
    for i in 0..n {
        put(&music.join(format!("{i:02}.wav")), &wav(1.0), Some(json!({ "title": format!("t{i}") })));
    }
    // Each playlist's entries by the track they name, `n` naming a file that isn't there.
    let mut lists: Vec<(&str, Vec<usize>)> = Vec::new();
    for path in ["Sets/Friday", "Sets/Late/Peak", "Warmup"] {
        if tc.draw(gs::booleans()) {
            lists.push((path, tc.draw(gs::vecs(gs::integers::<usize>().max_value(n)).max_size(4))));
        }
    }
    let file = |i: usize| if i < n { format!("{i:02}.wav") } else { "gone.wav".into() };
    let named: serde_json::Map<String, Value> =
        lists.iter().map(|(p, entries)| (p.to_string(), entries.iter().map(|i| json!(file(*i))).collect())).collect();
    std::fs::write(music.join("playlists.json"), Value::Object(named).to_string()).unwrap();
    let mut options = json!({ "as_blob": tc.draw(gs::booleans()) });
    if tc.draw(gs::booleans()) {
        options["per_page"] = json!(tc.draw(gs::integers::<u8>().min_value(1).max_value(4)));
    }
    // What the source reads that isn't JSON (its playlists, or what it's told of a file) fails it.
    let broken = match tc.draw(gs::integers::<u8>().max_value(7)) {
        0 => Some(music.join("playlists.json")),
        1 => Some(music.join("zz.wav.json")),
        _ => None,
    };
    if let Some(broken) = broken {
        put(&music.join("zz.wav"), &wav(1.0), None);
        std::fs::write(broken, "{").unwrap();
        let e = f.build(&[&music], options).map(|_| ()).unwrap_err();
        let said = failure(e);
        assert!(said.starts_with("the Echo plugin failed: ") && said.contains("EOF"), "{said}");
        return;
    }
    if tc.draw(gs::integers::<u8>().max_value(3)) == 0 {
        options["again"] = json!(true);
        let e = f.build(&[&music], options).map(|_| ()).unwrap_err();
        assert_eq!(failure(e), format!("the Echo plugin failed: it answered the page after \"{n}\" twice"));
        return;
    }
    let planned = f.build(&[&music], options.clone()).unwrap();
    assert_eq!((planned.plan.tracks, planned.plan.playlist_count), (n, lists.len()), "{options}");
    let gone: usize = lists.iter().map(|(_, e)| e.iter().filter(|i| **i == n).count()).sum();
    let said = planned.warnings.iter().find(|w| w.contains("playlist entr(ies) left out"));
    assert_eq!(said.is_some(), gone > 0, "{:?}", planned.warnings);
    assert!(said.is_none_or(|w| w.starts_with(&format!("{gone} playlist entr(ies) left out, e.g. "))), "{said:?}");
    planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    let library = locallibrary::Device::open(f.stick()).unwrap().library().unwrap();
    assert_eq!(library.tracks.len(), n);
    for (path, entries) in &lists {
        let list = library.find_playlist(path).unwrap_or_else(|| panic!("no {path}"));
        let titles: Vec<&str> = list.track_ids.iter().map(|id| library.track(*id).unwrap().title.as_str()).collect();
        let wanted: Vec<String> = entries.iter().filter(|i| **i < n).map(|i| format!("t{i}")).collect();
        assert_eq!(titles, wanted, "{path}");
    }
}

/// What a drawn track brings besides its file, as the source tells it, and what the host keeps.
struct Brought {
    told: Value,
    /// Its title, key, tempo and rating as written.
    written: (String, &'static str, f64, u8),
    /// Its cues kept, beat grid kept, and whether its waveform is.
    cues: Vec<(Option<u8>, u32, Option<u32>)>,
    beats: bool,
    waveform: bool,
    /// How many of each kind of thing it brings are left out, by the warning's name for them.
    left: [(&'static str, usize); 6],
}

/// A track (the `i`th, its file named `name`) bringing a key, a beat grid, a waveform, My Tags
/// and cues, each drawn to be one players take or one they don't.
#[hegel::test_helper]
fn brought(tc: &TestCase, i: usize, name: &str) -> Brought {
    let mut left =
        [("key(s)", 0), ("beat grid(s)", 0), ("waveform(s)", 0), ("My Tag(s)", 0), ("cue(s)", 0), ("cue colour(s)", 0)];
    let (key, written_key) = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => (Value::Null, ""),
        1 => (json!("8A"), "Am"),
        2 => (json!("  "), ""),
        _ => {
            left[0].1 += 1;
            (json!("x"), "")
        }
    };
    let mut grid = beats(8);
    let beats_kept = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => {
            grid.clear();
            false
        }
        1 => true,
        _ => {
            grid.swap(2, 3);
            left[1].1 += 1;
            false
        }
    };
    // The tracks of a page share a scratch folder: each names its waveform's blob for itself.
    let blob = format!("{i}.bands");
    let packed = Bands((0..300).map(|c| [(c % 256) as u8, 50, 9]).collect()).pack();
    let (columns_per_sec, bytes, waveform_kept) = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => (None, None, false),
        1 => (Some(150.0), Some(json!(packed)), true),
        2 => (Some(0.0), Some(json!(packed)), false),
        3 => (Some(150.0), None, false),
        4 => (Some(150.0), Some(json!([1, 2])), false),
        _ => (Some(150.0), Some(json!([])), false),
    };
    let waveform = columns_per_sec.map(|c| json!({ "blob": blob, "columns_per_sec": c }));
    let blobs = Value::Object(bytes.map(|b| (blob.clone(), b)).into_iter().collect());
    left[2].1 += usize::from(waveform.is_some() && !waveform_kept);
    let tags: Vec<&str> =
        tc.draw(gs::vecs(gs::sampled_from(vec!["Mood/Dark", "Genre/House", "untagged", "/x", "Mood/ "])).max_size(3));
    left[3].1 += tags.iter().filter(|t| !t.starts_with("Mood/D") && !t.starts_with("Genre/")).count();
    let (mut cues, mut kept, mut slots) = (Vec::new(), Vec::new(), Vec::new());
    for j in 0..tc.draw(gs::integers::<u32>().max_value(4)) {
        let start = 1000 + j * 1000;
        let hot: Option<u8> = tc.draw(gs::booleans()).then(|| tc.draw(gs::integers::<u8>().max_value(9)));
        let end = match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => None,
            1 => Some(start + 500),
            _ => Some(start),
        };
        let colour = tc.draw(gs::sampled_from(vec![None, Some("red"), Some("nope")]));
        cues.push(json!({ "hot_cue": hot, "start_ms": start, "end_ms": end, "color": colour }));
        let refused = match hot {
            Some(s) if s > 7 => true,
            Some(s) if slots.contains(&s) => true,
            _ => end == Some(start),
        };
        if let Some(s) = hot.filter(|s| *s <= 7) {
            slots.push(s);
        }
        if refused {
            left[4].1 += 1;
            continue;
        }
        left[5].1 += usize::from(colour == Some("nope"));
        kept.push((hot, start, end));
    }
    let bpm = tc.draw(gs::sampled_from(vec![None, Some(124.0), Some(-1.0)]));
    let written_bpm = bpm.filter(|b: &f64| *b > 0.0).or(beats_kept.then_some(120.0)).unwrap_or(0.0);
    let titled = tc.draw(gs::booleans());
    let title = if titled { format!("t{i}") } else { String::new() };
    let rating = tc.draw(gs::integers::<u8>().max_value(9));
    let told = json!({
        "title": title, "key": key, "beats": grid, "waveform": waveform, "blobs": blobs, "my_tags": tags,
        "cues": cues, "bpm": bpm, "rating": rating,
    });
    let written_title = if titled { title } else { name.trim_end_matches(".wav").to_string() };
    kept.sort();
    Brought {
        told,
        written: (written_title, written_key, written_bpm, rating.min(5)),
        cues: kept,
        beats: beats_kept,
        waveform: waveform_kept,
        left,
    }
}

/// For any tracks bringing keys, beat grids, waveforms, My Tags and cues players take or don't,
/// each is written with what players take, read back from the drive (its title its file's name
/// when it has none, its tempo the beat grid's when it gives none), and one warning a kind counts
/// what's left out.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn what_tracks_bring_is_written_or_counted_as_left_out(tc: TestCase) {
    let f = Fixture::sourcing();
    let music = f.folder("music");
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let brought: Vec<Brought> = (0..n)
        .map(|i| {
            let name = format!("{i:02}.wav");
            let b = brought(&tc, i, &name);
            // Long enough for its beat grid and cues.
            put(&music.join(&name), &wav(5.0), Some(b.told.clone()));
            b
        })
        .collect();
    let planned = f.build(&[&music], Value::Null).unwrap();
    for (i, what) in brought[0].left.iter().map(|(what, _)| *what).enumerate() {
        let left: usize = brought.iter().map(|b| b.left[i].1).sum();
        let said = planned.warnings.iter().find(|w| w.contains(&format!(" {what} left out, e.g. ")));
        let count = said.and_then(|w| w.split(' ').next()?.parse::<usize>().ok());
        assert_eq!(count, (left > 0).then_some(left), "{what}: {:?}", planned.warnings);
    }
    planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
    let mut wanted: Vec<&Brought> = brought.iter().collect();
    wanted.sort_by(|a, b| a.written.0.cmp(&b.written.0));
    let tracks = built(&f.stick());
    assert_eq!(tracks.len(), n);
    for ((t, a), b) in tracks.iter().zip(wanted) {
        assert_eq!((t.title.clone(), t.key.as_str(), t.bpm, t.rating), b.written);
        let mut cues: Vec<(Option<u8>, u32, Option<u32>)> =
            a.cues.iter().map(|c| (c.hot_cue, c.start_ms, c.end_ms)).collect();
        cues.sort();
        assert_eq!(cues, b.cues, "{}", t.title);
        assert_eq!((a.beats.is_empty(), a.preview.is_some()), (!b.beats, b.waveform), "{}", t.title);
        let nothing = b.cues.is_empty() && !b.beats && !b.waveform;
        assert_eq!(t.analyze_path.is_empty(), nothing, "analysis files only for what there is: {}", t.title);
    }
}

/// Grows `file` by a byte as the export plan starts hashing the files it copies, as a file
/// being written while the export is planned does.
struct GrowsWhileHashed {
    file: PathBuf,
    grown: AtomicBool,
}

impl locallibrary::Progress for GrowsWhileHashed {
    fn on_event(&self, e: &locallibrary::Event) {
        let hashing = matches!(e, locallibrary::Event::Item { phase: locallibrary::progress::Phase::Hash, .. });
        if hashing && !self.grown.swap(true, Ordering::SeqCst) {
            grow(&self.file);
        }
    }
}

/// Adds a byte to the end of `file`.
fn grow(file: &Path) {
    use std::io::Write;
    std::fs::OpenOptions::new().append(true).open(file).unwrap().write_all(&[0]).unwrap();
}

/// For any of a source's files changed while the export is planned, after its check, or after
/// the plan, or none: a size changed while it's planned refuses the plan, naming the file; any
/// change after it refuses the copy; with none, each file is copied as it is, and planned again
/// there's nothing to copy.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn files_are_copied_only_as_they_were_checked(tc: TestCase) {
    let f = Fixture::sourcing();
    let music = f.folder("music");
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let files: Vec<PathBuf> = (0..n).map(|i| music.join(format!("{i:02}.wav"))).collect();
    for (i, file) in files.iter().enumerate() {
        put(file, &wav(1.0 + i as f64), None);
    }
    let changed = &files[tc.draw(gs::integers::<usize>().max_value(n - 1))];
    let name = changed.file_name().unwrap().to_string_lossy().into_owned();
    let watch = GrowsWhileHashed { file: changed.clone(), grown: AtomicBool::new(false) };
    let req = f.request(&[&music], Value::Null);
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {
            let e = f
                .plugins
                .plan_source_export(&req, Some(&f.store), &Ctx::new(&watch, locallibrary::CancelToken::new()))
                .unwrap_err();
            assert_eq!(e.code(), "invalid", "{e}");
            assert!(
                e.to_string().ends_with(&format!("{name} changed while the export was planned; plan it again")),
                "{e}"
            );
        }
        changed_after @ (1 | 2) => {
            let planned = f.build(&[&music], Value::Null).unwrap();
            match changed_after {
                1 => common::rot(changed),
                _ => grow(changed),
            }
            let e = planned.apply(Some(&f.store), &Ctx::silent()).unwrap_err();
            let Error::Corrupt { path, .. } = &e else { panic!("{e}") };
            assert!(path.ends_with(&name), "{e}");
        }
        _ => {
            let planned = f.build(&[&music], Value::Null).unwrap();
            assert_eq!(planned.plan.copies.iter().filter(|c| c.path.ends_with(".wav")).count(), n);
            planned.apply(Some(&f.store), &Ctx::silent()).unwrap();
            let mut copied: Vec<Vec<u8>> = built(&f.stick())
                .iter()
                .map(|(t, _)| std::fs::read(paths::to_fs(&f.stick(), &paths::key(&t.file_path))).unwrap())
                .collect();
            let mut wanted: Vec<Vec<u8>> = files.iter().map(|p| std::fs::read(p).unwrap()).collect();
            copied.sort();
            wanted.sort();
            assert!(copied == wanted, "each file copied as it is");
            assert!(f.build(&[&music], Value::Null).unwrap().plan.copies.is_empty(), "nothing changed since");
        }
    }
}

/// A folder a drawn export grants the source.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grant {
    /// The music folder, with one WAV file.
    Music,
    /// The music folder again, spelled another way.
    MusicAgain,
    /// Another folder, with one WAV file.
    Other,
    /// A relative path.
    Relative,
    /// A path to nothing.
    Nowhere,
    /// A file, not a folder.
    File,
    /// A folder on the drive written to.
    OnDrive,
}

/// For any folders granted, the export is refused with none, at the first that's relative (as
/// invalid) or isn't a folder (as not found), or for one on the drive written to; otherwise the
/// source reads each folder once, its links resolved, and the plan says which it read.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn only_absolute_folders_off_the_drive_are_granted(tc: TestCase) {
    let f = Fixture::sourcing();
    let (music, other) = (f.folder("music"), f.folder("other"));
    put(&music.join("a.wav"), &wav(1.0), None);
    put(&other.join("b.wav"), &wav(1.0), None);
    let kinds = vec![
        Grant::Music,
        Grant::MusicAgain,
        Grant::Other,
        Grant::Relative,
        Grant::Nowhere,
        Grant::File,
        Grant::OnDrive,
    ];
    let drawn: Vec<Grant> = tc.draw(gs::vecs(gs::sampled_from(kinds)).max_size(4));
    if drawn.contains(&Grant::OnDrive) {
        std::fs::create_dir_all(f.stick().join("Contents")).unwrap();
    }
    let grants: Vec<PathBuf> = drawn
        .iter()
        .map(|g| match g {
            Grant::Music => music.clone(),
            Grant::MusicAgain => music.join("."),
            Grant::Other => other.clone(),
            Grant::Relative => PathBuf::from("music"),
            Grant::Nowhere => f._dir.path().join("nowhere"),
            Grant::File => music.join("a.wav"),
            Grant::OnDrive => f.stick().join("Contents"),
        })
        .collect();
    let given: Vec<&Path> = grants.iter().map(PathBuf::as_path).collect();
    let got = f.build(&given, Value::Null);
    let refused = drawn.iter().find(|g| matches!(g, Grant::Relative | Grant::Nowhere | Grant::File));
    let e = match (drawn.is_empty(), refused) {
        (true, _) => Some(("invalid", "grant the plugin a folder")),
        (_, Some(Grant::Relative)) => Some(("invalid", "must be an absolute path")),
        (_, Some(_)) => Some(("not_found", "the folder")),
        _ if drawn.contains(&Grant::OnDrive) => Some(("invalid", "the drive written to: grant a folder elsewhere")),
        _ => None,
    };
    if let Some((code, said)) = e {
        let e = got.map(|_| ()).unwrap_err();
        assert!(e.code() == code && e.to_string().contains(said), "{drawn:?}: {e}");
        return;
    }
    let planned = got.unwrap();
    let mut read: Vec<PathBuf> = Vec::new();
    for g in &grants {
        let real = real(g);
        if !read.contains(&real) {
            read.push(real);
        }
    }
    assert_eq!(planned.plan.tracks, read.len(), "a track a folder");
    let folders: Vec<String> = read.iter().map(|r| r.display().to_string()).collect();
    let summary = format!("read the files Audio files finds in {}", folders.join(", "));
    assert!(planned.summary.contains(&summary), "{:?}", planned.summary);
}
