//! Property tests of plugins on the command line and in the sidecar, with drawn manifests the
//! test plugin serves: `plugin list` and `plugin info` say in text what `--json` says, settings
//! read back as they were set, a command's flags give it the params typed, its views read as
//! their JSON, analyzers keep what they worked out, a plugin that clashes is left out, and the
//! sidecar's `plugins.*` methods answer in the contract's shapes what the command line says.

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use serde_json::{Map, Value, json};

use super::SHIPPED;
use crate::props_rpc::{at, contract, violation};
use crate::support::{Env, Rpc, path};

/// `v` (a JSON object without nulls) as TOML, which the test plugin reads manifests in: each
/// key on a line of its own, with inline tables and arrays under it.
fn toml(v: &Value) -> String {
    fn inline(v: &Value) -> String {
        match v {
            Value::Object(o) => {
                let pairs: Vec<String> = o.iter().map(|(k, v)| format!("{} = {}", json!(k), inline(v))).collect();
                format!("{{ {} }}", pairs.join(", "))
            }
            Value::Array(a) => format!("[{}]", a.iter().map(inline).collect::<Vec<_>>().join(", ")),
            Value::Null => panic!("TOML has no null"),
            // JSON's strings, numbers and booleans are TOML's too.
            other => other.to_string(),
        }
    }
    v.as_object().unwrap().iter().map(|(k, v)| format!("{} = {}\n", json!(k), inline(v))).collect()
}

/// Every capability a manifest can name but `net:<host>`.
const CAPABILITIES: [&str; 16] = [
    "library:read",
    "analysis:read",
    "artwork:read",
    "drive:info",
    "health:read",
    "store:read",
    "settings:read",
    "plays:read",
    "sets:read",
    "account:status",
    "collection:read",
    "library:plan",
    "library:delete",
    "tags:plan",
    "cues:plan",
    "artwork:plan",
];

/// Text a person reads in a manifest: a title or a name.
#[hegel::test_helper]
fn title(tc: &TestCase) -> String {
    match tc.draw(gs::weighted_booleans(0.7)) {
        true => tc.draw(gs::sampled_from(&["Echo", "Make a playlist", "Ünïcödé 日本", "x", "Two  spaces"][..])).into(),
        false => {
            let t = tc.draw(gs::text().alphabet("abc XYZ.é\"\\").min_size(1).max_size(12));
            match t.trim().is_empty() {
                true => "t".into(),
                false => t,
            }
        }
    }
}

/// A setting of a drawn kind, with a default or not, keyed `key`.
#[hegel::test_helper]
fn setting(tc: &TestCase, key: &str) -> Value {
    let kind = tc.draw(gs::sampled_from(&["string", "number", "boolean", "enum", "file", "directory"][..]));
    let mut s = json!({ "key": key, "title": title(tc), "type": kind });
    let choices: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[a-z]{1,5}")).min_size(1).max_size(3).unique(true));
    if kind == "enum" {
        s["choices"] = json!(choices);
    }
    if tc.draw(gs::booleans()) {
        s["default"] = match kind {
            "number" => match tc.draw(gs::booleans()) {
                true => json!(tc.draw(gs::integers::<i64>().min_value(-500).max_value(500))),
                false => json!(tc.draw(gs::floats::<f64>().min_value(-1e4).max_value(1e4).allow_nan(false))),
            },
            "boolean" => json!(tc.draw(gs::booleans())),
            "enum" => json!(tc.draw(gs::sampled_from(choices))),
            "file" | "directory" => json!(tc.draw(gs::sampled_from(&["/tmp/x", "~/Music", "list.txt"][..]))),
            _ => json!(title(tc)),
        };
    }
    s
}

/// A params schema of a few simple properties, none required.
#[hegel::test_helper]
fn params(tc: &TestCase) -> Value {
    let kinds = ["string", "integer", "number", "boolean"];
    let properties: Map<String, Value> = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|i| (format!("p{i}"), json!({ "type": tc.draw(gs::sampled_from(&kinds[..])) })))
        .collect();
    json!({ "type": "object", "properties": properties })
}

/// `x` (a command or an operation) offered in drawn menus, or where it is when it doesn't say.
#[hegel::test_helper]
fn offered(tc: &TestCase, x: &mut Value) {
    if tc.draw(gs::booleans()) {
        x["where"] = json!(tc.draw(gs::subsequences(&["track", "playlist", "drive"][..]).min_size(1)));
    }
}

/// A plugin `id` with drawn capabilities and contributions: commands (`cmd0`...) and
/// operations (`op0`...), each at a place of its own on the command line or not and in drawn
/// menus; analyzers of drawn fields and versions, and the test plugin's `size` (`bytes`) and
/// `order` (`a`, `b`) it can run; export formats writing a file or a folder; providers of either
/// kind (granted reading tracks); and settings of every kind.
#[hegel::test_helper]
fn manifest(tc: &TestCase, id: &str) -> Value {
    let version = tc.draw(gs::sampled_from(&["0.1.0", "1.2.3-beta.1", "10.0.0"][..]));
    let mut capabilities: Vec<String> = tc
        .draw(gs::vecs(gs::sampled_from(&CAPABILITIES[..])).max_size(4).unique(true))
        .into_iter()
        .map(String::from)
        .collect();
    if tc.draw(gs::weighted_booleans(0.3)) {
        capabilities.push(format!("net:{}", tc.draw(gs::sampled_from(&["musicbrainz.org", "localhost"][..]))));
    }
    let place = |tc: &TestCase, name: &str| -> Option<String> {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => Some(format!("{id}-{name}")),
            1 => Some(format!("playlist {id}-{name}")),
            _ => None,
        }
    };
    let mut commands = Vec::new();
    for j in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
        let mode = tc.draw(gs::sampled_from(&["headless", "view"][..]));
        let mut c = json!({ "name": format!("cmd{j}"), "title": title(tc), "mode": mode });
        if tc.draw(gs::booleans()) {
            c["params"] = params(tc);
        }
        if let Some(p) = place(tc, &format!("cmd{j}")) {
            c["cli"] = json!(p);
        }
        offered(tc, &mut c);
        commands.push(c);
    }
    let mut operations = Vec::new();
    for j in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let mut o = json!({ "name": format!("op{j}"), "title": title(tc) });
        if tc.draw(gs::booleans()) {
            o["params"] = params(tc);
        }
        if let Some(p) = place(tc, &format!("op{j}")) {
            o["cli"] = json!(p);
        }
        offered(tc, &mut o);
        operations.push(o);
    }
    let mut analyzers = Vec::new();
    let types = ["number", "integer", "string", "boolean", "string[]"];
    for j in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let fields: Map<String, Value> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
            .map(|k| (format!("f{j}-{k}"), json!(tc.draw(gs::sampled_from(&types[..])))))
            .collect();
        let version = tc.draw(gs::sampled_from(&["1", "0.2.0", "2026-10_b+1"][..]));
        analyzers.push(json!({ "name": format!("an{j}"), "title": title(tc), "version": version, "fields": fields }));
    }
    if tc.draw(gs::booleans()) {
        let size = json!({ "name": "size", "title": "File size", "version": "1", "fields": { "bytes": "integer" } });
        analyzers.push(size);
    }
    if tc.draw(gs::booleans()) {
        let fields = json!({ "a": "integer", "b": "integer" });
        analyzers.push(json!({ "name": "order", "title": "Order", "version": "1", "fields": fields }));
    }
    let checks: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|j| {
            let mut c = json!({ "name": format!("ck{j}"), "title": title(tc) });
            if tc.draw(gs::booleans()) {
                c["default_on"] = json!(tc.draw(gs::booleans()));
            }
            if tc.draw(gs::booleans()) {
                c["when"] = json!(tc.draw(gs::sampled_from(&[&["check"][..], &["apply"], &["apply", "check"]][..])));
            }
            c
        })
        .collect();
    let exporters: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|j| {
            let mut e = json!({ "format": format!("{id}-x{j}"), "title": title(tc) });
            match tc.draw(gs::booleans()) {
                true => e["extension"] = json!(tc.draw(gs::sampled_from(&["txt", "m3u8", "nml"][..]))),
                false => e["folder"] = json!(true),
            }
            e
        })
        .collect();
    let providers: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|j| {
            let kind = tc.draw(gs::sampled_from(&["artwork", "metadata"][..]));
            json!({ "name": format!("pv{j}"), "title": title(tc), "kind": kind })
        })
        .collect();
    if !providers.is_empty() && !capabilities.iter().any(|c| c == "library:read") {
        capabilities.push("library:read".into());
    }
    let settings: Vec<Value> =
        (0..tc.draw(gs::integers::<usize>().max_value(4))).map(|j| setting(tc, &format!("k{j}"))).collect();
    json!({
        "id": id,
        "name": title(tc),
        "version": version,
        "protocol": "^0.2",
        "capabilities": capabilities,
        "contributes": {
            "commands": commands,
            "operations": operations,
            "analyzers": analyzers,
            "checks": checks,
            "exporters": exporters,
            "providers": providers,
            "settings": settings,
        },
    })
}

