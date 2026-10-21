//! Panels, views of every kind, themes and how analyzers' fields are shown, on the command line
//! and in the sidecar, with the test plugin's panel `said` answering as its `panel` setting says.

use serde_json::{Value, json};

use crate::support::{Env, Rpc, jpeg, path};

/// The echo manifest these tests register, with a panel `said`, a setting it answers by, and its
/// `bytes` field shown in bytes.
fn panelled() -> String {
    let more = "\n[[contributes.panels]]\nname = \"said\"\ntitle = \"As it says\"\n\n[[contributes.settings]]\nkey = \"panel\"\ntitle = \"What it shows\"\ntype = \"string\"\n";
    super::echo().replace(
        "fields = { bytes = \"integer\" }",
        "fields = { bytes = \"integer\" }\ndisplay.bytes = { unit = \"B\" }",
    ) + more
}

fn env() -> Env {
    let mut env = Env::new();
    env.plugins(&[&panelled()]);
    env.demo("USB");
    env
}

/// The `said` panel answers `answer`.
fn saying(env: &Env, answer: Value) {
    env.ok(&["--yes", "plugin", "settings", "echo", "panel", &answer.to_string()]);
}

/// `track show` prints each panel as a section under a track's details (and with `--json`, each
/// panel's view), a panel that fails as a warning while the rest show; a field as its analyzer
/// says it's shown.
#[test]
fn track_show_prints_panels_under_the_details() {
    let env = env();
    let out = env.ok(&["track", "USB", "--track", "1"]);
    assert!(out.contains("\nAbout the track\nid     1\ntitle  Demo Track 1\n"), "{out}");
    assert!(out.contains("\nAs it says\nnothing to show\n"), "{out}");
    let shown = env.json(&["track", "USB", "--track", "1"]);
    let pairs = json!([{ "label": "id", "value": 1 }, { "label": "title", "value": "Demo Track 1" }]);
    let about = json!({ "plugin": "echo", "panel": "about", "title": "About the track", "view": { "kind": "kv", "pairs": pairs } });
    assert_eq!(shown[0]["panels"][0], about);
    saying(&env, json!({ "fail": "invalid" }));
    let out = env.ok(&["track", "USB", "--track", "1"]);
    assert!(out.contains("! As it says: it failed (the Echo plugin)") && out.contains("title  Demo Track 1"), "{out}");
    assert_eq!(env.json(&["track", "USB", "--track", "1"])[0]["panels"][1]["error"], "it failed (the Echo plugin)");

    env.ok(&["analyze", "USB", "--analyzer", "size"]);
    let out = env.ok(&["track", "USB", "--track", "1"]);
    let line = crate::support::line(&out, "echo.bytes");
    assert!(line.ends_with(" B"), "{out}");
}

/// Every kind of view is drawn as plain text: a tree indented, a chart as a table of its points,
/// an image as what it shows, sections each under its title, a table's typed columns as they say.
#[test]
fn every_kind_of_view_is_plain_text_on_the_command_line() {
    let env = env();
    let view = |v: Value, blobs: Value| {
        let params = json!({ "view": v, "blobs": blobs }).to_string();
        env.ok(&["echo", "titles", "USB", "--params", &params])
    };
    let tree = json!({ "kind": "tree", "title": "Lists", "items": [{ "label": "Club", "detail": "2 lists", "children": [{ "label": "Warm up" }] }] });
    assert_eq!(view(tree.clone(), json!({})), "Lists\nClub  2 lists\n  Warm up\n");
    let chart = json!({ "kind": "chart", "style": "bar", "series": [{ "name": "plays", "points": [[1, 4], [2, 6]] }] });
    assert_eq!(view(chart, json!({})), "  x  plays\n1.0    4.0\n2.0    6.0\n");
    let image = json!({ "kind": "image", "blob": "a.jpg", "alt": "the cover" });
    assert_eq!(view(image.clone(), json!({ "a.jpg": jpeg(16) })), "[image: the cover]\n");
    let sections = json!({ "kind": "sections", "items": [{ "title": "Shape", "view": tree }, { "title": "None", "view": { "kind": "list", "items": [] } }] });
    // A section showing nothing says so; the command line's own empty lists say nothing.
    assert_eq!(view(sections, json!({})), "Shape\nClub  2 lists\n  Warm up\nNone\nnothing to show\n");
    assert_eq!(view(json!({ "kind": "tree", "items": [] }), json!({})), "");
    let typed = json!({ "kind": "table", "columns": [{ "name": "Track", "kind": "track" }, { "name": "Time", "kind": "duration" }, { "name": "Key", "kind": "key" }], "rows": [[1, 187, "Am"], [9, 61, "x"]] });
    assert_eq!(
        view(typed, json!({})),
        "Track                       Time  Key\nLoopmasters - Demo Track 1  3:07  8A\n#9                          1:01  x\n"
    );
    // With `--json`, the image as the host read it.
    let params = json!({ "view": image, "blobs": { "a.jpg": jpeg(16) } }).to_string();
    let read = env.json(&["echo", "titles", "USB", "--params", &params]);
    assert!(read["data"].as_str().unwrap().starts_with("data:image/png;base64,"), "{read}");
}

/// `plugin list` and `plugin info` say what panels and themes a plugin adds; the sidecar lists
/// them, and renders a panel's view of a track (`plugins.panel`).
#[test]
fn panels_and_themes_are_listed_and_the_sidecar_renders_a_panel() {
    let env = env();
    let list = env.ok(&["plugin", "list"]);
    assert!(list.contains("; provider tags; panels about, said; theme night; settings greeting, panel"), "{list}");
    let info = env.ok(&["plugin", "info", "echo"]);
    assert!(info.contains("Panels (locallibrary track show, and the player):"), "{info}");
    assert!(crate::support::line(&info, "    about").ends_with("About the track, under a track's details"), "{info}");
    assert!(crate::support::line(&info, "    night").ends_with("Night, in the player"), "{info}");

    let drive = path(&env.volumes.join("USB")).to_string();
    let mut s = Rpc::start(env.command());
    let result = |r: Value| r.get("result").cloned().unwrap_or_else(|| panic!("{r}"));
    let listed = result(s.call(1, "plugins.list", Value::Null));
    let echo = listed.as_array().unwrap().iter().find(|p| p["id"] == "echo").unwrap().clone();
    assert_eq!((&echo["panels"], &echo["themes"]), (&json!(["about", "said"]), &json!(["night"])));
    let rendered =
        result(s.call(2, "plugins.panel", json!({ "plugin": "echo", "panel": "about", "drive": drive, "track": 1 })));
    assert_eq!((&rendered["kind"], &rendered["pairs"][1]["value"]), (&json!("kv"), &json!("Demo Track 1")));
    let missing = s.call(3, "plugins.panel", json!({ "plugin": "echo", "panel": "nope", "drive": drive, "track": 1 }));
    assert_eq!(missing["error"]["data"]["kind"], "not_found", "{missing}");
}
