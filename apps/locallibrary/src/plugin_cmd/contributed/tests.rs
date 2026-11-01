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
            "choices": { "type": "object", "additionalProperties": { "type": "integer" }, "x-cli": { "flag": "pick" } },
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
            "--pick",
            "1=42",
            "--pick",
            " 3 = #7 ",
            "--names",
            "x=y",
        ],
        false,
    )
    .unwrap();
    let expected = json!({
        "name": "Fresh", "min_bpm": 120.5, "count": 3, "append": true, "mode": "slow", "level": 2, "tags": ["a", "b"],
        "ids": [4], "playlist": "Sets", "choices": { "1": 42, "3": 7 }, "names": { "x": "y" },
    });
    assert_eq!(p, expected);
    // A required param is asked for by its flag, unless `--params` gives the params.
    let missing = given(&[], false).unwrap_err().to_string();
    assert!(missing.contains("the following required arguments were not provided:\n  --name <NAME>"), "{missing}");
    assert_eq!(given(&["--params", "{}"], false).unwrap(), json!({}));
}

/// `--choose` answers a question the command asked, over what's given otherwise: JSON, else text.
#[test]
fn choose_answers_a_question_over_the_flags() {
    let p = given(&["--name", "x", "--choose", "name=Peak Time", "--choose", "7=42", "--choose", "8=null"], true);
    assert_eq!(p.unwrap(), json!({ "name": "Peak Time", "7": 42, "8": null }));
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
    assert_eq!(wrong(&["--pick", "1"]), "--pick takes KEY=VALUE, not \"1\"");
    assert_eq!(wrong(&["--pick", "1=x"]), "--pick takes a whole number, not \"x\"");
    assert_eq!(wrong(&["--choose", "1"]), "--choose takes PARAM=VALUE, not \"1\"");
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
    let given =
        |pairs: &[&str]| options("format", Some(&schema), &pairs.iter().map(|p| p.to_string()).collect::<Vec<_>>());
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

/// A param a drawn schema has: its name and JSON Schema, what's typed for it (after the drive,
/// for the positional one) and the value the command then gets, if it gets one.
struct Param {
    name: String,
    schema: Value,
    /// Given with this flag (`None`: positionally).
    flag: Option<String>,
    required: bool,
    args: Vec<String>,
    expected: Option<Value>,
    /// Typed text its type can't hold, and what refusing it says.
    wrong: Option<(String, String)>,
}

/// Text a person types for a param, with blanks around it now and then (which are dropped).
#[hegel::test_helper]
fn typed(tc: &hegel::TestCase, path: bool) -> String {
    use hegel::generators as gs;
    let text = match path {
        true => tc.draw(gs::text().alphabet("abz._/").min_size(1).max_size(8)),
        false => tc.draw(gs::text().alphabet("abcXYZ 09._é日").max_size(8)),
    };
    match tc.draw(gs::weighted_booleans(0.2)) {
        true => format!(" {text} "),
        false => text,
    }
}

/// A flag and the value typed for it: `--flag=-5` for one that starts as a flag would, which
/// the command line would otherwise read as one.
fn flagged(flag: &str, raw: &str) -> Vec<String> {
    match raw.starts_with('-') {
        true => vec![format!("--{flag}={raw}")],
        false => vec![format!("--{flag}"), raw.to_string()],
    }
}

/// `raw` as the command gets a string param: trimmed, and made absolute when it's a file.
fn string_of(raw: &str, path: bool) -> Value {
    match path {
        true => json!(std::path::absolute(raw.trim()).unwrap().display().to_string()),
        false => json!(raw.trim()),
    }
}

/// The `i`th param of a drawn schema, of a kind the command line takes as text (or one it
/// doesn't, given only with `--params`), given or not. `positional` once one is.
#[hegel::test_helper]
fn param(tc: &hegel::TestCase, i: usize, positional: &mut bool) -> Param {
    use hegel::generators as gs;
    // Unique by their numbers: no two flags alike, and none the command has anyway.
    let base = tc.draw(gs::from_regex("[a-z]{1,5}"));
    let name = format!("{base}_{i}");
    let kind = tc.draw(gs::integers::<u8>().max_value(9));
    let given = tc.draw(gs::weighted_booleans(0.7));
    let mut schema = match kind {
        0 | 9 => json!({ "type": "string" }),
        1 => json!({ "type": "number" }),
        2 => json!({ "type": "integer" }),
        3 => json!({ "type": "boolean" }),
        4 => json!({ "type": "array", "items": { "type": "string" } }),
        5 => json!({ "type": "array", "items": { "type": "integer" } }),
        6 => json!({ "type": "object", "additionalProperties": { "type": "integer" } }),
        7 => json!({ "type": "object", "additionalProperties": { "type": "string" } }),
        _ => json!({ "type": "object", "additionalProperties": { "type": "object" } }),
    };
    let positional = match kind == 9 && !*positional {
        true => {
            *positional = true;
            schema["x-cli"] = json!({ "positional": true });
            true
        }
        false => false,
    };
    let flag = match !positional && tc.draw(gs::weighted_booleans(0.3)) {
        true => {
            let flag = format!("{base}-x{i}");
            schema["x-cli"] = json!({ "flag": flag });
            flag
        }
        false => name.replace('_', "-"),
    };
    let path = matches!(kind, 0 | 4) && tc.draw(gs::weighted_booleans(0.3));
    if path {
        schema["x-path"] = json!(true);
    }
    let choices: Option<Vec<String>> = match kind {
        0 if !path && tc.draw(gs::weighted_booleans(0.3)) => {
            Some(tc.draw(gs::vecs(gs::from_regex("[a-z]{1,4}")).min_size(1).max_size(3).unique(true)))
        }
        2 if tc.draw(gs::weighted_booleans(0.3)) => {
            let n = tc.draw(gs::vecs(gs::integers::<i64>().min_value(-9).max_value(99)).min_size(1).max_size(3));
            let mut n: Vec<i64> = n.into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            n.sort();
            schema["enum"] = json!(n);
            Some(n.iter().map(i64::to_string).collect())
        }
        _ => None,
    };
    if kind == 0
        && let Some(c) = &choices
    {
        schema["enum"] = json!(c);
    }
    let what = match positional {
        true => name.to_uppercase(),
        false => format!("--{flag}"),
    };
    let integer = |tc: &hegel::TestCase| {
        let n = tc.draw(gs::integers::<i64>().min_value(-1000).max_value(1000));
        let raw = match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => n.to_string(),
            1 => format!("#{n}"),
            _ => format!(" {n} "),
        };
        (raw, json!(n))
    };
    let (raws, expected): (Vec<String>, Option<Value>) = match kind {
        _ if !given => (Vec::new(), None),
        0 | 9 => match &choices {
            Some(c) => {
                let pick = tc.draw(gs::sampled_from(c.clone()));
                (vec![pick.clone()], Some(json!(pick)))
            }
            None => {
                let raw = typed(tc, path);
                let value = string_of(&raw, path);
                (vec![raw], Some(value))
            }
        },
        1 => {
            let f = tc.draw(gs::floats::<f64>().min_value(-1e6).max_value(1e6).allow_nan(false));
            // With its point (`{f:?}`): a whole number reads back as one, not as this float.
            (vec![format!("{f:?}")], Some(json!(f)))
        }
        2 => match &choices {
            Some(c) => {
                let pick = tc.draw(gs::sampled_from(c.clone()));
                let n: i64 = pick.parse().unwrap();
                (vec![pick], Some(json!(n)))
            }
            None => {
                let (raw, n) = integer(tc);
                (vec![raw], Some(n))
            }
        },
        3 => (Vec::new(), Some(json!(true))),
        4 => {
            let raws: Vec<String> =
                (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3))).map(|_| typed(tc, path)).collect();
            let values = raws.iter().map(|r| string_of(r, path)).collect();
            (raws, Some(Value::Array(values)))
        }
        5 => {
            let drawn: Vec<(String, Value)> =
                (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3))).map(|_| integer(tc)).collect();
            let values = drawn.iter().map(|(_, v)| v.clone()).collect();
            (drawn.into_iter().map(|(r, _)| r).collect(), Some(Value::Array(values)))
        }
        6 | 7 => {
            let mut pairs = Map::new();
            let mut raws = Vec::new();
            for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
                let key = tc.draw(gs::from_regex("[a-z]{1,3}"));
                let (raw, value) = match kind {
                    6 => integer(tc),
                    _ => {
                        let raw = typed(tc, false);
                        let value = string_of(&raw, false);
                        (raw, value)
                    }
                };
                raws.push(format!("{key}={raw}"));
                pairs.insert(key, value);
            }
            (raws, Some(Value::Object(pairs)))
        }
        _ => (Vec::new(), None),
    };
    let args = match (positional, kind) {
        (true, _) => raws,
        (false, 3) if given => vec![format!("--{flag}")],
        (false, _) => raws.iter().flat_map(|r| flagged(&flag, r)).collect(),
    };
    let wrong = match kind {
        1 if tc.draw(gs::weighted_booleans(0.2)) => {
            Some((format!("x{i}"), format!("{what} takes a number, not \"x{i}\"")))
        }
        2 | 5 if choices.is_none() && tc.draw(gs::weighted_booleans(0.2)) => {
            Some((format!("{i}.5"), format!("{what} takes a whole number, not \"{i}.5\"")))
        }
        6 if tc.draw(gs::weighted_booleans(0.2)) => {
            Some((i.to_string(), format!("{what} takes KEY=VALUE, not \"{i}\"")))
        }
        _ => None,
    };
    let required = kind != 3 && kind != 8 && tc.draw(gs::weighted_booleans(0.3));
    Param { name, schema, flag: (!positional).then_some(flag), required, args, expected, wrong }
}