/// An [`Env`] with `manifests` besides the plugins locallibrary ships.
fn env_with(manifests: &[Value]) -> Env {
    let mut env = Env::new();
    let tomls: Vec<String> = manifests.iter().map(toml).collect();
    env.plugins(&tomls.iter().map(String::as_str).collect::<Vec<_>>());
    env
}

/// `v` without the `schema` every JSON document on stdout has.
fn unschema(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.shift_remove("schema");
    }
    v
}

/// A value as the command line shows it: text as it is, a list joined, nothing as `-`.
fn cell(v: &Value) -> String {
    match v {
        Value::Null => "-".into(),
        Value::String(s) => s.clone(),
        Value::Array(list) => list.iter().map(cell).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// `s` padded with spaces to `width` characters (all of them one column wide here).
fn pad(s: &str, width: usize) -> String {
    format!("{s}{}", " ".repeat(width.saturating_sub(s.chars().count())))
}

/// What `plugin list` says of the plugins `--json` lists.
fn listed_text(listed: &[Value]) -> String {
    let mut out = String::new();
    let names =
        |v: &Value| -> Vec<String> { v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().into()).collect() };
    for (i, p) in listed.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!(
            "{}  {} {}\n",
            p["id"].as_str().unwrap(),
            p["name"].as_str().unwrap(),
            p["version"].as_str().unwrap()
        ));
        for c in p["capabilities"].as_array().unwrap() {
            out.push_str(&format!("  may {}\n", c["means"].as_str().unwrap()));
        }
        let kinds = [
            ("command", "commands"),
            ("operation", "operations"),
            ("analyzer", "analyzers"),
            ("check", "checks"),
            ("export format", "exporters"),
            ("provider", "providers"),
            ("setting", "settings"),
        ];
        let adds: Vec<String> = kinds
            .iter()
            .filter_map(|(what, key)| {
                let all = names(&p[*key]);
                let s = if all.len() == 1 { "" } else { "s" };
                (!all.is_empty()).then(|| format!("{what}{s} {}", all.join(", ")))
            })
            .collect();
        if !adds.is_empty() {
            out.push_str(&format!("  adds {}\n", adds.join("; ")));
        }
    }
    out
}

/// What `plugin info` says of the plugin `--json` describes.
fn described_text(d: &Value) -> String {
    let m = &d["manifest"];
    let id = m["id"].as_str().unwrap();
    let str_of = |v: &Value| v.as_str().unwrap().to_string();
    let mut out = format!("{} ({id}) {}\n", str_of(&m["name"]), str_of(&m["version"]));
    let mut section = |title: String, lines: Vec<(String, String)>| {
        if lines.is_empty() {
            return;
        }
        out.push_str(&format!("  {title}\n"));
        let width = lines.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
        for (l, r) in lines {
            out.push_str(&match r.is_empty() {
                true => format!("    {l}\n"),
                false => format!("    {}  {r}\n", pad(&l, width)),
            });
        }
    };
    let list = |key: &str| m["contributes"][key].as_array().cloned().unwrap_or_default();
    let means = d["capabilities"].as_array().unwrap().iter().map(|c| (str_of(&c["means"]), String::new()));
    section("May:".into(), means.collect());
    let invocation = |x: &Value| match x["cli"].as_str() {
        Some(place) => format!("locallibrary {place}"),
        None => format!("locallibrary {id} {}", str_of(&x["name"])),
    };
    // With the menus that offer each.
    let offered = |x: &Value| {
        let menus: Vec<String> = x["where"].as_array().unwrap().iter().map(str_of).collect();
        format!("{} (menus: {})", str_of(&x["title"]), menus.join(", "))
    };
    section("Commands:".into(), list("commands").iter().map(|x| (invocation(x), offered(x))).collect());
    let operations = list("operations").iter().map(|x| (invocation(x), offered(x))).collect();
    section("Operations (planned, then confirmed):".into(), operations);
    let analyzers = list("analyzers")
        .iter()
        .map(|a| {
            let fields: Vec<&String> = a["fields"].as_object().unwrap().keys().collect();
            let fields: Vec<&str> = fields.iter().map(|f| f.as_str()).collect();
            (str_of(&a["name"]), format!("{}: {}", str_of(&a["title"]), fields.join(", ")))
        })
        .collect();
    section(format!("Analyzers (locallibrary analyze --plugin {id}):"), analyzers);
    let checks = list("checks")
        .iter()
        .map(|c| {
            let when: Vec<String> = c["when"].as_array().unwrap().iter().map(str_of).collect();
            (str_of(&c["name"]), format!("{} (when: {})", str_of(&c["title"]), when.join(", ")))
        })
        .collect();
    section("Checks (locallibrary plugin checks):".into(), checks);
    let exporters = list("exporters")
        .iter()
        .map(|e| {
            let writes = e["extension"].as_str().map_or_else(|| "a folder".to_string(), |x| format!("a .{x} file"));
            (str_of(&e["format"]), format!("{}, {writes}", str_of(&e["title"])))
        })
        .collect();
    section("Export formats (locallibrary export LIBRARY --format FORMAT -o ...):".into(), exporters);
    let providers = list("providers")
        .iter()
        .map(|p| {
            let finds = match p["kind"].as_str().unwrap() {
                "artwork" => "cover art, for locallibrary art fetch",
                _ => "track details, for locallibrary track fetch",
            };
            (str_of(&p["name"]), format!("{}: {finds}", str_of(&p["title"])))
        })
        .collect();
    section("Providers:".into(), providers);
    let settings = list("settings")
        .iter()
        .map(|s| {
            let key = str_of(&s["key"]);
            let value = d["settings"].get(&key).map_or_else(|| "not set".into(), cell);
            (key, format!("{}: {value}", str_of(&s["title"])))
        })
        .collect();
    section(format!("Settings (locallibrary plugin settings {id}):"), settings);
    out
}

/// The plugins' ids, for a test case of `n`: none a command or a namespace of locallibrary's.
fn ids(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("plug-{}", ["a", "b", "c", "d"][i])).collect()
}

