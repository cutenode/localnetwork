//! The command line as parsed: the top-level help lists the commands under headings.

use clap::{Command, CommandFactory};

/// The top-level commands under each heading, in order. A command that isn't listed goes under
/// "Other"; one listed that doesn't exist is left out.
const GROUPS: &[(&str, &[&str])] = &[
    ("Everyday", &["setup", "status", "refresh", "prep", "sync", "pull", "check", "eject", "undo", "tui"]),
    ("Drives", &["drive", "provision", "doctor", "watch"]),
    ("Library and store", &["tracks", "library", "store", "diff", "collection", "export", "xml", "analyze"]),
    (
        "Editing",
        &[
            "playlist", "track", "tags", "cues", "art", "edits", "smart", "merge", "pack", "convert", "clean",
            "settings",
        ],
    ),
    ("Plays and players", &["plays", "sets", "mix", "profile", "players"]),
    ("Other", &["account", "plugin", "completions", "help"]),
];

/// The old top-level names of commands that moved into a group, and where they went: a "did you
/// mean" names the new spelling, and the hidden names aren't suggested.
const MOVED: &[(&str, &str)] = &[
    ("drives", "drive ls"),
    ("info", "drive info"),
    ("adopt", "drive adopt"),
    ("disks", "drive disks"),
    ("format", "drive format"),
    ("verify", "drive verify"),
    ("fsck", "drive fsck"),
    ("space", "drive space"),
    ("tidy", "drive tidy"),
    ("recordings", "drive recordings"),
    ("firmware", "drive firmware"),
    ("backups", "drive backups"),
    ("restore", "drive restore"),
    ("log", "library log"),
    ("push", "library push"),
    ("history", "plays lists"),
    ("stats", "plays stats"),
    ("playlists", "playlist ls"),
    ("mirror", "sync --from"),
    ("compare", "diff --summary"),
];

/// A usage error's "did you mean": hidden names give way to the commands that replaced them.
pub fn tidy_suggestions(e: clap::Error, cmd: &Command) -> clap::Error {
    tidy(e, cmd, &command_words())
}

/// [`tidy_suggestions`] for the command words typed. The moved names are top-level ones, so a
/// slip further in keeps its suggestion: `collection playlists` is a command of its own.
fn tidy(e: clap::Error, cmd: &Command, words: &[String]) -> clap::Error {
    use clap::error::{ContextKind, ContextValue};
    let Some(ContextValue::Strings(names)) = e.get(ContextKind::SuggestedSubcommand) else { return e };
    let top_level =
        matches!(e.get(ContextKind::InvalidSubcommand), Some(ContextValue::String(w)) if words.first() == Some(w));
    let mut visible = Vec::new();
    visible_names(cmd, &mut visible);
    let mut kept: Vec<String> = Vec::new();
    for name in names {
        let name = match MOVED.iter().find(|(old, _)| top_level && old == name) {
            Some((_, new)) => new.to_string(),
            None if visible.contains(name) => name.clone(),
            None => continue,
        };
        if !kept.contains(&name) {
            kept.push(name);
        }
    }
    let mut fresh = clap::Error::new(e.kind()).with_cmd(cmd);
    for (kind, value) in e.context() {
        if kind != ContextKind::SuggestedSubcommand {
            fresh.insert(kind, value.clone());
        }
    }
    if !kept.is_empty() {
        fresh.insert(ContextKind::SuggestedSubcommand, ContextValue::Strings(kept));
    }
    fresh
}

/// The night-before job's flags, from when it was `set`.
const PREP_FLAGS: &[&str] = &["--no-eject", "--players", "--audio", "--library", "-l", "--verbose", "-v", "--dry-run"];

