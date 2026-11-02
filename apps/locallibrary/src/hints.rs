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
        "accounts.sign_in.begin" => {
            // Again as an account, named TYPE:SUBJECT.
            let target = match a.params.get("account").and_then(Value::as_str) {
                Some(subject) => quote(&format!("{}:{subject}", a.params.get("type")?.as_str()?)),
                None => p("type")?,
            };
            let flags = [("with", p("with")), ("for", p("for"))]
                .into_iter()
                .filter_map(|(f, v)| Some(format!(" --{f} {}", v?)));
            let scopes = a.params.get("scopes").and_then(Value::as_array).into_iter().flatten();
            let scopes = scopes.filter_map(Value::as_str).map(|s| format!(" --scope {}", quote(s)));
            format!("locallibrary account {target} sign-in{}", flags.chain(scopes).collect::<String>())
        }
        "accounts.access.set_default" => {
            let subject = a.params.get("account").and_then(|x| x.get("subject")).and_then(Value::as_str).map(quote);
            format!("locallibrary plugin access {} --default {}", p("plugin")?, subject?)
        }
        "accounts.access.give" => {
            let subject = a.params.get("account").and_then(|x| x.get("subject")).and_then(Value::as_str).map(quote);
            let scopes = a.params.get("scopes").and_then(Value::as_array).into_iter().flatten();
            let scopes: String = scopes.filter_map(Value::as_str).map(|s| format!(" --scope {}", quote(s))).collect();
            format!("locallibrary plugin access {} --give {}{scopes}", p("plugin")?, subject?)
        }
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
mod tests;
