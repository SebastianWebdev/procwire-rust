//! Frame buffer for accumulating partial reads.
//!
//! Uses `bytes::BytesMut` for zero-copy buffer management.
//! Implements a state machine for handling fragmented frames:
//! - `WaitingForHeader`: Need at least 11 bytes
//! - `WaitingForPayload`: Header parsed, need N more payload bytes
//!
//! # Example
//!
//! ```ignore
//! use procwire_client::protocol::FrameBuffer;
//!
//! let mut buffer = FrameBuffer::new();
//!
//! // Data arrives in chunks from socket
//! let chunk = vec![0u8; 100];
//! let frames = buffer.push(&chunk).unwrap();
//!
//! for frame in frames {
//!     println!("Got frame with method_id: {}", frame.method_id());
//! }
//! ```

use bytes::{Bytes, BytesMut};

use super::wire_format::{Header, DEFAULT_MAX_PAYLOAD_SIZE, HEADER_SIZE};
use super::Frame;
use crate::error::{ProcwireError, Result};

/// State machine for frame parsing.
#[derive(Debug, Clone)]
enum State {
    /// Waiting for complete header (need 11 bytes).
    WaitingForHeader,
    /// Header parsed, waiting for payload bytes.
    WaitingForPayload { header: Header, remaining: u32 },
}

/// Buffer for accumulating incoming bytes and extracting complete frames.
///
/// Uses a state machine to handle partial reads efficiently.
/// All data is stored in a single `BytesMut` buffer to minimize allocations.
pub struct FrameBuffer {
    /// Accumulated bytes from socket reads.
    buffer: BytesMut,
    /// Current parsing state.
    state: State,
    /// Maximum allowed payload size.
    max_payload_size: u32,
}

