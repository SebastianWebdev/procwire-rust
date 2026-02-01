//! Error types for procwire-client.

use thiserror::Error;

/// Main error type for all procwire operations.
#[derive(Debug, Error)]
pub enum ProcwireError {
    /// I/O error during pipe/socket operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error (control plane only).
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// MsgPack serialization/deserialization error.
    #[error("MsgPack encode error: {0}")]
    MsgPackEncode(#[from] rmp_serde::encode::Error),

    /// MsgPack deserialization error.
    #[error("MsgPack decode error: {0}")]
    MsgPackDecode(#[from] rmp_serde::decode::Error),

    /// Protocol error (invalid frame, wrong flags, etc.).
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Handler not found for the given method ID.
    #[error("Handler not found for method ID: {0}")]
    HandlerNotFound(u16),

    /// Connection closed unexpectedly.
    #[error("Connection closed")]
    ConnectionClosed,

    /// Backpressure timeout - write buffer full.
    #[error("Backpressure timeout")]
    BackpressureTimeout,
}

/// Result type alias using ProcwireError.
pub type Result<T> = std::result::Result<T, ProcwireError>;
