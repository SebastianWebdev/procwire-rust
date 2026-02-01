# procwire-client-rs — Plan Paczki Rust

> Osobne repozytorium GitHub: `procwire-client-rs`
> Paczka crates.io: `procwire-client` (docelowo)

## 1. Cel

Dostarczyć Rust SDK umożliwiający pisanie workerów (child processes), które komunikują się z
**Parent procesem Node.js** (@procwire/core ModuleManager) według protokołu Procwire v2.0.

Klient Rust musi:
- Implementować Control Plane (JSON-RPC na stdio: handshake `$init`)
- Implementować Data Plane (Binary Protocol na Unix Socket / Named Pipe)
- Obsługiwać wszystkie typy odpowiedzi: `result`, `stream`, `ack`, `none`
- Obsługiwać eventy (child → parent)
- Obsługiwać abort/cancellation (parent → child)
- Obsługiwać backpressure
- Zapewnić ergonomiczne API z builder pattern
- Być cross-platform (Linux, macOS, Windows)

---

## 2. Struktura Crate

```
procwire-client-rs/
├── Cargo.toml
├── README.md
├── LICENSE
├── CLAUDE.md                    # Instrukcje dla agenta kodującego
├── docs/
│   ├── 01-PROTOCOL-SPEC.md     # Pełna spec protokołu
│   ├── 02-PACKAGE-PLAN.md      # Ten dokument
│   └── 03-TASK-PLAN.md         # Granularne taski implementacji
├── src/
│   ├── lib.rs                   # Public API exports
│   ├── client.rs                # Client struct (builder + runtime)
│   ├── protocol/
│   │   ├── mod.rs
│   │   ├── wire_format.rs       # Header encode/decode, flags, constants
│   │   ├── frame_buffer.rs      # Frame accumulation from socket chunks
│   │   └── frame.rs             # Frame struct (header + payload)
│   ├── control/
│   │   ├── mod.rs
│   │   ├── init.rs              # $init message builder
│   │   └── stdio.rs             # Stdin/stdout JSON-RPC I/O
│   ├── transport/
│   │   ├── mod.rs
│   │   └── pipe.rs              # Unix Socket / Named Pipe listener
│   ├── codec/
│   │   ├── mod.rs
│   │   ├── traits.rs            # Codec trait definition
│   │   ├── msgpack.rs           # MsgPack codec (rmp-serde)
│   │   └── raw.rs               # Raw bytes pass-through
│   ├── handler/
│   │   ├── mod.rs
│   │   ├── context.rs           # RequestContext (respond/ack/chunk/end/error)
│   │   └── registry.rs          # Method handler registry
│   ├── backpressure.rs          # Drain/write management
│   └── error.rs                 # Error types
├── tests/
│   ├── protocol_test.rs         # Wire format unit tests
│   ├── frame_buffer_test.rs     # Frame buffer tests
│   ├── codec_test.rs            # Codec interop tests
│   ├── client_test.rs           # Client builder tests
│   └── integration_test.rs      # End-to-end z Node.js parent
├── examples/
│   ├── echo_worker.rs           # Prosty echo worker
│   ├── stream_worker.rs         # Worker ze streamingiem
│   └── event_worker.rs          # Worker emitujący eventy
└── benches/
    ├── throughput.rs             # Benchmark przepustowości
    └── latency.rs               # Benchmark latencji
```

---

## 3. Zależności (Cargo.toml)

```toml
[package]
name = "procwire-client"
version = "0.1.0"
edition = "2021"
description = "Rust client SDK for Procwire IPC — high-performance binary protocol"
license = "MIT"
repository = "https://github.com/<org>/procwire-client-rs"
keywords = ["ipc", "binary-protocol", "process-communication"]
categories = ["network-programming"]

[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"                  # Tylko dla control plane ($init)
rmp-serde = "1"                   # MsgPack codec

# Bytes management
bytes = "1"                       # Zero-copy byte buffers

# Logging
tracing = "0.1"                   # Structured logging

# Error handling
thiserror = "2"                   # Derive Error trait

[dev-dependencies]
tokio-test = "0.4"
assert_matches = "1"
criterion = { version = "0.5", features = ["async_tokio"] }

[[bench]]
name = "throughput"
harness = false

[[bench]]
name = "latency"
harness = false
```

### Uzasadnienie wyboru zależności:

