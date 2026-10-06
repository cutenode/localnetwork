//! What to type next: the CLI's wording for what resolves a library error. The library says what
//! went wrong and, as data, what would resolve it ([`locallibrary::error::Detail`]: a reason,
//! and actions that name RPC methods and parameters); this turns those into commands and flags.

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
}
