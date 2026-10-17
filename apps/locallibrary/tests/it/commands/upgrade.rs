//! Lossless files where players need them: `track upgrade` swapping MP3s for lossless files of
//! their recording, and `pack --transcode` packing copies a set's players play (which `check`
//! points to).

use std::path::Path;

use rekordbox_pdb::{NewTrack, PdbEditor};

use crate::support::{Env, PDB, path, recording};

#[test]
fn track_upgrade_points_an_mp3_at_its_lossless_file() {
    let env = Env::ascii();
    let demo = env.demo("Demo");
    let d = path(&demo);
    let album = demo.join("Contents/Loopmasters/UnknownAlbum");
    // A WAV standing in for the MP3 (symphonia goes by the content), 26 ms late as an encoder's
    // delay makes it.
    std::fs::write(album.join("Demo Track 1.mp3"), recording(7919, 30.0, 44_100, 16, 26)).unwrap();
    let music = env.outside("Lossless");
    std::fs::create_dir_all(&music).unwrap();
    std::fs::write(music.join("Demo Track 1.wav"), recording(7919, 30.0, 44_100, 16, 0)).unwrap();

    let out = env.ok(&["track", "upgrade", d, "--from", path(&music), "--dry-run"]);
    let line = "~ Loopmasters - Demo Track 1  Contents/Loopmasters/UnknownAlbum/Demo Track 1.wav  cues and grid -26 ms";
    assert!(out.contains(line), "{out}");
    assert!(!album.join("Demo Track 1.wav").exists());
    let out = env.ok(&["--yes", "track", "upgrade", d, "--from", path(&music), "--remove-old"]);
    assert!(
        out.contains("upgraded 1 track(s)") && out.contains("x Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3"),
        "{out}"
    );
    assert!(album.join("Demo Track 1.wav").is_file() && !album.join("Demo Track 1.mp3").exists());
    let v = env.json(&["track", "upgrade", d, "--from", path(&music)]);
    assert_eq!(v["outcome"], "nothing_to_do", "{v}");
}

/// The demo drive plus a 24-bit 96 kHz WAV in a playlist "Set".
fn with_hi_res(root: &Path) {
    let file = "/Contents/Hi/Hi Res.wav";
    // A second: resampling takes a while in a debug build.
    let wav = recording(5, 1.0, 96_000, 24, 0);
    std::fs::create_dir_all(root.join("Contents/Hi")).unwrap();
    std::fs::write(root.join(&file[1..]), &wav).unwrap();
    let mut ed = PdbEditor::from_file(root.join(PDB)).unwrap();
    let track = NewTrack {
        sample_rate: 96_000,
        sample_depth: 24,
        duration: 1,
        file_size: wav.len() as u32,
        ..NewTrack::new("Hi Res", file)
    };
    let id = ed.add_track(&track).unwrap();
    let set = ed.create_playlist("Set", 0, false).unwrap();
    ed.set_playlist_entries(set, &[id]).unwrap();
    ed.save(root.join(PDB)).unwrap();
}

#[test]
fn check_points_to_pack_transcode_which_makes_copies_the_players_play() {
    let env = Env::ascii();
    let demo = env.demo("Demo");
    with_hi_res(&demo);
    let r = env.run(&["check", path(&demo), "--players", "CDJ-2000NXS"]);
    assert!(r.out.contains("--transcode` packs lossless tracks"), "{}{}", r.out, r.err);

    let stick = env.blank("Stick");
    let args = ["pack", path(&demo), path(&stick), "--playlist", "Set", "--transcode", "--players", "CDJ-2000NXS"];
    let out = env.ok(&[&args[..], &["--dry-run"]].concat());
    assert!(out.contains("for CDJ-2000NXS: 1 copy(ies) to make"), "{out}");
    assert!(out.contains("Hi Res: aiff-48000-24-tpdf-v1 (CDJ-2000NXS doesn't play it at 96 kHz)"), "{out}");
    let out = env.ok(&[&["--yes"], &args[..]].concat());
    assert!(out.contains("1 copy(ies) made for the players"), "{out}");
    assert!(stick.join("Contents/Hi/Hi Res.aiff").is_file() && !stick.join("Contents/Hi/Hi Res.wav").exists());
}
