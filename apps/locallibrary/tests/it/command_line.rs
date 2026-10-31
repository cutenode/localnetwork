//! Reading the command line and answering it: what a typo is taken for, where moved commands went,
//! the commands that need something this run doesn't have, colour and symbols, and errors.

use crate::support::{Env, line, path};

/// A typo of a command that moved into a group suggests its new spelling, never the old one.
#[test]
fn a_typo_suggests_where_a_moved_command_went() {
    let env = Env::new();
    let err = env.fails(2, &["histroy"]);
    assert!(err.contains("tip: a similar subcommand exists: 'plays lists'"), "{err}");
    let err = env.fails(2, &["statsu"]);
    assert!(err.contains("'status'") && err.contains("'plays stats'") && !err.contains("'stats'"), "{err}");
}

/// A group that takes a drive in place of a subcommand (`settings Main`) takes a typo of a
/// subcommand for what it is; a word like no subcommand is looked for as a drive.
#[test]
fn a_typo_in_place_of_a_drive_names_the_command_meant() {
    let env = Env::new();
    let err = env.fails(2, &["settings", "shwo"]);
    assert!(err.contains("no drive or command \"shwo\"; did you mean `locallibrary settings show`?"), "{err}");
    let err = env.fails(1, &["art", "qqqqqq"]);
    assert!(err.contains("no mounted drive matches \"qqqqqq\"") && !err.contains("did you mean"), "{err}");
}

#[cfg(feature = "tui")]
#[test]
fn the_player_needs_a_terminal() {
    let err = Env::new().fails(1, &["tui"]);
    assert!(err.contains("error: the player needs a terminal"), "{err}");
}

#[test]
fn the_rpc_contract_is_written_where_asked() {
    let env = Env::new();
    let dir = env.outside("contract");
    let said = env.ok(&["rpc", "--emit-schema", path(&dir)]);
    for name in ["schema.json", "locallibrary.d.ts"] {
        assert!(said.contains(&format!("✓ wrote {}", path(&dir.join(name)))), "{said}");
        assert!(dir.join(name).is_file());
    }
}

/// Piped, nothing is coloured; `--color always` colours it anyway, clap's help included.
#[test]
fn colour_follows_the_flag_and_the_terminal() {
    let env = Env::new();
    env.demo("A");
    let escape = '\x1b';
    assert!(!env.ok(&["drive", "info", "A"]).contains(escape));
    assert!(env.ok(&["--color", "always", "drive", "info", "A"]).contains(escape));
    assert!(!env.ok(&["--color=never", "drive", "info", "A"]).contains(escape));
    assert!(env.ok(&["--color=always", "--help"]).contains(escape));
    assert!(!env.ok(&["--color", "never", "--help"]).contains(escape));
}

/// A locale that isn't UTF-8 gets ASCII stand-ins for the symbols, as `--ascii` does: the first
/// of LC_ALL, LC_CTYPE and LANG that's set says, and with none set it's taken to be UTF-8.
#[test]
fn a_locale_that_isnt_utf8_gets_ascii_symbols() {
    let env = Env::new();
    let gc = |vars: &[(&str, &str)]| {
        let out = env.command().envs(vars.iter().copied()).args(["store", "gc"]).output().unwrap();
        String::from_utf8(out.stdout).unwrap()
    };
    assert_eq!(gc(&[("LC_ALL", "C")]).trim(), "v nothing to collect");
    assert_eq!(gc(&[("LC_ALL", ""), ("LC_CTYPE", "en_GB.UTF-8"), ("LANG", "C")]).trim(), "✓ nothing to collect");
    assert_eq!(gc(&[]).trim(), "✓ nothing to collect");
}

/// Errors go to stderr with what would fix them; `--json` makes a usage error JSON too.
#[test]
fn errors_say_what_went_wrong_and_what_to_do() {
    let env = Env::new();
    let r = env.run(&["drive", "info", "/definitely/not/here"]);
    assert_eq!(r.code, 1);
    assert!(r.out.is_empty() && line(&r.err, "error:").contains("not/here"), "{}", r.err);

    let r = env.run(&["sync", "--from", "A"]);
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.err.contains("--from copies onto one drive"), "{}", r.err);
    let r = env.run(&["pull", "--label", "x"]);
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.err.contains("go with one drive: name it"), "{}", r.err);
    let r = env.run(&["sync", "A"]);
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.err.contains("give two or more drives, or none"), "{}", r.err);
    let r = env.run(&["settings", "import", "--replace"]);
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.err.contains("--replace goes with a file `settings export` wrote"), "{}", r.err);

    let r = env.run(&["--json", "sync", "--no-such-flag"]);
    assert_eq!(r.code, 2);
    let v: serde_json::Value = serde_json::from_str(&r.out).unwrap_or_else(|e| panic!("{e}: {}", r.out));
    assert_eq!((&v["error"]["kind"], &v["outcome"]), (&"usage".into(), &"error".into()), "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("--no-such-flag"), "{v}");

    // On its own, with no terminal: the usage.
    let r = env.run(&[]);
    assert_eq!(r.code, 2);
    assert!(r.out.is_empty() && r.err.contains("Usage: locallibrary"), "{}", r.err);
}