/// For any plugins, `plugin list` says in text what `--json` lists, each capability in plain
/// words and everything it adds; `plugin info` says what `--json` describes, each command and
/// operation as it's run, each setting's value or that it's not set; and both are what the
/// manifests say, the plugins locallibrary ships first.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn plugins_are_listed_and_described_in_text_as_in_json(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    let manifests: Vec<Value> = ids(n).iter().map(|id| manifest(&tc, id)).collect();
    let env = env_with(&manifests);

    let listed = env.json(&["plugin", "list"]);
    let listed = listed.as_array().unwrap();
    let ids: Vec<&str> = listed.iter().map(|p| p["id"].as_str().unwrap()).collect();
    let drawn: Vec<&str> = manifests.iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [&SHIPPED[..], &drawn[..]].concat());
    for (p, m) in listed[SHIPPED.len()..].iter().zip(&manifests) {
        let c = &m["contributes"];
        let names = |key: &str, field: &str| -> Vec<Value> {
            c[key].as_array().unwrap().iter().map(|x| x[field].clone()).collect()
        };
        let capabilities: Vec<&Value> =
            p["capabilities"].as_array().unwrap().iter().map(|g| &g["capability"]).collect();
        assert_eq!(capabilities, m["capabilities"].as_array().unwrap().iter().collect::<Vec<_>>());
        for (key, field) in [
            ("commands", "name"),
            ("operations", "name"),
            ("analyzers", "name"),
            ("checks", "name"),
            ("exporters", "format"),
            ("providers", "name"),
            ("settings", "key"),
        ] {
            assert_eq!(p[key].as_array().unwrap(), &names(key, field), "{key}");
        }
        assert_eq!(
            (&p["name"], &p["version"], &p["running"], &p["left_out"]),
            (&m["name"], &m["version"], &json!(false), &Value::Null)
        );
    }
    // Each capability is said in words of its own.
    let means: Vec<(&Value, &Value)> = listed
        .iter()
        .flat_map(|p| p["capabilities"].as_array().unwrap())
        .map(|g| (&g["capability"], &g["means"]))
        .collect();
    for (c, m) in &means {
        assert!(m.as_str().is_some_and(|m| !m.is_empty()), "{c}");
        assert!(means.iter().all(|(c2, m2)| (c == c2) == (m == m2)), "{c} and another are said alike: {m}");
    }
    assert_eq!(env.ok(&["plugin", "list"]), listed_text(listed));
    assert_eq!(env.ok(&["plugin"]), listed_text(listed), "listing is what `plugin` does");

    for m in &manifests {
        let id = m["id"].as_str().unwrap();
        let described = unschema(env.json(&["plugin", "info", id]));
        let mut manifest = m.clone();
        // The manifest as it reads back: nothing left out is written.
        for key in ["commands", "operations", "analyzers", "checks", "exporters"] {
            for x in manifest["contributes"][key].as_array_mut().unwrap() {
                x.as_object_mut().unwrap().retain(|_, v| !v.is_null());
                if key == "operations" {
                    x["long_running"] = json!(false);
                }
                if ["commands", "operations"].contains(&key) && x.get("where").is_none() {
                    x["where"] = json!(["drive"]);
                }
                for (default, value) in [("default_on", json!(true)), ("when", json!(["check"]))] {
                    if key == "checks" && x.get(default).is_none() {
                        x[default] = value;
                    }
                }
                if key == "exporters" && x.get("folder").is_none() {
                    x["folder"] = json!(false);
                }
            }
        }
        assert_eq!(described["manifest"], manifest);
        let defaults: Map<String, Value> = m["contributes"]["settings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s.get("default").is_some())
            .map(|s| (s["key"].as_str().unwrap().to_string(), s["default"].clone()))
            .collect();
        assert_eq!(described["settings"], Value::Object(defaults));
        assert_eq!(described["capabilities"], listed.iter().find(|p| p["id"] == id).unwrap()["capabilities"]);
        assert_eq!(env.ok(&["plugin", "info", id]), described_text(&described));
        let shown: Vec<&Value> = m["contributes"]["settings"].as_array().unwrap().iter().collect();
        assert_eq!(env.ok(&["plugin", "settings", id]), settings_text(id, &shown, &described["settings"]));
    }
    let r = env.run(&["plugin", "info", "plug-z"]);
    assert_eq!((r.code, r.out.as_str()), (1, ""));
    assert!(r.err.contains("not found: plugin \"plug-z\""), "{}", r.err);
}

/// A setting's value as typed on the command line and what it reads back as, drawn for its kind.
#[hegel::test_helper]
fn typed_value(tc: &TestCase, s: &Value) -> (String, Value) {
    match s["type"].as_str().unwrap() {
        // A whole number reads back as one, and one with a point as written (`{f:?}` writes
        // 3.0 with its point).
        "number" => match tc.draw(gs::booleans()) {
            true => {
                let n = tc.draw(gs::integers::<i64>().min_value(-100_000).max_value(100_000));
                (format!(" {n}"), json!(n))
            }
            false => {
                let f = tc.draw(gs::floats::<f64>().min_value(-1e5).max_value(1e5).allow_nan(false));
                (format!(" {f:?}"), json!(f))
            }
        },
        "boolean" => {
            let (raw, b) = tc.draw(gs::sampled_from(
                &[("yes", true), ("On", true), ("true", true), ("no", false), ("OFF", false), ("false", false)][..],
            ));
            (raw.into(), json!(b))
        }
        "enum" => {
            let choices: Vec<String> =
                s["choices"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().into()).collect();
            let c = tc.draw(gs::sampled_from(choices));
            (c.clone(), json!(c))
        }
        "file" | "directory" => {
            let raw = tc.draw(gs::sampled_from(&["list.txt", "a/b", "/abs/olute"][..]));
            (raw.into(), json!(std::path::absolute(raw).unwrap().display().to_string()))
        }
        _ => {
            let raw = title(tc);
            (raw.clone(), json!(raw))
        }
    }
}

/// What `plugin settings` says of `settings` (as `--json` has them) for the manifest's
/// `shown` settings.
fn settings_text(plugin: &str, shown: &[&Value], settings: &Value) -> String {
    if shown.is_empty() {
        return format!("the {plugin} plugin has no settings\n");
    }
    let width = shown.iter().map(|s| s["key"].as_str().unwrap().len()).max().unwrap();
    shown
        .iter()
        .map(|s| {
            let key = s["key"].as_str().unwrap();
            let value = settings.get(key);
            let default = value.is_some() && value == s.get("default");
            let value = value.map_or_else(|| "not set".into(), cell);
            format!(
                "{}  {value}  {}{}\n",
                pad(key, width),
                s["title"].as_str().unwrap(),
                if default { " (default)" } else { "" }
            )
        })
        .collect()
}

