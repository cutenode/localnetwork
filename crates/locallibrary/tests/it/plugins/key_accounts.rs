//! Accounts signed in to with keys, through an account plugin whose type says where they go, at a
//! mock service on this computer that says who a pair of keys is for at its `/whoami`: the keys
//! the user enters, checked, kept once per subject and filled in only where the type says; a
//! service refusing them passed on to the plugin, the account still signed in; and no key
//! anywhere but the store's secrets.

use std::collections::VecDeque;

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::plugin::{AccountSignIn, AccountSignedIn};

use super::net::{Reply, Server, ok};
use super::shared_accounts::{Mock, WITH, asking, mock_for, options};
use super::*;
use crate::mock_oauth::{json_reply, keys_account_plugin};

/// Where a [`Goes`] puts the keys: the `Authorization` header.
const HEADER: &str = r#"{ header = "Authorization", value = "LOW {access}:{secret}" }"#;
/// Or the query, the access key alone.
const ACCESS: &str = r#"{ query = "accesskey", value = "{access}" }"#;
/// Or the query, the secret key alone.
const SECRET: &str = r#"{ query = "secretkey", value = "{secret}" }"#;

/// Where an account type puts the keys, and what a request naming them elsewhere is told.
#[derive(Debug, Clone, Copy)]
struct Goes {
    places: &'static [&'static str],
    refused: &'static str,
}

impl Goes {
    fn toml(self) -> String {
        format!("[{}]", self.places.join(", "))
    }

    /// The query parameter the place `query` names gets, of `keys`, when it's one of its places.
    fn query(self, query: &str, keys: &(String, String)) -> Option<String> {
        let place = self.places.iter().find(|p| p.contains(&format!("\"{query}\"")))?;
        Some(if place.contains("{access}") { keys.0.clone() } else { keys.1.clone() })
    }

    fn header(self) -> bool {
        self.places.contains(&HEADER)
    }
}

const GOES: [Goes; 4] = [
    Goes { places: &[HEADER], refused: "an account goes only in the Authorization header: {account:<type>}" },
    Goes {
        places: &[ACCESS, SECRET],
        refused: "this account goes only in the query: accesskey={account:<type>}, secretkey={account:<type>}",
    },
    Goes {
        places: &[HEADER, ACCESS],
        refused: "this account goes only in the Authorization header: {account:<type>} or the query: \
                  accesskey={account:<type>}",
    },
    Goes {
        places: &[ACCESS, HEADER],
        refused: "this account goes only in the Authorization header: {account:<type>} or the query: \
                  accesskey={account:<type>}",
    },
];

/// What the mock service says: who each access key is for, at `/whoami` (a 403 for one it doesn't
/// know), and what it answers other requests with, in turn, before answering what they were sent.
#[derive(Default)]
struct Service {
    known: IndexMap<String, Value>,
    script: VecDeque<Reply>,
}

/// The access key a request to the service was sent: in the `Authorization` header, else as
/// `accesskey` in its query.
fn access_sent(got: &super::net::Got) -> Option<String> {
    let header = got.headers.get("authorization").and_then(|a| a.strip_prefix("LOW "));
    let header = header.and_then(|a| a.split_once(':')).map(|(access, _)| access.to_string());
    let query = got.target.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let mut query = form_urlencoded::parse(query.as_bytes());
    header.or_else(|| query.find(|(k, _)| k == "accesskey").map(|(_, v)| v.into_owned()))
}

/// The mock service, the echo plugin needing `read` of a mock account, and an account plugin
/// whose keys go where `goes` says, asking the service who they're for when `whoami`.
struct Keyed {
    service: Arc<Mutex<Service>>,
    server: Server,
    f: Fixture,
}

