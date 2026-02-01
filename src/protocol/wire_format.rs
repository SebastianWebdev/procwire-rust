//! Wire format encoding and decoding.
//!
//! Implements the 11-byte header format:
//! ```text
//! ┌──────────┬───────┬──────────┬──────────┐
//! │ Method ID│ Flags │ Req ID   │ Length   │
//! │ 2 bytes  │ 1 byte│ 4 bytes  │ 4 bytes  │
//! │ uint16 BE│       │ uint32 BE│ uint32 BE│
//! └──────────┴───────┴──────────┴──────────┘
//! ```
//!
//! All multi-byte integers are Big Endian.

use crate::error::{ProcwireError, Result};

/// Header size in bytes (fixed, exactly 11).
pub const HEADER_SIZE: usize = 11;

/// Default maximum payload size (1 GB).
pub const DEFAULT_MAX_PAYLOAD_SIZE: u32 = 1_073_741_824;

/// Absolute maximum payload size (~2 GB, max i32).
pub const ABSOLUTE_MAX_PAYLOAD_SIZE: u32 = 2_147_483_647;

/// Reserved method ID for ABORT signal.
pub const ABORT_METHOD_ID: u16 = 0xFFFF;

/// Reserved method ID (never use).
pub const RESERVED_METHOD_ID: u16 = 0;

/// Header pool size for ring buffer optimization.
pub const HEADER_POOL_SIZE: usize = 16;

/// Flag constants for the protocol.
pub mod flags {
    /// Direction: to parent (1) or to child (0).
    pub const DIRECTION_TO_PARENT: u8 = 0b0000_0001;
    /// Message type: response (1) or request/event (0).
    pub const IS_RESPONSE: u8 = 0b0000_0010;
    /// Error flag: error (1) or ok (0).
    pub const IS_ERROR: u8 = 0b0000_0100;
    /// Stream flag: stream chunk (1) or single message (0).
    pub const IS_STREAM: u8 = 0b0000_1000;
    /// Stream end flag: final chunk (1) or more coming (0).
    pub const STREAM_END: u8 = 0b0001_0000;
    /// ACK flag: acknowledgment only (1) or full response (0).
    pub const IS_ACK: u8 = 0b0010_0000;

    /// Reserved bits mask (bits 6-7).
    pub const RESERVED_MASK: u8 = 0b1100_0000;

    /// Check if a specific flag is set.
    #[inline]
    pub fn has_flag(flags: u8, flag: u8) -> bool {
        flags & flag != 0
    }

    // Common flag combinations for responses

    /// Response flags: to_parent + is_response = 0x03
    pub const RESPONSE: u8 = DIRECTION_TO_PARENT | IS_RESPONSE;
    /// Error response flags: to_parent + is_response + is_error = 0x07
    pub const ERROR_RESPONSE: u8 = DIRECTION_TO_PARENT | IS_RESPONSE | IS_ERROR;
    /// Stream chunk flags: to_parent + is_response + is_stream = 0x0B
    pub const STREAM_CHUNK: u8 = DIRECTION_TO_PARENT | IS_RESPONSE | IS_STREAM;
    /// Stream end flags: to_parent + is_response + is_stream + stream_end = 0x1B
    pub const STREAM_END_RESPONSE: u8 = DIRECTION_TO_PARENT | IS_RESPONSE | IS_STREAM | STREAM_END;
    /// ACK response flags: to_parent + is_response + is_ack = 0x23
    pub const ACK_RESPONSE: u8 = DIRECTION_TO_PARENT | IS_RESPONSE | IS_ACK;
}

/// Decoded header from wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Method identifier (1-65534, 0 reserved, 0xFFFF = abort).
    pub method_id: u16,
    /// Flags byte (see `flags` module).
    pub flags: u8,
    /// Request identifier (0 = event/fire-and-forget).
    pub request_id: u32,
    /// Payload length in bytes.
    pub payload_length: u32,
}

impl Header {
    /// Create a new header.
    pub fn new(method_id: u16, flags: u8, request_id: u32, payload_length: u32) -> Self {
        Self {
            method_id,
            flags,
            request_id,
            payload_length,
        }
    }

    /// Encode header to bytes (Big Endian).
    ///
    /// # Example
    ///
    /// ```
    /// use procwire_client::protocol::{Header, flags};
    ///
    /// let header = Header::new(1, flags::RESPONSE, 42, 100);
    /// let bytes = header.encode();
    /// assert_eq!(bytes.len(), 11);
    /// ```
    pub fn encode(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        self.encode_into(&mut buf);
        buf
    }

