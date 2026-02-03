# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-02-03

### Added

- Initial release of `procwire-client` Rust SDK
- **Protocol Implementation**
  - 11-byte binary header format (Big Endian)
  - Full flag support: direction, response, error, stream, stream_end, ack
  - Method ID and Request ID handling
  - Reserved IDs: 0 (reserved), 0xFFFF (ABORT signal)
- **Codec System**
  - `MsgPackCodec` - MsgPack serialization using `rmp-serde` with `to_vec_named`
  - `RawCodec` - Pass-through for raw bytes (zero-copy)
- **Transport Layer**
  - Unix Domain Socket support (Linux, macOS)
  - Named Pipe support (Windows)
  - Platform-specific pipe path generation
- **Handler System**
  - Type-safe handler registration with generic request/response types
  - `RequestContext` for responding to requests
  - Response patterns: `respond()`, `ack()`, `chunk()`, `end()`, `error()`
  - Concurrent handler limit with `Semaphore`
- **Cancellation Support**
  - ABORT signal handling (method_id=0xFFFF)
  - `CancellationToken` integration
  - `RequestContext::is_cancelled()`, `cancelled()`, `cancellation_token()`
- **Performance Optimizations**
  - Writer Task pattern (eliminates Mutex bottleneck)
  - `write_vectored` / scatter-gather I/O for batching
  - Backpressure handling with configurable timeout
  - Zero-copy frame buffer using `bytes::BytesMut`
- **Client Builder**
  - Fluent API for configuration
  - Method and event handler registration
  - Configurable concurrent handler limit
  - Configurable backpressure timeout
- **Control Plane**
  - `$init` message builder with JSON-RPC format
  - Schema serialization with method/event IDs

### Technical Details

- Wire format: `[methodId:2][flags:1][requestId:4][payloadLength:4][payload:N]`
- All multi-byte integers in Big Endian
- MsgPack uses struct-as-map format for Node.js compatibility
- STREAM_END frames always have empty payload

[0.1.0]: https://github.com/SebastianWebdev/procwire-client-rs/releases/tag/v0.1.0
