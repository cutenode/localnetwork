//! `settings`: the settings profile listed and set, compared with a drive, and written to it.

use crate::support::{Env, line};

/// The settings profile: listed, set, compared with a drive and written to it.
#[test]
fn settings_are_listed_compared_and_written() {
    let env = Env::new();
    env.demo("A");
    env.ok(&["--yes", "drive", "adopt", "A", "--name", "Main"]);
    let schema = env.ok(&["settings", "schema"]);
    assert!(schema.starts_with("Player\n") && schema.contains("\nDisplay\n"), "{schema}");
    assert!(line(&schema, "quantize ").contains("off | on"), "{schema}");
    assert!(schema.ends_with("Defaults underlined. Set with `locallibrary settings set KEY=VALUE`.\n"));
    assert!(env.ok(&["settings"]).starts_with("The profile is empty: drives keep whatever settings they have."));

    let set = env.ok(&["--yes", "settings", "set", "quantize=off", "lcd_brightness=5"]);
    assert!(set.contains("✓ quantize = off for all drives\n✓ lcd_brightness = 5 for all drives"), "{set}");
    env.ok(&["--yes", "settings", "set", "--drive", "Main", "sync=on"]);
    let profile = env.ok(&["settings"]);
    assert!(profile.starts_with("All drives\n  lcd_brightness               5\n  quantize                     off\n"));
    assert!(line(&profile, "Main ").starts_with("Main ") && profile.ends_with("  sync                         on\n"));

    let show = env.ok(&["settings", "Main"]);
    assert!(show.contains("  quantize                     on → off\n"), "{show}");
    assert!(show.contains("  sync                         off → on (drive override)"), "{show}");
    assert!(show.contains("3 setting(s) differ; `locallibrary settings apply` fixes them"), "{show}");
    let plan = env.ok(&["settings", "apply", "Main", "--dry-run"]);
    assert!(plan.starts_with("✓ Main: would update 3 setting(s)\n    lcd_brightness               3 → 5\n"), "{plan}");
    let done = env.ok(&["--yes", "settings", "apply", "Main"]);
    assert!(done.contains("✓ Main: updated 3 setting(s)"), "{done}");
    assert!(env.ok(&["settings", "show", "Main"]).ends_with("✓ matches the profile\n"));
    assert_eq!(env.ok(&["settings", "apply", "Main"]).trim(), "✓ Main: already matches");

    let unset = env.ok(&["--yes", "settings", "unset", "sync", "--drive", "Main"]);
    assert!(!unset.contains("sync    "), "{unset}");
    let import = env.ok(&["settings", "import", "Main", "--dry-run"]);
    assert!(import.contains("    auto_cue                     (not set) → on"), "{import}");
    assert!(env.ok(&["--yes", "settings", "import", "Main"]).contains("✓ imported "));
    assert!(env.ok(&["settings", "import", "Main"]).contains("the profile has those values already"));

    // A settings file the drive doesn't have is made only when asked.
    std::fs::remove_file(env.volumes.join("A/PIONEER/DEVSETTING.DAT")).unwrap();
    assert!(env.ok(&["settings", "show", "Main"]).contains("\n  DEVSETTING.DAT missing\n"));
    let plan = env.ok(&["settings", "apply", "Main", "--create-missing", "--dry-run"]);
    assert!(plan.ends_with("    would create DEVSETTING.DAT from the profile\n"), "{plan}");
    let done = env.ok(&["--yes", "settings", "apply", "Main", "--create-missing"]);
    assert!(done.ends_with("    created DEVSETTING.DAT from the profile\n"), "{done}");
}
