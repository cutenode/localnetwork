//! What to type next: the CLI's wording for what resolves a library error. The library says what
//! went wrong and, as data, what would resolve it ([`locallibrary::error::Detail`]: a reason,
//! and actions that name RPC methods and parameters); this turns those into commands and flags.
//! And after saving drives that came back with new sessions, what makes sets of them.

use locallibrary::Store;
use locallibrary::api::{self, JobReport, StepKind};
use locallibrary::error::{Action, Detail};
use serde_json::{Map, Value};

/// A line for each action the error offers, `label: command` where there's a command for it.
pub fn hints(e: &locallibrary::Error) -> Vec<String> {
    let Detail { actions, .. } = e.detail();
    actions
        .iter()
        .map(|a| match command(a) {
            Some(cmd) => format!("{}: {cmd}", a.label),
            None => a.label.clone(),
        })
        .collect()
}

/// After saving drives: `n` new sessions, and how to find the sets among them (nothing when
/// there are none).
pub fn new_sessions(n: usize) {
    match n {
        0 => {}
        1 => crate::output::hint("1 new session: `locallibrary sets suggest` says if it looks like a set"),
        n => crate::output::hint(format!("{n} new sessions: `locallibrary sets suggest` says which look like sets")),
    }
}

/// How many history lists the snapshots `saved` (ids, with their parents') hold that their
/// parents don't, every one for a library's first: the sessions saving them added. One that
/// can't be read adds none: it's only for a hint.
pub fn saved_sessions(store: &Store, saved: impl IntoIterator<Item = (String, Option<String>)>) -> usize {
    saved
        .into_iter()
        .filter_map(|(id, parent)| {
            let req = api::PlayStatsRequest {
                source: format!("snapshot:{id}"),
                since: parent.map(|p| format!("snapshot:{p}")),
                limit: Some(0),
            };
            let stats = api::play_stats(Some(store), &req).ok()?;
            Some(stats.since.map_or(stats.history_lists, |s| s.new_histories.len()))
        })
        .sum()
}

/// [`saved_sessions`] of the snapshots a job's pull step saved.
pub fn job_sessions(store: &Store, r: &JobReport) -> usize {
    let saved = r.steps.iter().filter(|s| s.step == StepKind::Pull).flat_map(|s| &s.reports).filter_map(|p| {
        let s = &p["snapshot"];
        let id = s["id"].as_str().filter(|_| p["up_to_date"] != true)?;
        Some((id.to_string(), s["parents"][0].as_str().map(String::from)))
    });
    saved_sessions(store, saved)
}

/// The command (or flags) an action stands for.
fn command(a: &Action) -> Option<String> {
    if a.elevate {
        return Some(if cfg!(windows) {
            "run it again from an administrator terminal".into()
        } else {
            "run it again with sudo".into()
        });
    }
    let Some(method) = a.method.as_deref() else { return flags(&a.params).map(|f| format!("add {f}")) };
    let p = |key: &str| a.params.get(key).and_then(Value::as_str).map(quote);
    let yes = |key: &str| a.params.get(key) == Some(&Value::Bool(true));
    Some(match method {
        "pull" => format!("locallibrary pull {}", p("drive")?),
        "push.plan" => format!("locallibrary library push {}", p("drive")?),
        "sync.plan" => match a.params.get("direction").and_then(Value::as_str) {
            Some("left_to_right") => format!("locallibrary sync --from {} {}", p("left")?, p("right")?),
            Some("right_to_left") => format!("locallibrary sync --from {} {}", p("right")?, p("left")?),
            _ => format!("locallibrary sync {} {}", p("left")?, p("right")?),
        },
        "merge.plan" => format!("locallibrary merge {} {}", p("source")?, p("target")?),
        "undo.plan" => format!("locallibrary undo {}", p("drive")?),
        "undo.list" => "locallibrary undo --list".into(),
        "drive.backups" => format!("locallibrary drive backups {}", p("drive")?),
        "drive.role" => {
            format!("locallibrary drive role {} {}", p("drive").unwrap_or_else(|| "<DRIVE>".into()), p("role")?)
        }
        "drive.retire" if yes("undo") => format!("locallibrary drive retire {} --undo", p("drive")?),
        "drive.reidentify" => format!("locallibrary drive reidentify {}", p("drive")?),
        "snapshot.label" if yes("remove") => format!("locallibrary store unlabel {}", p("label")?),
        "convert.plan" => {
            let to = p("to")?.replace('_', "");
            let to = if to == "devicelibrary" { "device-library".to_string() } else { to };
            let replace = if yes("replace") { " --replace" } else { "" };
            format!("locallibrary convert {} --to {to}{replace}", p("drive")?)
        }
        "account.login.begin" if yes("publish") => "locallibrary account login --publish".into(),
        "account.login.begin" => "locallibrary account login".into(),
        "settings.schema" => "locallibrary settings schema".into(),
        "settings.import.plan" => "locallibrary settings import <DRIVE>".into(),
        "library.list" => "locallibrary library ls".into(),
        _ => return None,
    })
}

