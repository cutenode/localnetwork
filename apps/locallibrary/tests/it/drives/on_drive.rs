//! What locallibrary puts on a drive besides the library, and takes off it: contact details,
//! the name players show, firmware updates, recordings made on the player, and the backups
//! made before each change.

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::Value;

use crate::support::{Env, line, path};

/// Recordings copied off a drive into a folder, then copied there again with some made since:
/// each one copied before is already there, so it isn't copied again and the run says why (in
/// text as in `--json`) and exits as a partial change does; the new ones are copied.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn recordings_copied_off_again_are_already_there(tc: TestCase) {
    let env = Env::new();
    let main = env.drive("Main");
    let to = env.outside("Recordings");
    let record = |n: usize| {
        let name = format!("ALPHATHETA REC/REC{n:03}.WAV");
        std::fs::create_dir_all(main.join("ALPHATHETA REC")).unwrap();
        std::fs::write(main.join(&name), format!("recording {n}").repeat(n)).unwrap();
        name
    };
    let before: Vec<String> = (1..=tc.draw(gs::integers::<usize>().min_value(1).max_value(3))).map(record).collect();
    env.ok(&["--yes", "drive", "recordings", path(&main), "--copy-to", path(&to)]);
    let since: Vec<String> =
        (before.len() + 1..=before.len() + tc.draw(gs::integers::<usize>().max_value(2))).map(record).collect();
    let json = tc.draw(gs::booleans());
    let mut args = vec!["--yes", "drive", "recordings", path(&main), "--copy-to", path(&to)];
    if json {
        args.insert(0, "--json");
    }
    let r = env.run(&args);
    assert_eq!(r.code, 5, "{}\n{}", r.out, r.err);
    if json {
        let v: Value = serde_json::from_str(&r.out).unwrap();
        let failed: Vec<&str> =
            v["report"]["failed"].as_array().unwrap().iter().map(|f| f[0].as_str().unwrap()).collect();
        let copied: Vec<&str> =
            v["report"]["collected"].as_array().unwrap().iter().map(|c| c["recording"].as_str().unwrap()).collect();
        assert_eq!((failed, copied), (strs(&before), strs(&since)), "{v}");
        return;
    }
    for name in &before {
        let said = line(&r.out, &format!("{name}: "));
        assert!(said.starts_with("! ") && said.ends_with("already exists"), "{name}: {}", r.out);
    }
    for name in &since {
        assert!(line(&r.out, &format!("{name} →")).starts_with("✓ "), "{name}: {}", r.out);
    }
}

/// Contact details for a drive, with the playlist players show first or without: the plan names
/// the file and the playlist asked for, and a dry run writes nothing; written, the file holds the
/// details and the drive has the playlist exactly when asked.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn contact_details_are_written_as_asked(tc: TestCase) {
    let env = Env::new();
    let main = env.drive("Main");
    let contact = tc.draw(gs::sampled_from(&["sam@example.com", "+44 7700 900123", "DJ Sam"][..]));
    let playlist = tc.draw(gs::booleans());
    let mut args = vec!["drive", "contact", path(&main), contact];
    if playlist {
        args.push("--playlist");
    }
    let named = format!("IF FOUND: {contact}");
    let out = env.ok(&[&args[..], &["--dry-run"]].concat());
    assert!(out.contains("IF FOUND.txt"), "{out}");
    assert_eq!(out.contains(&format!("add the playlist {named:?}")), playlist, "{out}");
    assert!(!main.join("IF FOUND.txt").exists());
    args.insert(0, "--yes");
    env.ok(&args);
    assert!(std::fs::read_to_string(main.join("IF FOUND.txt")).unwrap().contains(contact));
    assert_eq!(env.ok(&["playlist", "ls", path(&main)]).contains(&named), playlist);
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[test]
fn contact_details_go_in_a_file_and_a_playlist_players_show_first() {
    let env = Env::new();
    let main = env.drive("Main");
    let out = env.ok(&["drive", "contact", path(&main), "sam@example.com", "--playlist", "--dry-run"]);
    assert!(out.contains("IF FOUND.txt") && out.contains("add the playlist \"IF FOUND: sam@example.com\""), "{out}");
    assert!(!main.join("IF FOUND.txt").exists());
    let out = env.ok(&["--yes", "drive", "contact", path(&main), "sam@example.com", "--playlist"]);
    assert!(line(&out, "wrote").ends_with("IF FOUND.txt"), "{out}");
    assert!(out.contains("`locallibrary edits replay` puts it back"), "{out}");
    assert!(std::fs::read_to_string(main.join("IF FOUND.txt")).unwrap().contains("sam@example.com"));
    assert!(env.ok(&["playlist", "ls", path(&main)]).contains("IF FOUND: sam@example.com"));
}

