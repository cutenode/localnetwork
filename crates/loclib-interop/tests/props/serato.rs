//! Properties of Serato's crates and its `Serato Markers2` cue tag, read back by the formats'
//! descriptions (the mixxx wiki, serato-tags) and, for tags written into audio, by lofty.

use std::path::{Path, PathBuf};

use base64::prelude::*;
use hegel::generators::{self as gs, PrintableGenerator};
use hegel::{Generator, TestCase};
use loclib_interop::serato::{self, Copies, Markers};
use loclib_interop::{Content, Error, Export, Source};
use loclib_model::analysis::Cue;
use loclib_model::{Library, Track, paths};
use loclib_progress::{CancelToken, Ctx};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::common;

/// Serato's 18 hot cue colours (serato-tags' `docs/colors.md`): hues 20° apart from red.
const CUE_COLOURS: [[u8; 3]; 18] = [
    [0xcc, 0x00, 0x00],
    [0xcc, 0x44, 0x00],
    [0xcc, 0x88, 0x00],
    [0xcc, 0xcc, 0x00],
    [0x88, 0xcc, 0x00],
    [0x44, 0xcc, 0x00],
    [0x00, 0xcc, 0x00],
    [0x00, 0xcc, 0x44],
    [0x00, 0xcc, 0x88],
    [0x00, 0xcc, 0xcc],
    [0x00, 0x88, 0xcc],
    [0x00, 0x44, 0xcc],
    [0x00, 0x00, 0xcc],
    [0x44, 0x00, 0xcc],
    [0x88, 0x00, 0xcc],
    [0xcc, 0x00, 0xcc],
    [0xcc, 0x00, 0x88],
    [0xcc, 0x00, 0x44],
];

/// The colours Serato gives pads 1-8 when a cue has none.
const PAD_COLOURS: [[u8; 3]; 8] = [
    [0xcc, 0x00, 0x00],
    [0xcc, 0x88, 0x00],
    [0x00, 0x00, 0xcc],
    [0xcc, 0xcc, 0x00],
    [0x00, 0xcc, 0x00],
    [0xcc, 0x00, 0xcc],
    [0x00, 0xcc, 0xcc],
    [0x88, 0x00, 0xcc],
];

