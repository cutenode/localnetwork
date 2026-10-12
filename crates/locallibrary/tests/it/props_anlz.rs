//! Property tests for analysis files (`ANLZ0000.DAT/.EXT/.2EX`): cue lists written and read back,
//! checked against rekordcrate's independent parser and the older reader, the `PPTH` path, and
//! every reader on damaged files.

use crate::common;

use std::io::Cursor;
use std::sync::OnceLock;

use binrw::BinRead;
use hegel::generators as gs;
use hegel::{Generator, TestCase};
use locallibrary::anlz::{self, CueFile, CuePoint};

const ANALYSED: [&str; 2] = ["P016/0000875E", "P053/0001D21F"];

/// One track's `.DAT`, `.EXT` and `.2EX`.
type Analysis = (Vec<u8>, Vec<u8>, Vec<u8>);

/// The demo export's analysis files.
fn real_files() -> &'static [Analysis] {
    static FILES: OnceLock<Vec<Analysis>> = OnceLock::new();
    FILES.get_or_init(|| {
        ANALYSED
            .iter()
            .map(|dir| {
                let d = common::fixtures().join("rekordcrate/demo_tracks/PIONEER/USBANLZ").join(dir);
                let read = |ext: &str| std::fs::read(d.join(format!("ANLZ0000.{ext}"))).unwrap();
                (read("DAT"), read("EXT"), read("2EX"))
            })
            .collect()
    })
}

/// A `.DAT`/`.EXT` pair to write cues into: one of rekordbox's, or a minimal synthetic file.
#[hegel::test_helper]
fn base_pair(tc: &TestCase) -> (Vec<u8>, Vec<u8>) {
    let which = tc.draw(gs::integers::<usize>().max_value(ANALYSED.len()));
    match real_files().get(which) {
        Some((dat, ext, _)) => (dat.clone(), ext.clone()),
        None => (common::synthetic_anlz("/Contents/a.mp3"), common::synthetic_anlz("/Contents/a.mp3")),
    }
}

/// A position in ms: usually one of a few shared ones, so cues often coincide.
#[hegel::test_helper]
fn position(tc: &TestCase) -> u32 {
    let shared = tc.draw(gs::booleans());
    if shared { tc.draw(gs::integers::<u32>().max_value(3)) * 1000 } else { tc.draw(gs::integers::<u32>()) }
}

/// A cue in `slot` (`None` for a memory cue) with everything the files can store about it.
/// Loops end after they start (and not at `u32::MAX`, which means "no loop"), only loops are
/// active, quantised lengths are non-zero, comments have no NUL (it ends them) and colours are
/// in their documented ranges.
#[hegel::test_helper]
fn cue(tc: &TestCase, slot: Option<u8>, named: bool) -> CuePoint {
    let start_ms = position(tc);
    let is_loop = start_ms <= u32::MAX - 2 && tc.draw(gs::booleans());
    let end_ms = if is_loop {
        let len = tc.draw(gs::integers::<u32>().min_value(1).max_value(u32::MAX - 1 - start_ms));
        Some(start_ms + len)
    } else {
        None
    };
    let quantised = tc.draw(gs::booleans());
    let loop_beats = if quantised {
        let n = tc.draw(gs::integers::<u16>().min_value(1));
        let d = tc.draw(gs::integers::<u16>().min_value(1));
        Some((n, d))
    } else {
        None
    };
    let comment = tc.draw(gs::text().min_size(usize::from(named)).max_size(16).exclude_characters("\0"));
    let color_id = tc.draw(gs::integers::<u8>().max_value(8));
    let color_code = tc.draw(gs::integers::<u8>().max_value(62));
    let color_rgb = tc.draw(gs::arrays::<_, u8, 3>(gs::integers::<u8>()));
    let active = tc.draw(gs::booleans());
    CuePoint {
        hot_cue: slot,
        start_ms,
        end_ms,
        loop_beats,
        comment,
        color_id,
        color_code,
        color_rgb,
        active_loop: is_loop && active,
        ..Default::default()
    }
}

/// Hot cues in distinct slots (mostly A-P, sometimes far beyond) and some memory cues.
#[hegel::test_helper]
fn cue_list(tc: &TestCase, named: bool) -> Vec<CuePoint> {
    let slot = hegel::one_of!(gs::integers::<u8>().max_value(15), gs::integers::<u8>());
    let slots: Vec<u8> = tc.draw(gs::vecs(slot).unique(true).max_size(10));
    let memory = tc.draw(gs::integers::<usize>().max_value(6));
    let mut cues: Vec<CuePoint> = slots.into_iter().map(|s| cue(tc, Some(s), named)).collect();
    cues.extend((0..memory).map(|_| cue(tc, None, named)));
    let order = tc.draw(gs::permutations((0..cues.len()).collect::<Vec<_>>()));
    order.into_iter().map(|i| cues[i].clone()).collect()
}

/// Whether two cue lists hold the same cues (as multisets, by [`CuePoint::same_as`]).
fn same_cues(a: &[CuePoint], b: &[CuePoint]) -> bool {
    let mut unused: Vec<&CuePoint> = b.iter().collect();
    a.len() == b.len()
        && a.iter().all(|x| match unused.iter().position(|y| x.same_as(y)) {
            Some(i) => {
                unused.swap_remove(i);
                true
            }
            None => false,
        })
}

/// Cues for a failure message, without the raw entries they were read from.
fn show(cues: &[CuePoint]) -> String {
    cues.iter()
        .map(|c| {
            let raw = if c.raw == anlz::RawCue::default() { "" } else { " (read)" };
            let CuePoint { hot_cue, start_ms, end_ms, loop_beats, comment, color_id, color_code, color_rgb, .. } = c;
            let active = c.active_loop;
            let position = format!("{hot_cue:?} {start_ms}..{end_ms:?} beats {loop_beats:?}");
            format!("\n  hot {position} {comment:?} colour {color_id}/{color_code}/{color_rgb:?} active {active}{raw}")
        })
        .collect()
}

