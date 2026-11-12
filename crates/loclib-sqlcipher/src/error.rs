/// What can go wrong encrypting or decrypting a database, or replaying its WAL.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A page whose HMAC doesn't match: the wrong key, or a damaged or altered file.
    #[error("page {0} fails authentication (the wrong key, or damaged)")]
    Authentication(u32),

    /// Bytes that aren't laid out as SQLCipher 4 lays a database out, or a WAL that doesn't fit
    /// its database.
    #[error("{0}")]
    Malformed(String),

    /// The system had no randomness for a page's IV.
    #[error("no randomness available: {0}")]
    NoRandomness(getrandom::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
