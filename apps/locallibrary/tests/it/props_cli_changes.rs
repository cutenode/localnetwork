//! Property tests of what changes say: two copies of the same drawn drives and store (see
//! `props_cli_human`), each given the same changes in the same drawn order, one as a person runs
//! them and the other with `--json`. Each change comes out the same way in both (its exit
//! status, or its error), and its text says what its JSON plan and report hold: every name,
//! count and path a person reads, and no drive, library, playlist or track the JSON doesn't name.

use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::props_cli_human::{FLAGS, State, TRACK_1, agree_runs, as_in, build, side_by_side, twin, universe, write};
use crate::support::{Env, Run, path, recording};

/// A tenth of a second of silence, as a WAV file.
fn silent_wav() -> Vec<u8> {
    let data = [0u8; 8820];
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + data.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    // PCM, one channel, 44.1 kHz, two bytes a frame, 16-bit.
    for field in [16u32.to_le_bytes(), [1, 0, 1, 0], 44100u32.to_le_bytes(), 88200u32.to_le_bytes(), [2, 0, 16, 0]] {
        wav.extend(field);
    }
    wav.extend(b"data");
    wav.extend((data.len() as u32).to_le_bytes());
    wav.extend(data);
    wav
}

/// What the changes need on "this computer": firmware downloads, a song to embed artwork in, an
/// image, and a folder to collect into.
fn this_computer(fx: &Env) {
    write(&fx.home("CDJ3Kv321.UPD"), "update");
    write(&fx.home("firmware.UPD"), "update");
    std::fs::write(fx.home("song.wav"), silent_wav()).unwrap();
    image::RgbImage::from_pixel(300, 300, image::Rgb([30, 30, 200])).save(fx.home("art.png")).unwrap();
}

/// A change's words with its placeholders filled in for `fx`: those of
/// [`props_cli_human::fill`](crate::props_cli_human::fill), and `@snap` the first library's
/// latest snapshot.
fn fill(fx: &Env, words: &[&str]) -> Vec<String> {
    words
        .iter()
        .map(|w| match *w {
            "@snap" => {
                let v = fx.json(&["library", "ls"]);
                v[0]["head"]["short_id"].as_str().unwrap_or("0000").to_string()
            }
            w => crate::props_cli_human::fill(fx, w),
        })
        .collect()
}

/// What a step does to the files, as a person or a failing disk would (the same to both copies),
/// rather than a command.
fn by_hand(fx: &Env, step: &str) {
    let a = fx.volumes.join("A");
    match step {
        // A file in the store changes on disk.
        "!corrupt-store" => {
            if let Some(f) = same_in_both(&fx.store.join("objects")) {
                std::fs::write(f, "not what was stored").unwrap();
            }
        }
        // Track 1's artwork, and its audio, damaged.
        "!damage" => {
            if let Ok(bytes) = std::fs::read(a.join(TRACK_1)) {
                let modified = std::fs::metadata(a.join(TRACK_1)).unwrap().modified().unwrap();
                std::fs::write(a.join(TRACK_1), bytes.to_ascii_uppercase()).unwrap();
                std::fs::File::options().write(true).open(a.join(TRACK_1)).unwrap().set_modified(modified).unwrap();
            }
            let _ = std::fs::remove_dir_all(a.join("PIONEER/Artwork"));
        }
        // Clutter and a recording, again.
        "!clutter" => {
            write(&a.join(".DS_Store"), "finder");
            write(&a.join(".Trashes/501/old.mp3"), "trash");
            write(&a.join("ALPHATHETA REC/REC002.WAV"), "a recording");
            write(&a.join("PIONEER/USBANLZ/P0FE/0000CAFE/ANLZ0000.DAT"), "PMAI orphan");
        }
        // A's or Club's identity lost, as when it's erased on another computer.
        "!erase-a" => {
            let _ = std::fs::remove_dir_all(a.join(".locallibrary"));
        }
        "!erase-club" => {
            let _ = std::fs::remove_dir_all(fx.volumes.join("Club/.locallibrary"));
        }
        // Club unplugged (kept on this computer).
        "!unplug-club" => {
            if fx.volumes.join("Club").exists() {
                std::fs::rename(fx.volumes.join("Club"), fx.home("club-drive")).unwrap();
            }
        }
        // The store's files gone (a folder deleted by hand): only another drive has good copies.
        "!lose-store-objects" => {
            let _ = std::fs::remove_dir_all(fx.store.join("objects"));
        }
        // A file in the store's copy changes on disk.
        "!corrupt-copy" => {
            if let Some(f) = same_in_both(&fx.home("copy/objects")) {
                std::fs::write(f, "not what was copied").unwrap();
            }
        }
        // A file in the store nothing refers to, and one the store refers to gone.
        "!stray-object" => write(&fx.store.join(format!("objects/ff/{}", "f".repeat(62))), "nothing uses this"),
        "!lose-object" => {
            if let Some(f) = same_in_both(&fx.store.join("objects")) {
                std::fs::remove_file(f).unwrap();
            }
        }
        // rekordbox exported a playlist to A (an edit through locallibrary would have been copied
        // onto A's clones too).
        "!export-a" => {
            let mut ed = rekordbox_pdb::PdbEditor::from_file(a.join(crate::support::PDB)).unwrap();
            ed.create_playlist("Fresh Export", 0, false).unwrap();
            ed.save(a.join(crate::support::PDB)).unwrap();
        }
        // A player recorded a history list on Blank: back from a set.
        "!play-blank" => {
            if fx.volumes.join("Blank").join(crate::support::PDB).exists() {
                crate::support::play(&fx.volumes.join("Blank"), "HISTORY 009", &[1]);
            }
        }
        // A cover image beside the audio, for `art auto` to find.
        "!cover" => {
            let cover = a.join("Contents/Loopmasters/UnknownAlbum/cover.jpg");
            image::RgbImage::from_pixel(300, 300, image::Rgb([200, 30, 30])).save(&cover).unwrap();
            filetime::set_file_mtime(&cover, filetime::FileTime::from_unix_time(1_790_000_000, 0)).unwrap();
        }
        other => panic!("no step {other}"),
    }
}

