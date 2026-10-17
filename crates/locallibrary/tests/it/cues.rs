//! Editing cue points: the analysis file writer against real rekordbox files and rekordcrate's
//! independent parser, OneLibrary's cue rows, and the plan/apply pipeline with undo.

mod refusals;

use crate::common;

use std::fs;
use std::io::Cursor;
use std::path::Path;

use binrw::BinRead;
use locallibrary::anlz::{self, CueFile, CuePoint};
use locallibrary::api::{self, CueOp, EditCues, ListCues, TrackSelector};
use locallibrary::cue_edit::{CuePosition, CueRef, HotCue, SetCue, UpdateCue};
use locallibrary::onelibrary::{self, OneLibrary};
use locallibrary::progress::Ctx;
use locallibrary::{Device, Error, backup, paths};

const ANALYSED: [&str; 2] = ["P016/0000875E", "P053/0001D21F"];

fn fixture_analysis(dir: &str) -> (Vec<u8>, Vec<u8>) {
    let d = common::fixtures().join("rekordcrate/demo_tracks/PIONEER/USBANLZ").join(dir);
    (fs::read(d.join("ANLZ0000.DAT")).unwrap(), fs::read(d.join("ANLZ0000.EXT")).unwrap())
}

/// One of each kind of cue.
fn sample() -> Vec<CuePoint> {
    vec![
        CuePoint { start_ms: 25, comment: "Intro".into(), color_id: 6, ..Default::default() },
        CuePoint {
            start_ms: 30_025,
            end_ms: Some(33_775),
            loop_beats: Some((8, 1)),
            active_loop: true,
            ..Default::default()
        },
        CuePoint {
            hot_cue: Some(0),
            start_ms: 15_025,
            comment: "Vocal ♪".into(),
            color_code: 0x2a,
            color_rgb: [0xe6, 0x28, 0x28],
            ..Default::default()
        },
        CuePoint {
            hot_cue: Some(4),
            start_ms: 60_025,
            end_ms: Some(60_259),
            loop_beats: Some((1, 2)),
            color_rgb: anlz::DEFAULT_HOT_CUE_RGB,
            ..Default::default()
        },
    ]
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

#[test]
fn rewrites_real_analysis_files_byte_for_byte() {
    for dir in ANALYSED {
        let (dat, ext) = fixture_analysis(dir);
        // rekordbox exported these without cues: empty lists in both files.
        let cues = anlz::read_cue_points(&dat, Some(&ext)).unwrap();
        assert!(cues.is_empty());
        assert_eq!(anlz::write_cue_points(&dat, CueFile::Dat, &cues).unwrap(), dat);
        assert_eq!(anlz::write_cue_points(&ext, CueFile::Ext, &cues).unwrap(), ext);

        let dat2 = anlz::write_cue_points(&dat, CueFile::Dat, &sample()).unwrap();
        let ext2 = anlz::write_cue_points(&ext, CueFile::Ext, &sample()).unwrap();
        assert_eq!(non_cue_content(&dat2), non_cue_content(&dat));
        assert_eq!(non_cue_content(&ext2), non_cue_content(&ext));
        let back = anlz::read_cue_points(&dat2, Some(&ext2)).unwrap();
        assert_eq!(back.len(), 4);
        for (a, b) in back.iter().zip(sample().iter()) {
            assert!(a.same_as(b), "{a:?} vs {b:?}");
        }
        // What we wrote is written back unchanged, and clearing it restores rekordbox's files.
        assert_eq!(anlz::write_cue_points(&dat2, CueFile::Dat, &back).unwrap(), dat2);
        assert_eq!(anlz::write_cue_points(&ext2, CueFile::Ext, &back).unwrap(), ext2);
        assert_eq!(anlz::write_cue_points(&dat2, CueFile::Dat, &[]).unwrap(), dat);
        assert_eq!(anlz::write_cue_points(&ext2, CueFile::Ext, &[]).unwrap(), ext);

        // The existing reader (player, XML export) sees them too.
        let shown = anlz::cues(&ext2).unwrap();
        assert_eq!(shown.len(), 4);
        assert_eq!(
            (shown[2].hot_cue, shown[2].comment.as_str(), shown[2].color),
            (Some(0), "Vocal ♪", Some([0xe6, 0x28, 0x28]))
        );
        assert_eq!(anlz::cues(&dat2).unwrap().len(), 3, "the .DAT has hot cues A-C and the memory cues");
    }
}

/// rekordcrate, an independent parser, on what we write. It models a point cue's type as 0 where
/// rekordbox writes 1 (crate-digger's docs and real exports agree), and expects 20 bytes after a
/// `PCP2` entry's colour where rekordbox 7 writes 40, so loops are checked, and the `.EXT`'s
/// entries one at a time inside sections framed by rekordcrate's own header parser.
#[test]
fn rekordcrate_reads_what_is_written() {
    use rekordcrate::anlz::{ANLZ, Content, ContentKind, CueList, CueListType, CueType, ExtendedCue, Header};
    use rekordcrate::util::ColorIndex;

    let (dat, ext) = fixture_analysis(ANALYSED[0]);
    let cues = vec![
        CuePoint {
            hot_cue: Some(0),
            start_ms: 1_000,
            end_ms: Some(2_875),
            loop_beats: Some((4, 1)),
            comment: "Build".into(),
            color_code: 0x2a,
            color_rgb: [0xe6, 0x28, 0x28],
            ..Default::default()
        },
        CuePoint {
            hot_cue: Some(5),
            start_ms: 60_025,
            end_ms: Some(63_775),
            comment: "Drop".into(),
            ..Default::default()
        },
        CuePoint {
            start_ms: 90_000,
            end_ms: Some(91_000),
            comment: "Outro".into(),
            color_id: 3,
            active_loop: true,
            ..Default::default()
        },
    ];
    let dat2 = anlz::write_cue_points(&dat, CueFile::Dat, &cues).unwrap();
    let ext2 = anlz::write_cue_points(&ext, CueFile::Ext, &cues).unwrap();

    // The whole .DAT.
    let file = ANLZ::read(&mut Cursor::new(&dat2)).unwrap();
    assert_eq!(file.header.total_size as usize, dat2.len());
    let mut lists = Vec::new();
    for s in &file.sections {
        if let Content::CueList(l) = &s.content {
            let entries: Vec<(u32, u32, u32, u32, u16, u16)> = l
                .cues
                .iter()
                .map(|c| (c.hot_cue, c.status, c.time, c.loop_time, c.order_first, c.order_last))
                .collect();
            assert!(l.cues.iter().all(|c| c.cue_type == CueType::Loop));
            lists.push((l.list_type == CueListType::HotCues, entries));
        }
    }
    assert_eq!(
        lists,
        [(true, vec![(1, 0, 1_000, 2_875, 0xffff, 0xffff)]), (false, vec![(0, 4, 90_000, 91_000, 0xffff, 0xffff)])]
    );

    // The .EXT, section by section.
    let mut pos = u32::from_be_bytes(ext2[4..8].try_into().unwrap()) as usize;
    let (mut basic, mut extended) = (Vec::new(), Vec::new());
    while pos < ext2.len() {
        let h = Header::read_be(&mut Cursor::new(&ext2[pos..])).unwrap();
        match h.kind {
            ContentKind::CueList => {
                let l = CueList::read_be(&mut Cursor::new(&ext2[pos + 12..])).unwrap();
                basic.extend(l.cues.iter().map(|c| (c.hot_cue, c.time, c.loop_time)));
            }
            ContentKind::ExtendedCueList => {
                let mut e = pos + h.size as usize;
                while e < pos + h.total_size as usize {
                    let eh = Header::read_be(&mut Cursor::new(&ext2[e..])).unwrap();
                    assert_eq!(eh.kind, ContentKind::ExtendedCue);
                    let c = ExtendedCue::read_be(&mut Cursor::new(&ext2[e..])).unwrap();
                    assert_eq!(c.cue_type, CueType::Loop);
                    extended.push((
                        c.hot_cue,
                        c.time,
                        c.loop_time,
                        String::from_utf16_lossy(&c.comment.0),
                        c.color.clone(),
                        c.hot_cue_color_index,
                        c.hot_cue_color_rgb,
                        (c.loop_numerator, c.loop_denominator),
                    ));
                    e += eh.total_size as usize;
                }
            }
            _ => {}
        }
        pos += h.total_size as usize;
    }
    assert_eq!(pos, u32::from_be_bytes(ext2[8..12].try_into().unwrap()) as usize);
    assert_eq!(pos, ext2.len());
    assert_eq!(basic, [(6, 60_025, 63_775)], "only hot cues D-H in the .EXT's PCOB");
    assert_eq!(
        extended,
        [
            (1, 1_000, 2_875, "Build".into(), ColorIndex::None, 0x2a, (0xe6, 0x28, 0x28), (4, 1)),
            (6, 60_025, 63_775, "Drop".into(), ColorIndex::None, 0, (0, 0, 0), (0, 0)),
            (0, 90_000, 91_000, "Outro".into(), ColorIndex::Orange, 0, (0, 0, 0), (0, 0)),
        ]
    );
}

fn drive_arg(root: &Path) -> String {
    root.display().to_string()
}

fn edit(root: &Path, ids: &[u32], op: CueOp) -> EditCues {
    EditCues { drive: drive_arg(root), tracks: TrackSelector { ids: ids.to_vec(), ..Default::default() }, edit: op }
}

fn set(hot: Option<char>, at: CuePosition) -> SetCue {
    SetCue {
        hot_cue: hot.map(|c| c.to_string().parse().unwrap()),
        at,
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    }
}

fn apply(root: &Path, ids: &[u32], op: CueOp) -> locallibrary::cue_edit::CueReport {
    api::cues_apply(&edit(root, ids, op), &Ctx::silent()).unwrap()
}

fn listed(root: &Path, id: u32) -> Vec<locallibrary::cue_edit::CueInfo> {
    let req =
        ListCues { library: drive_arg(root).into(), tracks: TrackSelector { ids: vec![id], ..Default::default() } };
    api::cues_list(&req).unwrap().remove(0).cues
}

fn analysis_files(root: &Path) -> Vec<(String, Vec<u8>)> {
    common::library_files(root).into_iter().filter(|(k, _)| k.starts_with("PIONEER/USBANLZ/")).collect()
}

#[test]
fn sets_renames_recolours_and_deletes_cues_with_undo() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let original = analysis_files(&root);

    // Hot cue B at 1:02.5 as an 8-beat loop: track 1 is 128 BPM, so the loop is 3.75 s.
    let b = SetCue {
        loop_beats: Some(8.0),
        name: Some("Drop".into()),
        color: Some("red".into()),
        ..set(Some('B'), CuePosition::Ms(62_500))
    };
    let plan = api::cues_plan(&edit(&root, &[1], CueOp::Set(b)), &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks.len(), 1);
    let t = &plan.tracks[0];
    assert!(t.before.is_empty());
    assert_eq!(t.files, ["PIONEER/USBANLZ/P016/0000875E/ANLZ0000.DAT", "PIONEER/USBANLZ/P016/0000875E/ANLZ0000.EXT"]);
    assert!(!t.onelibrary);
    let report = locallibrary::cue_edit::apply(&plan, &mut Device::open(&root).unwrap(), &Ctx::silent()).unwrap();
    assert_eq!(report.files_written.len(), 2);
    assert!(report.backup.is_some() && report.fingerprint.is_some());
    let cues = listed(&root, 1);
    assert_eq!(cues.len(), 1);
    assert_eq!((cues[0].hot_cue.as_deref(), cues[0].start_ms, cues[0].end_ms), (Some("B"), 62_500, Some(66_250)));
    assert_eq!(
        (cues[0].loop_beats.as_deref(), cues[0].name.as_str(), cues[0].color.as_deref()),
        (Some("8"), "Drop", Some("#e62828"))
    );

    // A memory cue on beat 129 sits exactly on the grid.
    apply(&root, &[1], CueOp::Set(SetCue { color: Some("green".into()), ..set(None, CuePosition::Beat(129)) }));
    let cues = listed(&root, 1);
    assert_eq!((cues[0].hot_cue.clone(), cues[0].start_ms, cues[0].beat), (None, 60_025, Some(129)));
    assert_eq!(cues[0].color.as_deref(), Some("green"));
    // Snapping a time to the grid, as hot cue A.
    let d = Device::open(&root).unwrap();
    let beats = anlz::load_grid(&d, d.library().unwrap().track(1).unwrap()).beats;
    let near = CuePosition::Ms(beats[2].time_ms + 40);
    apply(&root, &[1], CueOp::Set(SetCue { snap: true, ..set(Some('a'), near) }));
    let a = &listed(&root, 1)[1];
    assert_eq!((a.hot_cue.as_deref(), a.start_ms, a.beat), (Some("A"), beats[2].time_ms, Some(3)));

    // Rename and recolour B; the player's own reader sees it.
    apply(
        &root,
        &[1],
        CueOp::Update(UpdateCue {
            cue: CueRef::Hot(HotCue(1)),
            name: Some("Big drop".into()),
            color: Some("#00e0ff".into()),
        }),
    );
    let d = Device::open(&root).unwrap();
    let track = d.library().unwrap().track(1).unwrap().clone();
    let shown = anlz::load(&d, &track).cues;
    let b = shown.iter().find(|c| c.hot_cue == Some(1)).unwrap();
    assert_eq!((b.comment.as_str(), b.color, b.end_ms), ("Big drop", Some([0, 0xe0, 0xff]), Some(66_250)));
    // Moving B keeps its name; the memory cue is found by its position.
    apply(&root, &[1], CueOp::Set(set(Some('B'), CuePosition::Beat(200))));
    assert_eq!(listed(&root, 1)[2].name, "Big drop");
    assert_eq!(listed(&root, 1)[2].end_ms, None);
    apply(
        &root,
        &[1],
        CueOp::Update(UpdateCue {
            cue: CueRef::Memory(CuePosition::Ms(60_000)),
            name: Some("Break".into()),
            color: None,
        }),
    );
    assert_eq!(listed(&root, 1)[0].name, "Break");

    // Delete A; nothing to delete is an error, an unchanged edit a no-op.
    let before_delete = analysis_files(&root);
    apply(&root, &[1], CueOp::Delete { cue: CueRef::Hot(HotCue(0)) });
    assert_eq!(listed(&root, 1).len(), 2);
    let missing = api::cues_plan(&edit(&root, &[1], CueOp::Delete { cue: CueRef::Hot(HotCue(0)) }), &Ctx::silent());
    assert!(matches!(missing, Err(Error::NotFound(_))));
    let same = SetCue { name: Some("Break".into()), ..set(None, CuePosition::Beat(129)) };
    assert!(api::cues_plan(&edit(&root, &[1], CueOp::Set(same)), &Ctx::silent()).unwrap().is_noop());
    // Out of range.
    let late = api::cues_plan(&edit(&root, &[1], CueOp::Set(set(None, CuePosition::Ms(200_000)))), &Ctx::silent());
    assert!(matches!(late, Err(Error::Invalid(_))));

    // Undo the delete, then everything.
    let mut d = Device::open(&root).unwrap();
    backup::restore(&mut d, None, false).unwrap();
    assert_eq!(analysis_files(&root), before_delete);
    let oldest = backup::list(&d).unwrap().pop().unwrap();
    backup::restore(&mut d, Some(&oldest.id), false).unwrap();
    assert_eq!(analysis_files(&root), original);
}

