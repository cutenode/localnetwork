use hegel::TestCase;
use hegel::generators::{self as gs, Generator};
use indexmap::IndexMap;
use serde_json::{Map, json};

use super::*;
use crate::{
    Account, Analyzer, Command, Destination, DestinationKind, EventName, Exporter, FieldType, FieldValue, Listener,
    Mode, Operation, Provider, ProviderKind, Role, Service, code,
};

const HEAD: &str = "id = \"demo\"\nname = \"Demo\"\nversion = \"0.1.0\"\nprotocol = \"^0.2\"\n";

const ECHO: &str = include_str!("../../examples/echo/locallibrary-plugin.toml");

/// What's wrong with `HEAD` followed by `rest`, for a third-party plugin (`""`: nothing).
fn problem(rest: &str) -> String {
    match Manifest::parse(&format!("{HEAD}{rest}")).and_then(|m| m.validate(false)) {
        Ok(()) => String::new(),
        Err(e) => e.to_string(),
    }
}

#[test]
fn the_example_manifest_is_valid() {
    let m = Manifest::parse(ECHO).unwrap();
    m.validate(false).unwrap();
    assert_eq!(m.id, "echo");
    assert_eq!(m.capabilities, [Capability::LibraryRead, Capability::LibraryPlan]);
    assert_eq!(m.contributes.commands[1].mode, Mode::View);
    assert_eq!(m.contributes.analyzers[0].fields["bytes"], FieldType::Integer);
    assert_eq!(m.contributes.settings[0].default, Some(json!("hello")));
}

/// `describe` sends the manifest as JSON, and the host compares it with the one it knows.
#[test]
fn a_manifest_reads_back_from_json_unchanged() {
    let m = Manifest::parse(ECHO).unwrap();
    let back: Manifest = serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
    assert_eq!(back, m);
}

#[test]
fn only_first_party_plugins_add_cli_commands() {
    let m = Manifest::parse(&format!(
        "{HEAD}[[contributes.commands]]\nname = \"import\"\ntitle = \"Import\"\nmode = \"headless\"\ncli = \"import\"\n"
    ))
    .unwrap();
    m.validate(true).unwrap();
    let e = m.validate(false).unwrap_err().to_string();
    assert_eq!(e, "contributes.commands[0].cli: only plugins locallibrary ships add CLI commands");
}

#[test]
fn cli_names_are_names_and_unique() {
    let command = |name: &str, cli: &str| {
        format!("[[contributes.commands]]\nname = \"{name}\"\ntitle = \"T\"\nmode = \"headless\"\ncli = \"{cli}\"\n")
    };
    let m = Manifest::parse(&format!("{HEAD}{}{}", command("a", "go"), command("b", "go"))).unwrap();
    assert_eq!(m.validate(true).unwrap_err().to_string(), "contributes.commands[1].cli: \"go\" is used twice");
    let m = Manifest::parse(&format!("{HEAD}{}", command("a", "Go"))).unwrap();
    assert!(m.validate(true).unwrap_err().to_string().contains("\"Go\" isn't one or two names"));
    // Commands and operations share the command line.
    let operation = "[[contributes.operations]]\nname = \"a\"\ntitle = \"T\"\ncli = \"go\"\n";
    let m = Manifest::parse(&format!("{HEAD}{}{operation}", command("a", "go"))).unwrap();
    assert_eq!(m.validate(true).unwrap_err().to_string(), "contributes.operations[0].cli: \"go\" is used twice");
}

#[test]
fn only_first_party_plugins_give_operations_a_cli_place() {
    let m = Manifest::parse(&format!(
        "{HEAD}[[contributes.operations]]\nname = \"import\"\ntitle = \"Import\"\ncli = \"playlist import\"\n"
    ))
    .unwrap();
    m.validate(true).unwrap();
    assert_eq!(m.contributes.operations[0].cli.as_deref(), Some("playlist import"));
    let e = m.validate(false).unwrap_err().to_string();
    assert_eq!(e, "contributes.operations[0].cli: only plugins locallibrary ships add CLI commands");
}

