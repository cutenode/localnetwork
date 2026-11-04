use std::ops::ControlFlow;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_progress::Ctx;

use super::*;
use crate::audio::{Damaged, decode, decode_native, open};
use crate::test_util::{be, m4a, mp4_atom, mp4_empty_tables, mp4_full};

/// Every way of reading `path` (opening it, decoding it at its rate and to mono), as the error
/// message each gives, or `None` for one that read it.
fn read_all(path: &Path) -> Vec<Option<String>> {
    let ctx = Ctx::silent();
    let message = |r: Result<()>| match r {
        Ok(()) => None,
        Err(Error::Parse { message, .. }) => Some(message),
        Err(e) => panic!("{e}"),
    };
    vec![
        message(open(path).map(drop)),
        message(
            decode_native(path, Damaged::Skip, &ctx, |_: &[f32], _, _| -> Result<_> { Ok(ControlFlow::Continue(())) })
                .map(drop),
        ),
        message(decode(path, 11_025, &ctx, &mut |_| {}).map(drop)),
    ]
}

/// An M4A that holds what it claims passes, and opens at its rate.
#[test]
fn an_m4a_that_holds_what_it_claims_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.m4a");
    std::fs::write(&path, m4a(8_000, &mp4_empty_tables(), &[])).unwrap();
    vet_container(&path).unwrap();
    let (_, track) = open(&path).unwrap();
    assert_eq!(track.codec_params.sample_rate, Some(8_000));
}

/// The sample tables symphonia sizes from their counts, with each entry's bytes.
const SIZED: [(&[u8; 4], usize); 5] = [(b"stsz", 4), (b"stco", 4), (b"co64", 8), (b"stts", 8), (b"stsc", 12)];

/// An M4A whose sample table claims any number of entries: one that holds them all is read as
/// before, and one claiming more than the file holds (a count near `u32::MAX` would have
/// symphonia ask for 16 GiB, aborting the process where memory isn't overcommitted) is refused
/// by every way of reading it, before symphonia sees it, wherever the MP4 starts in a file
/// named as one (symphonia finds it after other bytes too). An `stsz` with a constant sample size has no entries, so any count is fine.
#[hegel::test(test_cases = 200)]
#[hegel::explicit_test_case(table = 0usize, count = u32::MAX, held = 0usize, constant_size = false, junk = 0usize)]
#[hegel::explicit_test_case(table = 1usize, count = 1_000_000u32, held = 3usize, constant_size = false, junk = 5usize)]
fn sample_tables_claiming_more_than_the_file_holds_are_refused(tc: TestCase) {
    let table = tc.draw(gs::integers::<usize>().max_value(SIZED.len() - 1));
    let (kind, entry) = SIZED[table];
    let count = tc.draw(hegel::one_of!(gs::integers::<u32>().max_value(64), gs::integers::<u32>()));
    let held = tc.draw(gs::integers::<usize>().max_value(64));
    let constant_size = tc.draw(gs::booleans());
    let constant = constant_size && kind == b"stsz";
    let junk = tc.draw(gs::integers::<usize>().max_value(32));
    let entries = vec![0x11; held * entry];
    let sized = match kind {
        b"stsz" => mp4_full(kind, &[be(&[u32::from(constant), count]), entries].concat()),
        _ => mp4_full(kind, &[be(&[count]), entries].concat()),
    };
    let others = mp4_empty_tables();
    let tables = [sized, others].concat();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.m4a");
    std::fs::write(&path, [vec![0x20; junk], m4a(8_000, &tables, &[])].concat()).unwrap();

    let fits = constant || count as usize <= held;
    match vet_container(&path) {
        Ok(()) => assert!(fits, "{count} {kind:?} entries passed in {held}"),
        Err(Error::Parse { message, .. }) => {
            assert!(!fits, "{count} in {held}: {message}");
            assert!(message.contains("sample table"), "{message}");
            for read in read_all(&path) {
                assert_eq!(read.as_deref(), Some(message.as_str()));
            }
        }
        Err(e) => panic!("{e}"),
    }
}