/// What a `PCPT` entry can hold: hot cue, start, end, active.
type Basic = (Option<u8>, u32, Option<u32>, bool);

fn basic(c: &CuePoint) -> Basic {
    (c.hot_cue, c.start_ms, c.end_ms, c.active_loop)
}

fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort();
    v
}

fn basics(cues: &[CuePoint]) -> Vec<Basic> {
    sorted(cues.iter().map(basic).collect())
}

/// Everything in an analysis file but its cues.
fn non_cue_content(bytes: &[u8]) -> String {
    format!(
        "{:?} {:?} {:?} {:?} {:?}",
        anlz::read_path(bytes).unwrap(),
        anlz::beat_grid(bytes).unwrap(),
        anlz::preview(bytes).unwrap(),
        anlz::detail(bytes).unwrap(),
        anlz::song_structure(bytes).unwrap()
    )
}

fn write(bytes: &[u8], file: CueFile, cues: &[CuePoint]) -> Vec<u8> {
    anlz::write_cue_points(bytes, file, cues).unwrap_or_else(|e| panic!("writing {file:?}: {e}"))
}

/// Cues written into a `.DAT` and `.EXT` pair read back as the same cues, and the files'
/// lengths and other content are as they were.
#[hegel::test(test_cases = 300)]
fn cues_written_to_dat_and_ext_read_back(tc: TestCase) {
    let (dat, ext) = base_pair(&tc);
    let cues = cue_list(&tc, false);
    let dat2 = write(&dat, CueFile::Dat, &cues);
    let ext2 = write(&ext, CueFile::Ext, &cues);
    let back = anlz::read_cue_points(&dat2, Some(&ext2)).unwrap();
    assert!(same_cues(&back, &cues), "wrote {}\nread {}", show(&cues), show(&back));
    for (before, after) in [(&dat, &dat2), (&ext, &ext2)] {
        assert_eq!(u32::from_be_bytes(after[8..12].try_into().unwrap()) as usize, after.len());
        assert_eq!(non_cue_content(after), non_cue_content(before));
    }
}

/// A `.DAT` without an `.EXT` keeps every cue's position, loop and active flag (it has nowhere
/// to put names or colours).
#[hegel::test]
fn cues_written_to_a_lone_dat_keep_their_positions(tc: TestCase) {
    let (dat, _) = base_pair(&tc);
    let cues = cue_list(&tc, false);
    let dat2 = write(&dat, CueFile::DatAlone, &cues);
    let back = anlz::read_cue_points(&dat2, None).unwrap();
    assert_eq!(basics(&back), basics(&cues));
    assert!(back.iter().all(|c| c.comment.is_empty() && c.loop_beats.is_none()));
    assert_eq!(non_cue_content(&dat2), non_cue_content(&dat));
}

/// Writing back what was read changes nothing: every list holding the wanted cues is kept byte
/// for byte, so `write(read(write(cues))) == write(cues)`.
#[hegel::test(test_cases = 300)]
fn writing_what_was_read_is_a_fixpoint(tc: TestCase) {
    let (dat, ext) = base_pair(&tc);
    let cues = cue_list(&tc, false);
    let dat2 = write(&dat, CueFile::Dat, &cues);
    let ext2 = write(&ext, CueFile::Ext, &cues);
    let back = anlz::read_cue_points(&dat2, Some(&ext2)).unwrap();
    assert_eq!(write(&dat2, CueFile::Dat, &back), dat2);
    assert_eq!(write(&ext2, CueFile::Ext, &back), ext2);

    let alone = write(&dat, CueFile::DatAlone, &cues);
    let back = anlz::read_cue_points(&alone, None).unwrap();
    assert_eq!(write(&alone, CueFile::DatAlone, &back), alone);
}

/// Shrunk counterexample of `writing_what_was_read_is_a_fixpoint`: two memory cues at the same
/// position in a `.DAT` with no `.EXT` were read as one, so rewriting them dropped the other.
#[test]
fn twin_memory_cues_in_a_lone_dat_survive_a_rewrite() {
    let base = common::synthetic_anlz("/Contents/a.mp3");
    let alone = write(&base, CueFile::DatAlone, &[CuePoint::default(), CuePoint::default()]);
    let back = anlz::read_cue_points(&alone, None).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(write(&alone, CueFile::DatAlone, &back), alone);
}

/// The edit cycle `cue_edit` runs: read a track's cues, reorder, rename, recolour, (de)activate
/// loops, move, drop and add some, write them back over the old ones and read them again. A
/// list still holding the wanted cues is kept byte for byte while the others are rewritten, so
/// the `.DAT`'s `PCPT` entries and the `.EXT`'s `PCP2` entries can end up in different orders;
/// the reader must still pair each entry with its own cue.
#[hegel::test(test_cases = 1000)]
fn edited_cues_read_back_after_rewriting(tc: TestCase) {
    let (dat, ext) = base_pair(&tc);
    let first = cue_list(&tc, false);
    let (dat1, ext1) = (write(&dat, CueFile::Dat, &first), write(&ext, CueFile::Ext, &first));
    let read = anlz::read_cue_points(&dat1, Some(&ext1)).unwrap();

    // Renaming, recolouring and (de)activating leave the positions as they were.
    let moves = tc.draw(gs::booleans());
    let reorder = tc.draw(gs::permutations((0..read.len()).collect::<Vec<_>>()));
    let mut edited = Vec::new();
    for i in reorder {
        let mut c = read[i].clone();
        match tc.draw(gs::integers::<u8>().max_value(if moves { 5 } else { 3 })) {
            0 => {}
            1 => c.comment = tc.draw(gs::text().max_size(8).exclude_characters("\0")),
            2 => {
                c.color_id = tc.draw(gs::integers::<u8>().max_value(8));
                c.color_code = tc.draw(gs::integers::<u8>().max_value(62));
            }
            // Only the `.DAT`'s lists (and the `.EXT`'s hot cues D-H) record it.
            3 => c.active_loop = c.is_loop() && !c.active_loop,
            4 if !c.is_loop() => c.start_ms = position(&tc),
            4 => {}
            _ => continue,
        }
        edited.push(c);
    }
    if moves {
        let added = tc.draw(gs::integers::<usize>().max_value(3));
        edited.extend((0..added).map(|_| cue(&tc, None, false)));
    }

    let (dat2, ext2) = (write(&dat1, CueFile::Dat, &edited), write(&ext1, CueFile::Ext, &edited));
    let back = anlz::read_cue_points(&dat2, Some(&ext2)).unwrap();
    assert!(same_cues(&back, &edited), "wrote {}\nread {}", show(&edited), show(&back));
}