#[test]
fn a_cli_place_is_one_or_two_names() {
    let place = |cli: &str| {
        let toml = format!("{HEAD}[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\ncli = {cli:?}\n");
        match Manifest::parse(&toml).unwrap().validate(true) {
            Ok(()) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    for fine in ["import", "playlist import", "tags-2 set_all"] {
        assert_eq!(place(fine), "", "{fine}");
    }
    for wrong in ["playlist import now", "playlist  import", " import", "import ", "Playlist import", "a/b", ""] {
        let e = place(wrong);
        assert!(e.starts_with(&format!("contributes.operations[0].cli: {wrong:?} isn't one or two names (")), "{e}");
    }
}

/// `x-cli` is checked wherever params or options are, so the CLI can rely on it.
#[test]
fn a_propertys_cli_hints_are_checked() {
    let hinted = |hints: &str| {
        problem(&format!(
            "[[contributes.commands]]\nname = \"c\"\ntitle = \"C\"\nmode = \"headless\"\n\
             params = {{ type = \"object\", properties = {{ from = {{ type = \"string\", x-cli = {hints} }} }} }}\n"
        ))
    };
    assert_eq!(hinted("{ flag = \"source\", positional = true }"), "");
    let at = "contributes.commands[0].params.properties.from.x-cli: ";
    for (hints, said) in [
        ("{ short = \"f\" }", "unknown field `short`"),
        ("{ flag = \"From\" }", "its flag \"From\" isn't a name: "),
        ("{ flag = \"--from\" }", "its flag \"--from\" isn't a name: "),
        ("{ positional = \"yes\" }", "invalid type: string \"yes\", expected a boolean"),
        ("{ path = true }", "unknown field `path`"),
        ("{ one_based = true }", "unknown field `one_based`"),
        ("5", "invalid type: integer `5`"),
    ] {
        let e = hinted(hints);
        assert!(e.starts_with(&format!("{at}{said}")), "{hints}: {e}");
    }
    let e = problem(
        "[[contributes.exporters]]\nformat = \"f\"\ntitle = \"F\"\nfolder = true\n\
         options = { type = \"object\", properties = { to = { x-cli = { flag = \"To\" } } } }\n",
    );
    assert!(e.starts_with("contributes.exporters[0].options.properties.to.x-cli: its flag \"To\""), "{e}");
}

/// A command line makes flags of params' names, and gives one of them without a flag at most.
#[test]
fn params_are_named_for_flags_and_one_is_positional() {
    let params = |properties: &str| {
        problem(&format!(
            "[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\n\
             params = {{ type = \"object\", properties = {{ {properties} }} }}\n"
        ))
    };
    assert_eq!(params("min_bpm = { type = \"number\" }"), "");
    for name in ["\"min-bpm\"", "\"Bpm\"", "\"2x\"", "\"a b\""] {
        let e = params(&format!("{name} = {{ type = \"string\" }}"));
        assert!(e.contains("isn't a param's name: lowercase letters, digits and _"), "{name}: {e}");
    }
    let positional = "{ type = \"string\", x-cli = { positional = true } }";
    assert_eq!(params(&format!("file = {positional}")), "");
    // Flags around it are fine.
    let flag = "{ type = \"string\" }";
    assert_eq!(params(&format!("dest = {flag}, file = {positional}, to = {flag}")), "");
    let e = params(&format!("file = {positional}, list = {positional}"));
    assert_eq!(e, "contributes.operations[0].params.properties.list.x-cli: \"file\" is positional already");
    let e = params("n = { type = \"integer\", x-cli = { positional = true } }");
    assert!(e.ends_with("only a string is positional"), "{e}");
}

/// A file or folder is a string (or a list of them) marked `x-path`, and hosts take it only as
/// an absolute path.
#[test]
fn paths_are_strings_and_absolute() {
    let params = |properties: &str| {
        problem(&format!(
            "[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\n\
             params = {{ type = \"object\", properties = {{ {properties} }} }}\n"
        ))
    };
    assert_eq!(params("file = { type = \"string\", x-path = true }"), "");
    assert_eq!(params("files = { type = \"array\", items = { type = \"string\" }, x-path = true }"), "");
    let wrong = "contributes.operations[0].params.properties.n.x-path: is true or false, on a string or a list of them";
    assert_eq!(params("n = { type = \"integer\", x-path = true }"), wrong);
    assert_eq!(params("n = { type = \"string\", x-path = \"yes\" }"), wrong);

    let schema = json!({ "type": "object", "properties": {
        "file": { "type": "string", "x-path": true },
        "files": { "type": "array", "items": { "type": "string" }, "x-path": true },
        "name": { "type": "string" },
    } });
    let absolute = if cfg!(windows) { "C:\\list.txt" } else { "/list.txt" };
    let fine = json!({ "file": absolute, "files": [absolute], "name": "list.txt" });
    assert_eq!(check_params(Some(&schema), &fine), Ok(()));
    for (params, at) in [(json!({ "file": "list.txt" }), "/file"), (json!({ "files": [absolute, "b.txt"] }), "/files")]
    {
        let e = check_params(Some(&schema), &params).unwrap_err();
        assert!(e.message.ends_with("isn't an absolute path"), "{e}");
        assert_eq!((e.code, e.data.unwrap().fields["path"].clone()), (crate::code::INVALID_PARAMS, json!(at)));
    }
}

#[test]
fn cli_hints_are_read_from_a_property() {
    let property = json!({ "type": "string", "x-cli": { "flag": "from", "positional": true } });
    let hints = CliHints::of(&property).unwrap();
    assert_eq!(hints, CliHints { flag: Some("from".into()), positional: true });
    assert!(is_path(&json!({ "type": "string", "x-path": true })));
    assert!(!is_path(&json!({ "type": "string" })));
    assert_eq!(CliHints::of(&json!({ "type": "string" })).unwrap(), CliHints::default());
}

/// An analyzer's fields are columns, in the order its manifest gives them.
#[test]
fn an_analyzers_fields_keep_the_manifests_order() {
    let m = Manifest::parse(&format!(
        "{HEAD}[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nversion = \"1\"\n\
         fields = {{ zeta = \"integer\", alpha = \"number\", mid = \"string\" }}\n"
    ))
    .unwrap();
    let names: Vec<&str> = m.contributes.analyzers[0].fields.keys().map(String::as_str).collect();
    assert_eq!(names, ["zeta", "alpha", "mid"]);
}

#[test]
fn ids_and_versions_are_checked() {
    for (id, ok) in [("demo", true), ("demo-2_x", true), ("Demo", false), ("2demo", false), ("", false)] {
        let m = Manifest::parse(&HEAD.replace("\"demo\"", &format!("{id:?}"))).unwrap();
        assert_eq!(m.validate(false).is_ok(), ok, "{id:?}");
    }
    let long = "a".repeat(65);
    let m = Manifest::parse(&HEAD.replace("demo", &long)).unwrap();
    assert!(m.validate(false).unwrap_err().to_string().starts_with("id: "));

    let m = Manifest::parse(&HEAD.replace("version = \"0.1.0\"", "version = \"one\"")).unwrap();
    assert!(m.validate(false).unwrap_err().to_string().starts_with("version: "));
    let m = Manifest::parse(&HEAD.replace("^0.2", "^^")).unwrap();
    assert!(m.validate(false).unwrap_err().to_string().starts_with("protocol: "));
    let m = Manifest::parse(&HEAD.replace("^0.2", "^0.1")).unwrap();
    assert_eq!(m.validate(false).unwrap_err().to_string(), "protocol: \"^0.1\" doesn't include 0.2.0");
}

#[test]
fn unknown_capabilities_are_refused() {
    let e = problem("capabilities = [\"library:write\"]");
    assert!(e.contains("unknown capability \"library:write\": one of library:read, "), "{e}");
    let e = problem("capabilities = [\"net:\"]");
    assert!(e.contains("\"net:\" names no host"), "{e}");
    assert!(problem("capabilities = [\"net:https://example.com\"]").contains("names no host"));
}

/// A key a manifest has that this side doesn't know is left out, saying where it was: a mistake
/// in a plugin locallibrary ships, refused; another plugin's may be a newer locallibrary's.
#[test]
fn unknown_keys_are_left_out_and_refused_only_first_party() {
    // The host works out whether an operation is destructive from its edits.
    let toml = format!(
        "{HEAD}homepage = \"https://example.com\"\n[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\n\
         destructive = true\n[[contributes.widgets]]\nname = \"w\"\n"
    );
    let m = Manifest::parse(&toml).unwrap();
    assert_eq!(m.unknown(), ["homepage", "contributes.operations[0].destructive", "contributes.widgets"]);
    assert_eq!((m.contributes.operations[0].name.as_str(), m.validate(false).is_ok()), ("o", true));
    assert_eq!(m.validate(true).unwrap_err().to_string(), "homepage: isn't a key locallibrary knows");
    assert!(Manifest::parse(ECHO).unwrap().unknown().is_empty());
}

#[test]
fn capabilities_read_and_write_as_their_names() {
    let mut all = Capability::NAMED.to_vec();
    all.push(Capability::Net("coverartarchive.org".into()));
    for c in all {
        let name = c.to_string();
        assert_eq!(name.parse::<Capability>().unwrap(), c);
        assert_eq!(serde_json::to_value(&c).unwrap(), json!(name));
    }
    assert_eq!(Capability::ArtworkPlan.to_string(), "artwork:plan");
}

#[test]
fn contribution_names_are_names_and_unique() {
    let twice = "[[contributes.operations]]\nname = \"x\"\ntitle = \"X\"\n[[contributes.operations]]\nname = \"x\"\ntitle = \"Y\"\n";
    assert_eq!(problem(twice), "contributes.operations[1].name: \"x\" is used twice");
    let e = problem("[[contributes.commands]]\nname = \"Hi there\"\ntitle = \"T\"\nmode = \"view\"\n");
    assert!(e.starts_with("contributes.commands[0].name: \"Hi there\" isn't a name"), "{e}");
    let twice = "[[contributes.exporters]]\nformat = \"nml\"\ntitle = \"A\"\nextension = \"nml\"\n\
                 [[contributes.exporters]]\nformat = \"nml\"\ntitle = \"B\"\nfolder = true\n";
    assert_eq!(problem(twice), "contributes.exporters[1].format: \"nml\" is used twice");
    let twice = "[[contributes.settings]]\nkey = \"k\"\ntitle = \"A\"\ntype = \"string\"\n\
                 [[contributes.settings]]\nkey = \"k\"\ntitle = \"B\"\ntype = \"string\"\n";
    assert_eq!(problem(twice), "contributes.settings[1].key: \"k\" is used twice");
    let twice = "[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nversion = \"1\"\nfields = { x = \"number\" }\n\
                 [[contributes.analyzers]]\nname = \"a\"\ntitle = \"B\"\nversion = \"1\"\nfields = { x = \"number\" }\n";
    assert_eq!(problem(twice), "contributes.analyzers[1].name: \"a\" is used twice");
}

#[test]
fn params_and_options_are_schemas_for_objects() {
    let e = problem(
        "[[contributes.commands]]\nname = \"c\"\ntitle = \"C\"\nmode = \"view\"\nparams = { type = \"string\" }\n",
    );
    assert_eq!(e, "contributes.commands[0].params: must be a JSON Schema for an object (type = \"object\")");
    let e = problem("[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\nparams = { properties = {} }\n");
    assert!(e.starts_with("contributes.operations[0].params: "), "{e}");
    let e = problem(
        "[[contributes.exporters]]\nformat = \"f\"\ntitle = \"F\"\nfolder = true\noptions = { type = \"array\" }\n",
    );
    assert!(e.starts_with("contributes.exporters[0].options: "), "{e}");
    let ok = "[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\nparams = { type = \"object\" }\n";
    assert_eq!(problem(ok), "");
}

#[test]
fn analyzers_declare_typed_fields() {
    let e = problem(
        "[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nversion = \"1\"\nfields = { bpm = \"float\" }\n",
    );
    assert!(e.contains("unknown variant `float`"), "{e}");
    let e = problem("[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nversion = \"1\"\nfields = {}\n");
    assert_eq!(e, "contributes.analyzers[0].fields: an analyzer needs a field");
    let e = problem(
        "[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nversion = \"1\"\nfields = { BPM = \"number\" }\n",
    );
    assert!(e.starts_with("contributes.analyzers[0].fields.BPM: isn't a name"), "{e}");
}

#[test]
fn field_types_accept_their_values() {
    let cases = [
        (FieldType::Number, json!(1.5), true),
        (FieldType::Number, json!("1.5"), false),
        (FieldType::Integer, json!(-3), true),
        (FieldType::Integer, json!(3), true),
        (FieldType::Integer, json!(1.5), false),
        (FieldType::String, json!("Am"), true),
        // Worked out, with nothing to show: any field may be.
        (FieldType::String, json!(null), true),
        (FieldType::Number, json!(null), true),
        (FieldType::Boolean, json!(true), true),
        (FieldType::Strings, json!(["a", "b"]), true),
        (FieldType::Strings, json!(["a", 1]), false),
        (FieldType::Strings, json!("a"), false),
    ];
    for (kind, value, accepted) in cases {
        // A value of none of the types isn't a field's value at all.
        let value_accepted = serde_json::from_value(value.clone()).is_ok_and(|v: FieldValue| kind.accepts(&v));
        assert_eq!(value_accepted, accepted, "{kind:?} {value}");
    }
}

/// A provider is given tracks' tags: it's granted reading them, whatever it finds.
#[test]
fn a_provider_is_granted_reading_tracks() {
    let provider = |grants: &str, kind: &str| {
        problem(&format!(
            "capabilities = [{grants}]\n[[contributes.providers]]\nname = \"p\"\ntitle = \"P\"\nkind = \"{kind}\"\n"
        ))
    };
    assert_eq!(provider("\"library:read\", \"net:a.org\"", "artwork"), "");
    assert_eq!(provider("\"library:read\"", "metadata"), "");
    let said = "contributes.providers[0]: is given tracks' tags, which needs the library:read capability";
    assert_eq!(provider("\"artwork:read\"", "artwork"), said);
    assert_eq!(provider("", "metadata"), said);
    assert!(provider("", "lyrics").contains("unknown variant `lyrics`"));
}

/// An account's endpoints are read as the HTTP client reads them: https, or http to this computer
/// itself, never with a user name or password, whatever the address looks like.
#[test]
fn an_accounts_endpoints_are_read_as_the_client_reads_them() {
    let token_url = |url: &str| {
        problem(&format!(
            "capabilities = [\"net:auth.example\"]\n[[contributes.accounts]]\nid = \"main\"\ntitle = \"Main\"\n\
             authorize_url = \"https://auth.example/authorize\"\ntoken_url = {url:?}\nclient_id = \"demo\"\n"
        ))
    };
    for url in
        ["https://auth.example/token", "http://127.0.0.1:8080/token", "http://localhost/token", "http://[::1]:9/t"]
    {
        assert_eq!(token_url(url), "", "{url}");
    }
    let refused = [
        "http://localhost@evil.example/token",
        "http://127.0.0.1@evil.example/token",
        "http://127.0.0.1.evil.example/token",
        "http://localhost.evil.example/token",
        "http://localhost%2eevil.example/token",
        "http://evil.example\\@localhost/token",
        "http://[::1].evil.example/token",
        "http://0x7f.0.0.2/token",
        "https://someone@auth.example/token",
        "http://local host/token",
    ];
    for url in refused {
        let said = format!(
            "contributes.accounts[0].token_url: {url:?} isn't an https:// address without a user name or password"
        );
        assert_eq!(token_url(url), said, "{url}");
    }
}

#[test]
fn exporters_write_a_file_or_a_folder() {
    let both = "[[contributes.exporters]]\nformat = \"f\"\ntitle = \"F\"\nextension = \"x\"\nfolder = true\n";
    assert_eq!(problem(both), "contributes.exporters[0]: an exporter has either an extension or folder = true");
    let neither = "[[contributes.exporters]]\nformat = \"f\"\ntitle = \"F\"\n";
    assert_eq!(problem(neither), "contributes.exporters[0]: an exporter has either an extension or folder = true");
}

#[test]
fn settings_have_known_types_and_fitting_defaults() {
    let setting = |rest: &str| format!("[[contributes.settings]]\nkey = \"s\"\ntitle = \"S\"\n{rest}\n");
    assert!(problem(&setting("type = \"color\"")).contains("unknown variant `color`"));
    let fine = [
        "type = \"string\"\ndefault = \"x\"",
        "type = \"number\"\ndefault = 2.5",
        "type = \"boolean\"\ndefault = false",
        "type = \"file\"",
        "type = \"directory\"\ndefault = \"/tmp\"",
        "type = \"secret\"",
        "type = \"enum\"\nchoices = [\"a\", \"b\"]\ndefault = \"b\"",
    ];
    for rest in fine {
        assert_eq!(problem(&setting(rest)), "", "{rest}");
    }
    let choices = "contributes.settings[0].choices: an enum setting has choices, and only an enum";
    assert_eq!(problem(&setting("type = \"enum\"")), choices);
    assert_eq!(problem(&setting("type = \"string\"\nchoices = [\"a\"]")), choices);
    let default = "contributes.settings[0].default: doesn't fit the setting's type (and a secret has none)";
    for rest in [
        "type = \"enum\"\nchoices = [\"a\"]\ndefault = \"c\"",
        "type = \"number\"\ndefault = \"2\"",
        "type = \"boolean\"\ndefault = 1",
        "type = \"secret\"\ndefault = \"hunter2\"",
    ] {
        assert_eq!(problem(&setting(rest)), default, "{rest}");
    }
}

/// The manifest's JSON Schema describes what parses: it takes the example and refuses an
/// unknown capability.
#[test]
fn the_json_schema_describes_manifests() {
    let schema = serde_json::to_value(schemars::schema_for!(Manifest)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut echo = serde_json::to_value(Manifest::parse(ECHO).unwrap()).unwrap();
    assert!(validator.is_valid(&echo), "{:?}", validator.iter_errors(&echo).map(|e| e.to_string()).collect::<Vec<_>>());
    echo["capabilities"] = json!(["net:example.com"]);
    assert!(validator.is_valid(&echo));
    echo["capabilities"] = json!(["library:write"]);
    assert!(!validator.is_valid(&echo));
}

#[test]
fn names_and_titles_arent_empty() {
    let m = Manifest::parse(&HEAD.replace("name = \"Demo\"", "name = \" \"")).unwrap();
    assert_eq!(m.validate(false).unwrap_err().to_string(), "name: is empty");
    for (kind, rest) in [
        ("commands", "name = \"c\"\nmode = \"view\""),
        ("operations", "name = \"o\""),
        ("analyzers", "name = \"a\"\nversion = \"1\"\nfields = { x = \"number\" }"),
        ("exporters", "format = \"f\"\nfolder = true"),
        ("settings", "key = \"s\"\ntype = \"string\""),
    ] {
        let e = problem(&format!("[[contributes.{kind}]]\ntitle = \"\"\n{rest}\n"));
        assert_eq!(e, format!("contributes.{kind}[0].title: is empty"));
    }
}

/// `net:*.<domain>` reaches any host below the domain, and not the domain itself; any other
/// grant only its own host.
#[test]
fn a_net_grant_reaches_its_host_or_those_below_its_domain() {
    let any = Capability::Net("*.archive.org".into());
    for (host, reached) in [
        ("ia800.us.archive.org", true),
        ("www.archive.org", true),
        ("archive.org", false),
        ("notarchive.org", false),
        ("archive.org.evil.example", false),
    ] {
        assert_eq!(any.reaches(host), reached, "{host}");
    }
    let one = Capability::Net("MusicBrainz.org".into());
    assert!(one.reaches("musicbrainz.org") && !one.reaches("beta.musicbrainz.org"));
    assert!(!Capability::LibraryRead.reaches("musicbrainz.org"));
    assert_eq!(problem("capabilities = [\"net:*.archive.org\", \"net:archive.org\"]"), "");
    assert!(problem("capabilities = [\"net:*.\"]").contains("names no host"));
    assert!(problem("capabilities = [\"net:a.*.org\"]").contains("names no host"));
}

/// A `[[net]]` limit is for one of the plugin's `net:` hosts, as it's written, once, at a rate
/// the host can keep.
#[test]
fn a_net_limit_is_for_a_host_the_plugin_talks_to() {
    let limited =
        |limits: &str| problem(&format!("capabilities = [\"net:musicbrainz.org\", \"net:*.archive.org\"]\n{limits}"));
    let one = |host: &str, rate: &str| format!("[[net]]\nhost = \"{host}\"\nper_second = {rate}\n");
    assert_eq!(limited(&one("musicbrainz.org", "1")), "");
    assert_eq!(limited(&format!("{}{}", one("*.archive.org", "0.5"), one("musicbrainz.org", "1"))), "");
    assert_eq!(limited(&one("archive.org", "1")), "net[0].host: net:archive.org isn't one of its capabilities");
    let twice = format!("{}{}", one("musicbrainz.org", "1"), one("musicbrainz.org", "2"));
    assert_eq!(limited(&twice), "net[1].host: \"musicbrainz.org\" is limited twice");
    let rate = "net[0].per_second: is how many requests a second, at least 1/3600";
    assert_eq!(limited(&one("musicbrainz.org", "0")), rate);
    assert_eq!(limited(&one("musicbrainz.org", "0.0002")), rate);
    let m = Manifest::parse(&format!("{HEAD}{}", one("musicbrainz.org", "1"))).unwrap();
    assert_eq!(m.net, [NetLimit { host: "musicbrainz.org".into(), per_second: 1.0 }]);
}

#[test]
fn a_capability_is_granted_once() {
    let e = problem("capabilities = [\"library:read\", \"net:a.org\", \"net:a.org\"]");
    assert_eq!(e, "capabilities[2]: net:a.org is there twice");
    assert_eq!(problem("capabilities = [\"library:read\", \"net:a.org\", \"net:b.org\"]"), "");
}

#[test]
fn an_exporters_extension_is_plain() {
    let exporter =
        |ext: &str| format!("[[contributes.exporters]]\nformat = \"f\"\ntitle = \"F\"\nextension = {ext:?}\n");
    for ext in ["nml", "m3u8", "xml"] {
        assert_eq!(problem(&exporter(ext)), "", "{ext}");
    }
    for ext in [".nml", "tar.gz", "a/b", "NML", "", "abcdefghijklmnopq"] {
        let e = problem(&exporter(ext));
        assert!(e.starts_with("contributes.exporters[0].extension: "), "{ext}: {e}");
    }
}

/// Fields become columns, so two analyzers can't both fill one.
#[test]
fn field_names_are_unique_across_analyzers() {
    let two = "[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nversion = \"1\"\nfields = { bpm = \"number\" }\n\
               [[contributes.analyzers]]\nname = \"b\"\ntitle = \"B\"\nversion = \"1\"\nfields = { key = \"string\", bpm = \"number\" }\n";
    assert_eq!(problem(two), "contributes.analyzers[1].fields.bpm: another analyzer has this field");
}

#[test]
fn schemas_must_compile() {
    let e = problem(
        "[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\nparams = { type = \"object\", properties = 5 }\n",
    );
    assert!(e.starts_with("contributes.operations[0].params: isn't a valid JSON Schema: "), "{e}");
}

#[test]
fn an_enum_settings_choices_are_unique() {
    let e = problem(
        "[[contributes.settings]]\nkey = \"s\"\ntitle = \"S\"\ntype = \"enum\"\nchoices = [\"a\", \"b\", \"a\"]\n",
    );
    assert_eq!(e, "contributes.settings[0].choices: \"a\" is there twice");
}

#[test]
fn params_are_checked_against_their_schema() {
    let m = Manifest::parse(ECHO).unwrap();
    let schema = m.contributes.operations[0].params.as_ref();
    assert_eq!(check_params(schema, &json!({ "name": "Warmup" })), Ok(()));
    let e = check_params(schema, &json!({ "name": 3 })).unwrap_err();
    assert_eq!((e.code, e.data.unwrap().fields["path"].clone()), (crate::code::INVALID_PARAMS, json!("/name")));
    let e = check_params(schema, &json!({})).unwrap_err();
    assert!(e.message.contains("\"name\" is a required property"), "{e}");
    assert!(e.data.unwrap().fields.is_empty(), "the params themselves are wrong");
    // Without a schema, any object.
    assert_eq!(check_params(None, &json!({ "anything": [1] })), Ok(()));
    assert_eq!(check_params(None, &json!([1])).unwrap_err().kind(), Some("invalid_params"));
}

#[test]
fn a_setting_accepts_values_of_its_type() {
    let setting = |kind: &str| {
        let toml = format!("{HEAD}[[contributes.settings]]\nkey = \"s\"\ntitle = \"S\"\ntype = \"{kind}\"\n");
        let toml = if kind == "enum" { format!("{toml}choices = [\"a\", \"b\"]\n") } else { toml };
        Manifest::parse(&toml).unwrap().contributes.settings.remove(0)
    };
    let cases = [
        ("string", json!("x"), true),
        ("file", json!("/a"), true),
        ("directory", json!(1), false),
        ("number", json!(1.5), true),
        ("number", json!("1"), false),
        ("boolean", json!(false), true),
        ("enum", json!("b"), true),
        ("enum", json!("c"), false),
        ("secret", json!("hunter2"), true),
        ("secret", json!(""), false),
        ("secret", json!(2), false),
    ];
    for (kind, value, accepted) in cases {
        assert_eq!(setting(kind).accepts(&value), accepted, "{kind} {value}");
    }
}

#[test]
fn an_analyzer_has_a_version_of_its_own() {
    let analyzer = |version: &str| {
        format!(
            "[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nversion = {version:?}\nfields = {{ x = \"number\" }}\n"
        )
    };
    for fine in ["3", "1.2.0", "2026-10_b+1"] {
        assert_eq!(problem(&analyzer(fine)), "", "{fine}");
    }
    for wrong in ["", ".3", "a/b", "1 2"] {
        let e = problem(&analyzer(wrong));
        assert!(e.starts_with(&format!("contributes.analyzers[0].version: {wrong:?} isn't a version")), "{e}");
    }
    let e = problem("[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nfields = { x = \"number\" }\n");
    assert!(e.contains("missing field `version`"), "{e}");
}

/// Commands and operations are offered in a drive's menu unless they say where.
#[test]
fn commands_and_operations_say_where_they_are_offered() {
    let m = Manifest::parse(ECHO).unwrap();
    assert_eq!(m.contributes.commands[0].offered, [Where::Drive]);
    let operation =
        |places: &str| problem(&format!("[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\nwhere = {places}\n"));
    assert_eq!(operation("[\"track\", \"playlist\", \"drive\"]"), "");
    assert_eq!(operation("[]"), "contributes.operations[0].where: names no place: track, playlist or drive");
    assert_eq!(operation("[\"track\", \"track\"]"), "contributes.operations[0].where: track is there twice");
    assert!(operation("[\"deck\"]").contains("unknown variant `deck`"));
    let command = "[[contributes.commands]]\nname = \"c\"\ntitle = \"C\"\nmode = \"view\"\nwhere = []\n";
    assert_eq!(problem(command), "contributes.commands[0].where: names no place: track, playlist or drive");
}

/// A param filled in from what's selected (`x-from`) is of a type that holds it.
#[test]
fn a_param_from_the_selection_holds_it() {
    let param = |property: &str| {
        problem(&format!(
            "[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\n\
             params = {{ type = \"object\", properties = {{ p = {property} }} }}\n"
        ))
    };
    let tracks = "{ type = \"array\", items = { type = \"integer\" }, x-from = \"selection.tracks\" }";
    assert_eq!(param(tracks), "");
    assert_eq!(param("{ type = \"string\", x-from = \"selection.playlist\" }"), "");
    assert_eq!(param("{ type = \"string\", x-from = \"selection.drive\" }"), "");
    let at = "contributes.operations[0].params.properties.p.x-from: ";
    for (property, said) in [
        (
            "{ type = \"string\", x-from = \"selection.deck\" }",
            "is selection.tracks, selection.playlist or selection.drive",
        ),
        (
            "{ type = \"string\", x-from = \"selection.tracks\" }",
            "\"selection.tracks\" is integers, which the param isn't",
        ),
        (
            "{ type = \"integer\", x-from = \"selection.drive\" }",
            "\"selection.drive\" is a string, which the param isn't",
        ),
    ] {
        assert_eq!(param(property), format!("{at}{said}"), "{property}");
    }
}

#[test]
fn a_selection_is_what_the_context_has() {
    let context = Context {
        drive: Some("USB".into()),
        playlist: Some("Sets/Friday".into()),
        tracks: vec![3, 1],
        ..Context::default()
    };
    assert_eq!(Selection::Tracks.value(&context), Some(json!([3, 1])));
    assert_eq!(Selection::Playlist.value(&context), Some(json!("Sets/Friday")));
    assert_eq!(Selection::Drive.value(&context), Some(json!("USB")));
    for nothing in [Selection::Tracks, Selection::Playlist, Selection::Drive] {
        assert_eq!(nothing.value(&Context::default()), None, "{nothing:?}");
    }
    assert_eq!(Selection::of(&json!({ "type": "string" })), Ok(None));
}

/// An analyzer says which of its fields has each role it answers, each of the role's type; a role
/// that isn't one, or names a field it hasn't, is refused.
#[test]
fn analyzers_give_fields_roles_of_their_type() {
    let analyzer = |fields: &str, roles: &str| {
        format!(
            "[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nversion = \"1\"\nfields = {{ {fields} }}\nroles = {{ {roles} }}\n"
        )
    };
    let ok = analyzer(
        "tempo = \"number\", camelot = \"string\", runs = \"integer\"",
        "bpm = \"tempo\", key = \"camelot\", clipped_runs = \"runs\"",
    );
    let m = Manifest::parse(&format!("{HEAD}{ok}")).unwrap();
    m.validate(true).unwrap();
    let roles: Vec<(Role, &str)> = m.contributes.analyzers[0].roles.iter().map(|(r, f)| (*r, f.as_str())).collect();
    assert_eq!(roles, [(Role::Bpm, "tempo"), (Role::Key, "camelot"), (Role::ClippedRuns, "runs")]);
    let back: Manifest = serde_json::from_value(serde_json::to_value(&m).unwrap()).unwrap();
    assert_eq!(back, m, "describe sends them as they are");

    let e = problem(&analyzer("tempo = \"integer\"", "bpm = \"tempo\""));
    assert_eq!(e, "contributes.analyzers[0].roles.bpm: tempo isn't a number, which the role is");
    let e = problem(&analyzer("camelot = \"number\"", "key = \"camelot\""));
    assert_eq!(e, "contributes.analyzers[0].roles.key: camelot isn't a string, which the role is");
    let e = problem(&analyzer("tempo = \"number\"", "bpm = \"speed\""));
    assert_eq!(e, "contributes.analyzers[0].roles.bpm: \"speed\" isn't its field");
    let e = problem(&analyzer("tempo = \"number\"", "tempo = \"tempo\""));
    assert!(e.contains("unknown variant `tempo`"), "{e}");
    // Every role reads back from its name.
    assert!(Role::ALL.iter().all(|r| r.name().parse() == Ok(*r)));
    assert_eq!("tempo".parse::<Role>().unwrap_err().split(':').next(), Some("\"tempo\" isn't a role"));
}

// Properties: manifests drawn whole, then broken in one place each.

/// What `validate` says about `m` (`""`: nothing).
fn problem_with(m: &Manifest, first_party: bool) -> String {
    m.validate(first_party).err().map(|e| e.to_string()).unwrap_or_default()
}

/// A name as drawn: at most 12 bytes, so the ones [`add`] makes (longer) never clash with it.
const DRAWN_NAME: &str = "[a-z][a-z0-9_-]{0,11}";

#[hegel::test_helper]
fn drawn_names(tc: &TestCase, max: usize) -> Vec<String> {
    tc.draw(gs::vecs(gs::from_regex(DRAWN_NAME)).max_size(max).unique(true))
}

#[hegel::test_helper]
fn title(tc: &TestCase) -> String {
    tc.draw(gs::text().min_size(1).max_size(12).filter(|s: &String| !s.trim().is_empty()))
}

/// Anything but a name: empty, a bad first character, a bad character later, or too long.
#[hegel::test_helper]
fn bad_name(tc: &TestCase) -> String {
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => String::new(),
        1 => tc.draw(gs::from_regex("[A-Z0-9_-][a-z0-9_-]{0,8}")),
        2 => tc.draw(gs::from_regex("[a-z][a-z0-9_-]{0,4}[A-Z.:/é][a-z0-9_-]{0,4}")),
        // A byte too long as often as longer still.
        _ => match tc.draw(gs::booleans()) {
            true => "a".repeat(65),
            false => "a".repeat(tc.draw(gs::integers::<usize>().min_value(66).max_value(80))),
        },
    }
}

/// A param's type in a drawn schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    String,
    Strings,
    Integer,
    Number,
    Boolean,
    Object,
}

const KINDS: [Kind; 6] = [Kind::String, Kind::Strings, Kind::Integer, Kind::Number, Kind::Boolean, Kind::Object];

impl Kind {
    fn schema(self) -> Value {
        let name = match self {
            Kind::Strings => return json!({ "type": "array", "items": { "type": "string" } }),
            Kind::String => "string",
            Kind::Integer => "integer",
            Kind::Number => "number",
            Kind::Boolean => "boolean",
            Kind::Object => "object",
        };
        json!({ "type": name })
    }

    /// A string or a list of them: what `x-path` may mark.
    fn stringy(self) -> bool {
        matches!(self, Kind::String | Kind::Strings)
    }
}

/// A param of a drawn schema, as far as checking params against it goes.
#[derive(Debug, Clone)]
struct Param {
    name: String,
    kind: Kind,
    required: bool,
    /// Its `x-path`, when it says.
    x_path: Option<bool>,
    hints: CliHints,
}

impl Param {
    fn is_path(&self) -> bool {
        self.x_path == Some(true)
    }
}

/// A params or options schema's params: any types, hints that fit them, one positional at most.
#[hegel::test_helper]
fn params(tc: &TestCase) -> Vec<Param> {
    let names = tc.draw(gs::vecs(gs::from_regex("[a-z][a-z0-9_]{0,11}")).max_size(5).unique(true));
    let mut positional = false;
    let mut params = Vec::new();
    for name in names {
        let kind = tc.draw(gs::sampled_from(KINDS.to_vec()));
        let x_path = if kind.stringy() { tc.draw(gs::optional(gs::booleans())) } else { None };
        let here = kind == Kind::String && !positional && tc.draw(gs::booleans());
        positional |= here;
        let hints = CliHints { flag: tc.draw(gs::optional(gs::from_regex(DRAWN_NAME))), positional: here };
        params.push(Param { name, kind, required: tc.draw(gs::booleans()), x_path, hints });
    }
    params
}

/// The JSON Schema for an object with `params`.
fn schema(params: &[Param]) -> Value {
    let mut properties = Map::new();
    for p in params {
        let mut property = p.kind.schema();
        if let Some(path) = p.x_path {
            property["x-path"] = path.into();
        }
        let mut hints = Map::new();
        if let Some(flag) = &p.hints.flag {
            hints.insert("flag".into(), flag.as_str().into());
        }
        if p.hints.positional {
            hints.insert("positional".into(), true.into());
        }
        if !hints.is_empty() {
            property["x-cli"] = hints.into();
        }
        properties.insert(p.name.clone(), property);
    }
    let required: Vec<&str> = params.iter().filter(|p| p.required).map(|p| p.name.as_str()).collect();
    match (properties.is_empty(), required.is_empty()) {
        (true, _) => json!({ "type": "object" }),
        (false, true) => json!({ "type": "object", "properties": properties }),
        (false, false) => json!({ "type": "object", "properties": properties, "required": required }),
    }
}

#[hegel::test_helper]
fn maybe_schema(tc: &TestCase) -> Option<Value> {
    tc.draw(gs::booleans()).then(|| schema(&params(tc)))
}

const SETTING_TYPES: [SettingType; 7] = [
    SettingType::String,
    SettingType::Number,
    SettingType::Boolean,
    SettingType::Enum,
    SettingType::File,
    SettingType::Directory,
    SettingType::Secret,
];

/// A finite number, as JSON has: an integer or not.
#[hegel::test_helper]
fn number(tc: &TestCase) -> Value {
    match tc.draw(gs::booleans()) {
        true => tc.draw(gs::integers::<i64>()).into(),
        false => tc.draw(gs::floats::<f64>().allow_nan(false).allow_infinity(false)).into(),
    }
}

/// A setting of any type, with choices when it's an enum and, but for a secret, maybe a default
/// that fits.
#[hegel::test_helper]
fn setting(tc: &TestCase, key: String) -> Setting {
    let kind = tc.draw(gs::sampled_from(SETTING_TYPES.to_vec()));
    let choices = match kind {
        SettingType::Enum => tc.draw(gs::vecs(gs::text().max_size(8)).min_size(1).max_size(4).unique(true)),
        _ => Vec::new(),
    };
    let default = match kind {
        SettingType::Secret => None,
        _ if !tc.draw(gs::booleans()) => None,
        SettingType::String | SettingType::File | SettingType::Directory => Some(tc.draw(gs::text()).into()),
        SettingType::Number => Some(number(tc)),
        SettingType::Boolean => Some(tc.draw(gs::booleans()).into()),
        SettingType::Enum => Some(tc.draw(gs::sampled_from(choices.clone())).into()),
    };
    Setting { key, title: title(tc), kind, default, choices }
}

/// The menus a command or an operation is offered in: one or more, each once.
#[hegel::test_helper]
fn offered(tc: &TestCase) -> Vec<Where> {
    tc.draw(gs::subsequences(vec![Where::Track, Where::Playlist, Where::Drive]).min_size(1))
}

/// An endpoint an account may have: https anywhere, or http on this computer.
#[hegel::test_helper]
fn endpoint(tc: &TestCase) -> String {
    let path = tc.draw(gs::from_regex("(/[a-z0-9_-]{0,8}){0,2}(\\?[a-z]=[a-z0-9]{0,4})?"));
    match tc.draw(gs::booleans()) {
        true => {
            let host = tc.draw(gs::from_regex("[a-z][a-z0-9]{0,7}(\\.[a-z][a-z0-9]{0,7}){0,2}"));
            let port = tc.draw(gs::optional(gs::integers::<u16>())).map(|p| format!(":{p}")).unwrap_or_default();
            format!("https://{host}{port}{path}")
        }
        false => {
            let host = tc.draw(gs::sampled_from(vec!["127.0.0.1", "localhost", "[::1]"]));
            let port = tc.draw(gs::optional(gs::integers::<u16>())).map(|p| format!(":{p}")).unwrap_or_default();
            format!("http://{host}{port}{path}")
        }
    }
}

/// An account with endpoints it may have, scopes and maybe a revocation endpoint.
#[hegel::test_helper]
fn account(tc: &TestCase, id: String) -> Account {
    Account {
        id,
        title: title(tc),
        authorize_url: endpoint(tc),
        token_url: endpoint(tc),
        scopes: tc.draw(gs::vecs(gs::from_regex("[!-~]{1,12}")).max_size(3)),
        client_id: title(tc),
        pkce: true,
        revoke_url: tc.draw(gs::booleans()).then(|| endpoint(tc)),
    }
}

/// A host granted for an account's requests when no other is: longer than any drawn.
const ACCOUNT_HOST: &str = "account-host.example";

/// A manifest that keeps every rule: commands and operations with places on the command line
/// only for a first-party plugin.
#[hegel::test_helper]
fn manifest(tc: &TestCase, first_party: bool) -> Manifest {
    let mut places = FxHashSet::default();
    let mut place = |tc: &TestCase| {
        if !first_party || !tc.draw(gs::booleans()) {
            return None;
        }
        let cli = tc.draw(gs::from_regex("[a-z][a-z0-9_-]{0,7}( [a-z][a-z0-9_-]{0,7})?"));
        places.insert(cli.clone()).then_some(cli)
    };
    let modes = vec![Mode::Headless, Mode::View];
    let commands = drawn_names(tc, 3)
        .into_iter()
        .map(|name| Command {
            name,
            title: title(tc),
            mode: tc.draw(gs::sampled_from(modes.clone())),
            offered: offered(tc),
            params: maybe_schema(tc),
            cli: place(tc),
        })
        .collect();
    let operations = drawn_names(tc, 3)
        .into_iter()
        .map(|name| Operation {
            name,
            title: title(tc),
            offered: offered(tc),
            long_running: tc.draw(gs::booleans()),
            params: maybe_schema(tc),
            cli: place(tc),
        })
        .collect();
    // Each analyzer a field of its own at least, and the rest shared out.
    let names = drawn_names(tc, 3);
    let n = names.len();
    let field_types =
        vec![FieldType::Number, FieldType::Integer, FieldType::String, FieldType::Boolean, FieldType::Strings];
    let fields = tc.draw(gs::vecs(gs::from_regex(DRAWN_NAME)).min_size(n).max_size(n * 3).unique(true));
    let mut fields = fields.into_iter();
    let mut analyzers: Vec<Analyzer> = names
        .into_iter()
        .zip(fields.by_ref())
        .map(|(name, field)| Analyzer {
            name,
            title: title(tc),
            version: tc.draw(gs::from_regex("[A-Za-z0-9_+-][A-Za-z0-9._+-]{0,11}")),
            fields: IndexMap::from_iter([(field, tc.draw(gs::sampled_from(field_types.clone())))]),
            roles: IndexMap::new(),
        })
        .collect();
    for field in fields {
        let i = tc.draw(gs::integers::<usize>().max_value(n - 1));
        analyzers[i].fields.insert(field, tc.draw(gs::sampled_from(field_types.clone())));
    }
    // Some roles for each, each from one of its fields of the role's type.
    for a in &mut analyzers {
        for role in tc.draw(gs::subsequences(Role::ALL.to_vec())) {
            let typed: Vec<String> =
                a.fields.iter().filter(|(_, t)| **t == role.kind()).map(|(f, _)| f.clone()).collect();
            if !typed.is_empty() {
                a.roles.insert(role, tc.draw(gs::sampled_from(typed)));
            }
        }
    }
    let exporters = drawn_names(tc, 3)
        .into_iter()
        .map(|format| {
            let extension = tc.draw(gs::optional(gs::from_regex("[a-z0-9]{1,16}")));
            Exporter { format, title: title(tc), folder: extension.is_none(), extension, options: maybe_schema(tc) }
        })
        .collect();
    let settings = drawn_names(tc, 3).into_iter().map(|key| setting(tc, key)).collect();
    let accounts: Vec<Account> = drawn_names(tc, 2).into_iter().map(|id| account(tc, id)).collect();
    let destination_kinds = vec![DestinationKind::Store, DestinationKind::Set];
    let destinations = drawn_names(tc, 3)
        .into_iter()
        .map(|name| Destination { name, title: title(tc), kind: tc.draw(gs::sampled_from(destination_kinds.clone())) })
        .collect();
    let mut capabilities = tc.draw(gs::subsequences(Capability::NAMED.to_vec()));
    let kinds = vec![ProviderKind::Artwork, ProviderKind::Metadata];
    let providers: Vec<Provider> = drawn_names(tc, 3)
        .into_iter()
        .map(|name| Provider { name, title: title(tc), kind: tc.draw(gs::sampled_from(kinds.clone())) })
        .collect();
    // Granted reading what they're given.
    if !providers.is_empty() && !capabilities.contains(&Capability::LibraryRead) {
        capabilities.push(Capability::LibraryRead);
    }
    // Each event once, shared out among a few listeners, and granted.
    let heard = tc.draw(gs::permutations(EventName::ALL.to_vec()));
    let heard = &heard[..tc.draw(gs::integers::<usize>().max_value(heard.len()))];
    let mut events: Vec<Listener> = Vec::new();
    for (i, event) in heard.iter().enumerate() {
        match events.last_mut() {
            Some(last) if i > 0 && tc.draw(gs::booleans()) => last.on.push(*event),
            _ => events.push(Listener { on: vec![*event] }),
        }
        if !capabilities.contains(&event.capability()) {
            capabilities.push(event.capability());
        }
    }
    let services = drawn_names(tc, 3).into_iter().map(|name| Service { name, title: title(tc) }).collect();
    let hosts: Vec<String> =
        tc.draw(gs::vecs(gs::from_regex(r"(\*\.)?[A-Za-z0-9.-]{1,16}").fullmatch(true)).max_size(2).unique(true));
    let limited = tc.draw(gs::subsequences(hosts.clone()));
    let net = limited
        .into_iter()
        .map(|host| NetLimit {
            host,
            per_second: tc.draw(gs::floats::<f64>().min_value(1.0 / 3600.0).max_value(100.0)),
        })
        .collect();
    // Granted a server for the accounts' requests.
    if !accounts.is_empty() && hosts.is_empty() {
        capabilities.push(Capability::Net(ACCOUNT_HOST.into()));
    }
    capabilities.extend(hosts.into_iter().map(Capability::Net));
    let pre = tc.draw(gs::optional(gs::from_regex("[a-z][a-z0-9]{0,4}"))).map(|p| format!("-{p}")).unwrap_or_default();
    let [major, minor, patch] = tc.draw(gs::arrays(gs::integers::<u16>()));
    let protocols = vec!["^0.2", "0.2", "~0.2", "^0.2.0", "=0.2.0", "*", ">=0.2.0, <0.3.0", ">=0.0.1"];
    Manifest {
        id: tc.draw(gs::from_regex(DRAWN_NAME)),
        name: title(tc),
        version: format!("{major}.{minor}.{patch}{pre}"),
        protocol: tc.draw(gs::sampled_from(protocols)).to_string(),
        capabilities: tc.draw(gs::permutations(capabilities)),
        contributes: Contributions {
            accounts,
            commands,
            destinations,
            events,
            operations,
            analyzers,
            exporters,
            providers,
            services,
            settings,
        },
        net,
        unknown: Vec::new(),
    }
}

/// `manifest` (as JSON) as TOML a plugin's author could write, a line for each key and inline
/// tables below it.
fn toml_of(manifest: &Value) -> String {
    let lines = manifest
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| format!("{} = {}\n", inline(&key.as_str().into()), inline(value)));
    lines.collect()
}