/// An atom inside another that runs past it or past the file's end (a tag's `data` claiming
/// gigabytes, which symphonia would ask for in one go) is refused by every way of reading the
/// file; the file's own last atom running past its end (an `mdat` cut short) isn't.
#[hegel::test(test_cases = 100)]
#[hegel::explicit_test_case(extra = u32::MAX, nested = true)]
#[hegel::explicit_test_case(extra = 1_000_000u32, nested = false)]
fn atoms_running_past_what_holds_them_are_refused(tc: TestCase) {
    let extra =
        tc.draw(hegel::one_of!(gs::integers::<u32>().max_value(64), gs::integers::<u32>().max_value(u32::MAX - 64)));
    let nested = tc.draw(gs::booleans());
    let value = b"\0\0\0\x01\0\0\0\0a set";
    let data = mp4_atom(b"data", value);
    let mut tag = mp4_atom(b"\xa9nam", &data);
    let claim = |atom: &mut Vec<u8>, at: usize| {
        let size = u32::from_be_bytes(atom[at..at + 4].try_into().unwrap()).saturating_add(extra);
        atom[at..at + 4].copy_from_slice(&size.to_be_bytes());
    };
    let mut file = if nested {
        // The `data` atom inside the tag.
        claim(&mut tag, 8);
        let udta = mp4_atom(b"udta", &mp4_full(b"meta", &mp4_atom(b"ilst", &tag)));
        m4a(8_000, &mp4_empty_tables(), &udta)
    } else {
        m4a(8_000, &mp4_empty_tables(), &[])
    };
    if !nested {
        let mdat = file.len() - 8;
        claim(&mut file, mdat);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.m4a");
    std::fs::write(&path, &file).unwrap();
    match vet_container(&path) {
        Ok(()) => assert!(!nested || extra == 0, "a data atom {extra} bytes too long passed"),
        Err(Error::Parse { message, .. }) => {
            assert!(nested && extra > 0, "{extra} past, nested {nested}: {message}");
            for read in read_all(&path) {
                assert_eq!(read.as_deref(), Some(message.as_str()));
            }
        }
        Err(e) => panic!("{e}"),
    }
}

/// Anything that isn't an MP4 is left alone, as is a file too short to say.
#[hegel::test(test_cases = 100)]
fn other_files_are_left_to_the_decoder(tc: TestCase) {
    let bytes: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(64));
    let name = tc.draw(gs::sampled_from(vec!["x.wav", "x.mp3", "x.bin"]));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, &bytes).unwrap();
    let is_mp4 = bytes.get(4..8) == Some(b"ftyp");
    assert!(is_mp4 || vet_container(&path).is_ok());
}

/// A `data` atom whose 64-bit size claims 16 TiB (symphonia would ask for all of it at once,
/// which aborts the process anywhere) is refused.
#[test]
fn a_tag_claiming_terabytes_is_refused() {
    let data = [be(&[1]), b"data".to_vec(), (1u64 << 44).to_be_bytes().to_vec(), be(&[1, 0])].concat();
    let udta = mp4_atom(b"udta", &mp4_full(b"meta", &mp4_atom(b"ilst", &mp4_atom(b"\xa9nam", &data))));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.m4a");
    std::fs::write(&path, m4a(8_000, &mp4_empty_tables(), &udta)).unwrap();
    for read in read_all(&path) {
        assert_eq!(read.as_deref(), Some("an atom runs past the one holding it"));
    }
}

/// The sample table atom `kind` holding the first `held` bytes of a table of one entry: version
/// and flags, `stsz`'s sample size (0, so entries follow), the count, the entry.
fn table_cut_at(kind: &[u8; 4], held: usize) -> Vec<u8> {
    let body = if kind == b"stsz" { be(&[0, 0, 1, 7]) } else { be(&[0, 1, 7, 7, 7]) };
    mp4_atom(kind, &body[..held.min(body.len())])
}

/// `inner` inside each of `kinds` in turn, the last outermost.
fn nest(inner: Vec<u8>, kinds: &[&[u8; 4]]) -> Vec<u8> {
    kinds.iter().fold(inner, |inner, kind| mp4_atom(*kind, &inner))
}

const FTYP: &[u8] = b"\0\0\0\x14ftypM4A \0\0\0\0M4A ";

