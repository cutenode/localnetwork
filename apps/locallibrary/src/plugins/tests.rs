use serde_json::json;

use super::*;
use crate::export_cmd::FormatArg;

/// A built-in plugin's manifest with the id `id` and `contributes` (TOML) added.
fn manifest(id: &str, contributes: &str) -> Manifest {
    let m = Manifest::parse(&format!(
        "id = \"{id}\"\nname = \"Test\"\nversion = \"0.1.0\"\nprotocol = \"^0.2\"\n{contributes}"
    ))
    .unwrap();
    m.validate(true).unwrap();
    m
}

fn command(name: &str, more: &str) -> String {
    format!("[[contributes.commands]]\nname = \"{name}\"\ntitle = \"T\"\nmode = \"headless\"\n{more}\n")
}

fn operation(name: &str, more: &str) -> String {
    format!("[[contributes.operations]]\nname = \"{name}\"\ntitle = \"T\"\n{more}\n")
}

/// Why each of `manifests` is left out, if it is.
fn left_out(manifests: &[Manifest]) -> Vec<Option<String>> {
    check(&manifests.iter().collect::<Vec<_>>())
}

/// Why the one plugin, or each of the plugins clashing with each other, is left out.
fn refused(manifests: &[Manifest]) -> String {
    let why = left_out(manifests);
    assert!(why.iter().all(|w| w == &why[0]), "{why:?}");
    why[0].clone().unwrap()
}

/// The plugins locallibrary links in are never left out (a clash is a build's mistake, caught
/// here before it ships).
#[test]
fn the_plugins_locallibrary_ships_are_accepted() {
    let launch = Launch { program: "locallibrary".into(), args: Vec::new(), env_remove: Vec::new() };
    let specs: Vec<PluginSpec> =
        BUILT_IN.iter().map(|(m, _)| PluginSpec::built_in(m, launch.clone()).unwrap()).collect();
    assert_eq!(check(&specs.iter().map(|s| &s.manifest).collect::<Vec<_>>()), vec![None; BUILT_IN.len()]);
}

/// The manifests locallibrary links in are valid, their JSON Schemas included: a release build
/// doesn't check them as it starts (see `PluginSpec::built_in`), so this does, whatever the
/// build.
#[test]
fn the_manifests_locallibrary_ships_are_valid() {
    for (manifest, _) in BUILT_IN {
        let m = Manifest::parse(manifest).unwrap();
        m.validate(true).unwrap_or_else(|e| panic!("the {} plugin's manifest: {e}", m.id));
    }
}

/// Only account plugins ship without a crate to serve them, and every one does.
#[test]
fn only_account_plugins_ship_without_a_crate() {
    for (manifest, serve) in BUILT_IN {
        let m = Manifest::parse(manifest).unwrap();
        assert_eq!(serve.is_none(), m.is_account_plugin(), "the {} plugin", m.id);
    }
}

/// Each account plugin locallibrary ships that signs in through Local Network's token exchange
/// names a service the exchange has.
#[test]
fn the_account_plugins_locallibrary_ships_use_services_the_token_exchange_has() {
    let file =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/src/lib/server/token-exchange/services.json");
    let services: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let manifests = BUILT_IN.iter().map(|(m, _)| Manifest::parse(m).unwrap());
    let exchanges: Vec<String> =
        manifests.flat_map(|m| m.contributes.account_types).filter_map(|t| t.token_exchange).collect();
    assert!(!exchanges.is_empty());
    for exchange in exchanges {
        let service = exchange.strip_prefix("https://localnetwork.computer/api/token-exchange/").unwrap_or(&exchange);
        assert!(services["services"].get(service).is_some(), "{exchange} isn't a service of the token exchange");
    }
}

