use hegel::TestCase;
use hegel::generators as gs;
use loclib_progress::Ctx;
use symphonia::core::io::{MediaSourceStream, ReadBytes};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Instantiate;

use super::*;
use crate::tests::flac;
use crate::{decode, open, vet_container};

/// A FLAC of 16 samples, 1 to 16, with `blocks` (each a type and a body) after its stream info,
/// the last of them marked as the last.
fn flac_with(blocks: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut file = flac(&[(1..=16).collect()]);
    let mut metadata = vec![];
    for (n, (kind, body)) in blocks.iter().enumerate() {
        let last = if n == blocks.len() - 1 { 0x80 } else { 0 };
        metadata.push(last | kind);
        metadata.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        metadata.extend_from_slice(body);
    }
    if !blocks.is_empty() {
        // The stream info is no longer the last block.
        file[4] &= 0x7f;
    }
    file.splice(42..42, metadata);
    file
}

/// A picture's body: the front cover's type, then a media type, description and data, each after
/// its length, the data after a 1x1 picture's width, height, colour depth and palette size.
fn picture_body(media: &[u8], description: &[u8], data: &[u8]) -> Vec<u8> {
    let mut body = 3u32.to_be_bytes().to_vec();
    for (field, before) in
        [(media, &[][..]), (description, &[]), (data, &[0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 24, 0, 0, 0, 0])]
    {
        body.extend_from_slice(before);
        body.extend_from_slice(&(field.len() as u32).to_be_bytes());
        body.extend_from_slice(field);
    }
    body
}

/// Vorbis comments' body: the vendor's name and each comment after its length, the comments after
/// their count.
fn comments_body(vendor: &[u8], comments: &[Vec<u8>]) -> Vec<u8> {
    let mut body = [&(vendor.len() as u32).to_le_bytes()[..], vendor, &(comments.len() as u32).to_le_bytes()].concat();
    for comment in comments {
        body.extend_from_slice(&(comment.len() as u32).to_le_bytes());
        body.extend_from_slice(comment);
    }
    body
}

/// `bytes` in base64, padded.
fn encode(bytes: &[u8]) -> String {
    const SYMBOLS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for group in bytes.chunks(3) {
        let bits = group.iter().fold(0u32, |n, &b| n << 8 | u32::from(b)) << (8 * (3 - group.len()));
        for k in 0..4 {
            out.push(if k <= group.len() { char::from(SYMBOLS[(bits >> (18 - 6 * k) & 63) as usize]) } else { '=' });
        }
    }
    out
}

/// A comment holding `picture`, under the key in any case.
fn picture_comment(key: &str, picture: &[u8]) -> Vec<u8> {
    format!("{key}={}", encode(picture)).into_bytes()
}

/// Where a picture or Vorbis comments claim a length, and what that claim reads as.
const CLAIMS: [&str; 7] =
    ["media type", "description", "picture data", "vendor", "comment", "second comment", "picture in a comment"];

