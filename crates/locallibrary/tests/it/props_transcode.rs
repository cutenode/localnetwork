//! Properties of packing tracks the players can't play as copies they can (`pack`'s transcode),
//! against the player table.

use std::fs;
use std::sync::OnceLock;

use hegel::TestCase;
use hegel::generators as gs;
use rekordbox_pdb::NewTrack;

use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::equipment::{self, Codec, MODELS, Model};
use locallibrary::pack::{self, Container, PackOptions, PackPlan, Transcode};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;

use crate::common::{self, Template, media};

/// The masters on the source drive: extension, sample rate and depth (0 for MP3). The WAVs and
/// MP3s are real audio, so only they are ever transcoded. The last is `T0.aiff`, beside `T0.wav`:
/// a WAV copy of it would take the other's name.
const MASTERS: [(&str, u32, u16); 14] = [
    ("wav", 44_100, 16),
    ("wav", 48_000, 24),
    ("wav", 88_200, 16),
    ("wav", 96_000, 24),
    ("wav", 192_000, 32),
    ("aiff", 96_000, 24),
    ("aiff", 44_100, 32),
    ("flac", 48_000, 16),
    ("flac", 192_000, 24),
    ("mp3", 44_100, 0),
    ("mp3", 22_050, 0),
    ("mp3", 48_000, 0),
    ("ogg", 44_100, 16),
    ("aiff", 192_000, 24),
];

fn stem(i: usize) -> usize {
    if i == MASTERS.len() - 1 { 0 } else { i }
}

fn master_path(i: usize) -> String {
    format!("Contents/Set/T{}.{}", stem(i), MASTERS[i].0)
}

fn wav(i: usize, secs: f64) -> Vec<u8> {
    media::wav_of(MASTERS[i].1, MASTERS[i].2, 2, secs)
}

/// The source drive (the twelve-track export), with the masters in playlist "Set" and the WAVs
/// above 48 kHz alone in "Wavs". Made once, as making it is slow, and written out for each case.
pub(crate) fn source() -> &'static Template {
    static SOURCE: OnceLock<Template> = OnceLock::new();
    SOURCE.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let root = common::bigger_drive(dir.path(), "Main");
        let ids: Vec<u32> = (0..MASTERS.len())
            .map(|i| {
                let (ext, rate, depth) = MASTERS[i];
                let track = NewTrack {
                    analyze_path: format!("/PIONEER/USBANLZ/P0SE/{i:08X}/ANLZ0000.DAT"),
                    duration: 1,
                    sample_rate: rate,
                    sample_depth: depth,
                    ..NewTrack::new(format!("T{i}"), format!("/{}", master_path(i)))
                };
                let audio = match ext {
                    "wav" => wav(i, 0.05),
                    "mp3" => media::mp3(),
                    _ => b"not audio".to_vec(),
                };
                common::add_track(&root, track, &audio)
            })
            .collect();
        let wavs: Vec<u32> =
            ids.iter().zip(MASTERS).filter(|(_, m)| m.0 == "wav" && m.1 > 48_000).map(|(id, _)| *id).collect();
        let set = |playlist: &str, ids: Vec<u32>| Edit::SetTracks {
            playlist: playlist.into(),
            tracks: TrackSelector { ids, ..Default::default() },
            sort: None,
            limit: None,
            create: true,
        };
        let mut d = Device::open_export(&root).unwrap();
        let edits = [set("Set", ids), set("Wavs", wavs)];
        let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
        edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
        d.adopt(None).unwrap();
        Template::of(&root)
    })
}

fn codec(ext: &str) -> Option<Codec> {
    match ext {
        "wav" => Some(Codec::Wav),
        "aiff" => Some(Codec::Aiff),
        "flac" => Some(Codec::Flac),
        "mp3" => Some(Codec::Mp3),
        _ => None,
    }
}

/// The players (mixers play nothing) that don't play `codec` at `rate` and `depth`.
fn not_playing<'a>(players: &[&'a Model], codec: Codec, rate: u32, depth: u16) -> Vec<&'a Model> {
    players.iter().copied().filter(|m| !m.is_mixer() && m.plays(codec, rate, depth).result.is_err()).collect()
}