/// `value` as a TOML value on one line. A JSON string is a TOML basic string, but for DEL, which
/// TOML wants escaped.
fn inline(value: &Value) -> String {
    let joined = |items: Vec<String>| items.join(", ");
    match value {
        Value::Object(table) => {
            let pairs =
                table.iter().map(|(key, value)| format!("{} = {}", inline(&key.as_str().into()), inline(value)));
            format!("{{ {} }}", joined(pairs.collect()))
        }
        Value::Array(list) => format!("[{}]", joined(list.iter().map(inline).collect())),
        Value::String(_) => value.to_string().replace('\u{7f}', "\\u007f"),
        Value::Null => unreachable!("a manifest has no nulls: TOML has none"),
        _ => value.to_string(),
    }
}

/// Where the first place on the command line is, in the order they're checked.
fn first_cli(m: &Manifest) -> Option<String> {
    let c = &m.contributes;
    let commands = c.commands.iter().map(|x| x.cli.is_some()).enumerate().map(|(i, has)| ("commands", i, has));
    let operations = c.operations.iter().map(|x| x.cli.is_some()).enumerate().map(|(i, has)| ("operations", i, has));
    commands.chain(operations).find(|(_, _, has)| *has).map(|(kind, i, _)| format!("contributes.{kind}[{i}].cli"))
}