/// For any settings and any sequence of setting, resetting and showing them: each reads back
/// (in text as in `--json`, and in what the plugin's commands are given) as last set with
/// `--yes`, else its default; nothing changes without `--yes`; and a value its kind can't hold,
/// one an enum doesn't have or a key the plugin has no setting for is refused naming it.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn settings_read_back_as_last_set(tc: TestCase) {
    let id = "tune";
    let mut m = manifest(&tc, id);
    let settings: Vec<Value> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)))
        .map(|j| setting(&tc, &format!("k{j}")))
        .collect();
    m["contributes"]["settings"] = json!(settings);
    // A command that answers what it's given, settings included.
    m["contributes"]["commands"] = json!([{ "name": "ask", "title": "Ask", "mode": "headless" }]);
    let env = env_with(&[m]);
    let mut set: Map<String, Value> = Map::new();
    let now = |set: &Map<String, Value>| -> Value {
        let now: Map<String, Value> = settings
            .iter()
            .filter_map(|s| {
                let key = s["key"].as_str().unwrap();
                Some((key.to_string(), set.get(key).or(s.get("default"))?.clone()))
            })
            .collect();
        Value::Object(now)
    };
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(2).max_value(7)) {
        let s = &settings[tc.draw(gs::integers::<usize>().max_value(settings.len() - 1))];
        let key = s["key"].as_str().unwrap();
        let kind = s["type"].as_str().unwrap();
        match tc.draw(gs::integers::<u8>().max_value(6)) {
            0 => {
                let (raw, value) = typed_value(&tc, s);
                let yes = tc.draw(gs::weighted_booleans(0.7));
                let args = [&["plugin", "settings", id, key][..], &["--", &raw]].concat();
                let change = format!("• set the {id} plugin's {key} to {}\n", cell(&value));
                match yes {
                    true => {
                        let out = env.ok(&[&["-y"], &args[..]].concat());
                        assert_eq!(out, format!("{change}✓ {key} is {}\n", cell(&value)));
                        set.insert(key.into(), value);
                    }
                    false => assert_eq!(env.exits(3, &args), change),
                }
            }
            1 => {
                let yes = tc.draw(gs::weighted_booleans(0.7));
                let args = ["plugin", "settings", id, key, "--reset"];
                let change = format!("• put the {id} plugin's {key} back to its default\n");
                match yes {
                    true => {
                        let out = env.ok(&[&["-y"], &args[..]].concat());
                        let now = s.get("default").map_or_else(|| "not set".into(), cell);
                        assert_eq!(out, format!("{change}✓ {key} is {now}\n"));
                        set.shift_remove(key);
                    }
                    false => assert_eq!(env.exits(3, &args), change),
                }
            }
            2 => {
                let (raw, says) = match kind {
                    "number" => ("many", format!("{key} is a number, not \"many\"")),
                    "boolean" => ("maybe", format!("{key} is true or false, not \"maybe\"")),
                    "enum" => {
                        let choices: Vec<&str> =
                            s["choices"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
                        ("NOPE", format!("{key} is one of {}, not \"NOPE\"", choices.join(", ")))
                    }
                    _ => continue,
                };
                let r = env.run(&["-y", "plugin", "settings", id, key, raw]);
                assert_eq!(r.code, if kind == "enum" { 1 } else { 2 }, "{}", r.err);
                assert!(r.err.contains(&says), "{}", r.err);
            }
            3 => {
                let r = env.run(&["-y", "plugin", "settings", id, "nope", "x"]);
                assert_eq!(r.code, 2);
                let says =
                    format!("the {id} plugin has no setting \"nope\" (`locallibrary plugin settings {id}` lists them)");
                assert!(r.err.contains(&says), "{}", r.err);
            }
            4 => {
                let one = tc.draw(gs::booleans());
                let mut args = vec!["plugin", "settings", id];
                if one {
                    args.push(key);
                }
                let shown: Vec<&Value> = settings.iter().filter(|s| !one || s["key"] == key).collect();
                let json = unschema(env.json(&args));
                let mut expected = now(&set);
                if one {
                    expected.as_object_mut().unwrap().retain(|k, _| k == key);
                }
                assert_eq!(json, json!({ "plugin": id, "settings": expected, "secrets": {} }));
                assert_eq!(env.ok(&args), settings_text(id, &shown, &json["settings"]));
            }
            _ => {
                let given = env.json(&["tune", "ask", "--params", r#"{"context": true}"#]);
                assert_eq!(
                    given.get("settings").cloned().unwrap_or(json!({})),
                    now(&set),
                    "the settings a command is given"
                );
            }
        }
    }
}

/// A flag and the value typed for it: `--flag=-5` for one that starts as a flag would.
fn flagged(flag: &str, raw: &str) -> Vec<String> {
    match raw.starts_with('-') {
        true => vec![format!("--{flag}={raw}")],
        false => vec![format!("--{flag}"), raw.to_string()],
    }
}

fn strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// Wherever a plugin's command is (its own place, one in a group, or under the plugin's id),
/// the flags typed for it give the plugin exactly the params they say, shown as the plugin
/// answers them (text as it is, nothing as nothing, anything else as JSON, in `--json` as
/// JSON); a value its type can't hold, or `--params` that aren't an object, are refused
/// naming them. Its operation, wherever it is, is planned from them, asked about and applied.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_commands_flags_give_it_the_params_typed(tc: TestCase) {
    let id = "flags";
    let place = |tc: &TestCase, name: &str| match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => Some(format!("{id}-{name}")),
        1 => Some(format!("playlist {id}-{name}")),
        _ => None,
    };
    let renamed = tc.draw(gs::booleans());
    let mut ratio = json!({ "type": "number" });
    if renamed {
        ratio["x-cli"] = json!({ "flag": "by" });
    }
    let schema = json!({
        "type": "object",
        "properties": {
            "target": { "type": "string", "x-cli": { "positional": true } },
            "name": { "type": "string" },
            "times": { "type": "integer" },
            "ratio": ratio,
            "loud": { "type": "boolean" },
            "tags": { "type": "array", "items": { "type": "string" } },
            "file": { "type": "string", "x-path": true },
        },
    });
    let (command_at, op_at) = (place(&tc, "say"), place(&tc, "make"));
    let mut command = json!({ "name": "echo", "title": "Say", "mode": "headless", "params": schema });
    let mut op = json!({ "name": "make", "title": "Make" });
    if let Some(p) = &command_at {
        command["cli"] = json!(p);
    }
    if let Some(p) = &op_at {
        op["cli"] = json!(p);
    }
    let m = json!({
        "id": id, "name": "Flags", "version": "0.1.0", "protocol": "^0.2",
        "capabilities": ["library:read", "library:plan"],
        "contributes": { "commands": [command], "operations": [op] },
    });
    let env = env_with(&[m]);
    env.demo("USB");
    let at = |place: &Option<String>, name: &str| -> Vec<String> {
        match place {
            Some(p) => p.split(' ').map(String::from).collect(),
            None => vec![id.into(), name.into()],
        }
    };

    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let mut args = at(&command_at, "echo");
        let mut expected = Map::new();
        if tc.draw(gs::booleans()) {
            let target = tc.draw(gs::from_regex("[a-z][a-z ]{0,5}"));
            args.extend(["USB".into(), target.clone()]);
            expected.insert("target".into(), json!(target.trim()));
        }
        if tc.draw(gs::booleans()) {
            let name = title(&tc);
            args.extend([format!("--name={name}")]);
            expected.insert("name".into(), json!(name.trim()));
        }
        if tc.draw(gs::booleans()) {
            let n = tc.draw(gs::integers::<i64>().min_value(-99).max_value(99));
            args.extend(flagged("times", &format!("#{n}")));
            expected.insert("times".into(), json!(n));
        }
        if tc.draw(gs::booleans()) {
            let f = tc.draw(gs::floats::<f64>().min_value(-1e3).max_value(1e3).allow_nan(false));
            // With its point (`{f:?}`): a whole number reads back as one, not as this float.
            args.extend(flagged(if renamed { "by" } else { "ratio" }, &format!("{f:?}")));
            expected.insert("ratio".into(), json!(f));
        }
        if tc.draw(gs::booleans()) {
            args.push("--loud".into());
            expected.insert("loud".into(), json!(true));
        }
        let tags: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[a-z]{1,4}")).max_size(3));
        if !tags.is_empty() {
            args.extend(tags.iter().flat_map(|t| ["--tags".to_string(), t.clone()]));
            expected.insert("tags".into(), json!(tags));
        }
        if tc.draw(gs::booleans()) {
            args.extend(["--file".into(), "list.txt".into()]);
            expected.insert("file".into(), json!(path(&std::env::current_dir().unwrap().join("list.txt"))));
        }
        // Flags are read in the schema's order: the expected params are in it too.
        let order = ["target", "name", "times", "ratio", "loud", "tags", "file"];
        let expected: Map<String, Value> =
            order.iter().filter_map(|k| Some((k.to_string(), expected.get(*k)?.clone()))).collect();
        let expected = Value::Object(expected);
        let got = env.json(&strs(&args));
        assert_eq!(unschema(got), expected, "{args:?}");
        let text = env.ok(&strs(&args));
        assert_eq!(text, format!("{}\n", serde_json::to_string_pretty(&expected).unwrap()));

        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => {
                let mut wrong = at(&command_at, "echo");
                wrong.extend(["--times".into(), "3.5".into()]);
                let err = env.fails(2, &strs(&wrong));
                assert!(err.contains("--times takes a whole number, not \"3.5\""), "{err}");
            }
            1 => {
                let mut wrong = at(&command_at, "echo");
                wrong.extend(["--params".into(), "[1]".into()]);
                assert!(env.fails(2, &strs(&wrong)).contains("--params takes a JSON object, not [1]"));
            }
            _ => {
                // What the plugin answers is shown as it is: text, or nothing.
                let says = tc.draw(gs::optional(gs::from_regex("[a-z]{1,8}( [a-z]{1,8})?")));
                let mut asking = at(&command_at, "echo");
                asking.extend(["--params".into(), r#"{"env": "PLUGIN_SAYS"}"#.into()]);
                let mut c = env.command();
                c.env_remove("PLUGIN_SAYS");
                if let Some(s) = &says {
                    c.env("PLUGIN_SAYS", s);
                }
                let out = c.args(&asking).output().unwrap();
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
                let text = String::from_utf8(out.stdout).unwrap();
                assert_eq!(text, says.map(|s| format!("{s}\n")).unwrap_or_default());
            }
        }
    }

    // The operation: planned from its params, not applied without a yes, then applied.
    let name = tc.draw(gs::sampled_from(&["Fresh", "Set 2", "Ünï"][..]));
    let edits = json!({ "edits": [{ "op": "create_playlist", "path": name }] }).to_string();
    let mut args = at(&op_at, "make");
    if tc.draw(gs::booleans()) {
        args.push("USB".into());
    }
    args.extend(["--params".into(), edits]);
    let planned = env.ok(&[&strs(&args)[..], &["--dry-run"]].concat());
    assert!(planned.starts_with("as given\n"), "{planned}");
    let before = Env::files_in(&env.volumes);
    env.exits(3, &strs(&args));
    assert_eq!(Env::files_in(&env.volumes), before, "nothing written without a yes");
    let applied = env.json(&[&["--yes"], &strs(&args)[..]].concat());
    assert_eq!((&applied["applied"], &applied["plan"]["operation"]), (&json!(true), &json!(format!("{id}.make"))));
    assert!(env.ok(&["playlist", "ls", "USB"]).contains(name));
}