/// HSV hue in degrees, `None` for a grey (channels within 16 of each other).
fn hue(rgb: [u8; 3]) -> Option<f64> {
    if rgb.iter().max()? - rgb.iter().min()? < 16 {
        return None;
    }
    let [r, g, b] = rgb.map(|c| f64::from(c) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let h = if max == r {
        60.0 * ((g - b) / (max - min))
    } else if max == g {
        60.0 * ((b - r) / (max - min)) + 120.0
    } else {
        60.0 * ((r - g) / (max - min)) + 240.0
    };
    Some(h.rem_euclid(360.0))
}

/// A `Serato Markers2` frame's entries: undoes the frame's `01 01`, its base64 (lines of 72, `=`
/// written as `A`) and NUL padding, then splits `01 01` and the NUL-terminated, length-prefixed
/// entries up to the final NUL.
fn decode_markers(frame: &[u8]) -> Vec<(String, Vec<u8>)> {
    assert!(frame.len() >= 470, "padded to 470 bytes");
    assert_eq!(&frame[..2], [1, 1]);
    let end = frame.iter().skip(2).position(|&b| b == 0).map_or(frame.len(), |i| i + 2);
    assert!(frame[end..].iter().all(|&b| b == 0), "only NULs after the text");
    assert!(end == frame.len() || frame.len() == 470, "padding only up to 470 bytes");
    let lines: Vec<&[u8]> = frame[2..end].split(|&b| b == b'\n').collect();
    let (last, full) = lines.split_last().unwrap();
    assert!(full.iter().all(|l| l.len() == 72) && (1..=72).contains(&last.len()), "lines of 72");
    let text = lines.concat();
    assert!(!text.contains(&b'='), "padding is written as A");
    let p = BASE64_STANDARD.decode(&text).expect("base64");
    assert_eq!(&p[..2], [1, 1]);
    let mut at = 2;
    let mut out = Vec::new();
    loop {
        let nul = at + p[at..].iter().position(|&b| b == 0).expect("a NUL-terminated name");
        let name = String::from_utf8(p[at..nul].to_vec()).unwrap();
        if name.is_empty() {
            // The final NUL, then at most the two zero bytes `A` padding decodes to.
            assert!(p.len() - nul <= 3 && p[nul..].iter().all(|&b| b == 0), "{:?}", &p[nul..]);
            return out;
        }
        let len = u32::from_be_bytes(p[nul + 1..nul + 5].try_into().unwrap()) as usize;
        out.push((name, p[nul + 5..nul + 5 + len].to_vec()));
        at = nul + 5 + len;
    }
}

/// The NUL-terminated name ending an entry's data, with nothing after it.
fn tail_name(d: &[u8]) -> String {
    let (last, name) = d.split_last().unwrap();
    assert_eq!(*last, 0);
    String::from_utf8(name.to_vec()).unwrap()
}

/// What [`decode_markers`] found, as [`Markers`] holds it (`left_out` isn't written).
fn markers_of(entries: &[(String, Vec<u8>)]) -> Markers {
    let mut m = Markers::default();
    for (name, d) in entries {
        let be = |i: usize| u32::from_be_bytes(d[i..i + 4].try_into().unwrap());
        match name.as_str() {
            "COLOR" => {
                assert_eq!((d.len(), d[0]), (4, 0));
                m.colour = [d[1], d[2], d[3]];
            }
            "CUE" => {
                assert_eq!((d[0], d[6], &d[10..12]), (0, 0, &[0, 0][..]));
                m.cues.push((d[1], be(2), [d[7], d[8], d[9]], tail_name(&d[12..])));
            }
            "LOOP" => {
                assert_eq!((d[0], &d[10..14], &d[18..20]), (0, &[0xff; 4][..], &[0, 0][..]));
                assert_eq!(&d[14..18], [0x00, 0x27, 0xaa, 0xe1], "saved loops' colour");
                m.loops.push((d[1], be(2), be(6), tail_name(&d[20..])));
            }
            "BPMLOCK" => assert_eq!(d, &[0]),
            other => panic!("unknown entry {other}"),
        }
    }
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!((names.first(), names.last()), (Some(&"COLOR"), Some(&"BPMLOCK")));
    m
}

fn markers() -> impl PrintableGenerator<Markers> {
    let name = || hegel::one_of!(common::text(), gs::text().min_size(60).max_size(200));
    hegel::tuples!(
        gs::vecs(hegel::tuples!(
            gs::integers::<u8>().max_value(7),
            gs::integers::<u32>(),
            gs::arrays::<_, u8, 3>(gs::integers::<u8>()),
            name()
        ))
        .max_size(8),
        gs::vecs(hegel::tuples!(
            gs::integers::<u8>().max_value(7),
            gs::integers::<u32>(),
            gs::integers::<u32>(),
            name()
        ))
        .max_size(8),
        gs::arrays::<_, u8, 3>(gs::integers::<u8>()),
    )
    .map(|(cues, loops, colour)| Markers { cues, loops, colour, left_out: 0 })
    .print_as_debug()
}

/// Every cue, loop and the track colour come back from the frame as they went in (names less
/// their NULs, which would end them early), in order, base64 in lines of 72 and the frame padded
/// to 470 bytes.
#[hegel::test(test_cases = 300)]
fn markers_decode_to_what_was_encoded(tc: TestCase) {
    let m = tc.draw(markers());
    let back = markers_of(&decode_markers(&m.encode()));
    let unnul = |s: &String| s.replace('\0', "");
    let want = Markers {
        cues: m.cues.iter().map(|(p, at, rgb, n)| (*p, *at, *rgb, unnul(n))).collect(),
        loops: m.loops.iter().map(|(s, a, b, n)| (*s, *a, *b, unnul(n))).collect(),
        colour: m.colour,
        left_out: 0,
    };
    assert_eq!(back, want);
}

/// rekordbox's cues as Serato's: hot cues on their own pads (in one of Serato's colours within 10°
/// of rekordbox's hue, or the pad's own for none or a grey), hot cue loops in their own slots,
/// memory loops in the lowest slots left; everything else is counted as left out, so nothing goes
/// missing silently.
#[hegel::test(test_cases = 400)]
fn rekordbox_cues_go_to_pads_and_slots(tc: TestCase) {
    let colour = tc.draw(gs::optional(gs::sampled_from(common::COLOURS.to_vec()).print_as_debug()));
    let cues: Vec<Cue> = tc.draw(gs::vecs(common::cue()).max_size(20));
    let m = Markers::from_rekordbox(colour, &cues);
    assert_eq!(m.cues.len() + m.loops.len() + m.left_out, cues.len(), "{m:?}");

    let (mut pads, mut loops, mut left_out) = (Vec::new(), Vec::new(), 0);
    for c in &cues {
        match (c.hot_cue, c.end_ms) {
            (Some(h @ 0..8), None) => pads.push((h, c.start_ms, c.comment.clone())),
            (Some(h @ 0..8), Some(end)) => loops.push((h, c.start_ms, end, c.comment.clone())),
            (None, Some(_)) => {}
            _ => left_out += 1,
        }
    }
    let mut used: FxHashSet<u8> = loops.iter().map(|l| l.0).collect();
    for c in &cues {
        let (None, Some(end)) = (c.hot_cue, c.end_ms) else { continue };
        match (0..8).find(|s| !used.contains(s)) {
            Some(slot) => {
                used.insert(slot);
                loops.push((slot, c.start_ms, end, c.comment.clone()));
            }
            None => left_out += 1,
        }
    }
    pads.sort_by_key(|p| p.0);
    loops.sort_by_key(|l| l.0);
    let got_pads: Vec<(u8, u32, String)> = m.cues.iter().map(|(p, at, _, n)| (*p, *at, n.clone())).collect();
    assert_eq!(got_pads, pads);
    assert_eq!(m.loops, loops);
    assert_eq!(m.left_out, left_out);

    // Each pad's colour, by its cue (pads of one number keep their order).
    let mut sources: Vec<&Cue> =
        cues.iter().filter(|c| c.end_ms.is_none() && c.hot_cue.is_some_and(|h| h < 8)).collect();
    sources.sort_by_key(|c| c.hot_cue);
    for ((pad, _, rgb, _), c) in m.cues.iter().zip(sources) {
        match c.color.and_then(hue) {
            None => assert_eq!(*rgb, PAD_COLOURS[*pad as usize]),
            Some(h) => {
                assert!(CUE_COLOURS.contains(rgb), "{rgb:x?}");
                let d = (hue(*rgb).unwrap() - h).rem_euclid(360.0);
                assert!(d.min(360.0 - d) <= 10.0 + 1e-9, "{rgb:x?} is {d}° from {:x?}", c.color);
            }
        }
    }
    let [r, g, b] = m.colour;
    match colour {
        None => assert_eq!(m.colour, [0xff; 3], "white is no colour"),
        Some(_) => assert_eq!((r.min(g).min(b), r.max(g).max(b)), (0x99, 0xff), "a pastel: {:x?}", m.colour),
    }
}

/// A crate file's records: tag, big-endian length, data.
fn records(mut b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    while !b.is_empty() {
        let len = u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize;
        out.push((b[..4].try_into().unwrap(), &b[8..8 + len]));
        b = &b[8 + len..];
    }
    out
}

fn utf16be(b: &[u8]) -> String {
    assert_eq!(b.len() % 2, 0);
    String::from_utf16(&b.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap()
}

/// The tracks a `.crate` file lists, after checking its header records.
fn crate_tracks(bytes: &[u8]) -> Vec<String> {
    let recs = records(bytes);
    assert_eq!((&recs[0].0, utf16be(recs[0].1).as_str()), (b"vrsn", "1.0/Serato ScratchLive Crate"));
    assert_eq!(&recs[1].0, b"osrt");
    let sort = records(recs[1].1);
    assert_eq!((&sort[0].0, utf16be(sort[0].1).as_str(), &sort[1].0, sort[1].1), (b"tvcn", "#", b"brev", &[0][..]));
    let columns: Vec<String> = recs[2..9]
        .iter()
        .map(|(tag, data)| {
            assert_eq!(tag, b"ovct");
            let col = records(data);
            assert_eq!((&col[0].0, &col[1].0, utf16be(col[1].1).as_str()), (b"tvcn", b"tvcw", "0"));
            utf16be(col[0].1)
        })
        .collect();
    assert_eq!(columns, ["song", "artist", "bpm", "key", "album", "length", "comment"]);
    recs[9..]
        .iter()
        .map(|(tag, data)| {
            assert_eq!(tag, b"otrk");
            let ptrk = records(data);
            assert_eq!((ptrk.len(), &ptrk[0].0), (1, b"ptrk"));
            utf16be(ptrk[0].1)
        })
        .collect()
}

/// Where the crates' tracks are, and its volume: a Mac volume, a Windows drive, or the startup
/// disk (`/`).
#[hegel::test_helper]
fn root(tc: &TestCase) -> (PathBuf, &'static str) {
    let (root, volume) =
        tc.draw(gs::sampled_from(vec![("/Volumes/USB", "/Volumes/USB"), ("e:\\", "e:\\"), ("/Users/me/Music", "/")]));
    (PathBuf::from(root), volume)
}

/// A path as a crate names it: relative to `volume`'s root, `/`-separated.
fn relative(path: &Path, volume: &str) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let rest = match volume {
        "/" => &s,
        "e:\\" => &s[2..],
        v => s.strip_prefix(v).unwrap(),
    };
    rest.trim_start_matches('/').to_string()
}

/// Each crate an export writes, by name, with the tracks it lists.
fn crates(export: &Export) -> FxHashMap<String, Vec<String>> {
    export
        .files
        .iter()
        .filter_map(|f| {
            let name = f.path.strip_prefix("out/_Serato_/Subcrates").ok()?.to_str()?.strip_suffix(".crate")?;
            let Content::Bytes(b) = &f.content else { panic!("a crate is written out") };
            assert_eq!(f.bytes, b.len() as u64);
            Some((name.to_string(), crate_tracks(b)))
        })
        .collect()
}

/// Every playlist and folder gets a `.crate` file: names unique ignoring case and short enough
/// for a file name, a subcrate's its folder's whole crate name, `%%` and at least the start of its
/// own name (which holds no `%%`), cut only to fit 200 bytes with 10 for each level of crates
/// below it, each listing its playlist's tracks in order (repeats kept, missing ones left out) by
/// their path on their volume; the notes say which volume's `_Serato_` they go in.
#[hegel::test(test_cases = 200)]
fn crates_list_every_playlist(tc: TestCase) {
    let lib = common::library(&tc, 4);
    let (root, volume) = root(&tc);
    let src = Source { library: &lib, root, analysis: None };
    let export = serato::build::<Error>(&src, Path::new("out"), None, &Ctx::silent()).unwrap();
    let by_name = crates(&export);
    assert_eq!((export.files.len(), by_name.len()), (lib.playlists.len(), lib.playlists.len()));

    // One crate per node, parents before children, in display order.
    fn walk(lib: &Library, parent: Option<u32>, out: &mut Vec<u32>) {
        for p in lib.children(parent) {
            out.push(p.id);
            if p.is_folder {
                walk(lib, Some(p.id), out);
            }
        }
    }
    let mut order = Vec::new();
    walk(&lib, None, &mut order);
    let names =
        export.files.iter().map(|f| f.path.file_name().unwrap().to_str().unwrap().strip_suffix(".crate").unwrap());
    let named: Vec<(u32, &str)> = order.into_iter().zip(names).collect();
    let name_of: FxHashMap<u32, &str> = named.iter().copied().collect();
    let lower: FxHashSet<String> = named.iter().map(|(_, n)| n.to_lowercase()).collect();
    assert_eq!(lower.len(), named.len(), "unique ignoring case: {named:?}");
    // How many levels of crates a node holds.
    fn levels_below(lib: &Library, id: u32) -> usize {
        lib.children(Some(id)).iter().map(|c| 1 + levels_below(lib, c.id)).max().unwrap_or(0)
    }
    for (id, name) in &named {
        let p = lib.playlist(*id).unwrap();
        assert!(format!("{name}.crate").len() <= 255 && !name.contains(['/', '\\', ':']), "{name:?}");
        // Its name made safe, with no `%%` of its own, after its folder's whole crate name and
        // `%%`, cut on a character boundary, then told apart from a crate named the same.
        let mut own = paths::sanitize_component(&p.name);
        while own.contains("%%") {
            own = own.replace("%%", "%");
        }
        let prefix = p.parent_id.map_or(String::new(), |parent| format!("{}%%", name_of[&parent]));
        let rest = name.strip_prefix(prefix.as_str()).unwrap_or_else(|| panic!("{name:?} is under {prefix:?}"));
        let told_apart = |cut: &str| {
            let n = rest.strip_prefix(cut).and_then(|n| n.strip_prefix(" (")).and_then(|n| n.strip_suffix(')'));
            rest == cut || n.and_then(|n| n.parse::<u32>().ok()).is_some_and(|n| n >= 2)
        };
        let cut = (1..=own.len())
            .rev()
            .filter(|&i| own.is_char_boundary(i))
            .map(|i| &own[..i])
            .find(|cut| told_apart(cut))
            .unwrap_or_else(|| panic!("{name:?} is {prefix:?} and the start of {own:?}"));
        let base = prefix.len() + cut.len();
        assert!(base <= 200 || cut.chars().count() == 1, "{name:?} is {base} bytes");
        if let Some(next) = own[cut.len()..].chars().next() {
            let reserve = 10 * levels_below(&lib, p.id);
            assert!(base + next.len_utf8() + reserve > 200, "{name:?} is cut short of {own:?}");
        }
        let want: Vec<String> =
            lib.tracks_by_id(&p.track_ids).iter().map(|t| relative(&src.audio(t), volume)).collect();
        assert_eq!(by_name[*name], want, "{name}");
    }

    assert_eq!(export.tracks, lib.tracks.len());
    assert_eq!(export.notes.len(), 3);
    let home = if volume == "/" { "~/Music/_Serato_/Subcrates".to_string() } else { format!("on {volume}") };
    assert!(export.notes[0].contains(&home), "{}", export.notes[0]);
    assert!(export.warnings.is_empty());
    assert_eq!((export.cues, export.copy_bytes), (0, 0));
}

/// The kind of audio file a name says, when its copy can hold Serato's ID3v2 tag.
fn kind_of(path: &str) -> Option<Kind> {
    match path.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
        "mp3" => Some(Kind::Mp3),
        "wav" => Some(Kind::Wav),
        "aif" | "aiff" => Some(Kind::Aiff),
        _ => None,
    }
}

