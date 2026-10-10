//! `players`: the models `check` uses when none are given, and what each model reads.

use crate::support::{Env, line};

#[test]
fn remembered_models_are_set_shown_and_forgotten() {
    let env = Env::new();
    assert!(env.ok(&["players"]).contains("No player models remembered."));
    assert!(!env.store.exists(), "showing never makes a store");

    let out = env.ok(&["players", "set", "CDJ-3000X,CDJ-2000NXS2", "--dry-run"]);
    assert!(out.contains("CDJ-3000X"), "{out}");
    assert!(env.ok(&["players"]).contains("No player models remembered."));
    let out = env.ok(&["--yes", "players", "set", "CDJ-3000X,CDJ-2000NXS2"]);
    assert!(out.contains("remembered") && line(&out, "Every library").ends_with("CDJ-3000X, CDJ-2000NXS2"), "{out}");
    assert!(env.ok(&["--yes", "players", "set", "CDJ-3000X,CDJ-2000NXS2"]).contains("remembered already"));

    // For one library, under its name.
    env.pulled("Main");
    env.ok(&["--yes", "players", "set", "XDJ-AZ", "--library", "Main"]);
    assert!(line(&env.ok(&["players", "show"]), "Main").ends_with("XDJ-AZ"));
    assert!(env.ok(&["--yes", "players", "clear", "--library", "Main"]).contains("forgotten"));
    assert!(!env.ok(&["players"]).contains("XDJ-AZ"));
    env.ok(&["--yes", "players", "clear"]);
    assert!(env.ok(&["players"]).contains("No player models remembered."));
}

#[test]
fn each_model_says_what_it_reads() {
    let env = Env::new();
    let out = env.ok(&["players", "models"]);
    assert!(out.starts_with("Model "), "{out}");
    let cdj = line(&out, "CDJ-3000X ");
    assert!(cdj.contains("OneLibrary") && cdj.contains("FLAC 96 kHz"), "{cdj}");
    assert!(line(&out, "CDJ-2000NXS2 ").contains("export.pdb"), "{out}");
    assert!(line(&out, "DJM-").contains("settings and firmware only"), "{out}");
    assert!(out.contains("* some values are inferred"), "{out}");

    let out = env.ok(&["players", "models", "cdj-3000x"]);
    assert!(out.starts_with("CDJ-3000X\n") && line(&out, "reads").ends_with("OneLibrary"), "{out}");
    assert!(line(&out, "drives").contains("FAT32") && line(&out, "drives").contains("not NTFS"), "{out}");
    assert!(line(&out, "FLAC").contains("-bit"), "lossless formats give their bit depths: {out}");
    assert!(line(&out, "source").contains("CDJ-3000X Instruction Manual"), "{out}");

    let out = env.ok(&["players", "models", "CDJ-3000"]);
    assert!(line(&out, "drives").contains(", exFAT (firmware 1.20+) on MBR"), "{out}");
    let out = env.ok(&["players", "models", "CDJ-2000"]);
    assert!(line(&out, "drives").contains("FAT16, FAT32, HFS+ on MBR"), "{out}");
    assert!(out.contains("(which formats it plays is inferred)"), "{out}");

    let r = env.run(&["players", "models", "CDJ-9000"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("not a model locallibrary knows: CDJ-9000"), "{}", r.err);
}
