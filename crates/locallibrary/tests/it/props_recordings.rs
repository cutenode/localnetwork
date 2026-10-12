//! Set recordings made on the player: WAV headers of any shape read back their format and
//! length, a drive's recordings are exactly the WAV files in its REC folders, and collecting
//! copies each byte for byte (with the tracklist asked for) before deleting it from the drive.

use crate::common::{self, media};

use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::recordings::{self, CollectOptions, Tracklist, WavFormat};

/// A WAV file: the format chunk, chunks players add around it (odd lengths padded), and a data
/// chunk whose size the recorder may never have filled in.
#[hegel::test(test_cases = 300)]
fn wav_headers_read_back_their_format_and_length(tc: TestCase) {
    let channels = tc.draw(gs::integers::<u16>().min_value(1).max_value(2));
    let rate = tc.draw(gs::sampled_from(vec![44_100u32, 48_000, 96_000]));
    let bits = tc.draw(gs::sampled_from(vec![16u16, 24]));
    let frames = tc.draw(gs::integers::<u32>().max_value(2000));
    let data = frames * u32::from(channels) * u32::from(bits / 8);
    let declared = tc.draw(gs::sampled_from(vec![Some(data), Some(0), Some(u32::MAX), Some(data + 100), None]));
    let mut w = b"RIFF\0\0\0\0WAVE".to_vec();
    let junk = |w: &mut Vec<u8>, len: u32| {
        w.extend(b"LIST");
        w.extend(len.to_le_bytes());
        w.extend(vec![1u8; (len + (len & 1)) as usize]);
    };
    let before = tc.draw(gs::vecs(gs::integers::<u32>().max_value(9)).max_size(3));
    for len in &before {
        junk(&mut w, *len);
    }
    w.extend(b"fmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(channels.to_le_bytes());
    w.extend(rate.to_le_bytes());
    w.extend((rate * u32::from(channels) * u32::from(bits / 8)).to_le_bytes());
    w.extend((channels * bits / 8).to_le_bytes());
    w.extend(bits.to_le_bytes());
    if tc.draw(gs::booleans()) {
        junk(&mut w, tc.draw(gs::integers::<u32>().max_value(9)));
    }
    w.extend(b"data");
    w.extend(declared.unwrap_or(data).to_le_bytes());
    w.extend(vec![0u8; data as usize]);
    let header = recordings::read_wav_header(Cursor::new(&w), w.len() as u64);
    let (format, len) = header.expect("a WAV file");
    assert_eq!(format, WavFormat { channels, sample_rate: rate, bits });
    // A size never filled in, or past the end of the file, gives what the file holds.
    assert_eq!(len, u64::from(data));

    // The test media's own WAVs read the same way.
    let secs = tc.draw(gs::sampled_from(vec![0.0, 0.1, 0.5]));
    let made = media::wav_of(rate, bits, channels, secs);
    let (f, len) = recordings::read_wav_header(Cursor::new(&made), made.len() as u64).unwrap();
    assert_eq!((f.channels, f.sample_rate, f.bits), (channels, rate, bits));
    assert_eq!(len, made.len() as u64 - 44);
    // Cut anywhere in its header, it's no WAV file.
    let cut = tc.draw(gs::integers::<usize>().max_value(35));
    assert!(recordings::read_wav_header(Cursor::new(&w[..cut]), cut as u64).is_none());
}

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn recordings_are_collected_byte_for_byte(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "AZ");
    common::add_history(&root, "HISTORY 001", &[1, 2]);
    common::add_history(&root, "HISTORY 002", &[3]);
    // Recordings in either folder, a minute apart, and WAV files elsewhere that aren't.
    let mut want = Vec::new();
    for n in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let folder = tc.draw(gs::sampled_from(vec!["PIONEER REC", "ALPHATHETA REC", "ALPHATHETA REC/2026"]));
        let key = format!("{folder}/REC{n:03}.{}", tc.draw(gs::sampled_from(vec!["WAV", "wav"])));
        let p = root.join(&key);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, media::wav_of(44_100, 16, 2, 0.05 * (n + 1) as f64)).unwrap();
        filetime::set_file_mtime(&p, filetime::FileTime::from_unix_time(1_790_000_000 + 60 * n as i64, 0)).unwrap();
        want.push(key);
    }
    for key in ["Contents/song.wav", "PIONEER REC/._REC000.WAV", "PIONEER REC/notes.txt"] {
        let p = root.join(key);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, media::wav()).unwrap();
    }
    let mut d = Device::open_export(&root).unwrap();
    let listed = recordings::list(&d).unwrap();
    assert_eq!(listed.iter().map(|r| r.path.clone()).collect::<Vec<_>>(), want, "oldest first");
    for (n, r) in listed.iter().enumerate() {
        let secs = 0.05 * (n + 1) as f64;
        assert!((r.duration_secs.unwrap() - secs).abs() < 1e-3, "{r:?}");
        assert!(r.started.unwrap() < r.modified, "it started before it was last written");
    }

    let only: Vec<String> = want.iter().filter(|_| tc.draw(gs::booleans())).cloned().collect();
    let tracklist = tc.draw(gs::sampled_from(vec![0u8, 1, 2]));
    let tracklist = match tracklist {
        0 => Tracklist::None,
        1 => Tracklist::Latest,
        _ => Tracklist::Named("HISTORY 001".into()),
    };
    let delete = tc.draw(gs::booleans());
    let originals: Vec<(String, Vec<u8>)> = want.iter().map(|k| (k.clone(), fs::read(root.join(k)).unwrap())).collect();
    let out = dir.path().join("Sets");
    let opts = CollectOptions { to: out.clone(), only: only.clone(), tracklist: tracklist.clone(), delete };
    let r = recordings::collect(&mut d, &opts, &Ctx::silent()).unwrap();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let chosen: Vec<&String> = if only.is_empty() { want.iter().collect() } else { only.iter().collect() };
    assert_eq!(r.collected.iter().map(|c| &c.recording).collect::<BTreeSet<_>>(), chosen.iter().copied().collect());
    let lib = d.library().unwrap();
    let newest = chosen.iter().max_by_key(|k| want.iter().position(|w| w == **k)).copied();
    for c in &r.collected {
        assert!(c.copied_to.starts_with(out.join("AZ")));
        let original = &originals.iter().find(|(k, _)| *k == c.recording).unwrap().1;
        assert_eq!(&fs::read(&c.copied_to).unwrap(), original);
        assert_eq!(c.deleted, delete);
        assert_eq!(root.join(&c.recording).exists(), !delete);
        let history = match &tracklist {
            Tracklist::None => None,
            Tracklist::Latest => {
                (Some(&c.recording) == newest).then(|| lib.history.iter().max_by_key(|h| h.id).unwrap())
            }
            Tracklist::Named(_) => lib.history.iter().find(|h| h.name == "HISTORY 001"),
        };
        match (history, &c.tracklist) {
            (Some(h), Some(t)) => assert_eq!(fs::read_to_string(t).unwrap(), recordings::tracklist_text(&lib, h)),
            (None, None) => {}
            other => panic!("tracklist {other:?}"),
        }
    }
    // What wasn't chosen stays, and the library is untouched.
    for k in want.iter().filter(|k| !chosen.contains(k)) {
        assert!(root.join(k).exists());
    }
    assert!(paths::to_fs(&root, "Contents/song.wav").exists());
    // Collected again, each copy is there already: it's refused, file by file.
    if !delete && !r.collected.is_empty() {
        let again = recordings::collect(&mut d, &opts, &Ctx::silent()).unwrap();
        assert_eq!(again.failed.len(), r.collected.len());
    }
}
