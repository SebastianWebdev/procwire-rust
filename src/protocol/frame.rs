//! Frame struct with typed accessors.
//!
//! Represents a complete protocol frame with header and payload.
//! Uses `bytes::Bytes` for zero-copy payload sharing.
//!
//! # Example
//!
//! ```
//! use procwire_client::protocol::{Frame, Header, flags};
//! use bytes::Bytes;
//!
//! let header = Header::new(1, flags::RESPONSE, 42, 5);
//! let payload = Bytes::from_static(b"hello");
//! let frame = Frame::new(header, payload);
//!
//! assert_eq!(frame.method_id(), 1);
//! assert_eq!(frame.payload(), b"hello");
//! ```

use bytes::Bytes;

use super::wire_format::{flags, Header, HEADER_SIZE};

/// A complete protocol frame.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Decoded header.
    pub header: Header,
    /// Payload bytes (zero-copy via `bytes::Bytes`).
    pub payload: Bytes,
}

impl Frame {
    /// Create a new frame from header and payload.
    pub fn new(header: Header, payload: Bytes) -> Self {
        Self { header, payload }
    }

    /// Create a frame from header and raw bytes (copies data).
    pub fn from_parts(header: Header, payload: &[u8]) -> Self {
        Self {
            header,
            payload: Bytes::copy_from_slice(payload),
        }
    }

    /// Get a reference to the payload bytes.
    #[inline]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Get a clone of the payload as Bytes (cheap, zero-copy).
    #[inline]
    pub fn payload_bytes(&self) -> Bytes {
        self.payload.clone()
    }

    /// Get the payload length.
    #[inline]
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }

    /// Get the method ID.
    #[inline]
    pub fn method_id(&self) -> u16 {
        self.header.method_id
    }

    /// Get the flags byte.
    #[inline]
    pub fn flags(&self) -> u8 {
        self.header.flags
    }

    /// Get the request ID.
    #[inline]
    pub fn request_id(&self) -> u32 {
        self.header.request_id
    }

    /// Check if this is a response.
    #[inline]
    pub fn is_response(&self) -> bool {
        flags::has_flag(self.header.flags, flags::IS_RESPONSE)
    }

    /// Check if this is an error response.
    #[inline]
    pub fn is_error(&self) -> bool {
        flags::has_flag(self.header.flags, flags::IS_ERROR)
    }

    /// Check if this is a stream chunk.
    #[inline]
    pub fn is_stream(&self) -> bool {
        flags::has_flag(self.header.flags, flags::IS_STREAM)
    }

    /// Check if this is the final stream chunk.
    #[inline]
    pub fn is_stream_end(&self) -> bool {
        flags::has_flag(self.header.flags, flags::STREAM_END)
    }

    /// Check if this is an ACK.
    #[inline]
    pub fn is_ack(&self) -> bool {
        flags::has_flag(self.header.flags, flags::IS_ACK)
    }

    /// Check if direction is to parent.
    #[inline]
    pub fn is_to_parent(&self) -> bool {
        flags::has_flag(self.header.flags, flags::DIRECTION_TO_PARENT)
    }

    /// Check if this is an event (request_id == 0, not a response).
    #[inline]
    pub fn is_event(&self) -> bool {
        self.header.request_id == 0 && !self.is_response()
    }

    /// Check if this is an abort signal.
    #[inline]
    pub fn is_abort(&self) -> bool {
        self.header.is_abort()
    }
}

/// Build a complete frame as a single byte vector.
///
/// Encodes header and appends payload into a contiguous buffer.
/// Use `build_frame_parts` for scatter/gather I/O (writev).
///
/// # Example
///
/// ```
/// use procwire_client::protocol::{build_frame, Header, flags};
///
/// let header = Header::new(1, flags::RESPONSE, 42, 5);
/// let bytes = build_frame(&header, b"hello");
/// assert_eq!(bytes.len(), 11 + 5); // header + payload
/// ```
pub fn build_frame(header: &Header, payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_SIZE + payload.len());
    buf.extend_from_slice(&header.encode());
    buf.extend_from_slice(payload);
    buf
}