/// Only the plugins that clash are left out, whatever order they come in: one clashing with
/// locallibrary (by a flag, after its command's place) clashes with no other plugin.
#[test]
fn only_plugins_that_clash_are_left_out() {
    let echo = manifest("echo", &command("say", "cli = \"say\""));
    let params = "params = { type = \"object\", properties = { json = { type = \"boolean\" } } }";
    let flagged = manifest("flagged", &command("say", &format!("cli = \"say\"\n{params}")));
    let why = "the flagged plugin's say has a param \"json\" for --json, a flag it has already".to_string();
    assert_eq!(left_out(&[flagged.clone(), echo.clone()]), [Some(why.clone()), None]);
    assert_eq!(left_out(&[echo.clone(), flagged]), [None, Some(why)]);
    let why = "the say plugin's id is the echo plugin's command already".to_string();
    let other = manifest("other", "");
    assert_eq!(left_out(&[echo, other, manifest("say", "")]), [Some(why.clone()), None, Some(why)]);
}

/// An account plugin `id` providing the type `kind`, its scopes (`read`, `write`) covering
/// requests to the hosts given.
fn account_plugin(id: &str, kind: &str, scopes: &[(&str, &str)]) -> Manifest {
    let scopes: String = scopes
        .iter()
        .map(|(scope, host)| {
            format!(
                "[contributes.account_types.scopes.{scope}]\ntitle = \"T\"\nrequests = [{{ method = \"GET\", url = \"https://{host}/*\" }}]\n"
            )
        })
        .collect();
    let at = "https://accounts.example";
    manifest(
        id,
        &format!(
            "[[contributes.account_types]]\ntype = \"{kind}\"\ntitle = \"T\"\nauthorize_url = \"{at}/authorize\"\n\
             token_url = \"{at}/token\"\nclient_id = \"c\"\nwhoami = {{ url = \"{at}/me\", subject = \"/sub\", name = \"/name\" }}\n{scopes}"
        ),
    )
}

/// For any account plugins providing `mock` (or a type named like an `account` command), listing
/// any of its scopes with requests to any host, and a plugin asking for any of those scopes,
/// reaching any of the hosts, in any order: an account plugin is left out exactly when its type
/// is named like an `account` command, and the plugin exactly when no account plugin kept
/// provides the type, none of those lists a scope it asks for, or one of them that does sends a
/// request of it to a host the plugin can't reach, saying which.
#[hegel::test(test_cases = 200)]
fn only_plugins_whose_accounts_can_be_used_are_kept(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let commands = crate::Cli::command().find_subcommand("account").map(|a| a.get_subcommands().count()).unwrap_or(0);
    let hosts = ["a.example", "b.example"];
    let mut providers = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let kind = tc.draw(gs::sampled_from(vec!["mock", "mock", "status"]));
        let scopes: Vec<(&str, &str)> = ["read", "write"]
            .into_iter()
            .filter(|_| tc.draw(gs::booleans()))
            .map(|s| (s, tc.draw(gs::sampled_from(hosts.to_vec()))))
            .collect();
        providers.push((format!("mock-{i}"), kind, scopes));
    }
    let needs: Vec<&str> = ["read", "write"].into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    let needs = if needs.is_empty() { vec!["read"] } else { needs };
    let reaches: Vec<&str> = hosts.into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    let capabilities: Vec<String> = ["c.example"].iter().chain(&reaches).map(|h| format!("net:{h}")).collect();
    let asker = manifest(
        "asker",
        &format!("capabilities = {capabilities:?}\n[[accounts]]\ntype = \"mock\"\nscopes = {needs:?}\nwhy = \"W\"\n"),
    );
    let mut all: Vec<Manifest> = providers.iter().map(|(id, kind, scopes)| account_plugin(id, kind, scopes)).collect();
    let at = tc.draw(gs::integers::<usize>().max_value(all.len()));
    all.insert(at, asker);

    // An `account` command: refused when locallibrary has `account status`.
    let clashes = |kind: &str| kind == "status" && commands > 0;
    let kept: Vec<_> = providers.iter().filter(|(_, kind, _)| *kind == "mock").collect();
    let ids: Vec<&str> = kept.iter().map(|(id, _, _)| id.as_str()).collect();
    let unmet = || {
        if kept.is_empty() {
            return Some("the asker plugin asks for mock accounts, which no account plugin provides".to_string());
        }
        for scope in &needs {
            let listed: Vec<&str> =
                kept.iter().flat_map(|(_, _, s)| s.iter().filter(|(n, _)| n == scope).map(|(_, h)| *h)).collect();
            if listed.is_empty() {
                let ids = ids.join(", ");
                return Some(format!(
                    "the asker plugin asks for mock's scope {scope:?}, which no account plugin providing mock lists ({ids})"
                ));
            }
            if let Some(host) = listed.into_iter().find(|h| !reaches.contains(h)) {
                return Some(format!(
                    "the asker plugin asks for mock's scope {scope:?}, whose requests go to {host}, which it has no net:{host} capability for"
                ));
            }
        }
        None
    };
    let mut expected: Vec<Option<String>> = providers
        .iter()
        .map(|(id, kind, _)| {
            clashes(kind)
                .then(|| format!("the {id} plugin's account type {kind:?} is named like the account {kind} command"))
        })
        .collect();
    expected.insert(at, unmet());
    assert_eq!(left_out(&all), expected);
}

