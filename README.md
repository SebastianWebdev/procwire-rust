# procwire-client

[![Crates.io](https://img.shields.io/crates/v/procwire-client.svg)](https://crates.io/crates/procwire-client)
[![Documentation](https://docs.rs/procwire-client/badge.svg)](https://docs.rs/procwire-client)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Rust client SDK for [Procwire](https://github.com/SebastianWebdev/procwire) v2.0 IPC protocol.

This crate enables Rust workers (child processes) to communicate with a Node.js parent process running `@procwire/core` using a high-performance binary protocol.

## Features

- **High Performance**: Binary protocol with MsgPack serialization, achieving >1 GB/s throughput
- **Zero-copy**: Uses `bytes::BytesMut` for efficient buffer management
- **Async/await**: Built on Tokio for non-blocking I/O
- **Cross-platform**: Works on Linux, macOS, and Windows
- **Type-safe**: Strongly typed handlers with Serde integration
- **Streaming**: Support for chunked responses and backpressure
- **Cancellation**: Full abort signal support with `CancellationToken`

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
        .method("echo", |ctx, payload: EchoRequest| async move {
            ctx.respond(&EchoResponse {
                message: payload.message,
            })
            .await
        })
        .build()
        .await?;

    client.run().await?;
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

## Configuration

```rust
ClientBuilder::new()
    .max_concurrent_handlers(256)  // Limit concurrent handler tasks
    .backpressure_timeout(Duration::from_secs(30))
    .method("handler", |ctx, req| async move { /* ... */ })
    .build()
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