/// With copies, each file is copied once (rows sharing a file in any case are one copy), files
/// that aren't there get none, the crates list the copies, and with analysis the MP3, AIFF and WAV
/// copies carry their cues' `Serato Markers2`; other formats and cues left out are counted. An
/// error finding a track's audio comes back as it is, and a cancelled export stops.
#[hegel::test(test_cases = 60)]
fn copies_hold_each_file_once(tc: TestCase) {
    let lib = common::library(&tc, 5);
    let with_analysis = tc.draw(gs::booleans());
    let analysis = common::analysis(&tc, &lib);
    let load = common::loader(&analysis);
    let src = Source {
        library: &lib,
        root: PathBuf::from("/Volumes/USB"),
        analysis: if with_analysis { Some(&load) } else { None },
    };
    let music = tempfile::tempdir().unwrap();
    let file_key = |t: &Track| paths::key(&t.file_path).to_lowercase();
    let firsts = common::first_per_file(&lib, file_key);
    let mut sizes: FxHashMap<String, u64> = FxHashMap::default();
    for t in &firsts {
        if tc.draw(gs::booleans()) {
            let size = tc.draw(gs::integers::<usize>().min_value(2).max_value(3000));
            let path = paths::to_fs(music.path(), &paths::key(&t.file_path));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            // Audio of the kind its name says, which Serato's tag can go into.
            let bytes = match kind_of(&t.file_path) {
                Some(kind) => audio_file(kind, &vec![0x55; size / 2 * 2]),
                None => vec![0x55; size],
            };
            std::fs::write(&path, &bytes).unwrap();
            sizes.insert(file_key(t), bytes.len() as u64);
        }
    }
    let failing = tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(6)));
    let audio = |t: &Track| match failing {
        Some(id) if id == t.id => Err(Error::Invalid(format!("no audio for {id}"))),
        _ => Ok(paths::to_fs(music.path(), &paths::key(&t.file_path))),
    };
    let copies = Copies { root: PathBuf::from("/Volumes/USB/out"), audio: &audio };
    let result = serato::build(&src, Path::new("out"), Some(&copies), &Ctx::silent());
    let export = match (result, firsts.iter().any(|t| Some(t.id) == failing)) {
        (Err(Error::Invalid(e)), true) => return assert_eq!(e, format!("no audio for {}", failing.unwrap())),
        (Ok(export), false) => export,
        (Ok(_), true) => panic!("the audio error is lost"),
        (Err(e), _) => panic!("{e}"),
    };

    let copied: Vec<&Track> = firsts.iter().copied().filter(|t| sizes.contains_key(&file_key(t))).collect();
    let files: Vec<_> = export.files.iter().filter(|f| matches!(f.content, Content::TaggedCopy { .. })).collect();
    assert_eq!(files.len(), copied.len());
    let (mut with_cues, mut cues, mut untaggable, mut left_out) = (0, 0, 0, 0);
    for (t, f) in copied.iter().zip(&files) {
        let Content::TaggedCopy { from, markers } = &f.content else { unreachable!() };
        let key = paths::key(&t.file_path);
        assert_eq!(from, &paths::to_fs(music.path(), &key));
        assert_eq!(f.path, paths::to_fs(Path::new("/Volumes/USB/out"), &key));
        // A tagged copy is planned at its size and the tag's (see `copies_are_planned_as_written`).
        match markers {
            Some(_) => assert!(f.bytes > sizes[&file_key(t)], "{}", f.path.display()),
            None => assert_eq!(f.bytes, sizes[&file_key(t)]),
        }
        match (with_analysis, kind_of(&t.file_path).is_some()) {
            (true, true) => {
                let want = Markers::from_rekordbox(t.color, &analysis[&t.id].1);
                with_cues += usize::from(!want.cues.is_empty() || !want.loops.is_empty());
                cues += want.cues.len() + want.loops.len();
                left_out += want.left_out;
                assert_eq!(markers.as_deref(), Some(want.encode().as_slice()));
            }
            (true, false) => {
                untaggable += 1;
                assert_eq!(markers, &None);
            }
            (false, _) => assert_eq!(markers, &None),
        }
    }
    assert_eq!(export.copy_bytes, copied.iter().map(|t| sizes[&file_key(t)]).sum::<u64>());
    assert_eq!(export.cues, cues);
    let mut want = Vec::new();
    if untaggable > 0 {
        want.push(format!(
            "{untaggable} track(s) aren't MP3, AIFF or WAV; their copies get no cues (FLAC and MP4 tags aren't written)"
        ));
    }
    if left_out > 0 {
        want.push(format!(
            "{left_out} cue(s) left out: memory cues (Serato has none) and cues past the eighth pad or loop slot"
        ));
    }
    assert_eq!(export.warnings, want);
    assert!(export.notes.iter().any(|n| n.contains(&format!("; {with_cues} of them get"))), "{:?}", export.notes);
    // The crates list the copies, relative to the volume they're on.
    let listed: FxHashSet<Vec<String>> = crates(&export).into_values().collect();
    for p in lib.playlists.iter().filter(|p| !p.is_folder) {
        let want: Vec<String> = lib
            .tracks_by_id(&p.track_ids)
            .iter()
            .map(|t| relative(&paths::to_fs(&copies.root, &paths::key(&t.file_path)), "/Volumes/USB"))
            .collect();
        assert!(listed.contains(&want), "{want:?} in {listed:?}");
    }

    let cancel = CancelToken::new();
    cancel.cancel();
    let cancelled = serato::build(&src, Path::new("out"), Some(&copies), &Ctx { cancel, ..Ctx::silent() });
    assert_eq!(matches!(cancelled, Err(Error::Cancelled(_))), !lib.tracks.is_empty());
}

