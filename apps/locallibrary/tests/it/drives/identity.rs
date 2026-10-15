//! Naming drives and keeping track of them: adopt, rename, roles, retiring, the logbook, the
//! registry of known drives, copies and drives that lost their identity, and drive groups.

use crate::support::{Env, PDB, copy_dir, fixtures, line, path};

#[test]
fn a_drive_is_named_given_a_role_retired_and_forgotten() {
    let env = Env::new();
    let a = env.drive("A");
    // Mounted drives by volume name; none registered yet.
    assert!(line(&env.ok(&["drive"]), "2 tracks").starts_with("  A  "));
    assert!(env.ok(&["drive", "ls", "--known"]).contains("No drives registered yet"));

    // Adopting asks first, and a second adopt has nothing to do.
    let out = env.exits(3, &["adopt", path(&a), "--name", "Main"]);
    assert!(out.contains(&format!("give {} an identity, named \"Main\"", a.display())), "{out}");
    assert!(!a.join(".locallibrary").exists());
    assert!(env.ok(&["--yes", "adopt", path(&a), "--name", "Main"]).contains(&format!("{} is Main (", a.display())));
    assert!(env.ok(&["--yes", "drive", "adopt", "Main", "--name", "Main"]).contains("Main has its identity already"));
    // A drive named after `drive` shows it, as `drive info` does (free space aside: other
    // programs write to the disk meanwhile).
    let info =
        |args: &[&str]| env.ok(args).lines().filter(|l| !l.starts_with("  space ")).collect::<Vec<_>>().join("\n");
    assert!(info(&["drive", "info", "Main"]).contains("2 tracks"));
    assert_eq!(info(&["drive", "Main"]), info(&["drive", "info", "Main"]));

    // Renaming: the same name is nothing to do; a new one, with --label, also names the volume
    // (planned only here: it would relabel the disk holding the test's folder).
    assert!(env.ok(&["drive", "rename", "Main", "Main"]).contains("Main has that name already"));
    let out = env.ok(&["drive", "rename", "Main", "Club", "--label", "--dry-run"]);
    assert!(out.contains("rename Main to \"Club\"") && out.contains("to \"CLUB\""), "{out}");
    assert!(env.ok(&["--yes", "drive", "rename", "Main", "Club"]).contains("Main is now Club"));

    // A role, said with the right article, and taken off again.
    let out = env.ok(&["--yes", "drive", "role", "Club", "archive"]);
    assert!(
        out.contains("make Club (") && out.contains(") an archive drive") && out.contains("Club is an archive drive"),
        "{out}"
    );
    assert!(env.ok(&["--yes", "drive", "role", "Club", "backup"]).contains("Club is a backup drive"));
    assert!(env.fails(1, &["drive", "role", "Club", "dj"]).contains("unknown role \"dj\""));
    assert!(line(&env.ok(&["drive", "ls", "--known"]), "Club [backup]").contains("not linked to a library"));
    assert!(line(&env.ok(&["drive", "log", "Club"]), "role").ends_with("role        backup"));
    assert!(env.ok(&["--yes", "drive", "role", "Club", "none"]).contains("Club has no role"));

    // Retired drives are listed only when asked for.
    let out = env.ok(&["--yes", "drive", "retire", "Club"]);
    assert!(out.contains("syncs leave it out") && out.contains("Club is retired"), "{out}");
    assert!(env.ok(&["drive", "ls", "--known"]).contains("No drives registered yet"));
    assert!(line(&env.ok(&["drive", "list", "--retired"]), "Club").contains(" retired  "));
    let out = env.ok(&["drive", "log", "Club"]);
    assert!(out.starts_with("Club\n") && line(&out, "last seen").ends_with(path(&a)), "{out}");
    assert!(line(&out, "written").contains("0 B in 0 operation(s), 0 clean eject(s)"), "{out}");
    assert!(line(&out, "retired").starts_with("  retired     20"), "{out}");
    assert!(env.ok(&["--yes", "drive", "retire", "Club", "--undo"]).contains("Club is back in use"));
    assert!(!env.ok(&["drive", "log", "Club"]).contains("retired"));

    // Forgotten, it's gone from the registry, not from the drive.
    assert!(env.ok(&["--yes", "drive", "forget", "Club"]).contains("forgot Club ("));
    assert!(env.ok(&["drive", "ls", "--known", "--retired"]).contains("No drives registered yet"));
    assert!(a.join(".locallibrary/state.json").is_file());
}

