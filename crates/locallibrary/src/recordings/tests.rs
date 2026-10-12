use super::*;
use std::io::Cursor;

/// A WAV header for `data_len` bytes of 44.1 kHz 16-bit stereo, with an extra chunk first.
pub(crate) fn wav(data_len: u32, declared: u32) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + 12 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"JUNK");
    v.extend_from_slice(&4u32.to_le_bytes());
    v.extend_from_slice(&[0; 4]);
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&44_100u32.to_le_bytes());
    v.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&declared.to_le_bytes());
    v.extend(std::iter::repeat_n(0u8, data_len as usize));
    v
}

#[test]
fn reads_wav_lengths() {
    let one_sec = 44_100 * 4;
    let bytes = wav(one_sec, one_sec);
    let (f, data) = read_wav_header(Cursor::new(&bytes), bytes.len() as u64).unwrap();
    assert_eq!(f, WavFormat { channels: 2, sample_rate: 44_100, bits: 16 });
    assert_eq!(data, u64::from(one_sec));
    // Header never finalised: the file length counts.
    let cut = wav(one_sec * 2, 0);
    assert_eq!(read_wav_header(Cursor::new(&cut), cut.len() as u64).unwrap().1, u64::from(one_sec * 2));
    assert!(read_wav_header(Cursor::new(b"not a wav file"), 14).is_none());
}

/// A header that goes on and on without its data is no header.
#[test]
fn a_header_without_data_is_no_header() {
    let mut endless = b"RIFF\0\0\0\0WAVE".to_vec();
    for _ in 0..64 {
        endless.extend(b"JUNK\0\0\0\0");
    }
    endless.extend(b"data\0\0\0\0");
    assert!(read_wav_header(Cursor::new(&endless), endless.len() as u64).is_none());
}

/// A header saying there are no channels gives no length rather than an endless one.
#[test]
fn a_recording_of_no_channels_has_no_length() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = wav(400, 400);
    // The channel count, in the fmt chunk after the 12-byte JUNK chunk.
    bytes[12 + 12 + 10..12 + 12 + 12].copy_from_slice(&0u16.to_le_bytes());
    let path = dir.path().join("REC001.WAV");
    std::fs::write(&path, bytes).unwrap();
    let r = recording(dir.path(), &path).unwrap();
    assert_eq!((r.format.map(|f| f.channels), r.duration_secs, r.started), (Some(0), None, None));
}

#[test]
fn lists_and_collects_recordings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("AZ");
    std::fs::create_dir_all(root.join("ALPHATHETA REC")).unwrap();
    std::fs::write(root.join("ALPHATHETA REC/REC001.WAV"), wav(44_100 * 4 * 3, 44_100 * 4 * 3)).unwrap();
    std::fs::write(root.join("ALPHATHETA REC/._REC001.WAV"), b"\0\x05\x16\x07").unwrap();
    std::fs::create_dir_all(root.join("Music")).unwrap();
    std::fs::write(root.join("Music/song.wav"), wav(4, 4)).unwrap();
    let mut d = Device::open(&root).unwrap();
    let recs = list(&d).unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].path, "ALPHATHETA REC/REC001.WAV");
    assert!((recs[0].duration_secs.unwrap() - 3.0).abs() < 1e-9);

    let out = dir.path().join("Sets");
    let opts = CollectOptions { to: out.clone(), only: vec![], tracklist: Tracklist::None, delete: true };
    let r = collect(&mut d, &opts, &Ctx::silent()).unwrap();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert_eq!(r.collected.len(), 1);
    assert!(r.collected[0].copied_to.starts_with(out.join("AZ")));
    assert!(r.collected[0].copied_to.exists());
    assert!(!root.join("ALPHATHETA REC/REC001.WAV").exists());
}
