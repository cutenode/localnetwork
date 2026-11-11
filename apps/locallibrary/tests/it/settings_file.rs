//! `settings export` and `settings import FILE`, run as a person runs them: everything set here
//! goes to a file, and another computer adds it to its own (taking the file's values with
//! `--replace`), a plugin's key bindings included.

use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use crate::support::{Env, path};

#[test]
fn settings_go_to_a_file_and_come_back_elsewhere() {
    let (here, there) = (Env::new(), Env::new());
    here.json(&["--yes", "settings", "set", "quantize=off", "jog_mode=cdj"]);
    let plugin = here.store.join("plugins/sound");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("settings.json"), json!({ "keys.analyze": "a" }).to_string()).unwrap();

    let file = here.outside("settings.json");
    let said = here.ok(&["settings", "export", path(&file)]);
    assert!(said.contains(&format!("wrote {}", path(&file))), "{said}");
    let written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(written["plugins"]["sound"]["keys.analyze"], "a", "{written}");
    assert_eq!(serde_json::from_str::<Value>(&here.ok(&["settings", "export"])).unwrap(), written);
    let err = here.fails(1, &["settings", "export", path(&file)]);
    assert!(err.contains("is already there"), "{err}");
    here.ok(&["settings", "export", path(&file), "--replace"]);

    there.json(&["--yes", "settings", "set", "quantize=on"]);
    let shown = there.ok(&["settings", "import", path(&file), "--dry-run"]);
    assert!(shown.contains("+ my_settings.base.jog_mode = cdj"), "{shown}");
    assert!(shown.contains("+ plugins.sound.keys.analyze = a"), "{shown}");
    assert!(shown.contains("= my_settings.base.quantize: on (the file has off)"), "{shown}");
    assert!(!there.store.join("plugins").exists(), "a dry run writes nothing");

    let v = there.json(&["--yes", "settings", "import", path(&file)]);
    assert_eq!(v["applied"], true, "{v}");
    assert_eq!(v["report"]["my_settings"]["base"], json!({ "jog_mode": "cdj", "quantize": "on" }), "{v}");
    assert_eq!(v["report"]["plugins"]["sound"]["keys.analyze"], "a", "{v}");
    let v = there.json(&["settings", "import", path(&file)]);
    assert_eq!(v["outcome"], "nothing_to_do", "{v}");
    let v = there.json(&["--yes", "settings", "import", path(&file), "--replace"]);
    assert_eq!(v["report"]["my_settings"]["base"]["quantize"], "off", "{v}");
}

/// Settings this property sets, with the values each takes.
const SET: [(&str, [&str; 2]); 3] =
    [("quantize", ["on", "off"]), ("jog_mode", ["cdj", "vinyl"]), ("auto_cue", ["on", "off"])];

/// For any of [`SET`] set here and any set there, a file exported here and imported there (with
/// `--replace` or not) shows a line for each value of the file's that isn't there as it is: `+`
/// one it adds, `~` one it replaces, `=` one it keeps, saying what the file has. With `--yes`
/// it imports them, saying how many, so there each is the file's unless it was kept; with
/// nothing to add or replace, it says so and changes nothing.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_settings_file_imports_as_its_plan_shows(tc: TestCase) {
    let (here, there) = (Env::new(), Env::new());
    let draw = || -> Vec<Option<&str>> {
        SET.iter().map(|(_, values)| tc.draw(gs::optional(gs::sampled_from(values.to_vec())))).collect()
    };
    let (mut ours, mut theirs) = (draw(), draw());
    // The first is set on both sides, differently: replaced, or kept as it is there.
    let first = tc.draw(gs::booleans());
    (ours[0], theirs[0]) = (Some(SET[0].1[usize::from(first)]), Some(SET[0].1[usize::from(!first)]));
    for (env, values) in [(&here, &ours), (&there, &theirs)] {
        let set: Vec<String> = SET.iter().zip(values).filter_map(|((k, _), v)| v.map(|v| format!("{k}={v}"))).collect();
        if !set.is_empty() {
            let words: Vec<&str> =
                ["--yes", "settings", "set"].into_iter().chain(set.iter().map(String::as_str)).collect();
            env.json(&words);
        }
    }
    let file = here.outside("settings.json");
    here.ok(&["settings", "export", path(&file)]);
    let replace = tc.draw(gs::booleans());
    let mut import = vec!["settings", "import", path(&file)];
    if replace {
        import.push("--replace");
    }
    let shown = there.ok(&[&import[..], &["--dry-run"]].concat());
    let mut changed = 0;
    let mut after = Vec::new();
    for (((k, _), file_has), is) in SET.iter().zip(&ours).zip(&theirs) {
        let key = format!("my_settings.base.{k}");
        let line = match (file_has, is) {
            (Some(f), None) => Some(format!("+ {key} = {f}")),
            (Some(f), Some(t)) if f != t && replace => Some(format!("~ {key}: {t} → {f}")),
            (Some(f), Some(t)) if f != t => Some(format!("= {key}: {t} (the file has {f})")),
            _ => None,
        };
        let shows = shown.lines().any(|l| l.contains(&format!(" {key} ")) || l.contains(&format!(" {key}:")));
        assert_eq!(shows, line.is_some(), "{key}: {shown}");
        if let Some(line) = &line {
            assert!(shown.contains(line), "{line:?}: {shown}");
            changed += usize::from(!line.starts_with('='));
        }
        let kept = is.is_some() && !replace;
        after.push(if kept { *is } else { file_has.or(*is) });
    }
    let applied = there.ok(&[&["--yes"][..], &import].concat());
    match changed {
        0 => assert!(applied.contains("everything the file has is set here already"), "{applied}"),
        n => {
            let imported = format!("✓ imported {n} setting{} from ", if n == 1 { "" } else { "s" });
            assert!(applied.contains(&imported), "{applied}");
            // The store keeps them, but they're no snapshot.
            let log = there.ok(&["log"]);
            assert!(log.starts_with("The store is empty."), "{log}");
        }
    }
    let exported: Value = serde_json::from_str(&there.ok(&["settings", "export"])).unwrap();
    for ((k, _), v) in SET.iter().zip(&after) {
        assert_eq!(exported["my_settings"]["base"][k].as_str(), *v, "{k}: {exported}");
    }
}
