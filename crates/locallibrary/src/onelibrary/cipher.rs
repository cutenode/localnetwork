//! SQLCipher 4 page encryption, as rekordbox uses it for `exportLibrary.db`.
//!
//! The file is an ordinary SQLite database whose pages are encrypted one by one:
//!
//! * the first 16 bytes of the file are the key-derivation salt (in place of SQLite's
//!   `SQLite format 3\0` magic);
//! * the encryption key is PBKDF2-HMAC-SHA512 of the passphrase and salt (256,000 rounds), the
//!   HMAC key PBKDF2-HMAC-SHA512 of the encryption key and the salt XOR `0x3a` (2 rounds);
//! * each 4096-byte page keeps 80 reserved bytes at its end (SQLite's "reserved space" header
//!   field says so): a random IV and an HMAC-SHA512 over the ciphertext, the IV and the page
//!   number (little-endian). The rest of the page (minus the salt on page 1) is AES-256-CBC.
//!
//! These are SQLCipher 4's defaults; rekordbox's only twist is a passphrase shared by every
//! installation (found by the pyrekordbox project). rekordbox's own collection database
//! (`master.db`, see [`crate::masterdb`]) is encrypted the same way under another passphrase
//! ([`Codec::with_passphrase`]).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::NoPadding};
use hmac::{Hmac, Mac};
use sha2::Sha512;

use crate::{Error, Result};

pub const PAGE_SIZE: usize = 4096;
pub const SALT_LEN: usize = 16;
const IV_LEN: usize = 16;
const HMAC_LEN: usize = 64;
/// Bytes at the end of every page holding the IV and HMAC (already a multiple of the AES block).
pub const RESERVE: usize = IV_LEN + HMAC_LEN;
const KDF_ROUNDS: u32 = 256_000;
const HMAC_KDF_ROUNDS: u32 = 2;
const HMAC_SALT_MASK: u8 = 0x3a;
/// The passphrase every rekordbox installation encrypts `exportLibrary.db` under (`PRAGMA key`).
pub const PASSPHRASE: &str = "r8gddnr4k847830ar6cqzbkk0el6qytmb3trbbx805jm74vez64i5o8fnrqryqls";
pub const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
/// The page holding SQLite's lock bytes (at 1 GiB), which SQLite never writes or reads: in a file
/// that large it is a hole of zeros.
pub const LOCK_PAGE: u32 = (0x4000_0000 / PAGE_SIZE) as u32 + 1;

type HmacSha512 = Hmac<Sha512>;
type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

#[derive(Clone, Copy)]
struct Keys {
    cipher: [u8; 32],
    hmac: [u8; 32],
}

/// Key derivation costs a few hundred milliseconds and every file keeps its salt for life, so
/// derived keys are remembered per passphrase and salt.
fn derive(passphrase: &[u8], salt: &[u8; SALT_LEN]) -> Keys {
    type Cache = HashMap<(Vec<u8>, [u8; SALT_LEN]), Keys>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let id = (passphrase.to_vec(), *salt);
    if let Some(k) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        return *k;
    }
    let mut cipher = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha512>(passphrase, salt, KDF_ROUNDS, &mut cipher);
    let hmac_salt: Vec<u8> = salt.iter().map(|b| b ^ HMAC_SALT_MASK).collect();
    let mut hmac = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha512>(&cipher, &hmac_salt, HMAC_KDF_ROUNDS, &mut hmac);
    let keys = Keys { cipher, hmac };
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(id, keys);
    keys
}

/// Encrypts and decrypts the pages of one database file.
#[derive(Clone)]
pub struct Codec {
    salt: [u8; SALT_LEN],
    keys: Keys,
}

