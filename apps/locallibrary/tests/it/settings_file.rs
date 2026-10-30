//! `settings export` and `settings import FILE`, run as a person runs them: everything set here
//! goes to a file, and another computer adds it to its own (taking the file's values with
//! `--replace`), a plugin's key bindings included.

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
