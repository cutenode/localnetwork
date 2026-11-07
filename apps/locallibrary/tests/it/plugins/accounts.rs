//! Accounts on the command line: `account` listing them, signing in to one through an account
//! plugin at the mock service (see [`super::shared_accounts`]) and out, `plugin access` giving the
//! echo plugin access, and a plugin's destination asking which account it may use only where it
//! can ask (see [`cli`]). Here, a sign-in the browser doesn't come back to approved, on the command
//! line and in the sidecar.

mod cli;

use hegel::TestCase;
use hegel::generators as gs;
use serde_json::json;

use super::shared_accounts::Mock;
use crate::support::Rpc;

/// Signing in prints the service's sign-in page (with PKCE) when it isn't opened, and gives up
/// once the browser hasn't come back in time. A plugin to give access that isn't one is refused
/// naming it.
#[test]
fn signing_in_sends_the_browser_to_the_service_and_waits_for_it() {
    let m = Mock::new();
    let (code, out, err) = m.run(&["account", "mock", "sign-in", "--no-browser", "--timeout", "1"]);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    let page = format!("{}/authorize?", m.server.url);
    let link = err.lines().find(|l| l.trim_start().starts_with(&page)).unwrap_or_default();
    for part in [
        "response_type=code",
        "client_id=locallibrary-test",
        "code_challenge_method=S256",
        "scope=openid",
        "redirect_uri=http%3A%2F%2F127.0.0.1%3A",
    ] {
        assert!(link.contains(part), "{part}: {err}");
    }
    assert!(err.contains("no answer from the browser within 1 second; sign in again"), "{err}");
    let (code, _, err) = m.run(&["account", "mock", "sign-in", "--for", "nope"]);
    assert!(code == 1 && err.contains("not found: plugin \"nope\""), "{err}");
}

/// How a step of [`a_sign_in_the_browser_doesnt_approve_says_why_and_is_over`] ends a sign-in.
#[derive(Debug, Clone, Copy)]
enum Ending {
    /// In the sidecar, the address of another sign-in pasted.
    Foreign,
    /// In the sidecar, declined at the service, pasted.
    Declined,
    /// In the sidecar, stopped by the service, saying why, pasted.
    Stopped,
    /// In the sidecar, never back, waited for no time at all.
    Waited,
    /// In the sidecar, cancelled.
    Cancelled,
    /// `account mock sign-in --paste`, with no terminal to paste into.
    Paste,
    /// `account mock sign-in --no-browser --timeout 0`.
    GaveUp,
}

/// For any sign-ins, in the sidecar or on the command line, that the browser doesn't come back to
/// approved (another sign-in's address pasted, declined, stopped by the service, not back in time,
/// pasted with no terminal) or that are cancelled: each fails as `account`, saying why, and is
/// over (a cancelled one can't be finished); none keeps an account, and nothing printed or answered
/// holds a token.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_sign_in_the_browser_doesnt_approve_says_why_and_is_over(tc: TestCase) {
    let m = Mock::new();
    let mut s = Rpc::start(m.command());
    let endings = tc.draw(
        gs::vecs(gs::sampled_from(vec![
            Ending::Foreign,
            Ending::Declined,
            Ending::Stopped,
            Ending::Waited,
            Ending::Cancelled,
            Ending::Paste,
            Ending::GaveUp,
        ]))
        .min_size(1)
        .max_size(5),
    );
    let mut said = String::new();
    for (i, ending) in endings.into_iter().enumerate() {
        let id = i as i64 * 10;
        if let Ending::Paste | Ending::GaveUp = ending {
            let (how, why): (&[&str], &str) = match ending {
                Ending::Paste => (&["--paste"], "--paste needs a terminal to paste into"),
                _ => {
                    (&["--no-browser", "--timeout", "0"], "no answer from the browser within 0 seconds; sign in again")
                }
            };
            let (code, out, err) = m.run(&[&["account", "mock", "sign-in"][..], how].concat());
            assert_eq!((code, out.as_str()), (1, ""), "step {i}: {ending:?}");
            assert!(
                err.contains(&format!("Sign in: open this link\n  {}/authorize?", m.server.url)),
                "step {i}: {err}"
            );
            assert!(err.contains(why), "step {i}: {err}");
            said.push_str(&err);
            continue;
        }
        let begun = s.call(id, "accounts.sign_in.begin", json!({ "type": "mock" }));
        let r = &begun["result"];
        let (_, query) = r["url"].as_str().unwrap().split_once('?').unwrap();
        let state = form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == "state").unwrap().1;
        let paste = |s: &mut Rpc, back: String| {
            let url = format!("{}?{back}", r["redirect_uri"].as_str().unwrap());
            s.call(id + 1, "accounts.sign_in.paste", json!({ "id": r["id"], "url": url }))
        };
        let (answer, why) = match ending {
            Ending::Foreign => {
                (paste(&mut s, "code=c0de&state=guess".into()), "that address is from another sign-in: sign in again")
            }
            Ending::Declined => (
                paste(&mut s, format!("error=access_denied&state={state}")),
                "the sign-in was declined, so locallibrary wasn't given access",
            ),
            Ending::Stopped => (
                paste(&mut s, format!("error=server_error&error_description=down+for+now&state={state}")),
                "your account's server stopped the sign-in: down for now",
            ),
            Ending::Waited => (
                s.call(id + 1, "accounts.sign_in.finish", json!({ "id": r["id"], "timeout_secs": 0 })),
                "no answer from the browser within 0 seconds; sign in again",
            ),
            _ => {
                let cancelled = s.call(id + 1, "accounts.sign_in.cancel", json!({ "id": r["id"] }));
                assert_eq!(cancelled["result"], json!({ "cancelled": true }), "step {i}");
                let finished = s.call(id + 2, "accounts.sign_in.finish", json!({ "id": r["id"], "timeout_secs": 1 }));
                assert_eq!(finished["error"]["data"]["kind"], "not_found", "step {i}: {finished}");
                continue;
            }
        };
        assert_eq!(answer["error"]["data"]["kind"], "account", "step {i}: {answer}");
        assert!(answer["error"]["message"].as_str().unwrap_or_default().starts_with(why), "step {i}: {answer}");
        let again = s.call(id + 2, "accounts.sign_in.cancel", json!({ "id": r["id"] }));
        assert_eq!(again["result"], json!({ "cancelled": false }), "step {i}: {ending:?} isn't over");
        said.push_str(&answer.to_string());
    }
    assert_eq!(s.call(1000, "accounts.list", json!({}))["result"]["accounts"], json!([]));
    assert!(!m.leaked(&said), "a token was printed or answered");
}
