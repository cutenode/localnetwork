//! Properties of the Atmosphere account: granted scopes and session expiry, the sign-in
//! redirect's query, the client metadata document, the session split between `atmosphere.json`
//! and its secrets, and signing in, checking and signing out against the pretend data server
//! (`support/mock_pds.rs`). Sessions are kept in files, never the keychain, and nothing leaves
//! 127.0.0.1.

use crate::mock_pds::{MockPds, Writes};
use crate::session::{session_json, sign_in_as};

use indexmap::IndexMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use hegel::TestCase;
use hegel::generators as gs;
use jacquard_oauth::session::ClientSessionData;
use locallibrary::api::{self, AccountLoginRequest};
use locallibrary::atmosphere::lexicon::{PROFILE, RECORDING, SET, SETTINGS};
use locallibrary::atmosphere::store::{self, AccountFile, StoredAccount};
use locallibrary::atmosphere::{
    self, AccountInfo, CallbackQuery, Client, PUBLISH_SCOPES, PUBLISH_SET, REPORT_SCOPE, SESSION_LIMIT_DAYS, Storage,
    UPLOAD_SCOPE, publish_scopes,
};
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use serde_json::{Map, Value, json};

/// What the terminal gets of a server's words: control characters as spaces.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// An account signed in as `did`, granted `scopes`, due to sign in again by `sign_in_again_by`.
fn info(did: &str, handle: Option<String>, scopes: Vec<String>, sign_in_again_by: DateTime<Utc>) -> AccountInfo {
    AccountInfo {
        did: did.into(),
        handle,
        pds: "https://pds.example".into(),
        auth_server: "https://pds.example".into(),
        scopes,
        signed_in_at: Utc::now(),
        sign_in_again_by,
        access_expires_at: None,
        client: Client::Loopback,
        storage: Storage::File,
    }
}

// ---------------------------------------------------------------------------------------------
// Scopes and expiry
// ---------------------------------------------------------------------------------------------

/// Record types a scope can name: locallibrary's, and someone else's.
const COLLECTIONS: [&str; 4] = [SETTINGS, PROFILE, SET, "app.example.thing"];

