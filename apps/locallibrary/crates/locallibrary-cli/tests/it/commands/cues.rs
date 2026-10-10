//! `locallibrary cues`: a track's hot cues, memory cues and loops, listed and edited in its
//! analysis files.

use crate::support::{Env, copy_dir, fixtures, path};

#[test]
fn cues_are_set_renamed_recoloured_and_deleted() {
    let env = Env::ascii();
    let drive = env.demo("Demo");
    let d = path(&drive);
    let list = |track: &str| env.ok(&["cues", d, "--track", track]);
    assert!(list("1").contains("  no cues"), "{}", list("1"));

    // Hot cue B as an 8-beat loop: track 1 is 128 BPM, so the loop is 3.75 s long.
    let set = ["cues", "set", d, "--track", "1", "--hot", "B", "--at", "1:02.5", "--loop", "8"];
    let named = [&set[..], &["--name", "Drop", "--colour", "red"]].concat();
    let out = env.ok(&[&named[..], &["--dry-run"]].concat());
    assert!(out.contains("change the cues of 1 track(s)"), "{out}");
    assert!(out.contains("+ B  1:02.500  loop 8 beats to 1:06.250  \"Drop\"  #e62828"), "{out}");
    assert!(out.contains("2 analysis file(s)"), "{out}");
    assert!(list("1").contains("no cues"), "a dry run writes nothing");
    // Not confirmed without a terminal or --yes: exit status 3, and nothing written.
    let err = env.fails(3, &named);
    assert!(err.contains("not confirmed"), "{err}");
    let out = env.ok(&[&["--yes"], &named[..]].concat());
    assert!(out.contains("updated the cues of 1 track(s) (2 analysis file(s))"), "{out}");
    assert!(out.contains(&format!("undo with: locallibrary undo {d:?}")), "{out}");
    let out = env.ok(&[&["--yes"], &named[..]].concat());
    assert!(out.contains("nothing to change"), "{out}");

    // A memory cue on beat 129, named and recoloured by its time.
    env.ok(&["--yes", "cues", "set", d, "--track", "1", "--memory", "--beat", "129"]);
    env.ok(&["--yes", "cues", "rename", d, "--track", "1", "--memory-at", "1:00.025", "Break"]);
    env.ok(&["--yes", "cues", "colour", d, "--track", "1", "--memory-beat", "129", "green"]);
    let out = list("1");
    assert!(out.contains("<>  1:00.025  beat 129  \"Break\"  green"), "{out}");
    assert!(out.contains("B  1:02.500  loop 8 beats to 1:06.250  \"Drop\"  #e62828"), "{out}");

    // Copied onto track 2, then taken off track 1 one at a time and then all at once.
    let out = env.ok(&["--yes", "cues", "copy", d, "--from", "1", "--to", "2"]);
    assert!(out.contains("updated the cues of 1 track(s)"), "{out}");
    assert_eq!(env.json(&["cues", "list", d, "--track", "2"])[0]["cues"].as_array().unwrap().len(), 2);
    let out = env.ok(&["--yes", "cues", "rm", d, "--track", "1", "--hot", "B"]);
    assert!(out.contains("- B  1:02.500"), "{out}");
    env.ok(&["--yes", "cues", "rm", d, "--query", "Demo Track 1", "--all"]);
    assert!(list("1").contains("no cues"), "{}", list("1"));
}

/// A track whose analysis has no .EXT keeps its cues in the .DAT, which holds no names: the plan
/// says so, and says it again when there's nothing to change.
#[test]
fn a_track_without_an_ext_is_warned_about() {
    let env = Env::ascii();
    let drive = env.demo("Demo");
    let d = path(&drive);
    std::fs::remove_file(drive.join("PIONEER/USBANLZ/P053/0001D21F/ANLZ0000.EXT")).unwrap();
    let set = ["--yes", "cues", "set", d, "--track", "2", "--hot", "G", "--beat", "100", "--name", "Late"];
    let out = env.ok(&set);
    assert!(out.contains("+ G  ") && out.contains("! ") && out.contains("no .EXT"), "{out}");
    let out = env.ok(&set);
    assert!(out.contains("no .EXT") && out.contains("nothing to change"), "{out}");
}

#[test]
fn a_cue_command_needs_a_drive_and_a_time_it_can_read() {
    let env = Env::ascii();
    let err = env.fails(1, &["cues"]);
    assert!(err.contains("give a library or drive and a track"), "{err}");
    let err = env.fails(2, &["cues", "set", "Demo", "--track", "1", "--hot", "A", "--at", "1:75"]);
    assert!(err.contains("not a time: \"1:75\""), "{err}");
}

#[test]
fn a_drive_with_onelibrary_has_its_cues_edited_there_too() {
    let env = Env::ascii();
    let drive = env.demo("Demo");
    let d = path(&drive);
    env.ok(&["--yes", "convert", "--to", "onelibrary", d]);
    let out = env.ok(&["--yes", "cues", "set", d, "--track", "1", "--hot", "B", "--beat", "9"]);
    assert!(out.contains("    + B  0:03.775  beat 9\n    2 analysis file(s) and exportLibrary.db\n"), "{out}");
    assert!(out.contains("  exportLibrary.db: 0 cue(s) updated, 1 added, 0 removed\n"), "{out}");

    // rekordbox writes the analysis files again, without the cue: the two disagree.
    copy_dir(&fixtures().join("rekordcrate/demo_tracks/PIONEER/USBANLZ"), &drive.join("PIONEER/USBANLZ"));
    let out = env.ok(&["cues", d, "--track", "1"]);
    assert!(
        out.contains("  no cues\n! exportLibrary.db has 1 cue(s) for this track; editing its cues makes it"),
        "{out}"
    );
}
