//! Integration tests for procwire-client.
//!
//! These tests verify the integration between different modules.

use procwire_client::codec::MsgPackCodec;
use procwire_client::protocol::{build_frame, flags, FrameBuffer, Header, HEADER_SIZE};

/// Test full frame encode/decode cycle with MsgPack payload.
#[test]
fn test_frame_with_msgpack_payload() {
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct TestPayload {
        id: i32,
        message: String,
    }

    let payload = TestPayload {
        id: 42,
        message: "Hello, world!".to_string(),
    };

    // Encode payload with MsgPack
    let payload_bytes = MsgPackCodec::encode(&payload).unwrap();

    // Build frame
    let header = Header::new(
        1,                          // method_id
        flags::RESPONSE,            // flags
        12345,                      // request_id
        payload_bytes.len() as u32, // payload_length
    );
    let frame_bytes = build_frame(&header, &payload_bytes);

    // Parse frame
    let mut buffer = FrameBuffer::new();
    let frames = buffer.push(&frame_bytes).unwrap();

    assert_eq!(frames.len(), 1);
    let frame = &frames[0];

    assert_eq!(frame.header.method_id, 1);
    assert_eq!(frame.header.request_id, 12345);
    assert!(frame.is_response());

    // Decode payload
    let decoded: TestPayload = MsgPackCodec::decode(&frame.payload).unwrap();
    assert_eq!(decoded, payload);
}

/// Test multiple frames in sequence.
#[test]
fn test_multiple_frames_sequence() {
    let mut buffer = FrameBuffer::new();
    let mut all_bytes = Vec::new();

    // Create 5 frames with different payloads
    for i in 1u32..=5 {
        let payload = MsgPackCodec::encode(&format!("message_{}", i)).unwrap();
        let header = Header::new(i as u16, flags::RESPONSE, i * 100, payload.len() as u32);
        all_bytes.extend(build_frame(&header, &payload));
    }

    // Parse all at once
    let frames = buffer.push(&all_bytes).unwrap();
    assert_eq!(frames.len(), 5);

    for (i, frame) in frames.iter().enumerate() {
        let expected_method_id = (i + 1) as u16;
        let expected_request_id = ((i + 1) * 100) as u32;

        assert_eq!(frame.header.method_id, expected_method_id);
        assert_eq!(frame.header.request_id, expected_request_id);

        let decoded: String = MsgPackCodec::decode(&frame.payload).unwrap();
        assert_eq!(decoded, format!("message_{}", i + 1));
    }
}

/// Test stream response pattern (multiple chunks + end).
#[test]
fn test_stream_pattern() {
    let mut buffer = FrameBuffer::new();
    let request_id = 999u32;

    // Simulate stream: 3 chunks + end
    let mut all_bytes = Vec::new();

    // Chunk 1
    let payload1 = MsgPackCodec::encode(&1i32).unwrap();
    let header1 = Header::new(1, flags::STREAM_CHUNK, request_id, payload1.len() as u32);
    all_bytes.extend(build_frame(&header1, &payload1));

    // Chunk 2
    let payload2 = MsgPackCodec::encode(&2i32).unwrap();
    let header2 = Header::new(1, flags::STREAM_CHUNK, request_id, payload2.len() as u32);
    all_bytes.extend(build_frame(&header2, &payload2));

    // Chunk 3
    let payload3 = MsgPackCodec::encode(&3i32).unwrap();
    let header3 = Header::new(1, flags::STREAM_CHUNK, request_id, payload3.len() as u32);
    all_bytes.extend(build_frame(&header3, &payload3));

    // End (empty payload)
    let header_end = Header::new(1, flags::STREAM_END_RESPONSE, request_id, 0);
    all_bytes.extend(build_frame(&header_end, &[]));

    // Parse
    let frames = buffer.push(&all_bytes).unwrap();
    assert_eq!(frames.len(), 4);

    // Verify chunks
    for i in 0..3 {
        assert!(frames[i].is_stream());
        assert!(!frames[i].is_stream_end());
        let value: i32 = MsgPackCodec::decode(&frames[i].payload).unwrap();
        assert_eq!(value, (i + 1) as i32);
    }

    // Verify end
    assert!(frames[3].is_stream());
    assert!(frames[3].is_stream_end());
    assert!(
        frames[3].payload.is_empty(),
        "STREAM_END must have empty payload"
    );
}

/// Test error response pattern.
#[test]
fn test_error_response() {
    let error_message = "Something went wrong";
    let payload = MsgPackCodec::encode(&error_message).unwrap();

    let header = Header::new(1, flags::ERROR_RESPONSE, 123, payload.len() as u32);
    let frame_bytes = build_frame(&header, &payload);

    let mut buffer = FrameBuffer::new();
    let frames = buffer.push(&frame_bytes).unwrap();

    assert_eq!(frames.len(), 1);
    let frame = &frames[0];

    assert!(frame.is_error());
    assert!(frame.is_response());

    let decoded: String = MsgPackCodec::decode(&frame.payload).unwrap();
    assert_eq!(decoded, error_message);
}

/// Test ACK response pattern.
#[test]
fn test_ack_response() {
    let header = Header::new(1, flags::ACK_RESPONSE, 456, 0);
    let frame_bytes = build_frame(&header, &[]);

    let mut buffer = FrameBuffer::new();
    let frames = buffer.push(&frame_bytes).unwrap();

    assert_eq!(frames.len(), 1);
    let frame = &frames[0];

    assert!(frame.is_ack());
    assert!(frame.is_response());
    assert!(frame.payload.is_empty());
}

/// Test event pattern (to parent, request_id = 0).
#[test]
fn test_event_to_parent() {
    let event_data = MsgPackCodec::encode(&"progress update").unwrap();
    let header = Header::new(5, flags::DIRECTION_TO_PARENT, 0, event_data.len() as u32);
    let frame_bytes = build_frame(&header, &event_data);

    let mut buffer = FrameBuffer::new();
    let frames = buffer.push(&frame_bytes).unwrap();

    assert_eq!(frames.len(), 1);
    let frame = &frames[0];

    assert!(frame.is_event());
    assert!(!frame.is_response());
    assert_eq!(frame.header.request_id, 0);

    let decoded: String = MsgPackCodec::decode(&frame.payload).unwrap();
    assert_eq!(decoded, "progress update");
}

/// Test fragmented frame parsing.
#[test]
fn test_fragmented_frame_parsing() {
    let payload = MsgPackCodec::encode(&"test data for fragmentation").unwrap();
    let header = Header::new(1, flags::RESPONSE, 789, payload.len() as u32);
    let frame_bytes = build_frame(&header, &payload);

    let mut buffer = FrameBuffer::new();

    // Push header in parts
    let frames1 = buffer.push(&frame_bytes[..5]).unwrap();
    assert!(frames1.is_empty());

    let frames2 = buffer.push(&frame_bytes[5..HEADER_SIZE]).unwrap();
    assert!(frames2.is_empty());

    // Push payload in parts
    let mid = HEADER_SIZE + payload.len() / 2;
    let frames3 = buffer.push(&frame_bytes[HEADER_SIZE..mid]).unwrap();
    assert!(frames3.is_empty());

    // Final part completes the frame
    let frames4 = buffer.push(&frame_bytes[mid..]).unwrap();
    assert_eq!(frames4.len(), 1);

    let decoded: String = MsgPackCodec::decode(&frames4[0].payload).unwrap();
    assert_eq!(decoded, "test data for fragmentation");
}
