# loclib-sqlcipher

Reads and writes SQLCipher 4 databases a page at a time, in memory and in pure Rust, and replays
SQLite's write-ahead log over them. It was split out of
[locallibrary](https://github.com/cutenode/localnetwork/tree/main/apps/locallibrary), which reads
and edits rekordbox's `exportLibrary.db` on USB drives and reads rekordbox's own `master.db`. Both
are SQLCipher 4 databases.

Nothing links SQLCipher, SQLite or OpenSSL, so it builds without a C toolchain. A decrypted
database is an ordinary SQLite file, for rusqlite or anything else to open.

```rust,no_run
use loclib_sqlcipher::{Codec, SALT_LEN, decrypt};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let db = std::fs::read("library.db")?;
let wal = std::fs::read("library.db-wal").ok();
let salt: [u8; SALT_LEN] = db[..SALT_LEN].try_into()?;
let codec = Codec::with_key(std::env::var("SQLCIPHER_KEY")?.as_bytes(), salt);
std::fs::write("plain.db", decrypt(&codec, &db, wal.as_deref())?)?;
# Ok(())
# }
```

## What it implements

SQLCipher 4's defaults:

- the first 16 bytes of the file are the key-derivation salt, in place of SQLite's
  `SQLite format 3\0` magic;
- the encryption key is PBKDF2-HMAC-SHA512 of the passphrase (`PRAGMA key`) and the salt, 256,000
  rounds, and the HMAC key PBKDF2-HMAC-SHA512 of the encryption key and the salt XOR `0x3a`, 2
  rounds;
- each 4096-byte page keeps 80 reserved bytes at its end (SQLite's "reserved space" header field
  says so): a random IV and an HMAC-SHA512 over the ciphertext, the IV and the page number,
  little-endian. The rest of the page, minus the salt on page 1, is AES-256-CBC.

`Codec` encrypts and decrypts single pages (`encrypt_page`, `decrypt_page`) or whole files.
Every page it decrypts must authenticate. `encrypt` takes a plain database of 4096-byte pages
with 80 bytes of reserved space each: one this crate decrypted, or one SQLite made with
`PRAGMA page_size = 4096` and `SQLITE_FCNTL_RESERVE_BYTES`.

Key derivation takes tens of milliseconds, so `derive_keys` remembers keys per passphrase and salt
for the rest of the process. A program that opens the same files run after run can keep the
`Keys` itself and hand them to `Codec::with_keys`.

## The write-ahead log

A database in WAL mode often has its latest changes only in its `-wal` file. `decrypt` does what
SQLite does when it opens one: it takes the frames of every committed transaction and lays their
pages over the main file. The format follows
[sqlite.org/fileformat2.html#walformat](https://sqlite.org/fileformat2.html#walformat): a frame
counts only if its salts match the header and the checksum chain holds up to it, and only frames up
to the last commit among those apply. With SQLCipher the frames' pages are encrypted like the main
file's.

`wal::committed` reads a WAL on its own. `reads_main_page` and `wal_pages` are the steps of
`decrypt`, for a program that decrypts a large file a page at a time instead of whole. Pages SQLite
won't read from the main file (the lock-byte page at 1 GiB, pages a commit replaces) aren't
checked, and a commit claiming more pages than the files hold is refused instead of allocated.

## rekordbox

rekordbox encrypts `exportLibrary.db` (OneLibrary, on drives) and `master.db` (its collection)
with SQLCipher 4's defaults, under passphrases every installation shares. This crate doesn't carry
them: [pyrekordbox](https://github.com/dylanljones/pyrekordbox) documents where they come from.

## Features

- `rusqlite`: `untrusted::distrust` and `untrusted::damaged`, for opening a decrypted database
  (or any SQLite file someone else wrote) with [rusqlite](https://github.com/rusqlite/rusqlite):
  SQLite's defensive mode, `trusted_schema` off and `cell_size_check` on, refusing schema objects
  the writer never makes (a view standing in for a table can loop forever when read, a trigger
  runs on every edit), and `PRAGMA quick_check`. It builds SQLite from C; the rest of the crate
  is Rust only.
- `test-util`: `wal::tests::build`, WALs written as SQLite writes them, for tests. Not a stable
  API.

## What it doesn't do

- Run SQL. Open the decrypted bytes with SQLite (the `rusqlite` feature above helps).
- Other page sizes, KDF settings or SQLCipher versions before 4.
- Write a WAL, or checkpoint one back into an encrypted file. Re-encrypt the whole database
  instead.

## Licence

MIT.
