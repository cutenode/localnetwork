//! Analysis files (`ANLZ0000.DAT`, `.EXT`, `.2EX`): every reader, and the rewrites that start
//! from a file on the drive (a relocated track's path, its cues, its times shifted).

#![no_main]

use libfuzzer_sys::fuzz_target;
use locallibrary::anlz::{self, CueFile, CuePoint};

const PATH: &str = "/Contents/fuzz/track.mp3";

fuzz_target!(|data: &[u8]| {
    let _ = anlz::read_path(data);
    let _ = anlz::beat_grid(data);
    let _ = anlz::cues(data);
    let _ = anlz::preview(data);
    let _ = anlz::detail(data);
    let _ = anlz::song_structure(data);

    // A merge relocates a track by rewriting its path, which replaces the whole `PPTH` section
    // (repairing a damaged one): a file that changed must read back with the new path.
    if let Ok(rewritten) = anlz::rewrite_path(data, PATH)
        && rewritten != data
    {
        assert_eq!(anlz::read_path(&rewritten).ok().flatten().as_deref(), Some(PATH));
    }

    for (file, ext) in [(CueFile::DatAlone, None), (CueFile::Ext, Some(data))] {
        let Ok(mut cues) = anlz::read_cue_points(data, ext) else { continue };
        let _ = anlz::write_cue_points(data, file, &cues);
        cues.push(CuePoint { hot_cue: Some(7), start_ms: 1000, comment: "fuzz".into(), ..Default::default() });
        let _ = anlz::write_cue_points(data, file, &cues);
    }

    // Offsets come from comparing two recordings, so any i32 can arrive.
    for ms in [1000, -1000, i32::MAX, i32::MIN] {
        let _ = anlz::shift_times(data, ms);
    }
});