/// Copies of MP3, WAV and AIFF files with cue points get one warning counting, over every copy,
/// the points Serato can't hold: memory cues and hot cues past the eighth pad. Copies of other
/// formats get no cues, and a warning counting them.
#[hegel::test(test_cases = 60)]
fn copies_warn_of_the_cues_they_leave_out(tc: TestCase) {
    let mut lib = common::library(&tc, 4);
    let music = tempfile::tempdir().unwrap();
    let mut analysis = FxHashMap::default();
    let (mut left_out, mut untaggable) = (0, 0);
    for t in &mut lib.tracks {
        let ext = tc.draw(gs::sampled_from(vec!["mp3", "MP3", "wav", "aif", "aiff", "flac", "m4a"]));
        t.file_path = format!("/Music/{}.{ext}", t.id);
        std::fs::create_dir_all(music.path().join("Music")).unwrap();
        let audio = kind_of(&t.file_path).map_or(b"audio".to_vec(), |kind| audio_file(kind, b"audio!"));
        std::fs::write(paths::to_fs(music.path(), &paths::key(&t.file_path)), audio).unwrap();
        let pads = tc.draw(gs::vecs(gs::optional(gs::integers::<u8>().max_value(12))).max_size(10));
        if matches!(ext, "flac" | "m4a") {
            untaggable += 1;
        } else {
            left_out += pads.iter().filter(|p| p.is_none_or(|p| p >= 8)).count();
        }
        let cues = pads
            .into_iter()
            .enumerate()
            .map(|(i, hot_cue)| Cue {
                hot_cue,
                start_ms: 1000 * i as u32,
                end_ms: None,
                comment: String::new(),
                color: None,
                color_id: 0,
                loop_beats: None,
            })
            .collect();
        analysis.insert(t.id, (Vec::new(), cues));
    }
    let load = common::loader(&analysis);
    let src = Source { library: &lib, root: PathBuf::from("/Volumes/USB"), analysis: Some(&load) };
    let audio = |t: &Track| Ok::<_, Error>(paths::to_fs(music.path(), &paths::key(&t.file_path)));
    let copies = Copies { root: PathBuf::from("/Volumes/USB/out"), audio: &audio };
    let export = serato::build(&src, Path::new("out"), Some(&copies), &Ctx::silent()).unwrap();
    let mut want = Vec::new();
    if untaggable > 0 {
        want.push(format!(
            "{untaggable} track(s) aren't MP3, AIFF or WAV; their copies get no cues (FLAC and MP4 tags aren't written)"
        ));
    }
    if left_out > 0 {
        want.push(format!(
            "{left_out} cue(s) left out: memory cues (Serato has none) and cues past the eighth pad or loop slot"
        ));
    }
    assert_eq!(export.warnings, want);
    let copies: Vec<&Option<Vec<u8>>> = export
        .files
        .iter()
        .filter_map(|f| match &f.content {
            Content::TaggedCopy { markers, .. } => Some(markers),
            _ => None,
        })
        .collect();
    assert_eq!(copies.len(), lib.tracks.len());
    for (t, markers) in lib.tracks.iter().zip(copies) {
        let tagged = !t.file_path.ends_with(".flac") && !t.file_path.ends_with(".m4a");
        assert_eq!(markers.is_some(), tagged, "{}", t.file_path);
    }
}