fn keyed(goes: Goes, whoami: bool) -> Keyed {
    let service = Arc::new(Mutex::new(Service::default()));
    let state = service.clone();
    let server = Server::new(move |_, got| {
        let mut s = state.lock().unwrap();
        if got.target.split('?').next() == Some("/whoami") {
            return match access_sent(got).and_then(|a| s.known.get(&a).cloned()) {
                Some(who) => json_reply(200, who),
                None => json_reply(403, json!({ "error": "unknown keys" })),
            };
        }
        s.script.pop_front().unwrap_or_else(|| {
            ok(format!("{} {}", got.target, got.headers.get("authorization").map_or("-", String::as_str)))
        })
    });
    let asked = format!("{}/whoami", server.url);
    let manifest = keys_account_plugin(&server.url, whoami.then_some(asked.as_str()), &goes.toml());
    let echo = asking(&["read"], &[]);
    let f = Fixture::new(move |dir| vec![spec(dir, &echo, &[]), spec(dir, &manifest, &[])], options());
    Keyed { service, server, f }
}

/// A pair of keys, as entered.
fn entered(access: &str, secret: &str) -> IndexMap<String, String> {
    IndexMap::from_iter([("access".to_string(), access.to_string()), ("secret".to_string(), secret.to_string())])
}

impl Keyed {
    /// Starts signing in to a mock account for the echo plugin.
    fn begin(&self) -> AccountSignIn {
        self.f.plugins.begin_account_sign_in("mock", WITH, Some("echo"), None, &[], &self.f.store).unwrap()
    }

    /// Signs in to a mock account for the echo plugin with `keys`, named `name`.
    fn sign_in(&self, keys: &IndexMap<String, String>, name: Option<&str>) -> locallibrary::Result<AccountSignedIn> {
        self.f.plugins.finish_account_sign_in_with_keys(&self.begin().id, keys, name)
    }

    fn listed(&self) -> Vec<locallibrary::plugin::Account> {
        self.f.plugins.store_accounts(&self.f.store).unwrap()
    }

    /// The echo plugin's request to `path` at the service naming `named` in its `Authorization`
    /// header.
    fn fetch(&self, path: &str, named: &str) -> Value {
        self.f.fetch(json!({ "url": format!("{}{path}", self.server.url), "headers": { "Authorization": named } }))
    }

    /// Whether any of `keys` is out in the plugins' logs, the store but its secrets, or `answers`.
    fn leaked(&self, answers: &[&Value], keys: &[&str]) -> bool {
        let seen = self.f.seen(answers);
        keys.iter().any(|k| seen.contains(k))
    }

    /// Everything in the store's secrets.
    fn secrets(&self) -> String {
        let files = walkdir::WalkDir::new(self.f.store.root().join("secrets")).into_iter().flatten();
        let files = files.filter(|e| e.file_type().is_file()).map(|e| std::fs::read(e.path()).unwrap());
        files.map(|b| String::from_utf8_lossy(&b).into_owned()).collect()
    }
}