| Crate       | Dlaczego                                                    |
|-------------|--------------------------------------------------------------|
| `tokio`     | De facto standard async runtime, pipe/socket support         |
| `serde`     | Standard serializacji, interop z MsgPack                     |
| `serde_json`| TYLKO dla `$init` (control plane), nie dla data plane        |
| `rmp-serde` | MsgPack <=> Rust structs, kompatybilne z `@msgpack/msgpack`  |
| `bytes`     | Zero-copy `Bytes`/`BytesMut`, scatter/gather I/O             |
| `tracing`   | Strukturalne logowanie, integracja z tokio-console            |
| `thiserror` | Ergonomiczne error types z `#[derive(Error)]`                |

---

## 4. Public API Design

### 4.1. Builder Pattern

```rust
use procwire_client::{Client, RequestContext};
use serde::{Serialize, Deserialize};

#[derive(Deserialize)]
struct QueryInput { query: String, top_k: usize }

#[derive(Serialize)]
struct QueryResult { ids: Vec<u64>, scores: Vec<f32> }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .handle("echo", |data: serde_json::Value, ctx: RequestContext| async move {
            ctx.respond(&data).await
        })
        .handle("query", |data: QueryInput, ctx: RequestContext| async move {
            let results = search(&data).await;
            ctx.respond(&results).await
        })
        .handle_stream("generate", |data: GenInput, ctx: RequestContext| async move {
            for item in generate(&data) {
                ctx.chunk(&item).await?;
            }
            ctx.end().await
        })
        .event("progress")
        .build()
        .start()
        .await?;

    // Emit events
    client.emit("progress", &ProgressEvent { percent: 50 }).await?;

    // Keep running until parent kills us
    client.wait_for_shutdown().await?;
    Ok(())
}
```

### 4.2. RequestContext API

```rust
pub struct RequestContext {
    // ... internal fields
}

impl RequestContext {
    /// Send full response (response type: "result")
    pub async fn respond<T: Serialize>(&self, data: &T) -> Result<()>;

    /// Send ACK (response type: "ack")
    pub async fn ack<T: Serialize>(&self, data: &T) -> Result<()>;

    /// Send ACK without data
    pub async fn ack_empty(&self) -> Result<()>;

    /// Send stream chunk (response type: "stream")
    pub async fn chunk<T: Serialize>(&self, data: &T) -> Result<()>;

    /// End stream (sends STREAM_END frame with empty payload)
    pub async fn end(&self) -> Result<()>;

    /// Send error response
    pub async fn error(&self, message: &str) -> Result<()>;

    /// Check if request was aborted
    pub fn is_aborted(&self) -> bool;

    /// Get CancellationToken for cooperative cancellation
    pub fn cancellation_token(&self) -> CancellationToken;

    /// Request ID
    pub fn request_id(&self) -> u32;

    /// Method name
    pub fn method(&self) -> &str;
}
```

### 4.3. Event Emitting

```rust
impl Client {
    /// Emit event to parent (fire-and-forget)
    pub async fn emit<T: Serialize>(&self, event: &str, data: &T) -> Result<()>;
}
```

### 4.4. Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProcwireError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Codec error: {0}")]
    Codec(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Already started")]
    AlreadyStarted,

    #[error("Unknown method: {0}")]
    UnknownMethod(String),

    #[error("Unknown event: {0}")]
    UnknownEvent(String),

    #[error("Response already sent for request {0}")]
    ResponseAlreadySent(u32),

    #[error("Handler error: {0}")]
    HandlerError(String),

    #[error("Backpressure timeout")]
    BackpressureTimeout,
}
```

---

## 5. Wewnętrzna architektura

### 5.1. Runtime Model

```
┌─────────────────────────────────────────────────┐
│                Client Runtime                    │
│                                                  │
│  ┌──────────────┐  ┌─────────────────────────┐  │
│  │ Control Task  │  │ Data Task               │  │
│  │ (stdio)       │  │ (pipe socket)           │  │
│  │               │  │                         │  │
│  │ • Send $init  │  │ • Read frames           │  │
│  │ • Read stdin  │  │ • Dispatch to handlers  │  │
│  │   (future:    │  │ • Write responses       │  │
│  │    heartbeat)  │  │ • Handle backpressure   │  │
│  └──────────────┘  └─────────────────────────┘  │
│                                                  │
│  ┌──────────────────────────────────────────┐   │
│  │ Handler Tasks (tokio::spawn per request) │   │
│  │ • Deserialize request payload            │   │
│  │ • Execute user handler fn                │   │
│  │ • Serialize + send response              │   │
│  │ • Support cancellation via token         │   │
│  └──────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

### 5.2. Flow (Read Path)