/// Any manifest that keeps the rules validates, reads back as it was written from TOML and from
/// JSON (what `describe` sends), and fits the manifest's JSON Schema. One that gives commands or
/// operations places on the command line is valid only for a first-party plugin.
#[hegel::test(test_cases = 60)]
fn a_drawn_manifest_is_valid_and_reads_back_as_it_was_written(tc: TestCase) {
    let first_party = tc.draw(gs::booleans());
    let m = manifest(&tc, first_party);
    assert_eq!(problem_with(&m, first_party), "");
    let toml = toml_of(&serde_json::to_value(&m).unwrap());
    assert_eq!(Manifest::parse(&toml).unwrap(), m, "{toml}");
    let json = serde_json::to_value(&m).unwrap();
    assert_eq!(serde_json::from_value::<Manifest>(json.clone()).unwrap(), m);
    let schema = serde_json::to_value(schemars::schema_for!(Manifest)).unwrap();
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&json), "{json}");
    match first_cli(&m) {
        None => assert_eq!(problem_with(&m, false), ""),
        Some(at) => {
            assert_eq!(problem_with(&m, false), format!("{at}: only plugins locallibrary ships add CLI commands"));
        }
    }
}

const CONTRIBUTIONS: [&str; 9] = [
    "accounts",
    "commands",
    "destinations",
    "operations",
    "analyzers",
    "exporters",
    "providers",
    "services",
    "settings",
];

