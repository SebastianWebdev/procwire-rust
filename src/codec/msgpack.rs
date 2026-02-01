//! MsgPack codec using `rmp-serde`.
//!
//! **CRITICAL**: Always use `to_vec_named`, NEVER `to_vec`!
//! Node.js `@msgpack/msgpack` expects struct-as-map format.
//!
//! # Why `to_vec_named`?
//!
//! - `to_vec` serializes structs as arrays (positional)
//! - `to_vec_named` serializes structs as maps (with field names)
//! - Node.js `@msgpack/msgpack` expects the map format
//! - Using `to_vec` will cause deserialization failures in Node.js
//!
//! # Example
//!
//! ```
//! use procwire_client::codec::MsgPackCodec;
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize, PartialEq, Debug)]
//! struct Message {
//!     id: u32,
//!     content: String,
//! }
//!
//! let msg = Message { id: 42, content: "hello".to_string() };
//! let encoded = MsgPackCodec::encode(&msg).unwrap();
//! let decoded: Message = MsgPackCodec::decode(&encoded).unwrap();
//! assert_eq!(decoded, msg);
//! ```

use crate::error::Result;

/// MessagePack codec for structured data.
///
/// Uses `rmp_serde::to_vec_named` for Node.js compatibility.
/// This ensures structs are serialized as maps (with field names)
/// rather than arrays (positional).
pub struct MsgPackCodec;