impl FrameBuffer {
    /// Create a new frame buffer with default settings.
    ///
    /// Default capacity: 64KB, max payload: 1GB.
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::with_capacity(64 * 1024),
            state: State::WaitingForHeader,
            max_payload_size: DEFAULT_MAX_PAYLOAD_SIZE,
        }
    }

    /// Create a new frame buffer with custom max payload size.
    pub fn with_max_payload(max_payload_size: u32) -> Self {
        Self {
            buffer: BytesMut::with_capacity(64 * 1024),
            state: State::WaitingForHeader,
            max_payload_size,
        }
    }

    /// Create a new frame buffer with custom capacity and max payload.
    pub fn with_capacity_and_max_payload(capacity: usize, max_payload_size: u32) -> Self {
        Self {
            buffer: BytesMut::with_capacity(capacity),
            state: State::WaitingForHeader,
            max_payload_size,
        }
    }

    /// Push data into the buffer and extract all complete frames.
    ///
    /// This is the main API for processing incoming data from the socket.
    /// Returns a vector of complete frames. If data is fragmented,
    /// partial data is buffered internally for the next push.
    ///
    /// # Arguments
    ///
    /// * `data` - Raw bytes from socket read
    ///
    /// # Returns
    ///
    /// Vector of complete frames (may be empty if still waiting for data).
    ///
    /// # Errors
    ///
    /// Returns error if payload exceeds max_payload_size.
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Frame>> {
        // Single allocation to add data to buffer
        self.buffer.extend_from_slice(data);

        let mut frames = Vec::new();

        // Process as many complete frames as possible
        while let Some(frame) = self.try_extract_one()? {
            frames.push(frame);
        }

        Ok(frames)
    }

    /// Try to extract a single frame from the buffer.
    ///
    /// Returns:
    /// - `Ok(Some(frame))` if a complete frame was extracted
    /// - `Ok(None)` if more data is needed
    /// - `Err(...)` if protocol violation (e.g., payload too large)
    fn try_extract_one(&mut self) -> Result<Option<Frame>> {
        match &self.state {
            State::WaitingForHeader => {
                if self.buffer.len() < HEADER_SIZE {
                    return Ok(None);
                }

                // Parse header (peek, don't consume yet)
                let header =
                    Header::decode(&self.buffer[..HEADER_SIZE]).expect("Buffer has enough bytes");

                // Validate payload size
                if header.payload_length > self.max_payload_size {
                    return Err(ProcwireError::Protocol(format!(
                        "Payload size {} exceeds maximum {}",
                        header.payload_length, self.max_payload_size
                    )));
                }

                // Consume header bytes
                let _ = self.buffer.split_to(HEADER_SIZE);

                if header.payload_length == 0 {
                    // Empty payload, frame is complete
                    return Ok(Some(Frame::new(header, Bytes::new())));
                }

                // Transition to waiting for payload
                self.state = State::WaitingForPayload {
                    header,
                    remaining: header.payload_length,
                };

                // Try to get payload immediately
                self.try_extract_one()
            }

            State::WaitingForPayload { header, remaining } => {
                let remaining = *remaining as usize;

                if self.buffer.len() < remaining {
                    return Ok(None);
                }

                // Extract payload (zero-copy freeze)
                let payload = self.buffer.split_to(remaining).freeze();
                let header = *header;

                // Reset state for next frame
                self.state = State::WaitingForHeader;

                Ok(Some(Frame::new(header, payload)))
            }
        }
    }

    /// Legacy method - try to extract a single frame.
    ///
    /// Prefer using `push()` which handles multiple frames efficiently.
    #[deprecated(note = "Use push() instead for proper multi-frame handling")]
    pub fn try_extract(&mut self) -> Option<Frame> {
        self.try_extract_one().ok().flatten()
    }

    /// Append data to the buffer without extracting frames.
    ///
    /// Prefer using `push()` which does extend + extract in one call.
    pub fn extend(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Get the number of buffered bytes.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Clear the buffer and reset state.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.state = State::WaitingForHeader;
    }

    /// Get the current state for debugging.
    #[cfg(test)]
    fn state_name(&self) -> &'static str {
        match &self.state {
            State::WaitingForHeader => "WaitingForHeader",
            State::WaitingForPayload { .. } => "WaitingForPayload",
        }
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::flags;

    /// Helper to create a valid frame as bytes.
    fn make_frame_bytes(method_id: u16, flags: u8, request_id: u32, payload: &[u8]) -> Vec<u8> {
        let header = Header::new(method_id, flags, request_id, payload.len() as u32);
        let mut bytes = header.encode().to_vec();
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn test_single_complete_frame() {
        let mut buffer = FrameBuffer::new();
        let frame_bytes = make_frame_bytes(1, flags::RESPONSE, 42, b"hello");

        let frames = buffer.push(&frame_bytes).unwrap();

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].method_id(), 1);
        assert_eq!(frames[0].request_id(), 42);
        assert_eq!(&frames[0].payload[..], b"hello");
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_multiple_frames_in_one_push() {
        let mut buffer = FrameBuffer::new();

        let frame1 = make_frame_bytes(1, 0, 1, b"first");
        let frame2 = make_frame_bytes(2, 0, 2, b"second");
        let frame3 = make_frame_bytes(3, 0, 3, b"third");

        let mut combined = Vec::new();
        combined.extend_from_slice(&frame1);
        combined.extend_from_slice(&frame2);
        combined.extend_from_slice(&frame3);

        let frames = buffer.push(&combined).unwrap();

        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].method_id(), 1);
        assert_eq!(frames[1].method_id(), 2);
        assert_eq!(frames[2].method_id(), 3);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_fragmented_header() {
        let mut buffer = FrameBuffer::new();
        let frame_bytes = make_frame_bytes(1, 0, 42, b"test");

        // Push first 5 bytes of header
        let frames = buffer.push(&frame_bytes[..5]).unwrap();
        assert!(frames.is_empty());
        assert_eq!(buffer.state_name(), "WaitingForHeader");

        // Push rest of header and payload
        let frames = buffer.push(&frame_bytes[5..]).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].method_id(), 1);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_fragmented_payload() {
        let mut buffer = FrameBuffer::new();
        let payload = b"this is a longer payload that will be fragmented";
        let frame_bytes = make_frame_bytes(1, 0, 42, payload);

        // Push header + partial payload
        let partial_len = HEADER_SIZE + 10;
        let frames = buffer.push(&frame_bytes[..partial_len]).unwrap();
        assert!(frames.is_empty());
        assert_eq!(buffer.state_name(), "WaitingForPayload");

        // Push rest of payload
        let frames = buffer.push(&frame_bytes[partial_len..]).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(&frames[0].payload[..], payload);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_empty_payload() {
        let mut buffer = FrameBuffer::new();
        let frame_bytes = make_frame_bytes(1, 0, 42, b"");

        let frames = buffer.push(&frame_bytes).unwrap();

        assert_eq!(frames.len(), 1);
        assert!(frames[0].payload.is_empty());
        assert_eq!(frames[0].header.payload_length, 0);
    }

    #[test]
    fn test_large_payload() {
        let mut buffer = FrameBuffer::new();
        let payload = vec![0xAB; 1024 * 1024]; // 1MB
        let frame_bytes = make_frame_bytes(1, 0, 42, &payload);

        let frames = buffer.push(&frame_bytes).unwrap();

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload.len(), 1024 * 1024);
        assert!(frames[0].payload.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn test_max_payload_validation() {
        let mut buffer = FrameBuffer::with_max_payload(100);

        // Create header claiming 1000 byte payload
        let header = Header::new(1, 0, 42, 1000);
        let header_bytes = header.encode();

        let result = buffer.push(&header_bytes);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    #[test]
    fn test_frame_with_all_header_fields() {
        let mut buffer = FrameBuffer::new();
        let frame_bytes = make_frame_bytes(0x1234, flags::STREAM_END_RESPONSE, 0xDEADBEEF, b"data");

        let frames = buffer.push(&frame_bytes).unwrap();

        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(frame.method_id(), 0x1234);
        assert_eq!(frame.header.flags, flags::STREAM_END_RESPONSE);
        assert_eq!(frame.request_id(), 0xDEADBEEF);
        assert!(frame.is_stream());
        assert!(frame.is_stream_end());
    }

    #[test]
    fn test_clear_resets_state() {
        let mut buffer = FrameBuffer::new();

        // Push partial header (not complete)
        let frame_bytes = make_frame_bytes(1, 0, 42, b"test");
        buffer.push(&frame_bytes[..5]).unwrap(); // Only 5 bytes of header

        assert_eq!(buffer.state_name(), "WaitingForHeader");
        assert!(!buffer.is_empty());
        assert_eq!(buffer.len(), 5);

        // Push rest of header to transition to WaitingForPayload
        buffer.push(&frame_bytes[5..HEADER_SIZE]).unwrap();
        assert_eq!(buffer.state_name(), "WaitingForPayload");

        buffer.clear();

        assert_eq!(buffer.state_name(), "WaitingForHeader");
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_mixed_complete_and_partial() {
        let mut buffer = FrameBuffer::new();

        let frame1 = make_frame_bytes(1, 0, 1, b"first");
        let frame2 = make_frame_bytes(2, 0, 2, b"second");

        // Push first complete frame + partial second
        let mut data = frame1.clone();
        data.extend_from_slice(&frame2[..5]);

        let frames = buffer.push(&data).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].method_id(), 1);
        assert_eq!(buffer.state_name(), "WaitingForHeader");

        // Complete second frame
        let frames = buffer.push(&frame2[5..]).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].method_id(), 2);
    }

    #[test]
    fn test_byte_at_a_time() {
        let mut buffer = FrameBuffer::new();
        let frame_bytes = make_frame_bytes(1, 0, 42, b"hi");

        let mut all_frames = Vec::new();

        for byte in &frame_bytes {
            let frames = buffer.push(&[*byte]).unwrap();
            all_frames.extend(frames);
        }

        assert_eq!(all_frames.len(), 1);
        assert_eq!(all_frames[0].method_id(), 1);
        assert_eq!(&all_frames[0].payload[..], b"hi");
    }
}
