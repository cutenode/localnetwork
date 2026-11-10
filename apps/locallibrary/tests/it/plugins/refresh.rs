//! `refresh --then` and `refresh.plan`'s `operations`: plugins' operations run as a step of the
//! refresh, in its one plan.

use super::*;

/// `--then 'PLUGIN.OPERATION KEY=VALUE...'` runs the operation in the refresh's plan, its words
/// split as a shell would and its params read as its schema says, each `--then` in the order given,
/// the drive named before or after them. One that can't be planned fails the plan, and one that
/// isn't an operation and its params is a usage error naming it.
#[test]
fn refresh_then_runs_a_plugins_operation() {
    let env = env();
    let plan = env.ok(&["refresh", "USB", "--then", "echo.playlist name=Fresh", "--dry-run"]);
    assert!(plan.contains("Run the plugins' operations\n   • make the playlist Fresh\n"), "{plan}");
    let applied = env.json(&[
        "--yes",
        "refresh",
        "--then",
        "echo.playlist name=Fresh",
        "--then",
        "echo.playlist name='Late Night'",
        "USB",
    ]);
    assert_eq!(applied["outcome"], "applied", "{applied}");
    let steps = applied["report"]["steps"].as_array().unwrap();
    let ran = steps.iter().find(|s| s["step"] == "operations").unwrap();
    assert_eq!(ran["reports"].as_array().unwrap().len(), 2, "{ran}");
    let listed = env.ok(&["playlist", "ls", "USB"]);
    assert!(listed.contains("Fresh") && listed.contains("Late Night"), "{listed}");

    assert!(env.fails(1, &["refresh", "USB", "--then", "nope.playlist"]).contains("not found: plugin \"nope\""));
    let e = env.fails(2, &["refresh", "USB", "--then", "echo.playlist colour=red"]);
    assert!(e.contains("this echo.playlist operation has no option \"colour\""), "{e}");
    for bad in ["", "  ", "playlist name=x"] {
        let e = env.fails(2, &["refresh", "USB", "--then", bad]);
        assert!(e.contains(&format!("--then takes 'PLUGIN.OPERATION KEY=VALUE...', not {bad:?}")), "{e}");
    }
    let e = env.fails(2, &["refresh", "USB", "--then", "echo.playlist name='Late"]);
    assert!(e.contains(r#"--then "echo.playlist name='Late" has a quote it doesn't close"#), "{e}");
}

/// A `--then` of [`refresh_then_plans_each_operation_or_names_the_first_it_cant_read`].
#[derive(Debug, Clone)]
enum Then {
    /// `echo.playlist name='NAME'`.
    Playlist(String),
    /// An operation of a plugin that isn't there.
    Missing,
    /// Words that aren't PLUGIN.OPERATION and its params.
    NoOperation(&'static str),
    Unclosed,
    UnknownOption,
}

/// For any `--then`s, each an operation, one of a plugin that isn't there, or one that can't be
/// read (no PLUGIN.OPERATION, a quote left open, an option it doesn't have): a refresh's dry run
/// fails as a usage error naming the first that can't be read; with none, it fails not finding
/// the plugin that isn't there; else its plan makes each playlist, in the order given.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn refresh_then_plans_each_operation_or_names_the_first_it_cant_read(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    let env = env();
    let playlist = || gs::text().alphabet("abZ").min_size(1).max_size(8).map(Then::Playlist);
    let then = || {
        hegel::one_of!(
            playlist(),
            playlist(),
            gs::just(Then::Missing),
            gs::sampled_from(vec!["", "  ", "playlist name=x"]).map(Then::NoOperation),
            gs::just(Then::Unclosed),
            gs::just(Then::UnknownOption),
        )
    };
    let thens: Vec<Then> = tc.draw(gs::vecs(then()).min_size(1).max_size(3));
    let words: Vec<String> = thens
        .iter()
        .map(|t| match t {
            Then::Playlist(name) => format!("echo.playlist name='{name}'"),
            Then::Missing => "nope.playlist".into(),
            Then::NoOperation(w) => w.to_string(),
            Then::Unclosed => "echo.playlist name='Late".into(),
            Then::UnknownOption => "echo.playlist colour=red".into(),
        })
        .collect();
    let mut args = vec!["refresh", "USB", "--dry-run"];
    args.extend(words.iter().flat_map(|w| ["--then", w.as_str()]));
    let r = env.run(&args);
    let said = format!("{thens:?}: exit {}\n{}{}", r.code, r.out, r.err);
    let unread = thens.iter().zip(&words).find(|(t, _)| !matches!(t, Then::Playlist(_) | Then::Missing));
    if let Some((t, w)) = unread {
        let why = match t {
            Then::Unclosed => format!("--then {w:?} has a quote it doesn't close"),
            Then::UnknownOption => "this echo.playlist operation has no option \"colour\"".into(),
            _ => format!("--then takes 'PLUGIN.OPERATION KEY=VALUE...', not {w:?}"),
        };
        assert!(r.code == 2 && r.err.contains(&why), "{why}: {said}");
        return;
    }
    if thens.iter().any(|t| matches!(t, Then::Missing)) {
        assert!(r.code == 1 && r.err.contains("not found: plugin \"nope\""), "{said}");
        return;
    }
    assert_eq!(r.code, 0, "{said}");
    let mut at = r.out.find("Run the plugins' operations\n").unwrap_or_else(|| panic!("{said}"));
    for t in &thens {
        let Then::Playlist(name) = t else { unreachable!() };
        let made = format!("   • make the playlist {name}\n");
        at += r.out[at..].find(&made).unwrap_or_else(|| panic!("{made:?} in order: {said}")) + made.len();
    }
}

/// The sidecar's `refresh.plan` takes `operations`; one that can't be planned is refused, the
/// error naming it.
#[test]
fn the_sidecars_refresh_takes_operations() {
    let env = env();
    let drive = path(&env.volumes.join("USB")).to_string();
    let mut s = Rpc::start(env.command());
    let refresh = |ops: Value| json!({ "drive": drive, "skip": ["pull"], "operations": ops });
    let planned =
        s.call(1, "refresh.plan", refresh(json!([{ "operation": "echo.playlist", "params": { "name": "Rpc" } }])));
    let plan = &planned["result"];
    let step = plan["steps"].as_array().unwrap().iter().find(|s| s["step"] == "operations").unwrap();
    assert_eq!((&step["runs"], &step["plans"][0]["operation"]), (&json!(true), &json!("echo.playlist")), "{planned}");
    let applied = s.call(2, "refresh.apply", json!({ "plan_id": plan["plan_id"] }));
    assert!(applied["result"]["stopped"].is_null(), "{applied}");

    let refused = s.call(3, "refresh.plan", refresh(json!([{ "operation": "echo.nope" }])));
    assert_eq!(refused["error"]["data"]["kind"], "not_found", "{refused}");
    assert_eq!(refused["error"]["data"]["fields"]["operation"], "echo.nope", "{refused}");
}
