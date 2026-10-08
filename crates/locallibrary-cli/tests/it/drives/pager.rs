//! A long listing in a terminal goes through the pager, as git's do. The command gets a terminal
//! of its own (a pseudo-terminal), and a pager that writes what it's given to a file.
#![cfg(unix)]

use std::io::Read;
use std::path::Path;
use std::process::Stdio;

use crate::support::{Env, terminal};

/// Runs `locallibrary players models` (a listing it pages) with stdout a terminal and the
/// environment `env_vars`; returns what reached the terminal.
fn models_in_a_terminal(env: &Env, args: &[&str], env_vars: &[(&str, &str)]) -> String {
    let (mut screen_end, stdout) = terminal();
    let mut command = env.command();
    command.env_remove("LESS").envs(env_vars.iter().copied()).args(args).args(["players", "models"]);
    let mut child = command.stdout(Stdio::from(stdout)).spawn().unwrap();
    // The command (and its pager) hold the terminal now: reading ends when they're done.
    drop(command);
    let mut screen = Vec::new();
    // Linux ends a terminal's output with an error rather than an end of file.
    let _ = screen_end.read_to_end(&mut screen);
    assert!(child.wait().unwrap().success());
    String::from_utf8_lossy(&screen).into_owned()
}

/// A pager that writes the `LESS` it was run with to `less` and what it was given to `paged`.
fn pager(less: &Path, paged: &Path) -> String {
    format!("printf %s \"$LESS\" > '{}'; cat > '{}'", less.display(), paged.display())
}

#[test]
fn a_listing_in_a_terminal_goes_through_the_pager() {
    let env = Env::new();
    let (less, paged) = (env.outside("less"), env.outside("paged"));
    let screen = models_in_a_terminal(&env, &[], &[("PAGER", &pager(&less, &paged))]);
    let listing = std::fs::read_to_string(&paged).unwrap();
    assert!(listing.starts_with("Model ") && listing.contains("CDJ-3000X"), "{listing}");
    assert!(!screen.contains("CDJ-3000X"), "{screen}");
    // less shows colour, quits when the listing fits, and leaves it on the screen.
    assert_eq!(std::fs::read_to_string(&less).unwrap(), "FRX");
    models_in_a_terminal(&env, &[], &[("PAGER", &pager(&less, &paged)), ("LESS", "S")]);
    assert_eq!(std::fs::read_to_string(&less).unwrap(), "S", "LESS as the user set it");
}

#[test]
fn the_pager_can_be_turned_off() {
    let env = Env::new();
    let paged = env.outside("paged");
    let to_a_file = pager(&env.outside("less"), &paged);
    for (args, pager_var) in [(&["--no-pager"][..], to_a_file.as_str()), (&[], "cat"), (&[], " ")] {
        let screen = models_in_a_terminal(&env, args, &[("PAGER", pager_var)]);
        assert!(screen.contains("CDJ-3000X"), "{args:?} {pager_var:?}: {screen}");
    }
    assert!(!paged.exists());
}