/// Where a step of [`keys_land_exactly_where_their_account_type_says`] names the account, as the
/// whole value.
#[derive(Debug, Clone, Copy)]
enum Named {
    Authorization,
    /// Another header.
    Header,
    /// A query parameter.
    Query(&'static str),
    Body,
}

/// For an account type whose keys go in the `Authorization` header, query parameters or both, and
/// any requests naming the account anywhere: one naming it where the type says reaches the server
/// with that place's value there, the keys nowhere else in the request, and one sent in the query
/// is answered to the plugin as the placeholder that named it; one naming it anywhere else is
/// refused, saying every place it goes, before anything is sent; and no key gets into the plugins'
/// logs or the store but its secrets.
#[hegel::test(test_cases = 16, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn keys_land_exactly_where_their_account_type_says(tc: TestCase) {
    let goes = tc.draw(gs::sampled_from(GOES.to_vec()));
    let k = keyed(goes, false);
    let keys = ("ak-0123456789".to_string(), "sk-9876543210".to_string());
    k.sign_in(&entered(&keys.0, &keys.1), Some("me")).unwrap();
    let named = [
        Named::Authorization,
        Named::Header,
        Named::Query("accesskey"),
        Named::Query("secretkey"),
        Named::Query("other"),
        Named::Body,
    ];
    let steps = tc.draw(gs::vecs(gs::sampled_from(named.to_vec())).max_size(4));
    let mut answers = Vec::new();
    for (i, step) in steps.into_iter().enumerate() {
        let (path, sent) = (format!("/r/s{i}?page=2"), k.server.got().len());
        let url = format!("{}{path}", k.server.url);
        let answer = k.f.fetch(match step {
            Named::Authorization => {
                json!({ "url": url, "headers": { "Authorization": "{account:mock}", "X-Other": "x" } })
            }
            Named::Header => json!({ "url": url, "headers": { "X-Token": "{account:mock}" } }),
            Named::Query(name) => json!({ "url": format!("{url}&{name}={{account:mock}}") }),
            Named::Body => json!({ "url": url, "method": "POST", "body": "{account:mock}" }),
        });
        let query = match step {
            Named::Query(name) => goes.query(name, &keys).map(|value| (name, value)),
            _ => None,
        };
        let lands = matches!(step, Named::Authorization) && goes.header() || query.is_some();
        if !lands {
            assert_eq!(answer["error"]["message"], goes.refused, "step {i}: not refused, or not saying where it goes");
            assert_eq!(k.server.got().len(), sent, "step {i}: something was sent");
            answers.push(answer);
            continue;
        }
        let got = k.server.got().last().cloned().unwrap_or_default();
        let (target, authorization) = match &query {
            Some((name, value)) => (format!("{path}&{name}={value}"), None),
            None => (path.clone(), Some(format!("LOW {}:{}", keys.0, keys.1))),
        };
        assert!(got.target == target, "step {i}: the address isn't what it was named in");
        assert!(got.headers.get("authorization") == authorization.as_ref(), "step {i}: the header isn't");
        let elsewhere = |v: &String| v.contains(&keys.0) || v.contains(&keys.1);
        let others = got.headers.iter().filter(|(name, _)| *name != "authorization").any(|(_, v)| elsewhere(v));
        assert!(!others && !elsewhere(&got.body), "step {i}: a key is elsewhere in the request");
        if let Some((name, _)) = query {
            // The server says the address it was asked at back: as the plugin named it.
            assert_eq!(answer["result"]["body"], format!("{path}&{name}={{account:mock}} -"), "step {i}");
            answers.push(answer);
        }
    }
    assert!(!k.leaked(&answers.iter().collect::<Vec<_>>(), &[&keys.0, &keys.1]), "a key is out where it may not be");
}

/// Who a step of [`a_keys_account_is_kept_once_per_subject_with_the_keys_last_entered`] signs in
/// as, and the keys each has: two pairs.
const PEOPLE: [&str; 2] = ["alice", "bob"];

/// The `n`th pair of keys of `person`.
fn keys_of(person: &str, n: usize) -> (String, String) {
    (format!("ak-{person}-{n}-0000"), format!("sk-{person}-{n}-0000"))
}

/// A step of [`a_keys_account_is_kept_once_per_subject_with_the_keys_last_entered`], naming people
/// by their place in [`PEOPLE`].
#[derive(Debug, Clone, Copy)]
enum Step {
    /// Signing in with a person's keys.
    SignIn(usize, usize),
    SignOut(usize),
    /// The echo plugin's request through its access to a person's account.
    Fetch(usize),
}