#[test]
fn the_name_players_show_is_set_in_onelibrary() {
    let env = Env::new();
    let main = env.drive("Main");
    assert!(line(&env.ok(&["drive", "player", path(&main)]), "OneLibrary").ends_with("none"));
    env.ok(&["--yes", "convert", path(&main), "--to", "onelibrary"]);
    let out = env.ok(&["drive", "player", path(&main)]);
    assert!(line(&out, "OneLibrary").contains("not set (players show NO DEVICE NAME), colour 0"), "{out}");

    let out = env.ok(&["drive", "player", path(&main), "--name", "MAIN A", "--colour", "6", "--dry-run"]);
    assert!(out.contains("players show Main as \"MAIN A\"") && out.contains("in colour 6"), "{out}");
    env.ok(&["--yes", "drive", "player", path(&main), "--name", "MAIN A", "--colour", "6"]);
    let out = env.ok(&["drive", "player", path(&main)]);
    assert!(line(&out, "OneLibrary").contains("name \"MAIN A\", colour 6"), "{out}");

    // What rekordbox keeps for the drive, shown beside it.
    let backup = main.join("PIONEER/DeviceLibBackup");
    std::fs::create_dir_all(&backup).unwrap();
    let info = r#"{"uuid":"x","info":[{"device_id":"x","device_name":"MAIN","background_color":"3",
        "background_color_libplus":"4","device_filesystem":"FAT32","backup_pc_id":"1","backup_pc_name":"Studio"}]}"#;
    std::fs::write(backup.join("rbDevLibBaInfo_1.json"), info).unwrap();
    let out = env.ok(&["drive", "player", path(&main)]);
    assert!(line(&out, "rekordbox").ends_with("name \"MAIN\", colour 3 (OneLibrary 4), Studio set it up"), "{out}");
}

/// Regression: both stopped with `exportLibrary.db` damaged, exit statuses 4 and 5.
#[test]
fn a_drive_with_a_name_of_its_own_verifies_intact() {
    let env = Env::new();
    let main = env.drive("Main");
    env.ok(&["--yes", "convert", path(&main), "--to", "onelibrary"]);
    env.ok(&["pull", path(&main), "--library", "Test", "--new-library"]);
    assert!(env.ok(&["drive", "verify", path(&main)]).contains("Main is intact"));

    env.ok(&["--yes", "drive", "player", path(&main), "--name", "OTHER"]);
    let out = env.ok(&["drive", "verify", path(&main)]);
    assert!(out.contains("Main is intact") && !out.contains("damaged"), "{out}");

    let spare = env.blank("Spare");
    let out = env.ok(&["--yes", "provision", path(&spare), "--like", "Test", "--player-name", "SPARE"]);
    assert!(out.contains("Spare is intact"), "{out}");
}