/// What names a contribution of `kind`.
fn name_key(kind: &str) -> &'static str {
    match kind {
        "accounts" => "id",
        "exporters" => "format",
        "settings" => "key",
        _ => "name",
    }
}

fn len(c: &Contributions, kind: &str) -> usize {
    match kind {
        "accounts" => c.accounts.len(),
        "commands" => c.commands.len(),
        "destinations" => c.destinations.len(),
        "operations" => c.operations.len(),
        "analyzers" => c.analyzers.len(),
        "exporters" => c.exporters.len(),
        "providers" => c.providers.len(),
        "services" => c.services.len(),
        _ => c.settings.len(),
    }
}

/// Adds valid contributions of `kind` until there are `n`, named longer than any drawn name.
fn add(c: &mut Contributions, kind: &str, n: usize) {
    while len(c, kind) < n {
        let name = format!("added-{kind}-{}", len(c, kind));
        let title = "Added".to_string();
        match kind {
            // Granted a net: capability by the caller.
            "accounts" => c.accounts.push(Account {
                id: name,
                title,
                authorize_url: "https://auth.example/authorize".into(),
                token_url: "https://auth.example/token".into(),
                scopes: Vec::new(),
                client_id: "added".into(),
                pkce: true,
                revoke_url: None,
            }),
            "destinations" => c.destinations.push(Destination { name, title, kind: DestinationKind::Store }),
            "commands" => {
                let offered = on_a_drive();
                c.commands.push(Command { name, title, mode: Mode::Headless, offered, params: None, cli: None });
            }
            "operations" => {
                let offered = on_a_drive();
                c.operations.push(Operation { name, title, offered, long_running: false, params: None, cli: None });
            }
            "analyzers" => {
                let fields = IndexMap::from_iter([(format!("{name}-field"), FieldType::Number)]);
                c.analyzers.push(Analyzer { name, title, version: "1".into(), fields, roles: IndexMap::new() });
            }
            "exporters" => {
                c.exporters.push(Exporter { format: name, title, extension: None, folder: true, options: None });
            }
            // Granted library:read by the caller.
            "providers" => c.providers.push(Provider { name, title, kind: ProviderKind::Metadata }),
            "services" => c.services.push(Service { name, title }),
            _ => c.settings.push(Setting {
                key: name,
                title,
                kind: SettingType::String,
                default: None,
                choices: Vec::new(),
            }),
        }
    }
}

/// The name (or format, or key) and title of each contribution of `kind`.
fn names_and_titles<'a>(c: &'a mut Contributions, kind: &str) -> Vec<(&'a mut String, &'a mut String)> {
    match kind {
        "accounts" => c.accounts.iter_mut().map(|x| (&mut x.id, &mut x.title)).collect(),
        "commands" => c.commands.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "destinations" => c.destinations.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "operations" => c.operations.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "analyzers" => c.analyzers.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "exporters" => c.exporters.iter_mut().map(|x| (&mut x.format, &mut x.title)).collect(),
        "providers" => c.providers.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "services" => c.services.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        _ => c.settings.iter_mut().map(|x| (&mut x.key, &mut x.title)).collect(),
    }
}