/// A view a plugin's command could answer, of any kind, titled or not: cells of every type, some
/// wider than a column is shown, rows short or long of the columns.
#[hegel::test_helper]
fn view(tc: &TestCase) -> Value {
    let text = |tc: &TestCase, max: usize| tc.draw(gs::text().alphabet("abc XYZé.-").max_size(max));
    let scalar = |tc: &TestCase| -> Value {
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => match tc.draw(gs::weighted_booleans(0.4)) {
                // Wider than a column is shown.
                true => json!(tc.draw(gs::text().alphabet("abc XYZé.-").min_size(49).max_size(60))),
                false => json!(text(tc, 12)),
            },
            1 => json!(tc.draw(gs::integers::<i64>().min_value(-9999).max_value(9999))),
            2 => json!(tc.draw(gs::floats::<f64>().min_value(-1e3).max_value(1e3).allow_nan(false))),
            3 => json!(tc.draw(gs::booleans())),
            _ => Value::Null,
        }
    };
    let mut v = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {
            let items: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(4)))
                .map(|_| {
                    let mut item = json!({ "label": text(tc, 12) });
                    if tc.draw(gs::booleans()) {
                        item["detail"] = json!(text(tc, 12));
                    }
                    item
                })
                .collect();
            json!({ "kind": "list", "items": items })
        }
        1 => {
            let columns: Vec<String> =
                (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3))).map(|_| text(tc, 10)).collect();
            let rows: Vec<Vec<Value>> = (0..tc.draw(gs::integers::<usize>().max_value(4)))
                .map(|_| {
                    let n = match tc.draw(gs::weighted_booleans(0.8)) {
                        true => columns.len(),
                        false => tc.draw(gs::integers::<usize>().max_value(columns.len() + 1)),
                    };
                    let mut row: Vec<Value> = (0..n).map(|_| scalar(tc)).collect();
                    // Now and then a cell wider than its column can be.
                    if let Some(c) = row.first_mut().filter(|_| tc.draw(gs::weighted_booleans(0.3))) {
                        *c = json!(tc.draw(gs::text().alphabet("xyz é").min_size(49).max_size(70)));
                    }
                    row
                })
                .collect();
            json!({ "kind": "table", "columns": columns, "rows": rows })
        }
        2 => {
            let pairs: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(4)))
                .map(|_| {
                    let value = match tc.draw(gs::weighted_booleans(0.2)) {
                        true => json!(tc.draw(gs::vecs(gs::from_regex("[a-z]{0,4}")).max_size(3))),
                        false => scalar(tc),
                    };
                    json!({ "label": text(tc, 10), "value": value })
                })
                .collect();
            json!({ "kind": "kv", "pairs": pairs })
        }
        _ => json!({ "kind": "text", "text": tc.draw(gs::text().alphabet("ab \n").max_size(20)) }),
    };
    if tc.draw(gs::booleans()) {
        v["title"] = json!(text(tc, 12));
    }
    v
}

/// What the command line shows of `view`: its title, then its lines drawn as the CLI's own lists
/// and tables are (a table's columns as wide as their widest cell, at most 48, cut short with an
/// ellipsis past that, two spaces apart).
fn view_text(v: &Value) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(title) = v["title"].as_str() {
        lines.push(title.into());
    }
    let fit = |s: &str, width: usize| -> String {
        match s.chars().count() <= width {
            true => pad(s, width),
            false => format!("{}…", s.chars().take(width - 1).collect::<String>()),
        }
    };
    let str_of = |v: &Value| v.as_str().unwrap().to_string();
    match v["kind"].as_str().unwrap() {
        "list" => {
            for i in v["items"].as_array().unwrap() {
                lines.push(match i["detail"].as_str() {
                    Some(d) => format!("{}  {d}", str_of(&i["label"])),
                    None => str_of(&i["label"]),
                });
            }
        }
        "table" => {
            let columns: Vec<String> = v["columns"].as_array().unwrap().iter().map(str_of).collect();
            let rows: Vec<Vec<String>> = v["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_array().unwrap().iter().map(cell).collect())
                .collect();
            let widths: Vec<usize> = (0..columns.len())
                .map(|i| {
                    let widest = rows.iter().filter_map(|r| r.get(i)).map(|c| c.chars().count()).max().unwrap_or(0);
                    widest.max(columns[i].chars().count()).min(48)
                })
                .collect();
            for row in std::iter::once(&columns).chain(&rows) {
                let cells: Vec<String> = row.iter().zip(&widths).map(|(c, w)| fit(c, *w)).collect();
                lines.push(cells.join("  ").trim_end().to_string());
            }
        }
        "kv" => {
            let pairs = v["pairs"].as_array().unwrap();
            let width = pairs.iter().map(|p| p["label"].as_str().unwrap().chars().count()).max().unwrap_or(0);
            for p in pairs {
                lines.push(format!("{}  {}", pad(p["label"].as_str().unwrap(), width), cell(&p["value"])));
            }
        }
        _ => lines.push(str_of(&v["text"])),
    }
    lines.iter().map(|l| format!("{l}\n")).collect()
}

/// Any view a plugin's command answers reads back in `--json` as it was answered, and in text
/// as the command line draws its own lists, tables, labelled values and text.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn views_read_in_text_as_in_json(tc: TestCase) {
    // The test plugin's `echo` answers its params: here, a view.
    let m = json!({
        "id": "views", "name": "Views", "version": "0.1.0", "protocol": "^0.2",
        "contributes": { "commands": [{ "name": "echo", "title": "Show", "mode": "view" }] },
    });
    let env = env_with(&[m]);
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let v = view(&tc);
        let args = ["views", "echo", "--params", &v.to_string()];
        assert_eq!(unschema(env.json(&args)), v);
        assert_eq!(env.ok(&args), view_text(&v), "{v}");
    }
    // Anything else is the plugin's mistake.
    let r = env.run(&["views", "echo", "--params", r#"{"kind": "chart"}"#]);
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.err.contains("its view of echo isn't one"), "{}", r.err);
}

/// What one of a drive's tracks' audio file is now.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Audio {
    /// This many bytes, written as the test's `n`th version of a file.
    Bytes(u64, usize),
    Gone,
}

/// The `n`th file the test writes: as long as no other it writes.
fn audio_of(n: usize) -> String {
    "x".repeat(1000 + n)
}