/// An MP4 that ends inside what an atom says next (the 64-bit size of the file's last atom, or
/// the counts of a sample table that's the file's last bytes) is the decoder's to report, and so
/// is anything that isn't a regular file (a folder named as an M4A): vetting passes them.
#[hegel::test(test_cases = 100)]
fn prop_files_ending_inside_what_an_atom_says_are_left_to_the_decoder(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.m4a");
    if tc.draw(gs::booleans()) {
        // A 64-bit size cut short: `size` 1, then fewer than its 8 bytes.
        let held = tc.draw(gs::integers::<usize>().max_value(7));
        let last = [be(&[1]), b"free".to_vec(), vec![0; held]].concat();
        std::fs::write(&path, [m4a(8_000, &mp4_empty_tables(), &[]), last].concat()).unwrap();
    } else {
        let kind = tc.draw(gs::sampled_from(vec![b"stsz", b"stco", b"co64", b"stts", b"stsc"]));
        // Short of the count (and for `stsz`, maybe of its sample size too).
        let held = tc.draw(gs::integers::<usize>().max_value(if kind == b"stsz" { 11 } else { 7 }));
        let moov = nest(table_cut_at(kind, held), &[b"stbl", b"minf", b"mdia", b"trak", b"moov"]);
        std::fs::write(&path, [FTYP, &moov].concat()).unwrap();
    }
    vet_container(&path).unwrap();
    let folder = dir.path().join("folder.m4a");
    std::fs::create_dir(&folder).unwrap();
    vet_container(&folder).unwrap();
}

/// An atom whose size is 0 runs to the end of what holds it (the file, for one of the file's
/// own), and is walked like any other: one holding a sample table claiming more than it holds is
/// refused, one holding what it claims passes.
#[hegel::test(test_cases = 100)]
fn prop_atoms_of_size_0_run_to_the_end_of_what_holds_them(tc: TestCase) {
    let claim = tc.draw(gs::integers::<u32>().max_value(4));
    let held = tc.draw(gs::integers::<usize>().max_value(4));
    let top = tc.draw(gs::booleans());
    let stco = mp4_full(b"stco", &be(&[claim, 7, 7, 7, 7][..1 + held]));
    // `stbl` of size 0, the last atom of `minf`.
    let stbl = [be(&[0]), b"stbl".to_vec(), stco].concat();
    let mut moov = nest(stbl, &[b"minf", b"mdia", b"trak", b"moov"]);
    if top {
        // The file's own last atom, of size 0.
        moov[..4].copy_from_slice(&be(&[0]));
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.m4a");
    std::fs::write(&path, [FTYP, &moov].concat()).unwrap();
    match vet_container(&path) {
        Ok(()) => assert!(claim as usize <= held, "{claim} entries passed in {held}"),
        Err(Error::Parse { message, .. }) => {
            assert!(claim as usize > held, "{claim} in {held}: {message}");
            assert_eq!(message, "a sample table claims more entries than the file holds");
        }
        Err(e) => panic!("{e}"),
    }
}

/// An atom saying it's smaller than its own header (8 bytes, or 16 with a 64-bit size), the
/// file's own or inside another, is refused by every way of reading the file.
#[hegel::test(test_cases = 100)]
fn prop_atoms_smaller_than_their_header_are_refused(tc: TestCase) {
    let wide = tc.draw(gs::booleans());
    // A 32-bit size of 0 or 1 says where the size is rather than what it is.
    let size = tc.draw(gs::integers::<u64>().min_value(if wide { 0 } else { 2 }).max_value(if wide { 15 } else { 7 }));
    let atom = match wide {
        true => [be(&[1]), b"free".to_vec(), size.to_be_bytes().to_vec(), vec![0; 8]].concat(),
        false => [be(&[size as u32]), b"free".to_vec()].concat(),
    };
    let file = if tc.draw(gs::booleans()) {
        m4a(8_000, &mp4_empty_tables(), &mp4_atom(b"udta", &atom))
    } else {
        [m4a(8_000, &mp4_empty_tables(), &[]), atom].concat()
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.m4a");
    std::fs::write(&path, &file).unwrap();
    for read in read_all(&path) {
        assert_eq!(read.as_deref(), Some("an atom is smaller than its header"));
    }
}

/// A file that can't be read is that file's I/O error, before anything reads its atoms.
#[cfg(unix)]
#[hegel::test(test_cases = 5)]
fn prop_a_file_that_cant_be_read_is_its_io_error(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(tc.draw(gs::sampled_from(vec!["x.m4a", "x.wav"])));
    std::fs::write(&path, m4a(8_000, &mp4_empty_tables(), &[])).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    match vet_container(&path) {
        Err(Error::Io { path: at, .. }) => assert_eq!(at, path),
        other => panic!("{other:?}"),
    }
}