/// Ids and places on the command line clash with locallibrary's commands, their aliases, the
/// sidecar's method namespaces and each other's.
#[test]
fn ids_and_places_that_clash_are_refused() {
    let echo = manifest("echo", &command("say", "cli = \"say\""));
    assert_eq!(check(&[&echo]), [None]);
    assert_eq!(refused(&[manifest("eject", "")]), "the eject plugin's id is a locallibrary command already");
    assert_eq!(refused(&[manifest("info", "")]), "the info plugin's id is a locallibrary command already");
    assert_eq!(
        refused(&[manifest("events", "")]),
        "the events plugin's id is the sidecar's events.* methods' too: its plans would be named like them"
    );
    assert_eq!(
        refused(&[echo.clone(), manifest("say", "")]),
        "the say plugin's id is the echo plugin's command already"
    );
    assert_eq!(refused(&[echo.clone(), echo.clone()]), "the echo plugin's id is the echo plugin already");
    let place = |p: &str| manifest("other", &operation("go", &format!("cli = \"{p}\"")));
    let taken = |p: &str| format!("the other plugin's command \"{p}\" is a locallibrary command already");
    for p in ["status", "playlist new", "playlist ls"] {
        assert_eq!(refused(&[place(p)]), taken(p));
    }
    for (p, group) in [("status go", "status"), ("nope go", "nope")] {
        let e = format!("the other plugin's command \"{p}\" is in \"{group}\", which isn't a group of commands");
        assert_eq!(refused(&[place(p)]), e);
    }
    assert_eq!(check(&[&place("playlist fresh"), &echo]), [None, None]);
    assert_eq!(
        refused(&[place("playlist fresh"), manifest("third", &command("x", "cli = \"playlist fresh\""))]),
        "the third plugin's command \"playlist fresh\" is the other plugin's command already"
    );
}

/// A plugin's commands and operations are one command line's commands, whose params mustn't be
/// named like the flags every such command has.
#[test]
fn commands_and_params_that_clash_are_refused() {
    let twice = "the p plugin has two commands called";
    let both = manifest("p", &format!("{}{}", command("go", ""), operation("go", "")));
    assert_eq!(refused(&[both]), format!("{twice} \"go\" (a command and an operation)"));
    assert_eq!(
        refused(&[manifest("p", &command("help", ""))]),
        format!("{twice} \"help\" (a command and an operation)")
    );
    let with = |property: &str| {
        let params = format!("params = {{ type = \"object\", properties = {{ {property} }} }}");
        manifest("p", &operation("go", &params))
    };
    assert_eq!(check(&[&with("name = { type = \"string\" }")]), [None]);
    for (property, flag) in [
        ("json = { type = \"boolean\" }", "json"),
        ("dry_run = { type = \"boolean\" }", "dry-run"),
        ("params = { type = \"string\" }", "params"),
        ("store = { type = \"string\" }", "store"),
        ("from = { type = \"string\", x-cli = { flag = \"yes\" } }", "yes"),
        ("a_b = { type = \"string\" }, c = { type = \"string\", x-cli = { flag = \"a-b\" } }", "a-b"),
    ] {
        let e = refused(&[with(property)]);
        assert!(e.ends_with(&format!("for --{flag}, a flag it has already")), "{property}: {e}");
    }
}

