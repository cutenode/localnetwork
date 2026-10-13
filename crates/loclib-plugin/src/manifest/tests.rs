use serde_json::json;

use super::*;

const HEAD: &str = "id = \"demo\"\nname = \"Demo\"\nversion = \"0.1.0\"\nprotocol = \"^0.1\"\n";

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
    assert!(m.validate(true).unwrap_err().to_string().contains("\"Go\" isn't a name"));
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
    let m = Manifest::parse(&HEAD.replace("^0.1", "^^")).unwrap();
    assert!(m.validate(false).unwrap_err().to_string().starts_with("protocol: "));
    let m = Manifest::parse(&HEAD.replace("^0.1", "^0.2")).unwrap();
    assert_eq!(m.validate(false).unwrap_err().to_string(), "protocol: \"^0.2\" doesn't include 0.1.0");
}

#[test]
fn unknown_capabilities_and_keys_are_refused() {
    let e = problem("capabilities = [\"library:write\"]");
    assert!(e.contains("unknown capability \"library:write\": one of library:read, "), "{e}");
    let e = problem("capabilities = [\"net:\"]");
    assert!(e.contains("\"net:\" names no host"), "{e}");
    assert!(problem("capabilities = [\"net:https://example.com\"]").contains("names no host"));
    assert!(problem("homepage = \"https://example.com\"").contains("unknown field `homepage`"));
    // The host works out whether an operation is destructive from its edits.
    let declared = "[[contributes.operations]]\nname = \"o\"\ntitle = \"O\"\ndestructive = true\n";
    assert!(problem(declared).contains("unknown field `destructive`"));
    assert!(problem("[[contributes.widgets]]\nname = \"w\"").contains("unknown field `widgets`"));
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
    let twice = "[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nfields = { x = \"number\" }\n\
                 [[contributes.analyzers]]\nname = \"a\"\ntitle = \"B\"\nfields = { x = \"number\" }\n";
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
    let e = problem("[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nfields = { bpm = \"float\" }\n");
    assert!(e.contains("unknown variant `float`"), "{e}");
    let e = problem("[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nfields = {}\n");
    assert_eq!(e, "contributes.analyzers[0].fields: an analyzer needs a field");
    let e = problem("[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nfields = { BPM = \"number\" }\n");
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
        (FieldType::String, json!(null), false),
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
        ("analyzers", "name = \"a\"\nfields = { x = \"number\" }"),
        ("exporters", "format = \"f\"\nfolder = true"),
        ("settings", "key = \"s\"\ntype = \"string\""),
    ] {
        let e = problem(&format!("[[contributes.{kind}]]\ntitle = \"\"\n{rest}\n"));
        assert_eq!(e, format!("contributes.{kind}[0].title: is empty"));
    }
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
    let two = "[[contributes.analyzers]]\nname = \"a\"\ntitle = \"A\"\nfields = { bpm = \"number\" }\n\
               [[contributes.analyzers]]\nname = \"b\"\ntitle = \"B\"\nfields = { key = \"string\", bpm = \"number\" }\n";
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
        ("secret", json!("hunter2"), false),
    ];
    for (kind, value, accepted) in cases {
        assert_eq!(setting(kind).accepts(&value), accepted, "{kind} {value}");
    }
}