/// A command's flags, made from any schema of params the command line takes, give its params of
/// their types: the last of a value given again, each of a list, each `KEY=VALUE`, files made
/// absolute, over what `--params` gives. A required one
/// left out, or a value its type can't hold, is refused naming its flag.
#[hegel::test(test_cases = 200)]
fn flags_drawn_from_a_schema_give_the_params_typed(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut positional = false;
    let count = tc.draw(gs::integers::<usize>().max_value(6));
    let params: Vec<Param> = (0..count).map(|i| param(&tc, i, &mut positional)).collect();
    let properties: Map<String, Value> = params.iter().map(|p| (p.name.clone(), p.schema.clone())).collect();
    let required: Vec<&str> = params.iter().filter(|p| p.required).map(|p| p.name.as_str()).collect();
    let schema = json!({ "type": "object", "properties": properties, "required": required });
    let operation = tc.draw(gs::booleans());
    let cmd = contributed("go", "Go", Some(&schema), operation);

    // What `--params` gives first: a value for some params, given with flags too or not.
    let base: Option<Map<String, Value>> = tc.draw(gs::weighted_booleans(0.3)).then(|| {
        let some = tc.draw(gs::subsequences(params.iter().map(|p| p.name.clone()).collect::<Vec<_>>()));
        some.into_iter().map(|name| (name, json!({ "from": "json" }))).collect()
    });
    let mut args = vec!["go".to_string(), "USB".into()];
    let (named, positional): (Vec<&Param>, Vec<&Param>) = params.iter().partition(|p| p.flag.is_some());
    args.extend(positional.iter().flat_map(|p| p.args.clone()));
    let wrong = tc.draw(gs::booleans()).then(|| named.iter().find_map(|p| Some((p, p.wrong.clone()?)))).flatten();
    let wrong_one = wrong.as_ref().map(|(p, _)| p.name.as_str());
    for p in &named {
        // A flag taking one value is given once (the command line refuses it twice): the wrong
        // value stands in for the right one.
        let once = p.schema["type"] != "array" && p.schema["type"] != "object";
        if !(once && wrong_one == Some(p.name.as_str())) {
            args.extend(p.args.clone());
        }
    }
    if let Some(base) = &base {
        args.extend(["--params".to_string(), Value::Object(base.clone()).to_string()]);
    }
    let dry_run = operation && tc.draw(gs::booleans());
    if dry_run {
        args.push("--dry-run".into());
    }
    if let Some((p, (raw, _))) = &wrong {
        args.extend(flagged(p.flag.as_ref().unwrap(), raw));
    }
    tc.note(&format!("schema: {schema}\nargs: {args:?}"));

    let matches = cmd.try_get_matches_from(&args);
    let missing: Vec<&&Param> =
        named.iter().filter(|p| p.required && p.expected.is_none() && wrong_one != Some(p.name.as_str())).collect();
    let missing_positional = positional.iter().any(|p| p.required && p.expected.is_none());
    if base.is_none() && (!missing.is_empty() || missing_positional) {
        let e = matches.expect_err("a required param left out is refused").to_string();
        assert!(e.contains("the following required arguments were not provided"), "{e}");
        for p in missing {
            assert!(e.contains(&format!("--{}", p.flag.as_ref().unwrap())), "{e}");
        }
        return;
    }
    let m = matches.unwrap_or_else(|e| panic!("{e}"));
    if operation {
        assert_eq!(m.get_flag(DRY_RUN_ID), dry_run);
    }
    assert_eq!(m.get_one::<String>(DRIVE).map(String::as_str), Some("USB"));
    let got = params_of(Some(&schema), &m);
    if let Some((_, (_, message))) = &wrong {
        // Flags are read in the schema's order: the first param with a wrong value is the one
        // refused, and only one has one here.
        assert_eq!(got.unwrap_err().to_string(), *message);
        return;
    }
    let mut expected = base.unwrap_or_default();
    for p in &params {
        if let Some(v) = &p.expected {
            expected.insert(p.name.clone(), v.clone());
        }
    }
    assert_eq!(got.unwrap(), Value::Object(expected));
}

