//! Codec module - serialization/deserialization for payloads.
//!
//! This module provides codecs for encoding/decoding frame payloads:
//!
//! - [`RawCodec`] - Pass-through for raw bytes (zero-copy)
//! - [`MsgPackCodec`] - MessagePack using `rmp-serde` (to_vec_named for Node.js compatibility)
//!
//! # Design
//!
//! Codecs are implemented as marker structs with static methods rather than trait objects.
//! This allows for compile-time codec selection and enables zero-copy optimizations.
//!
//! # Example
//!
//! ```
//! use procwire_client::codec::{MsgPackCodec, RawCodec};
//! use bytes::Bytes;
//!
//! // MsgPack codec for structured data
//! let encoded = MsgPackCodec::encode(&"hello").unwrap();
//! let decoded: String = MsgPackCodec::decode(&encoded).unwrap();
//! assert_eq!(decoded, "hello");
//!
//! // Raw codec for binary data
//! let raw = RawCodec::serialize(b"binary data");
//! assert_eq!(RawCodec::deserialize(&raw), b"binary data");
//! ```

mod msgpack;
mod raw;

pub use msgpack::MsgPackCodec;
pub use raw::RawCodec;