/// The audio file kinds Serato's tag is written into.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Mp3,
    Wav,
    Aiff,
}

/// A small but valid file of `kind` holding `audio` (an even number of bytes).
fn audio_file(kind: Kind, audio: &[u8]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8], len: [u8; 4]) {
        out.extend_from_slice(id);
        out.extend_from_slice(&len);
        out.extend_from_slice(data);
    }
    match kind {
        // MPEG-1 layer III, 128 kbit/s at 44.1 kHz: 417-byte frames.
        Kind::Mp3 => audio
            .chunks(413)
            .flat_map(|c| [&[0xff, 0xfb, 0x90, 0x64][..], c, &vec![0; 413 - c.len()]].concat())
            .collect(),
        Kind::Wav => {
            let fmt = [
                &1u16.to_le_bytes()[..],
                &1u16.to_le_bytes(),
                &44_100u32.to_le_bytes(),
                &88_200u32.to_le_bytes(),
                &2u16.to_le_bytes(),
                &16u16.to_le_bytes(),
            ]
            .concat();
            let mut body = b"WAVE".to_vec();
            chunk(&mut body, b"fmt ", &fmt, (fmt.len() as u32).to_le_bytes());
            chunk(&mut body, b"data", audio, (audio.len() as u32).to_le_bytes());
            let mut out = Vec::new();
            chunk(&mut out, b"RIFF", &body, (body.len() as u32).to_le_bytes());
            out
        }
        Kind::Aiff => {
            let frames = (audio.len() / 2) as u32;
            // 44100 as an 80-bit extended float.
            let rate = [0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0];
            let comm = [&1u16.to_be_bytes()[..], &frames.to_be_bytes(), &16u16.to_be_bytes(), &rate].concat();
            let ssnd = [&[0; 8][..], audio].concat();
            let mut body = b"AIFF".to_vec();
            chunk(&mut body, b"COMM", &comm, (comm.len() as u32).to_be_bytes());
            chunk(&mut body, b"SSND", &ssnd, (ssnd.len() as u32).to_be_bytes());
            let mut out = Vec::new();
            chunk(&mut out, b"FORM", &body, (body.len() as u32).to_be_bytes());
            out
        }
    }
}

