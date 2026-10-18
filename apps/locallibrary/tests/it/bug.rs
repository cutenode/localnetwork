//! `locallibrary bug`: signed out, it gives GitHub's bug report form filled in; signed in, with a
//! space to post to, it posts the report from the account to a pretend data server on 127.0.0.1,
//! and falls back to GitHub when it can't. Every run says `--no-browser` or `--json`, so nothing
//! opens a browser, and nothing reaches the network.

use serde_json::Value;

use crate::support::{Env, Run};

/// GitHub's form, as the runs print it.
const FORM: &str = "https://github.com/cutenode/localnetwork/issues/new?template=locallibrary-bug.yml&";

/// The form's address in what a run printed.
fn form_in(out: &str) -> &str {
    let start = out.find(FORM).unwrap_or_else(|| panic!("no form address in:\n{out}"));
    out[start..].split_whitespace().next().unwrap()
}

/// `locallibrary bug ARGS` with `space` as the space bug reports go to.
fn bug(env: &Env, space: Option<&str>, args: &[&str]) -> Run {
    let mut c = env.command();
    if let Some(space) = space {
        c.env("LOCALLIBRARY_USERINPUT_SPACE", space);
    }
    let out = c.arg("bug").args(args).output().unwrap();
    Run {
        out: String::from_utf8(out.stdout).unwrap(),
        err: String::from_utf8(out.stderr).unwrap(),
        code: out.status.code().unwrap(),
    }
}

