# procwire-client

[![Crates.io](https://img.shields.io/crates/v/procwire-client.svg)](https://crates.io/crates/procwire-client)
[![Documentation](https://docs.rs/procwire-client/badge.svg)](https://docs.rs/procwire-client)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Rust client SDK for the [Procwire](https://github.com/SebastianWebdev/procwire) IPC protocol (wire version `1.0.0`).

This crate enables Rust workers (child processes) to communicate with a Node.js (or Bun) parent process running `@procwire/core` using a high-performance binary protocol. It is wire-compatible with the post-audit ("Phase 4") Node/Bun client.

## Features

- **High Performance**: Binary protocol with MsgPack serialization, achieving >1 GB/s throughput
- **Zero-copy**: Uses `bytes::BytesMut` for efficient buffer management
- **Async/await**: Built on Tokio for non-blocking I/O
- **Cross-platform**: Works on Linux, macOS, and Windows
- **Type-safe**: Strongly typed handlers with Serde integration
- **Streaming**: Support for chunked responses and backpressure
- **Cancellation**: Full abort signal support with `CancellationToken`
- **Liveness**: Answers the control-plane `$ping`/`$pong` heartbeat and shuts down gracefully on `$shutdown`
- **Authentication**: Optional data-plane AUTH handshake (`PROCWIRE_TOKEN`) for `auth: true` parents
- **Hardened framing**: Bounds incoming `payloadLength` and rejects malformed headers before allocating

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
procwire-client = "0.1"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

## Quick Start

```rust
use procwire_client::ClientBuilder;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct EchoRequest {
    message: String,
}

#[derive(Serialize, Deserialize)]
struct EchoResponse {
    message: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ClientBuilder::new()
        .handle("echo", |payload: EchoRequest, ctx| async move {
            ctx.respond(&EchoResponse {
                message: payload.message,
            })
            .await
        })
        .start()
        .await?;

    client.wait_for_shutdown().await?;
    Ok(())
}
```

## Architecture

Procwire uses a dual-channel architecture:

- **Control Plane** (stdio): JSON-RPC for the `$init` handshake
- **Data Plane** (named pipe/Unix socket): Binary protocol for high-throughput communication

```
┌─────────────────┐                    ┌─────────────────┐
│  Node.js Parent │                    │   Rust Worker   │
│ (@procwire/core)│                    │ (this crate)    │
├─────────────────┤                    ├─────────────────┤
│                 │◄── $init (JSON) ───│                 │
│                 │                    │                 │
│                 │◄═══ Binary ═══════►│                 │
│                 │   (Named Pipe)     │                 │
└─────────────────┘                    └─────────────────┘
```

## Wire Format

All data plane messages use an 11-byte binary header:

```
┌──────────┬───────┬──────────┬──────────┬──────────────────────┐
│ Method ID│ Flags │ Req ID   │ Length   │ Payload              │
│ 2 bytes  │ 1 byte│ 4 bytes  │ 4 bytes  │ N bytes              │
│ uint16 BE│       │ uint32 BE│ uint32 BE│ (MsgPack)            │
└──────────┴───────┴──────────┴──────────┴──────────────────────┘
```

## Response Types

### Simple Response

```rust
ctx.respond(&data).await?;
```

### Acknowledgment (for fire-and-forget)

```rust
ctx.ack().await?;
```

### Streaming Response

```rust
for chunk in chunks {
    ctx.chunk(&chunk).await?;
}
ctx.end().await?;
```

### Error Response

```rust
ctx.error("Something went wrong").await?;
```

## Events (Fire-and-Forget)

Send events to the parent process:

```rust
client.emit("progress", &ProgressEvent { percent: 50 }).await?;
```

## Cancellation Support

Handlers can respond to abort signals from the parent:

```rust
.method("long_task", |ctx, _payload: ()| async move {
    loop {
        if ctx.is_cancelled() {
            return Ok(());
        }

        // Or use select! for async operations
        tokio::select! {
            _ = ctx.cancelled() => return Ok(()),
            result = do_work() => {
                ctx.chunk(&result).await?;
            }
        }
    }
})
```

## Authentication (optional)

When the parent enables auth (`spawnPolicy({ auth: true })`), it passes a
per-spawn token to the child via the `PROCWIRE_TOKEN` environment variable and
sends it as the first data-plane frame (an AUTH frame, method id `0xFFFE`). The
client picks up `PROCWIRE_TOKEN` automatically and refuses to adopt a connection
whose first frame is not a matching AUTH frame (compared in constant time):

```rust
// Nothing to do — PROCWIRE_TOKEN is read from the environment automatically.
let client = ClientBuilder::new()
    .handle("echo", |req: String, ctx| async move { ctx.respond(&req).await })
    .start()
    .await?;

// Or wire the token explicitly (an explicit token wins over the env var):
let client = ClientBuilder::new()
    .auth_token("….")
    .handle("echo", |req: String, ctx| async move { ctx.respond(&req).await })
    .start()
    .await?;
```

With auth disabled (the default), no token is present and connections are
adopted on accept — fully compatible with non-auth parents.

## Configuration

```rust
ClientBuilder::new()
    .max_concurrent_handlers(256)              // Limit concurrent handler tasks
    .max_payload_size(64 * 1024 * 1024)        // Bound incoming frame payloads (default 1 GiB)
    .backpressure_timeout(Duration::from_secs(30))
    .handle("handler", |req: String, ctx| async move { /* ... */ })
    .start()
    .await?;
```

## Platform Support

| Platform | Transport |
|----------|-----------|
| Linux    | Unix Domain Socket |
| macOS    | Unix Domain Socket |
| Windows  | Named Pipe |

## Related Projects

- [procwire](https://github.com/SebastianWebdev/procwire) - Node.js/TypeScript parent library (`@procwire/core`)

## License

MIT License - see [LICENSE](LICENSE) for details.
