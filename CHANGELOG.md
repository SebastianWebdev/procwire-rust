# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Aligns the Rust client with the post-audit ("Phase 4") Procwire protocol. See
`docs/rust-client-compatibility.md` in `SebastianWebdev/procwire` for the
authoritative change list.

### Added

- **Control-plane heartbeat**: child now reads newline-delimited JSON-RPC on
  stdin and replies to `$ping` with `$pong`, keeping it alive under
  heartbeat-enabled parents (`spawnPolicy({ heartbeat })`).
- **Graceful `$shutdown`**: child reacts to a `$shutdown` control message and
  exits promptly via `wait_for_shutdown()` instead of waiting for the parent's
  force-kill grace period. New `Client::is_shutdown_requested()`.
- **Configurable incoming frame limit**: `ClientBuilder::max_payload_size()`
  bounds the declared `payloadLength` of incoming frames (clamped to
  `ABSOLUTE_MAX_PAYLOAD_SIZE`); oversized frames tear down the connection with
  no giant allocation.
- **Structured error responses**: `RequestContext::error_with()` sends an
  arbitrary `Serialize` error payload (e.g. `{ "message", "code" }`).
- **Data-plane authentication (opt-in)**: support for the AUTH frame
  (`AUTH_METHOD_ID = 0xFFFE`). When `PROCWIRE_TOKEN` is set (an `auth: true`
  parent sets it) or `ClientBuilder::auth_token()` is used, the first data-plane
  frame must be a matching AUTH frame (payload compared in constant time) before
  the connection is adopted; a missing/mismatched first frame drops the
  connection while the listener keeps waiting for the real parent. Frames
  pipelined after AUTH are dispatched normally. With no token, connections are
  adopted on accept (backward compatible). New `Header::is_auth()`.

### Changed

- **Protocol version string** in `$init` is now `"1.0.0"` (was `"2.0.0"`):
  Procwire dropped the "v2" framing; this matches the Node/Bun reference client.
- **Full receive-side header validation (D6)**: `FrameBuffer` now rejects
  `methodId 0`, reserved flag bits (6–7), and payloads over the configured or
  absolute maximum on every parsed frame — mirroring the TypeScript
  `FrameBuffer` — and tears the connection down instead of dispatching.
- **Hardened Unix socket path**: the data-plane socket now lives under
  `XDG_RUNTIME_DIR`, then `TMPDIR`, then `/tmp` (was always `/tmp`), preferring a
  per-user runtime directory as defense-in-depth.

### Notes

- The control-plane reader runs on a dedicated OS thread so a blocking stdin
  read never keeps the async runtime or process alive.
- The writer task is only spawned after a connection is adopted, so no
  child→parent frame is ever written to an unauthenticated peer.

## [1.0.0] - 2026-02-03

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

[1.0.0]: https://github.com/SebastianWebdev/procwire-client-rs/releases/tag/v0.1.0
