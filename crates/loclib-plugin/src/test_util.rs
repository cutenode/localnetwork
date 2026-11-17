//! Test support: [`FakeHost`], a host to test a plugin against over the protocol.

mod fake_host;

pub use self::fake_host::{FakeHost, Response, Sent};