/// For any sequence of analyses (of every analyzer or one, of every track or some) and of
/// changes to the tracks' audio (emptied, rewritten, removed): each analysis works out what
/// it hasn't already for a file as it is now, fails on a file that's gone (and `order` on an
/// empty one), and says so in text as in `--json`, exiting as some or all failing does; and
/// `track` shows what was worked out for each file as it is now, in text as in `--json`. An
/// analyzer or plugin `analyze` hasn't got is refused.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn analyzers_work_out_each_file_once(tc: TestCase) {
    let id = "meter";
    let size = json!({ "name": "size", "title": "File size", "version": "1", "fields": { "bytes": "integer" } });
    let fields = json!({ "a": "integer", "b": "integer" });
    let order = json!({ "name": "order", "title": "Order", "version": "1", "fields": fields });
    let analyzers = match tc.draw(gs::booleans()) {
        true => vec![size, order],
        false => vec![order, size],
    };
    let names: Vec<String> = analyzers.iter().map(|a| a["name"].as_str().unwrap().to_string()).collect();
    let m = json!({
        "id": id, "name": "Meter", "version": "0.2.0", "protocol": "^0.2",
        "contributes": { "analyzers": analyzers },
    });
    let env = env_with(&[m]);
    let root = env.demo("USB");
    let listed = env.json(&["tracks", "USB"]);
    // Each track's id, its audio file, and its path on the drive.
    let tracks: Vec<(u64, std::path::PathBuf, String)> = listed["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            let on_drive = t["file_path"].as_str().unwrap();
            (t["id"].as_u64().unwrap(), root.join(on_drive.trim_start_matches('/')), on_drive.to_string())
        })
        .collect();
    let mut audio: Vec<Audio> =
        tracks.iter().map(|(_, p, _)| Audio::Bytes(std::fs::metadata(p).unwrap().len(), 0)).collect();
    // The version of each file each analyzer worked out fields for, by analyzer then track.
    let mut kept: Vec<Vec<Option<Audio>>> = names.iter().map(|_| vec![None; tracks.len()]).collect();
    let mut written = 0;
    let mut emptied = vec![false; tracks.len()];

    for _ in 0..tc.draw(gs::integers::<usize>().min_value(2).max_value(7)) {
        match tc.draw(gs::integers::<u8>().max_value(7)) {
            0..=2 => {
                let only = tc.draw(gs::optional(gs::sampled_from(names.clone())));
                let chosen: Vec<usize> = (0..tracks.len()).filter(|_| tc.draw(gs::booleans())).collect();
                let mut args: Vec<String> = vec!["analyze".into(), "USB".into()];
                match (&only, tc.draw(gs::booleans())) {
                    (Some(a), true) => args.extend(["--analyzer".into(), a.clone()]),
                    (Some(a), false) => args.extend(["--plugin".into(), id.into(), "--analyzer".into(), a.clone()]),
                    (None, _) => args.extend(["--plugin".into(), id.into()]),
                }
                args.extend(chosen.iter().flat_map(|&t| ["--track".to_string(), tracks[t].0.to_string()]));
                let chosen: Vec<usize> = match chosen.is_empty() {
                    true => (0..tracks.len()).collect(),
                    false => chosen,
                };
                let mut report = Vec::new();
                // A warning for each track that failed, gone or (for `order`) empty.
                let mut warnings = Vec::new();
                for (a, name) in names.iter().enumerate().filter(|(_, n)| only.as_ref().is_none_or(|o| o == *n)) {
                    let (mut analyzed, mut already, mut failed) = (0, 0, 0);
                    for &t in &chosen {
                        match audio[t] {
                            Audio::Gone => {
                                failed += 1;
                                warnings.push(format!("! {}: its audio file isn't on the drive", tracks[t].2));
                            }
                            now if kept[a][t] == Some(now) => already += 1,
                            Audio::Bytes(0, _) if name == "order" => {
                                failed += 1;
                                warnings.push(format!("{} is empty", tracks[t].2));
                            }
                            now => {
                                analyzed += 1;
                                kept[a][t] = Some(now);
                            }
                        }
                    }
                    report.push(json!({ "plugin": id, "analyzer": name, "analyzed": analyzed, "kept": already, "failed": failed }));
                }
                let total = |key: &str| report.iter().map(|r| r[key].as_u64().unwrap()).sum::<u64>();
                let (failed, worked) = (total("failed"), total("analyzed") + total("kept"));
                let json = tc.draw(gs::booleans());
                let r = match json {
                    true => env.run(&strs(&[&["--json".to_string()], &args[..]].concat())),
                    false => env.run(&strs(&args)),
                };
                if failed > 0 && worked == 0 {
                    assert_eq!(r.code, 1, "{}", r.err);
                    // On stderr, or with `--json` as the error's JSON on stdout.
                    let said = if json { &r.out } else { &r.err };
                    assert!(said.contains(&format!("every track failed ({failed})")), "{said}");
                    continue;
                }
                assert_eq!(r.code, if failed > 0 { 5 } else { 0 }, "{args:?}\n{}", r.err);
                if json {
                    let got: Value = serde_json::from_str(&r.out).unwrap();
                    let outcome = if failed > 0 { "partial" } else { "applied" };
                    let expected = json!({ "plan": null, "report": { "drive": "USB", "analyzers": report }, "applied": true, "outcome": outcome });
                    assert_eq!(unschema(got), expected);
                } else {
                    let lines: String = report
                        .iter()
                        .map(|r| {
                            let n = r["analyzed"].as_u64().unwrap();
                            let line = format!(
                                "{} ({id}): {n} track{} analyzed, {} already",
                                r["analyzer"].as_str().unwrap(),
                                if n == 1 { "" } else { "s" },
                                r["kept"]
                            );
                            match r["failed"].as_u64().unwrap() {
                                0 => format!("✓ {line}\n"),
                                f => format!("! {line}, {f} failed\n"),
                            }
                        })
                        .collect();
                    // The analyzers' lines once they're done; the warnings on stderr as each track
                    // fails, beside the test switch's.
                    assert_eq!(r.out, lines);
                    let mut warned: Vec<&str> =
                        r.err.lines().filter(|l| l.starts_with("! ") && !l.contains("(a test switch)")).collect();
                    assert_eq!(warned.len(), warnings.len(), "{}", r.err);
                    for w in &warnings {
                        let at = warned.iter().position(|l| l.contains(w.trim_start_matches("! ")));
                        warned.remove(at.unwrap_or_else(|| panic!("no {w:?} in:\n{}", r.err)));
                    }
                }
            }
            3 => {
                let t = tc.draw(gs::integers::<usize>().max_value(tracks.len() - 1));
                written += 1;
                let now = match tc.draw(gs::booleans()) {
                    // Emptied once at most: no two versions of a file are alike.
                    true if !emptied[t] => {
                        emptied[t] = true;
                        std::fs::write(&tracks[t].1, "").unwrap();
                        Audio::Bytes(0, written)
                    }
                    _ => {
                        std::fs::write(&tracks[t].1, audio_of(written)).unwrap();
                        Audio::Bytes(audio_of(written).len() as u64, written)
                    }
                };
                audio[t] = now;
            }
            4 => {
                let t = tc.draw(gs::integers::<usize>().max_value(tracks.len() - 1));
                if audio[t] != Audio::Gone {
                    std::fs::remove_file(&tracks[t].1).unwrap();
                    audio[t] = Audio::Gone;
                }
            }
            5 => {
                let (args, says, code): (&[&str], String, i32) = match tc.draw(gs::integers::<u8>().max_value(3)) {
                    0 => (&["analyze", "USB", "--analyzer", "tempo"], "no plugin has an analyzer \"tempo\"".into(), 2),
                    1 => (
                        &["analyze", "USB", "--plugin", id, "--analyzer", "tempo"],
                        "no plugin has an analyzer \"tempo\"".into(),
                        2,
                    ),
                    2 => (&["analyze", "USB", "--plugin", "nope"], "not found: plugin \"nope\"".into(), 1),
                    _ => (&["analyze", "USB", "--plugin", "coverart"], "the coverart plugin has no analyzer".into(), 2),
                };
                assert!(env.fails(code, args).contains(&says));
            }
            _ => {
                let t = tc.draw(gs::integers::<usize>().max_value(tracks.len() - 1));
                let track = tracks[t].0.to_string();
                let mut fields = Map::new();
                for (a, name) in names.iter().enumerate() {
                    if kept[a][t].is_some() && kept[a][t] == Some(audio[t]) {
                        let values = match name.as_str() {
                            "size" => {
                                json!({ "bytes": match audio[t] { Audio::Bytes(n, _) => n, Audio::Gone => unreachable!() } })
                            }
                            _ => json!({ "a": 2, "b": 1 }),
                        };
                        fields.insert(name.clone(), values);
                    }
                }
                let shown = env.json(&["track", "USB", "--track", &track]);
                let got = shown[0]["fields"][id].as_object().cloned().unwrap_or_default();
                // In the order the manifest declares them, whichever ran first.
                assert_eq!(got.keys().collect::<Vec<_>>(), fields.keys().collect::<Vec<_>>(), "{shown}");
                assert_eq!(Value::Object(got), Value::Object(fields.clone()));
                let text = env.ok(&["track", "USB", "--track", &track]);
                for values in fields.values() {
                    for (name, v) in values.as_object().unwrap() {
                        let line = format!("  {:<11} {v}\n", format!("{id}.{name}"));
                        assert!(text.contains(&line), "{line:?} in:\n{text}");
                    }
                }
                if fields.is_empty() {
                    assert!(!text.contains(&format!("{id}.")), "{text}");
                }
            }
        }
    }
}