#[test]
fn signed_out_it_gives_githubs_form_filled_in() {
    let env = Env::new();
    let r = bug(&env, None, &["--no-browser", "--title", "Sync stops at 99%", "-m", "It stopped & said 100%"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let url = form_in(&r.out);
    assert!(url.contains("&version=locallibrary%20"), "{url}");
    assert!(url.contains("&os="), "{url}");
    assert!(url.contains("&title=Sync%20stops%20at%2099%25"), "{url}");
    assert!(url.contains("&what-happened=It%20stopped%20%26%20said%20100%25"), "{url}");
    assert!(!r.out.contains("tip:") && !r.err.contains("tip:"), "no setup tip: {}", r.err);

    let r = bug(&env, None, &["--json"]);
    let v: Value = serde_json::from_str(&r.out).unwrap();
    assert_eq!((r.code, &v["opened"]), (0, &Value::Bool(false)), "{}", r.out);
    assert!(v["url"].as_str().unwrap().starts_with(FORM) && !v["url"].as_str().unwrap().contains("title="), "{v}");
}

#[cfg(feature = "account")]
mod posting {
    use serde_json::{Value, json};

    use super::{FORM, bug, form_in};
    use crate::mock_pds::{MockPds, Writes};
    use crate::support::Env;
    use crate::support::account::{DID, PUBLISHING, sign_in};

    const DISCUSSION: &str = "app.userinput.discussion";
    const REPORTING: [&str; 2] = ["atproto", "repo:app.userinput.discussion?action=create"];
    const CREATE: &str = "com.atproto.repo.createRecord";

    /// A pretend data server for `DID` holding a space of its own, and the space's at:// URI.
    fn server_with_a_space() -> (MockPds, String) {
        let pds = MockPds::start();
        pds.put(DID, "app.userinput.space", "3lspace", json!({ "name": "locallibrary" }));
        (pds, format!("at://{DID}/app.userinput.space/3lspace"))
    }

    #[test]
    fn signed_in_it_posts_the_report_from_the_account() {
        let (pds, space) = server_with_a_space();
        let env = Env::new();
        sign_in(&env.store, &pds.url, &REPORTING, json!({}));
        let args = ["--no-browser", "--title", "Sync stops", "-m", "It stopped."];

        // Without a terminal or --yes it shows the report and stops (exit status 3), offering GitHub.
        let r = bug(&env, Some(&space), &args);
        assert_eq!(r.code, 3, "{}\n{}", r.out, r.err);
        assert!(r.out.contains("Bug report from @dj.example.com") && r.out.contains("  It stopped."), "{}", r.out);
        assert!(r.out.contains("Version: locallibrary ") && r.out.contains("Operating system: "), "{}", r.out);
        assert!(form_in(&r.out).contains("title=Sync%20stops"), "{}", r.out);
        assert!(pds.calls(CREATE).is_empty());

        let r = bug(&env, Some(&space), &[&["--yes"][..], &args].concat());
        assert_eq!(r.code, 0, "{}", r.err);
        // The last request, after the server asked for its DPoP nonce.
        let sent = pds.calls(CREATE).pop().unwrap().json();
        assert_eq!((sent["repo"].as_str(), sent["collection"].as_str()), (Some(DID), Some(DISCUSSION)));
        let record = &sent["record"];
        let (_, space_cid) = pds.record(DID, "app.userinput.space", "3lspace").unwrap();
        assert_eq!(record["space"], json!({ "uri": space, "cid": space_cid }));
        assert_eq!((&record["title"], &record["tags"]), (&json!("Sync stops"), &json!(["bug", "locallibrary"])));
        assert!(record["body"].as_str().unwrap().starts_with("It stopped.\n\nVersion: locallibrary "), "{record}");
        // The pretend server numbers the records it keys itself after those it holds.
        let posted = pds.record(DID, DISCUSSION, "3mockrecord02").expect("the report is in the account's repository");
        assert_eq!(posted.0, *record);
        assert!(r.out.contains(&format!("https://userinput.app/d/{DID}/3mockrecord02")), "{}", r.out);
        assert!(!r.out.contains(FORM), "{}", r.out);

        // No space to post to: GitHub's form.
        let v = env.json(&["--yes", "bug", "--title", "Again"]);
        assert!(v["url"].as_str().unwrap().starts_with(FORM), "{v}");
        let r = bug(&env, Some(&space), &["--json", "--yes", "--title", "Again"]);
        let v: Value = serde_json::from_str(&r.out).unwrap();
        assert_eq!(v["outcome"], "applied", "{}", r.out);
        assert_eq!(v["report"]["url"], format!("https://userinput.app/d/{DID}/3mockrecord03"));
        assert_eq!(v["plan"]["title"], "Again");
    }

    #[test]
    fn when_it_cant_post_it_gives_githubs_form_with_what_was_typed() {
        let (pds, space) = server_with_a_space();
        let env = Env::new();
        let args = ["--yes", "--no-browser", "--title", "Sync stops", "-m", "It stopped."];

        // Signed in only to publish, without a terminal to sign in again on.
        sign_in(&env.store, &pds.url, &PUBLISHING, json!({}));
        let r = bug(&env, Some(&space), &args);
        assert_eq!(r.code, 0, "{}", r.err);
        assert!(r.out.contains("can't post bug reports yet"), "{}", r.out);
        assert!(form_in(&r.out).contains("what-happened=It%20stopped."), "{}", r.out);

        // The server refuses it.
        sign_in(&env.store, &pds.url, &REPORTING, json!({}));
        pds.set_writes(Writes::RefuseRecord);
        let r = bug(&env, Some(&space), &args);
        assert_eq!(r.code, 0, "{}", r.err);
        assert!(r.out.contains("couldn't post it") && r.out.contains("Record is too big"), "{}", r.out);
        let url = form_in(&r.out);
        assert!(url.contains("title=Sync%20stops") && url.contains("what-happened=It%20stopped."), "{url}");
        pds.set_writes(Writes::Accept);
        let tried = pds.calls(CREATE).len();

        // The space isn't there, or its owner's server can't be used.
        let gone = format!("at://{DID}/app.userinput.space/3lgone");
        for space in [gone.as_str(), "at://did:web:localhost/app.userinput.space/3lspace"] {
            let r = bug(&env, Some(space), &args);
            assert_eq!(r.code, 0, "{}", r.err);
            assert!(r.out.contains("couldn't post it") && r.out.contains(FORM), "{space}: {}", r.out);
        }
        assert_eq!(pds.calls(CREATE).len(), tried);

        // Signed in, with no title and nothing to ask on: GitHub's form takes it from there.
        let r = bug(&env, Some(&space), &["--no-browser"]);
        assert_eq!(r.code, 0, "{}", r.err);
        assert!(r.out.contains(FORM), "{}", r.out);

        // A space that isn't one is a mistake to fix, not to work around.
        let r = bug(&env, Some("https://userinput.app/s/locallibrary"), &args);
        assert_eq!(r.code, 1, "{}", r.out);
        assert!(r.err.contains("isn't a userinput.app space"), "{}", r.err);

        // Too long to post: said before anything is sent.
        let long = "a".repeat(301);
        let r = bug(&env, Some(&space), &["--yes", "--no-browser", "--title", &long]);
        assert_eq!(r.code, 1, "{}", r.out);
        assert!(r.err.contains("the title is too long to post: 301 characters, 300 at most"), "{}", r.err);
        assert_eq!(pds.calls(CREATE).len(), tried);
    }
}