#[test]
fn a_stale_plan_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let plan =
        api::cues_plan(&edit(&root, &[2], CueOp::Set(set(Some('C'), CuePosition::Beat(9)))), &Ctx::silent()).unwrap();
    apply(&root, &[2], CueOp::Set(set(None, CuePosition::Beat(17))));
    let err = locallibrary::cue_edit::apply(&plan, &mut Device::open(&root).unwrap(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
}

#[test]
fn copies_cues_between_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    apply(&root, &[1], CueOp::Set(SetCue { name: Some("In".into()), ..set(Some('A'), CuePosition::Beat(1)) }));
    apply(&root, &[1], CueOp::Set(SetCue { loop_beats: Some(4.0), ..set(None, CuePosition::Beat(33)) }));
    apply(&root, &[2], CueOp::Set(set(Some('H'), CuePosition::Beat(5))));
    let from = TrackSelector { ids: vec![1], ..Default::default() };
    let plan = api::cues_plan(&edit(&root, &[2], CueOp::Copy { from: from.clone() }), &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks[0].before.len(), 1);
    assert_eq!(plan.tracks[0].after.len(), 2);
    apply(&root, &[2], CueOp::Copy { from });
    let (one, two) = (listed(&root, 1), listed(&root, 2));
    let strip = |l: &[locallibrary::cue_edit::CueInfo]| -> Vec<_> {
        l.iter().map(|c| (c.hot_cue.clone(), c.start_ms, c.end_ms, c.name.clone(), c.loop_beats.clone())).collect()
    };
    assert_eq!(strip(&two), strip(&one));
    // Copying more than one source is refused.
    let both = TrackSelector { ids: vec![1, 2], ..Default::default() };
    assert!(api::cues_plan(&edit(&root, &[2], CueOp::Copy { from: both }), &Ctx::silent()).is_err());
}