/// An export format is one `export --format` takes: never one locallibrary has (or its alias),
/// nor another plugin's.
#[test]
fn import_formats_and_set_formats_that_clash_are_refused() {
    let importer = |id: &str, format: &str| {
        manifest(
            id,
            &format!("[[contributes.importers]]\nformat = \"{format}\"\ntitle = \"T\"\nprovides = [\"tags\"]\n"),
        )
    };
    // An import format is named apart from export formats.
    assert_eq!(check(&[&importer("a", "csv")]), [None]);
    assert_eq!(
        refused(&[importer("a", "nml"), importer("b", "nml")]),
        "the b plugin's import format \"nml\" is the a plugin's already"
    );
    let set = |format: &str| {
        let exporter = format!(
            "capabilities = [\"sets:read\"]\n[[contributes.exporters]]\nformat = \"{format}\"\ntitle = \"T\"\n\
             extension = \"txt\"\nsource = \"set\"\n"
        );
        manifest("a", &exporter)
    };
    assert_eq!(
        refused(&[set("mixcloud")]),
        "the a plugin's export format \"mixcloud\" is a locallibrary format already"
    );
    assert_eq!(check(&[&set("m3u")]), [None], "export's own formats are a drive's");
}

#[test]
fn formats_that_clash_are_refused() {
    let exporter = |id: &str, format: &str| {
        let toml = format!("[[contributes.exporters]]\nformat = \"{format}\"\ntitle = \"T\"\nextension = \"txt\"\n");
        manifest(id, &toml)
    };
    assert_eq!(check(&[&exporter("a", "plain")]), [None]);
    for format in ["csv", "m3u", "txt"] {
        let e = refused(&[exporter("a", format)]);
        assert_eq!(e, format!("the a plugin's export format \"{format}\" is a locallibrary format already"));
    }
    assert_eq!(
        refused(&[exporter("a", "plain"), exporter("b", "plain")]),
        "the b plugin's export format \"plain\" is the a plugin's already"
    );
}

/// Capabilities are said in plain words, a server by its name, each in words of its own.
#[test]
fn capabilities_are_said_plainly() {
    let m = manifest("p", "capabilities = [\"library:read\", \"net:musicbrainz.org\"]");
    let said: Vec<String> = Granted::all(&m).into_iter().map(|g| g.means).collect();
    assert_eq!(said, ["read your libraries' tracks and playlists", "talk to musicbrainz.org"]);
    let mut all: Vec<String> = Capability::NAMED.iter().map(means).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), Capability::NAMED.len());
}

/// What one drawn plugin is: its manifest's JSON, what it claims (its id and places on the
/// command line, its export formats, its import formats) as `(name, whose, what)`, and why it's
/// left out on its own.
struct Planned {
    manifest: Value,
    places: Vec<(String, String, String)>,
    formats: Vec<(String, String, String)>,
    imports: Vec<(String, String, String)>,
    alone: Option<String>,
}

