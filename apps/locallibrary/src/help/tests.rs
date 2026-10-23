use super::*;

#[test]
fn every_visible_command_has_a_heading_and_old_names_are_hidden() {
    let cmd = command();
    let help = cmd.clone().render_help().to_string();
    for heading in ["Everyday:", "Drives:", "Library and store:", "Editing:", "Plays and players:", "Other:"] {
        assert!(help.contains(heading), "{heading} missing:\n{help}");
    }
    for c in cmd.get_subcommands().filter(|s| !s.is_hide_set()) {
        assert!(help.contains(&format!("  {}", c.get_name())), "{} isn't listed:\n{help}", c.get_name());
    }
    for old in [
        "drives",
        "info",
        "adopt",
        "log",
        "backups",
        "restore",
        "history",
        "stats",
        "disks",
        "fsck",
        "verify",
        "space",
        "tidy",
        "recordings",
        "firmware",
        "playlists",
        "mirror",
        "compare",
        "push",
        "format",
        "rpc",
    ] {
        let sc = cmd.find_subcommand(old).unwrap_or_else(|| panic!("{old} is gone"));
        assert!(sc.is_hide_set(), "{old} should be hidden");
        assert!(!help.contains(&format!("\n  {old} ")), "{old} is listed:\n{help}");
    }
    assert!(help.contains("Common tasks:"));
    assert!(help.contains("Options:") && help.contains("--json"));
}

#[test]
fn a_mistyped_subcommand_is_recognised() {
    assert_eq!(similar_subcommand("drive", "lst").as_deref(), Some("ls"));
    assert_eq!(similar_subcommand("playlist", "mov").as_deref(), Some("move"));
    assert_eq!(similar_subcommand("plays", "stat").as_deref(), Some("stats"));
    assert_eq!(similar_subcommand("drive", "Main"), None);
    assert_eq!(similar_subcommand("drive", "Club drive"), None);
    assert_eq!(similar_subcommand("library", "lgo").as_deref(), Some("log"));
}

/// `--json` says to add `--yes` on a command that asks before it writes (`profile reset`),
/// and not on one that doesn't ask (`account login`).
#[test]
fn json_help_says_yes_where_a_command_asks() {
    let cmd = command();
    let json_help = |path: &[&str]| {
        let sc = path.iter().fold(&cmd, |c, name| c.find_subcommand(name).unwrap_or_else(|| panic!("{name}")));
        let json = sc.get_arguments().find(|a| a.get_id() == "json").expect("--json");
        json.get_help().map(|h| h.to_string()).unwrap_or_default()
    };
    assert!(json_help(&["profile", "reset"]).contains("add --yes"));
    #[cfg(feature = "account")]
    assert!(!json_help(&["account", "login"]).contains("--yes"));
}

#[test]
fn wrapping_keeps_words_whole() {
    assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
    assert!(wrap("", 10).is_empty());
}

/// Every word, in order, on lines no wider than `width` (a word longer than that on a line of
/// its own), and no line that the next one's first word would have fitted on.
#[hegel::test(test_cases = 300)]
fn wrapped_lines_fit_and_keep_every_word(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let text: String = tc.draw(gs::text().alphabet("ab cdé日 \n\t").max_size(80));
    let width = tc.draw(gs::integers::<usize>().min_value(1).max_value(20));
    let lines = wrap(&text, width);
    let words: Vec<&str> = text.split_whitespace().collect();
    let rewrapped: Vec<&str> = lines.iter().flat_map(|l| l.split(' ')).collect();
    assert_eq!(rewrapped, words, "{lines:?}");
    for (i, line) in lines.iter().enumerate() {
        let one_word = !line.contains(' ');
        assert!(line.chars().count() <= width || one_word, "{line:?} is wider than {width}");
        if let Some(next) = lines.get(i + 1) {
            let first = next.split(' ').next().unwrap();
            assert!(line.chars().count() + 1 + first.chars().count() > width, "{first:?} fits on {line:?}");
        }
    }
}

/// The visible subcommands of every visible group that has some.
fn groups() -> Vec<(String, Vec<String>)> {
    crate::Cli::command()
        .get_subcommands()
        .filter(|g| !g.is_hide_set())
        .map(|g| {
            let names = g.get_subcommands().filter(|s| !s.is_hide_set()).map(|s| s.get_name().to_string());
            (g.get_name().to_string(), names.collect::<Vec<_>>())
        })
        .filter(|(_, names)| !names.is_empty())
        .collect()
}

/// `word` with one character dropped, doubled, swapped with the next or replaced.
#[hegel::test_helper]
fn typo(tc: &hegel::TestCase, word: &str) -> String {
    use hegel::generators as gs;
    let mut chars: Vec<char> = word.chars().collect();
    let i = tc.draw(gs::integers::<usize>().max_value(chars.len() - 1));
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => drop(chars.remove(i)),
        1 => chars.insert(i, chars[i]),
        2 if i + 1 < chars.len() => chars.swap(i, i + 1),
        _ => chars[i] = tc.draw(gs::sampled_from(&['x', 'q', 'z'][..])),
    }
    chars.into_iter().collect()
}