#[test]
fn a_copied_drive_is_found_out_and_given_an_identity_of_its_own() {
    let env = Env::new();
    let main = env.pulled("Main");
    let copy = env.volumes.join("Copy");
    copy_dir(&main, &copy);
    let out = env.ok(&["status"]);
    assert!(line(&out, "drives claim to be Main").contains(path(&copy)), "{out}");

    let out = env.ok(&["drive", "reidentify", path(&copy), "--dry-run"]);
    assert!(out.contains("an identity of its own, named \"Main (copy)\""), "{out}");
    let out = env.ok(&["--yes", "drive", "reidentify", path(&copy)]);
    assert!(out.contains(&format!("{} is now Main (copy) (", copy.display())), "{out}");
    assert!(!env.ok(&["status"]).contains("claim to be"));
}

#[test]
fn a_drive_that_lost_its_identity_is_recognised_and_given_it_back() {
    let env = Env::new();
    let main = env.pulled("Main");
    // Erased elsewhere and given the same library again: no identity, the same content.
    let erased = env.volumes.join("USB");
    copy_dir(&main, &erased);
    std::fs::remove_dir_all(erased.join(".locallibrary")).unwrap();
    std::fs::rename(&main, env.outside("Main")).unwrap();

    let out = env.ok(&["drive", "recognise", path(&erased)]);
    assert!(out.contains("holds \"Main\" as of snapshot"), "{out}");
    assert!(out.contains("holds what Main held when last seen"), "{out}");
    assert!(out.contains(&format!("`locallibrary drive adopt {} --as \"Main\"`", erased.display())), "{out}");
    let out = env.ok(&["adopt", path(&erased), "--as", "Main", "--dry-run"]);
    assert!(out.contains(&format!("give {} the identity of Main (", erased.display())), "{out}");
    assert!(!erased.join(".locallibrary").exists());
    let out = env.ok(&["--yes", "adopt", path(&erased), "--as", "Main"]);
    assert!(out.contains(&format!("{} is Main again (", erased.display())), "{out}");

    // A drive the store has never seen anything like.
    let other = env.blank("Other");
    let out = env.ok(&["drive", "recognise", path(&other)]);
    assert!(out.contains(&format!("{} doesn't match any drive or snapshot the store knows", other.display())), "{out}");
}

#[test]
fn the_registry_says_how_far_behind_each_drive_is() {
    let started = chrono::Local::now();
    let env = Env::new();
    let main = env.pulled("Main");
    // Synced from an edit nobody pulled.
    env.ok(&["--yes", "playlist", "new", "Main", "New"]);
    let backup = env.blank("Backup");
    env.ok(&["--yes", "sync", "Main", path(&backup)]);
    env.ok(&["--yes", "adopt", path(&backup), "--name", "Backup"]);
    let out = env.ok(&["drive", "ls", "--known"]);
    assert!(line(&out, "Backup  Main").ends_with("had changes not in the store"), "{out}");
    env.ok(&["pull", "Main"]);
    assert!(line(&env.ok(&["drive", "ls", "--known"]), "Backup  Main").ends_with("up to date"));
    // Made with Backup unplugged, or the edit would reach it too.
    std::fs::rename(&backup, env.outside("Backup")).unwrap();
    env.ok(&["--yes", "playlist", "new", "Main", "Newer"]);
    std::fs::rename(env.outside("Backup"), &backup).unwrap();
    env.ok(&["pull", "Main"]);
    let out = env.ok(&["drive", "ls", "--known"]);
    assert!(line(&out, "Backup  Main").contains("1 snapshot(s) behind (held "), "{out}");

    // Unplugged: last seen, and under its library in the status, not plugged in.
    std::fs::rename(&backup, env.outside("Backup")).unwrap();
    // The day it was last seen (the test may run past midnight).
    let days = [started, chrono::Local::now()].map(|t| format!("last seen {}", t.format("%Y-%m-%d")));
    let out = env.ok(&["drive", "ls", "--known"]);
    assert!(days.iter().any(|d| line(&out, "Backup").contains(d.as_str())), "{out}");
    let out = env.ok(&["status"]);
    assert!(line(&out, "  Backup ").contains("not plugged in · 1 snapshot(s) behind (held "), "{out}");

    // rekordbox exported over the playlist made on the drive, after it was made.
    let fixture = fixtures().join("rekordcrate/demo_tracks").join(PDB);
    std::fs::copy(fixture, main.join(PDB)).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    std::fs::File::options().write(true).open(main.join(PDB)).unwrap().set_modified(later).unwrap();
    let out = env.ok(&["status"]);
    assert!(out.contains("rekordbox exported over 2 recorded edit(s): run `locallibrary refresh \"Main\"`"), "{out}");
}

