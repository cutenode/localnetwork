use std::path::Path;

use serde_json::json;

use super::*;

/// A command's params schema with every kind of property the command line takes, and one it
/// doesn't (an object of objects, given in `--params`).
fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "The playlist" },
            "min_bpm": { "type": "number" },
            "count": { "type": "integer" },
            "append": { "type": "boolean" },
            "mode": { "type": "string", "enum": ["fast", "slow"] },
            "level": { "type": "integer", "enum": [1, 2] },
            "tags": { "type": "array", "items": { "type": "string" } },
            "ids": { "type": "array", "items": { "type": "integer" } },
            "file": { "type": "string", "x-path": true },
            "playlist": { "type": "string", "x-cli": { "flag": "from" } },
            "choices": { "type": "object", "additionalProperties": { "type": "integer" }, "x-cli": { "flag": "choose", "one_based": true } },
            "names": { "type": "object", "additionalProperties": { "type": "string" } },
            "deep": { "type": "object", "additionalProperties": { "type": "object" } },
        },
        "required": ["name"],
    })
}

/// The params `args` give the command whose params are [`schema`] (an operation's when
/// `operation`).
fn given(args: &[&str], operation: bool) -> Result<Value> {
    let schema = schema();
    let cmd = contributed("go", "Go", Some(&schema), operation);
    let m = cmd.try_get_matches_from(std::iter::once("go").chain(args.iter().copied()))?;
    params_of(Some(&schema), &m)
}

#[test]
fn flags_give_params_of_their_types() {
    let p = given(
        &[
            "USB",
            "--name",
            "Fresh",
            "--min-bpm",
            "120.5",
            "--count",
            "3",
            "--append",
            "--mode",
            "slow",
            "--level",
            "2",
            "--tags",
            "a",
            "--tags",
            "b",
            "--ids",
            "4",
            "--from",
            "Sets",
            "--choose",
            "1=42",
            "--choose",
            " 3 = #7 ",
            "--names",
            "x=y",
        ],
        false,
    )
    .unwrap();
    let expected = json!({
        "name": "Fresh", "min_bpm": 120.5, "count": 3, "append": true, "mode": "slow", "level": 2, "tags": ["a", "b"],
        "ids": [4], "playlist": "Sets", "choices": { "0": 42, "2": 7 }, "names": { "x": "y" },
    });
    assert_eq!(p, expected);
    // A required param is asked for by its flag, unless `--params` gives the params.
    let missing = given(&[], false).unwrap_err().to_string();
    assert!(missing.contains("the following required arguments were not provided:\n  --name <NAME>"), "{missing}");
    assert_eq!(given(&["--params", "{}"], false).unwrap(), json!({}));
}

#[test]
fn a_file_is_given_absolute() {
    let p = given(&["--name", "x", "--file", "list.txt"], false).unwrap();
    let file = std::path::PathBuf::from(p["file"].as_str().unwrap());
    assert!(file.is_absolute() && file.ends_with("list.txt"), "{p}");
}

#[test]
fn params_as_json_give_way_to_flags() {
    let p = given(&["--params", r#"{"name": "Old", "deep": {"a": {}}}"#, "--name", "New"], false).unwrap();
    assert_eq!(p, json!({ "name": "New", "deep": { "a": {} } }));
    let e = given(&["--params", "[1]"], false).unwrap_err();
    assert_eq!(e.to_string(), "--params takes a JSON object, not [1]");
}

#[test]
fn values_not_of_their_type_are_refused() {
    let wrong = |args: &[&str]| given(&[&["--name", "x"], args].concat(), false).unwrap_err().to_string();
    assert_eq!(wrong(&["--count", "3.5"]), "--count takes a whole number, not \"3.5\"");
    assert_eq!(wrong(&["--min-bpm", "fast"]), "--min-bpm takes a number, not \"fast\"");
    assert_eq!(wrong(&["--choose", "1"]), "--choose takes KEY=VALUE, not \"1\"");
    assert_eq!(wrong(&["--choose", "0=4"]), "--choose's keys count from 1: \"0\" isn't one");
    assert_eq!(wrong(&["--choose", "1=x"]), "--choose takes a whole number, not \"x\"");
    assert!(wrong(&["--mode", "medium"]).contains("invalid value 'medium'"));
    assert!(wrong(&["--level", "3"]).contains("invalid value '3'"));
    assert!(wrong(&["--deep", "a=b"]).contains("unexpected argument '--deep'"));
}

#[test]
fn only_an_operation_has_dry_run() {
    let schema = schema();
    let op =
        contributed("go", "Go", Some(&schema), true).try_get_matches_from(["go", "--name", "x", "--dry-run"]).unwrap();
    assert!(op.get_flag(DRY_RUN_ID));
    assert!(
        contributed("go", "Go", Some(&schema), false).try_get_matches_from(["go", "--name", "x", "--dry-run"]).is_err()
    );
    let help = contributed("go", "Go", Some(&schema), true).render_long_help().to_string();
    assert!(help.contains("The playlist (required)"), "{help}");
}

#[test]
fn options_take_their_schemas_types() {
    let schema = json!({
        "type": "object",
        "properties": {
            "level": { "type": "integer" },
            "loud": { "type": "boolean" },
            "lists": { "type": "array", "items": { "type": "string" } },
            "root": { "type": "string", "x-path": true },
        },
    });
    let given = |pairs: &[&str]| options(Some(&schema), &pairs.iter().map(|p| p.to_string()).collect::<Vec<_>>());
    let o = given(&["level=2", "loud=yes", "lists=a", "lists=b"]).unwrap();
    assert_eq!(o, json!({ "level": 2, "loud": true, "lists": ["a", "b"] }));
    assert!(Path::new(given(&["root=out"]).unwrap()["root"].as_str().unwrap()).is_absolute());
    assert_eq!(given(&["level=high"]).unwrap_err().to_string(), "the level option takes a whole number, not \"high\"");
    assert_eq!(given(&["loud=maybe"]).unwrap_err().to_string(), "the loud option takes true or false, not \"maybe\"");
    assert_eq!(
        given(&["other=x"]).unwrap_err().to_string(),
        "this format has no option \"other\": its options are level, loud, lists, root"
    );
    assert_eq!(given(&["level"]).unwrap_err().to_string(), "--option takes KEY=VALUE, not \"level\"");
}