#[test]
fn a_track_without_an_ext_keeps_every_cue_in_its_dat() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let ext = root.join("PIONEER/USBANLZ/P053/0001D21F/ANLZ0000.EXT");
    fs::remove_file(&ext).unwrap();
    let op = CueOp::Set(SetCue { name: Some("Late".into()), ..set(Some('G'), CuePosition::Beat(100)) });
    let plan = api::cues_plan(&edit(&root, &[2], op.clone()), &Ctx::silent()).unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("no .EXT")), "{:?}", plan.warnings);
    apply(&root, &[2], op);
    assert!(!ext.exists());
    let cues = listed(&root, 2);
    assert_eq!((cues[0].hot_cue.as_deref(), cues[0].name.as_str()), (Some("G"), ""));
}

/// Run SQL against a drive's OneLibrary database and write it back.
fn edit_onelibrary(root: &Path, sql: &str) {
    let one = OneLibrary::open_root(root).unwrap();
    one.connection().execute_batch(sql).unwrap();
    onelibrary::write(&Device::open(root).unwrap(), &one.to_encrypted().unwrap()).unwrap();
}

fn plus_rows(root: &Path, title: &str) -> Vec<(i64, i64, i64, i64, String, i64)> {
    let one = OneLibrary::open_root(root).unwrap();
    one.connection()
        .prepare(
            "SELECT c.cue_id, c.kind, c.inUsec, c.outUsec, c.cueComment, c.colorTableIndex FROM cue c
             JOIN content t ON t.content_id = c.content_id WHERE t.title = ?1 ORDER BY c.cue_id",
        )
        .unwrap()
        .query_map([title], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn bank_entries(root: &Path) -> Vec<(i64, i64)> {
    let one = OneLibrary::open_root(root).unwrap();
    one.connection()
        .prepare("SELECT cue_id, sequenceNo FROM hotCueBankList_cue ORDER BY sequenceNo")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn onelibrary_rows_follow_and_keep_their_ids() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    common::add_onelibrary(&root);
    let pristine = common::library_files(&root);

    let a = SetCue { name: Some("Start".into()), color: Some("orange".into()), ..set(Some('A'), CuePosition::Beat(1)) };
    let plan = api::cues_plan(&edit(&root, &[1], CueOp::Set(a)), &Ctx::silent()).unwrap();
    assert!(plan.tracks[0].onelibrary);
    let report = locallibrary::cue_edit::apply(&plan, &mut Device::open(&root).unwrap(), &Ctx::silent()).unwrap();
    assert_eq!(report.onelibrary.unwrap().added, 1);
    let rows = plus_rows(&root, "Demo Track 1");
    assert_eq!(rows, [(1, 1, 25_000, -1, "Start".to_string(), 0x24)]);
    let frames: i64 = OneLibrary::open_root(&root)
        .unwrap()
        .connection()
        .query_row("SELECT in150FramePerSec FROM cue WHERE cue_id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(frames, 3);

    // A hot cue bank list uses it; a stray memory cue row exists that the analysis files lack.
    edit_onelibrary(
        &root,
        "INSERT INTO hotCueBankList VALUES (1, 0, 'Bank', NULL, 0, 0);
         INSERT INTO hotCueBankList_cue VALUES (1, 1, 1);
         INSERT INTO cue (cue_id, content_id, kind, inUsec, outUsec) VALUES (5, 1, 0, 9000000, -1);",
    );

    // Renaming keeps the row (and the bank list entry); the stray row goes, with a warning.
    let op = CueOp::Update(UpdateCue { cue: CueRef::Hot(HotCue(0)), name: Some("Begin".into()), color: None });
    let plan = api::cues_plan(&edit(&root, &[1], op.clone()), &Ctx::silent()).unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("1 cue(s)")), "{:?}", plan.warnings);
    let report = apply(&root, &[1], op);
    let changes = report.onelibrary.unwrap();
    assert_eq!((changes.updated, changes.removed), (1, 1));
    assert_eq!(plus_rows(&root, "Demo Track 1"), [(1, 1, 25_000, -1, "Begin".to_string(), 0x24)]);
    assert_eq!(bank_entries(&root), [(1, 1)]);

    // A loop and a memory cue are added; deleting A drops its bank list entry.
    apply(&root, &[1], CueOp::Set(SetCue { loop_beats: Some(0.5), ..set(Some('F'), CuePosition::Beat(65)) }));
    apply(&root, &[1], CueOp::Set(set(None, CuePosition::Ms(45_000))));
    apply(&root, &[1], CueOp::Delete { cue: CueRef::Hot(HotCue(0)) });
    let rows = plus_rows(&root, "Demo Track 1");
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].1, rows[0].2, rows[0].3), (7, 30_025_000, 30_259_000), "hot cue F is kind 7");
    assert_eq!((rows[1].1, rows[1].2), (0, 45_000_000));
    assert!(bank_entries(&root).is_empty());
    let report = api::cues_list(&ListCues {
        library: drive_arg(&root).into(),
        tracks: TrackSelector { ids: vec![1], ..Default::default() },
    });
    assert_eq!(report.unwrap()[0].onelibrary_cues, Some(2));

    common::sqlcipher_reads(&root);
    // Copying onto the other track gives it rows too.
    apply(&root, &[2], CueOp::Copy { from: TrackSelector { ids: vec![1], ..Default::default() } });
    let rows = plus_rows(&root, "Demo Track 2");
    assert_eq!(rows.iter().map(|r| (r.1, r.2)).collect::<Vec<_>>(), [(0, 45_000_000), (7, 30_025_000)]);

    // Undo it all, newest edit first: both databases and every analysis file are as they were.
    let mut d = Device::open(&root).unwrap();
    for b in backup::list(&d).unwrap() {
        backup::restore(&mut d, Some(&b.id), false).unwrap();
    }
    assert!(common::library_files(&root) == pristine);
}