/// The store object to damage, the same one in both copies of the fixture: the largest (audio,
/// not a snapshot's manifest, whose name follows the time it was made), then by path.
fn same_in_both(objects: &Path) -> Option<std::path::PathBuf> {
    walk(objects)
        .into_iter()
        .max_by_key(|f| (std::fs::metadata(f).map_or(0, |m| m.len()), std::cmp::Reverse(f.clone())))
}

/// Every file under `dir`, by path.
fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<_> = walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    out.sort();
    out
}

/// Each change comes with what its text must say from its JSON (see `props_cli_human::expected`):
/// `plan` is what it was to do, `report` what it did. They run with `--yes` but for those that
/// say they ask (`?` first: run as they are, with no terminal to ask on); `!` steps are done by
/// hand (see [`by_hand`]); `;` runs several in turn.
type Changes = &'static [(&'static [&'static str], &'static [&'static str])];

/// Changes to a drive of its own: its name, role, identity, what's on it besides the library.
const DRIVE_CHANGES: Changes = &[
    (&["drive", "rename", "A", "Main", "--dry-run"], &["plan.summary.[]"]),
    (&["drive", "rename", "Club", "Floor"], &["plan.summary.[]", "report.name", "report.old_name"]),
    (&["drive", "rename", "Floor", "Club", "--label", "CLUB"], &["report.name"]),
    (&["drive", "role", "Club", "backup"], &["plan.summary.[]", "report.name"]),
    (&["drive", "role", "Club", "none"], &["plan.summary.[]", "report.name"]),
    (&["drive", "retire", "Club"], &["plan.summary.[]", "report.name"]),
    (&["drive", "retire", "Club", "--undo"], &["plan.summary.[]", "report.name"]),
    (
        &["drive", "contact", "A", "dj@example.com", "--playlist"],
        &["plan.summary.[]", "report.file", "report.playlist"],
    ),
    (&["drive", "player", "A", "--name", "MAIN A", "--colour", "6"], &["plan.summary.[]", "report.drive"]),
    (&["drive", "player", "Blank", "--name", "SPARE", "--dry-run"], &["plan.summary.[]"]),
    (&["drive", "adopt", "Blank", "--name", "Spare"], &["plan.summary.[]", "report.root", "report.name"]),
    (&["drive", "adopt", "@v/Blank", "--as", "A"], &["plan.summary.[]", "report.name"]),
    (&["drive", "reidentify", "@v/Blank", "--force"], &["plan.summary.[]", "report.root"]),
    (&["drive", "reidentify", "@v/Club", "--name", "Floor", "--dry-run"], &["plan.summary.[]"]),
    (&["drive", "forget", "Spare"], &["plan.summary.[]", "report.name"]),
    (&["drive", "tidy", "A", "--dry-run"], &["plan.drive"]),
    (&["drive", "tidy", "A", "--empty-trash"], &["plan.drive", "removed {report.removed} file(s)"]),
    (&["drive", "recordings", "A", "--copy-to", "@h/rec", "--dry-run"], &["plan.summary.[]"]),
    (
        &["drive", "recordings", "A", "--copy-to", "@h/rec", "--tracklist", "latest", "--delete"],
        &["plan.summary.[]", "report.collected.[].recording", "report.collected.[].copied_to"],
    ),
    (
        &["drive", "recordings", "A", "--copy-to", "@h/rec2", "--tracklist", "HISTORY 001"],
        &["report.collected.[].tracklist"],
    ),
    (&["drive", "firmware", "A", "@h/firmware.UPD"], &["plan.file_name", "plan.problems.[]"]),
    (&["drive", "firmware", "A", "@h/firmware.UPD", "--force"], &["plan.file_name", "report.written"]),
    (
        &["drive", "firmware", "A", "@h/CDJ3Kv321.UPD", "--replace", "--dry-run"],
        &["plan.file_name", "plan.existing.[]"],
    ),
    (&["drive", "firmware", "A", "--remove"], &["plan.summary.[]", "removed {report.[]}"]),
    (&["drive", "verify", "A", "--repair"], &["plan.drive", "plan.damaged.[].path", "plan.missing.[].path"]),
    (&["drive", "verify", "A", "--repair", "--against", "Blank"], &["plan.drive"]),
    (&["drive", "fsck", "A", "--repair"], &[]),
    (&["?drive", "fsck", "A", "--repair"], &[]),
    (&["drive", "restore", "A", "--dry-run"], &[]),
    (&["drive", "restore", "A", "--playlist", "Set", "--dry-run"], &[]),
    (&["eject", "A"], &[]),
    (&["!damage"], &[]),
    (&["!clutter"], &[]),
    (&["drive", "rename", "A", "A"], &[]),
    (&["?drive", "reidentify", "@v/Blank"], &[]),
    (&["?drive", "adopt", "Demo"], &[]),
    (&["adopt", "Demo", "--name", "Demo"], &["report.name"]),
    (&["!erase-club"], &[]),
    (&["?drive", "adopt", "@v/Club", "--as", "Club"], &[]),
    (&["drive", "adopt", "@v/Club", "--as", "Club"], &["plan.summary.[]"]),
    (&["?drive", "firmware", "A", "--remove"], &[]),
    (&["firmware", "A", "--remove", "--dry-run"], &[]),
    (&["?drive", "verify", "A", "--repair"], &[]),
    (&["!lose-store-objects"], &[]),
    (&["drive", "verify", "A", "--repair", "--against", "Blank"], &["plan.drive"]),
    (&["drive", "tidy", "A"], &[]),
    (&["tidy", "A", "--dry-run"], &[]),
    (&["track", "rm", "A", "--track", "2"], &[]),
    (&["drive", "backups", "A"], &["[].id"]),
    (&["restore", "A", "--dry-run"], &[]),
    (&["!unplug-club"], &[]),
    (&["status"], &["libraries.[].name", "offline.[].name"]),
    (&["drive", "ls", "--known"], &["[].name"]),
    (&["eject"], &[]),
    (&["drive", "rename", "A", "Main", ";", "drive", "rename", "Main", "A", "--label", "A"], &["report.name"]),
    (&["!erase-a", ";", "drive", "adopt", "@v/A", "--as", "A"], &["plan.summary.[]", "report.name"]),
    (&["track", "rm", "A", "--track", "1", ";", "drive", "backups", "A"], &["[].id", "[].reason"]),
];

/// Changes that move libraries between drives and the store: merge, convert, clean, pull, push
/// and undo.
const LIBRARY_CHANGES: Changes = &[
    (&["merge", "Club", "A", "--dry-run"], &["plan.source", "~plan.new_tracks.[].name"]),
    (&["merge", "Club", "A", "--playlists", "by-name"], &["added {report.tracks_added} track(s)"]),
    (&["merge", "A", "Blank", "--playlists", "folder", "--folder", "From A", "--history"], &[]),
    (&["convert", "A", "--to", "onelibrary", "--dry-run"], &["plan.drive", "{plan.tracks} track(s)"]),
    (&["convert", "A", "--to", "onelibrary"], &["converted {report.tracks} track(s)"]),
    (&["convert", "A", "--to", "device-library", "--replace"], &["converted {report.tracks} track(s)"]),
    (&["clean", "Blank", "--dry-run"], &["plan.drive", "plan.tracks.[].name"]),
    (&["clean", "Blank"], &["removed {report.tracks_removed} track(s)"]),
    (&["pull", "A"], &[]),
    (&["pull", "A", "--label", "gig", "--message", "before the gig"], &["report.library"]),
    (&["pull"], &["report.steps.[].done.[]"]),
    (&["pull", "A", "--library", "Club"], &[]),
    (&["pull", "Club", "--library", "Nope"], &[]),
    (&["pull", "--label", "x"], &[]),
    (&["library", "push", "Blank", "-l", "A", "--dry-run"], &[]),
    (&["library", "push", "Blank", "-l", "A", "--force"], &[]),
    (&["push", "Blank", "--eject"], &[]),
    (&["undo", "A", "--dry-run"], &[]),
    (&["undo", "A"], &[]),
    (&["!damage"], &[]),
    (&["merge", "A", "Club", "--history", "--dry-run"], &["plan.source"]),
    (&["merge", "A", "Club", "--history"], &["added {report.tracks_added} track(s)"]),
    (&["merge", "A", "Club", "--playlists", "none", "--match-path-only", "--dry-run"], &[]),
    (&["pull", "A", "--library", "A"], &[]),
    (&["pull", "A", "--new-library"], &[]),
    (&["clean", "A", "--dry-run"], &["plan.drive", "~plan.tracks.[].name", "plan.playlists.[].path"]),
    (&["clean", "A"], &["removed {report.tracks_removed} track(s)"]),
    (&["merge", "A", "Blank", "--playlists", "none"], &[]),
    (&["--overwrite-archive", "merge", "A", "Club", "--dry-run"], &[]),
    (&["--overwrite-archive", "merge", "A", "Club"], &[]),
    (&["playlist", "sort", "A", "Set", "--sort", "bpm"], &[]),
    (&["diff", "A", "Blank"], &["left", "right", "changed_playlists.[].path"]),
    (&["pull", "Clubland", "--library", "A"], &[]),
    (&["merge", "Blank", "A"], &[]),
    (&["merge", "A", "A"], &[]),
    (&["!play-blank"], &[]),
    (&["playlist", "new", "A", "Late Set"], &[]),
    (&["drive", "ls", "--known"], &["[].name"]),
];

/// Changes that copy a library from drive to drive: sync, mirror, pack and provision.
const SYNC_CHANGES: Changes = &[
    (&["provision", "Blank", "--library", "A", "--dry-run"], &["plan.steps.[]"]),
    (&["provision", "Blank", "--library", "A", "--role", "backup"], &["plan.steps.[]"]),
    (&["sync", "A", "Blank", "--dry-run"], &["plan.comparison.left.label", "plan.comparison.right.label"]),
    (&["sync", "A", "Blank"], &["copied {report.report.copied} files"]),
    (&["sync", "--from", "A", "Blank", "--verify", "--keep-extra"], &[]),
    (&["sync", "A", "Blank", "--eject"], &[]),
    (&["sync"], &[]),
    (&["sync", "A"], &[]),
    (&["sync", "A", "Club", "Blank", "--dry-run"], &[]),
    (&["sync", "A", "Club"], &[]),
    (&["mirror", "A", "Blank", "--delete-first"], &[]),
    (&["sync", "A", "--library", "A"], &[]),
    (&["sync", "Club"], &[]),
    (&["sync", "--from", "A", "Blank", "Club"], &[]),
    (
        &[
            "pack",
            "A",
            "Blank",
            "--playlist",
            "Set",
            "--transcode",
            "--players",
            "CDJ-2000NXS",
            "--container",
            "wav",
            "--dry-run",
        ],
        &[],
    ),
    (&["?provision", "Blank", "--library", "A"], &[]),
    (&["sync", "Clubland", "--library", "A"], &[]),
    (&["sync", "Clubland", "--dry-run"], &[]),
    (&["sync", "--eject"], &[]),
    (&["sync", "A", "Blank", "--dry-run"], &[]),
    (&["sync", "A", "Blank"], &[]),
    (&["sync", "A", "Blank", "Club", "--eject"], &[]),
    (&["pack", "A", "Blank", "--playlist", "Set", "--transcode", "--container", "aiff", "--dry-run"], &[]),
    (&["pack", "A", "Blank", "--playlist", "Set", "--transcode", "--container", "flac", "--dry-run"], &[]),
    (&["provision", "Blank", "--format", "--fs", "fat32", "--cluster-size", "32K", "--dry-run"], &[]),
    (&["provision", "Blank", "--format", "--fs", "exfat", "--cluster-size", "1MiB", "--dry-run"], &[]),
    (&["provision", "Blank", "--format", "--fs", "hfs+", "--cluster-size", "4096", "--dry-run"], &[]),
    (&["!export-a", ";", "sync", "A", "Blank", "--eject"], &[]),
    (&["sync", "A", "Blank", ";", "!export-a", ";", "sync", "--eject"], &[]),
    (&["sync", "A", "Blank", ";", "!play-blank", ";", "!export-a", ";", "sync", "A", "Blank"], &[]),
];

/// Changes to the store and what it keeps: snapshots, labels, copies, libraries, players, the
/// settings profile and artwork.
const STORE_CHANGES: Changes = &[
    (&["store", "gc", "--dry-run"], &["plan.summary.[]"]),
    (&["store", "gc"], &[]),
    (&["store", "prune", "--keep-last", "1", "--dry-run"], &["plan.libraries.[].library"]),
    (
        &["store", "prune", "--keep-daily", "1", "--keep-weekly", "1"],
        &["removed {report.removed_snapshots} snapshot(s)"],
    ),
    (
        &["library", "prune", "--keep-last", "1", "-l", "A"],
        &["removed {report.removed_snapshots} snapshot(s)", "({report.removed_objects} file(s))"],
    ),
    (&["store", "label", "@snap", "gig"], &["plan.summary.[]", "report.name"]),
    (&["store", "unlabel", "gig"], &["plan.summary.[]", "report.name"]),
    (&["store", "copy", "@h/copy", "--dry-run"], &["plan.path"]),
    (&["store", "copy", "@h/copy", "--mirror", "--verify"], &["report.path", "copied {report.objects_copied} file(s)"]),
    (&["store", "copy", "@h/copy", "--force"], &["report.path"]),
    (&["store", "copies"], &["[].path"]),
    (&["store", "verify"], &["corrupt.[]", "missing.[]"]),
    (&["library", "rename", "A", "Home"], &["plan.summary.[]", "report.name"]),
    (&["library", "rename", "Home", "A"], &["report.name"]),
    (&["players", "set", "CDJ-3000", "--dry-run"], &["plan.summary.[]"]),
    (&["players", "set", "XDJ-AZ", "-l", "A"], &["plan.summary.[]", "report.libraries.[].models.[]"]),
    (&["players", "set", "CDJ-3000,CDJ-2000NXS2"], &["report.default.[]"]),
    (&["players", "clear", "-l", "A"], &["plan.summary.[]"]),
    (&["players", "clear"], &["plan.summary.[]"]),
    (&["settings", "set", "quantize=off", "sync=on"], &["plan.summary.[]"]),
    (&["settings", "set", "lcd_brightness=4", "--drive", "A"], &["plan.summary.[]"]),
    (&["settings", "unset", "quantize"], &["plan.summary.[]", "report.base.&"]),
    (&["settings", "apply", "--dry-run"], &[]),
    (&["settings", "apply"], &["report.[].drive"]),
    (&["settings", "apply", "Blank", "--create-missing"], &["report.[].drive"]),
    (&["settings", "import", "A", "--dry-run"], &["plan.summary.[]", "plan.changes.[].key"]),
    (&["settings", "import", "A"], &[]),
    (&["art", "set", "A", "@h/art.png", "--track", "2"], &["plan.summary.[]", "plan.tracks.[].name"]),
    (&["art", "auto", "A"], &[]),
    (&["art", "auto"], &[]),
    (&["art", "auto", "--dry-run"], &[]),
    (&["art", "embed", "@h/art.png", "@h/song.wav", "--dry-run"], &["plan.summary.[]", "plan.files.[].path"]),
    (&["art", "embed", "@h/art.png", "@h/song.wav"], &["embedded into {report.embedded.#} file(s)"]),
    (&["store", "info"], &["root"]),
    (&["art", "unembed", "--list"], &["[].id"]),
    (&["art", "unembed", "--dry-run"], &["plan.summary.[]"]),
    (&["art", "unembed"], &[]),
    (&["art", "get", "A", "1", "-o", "@h/cover.jpg"], &["report.written"]),
    (&["!corrupt-store"], &[]),
    (&["!corrupt-copy"], &[]),
    (&["store", "copy", "@h/copy", "--verify"], &["report.path"]),
    // A newer snapshot, in the copy too, and a file in the copy changed; then pruned, so mirroring
    // the store removes from the copy what the store no longer has.
    (&["playlist", "new", "A", "Prune Me"], &[]),
    (&["pull", "A"], &[]),
    (&["store", "copy", "@h/copy"], &["report.path"]),
    (&["!corrupt-copy"], &[]),
    (&["store", "copy", "@h/copy", "--verify"], &["report.path"]),
    (
        &["store", "prune", "--keep-last", "1"],
        &["removed {report.removed_snapshots} snapshot(s)", "({report.removed_objects} file(s))"],
    ),
    (&["?store", "copy", "@h/copy", "--mirror"], &[]),
    (&["store", "copy", "@h/copy", "--mirror", "--dry-run"], &["plan.path"]),
    (&["store", "copy", "@h/copy", "--mirror"], &["report.path"]),
    (&["store"], &["root", "{stats.snapshots}"]),
    (&["!stray-object"], &[]),
    (&["store", "gc", "--dry-run"], &["plan.summary.[]"]),
    (&["?store", "gc"], &[]),
    (&["store", "gc"], &[]),
    (&["!lose-object"], &[]),
    (&["store", "verify"], &["missing.[]"]),
    (&["?art", "set", "A", "@h/art.png", "--track", "2"], &[]),
    (&["art", "set", "Club", "@h/art.png", "--filter", "bpm:>=0", "--dry-run"], &["plan.summary.[]"]),
    (&["!cover"], &[]),
    (&["art", "auto", "A", "--dry-run"], &["~plan.found.[].track.name"]),
    (&["?art", "auto", "A"], &[]),
    (&["art", "auto", "A"], &[]),
];

/// The jobs of several steps (refresh, prep, setup), the checks that change files (orphans), and
/// what writes files on this computer or watches for drives.
const JOBS_CHANGES: Changes = &[
    (&["refresh", "A", "--dry-run"], &[]),
    (&["refresh", "A", "--skip", "pull"], &[]),
    (&["refresh"], &[]),
    (&["prep", "--no-eject", "--dry-run"], &[]),
    (&["prep", "--no-eject", "--verbose"], &[]),
    (
        &[
            "setup",
            "A",
            "Blank",
            "--library",
            "A",
            "--primary",
            "A",
            "--role",
            "Blank=backup",
            "--player-name",
            "A=MAIN",
            "--contact",
            "dj@example.com",
            "--contact-playlist",
            "--players",
            "CDJ-3000",
        ],
        &["report.mounted.[].library", "report.notes.[]"],
    ),
    (&["setup", "--library", "Fresh", "--no-settings"], &["report.mounted.[].library", "report.notes.[]"]),
    (&["doctor", "A", "--fix-orphans", "--dry-run"], &["plan.drive", "plan.files.[..20].path"]),
    (&["doctor", "A", "--fix-orphans"], &[]),
    (&["doctor", "A", "--audio", "--analyze"], &["drive", "audio.problems.[].track.name"]),
    (&["check", "A", "--audio", "--verbose"], &["items.[].message"]),
    (&["xml", "A", "-o", "@h/rekordbox.xml"], &[]),
    (&["xml", "-o", "@h/rekordbox.xml"], &[]),
    (&["watch", "--once", "--settle", "0"], &[]),
    (&["watch", "--once", "--pull", "--sync-group", "--settle", "0"], &[]),
    (&["watch", "--uninstall"], &[]),
    (&["!clutter"], &[]),
    (&["?setup", "--library", "Fresh"], &[]),
    (&["setup"], &[]),
    (&["setup", "A", "--library", "A", "--name", "bogus"], &[]),
    (&["setup", "A", "--library", "A", "--role", "A=boss"], &[]),
    (&["setup", "Nope", "--library", "A"], &[]),
    (&["setup", "Club", "--library", "A", "--primary", "Club"], &[]),
    (
        &["setup", "A", "Blank", "--library", "Fresh2", "--no-settings"],
        &["report.mounted.[].library", "report.notes.[]"],
    ),
    (&["setup", "Demo", "--library", "Fresh3"], &["report.mounted.[].library", "report.notes.[]"]),
    (
        &[
            "setup",
            "A",
            "Blank",
            "--library",
            "A",
            "--role",
            "Blank=archive",
            "--contact",
            "x@example.com",
            "--player-name",
            "Blank=B",
        ],
        &["report.mounted.[].library", "report.notes.[]"],
    ),
    (&["refresh", "A"], &[]),
    (&["refresh", "Blank"], &[]),
    (&["!erase-club"], &[]),
    (&["setup", "Club", "--library", "Clubland"], &["report.mounted.[].library", "report.notes.[]"]),
    (&["setup", "A", "Blank", "--library", "A", "--role", "Blank=boss"], &[]),
    (&["setup", "A", "--library", "A", "--players", "NOPE-1"], &[]),
    (
        &[
            "sync",
            "A",
            "Blank",
            ";",
            "playlist",
            "new",
            "A",
            "Watch Me",
            ";",
            "watch",
            "--once",
            "--pull",
            "--sync-group",
            "--settle",
            "0",
        ],
        &[],
    ),
];

/// `text` with what differs between the two copies for no reason of the change's (the ids made
/// for drives and libraries, snapshot and file hashes, backups named for when they were made)
/// written the same: `<id>`, `<hex>`, `<backup>`.
fn same_ids(text: &str) -> String {
    let b = text.as_bytes();
    let hex = |i: usize| b.get(i).is_some_and(u8::is_ascii_hexdigit);
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let run = |i: usize, f: &dyn Fn(usize) -> bool| (i..).take_while(|&k| f(k)).count();
    let edge = |i: usize| i == 0 || !b[i - 1].is_ascii_alphanumeric();
    let (mut out, mut i) = (String::new(), 0);
    while i < b.len() {
        if edge(i) {
            // 8-4-4-4-12 hex digits.
            let groups = [8, 4, 4, 4, 12];
            let mut k = i;
            let uuid = groups.iter().enumerate().all(|(n, &len)| {
                let ok = run(k, &hex) >= len && (n == 4 || b.get(k + len) == Some(&b'-'));
                k += len + 1;
                ok
            }) && !b.get(k - 1).is_some_and(u8::is_ascii_alphanumeric);
            if uuid {
                out.push_str("<id>");
                i = k - 1;
                continue;
            }
            // 20261012T055510.711Z
            let at = |k: usize, c: u8| b.get(i + k) == Some(&c);
            if run(i, &digit) == 8
                && at(8, b'T')
                && run(i + 9, &digit) == 6
                && at(15, b'.')
                && run(i + 16, &digit) == 3
                && at(19, b'Z')
            {
                out.push_str("<backup>");
                i += 20;
                continue;
            }
            let n = run(i, &hex);
            if n >= 12 && !b.get(i + n).is_some_and(u8::is_ascii_alphanumeric) {
                out.push_str("<hex>");
                i += n;
                continue;
            }
        }
        let c = text[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// [`same_ids`] of each string in `v`.
fn same_ids_in(v: Value) -> Value {
    match v {
        Value::String(s) => Value::String(same_ids(&s)),
        Value::Array(list) => Value::Array(list.into_iter().map(same_ids_in).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (same_ids(&k), same_ids_in(v))).collect()),
        v => v,
    }
}

/// One step, given to both copies: by hand to each, or the change as a person runs it in `h` and
/// with `--json` in `j`, the two said the same.
fn step(h: &Env, j: &Env, words: &[&str], spec: &[&str], universe: &[String]) {
    if words[0].starts_with('!') {
        by_hand(h, words[0]);
        by_hand(j, words[0]);
        return;
    }
    // `?`: run without `--yes`, as when there's no terminal to ask on.
    let (yes, words): (&[&str], Vec<&str>) = match words[0].strip_prefix('?') {
        Some(first) => (&[], [first].into_iter().chain(words[1..].iter().copied()).collect()),
        None => (&["--yes"], words.to_vec()),
    };
    let args_h = [yes.iter().map(|s| s.to_string()).collect(), fill(h, &words)].concat();
    let run_json = || {
        let args_j =
            [vec!["--json".to_string()], yes.iter().map(|s| s.to_string()).collect(), fill(j, &words)].concat();
        j.run(&args_j.iter().map(String::as_str).collect::<Vec<_>>())
    };
    let (human, json) = side_by_side(|| h.run(&args_h.iter().map(String::as_str).collect::<Vec<_>>()), run_json);
    // The copies are in folders of their own: the JSON says the human run's paths.
    let json = as_in(json, j, h);
    let docs: Vec<Value> = serde_json::Deserializer::from_str(&json.out)
        .into_iter::<Value>()
        .map(|d| same_ids_in(d.unwrap_or_else(|e| panic!("{words:?}: not JSON ({e}): {}", json.out))))
        .collect();
    let out = docs.iter().map(|d| serde_json::to_string_pretty(d).unwrap() + "\n").collect();
    let json = Run { out, err: same_ids(&json.err), code: json.code };
    let human = Run { out: same_ids(&human.out), err: same_ids(&human.err), code: human.code };
    let args: Vec<&str> = args_h.iter().map(String::as_str).collect();
    agree_runs(&args, &human, &json, spec, universe);
}

/// `changes`, in `order`, on the drives and store `flags` draw (see [`State::from_flags`]): each says in its text what its JSON
/// says, and comes out the same way. Whatever was done, the two copies hold the same drives and
/// libraries.
fn changes_agree(flags: &[bool], order: Vec<usize>, changes: Changes) {
    let h = build(State::from_flags(flags));
    this_computer(&h);
    let j = twin(&h);
    let universe = universe(&h);
    for i in order {
        // A change of several steps (`;` between them) runs them in turn; the spec is the last's.
        let (words, spec) = changes[i];
        let steps: Vec<&[&str]> = words.split(|w| *w == ";").collect();
        for (n, words) in steps.iter().enumerate() {
            step(&h, &j, words, if n + 1 == steps.len() { spec } else { &[] }, &universe);
        }
    }
    let names = |fx: &Env| -> Vec<Value> {
        let drives = fx.json(&["drive", "ls"]);
        let mut out: Vec<Value> = drives.as_array().unwrap().iter().map(|d| d["name"].clone()).collect();
        out.extend(fx.json(&["library", "ls"]).as_array().unwrap().iter().map(|l| l["name"].clone()));
        out
    };
    let (in_h, in_j) = side_by_side(|| names(&h), || names(&j));
    assert_eq!(in_h, in_j);
}

/// [`DRIVE_CHANGES`] say what their JSON says, in any order, on any drawn drives and store.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS], order = (0..DRIVE_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = vec![false; FLAGS], order = (0..DRIVE_CHANGES.len()).rev().collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 0).collect::<Vec<bool>>(), order = (0..DRIVE_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 1).collect::<Vec<bool>>(), order = (0..DRIVE_CHANGES.len()).rev().collect::<Vec<usize>>())]
fn drive_changes_say_what_their_json_says(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let order = tc.draw(gs::permutations((0..DRIVE_CHANGES.len()).collect::<Vec<usize>>()));
    changes_agree(&flags, order, DRIVE_CHANGES);
}

/// [`LIBRARY_CHANGES`] say what their JSON says, in any order, on any drawn drives and store.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS], order = (0..LIBRARY_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = vec![false; FLAGS], order = (0..LIBRARY_CHANGES.len()).rev().collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 0).collect::<Vec<bool>>(), order = (0..LIBRARY_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 1).collect::<Vec<bool>>(), order = (0..LIBRARY_CHANGES.len()).rev().collect::<Vec<usize>>())]
fn library_changes_say_what_their_json_says(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let order = tc.draw(gs::permutations((0..LIBRARY_CHANGES.len()).collect::<Vec<usize>>()));
    changes_agree(&flags, order, LIBRARY_CHANGES);
}

/// [`SYNC_CHANGES`] say what their JSON says, in any order, on any drawn drives and store.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS], order = (0..SYNC_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = vec![false; FLAGS], order = (0..SYNC_CHANGES.len()).rev().collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 0).collect::<Vec<bool>>(), order = (0..SYNC_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 1).collect::<Vec<bool>>(), order = (0..SYNC_CHANGES.len()).rev().collect::<Vec<usize>>())]
fn sync_changes_say_what_their_json_says(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let order = tc.draw(gs::permutations((0..SYNC_CHANGES.len()).collect::<Vec<usize>>()));
    changes_agree(&flags, order, SYNC_CHANGES);
}

/// [`STORE_CHANGES`] say what their JSON says, in any order, on any drawn drives and store.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS], order = (0..STORE_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = vec![false; FLAGS], order = (0..STORE_CHANGES.len()).rev().collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 0).collect::<Vec<bool>>(), order = (0..STORE_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 1).collect::<Vec<bool>>(), order = (0..STORE_CHANGES.len()).rev().collect::<Vec<usize>>())]
fn store_changes_say_what_their_json_says(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let order = tc.draw(gs::permutations((0..STORE_CHANGES.len()).collect::<Vec<usize>>()));
    changes_agree(&flags, order, STORE_CHANGES);
}

/// [`JOBS_CHANGES`] say what their JSON says, in any order, on any drawn drives and store.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(flags = vec![true; FLAGS], order = (0..JOBS_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = vec![false; FLAGS], order = (0..JOBS_CHANGES.len()).rev().collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 0).collect::<Vec<bool>>(), order = (0..JOBS_CHANGES.len()).collect::<Vec<usize>>())]
#[hegel::explicit_test_case(flags = (0..FLAGS).map(|i| i % 2 == 1).collect::<Vec<bool>>(), order = (0..JOBS_CHANGES.len()).rev().collect::<Vec<usize>>())]
fn jobs_say_what_their_json_says(tc: TestCase) {
    let flags = tc.draw(gs::vecs(gs::booleans()).min_size(FLAGS).max_size(FLAGS));
    let order = tc.draw(gs::permutations((0..JOBS_CHANGES.len()).collect::<Vec<usize>>()));
    changes_agree(&flags, order, JOBS_CHANGES);
}

/// The detail waveform (`PWV5`) of the analysis file `ext` redrawn from `wav` (16-bit stereo at
/// 44.1 kHz, as [`recording`] writes it), 294 samples a column, as rekordbox draws it when its
/// timeline starts at the first sample. `track upgrade` tells where an MP3's timeline starts by
/// its waveform, and the fixture's was drawn from other audio: against that, a timeline a frame
/// (26 ms) early matched better at some delays (7 to 20 ms, and 60).
fn redraw_waveform(ext: &Path, wav: &[u8]) {
    let level: Vec<f64> =
        wav[44..].chunks_exact(4).map(|s| f64::from(i16::from_le_bytes([s[0], s[1]])).abs()).collect();
    let mut bytes = std::fs::read(ext).unwrap();
    let be = |b: &[u8], at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap()) as usize;
    let mut at = be(&bytes, 4);
    while &bytes[at..at + 4] != b"PWV5" {
        at += be(&bytes, at + 8);
    }
    let (body, end) = (at + be(&bytes, at + 4), at + be(&bytes, at + 8));
    let columns: Vec<f64> = (0..(end - body) / 2)
        .map(|i| level.get(i * 294..(i + 1) * 294).map_or(0.0, |w| w.iter().sum::<f64>() / 294.0))
        .collect();
    let loudest = columns.iter().copied().fold(0.0, f64::max);
    for (i, c) in columns.iter().enumerate() {
        // Height is bits 2-6 of each big-endian u16; the colour above it is left as it was.
        let p = body + 2 * i;
        let colour = u16::from_be_bytes([bytes[p], bytes[p + 1]]) & !(0x1f << 2);
        bytes[p..p + 2].copy_from_slice(&(colour | ((c / loudest * 31.0).round() as u16) << 2).to_be_bytes());
    }
    std::fs::write(ext, bytes).unwrap();
}

/// Lossless files of a drive's recordings in a folder on this computer: `track upgrade` (dry
/// run, then for real, keeping or removing the old files) says what its JSON says, and moves the
/// cues and grid by the encoder's delay the MP3 has over the lossless file. `check` with a player
/// that can't play a 96 kHz track says what its JSON says, and points to `pack --transcode`.
#[hegel::test(test_cases = 1, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(delay = 26usize, remove_old = true)]
fn upgrades_say_what_their_json_says(tc: TestCase) {
    let delay = tc.draw(gs::integers::<usize>().max_value(60));
    let remove_old = tc.draw(gs::booleans());
    let h = Env::from_template();
    let a = h.volumes.join("A");
    // A WAV standing in for the MP3 (decoders go by the content), late by an encoder's delay.
    let mp3 = recording(7919, 30.0, 44_100, 16, delay);
    std::fs::write(a.join(TRACK_1), &mp3).unwrap();
    let pdb = rekordbox_pdb::Database::from_file(a.join(crate::support::PDB)).unwrap();
    let track = pdb.tracks.iter().find(|t| t.file_path() == format!("/{TRACK_1}")).unwrap();
    redraw_waveform(&a.join(track.analyze_path().trim_start_matches('/')).with_extension("EXT"), &mp3);
    // Another recording, with no lossless file of it in the folder, clipped: full scale and over.
    let loud =
        loclib_mixid::test_util::synth(5.0, 31).into_iter().map(|s| ((s * 200_000.0) as i32).clamp(-32_768, 32_767));
    std::fs::write(
        a.join(crate::props_cli_human::TRACK_2),
        loclib_mixid::test_util::wav_of(11_025, 16, loud.collect::<Vec<_>>().into_iter()),
    )
    .unwrap();
    std::fs::write(a.join("Contents/Hi Res.wav"), recording(5, 1.0, 96_000, 24, 0)).unwrap();
    let mut ed = rekordbox_pdb::PdbEditor::from_file(a.join(crate::support::PDB)).unwrap();
    let hi = rekordbox_pdb::NewTrack {
        sample_rate: 96_000,
        sample_depth: 24,
        duration: 1,
        ..rekordbox_pdb::NewTrack::new("Hi Res", "/Contents/Hi Res.wav")
    };
    let id = ed.add_track(&hi).unwrap();
    let set = ed.create_playlist("Hi", 0, false).unwrap();
    ed.set_playlist_entries(set, &[id]).unwrap();
    ed.save(a.join(crate::support::PDB)).unwrap();
    std::fs::create_dir_all(h.home("Lossless")).unwrap();
    std::fs::write(h.home("Lossless/Demo Track 1.wav"), recording(7919, 30.0, 44_100, 16, 0)).unwrap();
    let j = twin(&h);
    let universe = universe(&h);
    let check: &[&str] = &["items.[].message", "unloadable.[].tracks.[].name"];
    step(&h, &j, &["check", "A", "--players", "CDJ-2000NXS", "--verbose"], check, &universe);
    step(&h, &j, &["check", "A", "--players", "CDJ-2000NXS"], &check[..1], &universe);
    step(
        &h,
        &j,
        &["check", "A", "--audio", "--verbose"],
        &["items.[].message", "loudness.[].tracks.[].track.name"],
        &universe,
    );
    // Decodable audio, analysed: the stored tempos and keys against what's heard, and the levels.
    step(
        &h,
        &j,
        &["doctor", "A", "--analyze", "--audio"],
        &["drive", "analysis.tempo.[].track.name", "analysis.key.[].track.name"],
        &universe,
    );
    let plan: &[&str] = &["plan.drive", "plan.upgrades.[].track.name", "plan.upgrades.[].to", "{plan.unmatched} lossy"];
    step(&h, &j, &["track", "upgrade", "A", "--from", "@h/Lossless", "--dry-run"], plan, &universe);
    let v = j.json(&["track", "upgrade", "A", "--from", path(&j.home("Lossless")), "--dry-run"]);
    let shift = v["plan"]["upgrades"][0]["shift_ms"].as_i64().unwrap_or_else(|| panic!("no upgrade: {v}"));
    assert!((shift + delay as i64).abs() <= 3, "a delay of {delay} ms moved the cues {shift} ms: {v}");
    let mut upgrade = vec!["track", "upgrade", "A", "--from", "@h/Lossless"];
    if remove_old {
        upgrade.push("--remove-old");
    }
    step(&h, &j, &upgrade, &["upgraded {plan.upgrades.#} track(s)"], &universe);
    step(&h, &j, &["track", "upgrade", "A", "--from", "@h/Lossless"], &[], &universe);
    assert_eq!(a.join(TRACK_1).exists(), !remove_old);
}
