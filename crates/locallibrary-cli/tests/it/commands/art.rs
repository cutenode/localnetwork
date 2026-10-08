//! `art`: tracks without artwork, artwork found beside the audio or set by hand and saved back
//! out, and artwork embedded into files on this computer and taken back out.

use crate::support::{Env, line, path};

#[test]
fn art_commands_say_what_they_found() {
    let env = Env::new();
    let a = env.demo("A");
    let missing = env.ok(&["art", "A"]);
    assert_eq!(
        missing.lines().collect::<Vec<_>>(),
        ["    #1  Loopmasters - Demo Track 1", "    #2  Loopmasters - Demo Track 2", "2 track(s) without artwork"]
    );
    let none = env.ok(&["art", "auto", "A", "--dry-run"]);
    assert_eq!(none.trim(), "0 found, 2 still missing");

    // A cover next to the audio is found.
    let cover = a.join("Contents/Loopmasters/UnknownAlbum/cover.jpg");
    image::RgbImage::from_pixel(300, 300, image::Rgb([200, 30, 30])).save(&cover).unwrap();
    let found = env.ok(&["art", "auto", "A", "--dry-run"]);
    assert!(line(&found, "#1").contains("<- ") && found.ends_with("2 found, 0 still missing\n"), "{found}");
    assert!(env.ok(&["--yes", "art", "auto", "A"]).contains("✓ artwork set on 2 track(s)"));
    assert!(env.ok(&["art", "A"]).starts_with("0 track(s) without artwork"));

    // Set by hand, then saved back out.
    let image = env.outside("art.png");
    image::RgbImage::from_pixel(300, 300, image::Rgb([30, 30, 200])).save(&image).unwrap();
    let plan = env.ok(&["art", "set", "A", path(&image), "--track", "2", "--dry-run"]);
    assert!(plan.contains("    Loopmasters - Demo Track 2"), "{plan}");
    assert!(
        env.ok(&["--yes", "art", "set", "A", path(&image), "--track", "2"]).contains("✓ artwork set on 1 track(s)")
    );
    let out = env.outside("saved.jpg");
    assert!(env.ok(&["art", "get", "A", "2", "-o", path(&out)]).contains("✓ wrote "));
    assert!(std::fs::read(&out).unwrap().starts_with(b"\xff\xd8\xff"), "a JPEG");
    assert_eq!(env.ok(&["art", "unembed", "--list"]).trim(), "nothing to take back");

    // Embedded into a file on this computer, then taken back out.
    let song = env.outside("song.wav");
    std::fs::write(&song, silent_wav()).unwrap();
    let plan = env.ok(&["art", "embed", path(&image), path(&song), "--dry-run"]);
    assert!(
        plan.contains(" in 1 file(s) on this computer") && line(&plan, "song.wav").ends_with("song.wav "),
        "{plan}"
    );
    let done = env.ok(&["--yes", "art", "embed", path(&image), path(&song)]);
    assert!(done.contains("✓ embedded into 1 file(s)\n"), "{done}");
    let backup = line(&done, "--backup").split_whitespace().nth(4).unwrap().trim_end_matches('`').to_string();
    assert_ne!(std::fs::read(&song).unwrap(), silent_wav());
    assert!(line(&env.ok(&["store"]), "  embeds ").ends_with(" KB kept for `art unembed`"), "a copy is kept");
    assert!(line(&env.ok(&["art", "unembed", "--list"]), &backup).contains("  1 file(s)  "));
    assert!(env.ok(&["--yes", "art", "unembed"]).ends_with("✓ put back 1 file(s)\n"));
    assert_eq!(std::fs::read(&song).unwrap(), silent_wav());
}

/// A tenth of a second of silence, as a WAV file.
fn silent_wav() -> Vec<u8> {
    let samples = [0u8; 8820];
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + samples.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    // PCM, one channel, 44.1 kHz, two bytes a frame, 16-bit.
    for field in [16u32.to_le_bytes(), [1, 0, 1, 0], 44100u32.to_le_bytes(), 88200u32.to_le_bytes(), [2, 0, 16, 0]] {
        wav.extend(field);
    }
    wav.extend(b"data");
    wav.extend((samples.len() as u32).to_le_bytes());
    wav.extend(samples);
    wav
}