#[test]
fn onelibrary_only_drives_are_edited_too() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    common::onelibrary_only(&root);
    let lib = common::plus_library(&root);
    let id = lib.tracks.iter().find(|t| t.title == "Demo Track 2").unwrap().id;
    // Through the RPC's JSON shape.
    let req: EditCues = serde_json::from_value(serde_json::json!({
        "drive": drive_arg(&root),
        "query": "Demo Track 2",
        "edit": { "op": "set", "hot_cue": "D", "at": { "beat": 17 }, "loop_beats": 2, "name": "Hook", "color": "violet" }
    }))
    .unwrap();
    api::cues_apply(&req, &Ctx::silent()).unwrap();
    common::sqlcipher_reads(&root);
    let rows = plus_rows(&root, "Demo Track 2");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].1, rows[0].2, rows[0].3, rows[0].4.as_str(), rows[0].5),
        (5, 8_025_000, 9_025_000, "Hook", 0x3c)
    );
    let cues = listed(&root, id);
    assert_eq!(
        (cues[0].hot_cue.as_deref(), cues[0].loop_beats.as_deref(), cues[0].color.as_deref()),
        (Some("D"), Some("2"), Some("#aa72ff"))
    );
    let json: CueRef = serde_json::from_value(serde_json::json!({ "memory": { "ms": 1500 } })).unwrap();
    assert_eq!(json, CueRef::Memory(CuePosition::Ms(1500)));
    assert_eq!(serde_json::from_value::<CueRef>(serde_json::json!("all")).unwrap(), CueRef::All);
}

#[test]
fn needs_analysis_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let d = Device::open(&root).unwrap();
    let t = d.library().unwrap().track(2).unwrap().clone();
    fs::remove_file(d.path(&paths::key(&t.analyze_path))).unwrap();
    let r = api::cues_plan(&edit(&root, &[t.id], CueOp::Set(set(Some('A'), CuePosition::Ms(0)))), &Ctx::silent());
    assert!(matches!(&r, Err(Error::Invalid(m)) if m.contains("no analysis files")), "{r:?}");
    // Listing reports it instead of failing.
    let listed = api::cues_list(&ListCues {
        library: drive_arg(&root).into(),
        tracks: TrackSelector { ids: vec![t.id], ..Default::default() },
    });
    assert!(!listed.unwrap()[0].warnings.is_empty());
}
