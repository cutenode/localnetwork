use hegel::TestCase;
use hegel::generators as gs;

use super::*;

fn net(hosts: &[&str]) -> Vec<Capability> {
    hosts.iter().map(|h| Capability::Net(h.to_string())).collect()
}

fn refused_at(capabilities: &[Capability], url: &str, loopback: bool) -> Option<String> {
    refused(capabilities, &Url::parse(url).unwrap(), loopback)
}

#[test]
fn only_granted_hosts_at_public_https_addresses_are_reached() {
    let granted = net(&["musicbrainz.org", "*.archive.org", "127.0.0.1", "10.0.0.1"]);
    assert_eq!(refused_at(&granted, "https://musicbrainz.org/ws/2/release/?query=x", false), None);
    assert_eq!(refused_at(&granted, "https://ia800.us.archive.org/0/items/x.jpg", false), None);
    let e = refused_at(&granted, "https://archive.org/download/x", false).unwrap();
    assert_eq!(e, "archive.org isn't a server the plugin may talk to (no net:archive.org capability)");
    assert!(refused_at(&granted, "https://beta.musicbrainz.org/", false).is_some(), "exactly the host");
    let e = refused_at(&granted, "http://musicbrainz.org/", false).unwrap();
    assert!(e.ends_with("isn't an https address"), "{e}");
    let e = refused_at(&granted, "https://127.0.0.1:8080/", false).unwrap();
    assert!(e.ends_with("is on this computer or its local network"), "{e}");
    // Tests reach this computer, over http too, and only this computer.
    assert_eq!(refused_at(&granted, "http://127.0.0.1:8080/", true), None);
    assert!(refused_at(&granted, "https://10.0.0.1/", true).is_some(), "the local network still isn't");
    assert!(refused_at(&granted, "ftp://127.0.0.1/", true).is_some());
    assert!(refused_at(&net(&[]), "http://127.0.0.1:8080/", true).is_some(), "granted or not at all");
}

/// For any user name and password in an address at a host the plugin may reach, it's refused
/// when it names either, with what an account's endpoint is refused with. Regression: neither
/// was looked at, so the plugin's request went with them.
#[hegel::test(test_cases = 100)]
fn an_address_naming_a_user_is_refused(tc: TestCase) {
    let user: String = tc.draw(gs::from_regex("[a-z0-9]{0,4}").fullmatch(true));
    let password: Option<String> = tc.draw(gs::optional(gs::from_regex("[a-z0-9]{0,4}").fullmatch(true)));
    let loopback = tc.draw(gs::booleans());
    let host = if loopback { "http://127.0.0.1:8080" } else { "https://musicbrainz.org" };
    let mut url = Url::parse(&format!("{host}/ws/2/")).unwrap();
    url.set_username(&user).unwrap();
    url.set_password(password.as_deref()).unwrap();
    let named = !url.username().is_empty() || url.password().is_some();
    let why = refused(&net(&["musicbrainz.org", "127.0.0.1"]), &url, loopback);
    let said = format!("{:?} isn't an https:// address without a user name or password", url.as_str());
    assert_eq!(why, named.then_some(said), "{url}");
}

/// For any secrets filled into a plugin's requests and any text, what's scrubbed holds none of
/// them as they're written or as a URL's query spells them, and text without any is as it was.
#[hegel::test(test_cases = 200)]
fn scrubbed_text_never_holds_a_secret(tc: TestCase) {
    let state = State::default();
    // Of other letters than the text and `<secret>`: what's said in its place.
    let secrets: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[A-Z0-9 &=+/%]{4,8}").fullmatch(true)).max_size(3));
    for s in &secrets {
        state.saw(s);
    }
    let plain = tc.draw(gs::from_regex("[a-z :]{0,12}").fullmatch(true));
    let encoded = |s: &str| form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    let mut text = plain.clone();
    for s in &secrets {
        text = format!("{text} {s} key={} ", encoded(s));
    }
    let scrubbed = state.scrub(&text);
    for s in &secrets {
        assert!(!scrubbed.contains(s.as_str()) && !scrubbed.contains(&encoded(s)), "{s:?} in {scrubbed:?}");
    }
    assert_eq!(state.scrub(&plain), plain);
    let mut v = serde_json::json!({ "message": text, "fields": [text] });
    state.scrub_value(&mut v);
    assert_eq!(v, serde_json::json!({ "message": scrubbed, "fields": [scrubbed] }));
}