/// A plugin whose id or command clashes with locallibrary's commands, the sidecar's namespaces
/// or locallibrary's formats is left out, the command line saying why as it starts and the
/// sidecar listing it with why (as the contract says), while a plugin beside it works.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_plugin_that_clashes_is_left_out_saying_why(tc: TestCase) {
    let plain = |id: &str| json!({ "id": id, "name": "P", "version": "0.1.0", "protocol": "^0.2" });
    let (clashing, why) = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {
            let ns = tc.draw(gs::sampled_from(&["events", "tracks", "playlists", "drive"][..]));
            (
                plain(ns),
                format!(
                    "the {ns} plugin's id is the sidecar's {ns}.* methods' too: its plans would be named like them"
                ),
            )
        }
        1 => {
            let c = tc.draw(gs::sampled_from(&["eject", "info"][..]));
            (plain(c), format!("the {c} plugin's id is a locallibrary command already"))
        }
        2 => {
            let place = tc.draw(gs::sampled_from(&["export", "playlist new", "playlist ls"][..]));
            let mut m = plain("placed");
            m["contributes"] =
                json!({ "commands": [{ "name": "go", "title": "Go", "mode": "headless", "cli": place }] });
            (m, format!("the placed plugin's command {place:?} is a locallibrary command already"))
        }
        _ => {
            let format = tc.draw(gs::sampled_from(&["csv", "m3u", "txt"][..]));
            let mut m = plain("exporting");
            m["contributes"] = json!({ "exporters": [{ "format": format, "title": "F", "extension": "txt" }] });
            (m, format!("the exporting plugin's export format {format:?} is a locallibrary format already"))
        }
    };
    let id = clashing["id"].as_str().unwrap().to_string();
    let mut kept = plain("kept");
    kept["contributes"] = json!({ "commands": [{ "name": "echo", "title": "Echo", "mode": "headless" }] });
    let manifests = match tc.draw(gs::booleans()) {
        true => vec![clashing, kept],
        false => vec![kept, clashing],
    };
    let env = env_with(&manifests);
    let r = env.run(&["--json", "plugin", "list"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.err.contains(&format!("the {id} plugin is left out: {why}\n")), "{}", r.err);
    let listed: Value = serde_json::from_str(&r.out).unwrap();
    let ids: Vec<&str> = listed.as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [&SHIPPED[..], &["kept"]].concat());
    assert_eq!(env.json(&["kept", "echo", "--params", r#"{"a": 1}"#])["a"], 1);

    let mut s = Rpc::start(env.command());
    let m = method("plugins.list");
    let answer = s.call(1, "plugins.list", Value::Null);
    let listed = &answer["result"];
    assert_eq!(violation(listed, &format!("{}/result", at(m))), None, "{answer}");
    let left_out: Vec<(&str, &Value)> =
        listed.as_array().unwrap().iter().map(|p| (p["id"].as_str().unwrap(), &p["left_out"])).collect();
    let mut expected: Vec<(&str, Value)> = SHIPPED.iter().map(|id| (*id, Value::Null)).collect();
    expected.extend([("kept", Value::Null), (id.as_str(), json!(why))]);
    assert_eq!(left_out, expected.iter().map(|(id, w)| (*id, w)).collect::<Vec<_>>());
    drop(s);

    // A manifest that can't be read leaves no plugin to run, saying which.
    let missing = env.outside("missing.toml");
    let out = env.command().env("LOCALLIBRARY_TEST_PLUGINS", &missing).args(["plugin", "list"]).output().unwrap();
    let err = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains(&format!("the test plugin {}: ", missing.display())), "{err}");
}

/// The contract's method `name`.
fn method(name: &str) -> &'static Value {
    contract()["methods"].as_array().unwrap().iter().find(|m| m["name"] == name).unwrap_or_else(|| panic!("{name}"))
}

/// The sidecar's answer to `method` with `params`: its result, which must fit the contract, or
/// its error's data, which must fit the contract's errors.
fn answer(s: &mut Rpc, id: &mut i64, name: &str, params: Value) -> Result<Value, Value> {
    *id += 1;
    let a = s.call(*id, name, params);
    match (a.get("result"), a.get("error")) {
        (Some(r), None) => {
            assert_eq!(violation(r, &format!("{}/result", at(method(name)))), None, "{name}: {a}");
            Ok(r.clone())
        }
        (None, Some(e)) => {
            assert_eq!(violation(&e["data"], "/error"), None, "{name}: {a}");
            Err(e["data"].clone())
        }
        _ => panic!("{name}: {a}"),
    }
}

/// A setting's value as JSON (as a GUI sends it) drawn for its kind, and one of a type it can't
/// hold.
#[hegel::test_helper]
fn json_value(tc: &TestCase, s: &Value) -> (Value, Value) {
    match s["type"].as_str().unwrap() {
        "number" => (json!(tc.draw(gs::integers::<i64>().min_value(-99).max_value(99))), json!("many")),
        "boolean" => (json!(tc.draw(gs::booleans())), json!(1)),
        "enum" => {
            let choices: Vec<String> =
                s["choices"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().into()).collect();
            (json!(tc.draw(gs::sampled_from(choices))), json!("NOPE"))
        }
        _ => (json!(title(tc)), json!(5)),
    }
}