/// Another of contribution `i` of `kind`, named the same, at the end.
fn duplicate(c: &mut Contributions, kind: &str, i: usize) {
    match kind {
        "accounts" => c.accounts.push(c.accounts[i].clone()),
        "commands" => c.commands.push(Command { cli: None, ..c.commands[i].clone() }),
        "destinations" => c.destinations.push(c.destinations[i].clone()),
        "operations" => c.operations.push(Operation { cli: None, ..c.operations[i].clone() }),
        "analyzers" => c.analyzers.push(c.analyzers[i].clone()),
        "exporters" => c.exporters.push(c.exporters[i].clone()),
        "providers" => c.providers.push(c.providers[i].clone()),
        "services" => c.services.push(c.services[i].clone()),
        _ => c.settings.push(c.settings[i].clone()),
    }
}

/// The params or options schema of contribution `i` of `kind`, and where it is.
fn schema_of<'a>(c: &'a mut Contributions, kind: &str, i: usize) -> (&'a mut Option<Value>, String) {
    match kind {
        "commands" => (&mut c.commands[i].params, format!("contributes.commands[{i}].params")),
        "operations" => (&mut c.operations[i].params, format!("contributes.operations[{i}].params")),
        _ => (&mut c.exporters[i].options, format!("contributes.exporters[{i}].options")),
    }
}

#[hegel::test_helper]
fn index(tc: &TestCase, len: usize) -> usize {
    tc.draw(gs::integers::<usize>().max_value(len - 1))
}

