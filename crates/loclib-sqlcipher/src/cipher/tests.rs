use super::*;

const KEY: &[u8] = b"a key";

fn keyed(salt: u8) -> Codec {
    Codec::with_key(KEY, [salt; SALT_LEN])
}

pub(crate) fn plain_db(pages: usize) -> Vec<u8> {
    let mut db: Vec<u8> = (0..pages * PAGE_SIZE).map(|i| (i * 7 % 251) as u8).collect();
    db[..16].copy_from_slice(SQLITE_MAGIC);
    db[16..18].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
    db[20] = RESERVE as u8;
    db
}

#[test]
fn roundtrip_keeps_everything_but_the_reserve() {
    let codec = keyed(7);
    let plain = plain_db(3);
    let enc = codec.encrypt(&plain).unwrap();
    assert_eq!(&enc[..SALT_LEN], &[7; SALT_LEN]);
    assert_ne!(&enc[SALT_LEN..PAGE_SIZE - RESERVE], &plain[SALT_LEN..PAGE_SIZE - RESERVE]);
    let back = keyed(7).decrypt(&enc, |_| true).unwrap();
    for (a, b) in back.chunks(PAGE_SIZE).zip(plain.chunks(PAGE_SIZE)) {
        assert_eq!(&a[..PAGE_SIZE - RESERVE], &b[..PAGE_SIZE - RESERVE]);
    }
    // Fresh IVs: encrypting twice gives different files that decrypt the same.
    assert_ne!(codec.encrypt(&plain).unwrap(), enc);
}

#[test]
fn tampering_and_wrong_salt_are_detected() {
    let codec = keyed(1);
    let mut enc = codec.encrypt(&plain_db(2)).unwrap();
    enc[PAGE_SIZE + 100] ^= 1;
    assert!(codec.decrypt(&enc, |_| true).is_err());
    // A page SQLite won't read isn't checked.
    assert!(codec.decrypt(&enc, |pgno| pgno != 2).is_ok());
    let enc = codec.encrypt(&plain_db(2)).unwrap();
    assert!(keyed(2).decrypt_page(2, &enc[PAGE_SIZE..]).is_err());
    // A page wiped to zeros (a torn write on a FAT drive, say) is damage, as in SQLCipher.
    assert!(codec.decrypt_page(2, &[0; PAGE_SIZE]).is_err());
}

#[test]
fn refuses_databases_without_room_for_the_cipher() {
    let mut plain = plain_db(1);
    plain[20] = 0;
    assert!(keyed(0).encrypt(&plain).is_err());
}

/// The keys are SQLCipher 4's (PBKDF2-HMAC-SHA512 of the key, 256,000 rounds; then of that and
/// the salt XOR 0x3a, 2 rounds), as Python's `hashlib.pbkdf2_hmac` computes them.
#[test]
fn keys_are_sqlciphers() {
    let hex = |b: &[u8]| b.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let keys = derive_keys(KEY, &[7; SALT_LEN]);
    assert_eq!(hex(&keys.cipher), "1c462c44cc434a716a2d9604c2f6c523f7e9cfd8fe1be78fd6d9a4aca5a98181");
    assert_eq!(hex(&keys.hmac), "623140ec7a02b70621bf839b25546734320db0f1eb8fc8acae5e3d0209d1418f");
}

/// Keys handed in (kept from another run, say) read what the key wrote; other keys don't.
#[test]
fn a_codec_takes_keys_derived_elsewhere() {
    let enc = keyed(7).encrypt(&plain_db(2)).unwrap();
    let kept = Codec::with_keys([7; SALT_LEN], derive_keys(KEY, &[7; SALT_LEN]));
    assert_eq!(kept.decrypt(&enc, |_| true).unwrap(), keyed(7).decrypt(&enc, |_| true).unwrap());
    let other = Codec::with_keys([7; SALT_LEN], Keys { cipher: [1; 32], hmac: [2; 32] });
    assert!(matches!(other.decrypt_page(2, &enc[PAGE_SIZE..]), Err(Error::Authentication(2))));
}

#[test]
fn only_sqlcipher_layouts_are_plain_databases() {
    assert!(check_plain_header(&plain_db(2)).is_ok());
    let mut other = plain_db(1);
    other[16..18].copy_from_slice(&1024u16.to_be_bytes());
    let err = check_plain_header(&other).unwrap_err().to_string();
    assert_eq!(err, "unsupported layout: 1024-byte pages with 80 reserved bytes");
    assert!(check_plain_header(&plain_db(1)[..99]).is_err());
    assert!(check_plain_header(&[0; PAGE_SIZE]).is_err());
    // The lock-byte page is the one holding byte 2^30.
    assert_eq!((LOCK_PAGE as usize - 1) * PAGE_SIZE, 1 << 30);
}