/// For any plugins, the sidecar's `plugins.*` methods answer as the contract says and as the
/// command line does: `plugins.list` and `plugins.describe` what `plugin list` and `plugin info`
/// say; settings set (or refused) and reset read back the same through both, and are what the
/// plugin's commands are given, with where the user is; an operation and an export are planned
/// and applied; and an analysis is read back by `track.fields` as `track` shows it.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn the_sidecar_hosts_plugins_as_the_command_line_does(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
    let mut manifests: Vec<Value> = ids(n).iter().map(|id| manifest(&tc, id)).collect();
    let id = manifests[0]["id"].as_str().unwrap().to_string();
    // What the test plugin can run, beside what was drawn.
    let c = &mut manifests[0]["contributes"];
    let push = |list: &mut Value, v: Value| list.as_array_mut().unwrap().push(v);
    push(&mut c["commands"], json!({ "name": "ask", "title": "Ask", "mode": "headless" }));
    push(&mut c["operations"], json!({ "name": "make", "title": "Make" }));
    push(&mut c["exporters"], json!({ "format": format!("{id}-out"), "title": "Out", "extension": "m3u" }));
    if !c["analyzers"].as_array().unwrap().iter().any(|a| a["name"] == "size") {
        let size = json!({ "name": "size", "title": "File size", "version": "1", "fields": { "bytes": "integer" } });
        push(&mut c["analyzers"], size);
    }
    if c["settings"].as_array().unwrap().is_empty() {
        push(&mut c["settings"], setting(&tc, "k0"));
    }
    let settings = c["settings"].as_array().unwrap().clone();
    // Its operation plans edits and its export reads the drive's tracks: each needs a capability.
    let caps = &mut manifests[0]["capabilities"];
    for cap in ["library:read", "library:plan"] {
        if !caps.as_array().unwrap().contains(&json!(cap)) {
            push(caps, json!(cap));
        }
    }
    let env = env_with(&manifests);
    let root = env.demo("USB");
    let drive = path(&root).to_string();
    let listed = env.json(&["plugin", "list"]);
    let described: Vec<Value> =
        manifests.iter().map(|m| unschema(env.json(&["plugin", "info", m["id"].as_str().unwrap()]))).collect();

    let mut s = Rpc::start(env.command());
    let call = &mut 0;
    assert_eq!(answer(&mut s, call, "plugins.list", Value::Null).unwrap(), listed);
    for (m, d) in manifests.iter().zip(&described) {
        assert_eq!(&answer(&mut s, call, "plugins.describe", json!({ "plugin": m["id"] })).unwrap(), d);
    }
    let e = answer(&mut s, call, "plugins.describe", json!({ "plugin": "plug-z" })).unwrap_err();
    assert_eq!(e["kind"], "not_found");

    // Settings, set and reset through the sidecar, against a model.
    let mut set: Map<String, Value> = Map::new();
    let now = |set: &Map<String, Value>| -> Map<String, Value> {
        settings
            .iter()
            .filter_map(|s| {
                let key = s["key"].as_str().unwrap();
                Some((key.to_string(), set.get(key).or(s.get("default"))?.clone()))
            })
            .collect()
    };
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        let s0 = &settings[tc.draw(gs::integers::<usize>().max_value(settings.len() - 1))];
        let key = s0["key"].as_str().unwrap();
        let (good, bad) = json_value(&tc, s0);
        let (value, refused) = match tc.draw(gs::integers::<u8>().max_value(2)) {
            // Put back to its default.
            0 => (Value::Null, None),
            1 => (bad, Some("invalid")),
            _ => (good, None),
        };
        let unknown = tc.draw(gs::weighted_booleans(0.15));
        let key = if unknown { "nope" } else { key };
        let got = answer(&mut s, call, "plugins.settings.set", json!({ "plugin": id, "key": key, "value": value }));
        match (unknown, refused) {
            (true, _) => assert_eq!(got.unwrap_err()["kind"], "not_found"),
            (false, Some(kind)) => assert_eq!(got.unwrap_err()["kind"], kind),
            (false, None) => {
                match value.is_null() {
                    true => set.shift_remove(key),
                    false => set.insert(key.into(), value),
                };
                assert_eq!(got.unwrap(), json!({ "plugin": id, "settings": now(&set), "secrets": {} }));
            }
        }
    }
    let settings_now = json!({ "plugin": id, "settings": now(&set), "secrets": {} });
    assert_eq!(answer(&mut s, call, "plugins.settings", json!({ "plugin": id })).unwrap(), settings_now);
    assert_eq!(unschema(env.json(&["plugin", "settings", &id])), settings_now);

    // A command is given where the user is, and the settings.
    let mut context = json!({});
    let mut given = Map::new();
    if tc.draw(gs::booleans()) {
        context["drive"] = json!(tc.draw(gs::sampled_from(vec!["USB".to_string(), drive.clone()])));
        given.insert("drive".into(), json!("USB"));
    }
    if tc.draw(gs::booleans()) {
        context["playlist"] = json!("Sets/Friday");
        given.insert("playlist".into(), json!("Sets/Friday"));
    }
    let tracks: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(9)).max_size(3));
    context["tracks"] = json!(tracks);
    if !tracks.is_empty() {
        given.insert("tracks".into(), json!(tracks));
    }
    if !now(&set).is_empty() {
        given.insert("settings".into(), Value::Object(now(&set)));
    }
    let ran = json!({ "plugin": id, "command": "ask", "params": { "context": true }, "context": context });
    let mut got = answer(&mut s, call, "plugins.run", ran).unwrap();
    // And a scratch folder of its own.
    assert!(got.as_object_mut().unwrap().shift_remove("scratch").is_some_and(|s| s.is_string()));
    assert_eq!(got, Value::Object(given));
    let e = answer(&mut s, call, "plugins.run", json!({ "plugin": id, "command": "nope" })).unwrap_err();
    assert_eq!(e["kind"], "not_found");

    // An operation, planned then applied, makes its playlist.
    let name = tc.draw(gs::sampled_from(&["Fresh", "Ünï", "Set 2"][..]));
    let edits = json!([{ "op": "create_playlist", "path": name }]);
    let op = json!({ "plugin": id, "operation": "make", "params": { "edits": edits }, "context": { "drive": drive } });
    let plan = answer(&mut s, call, "plugins.op.plan", op).unwrap();
    assert_eq!(plan["operation"], format!("{id}.make"));
    let applied = answer(&mut s, call, "plugins.op.apply", json!({ "plan_id": plan["plan_id"] })).unwrap();
    assert_eq!(applied["changes"], 1, "{applied}");
    let playlists = env.ok(&["playlist", "ls", "USB"]);
    assert!(playlists.contains(name), "{playlists}");

    // An export, planned then applied, writes the file it said it would.
    let out = env.outside("out.m3u");
    let export = json!({ "plugin": id, "exporter": format!("{id}-out"), "drive": drive, "output": path(&out) });
    let plan = answer(&mut s, call, "plugins.export.plan", export).unwrap();
    assert_eq!(plan["files"][0]["path"], path(&out));
    assert!(!out.exists(), "a plan writes nothing");
    let written = answer(&mut s, call, "plugins.export.apply", json!({ "plan_id": plan["plan_id"] })).unwrap();
    assert_eq!(written["files"][0]["bytes"], std::fs::metadata(&out).unwrap().len());

    // An analysis of some tracks, read back as `track` shows it.
    let chosen: Vec<u64> = [1, 2].into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    let mut analyze = json!({ "plugin": id, "analyzer": "size", "drive": drive });
    if !chosen.is_empty() {
        analyze["tracks"] = json!(chosen);
    }
    let done = answer(&mut s, call, "plugins.analyze", analyze).unwrap();
    let analyzed: Vec<u64> = if chosen.is_empty() { vec![1, 2] } else { chosen };
    assert_eq!(done, json!({ "analyzed": analyzed.len(), "kept": 0, "failed": 0 }));
    let fields = answer(&mut s, call, "track.fields", json!({ "drive": drive, "tracks": [1, 2] })).unwrap();
    for t in [1u64, 2] {
        // A track nothing was worked out for isn't there.
        let read = fields["tracks"].as_array().unwrap().iter().find(|r| r["track"] == t);
        let shown = env.json(&["track", "USB", "--track", &t.to_string()]);
        let bytes = read.map_or(&Value::Null, |r| &r["plugins"][&id]["size"]["bytes"]);
        match analyzed.contains(&t) {
            true => {
                let file = root.join(shown[0]["track"]["file_path"].as_str().unwrap().trim_start_matches('/'));
                assert_eq!(bytes, &json!(std::fs::metadata(file).unwrap().len()), "{fields}");
            }
            false => assert!(bytes.is_null(), "{fields}"),
        }
        assert_eq!(&shown[0]["fields"][&id]["size"]["bytes"], bytes, "{shown}");
    }
}