/// For any order of signing in as one person or another with either of their pairs of keys, the
/// service saying who they're for or the user naming the account, out, and requests through the
/// plugin's access to each: the store keeps exactly the accounts signed in since they were last
/// signed out, once each by subject, in the order they were first signed in then, never refreshed
/// or expiring; a request gets the keys last entered for its account, and fails as signed out once
/// it's signed out; signing out revokes nothing; the store's secrets hold the keys last entered for
/// each account kept and no others; and no key gets into `accounts.json`, an error, a listing or a
/// log.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_keys_account_is_kept_once_per_subject_with_the_keys_last_entered(tc: TestCase) {
    let whoami = tc.draw(gs::booleans());
    let k = keyed(GOES[0], whoami);
    for person in PEOPLE {
        for n in 0..2 {
            let who = json!({ "id": person, "name": format!("{person} at mock") });
            k.service.lock().unwrap().known.insert(keys_of(person, n).0, who);
        }
    }
    let people = 0..PEOPLE.len();
    let each = people.clone().flat_map(|p| [Step::SignIn(p, 0), Step::SignIn(p, 1), Step::SignOut(p), Step::Fetch(p)]);
    let steps = tc.draw(gs::vecs(gs::sampled_from(each.collect::<Vec<_>>())).max_size(6));
    // The accounts kept, by subject, with the pair of keys last entered; and echo's accesses.
    let mut kept: IndexMap<&str, usize> = IndexMap::new();
    let mut accesses: IndexMap<&str, String> = IndexMap::new();
    let mut answers = Vec::new();
    for (i, step) in steps.into_iter().enumerate() {
        match step {
            Step::SignIn(p, n) => {
                let (person, keys) = (PEOPLE[p], keys_of(PEOPLE[p], n));
                let done = k.sign_in(&entered(&keys.0, &keys.1), (!whoami).then_some(person)).unwrap();
                let name = if whoami { format!("{person} at mock") } else { person.to_string() };
                assert_eq!(
                    (done.account.subject.as_str(), done.account.name.as_str()),
                    (person, name.as_str()),
                    "step {i}"
                );
                accesses.insert(person, done.access.as_ref().expect("given to echo").id.clone());
                answers.push(serde_json::to_value(&done).unwrap());
                kept.insert(person, n);
            }
            Step::SignOut(p) => {
                let out = k.f.plugins.sign_out_account(WITH, PEOPLE[p], &k.f.store);
                match kept.shift_remove(PEOPLE[p]) {
                    Some(_) => {
                        let out = out.unwrap();
                        assert!(!out.revoked && out.account.signed_in, "step {i}");
                        answers.push(serde_json::to_value(&out).unwrap());
                    }
                    None => assert_eq!(out.unwrap_err().code(), "not_found", "step {i}"),
                }
            }
            // Echo has no access to someone never signed in to name.
            Step::Fetch(p) if !accesses.contains_key(PEOPLE[p]) => {}
            Step::Fetch(p) => {
                let answer = k.fetch("/r/x", &format!("{{account:mock/{}}}", accesses[PEOPLE[p]]));
                match kept.get(PEOPLE[p]) {
                    // What the service answered is the service's: it echoes the header it got.
                    Some(n) => {
                        let (access, secret) = keys_of(PEOPLE[p], *n);
                        assert_eq!(answer["result"]["body"], format!("/r/x LOW {access}:{secret}"), "step {i}");
                    }
                    None => {
                        assert_eq!(answer["error"]["data"]["reason"], "signed_out", "step {i}: {answer}");
                        answers.push(answer);
                    }
                }
            }
        }
        // The secrets hold each kept account's keys last entered, and no others: none entered
        // before them, nor any of an account signed out.
        let secrets = k.secrets();
        for (person, n) in PEOPLE.iter().flat_map(|p| (0..2).map(move |n| (*p, n))) {
            let (access, secret) = keys_of(person, n);
            let last = kept.get(person) == Some(&n);
            assert_eq!(secrets.contains(&access), last, "step {i}: {person}'s keys {n}, kept {last}");
            assert_eq!(secrets.contains(&secret), last, "step {i}: {person}'s keys {n}, kept {last}");
        }
        let listed = k.listed();
        let said: Vec<&str> = listed.iter().map(|a| a.subject.as_str()).collect();
        assert_eq!(said, kept.keys().copied().collect::<Vec<_>>(), "step {i}");
        let unexpiring =
            |a: &locallibrary::plugin::Account| !a.refreshes && a.expires.is_none() && a.signs_out.is_none();
        assert!(listed.iter().all(|a| a.kind == "mock" && a.signed_in && unexpiring(a)), "step {i}");
        answers.push(serde_json::to_value(&listed).unwrap());
    }
    let all: Vec<String> =
        PEOPLE.iter().flat_map(|p| (0..2).flat_map(|n| <[String; 2]>::from(keys_of(p, n)))).collect();
    let all: Vec<&str> = all.iter().map(String::as_str).collect();
    assert!(!k.leaked(&answers.iter().collect::<Vec<_>>(), &all), "a key is out where it may not be");
}