/// The flags for parameters added to the request that failed: `--force`, `--library <LIBRARY>`.
fn flags(params: &Map<String, Value>) -> Option<String> {
    let flags: Vec<String> = params
        .iter()
        .filter_map(|(key, value)| {
            let flag = match key.as_str() {
                "allow_low_confidence" => "--allow-guess".to_string(),
                "folder_name" => "--folder".to_string(),
                // A drive is named on its own, not after a flag.
                "drive" if value.is_null() => return Some("<DRIVE>".to_string()),
                _ => format!("--{}", key.replace('_', "-")),
            };
            match value {
                Value::Bool(true) => Some(flag),
                Value::Bool(false) => None,
                Value::Null => Some(format!("{flag} <{}>", key.to_uppercase())),
                Value::String(s) => Some(format!("{flag} {}", quote(s))),
                other => Some(format!("{flag} {other}")),
            }
        })
        .collect();
    (!flags.is_empty()).then(|| flags.join(" "))
}

/// A shell word: as it is when nothing in it means anything to a shell, else in double quotes.
/// Double quotes still expand `$` and backticks (and `!` in an interactive bash), so a word with
/// any of those, `\` or `"` goes in single quotes, each `'` in it as `'\''`.
fn quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-:@+".contains(c)) {
        s.to_string()
    } else if !s.contains(['$', '`', '!', '\\', '"']) {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn actions_become_commands_and_flags() {
        let e = locallibrary::Error::NotFound("library \"Club\" in the store".into())
            .action(Action::call("pull", json!({ "drive": "/Volumes/My DNA" }), "Pull DNA into the store"))
            .action(Action::retry(json!({ "force": true, "library": null }), "Force it"))
            .action(Action::call(
                "sync.plan",
                json!({ "left": "/Volumes/A", "right": "/Volumes/B", "direction": "right_to_left" }),
                "Copy B over A",
            ))
            .action(Action::call("something.new", json!({}), "Something new"));
        assert_eq!(
            hints(&e),
            [
                "Pull DNA into the store: locallibrary pull \"/Volumes/My DNA\"",
                "Force it: add --force --library <LIBRARY>",
                "Copy B over A: locallibrary sync --from /Volumes/B /Volumes/A",
                "Something new",
            ]
        );
        let admin = locallibrary::Error::Permission("formatting /dev/disk4 needs administrator rights".into());
        assert!(hints(&admin)[0].contains("run it again"), "{:?}", hints(&admin));
    }

    #[test]
    fn a_switch_is_a_flag_without_a_value() {
        let e = locallibrary::Error::Account("the system keychain isn't available".into())
            .action(Action::retry(json!({ "session_file": true }), "Keep the session in a file"));
        assert_eq!(hints(&e), ["Keep the session in a file: add --session-file"]);
    }

    /// Values become flags as typed: a word as it is, a number bare, an unset switch left out; and
    /// a method's own spelling of a value becomes the command line's.
    #[test]
    fn values_are_spelled_as_the_command_line_takes_them() {
        let e = locallibrary::Error::Conflict("the drives differ".into())
            .action(Action::retry(json!({ "folder_name": "From B", "limit": 3, "dry_run": false }), "Retry"))
            .action(Action::call("sync.plan", json!({ "left": "A", "right": "B" }), "Sync"))
            .action(Action::call("drive.role", json!({ "role": "backup" }), "Make it a backup"))
            .action(Action::call("convert.plan", json!({ "drive": "A", "to": "device_library" }), "Rebuild"))
            .action(Action::call("convert.plan", json!({ "drive": "A", "to": "one_library" }), "Convert"))
            .action(Action::call(
                "convert.plan",
                json!({ "drive": "A", "to": "one_library", "replace": true }),
                "Redo",
            ));
        assert_eq!(
            hints(&e),
            [
                "Retry: add --folder \"From B\" --limit 3",
                "Sync: locallibrary sync A B",
                "Make it a backup: locallibrary drive role <DRIVE> backup",
                "Rebuild: locallibrary convert A --to device-library",
                "Convert: locallibrary convert A --to onelibrary",
                "Redo: locallibrary convert A --to onelibrary --replace",
            ]
        );
    }

    /// `line`'s words as a shell reads them, expanding what it expands: what `printf` is handed.
    #[cfg(unix)]
    fn sh_words(line: &str) -> Vec<String> {
        let out = std::process::Command::new("sh").arg("-c").arg(format!("printf '%s\\n' {line}")).output().unwrap();
        assert!(out.status.success(), "{line}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().lines().map(String::from).collect()
    }

    /// `line`'s words: split as a shell splits them, and on Unix read the same by a real one, so
    /// nothing in them expands.
    fn shell_words(line: &str) -> Vec<String> {
        let words = shlex::split(line).unwrap_or_else(|| panic!("{line} doesn't split"));
        #[cfg(unix)]
        if !line.contains('<') {
            assert_eq!(sh_words(line), words, "a shell reads {line} otherwise");
        }
        words
    }

    /// Drives, libraries and labels as people name them: with spaces, quotes, backslashes and
    /// what a shell would expand.
    const VALUES: &[&str] = &[
        "A",
        "/Volumes/My DNA",
        "Club \"Night\"",
        "back\\slash",
        "rekordbox:usb",
        "Ünïcödé",
        "a$b",
        "`x`",
        "DJ's USB",
        "Wow!",
    ];

    /// Every hint that names a command is one the command line reads, with each value given
    /// back as it was (quoted where a shell needs it); one that adds to the request names a flag
    /// for each parameter it adds, and leaves out a switch that's off.
    #[hegel::test(test_cases = 200)]
    fn hints_are_command_lines_that_parse(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let methods = [
            "pull",
            "push.plan",
            "sync.plan",
            "merge.plan",
            "undo.plan",
            "undo.list",
            "drive.backups",
            "drive.role",
            "drive.retire",
            "drive.reidentify",
            "snapshot.label",
            "convert.plan",
            "account.login.begin",
            "settings.schema",
            "settings.import.plan",
            "library.list",
        ];
        let value = |tc: &hegel::TestCase| VALUES[tc.draw(gs::integers::<usize>().max_value(VALUES.len() - 1))];
        let mut params = Map::new();
        for key in ["drive", "left", "right", "source", "target", "label"] {
            if tc.draw(gs::weighted_booleans(0.8)) {
                params.insert(key.into(), value(&tc).into());
            }
        }
        params.insert("role".into(), tc.draw(gs::sampled_from(&["backup", "primary", "archive"][..])).into());
        params.insert("to".into(), tc.draw(gs::sampled_from(&["one_library", "device_library"][..])).into());
        let direction = tc.draw(gs::sampled_from(&["left_to_right", "right_to_left", "either"][..]));
        params.insert("direction".into(), direction.into());
        for key in ["replace", "undo", "remove", "publish"] {
            params.insert(key.into(), tc.draw(gs::booleans()).into());
        }
        let method = tc.draw(gs::sampled_from(&methods[..]));
        let e = locallibrary::Error::Conflict("x".into()).action(Action::call(
            method,
            Value::Object(params.clone()),
            "Do it",
        ));
        let hint = &hints(&e)[0];
        let Some(cmd) = hint.strip_prefix("Do it: ") else {
            assert_eq!(hint, "Do it", "no command, no colon");
            return;
        };
        let words = shell_words(cmd);
        assert_eq!(words[0], "locallibrary", "{cmd}");
        if !cmd.contains('<') {
            // Built once: it's the slow part.
            static COMMAND: std::sync::OnceLock<clap::Command> = std::sync::OnceLock::new();
            let parsed = COMMAND.get_or_init(crate::help::command).clone().try_get_matches_from(&words);
            assert!(parsed.is_ok(), "{cmd}: {}", parsed.err().map(|e| e.to_string()).unwrap_or_default());
        }
        // The values each command is made of.
        let used: &[&str] = match method {
            "sync.plan" => &["left", "right"],
            "merge.plan" => &["source", "target"],
            "snapshot.label" => &["label"],
            "undo.list" | "account.login.begin" | "settings.schema" | "settings.import.plan" | "library.list" => &[],
            _ => &["drive"],
        };
        for key in used {
            if let Some(v) = params.get(*key).and_then(Value::as_str) {
                assert!(words.iter().any(|w| w == v), "{v:?} doesn't come back from {cmd}: {words:?}");
            }
        }

        // The request that failed, again with more parameters.
        let mut extra = Map::new();
        for key in ["force", "dry_run", "library", "folder_name", "allow_low_confidence", "limit", "session_file"] {
            if tc.draw(gs::booleans()) {
                let v = match tc.draw(gs::integers::<u8>().max_value(3)) {
                    0 => Value::Bool(true),
                    1 => Value::Bool(false),
                    2 => Value::Null,
                    _ => value(&tc).into(),
                };
                extra.insert(key.into(), v);
            }
        }
        let e = locallibrary::Error::Conflict("x".into()).action(Action::retry(Value::Object(extra.clone()), "Again"));
        let hint = &hints(&e)[0];
        let on: Vec<(&String, &Value)> = extra.iter().filter(|(_, v)| **v != Value::Bool(false)).collect();
        if on.is_empty() {
            assert_eq!(hint, "Again");
            return;
        }
        let flags = hint.strip_prefix("Again: add ").unwrap_or_else(|| panic!("{hint}"));
        let words = shell_words(flags);
        for (key, v) in on {
            let flag = match key.as_str() {
                "allow_low_confidence" => "--allow-guess".to_string(),
                "folder_name" => "--folder".to_string(),
                k => format!("--{}", k.replace('_', "-")),
            };
            let at = words.iter().position(|w| *w == flag).unwrap_or_else(|| panic!("no {flag} in {hint}"));
            match v {
                Value::Bool(true) => {}
                Value::Null => assert_eq!(words[at + 1], format!("<{}>", key.to_uppercase()), "{hint}"),
                Value::String(s) => assert_eq!(&words[at + 1], s, "{hint}"),
                _ => unreachable!(),
            }
        }
        for (key, _) in extra.iter().filter(|(_, v)| **v == Value::Bool(false)) {
            assert!(!flags.contains(&key.replace('_', "-")), "{key} is off, yet in {hint}");
        }
    }
}