impl Codec {
    /// The codec of an existing encrypted file (its salt is the first 16 bytes).
    pub fn for_file(bytes: &[u8]) -> Result<Codec> {
        let salt: [u8; SALT_LEN] = bytes
            .get(..SALT_LEN)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::OneLibrary("file too short".into()))?;
        Ok(Codec::with_salt(salt))
    }

    pub fn with_salt(salt: [u8; SALT_LEN]) -> Codec {
        Codec::with_passphrase(PASSPHRASE.as_bytes(), salt)
    }

    /// A codec for a file encrypted under another SQLCipher 4 passphrase (`PRAGMA key`).
    pub fn with_passphrase(passphrase: &[u8], salt: [u8; SALT_LEN]) -> Codec {
        Codec { salt, keys: derive(passphrase, &salt) }
    }

    /// A codec with a fresh random salt, for a new database file.
    pub fn random() -> Result<Codec> {
        let mut salt = [0u8; SALT_LEN];
        getrandom::fill(&mut salt).map_err(|e| Error::OneLibrary(format!("no randomness available: {e}")))?;
        Ok(Codec::with_salt(salt))
    }

    fn mac(&self, region: &[u8], pgno: u32) -> HmacSha512 {
        let mut mac = HmacSha512::new_from_slice(&self.keys.hmac).expect("HMAC takes any key length");
        mac.update(region);
        mac.update(&pgno.to_le_bytes());
        mac
    }

    /// Decrypt page `pgno` (1-based). Page 1 comes back with SQLite's magic in place of the salt.
    /// Every page must authenticate, zeros included: SQLCipher lets a page of zeros through only
    /// in an auto-vacuum database reading past the end of the file, which never happens here.
    pub fn decrypt_page(&self, pgno: u32, page: &[u8]) -> Result<Vec<u8>> {
        if page.len() != PAGE_SIZE {
            return Err(Error::OneLibrary(format!("page {pgno} is {} bytes, expected {PAGE_SIZE}", page.len())));
        }
        let start = if pgno == 1 { SALT_LEN } else { 0 };
        let end = PAGE_SIZE - RESERVE;
        let (iv, tag) = (&page[end..end + IV_LEN], &page[end + IV_LEN..]);
        self.mac(&page[start..end + IV_LEN], pgno).verify_slice(tag).map_err(|_| {
            Error::OneLibrary(format!("page {pgno} fails authentication (not a rekordbox database, or corrupt)"))
        })?;
        let mut out = page.to_vec();
        Aes256CbcDec::new(&self.keys.cipher.into(), iv.into())
            .decrypt_padded_mut::<NoPadding>(&mut out[start..end])
            .map_err(|_| Error::OneLibrary(format!("page {pgno} could not be decrypted")))?;
        if pgno == 1 {
            out[..SALT_LEN].copy_from_slice(SQLITE_MAGIC);
        }
        Ok(out)
    }

    /// Encrypt page `pgno` (1-based) with a fresh IV.
    pub fn encrypt_page(&self, pgno: u32, plain: &[u8]) -> Result<Vec<u8>> {
        if plain.len() != PAGE_SIZE {
            return Err(Error::OneLibrary(format!("page {pgno} is {} bytes, expected {PAGE_SIZE}", plain.len())));
        }
        let start = if pgno == 1 { SALT_LEN } else { 0 };
        let end = PAGE_SIZE - RESERVE;
        let mut out = plain.to_vec();
        let mut iv = [0u8; IV_LEN];
        getrandom::fill(&mut iv).map_err(|e| Error::OneLibrary(format!("no randomness available: {e}")))?;
        Aes256CbcEnc::new(&self.keys.cipher.into(), &iv.into())
            .encrypt_padded_mut::<NoPadding>(&mut out[start..end], end - start)
            .map_err(|_| Error::OneLibrary(format!("page {pgno} could not be encrypted")))?;
        out[end..end + IV_LEN].copy_from_slice(&iv);
        let tag = self.mac(&out[start..end + IV_LEN], pgno).finalize().into_bytes();
        out[end + IV_LEN..].copy_from_slice(&tag);
        if pgno == 1 {
            out[..SALT_LEN].copy_from_slice(&self.salt);
        }
        Ok(out)
    }

    /// Decrypt a whole database file. Pages `read` says SQLite won't read (the lock-byte page, or
    /// pages a WAL replaces) come back as zeros, unchecked.
    pub fn decrypt(&self, bytes: &[u8], read: impl Fn(u32) -> bool) -> Result<Vec<u8>> {
        if bytes.is_empty() || !bytes.len().is_multiple_of(PAGE_SIZE) {
            return Err(Error::OneLibrary(format!("{} bytes is not a whole number of pages", bytes.len())));
        }
        let mut out = Vec::with_capacity(bytes.len());
        for (i, page) in bytes.chunks(PAGE_SIZE).enumerate() {
            let pgno = i as u32 + 1;
            if read(pgno) {
                out.extend(self.decrypt_page(pgno, page)?);
            } else {
                out.resize(out.len() + PAGE_SIZE, 0);
            }
        }
        Ok(out)
    }

    /// Encrypt a whole plain SQLite database, which must have been created with [`RESERVE`] bytes
    /// of reserved space per page (as every decrypted file is).
    pub fn encrypt(&self, plain: &[u8]) -> Result<Vec<u8>> {
        check_plain_header(plain)?;
        let mut out = Vec::with_capacity(plain.len());
        for (i, page) in plain.chunks(PAGE_SIZE).enumerate() {
            out.extend(self.encrypt_page(i as u32 + 1, page)?);
        }
        Ok(out)
    }
}

/// A decrypted (or plain) database must be 4096-byte pages with 80 reserved bytes each, or the
/// IV and HMAC would overwrite data.
pub fn check_plain_header(plain: &[u8]) -> Result<()> {
    if plain.len() < 100 || !plain.len().is_multiple_of(PAGE_SIZE) || &plain[..16] != SQLITE_MAGIC {
        return Err(Error::OneLibrary("not an SQLite database".into()));
    }
    let page_size = u16::from_be_bytes([plain[16], plain[17]]) as usize;
    if page_size != PAGE_SIZE || plain[20] as usize != RESERVE {
        return Err(Error::OneLibrary(format!(
            "unsupported layout: {page_size}-byte pages with {} reserved bytes",
            plain[20]
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