/// Build frame parts for scatter/gather I/O.
///
/// Returns the encoded header and a reference to the payload.
/// This avoids copying for writev-style operations.
///
/// # Example
///
/// ```
/// use procwire_client::protocol::{build_frame_parts, Header, flags, HEADER_SIZE};
///
/// let header = Header::new(1, flags::RESPONSE, 42, 5);
/// let payload = b"hello";
/// let (header_bytes, payload_ref) = build_frame_parts(&header, payload);
///
/// assert_eq!(header_bytes.len(), HEADER_SIZE);
/// assert_eq!(payload_ref, b"hello");
/// ```
pub fn build_frame_parts<'a>(header: &Header, payload: &'a [u8]) -> ([u8; HEADER_SIZE], &'a [u8]) {
    (header.encode(), payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_creation() {
        let header = Header::new(1, flags::RESPONSE, 42, 5);
        let payload = Bytes::from_static(b"hello");
        let frame = Frame::new(header, payload);

        assert_eq!(frame.method_id(), 1);
        assert_eq!(frame.flags(), flags::RESPONSE);
        assert_eq!(frame.request_id(), 42);
        assert_eq!(frame.payload(), b"hello");
        assert_eq!(frame.payload_len(), 5);
    }

    #[test]
    fn test_frame_from_parts() {
        let header = Header::new(2, 0, 100, 4);
        let frame = Frame::from_parts(header, b"test");

        assert_eq!(frame.method_id(), 2);
        assert_eq!(frame.payload(), b"test");
    }

    #[test]
    fn test_frame_empty_payload() {
        let header = Header::new(1, 0, 1, 0);
        let frame = Frame::new(header, Bytes::new());

        assert_eq!(frame.payload_len(), 0);
        assert!(frame.payload().is_empty());
    }

    #[test]
    fn test_frame_flag_accessors() {
        // Response frame
        let response_frame = Frame::new(Header::new(1, flags::RESPONSE, 1, 0), Bytes::new());
        assert!(response_frame.is_response());
        assert!(response_frame.is_to_parent());
        assert!(!response_frame.is_error());
        assert!(!response_frame.is_stream());

        // Error frame
        let error_frame = Frame::new(Header::new(1, flags::ERROR_RESPONSE, 1, 0), Bytes::new());
        assert!(error_frame.is_response());
        assert!(error_frame.is_error());

        // Stream frame
        let stream_frame = Frame::new(Header::new(1, flags::STREAM_CHUNK, 1, 0), Bytes::new());
        assert!(stream_frame.is_stream());
        assert!(!stream_frame.is_stream_end());

        // Stream end frame
        let stream_end_frame = Frame::new(
            Header::new(1, flags::STREAM_END_RESPONSE, 1, 0),
            Bytes::new(),
        );
        assert!(stream_end_frame.is_stream());
        assert!(stream_end_frame.is_stream_end());

        // ACK frame
        let ack_frame = Frame::new(Header::new(1, flags::ACK_RESPONSE, 1, 0), Bytes::new());
        assert!(ack_frame.is_ack());
    }

    #[test]
    fn test_frame_is_event() {
        // Event: request_id == 0, not a response
        let event_frame = Frame::new(
            Header::new(1, flags::DIRECTION_TO_PARENT, 0, 0),
            Bytes::new(),
        );
        assert!(event_frame.is_event());

        // Not an event: is a response
        let response_frame = Frame::new(Header::new(1, flags::RESPONSE, 0, 0), Bytes::new());
        assert!(!response_frame.is_event());

        // Not an event: has request_id
        let request_frame = Frame::new(Header::new(1, 0, 42, 0), Bytes::new());
        assert!(!request_frame.is_event());
    }

    #[test]
    fn test_frame_is_abort() {
        use super::super::wire_format::ABORT_METHOD_ID;

        let abort_frame = Frame::new(Header::new(ABORT_METHOD_ID, 0, 0, 0), Bytes::new());
        assert!(abort_frame.is_abort());

        let normal_frame = Frame::new(Header::new(1, 0, 1, 0), Bytes::new());
        assert!(!normal_frame.is_abort());
    }

    #[test]
    fn test_payload_bytes_zero_copy() {
        let original = Bytes::from_static(b"test data");
        let frame = Frame::new(Header::new(1, 0, 1, 9), original.clone());

        // payload_bytes() should return a cheap clone
        let cloned = frame.payload_bytes();
        assert_eq!(cloned, original);

        // Both should point to the same data
        assert_eq!(cloned.as_ptr(), original.as_ptr());
    }

    #[test]
    fn test_build_frame() {
        let header = Header::new(1, flags::RESPONSE, 42, 5);
        let bytes = build_frame(&header, b"hello");

        assert_eq!(bytes.len(), HEADER_SIZE + 5);

        // Parse it back
        let parsed_header = Header::decode(&bytes[..HEADER_SIZE]).unwrap();
        assert_eq!(parsed_header, header);
        assert_eq!(&bytes[HEADER_SIZE..], b"hello");
    }

    #[test]
    fn test_build_frame_empty_payload() {
        let header = Header::new(1, 0, 1, 0);
        let bytes = build_frame(&header, b"");

        assert_eq!(bytes.len(), HEADER_SIZE);
    }

    #[test]
    fn test_build_frame_parts() {
        let header = Header::new(1, flags::RESPONSE, 42, 5);
        let payload = b"hello";
        let (header_bytes, payload_ref) = build_frame_parts(&header, payload);

        assert_eq!(header_bytes.len(), HEADER_SIZE);
        assert_eq!(payload_ref, b"hello");

        // Verify header encoding
        let parsed = Header::decode(&header_bytes).unwrap();
        assert_eq!(parsed, header);
    }

    #[test]
    fn test_build_frame_roundtrip() {
        use super::super::FrameBuffer;

        let header = Header::new(123, flags::STREAM_CHUNK, 456, 10);
        let payload = b"0123456789";
        let bytes = build_frame(&header, payload);

        let mut buffer = FrameBuffer::new();
        let frames = buffer.push(&bytes).unwrap();

        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(frame.method_id(), 123);
        assert_eq!(frame.request_id(), 456);
        assert_eq!(frame.payload(), payload);
        assert!(frame.is_stream());
    }
}
