use serde_json::json;

use super::*;

#[test]
fn detail_rides_along_without_changing_the_error() {
    let e = Error::conflict(Reason::NotSaved, "DNA's history isn't in the store yet")
        .with("drive", "/Volumes/DNA")
        .action(Action::call("pull", json!({ "drive": "/Volumes/DNA" }), "Pull DNA"))
        .action(Action::retry(json!({ "force": true }), "Delete them anyway"));
    assert_eq!((e.code(), e.to_string().as_str()), ("conflict", "DNA's history isn't in the store yet"));
    assert!(matches!(e.inner(), Error::Conflict(_)));
    let d = e.detail();
    assert_eq!(d.reason, Some(Reason::NotSaved));
    assert_eq!(d.fields["drive"], "/Volumes/DNA");
    assert_eq!(
        serde_json::to_value(&d.actions).unwrap()[1],
        json!({ "label": "Delete them anyway", "params": { "force": true } })
    );

    let space = Error::NoSpace { path: "/Volumes/A".into(), needed: 10, available: 3 }.detail();
    assert_eq!(
        space.fields,
        json!({ "path": "/Volumes/A", "needed": 10, "available": 3 }).as_object().unwrap().clone()
    );
    let admin = Error::Permission("formatting /dev/disk4 needs administrator rights".into()).detail();
    assert_eq!((admin.reason, admin.actions[0].elevate), (Some(Reason::NeedsAdmin), true));
}

/// Front ends switch on the codes, so they stay what they are; the values an error is about
/// come with it, whatever detail was added on top.
#[test]
fn every_kind_keeps_its_code_and_says_what_its_about() {
    let io = || io::Error::other("gone");
    let cases = [
        (Error::Io { path: "/a".into(), source: io() }, "io", json!({ "path": "/a" })),
        (Error::NotAnExport("/b".into()), "not_an_export", json!({ "path": "/b" })),
        (Error::parse("/c", "bad"), "parse", json!({ "path": "/c" })),
        (Error::Locked("/d".into()), "locked", json!({ "path": "/d" })),
        (
            Error::Corrupt { path: "/e".into(), expected: "ab".into(), actual: "cd".into() },
            "corrupt",
            json!({ "path": "/e", "expected": "ab", "actual": "cd" }),
        ),
        (
            Error::External { command: "diskutil".into(), message: "no".into() },
            "external",
            json!({ "command": "diskutil" }),
        ),
        (Error::stale("DNA", "changed"), "stale_plan", json!({})),
        (Error::ExportExt("x".into()), "pdb", json!({})),
        (Error::OneLibrary("x".into()), "onelibrary", json!({})),
        (Error::Image("x".into()), "image", json!({})),
        (Error::Tags("x".into()), "tags", json!({})),
        (Error::Account("x".into()), "account", json!({})),
        (Error::Peer("x".into()), "peer", json!({})),
        (Error::Cancelled, "cancelled", json!({})),
        (
            Error::NoSpace { path: "/f".into(), needed: 2, available: 1 },
            "no_space",
            json!({ "path": "/f", "needed": 2, "available": 1 }),
        ),
        (Error::Permission("x".into()), "permission", json!({})),
    ];
    for (e, code, fields) in cases {
        let e = e.with("drive", "DNA");
        let mut want = fields.as_object().unwrap().clone();
        want.insert("drive".into(), "DNA".into());
        assert_eq!((e.code(), e.detail().fields), (code, want), "{e}");
    }
    assert_eq!(Error::stale("DNA", "changed").to_string(), "DNA changed since the plan was made; plan again");
    // An action's params are an object, or nothing.
    assert!(Action::call("pull", json!("DNA"), "Pull").params.is_empty());
}

#[test]
fn panics_say_what_they_said() {
    let said = |f: fn()| panic_text(&*std::panic::catch_unwind(f).unwrap_err());
    assert_eq!(said(|| panic!("plain")), "plain");
    assert_eq!(said(|| panic!("{}", String::from("formatted"))), "formatted");
    assert_eq!(said(|| std::panic::panic_any(7)), "a panic");
}