/// Any one thing wrong with a manifest that's otherwise right is refused, saying where it is and
/// what's wrong with it: the first thing `validate` finds is that.
#[hegel::test(test_cases = 150)]
fn each_kind_of_mistake_is_refused_saying_where(tc: TestCase) {
    let mut first_party = tc.draw(gs::booleans());
    let mut m = manifest(&tc, first_party);
    let mistakes = vec![
        "id",
        "name",
        "version",
        "protocol",
        "protocol range",
        "capability twice",
        "cli",
        "cli place",
        "cli twice",
        "contribution name",
        "contribution twice",
        "title",
        "not an object",
        "bad schema",
        "param name",
        "x-cli",
        "positional twice",
        "x-path",
        "no fields",
        "field name",
        "field twice",
        "role field",
        "role type",
        "file or folder",
        "extension",
        "choices",
        "choice twice",
        "default",
        "net host",
        "net twice",
        "net rate",
        "provider grant",
        "account endpoint",
        "account client",
        "account scope",
        "account pkce",
        "account grant",
        "no events",
        "event twice",
        "event grant",
    ];
    let mistake = tc.draw(gs::sampled_from(mistakes));
    let c = &mut m.contributes;
    // What `validate` says, and only `prefix` of it, when the rest is another crate's words.
    let (said, prefix) = match mistake {
        "id" => {
            m.id = bad_name(&tc);
            (format!("id: {:?} isn't a name: {NAME_RULE}", m.id), false)
        }
        "name" => {
            m.name = tc.draw(gs::text().alphabet(" \t\r\n").max_size(3));
            ("name: is empty".into(), false)
        }
        "version" => {
            let versions = vec!["", "1", "1.2", "v1.0.0", "1.0.0.0", "01.0.0", "one", "1.0.0-"];
            m.version = tc.draw(gs::sampled_from(versions)).into();
            ("version: ".into(), true)
        }
        "protocol" => {
            m.protocol = tc.draw(gs::sampled_from(vec!["^^", "one", "0.2.x.y", ">= ^1", "^0.2 ||"])).into();
            ("protocol: ".into(), true)
        }
        "protocol range" => {
            let range = tc.draw(gs::tuples!(gs::integers::<u8>(), gs::integers::<u8>()).filter(|v| *v != (0, 2)));
            m.protocol = format!("^{}.{}", range.0, range.1);
            (format!("protocol: {:?} doesn't include {PROTOCOL_VERSION}", m.protocol), false)
        }
        "capability twice" => {
            if m.capabilities.is_empty() {
                m.capabilities.push(Capability::LibraryRead);
            }
            let again = m.capabilities[index(&tc, m.capabilities.len())].clone();
            m.capabilities.push(again.clone());
            (format!("capabilities[{}]: {again} is there twice", m.capabilities.len() - 1), false)
        }
        "cli" | "cli place" | "cli twice" => {
            let at_least = if mistake == "cli twice" { 2 } else { 1 };
            if c.commands.len() + c.operations.len() < at_least {
                add(c, "commands", at_least);
            }
            let mut places: Vec<(String, &mut Option<String>)> = c
                .commands
                .iter_mut()
                .enumerate()
                .map(|(i, x)| (format!("contributes.commands[{i}].cli"), &mut x.cli))
                .collect();
            places.extend(
                c.operations
                    .iter_mut()
                    .enumerate()
                    .map(|(i, x)| (format!("contributes.operations[{i}].cli"), &mut x.cli)),
            );
            let i = index(&tc, places.len());
            // Only a first-party plugin's places are checked further.
            first_party = true;
            match mistake {
                "cli" => {
                    places[i].1.get_or_insert_with(|| "added-place".into());
                    let at = first_cli(&m).unwrap();
                    return assert_eq!(
                        problem_with(&m, false),
                        format!("{at}: only plugins locallibrary ships add CLI commands")
                    );
                }
                "cli place" => {
                    let word = tc.draw(gs::from_regex("[a-z]{1,3}"));
                    let wrong = match tc.draw(gs::integers::<u8>().max_value(5)) {
                        0 => format!("{word} {word} {word}"),
                        1 => format!("{word}  {word}"),
                        2 => format!(" {word}"),
                        3 => format!("{word} "),
                        4 => word.to_uppercase(),
                        _ => String::new(),
                    };
                    *places[i].1 = Some(wrong.clone());
                    let at = places[i].0.clone();
                    (format!("{at}: {wrong:?} isn't one or two names ({NAME_RULE}), like \"playlist import\""), false)
                }
                _ => {
                    // Twice: at `i` and at a later one.
                    let i = i.min(places.len() - 2);
                    let j = i + 1 + index(&tc, places.len() - i - 1);
                    let place = places[i].1.get_or_insert_with(|| "added-place".into()).clone();
                    *places[j].1 = Some(place.clone());
                    (format!("{}: {place:?} is used twice", places[j].0), false)
                }
            }
        }
        "contribution name" | "contribution twice" | "title" => {
            let kind = tc.draw(gs::sampled_from(CONTRIBUTIONS.to_vec()));
            add(c, kind, 1);
            if kind == "providers" && !m.capabilities.contains(&Capability::LibraryRead) {
                m.capabilities.push(Capability::LibraryRead);
            }
            if kind == "accounts" && !m.capabilities.iter().any(|g| matches!(g, Capability::Net(_))) {
                m.capabilities.push(Capability::Net(ACCOUNT_HOST.into()));
            }
            let i = index(&tc, len(c, kind));
            let key = name_key(kind);
            match mistake {
                "contribution name" => {
                    let wrong = bad_name(&tc);
                    *names_and_titles(c, kind)[i].0 = wrong.clone();
                    (format!("contributes.{kind}[{i}].{key}: {wrong:?} isn't a name: {NAME_RULE}"), false)
                }
                "contribution twice" => {
                    duplicate(c, kind, i);
                    let name = names_and_titles(c, kind)[i].0.clone();
                    (format!("contributes.{kind}[{}].{key}: {name:?} is used twice", len(c, kind) - 1), false)
                }
                _ => {
                    *names_and_titles(c, kind)[i].1 = tc.draw(gs::text().alphabet(" \t\r\n").max_size(3));
                    (format!("contributes.{kind}[{i}].title: is empty"), false)
                }
            }
        }
        "not an object" | "bad schema" | "param name" | "x-cli" | "positional twice" | "x-path" => {
            let kind = tc.draw(gs::sampled_from(vec!["commands", "operations", "exporters"]));
            add(c, kind, 1);
            let i = index(&tc, len(c, kind));
            let (slot, at) = schema_of(c, kind, i);
            let mut drawn = params(&tc);
            let (property, said) = match mistake {
                "not an object" => {
                    let wrong = vec![
                        json!({}),
                        json!({ "type": "string" }),
                        json!({ "type": ["object"] }),
                        json!({ "properties": {} }),
                    ];
                    *slot = Some(tc.draw(gs::sampled_from(wrong)));
                    return assert_eq!(
                        problem_with(&m, first_party),
                        format!("{at}: must be a JSON Schema for an object (type = \"object\")")
                    );
                }
                "bad schema" => {
                    let wrong = vec![
                        json!({ "type": "object", "properties": 5 }),
                        json!({ "type": "object", "required": "name" }),
                        json!({ "type": "object", "minProperties": -1 }),
                    ];
                    *slot = Some(tc.draw(gs::sampled_from(wrong)));
                    return assert!(
                        problem_with(&m, first_party).starts_with(&format!("{at}: isn't a valid JSON Schema: "))
                    );
                }
                "param name" => {
                    let name = match tc.draw(gs::booleans()) {
                        true => bad_name(&tc),
                        false => tc.draw(gs::from_regex("[a-z][a-z0-9]{0,4}-[a-z0-9]{0,4}")),
                    };
                    let said = format!("{name:?} isn't a param's name: {PARAM_RULE}");
                    ((name, json!({ "type": "string" })), (String::new(), said))
                }
                "x-cli" => {
                    let (property, said) = match tc.draw(gs::integers::<u8>().max_value(4)) {
                        0 => {
                            let flag = bad_name(&tc);
                            (
                                json!({ "type": "string", "x-cli": { "flag": flag } }),
                                format!("its flag {flag:?} isn't a name: {NAME_RULE}"),
                            )
                        }
                        1 => {
                            let kind = tc.draw(gs::sampled_from(vec![
                                Kind::Strings,
                                Kind::Integer,
                                Kind::Number,
                                Kind::Boolean,
                                Kind::Object,
                            ]));
                            let mut property = kind.schema();
                            property["x-cli"] = json!({ "positional": true });
                            (property, "only a string is positional".into())
                        }
                        // A hint the command line no longer takes.
                        2 => (
                            json!({ "type": "object", "x-cli": { "one_based": true } }),
                            "unknown field `one_based`".into(),
                        ),
                        3 => (json!({ "type": "string", "x-cli": { "short": "s" } }), "unknown field `short`".into()),
                        _ => (json!({ "type": "string", "x-cli": 5 }), "invalid type: integer `5`".into()),
                    };
                    (("added_param".into(), property), (".x-cli".into(), said))
                }
                "positional twice" => {
                    for p in &mut drawn {
                        p.hints.positional = false;
                    }
                    let positional = json!({ "type": "string", "x-cli": { "positional": true } });
                    let mut s = schema(&drawn);
                    s["properties"]["added_param_one"] = positional.clone();
                    s["properties"]["added_param_two"] = positional;
                    *slot = Some(s);
                    return assert_eq!(
                        problem_with(&m, first_party),
                        format!("{at}.properties.added_param_two.x-cli: \"added_param_one\" is positional already")
                    );
                }
                _ => {
                    let property = match tc.draw(gs::booleans()) {
                        true => {
                            let kind = tc.draw(gs::sampled_from(vec![
                                Kind::Integer,
                                Kind::Number,
                                Kind::Boolean,
                                Kind::Object,
                            ]));
                            let mut property = kind.schema();
                            property["x-path"] = true.into();
                            property
                        }
                        false => {
                            let mut property = tc.draw(gs::sampled_from(vec![Kind::String, Kind::Strings])).schema();
                            property["x-path"] = tc.draw(gs::sampled_from(vec![json!("yes"), json!(1), json!(null)]));
                            property
                        }
                    };
                    (
                        ("added_param".into(), property),
                        (".x-path".into(), "is true or false, on a string or a list of them".into()),
                    )
                }
            };
            let ((name, property), (under, said)) = (property, said);
            let mut s = schema(&drawn);
            s["properties"][&name] = property;
            *slot = Some(s);
            (format!("{at}.properties.{name}{under}: {said}"), mistake == "x-cli")
        }
        "no fields" | "field name" | "field twice" => {
            add(c, "analyzers", if mistake == "field twice" { 2 } else { 1 });
            let i = index(&tc, c.analyzers.len());
            match mistake {
                "no fields" => {
                    c.analyzers[i].fields.clear();
                    (format!("contributes.analyzers[{i}].fields: an analyzer needs a field"), false)
                }
                "field name" => {
                    let name = bad_name(&tc);
                    c.analyzers[i].fields.insert(name.clone(), FieldType::String);
                    (format!("contributes.analyzers[{i}].fields.{name}: isn't a name: {NAME_RULE}"), false)
                }
                _ => {
                    let (i, j) = (i.max(1), index(&tc, i.max(1)));
                    let field = c.analyzers[j].fields.keys().next().unwrap().clone();
                    c.analyzers[i].fields.insert(field.clone(), FieldType::Boolean);
                    (format!("contributes.analyzers[{i}].fields.{field}: another analyzer has this field"), false)
                }
            }
        }
        "role field" | "role type" => {
            add(c, "analyzers", 1);
            let i = index(&tc, c.analyzers.len());
            let a = &mut c.analyzers[i];
            let role = tc.draw(gs::sampled_from(Role::ALL.to_vec()));
            let at = format!("contributes.analyzers[{i}].roles.{}", role.name());
            // Longer than any drawn name, so none of the analyzers' fields.
            let field = "added-role-field".to_string();
            if mistake == "role field" {
                a.roles.insert(role, field.clone());
                (format!("{at}: {field:?} isn't its field"), false)
            } else {
                let others = vec![FieldType::Number, FieldType::Integer, FieldType::String, FieldType::Boolean];
                let others: Vec<FieldType> = others.into_iter().filter(|t| *t != role.kind()).collect();
                a.fields.insert(field.clone(), tc.draw(gs::sampled_from(others)));
                a.roles.insert(role, field.clone());
                let kind = serde_json::to_value(role.kind()).unwrap();
                (format!("{at}: {field} isn't a {}, which the role is", kind.as_str().unwrap()), false)
            }
        }
        "file or folder" | "extension" => {
            add(c, "exporters", 1);
            let i = index(&tc, c.exporters.len());
            let x = &mut c.exporters[i];
            if mistake == "file or folder" {
                x.folder = tc.draw(gs::booleans());
                x.extension = x.folder.then(|| "x".into());
                (format!("contributes.exporters[{i}]: an exporter has either an extension or folder = true"), false)
            } else {
                let ext = match tc.draw(gs::integers::<u8>().max_value(2)) {
                    0 => String::new(),
                    1 => tc.draw(gs::from_regex("[a-z0-9]{0,3}[A-Z._/ -][a-z0-9]{0,3}")),
                    _ => "a".repeat(tc.draw(gs::integers::<usize>().min_value(17).max_value(24))),
                };
                (x.folder, x.extension) = (false, Some(ext.clone()));
                let said = format!("{ext:?} isn't an extension: lowercase letters and digits, without the dot");
                (format!("contributes.exporters[{i}].extension: {said}"), false)
            }
        }
        "net host" | "net twice" | "net rate" => {
            // Longer than any host drawn.
            let host = "added-host.example".to_string();
            let limit = |per_second| NetLimit { host: host.clone(), per_second };
            let i = m.net.len();
            match mistake {
                "net host" => {
                    m.net.push(limit(1.0));
                    (format!("net[{i}].host: net:{host} isn't one of its capabilities"), false)
                }
                "net twice" => {
                    m.capabilities.push(Capability::Net(host.clone()));
                    m.net.extend([limit(1.0), limit(2.0)]);
                    (format!("net[{}].host: {host:?} is limited twice", i + 1), false)
                }
                _ => {
                    m.capabilities.push(Capability::Net(host.clone()));
                    let rate = tc.draw(gs::sampled_from(vec![0.0, -1.0, f64::NAN, f64::INFINITY, 1.0 / 7200.0]));
                    m.net.push(limit(rate));
                    (format!("net[{i}].per_second: is how many requests a second, at least 1/3600"), false)
                }
            }
        }
        "account endpoint" | "account client" | "account scope" | "account pkce" => {
            add(c, "accounts", 1);
            if !m.capabilities.iter().any(|g| matches!(g, Capability::Net(_))) {
                m.capabilities.push(Capability::Net(ACCOUNT_HOST.into()));
            }
            let i = index(&tc, c.accounts.len());
            // Not indexed: CodeQL takes `accounts[i]` for an account's secrets, and changing its
            // scopes for logging them.
            let x = c.accounts.get_mut(i).expect("one at i");
            let at = format!("contributes.accounts[{i}]");
            match mistake {
                "account endpoint" => {
                    let odd = tc.draw(gs::sampled_from(vec![
                        "",
                        "auth.example/token",
                        "ftp://auth.example",
                        "https://",
                        "https:// auth.example",
                        "http://auth.example/token",
                        "http://127.0.0.1.example",
                        "http://127.0.0.1.evil.example/token",
                        "http://localhost@evil.example/token",
                        "http://localhost:80@evil.example/token",
                        "http://evil.example\\@localhost/token",
                        "http://localhost%2eevil.example/token",
                        "http://[::1].evil.example/token",
                        "https://someone:words@auth.example/token",
                        "https://auth.example:99999/token",
                        "javascript:alert(1)",
                    ]));
                    // An endpoint it may have, but with a user in it, is read as the client
                    // would: a user name (and maybe a password), never the host.
                    let user = tc.draw(gs::from_regex("[a-z0-9]{1,6}(:[a-z0-9]{0,6})?"));
                    let with_user = endpoint(&tc).replacen("://", &format!("://{user}@"), 1);
                    let wrong = if tc.draw(gs::booleans()) { with_user } else { odd.to_string() };
                    let key = tc.draw(gs::sampled_from(vec!["authorize_url", "token_url", "revoke_url"]));
                    match key {
                        "authorize_url" => x.authorize_url = wrong.clone(),
                        "token_url" => x.token_url = wrong.clone(),
                        _ => x.revoke_url = Some(wrong.clone()),
                    }
                    (format!("{at}.{key}: {wrong:?} isn't an https:// address without a user name or password"), false)
                }
                "account client" => {
                    x.client_id = tc.draw(gs::text().alphabet(" \t\r\n").max_size(3));
                    (format!("{at}.client_id: is empty"), false)
                }
                "account scope" => {
                    let wrong = tc.draw(gs::sampled_from(vec!["", "two words", " lead"]));
                    let j = tc.draw(gs::integers::<usize>().max_value(x.scopes.len()));
                    x.scopes.insert(j, wrong.into());
                    (format!("{at}.scopes[{j}]: {wrong:?} isn't a scope: a word, no spaces"), false)
                }
                _ => {
                    x.pkce = false;
                    (format!("{at}.pkce: is true: locallibrary signs in with PKCE only"), false)
                }
            }
        }
        "account grant" => {
            add(c, "accounts", 1);
            m.capabilities.retain(|g| !matches!(g, Capability::Net(_)));
            m.net.clear();
            ("contributes.accounts[0]: goes into net/fetch requests, which needs a net:<host> capability".into(), false)
        }
        "provider grant" => {
            add(c, "providers", 1);
            m.capabilities.retain(|g| *g != Capability::LibraryRead);
            ("contributes.providers[0]: is given tracks' tags, which needs the library:read capability".into(), false)
        }
        "no events" => {
            c.events.push(Listener { on: Vec::new() });
            (format!("contributes.events[{}].on: names no event", c.events.len() - 1), false)
        }
        "event twice" => {
            // One a listener has already, or one heard twice by a new listener.
            let heard: Vec<EventName> = c.events.iter().flat_map(|x| x.on.clone()).collect();
            let again = !heard.is_empty() && tc.draw(gs::booleans());
            let event = match again {
                true => tc.draw(gs::sampled_from(heard)),
                false => tc.draw(gs::sampled_from(EventName::ALL.to_vec())),
            };
            c.events.retain(|x| again || !x.on.contains(&event));
            if !m.capabilities.contains(&event.capability()) {
                m.capabilities.push(event.capability());
            }
            let on = if again { vec![event] } else { vec![event, event] };
            let at = format!("contributes.events[{}].on[{}]", c.events.len(), on.len() - 1);
            c.events.push(Listener { on });
            (format!("{at}: {} is listened for twice", event.name()), false)
        }
        "event grant" => {
            let event = tc.draw(gs::sampled_from(EventName::ALL.to_vec()));
            let needs = event.capability();
            // No other listener hears what it takes, so nothing else is refused first.
            c.events.iter_mut().for_each(|x| x.on.retain(|e| e.capability() != needs));
            c.events.retain(|x| !x.on.is_empty());
            m.capabilities.retain(|g| *g != needs);
            let at = format!("contributes.events[{}].on[0]", c.events.len());
            c.events.push(Listener { on: vec![event] });
            (format!("{at}: {} needs the {needs} capability", event.name()), false)
        }
        _ => {
            add(c, "settings", 1);
            let i = index(&tc, c.settings.len());
            let x = &mut c.settings[i];
            let at = format!("contributes.settings[{i}]");
            match mistake {
                "choices" => {
                    match x.kind {
                        SettingType::Enum => (x.choices, x.default) = (Vec::new(), None),
                        _ => x.choices = tc.draw(gs::vecs(gs::text()).min_size(1).max_size(3)),
                    }
                    (format!("{at}.choices: an enum setting has choices, and only an enum"), false)
                }
                "choice twice" => {
                    if x.kind != SettingType::Enum {
                        (x.kind, x.default) = (SettingType::Enum, None);
                        x.choices.push(tc.draw(gs::text()));
                    }
                    let again = x.choices[index(&tc, x.choices.len())].clone();
                    x.choices.push(again.clone());
                    (format!("{at}.choices: {again:?} is there twice"), false)
                }
                _ => {
                    let choices = x.choices.clone();
                    x.default = Some(match x.kind {
                        SettingType::String | SettingType::File | SettingType::Directory => number(&tc),
                        SettingType::Number => tc.draw(gs::text()).into(),
                        SettingType::Boolean => number(&tc),
                        SettingType::Enum => tc.draw(gs::text().filter(move |s: &String| !choices.contains(s))).into(),
                        SettingType::Secret => tc.draw(gs::text()).into(),
                    });
                    (format!("{at}.default: doesn't fit the setting's type (and a secret has none)"), false)
                }
            }
        }
    };
    let problem = problem_with(&m, first_party);
    match prefix {
        // Not echoed on failure (hegel shows what it drew): CodeQL takes a count of accounts in
        // it for an account's secrets.
        true => assert!(problem.starts_with(&said)),
        false => assert_eq!(problem, said),
    }
}

/// An absolute path on this computer, ending in `name`.
fn absolute(name: &str) -> String {
    if cfg!(windows) { format!("C:\\{name}") } else { format!("/{name}") }
}