/// A FLAC whose picture or Vorbis comments claim any length at any of `CLAIMS`, with `left` bytes
/// after that claim: one past them is refused by every way of reading the file (symphonia would
/// allocate it first, 4 GiB for the fuzz input's picture), and one inside them is left to
/// symphonia, which never asks for more than its block holds.
#[hegel::test(test_cases = 200)]
#[hegel::explicit_test_case(
    at = 2usize,
    len = 0xff00_0000u32,
    left = 0usize,
    held = 0usize,
    key = "METADATA_BLOCK_PICTURE"
)]
#[hegel::explicit_test_case(at = 4usize, len = u32::MAX, left = 3usize, held = 5usize, key = "METADATA_BLOCK_PICTURE")]
#[hegel::explicit_test_case(at = 6usize, len = 9u32, left = 8usize, held = 1usize, key = "metadata_block_picture")]
#[hegel::explicit_test_case(at = 3usize, len = 8u32, left = 8usize, held = 0usize, key = "METADATA_BLOCK_PICTURE")]
fn prop_metadata_claiming_more_than_its_block_holds_is_refused(tc: TestCase) {
    let at = tc.draw(gs::integers::<usize>().max_value(CLAIMS.len() - 1));
    let len = tc.draw(hegel::one_of!(gs::integers::<u32>().max_value(96), gs::integers::<u32>()));
    let left = tc.draw(gs::integers::<usize>().max_value(64));
    // The fields before the claim each hold this many bytes.
    let held = tc.draw(gs::integers::<usize>().max_value(16));
    let key = tc.draw(gs::sampled_from(vec!["METADATA_BLOCK_PICTURE", "metadata_block_picture"]));
    let field = vec![b'x'; held];
    // What comes before the claim, the claim, and `left` bytes the rest is read from.
    let claimed = |before: Vec<u8>, len: [u8; 4]| [before, len.to_vec(), vec![0; left]].concat();
    let picture = |at: usize| {
        let whole = picture_body(&field, &field, &[]);
        // The type, then the media type and description each after its length, then 16 bytes.
        let ends = [4, 8 + held, 28 + 2 * held];
        claimed(whole[..ends[at]].to_vec(), len.to_be_bytes())
    };
    let comments = |at: usize| {
        let whole = comments_body(&field, &[field.clone(), field.clone()]);
        // The vendor's name after its length, then the count and the comments, each after its length.
        let ends = [0, 8 + held, 12 + 2 * held];
        claimed(whole[..ends[at]].to_vec(), len.to_le_bytes())
    };
    let (block, why) = match at {
        0..3 => ((6, picture(at)), PICTURE),
        3..6 => ((4, comments(at - 3)), COMMENT),
        _ => ((4, comments_body(b"x", &[picture_comment(key, &picture(2))])), PICTURE),
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.flac");
    std::fs::write(&path, flac_with(&[block])).unwrap();

    let past = u64::from(len) > left as u64;
    match vet_container(&path) {
        Ok(()) => {
            assert!(!past, "{} claiming {len} of {left} passed", CLAIMS[at]);
            let _ = open(&path);
        }
        Err(Error::Parse { message, .. }) => {
            assert!(past && message == why, "{} claiming {len} of {left}: {message}", CLAIMS[at]);
            for read in [open(&path).map(drop), decode(&path, 11_025, &Ctx::silent(), &mut |_| {}).map(drop)] {
                assert!(matches!(&read, Err(Error::Parse { message, .. }) if message == why), "{read:?}");
            }
        }
        Err(e) => panic!("{e}"),
    }
}

/// A FLAC holding what its pictures and comments claim (a picture block, comments with a picture
/// among them, in any order, after an ID3v2 tag or not) passes, and decodes to its samples.
#[hegel::test(test_cases = 50)]
fn prop_flacs_holding_what_their_metadata_claims_decode(tc: TestCase) {
    let bytes = || gs::vecs(gs::integers::<u8>()).max_size(48);
    let picture = picture_body(b"image/png", &tc.draw(bytes()), &tc.draw(bytes()));
    let texts: Vec<Vec<u8>> = tc.draw(gs::vecs(bytes()).max_size(4));
    let mut comments: Vec<Vec<u8>> = texts.iter().map(|t| [b"COMMENT=".as_slice(), t].concat()).collect();
    if tc.draw(gs::booleans()) {
        comments.push(picture_comment("METADATA_BLOCK_PICTURE", &picture));
    }
    let mut blocks = vec![(6, picture), (4, comments_body(&tc.draw(bytes()), &comments))];
    if tc.draw(gs::booleans()) {
        blocks.reverse();
    }
    let tagged = tc.draw(gs::booleans());
    // An ID3v2.4 tag of 20 bytes of padding.
    let tag = if tagged { [&b"ID3\x04\0\0\0\0\0\x14"[..], &[0; 20]].concat() } else { vec![] };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.flac");
    std::fs::write(&path, [tag, flac_with(&blocks)].concat()).unwrap();

    vet_container(&path).unwrap();
    let mut heard = vec![];
    let format = crate::decode_native(&path, crate::Damaged::Fail, &Ctx::silent(), |s: &[i16], _, _| -> Result<_> {
        heard.extend_from_slice(s);
        Ok(std::ops::ControlFlow::Continue(()))
    })
    .unwrap();
    assert_eq!((format.sample_rate, heard), (44_100, (1..=16).collect::<Vec<i16>>()));
}

/// What symphonia's own probe makes of a file.
#[derive(Debug, PartialEq)]
enum Probed {
    /// It reads FLAC metadata from here, just past the marker.
    Flac(u64),
    /// It reads another format.
    Other,
    /// It fails: a marker or an ID3v2 tag cut short, or no marker at all.
    Failed,
}

fn probed(path: &Path) -> Probed {
    let mut mss = MediaSourceStream::new(Box::new(File::open(path).unwrap()), Default::default());
    loop {
        match symphonia::default::get_probe().next(&mut mss) {
            Ok(Instantiate::Format(_)) => {
                let at = mss.pos();
                return match mss.read_quad_bytes() {
                    Ok(marker) if marker == *b"fLaC" => Probed::Flac(at + 4),
                    _ => Probed::Other,
                };
            }
            Ok(Instantiate::Metadata(reader)) => {
                if reader(&MetadataOptions::default()).read_all(&mut mss).is_err() {
                    return Probed::Failed;
                }
            }
            Err(_) => return Probed::Failed,
        }
    }
}

/// What can come before a FLAC's marker: any bytes, an ID3v2 tag (which the probe reads past), a
/// marker of a format symphonia reads (MP3's, ADTS's, RIFF's, AIFF's, MP4's) or doesn't (Ogg's),
/// zeros, or another FLAC's marker.
const PIECES: usize = 5;

/// Whatever comes before a FLAC, the one the vet walks is the one symphonia's probe has its FLAC
/// reader read, and when the probe settles on another format, the vet walks none.
#[hegel::test(test_cases = 200)]
// Zeros to the probe's limit, a tag at its edge, and zeros to the edge of the limit after the tag.
#[hegel::explicit_test_case(pieces = vec![3usize, 1, 3], zeros = (1usize << 20) - 2, junk = Vec::<u8>::new(), size = 10u32, marker = 0usize)]
#[hegel::explicit_test_case(pieces = vec![2usize, 4], zeros = 0usize, junk = Vec::<u8>::new(), size = 0u32, marker = 5usize)]
fn prop_the_flac_vetted_is_the_one_symphonia_reads(tc: TestCase) {
    let pieces = tc.draw(gs::vecs(gs::integers::<usize>().max_value(PIECES - 1)).max_size(4));
    let zeros = tc.draw(gs::integers::<usize>().max_value(64));
    let junk: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(24));
    // The ID3v2 tag's size: under 10 bytes, symphonia can't read a frame from it.
    let size = tc.draw(gs::integers::<u32>().max_value(64));
    let markers: [&[u8]; 6] = [b"\xff\xfb", b"\xff\xf1", b"RIFF", b"FORM", b"ftyp", b"OggS"];
    let marker = tc.draw(gs::integers::<usize>().max_value(markers.len() - 1));
    let mut file = vec![];
    for piece in pieces {
        match piece {
            0 => file.extend_from_slice(&junk),
            1 => {
                let syncsafe = (0..4).rev().map(|k| (size >> (7 * k) & 0x7f) as u8);
                file.extend(b"ID3\x04\0\0".iter().copied().chain(syncsafe));
                file.resize(file.len() + size as usize, 0);
            }
            2 => file.extend_from_slice(markers[marker]),
            3 => file.resize(file.len() + zeros, 0),
            _ => file.extend_from_slice(b"fLaC"),
        }
    }
    file.extend(flac_with(&[]));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.flac");
    std::fs::write(&path, &file).unwrap();

    let found = find_flac(&mut File::open(&path).unwrap()).unwrap();
    match probed(&path) {
        Probed::Flac(at) => assert_eq!(found, Some(at)),
        Probed::Other => assert_eq!(found, None),
        Probed::Failed => {}
    }
}

