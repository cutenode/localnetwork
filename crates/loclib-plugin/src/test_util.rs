//! Test support: [`FakeHost`], a host to test a plugin against over the protocol.

mod fake_host;

pub use self::fake_host::{FakeHost, Response, Sent};

/// An absolute path on this computer, ending in `name`: hosts give an `x-path` param only as one.
#[cfg(test)]
pub(crate) fn absolute(name: &str) -> String {
    if cfg!(windows) { format!("C:\\{name}") } else { format!("/{name}") }
}