/// Whether a copy in some lossless container plays on every one of `players`.
fn some_copy_plays(players: &[&Model]) -> bool {
    let rates = [22_050, 32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
    Container::ALL
        .iter()
        .any(|c| rates.iter().any(|&r| [16, 24, 32].iter().any(|&d| not_playing(players, c.codec(), r, d).is_empty())))
}

/// `wav-48000-24-tpdf-v1` as its container, rate and depth.
fn recipe(r: &str) -> (Codec, u32, u16) {
    let parts: Vec<&str> = r.split('-').collect();
    (codec(parts[0]).expect("a lossless container"), parts[1].parse().unwrap(), parts[2].parse().unwrap())
}

fn plan(src: &Device, target: &Device, opts: &PackOptions) -> PackPlan {
    pack::plan(src, target, opts, &Ctx::silent()).unwrap()
}

fn options(playlist: &str, players: &[String], container: Container, decode_lossy: bool) -> PackOptions {
    PackOptions {
        playlists: vec![playlist.into()],
        transcode: Some(Transcode { players: players.to_vec(), container, decode_lossy }),
        ..Default::default()
    }
}

/// For random players (mixers among them) and containers, the pack makes copies of exactly the
/// lossless masters some player can't play when some lossless format plays on every player (and
/// of lossy ones only when decoding them is asked for), in that format (the one asked for, when
/// it does) and named after the master, never taking another track's file and never encoding
/// lossy again; every other master a player can't play is left as it is, saying which player and
/// why. Once made, a copy whose master and recipe are as recorded is kept, and packing again
/// doesn't write it; a master replaced at the same path, or another container asked for, makes
/// it again, as does a pack forgotten.
#[hegel::test(test_cases = 4)]
fn copies_are_made_of_exactly_what_the_players_cant_play(tc: TestCase) {
    let names: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
    let chosen: Vec<String> =
        tc.draw(gs::vecs(gs::sampled_from(names)).min_size(1).max_size(3)).into_iter().map(String::from).collect();
    let players: Vec<&Model> =
        equipment::canonical(&chosen).unwrap().iter().map(|n| equipment::find(n).unwrap()).collect();
    let container = tc.draw(gs::sampled_from(Container::ALL.to_vec()));
    let decode = tc.draw(gs::weighted_booleans(0.3));
    let dir = tempfile::tempdir().unwrap();
    let mut src = Device::open_export(source().write(&dir.path().join("Main"))).unwrap();
    fs::create_dir(dir.path().join("Club")).unwrap();
    let mut club = Device::open(dir.path().join("Club")).unwrap();
    // A record of copies that can't be read counts as none, with a warning.
    let unreadable = tc.draw(gs::weighted_booleans(0.3));
    if unreadable {
        fs::create_dir_all(club.state_dir()).unwrap();
        fs::write(club.state_dir().join("transcodes.json"), b"{not json").unwrap();
    }
    let err = pack::plan(&src, &club, &options("Set", &[], container, decode), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("name the players"), "{err}");
    let p = plan(&src, &club, &options("Set", &chosen, container, decode));
    let warned = p.warnings.iter().any(|w| w.contains("transcodes.json"));
    assert_eq!(warned, unreadable && !p.transcodes.is_empty(), "{:?}", p.warnings);
    if !unreadable {
        // The same pack to a drive whose record can't be read plans the same copies, warning.
        fs::create_dir(dir.path().join("Spare")).unwrap();
        let spare = Device::open(dir.path().join("Spare")).unwrap();
        fs::create_dir_all(spare.state_dir()).unwrap();
        fs::write(spare.state_dir().join("transcodes.json"), b"{not json").unwrap();
        let s = plan(&src, &spare, &options("Set", &chosen, container, decode));
        let warned = s.warnings.iter().any(|w| w.contains("transcodes.json") && w.contains("made again"));
        assert_eq!(warned, !p.transcodes.is_empty(), "{:?}", s.warnings);
        let to = |p: &PackPlan| p.transcodes.iter().map(|t| (t.to.clone(), t.recipe.clone())).collect::<Vec<_>>();
        assert_eq!(to(&s), to(&p));
    }

    let mut lossy = 0;
    for (i, &(ext, rate, depth)) in MASTERS.iter().enumerate() {
        let from = master_path(i);
        let made = p.transcodes.iter().find(|t| t.from == from);
        let left = p.unplayable.iter().find(|u| u.track.file_path == format!("/{from}"));
        let Some(c) = codec(ext) else {
            assert!(made.is_none(), "{from}");
            assert_eq!(left.map(|u| u.why.as_str()), Some("no player plays .ogg files"));
            continue;
        };
        let missing = not_playing(&players, c, rate, depth);
        if missing.is_empty() {
            assert!(made.is_none() && left.is_none(), "{from} plays everywhere: {made:?} {left:?}");
            continue;
        }
        if !c.lossless() && !decode {
            lossy += 1;
            assert!(made.is_none(), "{from} is lossy: never encoded again");
            assert!(left.is_some_and(|u| missing.iter().all(|p| u.why.contains(p.name))), "{left:?}");
            continue;
        }
        let taken = |to: &str| (0..MASTERS.len()).any(|j| j != i && master_path(j).eq_ignore_ascii_case(to));
        match made {
            Some(t) => {
                let (codec, rate, depth) = recipe(&t.recipe);
                assert!(not_playing(&players, codec, rate, depth).is_empty(), "{} doesn't play everywhere", t.recipe);
                if not_playing(&players, container.codec(), rate, depth).is_empty() {
                    assert_eq!(codec, container.codec(), "the container asked for, when it plays: {}", t.recipe);
                }
                assert_eq!(t.to, format!("Contents/Set/T{}.{}", stem(i), t.recipe.split('-').next().unwrap()));
                assert!(!taken(&t.to), "{} takes another track's file", t.to);
                assert!(missing.iter().all(|p| t.why.contains(p.name)), "{:?}", t.why);
                assert!(!t.current && t.size > 0, "{t:?}");
                assert!(p.copies.iter().all(|c| c.path != from), "the master isn't packed");
            }
            None => {
                let why = left.map_or("", |u| u.why.as_str());
                match why.strip_prefix("its copy's name, ") {
                    Some(name) => assert!(taken(name.split(',').next().unwrap()), "{why}"),
                    None => {
                        assert!(!some_copy_plays(&players), "{from}: no copy though one would play: {left:?}");
                        assert_eq!(why, "no lossless format plays on all of these players");
                    }
                }
            }
        }
    }
    let warned = p.warnings.iter().any(|w| w.starts_with(&format!("{lossy} lossy track(s)")));
    assert_eq!(warned, lossy > 0, "{:?}", p.warnings);
    assert_eq!(p.transcode_bytes, p.transcodes.iter().map(|t| t.size).sum::<u64>());

    // Making the copies is slow (each file written is synced): only now and then, of the WAVs.
    let wavs = plan(&src, &club, &options("Wavs", &chosen, container, false));
    if wavs.transcodes.is_empty() || !tc.draw(gs::weighted_booleans(0.3)) {
        return;
    }
    let made = pack::apply(&wavs, &src, &mut club, None, &Ctx::silent()).unwrap();
    assert_eq!(made.transcoded, wavs.transcodes.len());
    club = Device::open(dir.path().join("Club")).unwrap();
    let again = plan(&src, &club, &PackOptions::default());
    assert_eq!((again.transcodes.len(), again.transcode_bytes), (wavs.transcodes.len(), 0));
    let made_at = |to: &str| fs::metadata(dir.path().join("Club").join(to)).unwrap().modified().unwrap();
    for t in &again.transcodes {
        assert!(t.current, "{t:?}");
        assert_eq!(fs::metadata(dir.path().join("Club").join(&t.to)).unwrap().len(), t.size);
    }
    let times: Vec<_> = again.transcodes.iter().map(|t| made_at(&t.to)).collect();
    assert_eq!(pack::apply(&again, &src, &mut club, None, &Ctx::silent()).unwrap().transcoded, 0);
    assert_eq!(again.transcodes.iter().map(|t| made_at(&t.to)).collect::<Vec<_>>(), times, "kept, not written");
    club = Device::open(dir.path().join("Club")).unwrap();
    // A better master at the same path, or another container: made again.
    let replaced = tc.draw(gs::optional(gs::sampled_from(&wavs.transcodes))).map(|t| t.from);
    let i =
        replaced.as_ref().map(|from| MASTERS.iter().enumerate().position(|(i, _)| master_path(i) == *from).unwrap());
    if let Some(i) = i {
        fs::write(src.root().join(master_path(i)), wav(i, 0.06)).unwrap();
        src = Device::open_export(src.root()).unwrap();
    }
    let other = Container::ALL.into_iter().find(|c| *c != container).unwrap();
    let asked = if replaced.is_some() { PackOptions::default() } else { options("Wavs", &chosen, other, false) };
    let next = plan(&src, &club, &asked);
    for t in &next.transcodes {
        let same_recipe = wavs.transcodes.iter().any(|o| o.from == t.from && o.recipe == t.recipe);
        assert_eq!(t.current, same_recipe && replaced.as_ref() != Some(&t.from), "{t:?}");
    }
    pack::forget(&club).unwrap();
    assert!(!club.state_dir().join("transcodes.json").exists());
    let forgotten = plan(&src, &club, &options("Wavs", &chosen, container, false));
    assert!(forgotten.transcodes.iter().all(|t| !t.current), "a forgotten pack keeps no copies");
}
