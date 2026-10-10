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
            format!("locallibrary convert {} --to {to}", p("drive")?)
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

/// A shell word: quoted when it holds spaces or quotes.
fn quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-:@+".contains(c)) {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
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
            .action(Action::call("convert.plan", json!({ "drive": "A", "to": "one_library" }), "Convert"));
        assert_eq!(
            hints(&e),
            [
                "Retry: add --folder \"From B\" --limit 3",
                "Sync: locallibrary sync A B",
                "Make it a backup: locallibrary drive role <DRIVE> backup",
                "Rebuild: locallibrary convert A --to device-library",
                "Convert: locallibrary convert A --to onelibrary",
            ]
        );
    }
}