/// One granted scope, and what it lets a session do: `(collection or None for all, create,
/// update)`, `Some(None)` for `transition:generic`'s everything, and `None` when it can't be read.
#[derive(Debug, Clone)]
struct Granted {
    scope: String,
    grants: Option<Option<(Option<&'static str>, bool, bool)>>,
}

#[hegel::test_helper]
fn granted(tc: &TestCase) -> Granted {
    let c = tc.draw(gs::sampled_from(&COLLECTIONS));
    let kind = tc.draw(gs::integers::<u8>().max_value(12));
    let (scope, grants) = match kind {
        0 => ("atproto".to_string(), Some(Some((None, false, false)))),
        1 => ("transition:generic".to_string(), Some(None)),
        2 => ("repo:*".to_string(), Some(Some((None, true, true)))),
        3 => ("repo:*?action=create".to_string(), Some(Some((None, true, false)))),
        4 => (format!("repo:{c}"), Some(Some((Some(c), true, true)))),
        5 => (format!("repo:{c}?action=create"), Some(Some((Some(c), true, false)))),
        6 => (format!("repo:{c}?action=update"), Some(Some((Some(c), false, true)))),
        7 => (format!("repo:{c}?action=delete"), Some(Some((Some(c), false, false)))),
        8 => (format!("repo:{c}?action=create&action=update"), Some(Some((Some(c), true, true)))),
        9 => (format!("repo:{c}?action=*"), Some(Some((Some(c), true, true)))),
        10 => (format!("repo:{c}?action=update&action=delete"), Some(Some((Some(c), false, true)))),
        // Servers grant a permission set's scopes in its place: its name grants nothing.
        11 => (format!("include:{PUBLISH_SET}"), Some(Some((None, false, false)))),
        _ => (format!("repo:{c}?action=publish"), None),
    };
    Granted { scope, grants }
}

/// A session may write a record type when what it was granted lets it both create and replace
/// records of it (or it holds `transition:generic`), and a list with a scope that can't be read
/// grants nothing; otherwise `missing_scope` names exactly the scope to ask for.
#[hegel::test(test_cases = 400)]
fn missing_scope_agrees_with_what_was_granted(tc: TestCase) {
    let list: Vec<Granted> = (0..tc.draw(gs::integers::<usize>().max_value(5))).map(|_| granted(&tc)).collect();
    let collection = tc.draw(gs::sampled_from(vec![SETTINGS, PROFILE, SET, "app.example.thing", "not an nsid"]));
    let scopes: Vec<String> = list.iter().map(|g| g.scope.clone()).collect();
    let readable: Option<Vec<_>> = list.iter().map(|g| g.grants).collect();
    let may_write = readable.is_some_and(|grants| {
        let is_nsid = collection != "not an nsid";
        let covers = |c: Option<&str>| c.is_none_or(|c| c == collection);
        let create = grants.iter().any(|g| matches!(g, Some((c, true, _)) if covers(*c)));
        let update = grants.iter().any(|g| matches!(g, Some((c, _, true)) if covers(*c)));
        grants.iter().any(Option::is_none) || (is_nsid && create && update)
    });
    let account = info("did:plc:abc", None, scopes, Utc::now());
    let expected = (!may_write).then(|| format!("repo:{collection}?action=create&action=update"));
    assert_eq!(atmosphere::missing_scope(&account, collection), expected, "{:?}", account.scopes);
}

/// Publishing asks for `atproto`, the permission set, which a server grants as one
/// create-and-replace scope per record type and one for asking the directory to read them again,
/// and uploading audio files; a session granted them may write each of locallibrary's record types
/// and nothing else's, and upload audio and nothing else.
#[hegel::test(test_cases = 50)]
fn the_publish_scopes_cover_exactly_locallibrarys_records(tc: TestCase) {
    assert_eq!(publish_scopes(), ["atproto".to_string(), format!("include:{PUBLISH_SET}"), UPLOAD_SCOPE.into()]);
    let granted = crate::mock_pds::granted(&publish_scopes());
    let mut want: Vec<String> = PUBLISH_SCOPES.map(String::from).to_vec();
    // Servers write the `rpc` one with the audience jacquard leaves out.
    want[4] += "?aud=*";
    want.extend(["atproto".into(), UPLOAD_SCOPE.into()]);
    assert_eq!(granted, want);
    let order = tc.draw(gs::permutations(granted));
    let account = info("did:plc:abc", None, order, Utc::now());
    for c in [SETTINGS, PROFILE, SET, RECORDING] {
        assert_eq!(atmosphere::missing_scope(&account, c), None, "{c}");
    }
    // Audio files go up, and nothing else does.
    assert!(atmosphere::may_upload(&account, "audio/mp4") && atmosphere::may_upload(&account, "audio/mpeg"));
    assert!(!atmosphere::may_upload(&account, "video/mp4") && !atmosphere::may_upload(&account, "image/png"));
    assert!(atmosphere::missing_scope(&account, "app.example.thing").is_some());
}

/// Whole days left until signing in again, whether that's passed, and whether it's due within
/// `WARN_DAYS` (warned about), agree with the time it's due; an account is named by its handle
/// when it has one, its DID otherwise.
#[hegel::test(test_cases = 300)]
fn days_left_and_expiry_agree_with_the_due_date(tc: TestCase) {
    let day = 86_400;
    let offset = tc.draw(gs::integers::<i64>().min_value(-40 * day).max_value(40 * day));
    // Checked a moment after it's due: far enough from a day's boundary not to cross it.
    tc.assume((offset.rem_euclid(day)) > 5 && offset.rem_euclid(day) < day - 5);
    let handle = tc.draw(gs::optional(gs::text().min_size(1).max_size(12)));
    let account = info("did:plc:abc", handle.clone(), vec![], Utc::now() + chrono::Duration::seconds(offset));
    assert_eq!(account.days_left(), offset / day, "{offset}");
    assert_eq!(account.expired(), offset < 0);
    assert_eq!(account.due_soon(), offset <= locallibrary::plugin::WARN_DAYS * day, "{offset}");
    assert_eq!(account.name(), handle.map_or("did:plc:abc".to_string(), |h| format!("@{h}")));
}

// ---------------------------------------------------------------------------------------------
// The redirect's query
// ---------------------------------------------------------------------------------------------

/// `s` form-encoded: unreserved characters as they are or escaped, spaces as `+` or `%20`, and
/// everything else escaped byte by byte.
#[hegel::test_helper]
fn form_encoded(tc: &TestCase, s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let plain = c.is_ascii_alphanumeric() || "-._~".contains(c);
        if plain && tc.draw(gs::weighted_booleans(0.8)) {
            out.push(c);
        } else if c == ' ' && tc.draw(gs::booleans()) {
            out.push('+');
        } else {
            let mut buf = [0; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

#[hegel::test_helper]
fn callback_query(tc: &TestCase) -> CallbackQuery {
    // Anything, or what a query's encoding has to take care of: spaces, `+`, `%`, `&`, `=`, `#`.
    let value = || {
        gs::optional(hegel::one_of!(
            gs::text().max_size(16),
            gs::text().alphabet("ab +%&=#?é").max_size(8),
            gs::sampled_from(vec!["access_denied".to_string()])
        ))
    };
    CallbackQuery {
        code: tc.draw(value()),
        state: tc.draw(value()),
        iss: tc.draw(value()),
        error: tc.draw(value()),
        error_description: tc.draw(value()),
    }
}

/// `q` as a query string: its fields in any order, names and values encoded any way they may
/// be, among parameters it doesn't know and empty pairs.
#[hegel::test_helper]
fn query_string(tc: &TestCase, q: &CallbackQuery) -> String {
    let fields = [("code", &q.code), ("state", &q.state), ("iss", &q.iss), ("error", &q.error)];
    let fields = fields.into_iter().chain([("error_description", &q.error_description)]);
    let mut pairs: Vec<String> = fields
        .filter_map(|(k, v)| Some(format!("{}={}", form_encoded(tc, k), form_encoded(tc, v.as_ref()?))))
        .collect();
    let extra =
        tc.draw(gs::vecs(gs::sampled_from(vec!["scope=atproto", "foo=bar", "lonely", "", "codes=x"])).max_size(3));
    pairs.extend(extra.into_iter().map(String::from));
    tc.draw(gs::permutations(pairs)).join("&")
}

/// Parsing a query gets back each field, however it was encoded and whatever else is there.
#[hegel::test(test_cases = 150)]
fn a_query_parses_back_to_its_fields(tc: TestCase) {
    let q = callback_query(&tc);
    let query = query_string(&tc, &q);
    assert_eq!(CallbackQuery::parse(&query), q, "{query}");
}

/// A pasted address reads as its query (not its fragment) when it carries a code or a refusal,
/// and is refused otherwise.
#[hegel::test(test_cases = 150)]
fn a_pasted_address_reads_as_its_query(tc: TestCase) {
    let q = callback_query(&tc);
    let query = query_string(&tc, &q);
    let base = tc.draw(gs::sampled_from(vec![
        "http://127.0.0.1:53111/callback",
        "http://127.0.0.1/callback",
        "http://localhost:1/somewhere/else",
    ]));
    let fragment = tc.draw(gs::optional(gs::text().max_size(10)));
    let pad = tc.draw(gs::sampled_from(vec!["", " ", "\n", "\t "]));
    let url = format!("{pad}{base}?{query}{}{pad}", fragment.map(|f| format!("#{f}")).unwrap_or_default());
    match CallbackQuery::from_url(&url) {
        Ok(read) => {
            assert!(q.code.is_some() || q.error.is_some(), "{url:?}");
            assert_eq!(read, q, "{url:?}");
        }
        Err(e) => {
            assert!(q.code.is_none() && q.error.is_none(), "{url:?}: {e}");
            assert!(e.to_string().contains("no sign-in result"), "{e}");
        }
    }
    // Without a query, nothing.
    assert!(CallbackQuery::from_url(&format!("{pad}{base}{pad}")).is_err());
}

/// The server's refusal is worded for people: a declined sign-in says so, otherwise the
/// server's description or its error code, on one line without control characters. No error,
/// no refusal.
#[hegel::test(test_cases = 300)]
fn a_refusal_says_what_the_server_said_on_one_line(tc: TestCase) {
    let q = callback_query(&tc);
    let expected = q.error.as_deref().map(|e| match (e, q.error_description.as_deref()) {
        ("access_denied", _) => "the sign-in was declined, so locallibrary wasn't given access".to_string(),
        (_, Some(d)) if !d.is_empty() => format!("your account's server stopped the sign-in: {}", one_line(d)),
        (e, _) => format!("your account's server stopped the sign-in ({})", one_line(e)),
    });
    let refusal = q.refusal();
    assert_eq!(refusal, expected);
    assert!(!refusal.unwrap_or_default().chars().any(char::is_control));
}

/// A wait is said in whole seconds under two minutes, and in minutes, rounded up, from then.
#[hegel::test(test_cases = 300)]
fn waits_are_said_in_seconds_or_rounded_up_minutes(tc: TestCase) {
    let secs = tc.draw(hegel::one_of!(gs::integers::<u64>().max_value(200), gs::integers::<u64>().max_value(1 << 40)));
    let nanos = tc.draw(gs::integers::<u32>().max_value(999_999_999));
    let said = atmosphere::span(Duration::new(secs, nanos));
    let (n, unit) = said.split_once(' ').unwrap();
    let n: u64 = n.parse().unwrap();
    match unit {
        "second" => assert_eq!(secs, 1),
        "seconds" => assert!(n == secs && secs != 1 && secs < 120, "{said}"),
        "minutes" => assert!(secs >= 120 && n * 60 >= secs && (n - 1) * 60 < secs, "{secs}: {said}"),
        _ => panic!("{said}"),
    }
}

// ---------------------------------------------------------------------------------------------
// The client metadata document
// ---------------------------------------------------------------------------------------------

/// The client metadata document allows `atproto`, the permission set and the extra ones asked
/// for, each once; it's for a native app whose redirect is the loopback callback, named after
/// its host; the API's document is the same one; and a client ID that isn't https, or a scope
/// that can't be read, gets no document.
#[hegel::test(test_cases = 200)]
fn the_client_document_allows_what_it_should(tc: TestCase) {
    let host = tc.draw(gs::from_regex(r"[a-z][a-z0-9-]{0,8}(\.[a-z][a-z0-9]{0,5}){1,3}").fullmatch(true));
    let port = tc.draw(gs::optional(gs::integers::<u16>().min_value(1)));
    let path = tc.draw(gs::from_regex(r"(/[a-z0-9._-]{1,8}){0,3}").fullmatch(true));
    let scheme = tc.draw(gs::sampled_from(vec!["https", "https", "https", "http"]));
    let client_id = format!("{scheme}://{host}{}{path}", port.map(|p| format!(":{p}")).unwrap_or_default());
    let pool = vec![
        "atproto",
        "transition:generic",
        "repo:app.example.thing",
        "repo:app.example.other?action=create",
        PUBLISH_SCOPES[1],
        format!("include:{PUBLISH_SET}").leak(),
        "foo:bar",
    ];
    let extra: Vec<String> =
        tc.draw(gs::vecs(gs::sampled_from(pool)).max_size(6)).into_iter().map(String::from).collect();

    let direct = atmosphere::client_metadata(&client_id, &extra);
    let req = api::ClientMetadataRequest { client_id: client_id.clone(), scopes: extra.clone() };
    let through_api = api::account_client_metadata(&req);
    assert_eq!(
        direct.as_ref().map_err(ToString::to_string),
        through_api.as_ref().map_err(ToString::to_string),
        "the API's is the same"
    );
    if scheme != "https" || extra.iter().any(|s| s == "foo:bar") {
        assert!(direct.is_err(), "{client_id} {extra:?}");
        return;
    }
    let doc = direct.unwrap();
    let allowed: Vec<&str> = doc["scope"].as_str().unwrap().split(' ').collect();
    let mut expected: Vec<String> = publish_scopes();
    expected.push(REPORT_SCOPE.into());
    expected.extend(extra.iter().cloned());
    expected.sort();
    expected.dedup();
    let mut sorted: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
    sorted.sort();
    assert_eq!(sorted, expected, "each once");
    assert_eq!(doc["client_id"], client_id.as_str());
    assert_eq!(doc["client_uri"], format!("https://{host}"));
    assert_eq!(doc["application_type"], "native");
    assert_eq!(doc["redirect_uris"], json!(["http://127.0.0.1/callback"]));
    assert_eq!(doc["dpop_bound_access_tokens"], true);

    // Asked for without one, it's for locallibrary's own client ID.
    let default: api::ClientMetadataRequest = serde_json::from_value(json!({ "scopes": extra })).unwrap();
    assert_eq!(default.client_id, atmosphere::CLIENT_ID);
    let own = api::account_client_metadata(&default).unwrap();
    assert_eq!(own, atmosphere::client_metadata(atmosphere::CLIENT_ID, &extra).unwrap());
}

// ---------------------------------------------------------------------------------------------
// The session, split and stored
// ---------------------------------------------------------------------------------------------

const DIDS: [&str; 3] =
    ["did:plc:aaaaaaaaaaaaaaaaaaaaaaaa", "did:plc:bbbbbbbbbbbbbbbbbbbbbbbb", "did:web:dj.example.com"];

fn alnum(min: usize, max: usize) -> impl hegel::PrintableGenerator<String> {
    gs::text().alphabet("abcdefghijklmnopqrstuvwxyz0123456789").min_size(min).max_size(max)
}

#[hegel::test_helper]
fn random_session(tc: &TestCase) -> Value {
    let did = format!(
        "did:plc:{}",
        tc.draw(gs::text().alphabet("abcdefghijklmnopqrstuvwxyz234567").min_size(24).max_size(24))
    );
    let refresh = tc.draw(gs::optional(alnum(8, 16))).map(|r| format!("refresh-{r}"));
    let pds = tc.draw(gs::sampled_from(vec!["https://pds.example", "https://bsky.social", "http://127.0.0.1:2583"]));
    let expires = tc.draw(gs::sampled_from(vec!["2099-01-01T00:00:00.000Z", "2020-02-29T12:34:56.789Z"]));
    let scopes = tc.draw(gs::sampled_from(vec!["atproto", "atproto transition:generic", "atproto repo:*"]));
    let mut s = session_json(&did, &tc.draw(alnum(1, 12)), pds, scopes, refresh.as_deref(), expires);
    s["access_token"] = format!("access-{}", tc.draw(alnum(4, 12))).into();
    s["dpop_host_nonce"] = tc.draw(alnum(0, 8)).into();
    s
}

/// Splitting a session puts its DPoP key and refresh token, and nothing else, in the secrets,
/// with neither left anywhere in the half for `atmosphere.json`; the halves together are the
/// session again.
#[hegel::test(test_cases = 40)]
fn a_split_session_keeps_its_secrets_apart_and_joins_back(tc: TestCase) {
    let original = random_session(&tc);
    let session: ClientSessionData = serde_json::from_value(original.clone()).unwrap();
    let whole = serde_json::to_value(&session).unwrap();
    let (public, secrets) = store::split(&session).unwrap();
    let public_map = public.as_object().unwrap();
    assert!(secrets.keys().all(|k| k == "dpop_key" || k == "refresh_token"), "{:?}", secrets.keys());
    assert!(public_map.keys().all(|k| !secrets.contains_key(k)));
    let mut union = public_map.clone();
    union.extend(secrets.clone());
    assert_eq!(Value::Object(union), whole, "nothing lost or added");
    let text = public.to_string();
    assert!(!text.contains(original["dpop_key"]["d"].as_str().unwrap()), "the private key stays out");
    if let Some(refresh) = original["refresh_token"].as_str() {
        assert!(!text.contains(refresh), "the refresh token stays out");
    }
    let back = store::join(&public, secrets.clone()).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), whole);

    // Halves that aren't a session's are said to be, without quoting them.
    let not_an_object = tc.draw(gs::sampled_from(vec![Value::Null, json!([]), json!("x"), json!(1)]));
    let err = store::join(&not_an_object, secrets).unwrap_err().to_string();
    assert_eq!(err, "the saved session isn't a JSON object; sign in again");
    let wrong = Map::from_iter([("dpop_key".to_string(), Value::from("secret-key"))]);
    let err = store::join(&public, wrong).unwrap_err().to_string();
    assert!(err.contains("can't be used") && !err.contains("secret-key"), "{err}");
}

#[hegel::test_helper]
fn date(tc: &TestCase) -> DateTime<Utc> {
    let last = DateTime::from_timestamp(5_000_000_000, 999_999_999).unwrap();
    tc.draw(gs::default::<DateTime<Utc>>().min_value(DateTime::UNIX_EPOCH.naive_utc()).max_value(last.naive_utc()))
}

#[hegel::test_helper]
fn storage(tc: &TestCase) -> Storage {
    if tc.draw(gs::booleans()) { Storage::File } else { Storage::Keychain }
}

#[hegel::test_helper]
fn account_file(tc: &TestCase) -> AccountFile {
    let account = tc.draw(gs::booleans()).then(|| StoredAccount {
        info: AccountInfo {
            did: tc.draw(gs::sampled_from(DIDS.to_vec())).into(),
            handle: tc.draw(gs::optional(gs::text().max_size(12))),
            pds: tc.draw(gs::text().max_size(12)),
            auth_server: tc.draw(gs::text().max_size(12)),
            scopes: tc.draw(gs::vecs(gs::text().max_size(8)).max_size(3)),
            signed_in_at: date(tc),
            sign_in_again_by: date(tc),
            access_expires_at: tc.draw(gs::booleans()).then(|| date(tc)),
            client: match tc.draw(gs::optional(gs::text().max_size(12))) {
                Some(client_id) => Client::Hosted { client_id },
                None => Client::Loopback,
            },
            storage: storage(tc),
        },
        session_id: tc.draw(gs::text().max_size(8)),
        redirect_uri: tc.draw(gs::text().max_size(12)),
        requested_scopes: tc.draw(gs::vecs(gs::text().max_size(8)).max_size(3)),
        session: Value::Object(
            tc.draw(gs::vecs(gs::tuples!(gs::text().max_size(6), gs::text().max_size(6))).max_size(4))
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect(),
        ),
    });
    AccountFile {
        account,
        last_handle: tc.draw(gs::optional(gs::text().max_size(12))),
        left_behind: (0..tc.draw(gs::integers::<usize>().max_value(3)))
            .map(|_| (storage(tc), tc.draw(gs::sampled_from(DIDS.to_vec())).to_string()))
            .collect(),
        indexed: tc.draw(gs::subsequences(DIDS.to_vec())).into_iter().map(String::from).collect(),
    }
}

/// `atmosphere.json` reads back as it was saved, whatever's in it, and only its owner can read
/// it.
#[hegel::test(test_cases = 10)]
fn the_account_file_reads_back_as_saved(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(serde_json::to_value(AccountFile::load(dir.path()).unwrap()).unwrap(), json!({}), "none yet");
    let file = account_file(&tc);
    file.save(dir.path()).unwrap();
    let back = AccountFile::load(dir.path()).unwrap();
    assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&file).unwrap());
    assert_eq!(back.account.map(|a| a.info), file.account.map(|a| a.info));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join(store::ACCOUNT_FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[derive(Debug, Clone)]
enum SecretsOp {
    Write(usize, Map<String, Value>),
    Delete(usize),
    /// The file is damaged behind the store's back: 0, it isn't JSON; 1, it's a list; 2, one
    /// account's secrets aren't an object; 3, there's a folder in its place.
    Damage(u8, usize),
}

/// What's in the file damage writes: never to be quoted.
const SECRET: &str = "secret-xyz";

#[hegel::test_helper]
fn secrets_op(tc: &TestCase) -> SecretsOp {
    let did = tc.draw(gs::integers::<usize>().max_value(DIDS.len() - 1));
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => SecretsOp::Delete(did),
        1 => SecretsOp::Damage(tc.draw(gs::integers::<u8>().max_value(3)), did),
        _ => {
            let fields = tc.draw(
                gs::vecs(gs::tuples!(
                    gs::sampled_from(vec!["refresh_token", "dpop_key", "other"]),
                    gs::text().max_size(8)
                ))
                .max_size(3),
            );
            SecretsOp::Write(did, fields.into_iter().map(|(k, v)| (k.to_string(), Value::String(v))).collect())
        }
    }
}

/// What the session file holds for an account, as far as the model knows.
#[derive(Debug, Clone, PartialEq)]
enum Kept {
    Secrets(Map<String, Value>),
    NotAnObject,
}

/// The session file holds each account's secrets apart, like a map from DID to secrets: what's
/// read is what was last written for that DID, until it's deleted, and the file is there only
/// while it holds someone's. A file from before, with one account's secrets naming it, reads
/// as that account's. A damaged file is said to be, without quoting it, until the next sign-in
/// or sign-out replaces it; a folder in its place stops both.
#[hegel::test(test_cases = 8)]
fn the_session_file_is_a_map_of_each_accounts_secrets(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let path = root.join(store::SECRETS_FILE);
    let mut model: IndexMap<usize, Kept> = IndexMap::new();
    // Why the whole file can't be read, when it can't; and whether it can't be written either.
    let mut unreadable: Option<&str> = None;
    let mut blocked = false;
    // It starts as a file from before.
    {
        let did = tc.draw(gs::integers::<usize>().max_value(DIDS.len() - 1));
        let refresh = tc.draw(gs::text().max_size(8));
        let old = json!({ "did": DIDS[did], "refresh_token": refresh });
        std::fs::write(&path, old.to_string()).unwrap();
        model.insert(did, Kept::Secrets(Map::from_iter([("refresh_token".to_string(), Value::String(refresh))])));
    }
    let ops: Vec<SecretsOp> = (0..tc.draw(gs::integers::<usize>().max_value(8))).map(|_| secrets_op(&tc)).collect();
    for op in ops {
        tc.note(&format!("{op:?}"));
        match op {
            SecretsOp::Write(did, secrets) if blocked => {
                assert!(store::write_secrets(root, Storage::File, DIDS[did], &secrets).is_err());
            }
            SecretsOp::Delete(did) if blocked => {
                assert!(store::delete_secrets(root, Storage::File, DIDS[did]).is_err());
            }
            SecretsOp::Write(did, secrets) => {
                store::write_secrets(root, Storage::File, DIDS[did], &secrets).unwrap();
                if unreadable.take().is_some() {
                    model.clear();
                }
                model.insert(did, Kept::Secrets(secrets));
            }
            SecretsOp::Delete(did) => {
                store::delete_secrets(root, Storage::File, DIDS[did]).unwrap();
                if unreadable.take().is_some() {
                    model.clear();
                }
                model.shift_remove(&did);
            }
            SecretsOp::Damage(kind, did) => {
                model.clear();
                if std::mem::take(&mut blocked) {
                    std::fs::remove_dir(&path).unwrap();
                }
                let text = match kind {
                    0 => {
                        unreadable = Some("it isn't JSON");
                        format!(r#"{{"{}": {{"refresh_token": "{SECRET}", }}"#, DIDS[did])
                    }
                    1 => {
                        unreadable = Some("the saved session isn't a JSON object");
                        json!([SECRET]).to_string()
                    }
                    2 => {
                        unreadable = None;
                        model.insert(did, Kept::NotAnObject);
                        json!({ DIDS[did]: SECRET }).to_string()
                    }
                    _ => {
                        unreadable = Some(store::SECRETS_FILE);
                        blocked = true;
                        String::new()
                    }
                };
                match blocked {
                    true => {
                        let _ = std::fs::remove_file(&path);
                        std::fs::create_dir(&path).unwrap();
                    }
                    false => std::fs::write(&path, text).unwrap(),
                }
            }
        }
        for (i, did) in DIDS.iter().enumerate() {
            let read = store::read_secrets(root, Storage::File, did);
            match (unreadable, model.get(&i)) {
                (None, Some(Kept::Secrets(s))) => assert_eq!(read.unwrap().as_ref(), Some(s), "{did}"),
                (None, None) => assert_eq!(read.unwrap(), None, "{did}"),
                (why, _) => {
                    let err = read.unwrap_err().to_string();
                    assert!(err.contains(why.unwrap_or("the saved session isn't a JSON object")), "{did}: {err}");
                    assert!((blocked || err.ends_with("sign in again")) && !err.contains(SECRET), "{err}");
                }
            }
        }
        assert_eq!(path.exists(), unreadable.is_some() || !model.is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// Signed in, against the pretend data server
// ---------------------------------------------------------------------------------------------

const LATER: &str = "2099-01-01T00:00:00.000Z";
const EARLIER: &str = "2020-01-01T00:00:00.000Z";
const SESSION_IDS: [&str; 2] = ["s1", "s2"];

#[derive(Debug, Clone)]
enum Step {
    SignIn {
        did: usize,
        session: usize,
        ended: bool,
        refresh: bool,
        /// Its servers are the pretend one, not a port nothing listens on.
        reachable: bool,
    },
    /// The handle the server says the session is for (until then it takes no session).
    ServerSays(&'static str),
    /// Whether the server refuses the session's token.
    RefuseToken(bool),
    /// What revoking a token answers.
    Revocation(u16),
    Check {
        api: bool,
    },
    Logout {
        api: bool,
    },
}

#[hegel::test_helper]
fn step(tc: &TestCase) -> Step {
    match tc.draw(gs::integers::<u8>().max_value(7)) {
        0 | 1 => Step::SignIn {
            did: tc.draw(gs::integers::<usize>().max_value(1)),
            session: tc.draw(gs::integers::<usize>().max_value(1)),
            ended: tc.draw(gs::weighted_booleans(0.35)),
            refresh: tc.draw(gs::weighted_booleans(0.75)),
            reachable: tc.draw(gs::weighted_booleans(0.8)),
        },
        2 => {
            Step::ServerSays(tc.draw(gs::sampled_from(vec!["dj.example.com", "someone.bsky.social", "handle.invalid"])))
        }
        3 => Step::RefuseToken(tc.draw(gs::weighted_booleans(0.3))),
        4 => Step::Revocation(tc.draw(gs::sampled_from(vec![200, 400, 500]))),
        5 | 6 => Step::Check { api: tc.draw(gs::booleans()) },
        _ => Step::Logout { api: tc.draw(gs::booleans()) },
    }
}

#[derive(Debug, Clone)]
struct Signed {
    did: usize,
    session: usize,
    handle: Option<String>,
    /// Its access token ran out, and the server won't refresh it.
    ended: bool,
    reachable: bool,
}

/// Signing in, checking with the server and signing out, in any order, with the server
/// answering for someone or no one, refusing the token or not, revoking or not, or not there at
/// all, leave the account where a model says: checking takes the handle the server vouches for,
/// a session that has ended is removed when it's next used, a refused token or a server that
/// can't be reached keeps the session, and signing out always signs out (revoked only when the
/// server took both tokens). The API's calls and the library's say the same.
#[hegel::test(test_cases = 2)]
fn checking_and_signing_out_agree_with_a_model(tc: TestCase) {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut account: Option<Signed> = None;
    let mut last_handle: Option<String> = None;
    let mut server_says: Option<&str> = None;
    let (mut refuse, mut revocation) = (false, 200);
    let mut steps: Vec<Step> =
        (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5))).map(|_| step(&tc)).collect();
    // And then, so every run sees them: signing out until nobody's signed in, and checking then;
    // a fresh session the server vouches for, checked and signed out of; and one that has
    // ended, checked.
    steps.extend([
        Step::Logout { api: false },
        Step::Logout { api: false },
        Step::Check { api: false },
        Step::SignIn { did: 1, session: 1, ended: false, refresh: true, reachable: true },
        Step::ServerSays("dj.example.com"),
        Step::RefuseToken(false),
        Step::Check { api: true },
        Step::Logout { api: true },
        Step::SignIn { did: 0, session: 0, ended: true, refresh: false, reachable: true },
        Step::Check { api: false },
    ]);
    for s in steps {
        tc.note(&format!("{s:?}"));
        match s {
            Step::SignIn { did, session, ended, refresh, reachable } => {
                // Refreshing checks the account's DID document, which is on the network: a
                // session that has ended has nothing to refresh it with.
                let refresh = (refresh && !ended).then_some("refresh-1");
                let pds = if reachable { &pds.url } else { "http://127.0.0.1:9" };
                let expires = if ended { EARLIER } else { LATER };
                sign_in_as(root, session_json(DIDS[did], SESSION_IDS[session], pds, "atproto", refresh, expires));
                // The same session again (refreshed) keeps what was known of it.
                let handle = account.take().filter(|a| (a.did, a.session) == (did, session)).and_then(|a| a.handle);
                account = Some(Signed { did, session, handle, ended, reachable });
            }
            Step::ServerSays(handle) => {
                server_says = Some(handle);
                pds.set_account(DIDS[0], handle);
            }
            Step::RefuseToken(r) => {
                refuse = r;
                pds.set_writes(if r { Writes::RefuseToken } else { Writes::Accept });
            }
            Step::Revocation(status) => {
                revocation = status;
                pds.set_revocation(status);
            }
            Step::Check { api } => {
                let checked = if api {
                    api::account_check(root, &Ctx::silent())
                } else {
                    atmosphere::check(root, &Ctx::silent())
                };
                match account.as_mut() {
                    None => assert_eq!(checked.unwrap_err().reason(), Some(Reason::NotSignedIn)),
                    Some(a) if !a.reachable => {
                        // Ended, it can't be refreshed for now; otherwise the server can't be asked.
                        let err = checked.unwrap_err().to_string();
                        let said = if a.ended { "signing in: " } else { "asking the account's server who's signed in" };
                        assert!(err.contains(said), "{err}");
                    }
                    Some(a) if a.ended => {
                        let err = checked.unwrap_err();
                        assert_eq!(err.reason(), Some(Reason::SessionEnded), "{err}");
                        account = None;
                    }
                    Some(_) if refuse || server_says.is_none() => {
                        let err = checked.unwrap_err();
                        assert_eq!(err.reason(), Some(Reason::SessionEnded), "{err}");
                    }
                    Some(a) => {
                        if let Some(h) = server_says.filter(|h| *h != "handle.invalid") {
                            a.handle = Some(h.to_string());
                            last_handle = Some(h.to_string());
                        }
                        let info = checked.unwrap();
                        assert_eq!((info.did.as_str(), &info.handle), (DIDS[a.did], &a.handle));
                    }
                }
            }
            Step::Logout { api } => {
                let report = if api { api::account_logout(root) } else { atmosphere::logout(root) }.unwrap();
                let who = account.take();
                assert_eq!(report.account.map(|a| a.did), who.as_ref().map(|a| DIDS[a.did].to_string()));
                assert_eq!(report.revoked, who.is_some_and(|a| a.reachable && !a.ended) && revocation == 200);
                assert_eq!(report.left_behind, None);
            }
        }
        let status = api::account_status(root).unwrap();
        let signed_in = status.account.map(|a| (a.did, a.handle));
        assert_eq!(signed_in, account.as_ref().map(|a| (DIDS[a.did].to_string(), a.handle.clone())));
        assert_eq!(atmosphere::status(root).unwrap().map(|a| a.did), account.as_ref().map(|a| DIDS[a.did].to_string()));
        assert_eq!(status.last_handle, last_handle);
        assert_eq!(atmosphere::last_handle(root), last_handle);
        assert_eq!(root.join(store::SECRETS_FILE).exists(), account.is_some(), "secrets only while signed in");
        if let Some(a) = &account {
            let expected = Some(SESSION_LIMIT_DAYS - 1);
            assert_eq!(atmosphere::status(root).unwrap().map(|i| i.days_left()), expected, "{a:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Starting and finishing a sign-in, without the network
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum ClientChoice {
    Dev,
    /// A client document on this computer.
    Local,
    /// One at a plain http address.
    Insecure,
}

/// Starting a sign-in with servers on this computer or its local network fails before anything
/// is sent, for the first reason a model gives: no account named (and none signed in with
/// before), a client document that isn't at a public https address, scopes without `atproto`
/// or that can't be read, and otherwise the account's server, which isn't public. Nobody ends up
/// signed in.
#[hegel::test(test_cases = 8)]
fn starting_a_sign_in_with_local_servers_fails_for_the_first_reason(tc: TestCase) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = || {
        gs::sampled_from(vec![
            format!("https://127.0.0.1:{port}"),
            format!("http://127.0.0.1:{port}"),
            format!("  @https://localhost:{port}/ "),
            format!("https://10.0.0.1:{port}"),
            format!("https://[::1]:{port}"),
        ])
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let before = tc.draw(gs::optional(server()));
    if let Some(h) = &before {
        locallibrary::store::Store::open_or_init(&root).unwrap();
        AccountFile { last_handle: Some(h.clone()), ..Default::default() }.save(&root).unwrap();
    }
    let handle = tc.draw(gs::optional(hegel::one_of!(
        server(),
        gs::sampled_from(vec!["".to_string(), "   ".into(), " @ ".into()])
    )));
    let pool = vec!["atproto", "repo:app.example.thing", "transition:generic", "foo:bar"];
    let scopes: Vec<String> =
        tc.draw(gs::vecs(gs::sampled_from(pool)).max_size(3)).into_iter().map(String::from).collect();
    let publish = tc.draw(gs::booleans());
    let named = handle.clone().filter(|h| !h.trim().is_empty()).or(before);
    let requested: Vec<String> =
        if publish { publish_scopes().into_iter().chain(scopes.clone()).collect() } else { scopes.clone() };
    // The same request from each client.
    for client in [ClientChoice::Dev, ClientChoice::Local, ClientChoice::Insecure] {
        let req = AccountLoginRequest {
            handle: handle.clone(),
            client_id: match client {
                ClientChoice::Dev => None,
                ClientChoice::Local => Some(format!("https://127.0.0.1:{port}/oauth-client-metadata.json")),
                ClientChoice::Insecure => Some("http://locallibrary.example/c.json".into()),
            },
            dev_client: client == ClientChoice::Dev,
            session_file: Some(true),
            scopes: scopes.clone(),
            publish,
        };
        let err = api::account_login_begin(&root, &req).unwrap_err().to_string();
        let expected = if named.is_none() {
            "which Atmosphere account?"
        } else if named.as_deref() == Some(" @ ") {
            "enter your Atmosphere handle"
        } else if client == ClientChoice::Local {
            "is on this computer or its local network"
        } else if client == ClientChoice::Insecure {
            "isn't an https address"
        } else if requested.iter().any(|s| s == "foo:bar") {
            "scopes "
        } else if !requested.is_empty() && !requested.iter().any(|s| s == "atproto") {
            "the scopes have to include `atproto`"
        } else {
            "signing in: "
        };
        assert!(err.contains(expected), "{client:?}: expected {expected:?}: {err}");
        assert!(listener.accept().is_err(), "nothing was sent");
        assert!(atmosphere::status(&root).unwrap().is_none());
        assert!(locallibrary::store::Store::open(&root).is_ok(), "the store folder is a store");
    }
}

/// A sign-in nobody started can't be finished, by waiting or by a pasted address, nor
/// cancelled, whatever it's called.
#[hegel::test(test_cases = 100)]
fn a_sign_in_nobody_started_cant_be_finished(tc: TestCase) {
    let id = tc.draw(gs::text().max_size(20));
    let not_found = |e: locallibrary::Error| assert!(matches!(e, locallibrary::Error::NotFound(_)), "{e}");
    let finish = api::AccountLoginFinish { id: id.clone(), timeout_secs: tc.draw(gs::optional(gs::integers::<u64>())) };
    not_found(api::account_login_finish(&finish, &Ctx::silent()).unwrap_err());
    let paste = api::AccountLoginPaste { id: id.clone(), url: "http://127.0.0.1/callback?code=c&state=s".into() };
    not_found(api::account_login_paste(&paste).unwrap_err());
    assert!(!api::account_login_cancel(&api::AccountLoginCancel { id: id.clone() }));
    assert!(!atmosphere::cancel_login(&id));
}
