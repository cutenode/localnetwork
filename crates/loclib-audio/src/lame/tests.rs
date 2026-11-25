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

#[test]
fn a_file_without_an_id3_tag_is_read_from_its_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.mp3");
    std::fs::write(&path, head(b"Info", b"LAME")).unwrap();
    assert!(lame_frame_secs(&path).is_some());
}

#[test]
fn an_id3_tag_with_a_footer_is_read_past_it_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.mp3");
    // Flags 0x10: a 10-byte footer follows the tag's 128 bytes.
    let mut bytes = b"ID3\x04\x00\x10\x00\x00\x01\x00".to_vec();
    bytes.extend([0; 128 + 10]);
    bytes.extend(head(b"Info", b"LAME"));
    std::fs::write(&path, bytes).unwrap();
    assert!(lame_frame_secs(&path).is_some());
}

#[test]
fn a_file_that_cant_be_read_or_is_too_short_has_none() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(lame_frame_secs(&dir.path().join("missing.mp3")), None);
    let short = dir.path().join("short.mp3");
    std::fs::write(&short, b"ID3").unwrap();
    assert_eq!(lame_frame_secs(&short), None);
}

#[test]
fn an_id3_tag_too_big_to_skip_leaves_the_file_be() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.mp3");
    // A syncsafe size of 0x7f7f7f7f: 256 MiB, past what's skipped.
    let mut bytes = b"ID3\x04\x00\x00\x7f\x7f\x7f\x7f".to_vec();
    bytes.extend(head(b"Info", b"LAME"));
    std::fs::write(&path, bytes).unwrap();
    assert_eq!(lame_frame_secs(&path), None);
}

#[test]
fn every_version_and_channel_mode_has_its_frame_length() {
    // (byte 1, byte 3, samples, rate, side information): MPEG 1 mono, MPEG 2 stereo, MPEG 2.5.
    for (b1, b3, samples, rate, side) in
        [(0xfb, 0xc0, 1152, 44_100, 17), (0xf3, 0x00, 576, 22_050, 17), (0xe3, 0xc0, 576, 11_025, 9)]
    {
        assert_eq!(header(&[0xff, b1, 0x90, b3]), Some((samples, rate, side)));
    }
}

#[test]
fn what_isnt_a_layer_iii_header_isnt_one() {
    for b in [
        [0xfe, 0xfb, 0x90, 0x00], // no sync
        [0xff, 0x1b, 0x90, 0x00], // half a sync
        [0xff, 0xeb, 0x90, 0x00], // the reserved version
        [0xff, 0xfd, 0x90, 0x00], // layer II
        [0xff, 0xfb, 0x9c, 0x00], // the reserved sample rate
        [0xff, 0xfb, 0x00, 0x00], // free bitrate
        [0xff, 0xfb, 0xf0, 0x00], // the bad bitrate
    ] {
        assert_eq!(header(&b), None, "{b:02x?}");
    }
    assert_eq!(header(&[0xff, 0xfb]), None);
}

#[test]
fn a_tag_cut_short_has_none() {
    let whole = head(b"Info", b"LAME");
    // Cut inside the flags, inside the fields, and just before LAME's tag.
    for len in [4 + 32 + 6, 4 + 32 + 8 + 50, 4 + 32 + 8 + 112 + 2] {
        assert_eq!(lame_frame_in(&whole[..len]), None, "{len}");
    }
}
