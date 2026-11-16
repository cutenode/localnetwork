use hegel::generators as gs;

use super::super::{Codec, keys};
use super::*;

const KEYS: Keys = Keys { cipher: [1; 32], hmac: [2; 32] };

/// What's saved comes back for its salt, and for no other.
#[test]
fn keys_come_back_for_their_salt_alone() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("deeper/keys");
    save(&folder, &[7; SALT_LEN], &KEYS);
    let found = load(&folder, &[7; SALT_LEN]).unwrap();
    assert_eq!((found.cipher, found.hmac), (KEYS.cipher, KEYS.hmac));
    assert!(load(&folder, &[8; SALT_LEN]).is_none());
}

/// A file renamed to another salt's name holds keys that aren't that salt's.
#[test]
fn a_file_under_another_salts_name_reads_as_none() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), &[7; SALT_LEN], &KEYS);
    std::fs::rename(file(dir.path(), &[7; SALT_LEN]), file(dir.path(), &[8; SALT_LEN])).unwrap();
    assert!(load(dir.path(), &[8; SALT_LEN]).is_none());
}

/// Only this user can read the folder or what's in it, even a folder that was there already.
#[cfg(unix)]
#[test]
fn keys_are_readable_by_this_user_alone() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("keys");
    std::fs::create_dir(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    save(&folder, &[7; SALT_LEN], &KEYS);
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&folder), 0o700);
    assert_eq!(mode(&file(&folder, &[7; SALT_LEN])), 0o600);
}

/// A folder that can't be made costs only the time of deriving next run.
#[test]
fn a_failure_to_keep_keys_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let in_the_way = dir.path().join("file");
    std::fs::write(&in_the_way, b"").unwrap();
    save(&in_the_way, &[7; SALT_LEN], &KEYS);
    assert!(load(&in_the_way, &[7; SALT_LEN]).is_none());
}

/// Whatever damage a file takes, it reads as none rather than as other keys.
#[hegel::test(test_cases = 100)]
fn a_damaged_file_reads_as_none(tc: hegel::TestCase) {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), &[7; SALT_LEN], &KEYS);
    let path = file(dir.path(), &[7; SALT_LEN]);
    let mut bytes = std::fs::read(&path).unwrap();
    let at = tc.draw(gs::integers::<usize>().max_value(FILE_LEN - 1));
    match tc.draw(gs::booleans()) {
        true => bytes[at] ^= tc.draw(gs::integers::<u8>().min_value(1)),
        false => bytes.truncate(at),
    }
    std::fs::write(&path, bytes).unwrap();
    assert!(load(dir.path(), &[7; SALT_LEN]).is_none());
}

/// The codec takes keys kept for a salt, keeps those it derives, and derives again past a damaged
/// file. Only rekordbox's passphrase is kept: another key isn't this module's to write down.
#[test]
fn the_codec_uses_kept_keys_and_keeps_those_it_derives() {
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: nextest gives each test a process of its own, and no other thread of it reads the
    // environment while the test runs.
    unsafe { std::env::set_var(TEST_FOLDER, dir.path()) };
    save(dir.path(), &[1; SALT_LEN], &KEYS);
    assert_eq!(keys(&[1; SALT_LEN]).cipher, KEYS.cipher);

    let derived = keys(&[2; SALT_LEN]);
    let kept = load(dir.path(), &[2; SALT_LEN]).unwrap();
    assert_eq!((kept.cipher, kept.hmac), (derived.cipher, derived.hmac));

    std::fs::write(file(dir.path(), &[3; SALT_LEN]), b"damaged").unwrap();
    assert_ne!(keys(&[3; SALT_LEN]).cipher, [0; 32]);
    assert!(load(dir.path(), &[3; SALT_LEN]).is_some());

    let _ = Codec::with_key(b"another key", [4; SALT_LEN]);
    assert!(load(dir.path(), &[4; SALT_LEN]).is_none());
}