/// The ID3v2 tag of the file at `path`, read with lofty as `kind`.
fn id3v2(path: &Path, kind: Kind) -> Option<lofty::id3::v2::Id3v2Tag> {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    let mut f = std::fs::File::open(path).unwrap();
    let options = ParseOptions::new().read_properties(false);
    match kind {
        Kind::Mp3 => lofty::mpeg::MpegFile::read_from(&mut f, options).unwrap().id3v2().cloned(),
        Kind::Wav => lofty::iff::wav::WavFile::read_from(&mut f, options).unwrap().id3v2().cloned(),
        Kind::Aiff => lofty::iff::aiff::AiffFile::read_from(&mut f, options).unwrap().id3v2().cloned(),
    }
}

/// A tag's GEOB frames: (description, MIME type, data).
fn geobs(tag: &lofty::id3::v2::Id3v2Tag) -> Vec<(String, Option<String>, Vec<u8>)> {
    use lofty::id3::v2::{Frame, FrameFlags, GeneralEncapsulatedObject};
    tag.into_iter()
        .filter_map(|f| match f {
            Frame::Binary(b) if b.id().as_str() == "GEOB" => {
                let g = GeneralEncapsulatedObject::parse(&b.data, FrameFlags::default()).unwrap();
                Some((g.descriptor.unwrap_or_default(), g.mime_type, g.data))
            }
            _ => None,
        })
        .collect()
}