/// Shrunk counterexample of `edited_cues_read_back_after_rewriting`: a memory cue and an active
/// loop at the same position, renamed and listed in the other order. The `.DAT`'s list is kept
/// (the positions didn't change) while the `.EXT`'s is rewritten in the new order, and the
/// loop's active flag went to the point cue.
#[test]
fn active_loop_stays_with_its_loop_when_lists_disagree_on_order() {
    let point = CuePoint { start_ms: 1000, ..Default::default() };
    let lp = CuePoint { start_ms: 1000, end_ms: Some(2000), active_loop: true, ..Default::default() };
    let base = common::synthetic_anlz("/Contents/a.mp3");
    let cues = [point, lp];
    let (dat1, ext1) = (write(&base, CueFile::Dat, &cues), write(&base, CueFile::Ext, &cues));
    let mut read = anlz::read_cue_points(&dat1, Some(&ext1)).unwrap();
    read.reverse();
    read[0].comment = "Loop".into();
    let (dat2, ext2) = (write(&dat1, CueFile::Dat, &read), write(&ext1, CueFile::Ext, &read));
    assert_eq!(dat2, dat1);
    let back = anlz::read_cue_points(&dat2, Some(&ext2)).unwrap();
    assert!(same_cues(&back, &read), "wrote {}\nread {}", show(&read), show(&back));
}

/// Shrunk counterexample of `edited_cues_read_back_after_rewriting` once the one above was fixed:
/// two memory loops over the same span, one active, renamed and listed in the other order. Only
/// their order pairs a `PCPT` entry with its `PCP2` entry, and the `.DAT`'s list was kept in the
/// old order while the `.EXT`'s was rewritten in the new one.
#[test]
fn active_flag_of_twin_loops_survives_a_reordering_rename() {
    let lp = CuePoint { start_ms: 1000, end_ms: Some(2000), ..Default::default() };
    let cues =
        [CuePoint { active_loop: true, comment: "a".into(), ..lp.clone() }, CuePoint { comment: "b".into(), ..lp }];
    let base = common::synthetic_anlz("/Contents/a.mp3");
    let (dat1, ext1) = (write(&base, CueFile::Dat, &cues), write(&base, CueFile::Ext, &cues));
    let mut read = anlz::read_cue_points(&dat1, Some(&ext1)).unwrap();
    read.reverse();
    read[0].comment = "c".into();
    let (dat2, ext2) = (write(&dat1, CueFile::Dat, &read), write(&ext1, CueFile::Ext, &read));
    let back = anlz::read_cue_points(&dat2, Some(&ext2)).unwrap();
    assert!(same_cues(&back, &read), "wrote {}\nread {}", show(&read), show(&back));
}

/// The mirror image, also shrunk from `edited_cues_read_back_after_rewriting`: the active flag
/// moved from one twin loop to the other as they swap places. `PCP2` entries don't record it, so
/// the `.EXT`'s list was kept in the old order while the `.DAT`'s was rewritten in the new one.
#[test]
fn active_flag_moved_between_twin_loops_reads_back() {
    let lp = CuePoint { start_ms: 0, end_ms: Some(1), ..Default::default() };
    let cues = [CuePoint { color_rgb: [0, 0, 1], ..lp.clone() }, CuePoint { active_loop: true, ..lp }];
    let base = common::synthetic_anlz("/Contents/a.mp3");
    let (dat1, ext1) = (write(&base, CueFile::Dat, &cues), write(&base, CueFile::Ext, &cues));
    let mut read = anlz::read_cue_points(&dat1, Some(&ext1)).unwrap();
    read.reverse();
    for c in &mut read {
        c.active_loop = !c.active_loop;
    }
    let (dat2, ext2) = (write(&dat1, CueFile::Dat, &read), write(&ext1, CueFile::Ext, &read));
    let back = anlz::read_cue_points(&dat2, Some(&ext2)).unwrap();
    assert!(same_cues(&back, &read), "wrote {}\nread {}", show(&read), show(&back));
}

/// rekordcrate's colour table row for a memory cue colour id.
fn color_index(id: u8) -> rekordcrate::util::ColorIndex {
    use rekordcrate::util::ColorIndex::*;
    [None, Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple][usize::from(id)].clone()
}

/// `(hot cue number, status, time, loop time)` of a `PCPT` entry.
type PcptView = (u32, u32, u32, u32);
/// `(hot cue number, time, loop time, comment, colour code, RGB, loop beats)` of a `PCP2` entry.
type Pcp2View = (u32, u32, u32, String, u8, (u8, u8, u8), (u16, u16));

fn hot_number(c: &CuePoint) -> u32 {
    c.hot_cue.map_or(0, |h| u32::from(h) + 1)
}

fn pcpt_view(c: &CuePoint) -> PcptView {
    (hot_number(c), if c.active_loop { 4 } else { 0 }, c.start_ms, c.end_ms.unwrap_or(u32::MAX))
}

fn pcp2_view(c: &CuePoint) -> Pcp2View {
    let [r, g, b] = c.color_rgb;
    let (n, d) = c.loop_beats.unwrap_or((0, 0));
    (hot_number(c), c.start_ms, c.end_ms.unwrap_or(u32::MAX), c.comment.clone(), c.color_code, (r, g, b), (n, d))
}