```
Socket recv()
    │
    ▼
FrameBuffer::push(bytes)
    │
    ▼
Frame { header, payload_chunks }
    │
    ├── header.method_id == ABORT_METHOD_ID?
    │       └── Cancel handler task via CancellationToken
    │
    ├── Lookup method_id → method_name
    │
    ├── Deserialize payload (MsgPack → Rust type)
    │
    └── Spawn handler task with RequestContext
            │
            ▼
        User handler fn(data, ctx)
            │
            ▼
        ctx.respond() / ctx.chunk() / ctx.end() / ctx.error()
            │
            ▼
        Serialize response → Frame → socket.write()
```

### 5.3. Flow (Write Path)

```
ctx.respond(data)
    │
    ▼
Serialize data (Rust type → MsgPack bytes)
    │
    ▼
Encode header (11 bytes) into ring buffer slot
    │
    ▼
writev(header_iov, payload_iov)  ← single syscall!
    │
    ├── Ok → done
    └── WouldBlock → await writable readiness (backpressure)
```

---

## 6. Kompatybilność z istniejącymi implementacjami

### Mapowanie typów: Node.js ↔ Rust

| Node.js (MsgPack)       | Rust                          |
|--------------------------|-------------------------------|
| `string`                 | `String`                      |
| `number` (integer)       | `i64` / `u64`                 |
| `number` (float)         | `f64`                         |
| `boolean`                | `bool`                        |
| `null`                   | `Option<T>::None`             |
| `undefined`              | `Option<T>::None` (MsgPack nil)|
| `object`                 | `HashMap<String, Value>` lub struct|
| `Array`                  | `Vec<T>`                      |
| `Buffer`                 | `Vec<u8>` / `Bytes`           |
| `Date`                   | `SystemTime` / chrono (via ext)|

### Extension Types (MsgPack)

Node.js definiuje extension types:
- **Type 1:** Buffer → `Vec<u8>` (raw bytes)
- **Type 2:** Date → `f64` timestamp (ms since epoch)

Klient Rust MUSI zarejestrować te extension types w `rmp-serde` aby zapewnić
pełną interoperacyjność z Node.js.

---

## 7. Cele wydajnościowe

| Metryka                    | Cel                      |
|----------------------------|--------------------------|
| Throughput (10MB payload)  | > 1 GB/s                 |
| Throughput (1KB payload)   | > 500k req/s             |
| Latency (round-trip)       | < 100μs (small payload)  |
| Memory (idle)              | < 5 MB RSS               |
| Memory (1GB transfer)      | < 50 MB RSS (zero-copy)  |

### Techniki optymalizacji:

1. **Header ring buffer** — 16 pre-alokowanych buforów 11-byte, round-robin
2. **writev()** — scatter/gather I/O, header+payload w jednym syscall
3. **Zero-copy framing** — `bytes::BytesMut` jako frame buffer, split bez kopiowania
4. **Chunk accumulation** — `Vec<Bytes>` zamiast `Buffer.concat`
5. **Backpressure** — `AsyncWriteExt::writable()` zamiast unbounded buffering

---

## 8. Testowanie

### Poziomy testów:

| Poziom        | Co testuje                                   | Narzędzia              |
|---------------|----------------------------------------------|------------------------|
| Unit          | Wire format, flags, frame buffer             | `#[cfg(test)]`         |
| Integration   | Client ↔ mock parent (Rust)                  | tokio-test             |
| E2E           | Client Rust ↔ @procwire/core (Node.js)      | Node.js fixture scripts|
| Benchmark     | Throughput, latency                          | Criterion              |
| Cross-platform| CI na Linux, macOS, Windows                  | GitHub Actions         |

### E2E test flow:

```
Node.js test script:
  1. Importuje @procwire/core
  2. Konfiguruje ModuleManager
  3. Spawnuje Rust binary (cargo-built)
  4. Wysyła requesty
  5. Weryfikuje odpowiedzi
  6. Sprawdza throughput
```

---

## 9. Docelowa integracja

### Użycie z @procwire/core (Node.js parent):

```typescript
// Parent (Node.js)
const module = new Module("rust-worker")
  .executable("./target/release/my-worker") // Rust binary
  .method("query", { response: "result" })
  .method("generate", { response: "stream" })
  .event("progress");

await manager.spawn(module);

const result = await module.send("query", { text: "hello" });

for await (const chunk of module.stream("generate", { prompt: "..." })) {
  console.log(chunk);
}

module.onEvent("progress", (data) => {
  console.log(`Progress: ${data.percent}%`);
});
```

Żadne zmiany w `@procwire/core` nie są potrzebne — klient Rust implementuje
ten sam protokół co `@procwire/client` (TypeScript).