/// A "did you mean" for a group's subcommand only ever names a visible subcommand of that
/// group; a name typed right is itself, and one typed with a single slip still finds one.
#[hegel::test(test_cases = 200)]
fn suggestions_for_a_subcommand_are_real_subcommands(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (group, names) = &tc.draw(gs::sampled_from(groups()));
    let name = &tc.draw(gs::sampled_from(&names[..]));
    assert_eq!(similar_subcommand(group, name).as_ref(), Some(name), "{group} {name}");
    let slipped = tc.draw(gs::booleans());
    let word = match slipped {
        true => typo(&tc, name),
        false => tc.draw(gs::text().alphabet("abcdefghijklmnopqrstuvwxyz -").max_size(12)),
    };
    let suggested = similar_subcommand(group, &word);
    if let Some(s) = &suggested {
        assert!(names.contains(s), "{group} {word:?}: {s:?} isn't one of {names:?}");
    }
    // One edit away is within the distance `nearest` allows a name of four letters or more.
    if slipped && name.chars().count() >= 4 {
        assert!(suggested.is_some(), "{group} {word:?} (a slip of {name}): no suggestion");
    }
    match mistyped_subcommand(group, &word, "drive") {
        Ok(()) => assert!(suggested.is_none(), "{group} {word:?}"),
        Err(e) => {
            let said = e.to_string();
            let s = suggested.expect("an error names a subcommand");
            assert!(said.contains(&format!("`locallibrary {group} {s}`")), "{said}");
            assert!(crate::jobs_cmd::is_usage(&e), "{said}");
        }
    }
}

/// A mistyped top-level command is a usage error whose suggestions, once tidied, are only
/// visible commands or the new spelling of a moved one, each once.
#[hegel::test(test_cases = 100)]
fn suggestions_for_a_command_are_visible_or_moved(tc: hegel::TestCase) {
    use clap::error::{ContextKind, ContextValue};
    use hegel::generators as gs;
    // Built once: it's the slow part.
    static COMMAND: std::sync::OnceLock<Command> = std::sync::OnceLock::new();
    let cmd = COMMAND.get_or_init(command);
    let mut names: Vec<String> = cmd.get_subcommands().map(|s| s.get_name().to_string()).collect();
    names.extend(MOVED.iter().map(|(old, _)| old.to_string()));
    let name = &tc.draw(gs::sampled_from(&names[..]));
    let word = typo(&tc, name);
    // A slip that spells a command (swapping two letters that are the same) isn't one.
    if cmd.find_subcommand(&word).is_some() {
        return;
    }
    let e = cmd.clone().try_get_matches_from(["locallibrary", word.as_str()]).expect_err(&word);
    let tidied = tidy(e, cmd, std::slice::from_ref(&word));
    assert_eq!(tidied.kind(), clap::error::ErrorKind::InvalidSubcommand, "{word:?}");
    let mut visible = Vec::new();
    visible_names(cmd, &mut visible);
    let moved: Vec<&str> = MOVED.iter().map(|(_, new)| *new).collect();
    if let Some(ContextValue::Strings(kept)) = tidied.get(ContextKind::SuggestedSubcommand) {
        assert!(!kept.is_empty());
        for s in kept {
            assert!(visible.contains(s) || moved.contains(&s.as_str()), "{word:?}: {s:?} is hidden");
            assert_eq!(kept.iter().filter(|k| *k == s).count(), 1, "{word:?}: {kept:?}");
        }
    }
    assert_eq!(tidied.get(ContextKind::InvalidSubcommand), Some(&ContextValue::String(word.clone())));
}

/// A slip in a group's subcommand is suggested as that subcommand, even where a moved
/// top-level command had the same name.
#[test]
fn a_slip_in_a_group_keeps_the_groups_suggestion() {
    use clap::error::{ContextKind, ContextValue};
    let cmd = command();
    let suggested = |words: &[&str]| {
        let e =
            cmd.clone().try_get_matches_from(std::iter::once("locallibrary").chain(words.iter().copied())).unwrap_err();
        let words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
        match tidy(e, &cmd, &words).get(ContextKind::SuggestedSubcommand) {
            Some(ContextValue::Strings(s)) => s.clone(),
            _ => Vec::new(),
        }
    };
    assert_eq!(suggested(&["collection", "qlaylists"]), ["playlists"]);
    // At the top, the moved command is named by its new spelling.
    let top = suggested(&["qlaylists"]);
    assert!(top.contains(&"playlist ls".to_string()) && !top.contains(&"playlists".to_string()), "{top:?}");
}
