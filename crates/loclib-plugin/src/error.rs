//! JSON-RPC errors, with the codes and kinds of locallibrary's RPC sidecar: -32700 to -32603 for
//! the protocol's own failures, and -32000 with `data.kind` for everything else.

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL: i64 = -32603;
/// Any other failure: `data.kind` says which (`cancelled`, `not_found`, `conflict`...).
pub const FAILED: i64 = -32000;

/// A JSON-RPC error object, as sent and received.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[error("{message}")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    /// Always sent; `None` for an error from a peer that sends none.
    // Boxed: it's most of the error's size, which every `Result` carrying one would be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Box<ErrorData>>,
}

/// `error.data`: what the other side can act on without reading the message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorData {
    /// Stable: `invalid_params`, `cancelled`, `not_found`... (the sidecar's kinds), `incompatible`
    /// (a protocol version the other side doesn't speak) or `closed` (the connection closed
    /// before the response came).
    pub kind: String,
    /// Which decision a `conflict` needs, or what else would resolve the error (`diverged`,
    /// `exists`...): the sidecar's `reason`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The values it's about: for `invalid_params`, `path` says where in the params.
    #[serde(default)]
    pub fields: Map<String, Value>,
}

impl RpcError {
    pub fn new(code: i64, kind: &str, message: impl Into<String>) -> RpcError {
        let data = ErrorData { kind: kind.into(), reason: None, fields: Map::new() };
        RpcError { code, message: message.into(), data: Some(Box::new(data)) }
    }

    /// A failure of kind `kind` (code -32000).
    pub fn failed(kind: &str, message: impl Into<String>) -> RpcError {
        RpcError::new(FAILED, kind, message)
    }

    pub fn parse_error(message: impl std::fmt::Display) -> RpcError {
        RpcError::new(PARSE_ERROR, "parse_error", format!("parse error: {message}"))
    }

    pub fn invalid_request(message: impl Into<String>) -> RpcError {
        RpcError::new(INVALID_REQUEST, "invalid_request", message)
    }

    pub fn method_not_found(method: &str) -> RpcError {
        RpcError::new(METHOD_NOT_FOUND, "method_not_found", format!("unknown method {method:?}"))
    }

    pub fn invalid_params(message: impl std::fmt::Display) -> RpcError {
        RpcError::new(INVALID_PARAMS, "invalid_params", format!("invalid params: {message}"))
    }

    pub fn internal(message: impl Into<String>) -> RpcError {
        RpcError::new(INTERNAL, "internal", message)
    }

    pub fn cancelled() -> RpcError {
        RpcError::failed("cancelled", "operation cancelled")
    }

    /// The connection closed before a response came.
    pub fn closed() -> RpcError {
        RpcError::failed("closed", "the connection closed")
    }

    /// `data.kind`, when it has data.
    pub fn kind(&self) -> Option<&str> {
        self.data.as_ref().map(|d| d.kind.as_str())
    }
}

impl From<loclib_progress::Cancelled> for RpcError {
    fn from(_: loclib_progress::Cancelled) -> Self {
        RpcError::cancelled()
    }
}

/// A request's params as `T`; when they don't fit, `invalid_params` says where (`fields.path`,
/// `track.audio_path`).
pub fn params<T: DeserializeOwned>(v: Value) -> Result<T, RpcError> {
    serde_path_to_error::deserialize(v).map_err(|e| {
        let path = e.path().to_string();
        if path == "." {
            return RpcError::invalid_params(e.inner());
        }
        let message = format!("{path}: {}", e.inner());
        let fields = Map::from_iter([("path".to_string(), Value::from(path))]);
        let data = ErrorData { kind: "invalid_params".into(), reason: None, fields };
        RpcError { data: Some(Box::new(data)), ..RpcError::invalid_params(message) }
    })
}

/// `v` as a result.
pub(crate) fn ok<T: Serialize>(v: T) -> Result<Value, RpcError> {
    serde_json::to_value(v).map_err(|e| RpcError::internal(e.to_string()))
}

/// A response's result as `T`; one that doesn't fit is the other side's bug.
pub(crate) fn result<T: DeserializeOwned>(v: Value) -> Result<T, RpcError> {
    serde_json::from_value(v).map_err(|e| RpcError::internal(format!("unexpected result: {e}")))
}
