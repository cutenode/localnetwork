use std::net::UdpSocket;
use std::sync::{Arc, Mutex};

use hegel::TestCase;
use hegel::generators as gs;
use hickory_resolver::config::{NameServerConfig, ResolverConfig};
use hickory_resolver::proto::op::{Message, OpCode, ResponseCode};
use hickory_resolver::proto::rr::Record;
use hickory_resolver::proto::rr::rdata::TXT;
use jacquard_common::SmolStr;
use rustc_hash::FxHashMap;

use super::*;
use crate::atmosphere::block_on;

/// TXT records by name (lowercase, without the final dot), each record its strings.
type Records = Arc<Mutex<FxHashMap<String, Vec<Vec<String>>>>>;

/// A resolver whose DNS is a server on this computer, answering with `records` (and that a name
/// without any doesn't exist).
fn asking(records: Records) -> Resolver {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut buf = [0; 4096];
        while let Ok((len, from)) = socket.recv_from(&mut buf) {
            let query = Message::from_vec(&buf[..len]).unwrap();
            let mut answer = Message::response(query.metadata.id, OpCode::Query);
            answer.metadata.recursion_desired = query.metadata.recursion_desired;
            answer.metadata.recursion_available = true;
            let name = query.queries[0].name().clone();
            answer.add_queries(query.queries);
            match records.lock().unwrap().get(name.to_ascii().trim_end_matches('.').to_lowercase().as_str()) {
                Some(found) => {
                    let txt = |strings: &Vec<String>| RData::TXT(TXT::new(strings.clone()));
                    answer.add_answers(found.iter().map(|strings| Record::from_rdata(name.clone(), 60, txt(strings))));
                }
                None => answer.metadata.response_code = ResponseCode::NXDomain,
            }
            socket.send_to(&answer.to_vec().unwrap(), from).unwrap();
        }
    });
    let mut server = NameServerConfig::udp([127, 0, 0, 1].into());
    server.connections[0].port = port;
    let _guard = runtime().enter();
    let dns = TokioResolver::builder_with_config(ResolverConfig::from_name_servers(vec![server]), Default::default())
        .build()
        .unwrap();
    Resolver::new(Http::new(true).unwrap(), Some(dns))
}

/// Regression: a handle with a DNS record and no `/.well-known/atproto-did` wasn't found, as
/// jacquard's DNS resolver had no servers to ask, so signing in with it failed.
#[test]
fn a_handle_named_only_in_dns_is_found() {
    let records = Records::default();
    let found = vec![vec!["v=spf1 -all".to_string()], vec!["did=did:plc:uashgn65n5z7aqwk5cbuba5c".to_string()]];
    records.lock().unwrap().insert("_atproto.dj.test".into(), found);
    let handle = Handle::<SmolStr>::new_owned("dj.test").unwrap();
    let did = block_on(asking(records).resolve_handle(&handle)).unwrap();
    assert_eq!(did.as_str(), "did:plc:uashgn65n5z7aqwk5cbuba5c");
}

/// Without a DNS record, a handle is looked for at its own web address and nowhere else: not
/// Bluesky's AppView, and not jacquard's DNS over HTTPS. A `.test` name has no web address.
#[test]
fn a_handle_without_a_dns_record_is_only_looked_for_at_its_web_address() {
    let resolver = asking(Records::default());
    assert_eq!(resolver.options().handle_order, [HandleStep::HttpsWellKnown]);
    let handle = Handle::<SmolStr>::new_owned("nobody.test").unwrap();
    assert!(block_on(resolver.resolve_handle(&handle)).is_err());
}

/// A TXT string, and the DID it names when it's a valid `did=` one.
fn txt() -> impl hegel::PrintableGenerator<(String, Option<String>)> {
    hegel::compose!(|tc| {
        let did: String = tc.draw(hegel::one_of!(
            gs::from_regex("did:plc:[a-z2-7]{24}").fullmatch(true),
            gs::from_regex("did:web:[a-z]{2,8}\\.[a-z]{2,5}").fullmatch(true)
        ));
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => (format!("did={did}"), Some(did)),
            1 => (did, None),
            2 => (format!("DID={did}"), None),
            3 => (format!("did={}", did.to_uppercase()), None),
            _ => (format!("v={}", tc.draw(gs::text().max_size(30))), None),
        }
    })
}

/// Whatever TXT records a handle has, it's found as the DID in the first valid `did=` string,
/// across records in the order DNS gives them, and not found without one.
#[hegel::test(test_cases = 100)]
fn the_first_valid_did_in_dns_names_the_account(tc: TestCase) {
    let drawn: Vec<Vec<(String, Option<String>)>> =
        tc.draw(gs::vecs(gs::vecs(txt()).min_size(1).max_size(3)).max_size(3));
    let expected = drawn.iter().flatten().find_map(|(_, did)| did.clone());
    let records = Records::default();
    if !drawn.is_empty() {
        let strings = drawn.iter().map(|record| record.iter().map(|(s, _)| s.clone()).collect()).collect();
        records.lock().unwrap().insert("_atproto.dj.test".into(), strings);
    }
    let found = block_on(asking(records).dns_did("dj.test"));
    assert_eq!(found.map(|did| did.as_str().to_string()), expected, "{drawn:?}");
}
