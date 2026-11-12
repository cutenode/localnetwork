use hegel::TestCase;
use hegel::generators::{self as gs, Generator};
use indexmap::IndexMap;
use serde_json::{Map, json};
use url::Url;

use super::*;
use crate::hooks::account_types::{AUTHORIZE_PARAMS, METHODS};
use crate::{
    AccountNeed, AccountType, Analyzer, Check, Command, Destination, DestinationKind, EventName, ExportFrom,
    ExportSources, Exporter, FieldDisplay, FieldFormat, FieldType, FieldValue, ImportKind, Importer, Key,
    LibrarySource, Listener, Mode, Operation, OptionalScopes, Panel, Provider, ProviderKind, Role, SUBJECT_PLACEHOLDER,
    Scope, ScopeRequest, Service, SignIn, Theme, TokenPlace, When, Whoami, account_placeholder, code, not_plain_https,
};

const HEAD: &str = "id = \"demo\"\nname = \"Demo\"\nversion = \"0.1.0\"\nprotocol = \"^0.2\"\n";

const ECHO: &str = include_str!("../../examples/echo/locallibrary-plugin.toml");

/// The capabilities a third-party plugin may have (`net:` aside).
fn third_party_capabilities() -> Vec<Capability> {
    Capability::NAMED.into_iter().filter(|c| *c != Capability::LibraryDeleteUnbacked).collect()
}

/// What's wrong with `HEAD` followed by `rest`, for a third-party plugin (`""`: nothing).
fn problem(rest: &str) -> String {
    match Manifest::parse(&format!("{HEAD}{rest}")).and_then(|m| m.validate(false)) {
        Ok(()) => String::new(),
        Err(e) => e.to_string(),
    }
}