fn geob(descriptor: &str, data: &[u8]) -> lofty::id3::v2::Frame<'static> {
    use lofty::TextEncoding;
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{BinaryFrame, Frame, FrameId, GeneralEncapsulatedObject};
    let object = GeneralEncapsulatedObject::new(
        TextEncoding::Latin1,
        Some("application/octet-stream".into()),
        Some(String::new()),
        Some(descriptor.into()),
        data.to_vec(),
    );
    Frame::Binary(BinaryFrame::new(FrameId::Valid("GEOB".into()), object.as_bytes(WriteOptions::new()).unwrap()))
}

/// Written into an MP3, AIFF or WAV copy (tagged before or not, ID3v2.3 or 2.4), the frame reads
/// back with lofty as the one `Serato Markers2` GEOB, holding the last markers written; an older
/// `Serato Markers_` is gone, the other frames stay, the tag keeps its version and the audio is
/// untouched.
#[hegel::test(test_cases = 40)]
fn markers_written_into_copies_read_back(tc: TestCase) {
    use lofty::config::WriteOptions;
    use lofty::id3::v2::{Id3v2Tag, Id3v2Version};
    use lofty::tag::{Accessor, TagExt};

    let kind = tc.draw(gs::sampled_from(vec![Kind::Mp3, Kind::Wav, Kind::Aiff]).print_as_debug());
    let ext = tc.draw(gs::sampled_from(match kind {
        Kind::Mp3 => vec!["mp3", "MP3"],
        Kind::Wav => vec!["wav", "WAV"],
        Kind::Aiff => vec!["aiff", "aif", "AIF"],
    }));
    // Bytes no frame header or chunk id starts with, to find the audio again.
    let audio: Vec<u8> = tc
        .draw(gs::vecs(gs::integers::<u8>().min_value(1).max_value(0xfe)).min_size(2).max_size(1200))
        .into_iter()
        .collect();
    let audio = &audio[..audio.len() / 2 * 2];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copy");
    std::fs::write(&path, audio_file(kind, audio)).unwrap();

    let before = tc.draw(gs::optional(gs::booleans()));
    if let Some(v23) = before {
        let mut tag = Id3v2Tag::new();
        tag.set_title("Kept".into());
        tag.insert(geob("Serato Markers_", b"old"));
        tag.insert(geob("Serato Markers2", b"stale"));
        tag.insert(geob("Serato Analysis", b"\x02\x01"));
        let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        tag.save_to(&mut f, WriteOptions::new().use_id3v23(v23)).unwrap();
    }
    let writes = tc.draw(gs::vecs(gs::binary().max_size(700)).min_size(1).max_size(2));
    for markers in &writes {
        serato::write_markers(&path, Path::new(&format!("x.{ext}")), markers).unwrap();
    }

    let tag = id3v2(&path, kind).expect("an ID3v2 tag");
    let mut want = vec![(
        "Serato Markers2".to_string(),
        Some("application/octet-stream".to_string()),
        writes.last().unwrap().clone(),
    )];
    if before.is_some() {
        want.insert(
            0,
            ("Serato Analysis".to_string(), Some("application/octet-stream".to_string()), b"\x02\x01".to_vec()),
        );
        assert_eq!(tag.title().as_deref(), Some("Kept"));
    }
    let mut got = geobs(&tag);
    got.sort();
    want.sort();
    assert_eq!(got, want);
    let version = if before == Some(true) { Id3v2Version::V3 } else { Id3v2Version::V4 };
    assert_eq!(tag.original_version(), version);
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.windows(audio.len()).any(|w| w == audio), "the audio is still there, in one piece");
}

/// Serato's tag can't go into a file that isn't there, a FLAC or MP4 copy, or bytes that aren't
/// the MP3, AIFF or WAV its name says: each is a typed error and the file is left as it was. A
/// file cut off after its header is either refused the same way or tagged so that it reads back.
#[hegel::test(test_cases = 60)]
fn markers_refuse_files_they_cant_tag(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copy");
    let named = tc.draw(gs::sampled_from(vec!["x.mp3", "x.wav", "x.aiff", "x.flac", "x.m4a", "x"]));
    let kind = match named {
        "x.mp3" => Some(Kind::Mp3),
        "x.wav" => Some(Kind::Wav),
        "x.aiff" => Some(Kind::Aiff),
        _ => None,
    };
    let content = tc.draw(gs::optional(hegel::one_of!(
        // Bytes with no frame sync or chunk id in them.
        gs::vecs(gs::integers::<u8>().max_value(0x40)).max_size(300).map(|b| (b, false)),
        // Not a WAV whose RIFF size says 0: lofty 0.25 writes its tag chunk before `WAVE`, and
        // nothing can read the copy.
        gs::sampled_from(vec![
            b"RIFF\x04\0\0\0WAVE".to_vec(),
            b"FORM\0\0\0\x04AIFF".to_vec(),
            b"ID3\x04\0\0\0\0\0\0".to_vec(),
        ])
        .map(|b| (b, true)),
    )));
    if let Some((bytes, _)) = &content {
        std::fs::write(&path, bytes).unwrap();
    }
    match (serato::write_markers(&path, Path::new(named), b"markers"), &content) {
        (Err(Error::Io { path: p, .. }), None) => assert_eq!(p, path),
        (Err(Error::Tags(e)), Some((bytes, _))) => {
            // Named as the file it's a copy of: `path` is a temporary file's.
            assert!(e.starts_with(&format!("{named}: ")), "{e}");
            assert_eq!(&std::fs::read(&path).unwrap(), bytes, "left as it was");
        }
        (Ok(()), Some((_, true))) if kind.is_some() => {
            let tag = id3v2(&path, kind.unwrap()).expect("an ID3v2 tag");
            assert_eq!(
                geobs(&tag),
                [("Serato Markers2".into(), Some("application/octet-stream".into()), b"markers".to_vec())]
            );
        }
        (result, _) => panic!("{named} holding {content:?}: {result:?}"),
    }
}