#[test]
fn firmware_is_checked_against_the_models_rules_before_it_is_copied() {
    let env = Env::new();
    let main = env.drive("Main");
    let download = env.outside("firmware.UPD");
    std::fs::write(&download, "update").unwrap();
    // Not named as an update file for any model: a problem, so nothing is copied.
    let out = env.exits(4, &["drive", "firmware", path(&main), path(&download)]);
    assert!(out.contains("firmware.UPD: 6 B") && out.contains("isn't named like an AlphaTheta update file"), "{out}");
    assert!(!main.join("firmware.UPD").exists());
    // --force copies it anyway.
    let out = env.ok(&["--yes", "drive", "firmware", path(&main), path(&download), "--force"]);
    assert!(line(&out, "wrote").ends_with("firmware.UPD"), "{out}");
    assert!(out.contains("`locallibrary drive firmware <drive> --remove` removes it"), "{out}");

    // A file named for a model says which; one already on the drive would be replaced.
    let cdj = env.outside("CDJ3Kv321.UPD");
    std::fs::write(&cdj, "update").unwrap();
    let out = env.exits(0, &["drive", "firmware", path(&main), path(&cdj), "--replace", "--dry-run"]);
    assert!(out.contains("CDJ3Kv321.UPD update for the CDJ-3000: 6 B"), "{out}");
    assert!(out.contains("  remove firmware.UPD"), "{out}");
    // Some models take the drive in one socket only.
    let az = env.outside("XDJAZv110.UPD");
    std::fs::write(&az, "update").unwrap();
    let out = env.exits(0, &["drive", "firmware", path(&main), path(&az), "--dry-run"]);
    assert!(out.contains("insert the drive in the XDJ-AZ USB1 socket"), "{out}");

    let out = env.ok(&["drive", "firmware", path(&main), "--remove", "--dry-run"]);
    assert!(out.contains("delete firmware.UPD from Main"), "{out}");
    assert!(env.ok(&["--yes", "drive", "firmware", path(&main), "--remove"]).contains("removed firmware.UPD"));
    assert!(!main.join("firmware.UPD").exists());
    assert!(env.ok(&["drive", "firmware", path(&main), "--remove"]).contains("no update files on the drive"));
}

#[test]
fn recordings_made_on_the_player_are_listed_and_copied_off() {
    let env = Env::new();
    let main = env.drive("Main");
    assert!(env.ok(&["drive", "recordings", path(&main)]).contains("No recordings on this drive."));
    let to = env.outside("Recordings");
    assert!(
        env.ok(&["drive", "recordings", path(&main), "--copy-to", path(&to)]).contains("no recordings on this drive")
    );

    let rec = main.join("ALPHATHETA REC/REC001.WAV");
    std::fs::create_dir_all(rec.parent().unwrap()).unwrap();
    std::fs::write(&rec, "cut off before its header").unwrap();
    // A file too short for a WAV header has no length.
    let out = env.ok(&["drive", "recordings", path(&main)]);
    assert!(line(&out, "REC001.WAV").contains("  ?  25 B  ALPHATHETA REC/REC001.WAV"), "{out}");

    let out = env.ok(&["drive", "recordings", path(&main), "--copy-to", path(&to), "--dry-run"]);
    assert!(out.contains("copy 1 recording(s) (25 B) from Main"), "{out}");
    assert!(!to.exists());
    // Deleting from the drive asks first.
    let r = env.run(&["drive", "recordings", path(&main), "--copy-to", path(&to), "--delete"]);
    assert_eq!(r.code, 3, "{}", r.out);
    assert!(rec.exists());

    let out = env.ok(&["drive", "recordings", path(&main), "--copy-to", path(&to), "--tracklist", "HISTORY 001"]);
    assert!(line(&out, "tracklist:").contains(path(&to)) && !out.contains("deleted"), "{out}");
    let out = env.ok(&[
        "--yes",
        "drive",
        "recordings",
        path(&main),
        "--copy-to",
        path(&env.outside("Again")),
        "--tracklist",
        "latest",
        "--delete",
    ]);
    assert!(line(&out, "REC001.WAV →").ends_with("(deleted from the drive)"), "{out}");
    assert!(!rec.exists());
}

#[test]
fn every_change_leaves_a_backup_listed_newest_first() {
    let env = Env::new();
    let main = env.drive("Main");
    let other = env.drive("Other");
    assert_eq!(env.ok(&["drive", "backups", path(&main)]), "no backups\n");
    env.ok(&["--yes", "drive", "contact", path(&main), "sam@example.com"]);
    env.ok(&["--yes", "convert", path(&main), "--to", "onelibrary"]);
    let out = env.ok(&["drive", "backups", path(&main)]);
    let reasons: Vec<&str> = out.lines().map(|l| l.split("  ").nth(2).unwrap()).collect();
    assert_eq!(reasons, ["before converting to exportLibrary.db", "before writing the contact"], "{out}");
    assert!(out.lines().all(|l| l.ends_with("file(s)")), "{out}");

    // Of every mounted drive, each under its name.
    let out = env.ok(&["drive", "backups"]);
    assert!(out.contains(&format!("Main  {}\n", main.display())), "{out}");
    assert!(out.contains(&format!("Other  {}\nno backups", other.display())), "{out}");
}
