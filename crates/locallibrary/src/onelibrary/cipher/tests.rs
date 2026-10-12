use super::*;

fn plain_db(pages: usize) -> Vec<u8> {
    let mut db: Vec<u8> = (0..pages * PAGE_SIZE).map(|i| (i * 7 % 251) as u8).collect();
    db[..16].copy_from_slice(SQLITE_MAGIC);
    db[16..18].copy_from_slice(&(PAGE_SIZE as u16).to_be_bytes());
    db[20] = RESERVE as u8;
    db
}

#[test]
fn roundtrip_keeps_everything_but_the_reserve() {
    let codec = Codec::with_salt([7; SALT_LEN]);
    let plain = plain_db(3);
    let enc = codec.encrypt(&plain).unwrap();
    assert_eq!(&enc[..SALT_LEN], &[7; SALT_LEN]);
    assert_ne!(&enc[SALT_LEN..PAGE_SIZE - RESERVE], &plain[SALT_LEN..PAGE_SIZE - RESERVE]);
    let back = Codec::for_file(&enc).unwrap().decrypt(&enc, |_| true).unwrap();
    for (a, b) in back.chunks(PAGE_SIZE).zip(plain.chunks(PAGE_SIZE)) {
        assert_eq!(&a[..PAGE_SIZE - RESERVE], &b[..PAGE_SIZE - RESERVE]);
    }
    // Fresh IVs: encrypting twice gives different files that decrypt the same.
    assert_ne!(codec.encrypt(&plain).unwrap(), enc);
}

#[test]
fn tampering_and_wrong_salt_are_detected() {
    let codec = Codec::with_salt([1; SALT_LEN]);
    let mut enc = codec.encrypt(&plain_db(2)).unwrap();
    enc[PAGE_SIZE + 100] ^= 1;
    assert!(codec.decrypt(&enc, |_| true).is_err());
    // A page SQLite won't read isn't checked.
    assert!(codec.decrypt(&enc, |pgno| pgno != 2).is_ok());
    let enc = codec.encrypt(&plain_db(2)).unwrap();
    assert!(Codec::with_salt([2; SALT_LEN]).decrypt_page(2, &enc[PAGE_SIZE..]).is_err());
    // A page wiped to zeros (a torn write on a FAT drive, say) is damage, as in SQLCipher.
    assert!(codec.decrypt_page(2, &[0; PAGE_SIZE]).is_err());
}

#[test]
fn refuses_databases_without_room_for_the_cipher() {
    let mut plain = plain_db(1);
    plain[20] = 0;
    assert!(Codec::with_salt([0; SALT_LEN]).encrypt(&plain).is_err());
}
