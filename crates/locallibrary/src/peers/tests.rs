use super::*;
use hegel::generators as gs;
use hegel::{Generator, TestCase};

/// A share code for a random key and address, with `token`.
fn code(tc: &TestCase, token: String) -> ShareCode {
    let key: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).min_size(32).max_size(32));
    let key = iroh::SecretKey::from_bytes(&key.try_into().expect("32 bytes")).public();
    let ip = std::net::Ipv4Addr::from(tc.draw(gs::integers::<u32>()));
    let port = tc.draw(gs::integers::<u16>());
    let addr = iroh::EndpointAddr::new(key).with_ip_addr(std::net::SocketAddr::from((ip, port)));
    ShareCode { ticket: addr.into(), token }
}

/// A share code reads back from what it prints, around any whitespace, and names the same
/// computer; a fresh one's token is 32 hex digits.
#[hegel::test(test_cases = 200)]
fn share_codes_read_back(tc: TestCase) {
    let token: String = tc.draw(gs::from_regex("[0-9a-fA-F]{32}").fullmatch(true));
    let c = code(&tc, token);
    let printed = c.to_string();
    assert_eq!(format!(" {printed}\n").parse::<ShareCode>().unwrap(), c);
    assert_eq!(printed.parse::<ShareCode>().unwrap().addr(), c.addr());
    let fresh = ShareCode::new(c.addr().clone());
    assert!(fresh.token.len() == 32 && fresh.token.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(fresh.to_string().parse::<ShareCode>().unwrap(), fresh);
}

/// A code with its token cut short, made longer, or with a letter that isn't hex, or with
/// its ticket mangled, isn't read as one; whatever text does read as a code prints back as
/// a code with the same token.
#[hegel::test(test_cases = 300)]
fn mangled_share_codes_are_refused(tc: TestCase) {
    let c = code(&tc, Uuid::new_v4().simple().to_string());
    let printed = c.to_string();
    let (ticket, token) = printed.rsplit_once('-').unwrap();
    let cut = tc.draw(gs::integers::<usize>().min_value(1).max_value(31));
    let junk = tc.draw(gs::sampled_from(vec!['g', 'z', '-', ' ', 'é']));
    let at = tc.draw(gs::integers::<usize>().max_value(31));
    let mut bad_letter: Vec<char> = token.chars().collect();
    bad_letter[at] = junk;
    let bad_letter: String = bad_letter.into_iter().collect();
    for bad in [
        format!("{ticket}-{}", &token[..cut]),
        format!("{ticket}-{token}0"),
        format!("{ticket}-{bad_letter}"),
        format!("{ticket}{token}"),
        format!("-{token}"),
        format!("x{ticket}-{token}"),
    ] {
        assert!(bad.parse::<ShareCode>().is_err(), "{bad:?} was read as a code");
    }
    let padded = format!("{}{printed}{}", " ".repeat(cut % 3), "\n".repeat(at % 2));
    let text: String = tc.draw(hegel::one_of!(
        gs::text().max_size(80),
        gs::just(padded),
        gs::integers::<usize>().max_value(printed.len()).map(move |n| printed[..n].to_string()),
    ));
    if let Ok(read) = text.parse::<ShareCode>() {
        let token = text.trim().rsplit_once('-').unwrap().1;
        assert_eq!(read.token, token);
        assert_eq!(read.to_string().parse::<ShareCode>().unwrap(), read);
    }
}
