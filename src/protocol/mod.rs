//! Protocol module - wire format, framing, and frame types.
//!
//! This module implements the binary protocol for the data plane:
//! - 11-byte header encoding/decoding
//! - Frame buffer for accumulating partial reads
//! - Frame struct with typed accessors

mod frame;
mod frame_buffer;
mod wire_format;

pub use frame::{build_frame, build_frame_parts, Frame};
pub use frame_buffer::FrameBuffer;
pub use wire_format::{
    decode_header, encode_header, encode_header_into, flags, validate_header, Header,
    ABORT_METHOD_ID, ABSOLUTE_MAX_PAYLOAD_SIZE, DEFAULT_MAX_PAYLOAD_SIZE, HEADER_POOL_SIZE,
    HEADER_SIZE, RESERVED_METHOD_ID,
};