/// `(is hot list, entries)` of each `PCOB`, and every `PCP2` entry with its memory cue colour.
type Lists = (Vec<(bool, Vec<PcptView>)>, Vec<(Pcp2View, u8)>);

/// The cue lists of a file as rekordcrate reads them: `(is hot list, entries)` for each `PCOB`,
/// and every `PCP2` entry. rekordcrate frames every section with its own header parser. Two
/// known differences in its model are bridged: it calls a point cue type 0 where rekordbox
/// writes 1, and it expects 20 bytes after a `PCP2` entry's colour where rekordbox 7 writes 40
/// (so entries are read one at a time).
fn rekordcrate_lists(bytes: &[u8]) -> Lists {
    use rekordcrate::anlz::{ContentKind, CueList, CueListType, ExtendedCue, Header};
    let read_header = |at: usize| Header::read_be(&mut Cursor::new(&bytes[at..])).expect("rekordcrate header");
    let file = read_header(0);
    assert_eq!(file.kind, ContentKind::File);
    assert_eq!(file.total_size as usize, bytes.len());
    let (mut basic, mut extended) = (Vec::new(), Vec::new());
    let mut pos = file.size as usize;
    while pos < bytes.len() {
        let h = read_header(pos);
        let (size, total) = (h.size as usize, h.total_size as usize);
        assert!(12 <= size && size <= total && pos + total <= bytes.len(), "section at {pos} overruns the file");
        let mut section = bytes[pos..pos + total].to_vec();
        // Entries of cue lists, framed by rekordcrate, with point cues given rekordcrate's type.
        let mut entries = Vec::new();
        let mut e = if matches!(h.kind, ContentKind::CueList | ContentKind::ExtendedCueList) { size } else { total };
        while e < total {
            let eh = Header::read_be(&mut Cursor::new(&section[e..])).expect("rekordcrate entry header");
            let type_at = if eh.kind == ContentKind::Cue { 0x1c } else { 0x10 };
            if section[e + type_at] == 1 {
                section[e + type_at] = 0;
            }
            entries.push(e);
            e += eh.total_size as usize;
        }
        assert_eq!(e, total, "entries overrun their list");
        match h.kind {
            ContentKind::CueList => {
                let l = CueList::read_be(&mut Cursor::new(&section[12..])).expect("rekordcrate PCOB");
                assert_eq!(l.cues.len(), entries.len());
                let views = l.cues.iter().map(|c| (c.hot_cue, c.status, c.time, c.loop_time)).collect();
                basic.push((l.list_type == CueListType::HotCues, views));
            }
            ContentKind::ExtendedCueList => {
                for e in entries {
                    let c = ExtendedCue::read_be(&mut Cursor::new(&section[e..])).expect("rekordcrate PCP2");
                    let comment = String::from_utf16(&c.comment.0).unwrap();
                    let view = (
                        c.hot_cue,
                        c.time,
                        c.loop_time,
                        comment,
                        c.hot_cue_color_index,
                        c.hot_cue_color_rgb,
                        (c.loop_numerator, c.loop_denominator),
                    );
                    let color = (0..=8).find(|&i| color_index(i) == c.color).unwrap();
                    extended.push((view, color));
                }
            }
            _ => {}
        }
        pos += total;
    }
    (basic, extended)
}

/// rekordcrate, an independent parser, frames every section of what is written and finds the
/// cues where the module docs say they go: `.DAT` hot cues A-C and the memory cues, `.EXT` hot
/// cues D onwards in `PCOB` and every cue in `PCO2`. (Comments are non-empty: rekordcrate
/// expects a NUL even in an empty comment, where rekordbox 7 writes nothing.)
#[hegel::test]
fn rekordcrate_reads_what_is_written(tc: TestCase) {
    let (dat, ext) = base_pair(&tc);
    let cues = cue_list(&tc, true);
    let hot = |f: &dyn Fn(u8) -> bool| -> Vec<PcptView> {
        sorted(cues.iter().filter(|c| c.hot_cue.is_some_and(f)).map(pcpt_view).collect())
    };
    let memory: Vec<PcptView> = sorted(cues.iter().filter(|c| c.hot_cue.is_none()).map(pcpt_view).collect());
    let lists = |bytes: &[u8]| -> Lists {
        let (basic, extended) = rekordcrate_lists(bytes);
        (basic.into_iter().map(|(h, v)| (h, sorted(v))).collect(), sorted(extended))
    };

    let (basic, extended) = lists(&write(&dat, CueFile::Dat, &cues));
    assert_eq!(basic, [(true, hot(&|h| h < 3)), (false, memory.clone())]);
    assert!(extended.is_empty());
    let (basic, _) = lists(&write(&dat, CueFile::DatAlone, &cues));
    assert_eq!(basic, [(true, hot(&|_| true)), (false, memory)]);
    let (basic, extended) = lists(&write(&ext, CueFile::Ext, &cues));
    assert_eq!(basic, [(true, hot(&|h| h >= 3)), (false, Vec::new())]);
    assert_eq!(extended, sorted(cues.iter().map(|c| (pcp2_view(c), c.color_id)).collect::<Vec<_>>()));
}