/// Times print on this computer's calendar, whatever day it is in UTC, and `--json` keeps UTC.
#[test]
#[cfg_attr(windows, ignore = "chrono reads TZ only on Unix; Windows takes the zone from its settings")]
fn times_print_in_the_local_time_zone() {
    use chrono::Timelike;
    // A zone whose date isn't UTC's for at least the next hour: 12 hours behind before 11:00 UTC,
    // 14 ahead (no daylight saving in either) after.
    let now = chrono::Utc::now();
    let (tz, hours) = if now.hour() < 11 { ("Etc/GMT+12", -12) } else { ("Pacific/Kiritimati", 14) };
    let today = (now + chrono::Duration::hours(hours)).format("%Y-%m-%d").to_string();
    let env = Env::new();
    env.pulled("Main");
    let run = |args: &[&str]| {
        let out = env.command().env("TZ", tz).args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    };
    let out = run(&["drive", "info", "Main"]);
    assert!(line(&out, "  last seen ").contains(&format!(" {today} ")), "{out}");
    let info: serde_json::Value = serde_json::from_str(&run(&["--json", "drive", "info", "Main"])).unwrap();
    let at = chrono::DateTime::parse_from_rfc3339(info["last_known"]["at"].as_str().unwrap()).unwrap();
    assert_eq!(at.offset().local_minus_utc(), 0, "{info}");
}

/// The drive commands kept their old top-level names, hidden, and answer the same way.
#[test]
fn old_drive_command_names_answer_like_the_drive_ones() {
    let env = Env::new();
    let main = env.drive("Main");
    for args in [
        &["verify", path(&main)][..],
        &["tidy", path(&main), "--dry-run"],
        &["recordings", path(&main)],
        &["firmware", path(&main), "--remove"],
        &["fsck", path(&main), "--repair"],
        &["disks"],
        &["format", path(&main), "--dry-run"],
    ] {
        let (old, new) = (env.run(args), env.run(&[&["drive"], args].concat()));
        let error = |err: &str| err.lines().filter(|l| l.starts_with("error:")).collect::<Vec<_>>().join("\n");
        assert_eq!((old.code, &old.out, error(&old.err)), (new.code, &new.out, error(&new.err)), "{args:?}");
    }
}

#[test]
fn drives_of_one_library_make_a_group_that_syncs_together() {
    let env = Env::new();
    let main = env.drive("Main");
    assert!(env.ok(&["drive", "groups"]).contains("No mounted drives are linked to a library."));
    env.ok(&["--yes", "adopt", path(&main), "--name", "Main"]);
    env.ok(&["pull", "Main"]);
    // One drive of the library mounted: nothing to sync it with.
    assert!(env.fails(1, &["sync"]).contains("two or more mounted drives of one library"));

    let backup = env.blank("Backup");
    let out = env.ok(&["--yes", "sync", "Main", path(&backup)]);
    assert!(out.contains("Main → Backup"), "{out}");
    env.ok(&["--yes", "adopt", path(&backup), "--name", "Backup"]);
    env.ok(&["--yes", "drive", "role", "Main", "primary"]);
    let archive = env.volumes.join("Archive");
    copy_dir(&backup, &archive);
    env.ok(&["--yes", "drive", "reidentify", path(&archive), "--name", "Archive"]);
    env.ok(&["--yes", "drive", "role", "Archive", "archive"]);

    let out = env.ok(&["drive", "groups"]);
    assert!(out.starts_with("Main\n"), "{out}");
    assert!(line(&out, path(&main)).ends_with(" (primary)"), "{out}");
    assert!(line(&out, path(&backup)).ends_with(path(&backup)), "{out}");
    assert!(out.contains("Archive (archive or retired: not synced)"), "{out}");
    let out = env.ok(&["status"]);
    assert!(line(&out, "  Archive  ").contains("archive"), "{out}");

    // The group syncs from its newest drive, leaving the archive out; ejecting a folder fails
    // without failing the sync. (Backup is unplugged for the edit, which would reach it too.)
    std::fs::rename(&backup, env.outside("Backup")).unwrap();
    env.ok(&["--yes", "playlist", "new", "Main", "New"]);
    std::fs::rename(env.outside("Backup"), &backup).unwrap();
    let out = env.ok(&["--yes", "sync", "--eject"]);
    assert!(out.starts_with("Main is the newest") && out.contains("Backup"), "{out}");
    assert!(line(&out, &format!("couldn't eject {}", backup.display())).contains("there's nothing to eject"), "{out}");
    assert!(!out.contains(&format!("couldn't eject {}", archive.display())), "{out}");
    assert!(env.ok(&["playlist", "ls", "Backup"]).contains("New"));
}