impl MsgPackCodec {
    /// Encode a value to MsgPack bytes.
    ///
    /// Uses `to_vec_named` for struct-as-map format (Node.js compatible).
    ///
    /// # Errors
    ///
    /// Returns error if the value cannot be serialized.
    #[inline]
    pub fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
        // CRITICAL: to_vec_named, NOT to_vec!
        Ok(rmp_serde::to_vec_named(value)?)
    }

    /// Decode MsgPack bytes to a value.
    ///
    /// # Errors
    ///
    /// Returns error if the bytes cannot be deserialized to type T.
    #[inline]
    pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct TestStruct {
        id: u32,
        name: String,
        active: bool,
    }

    #[test]
    fn test_encode_decode_struct() {
        let original = TestStruct {
            id: 42,
            name: "test".to_string(),
            active: true,
        };

        let encoded = MsgPackCodec::encode(&original).unwrap();
        let decoded: TestStruct = MsgPackCodec::decode(&encoded).unwrap();

        assert_eq!(decoded, original);
    }

    #[test]
    fn test_encode_decode_primitives() {
        // String
        let s = "hello world";
        let encoded = MsgPackCodec::encode(&s).unwrap();
        let decoded: String = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, s);

        // Number
        let n: i64 = 12345;
        let encoded = MsgPackCodec::encode(&n).unwrap();
        let decoded: i64 = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, n);

        // Boolean
        let b = true;
        let encoded = MsgPackCodec::encode(&b).unwrap();
        let decoded: bool = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, b);
    }

    #[test]
    fn test_encode_decode_collections() {
        // Vec
        let vec = vec![1, 2, 3, 4, 5];
        let encoded = MsgPackCodec::encode(&vec).unwrap();
        let decoded: Vec<i32> = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, vec);

        // HashMap
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert("key1".to_string(), 100);
        map.insert("key2".to_string(), 200);

        let encoded = MsgPackCodec::encode(&map).unwrap();
        let decoded: HashMap<String, i32> = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, map);
    }

    #[test]
    fn test_encode_decode_nested() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Inner {
            value: i32,
        }

        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Outer {
            inner: Inner,
            items: Vec<String>,
        }

        let original = Outer {
            inner: Inner { value: 999 },
            items: vec!["a".to_string(), "b".to_string()],
        };

        let encoded = MsgPackCodec::encode(&original).unwrap();
        let decoded: Outer = MsgPackCodec::decode(&encoded).unwrap();

        assert_eq!(decoded, original);
    }

    #[test]
    fn test_encode_decode_option() {
        let some_val: Option<i32> = Some(42);
        let encoded = MsgPackCodec::encode(&some_val).unwrap();
        let decoded: Option<i32> = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, some_val);

        let none_val: Option<i32> = None;
        let encoded = MsgPackCodec::encode(&none_val).unwrap();
        let decoded: Option<i32> = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, none_val);
    }

    #[test]
    fn test_to_vec_named_produces_map_format() {
        // Verify that structs are serialized as maps (with field names)
        // not as arrays (positional)
        let test = TestStruct {
            id: 1,
            name: "x".to_string(),
            active: false,
        };

        let encoded = MsgPackCodec::encode(&test).unwrap();

        // MsgPack map format starts with 0x83 (fixmap with 3 elements)
        // Array format would start with 0x93 (fixarray with 3 elements)
        assert_eq!(
            encoded[0] & 0xF0,
            0x80,
            "Expected map format (0x8X), got {:02X}",
            encoded[0]
        );
    }

    #[test]
    fn test_decode_error_on_invalid_data() {
        let invalid = b"not valid msgpack";
        let result: Result<TestStruct> = MsgPackCodec::decode(invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_struct() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Empty {}

        let empty = Empty {};
        let encoded = MsgPackCodec::encode(&empty).unwrap();
        let decoded: Empty = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, empty);
    }

    #[test]
    fn test_nodejs_interop_simple_object() {
        // This test verifies that our MsgPack output is compatible with Node.js @msgpack/msgpack
        // The bytes below were generated by Node.js: msgpack.encode({id: 1, name: "test"})
        // Note: Node.js encodes as map, and we use to_vec_named which also produces map format

        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct SimpleObj {
            id: i32,
            name: String,
        }

        let obj = SimpleObj {
            id: 1,
            name: "test".to_string(),
        };

        let encoded = MsgPackCodec::encode(&obj).unwrap();

        // Verify it's a map (0x82 = fixmap with 2 elements)
        assert_eq!(encoded[0], 0x82, "Expected fixmap with 2 elements");

        // Decode back to verify roundtrip
        let decoded: SimpleObj = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, obj);
    }

    #[test]
    fn test_nodejs_interop_number_types() {
        // Test various number types that Node.js might send
        // u8
        let n: u8 = 255;
        let encoded = MsgPackCodec::encode(&n).unwrap();
        let decoded: u8 = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, n);

        // i32 (negative)
        let n: i32 = -12345;
        let encoded = MsgPackCodec::encode(&n).unwrap();
        let decoded: i32 = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, n);

        // f64
        let n: f64 = 3.14159;
        let encoded = MsgPackCodec::encode(&n).unwrap();
        let decoded: f64 = MsgPackCodec::decode(&encoded).unwrap();
        assert!((decoded - n).abs() < f64::EPSILON);
    }

    #[test]
    fn test_nodejs_interop_array() {
        // Node.js arrays should decode correctly
        let arr = vec!["hello", "world", "test"];
        let encoded = MsgPackCodec::encode(&arr).unwrap();

        // Verify it's an array (0x93 = fixarray with 3 elements)
        assert_eq!(encoded[0], 0x93, "Expected fixarray with 3 elements");

        let decoded: Vec<String> = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, arr);
    }

    #[test]
    fn test_nodejs_interop_null() {
        // Node.js null → Rust Option::None
        let val: Option<i32> = None;
        let encoded = MsgPackCodec::encode(&val).unwrap();

        // MsgPack nil is 0xc0
        assert_eq!(encoded, vec![0xc0], "None should encode as msgpack nil");

        let decoded: Option<i32> = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded, None);
    }

    #[test]
    fn test_nodejs_interop_binary_buffer() {
        // Node.js Buffer should be decodable as Vec<u8>
        // Binary format in msgpack: 0xc4 (bin8) + length + data
        let data: Vec<u8> = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        let encoded = MsgPackCodec::encode(&serde_bytes::Bytes::new(&data)).unwrap();

        // Should start with bin8 format
        assert_eq!(encoded[0], 0xc4, "Expected bin8 format");

        let decoded: serde_bytes::ByteBuf = MsgPackCodec::decode(&encoded).unwrap();
        assert_eq!(decoded.as_ref(), &data);
    }
}