/// A field of type `kind`, as a manifest's problems say it: "an integer", "a number".
fn a_field_of(kind: FieldType) -> String {
    match serde_json::to_value(kind).unwrap().as_str().unwrap() {
        "integer" => "an integer".into(),
        name => format!("a {name}"),
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

#[hegel::test(test_cases = 8)]
fn only_built_in_plugins_delete_without_a_backup(tc: TestCase) {
    let mut capabilities = tc.draw(gs::subsequences(third_party_capabilities()));
    let at = tc.draw(gs::integers::<usize>().max_value(capabilities.len()));
    capabilities.insert(at, Capability::LibraryDeleteUnbacked);
    let names: Vec<String> = capabilities.iter().map(|c| format!("{:?}", c.to_string())).collect();
    let m = Manifest::parse(&format!("{HEAD}capabilities = [{}]\n", names.join(", "))).unwrap();
    m.validate(true).unwrap();
    let e = m.validate(false).unwrap_err().to_string();
    assert_eq!(e, format!("capabilities[{at}]: only plugins locallibrary ships delete without a backup"));
}

#[test]
fn only_built_in_plugins_add_cli_commands() {
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
fn only_built_in_plugins_give_operations_a_cli_place() {
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
fn property_cli_hints_are_checked() {
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
    assert_eq!(
        m.validate(false).unwrap_err().to_string(),
        format!("protocol: \"^0.1\" doesn't include {PROTOCOL_VERSION}")
    );
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
fn unknown_keys_are_left_out_and_refused_only_built_in() {
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
    // Plugins once had accounts of their own, which they now ask for by type: one still declaring
    // one is told where.
    let own = Manifest::parse(&format!("{HEAD}[[contributes.accounts]]\nid = \"main\"\ntitle = \"Main\"\n")).unwrap();
    assert_eq!(own.validate(true).unwrap_err().to_string(), "contributes.accounts: isn't a key locallibrary knows");
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

/// A check runs with the check before a set unless it says when, and until it's turned off unless
/// it says it's off to start with.
#[test]
fn a_check_runs_with_the_check_and_is_on_unless_it_says() {
    let m = Manifest::parse(&format!("{HEAD}[[contributes.checks]]\nname = \"c\"\ntitle = \"C\"\n")).unwrap();
    assert_eq!(
        m.contributes.checks,
        [Check { name: "c".into(), title: "C".into(), default_on: true, when: vec![When::Check] }]
    );
    let when = |when: &str| problem(&format!("[[contributes.checks]]\nname = \"c\"\ntitle = \"C\"\nwhen = {when}\n"));
    assert_eq!(when(r#"["apply", "check"]"#), "");
    assert_eq!(when("[]"), "contributes.checks[0].when: names no time: check, apply or both");
    assert!(when(r#"["later"]"#).contains("unknown variant `later`"));
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

/// An account plugin as locallibrary ships one: Google, with a scope for uploading videos.
const GOOGLE: &str = r#"
[[contributes.account_types]]
type = "google"
title = "Google"
authorize_url = "https://accounts.google.com/o/oauth2/v2/auth"
token_exchange = "https://localnetwork.computer/api/token-exchange/google"
client_id = "local-network.apps.googleusercontent.com"
revoke_url = "https://oauth2.googleapis.com/revoke"
always = ["openid", "email"]
whoami = { url = "https://openidconnect.googleapis.com/v1/userinfo", subject = "/sub", name = "/email" }
one_account = { login_hint = "{subject}" }
another_account = { prompt = "select_account" }
params = { access_type = "offline" }

[contributes.account_types.scopes.youtube-upload]
title = "upload videos to YouTube"
asks = ["https://www.googleapis.com/auth/youtube.upload"]
requests = [{ method = "POST", url = "https://www.googleapis.com/upload/youtube/v3/videos*" }]
"#;

/// What's wrong with [`GOOGLE`] once `change` has been made to it, for a built-in plugin.
fn google_with(change: impl FnOnce(&mut Manifest)) -> String {
    let mut m = Manifest::parse(&format!("{HEAD}{GOOGLE}")).unwrap();
    change(&mut m);
    problem_with(&m, true)
}

/// An account plugin provides one account type and nothing else, and uses Local Network's token
/// exchange only when locallibrary ships it.
#[test]
fn an_account_plugin_provides_one_account_type_and_nothing_else() {
    let m = Manifest::parse(&format!("{HEAD}{GOOGLE}")).unwrap();
    m.validate(true).unwrap();
    assert!(m.is_account_plugin());
    assert!(!Manifest::parse(ECHO).unwrap().is_account_plugin());
    let google = &m.contributes.account_types[0];
    assert_eq!((google.name.as_str(), &google.token, google.redirect_port), ("google", &TokenPlace::default(), None));
    assert_eq!(
        (google.one_account["login_hint"].as_str(), google.params["access_type"].as_str()),
        ("{subject}", "offline")
    );
    assert_eq!(google.scopes["youtube-upload"].asks, ["https://www.googleapis.com/auth/youtube.upload"]);
    let at = "contributes.account_types[0]";
    assert_eq!(
        problem_with(&m, false),
        format!(
            "{at}.token_exchange: is only for account plugins locallibrary ships: it signs people in as Local Network"
        )
    );
    let own_client = google_with(|m| {
        let google = &mut m.contributes.account_types[0];
        (google.token_url, google.token_exchange) = (Some("https://oauth2.googleapis.com/token".into()), None);
    });
    assert_eq!(own_client, "");
    let alone = format!(
        "{at}: makes an account plugin, which has nothing else: no capabilities, accounts, net limits or other \
         contributions"
    );
    assert_eq!(google_with(|m| m.capabilities.push(Capability::Net("googleapis.com".into()))), alone);
    let need = AccountNeed {
        name: "google".into(),
        scopes: vec!["youtube-upload".into()],
        why: "To upload".into(),
        optional: Vec::new(),
    };
    assert_eq!(google_with(|m| m.accounts.push(need)), alone);
    let limit = NetLimit { host: "googleapis.com".into(), per_second: 1.0 };
    assert_eq!(google_with(|m| m.net.push(limit)), alone);
    for kind in CONTRIBUTIONS {
        assert_eq!(google_with(|m| add(&mut m.contributes, kind, 1)), alone, "{kind}");
    }
    let twice = google_with(|m| m.contributes.account_types.push(m.contributes.account_types[0].clone()));
    assert_eq!(twice, "contributes.account_types[1]: is a second account type: an account plugin provides one");
}

/// An account type's endpoints are read as the HTTP client reads them: https, or http to this
/// computer itself, never with a user name or password, whatever the address looks like.
#[test]
fn an_account_types_endpoints_are_read_as_the_client_reads_them() {
    let token_url = |url: &str| {
        let url = url.to_string();
        google_with(move |m| {
            let google = &mut m.contributes.account_types[0];
            (google.token_url, google.token_exchange) = (Some(url), None);
        })
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
            "contributes.account_types[0].token_url: {url:?} isn't an https:// address without a user name or \
             password"
        );
        assert_eq!(token_url(url), said, "{url}");
    }
}

/// An account type trades its tokens at its service's token endpoint or at Local Network's token
/// exchange, one of them, and an exchange is Local Network's (or a test server on this computer at
/// the same path): no other server is sent the user's codes in its name.
#[test]
fn an_account_type_trades_tokens_at_its_token_endpoint_or_local_networks_exchange_only() {
    let at = "contributes.account_types[0]";
    let trading = |token_url: Option<&str>, token_exchange: Option<&str>| {
        let (token_url, token_exchange) = (token_url.map(String::from), token_exchange.map(String::from));
        google_with(move |m| {
            let google = &mut m.contributes.account_types[0];
            (google.token_url, google.token_exchange) = (token_url, token_exchange);
        })
    };
    for url in [
        "https://localnetwork.computer/api/token-exchange/soundcloud",
        "https://localnetwork.computer:443/api/token-exchange/google",
        "https://LocalNetwork.computer/api/token-exchange/mixcloud-2",
        "http://127.0.0.1:5173/api/token-exchange/soundcloud",
        "http://localhost/api/token-exchange/x",
    ] {
        assert_eq!(trading(None, Some(url)), "", "{url}");
    }
    for url in [
        "https://evil.example/api/token-exchange/soundcloud",
        "https://localnetwork.computer.evil.example/api/token-exchange/soundcloud",
        "https://localnetwork.computer:8443/api/token-exchange/soundcloud",
        "http://localnetwork.computer/api/token-exchange/soundcloud",
        "https://someone@localnetwork.computer/api/token-exchange/soundcloud",
        "https://localnetwork.computer/api/token-exchange/",
        "https://localnetwork.computer/api/token-exchange/SoundCloud",
        "https://localnetwork.computer/api/token-exchange/a/b",
        "https://localnetwork.computer/api/token-exchange/../oauth/token",
        "https://localnetwork.computer/api/token-exchange/soundcloud?to=evil.example",
        "https://localnetwork.computer/api/token-exchange/soundcloud#x",
        "https://localnetwork.computer/oauth/token",
    ] {
        let said = format!(
            "{at}.token_exchange: {url:?} isn't Local Network's token exchange \
             (https://localnetwork.computer/api/token-exchange/<service>)"
        );
        assert_eq!(trading(None, Some(url)), said, "{url}");
    }
    assert_eq!(trading(None, None), format!("{at}.token_url: is missing, and so is token_exchange"));
    let both = trading(Some("https://auth.example/token"), Some("https://localnetwork.computer/api/token-exchange/x"));
    assert_eq!(both, format!("{at}.token_exchange: goes in token_url's place, not beside it"));
}

/// An account type's token goes in the `Authorization` header as `Bearer` unless it names another
/// scheme or a query parameter, one of them, and the browser comes back to any port unless it
/// names one a program may listen on.
#[test]
fn an_account_type_says_where_its_token_goes_and_where_the_browser_comes_back() {
    let google = |rest: &str| {
        let manifest = GOOGLE.replacen("type = \"google\"\n", &format!("type = \"google\"\n{rest}"), 1);
        Manifest::parse(&format!("{HEAD}{manifest}")).and_then(|m| {
            m.validate(true)?;
            let google = &m.contributes.account_types[0];
            Ok((google.token.clone(), google.redirect_port))
        })
    };
    let bearer = TokenPlace::Scheme("Bearer".into());
    assert_eq!(google("").unwrap(), (bearer.clone(), None));
    assert_eq!(google("token = { scheme = \"Bearer\" }\n").unwrap(), (bearer, None));
    let oauth = google("token = { scheme = \"OAuth\" }\nredirect_port = 8765\n").unwrap();
    assert_eq!(oauth, (TokenPlace::Scheme("OAuth".into()), Some(8765)));
    let query = google("token = { query = \"access_token\" }\n").unwrap();
    assert_eq!(query, (TokenPlace::Query("access_token".into()), None));
    let said = |rest: &str| google(rest).unwrap_err().to_string();
    let at = "contributes.account_types[0]";
    assert_eq!(
        said("token = { scheme = \"Bearer token\" }\n"),
        format!("{at}.token.scheme: \"Bearer token\" isn't a scheme: one word, like Bearer or OAuth")
    );
    assert_eq!(
        said("token = { query = \"a=b\" }\n"),
        format!("{at}.token.query: \"a=b\" isn't a query parameter's name: letters, digits, -, ., _ and ~")
    );
    assert!(said("token = { scheme = \"OAuth\", query = \"access_token\" }\n").contains("token"));
    assert!(said("token = { header = \"X-Token\" }\n").contains("unknown variant `header`"));
    assert!(said("token = \"OAuth\"\n").contains("token"));
    assert_eq!(said("redirect_port = 80\n"), format!("{at}.redirect_port: is 1024 or more: 80 is a system service's"));
    assert!(said("redirect_port = 70000\n").contains("redirect_port"));
}

/// An account plugin for the Internet Archive's S3 API, signed in to with a pair of keys.
const ARCHIVE: &str = r#"
[[contributes.account_types]]
type = "archive"
title = "Internet Archive"
kind = "keys"
keys_url = "https://archive.org/account/s3.php"
keys = { access = { title = "S3 access key" }, secret = { title = "S3 secret key" } }
authorization = "LOW {access}:{secret}"

[contributes.account_types.scopes.upload]
title = "upload items to the Internet Archive"
requests = [{ method = "PUT", url = "https://s3.us.archive.org/*" }]
"#;

/// What's wrong with [`ARCHIVE`] once `change` has been made to it, for a third-party plugin.
fn archive_with(change: impl FnOnce(&mut AccountType)) -> String {
    let mut m = Manifest::parse(&format!("{HEAD}{ARCHIVE}")).unwrap();
    change(&mut m.contributes.account_types[0]);
    problem_with(&m, false)
}

/// An account type signed in to with keys names them, where the user gets them and the
/// `Authorization` header's value they make, which names every key in braces and nothing else:
/// they go nowhere else, never the query. It has none of the fields of one signed in to in the
/// browser, which has none of its, and its scopes ask the service for nothing; its whoami, when
/// it has one, is at a host its scopes' requests go to.
#[test]
fn a_keys_account_type_names_its_keys_and_the_header_they_make() {
    let m = Manifest::parse(&format!("{HEAD}{ARCHIVE}")).unwrap();
    m.validate(false).unwrap();
    assert!(m.is_account_plugin());
    let archive = &m.contributes.account_types[0];
    assert_eq!(archive.kind, SignIn::Keys);
    let key = |name: &str| if name == "access" { "AK" } else { "SK" };
    let place = archive.place();
    let header = Some("Authorization".to_string());
    assert_eq!(
        (place.header.clone(), place.query.clone(), place.filled(key)),
        (header.clone(), None, "LOW AK:SK".into())
    );
    let google = Manifest::parse(&format!("{HEAD}{GOOGLE}")).unwrap().contributes.account_types[0].clone();
    let token = |place: TokenPlace| {
        let p = AccountType { token: place, ..google.clone() }.place();
        (p.header.clone(), p.query.clone(), p.filled(|_| "t"))
    };
    assert_eq!(token(TokenPlace::default()), (header.clone(), None, "Bearer t".into()));
    assert_eq!(token(TokenPlace::Scheme("OAuth".into())), (header, None, "OAuth t".into()));
    assert_eq!(token(TokenPlace::Query("access_token".into())), (None, Some("access_token".into()), "t".into()));

    let at = "contributes.account_types[0]";
    let browser = "is for an account type signed in to in the browser, not with keys";
    assert_eq!(archive_with(|x| x.client_id = Some("x".into())), format!("{at}.client_id: {browser}"));
    assert_eq!(archive_with(|x| x.token = TokenPlace::Scheme("OAuth".into())), format!("{at}.token: {browser}"));
    assert_eq!(archive_with(|x| x.token = TokenPlace::default()), "");
    let keyed = "is for an account type signed in to with keys (kind = \"keys\")";
    let keys_url = google_with(|m| m.contributes.account_types[0].keys_url = Some("https://a.example/".into()));
    assert_eq!(keys_url, format!("{at}.keys_url: {keyed}"));
    let authorization = google_with(|m| m.contributes.account_types[0].authorization = Some("LOW {token}".into()));
    assert_eq!(authorization, format!("{at}.authorization: {keyed}"));
    assert_eq!(
        archive_with(|x| x.keys.clear()),
        format!("{at}.keys: an account type signed in to with keys has a key at least")
    );
    assert_eq!(archive_with(|x| x.keys_url = None), format!("{at}.keys_url: is missing"));
    assert_eq!(archive_with(|x| x.authorization = None), format!("{at}.authorization: is missing"));
    // Keys can't be put in the query: a type saying so says nothing about the header.
    let query = ARCHIVE
        .replace("authorization = \"LOW {access}:{secret}\"", "goes = [{ query = \"k\", value = \"{access}\" }]");
    let m = Manifest::parse(&format!("{HEAD}{query}")).unwrap();
    assert_eq!(problem_with(&m, false), format!("{at}.authorization: is missing"));
    for (value, why) in [
        ("LOW {access}:{secret}\n", "isn't printable ASCII"),
        ("LOW {access}:{secret", "has a { or } that doesn't name a key"),
        ("LOW {access}:{Secret}", "has a { or } that doesn't name a key"),
        ("LOW {access}:{secret}}", "has a { or } that doesn't name a key"),
        ("LOW {access}:{secrets}", "names {secrets}, which isn't one of its keys"),
        ("LOW", "names none of its keys, like {key}"),
    ] {
        let said = archive_with(|x| x.authorization = Some(value.into()));
        assert_eq!(said, format!("{at}.authorization: {value:?} {why}"), "{value}");
    }
    let unused = archive_with(|x| x.authorization = Some("LOW {access}:{access}".into()));
    assert_eq!(unused, format!("{at}.keys.secret: goes nowhere: authorization doesn't name it"));
    let asks = archive_with(|x| x.scopes["upload"].asks.push("upload".into()));
    let why = "asks the service for nothing: an account signed in to with keys was granted nothing there";
    assert_eq!(asks, format!("{at}.scopes.upload.asks: {why}"));
    let whoami = |url: &str| {
        let w = Whoami { url: url.into(), subject: "/id".into(), name: "/name".into() };
        archive_with(|x| x.whoami = Some(w))
    };
    assert_eq!(whoami("https://s3.us.archive.org/?check_limit=1"), "");
    assert_eq!(
        whoami("https://archive.org/whoami"),
        format!("{at}.whoami.url: archive.org isn't a host its scopes' requests go to: it's sent the keys")
    );
    let kind = Manifest::parse(&format!("{HEAD}{}", ARCHIVE.replace("\"keys\"", "\"password\""))).unwrap_err();
    assert!(kind.to_string().contains("unknown variant `password`"), "{kind}");
}

/// An account type sets only the authorize params that can't change where a code goes or what it's
/// for, each in one place, and names the account only as `{subject}`, only to sign in to it again.
#[test]
fn an_account_types_params_name_the_account_only_as_its_subject() {
    let at = "contributes.account_types[0]";
    let one = |key: &str, value: &str| {
        google_with(|m| _ = m.contributes.account_types[0].one_account.insert(key.into(), value.into()))
    };
    let another = |key: &str, value: &str| {
        google_with(|m| _ = m.contributes.account_types[0].another_account.insert(key.into(), value.into()))
    };
    let every = |key: &str, value: &str| {
        google_with(|m| _ = m.contributes.account_types[0].params.insert(key.into(), value.into()))
    };
    assert_eq!(one("login_hint", "id:{subject}{subject}"), "");
    assert_eq!(another("hd", "example.com"), "");
    assert_eq!(every("hd", "example.com"), "");
    for (key, which) in [("login_hint", "one_account"), ("prompt", "another_account")] {
        assert_eq!(every(key, "x"), format!("{at}.params.{key}: is in {which} too: set it in one place"));
    }
    let said = "isn't one of the params an account type may set: login_hint, prompt, hd, access_type";
    for key in [
        "client_id",
        "redirect_uri",
        "response_type",
        "scope",
        "state",
        "code_challenge",
        "code_challenge_method",
        "request",
        "request_uri",
        "response_mode",
        "nonce",
        "claims",
        "a=b",
    ] {
        assert_eq!(one(key, "x"), format!("{at}.one_account.{key}: {said}"));
        assert_eq!(another(key, "x"), format!("{at}.another_account.{key}: {said}"));
        assert_eq!(every(key, "x"), format!("{at}.params.{key}: {said}"));
    }
    for value in ["{email}", "{subject", "{sub{subject}ject}", "}"] {
        let said = format!("{at}.one_account.login_hint: {value:?} has a {{ or }} besides {{subject}}");
        assert_eq!(one("login_hint", value), said);
    }
    assert_eq!(
        another("login_hint", "{subject}"),
        format!("{at}.another_account.login_hint: \"{{subject}}\" has a {{ or }}: only one_account names an account")
    );
}

/// Who signed in is read from the whoami endpoint's answer at JSON pointers (RFC 6901).
#[test]
fn whoami_says_where_in_its_answer_with_json_pointers() {
    let at = "contributes.account_types[0].whoami";
    let subject = |pointer: &str| {
        google_with(|m| m.contributes.account_types[0].whoami.as_mut().unwrap().subject = pointer.into())
    };
    for pointer in ["", "/", "/sub", "/a~0b~1c/0", "/~01"] {
        assert_eq!(subject(pointer), "", "{pointer}");
    }
    for pointer in ["sub", "#/sub", "/a~2", "/a~", "/~/b"] {
        assert_eq!(subject(pointer), format!("{at}.subject: {pointer:?} isn't a JSON pointer, like /sub"));
    }
    let name = google_with(|m| m.contributes.account_types[0].whoami.as_mut().unwrap().name = "email".into());
    assert_eq!(name, format!("{at}.name: \"email\" isn't a JSON pointer, like /sub"));
    let url =
        google_with(|m| m.contributes.account_types[0].whoami.as_mut().unwrap().url = "http://example.com/me".into());
    let said = "\"http://example.com/me\" isn't an https:// address without a user name or password";
    assert_eq!(url, format!("{at}.url: {said}"));
}

/// A scope is named, titled, asks for the service's scopes as OAuth 2 writes them, each once, and
/// covers some requests, each a method and an address written as it's read.
#[test]
fn a_scope_names_the_requests_it_covers_as_they_are_read() {
    let at = "contributes.account_types[0].scopes";
    let scope =
        |change: fn(&mut Scope)| google_with(|m| change(&mut m.contributes.account_types[0].scopes["youtube-upload"]));
    let at_scope = format!("{at}.youtube-upload");
    assert_eq!(scope(|s| s.title = " ".into()), format!("{at_scope}.title: is empty"));
    assert_eq!(scope(|s| s.asks.clear()), "");
    let word = "isn't a scope: printable ASCII without spaces, \" or \\";
    assert_eq!(scope(|s| s.asks.push("a b".into())), format!("{at_scope}.asks[1]: \"a b\" {word}"));
    assert_eq!(scope(|s| s.asks.push("a\"b".into())), format!("{at_scope}.asks[1]: \"a\\\"b\" {word}"));
    let twice = scope(|s| s.asks.push(s.asks[0].clone()));
    assert_eq!(twice, format!("{at_scope}.asks[1]: \"https://www.googleapis.com/auth/youtube.upload\" is there twice"));
    let always = google_with(|m| m.contributes.account_types[0].always.push("email".into()));
    assert_eq!(always, "contributes.account_types[0].always[2]: \"email\" is there twice");
    assert_eq!(scope(|s| s.requests.clear()), format!("{at_scope}.requests: a scope covers a request at least"));
    let get = scope(|s| s.requests[0].method = "get".into());
    assert_eq!(get, format!("{at_scope}.requests[0].method: \"get\" isn't one of GET, HEAD, POST, PUT, PATCH, DELETE"));
    let named = google_with(|m| {
        let google = &mut m.contributes.account_types[0];
        google.scopes = google.scopes.drain(..).map(|(_, s)| ("Upload".to_string(), s)).collect();
    });
    assert_eq!(named, format!("{at}.Upload: isn't a name: {NAME_RULE}"));
    let pattern = |url: &str| {
        let url = url.to_string();
        google_with(move |m| m.contributes.account_types[0].scopes["youtube-upload"].requests[0].url = url)
    };
    for url in ["https://a.example/", "https://a.example/*", "https://a.example/v1?x=1*", "http://127.0.0.1:8080/u"] {
        assert_eq!(pattern(url), "", "{url}");
    }
    let rule = "isn't an https:// address without a user name, password or fragment, maybe ending in *";
    for url in ["http://a.example/*", "https://u@a.example/*", "https://a.example/*/v", "https://a.example/#x", "*"] {
        assert_eq!(pattern(url), format!("{at_scope}.requests[0].url: {url:?} {rule}"), "{url}");
    }
    for (url, read) in [
        ("https://a.example*", "https://a.example/*"),
        ("https://a.example:443/v", "https://a.example/v"),
        ("https://A.example/", "https://a.example/"),
        ("https://a.example/v/../w*", "https://a.example/w*"),
    ] {
        let said = format!("{at_scope}.requests[0].url: {url:?} is read as {read:?}: write it that way");
        assert_eq!(pattern(url), said);
    }
}

/// A scope covers a request with one of its methods, in any case, to one of its addresses, or to
/// one starting with it when it ends in `*`: never to a host whose name starts the same, with a
/// user name, or to another path.
#[test]
fn a_scope_covers_the_requests_it_names_and_no_others() {
    let upload = |url: &str| ScopeRequest { method: "POST".into(), url: url.into() };
    let scope = Scope {
        title: "Upload".into(),
        asks: Vec::new(),
        requests: vec![upload("https://a.example/upload/*"), upload("https://a.example/me")],
    };
    let covers = |method: &str, url: &str| scope.covers(method, &Url::parse(url).unwrap());
    assert!(covers("POST", "https://a.example/upload/"));
    assert!(covers("post", "https://a.example/upload/videos?part=snippet"));
    assert!(covers("POST", "https://A.example:443/upload/x"));
    assert!(covers("POST", "https://a.example/me"));
    assert!(!covers("GET", "https://a.example/upload/x"));
    assert!(!covers("POST", "https://a.example/me/more"));
    assert!(!covers("POST", "https://a.example/upload"));
    assert!(!covers("POST", "https://a.example.evil/upload/x"));
    assert!(!covers("POST", "https://a.example@evil.example/upload/x"));
    assert!(!covers("POST", "https://someone@a.example/upload/x"));
    assert!(!covers("POST", "https://a.example/uploads/x"));
    assert!(!covers("POST", "https://a.example/elsewhere/../upload2/x"));
    assert!(!covers("POST", "http://a.example/upload/x"));
}

/// A library source reads files in the folders the user grants it: it's granted that.
#[test]
fn a_source_is_granted_reading_folders() {
    let source = |grants: &str| {
        problem(&format!("capabilities = [{grants}]\n[[contributes.sources]]\nname = \"s\"\ntitle = \"S\"\n"))
    };
    assert_eq!(source("\"fs:read\""), "");
    let said = "contributes.sources[0]: reads files in the folders it's granted, which needs the fs:read capability";
    assert_eq!(source("\"library:read\""), said);
    assert_eq!(source(""), said);
    assert_eq!("fs:read".parse::<Capability>().unwrap(), Capability::FsRead);
    assert_eq!(Capability::FsRead.to_string(), "fs:read");
}

#[test]
fn exporters_write_a_file_or_a_folder() {
    let both = "[[contributes.exporters]]\nformat = \"f\"\ntitle = \"F\"\nextension = \"x\"\nfolder = true\n";
    assert_eq!(problem(both), "contributes.exporters[0]: an exporter has either an extension or folder = true");
    let neither = "[[contributes.exporters]]\nformat = \"f\"\ntitle = \"F\"\n";
    assert_eq!(problem(neither), "contributes.exporters[0]: an exporter has either an extension or folder = true");
}

#[test]
fn an_exporter_of_sets_or_mixes_reads_sets() {
    // `source` as TOML has it: one name, or a list.
    let exporter = |source: &str, grants: &str| {
        problem(&format!(
            "capabilities = [{grants}]\n[[contributes.exporters]]\nformat = \"f\"\ntitle = \"F\"\nfolder = true\n\
             source = {source}\n"
        ))
    };
    assert_eq!(exporter("\"drive\"", ""), "");
    assert_eq!(exporter("\"set\"", "\"sets:read\""), "");
    assert_eq!(exporter("[\"drive\", \"set\", \"mix\"]", "\"sets:read\""), "");
    assert_eq!(exporter("[\"drive\"]", ""), "");
    let m =
        Manifest::parse(&format!("{HEAD}[[contributes.exporters]]\nformat = \"f\"\ntitle = \"F\"\nfolder = true\n"));
    assert_eq!(m.unwrap().contributes.exporters[0].sources(), [ExportFrom::Drive], "unless it says");
    let said =
        |what: &str| format!("contributes.exporters[0].source: reads {what}s, which needs the sets:read capability");
    assert_eq!(exporter("\"set\"", ""), said("set"));
    assert_eq!(exporter("\"mix\"", "\"library:read\""), said("mix"));
    assert_eq!(exporter("[\"drive\", \"mix\"]", ""), said("mix"));
    let wrong = |why: &str| format!("contributes.exporters[0].source: {why}");
    assert_eq!(exporter("[]", ""), wrong("an exporter writes out a drive, a set or a mix"));
    assert_eq!(exporter("[\"set\", \"drive\", \"set\"]", "\"sets:read\""), wrong("set is listed twice"));
    assert!(!exporter("\"playlist\"", "").is_empty());
    assert!(!exporter("[\"drive\", \"playlist\"]", "").is_empty());
}

#[test]
fn an_importer_says_what_it_provides() {
    let importer = |rest: &str| problem(&format!("[[contributes.importers]]\nformat = \"f\"\ntitle = \"F\"\n{rest}\n"));
    assert_eq!(importer("extension = \"nml\"\nprovides = [\"fields\", \"cues\", \"tags\", \"sessions\"]"), "");
    assert_eq!(
        importer("provides = []"),
        "contributes.importers[0].provides: names nothing: fields, cues, tags or sessions"
    );
    assert_eq!(importer("provides = [\"cues\", \"cues\"]"), "contributes.importers[0].provides: cues is there twice");
    assert!(importer("provides = [\"lyrics\"]").contains("unknown variant `lyrics`"));
    assert!(importer("").contains("missing field `provides`"));
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
        ("sources", "name = \"s\""),
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
    assert_eq!(
        operation("[]"),
        "contributes.operations[0].where: names no place: track, playlist, drive or collection"
    );
    assert_eq!(operation("[\"track\", \"track\"]"), "contributes.operations[0].where: track is there twice");
    assert!(operation("[\"deck\"]").contains("unknown variant `deck`"));
    let command = "[[contributes.commands]]\nname = \"c\"\ntitle = \"C\"\nmode = \"view\"\nwhere = []\n";
    assert_eq!(problem(command), "contributes.commands[0].where: names no place: track, playlist, drive or collection");
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
    let e = problem(&analyzer("mbids = \"string\"", "artists = \"mbids\""));
    assert_eq!(e, "contributes.analyzers[0].roles.artists: mbids isn't a string[], which the role is");
    let e = problem(&analyzer("names = \"string\"", "credits = \"names\""));
    assert_eq!(e, "contributes.analyzers[0].roles.credits: names isn't a string[], which the role is");
    let e = problem(&analyzer("runs = \"number\"", "clipped_runs = \"runs\""));
    assert_eq!(e, "contributes.analyzers[0].roles.clipped_runs: runs isn't an integer, which the role is");
    let ids = analyzer(
        "song = \"string\", album = \"string\", mbids = \"string[]\", code = \"string\", names = \"string[]\"",
        "recording = \"song\", release = \"album\", artists = \"mbids\", isrc = \"code\", credits = \"names\"",
    );
    Manifest::parse(&format!("{HEAD}{ids}")).unwrap().validate(true).unwrap();
    let e = problem(&analyzer("tempo = \"number\"", "bpm = \"speed\""));
    assert_eq!(e, "contributes.analyzers[0].roles.bpm: \"speed\" isn't its field");
    let e = problem(&analyzer("tempo = \"number\"", "tempo = \"tempo\""));
    assert!(e.contains("unknown variant `tempo`"), "{e}");
    // Every role reads back from its name.
    assert!(Role::ALL.iter().all(|r| r.name().parse() == Ok(*r)));
    assert_eq!("tempo".parse::<Role>().unwrap_err().split(':').next(), Some("\"tempo\" isn't a role"));
}

/// A panel is shown with a track, unless it says otherwise, which is refused.
#[test]
fn panels_are_shown_with_a_track() {
    let panel = |more: &str| format!("[[contributes.panels]]\nname = \"about\"\ntitle = \"About\"\n{more}");
    let m = Manifest::parse(&format!("{HEAD}{}", panel(""))).unwrap();
    m.validate(false).unwrap();
    assert_eq!(m.contributes.panels[0].offered, Where::Track);
    let e = problem(&panel("where = \"drive\""));
    assert_eq!(e, "contributes.panels[0].where: a panel is shown with a track: where = \"track\"");
}

/// A theme's colours are tables of the names a theme sets them by, each over the default's.
#[test]
fn a_theme_sets_colours_by_their_names() {
    let theme = |tokens: &str| {
        format!("[[contributes.themes]]\nname = \"dusk\"\ntitle = \"Dusk\"\n[contributes.themes.tokens]\n{tokens}")
    };
    let tokens = "playhead = \"#ffeedd\"\ncues.hot = \"#00ff00\"\nstatus.dark.good = \"#010203\"\nui = { accent = \"#ff8800\" }\nwaveform.scope = [\"#000000\", \"#ffffff\"]";
    let m = Manifest::parse(&format!("{HEAD}{}", theme(tokens))).unwrap();
    m.validate(false).unwrap();
    let t = m.contributes.themes[0].theme().unwrap();
    assert_eq!((t.playhead, t.hot_cue, t.good, t.accent), ([255, 238, 221], [0, 255, 0], [1, 2, 3], [255, 136, 0]));
    assert_eq!(t.scope.to_vec(), [[0, 0, 0], [255, 255, 255]]);
    assert_eq!(t.memory_cue, loclib_model::theme::Theme::DEFAULT.memory_cue, "the rest as they are");
    let at = "contributes.themes[0].tokens";
    assert_eq!(
        problem(&theme("ui.colour = \"#000000\"")),
        format!("{at}.ui.colour: \"ui.colour\" isn't a colour a theme sets")
    );
    assert_eq!(
        problem(&theme("playhead = \"white\"")),
        format!("{at}.playhead: isn't #rrggbb (lowercase), or a list of them")
    );
    assert_eq!(
        problem(&theme("waveform.spectrum = [\"#000000\"]")),
        format!("{at}.waveform.spectrum: waveform.spectrum is a gradient: two colours or more")
    );
    assert_eq!(problem(&theme("")), format!("{at}: a theme sets a colour at least"));
}

/// How an analyzer's fields are shown: hints for its own fields, in a format that fits each.
#[test]
fn an_analyzers_fields_say_how_they_are_shown() {
    let analyzer = |display: &str| {
        format!(
            "[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nversion = \"1\"\nfields = {{ lufs = \"number\", camelot = \"string\", runs = \"integer\" }}\n{display}"
        )
    };
    let ok = analyzer(
        "display.lufs = { unit = \"LUFS\", precision = 1 }\ndisplay.camelot = { format = \"key\", width = 4 }",
    );
    let m = Manifest::parse(&format!("{HEAD}{ok}")).unwrap();
    m.validate(false).unwrap();
    let d = &m.contributes.analyzers[0].display;
    assert_eq!(d["lufs"].show(&FieldValue::Number(-8.25)), "-8.2 LUFS");
    assert_eq!(d["camelot"].width, Some(4));
    let at = "contributes.analyzers[0].display";
    assert_eq!(
        problem(&analyzer("display.tempo = { unit = \"BPM\" }")),
        format!("{at}.tempo: \"tempo\" isn't its field")
    );
    assert_eq!(
        problem(&analyzer("display.lufs = { format = \"key\" }")),
        format!("{at}.lufs.format: lufs is a number, which key isn't for")
    );
    assert_eq!(
        problem(&analyzer("display.camelot = { format = \"db\" }")),
        format!("{at}.camelot.format: camelot is a string, which db isn't for")
    );
    assert_eq!(
        problem(&analyzer("display.runs = { format = \"key\" }")),
        format!("{at}.runs.format: runs is an integer, which key isn't for")
    );
    assert_eq!(
        problem(&analyzer("display.lufs = { precision = 7 }")),
        format!("{at}.lufs.precision: is 0 to 6 places")
    );
    assert_eq!(problem(&analyzer("display.lufs = { width = 0 }")), format!("{at}.lufs.width: is 1 to 40 cells"));
    assert_eq!(problem(&analyzer("display.lufs = { unit = \" \" }")), format!("{at}.lufs.unit: is empty"));
}

#[test]
fn field_values_are_shown_as_their_hints_say() {
    let hint = |format: Option<FieldFormat>, precision, width| FieldDisplay { unit: None, precision, format, width };
    let n = |v| FieldValue::Number(v);
    let cases = [
        (FieldDisplay::default(), n(124.5), "124.5"),
        (FieldDisplay::default(), n(-0.001), "0"),
        (FieldDisplay::default(), FieldValue::Integer(3), "3"),
        (FieldDisplay::default(), FieldValue::Null, "-"),
        (FieldDisplay::default(), FieldValue::Boolean(true), "yes"),
        (FieldDisplay::default(), FieldValue::Strings(vec!["a".into(), "b".into()]), "a, b"),
        (FieldDisplay { unit: Some("BPM".into()), ..FieldDisplay::default() }, n(128.0), "128 BPM"),
        (hint(None, Some(3), None), n(1.5), "1.500"),
        (hint(Some(FieldFormat::Duration), None, None), n(187.4), "3:07"),
        (hint(Some(FieldFormat::Duration), None, None), FieldValue::Integer(3723), "1:02:03"),
        (hint(Some(FieldFormat::Db), Some(1), None), n(0.44), "+0.4 dB"),
        (hint(Some(FieldFormat::Db), None, None), n(-8.2), "-8.2 dB"),
        (hint(Some(FieldFormat::Db), Some(1), None), n(0.01), "0.0 dB"),
        (hint(Some(FieldFormat::Percent), None, None), n(0.856), "86%"),
        (hint(Some(FieldFormat::Percent), Some(1), None), n(0.856), "85.6%"),
        (hint(Some(FieldFormat::Meter), None, Some(4)), n(0.5), "██░░"),
        (hint(Some(FieldFormat::Meter), None, None), n(2.0), "████████"),
        (hint(Some(FieldFormat::Key), None, None), FieldValue::String("8A".into()), "8A"),
    ];
    for (d, v, shown) in cases {
        assert_eq!(d.show(&v), shown, "{d:?} {v:?}");
    }
}

/// A meter is always as long as it says, and fuller for more; a percentage and a duration read
/// back as the number they show, to their rounding.
#[hegel::test(test_cases = 300)]
fn hints_show_numbers_as_they_are(tc: TestCase) {
    let v = tc.draw(gs::floats::<f64>().min_value(-1e7).max_value(1e7));
    let width = tc.draw(gs::integers::<u16>().min_value(1).max_value(FieldDisplay::MAX_WIDTH));
    let meter = FieldDisplay { format: Some(FieldFormat::Meter), width: Some(width), ..FieldDisplay::default() };
    let bar = meter.show(&FieldValue::Number(v));
    assert_eq!(bar.chars().count(), usize::from(width));
    let full = |b: &str| b.chars().filter(|c| *c == '█').count();
    let more = meter.show(&FieldValue::Number(v + 0.25));
    assert!(full(&more) >= full(&bar), "{bar} then {more}");
    let percent = FieldDisplay { format: Some(FieldFormat::Percent), ..FieldDisplay::default() };
    let shown = percent.show(&FieldValue::Number(v));
    let back: f64 = shown.strip_suffix('%').unwrap().parse().unwrap();
    assert!((back - v * 100.0).abs() <= 0.5 + 1e-6, "{v} as {shown}");
    let duration = FieldDisplay { format: Some(FieldFormat::Duration), ..FieldDisplay::default() };
    let shown = duration.show(&FieldValue::Number(v));
    let (sign, digits) = shown.strip_prefix('-').map_or((1.0, shown.as_str()), |d| (-1.0, d));
    let secs = digits.split(':').fold(0.0, |total, part| total * 60.0 + part.parse::<f64>().unwrap());
    assert!((sign * secs - v).abs() <= 0.5 + 1e-6, "{v} as {shown}");
}

// Properties: manifests drawn whole, then broken in one place each.

/// What `validate` says about `m` (`""`: nothing).
fn problem_with(m: &Manifest, built_in: bool) -> String {
    m.validate(built_in).err().map(|e| e.to_string()).unwrap_or_default()
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

/// The menus an operation is offered in: one or more, each once.
#[hegel::test_helper]
fn offered(tc: &TestCase) -> Vec<Where> {
    tc.draw(gs::subsequences(vec![Where::Track, Where::Playlist, Where::Drive]).min_size(1))
}

/// The places a command is offered in: an operation's, and the collection.
#[hegel::test_helper]
fn offered_command(tc: &TestCase) -> Vec<Where> {
    tc.draw(gs::subsequences(vec![Where::Track, Where::Playlist, Where::Drive, Where::Collection]).min_size(1))
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

/// Local Network's token exchange for a service, or the same path on a test server on this
/// computer.
#[hegel::test_helper]
fn exchange(tc: &TestCase) -> String {
    let origin = tc.draw(gs::sampled_from(vec![
        "https://localnetwork.computer",
        "https://localnetwork.computer:443",
        "http://127.0.0.1:5173",
        "http://localhost",
        "http://[::1]:9",
    ]));
    format!("{origin}/api/token-exchange/{}", tc.draw(gs::from_regex("[a-z0-9-]{1,12}")))
}

/// Where an account's token may go: the `Authorization` header after a scheme (`Bearer` when it
/// doesn't say), or a query parameter.
#[hegel::test_helper]
fn token_place(tc: &TestCase) -> TokenPlace {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => TokenPlace::default(),
        1 => TokenPlace::Scheme(tc.draw(gs::from_regex("[A-Za-z0-9!#$%&'*+.^_`|~-]{1,12}").fullmatch(true))),
        _ => TokenPlace::Query(tc.draw(gs::from_regex("[A-Za-z0-9._~-]{1,12}").fullmatch(true))),
    }
}

/// A host granted for the requests of the accounts a plugin asks for when no other is: longer than
/// any drawn.
const ACCOUNT_HOST: &str = "account-host.example";

/// How a field of type `kind` may be shown: any unit, places and width, and a format that fits it.
#[hegel::test_helper]
fn display(tc: &TestCase, kind: FieldType) -> FieldDisplay {
    let formats = match kind {
        FieldType::String => vec![FieldFormat::Key],
        FieldType::Number | FieldType::Integer => {
            vec![FieldFormat::Duration, FieldFormat::Db, FieldFormat::Percent, FieldFormat::Meter]
        }
        FieldType::Boolean | FieldType::Strings | FieldType::Beats | FieldType::Phrases | FieldType::Waveform => {
            Vec::new()
        }
    };
    let format = match formats.is_empty() {
        true => None,
        false => tc.draw(gs::optional(gs::sampled_from(formats))),
    };
    FieldDisplay {
        unit: tc.draw(gs::optional(gs::from_regex("[A-Za-z%]{1,5}"))),
        precision: tc.draw(gs::optional(gs::integers::<u8>().max_value(FieldDisplay::MAX_PRECISION))),
        format,
        width: tc.draw(gs::optional(gs::integers::<u16>().min_value(1).max_value(FieldDisplay::MAX_WIDTH))),
    }
}

/// Whether a field of type `kind` may be shown as `f`: a key a string, the rest a number.
fn fits(kind: FieldType, f: FieldFormat) -> bool {
    match f {
        FieldFormat::Key => kind == FieldType::String,
        _ => matches!(kind, FieldType::Number | FieldType::Integer),
    }
}

/// Some of the colours a theme sets, as tables by their names' parts (`cues.hot` in `cues`), one
/// at least: `#rrggbb` each, a gradient two or more.
#[hegel::test_helper]
fn tokens(tc: &TestCase) -> Map<String, Value> {
    let names = tc.draw(gs::subsequences(loclib_model::theme::TOKENS.to_vec()).min_size(1));
    let colour = |tc: &TestCase| {
        let [r, g, b] = tc.draw(gs::arrays(gs::integers::<u8>()));
        Value::from(format!("#{r:02x}{g:02x}{b:02x}"))
    };
    let mut tokens = Map::new();
    for name in names {
        let value = match name.starts_with("waveform.") {
            true => (0..tc.draw(gs::integers::<usize>().min_value(2).max_value(5))).map(|_| colour(tc)).collect(),
            false => colour(tc),
        };
        let mut parts: Vec<&str> = name.split('.').collect();
        let last = parts.pop().unwrap();
        let mut table = &mut tokens;
        for part in parts {
            table = table.entry(part).or_insert_with(|| Value::Object(Map::new())).as_object_mut().unwrap();
        }
        table.insert(last.into(), value);
    }
    tokens
}

/// A manifest that keeps every rule: commands and operations with places on the command line
/// only for a built-in plugin.
#[hegel::test_helper]
fn manifest(tc: &TestCase, built_in: bool) -> Manifest {
    let mut places = FxHashSet::default();
    let mut place = |tc: &TestCase| {
        if !built_in || !tc.draw(gs::booleans()) {
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
            offered: offered_command(tc),
            plan: tc.draw(gs::booleans()),
            long_running: tc.draw(gs::booleans()),
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
    let field_types = vec![
        FieldType::Number,
        FieldType::Integer,
        FieldType::String,
        FieldType::Boolean,
        FieldType::Strings,
        FieldType::Beats,
        FieldType::Phrases,
        FieldType::Waveform,
    ];
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
            display: IndexMap::new(),
        })
        .collect();
    for field in fields {
        let i = tc.draw(gs::integers::<usize>().max_value(n - 1));
        let mut kind = tc.draw(gs::sampled_from(field_types.clone()));
        // A structured field of each type at most.
        if kind.is_structured() && analyzers[i].fields.values().any(|t| *t == kind) {
            kind = FieldType::String;
        }
        analyzers[i].fields.insert(field, kind);
    }
    // Some roles for each, each from one of its fields of the role's type, and how some of its
    // fields are shown.
    for a in &mut analyzers {
        for (field, kind) in tc.draw(gs::subsequences(a.fields.clone().into_iter().collect::<Vec<_>>())) {
            a.display.insert(field, display(tc, kind));
        }
        for role in tc.draw(gs::subsequences(Role::ALL.to_vec())) {
            let typed: Vec<String> =
                a.fields.iter().filter(|(_, t)| **t == role.kind()).map(|(f, _)| f.clone()).collect();
            if !typed.is_empty() {
                a.roles.insert(role, tc.draw(gs::sampled_from(typed)));
            }
        }
    }
    let times = vec![When::Check, When::Apply];
    let checks = drawn_names(tc, 3)
        .into_iter()
        .map(|name| Check {
            name,
            title: title(tc),
            default_on: tc.draw(gs::booleans()),
            when: tc.draw(gs::permutations(tc.draw(gs::subsequences(times.clone()).min_size(1)))),
        })
        .collect();
    let sources = vec![ExportFrom::Drive, ExportFrom::Set, ExportFrom::Mix];
    let exporters: Vec<Exporter> = drawn_names(tc, 3)
        .into_iter()
        .map(|format| {
            let extension = tc.draw(gs::optional(gs::from_regex("[a-z0-9]{1,16}")));
            let (title, folder, options) = (title(tc), extension.is_none(), maybe_schema(tc));
            // One source as a name, or a list of any of them.
            let source = match tc.draw(gs::booleans()) {
                true => ExportSources::One(tc.draw(gs::sampled_from(sources.clone()))),
                false => ExportSources::Several(
                    tc.draw(gs::permutations(sources.clone()))
                        .into_iter()
                        .take(tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
                        .collect(),
                ),
            };
            Exporter { format, title, folder, extension, options, source }
        })
        .collect();
    let kinds = vec![ImportKind::Fields, ImportKind::Cues, ImportKind::Tags, ImportKind::Sessions];
    let importers = drawn_names(tc, 3)
        .into_iter()
        .map(|format| Importer {
            format,
            title: title(tc),
            extension: tc.draw(gs::optional(gs::from_regex("[a-z0-9]{1,16}"))),
            options: maybe_schema(tc),
            provides: {
                let mut provides = tc.draw(gs::permutations(kinds.clone()));
                provides.truncate(tc.draw(gs::integers::<usize>().min_value(1).max_value(kinds.len())));
                provides
            },
        })
        .collect::<Vec<_>>();
    let settings = drawn_names(tc, 3).into_iter().map(|key| setting(tc, key)).collect();
    let destination_kinds = vec![DestinationKind::Store, DestinationKind::Set];
    let destinations = drawn_names(tc, 3)
        .into_iter()
        .map(|name| Destination { name, title: title(tc), kind: tc.draw(gs::sampled_from(destination_kinds.clone())) })
        .collect();
    let panels = drawn_names(tc, 3).into_iter().map(|name| Panel { name, title: title(tc), offered: Where::Track });
    let themes = drawn_names(tc, 3).into_iter().map(|name| Theme { name, title: title(tc), tokens: tokens(tc) });
    let mut capabilities = tc.draw(gs::subsequences(third_party_capabilities()));
    // Granted reading the sets and mixes they write out.
    let reads_sets = exporters.iter().flat_map(Exporter::sources).any(|s| *s != ExportFrom::Drive);
    if reads_sets && !capabilities.contains(&Capability::SetsRead) {
        capabilities.push(Capability::SetsRead);
    }
    let kinds = vec![ProviderKind::Artwork, ProviderKind::Metadata];
    let providers: Vec<Provider> = drawn_names(tc, 3)
        .into_iter()
        .map(|name| Provider { name, title: title(tc), kind: tc.draw(gs::sampled_from(kinds.clone())) })
        .collect();
    // Granted reading what they're given.
    if !providers.is_empty() && !capabilities.contains(&Capability::LibraryRead) {
        capabilities.push(Capability::LibraryRead);
    }
    let sources: Vec<LibrarySource> = drawn_names(tc, 3)
        .into_iter()
        .map(|name| LibrarySource { name, title: title(tc), options: maybe_schema(tc) })
        .collect();
    // Granted reading the folders they're given.
    if !sources.is_empty() && !capabilities.contains(&Capability::FsRead) {
        capabilities.push(Capability::FsRead);
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
    let needs: Vec<AccountNeed> = drawn_names(tc, 2).into_iter().map(|name| account_need(tc, name)).collect();
    if !needs.is_empty() && hosts.is_empty() {
        capabilities.push(Capability::Net(ACCOUNT_HOST.into()));
    }
    capabilities.extend(hosts.into_iter().map(Capability::Net));
    let pre = tc.draw(gs::optional(gs::from_regex("[a-z][a-z0-9]{0,4}"))).map(|p| format!("-{p}")).unwrap_or_default();
    let [major, minor, patch] = tc.draw(gs::arrays(gs::integers::<u16>()));
    let exactly = format!("={PROTOCOL_VERSION}");
    let protocols = vec!["^0.2", "0.2", "~0.2", "^0.2.0", &exactly, "*", ">=0.2.0, <0.3.0", ">=0.0.1"];
    Manifest {
        id: tc.draw(gs::from_regex(DRAWN_NAME)),
        name: title(tc),
        version: format!("{major}.{minor}.{patch}{pre}"),
        protocol: tc.draw(gs::sampled_from(protocols)).to_string(),
        capabilities: tc.draw(gs::permutations(capabilities)),
        accounts: needs,
        contributes: Contributions {
            account_types: Vec::new(),
            commands,
            destinations,
            events,
            operations,
            panels: panels.collect(),
            analyzers,
            checks,
            exporters,
            importers,
            providers,
            services,
            sources,
            themes: themes.collect(),
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
/// operations places on the command line is valid only for a built-in plugin.
#[hegel::test(test_cases = 60)]
fn a_drawn_manifest_is_valid_and_reads_back_as_it_was_written(tc: TestCase) {
    let built_in = tc.draw(gs::booleans());
    let m = manifest(&tc, built_in);
    assert_eq!(problem_with(&m, built_in), "");
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

const CONTRIBUTIONS: [&str; 13] = [
    "commands",
    "destinations",
    "operations",
    "panels",
    "analyzers",
    "checks",
    "exporters",
    "importers",
    "providers",
    "services",
    "sources",
    "themes",
    "settings",
];

/// What names a contribution of `kind`.
fn name_key(kind: &str) -> &'static str {
    match kind {
        "exporters" | "importers" => "format",
        "settings" => "key",
        _ => "name",
    }
}

fn len(c: &Contributions, kind: &str) -> usize {
    match kind {
        "commands" => c.commands.len(),
        "destinations" => c.destinations.len(),
        "operations" => c.operations.len(),
        "analyzers" => c.analyzers.len(),
        "checks" => c.checks.len(),
        "exporters" => c.exporters.len(),
        "importers" => c.importers.len(),
        "providers" => c.providers.len(),
        "panels" => c.panels.len(),
        "services" => c.services.len(),
        "sources" => c.sources.len(),
        "themes" => c.themes.len(),
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
            "destinations" => c.destinations.push(Destination { name, title, kind: DestinationKind::Store }),
            "commands" => {
                let offered = on_a_drive();
                let mode = Mode::Headless;
                let (plan, long_running) = (false, false);
                c.commands.push(Command { name, title, mode, offered, plan, long_running, params: None, cli: None });
            }
            "operations" => {
                let offered = on_a_drive();
                c.operations.push(Operation { name, title, offered, long_running: false, params: None, cli: None });
            }
            "analyzers" => {
                let fields = IndexMap::from_iter([(format!("{name}-field"), FieldType::Number)]);
                let (roles, display) = (IndexMap::new(), IndexMap::new());
                c.analyzers.push(Analyzer { name, title, version: "1".into(), fields, roles, display });
            }
            "checks" => c.checks.push(Check { name, title, default_on: true, when: vec![When::Check] }),
            "exporters" => {
                let source = ExportSources::default();
                c.exporters.push(Exporter {
                    format: name,
                    title,
                    extension: None,
                    folder: true,
                    options: None,
                    source,
                });
            }
            "importers" => {
                let provides = vec![ImportKind::Fields];
                c.importers.push(Importer { format: name, title, extension: None, options: None, provides });
            }
            // Granted library:read by the caller.
            "providers" => c.providers.push(Provider { name, title, kind: ProviderKind::Metadata }),
            "panels" => c.panels.push(Panel { name, title, offered: Where::Track }),
            "services" => c.services.push(Service { name, title }),
            // Granted fs:read by the caller.
            "sources" => c.sources.push(LibrarySource { name, title, options: None }),
            "themes" => {
                let tokens = Map::from_iter([("playhead".into(), Value::from("#ffffff"))]);
                c.themes.push(Theme { name, title, tokens });
            }
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
        "commands" => c.commands.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "destinations" => c.destinations.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "operations" => c.operations.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "analyzers" => c.analyzers.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "checks" => c.checks.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "exporters" => c.exporters.iter_mut().map(|x| (&mut x.format, &mut x.title)).collect(),
        "importers" => c.importers.iter_mut().map(|x| (&mut x.format, &mut x.title)).collect(),
        "providers" => c.providers.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "panels" => c.panels.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "services" => c.services.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "sources" => c.sources.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        "themes" => c.themes.iter_mut().map(|x| (&mut x.name, &mut x.title)).collect(),
        _ => c.settings.iter_mut().map(|x| (&mut x.key, &mut x.title)).collect(),
    }
}

/// Another of contribution `i` of `kind`, named the same, at the end.
fn duplicate(c: &mut Contributions, kind: &str, i: usize) {
    match kind {
        "commands" => c.commands.push(Command { cli: None, ..c.commands[i].clone() }),
        "destinations" => c.destinations.push(c.destinations[i].clone()),
        "operations" => c.operations.push(Operation { cli: None, ..c.operations[i].clone() }),
        "analyzers" => c.analyzers.push(c.analyzers[i].clone()),
        "checks" => c.checks.push(c.checks[i].clone()),
        "exporters" => c.exporters.push(c.exporters[i].clone()),
        "importers" => c.importers.push(c.importers[i].clone()),
        "providers" => c.providers.push(c.providers[i].clone()),
        "panels" => c.panels.push(c.panels[i].clone()),
        "services" => c.services.push(c.services[i].clone()),
        "sources" => c.sources.push(c.sources[i].clone()),
        "themes" => c.themes.push(c.themes[i].clone()),
        _ => c.settings.push(c.settings[i].clone()),
    }
}

/// The params or options schema of contribution `i` of `kind`, and where it is.
fn schema_of<'a>(c: &'a mut Contributions, kind: &str, i: usize) -> (&'a mut Option<Value>, String) {
    match kind {
        "commands" => (&mut c.commands[i].params, format!("contributes.commands[{i}].params")),
        "operations" => (&mut c.operations[i].params, format!("contributes.operations[{i}].params")),
        "importers" => (&mut c.importers[i].options, format!("contributes.importers[{i}].options")),
        "sources" => (&mut c.sources[i].options, format!("contributes.sources[{i}].options")),
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
    refused_saying_where(&tc, tc.draw(gs::sampled_from(MISTAKES.to_vec())));
}

/// Each kind of mistake, made in a manifest that's otherwise right, is refused saying where it is
/// and what's wrong with it: every one of them in every run.
#[hegel::test(test_cases = 5)]
fn prop_every_kind_of_mistake_is_refused_saying_where(tc: TestCase) {
    for mistake in MISTAKES {
        refused_saying_where(&tc, mistake);
    }
}

/// The kinds of mistake [`refused_saying_where`] makes.
const MISTAKES: &[&str] = &[
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
    "structured twice",
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
    "check when",
    "export grant",
    "import extension",
    "provides nothing",
    "provides twice",
    "panel place",
    "no tokens",
    "token name",
    "token colour",
    "token count",
    "display field",
    "display format",
    "display precision",
    "display width",
    "display unit",
    "no events",
    "event twice",
    "event grant",
    "source grant",
];

/// A drawn manifest with `mistake` made in it is refused, as the first thing `validate` finds,
/// saying where the mistake is and what's wrong with it.
#[hegel::test_helper]
fn refused_saying_where(tc: &TestCase, mistake: &str) {
    let mut built_in = tc.draw(gs::booleans());
    let mut m = manifest(tc, built_in);
    let c = &mut m.contributes;
    // What `validate` says, and only `prefix` of it, when the rest is another crate's words.
    let (said, prefix) = match mistake {
        "id" => {
            m.id = bad_name(tc);
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
            let again = m.capabilities[index(tc, m.capabilities.len())].clone();
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
            let i = index(tc, places.len());
            // Only a built-in plugin's places are checked further.
            built_in = true;
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
                    let j = i + 1 + index(tc, places.len() - i - 1);
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
            if kind == "sources" && !m.capabilities.contains(&Capability::FsRead) {
                m.capabilities.push(Capability::FsRead);
            }
            let i = index(tc, len(c, kind));
            let key = name_key(kind);
            match mistake {
                "contribution name" => {
                    let wrong = bad_name(tc);
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
            let kind = tc.draw(gs::sampled_from(vec!["commands", "operations", "exporters", "importers", "sources"]));
            add(c, kind, 1);
            if kind == "sources" && !m.capabilities.contains(&Capability::FsRead) {
                m.capabilities.push(Capability::FsRead);
            }
            let i = index(tc, len(c, kind));
            let (slot, at) = schema_of(c, kind, i);
            let mut drawn = params(tc);
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
                        problem_with(&m, built_in),
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
                        problem_with(&m, built_in).starts_with(&format!("{at}: isn't a valid JSON Schema: "))
                    );
                }
                "param name" => {
                    let name = match tc.draw(gs::booleans()) {
                        true => bad_name(tc),
                        false => tc.draw(gs::from_regex("[a-z][a-z0-9]{0,4}-[a-z0-9]{0,4}")),
                    };
                    let said = format!("{name:?} isn't a param's name: {PARAM_RULE}");
                    ((name, json!({ "type": "string" })), (String::new(), said))
                }
                "x-cli" => {
                    let (property, said) = match tc.draw(gs::integers::<u8>().max_value(4)) {
                        0 => {
                            let flag = bad_name(tc);
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
                        problem_with(&m, built_in),
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
            let i = index(tc, c.analyzers.len());
            match mistake {
                "no fields" => {
                    c.analyzers[i].fields.clear();
                    (format!("contributes.analyzers[{i}].fields: an analyzer needs a field"), false)
                }
                "field name" => {
                    let name = bad_name(tc);
                    c.analyzers[i].fields.insert(name.clone(), FieldType::String);
                    (format!("contributes.analyzers[{i}].fields.{name}: isn't a name: {NAME_RULE}"), false)
                }
                _ => {
                    let (i, j) = (i.max(1), index(tc, i.max(1)));
                    let field = c.analyzers[j].fields.keys().next().unwrap().clone();
                    c.analyzers[i].fields.insert(field.clone(), FieldType::Boolean);
                    (format!("contributes.analyzers[{i}].fields.{field}: another analyzer has this field"), false)
                }
            }
        }
        "structured twice" => {
            add(c, "analyzers", 1);
            let i = index(tc, c.analyzers.len());
            let kind = tc.draw(gs::sampled_from(vec![FieldType::Beats, FieldType::Phrases, FieldType::Waveform]));
            let fields = &mut c.analyzers[i].fields;
            // Longer than any drawn name, so none of the analyzers' fields.
            fields.insert("added-structured-1".into(), kind);
            fields.insert("added-structured-2".into(), kind);
            let second = fields.iter().filter(|(_, t)| **t == kind).nth(1).map(|(f, _)| f.clone()).unwrap();
            let named = serde_json::to_value(kind).unwrap();
            let said = format!("another field is its {} already", named.as_str().unwrap());
            (format!("contributes.analyzers[{i}].fields.{second}: {said}"), false)
        }
        "role field" | "role type" => {
            add(c, "analyzers", 1);
            let i = index(tc, c.analyzers.len());
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
                (format!("{at}: {field} isn't {}, which the role is", a_field_of(role.kind())), false)
            }
        }
        "file or folder" | "extension" => {
            add(c, "exporters", 1);
            let i = index(tc, c.exporters.len());
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
        "check when" => {
            add(c, "checks", 1);
            let i = index(tc, c.checks.len());
            let x = &mut c.checks[i];
            let at = format!("contributes.checks[{i}].when");
            match tc.draw(gs::booleans()) {
                true => {
                    x.when.clear();
                    (format!("{at}: names no time: check, apply or both"), false)
                }
                false => {
                    let again = x.when[index(tc, x.when.len())];
                    x.when.push(again);
                    (format!("{at}: {} is there twice", again.name()), false)
                }
            }
        }
        "panel place" => {
            add(c, "panels", 1);
            let i = index(tc, c.panels.len());
            c.panels[i].offered = tc.draw(gs::sampled_from(vec![Where::Playlist, Where::Drive]));
            (format!("contributes.panels[{i}].where: a panel is shown with a track: where = \"track\""), false)
        }
        "no tokens" | "token name" | "token colour" | "token count" => {
            add(c, "themes", 1);
            let i = index(tc, c.themes.len());
            let tokens = &mut c.themes[i].tokens;
            let at = format!("contributes.themes[{i}].tokens");
            match mistake {
                "no tokens" => {
                    tokens.clear();
                    (format!("{at}: a theme sets a colour at least"), false)
                }
                "token name" => {
                    // Longer than any token, under a group a theme has or one it hasn't.
                    let group = tc.draw(gs::sampled_from(vec!["ui", "cues", "added"]));
                    let name = format!("{group}.added-colour");
                    tokens.insert(group.into(), json!({ "added-colour": "#000000" }));
                    (format!("{at}.{name}: {name:?} isn't a colour a theme sets"), false)
                }
                "token colour" => {
                    let wrong = tc.draw(gs::sampled_from(vec![
                        json!("#FFFFFF"),
                        json!("#fff"),
                        json!("ffffff"),
                        json!(5),
                        json!(true),
                        json!(["#ffffff", 1]),
                    ]));
                    tokens.insert("playhead".into(), wrong);
                    (format!("{at}.playhead: isn't #rrggbb (lowercase), or a list of them"), false)
                }
                _ => match tc.draw(gs::booleans()) {
                    true => {
                        tokens.insert("playhead".into(), json!(["#ffffff", "#000000"]));
                        (format!("{at}.playhead: playhead is one colour"), false)
                    }
                    false => {
                        tokens.insert("waveform".into(), json!({ "scope": tc.draw(gs::sampled_from(vec![json!("#ffffff"), json!([]), json!(["#ffffff"])])) }));
                        (format!("{at}.waveform.scope: waveform.scope is a gradient: two colours or more"), false)
                    }
                },
            }
        }
        "display field" | "display format" | "display precision" | "display width" | "display unit" => {
            add(c, "analyzers", 1);
            let i = index(tc, c.analyzers.len());
            let a = &mut c.analyzers[i];
            let (field, kind) = a.fields.iter().next().map(|(f, k)| (f.clone(), *k)).unwrap();
            let at = format!("contributes.analyzers[{i}].display");
            let mut d = display(tc, kind);
            let said = match mistake {
                "display field" => {
                    // Longer than any drawn name, so none of its fields.
                    a.display.insert("added-display-field".into(), d);
                    return assert_eq!(
                        problem_with(&m, built_in),
                        format!("{at}.added-display-field: \"added-display-field\" isn't its field")
                    );
                }
                "display format" => {
                    let all = [
                        FieldFormat::Key,
                        FieldFormat::Duration,
                        FieldFormat::Db,
                        FieldFormat::Percent,
                        FieldFormat::Meter,
                    ];
                    let unfit: Vec<FieldFormat> = all.into_iter().filter(|f| !fits(kind, *f)).collect();
                    let f = tc.draw(gs::sampled_from(unfit));
                    d.format = Some(f);
                    let format = serde_json::to_value(f).unwrap();
                    format!(".format: {field} is {}, which {} isn't for", a_field_of(kind), format.as_str().unwrap())
                }
                "display precision" => {
                    d.precision = Some(tc.draw(gs::integers::<u8>().min_value(FieldDisplay::MAX_PRECISION + 1)));
                    ".precision: is 0 to 6 places".into()
                }
                "display width" => {
                    d.width = Some(tc.draw(gs::sampled_from(vec![0, FieldDisplay::MAX_WIDTH + 1, u16::MAX])));
                    ".width: is 1 to 40 cells".into()
                }
                _ => {
                    d.unit = Some(tc.draw(gs::text().alphabet(" \t").max_size(3)));
                    ".unit: is empty".into()
                }
            };
            a.display.insert(field.clone(), d);
            (format!("{at}.{field}{said}"), false)
        }
        "provider grant" => {
            add(c, "providers", 1);
            m.capabilities.retain(|g| *g != Capability::LibraryRead);
            ("contributes.providers[0]: is given tracks' tags, which needs the library:read capability".into(), false)
        }
        "source grant" => {
            add(c, "sources", 1);
            m.capabilities.retain(|g| *g != Capability::FsRead);
            let said =
                "contributes.sources[0]: reads files in the folders it's granted, which needs the fs:read capability";
            (said.into(), false)
        }
        "export grant" => {
            add(c, "exporters", 1);
            let i = index(tc, c.exporters.len());
            let read = tc.draw(gs::sampled_from(vec![ExportFrom::Set, ExportFrom::Mix]));
            c.exporters[i].source = match tc.draw(gs::booleans()) {
                true => ExportSources::One(read),
                false => ExportSources::Several(vec![ExportFrom::Drive, read]),
            };
            m.capabilities.retain(|g| *g != Capability::SetsRead);
            let reads = |x: &Exporter| x.sources().iter().copied().find(|s| *s != ExportFrom::Drive);
            let (i, what) = c.exporters.iter().enumerate().find_map(|(i, x)| Some((i, reads(x)?.name()))).unwrap();
            (format!("contributes.exporters[{i}].source: reads {what}s, which needs the sets:read capability"), false)
        }
        "import extension" => {
            add(c, "importers", 1);
            let i = index(tc, c.importers.len());
            let ext = match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => String::new(),
                1 => tc.draw(gs::from_regex("[a-z0-9]{0,3}[A-Z._/ -][a-z0-9]{0,3}")),
                _ => "a".repeat(tc.draw(gs::integers::<usize>().min_value(17).max_value(24))),
            };
            c.importers[i].extension = Some(ext.clone());
            let said = format!("{ext:?} isn't an extension: lowercase letters and digits, without the dot");
            (format!("contributes.importers[{i}].extension: {said}"), false)
        }
        "provides nothing" | "provides twice" => {
            add(c, "importers", 1);
            let i = index(tc, c.importers.len());
            let x = &mut c.importers[i];
            let at = format!("contributes.importers[{i}].provides");
            if mistake == "provides nothing" {
                x.provides.clear();
                (format!("{at}: names nothing: fields, cues, tags or sessions"), false)
            } else {
                let again = x.provides[index(tc, x.provides.len())];
                x.provides.push(again);
                (format!("{at}: {} is there twice", again.name()), false)
            }
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
            let i = index(tc, c.settings.len());
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
                    let again = x.choices[index(tc, x.choices.len())].clone();
                    x.choices.push(again.clone());
                    (format!("{at}.choices: {again:?} is there twice"), false)
                }
                _ => {
                    let choices = x.choices.clone();
                    x.default = Some(match x.kind {
                        SettingType::String | SettingType::File | SettingType::Directory => number(tc),
                        SettingType::Number => tc.draw(gs::text()).into(),
                        SettingType::Boolean => number(tc),
                        SettingType::Enum => tc.draw(gs::text().filter(move |s: &String| !choices.contains(s))).into(),
                        SettingType::Secret => tc.draw(gs::text()).into(),
                    });
                    (format!("{at}.default: doesn't fit the setting's type (and a secret has none)"), false)
                }
            }
        }
    };
    let problem = problem_with(&m, built_in);
    match prefix {
        true => assert!(problem.starts_with(&said), "{problem:?} doesn't start with {said:?}"),
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
    match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => FieldValue::Integer(tc.draw(gs::integers())),
        1 => FieldValue::Number(tc.draw(gs::floats::<f64>().allow_nan(false).allow_infinity(false))),
        2 => FieldValue::Boolean(tc.draw(gs::booleans())),
        3 => FieldValue::String(tc.draw(gs::text())),
        4 => FieldValue::Strings(tc.draw(gs::vecs(gs::text()).max_size(3))),
        5 => FieldValue::Structured(json!({ "of": tc.draw(gs::integers::<i64>()) })),
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
        // What none of these hold.
        FieldValue::Structured(_) => Some(FieldType::Beats),
    };
    for kind in [FieldType::Number, FieldType::Integer, FieldType::String, FieldType::Boolean, FieldType::Strings] {
        let fits = own.is_none_or(|own| kind == own || (kind, own) == (FieldType::Number, FieldType::Integer));
        assert_eq!(kind.accepts(&value), fits, "{kind:?} {value:?}");
    }
}

/// A beat grid, phrases or a waveform made for a field fits that field's type alone (and null
/// fits each, as nothing to report; an empty grid is an empty list of strings too), reads back
/// from JSON as the same, and is what [`crate::Output`] reads; a waveform needs columns a second.
#[hegel::test(test_cases = 100)]
fn structured_values_fit_their_type_alone(tc: TestCase) {
    use loclib_model::analysis::{Beat, Mood, Phrase, PhraseKind, SongStructure};
    let n = tc.draw(gs::integers::<usize>().max_value(4));
    let beats: Vec<Beat> =
        (0..n).map(|i| Beat { beat_number: tc.draw(gs::integers::<u16>()), bpm: 120.5, time_ms: i as u32 }).collect();
    let phrases = SongStructure {
        mood: Mood::Low,
        phrases: vec![Phrase { kind: PhraseKind::Verse(2), start_beat: 1, end_beat: 9, fill_beat: Some(5) }],
    };
    let rate = tc.draw(gs::floats::<f64>().allow_nan(false).allow_infinity(false));
    let given = [
        (FieldType::Beats, FieldValue::beats(&beats), Some(crate::Output::Beats(beats))),
        (FieldType::Phrases, FieldValue::phrases(&phrases), Some(crate::Output::Phrases(phrases))),
        (
            FieldType::Waveform,
            FieldValue::waveform("w.bands", rate),
            (rate > 0.0).then(|| {
                crate::Output::Waveform(crate::WaveformBlob { blob: "w.bands".into(), columns_per_sec: rate })
            }),
        ),
    ];
    let all = [
        FieldType::Number,
        FieldType::Integer,
        FieldType::String,
        FieldType::Boolean,
        FieldType::Strings,
        FieldType::Beats,
        FieldType::Phrases,
        FieldType::Waveform,
    ];
    for (own, value, output) in given {
        let json = serde_json::to_string(&value).unwrap();
        let back: FieldValue = serde_json::from_str(&json).unwrap();
        assert_eq!(crate::Output::of(own, &back), crate::Output::of(own, &value), "{json}");
        assert_eq!(crate::Output::of(own, &value).ok().flatten(), output);
        for kind in all {
            let fits = (kind == own && output.is_some()) || (kind == FieldType::Strings && json == "[]");
            assert_eq!(kind.accepts(&back), fits, "{kind:?} {back:?}");
            assert!(kind.accepts(&FieldValue::Null));
        }
        assert_eq!(crate::Output::of(own, &FieldValue::Null), Ok(None));
    }
    assert!(crate::Output::of(FieldType::Number, &FieldValue::Integer(1)).is_err());
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
        "collection:files",
        "library:plan",
        "library:delete",
        "library:delete-unbacked",
        "tags:plan",
        "cues:plan",
        "artwork:plan",
        "analysis:plan",
        "events:drives",
        "events:jobs",
        "events:plans",
        "fs:read",
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

// Account plugins, drawn whole, then broken in one place each.

/// A scope as OAuth 2 writes one (RFC 6749): printable ASCII but for spaces, `"` and `\`.
const SERVICE_SCOPE: &str = r#"[!#-\[\]-~]{1,16}"#;

#[hegel::test_helper]
fn service_scopes(tc: &TestCase) -> Vec<String> {
    tc.draw(gs::vecs(gs::from_regex(SERVICE_SCOPE)).max_size(3).unique(true))
}

/// A JSON pointer: empty, or `/` and its parts, with `~` escaped.
#[hegel::test_helper]
fn pointer(tc: &TestCase) -> String {
    tc.draw(gs::from_regex("(/([a-z0-9]|~[01]){0,6}){0,3}"))
}

/// Authorize params an account type may set, but those `taken` sets, their values naming the
/// account (only as `{subject}`) where `subject`.
#[hegel::test_helper]
fn authorize_params(tc: &TestCase, subject: bool, taken: &[&IndexMap<String, String>]) -> IndexMap<String, String> {
    let parts = if subject { 3 } else { 1 };
    let keys = tc.draw(gs::vecs(gs::sampled_from(AUTHORIZE_PARAMS.to_vec())).max_size(3).unique(true));
    keys.into_iter()
        .filter(|key| !taken.iter().any(|set| set.contains_key(*key)))
        .map(|key| {
            let parts = tc.draw(gs::vecs(gs::text().max_size(6)).min_size(1).max_size(parts));
            let parts: Vec<String> = parts.iter().map(|p| p.replace(['{', '}'], "")).collect();
            (key.to_string(), parts.join(SUBJECT_PLACEHOLDER))
        })
        .collect()
}

/// An address a scope's request may name, written as it's read, maybe ending in `*`.
#[hegel::test_helper]
fn request_pattern(tc: &TestCase) -> String {
    let read = Url::parse(&endpoint(tc)).unwrap();
    let star = if tc.draw(gs::booleans()) { "*" } else { "" };
    format!("{read}{star}")
}

/// A scope covering a request or more.
#[hegel::test_helper]
fn scope(tc: &TestCase) -> Scope {
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let requests = (0..n)
        .map(|_| ScopeRequest { method: tc.draw(gs::sampled_from(METHODS.to_vec())).into(), url: request_pattern(tc) })
        .collect();
    Scope { title: title(tc), asks: service_scopes(tc), requests }
}

/// Where a service says who signed in.
#[hegel::test_helper]
fn whoami(tc: &TestCase) -> Whoami {
    Whoami { url: endpoint(tc), subject: pointer(tc), name: pointer(tc) }
}

/// An account type with endpoints it may have (Local Network's token exchange only when
/// `built_in`), authorize params, pointers into who signed in and some scopes.
#[hegel::test_helper]
fn account_type(tc: &TestCase, built_in: bool) -> AccountType {
    let exchanges = built_in && tc.draw(gs::booleans());
    let (one_account, another_account) = (authorize_params(tc, true, &[]), authorize_params(tc, false, &[]));
    let params = authorize_params(tc, false, &[&one_account, &another_account]);
    AccountType {
        name: tc.draw(gs::from_regex(DRAWN_NAME)),
        title: title(tc),
        kind: SignIn::Oauth,
        authorize_url: Some(endpoint(tc)),
        token_url: (!exchanges).then(|| endpoint(tc)),
        token_exchange: exchanges.then(|| exchange(tc)),
        client_id: Some(title(tc)),
        revoke_url: tc.draw(gs::booleans()).then(|| endpoint(tc)),
        token: token_place(tc),
        redirect_port: tc.draw(gs::optional(gs::integers::<u16>().min_value(1024))),
        always: service_scopes(tc),
        whoami: Some(whoami(tc)),
        one_account,
        another_account,
        params,
        keys: IndexMap::new(),
        keys_url: None,
        authorization: None,
        scopes: drawn_names(tc, 3).into_iter().map(|name| (name, scope(tc))).collect(),
    }
}

/// An account plugin: an account type and nothing else.
#[hegel::test_helper]
fn account_plugin(tc: &TestCase, built_in: bool) -> Manifest {
    Manifest {
        id: tc.draw(gs::from_regex(DRAWN_NAME)),
        name: title(tc),
        version: "0.1.0".into(),
        protocol: "^0.2".into(),
        capabilities: Vec::new(),
        accounts: Vec::new(),
        contributes: Contributions { account_types: vec![account_type(tc, built_in)], ..Contributions::default() },
        net: Vec::new(),
        unknown: Vec::new(),
    }
}

/// What a plugin locallibrary doesn't ship is told when its account type names the token exchange.
const NOT_OURS: &str = "contributes.account_types[0].token_exchange: is only for account plugins locallibrary \
                        ships: it signs people in as Local Network";

/// Any account plugin that keeps the rules validates, reads back as it was written from TOML and
/// from JSON, and fits the manifest's JSON Schema. One using Local Network's token exchange is
/// valid only for a built-in plugin.
#[hegel::test(test_cases = 100)]
fn a_drawn_account_plugin_is_valid_and_reads_back_as_it_was_written(tc: TestCase) {
    let built_in = tc.draw(gs::booleans());
    let m = account_plugin(&tc, built_in);
    assert_eq!(problem_with(&m, built_in), "");
    assert!(m.is_account_plugin());
    let json = serde_json::to_value(&m).unwrap();
    let toml = toml_of(&json);
    assert_eq!(Manifest::parse(&toml).unwrap(), m, "{toml}");
    assert_eq!(serde_json::from_value::<Manifest>(json.clone()).unwrap(), m);
    let schema = serde_json::to_value(schemars::schema_for!(Manifest)).unwrap();
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&json), "{json}");
    let exchanges = m.contributes.account_types[0].token_exchange.is_some();
    assert_eq!(problem_with(&m, false), if exchanges { NOT_OURS } else { "" });
}

/// Any one thing wrong with an account plugin that's otherwise right is refused, saying where it
/// is and what's wrong with it.
#[hegel::test(test_cases = 500)]
fn each_kind_of_account_type_mistake_is_refused_saying_where(tc: TestCase) {
    let mut built_in = tc.draw(gs::booleans());
    let mut m = account_plugin(&tc, built_in);
    let mistakes = vec![
        "second",
        "beside",
        "type",
        "title",
        "client",
        "endpoint",
        "token endpoint",
        "exchange",
        "token place",
        "port",
        "service scope",
        "pointer",
        "param",
        "placeholder",
        "clash",
        "scope name",
        "scope title",
        "no requests",
        "method",
        "pattern",
        "unread pattern",
        "keys field",
        "missing",
    ];
    let mistake = tc.draw(gs::sampled_from(mistakes));
    let at = "contributes.account_types[0]";
    let word = |tc: &TestCase| tc.draw(gs::from_regex("[a-z0-9]{0,4}"));
    // Empty, or one of `bad` between words.
    let spoilt = |tc: &TestCase, bad: Vec<&str>| match tc.draw(gs::booleans()) {
        true => String::new(),
        false => format!("{}{}{}", word(tc), tc.draw(gs::sampled_from(bad)), word(tc)),
    };
    let said = match mistake {
        "second" => {
            let another = account_type(&tc, built_in);
            m.contributes.account_types.push(another);
            "contributes.account_types[1]: is a second account type: an account plugin provides one".into()
        }
        "beside" => {
            match tc.draw(gs::sampled_from(vec!["capability", "accounts", "net", "contribution"])) {
                "capability" => m.capabilities.push(tc.draw(gs::sampled_from(third_party_capabilities()))),
                "accounts" => m.accounts.push(account_need(&tc, tc.draw(gs::from_regex(DRAWN_NAME)))),
                "net" => m.net.push(NetLimit { host: "a.example".into(), per_second: 1.0 }),
                _ => add(&mut m.contributes, tc.draw(gs::sampled_from(CONTRIBUTIONS.to_vec())), 1),
            }
            let others = "no capabilities, accounts, net limits or other contributions";
            format!("{at}: makes an account plugin, which has nothing else: {others}")
        }
        _ => {
            let x = &mut m.contributes.account_types[0];
            // A scope to break, adding one when there's none.
            let scope_key = |x: &mut AccountType| {
                if x.scopes.is_empty() {
                    x.scopes.insert("added-scope-name".into(), scope(&tc));
                }
                tc.draw(gs::sampled_from(x.scopes.keys().cloned().collect::<Vec<_>>()))
            };
            match mistake {
                "type" => {
                    x.name = bad_name(&tc);
                    format!("{at}.type: {:?} isn't a name: {NAME_RULE}", x.name)
                }
                "title" | "client" => {
                    let (text, key) = match mistake {
                        "title" => (&mut x.title, "title"),
                        _ => (x.client_id.get_or_insert_default(), "client_id"),
                    };
                    *text = tc.draw(gs::text().alphabet(" \t\r\n").max_size(3));
                    format!("{at}.{key}: is empty")
                }
                "endpoint" => {
                    let key = tc.draw(gs::sampled_from(vec!["authorize_url", "token_url", "revoke_url", "whoami.url"]));
                    let wrong = tc.draw(gs::sampled_from(vec![
                        "http://a.example/",
                        "https://someone@a.example/",
                        "https://someone:secret@a.example/",
                        "ftp://a.example/",
                        "a.example",
                        "https://a.example/a b",
                    ]));
                    match key {
                        "authorize_url" => x.authorize_url = Some(wrong.into()),
                        "token_url" => (x.token_url, x.token_exchange) = (Some(wrong.into()), None),
                        "revoke_url" => x.revoke_url = Some(wrong.into()),
                        _ => x.whoami.as_mut().unwrap().url = wrong.into(),
                    }
                    format!("{at}.{key}: {wrong:?} isn't an https:// address without a user name or password")
                }
                "token endpoint" => match tc.draw(gs::sampled_from(vec!["neither", "both", "elsewhere"])) {
                    "neither" => {
                        (x.token_url, x.token_exchange) = (None, None);
                        format!("{at}.token_url: is missing, and so is token_exchange")
                    }
                    "both" => {
                        (x.token_url, x.token_exchange) = (Some(endpoint(&tc)), Some(exchange(&tc)));
                        format!("{at}.token_exchange: goes in token_url's place, not beside it")
                    }
                    _ => {
                        let wrong = endpoint(&tc);
                        (x.token_url, x.token_exchange) = (None, Some(wrong.clone()));
                        let rule = "https://localnetwork.computer/api/token-exchange/<service>";
                        format!("{at}.token_exchange: {wrong:?} isn't Local Network's token exchange ({rule})")
                    }
                },
                "exchange" => {
                    built_in = false;
                    (x.token_url, x.token_exchange) = (None, Some(exchange(&tc)));
                    NOT_OURS.into()
                }
                "token place" => match tc.draw(gs::booleans()) {
                    true => {
                        let wrong = tc.draw(gs::sampled_from(vec!["", "Bearer token", "a=b", "(x)"]));
                        x.token = TokenPlace::Scheme(wrong.into());
                        format!("{at}.token.scheme: {wrong:?} isn't a scheme: one word, like Bearer or OAuth")
                    }
                    false => {
                        let wrong = tc.draw(gs::sampled_from(vec!["", "a=b", "a b", "a&b"]));
                        x.token = TokenPlace::Query(wrong.into());
                        let rule = "a query parameter's name: letters, digits, -, ., _ and ~";
                        format!("{at}.token.query: {wrong:?} isn't {rule}")
                    }
                },
                "keys field" => {
                    let key = tc.draw(gs::sampled_from(vec!["keys", "keys_url", "authorization"]));
                    match key {
                        "keys" => _ = x.keys.insert("access".into(), Key { title: title(&tc) }),
                        "keys_url" => x.keys_url = Some(endpoint(&tc)),
                        _ => x.authorization = Some("LOW {access}".into()),
                    }
                    format!("{at}.{key}: is for an account type signed in to with keys (kind = \"keys\")")
                }
                "missing" => {
                    let key = tc.draw(gs::sampled_from(vec!["authorize_url", "client_id", "whoami"]));
                    match key {
                        "authorize_url" => x.authorize_url = None,
                        "client_id" => x.client_id = None,
                        _ => x.whoami = None,
                    }
                    format!("{at}.{key}: is missing")
                }
                "port" => {
                    let port = tc.draw(gs::integers::<u16>().max_value(1023));
                    x.redirect_port = Some(port);
                    format!("{at}.redirect_port: is 1024 or more: {port} is a system service's")
                }
                "service scope" => {
                    let (list, at) = match tc.draw(gs::booleans()) {
                        true => (&mut x.always, format!("{at}.always")),
                        false => {
                            let key = scope_key(x);
                            (&mut x.scopes[&key].asks, format!("{at}.scopes.{key}.asks"))
                        }
                    };
                    let wrong = match tc.draw(gs::booleans()) {
                        true => spoilt(&tc, vec![" ", "\"", "\\", "\t", "é"]),
                        false => {
                            if list.is_empty() {
                                list.push("openid".into());
                            }
                            list[index(&tc, list.len())].clone()
                        }
                    };
                    list.push(wrong.clone());
                    let j = list.len() - 1;
                    match list[..j].contains(&wrong) {
                        true => format!("{at}[{j}]: {wrong:?} is there twice"),
                        false => {
                            format!("{at}[{j}]: {wrong:?} isn't a scope: printable ASCII without spaces, \" or \\")
                        }
                    }
                }
                "pointer" => {
                    let key = tc.draw(gs::sampled_from(vec!["subject", "name"]));
                    let w = word(&tc);
                    let wrong = tc.draw(gs::sampled_from(vec![
                        format!("{w}x"),
                        format!("/{w}~"),
                        format!("/{w}~2"),
                        format!("/~x{w}"),
                    ]));
                    match key {
                        "subject" => x.whoami.as_mut().unwrap().subject = wrong.clone(),
                        _ => x.whoami.as_mut().unwrap().name = wrong.clone(),
                    }
                    format!("{at}.whoami.{key}: {wrong:?} isn't a JSON pointer, like /sub")
                }
                "param" | "placeholder" => {
                    let (params, which) =
                        match tc.draw(gs::sampled_from(vec!["one_account", "another_account", "params"])) {
                            "one_account" => (&mut x.one_account, "one_account"),
                            "another_account" => (&mut x.another_account, "another_account"),
                            _ => (&mut x.params, "params"),
                        };
                    let (key, value, said) = match mistake {
                        "param" => {
                            let key = tc.draw(
                                gs::from_regex("[A-Za-z0-9._~=& {}-]{1,16}")
                                    .filter(|k: &String| !AUTHORIZE_PARAMS.contains(&k.as_str())),
                            );
                            let allowed = AUTHORIZE_PARAMS.join(", ");
                            let said = format!("isn't one of the params an account type may set: {allowed}");
                            (key, "x".to_string(), said)
                        }
                        _ => {
                            let (named, bad) = match which {
                                "one_account" => {
                                    (SUBJECT_PLACEHOLDER, vec!["{", "}", "{email}", "{subject", "subject}"])
                                }
                                _ => ("", vec!["{", "}", "{subject}"]),
                            };
                            let bad = tc.draw(gs::sampled_from(bad));
                            let value = format!("{}{named}{bad}{}", word(&tc), word(&tc));
                            let said = match which {
                                "one_account" => format!("{value:?} has a {{ or }} besides {{subject}}"),
                                _ => format!("{value:?} has a {{ or }}: only one_account names an account"),
                            };
                            ("login_hint".to_string(), value, said)
                        }
                    };
                    params.insert(key.clone(), value);
                    format!("{at}.{which}.{key}: {said}")
                }
                "clash" => {
                    let key = tc.draw(gs::sampled_from(AUTHORIZE_PARAMS.to_vec()));
                    let set = match tc.draw(gs::booleans()) {
                        true => &mut x.one_account,
                        false => &mut x.another_account,
                    };
                    set.entry(key.into()).or_insert_with(|| "x".into());
                    x.params.insert(key.into(), "x".into());
                    let which = if x.one_account.contains_key(key) { "one_account" } else { "another_account" };
                    format!("{at}.params.{key}: is in {which} too: set it in one place")
                }
                "scope name" => {
                    let name = bad_name(&tc);
                    x.scopes.insert(name.clone(), scope(&tc));
                    format!("{at}.scopes.{name}: isn't a name: {NAME_RULE}")
                }
                "scope title" | "no requests" => {
                    let key = scope_key(x);
                    let s = &mut x.scopes[&key];
                    match mistake {
                        "scope title" => {
                            s.title = tc.draw(gs::text().alphabet(" \t\r\n").max_size(3));
                            format!("{at}.scopes.{key}.title: is empty")
                        }
                        _ => {
                            s.requests.clear();
                            format!("{at}.scopes.{key}.requests: a scope covers a request at least")
                        }
                    }
                }
                _ => {
                    let key = scope_key(x);
                    let requests = &mut x.scopes[&key].requests;
                    let j = index(&tc, requests.len());
                    let r = &mut requests[j];
                    let at = format!("{at}.scopes.{key}.requests[{j}]");
                    match mistake {
                        "method" => {
                            let wrong =
                                tc.draw(gs::sampled_from(vec!["get", "Post", "CONNECT", "OPTIONS", "TRACE", ""]));
                            r.method = wrong.into();
                            let methods = METHODS.join(", ");
                            format!("{at}.method: {wrong:?} isn't one of {methods}")
                        }
                        "pattern" => {
                            let start = r.url.strip_suffix('*').unwrap_or(&r.url);
                            r.url = tc.draw(gs::sampled_from(vec![
                                format!("{start}#x"),
                                start.replacen("://", "://someone@", 1),
                                format!("{start}*x*"),
                                format!("*{start}"),
                                "ftp://a.example/*".into(),
                                "http://a.example/*".into(),
                            ]));
                            let rule = "isn't an https:// address without a user name, password or fragment, maybe ending in *";
                            format!("{at}.url: {:?} {rule}", r.url)
                        }
                        _ => {
                            let (wrong, read) = tc.draw(gs::sampled_from(vec![
                                ("https://a.example*", "https://a.example/*"),
                                ("https://a.example:443/v", "https://a.example/v"),
                                ("https://A.example/", "https://a.example/"),
                                ("https://a.example/v/../w*", "https://a.example/w*"),
                                ("http://LOCALHOST:80", "http://localhost/"),
                            ]));
                            r.url = wrong.into();
                            format!("{at}.url: {wrong:?} is read as {read:?}: write it that way")
                        }
                    }
                }
            }
        }
    };
    assert_eq!(problem_with(&m, built_in), said);
}

/// Text the `Authorization` header's value of keys may have between the keys it names: printable
/// ASCII but braces.
const VALUE_TEXT: &str = "[ -z|~]{0,4}";

/// A query parameter's name.
const QUERY_NAME: &str = "[A-Za-z0-9._~-]{1,12}";

/// An account type signed in to with keys: some keys, where the user gets them, the
/// `Authorization` header's value they make (naming each key once at least, in any order, some
/// more than once), and scopes asking the service for nothing, maybe with a whoami at a host one
/// of their requests goes to.
#[hegel::test_helper]
fn keys_account_type(tc: &TestCase) -> AccountType {
    let names = tc.draw(gs::vecs(gs::from_regex(DRAWN_NAME)).min_size(1).max_size(3).unique(true));
    let keys: IndexMap<String, Key> = names.iter().map(|n| (n.clone(), Key { title: title(tc) })).collect();
    let mut named = tc.draw(gs::vecs(gs::sampled_from(names.clone())).max_size(4));
    let unnamed: Vec<String> = names.iter().filter(|n| !named.contains(n)).cloned().collect();
    named.extend(unnamed);
    let text = |tc: &TestCase| tc.draw(gs::from_regex(VALUE_TEXT).fullmatch(true));
    let value: String = named.iter().map(|n| format!("{}{{{n}}}", text(tc))).collect();
    let authorization = Some(format!("{value}{}", text(tc)));
    let unasking = |tc: &TestCase| Scope { asks: Vec::new(), ..scope(tc) };
    let mut scopes: IndexMap<String, Scope> = drawn_names(tc, 3).into_iter().map(|name| (name, unasking(tc))).collect();
    let whoami = tc.draw(gs::booleans()).then(|| {
        if scopes.is_empty() {
            scopes.insert("added-scope-name".into(), unasking(tc));
        }
        let requests: Vec<&ScopeRequest> = scopes.values().flat_map(|s| &s.requests).collect();
        let url = requests[index(tc, requests.len())].url.trim_end_matches('*').to_string();
        Whoami { url, ..whoami(tc) }
    });
    AccountType {
        name: tc.draw(gs::from_regex(DRAWN_NAME)),
        title: title(tc),
        kind: SignIn::Keys,
        authorize_url: None,
        token_url: None,
        token_exchange: None,
        client_id: None,
        revoke_url: None,
        token: TokenPlace::default(),
        redirect_port: None,
        always: Vec::new(),
        whoami,
        one_account: IndexMap::new(),
        another_account: IndexMap::new(),
        params: IndexMap::new(),
        keys,
        keys_url: Some(endpoint(tc)),
        authorization,
        scopes,
    }
}

/// Any account plugin signed in to with keys that keeps the rules validates, built in or not,
/// reads back as it was written from TOML and from JSON, fits the manifest's JSON Schema, and goes
/// in the `Authorization` header only, with the keys its value names; and any one thing wrong with it is
/// refused, saying where it is and what's wrong with it.
#[hegel::test(test_cases = 500)]
fn a_drawn_keys_account_type_is_valid_and_broken_in_one_place_is_refused_there(tc: TestCase) {
    let mut m = account_plugin(&tc, false);
    m.contributes.account_types = vec![keys_account_type(&tc)];
    assert_eq!((problem_with(&m, false), problem_with(&m, true)), (String::new(), String::new()));
    let json = serde_json::to_value(&m).unwrap();
    let toml = toml_of(&json);
    assert_eq!(Manifest::parse(&toml).unwrap(), m, "{toml}");
    assert_eq!(serde_json::from_value::<Manifest>(json.clone()).unwrap(), m);
    let schema = serde_json::to_value(schemars::schema_for!(Manifest)).unwrap();
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&json), "{json}");
    let x = &mut m.contributes.account_types[0];
    let place = x.place();
    assert_eq!((place.header.as_deref(), place.query.as_deref()), (Some("Authorization"), None), "{place:?}");
    assert_eq!(Some(&place.value), x.authorization.as_ref());
    let angled: IndexMap<&str, String> = x.keys.keys().map(|k| (k.as_str(), format!("<{k}>"))).collect();
    let filled = place.filled(|name| angled.get(name).map_or("", String::as_str));
    assert_eq!(filled, place.value.replace('{', "<").replace('}', ">"), "{place:?}");

    let mistake = tc.draw(gs::sampled_from(vec![
        "browser field",
        "no keys",
        "key name",
        "key title",
        "no keys_url",
        "keys_url",
        "no authorization",
        "unprintable",
        "brace",
        "undeclared",
        "names none",
        "unused",
        "asks",
        "whoami host",
    ]));
    let at = "contributes.account_types[0]";
    let at_value = format!("{at}.authorization");
    let said = match mistake {
        "browser field" => {
            let key = tc.draw(gs::sampled_from(vec![
                "authorize_url",
                "token_url",
                "token_exchange",
                "client_id",
                "revoke_url",
                "token",
                "redirect_port",
                "always",
                "one_account",
                "another_account",
                "params",
            ]));
            match key {
                "authorize_url" => x.authorize_url = Some(endpoint(&tc)),
                "token_url" => x.token_url = Some(endpoint(&tc)),
                "token_exchange" => x.token_exchange = Some(exchange(&tc)),
                "client_id" => x.client_id = Some(title(&tc)),
                "revoke_url" => x.revoke_url = Some(endpoint(&tc)),
                "token" => x.token = TokenPlace::Query(tc.draw(gs::from_regex(QUERY_NAME).fullmatch(true))),
                "redirect_port" => x.redirect_port = Some(tc.draw(gs::integers::<u16>().min_value(1024))),
                "always" => x.always.push("openid".into()),
                "one_account" => _ = x.one_account.insert("login_hint".into(), SUBJECT_PLACEHOLDER.into()),
                "another_account" => _ = x.another_account.insert("prompt".into(), "select_account".into()),
                _ => _ = x.params.insert("access_type".into(), "offline".into()),
            }
            format!("{at}.{key}: is for an account type signed in to in the browser, not with keys")
        }
        "no keys" => {
            x.keys.clear();
            format!("{at}.keys: an account type signed in to with keys has a key at least")
        }
        "key name" => {
            let name = bad_name(&tc);
            x.keys.insert(name.clone(), Key { title: title(&tc) });
            format!("{at}.keys.{name}: isn't a name: {NAME_RULE}")
        }
        "key title" => {
            let name = tc.draw(gs::sampled_from(x.keys.keys().cloned().collect::<Vec<_>>()));
            x.keys[&name].title = tc.draw(gs::text().alphabet(" \t\r\n").max_size(3));
            format!("{at}.keys.{name}.title: is empty")
        }
        "no keys_url" => {
            x.keys_url = None;
            format!("{at}.keys_url: is missing")
        }
        "keys_url" => {
            let wrong = tc.draw(gs::sampled_from(vec!["http://a.example/", "https://someone@a.example/", "a.example"]));
            x.keys_url = Some(wrong.into());
            format!("{at}.keys_url: {wrong:?} isn't an https:// address without a user name or password")
        }
        "no authorization" => {
            x.authorization = None;
            format!("{at_value}: is missing")
        }
        "unused" => {
            x.keys.insert("unused-key-name".into(), Key { title: title(&tc) });
            format!("{at}.keys.unused-key-name: goes nowhere: authorization doesn't name it")
        }
        "asks" => {
            if x.scopes.is_empty() {
                x.scopes.insert("added-scope-name".into(), Scope { asks: Vec::new(), ..scope(&tc) });
            }
            let key = tc.draw(gs::sampled_from(x.scopes.keys().cloned().collect::<Vec<_>>()));
            x.scopes[&key].asks.push(tc.draw(gs::from_regex(SERVICE_SCOPE)));
            let why = "asks the service for nothing: an account signed in to with keys was granted nothing there";
            format!("{at}.scopes.{key}.asks: {why}")
        }
        "whoami host" => {
            x.whoami = Some(Whoami { url: "https://elsewhere-host.example/me".into(), ..whoami(&tc) });
            let why = "isn't a host its scopes' requests go to: it's sent the keys";
            format!("{at}.whoami.url: elsewhere-host.example {why}")
        }
        _ => {
            let value = x.authorization.as_mut().unwrap();
            match mistake {
                "unprintable" => {
                    value.push(tc.draw(gs::sampled_from(vec!['\n', '\t', 'é', '\u{7f}'])));
                    format!("{at_value}: {value:?} isn't printable ASCII")
                }
                "brace" => {
                    value.push_str(tc.draw(gs::sampled_from(vec!["{", "}", "{}", "{A}", "}{", "{a b}"])));
                    format!("{at_value}: {value:?} has a {{ or }} that doesn't name a key")
                }
                "undeclared" => {
                    value.push_str("{undeclared-key-name}");
                    format!("{at_value}: {value:?} names {{undeclared-key-name}}, which isn't one of its keys")
                }
                _ => {
                    *value = tc.draw(gs::from_regex(VALUE_TEXT).fullmatch(true));
                    format!("{at_value}: {value:?} names none of its keys, like {{key}}")
                }
            }
        }
    };
    assert_eq!(problem_with(&m, false), said);
}

/// A scope's request covers its method (in any case) to its address, or to any address starting
/// with it when it ends in `*`: never another method, a host whose name starts the same, a user
/// name, another scheme or a path beside it.
#[hegel::test(test_cases = 100)]
fn a_pattern_covers_exactly_the_urls_it_prefixes(tc: TestCase) {
    let host = tc.draw(gs::from_regex("[a-z][a-z0-9]{0,7}(\\.[a-z][a-z0-9]{0,7}){0,2}"));
    let path = tc.draw(gs::from_regex("[a-z0-9/]{0,12}"));
    let start = format!("https://{host}/{path}");
    assert_eq!(Url::parse(&start).unwrap().as_str(), start, "a pattern is written as it's read");
    let star = tc.draw(gs::booleans());
    let method = tc.draw(gs::sampled_from(METHODS.to_vec()));
    let url = format!("{start}{}", if star { "*" } else { "" });
    let scope =
        Scope { title: "Scope".into(), asks: Vec::new(), requests: vec![ScopeRequest { method: method.into(), url }] };
    let covers = |method: &str, url: &str| scope.covers(method, &Url::parse(url).unwrap());
    let more = tc.draw(gs::from_regex("[a-z0-9/]{1,8}(\\?[a-z]=[a-z0-9]{0,4})?"));
    assert!(covers(method, &start));
    assert!(covers(&method.to_lowercase(), &start));
    assert_eq!(covers(method, &format!("{start}{more}")), star);
    let others: Vec<&str> = METHODS.iter().copied().filter(|m| *m != method).collect();
    assert!(!covers(tc.draw(gs::sampled_from(others)), &start));
    assert!(!covers(method, &format!("https://{host}.evil.example/{path}{more}")));
    assert!(!covers(method, &format!("https://someone@{host}/{path}{more}")));
    assert!(!covers(method, &format!("https://{host}@evil.example/{path}{more}")));
    assert!(!covers(method, &format!("http://{host}/{path}{more}")));
    assert_eq!(covers(method, &format!("https://{host}/~{path}{more}")), star && path.is_empty());
}

// Plugins asking for accounts by type, drawn whole, then broken in one place each.

/// A plugin asks for an account type and the scopes it needs, saying why, and needs a `net:`
/// capability for the requests their tokens go into.
#[test]
fn a_plugin_asking_for_accounts_needs_a_net_capability() {
    let asking = |capabilities: &str| {
        problem(&format!(
            "capabilities = [{capabilities}]\n[[accounts]]\ntype = \"google\"\nscopes = [\"youtube-upload\"]\n\
             why = \"To upload your sets\"\noptional = [{{ scopes = [\"read\"], why = \"To find an upload\" }}]\n"
        ))
    };
    assert_eq!(asking("\"net:www.googleapis.com\""), "");
    let said = "accounts[0]: goes into net/fetch requests, which needs a net:<host> capability";
    assert_eq!(asking(""), said);
    assert_eq!(asking("\"library:read\""), said);
}

/// What a plugin asks for of an account type: its scopes, then groups of them it can do without,
/// none asked for twice.
#[hegel::test_helper]
fn account_need(tc: &TestCase, name: String) -> AccountNeed {
    let mut scopes = tc.draw(gs::vecs(gs::from_regex(DRAWN_NAME)).min_size(1).max_size(6).unique(true));
    let mut rest = scopes.split_off(tc.draw(gs::integers::<usize>().min_value(1).max_value(scopes.len())));
    let mut optional = Vec::new();
    while !rest.is_empty() {
        let after = rest.split_off(tc.draw(gs::integers::<usize>().min_value(1).max_value(rest.len())));
        optional.push(OptionalScopes { scopes: rest, why: title(tc) });
        rest = after;
    }
    AccountNeed { name, scopes, why: title(tc), optional }
}

/// A plugin asking for an account type or more, granted a server for their requests, and nothing
/// else.
#[hegel::test_helper]
fn asking_plugin(tc: &TestCase) -> Manifest {
    let types = tc.draw(gs::vecs(gs::from_regex(DRAWN_NAME)).min_size(1).max_size(3).unique(true));
    Manifest {
        id: tc.draw(gs::from_regex(DRAWN_NAME)),
        name: title(tc),
        version: "0.1.0".into(),
        protocol: "^0.2".into(),
        capabilities: vec![Capability::Net(ACCOUNT_HOST.into())],
        accounts: types.into_iter().map(|name| account_need(tc, name)).collect(),
        contributes: Contributions::default(),
        net: Vec::new(),
        unknown: Vec::new(),
    }
}

/// Any plugin asking for accounts that keeps the rules validates, has the `accounts` feature,
/// reads back as it was written from TOML and from JSON, and fits the manifest's JSON Schema.
#[hegel::test(test_cases = 100)]
fn a_drawn_plugin_asking_for_accounts_is_valid_and_reads_back(tc: TestCase) {
    let m = asking_plugin(&tc);
    assert_eq!(problem_with(&m, tc.draw(gs::booleans())), "");
    assert!(!m.is_account_plugin());
    assert!((hooks::accounts::FEATURE.contributed)(&m));
    let json = serde_json::to_value(&m).unwrap();
    let toml = toml_of(&json);
    assert_eq!(Manifest::parse(&toml).unwrap(), m, "{toml}");
    assert_eq!(serde_json::from_value::<Manifest>(json.clone()).unwrap(), m);
    let schema = serde_json::to_value(schemars::schema_for!(Manifest)).unwrap();
    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&json), "{json}");
}

/// Group `g` of `x`'s scopes, as a mistake breaks it: where it is, its scopes and why. `scopes` is
/// the first, then each optional group.
fn scope_group<'a>(x: &'a mut AccountNeed, at: &str, g: usize) -> (String, &'a mut Vec<String>, &'a mut String) {
    match g {
        0 => (at.into(), &mut x.scopes, &mut x.why),
        _ => {
            let o = &mut x.optional[g - 1];
            (format!("{at}.optional[{}]", g - 1), &mut o.scopes, &mut o.why)
        }
    }
}

/// Any one thing wrong with a plugin asking for accounts that's otherwise right is refused, saying
/// where it is and what's wrong with it.
#[hegel::test(test_cases = 300)]
fn each_kind_of_accounts_mistake_is_refused_saying_where(tc: TestCase) {
    let mut m = asking_plugin(&tc);
    let mistakes =
        vec!["type", "type twice", "why", "no scopes", "scope name", "scope twice", "no net", "account plugin"];
    let mistake = tc.draw(gs::sampled_from(mistakes));
    if mistake == "type twice" && m.accounts.len() < 2 {
        m.accounts.push(account_need(&tc, "added-account-type".into()));
    }
    let i = index(&tc, m.accounts.len());
    let at = format!("accounts[{i}]");
    let said = match mistake {
        "type" => {
            let wrong = bad_name(&tc);
            m.accounts[i].name = wrong.clone();
            format!("{at}.type: {wrong:?} isn't a name: {NAME_RULE}")
        }
        "type twice" => {
            // The same as one before it, or a later one the same as it.
            let (j, k) = match i {
                0 => (0, 1 + index(&tc, m.accounts.len() - 1)),
                _ => (index(&tc, i), i),
            };
            m.accounts[k].name = m.accounts[j].name.clone();
            format!("accounts[{k}].type: {:?} is used twice", m.accounts[j].name)
        }
        "why" | "no scopes" | "scope name" => {
            let x = &mut m.accounts[i];
            let g = index(&tc, 1 + x.optional.len());
            let (at, scopes, why) = scope_group(x, &at, g);
            match mistake {
                "why" => {
                    *why = tc.draw(gs::text().alphabet(" \t\r\n").max_size(3));
                    format!("{at}.why: is empty")
                }
                "no scopes" => {
                    scopes.clear();
                    format!("{at}.scopes: asks for a scope at least")
                }
                _ => {
                    let wrong = bad_name(&tc);
                    scopes.push(wrong.clone());
                    format!("{at}.scopes[{}]: {wrong:?} isn't a name: {NAME_RULE}", scopes.len() - 1)
                }
            }
        }
        "scope twice" => {
            // One of its own scopes, or of a group before it, again at its end.
            let x = &mut m.accounts[i];
            let g = index(&tc, 1 + x.optional.len());
            let from = scope_group(x, &at, index(&tc, g + 1)).1.clone();
            let again = tc.draw(gs::sampled_from(from));
            let (at, scopes, _) = scope_group(x, &at, g);
            scopes.push(again.clone());
            format!("{at}.scopes[{}]: {again:?} is asked for twice", scopes.len() - 1)
        }
        "no net" => {
            m.capabilities.clear();
            "accounts[0]: goes into net/fetch requests, which needs a net:<host> capability".into()
        }
        _ => {
            m.capabilities.clear();
            m.contributes.account_types.push(account_type(&tc, false));
            let others = "no capabilities, accounts, net limits or other contributions";
            format!("contributes.account_types[0]: makes an account plugin, which has nothing else: {others}")
        }
    };
    assert_eq!(problem_with(&m, false), said);
}

/// A request names an account by type as `{account:<type>}`, or one access to it as
/// `{account:<type>/<access id>}`, the access's 8 lowercase hex digits; anything else names none.
#[hegel::test(test_cases = 100)]
fn an_account_placeholder_names_a_type_and_maybe_an_access(tc: TestCase) {
    let kind = tc.draw(gs::from_regex(DRAWN_NAME));
    let access = tc.draw(gs::optional(gs::from_regex("[0-9a-f]{8}").fullmatch(true)));
    let named = match &access {
        Some(access) => format!("{kind}/{access}"),
        None => kind.clone(),
    };
    assert_eq!(account_placeholder(&format!("{{account:{named}}}")), Some((kind.as_str(), access.as_deref())));
    let short_or_long = tc.draw(gs::from_regex("[0-9a-f]{0,7}|[0-9a-f]{9,12}").fullmatch(true));
    let not_hex = tc.draw(gs::from_regex("[0-9a-f]{0,7}[A-Fg-z/}][0-9a-f]{0,7}").fullmatch(true));
    let wrong = tc.draw(gs::sampled_from(vec![
        format!("{{account:{}}}", bad_name(&tc)),
        format!("{{account:{kind}/{short_or_long}}}"),
        format!("{{account:{kind}/{not_hex}}}"),
        format!("{{account:{named}/}}"),
        format!("{{account:{named}"),
        format!("account:{named}}}"),
        format!("{{account:{named}}}x"),
        format!("x{{account:{named}}}"),
        format!("Bearer {{account:{named}}}"),
    ]));
    assert_eq!(account_placeholder(&wrong), None, "{wrong}");
}

/// Every kind of contribution, in a manifest that's otherwise right, is refused saying where when
/// one of them is named what isn't a name, is named as another is, or has a blank title.
#[hegel::test(test_cases = 10)]
fn prop_every_kinds_names_and_titles_are_checked(tc: TestCase) {
    let drawn = manifest(&tc, false);
    for kind in CONTRIBUTIONS {
        for mistake in ["name", "twice", "title"] {
            let mut m = drawn.clone();
            let granted = match kind {
                "providers" => Some(Capability::LibraryRead),
                "sources" => Some(Capability::FsRead),
                _ => None,
            };
            if let Some(g) = granted.filter(|g| !m.capabilities.contains(g)) {
                m.capabilities.push(g);
            }
            let c = &mut m.contributes;
            add(c, kind, 1);
            let i = index(&tc, len(c, kind));
            let key = name_key(kind);
            let said = match mistake {
                "name" => {
                    let wrong = bad_name(&tc);
                    *names_and_titles(c, kind)[i].0 = wrong.clone();
                    format!("contributes.{kind}[{i}].{key}: {wrong:?} isn't a name: {NAME_RULE}")
                }
                "twice" => {
                    duplicate(c, kind, i);
                    let name = names_and_titles(c, kind)[i].0.clone();
                    format!("contributes.{kind}[{}].{key}: {name:?} is used twice", len(c, kind) - 1)
                }
                _ => {
                    *names_and_titles(c, kind)[i].1 = tc.draw(gs::text().alphabet(" \t\r\n").max_size(3));
                    format!("contributes.{kind}[{i}].title: is empty")
                }
            };
            assert_eq!(problem_with(&m, false), said, "{kind} {mistake}");
        }
    }
}

/// A key this side doesn't know, of any value.
#[hegel::test_helper]
fn unknown_key(tc: &TestCase) -> (String, Value) {
    let key = format!("zz-{}", tc.draw(gs::from_regex("[a-z]{1,6}").fullmatch(true)));
    let value = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => json!(tc.draw(gs::integers::<i64>())),
        1 => json!(tc.draw(gs::text().max_size(6))),
        _ => json!({ "colour": tc.draw(gs::booleans()) }),
    };
    (key, value)
}

/// Any keys a manifest has that this side doesn't know, at the top, among its contributions, in a
/// contribution or in one's optional table (an account type's whoami), are left out, each said
/// where it was; another plugin's manifest is still valid, and one locallibrary ships is refused
/// for the first of them.
#[hegel::test(test_cases = 30)]
fn prop_unknown_keys_are_left_out_saying_where_they_were(tc: TestCase) {
    let mut drawn = manifest(&tc, false);
    add(&mut drawn.contributes, "commands", 1);
    for m in [account_plugin(&tc, false), drawn] {
        let mut json = serde_json::to_value(&m).unwrap();
        let mut said = Vec::new();
        let places: [&[&str]; 4] = [
            &[],
            &["contributes"],
            &["contributes", "commands", "0"],
            &["contributes", "account_types", "0", "whoami"],
        ];
        for place in places {
            let Some(table) = place.iter().try_fold(&mut json, |v, key| match key.parse::<usize>() {
                Ok(i) => v.get_mut(i),
                Err(_) => v.get_mut(*key),
            }) else {
                continue;
            };
            if !tc.draw(gs::booleans()) {
                continue;
            }
            let (key, value) = unknown_key(&tc);
            table[&key] = value;
            let mut at = String::new();
            for part in place.iter().chain([&key.as_str()]) {
                match part.parse::<usize>() {
                    Ok(i) => at.push_str(&format!("[{i}]")),
                    Err(_) if at.is_empty() => at.push_str(part),
                    Err(_) => at.push_str(&format!(".{part}")),
                }
            }
            said.push(at);
        }
        let read = Manifest::parse(&toml_of(&json)).unwrap();
        let mut unknown = read.unknown().to_vec();
        unknown.sort();
        said.sort();
        assert_eq!(unknown, said);
        assert_eq!(Manifest { unknown: Vec::new(), ..read.clone() }, m);
        assert_eq!(problem_with(&read, false), "");
        let refused = read.unknown().first().map(|key| format!("{key}: isn't a key locallibrary knows"));
        assert_eq!(problem_with(&read, true), refused.unwrap_or_default());
    }
}

/// What isn't TOML, or isn't a manifest's shape (an id that isn't a string, a capability that
/// isn't one), isn't read as a manifest.
#[hegel::test(test_cases = 30)]
fn prop_what_isnt_a_manifest_isnt_read(tc: TestCase) {
    let n = tc.draw(gs::integers::<i64>());
    let cut = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let unended = &HEAD[..HEAD.find("demo").unwrap() + cut];
    for wrong in [unended.to_string(), format!("id = {n}\n{}", &HEAD[HEAD.find('\n').unwrap() + 1..])] {
        assert!(Manifest::parse(&wrong).is_err(), "{wrong}");
    }
    assert!(Manifest::parse(&format!("{HEAD}capabilities = [{n}]\n")).is_err());
    assert!(serde_json::from_value::<Capability>(json!(n)).is_err());
}

/// A place says its name as a manifest writes it; a param filled in from the selection is of a type
/// that holds it, and is what the context has selected, if anything.
#[hegel::test(test_cases = 100)]
fn prop_a_selection_fits_its_param_and_is_what_the_context_has(tc: TestCase) {
    for place in [Where::Track, Where::Playlist, Where::Drive] {
        assert_eq!(json!(place.name()), serde_json::to_value(place).unwrap());
    }
    let properties = [
        json!({ "type": "string" }),
        json!({ "type": "integer" }),
        json!({ "type": "array", "items": { "type": "integer" } }),
        json!({ "type": "array", "items": { "type": "string" } }),
    ];
    let froms = [
        json!("selection.tracks"),
        json!("selection.playlist"),
        json!("selection.drive"),
        json!("selection.deck"),
        json!(tc.draw(gs::integers::<i64>())),
    ];
    // Each property with each selection, and with none.
    for (property, from) in
        properties.iter().flat_map(|p| froms.iter().cloned().map(Some).chain([None]).map(move |f| (p, f)))
    {
        selected(property.clone(), from);
    }
    let context = Context {
        drive: tc.draw(gs::optional(gs::text().max_size(4))),
        playlist: tc.draw(gs::optional(gs::text().max_size(4))),
        tracks: tc.draw(gs::vecs(gs::integers::<u32>()).max_size(3)),
        ..Context::default()
    };
    let tracks = (!context.tracks.is_empty()).then(|| json!(context.tracks));
    assert_eq!(Selection::Tracks.value(&context), tracks);
    assert_eq!(Selection::Playlist.value(&context), context.playlist.clone().map(Value::from));
    assert_eq!(Selection::Drive.value(&context), context.drive.clone().map(Value::from));
}

/// `property` filled in `from` a selection (or not) is read as one exactly when it's of a type that
/// holds what it names.
fn selected(mut property: Value, from: Option<Value>) {
    let integers = property == json!({ "type": "array", "items": { "type": "integer" } });
    let string = property == json!({ "type": "string" });
    let expected = match from.as_ref().and_then(Value::as_str) {
        _ if from.is_none() => Ok(None),
        Some("selection.tracks") if integers => Ok(Some(Selection::Tracks)),
        Some("selection.playlist") if string => Ok(Some(Selection::Playlist)),
        Some("selection.drive") if string => Ok(Some(Selection::Drive)),
        Some("selection.tracks") => Err(format!("{} is integers, which the param isn't", from.clone().unwrap())),
        Some("selection.playlist" | "selection.drive") => {
            Err(format!("{} is a string, which the param isn't", from.clone().unwrap()))
        }
        _ => Err("is selection.tracks, selection.playlist or selection.drive".to_string()),
    };
    if let Some(from) = from {
        property["x-from"] = from;
    }
    assert_eq!(Selection::of(&property), expected, "{property}");
}

/// A `net:` grant reaches its own host, in any case, and `net:*.<domain>` any host below the
/// domain (not the domain itself); no other capability reaches a host, nor names one.
#[hegel::test(test_cases = 100)]
fn prop_a_net_grant_reaches_its_host_or_those_below_its_domain(tc: TestCase) {
    let labels: Vec<String> =
        tc.draw(gs::vecs(gs::from_regex("[a-z0-9]{1,4}").fullmatch(true)).min_size(1).max_size(4));
    let host = labels.join(".");
    let from = tc.draw(gs::integers::<usize>().max_value(labels.len()));
    let domain = labels[from..].join(".");
    let below = !domain.is_empty() && from > 0;
    let wildcard = Capability::Net(format!("*.{}", domain.to_uppercase()));
    assert_eq!(wildcard.reaches(&host), below, "{wildcard} {host}");
    assert!(Capability::Net(host.to_uppercase()).reaches(&host));
    assert_eq!(Capability::Net(domain.clone()).reaches(&host), from == 0);
    let named = tc.draw(gs::sampled_from(Capability::NAMED.to_vec()));
    assert!(!named.reaches(&host));
    assert_eq!((named.name().is_some(), wildcard.name()), (true, None));
}

/// A manifest has a feature's contributions exactly when the feature says it does: accounts it
/// asks for, analyzers with a structured field, `net` limits, commands planned first, or a
/// contribution of the feature's kind; never for the SDK's other features.
#[hegel::test(test_cases = 50)]
fn prop_a_feature_is_contributed_exactly_when_the_manifest_has_its_contributions(tc: TestCase) {
    let m = manifest(&tc, tc.draw(gs::booleans()));
    let c = &m.contributes;
    let structured = c.analyzers.iter().any(|a| a.fields.values().any(|t| t.is_structured()));
    for feature in crate::FEATURES {
        let has = match feature.name {
            "accounts" => !m.accounts.is_empty(),
            "analysis" => structured,
            "net" => !m.net.is_empty(),
            "plans" => c.commands.iter().any(|x| x.plan),
            "blobs" | "choose" | "views" => false,
            "events" => !c.events.is_empty(),
            kind => len(c, kind) > 0,
        };
        assert_eq!((feature.contributed)(&m), has, "{}", feature.name);
    }
}

/// A request's host is its URL's, without the `*` a pattern ends in; what isn't a URL has none.
#[hegel::test(test_cases = 50)]
fn prop_a_requests_host_is_its_urls(tc: TestCase) {
    let host = tc.draw(gs::from_regex("[a-z][a-z0-9]{0,6}(\\.[a-z]{2,4}){1,2}").fullmatch(true));
    let star = if tc.draw(gs::booleans()) { "*" } else { "" };
    let request = |url: String| ScopeRequest { method: "GET".into(), url };
    assert_eq!(request(format!("https://{host}/api/{star}")).host(), Some(host.clone()));
    assert_eq!(request(format!("{host}/api/{star}")).host(), None);
}

/// An account type's endpoint that names no host (`mailto:`, `data:`) isn't one; a keys account
/// type's authorization with a brace that names no key (closed before any opens, left open,
/// closed twice, or around what isn't a name) is refused.
#[hegel::test(test_cases = 50)]
fn prop_an_endpoint_without_a_host_and_a_stray_brace_are_refused(tc: TestCase) {
    let mut m = account_plugin(&tc, false);
    let who = tc.draw(gs::from_regex("[a-z]{1,6}").fullmatch(true));
    let url = tc.draw(gs::sampled_from(vec![format!("mailto:{who}@a.org"), format!("data:,{who}")]));
    m.contributes.account_types[0].authorize_url = Some(url.clone());
    assert_eq!(
        problem_with(&m, false),
        format!("contributes.account_types[0].authorize_url: {}", not_plain_https(&url))
    );
    let x = keys_account_type(&tc);
    let key = x.keys.keys().next().unwrap().clone();
    let text = tc.draw(gs::from_regex("[a-z ]{0,4}").fullmatch(true));
    for value in [
        format!("{text}}}{{{key}}}"),
        format!("{{{key}}}{text}{{{key}"),
        format!("{{{key}}}}}{text}"),
        format!("{{{key}}}{text}{{Not-a-name}}"),
    ] {
        let mut m = m.clone();
        m.contributes.account_types[0] = AccountType { authorization: Some(value.clone()), ..x.clone() };
        let said =
            format!("contributes.account_types[0].authorization: {value:?} has a {{ or }} that doesn't name a key");
        assert_eq!(problem_with(&m, false), said);
    }
}

/// Every source, kind of import, kind of provider and time a check runs says its name as a
/// manifest writes it, and a provider what it finds; every display format is named where it
/// doesn't fit its field.
#[hegel::test(test_cases = 20)]
fn prop_every_name_is_as_a_manifest_writes_it(tc: TestCase) {
    let named = |name: &str, value: Value| assert_eq!(json!(name), value);
    for from in [ExportFrom::Drive, ExportFrom::Set, ExportFrom::Mix] {
        named(from.name(), serde_json::to_value(from).unwrap());
    }
    for kind in [ImportKind::Fields, ImportKind::Cues, ImportKind::Tags, ImportKind::Sessions] {
        named(kind.name(), serde_json::to_value(kind).unwrap());
    }
    for (kind, found) in [(ProviderKind::Artwork, "cover art"), (ProviderKind::Metadata, "track details")] {
        named(kind.name(), serde_json::to_value(kind).unwrap());
        assert_eq!(kind.found(), found);
    }
    for when in [When::Check, When::Apply] {
        named(when.name(), serde_json::to_value(when).unwrap());
    }
    let formats = [FieldFormat::Key, FieldFormat::Duration, FieldFormat::Db, FieldFormat::Percent, FieldFormat::Meter];
    let field = tc.draw(gs::from_regex(DRAWN_NAME));
    for format in formats {
        // A key is for a string, the rest for numbers.
        let kind = if format == FieldFormat::Key { FieldType::Number } else { FieldType::String };
        let display = FieldDisplay { format: Some(format), ..FieldDisplay::default() };
        let analyzer = Analyzer {
            name: "a".into(),
            title: title(&tc),
            version: "1".into(),
            fields: IndexMap::from_iter([(field.clone(), kind)]),
            roles: IndexMap::new(),
            display: IndexMap::from_iter([(field.clone(), display)]),
        };
        let mut m = manifest(&tc, false);
        m.contributes.analyzers = vec![analyzer];
        let format_name = serde_json::to_value(format).unwrap();
        let said = format!(
            "contributes.analyzers[0].display.{field}.format: {field} is {}, which {} isn't for",
            a_field_of(kind),
            format_name.as_str().unwrap()
        );
        assert_eq!(problem_with(&m, false), said);
    }
    let shown = FieldDisplay::default();
    let b = tc.draw(gs::booleans());
    assert_eq!(shown.show(&FieldValue::Boolean(b)), if b { "yes" } else { "no" });
}

/// Any value is shown as its hints say: nothing as `-`, a boolean as yes or no, text and an
/// integer without a format as they are, then the unit; a number to its places (two at most when
/// it doesn't say), never as `-0`, a level in dB signed when it's more than nothing.
#[hegel::test(test_cases = 200)]
fn prop_values_are_shown_as_their_hints_say(tc: TestCase) {
    let unit = tc.draw(gs::optional(gs::from_regex("[A-Za-z%]{1,4}").fullmatch(true)));
    let precision = tc.draw(gs::optional(gs::integers::<u8>().max_value(FieldDisplay::MAX_PRECISION)));
    let plain = FieldDisplay { unit: unit.clone(), precision, ..FieldDisplay::default() };
    let with_unit = |s: String| unit.as_ref().map_or(s.clone(), |u| format!("{s} {u}"));
    let text = tc.draw(gs::text().max_size(6));
    let texts: Vec<String> = tc.draw(gs::vecs(gs::text().max_size(3)).max_size(3));
    let i = tc.draw(gs::integers::<i64>());
    let b = tc.draw(gs::booleans());
    assert_eq!(plain.show(&FieldValue::Null), "-");
    assert_eq!(plain.show(&FieldValue::Structured(json!([1]))), "-");
    assert_eq!(plain.show(&FieldValue::Boolean(b)), if b { "yes" } else { "no" });
    assert_eq!(plain.show(&FieldValue::String(text.clone())), with_unit(text));
    assert_eq!(plain.show(&FieldValue::Strings(texts.clone())), with_unit(texts.join(", ")));
    assert_eq!(plain.show(&FieldValue::Integer(i)), with_unit(i.to_string()));
    // Near nothing as often as not, where `-0` would show.
    let n = match tc.draw(gs::booleans()) {
        true => tc.draw(gs::floats::<f64>().min_value(-0.004).max_value(0.004)),
        false => tc.draw(gs::floats::<f64>().min_value(-1e6).max_value(1e6)),
    };
    let places = i32::from(precision.unwrap_or(2));
    let close = |shown: &str| {
        let back: f64 = shown.parse().unwrap();
        assert!((back - n).abs() <= 0.5 * 10f64.powi(-places) + 1e-9, "{n} as {shown}");
        assert!(!(shown.starts_with('-') && back == 0.0), "{n} as {shown}");
        back
    };
    for format in [None, Some(FieldFormat::Key)] {
        let shown = FieldDisplay { format, ..plain.clone() }.show(&FieldValue::Number(n));
        let number = unit.as_ref().map_or(shown.as_str(), |u| shown.strip_suffix(&format!(" {u}")).unwrap());
        close(number);
        if precision.is_none() && number.contains('.') {
            assert!(!number.ends_with('0'), "{n} as {number}");
        }
    }
    let db = FieldDisplay { format: Some(FieldFormat::Db), ..plain.clone() };
    let level = |value: &FieldValue| {
        let shown = db.show(value);
        let level = shown.strip_suffix(&format!(" {}", unit.as_deref().unwrap_or("dB"))).unwrap().to_string();
        (level.starts_with('+'), level.trim_start_matches('+').to_string())
    };
    assert_eq!(level(&FieldValue::Integer(i)).0, i > 0, "{i}");
    let (signed, shown) = level(&FieldValue::Number(n));
    let back = close(&shown);
    assert_eq!(signed, n > 0.0 && back != 0.0, "{n} as {shown}");
}

/// A track a provider looks up says how long it is only when it knows; an import's or an
/// operation's answer that isn't JSON isn't read, and a file holding one reads back as that.
#[hegel::test(test_cases = 50)]
fn prop_answers_read_and_write_as_they_are(tc: TestCase) {
    let duration = tc.draw(gs::integers::<u32>().max_value(3));
    let track = crate::TrackTags { id: 1, duration, ..crate::TrackTags::default() };
    let sent = serde_json::to_value(&track).unwrap();
    assert_eq!(sent.get("duration"), (duration > 0).then(|| json!(duration)).as_ref());
    let blob = tc.draw(gs::from_regex("[a-z0-9.]{0,6}").fullmatch(true));
    let answer = crate::ImportAnswer::from(crate::Blob { blob: blob.clone() });
    assert_eq!(serde_json::to_value(&answer).unwrap(), json!({ "blob": blob }));
    let cut = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let text = json!({ "blob": blob }).to_string();
    assert!(serde_json::from_str::<crate::ImportAnswer>(&text[..text.len() - cut]).is_err());
    assert!(serde_json::from_str::<crate::OpAnswer>(&text[..text.len() - cut]).is_err());
}

/// A setting takes values of its type: text for a string, a file or a folder, a number, a
/// boolean, one of an enum's choices, and text that isn't empty for a secret.
#[hegel::test(test_cases = 100)]
fn prop_a_setting_takes_values_of_its_type(tc: TestCase) {
    let kind = tc.draw(gs::sampled_from(SETTING_TYPES.to_vec()));
    let choices = vec!["a".to_string(), "b".into()];
    let setting = Setting { key: "s".into(), title: "S".into(), kind, default: None, choices: choices.clone() };
    let value = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => json!(tc.draw(gs::text().max_size(2))),
        1 => number(&tc),
        2 => json!(tc.draw(gs::booleans())),
        _ => json!(tc.draw(gs::sampled_from(choices.clone()))),
    };
    let takes = match kind {
        SettingType::String | SettingType::File | SettingType::Directory => value.is_string(),
        SettingType::Number => value.is_number(),
        SettingType::Boolean => value.is_boolean(),
        SettingType::Enum => value.as_str().is_some_and(|v| choices.iter().any(|c| c == v)),
        SettingType::Secret => value.as_str().is_some_and(|v| !v.is_empty()),
    };
    assert_eq!(setting.accepts(&value), takes, "{kind:?} {value}");
}

/// An account type signed in to in the browser puts its token where its `token` says, and is
/// refused without where it signs in, its client or its whoami; only Local Network's own token
/// exchange is one, for a built-in plugin.
#[hegel::test(test_cases = 50)]
fn prop_a_browser_account_type_says_where_its_token_goes_and_what_it_needs(tc: TestCase) {
    let m = account_plugin(&tc, false);
    let x = &m.contributes.account_types[0];
    let place = x.place();
    let expected = match &x.token {
        TokenPlace::Scheme(scheme) => (Some("Authorization".to_string()), None, format!("{scheme} {{token}}")),
        TokenPlace::Query(name) => (None, Some(name.clone()), "{token}".to_string()),
    };
    assert_eq!((place.header, place.query, place.value), expected);
    let at = "contributes.account_types[0]";
    for key in ["authorize_url", "client_id", "whoami"] {
        let mut m = m.clone();
        let x = &mut m.contributes.account_types[0];
        match key {
            "authorize_url" => x.authorize_url = None,
            "client_id" => x.client_id = None,
            _ => x.whoami = None,
        }
        assert_eq!(problem_with(&m, false), format!("{at}.{key}: is missing"));
    }
    let mut untitled = m.clone();
    let blank = Scope { title: tc.draw(gs::text().alphabet(" \t\n").max_size(3)), ..scope(&tc) };
    untitled.contributes.account_types[0].scopes.insert("added-scope".into(), blank);
    assert_eq!(problem_with(&untitled, false), format!("{at}.scopes.added-scope.title: is empty"));
    let service = tc.draw(gs::from_regex("[a-z0-9-]{1,12}").fullmatch(true));
    let mut ours = m.clone();
    let x = &mut ours.contributes.account_types[0];
    let elsewhere = format!("https://{}/api/token-exchange/{service}", tc.draw(gs::from_regex("[a-z]{1,8}\\.example")));
    (x.token_url, x.token_exchange) =
        (None, Some(format!("https://localnetwork.computer/api/token-exchange/{service}")));
    assert_eq!(problem_with(&ours, true), "");
    let mut both = ours.clone();
    both.contributes.account_types[0].token_url = Some(endpoint(&tc));
    let beside = format!("{at}.token_exchange: goes in token_url's place, not beside it");
    assert_eq!(problem_with(&both, true), beside);
    both.contributes.account_types[0].token_url = None;
    both.contributes.account_types[0].token_exchange = None;
    assert_eq!(problem_with(&both, true), format!("{at}.token_url: is missing, and so is token_exchange"));
    ours.contributes.account_types[0].token_exchange = Some(elsewhere.clone());
    let said = format!(
        "{at}.token_exchange: {elsewhere:?} isn't Local Network's token exchange \
         (https://localnetwork.computer/api/token-exchange/<service>)"
    );
    assert_eq!(problem_with(&ours, true), said);
    let unreachable =
        format!("http://{}/api/token-exchange/{service}", tc.draw(gs::from_regex("[a-z]{1,8}\\.example")));
    ours.contributes.account_types[0].token_exchange = Some(unreachable.clone());
    assert_eq!(problem_with(&ours, true), said.replace(&format!("{elsewhere:?}"), &format!("{unreachable:?}")));
    // A brace that isn't the subject's, in the params naming the account and in those that don't.
    let key = tc.draw(gs::sampled_from(AUTHORIZE_PARAMS.to_vec()));
    let stray = format!(
        "{}{}",
        tc.draw(gs::from_regex("[a-z]{0,3}").fullmatch(true)),
        tc.draw(gs::sampled_from(vec!["{", "}"]))
    );
    for (subject, why) in [
        (true, format!("{stray:?} has a {{ or }} besides {SUBJECT_PLACEHOLDER}")),
        (false, format!("{stray:?} has a {{ or }}: only one_account names an account")),
    ] {
        let mut m = m.clone();
        let x = &mut m.contributes.account_types[0];
        (x.one_account, x.another_account, x.params) = (IndexMap::new(), IndexMap::new(), IndexMap::new());
        let stray = IndexMap::from_iter([(key.to_string(), stray.clone())]);
        let (params, named) = match subject {
            true => (&mut x.one_account, "one_account"),
            false => (&mut x.another_account, "another_account"),
        };
        *params = stray;
        assert_eq!(problem_with(&m, false), format!("{at}.{named}.{key}: {why}"));
    }
}

/// A theme's token that's neither a colour nor a list of them, however deep, is refused by its
/// dotted name.
#[hegel::test(test_cases = 50)]
fn prop_a_token_thats_not_a_colour_is_refused_by_its_name(tc: TestCase) {
    let names: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[a-z]{1,6}").fullmatch(true)).min_size(1).max_size(3));
    let wrong = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => json!(tc.draw(gs::integers::<i64>())),
        1 => json!(tc.draw(gs::booleans())),
        _ => Value::Null,
    };
    let tokens = names.iter().rev().fold(wrong, |below, name| json!({ name: below }));
    let theme = Theme { name: "t".into(), title: "T".into(), tokens: tokens.as_object().unwrap().clone() };
    let said = "isn't #rrggbb (lowercase), or a list of them".to_string();
    assert_eq!(theme.theme().err(), Some((names.join("."), said)));
}

/// Params a list of paths is given in are taken when every one is absolute, and refused at the
/// first that isn't.
#[hegel::test(test_cases = 50)]
fn prop_a_list_of_paths_is_taken_only_absolute(tc: TestCase) {
    let schema = json!({
        "type": "object",
        "properties": { "files": { "type": "array", "items": { "type": "string" }, "x-path": true } }
    });
    let names: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[a-z]{1,6}").fullmatch(true)).max_size(3));
    let mut files: Vec<String> = names.iter().map(|n| absolute(n)).collect();
    assert_eq!(check_params(Some(&schema), &json!({ "files": files })), Ok(()));
    let relative = tc.draw(gs::from_regex("[a-z]{1,6}/[a-z]{1,6}").fullmatch(true));
    files.insert(tc.draw(gs::integers::<usize>().max_value(files.len())), relative.clone());
    let e = check_params(Some(&schema), &json!({ "files": files })).unwrap_err();
    assert_eq!(e.message, format!("invalid params: /files: {relative:?} isn't an absolute path"));
}

/// Every role reads back from its name, and any other name is refused saying what the roles are;
/// an analyzer's version that isn't one is refused.
#[hegel::test(test_cases = 50)]
fn prop_roles_read_from_their_names_and_versions_are_checked(tc: TestCase) {
    for role in Role::ALL {
        assert_eq!(role.name().parse::<Role>(), Ok(role));
    }
    let names: Vec<&str> = Role::ALL.iter().map(|r| r.name()).collect();
    let wrong = tc.draw(gs::text().max_size(8).filter(move |s: &String| !names.contains(&s.as_str())));
    let all: Vec<&str> = Role::ALL.iter().map(|r| r.name()).collect();
    assert_eq!(wrong.parse::<Role>(), Err(format!("{wrong:?} isn't a role: one of {}", all.join(", "))));
    let mut m = manifest(&tc, false);
    add(&mut m.contributes, "analyzers", 1);
    let i = index(&tc, m.contributes.analyzers.len());
    let version =
        tc.draw(gs::sampled_from(vec![String::new(), ".1".into(), "1 2".into(), "1/2".into(), "1".repeat(65)]));
    m.contributes.analyzers[i].version = version.clone();
    let said =
        format!("contributes.analyzers[{i}].version: {version:?} isn't a version: letters, digits, ., _, + and -");
    assert_eq!(problem_with(&m, false), said);
}

/// Every contribution with params or options is refused saying where when its schema isn't for
/// an object, or fills a param in from what isn't a selection; a command or operation offered
/// nowhere, or somewhere twice, is too; and an exporter that writes out nothing, or one source
/// twice.
#[hegel::test(test_cases = 10)]
fn prop_every_schema_place_and_source_is_checked(tc: TestCase) {
    let drawn = manifest(&tc, false);
    for kind in ["commands", "operations", "exporters", "importers", "sources"] {
        for mistake in ["not an object", "x-from", "flag", "positional", "hints"] {
            let mut m = drawn.clone();
            if kind == "sources" && !m.capabilities.contains(&Capability::FsRead) {
                m.capabilities.push(Capability::FsRead);
            }
            add(&mut m.contributes, kind, 1);
            let i = index(&tc, len(&m.contributes, kind));
            let (slot, at) = schema_of(&mut m.contributes, kind, i);
            let property = |p: Value| Some(json!({ "type": "object", "properties": { "p": p } }));
            let said = match mistake {
                "not an object" => {
                    *slot = Some(json!({ "type": "string" }));
                    format!("{at}: must be a JSON Schema for an object (type = \"object\")")
                }
                "x-from" => {
                    *slot = property(json!({ "type": "string", "x-from": "selection.deck" }));
                    format!("{at}.properties.p.x-from: is selection.tracks, selection.playlist or selection.drive")
                }
                "flag" => {
                    let flag = bad_name(&tc);
                    *slot = property(json!({ "type": "string", "x-cli": { "flag": flag } }));
                    format!("{at}.properties.p.x-cli: its flag {flag:?} isn't a name: {NAME_RULE}")
                }
                "hints" => {
                    // Hints that aren't hints: a key they don't have, or not a table.
                    let hints = match tc.draw(gs::booleans()) {
                        true => json!({ "colour": tc.draw(gs::integers::<i64>()) }),
                        false => json!(tc.draw(gs::integers::<i64>())),
                    };
                    *slot = property(json!({ "type": "string", "x-cli": hints }));
                    let read = CliHints::deserialize(&hints).unwrap_err();
                    format!("{at}.properties.p.x-cli: {read}")
                }
                _ => {
                    let kind = tc.draw(gs::sampled_from(vec!["integer", "boolean", "number"]));
                    *slot = property(json!({ "type": kind, "x-cli": { "positional": true } }));
                    format!("{at}.properties.p.x-cli: only a string is positional")
                }
            };
            assert_eq!(problem_with(&m, false), said, "{kind} {mistake}");
        }
    }
    for kind in ["commands", "operations"] {
        for twice in [false, true] {
            let mut m = drawn.clone();
            add(&mut m.contributes, kind, 1);
            let i = index(&tc, len(&m.contributes, kind));
            let offered = match kind {
                "commands" => &mut m.contributes.commands[i].offered,
                _ => &mut m.contributes.operations[i].offered,
            };
            let said = match twice {
                false => {
                    offered.clear();
                    "names no place: track, playlist, drive or collection".to_string()
                }
                true => {
                    let again = offered[index(&tc, offered.len())];
                    offered.push(again);
                    format!("{} is there twice", again.name())
                }
            };
            assert_eq!(problem_with(&m, false), format!("contributes.{kind}[{i}].where: {said}"));
        }
    }
    let mut m = drawn.clone();
    add(&mut m.contributes, "exporters", 1);
    let i = index(&tc, m.contributes.exporters.len());
    let at = format!("contributes.exporters[{i}].source");
    m.contributes.exporters[i].source = ExportSources::Several(Vec::new());
    assert_eq!(problem_with(&m, false), format!("{at}: an exporter writes out a drive, a set or a mix"));
    m.contributes.exporters[i].source = ExportSources::Several(vec![ExportFrom::Drive, ExportFrom::Drive]);
    assert_eq!(problem_with(&m, false), format!("{at}: drive is listed twice"));
}

/// An operation's answer and an import's read back as they were sent, a question or a plan, a
/// file or what was read; one that's a plan, or a read, gone wrong says what's wrong with that;
/// a source's track leaves out a year and a rating it doesn't know.
#[hegel::test(test_cases = 50)]
fn prop_operation_and_import_answers_read_back_as_sent(tc: TestCase) {
    let title = title(&tc);
    let choice = crate::Choice {
        title: title.clone(),
        param: "p".into(),
        multi: false,
        items: vec![crate::ChoiceItem { label: title.clone(), detail: None, value: json!(1) }],
        more: Vec::new(),
    };
    let plan = crate::OpPlan { summary: title.clone(), warnings: Vec::new(), edits: Vec::new(), inputs: Vec::new() };
    for answer in [crate::OpAnswer::from(choice), crate::OpAnswer::from(plan)] {
        let sent = serde_json::to_value(&answer).unwrap();
        assert_eq!(serde_json::from_str::<crate::OpAnswer>(&sent.to_string()).unwrap(), answer);
    }
    let wrong = serde_json::from_str::<crate::OpAnswer>(r#"{ "summary": 5 }"#).unwrap_err();
    assert!(wrong.to_string().starts_with("invalid type: integer `5`, expected a string"), "{wrong}");
    let read = crate::Imported { tracks: Vec::new(), ..crate::Imported::default() };
    for answer in [crate::ImportAnswer::from(read), crate::ImportAnswer::from(crate::Blob { blob: title })] {
        let sent = serde_json::to_value(&answer).unwrap();
        assert_eq!(serde_json::from_str::<crate::ImportAnswer>(&sent.to_string()).unwrap(), answer);
    }
    assert!(serde_json::from_str::<crate::ImportAnswer>(r#"{ "tracks": 5 }"#).is_err());
    let (year, rating) = (tc.draw(gs::integers::<u16>().max_value(2)), tc.draw(gs::integers::<u8>().max_value(2)));
    let track = crate::SourceTrack { path: "/a.mp3".into(), year, rating, ..crate::SourceTrack::default() };
    let sent = serde_json::to_value(&track).unwrap();
    assert_eq!(
        (sent.get("year"), sent.get("rating")),
        ((year > 0).then(|| json!(year)).as_ref(), (rating > 0).then(|| json!(rating)).as_ref())
    );
}
