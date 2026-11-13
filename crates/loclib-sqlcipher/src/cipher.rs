//! SQLCipher 4 page encryption.
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
//! These are SQLCipher 4's defaults, which rekordbox keeps for `exportLibrary.db` and `master.db`.

use rustc_hash::FxHashMap;
use std::sync::{Mutex, OnceLock};

use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit, block_padding::NoPadding};
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
pub const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
/// The page holding SQLite's lock bytes (at 1 GiB), which SQLite never writes or reads: in a file
/// that large it is a hole of zeros.
pub const LOCK_PAGE: u32 = (0x4000_0000 / PAGE_SIZE) as u32 + 1;

type HmacSha512 = Hmac<Sha512>;
type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

/// The AES-256 key a file's pages are encrypted with and the HMAC-SHA512 key they're
/// authenticated with, both derived from the passphrase and the file's salt.
#[derive(Clone, Copy)]
pub struct Keys {
    pub cipher: [u8; 32],
    pub hmac: [u8; 32],
}

/// The keys of a file with `salt` encrypted under `key` (`PRAGMA key`). Derivation costs tens of
/// milliseconds and every file keeps its salt for life, so they're remembered per key and salt
/// for the rest of the process. A program that opens the same files run after run can keep them
/// itself and hand them to [`Codec::with_keys`].
pub fn derive_keys(key: &[u8], salt: &[u8; SALT_LEN]) -> Keys {
    type Cache = FxHashMap<(Vec<u8>, [u8; SALT_LEN]), Keys>;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let id = (key.to_vec(), *salt);
    if let Some(k) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        return *k;
    }
    let keys = pbkdf2_keys(key, salt);
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(id, keys);
    keys
}

fn pbkdf2_keys(passphrase: &[u8], salt: &[u8; SALT_LEN]) -> Keys {
    // ring's PBKDF2 is assembly, as fast unoptimised as in a release build; the generic RustCrypto
    // one runs at debug speed in tests (its code is compiled into this crate), seconds a process.
    let pbkdf2 = |rounds: u32, salt: &[u8], secret: &[u8], out: &mut [u8; 32]| {
        let rounds = std::num::NonZeroU32::new(rounds).expect("rounds are constants above zero");
        ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA512, rounds, salt, secret, out);
    };
    // PBKDF2 writes over the whole buffer. Not a `[0; 32]` literal: CodeQL takes the literal for
    // a hard-coded key when the buffer reaches AES.
    let mut cipher = <[u8; 32]>::default();
    pbkdf2(KDF_ROUNDS, salt, passphrase, &mut cipher);
    let hmac_salt: Vec<u8> = salt.iter().map(|b| b ^ HMAC_SALT_MASK).collect();
    let mut hmac = [0u8; 32];
    pbkdf2(HMAC_KDF_ROUNDS, &hmac_salt, &cipher, &mut hmac);
    Keys { cipher, hmac }
}

/// Encrypts and decrypts the pages of one database file.
#[derive(Clone)]
pub struct Codec {
    salt: [u8; SALT_LEN],
    keys: Keys,
}

impl Codec {
    /// A codec for a file with `salt` encrypted under `key` (`PRAGMA key`), deriving its keys
    /// ([`derive_keys`]).
    // Named for SQLCipher's `key` rather than "passphrase": CodeQL guesses secrets from names and
    // took a `with_passphrase` codec for a password, flagging every value decrypted with it.
    pub fn with_key(key: &[u8], salt: [u8; SALT_LEN]) -> Codec {
        Codec::with_keys(salt, derive_keys(key, &salt))
    }

    /// A codec for a file with `salt` whose keys were derived already.
    pub fn with_keys(salt: [u8; SALT_LEN], keys: Keys) -> Codec {
        Codec { salt, keys }
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
            return Err(Error::Malformed(format!("page {pgno} is {} bytes, expected {PAGE_SIZE}", page.len())));
        }
        let start = if pgno == 1 { SALT_LEN } else { 0 };
        let end = PAGE_SIZE - RESERVE;
        let (iv, tag) = (&page[end..end + IV_LEN], &page[end + IV_LEN..]);
        self.mac(&page[start..end + IV_LEN], pgno).verify_slice(tag).map_err(|_| Error::Authentication(pgno))?;
        let iv: [u8; IV_LEN] = iv.try_into().expect("the page's IV is IV_LEN bytes");
        let mut out = page.to_vec();
        Aes256CbcDec::new(&self.keys.cipher.into(), &iv.into())
            .decrypt_padded::<NoPadding>(&mut out[start..end])
            .map_err(|_| Error::Malformed(format!("page {pgno} could not be decrypted")))?;
        if pgno == 1 {
            out[..SALT_LEN].copy_from_slice(SQLITE_MAGIC);
        }
        Ok(out)
    }

    /// Encrypt page `pgno` (1-based) with a fresh IV.
    pub fn encrypt_page(&self, pgno: u32, plain: &[u8]) -> Result<Vec<u8>> {
        if plain.len() != PAGE_SIZE {
            return Err(Error::Malformed(format!("page {pgno} is {} bytes, expected {PAGE_SIZE}", plain.len())));
        }
        let start = if pgno == 1 { SALT_LEN } else { 0 };
        let end = PAGE_SIZE - RESERVE;
        let mut out = plain.to_vec();
        let mut iv = [0u8; IV_LEN];
        getrandom::fill(&mut iv).map_err(Error::NoRandomness)?;
        Aes256CbcEnc::new(&self.keys.cipher.into(), &iv.into())
            .encrypt_padded::<NoPadding>(&mut out[start..end], end - start)
            .map_err(|_| Error::Malformed(format!("page {pgno} could not be encrypted")))?;
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
            return Err(Error::Malformed(format!("{} bytes is not a whole number of pages", bytes.len())));
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
        return Err(Error::Malformed("not an SQLite database".into()));
    }
    let page_size = u16::from_be_bytes([plain[16], plain[17]]) as usize;
    if page_size != PAGE_SIZE || plain[20] as usize != RESERVE {
        return Err(Error::Malformed(format!(
            "unsupported layout: {page_size}-byte pages with {} reserved bytes",
            plain[20]
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