/// Regression (`--write-tags`): copies are planned as they're written. A tagged copy was planned
/// at its source's size, short by the tag, so a check for room passed copies that then didn't fit;
/// the note counted every tagged copy as getting hot cues, cues or none; and audio that isn't what
/// its name says was planned with a tag that then failed to write, naming a temporary file. Now
/// each copy, once tagged, takes no more than it's planned at (MP3, AIFF and WAV, tagged before or
/// not), the note counts the copies whose tag carries a cue or loop, and a file that can't be
/// tagged is copied as it is, with a warning naming it.
#[hegel::test(test_cases = 60)]
fn copies_are_planned_as_written(tc: TestCase) {
    use lofty::config::WriteOptions;
    use lofty::id3::v2::Id3v2Tag;
    use lofty::tag::{Accessor, TagExt};

    let mut lib = common::library(&tc, 4);
    let music = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(music.path().join("Music")).unwrap();
    let mut real = FxHashSet::default();
    for t in &mut lib.tracks {
        let kind = tc.draw(gs::sampled_from(vec![Kind::Mp3, Kind::Wav, Kind::Aiff]).print_as_debug());
        let ext = match kind {
            Kind::Mp3 => "mp3",
            Kind::Wav => "wav",
            Kind::Aiff => "aiff",
        };
        t.file_path = format!("/Music/{}.{ext}", t.id);
        let path = paths::to_fs(music.path(), &paths::key(&t.file_path));
        if !tc.draw(gs::booleans()) {
            // Bytes with no frame sync or chunk id in them.
            std::fs::write(&path, tc.draw(gs::vecs(gs::integers::<u8>().max_value(0x40)).max_size(300))).unwrap();
            continue;
        }
        let audio = tc.draw(gs::vecs(gs::integers::<u8>().min_value(1).max_value(0xfe)).min_size(2).max_size(1200));
        std::fs::write(&path, audio_file(kind, &audio[..audio.len() / 2 * 2])).unwrap();
        if let Some((title, v23, footer)) = tc.draw(gs::optional(hegel::tuples!(
            gs::integers::<usize>().max_value(3000),
            gs::booleans(),
            gs::booleans()
        ))) {
            let mut tag = Id3v2Tag::new();
            tag.set_title("t".repeat(title));
            let mut flags = *tag.flags();
            flags.footer = footer;
            tag.set_flags(flags);
            let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
            tag.save_to(&mut f, WriteOptions::new().use_id3v23(v23)).unwrap();
        }
        real.insert(t.id);
    }
    let analysis = common::analysis(&tc, &lib);
    let load = common::loader(&analysis);
    let src = Source { library: &lib, root: PathBuf::from("/Volumes/USB"), analysis: Some(&load) };
    let audio = |t: &Track| Ok::<_, Error>(paths::to_fs(music.path(), &paths::key(&t.file_path)));
    let copies = Copies { root: PathBuf::from("/Volumes/USB/out"), audio: &audio };
    let export = serato::build(&src, Path::new("out"), Some(&copies), &Ctx::silent()).unwrap();

    let staged = tempfile::tempdir().unwrap();
    let (mut cues, mut with_cues, mut refused) = (0, 0, Vec::new());
    let files = export.files.iter().filter(|f| matches!(f.content, Content::TaggedCopy { .. }));
    for (t, f) in lib.tracks.iter().zip(files) {
        let Content::TaggedCopy { from, markers } = &f.content else { unreachable!() };
        // As the export writes it: a copy, then the tag.
        let copy = staged.path().join(t.id.to_string());
        std::fs::copy(from, &copy).unwrap();
        match markers {
            Some(m) => {
                assert!(real.contains(&t.id), "{} isn't audio, but gets a tag", from.display());
                serato::write_markers(&copy, from, m).unwrap();
                let want = Markers::from_rekordbox(t.color, &analysis[&t.id].1);
                cues += want.cues.len() + want.loops.len();
                with_cues += usize::from(!want.cues.is_empty() || !want.loops.is_empty());
            }
            None => {
                assert!(!real.contains(&t.id), "{} is audio, but gets no tag", from.display());
                refused.push(from);
            }
        }
        let len = std::fs::metadata(&copy).unwrap().len();
        assert!(len <= f.bytes, "{} takes {len} bytes, planned at {}", from.display(), f.bytes);
    }
    assert_eq!(export.cues, cues);
    assert!(export.notes.iter().any(|n| n.contains(&format!("; {with_cues} of them get"))), "{:?}", export.notes);
    let warned = export.warnings.iter().find(|w| w.contains("can't take Serato's cue tag"));
    match refused.first() {
        Some(first) => assert_eq!(
            warned.map(|w| w.starts_with(&format!("{} track(s)", refused.len()))
                && w.contains(&format!("(e.g. {}: ", first.display()))),
            Some(true),
            "{:?}",
            export.warnings
        ),
        None => assert_eq!(warned, None),
    }
}