/// The older reader (`anlz::cues`, used for display and XML export) sees what
/// `read_cue_points` sees: from the `.EXT`'s extended lists when it has cues, else the `.DAT`.
#[hegel::test]
fn legacy_reader_agrees_with_read_cue_points(tc: TestCase) {
    let (dat, ext) = base_pair(&tc);
    let cues = cue_list(&tc, false);
    let view = |c: &anlz::Cue| (c.hot_cue, c.start_ms, c.end_ms, c.comment.clone(), c.color);
    let expect = |c: &CuePoint, named: bool| {
        let color = (named && c.color_rgb != [0; 3]).then_some(c.color_rgb);
        (c.hot_cue, c.start_ms, c.end_ms, if named { c.comment.clone() } else { String::new() }, color)
    };

    let (dat2, ext2) = (write(&dat, CueFile::Dat, &cues), write(&ext, CueFile::Ext, &cues));
    let mut shown = anlz::cues(&ext2).unwrap();
    if shown.is_empty() {
        shown = anlz::cues(&dat2).unwrap();
    }
    let read = anlz::read_cue_points(&dat2, Some(&ext2)).unwrap();
    assert_eq!(
        sorted(shown.iter().map(view).collect::<Vec<_>>()),
        sorted(read.iter().map(|c| expect(c, true)).collect::<Vec<_>>())
    );

    let alone = write(&dat, CueFile::DatAlone, &cues);
    let shown: Vec<_> = sorted(anlz::cues(&alone).unwrap().iter().map(view).collect());
    let read = anlz::read_cue_points(&alone, None).unwrap();
    assert_eq!(shown, sorted(read.iter().map(|c| expect(c, false)).collect::<Vec<_>>()));
}

/// Both readers give a `PCPT` entry's hot cue number (0 = memory cue, 1 = A, ...) the same
/// meaning, whatever number a file holds. (257 used to read as hot cue A in `anlz::cues`.)
#[hegel::test]
#[hegel::explicit_test_case(number = 257u32)]
fn readers_agree_on_any_hot_cue_number(tc: TestCase) {
    let number = tc.draw(hegel::one_of!(gs::integers::<u32>().max_value(300), gs::integers::<u32>()));
    let base = common::synthetic_anlz("/Contents/a.mp3");
    let mut f = write(&base, CueFile::DatAlone, &[CuePoint { hot_cue: Some(0), start_ms: 500, ..Default::default() }]);
    let at = f.windows(4).position(|w| w == b"PCPT").unwrap();
    f[at + 12..at + 16].copy_from_slice(&number.to_be_bytes());
    let legacy: Vec<Option<u8>> = anlz::cues(&f).unwrap().iter().map(|c| c.hot_cue).collect();
    let full: Vec<Option<u8>> = anlz::read_cue_points(&f, None).unwrap().iter().map(|c| c.hot_cue).collect();
    assert_eq!(legacy, full, "hot cue number {number}");
}

/// Listing any hot cue the files can hold (numbers 1-256) gives it a label. `HotCue::letter`
/// (also used for the mix export's labels) computed `b'A' + slot`, which overflowed from slot
/// 191: a panic in debug builds, a NUL in release.
#[hegel::test]
#[hegel::explicit_test_case(number = 200u32)]
fn every_hot_cue_read_can_be_listed(tc: TestCase) {
    let number = tc.draw(gs::integers::<u32>().min_value(1).max_value(256));
    let base = common::synthetic_anlz("/Contents/a.mp3");
    let mut f = write(&base, CueFile::DatAlone, &[CuePoint { hot_cue: Some(0), start_ms: 500, ..Default::default() }]);
    let at = f.windows(4).position(|w| w == b"PCPT").unwrap();
    f[at + 12..at + 16].copy_from_slice(&number.to_be_bytes());
    for c in anlz::read_cue_points(&f, None).unwrap() {
        let letter = locallibrary::cue_edit::HotCue(c.hot_cue.unwrap()).letter();
        assert_ne!(letter, '\0', "hot cue {number}");
    }
}

/// The `PPTH` path reads back as written, and nothing else in the file changes.
#[hegel::test]
fn rewritten_path_reads_back(tc: TestCase) {
    let which = tc.draw(gs::integers::<usize>().max_value(3 * ANALYSED.len()));
    let base = match real_files().get(which / 3) {
        Some((dat, ext, ex2)) => [dat, ext, ex2][which % 3].clone(),
        None => common::synthetic_anlz("/Contents/a.mp3"),
    };
    let path = tc.draw(gs::text().max_size(200).exclude_characters("\0"));
    let out = anlz::rewrite_path(&base, &path).unwrap();
    assert_eq!(anlz::read_path(&out).unwrap().as_deref(), Some(path.as_str()));
    assert_eq!(u32::from_be_bytes(out[8..12].try_into().unwrap()) as usize, out.len());
    assert_eq!(anlz::rewrite_path(&out, &path).unwrap(), out, "rewriting the same path again changes nothing");
    let others = |b: &[u8]| {
        format!(
            "{:?} {:?} {:?} {:?} {:?} {:?} {:?}",
            anlz::cues(b).unwrap(),
            anlz::read_cue_points(b, None).unwrap(),
            anlz::beat_grid(b).unwrap(),
            anlz::preview(b).unwrap(),
            anlz::detail(b).unwrap(),
            anlz::song_structure(b).unwrap(),
            b.len() as i64 - anlz::read_path(b).unwrap().unwrap().encode_utf16().count() as i64 * 2,
        )
    };
    assert_eq!(others(&out), others(&base));
    let original = anlz::read_path(&base).unwrap().unwrap();
    assert_eq!(anlz::rewrite_path(&out, &original).unwrap(), base);
}

/// Start offsets of the sections (and the entries of cue lists) of a well-formed file, for
/// aiming damage at length and count fields.
fn structure_offsets(bytes: &[u8]) -> Vec<usize> {
    let u32_at = |o: usize| bytes.get(o..o + 4).map(|s| u32::from_be_bytes(s.try_into().unwrap()) as usize);
    let mut out = vec![0];
    let mut pos = u32_at(4).unwrap_or(0);
    while let (Some(h), Some(t)) = (u32_at(pos + 4), u32_at(pos + 8)) {
        if t < 12 || h > t || pos + t > bytes.len() {
            break;
        }
        out.push(pos);
        if matches!(&bytes[pos..pos + 4], b"PCOB" | b"PCO2") {
            let mut e = pos + h;
            while let Some(len) = u32_at(e + 8).filter(|&l| l >= 12 && e + l <= pos + t) {
                out.push(e);
                e += len;
            }
        }
        pos += t;
    }
    out
}