/// An exporter's options, as `--option KEY=VALUE`s for any schema, are of their types (a key
/// given again adds to a list, or the last wins), and a key the format has no option for, or a
/// pair without `=`, is refused saying what it takes.
#[hegel::test(test_cases = 200)]
fn options_drawn_from_a_schema_are_typed_or_refused(tc: hegel::TestCase) {
    use hegel::generators as gs;
    const KINDS: [&str; 6] = ["integer", "number", "boolean", "string", "list", "json"];
    let keys: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[a-z]{1,4}")).max_size(4).unique(true));
    let kinds: Vec<&str> = keys.iter().map(|_| tc.draw(gs::sampled_from(&KINDS[..]))).collect();
    let properties: Map<String, Value> = keys
        .iter()
        .zip(&kinds)
        .map(|(k, kind)| {
            let schema = match *kind {
                "list" => json!({ "type": "array", "items": { "type": "string" } }),
                // Of a type no text stands for: given as JSON.
                "json" => json!({ "type": "object", "additionalProperties": { "type": "object" } }),
                kind => json!({ "type": kind }),
            };
            (k.clone(), schema)
        })
        .collect();
    let schema = json!({ "type": "object", "properties": properties });
    // Each pair given: a key, and the text typed for it.
    let mut given: Vec<(String, String)> = Vec::new();
    let mut expected = Map::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(5)) {
        if keys.is_empty() {
            break;
        }
        let i = tc.draw(gs::integers::<usize>().max_value(keys.len() - 1));
        let (raw, value) = match kinds[i] {
            "integer" => {
                let n = tc.draw(gs::integers::<i64>().min_value(-99).max_value(99));
                (format!("#{n}"), json!(n))
            }
            "number" => {
                let f = tc.draw(gs::floats::<f64>().min_value(-1e3).max_value(1e3).allow_nan(false));
                // With its point, as above.
                (format!("{f:?}"), json!(f))
            }
            "boolean" => {
                let (raw, b) =
                    tc.draw(gs::sampled_from(
                        &[
                            ("yes", true),
                            (" ON ", true),
                            ("true", true),
                            ("no", false),
                            ("Off", false),
                            ("false", false),
                        ][..],
                    ));
                (raw.to_string(), json!(b))
            }
            "json" => {
                let key: String = tc.draw(gs::from_regex("[a-z]{1,3}"));
                let v = Value::Object(Map::from_iter([(key, json!({}))]));
                (format!(" {v} "), v)
            }
            _ => {
                let raw = tc.draw(gs::from_regex("[a-z =]{0,6}"));
                (raw.clone(), json!(raw.trim()))
            }
        };
        let value = match kinds[i] {
            "list" => {
                let mut list = expected.get(&keys[i]).and_then(Value::as_array).cloned().unwrap_or_default();
                list.push(value);
                Value::Array(list)
            }
            _ => value,
        };
        expected.insert(keys[i].clone(), value);
        given.push((format!(" {} ", keys[i]), raw));
    }
    let mut typed: Vec<String> = given.iter().map(|(k, raw)| format!("{k}={raw}")).collect();
    let bad = tc.draw(gs::integers::<u8>().max_value(3));
    let refused = match bad {
        0 => {
            let unknown = format!("{}x", keys.concat());
            typed.push(format!("{unknown}=1"));
            Some(match keys.is_empty() {
                true => format!("this format has no options, not even {unknown:?}"),
                false => format!("this format has no option {unknown:?}: its options are {}", keys.join(", ")),
            })
        }
        1 => {
            typed.push("level".into());
            Some("--option takes KEY=VALUE, not \"level\"".to_string())
        }
        2 if !given.is_empty() => {
            // The last value of an option that isn't a list is the one read: a wrong one is refused.
            let i = keys.iter().position(|k| *k == given.last().unwrap().0.trim()).unwrap();
            let says = match kinds[i] {
                "integer" => Some("takes a whole number, not \"x\""),
                "number" => Some("takes a number, not \"x\""),
                "boolean" => Some("takes true or false, not \"x\""),
                "json" => Some("takes JSON, not \"x\""),
                _ => None,
            };
            says.map(|says| {
                typed.push(format!("{}=x", keys[i]));
                format!("the {} option {says}", keys[i])
            })
        }
        _ => None,
    };
    let got = options("format", Some(&schema), &typed);
    match refused {
        Some(message) => assert_eq!(got.unwrap_err().to_string(), message),
        None => assert_eq!(got.unwrap(), Value::Object(expected), "{typed:?}"),
    }
}