/// However detail is piled on, an error keeps its code and message; the last reason given
/// wins, fields given win over what the kind says (last given first), actions keep their
/// order, and a permission error always offers to run elevated.
#[hegel::test(test_cases = 300)]
fn detail_never_changes_what_the_error_is(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let kinds: Vec<fn() -> Error> = vec![
        || Error::Io { path: "/a".into(), source: io::Error::other("gone") },
        || Error::NotAnExport("/b".into()),
        || Error::parse("/c", "bad"),
        || Error::invalid("no"),
        || Error::NotFound("x".into()),
        || Error::Conflict("both".into()),
        || Error::stale("DNA", "changed"),
        || Error::Corrupt { path: "/e".into(), expected: "ab".into(), actual: "cd".into() },
        || Error::NoSpace { path: "/f".into(), needed: 2, available: 1 },
        || Error::Cancelled,
        || Error::Locked("/d".into()),
        || Error::Pdb("x".into()),
        || Error::ExportExt("x".into()),
        || Error::OneLibrary("x".into()),
        || Error::Image("x".into()),
        || Error::Tags("x".into()),
        || Error::External { command: "diskutil".into(), message: "no".into() },
        || Error::Permission("root".into()),
        || Error::Account("x".into()),
        || Error::Peer("x".into()),
    ];
    let k = tc.draw(gs::integers::<usize>().max_value(kinds.len() - 1));
    let base = kinds[k]();
    let (code, message) = (base.code(), base.to_string());
    let own = base.detail();
    let mut e = base;
    let mut reason = None;
    let mut fields: Map<String, Value> = Map::new();
    let mut actions = Vec::new();
    let reasons = [Reason::Exists, Reason::NotSaved, Reason::NeedsAdmin, Reason::ChooseDrive];
    let keys = ["path", "drive", "needed", "command"];
    for step in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => {
                let r = reasons[tc.draw(gs::integers::<usize>().max_value(reasons.len() - 1))];
                e = e.because(r);
                reason = Some(r);
            }
            1 => {
                let key = keys[tc.draw(gs::integers::<usize>().max_value(keys.len() - 1))];
                e = e.with(key, step);
                fields.insert(key.into(), step.into());
            }
            2 => {
                let a = Action::call("pull", serde_json::json!({ "step": step }), format!("Step {step}"));
                e = e.action(a.clone());
                actions.push(a);
            }
            _ => {
                let a = match tc.draw(gs::booleans()) {
                    true => Action::elevate("Elevate"),
                    false => Action::retry(serde_json::json!([step]), "Again"),
                };
                assert!(a.params.is_empty(), "params are an object, or nothing");
                e = e.action(a.clone());
                actions.push(a);
            }
        }
    }
    assert_eq!((e.code(), e.to_string()), (code, message.clone()));
    assert_eq!((e.inner().code(), e.inner().to_string()), (code, message));
    let d = e.detail();
    let permission = matches!(e.inner(), Error::Permission(_));
    assert_eq!(d.reason, reason.or(own.reason));
    assert_eq!(e.reason(), d.reason);
    let mut want = own.fields.clone();
    for (k, v) in fields {
        want.insert(k, v);
    }
    assert_eq!(d.fields, want);
    if permission && !actions.iter().any(|a| a.elevate) {
        actions.push(Action::elevate("Run it as an administrator"));
    }
    assert_eq!(d.actions, actions);
    // Round trips through JSON, as the sidecar sends it.
    let back: Detail = serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
    assert_eq!(back, d);
}

/// An I/O error given a path says which, and keeps its cause.
#[hegel::test(test_cases = 50)]
fn io_errors_name_their_path(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let name = tc.draw(gs::sampled_from(vec!["export.pdb", "Ünï/ANLZ0000.DAT", "a b"]));
    let r: io::Result<()> = Err(io::Error::new(io::ErrorKind::NotFound, "no such file"));
    let e = r.at(Path::new("/Volumes/USB").join(name)).unwrap_err();
    assert_eq!(e.code(), "io");
    assert_eq!(e.detail().fields["path"], format!("/Volumes/USB/{name}"));
    assert!(e.to_string().ends_with("no such file"), "{e}");
}