impl Planned {
    /// A plugin `id` that clashes with nothing: a command and an operation (at places of their
    /// own or not) whose params' flags are their own, an export format and an import format.
    fn clean(tc: &hegel::TestCase, id: &str, i: usize) -> Planned {
        use hegel::generators as gs;
        let mut p = Planned {
            manifest: json!({ "id": id, "name": "P", "version": "0.1.0", "protocol": "^0.2" }),
            places: vec![(id.into(), format!("the {id} plugin"), format!("the {id} plugin's id"))],
            formats: Vec::new(),
            imports: Vec::new(),
            alone: None,
        };
        for (list, name) in [("commands", "run"), ("operations", "change")] {
            let mut c = json!({ "name": name, "title": "T" });
            if list == "commands" {
                c["mode"] = json!("headless");
            }
            let params: Value = (0..tc.draw(gs::integers::<usize>().max_value(2)))
                .map(|j| (format!("p_{j}"), json!({ "type": "string" })))
                .collect::<serde_json::Map<_, _>>()
                .into();
            c["params"] = json!({ "type": "object", "properties": params });
            if tc.draw(gs::booleans()) {
                let place = match tc.draw(gs::booleans()) {
                    true => format!("{name}-of-{i}"),
                    false => format!("playlist {name}-of-{i}"),
                };
                p.claim_place(&place);
                c["cli"] = json!(place);
            }
            p.manifest["contributes"][list] = json!([c]);
        }
        if tc.draw(gs::booleans()) {
            p.add_format(&format!("fmt-of-{i}"), "drive");
        }
        if tc.draw(gs::booleans()) {
            p.add_import(&format!("imp-of-{i}"));
        }
        p
    }

    fn id(&self) -> String {
        self.manifest["id"].as_str().unwrap().to_string()
    }

    fn claim_place(&mut self, place: &str) {
        let id = self.id();
        self.places.push((
            place.into(),
            format!("the {id} plugin's command"),
            format!("the {id} plugin's command {place:?}"),
        ));
    }

    /// An exporter of `source` (`drive`, `set` or `mix`).
    fn add_format(&mut self, format: &str, source: &str) {
        let id = self.id();
        let exporter = json!({ "format": format, "title": "T", "extension": "txt", "source": source });
        if source != "drive" {
            self.manifest["capabilities"] = json!(["sets:read"]);
        }
        let exporters = self.manifest["contributes"]["exporters"].as_array().cloned().unwrap_or_default();
        self.manifest["contributes"]["exporters"] = json!([exporters, vec![exporter]].concat());
        self.formats.push((
            format.into(),
            format!("the {id} plugin's"),
            format!("the {id} plugin's export format {format:?}"),
        ));
    }

    fn add_import(&mut self, format: &str) {
        let id = self.id();
        let importer = json!({ "format": format, "title": "T", "provides": ["tags"] });
        let importers = self.manifest["contributes"]["importers"].as_array().cloned().unwrap_or_default();
        self.manifest["contributes"]["importers"] = json!([importers, vec![importer]].concat());
        self.imports.push((
            format.into(),
            format!("the {id} plugin's"),
            format!("the {id} plugin's import format {format:?}"),
        ));
    }

    /// A command placed at `place`, its first.
    fn place(&mut self, place: &str) {
        self.manifest["contributes"]["commands"][0]["cli"] = json!(place);
        self.places.retain(|(_, whose, _)| !whose.ends_with("command"));
        if let Some(op) = self.manifest["contributes"]["operations"][0]["cli"].as_str().map(String::from) {
            self.claim_place(&op);
        }
        self.claim_place(place);
        self.places.sort_by_key(|(_, whose, _)| !whose.ends_with("plugin"));
    }
}

/// The names locallibrary's command line has: its commands (and their aliases), and those in
/// its groups as `group name`; and the groups.
fn command_line() -> (Vec<String>, Vec<String>) {
    let cli = crate::Cli::command();
    let mut names = vec!["help".to_string()];
    let mut groups = Vec::new();
    for c in cli.get_subcommands() {
        names.extend(std::iter::once(c.get_name()).chain(c.get_all_aliases()).map(String::from));
        if c.has_subcommands() {
            groups.push(c.get_name().to_string());
            for s in c.get_subcommands() {
                names.extend(
                    std::iter::once(s.get_name()).chain(s.get_all_aliases()).map(|n| format!("{} {n}", c.get_name())),
                );
            }
        }
    }
    (names, groups)
}