    /// Encode header into an existing buffer.
    ///
    /// # Panics
    ///
    /// Panics if buffer is smaller than `HEADER_SIZE` (11 bytes).
    pub fn encode_into(&self, buf: &mut [u8]) {
        debug_assert!(buf.len() >= HEADER_SIZE);
        buf[0..2].copy_from_slice(&self.method_id.to_be_bytes());
        buf[2] = self.flags;
        buf[3..7].copy_from_slice(&self.request_id.to_be_bytes());
        buf[7..11].copy_from_slice(&self.payload_length.to_be_bytes());
    }

    /// Decode header from bytes (Big Endian).
    ///
    /// Returns `None` if buffer is too short.
    ///
    /// # Example
    ///
    /// ```
    /// use procwire_client::protocol::Header;
    ///
    /// let bytes = [0, 1, 0x03, 0, 0, 0, 42, 0, 0, 0, 100];
    /// let header = Header::decode(&bytes).unwrap();
    /// assert_eq!(header.method_id, 1);
    /// assert_eq!(header.request_id, 42);
    /// assert_eq!(header.payload_length, 100);
    /// ```
    pub fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < HEADER_SIZE {
            return None;
        }
        Some(Self {
            method_id: u16::from_be_bytes([buf[0], buf[1]]),
            flags: buf[2],
            request_id: u32::from_be_bytes([buf[3], buf[4], buf[5], buf[6]]),
            payload_length: u32::from_be_bytes([buf[7], buf[8], buf[9], buf[10]]),
        })
    }

    /// Validate the header for protocol compliance.
    ///
    /// Checks:
    /// - Method ID is not 0 (reserved)
    /// - Payload length doesn't exceed max
    /// - Reserved flag bits are 0
    pub fn validate(&self, max_payload_size: u32) -> Result<()> {
        if self.method_id == RESERVED_METHOD_ID {
            return Err(ProcwireError::Protocol(
                "Method ID 0 is reserved".to_string(),
            ));
        }

        if self.payload_length > max_payload_size {
            return Err(ProcwireError::Protocol(format!(
                "Payload size {} exceeds maximum {}",
                self.payload_length, max_payload_size
            )));
        }

        if self.flags & flags::RESERVED_MASK != 0 {
            return Err(ProcwireError::Protocol(
                "Reserved flag bits must be 0".to_string(),
            ));
        }

        Ok(())
    }

    /// Check if this is an abort signal.
    #[inline]
    pub fn is_abort(&self) -> bool {
        self.method_id == ABORT_METHOD_ID
    }

    /// Check if this is a response.
    #[inline]
    pub fn is_response(&self) -> bool {
        flags::has_flag(self.flags, flags::IS_RESPONSE)
    }

    /// Check if this is an error response.
    #[inline]
    pub fn is_error(&self) -> bool {
        flags::has_flag(self.flags, flags::IS_ERROR)
    }

    /// Check if this is a stream chunk.
    #[inline]
    pub fn is_stream(&self) -> bool {
        flags::has_flag(self.flags, flags::IS_STREAM)
    }

    /// Check if this is the final stream chunk.
    #[inline]
    pub fn is_stream_end(&self) -> bool {
        flags::has_flag(self.flags, flags::STREAM_END)
    }

    /// Check if this is an ACK.
    #[inline]
    pub fn is_ack(&self) -> bool {
        flags::has_flag(self.flags, flags::IS_ACK)
    }

    /// Check if direction is to parent.
    #[inline]
    pub fn is_to_parent(&self) -> bool {
        flags::has_flag(self.flags, flags::DIRECTION_TO_PARENT)
    }

    /// Check if this is an event (request_id == 0).
    #[inline]
    pub fn is_event(&self) -> bool {
        self.request_id == 0
    }
}

/// Encode a header to bytes (standalone function).
#[inline]
pub fn encode_header(header: &Header) -> [u8; HEADER_SIZE] {
    header.encode()
}

/// Encode a header into an existing buffer (standalone function).
#[inline]
pub fn encode_header_into(buf: &mut [u8], header: &Header) {
    header.encode_into(buf);
}

/// Decode a header from bytes (standalone function).
#[inline]
pub fn decode_header(buf: &[u8]) -> Option<Header> {
    Header::decode(buf)
}