/// `set` was the night-before job, and is a spelling of `sets` now: when what follows `sets` is
/// one of the job's flags, or what follows `set` a drive, rather than a subcommand of `sets`, say
/// where the job went. (`sets` followed by a drive is more likely a set named like one.)
pub fn prep_moved(e: &clap::Error) -> Option<String> {
    use clap::error::{ContextKind, ContextValue, ErrorKind};
    let words = command_words();
    let sets = crate::Cli::command().find_subcommand("sets").cloned()?;
    let first = words.first().map(String::as_str);
    if !matches!(first, Some("set" | "sets")) || words.get(1).is_some_and(|w| sets.find_subcommand(w).is_some()) {
        return None;
    }
    let given = |kind| match e.get(kind) {
        Some(ContextValue::String(w)) => Some(w.as_str()),
        _ => None,
    };
    let job = match e.kind() {
        ErrorKind::InvalidSubcommand => given(ContextKind::InvalidSubcommand).is_some_and(|w| {
            first == Some("set")
                && !matches!(locallibrary::api::resolve_drive(w), Err(locallibrary::Error::NotFound(_)))
        }),
        ErrorKind::UnknownArgument => {
            given(ContextKind::InvalidArg).is_some_and(|a| PREP_FLAGS.contains(&a.split('=').next().unwrap_or(a)))
        }
        _ => false,
    };
    job.then(|| {
        "the night-before job (save, sync, check, eject) is `locallibrary prep` now: `locallibrary sets` is the \
         sets you played"
            .to_string()
    })
}

/// The words of the command line that aren't options (nor the values of the global ones).
fn command_words() -> Vec<String> {
    let mut words = Vec::new();
    let mut args = std::env::args().skip(1).take_while(|a| a != "--");
    while let Some(a) = args.next() {
        match a.as_str() {
            "--store" | "--color" => {
                args.next();
            }
            a if a.starts_with('-') => {}
            _ => words.push(a),
        }
    }
    words
}

/// A group run with a drive (`drive Main` is `drive info Main`) takes a mistyped subcommand for
/// a drive: when `word` names no drive but looks like one of `group`'s subcommands, say so.
pub fn check_drive_word(group: &str, word: &str) -> anyhow::Result<()> {
    match locallibrary::api::resolve_drive(word) {
        Err(locallibrary::Error::NotFound(_)) => mistyped_subcommand(group, word, "drive"),
        _ => Ok(()),
    }
}

/// `group WORD`, where WORD names no `what` (a drive, a library): when it looks like one of
/// `group`'s subcommands, a usage error saying which.
pub fn mistyped_subcommand(group: &str, word: &str, what: &str) -> anyhow::Result<()> {
    match similar_subcommand(group, word) {
        Some(sc) => Err(crate::jobs_cmd::usage(format!(
            "no {what} or command {word:?}; did you mean `locallibrary {group} {sc}`? (`locallibrary {group} \
             --help` lists them)"
        ))),
        None => Ok(()),
    }
}

/// The subcommand of `group` that `word` is most likely a typo of.
pub fn similar_subcommand(group: &str, word: &str) -> Option<String> {
    let cmd = crate::Cli::command();
    let group = cmd.find_subcommand(group)?;
    let names = group.get_subcommands().filter(|s| !s.is_hide_set()).map(|s| s.get_name());
    locallibrary::query::nearest(word, names).map(str::to_string)
}

/// The names and visible aliases of every command that isn't hidden, at every level.
fn visible_names(cmd: &Command, out: &mut Vec<String>) {
    for sc in cmd.get_subcommands().filter(|s| !s.is_hide_set()) {
        out.push(sc.get_name().to_string());
        out.extend(sc.get_visible_aliases().map(str::to_string));
        visible_names(sc, out);
    }
}

/// The command line `locallibrary` parses: [`crate::Cli`] and what plugins add, with the
/// top-level help grouped.
pub fn command() -> Command {
    // The plugins first: `export --format` lists their formats once they're checked. Refused,
    // they add nothing (what uses them says why).
    let plugins = crate::plugins::host().ok();
    let mut cmd = crate::Cli::command();
    if let Some(plugins) = plugins {
        cmd = crate::plugin_cmd::add_to(cmd, plugins);
    }
    // `analyze` is offered only when a plugin has an analyzer.
    let analyzers = plugins.is_some_and(|p| !p.analyzers().is_empty());
    cmd = cmd.mut_subcommand("analyze", |c| c.hide(!analyzers));
    let json = cmd.get_arguments().find(|a| a.get_id() == "json").cloned().expect("--json is global");
    mark_writes(&mut cmd, &json);
    // Built first, so the help subcommand clap adds is there to be listed.
    cmd.build();
    let groups = grouped(&cmd);
    if let Some(choice) = color_asked() {
        cmd = cmd.color(choice);
    }
    let header = cmd.get_styles().get_header();
    let template = format!(
        "{{about-with-newline}}\n{{usage-heading}} {{usage}}\n\n{groups}{header}Options:{header:#}\n{{options}}{{after-help}}"
    );
    cmd.help_template(template)
}

