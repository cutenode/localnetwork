//! Analysis files (`ANLZ0000.DAT`, `.EXT`, `.2EX`): every reader, and the rewrites that start
//! from a file on the drive (a relocated track's path, its cues, its times shifted, its beat grid,
//! waveforms and phrases).

#![no_main]

use libfuzzer_sys::fuzz_target;
use loclib_anlz::{self as anlz, CueFile, CuePoint};

const PATH: &str = "/Contents/fuzz/track.mp3";

fuzz_target!(|data: &[u8]| {
    let _ = anlz::read_path(data);
    let _ = anlz::beat_grid(data);
    let _ = anlz::cues(data);
    let _ = anlz::preview(data);
    let _ = anlz::detail(data);
    let _ = anlz::song_structure(data);
    let _ = anlz::has_section(data, b"PQTZ");

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

    // Each writer reads what it wrote back and refuses what doesn't, so what's left to find is a
    // panic, a hang, or a write that doesn't settle: given what it just wrote, a writer has to
    // leave that file as it is. (Not the file it started from: a damaged section is rewritten in
    // the layout rekordbox uses.)
    for tag in anlz::waveform_tags() {
        match anlz::waveform_entries(data, tag) {
            Ok(Some(columns)) if !columns.is_empty() => {
                if let Ok(written) = anlz::write_waveform(data, tag, &columns) {
                    let again = anlz::write_waveform(&written, tag, &columns).expect("a waveform written takes itself");
                    assert_eq!(again, written, "{} written twice", String::from_utf8_lossy(tag));
                }
                let _ = anlz::write_waveform(data, tag, &[columns.as_slice(), &columns].concat());
            }
            // Six bytes are whole columns of every tag.
            _ => drop(anlz::write_waveform(data, tag, &[0x1f; 6])),
        }
    }
    if let Ok(beats) = anlz::beat_grid(data) {
        if let Ok(written) = anlz::write_beat_grid(data, &beats) {
            let again = anlz::write_beat_grid(&written, &beats).expect("a grid written takes itself");
            assert_eq!(again, written, "beat grid written twice");
        }
        let _ = anlz::write_beat_grid(data, &anlz::doubled_grid(&beats));
        let _ = anlz::write_beat_grid(data, &anlz::halved_grid(&beats));
    }
    let _ = anlz::without_extended_grid(data);
    if let Ok(structure) = anlz::song_structure(data) {
        if let Some(phrases) = &structure
            && let Ok(written) = anlz::write_song_structure(data, Some(phrases))
        {
            let again = anlz::write_song_structure(&written, Some(phrases)).expect("phrases written take themselves");
            assert_eq!(again, written, "phrases written twice");
        }
        let _ = anlz::write_song_structure(data, None);
    }
});