/// Validate a header for protocol compliance (standalone function).
#[inline]
pub fn validate_header(header: &Header, max_payload_size: u32) -> Result<()> {
    header.validate(max_payload_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_encode_decode_roundtrip() {
        let original = Header::new(1, flags::RESPONSE, 42, 100);
        let encoded = original.encode();
        let decoded = Header::decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_header_big_endian_byte_order() {
        let header = Header::new(0x0102, 0x03, 0x04050607, 0x08090A0B);
        let bytes = header.encode();

        // Method ID: 0x0102 in BE
        assert_eq!(bytes[0], 0x01);
        assert_eq!(bytes[1], 0x02);

        // Flags: 0x03
        assert_eq!(bytes[2], 0x03);

        // Request ID: 0x04050607 in BE
        assert_eq!(bytes[3], 0x04);
        assert_eq!(bytes[4], 0x05);
        assert_eq!(bytes[5], 0x06);
        assert_eq!(bytes[6], 0x07);

        // Payload length: 0x08090A0B in BE
        assert_eq!(bytes[7], 0x08);
        assert_eq!(bytes[8], 0x09);
        assert_eq!(bytes[9], 0x0A);
        assert_eq!(bytes[10], 0x0B);
    }

    #[test]
    fn test_header_size_is_exactly_11() {
        assert_eq!(HEADER_SIZE, 11);
        let header = Header::new(1, 0, 1, 0);
        assert_eq!(header.encode().len(), 11);
    }

    #[test]
    fn test_decode_too_short_buffer() {
        let buf = [0u8; 10]; // One byte short
        assert!(Header::decode(&buf).is_none());
    }

    #[test]
    fn test_validate_method_id_zero_rejected() {
        let header = Header::new(0, 0, 1, 0);
        let result = header.validate(DEFAULT_MAX_PAYLOAD_SIZE);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Method ID 0 is reserved"));
    }

    #[test]
    fn test_validate_method_id_abort_allowed() {
        let header = Header::new(ABORT_METHOD_ID, 0, 0, 0);
        let result = header.validate(DEFAULT_MAX_PAYLOAD_SIZE);
        assert!(result.is_ok());
        assert!(header.is_abort());
    }

    #[test]
    fn test_validate_payload_too_large() {
        let header = Header::new(1, 0, 1, 1_000_000);
        let result = header.validate(100); // Max 100 bytes
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    #[test]
    fn test_validate_reserved_bits_must_be_zero() {
        let header = Header::new(1, 0b1000_0000, 1, 0); // Bit 7 set
        let result = header.validate(DEFAULT_MAX_PAYLOAD_SIZE);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Reserved flag bits"));
    }

    #[test]
    fn test_flags_has_flag() {
        assert!(flags::has_flag(flags::RESPONSE, flags::DIRECTION_TO_PARENT));
        assert!(flags::has_flag(flags::RESPONSE, flags::IS_RESPONSE));
        assert!(!flags::has_flag(flags::RESPONSE, flags::IS_ERROR));
    }

    #[test]
    fn test_flag_combinations() {
        // Response: 0x03 = to_parent + is_response
        assert_eq!(flags::RESPONSE, 0x03);

        // Error response: 0x07 = to_parent + is_response + is_error
        assert_eq!(flags::ERROR_RESPONSE, 0x07);

        // Stream chunk: 0x0B = to_parent + is_response + is_stream
        assert_eq!(flags::STREAM_CHUNK, 0x0B);

        // Stream end: 0x1B = to_parent + is_response + is_stream + stream_end
        assert_eq!(flags::STREAM_END_RESPONSE, 0x1B);

        // ACK: 0x23 = to_parent + is_response + is_ack
        assert_eq!(flags::ACK_RESPONSE, 0x23);
    }

    #[test]
    fn test_header_accessors() {
        let header = Header::new(1, flags::STREAM_END_RESPONSE, 0, 0);

        assert!(header.is_response());
        assert!(header.is_stream());
        assert!(header.is_stream_end());
        assert!(header.is_to_parent());
        assert!(header.is_event()); // request_id == 0
        assert!(!header.is_error());
        assert!(!header.is_ack());
        assert!(!header.is_abort());
    }

    #[test]
    fn test_min_max_values() {
        // Minimum valid method ID
        let min_header = Header::new(1, 0, 0, 0);
        assert!(min_header.validate(DEFAULT_MAX_PAYLOAD_SIZE).is_ok());

        // Maximum method ID before abort
        let max_header = Header::new(0xFFFE, 0, u32::MAX, u32::MAX);
        assert!(max_header.validate(u32::MAX).is_ok());
    }

    #[test]
    fn test_encode_into() {
        let header = Header::new(1, flags::RESPONSE, 42, 100);
        let mut buf = [0u8; HEADER_SIZE];
        header.encode_into(&mut buf);

        let decoded = Header::decode(&buf).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn test_standalone_functions() {
        let header = Header::new(1, 0, 1, 0);

        let encoded = encode_header(&header);
        let decoded = decode_header(&encoded).unwrap();
        assert_eq!(header, decoded);

        let mut buf = [0u8; HEADER_SIZE];
        encode_header_into(&mut buf, &header);
        assert_eq!(buf, encoded);

        assert!(validate_header(&header, DEFAULT_MAX_PAYLOAD_SIZE).is_ok());
    }
}