/// A service refusing keys, saying they're no good or lack a scope, has its answer passed on to
/// the plugin as it is, sent once: the account stays signed in with the same keys, which have no
/// token to refresh.
#[test]
fn a_service_refusing_keys_leaves_the_account_signed_in() {
    let k = keyed(GOES[0], false);
    let keys = ("ak-0123456789", "sk-9876543210");
    k.sign_in(&entered(keys.0, keys.1), Some("me")).unwrap();
    let challenge = |said: &str| vec![("WWW-Authenticate", format!("Bearer realm=\"mock\", {said}"))];
    let refusals = [
        (401, challenge(r#"error="invalid_token""#)),
        (403, challenge(r#"error="insufficient_scope", scope="write""#)),
        (401, Vec::new()),
        (403, Vec::new()),
    ];
    for (status, headers) in refusals {
        let body = json!({ "said": status }).to_string().into_bytes();
        k.service.lock().unwrap().script.push_back(Reply { status, headers, body });
        let sent = k.server.got().len();
        let answer = k.fetch("/r/x", "{account:mock}");
        assert_eq!(
            (&answer["result"]["status"], &answer["result"]["body"]),
            (&json!(status), &json!(format!("{{\"said\":{status}}}"))),
            "{answer}"
        );
        assert_eq!(k.server.got().len(), sent + 1, "sent again");
        assert!(k.listed()[0].signed_in, "signed out for {status}");
    }
    assert_eq!(k.fetch("/r/x", "{account:mock}")["result"]["body"], format!("/r/x LOW {}:{}", keys.0, keys.1));
    let token = locallibrary::testing::account_token(&k.f.plugins, &k.f.store, WITH, "me").unwrap_err();
    assert_eq!(token.to_string(), "the Mock account me is signed in with keys, not a token");
    assert!(k.listed()[0].signed_in);
}

/// Keys filled into a request together are each kept out of what the plugin logs, as a service
/// answering with one of them might have it say.
#[test]
fn each_key_stays_out_of_the_plugins_log() {
    let k = keyed(GOES[0], false);
    let keys = ("ak-0123456789", "sk-9876543210");
    k.sign_in(&entered(keys.0, keys.1), Some("me")).unwrap();
    assert_eq!(k.fetch("/r/x", "{account:mock}")["result"]["body"], format!("/r/x LOW {}:{}", keys.0, keys.1));
    k.f.echo_params(json!({ "log": format!("got {} and {}", keys.1, keys.0) })).unwrap();
    eventually("its line is logged", || k.f.logs.lock().unwrap().iter().any(|(_, l)| l.starts_with("got ")));
    let logged: Vec<String> = k.f.logs.lock().unwrap().iter().map(|(_, l)| l.clone()).collect();
    assert!(logged.contains(&"got <secret> and <secret>".to_string()), "{logged:?}");
}

/// A sign-in with keys says where the user gets them and which to enter, listening for no
/// browser; keys missing, undeclared, short or with spaces, or an account named badly or not at
/// all, keep nothing and end the sign-in, saying why but never a key; a sign-in in the browser
/// isn't finished with keys, nor one with keys in the browser; and keys entered are trimmed, and
/// kept at the shortest a key is.
#[test]
fn keys_missing_or_undeclared_or_short_keep_nothing() {
    let k = keyed(GOES[0], false);
    let start = k.begin();
    assert_eq!((start.url.clone(), start.redirect_uri.clone()), (format!("{}/keys", k.server.url), None));
    let titles: IndexMap<String, String> =
        IndexMap::from_iter([("access".to_string(), "Access key".to_string()), ("secret".into(), "Secret key".into())]);
    assert_eq!(start.keys, titles);
    let (access, secret) = ("ak-0123456789", "sk-9876543210");
    let mut undeclared = entered(access, secret);
    undeclared.insert("token".into(), "tk-0123456789".into());
    let mut missing = entered(access, secret);
    missing.shift_remove("secret");
    let mut errors = Vec::new();
    for (keys, name, said) in [
        (missing, Some("me"), "the Mock account's Secret key (secret) is missing"),
        (entered(access, "  "), Some("me"), "the Mock account's Secret key (secret) is missing"),
        (undeclared, Some("me"), "\"token\" isn't one of the keys of a Mock account"),
        (
            entered(access, "sk-1234"),
            Some("me"),
            "the Mock account's Secret key (secret) is 7 characters: a key is 8 at least",
        ),
        (
            entered("ak 0123456789", secret),
            Some("me"),
            "the Mock account's Access key (access) isn't printable ASCII without spaces",
        ),
        (entered(access, secret), None, "name the Mock account: Mock doesn't say who the keys are for"),
        (entered(access, secret), Some(" "), "name the Mock account: Mock doesn't say who the keys are for"),
        (entered(access, secret), Some("me\nyou"), "an account's name has no line break or key in it"),
        (entered(access, secret), Some("me ak-0123456789"), "an account's name has no line break or key in it"),
    ] {
        let start = k.begin();
        let e = k.f.plugins.finish_account_sign_in_with_keys(&start.id, &keys, name).unwrap_err();
        assert_eq!(e.to_string(), said);
        let again = k.f.plugins.finish_account_sign_in_with_keys(&start.id, &entered(access, secret), Some("me"));
        assert_eq!(again.unwrap_err().code(), "not_found", "the sign-in isn't over");
        errors.push(json!(e.to_string()));
    }
    let start = k.begin();
    let e = k.f.plugins.finish_account_sign_in_with_url(&start.id, "http://127.0.0.1/callback?code=c&state=s");
    assert_eq!(e.unwrap_err().to_string(), format!("sign-in {:?} finishes with the account's keys", start.id));
    let e = k.f.plugins.finish_account_sign_in(&k.begin().id, Duration::from_secs(1), &Ctx::silent()).unwrap_err();
    assert!(e.to_string().ends_with("finishes with the account's keys"), "{e}");
    assert!(k.listed().is_empty() && k.server.got().is_empty(), "something was kept or sent");

    let browser: Mock = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    let start = browser.f.plugins.begin_account_sign_in("mock", WITH, None, None, &[], &browser.f.store).unwrap();
    let e = browser.f.plugins.finish_account_sign_in_with_keys(&start.id, &entered(access, secret), Some("me"));
    assert_eq!(e.unwrap_err().to_string(), format!("sign-in {:?} finishes in the browser", start.id));

    let done = k.sign_in(&entered(&format!(" {access}\n"), &format!("{secret} ")), Some(" me ")).unwrap();
    assert_eq!((done.account.subject.as_str(), done.account.name.as_str()), ("me", "me"));
    assert_eq!(k.fetch("/r/x", "{account:mock}")["result"]["body"], format!("/r/x LOW {access}:{secret}"));
    assert!(!k.leaked(&errors.iter().collect::<Vec<_>>(), &[access, secret]), "a key is out where it may not be");
    // The shortest a key is.
    k.sign_in(&entered(access, "sk-12345"), Some("me")).unwrap();
}

/// Signing in with keys asks the type's whoami who they're for with the keys where its first place
/// says, and nowhere else: the account kept is the one it says; one it refuses keeps nothing, and
/// naming the account with a whoami is refused.
#[test]
fn whoami_gets_the_keys_where_the_first_place_says() {
    let (access, secret) = ("ak-0123456789", "sk-9876543210");
    for goes in [GOES[0], GOES[1], GOES[3]] {
        let k = keyed(goes, true);
        let e = k.sign_in(&entered(access, secret), None).unwrap_err().to_string();
        assert!(e.starts_with("asking Mock who signed in: it answered 403"), "{goes:?}: {e}");
        let e = k.sign_in(&entered(access, secret), Some("me")).unwrap_err();
        assert_eq!(e.to_string(), "Mock says who the keys are for: sign in without naming the account");
        assert!(k.listed().is_empty(), "{goes:?}: kept one it refused");

        k.service.lock().unwrap().known.insert(access.into(), json!({ "id": 42, "name": "Uploader" }));
        let done = k.sign_in(&entered(access, secret), None).unwrap();
        assert_eq!((done.account.subject.as_str(), done.account.name.as_str()), ("42", "Uploader"), "{goes:?}");
        let asked = k.server.got().last().cloned().unwrap();
        let (target, authorization) = match goes.places[0] {
            HEADER => ("/whoami".to_string(), Some(format!("LOW {access}:{secret}"))),
            _ => (format!("/whoami?accesskey={access}"), None),
        };
        assert_eq!((asked.target, asked.headers.get("authorization").cloned()), (target, authorization), "{goes:?}");
        assert!(!asked.body.contains(secret), "{goes:?}");
        assert!(!k.leaked(&[&json!(e.to_string())], &[access, secret]), "{goes:?}: a key is out where it may not be");
    }
}
