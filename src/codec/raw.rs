//! Raw codec - pass-through for binary data.
//!
//! Used when payload is already serialized or is raw bytes.
//! Provides zero-copy operations where possible.
//!
//! # Example
//!
//! ```
//! use procwire_client::codec::RawCodec;
//! use bytes::Bytes;
//!
//! // Serialize raw bytes
//! let data = b"binary payload";
//! let serialized = RawCodec::serialize(data);
//! assert_eq!(&serialized[..], data);
//!
//! // Zero-copy with Bytes
//! let bytes = Bytes::from_static(b"zero copy");
//! let passed = RawCodec::serialize_bytes(bytes.clone());
//! assert_eq!(passed.as_ptr(), bytes.as_ptr()); // Same memory
//! ```

use bytes::Bytes;

/// Raw codec that passes bytes through without transformation.
///
/// This is the simplest codec - it does not perform any serialization.
/// Use this when you have raw binary data that should be sent as-is.
pub struct RawCodec;

impl RawCodec {
    /// Serialize raw bytes (copies data into Bytes).
    ///
    /// For truly zero-copy, use `serialize_bytes` with an existing `Bytes` value.
    #[inline]
    pub fn serialize(data: &[u8]) -> Bytes {
        Bytes::copy_from_slice(data)
    }

    /// Serialize Bytes (true zero-copy, just returns the input).
    #[inline]
    pub fn serialize_bytes(data: Bytes) -> Bytes {
        data
    }

    /// Deserialize - returns a reference to the input (zero-copy).
    #[inline]
    pub fn deserialize(data: &[u8]) -> &[u8] {
        data
    }

    /// Deserialize Bytes - returns a reference to the data (zero-copy).
    #[inline]
    pub fn deserialize_bytes(data: &Bytes) -> &[u8] {
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_round_trip() {
        let original = b"hello world";
        let serialized = RawCodec::serialize(original);
        let deserialized = RawCodec::deserialize(&serialized);
        assert_eq!(deserialized, original);
    }

    #[test]
    fn test_serialize_empty() {
        let empty: &[u8] = b"";
        let serialized = RawCodec::serialize(empty);
        assert!(serialized.is_empty());
        assert!(RawCodec::deserialize(&serialized).is_empty());
    }

    #[test]
    fn test_serialize_large_buffer() {
        let large = vec![0xAB; 1024 * 1024]; // 1MB
        let serialized = RawCodec::serialize(&large);
        assert_eq!(serialized.len(), 1024 * 1024);
        assert_eq!(RawCodec::deserialize(&serialized), &large[..]);
    }

    #[test]
    fn test_serialize_bytes_zero_copy() {
        let original = Bytes::from_static(b"static data");
        let serialized = RawCodec::serialize_bytes(original.clone());

        // Should be the exact same Bytes instance
        assert_eq!(serialized.as_ptr(), original.as_ptr());
        assert_eq!(serialized.len(), original.len());
    }

    #[test]
    fn test_deserialize_bytes_zero_copy() {
        let bytes = Bytes::from_static(b"test data");
        let deserialized = RawCodec::deserialize_bytes(&bytes);

        // Should point to the same memory
        assert_eq!(deserialized.as_ptr(), bytes.as_ptr());
    }

    #[test]
    fn test_binary_data_preserved() {
        // Test that all byte values are preserved
        let all_bytes: Vec<u8> = (0..=255).collect();
        let serialized = RawCodec::serialize(&all_bytes);
        assert_eq!(RawCodec::deserialize(&serialized), &all_bytes[..]);
    }
}