/// What the other crates' errors become: the code their kind says (a file that can't be read is
/// `io` with its path, one that isn't what it should be `parse`, a stop `cancelled`, a bad
/// argument `invalid`) and their message, detail added on top or not.
#[hegel::test(test_cases = 400)]
fn errors_from_the_other_crates_keep_their_kind_and_message(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let path = PathBuf::from(tc.draw(gs::sampled_from(vec!["/Volumes/A/x.mp3", "/tmp/é.flac"])));
    let message = tc.draw(gs::text().alphabet("abc é:").min_size(1).max_size(8));
    let io = || io::Error::other(message.clone());
    let mixid = |kind: u8| match kind {
        0 => loclib_mixid::Error::Io { path: path.clone(), source: io() },
        1 => loclib_mixid::Error::Parse { path: path.clone(), message: message.clone() },
        2 => loclib_mixid::Error::Invalid(message.clone()),
        _ => loclib_mixid::Error::Cancelled(loclib_progress::Cancelled),
    };
    let mixid_code = |kind: u8| ["io", "parse", "invalid", "cancelled"][usize::from(kind)];
    let kind = tc.draw(gs::integers::<u8>().max_value(3));
    // (the error, its code, whether its message (or path) comes through)
    let (e, code, says): (Error, &str, bool) = match tc.draw(gs::integers::<u8>().max_value(8)) {
        0 => (mixid(kind).into(), mixid_code(kind), kind < 3),
        1 => {
            let e = match tc.draw(gs::integers::<u8>().max_value(3)) {
                0 => (loclib_analysis::Error::Audio(mixid(kind)), mixid_code(kind), kind < 3),
                1 => (loclib_analysis::Error::Analyzer { path: path.clone(), message: message.clone() }, "parse", true),
                2 => (loclib_analysis::Error::Annotation { line: 7, message: message.clone() }, "invalid", true),
                _ => (loclib_analysis::Error::Cancelled(loclib_progress::Cancelled), "cancelled", false),
            };
            (e.0.into(), e.1, e.2)
        }
        2 => {
            use loclib_transcode::Error as T;
            let e = match tc.draw(gs::integers::<u8>().max_value(6)) {
                0 => (T::Decode(mixid(kind)), mixid_code(kind), kind < 3),
                1 => (T::Io { path: path.clone(), source: io() }, "io", true),
                2 => (T::Encode { path: path.clone(), message: message.clone() }, "parse", true),
                3 => (T::Tags { path: path.clone(), message: message.clone() }, "parse", true),
                4 => (T::TooShort { path: path.clone(), frames: 3, source_rate: 8000, rate: 44_100 }, "parse", false),
                5 => (T::Invalid(message.clone()), "invalid", true),
                _ => (T::Cancelled(loclib_progress::Cancelled), "cancelled", false),
            };
            (e.0.into(), e.1, e.2)
        }
        3 => {
            use loclib_mysettings::Error as M;
            let e = match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => (M::UnknownSetting(message.clone()), "invalid", true),
                1 => (
                    M::InvalidValue { key: "jog_mode", value: message.clone(), expected: vec!["vinyl"] },
                    "invalid",
                    true,
                ),
                _ => (
                    M::Parse { file: loclib_mysettings::SettingsFile::MySetting, message: message.clone() },
                    "parse",
                    true,
                ),
            };
            (e.0.into(), e.1, e.2)
        }
        4 => (loclib_progress::Cancelled.into(), "cancelled", false),
        5 => (loclib_equipment::UnknownModels(vec![message.clone()]).into(), "invalid", true),
        6 => (loclib_tracklist::Error::NoTitleColumn { found: message.clone() }.into(), "invalid", true),
        7 => (rekordbox_pdb::PdbError::UnknownField(message.clone()).into(), "pdb", true),
        _ => (Error::Io { path: path.clone(), source: io() }, "io", true),
    };
    let detailed = e.to_string();
    assert_eq!(e.code(), code, "{e:?}");
    if says {
        assert!(detailed.contains(&message) || detailed.contains(&path.display().to_string()), "{detailed:?}");
    }
    if code == "io" {
        assert_eq!(e.detail().fields["path"], path.display().to_string());
    }
    // Detail on top changes neither.
    let with = e.because(Reason::NotSaved).with("drive", "/Volumes/A");
    assert_eq!((with.code(), with.to_string()), (code, detailed));
}

/// What a panic said, whether it panicked with a literal or a formatted message.
#[hegel::test(test_cases = 100)]
fn a_panics_text_is_what_it_said(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let said = tc.draw(gs::text().max_size(12));
    let literal = std::panic::catch_unwind(|| std::panic::panic_any("fixed")).unwrap_err();
    assert_eq!(panic_text(&*literal), "fixed");
    let formatted = std::panic::catch_unwind(|| std::panic::panic_any(said.clone())).unwrap_err();
    assert_eq!(panic_text(&*formatted), said);
    let other = std::panic::catch_unwind(|| std::panic::panic_any(7u8)).unwrap_err();
    assert_eq!(panic_text(&*other), "a panic");
}