/// Of any plugins, exactly those that clash are left out, each saying with what: locallibrary's
/// commands, groups, flags, formats and the sidecar's namespaces on its own, and another plugin
/// claiming the same id, place or format (both are left out, the later named as clashing with
/// the earlier). The others are kept.
#[hegel::test(test_cases = 200)]
fn plugins_that_clash_are_left_out_saying_why(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (commands, groups) = command_line();
    // Those a plugin's id could be: the sidecar's own names (`version`) are namespaces too.
    let namespaces: Vec<&str> = crate::rpc::namespaces()
        .into_iter()
        .filter(|n| n.starts_with(|c: char| c.is_ascii_lowercase()))
        .filter(|n| n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'))
        .collect();
    let cli = crate::Cli::command();
    let globals: Vec<String> =
        cli.get_arguments().filter(|a| a.is_global_set()).filter_map(|a| a.get_long()).map(String::from).collect();
    let flags = [globals, ["help", "version", "params", "dry-run"].map(String::from).to_vec()].concat();
    fn names<F: ValueEnum>() -> Vec<String> {
        let values = F::value_variants().iter().filter_map(ValueEnum::to_possible_value);
        let names = values.flat_map(|v| v.get_name_and_aliases().map(String::from).collect::<Vec<_>>());
        // Those a format's name can be (not `1001`).
        names.filter(|n| n.starts_with(|c: char| c.is_ascii_lowercase())).collect()
    }
    // A drive exporter's against `export`'s, a set exporter's against `sets export`'s.
    let formats = [("drive", names::<FormatArg>()), ("set", names::<crate::sets_cmd::FormatArg>())];
    let top: Vec<&String> = commands.iter().filter(|c| !c.contains(' ') && !namespaces.contains(&c.as_str())).collect();
    let ungrouped: Vec<String> = commands.iter().filter(|c| !c.contains(' ') && !groups.contains(c)).cloned().collect();

    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(5));
    let mut plugins: Vec<Planned> = (0..n).map(|i| Planned::clean(&tc, &format!("plug-{i}"), i)).collect();
    // Each plugin clashes in one way at most, so which way says why is plain.
    let mut free: Vec<usize> = (0..n).collect();
    while !free.is_empty() && tc.draw(gs::booleans()) {
        let i = free.remove(tc.draw(gs::integers::<usize>().max_value(free.len() - 1)));
        let p = &mut plugins[i];
        let id = p.id();
        let kind = tc.draw(gs::integers::<u8>().max_value(if free.is_empty() { 6 } else { 10 }));
        match kind {
            0 => {
                let ns = tc.draw(gs::sampled_from(namespaces.clone()));
                p.manifest["id"] = json!(ns);
                p.alone = Some(format!(
                    "the {ns} plugin's id is the sidecar's {ns}.* methods' too: its plans would be named like them"
                ));
            }
            1 => {
                let name = tc.draw(gs::sampled_from(top.clone())).clone();
                p.manifest["id"] = json!(name);
                p.alone = Some(format!("the {name} plugin's id is a locallibrary command already"));
            }
            2 => {
                let place = tc.draw(gs::sampled_from(commands.clone()));
                p.place(&place);
                p.alone = Some(format!("the {id} plugin's command {place:?} is a locallibrary command already"));
            }
            3 => {
                let group = match tc.draw(gs::booleans()) {
                    true => tc.draw(gs::sampled_from(ungrouped.clone())),
                    false => "nogroup".into(),
                };
                let place = format!("{group} go");
                p.place(&place);
                p.alone = Some(format!(
                    "the {id} plugin's command {place:?} is in {group:?}, which isn't a group of commands"
                ));
            }
            4 => {
                let name = tc.draw(gs::sampled_from(&["change", "help"][..]));
                p.manifest["contributes"]["commands"][0]["name"] = json!(name);
                p.alone =
                    Some(format!("the {id} plugin has two commands called {name:?} (a command and an operation)"));
            }
            5 => {
                let flag = tc.draw(gs::sampled_from(flags.clone()));
                let (param, schema) = match flag.contains('-') || tc.draw(gs::booleans()) {
                    true => ("given".to_string(), json!({ "type": "string", "x-cli": { "flag": flag } })),
                    false => (flag.clone(), json!({ "type": "string" })),
                };
                let list = tc.draw(gs::sampled_from(&["commands", "operations"][..]));
                let c = &mut p.manifest["contributes"][list][0];
                let name = c["name"].as_str().unwrap().to_string();
                let mut properties = serde_json::Map::new();
                properties.insert("p_0".into(), json!({ "type": "string" }));
                properties.insert(param.clone(), schema);
                c["params"]["properties"] = properties.into();
                p.alone =
                    Some(format!("the {id} plugin's {name} has a param {param:?} for --{flag}, a flag it has already"));
            }
            6 => {
                let (source, theirs) = tc.draw(gs::sampled_from(formats.to_vec()));
                let format = tc.draw(gs::sampled_from(theirs));
                p.add_format(&format, source);
                p.alone = Some(format!("the {id} plugin's export format {format:?} is a locallibrary format already"));
            }
            // Clashing with another plugin, one not clashing yet.
            _ => {
                let j = free.remove(tc.draw(gs::integers::<usize>().max_value(free.len() - 1)));
                let other = plugins[j].id();
                let p = &mut plugins[i];
                match kind {
                    7 => {
                        p.manifest["id"] = json!(other);
                        p.places[0] =
                            (other.clone(), format!("the {other} plugin"), format!("the {other} plugin's id"));
                        let rest = p.places.split_off(1);
                        p.places.extend(
                            rest.into_iter().map(|(name, whose, what)| {
                                (name, whose.replace(&id, &other), what.replace(&id, &other))
                            }),
                        );
                        for claims in [&mut p.formats, &mut p.imports] {
                            *claims = claims
                                .drain(..)
                                .map(|(n, w, x)| (n, w.replace(&id, &other), x.replace(&id, &other)))
                                .collect();
                        }
                    }
                    8 => p.place(&other),
                    9 => {
                        // Whatever each writes out: formats are each plugin's alone.
                        let format = format!("shared-{i}-{j}");
                        p.add_format(&format, tc.draw(gs::sampled_from(vec!["drive", "set", "mix"])));
                        plugins[j].add_format(&format, "drive");
                    }
                    _ => {
                        let format = format!("shared-{i}-{j}");
                        p.add_import(&format);
                        plugins[j].add_import(&format);
                    }
                }
            }
        }
    }

    // Who's left out: on its own, else by the first name it claims that a plugin before it
    // claims too, else by the first name an earlier one claims that a later one claims too.
    let mut expected: Vec<Option<String>> = plugins.iter().map(|p| p.alone.clone()).collect();
    type Claims = fn(&Planned) -> Vec<(String, String, String)>;
    let kinds: [Claims; 3] = [|p| p.places.clone(), |p| p.formats.clone(), |p| p.imports.clone()];
    for claims in kinds {
        let mut first: Vec<(String, usize, String)> = Vec::new();
        for (i, p) in plugins.iter().enumerate().filter(|(_, p)| p.alone.is_none()) {
            for (name, whose, what) in claims(p) {
                match first.iter().find(|(n, _, _)| *n == name) {
                    Some((_, earlier, theirs)) => {
                        let why = format!("{what} is {theirs} already");
                        expected[*earlier].get_or_insert_with(|| why.clone());
                        expected[i].get_or_insert(why);
                    }
                    None => first.push((name, i, whose)),
                }
            }
        }
    }
    let manifests: Vec<Manifest> = plugins
        .iter()
        .map(|p| {
            let m: Manifest = serde_json::from_value(p.manifest.clone()).unwrap();
            m.validate(true).unwrap_or_else(|e| panic!("{e}: {}", p.manifest));
            m
        })
        .collect();
    tc.note(&format!("{:#?}", plugins.iter().map(|p| &p.manifest).collect::<Vec<_>>()));
    assert_eq!(left_out(&manifests), expected);
}