/// `--color WHEN` as given on the command line, for clap's own help and errors (which are
/// written before the command line is parsed into [`crate::Cli`]).
fn color_asked() -> Option<clap::ColorChoice> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = args.iter().enumerate().find_map(|(i, a)| match a.strip_prefix("--color") {
        Some(v) if v.starts_with('=') => Some(v[1..].to_string()),
        Some("") => args.get(i + 1).cloned(),
        _ => None,
    })?;
    match value.as_str() {
        "always" => Some(clap::ColorChoice::Always),
        "never" => Some(clap::ColorChoice::Never),
        "auto" => Some(clap::ColorChoice::Auto),
        _ => None,
    }
}

/// Commands that ask before they write and have no `--dry-run` to tell them by. (`pull` and
/// `eject` don't ask: a snapshot is only added, and ejecting changes nothing on the drive.
/// Neither does `account login`: signing in is done in the browser.)
const WRITES: &[&str] = &[
    "role", "forget", "retire", "set", "unset", "add", "rm", "rename", "label", "unlabel", "reset", "setup", "delete",
];

/// `--json` on a command that writes says it needs `--yes`; on one that only reads, it doesn't.
/// Before the command is built: a command's own copy of the global flag isn't replaced by the
/// one propagated to it.
fn mark_writes(cmd: &mut Command, json: &clap::Arg) {
    for sc in cmd.get_subcommands_mut() {
        let writes = sc.get_arguments().any(|a| matches!(a.get_id().as_str(), "dry_run" | "erase" | "repair"))
            || (WRITES.contains(&sc.get_name()) && !sc.has_subcommands());
        if writes {
            let help = "Machine-readable JSON output (never prompts: add --yes to apply changes)";
            *sc = std::mem::take(sc).arg(json.clone().help(help));
        }
        mark_writes(sc, json);
    }
}

/// The visible top-level commands, a heading for each group, aligned and wrapped.
fn grouped(cmd: &Command) -> String {
    let visible: Vec<&Command> = cmd.get_subcommands().filter(|s| !s.is_hide_set()).collect();
    let width = visible.iter().map(|s| s.get_name().len()).max().unwrap_or(0);
    let columns = terminal_width();
    let styles = cmd.get_styles();
    let (header, literal) = (styles.get_header(), styles.get_literal());
    let mut placed: Vec<&str> = Vec::new();
    let mut sections: Vec<(&str, Vec<&Command>)> = GROUPS
        .iter()
        .map(|(heading, names)| {
            let commands: Vec<&Command> =
                names.iter().filter_map(|n| visible.iter().copied().find(|s| s.get_name() == *n)).collect();
            placed.extend(names.iter().copied());
            (*heading, commands)
        })
        .collect();
    let others = visible.iter().copied().filter(|s| !placed.contains(&s.get_name()));
    sections.last_mut().expect("Other is last").1.extend(others);
    let mut out = String::new();
    for (heading, commands) in sections.into_iter().filter(|(_, c)| !c.is_empty()) {
        out.push_str(&format!("{header}{heading}:{header:#}\n"));
        for c in commands {
            let about = c.get_about().map(|a| a.to_string()).unwrap_or_default();
            let name = c.get_name();
            let indent = 2 + width + 2;
            let lines = wrap(&about, columns.saturating_sub(indent).max(30));
            for (i, line) in lines.iter().enumerate() {
                if i == 0 {
                    let pad = " ".repeat(width - name.len());
                    out.push_str(&format!("  {literal}{name}{literal:#}{pad}  {line}\n"));
                } else {
                    out.push_str(&format!("{}{line}\n", " ".repeat(indent)));
                }
            }
            if lines.is_empty() {
                out.push_str(&format!("  {literal}{name}{literal:#}\n"));
            }
        }
        out.push('\n');
    }
    out
}

/// Columns to fit the help into: the terminal's, at most 100.
fn terminal_width() -> usize {
    console::Term::stdout().size_checked().map(|(_, w)| w as usize).unwrap_or(100).min(100)
}

/// `text` broken into lines of at most `width` characters, between words.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

#[cfg(test)]
mod tests;
