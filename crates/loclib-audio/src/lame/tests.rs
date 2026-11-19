use super::*;

/// The head of an MP3: an MPEG 1 layer III stereo frame header at 44.1 kHz (128 kbit/s), its side
/// information, a tag with `id` and every field, and `encoder` where LAME's tag starts.
fn head(id: &[u8; 4], encoder: &[u8; 4]) -> Vec<u8> {
    let mut b = vec![0xff, 0xfb, 0x90, 0x00];
    b.extend([0; 32]);
    b.extend(id);
    b.extend(15u32.to_be_bytes());
    b.extend([0; 4 + 4 + 100 + 4]);
    b.extend(encoder);
    b.extend(b"3.100");
    b.resize(417, 0);
    b
}

#[test]
fn a_lame_info_frame_is_a_frame_long() {
    let secs = lame_frame_in(&head(b"Info", b"LAME")).unwrap();
    assert!((secs - 1152.0 / 44_100.0).abs() < 1e-9);
    assert!(lame_frame_in(&head(b"Xing", b"LAME")).is_some());
}

#[test]
fn other_encoders_tags_dont_count() {
    for encoder in [b"Lavc", b"Lavf", b"Lame"] {
        assert_eq!(lame_frame_in(&head(b"Info", encoder)), None);
    }
}

#[test]
fn audio_without_a_tag_has_none() {
    assert_eq!(lame_frame_in(&head(b"\0\0\0\0", b"LAME")), None);
    assert_eq!(lame_frame_in(b"RIFF....WAVEfmt "), None);
    assert_eq!(lame_frame_in(&[]), None);
}

#[test]
fn the_tag_is_found_where_fewer_fields_are() {
    // Only the frame count: LAME's tag is 4 bytes after the flags.
    let mut b = vec![0xff, 0xfb, 0x90, 0x00];
    b.extend([0; 32]);
    b.extend(b"Info");
    b.extend(1u32.to_be_bytes());
    b.extend([0; 4]);
    b.extend(b"LAME");
    b.resize(417, 0);
    assert!(lame_frame_in(&b).is_some());
}

#[test]
fn mpeg2_frames_are_half_as_long() {
    // MPEG 2, 22.05 kHz, mono: 9 bytes of side information.
    let mut b = vec![0xff, 0xf3, 0x40, 0xc0];
    b.extend([0; 9]);
    b.extend(b"Info");
    b.extend(0u32.to_be_bytes());
    b.extend(b"LAME");
    b.resize(300, 0);
    let secs = lame_frame_in(&b).unwrap();
    assert!((secs - 576.0 / 22_050.0).abs() < 1e-9);
}

#[test]
fn a_file_with_an_id3_tag_is_read_past_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.mp3");
    let mut bytes = b"ID3\x04\x00\x00\x00\x00\x01\x00".to_vec();
    bytes.extend([0; 128]);
    bytes.extend(head(b"Info", b"LAME"));
    std::fs::write(&path, bytes).unwrap();
    assert!(lame_frame_secs(&path).is_some());
}
