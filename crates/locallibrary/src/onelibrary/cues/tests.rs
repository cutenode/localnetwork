use super::*;

fn library_with_cues() -> (OneLibrary, u32) {
    let mut one = OneLibrary::create().unwrap();
    let defaults = one.content_defaults().unwrap();
    let rec = super::super::ContentRecord::from_track(
        &loclib_model::test_util::track(1, "A", "B", "/Contents/A/B.mp3"),
        true,
        &defaults,
    );
    let t = one.add_track(&rec, "/Contents/A/B.mp3", "", None).unwrap();
    one.conn
        .execute_batch(&format!(
            "INSERT INTO cue (cue_id, content_id, kind, colorTableIndex, cueComment, isActiveLoop, beatLoopNumerator,
                     beatLoopDenominator, inUsec, outUsec, in150FramePerSec, out150FramePerSec, inMpegFrameNumber,
                     outMpegFrameNumber, inMpegAbs, outMpegAbs, inDecodingStartFramePosition,
                     outDecodingStartFramePosition, inFileOffsetInBlock, OutFileOffsetInBlock,
                     inNumberOfSampleInBlock, outNumberOfSampleInBlock)
                 VALUES (7, {t}, 0, 0, '', 0, 0, 0, 1500250, -1, 225, -1, 57, 0, 120, 0, 3, 0, 418, 0, 1152, 0),
                        (8, {t}, 2, 42, 'Drop', 0, 0, 0, 30000000, -1, 4500, -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
                 INSERT INTO hotCueBankList VALUES (1, 0, 'Bank', NULL, 0, 0);
                 INSERT INTO hotCueBankList_cue VALUES (1, 7, 1), (1, 8, 2);"
        ))
        .unwrap();
    (one, t)
}

#[test]
fn reads_rows() {
    let (one, t) = library_with_cues();
    let rows = one.cue_rows(t).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].cue_id, rows[0].hot_cue, rows[0].in_usec, rows[0].out_usec), (Some(7), None, 1_500_250, None));
    assert_eq!((rows[1].hot_cue, rows[1].comment.as_str(), rows[1].color), (Some(1), "Drop", 42));
}

/// Pointed at another file, a track's row takes the file's details and its cues lose the old
/// file's seek columns; with a shift, they move, the start clamped at 0.
#[test]
fn pointing_at_another_file_moves_cues_and_clears_seeks() {
    let (mut one, t) = library_with_cues();
    let file = crate::edit::AudioFile { size: 9_000_000, sample_rate: 48_000, bit_depth: 24, bitrate: 2304 };
    one.set_track_path(t, "/Contents/A/B.aiff", Some(&file), 0).unwrap();
    let row: (String, i64, i64, i64, i64, i64) = one
        .conn
        .query_row(
            "SELECT path, fileSize, fileType, samplingRate, bitDepth, bitrate FROM content WHERE content_id = ?1",
            [t],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();
    assert_eq!(row, ("/Contents/A/B.aiff".into(), 9_000_000, 12, 48_000, 24, 2304));
    let seeks: (i64, i64, i64) = one
        .conn
        .query_row(
            "SELECT inMpegFrameNumber, inFileOffsetInBlock, inNumberOfSampleInBlock FROM cue WHERE cue_id = 7",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(seeks, (0, 0, 0));
    assert_eq!(one.cue_rows(t).unwrap()[0].in_usec, 1_500_250, "no shift, no move");

    one.set_track_path(t, "/Contents/A/B.aiff", None, -2000).unwrap();
    let starts: Vec<i64> = one.cue_rows(t).unwrap().iter().map(|r| r.in_usec).collect();
    assert_eq!(starts, [0, 28_000_000]);
}

#[test]
fn updates_adds_and_removes_keeping_ids() {
    let (mut one, t) = library_with_cues();
    let mut rows = one.cue_rows(t).unwrap();
    // Rename the memory cue (its seek columns stay), drop hot cue B, add a loop on hot cue C.
    rows[0].comment = "Intro".into();
    rows.remove(1);
    rows.push(CueRow {
        hot_cue: Some(2),
        in_usec: 60_000_000,
        out_usec: Some(64_000_000),
        loop_beats: Some((8, 1)),
        ..Default::default()
    });
    let changes = one.replace_cues(t, &rows).unwrap();
    assert_eq!(changes, CueRowChanges { updated: 1, added: 1, removed: 1, bank_list_entries_removed: 1 });
    let after = one.cue_rows(t).unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!((after[0].cue_id, after[0].comment.as_str()), (Some(7), "Intro"));
    let seek: i64 =
        one.conn.query_row("SELECT inFileOffsetInBlock FROM cue WHERE cue_id = 7", [], |r| r.get(0)).unwrap();
    assert_eq!(seek, 418, "untouched columns keep their values");
    assert_eq!((after[1].cue_id, after[1].out_usec, after[1].loop_beats), (Some(9), Some(64_000_000), Some((8, 1))));
    let frames: (i64, i64) = one
        .conn
        .query_row("SELECT in150FramePerSec, out150FramePerSec FROM cue WHERE cue_id = 9", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(frames, (9000, 9600));
    // The bank list lost its entry for B and was renumbered.
    let bank: Vec<(i64, i64)> = one
        .conn
        .prepare("SELECT cue_id, sequenceNo FROM hotCueBankList_cue")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(bank, [(7, 1)]);
    assert_eq!(one.dangling_bank_list_entries().unwrap(), 0);

    // Moving a cue resets its seek columns.
    let mut rows = one.cue_rows(t).unwrap();
    rows[0].in_usec = 2_000_000;
    one.replace_cues(t, &rows).unwrap();
    let seek: (i64, i64) = one
        .conn
        .query_row("SELECT inFileOffsetInBlock, in150FramePerSec FROM cue WHERE cue_id = 7", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(seek, (0, 300));
    // Another track's cue can't be claimed.
    let foreign = CueRow { cue_id: Some(7), ..Default::default() };
    assert!(one.replace_cues(t + 1, &[foreign]).is_err());
}