#[hegel::test_helper]
fn file_name(tc: &TestCase) -> String {
    tc.draw(gs::from_regex("[a-z0-9 ._-]{1,8}(/[a-z0-9]{1,4})?"))
}

/// A value that fits `p`: files and folders absolute.
#[hegel::test_helper]
fn fitting(tc: &TestCase, p: &Param) -> Value {
    let string = |tc: &TestCase| match p.is_path() {
        true => absolute(&file_name(tc)),
        false => tc.draw(gs::text().max_size(8)),
    };
    match p.kind {
        Kind::String => string(tc).into(),
        Kind::Strings => (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| string(tc)).collect(),
        Kind::Integer => tc.draw(gs::integers::<i64>()).into(),
        Kind::Number => number(tc),
        Kind::Boolean => tc.draw(gs::booleans()).into(),
        Kind::Object => {
            let keys = tc.draw(gs::vecs(gs::text().max_size(4)).max_size(3));
            Value::Object(keys.into_iter().map(|k| (k, number(tc))).collect())
        }
    }
}

/// A value of another type than `p`'s, and where in it the first thing wrong is (from `p`).
#[hegel::test_helper]
fn misfit(tc: &TestCase, p: &Param) -> (Value, String) {
    let at = format!("/{}", p.name);
    let text = || Value::from(tc.draw(gs::text().max_size(4)));
    match p.kind {
        Kind::String | Kind::Boolean => (number(tc), at),
        Kind::Integer => match tc.draw(gs::booleans()) {
            true => ((tc.draw(gs::integers::<i32>()) as f64 + 0.5).into(), at),
            false => (text(), at),
        },
        Kind::Number | Kind::Object => (text(), at),
        Kind::Strings => {
            let mut list: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(2))).map(|_| text()).collect();
            let i = tc.draw(gs::integers::<usize>().max_value(list.len()));
            list.insert(i, number(tc));
            (list.into(), format!("{at}/{i}"))
        }
    }
}

/// Params that fit a drawn schema pass `check_params`. Broken in one place, they're refused as
/// invalid params saying where: a value of the wrong type or a relative path at its JSON pointer,
/// a missing param or params that aren't an object as a whole. Each param's hints and `x-path`
/// read back as the schema gives them.
#[hegel::test(test_cases = 80)]
fn params_fit_their_schema_or_are_refused_saying_where(tc: TestCase) {
    let mistake = tc.draw(gs::sampled_from(vec!["none", "wrong type", "missing", "relative path", "not an object"]));
    let mut drawn = params(&tc);
    // A param the mistake can be made with.
    let kind = match mistake {
        "relative path" => tc.draw(gs::sampled_from(vec![Kind::String, Kind::Strings])),
        _ => tc.draw(gs::sampled_from(KINDS.to_vec())),
    };
    let hints = CliHints::default();
    let path = Some(mistake == "relative path");
    drawn.push(Param { name: "added_param".into(), kind, required: mistake == "missing", x_path: path, hints });
    let schema = schema(&drawn);
    for p in &drawn {
        let property = &schema["properties"][&p.name];
        assert_eq!(CliHints::of(property), Ok(p.hints.clone()), "{property}");
        assert_eq!(is_path(property), p.is_path(), "{property}");
    }
    let mut params = Map::new();
    for p in &drawn {
        if p.required || tc.draw(gs::booleans()) {
            params.insert(p.name.clone(), fitting(&tc, p));
        }
    }
    let added = drawn.last().unwrap();
    let (params, at, said) = match mistake {
        "none" => {
            assert_eq!(check_params(Some(&schema), &params.into()), Ok(()));
            return;
        }
        "wrong type" => {
            let (value, at) = misfit(&tc, added);
            params.insert(added.name.clone(), value);
            (params.into(), Some(at), String::new())
        }
        "missing" => {
            params.shift_remove(&added.name);
            (params.into(), None, "\"added_param\" is a required property".into())
        }
        "relative path" => {
            let relative = tc.draw(gs::from_regex("[a-z0-9_-]{1,8}(/[a-z0-9]{1,4})?"));
            let value = match added.kind {
                Kind::String => relative.clone().into(),
                _ => {
                    let mut list: Vec<Value> = (0..tc.draw(gs::integers::<usize>().max_value(2)))
                        .map(|_| absolute(&file_name(&tc)).into())
                        .collect();
                    list.insert(tc.draw(gs::integers::<usize>().max_value(list.len())), relative.clone().into());
                    list.into()
                }
            };
            params.insert(added.name.clone(), value);
            (params.into(), Some("/added_param".into()), format!("{relative:?} isn't an absolute path"))
        }
        _ => {
            let other = vec![json!(null), json!([]), json!("params"), json!(1), json!(true), json!([{}])];
            (tc.draw(gs::sampled_from(other)), None, String::new())
        }
    };
    let e = check_params(Some(&schema), &params).unwrap_err();
    assert_eq!((e.code, e.kind()), (code::INVALID_PARAMS, Some("invalid_params")), "{e}");
    let fields = &e.data.as_ref().unwrap().fields;
    assert_eq!(fields.get("path").and_then(Value::as_str), at.as_deref(), "{e}");
    let start = match &at {
        Some(at) => format!("invalid params: {at}: "),
        None => "invalid params: ".into(),
    };
    assert!(e.message.starts_with(&start) && e.message.contains(&said), "{e}");
    // Without a schema, any object is fine and nothing else.
    assert_eq!(check_params(None, &params).is_ok(), params.is_object());
}

/// No schema takes any object, and nothing else; a schema that doesn't compile is the host's bug.
#[hegel::test(test_cases = 20)]
fn without_a_schema_params_are_any_object(tc: TestCase) {
    let p = Param {
        name: "x".into(),
        kind: tc.draw(gs::sampled_from(KINDS.to_vec())),
        required: false,
        x_path: None,
        hints: CliHints::default(),
    };
    let value = fitting(&tc, &p);
    assert_eq!(check_params(None, &json!({ "x": value })), Ok(()));
    let e = check_params(None, &value).map_err(|e| e.message);
    assert_eq!(e, if value.is_object() { Ok(()) } else { Err("invalid params: expected an object".into()) });
    let e = check_params(Some(&json!({ "type": 5 })), &json!({ "x": value })).unwrap_err();
    assert_eq!((e.code, e.kind()), (code::INTERNAL, Some("internal")));
}

/// One of each kind of field value.
#[hegel::test_helper]
fn field_value(tc: &TestCase) -> FieldValue {
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => FieldValue::Integer(tc.draw(gs::integers())),
        1 => FieldValue::Number(tc.draw(gs::floats::<f64>().allow_nan(false).allow_infinity(false))),
        2 => FieldValue::Boolean(tc.draw(gs::booleans())),
        3 => FieldValue::String(tc.draw(gs::text())),
        4 => FieldValue::Strings(tc.draw(gs::vecs(gs::text()).max_size(3))),
        _ => FieldValue::Null,
    }
}

/// A field's value is accepted by its own type alone (an integer by a number's too, null by any),
/// and reads back from JSON as the same kind of value.
#[hegel::test(test_cases = 100)]
fn a_value_fits_its_field_type_alone(tc: TestCase) {
    let value = field_value(&tc);
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(serde_json::from_str::<FieldValue>(&json).unwrap(), value, "{json}");
    let own = match value {
        FieldValue::Integer(_) => Some(FieldType::Integer),
        FieldValue::Number(_) => Some(FieldType::Number),
        FieldValue::Boolean(_) => Some(FieldType::Boolean),
        FieldValue::String(_) => Some(FieldType::String),
        FieldValue::Strings(_) => Some(FieldType::Strings),
        FieldValue::Null => None,
    };
    for kind in [FieldType::Number, FieldType::Integer, FieldType::String, FieldType::Boolean, FieldType::Strings] {
        let fits = own.is_none_or(|own| kind == own || (kind, own) == (FieldType::Number, FieldType::Integer));
        assert_eq!(kind.accepts(&value), fits, "{kind:?} {value:?}");
    }
}

/// A setting takes a value of its type (an enum, one of its choices; a secret, text that isn't
/// empty) and nothing else.
#[hegel::test(test_cases = 100)]
fn a_setting_takes_values_of_its_type_alone(tc: TestCase) {
    let s = setting(&tc, "s".into());
    let value: Value = match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => Value::Null,
        1 => tc.draw(gs::booleans()).into(),
        2 => number(&tc),
        3 if !s.choices.is_empty() => tc.draw(gs::sampled_from(s.choices.clone())).into(),
        3 | 4 => tc.draw(gs::text().max_size(6)).into(),
        5 => json!([tc.draw(gs::text().max_size(6))]),
        _ => json!({ "value": tc.draw(gs::booleans()) }),
    };
    let fits = match s.kind {
        SettingType::String | SettingType::File | SettingType::Directory => value.is_string(),
        SettingType::Number => value.is_number(),
        SettingType::Boolean => value.is_boolean(),
        SettingType::Enum => s.choices.iter().any(|c| value == c.as_str()),
        SettingType::Secret => value.as_str().is_some_and(|v| !v.is_empty()),
    };
    assert_eq!(s.accepts(&value), fits, "{s:?} {value}");
}

/// A capability's name reads back as the capability, and any other string is refused saying
/// what it may be. In a manifest, the same.
#[hegel::test(test_cases = 100)]
fn capabilities_read_back_from_their_names_and_nothing_else(tc: TestCase) {
    let named = [
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
        "events:drives",
        "events:jobs",
        "events:plans",
    ];
    let s = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => tc.draw(gs::sampled_from(named.to_vec())).to_string(),
        1 => format!("net:{}", tc.draw(gs::from_regex(r"(\*\.)?[A-Za-z0-9.-]{0,12}"))),
        2 => format!("net:{}", tc.draw(gs::text().max_size(12))),
        _ => tc.draw(gs::text().max_size(16)),
    };
    let parsed = s.parse::<Capability>();
    let host = s.strip_prefix("net:");
    // A host's name, or `*.` and a domain's.
    let named_host = |h: &str| {
        let h = h.strip_prefix("*.").unwrap_or(h);
        !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
    };
    match parsed {
        Ok(Capability::Net(ref h)) => {
            assert_eq!(Some(h.as_str()), host);
            assert!(named_host(h), "{s:?}");
        }
        Ok(ref c) => assert!(named.contains(&s.as_str()) && c.to_string() == s, "{s:?}"),
        Err(ref e) if host.is_some() => {
            assert_eq!(*e, format!("{s:?} names no host: net:<host> or net:*.<domain>, like net:musicbrainz.org"));
            assert!(!named_host(host.unwrap()), "{s:?}");
        }
        Err(ref e) => {
            assert!(!named.contains(&s.as_str()), "{s:?}");
            let known = named.join(", ");
            assert_eq!(*e, format!("unknown capability {s:?}: one of {known} or net:<host>"));
        }
    }
    if let Ok(c) = &parsed {
        assert_eq!(c.to_string(), s);
    }
    let toml = format!("{HEAD}capabilities = [{}]\n", inline(&s.as_str().into()));
    match (Manifest::parse(&toml), parsed) {
        (Ok(m), Ok(c)) => assert_eq!(m.capabilities, [c]),
        (Err(e), Err(said)) => assert!(e.to_string().contains(&said), "{e}"),
        (m, c) => panic!("{m:?} {c:?}"),
    }
}
