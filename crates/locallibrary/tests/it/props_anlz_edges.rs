//! Analysis files as locallibrary reads them for a track on a drive: one whose only waveforms are
//! the 3-band ones in its `.2EX`, and hot cues of any number listed. The format's own edges are
//! loclib-anlz's tests.

use crate::common;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::anlz::{self, CueFile, CuePoint, WaveformStyle};
use locallibrary::device::Device;
use locallibrary::paths;

/// Each top-level section's tag and bytes, in order.
fn sections(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let be = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let (mut pos, end) = (be(4), be(8));
    let mut out = Vec::new();
    while pos + 12 <= end {
        let total = be(pos + 8);
        out.push((bytes[pos..pos + 4].try_into().unwrap(), bytes[pos..pos + total].to_vec()));
        pos += total;
    }
    out
}

/// `bytes` with `extra` (whole sections) added after its last section, its length fixed.
fn appended(bytes: &[u8], extra: &[Vec<u8>]) -> Vec<u8> {
    let mut out = [bytes.to_vec(), extra.concat()].concat();
    let len = out.len() as u32;
    out[8..12].copy_from_slice(&len.to_be_bytes());
    out
}

/// `bytes` without its sections tagged one of `tags`, its length fixed.
fn without(bytes: &[u8], tags: &[&[u8; 4]]) -> Vec<u8> {
    let header = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let kept: Vec<Vec<u8>> = sections(bytes).into_iter().filter(|(t, _)| !tags.contains(&t)).map(|(_, s)| s).collect();
    appended(&bytes[..header], &kept)
}

/// A track whose `.DAT` and `.EXT` lack an overview or detail waveform shows the 3-band ones of
/// its `.2EX` in their place; the ones it has come first (colour, then blue).
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_track_without_other_waveforms_shows_its_3_band_ones(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let analysed = |t: &&locallibrary::model::Track| {
        paths::to_fs(&root, &paths::key(&t.analyze_path)).with_extension("2EX").is_file()
    };
    let t = lib.tracks.iter().find(analysed).unwrap().clone();
    let dat = paths::to_fs(&root, &paths::key(&t.analyze_path));
    let (no_blue, no_colour, no_detail) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let strip = |path: &std::path::Path, tags: &[&[u8; 4]]| {
        std::fs::write(path, without(&std::fs::read(path).unwrap(), tags)).unwrap();
    };
    if no_blue {
        strip(&dat, &[b"PWAV"]);
    }
    let ext = dat.with_extension("EXT");
    let mut gone: Vec<&[u8; 4]> = Vec::new();
    if no_colour {
        gone.push(b"PWV4");
    }
    if no_detail {
        gone.extend([b"PWV3", b"PWV5"]);
    }
    strip(&ext, &gone);
    let a = anlz::load(&d, &t);
    let preview = match (no_colour, no_blue) {
        (false, _) => WaveformStyle::Rgb,
        (true, false) => WaveformStyle::Blue,
        (true, true) => WaveformStyle::ThreeBand,
    };
    assert_eq!(a.preview.map(|w| w.style), Some(preview));
    let detail = if no_detail { WaveformStyle::ThreeBand } else { WaveformStyle::Rgb };
    assert_eq!(a.detail.map(|w| w.style), Some(detail));
}

/// Listing any hot cue the files can hold (numbers 1-256) gives it a label. `HotCue::letter`
/// (also used for the mix export's labels) computed `b'A' + slot`, which overflowed from slot
/// 191: a panic in debug builds, a NUL in release.
#[hegel::test]
#[hegel::explicit_test_case(number = 200u32)]
fn every_hot_cue_read_can_be_listed(tc: TestCase) {
    let number = tc.draw(gs::integers::<u32>().min_value(1).max_value(256));
    let base = common::synthetic_anlz("/Contents/a.mp3");
    let one = [CuePoint { hot_cue: Some(0), start_ms: 500, ..Default::default() }];
    let mut f = anlz::write_cue_points(&base, CueFile::DatAlone, &one).unwrap();
    let at = f.windows(4).position(|w| w == b"PCPT").unwrap();
    f[at + 12..at + 16].copy_from_slice(&number.to_be_bytes());
    for c in anlz::read_cue_points(&f, None).unwrap() {
        let letter = locallibrary::cue_edit::HotCue(c.hot_cue.unwrap()).letter();
        assert_ne!(letter, '\0', "hot cue {number}");
    }
}
