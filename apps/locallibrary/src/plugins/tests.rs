use super::*;

/// A first-party manifest with the id `id` and `contributes` (TOML) added.
fn manifest(id: &str, contributes: &str) -> Manifest {
    let m = Manifest::parse(&format!(
        "id = \"{id}\"\nname = \"Test\"\nversion = \"0.1.0\"\nprotocol = \"^0.1\"\n{contributes}"
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
        FIRST_PARTY.iter().map(|(m, _)| PluginSpec::first_party(m, launch.clone()).unwrap()).collect();
    assert_eq!(check(&specs.iter().map(|s| &s.manifest).collect::<Vec<_>>()), vec![None; FIRST_PARTY.len()]);
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
fn formats_that_clash_are_refused() {
    let exporter = |id: &str, format: &str| {
        let toml = format!("[[contributes.exporters]]\nformat = \"{format}\"\ntitle = \"T\"\nextension = \"txt\"\n");
        manifest(id, &toml)
    };
    assert_eq!(check(&[&exporter("a", "plain")]), [None]);
    for format in ["csv", "m3u", "traktor"] {
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