/// Every reader and the writer return (an error or a result) on damaged files rather than
/// panicking: real files with bytes flipped, length and count fields overwritten (mostly in
/// section and entry headers) and tails cut off. The writer never reports that it disturbed
/// the sections it doesn't own.
#[hegel::test(test_cases = 300)]
fn readers_survive_damaged_files(tc: TestCase) {
    let which = tc.draw(gs::integers::<usize>().max_value(3 * ANALYSED.len()));
    let (mut f, cues_written) = match real_files().get(which / 3) {
        Some((dat, ext, ex2)) => ([dat, ext, ex2][which % 3].clone(), false),
        None => {
            let base = common::synthetic_anlz("/Contents/a.mp3");
            let cues = cue_list(&tc, false);
            let file = tc.draw(gs::sampled_from(vec![0u8, 1, 2]));
            (write(&base, [CueFile::Dat, CueFile::DatAlone, CueFile::Ext][usize::from(file)], &cues), true)
        }
    };
    tc.note(&format!("{} bytes, synthetic cues: {cues_written}", f.len()));
    let targets = structure_offsets(&f);
    let damage = tc.draw(gs::integers::<usize>().min_value(1).max_value(6));
    for _ in 0..damage {
        if f.len() < 4 {
            break;
        }
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => {
                let i = tc.draw(gs::integers::<usize>().max_value(f.len() - 1));
                f[i] = tc.draw(gs::integers::<u8>());
            }
            1 => {
                let base = tc.draw(gs::sampled_from(targets.clone()));
                let field = tc.draw(gs::integers::<usize>().max_value(11));
                let at = (base + 4 * field).min(f.len() - 4);
                let value = tc.draw(hegel::one_of!(
                    gs::sampled_from(vec![0u32, 1, 2, 3, 4, 11, 12, 16, 0x18, 0x7fff_ffff, u32::MAX]),
                    gs::integers::<u32>()
                ));
                f[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            2 => {
                let len = tc.draw(gs::integers::<usize>().max_value(f.len()));
                f.truncate(len);
            }
            _ => {
                // Make the declared file length match a truncation, so parsing gets further.
                let len = f.len() as u32;
                if f.len() >= 12 {
                    f[8..12].copy_from_slice(&len.to_be_bytes());
                }
            }
        }
    }

    let _ = anlz::read_path(&f);
    let _ = anlz::rewrite_path(&f, "/Contents/b.mp3");
    let _ = anlz::beat_grid(&f);
    let _ = anlz::cues(&f);
    let _ = anlz::preview(&f);
    let _ = anlz::detail(&f);
    let _ = anlz::song_structure(&f);
    let _ = anlz::read_cue_points(&common::synthetic_anlz("/x"), Some(&f));
    if let Ok(cues) = anlz::read_cue_points(&f, None) {
        for file in [CueFile::Dat, CueFile::DatAlone, CueFile::Ext] {
            match anlz::write_cue_points(&f, file, &cues) {
                Ok(out) => {
                    let _ = anlz::read_cue_points(&out, None).unwrap();
                }
                Err(e) => assert!(!e.to_string().contains("disturbed"), "{file:?}: {e}"),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Moving a track's times
// ---------------------------------------------------------------------------------------------

/// One of the demo's analysis files, or a synthetic one with cues.
#[hegel::test_helper]
fn any_analysis_file(tc: &TestCase) -> Vec<u8> {
    let which = tc.draw(gs::integers::<usize>().max_value(3 * ANALYSED.len()));
    match real_files().get(which / 3) {
        Some((dat, ext, ex2)) => [dat, ext, ex2][which % 3].clone(),
        None => {
            let file = [CueFile::Dat, CueFile::DatAlone, CueFile::Ext][tc.draw(gs::integers::<usize>().max_value(2))];
            write(&common::synthetic_anlz("/Contents/a.mp3"), file, &cue_list(tc, false))
        }
    }
}

fn shift(t: u32, ms: i32) -> u32 {
    (i64::from(t) + i64::from(ms)).clamp(0, i64::from(u32::MAX)) as u32
}

/// Moving a file's times by `ms` moves every beat, cue and loop end by `ms` (stopping at the
/// start and at the largest time), and the detail waveform by whole columns, keeping its length
/// with silence where it moved away from. Nothing else changes, nor does the file's size, and
/// moving by 0 changes no byte.
#[hegel::test(test_cases = 60)]
fn shifted_times_move_by_the_shift(tc: TestCase) {
    let f = any_analysis_file(&tc);
    let ms = tc.draw(hegel::one_of!(
        gs::integers::<i32>().min_value(-20_000).max_value(20_000),
        gs::sampled_from(vec![i32::MIN, i32::MAX, 0]),
    ));
    let out = anlz::shift_times(&f, ms).unwrap();
    assert_eq!(out.len(), f.len());
    assert_eq!(u32::from_be_bytes(out[8..12].try_into().unwrap()), u32::from_be_bytes(f[8..12].try_into().unwrap()));
    if ms == 0 {
        assert_eq!(out, f);
    }

    let (before, after) = (anlz::beat_grid(&f).unwrap(), anlz::beat_grid(&out).unwrap());
    assert_eq!(after.len(), before.len());
    for (b, a) in before.iter().zip(&after) {
        assert_eq!((a.beat_number, a.bpm, a.time_ms), (b.beat_number, b.bpm, shift(b.time_ms, ms)));
    }

    let (before, after) = (anlz::read_cue_points(&f, None).unwrap(), anlz::read_cue_points(&out, None).unwrap());
    // A loop squeezed to nothing against either end reads as a point cue.
    let moved: Vec<CuePoint> = before
        .iter()
        .map(|c| {
            let start_ms = shift(c.start_ms, ms);
            let end_ms = c.end_ms.map(|e| shift(e, ms)).filter(|&e| e > start_ms && e != u32::MAX);
            CuePoint { start_ms, end_ms, ..c.clone() }
        })
        .collect();
    assert_eq!(basics(&after), basics(&moved), "moved by {ms}");

    match (anlz::detail(&f).unwrap(), anlz::detail(&out).unwrap()) {
        (Some(b), Some(a)) => {
            let n = b.columns.len();
            assert_eq!((a.style, a.columns_per_sec, a.columns.len()), (b.style, b.columns_per_sec, n));
            let rate = b.columns_per_sec.unwrap();
            let by = ((f64::from(ms) * rate / 1000.0).round() as i64).clamp(-(n as i64), n as i64);
            for (i, col) in a.columns.iter().enumerate() {
                let from = i as i64 - by;
                match usize::try_from(from).ok().and_then(|j| b.columns.get(j)) {
                    Some(old) => assert_eq!(col, old, "column {i}, moved {by}"),
                    None => assert_eq!(col.back.height, 0.0, "column {i} is silent"),
                }
            }
        }
        (None, None) => {}
        other => panic!("the detail waveform came or went: {other:?}"),
    }
    let others = |b: &[u8]| {
        let (path, preview, phrases) =
            (anlz::read_path(b).unwrap(), anlz::preview(b).unwrap(), anlz::song_structure(b).unwrap());
        format!("{path:?} {preview:?} {phrases:?}")
    };
    assert_eq!(others(&out), others(&f));
}

/// Moving the times of a damaged file returns (a result or an error) without panicking. What it
/// returns keeps the file's size and can be moved again, and moving by nothing changes nothing.
/// (Moving back needn't give the file again: times stop at 0 and waveform columns moved past
/// either end are gone.)
#[hegel::test(test_cases = 200)]
fn shifting_survives_damaged_files(tc: TestCase) {
    let mut f = any_analysis_file(&tc);
    let targets = structure_offsets(&f);
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        if f.len() < 4 {
            break;
        }
        if tc.draw(gs::booleans()) {
            let base = tc.draw(gs::sampled_from(targets.clone()));
            let at = (base + 4 * tc.draw(gs::integers::<usize>().max_value(11))).min(f.len() - 4);
            let value = tc.draw(gs::sampled_from(vec![0u32, 1, 12, 16, 0x7fff_ffff, u32::MAX]));
            f[at..at + 4].copy_from_slice(&value.to_be_bytes());
        } else {
            f.truncate(tc.draw(gs::integers::<usize>().max_value(f.len())));
        }
    }
    let ms = tc.draw(gs::integers::<i32>());
    if let Ok(out) = anlz::shift_times(&f, ms) {
        assert_eq!(out.len(), f.len());
        assert!(anlz::shift_times(&out, ms.saturating_neg()).is_ok());
        assert_eq!(anlz::shift_times(&f, 0).unwrap(), f);
    }
}

/// More cues than a list can count, or a name longer than an entry can hold, are refused with
/// an error, whichever file they go to.
#[hegel::test(test_cases = 10)]
fn cue_lists_too_big_for_the_file_are_refused(tc: TestCase) {
    let base = common::synthetic_anlz("/Contents/a.mp3");
    let file = [CueFile::Dat, CueFile::DatAlone, CueFile::Ext][tc.draw(gs::integers::<usize>().max_value(2))];
    let over = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let (cues, why) = if tc.draw(gs::booleans()) {
        (vec![CuePoint::default(); usize::from(u16::MAX) + over], "too many cues")
    } else {
        (vec![CuePoint { comment: "x".repeat(0x10000 + over), ..Default::default() }], "too long")
    };
    let err = anlz::write_cue_points(&base, file, &cues).unwrap_err();
    assert!(err.to_string().contains(why), "{err}");
}

// ---------------------------------------------------------------------------------------------
// Entries the writer doesn't make
// ---------------------------------------------------------------------------------------------

fn be_at(b: &[u8], at: usize) -> usize {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as usize
}

/// The sections of a well-formed file, each as its bytes.
fn section_bytes(b: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut pos = be_at(b, 4);
    while pos < be_at(b, 8) {
        let len = be_at(b, pos + 8);
        out.push(b[pos..pos + len].to_vec());
        pos += len;
    }
    out
}

fn is_cue_list(s: &[u8]) -> bool {
    matches!(&s[..4], b"PCOB" | b"PCO2")
}

/// A cue list section's header and entries.
fn split_list(s: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let header = be_at(s, 4);
    let mut entries = Vec::new();
    let mut e = header;
    while e < s.len() {
        let len = be_at(s, e + 8);
        entries.push(s[e..e + len].to_vec());
        e += len;
    }
    (s[..header].to_vec(), entries)
}

/// Bytes with their big-endian length field at 8 set to how long they are.
fn sized(mut b: Vec<u8>) -> Vec<u8> {
    let len = b.len() as u32;
    b[8..12].copy_from_slice(&len.to_be_bytes());
    b
}

/// A file with `file`'s header and `sections`.
fn assemble(file: &[u8], sections: &[Vec<u8>]) -> Vec<u8> {
    let mut out = file[..be_at(file, 4)].to_vec();
    out.extend(sections.concat());
    sized(out)
}

/// An entry of a kind no rekordbox writes (neither `PCPT` nor `PCP2`) in a cue list is skipped
/// by the reader, left as it is when times move (the cues around it move), and gone once the
/// list is written again, the cues reading back as before.
#[hegel::test(test_cases = 60)]
fn foreign_entries_in_cue_lists_are_skipped(tc: TestCase) {
    let file = [CueFile::DatAlone, CueFile::Ext][tc.draw(gs::integers::<usize>().max_value(1))];
    let written = write(&common::synthetic_anlz("/Contents/a.mp3"), file, &cue_list(&tc, false));
    let tag = tc.draw(gs::arrays::<_, u8, 4>(gs::integers::<u8>()).filter(|t| t != b"PCPT" && t != b"PCP2"));
    let body = tc.draw(gs::binary().max_size(24));
    let foreign = sized([&tag[..], &12u32.to_be_bytes(), &[0; 4], &body].concat());
    let sections: Vec<Vec<u8>> = section_bytes(&written)
        .into_iter()
        .map(|s| {
            if !is_cue_list(&s) {
                return s;
            }
            let (header, mut entries) = split_list(&s);
            let at = tc.draw(gs::integers::<usize>().max_value(entries.len()));
            entries.insert(at, foreign.clone());
            sized([header, entries.concat()].concat())
        })
        .collect();
    let odd = assemble(&written, &sections);
    let cues = anlz::read_cue_points(&written, None).unwrap();
    assert_eq!(anlz::read_cue_points(&odd, None).unwrap(), cues);

    let ms = tc.draw(gs::integers::<i32>().min_value(-5_000).max_value(5_000));
    let shifted = anlz::shift_times(&odd, ms).unwrap();
    let foreign_entries = |b: &[u8]| -> Vec<Vec<u8>> {
        section_bytes(b)
            .iter()
            .filter(|s| is_cue_list(s))
            .flat_map(|s| split_list(s).1)
            .filter(|e| e[..4] == tag)
            .collect()
    };
    assert_eq!(foreign_entries(&shifted), foreign_entries(&odd));
    let moved = |b: &[u8]| basics(&anlz::read_cue_points(&anlz::shift_times(b, ms).unwrap(), None).unwrap());
    assert_eq!(moved(&odd), moved(&written));

    let rewritten = write(&odd, file, &cues);
    assert_eq!(foreign_entries(&rewritten), Vec::<Vec<u8>>::new());
    let back = anlz::read_cue_points(&rewritten, None).unwrap();
    assert!(same_cues(&back, &cues), "wrote {}\nread {}", show(&cues), show(&back));
}

/// A `PCP2` entry ending right after its comment, as older exports wrote them (an empty comment
/// sometimes stored as a lone NUL), reads back as an uncoloured cue. Rewriting its list for
/// another cue keeps the entry byte for byte, and only giving it a colour makes it longer: by
/// the four colour bytes, its comment kept as stored.
#[hegel::test(test_cases = 60)]
fn old_short_cue_entries_grow_only_for_a_colour(tc: TestCase) {
    let hot = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
    let time = tc.draw(gs::integers::<u32>().max_value(600_000));
    let name = tc.draw(gs::text().max_size(8).exclude_characters("\0"));
    let lone_nul = name.is_empty() && tc.draw(gs::booleans());
    let comment: Vec<u8> = if lone_nul || !name.is_empty() {
        name.encode_utf16().chain([0]).flat_map(u16::to_be_bytes).collect()
    } else {
        Vec::new()
    };
    let mut entry = vec![0u8; 0x2c];
    entry[..4].copy_from_slice(b"PCP2");
    entry[4..8].copy_from_slice(&0x10u32.to_be_bytes());
    entry[0x0c..0x10].copy_from_slice(&hot.to_be_bytes());
    entry[0x10] = 1;
    entry[0x12..0x14].copy_from_slice(&0x03e8u16.to_be_bytes());
    entry[0x14..0x18].copy_from_slice(&time.to_be_bytes());
    entry[0x18..0x1c].copy_from_slice(&u32::MAX.to_be_bytes());
    entry[0x1c] = tc.draw(gs::integers::<u8>().max_value(8));
    entry[0x28..0x2c].copy_from_slice(&(comment.len() as u32).to_be_bytes());
    entry.extend(&comment);
    let entry = sized(entry);
    let mut list = b"PCO2".to_vec();
    list.extend(0x14u32.to_be_bytes());
    list.extend([0; 4]);
    list.extend(1u32.to_be_bytes());
    list.extend([0, 1, 0, 0]);
    list.extend(&entry);
    let ext = assemble(&common::synthetic_anlz("/Contents/a.mp3"), &[sized(list)]);

    let mut cues = anlz::read_cue_points(&ext, None).unwrap();
    assert_eq!(cues.len(), 1);
    let c = &cues[0];
    assert_eq!((c.hot_cue, c.start_ms, c.end_ms, c.comment.as_str()), (Some(hot as u8 - 1), time, None, name.as_str()));
    assert_eq!((c.color_code, c.color_rgb), (0, [0; 3]));
    let entries_of = |b: &[u8]| -> Vec<Vec<u8>> {
        section_bytes(b)
            .iter()
            .filter(|s| &s[..4] == b"PCO2" && be_at(s, 12) == 1)
            .flat_map(|s| split_list(s).1)
            .collect()
    };
    // Its slot's entry, found by hot cue number.
    let ours = |b: &[u8]| entries_of(b).into_iter().find(|e| be_at(e, 0x0c) == hot as usize).unwrap();

    let other = (hot % 8) as u8 + 8;
    cues.push(CuePoint { hot_cue: Some(other), start_ms: 1, ..Default::default() });
    let grown = write(&ext, CueFile::Ext, &cues);
    assert_eq!(entries_of(&grown).len(), 2);
    assert_eq!(ours(&grown), entry);

    cues[0].color_code = tc.draw(gs::integers::<u8>().min_value(1).max_value(62));
    cues[0].color_rgb = tc.draw(gs::arrays::<_, u8, 3>(gs::integers::<u8>()));
    let coloured = write(&grown, CueFile::Ext, &cues);
    let new = ours(&coloured);
    assert_eq!(new.len(), entry.len() + 4);
    assert_eq!(new[0x28..entry.len()], entry[0x28..]);
    let back = anlz::read_cue_points(&coloured, None).unwrap();
    let c = back.iter().find(|c| c.hot_cue == Some(hot as u8 - 1)).unwrap();
    assert_eq!((c.color_code, c.color_rgb, c.comment.as_str()), (cues[0].color_code, cues[0].color_rgb, name.as_str()));
}
