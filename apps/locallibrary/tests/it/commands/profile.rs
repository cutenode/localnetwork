//! `locallibrary profile` in a terminal: the report from a collection alone and from sessions,
//! and the choices that shape it, each said as it's made.

use crate::support::{Env, path, play};

#[test]
fn a_collection_with_no_sets_is_described_as_it_is() {
    let env = Env::ascii();
    let demo = env.demo("Demo");
    let d = path(&demo);
    let out = env.ok(&["profile", d]);
    assert!(out.contains("  no sets found: describing the collection - built "), "{out}");
    for line in [
        "  no sets declared, and no sessions with enough tracks (`--min-tracks` sets the bar)\n",
        "  needs sets of 6 or more tracks with BPMs\n",
        "  needs sets\n",
        "  none tagged\n",
        "  shares of 2 tracks with an artist - 1 different\n",
        "  no release years tagged\n",
        "  no track is in two or more sets yet (`profile pin` names one)\n",
        "  hot cues on 0% - memory cues on 0% - loops on 0%  of 2 tracks\n",
        "  2 tracks - 0 playlists in 0 folders - 0% rated - 0% colour-tagged\n",
        "  from Demo\n",
    ] {
        assert!(out.contains(line), "{line:?} in\n{out}");
    }
    let out = env.ok(&["profile", d, "--since", "2020", "--until", "2020", "-s", "sets"]);
    assert!(out.contains("  no sets or sessions in these dates\n"), "{out}");
    let out = env.ok(&["profile", d, "--no-analysis", "-s", "energy,prep"]);
    assert!(out.contains("  no phrase analysis read") && out.contains("  no cues read"), "{out}");
}

#[test]
fn choices_are_said_as_theyre_made_and_shown_in_the_report() {
    let env = Env::ascii();
    let club = env.club("Club");
    // Two sessions that open and close alike, and play 1 into 2 both times.
    play(&club, "HISTORY 001", &[1, 2, 3, 4, 5, 6]);
    play(&club, "HISTORY 002", &[1, 2, 9, 10, 12, 6]);
    let d = path(&club);
    env.ok(&["pull", d, "--library", "Club", "--new-library"]);
    env.ok(&["--yes", "settings", "set", "quantize=off"]);

    let out = env.ok(&["profile", "sections"]);
    assert!(out.contains("  o signature   tracks you play again and again, by name\n"), "{out}");
    let out = env.ok(&["profile", "include", "signature,artists"]);
    assert!(out.starts_with("v sections updated\n  the document includes: summary, sets,"), "{out}");
    assert!(env.ok(&["profile", "exclude", "artists"]).contains("v sections updated"));
    let out = env.ok(&["profile", "pin", "about to be", d]);
    assert_eq!(out, "v pinned [a]pendics.shuffle - B1 - About To Be\n");
    env.ok(&["profile", "pin", "Someone - Dubplate", "--exact"]);
    // A signature track pinned is listed with the pins, not again among the rest.
    env.ok(&["profile", "pin", "deluxe cynic", d]);
    let out = env.ok(&["profile", "hide", "a2 - in charge", d]);
    assert!(out.contains("In Charge won't be named (it still counts in the numbers)"), "{out}");
    assert_eq!(env.ok(&["profile", "hide", "Someone", "--artist"]), "v Someone won't be named\n");
    assert_eq!(env.ok(&["profile", "headline", "Techno warm-ups"]), "v headline set\n");
    env.ok(&["profile", "alias", "electronic", "Electronica"]);
    assert_eq!(env.ok(&["profile", "precision", "rounded"]), "v precision set\n");

    let out = env.ok(&["profile", d, "-s", "summary,sets,signature"]);
    assert!(out.contains("  Techno warm-ups\n  (your headline; generated: "), "{out}");
    assert!(out.contains("  v Plays with quantize off  "), "{out}");
    assert!(out.contains("  archived sessions first seen on "), "{out}");
    assert!(out.contains("  * [a]pendics.shuffle - B1 - About To Be  1/2 sessions\n"), "{out}");
    assert!(out.contains("  * Someone - Dubplate  not in these sessions\n"), "{out}");
    assert!(out.contains("      2/2 sessions  [a]pendics.shuffle - A2 - In Charge  hidden\n"), "{out}");
    assert!(out.contains("moves played again and again\n      2x  [a]pendics.shuffle - A1 - It's In The Eyes ->"));
    assert!(out.contains("  openers: [a]pendics.shuffle - A1 - It's In The Eyes (2x)\n"), "{out}");
    assert!(out.contains("  closers: [a]pendics.shuffle - Deluxe Cynic (2x)\n"), "{out}");
    assert!(out.contains("  * [a]pendics.shuffle - Deluxe Cynic  2/2 sessions\n"), "{out}");
    assert!(!out.contains("2/2 sessions  [a]pendics.shuffle - Deluxe Cynic"), "{out}");
    assert!(out.contains("  never named: 1 track(s), 1 artist(s)\n"), "{out}");

    let out = env.ok(&["profile", "prefs"]);
    assert!(out.contains("Techno warm-ups") && out.contains("electronic -> Electronica"), "{out}");
    let err = env.fails(1, &["profile", "unpin", "a"]);
    assert!(err.contains("\"a\" matches 3 pinned entries; be more specific"), "{err}");
    env.ok(&["profile", "precision", "exact"]);
    assert!(env.ok(&["profile", "prefs"]).contains("exact"));
    assert_eq!(env.ok(&["profile", "unhide", "some", "--artist"]), "v updated\n");
    assert_eq!(env.ok(&["profile", "default-sections"]).lines().next(), Some("v back to the default sections"));
    // Forgetting every choice asks first.
    let err = env.fails(3, &["profile", "reset"]);
    assert!(err.contains("not confirmed"), "{err}");
    assert_eq!(env.ok(&["--yes", "profile", "reset"]).lines().next(), Some("v profile choices reset"));

    let file = env.outside("profile.json");
    let out = env.ok(&["profile", "document", d, "-o", path(&file)]);
    assert_eq!(out, format!("v wrote {}\n", file.display()));
}