/// The input the mp4 fuzz target crashed on: a FLAC after 8 other bytes, whose picture claims
/// 4,278,190,080 bytes of data in a block of 65,280.
#[test]
fn the_fuzzers_flac_picture_length_is_refused() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/mp4/flac-picture-length.flac");
    for read in
        [vet_container(&path), open(&path).map(drop), decode(&path, 11_025, &Ctx::silent(), &mut |_| {}).map(drop)]
    {
        assert!(matches!(&read, Err(Error::Parse { message, .. }) if message == PICTURE), "{read:?}");
    }
}

/// The probe looks for a marker as far as 2 bytes short of its limit, as symphonia's does.
#[test]
fn the_probe_limit_is_symphonias() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.flac");
    for (zeros, found) in [(PROBE_LIMIT - 2, true), (PROBE_LIMIT - 1, false)] {
        std::fs::write(&path, [vec![0; zeros as usize], flac_with(&[])].concat()).unwrap();
        let at = find_flac(&mut File::open(&path).unwrap()).unwrap();
        assert_eq!(at, found.then_some(zeros + 4));
        assert_eq!(probed(&path), if found { Probed::Flac(zeros + 4) } else { Probed::Failed });
    }
}

/// Pictures in comments decode as symphonia decodes them: padding or none, a lone last symbol
/// dropped, and any symbol outside the alphabet (padding before the end included) giving up.
#[test]
fn pictures_in_comments_decode_as_symphonia_decodes_them() {
    for (text, bytes) in [
        ("TWFu", Some(&b"Man"[..])),
        ("TWE=", Some(b"Ma")),
        ("TWE", Some(b"Ma")),
        ("TQ==", Some(b"M")),
        ("TWFuT", Some(b"Man")),
        ("TWFu!", Some(b"Man")),
        ("+/+/", Some(b"\xfb\xff\xbf")),
        ("", Some(b"")),
        ("TW=u", None),
        ("TW u", None),
    ] {
        assert_eq!(base64(text).as_deref(), bytes, "{text}");
    }
}

/// A folder isn't walked: there's nothing in it for symphonia to abort on.
#[cfg(unix)]
#[test]
fn a_folder_is_left_to_symphonia() {
    let dir = tempfile::tempdir().unwrap();
    vet_container(dir.path()).unwrap();
}
