# procwire-client-rs — Plan Wykonania (Task Plan)

> Granularne taski do realizacji przez agenta kodującego.
> Każdy task jest self-contained, testowany, i buduje na poprzednich.

---

## PHASE 0: Bootstrapping — Konfiguracja Środowiska

### TASK-00: Inicjalizacja Repozytorium i Środowiska

**Cel:** Utworzenie kompletnego scaffoldu projektu Rust.

**Wymagania wstępne:**
- Rust toolchain (rustup): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Minimum Rust 1.75+ (edition 2021)
- Git zainstalowany

**Kroki:**

1. Utwórz nowe repo:
```bash
mkdir procwire-client-rs
cd procwire-client-rs
git init
```

2. Utwórz projekt Cargo:
```bash
cargo init --lib
```

3. Skonfiguruj `Cargo.toml`:
```toml
[package]
name = "procwire-client"
version = "0.1.0"
edition = "2021"
description = "Rust client SDK for Procwire IPC — high-performance binary protocol"
license = "MIT"
keywords = ["ipc", "binary-protocol", "process-communication"]
categories = ["network-programming"]
rust-version = "1.75"

[dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rmp-serde = "1"
bytes = "1"
tracing = "0.1"
thiserror = "2"

[dev-dependencies]
tokio-test = "0.4"
criterion = { version = "0.5", features = ["async_tokio"] }

[[bench]]
name = "throughput"
harness = false
```

4. Utwórz strukturę katalogów:
```bash
mkdir -p src/protocol src/control src/transport src/codec src/handler
mkdir -p tests examples benches docs
```

5. Utwórz placeholder pliki:
```bash
# src/lib.rs (główny export)
# src/client.rs
# src/error.rs
# src/protocol/mod.rs, wire_format.rs, frame_buffer.rs, frame.rs
# src/control/mod.rs, init.rs, stdio.rs
# src/transport/mod.rs, pipe.rs
# src/codec/mod.rs, traits.rs, msgpack.rs, raw.rs
# src/handler/mod.rs, context.rs, registry.rs
# src/backpressure.rs
```

6. Utwórz `.gitignore`:
```
/target
Cargo.lock
*.swp
*.swo
.idea/
.vscode/
```

7. Utwórz `CLAUDE.md` (patrz TASK-00b)

8. Weryfikacja:
```bash
cargo check
cargo test  # Powinno przejść (puste testy)
```

**Kryterium akceptacji:**
- `cargo check` przechodzi bez błędów
- `cargo test` przechodzi
- Struktura katalogów zgodna z planem
- CLAUDE.md istnieje

---

### TASK-00b: CLAUDE.md — Instrukcje dla Agenta

**Cel:** Plik z instrukcjami dla agenta kodującego pracującego na tym repo.

Utwórz `CLAUDE.md` w korzeniu repo:

```markdown
# CLAUDE.md — Agent Instructions for procwire-client-rs

## Project Overview

Rust client SDK for Procwire IPC protocol v2.0.
This crate allows writing Rust workers (child processes) that communicate with
a Node.js parent process (@procwire/core ModuleManager).

## Architecture

### Dual-Channel Design
- **Control Plane:** stdio (stdin/stdout), JSON-RPC 2.0, for handshake ($init)
- **Data Plane:** Unix Socket / Named Pipe, Binary Protocol (11-byte header)

### CRITICAL RULES
1. **Data Plane = BINARY ONLY** — ZERO JSON on the pipe/socket
2. **Control Plane = JSON only for $init** — never send binary through stdio
3. All numbers in wire format are **Big Endian**
4. Header is exactly **11 bytes**
5. Method ID 0 is **reserved** (never use)
6. Method ID 0xFFFF is **reserved for ABORT**
7. Request ID 0 means **event** (no correlation)

### Wire Format
```
[Method ID: 2 bytes u16 BE][Flags: 1 byte][Request ID: 4 bytes u32 BE][Length: 4 bytes u32 BE][Payload: N bytes]
```

### Flags byte:
- bit 0: DIRECTION_TO_PARENT (0x01)
- bit 1: IS_RESPONSE (0x02)
- bit 2: IS_ERROR (0x04)
- bit 3: IS_STREAM (0x08)
- bit 4: STREAM_END (0x10)
- bit 5: IS_ACK (0x20)
- bits 6-7: reserved (must be 0)

## Commands
- `cargo check` — type check
- `cargo test` — run all tests
- `cargo test -- --nocapture` — tests with stdout
- `cargo clippy` — lint
- `cargo fmt` — format code
- `cargo bench` — run benchmarks

## Code Style
- Use `thiserror` for error types
- Use `tracing` for logging (not println!)
- Use `bytes::BytesMut` for frame buffer
- All async code uses `tokio`
- Prefer `writev` (scatter/gather) over concat+write for sending frames
- All public APIs must have doc comments with examples

## Testing
- Every module has unit tests in `#[cfg(test)] mod tests {}`
- Integration tests in `tests/` directory
- E2E tests spawn real Node.js parent (requires node installed)
- Never skip error handling — every Result must be handled

## Protocol Reference
Read `docs/01-PROTOCOL-SPEC.md` for the full protocol specification.
Read `docs/02-PACKAGE-PLAN.md` for the architecture plan.
```

**Kryterium akceptacji:**
- CLAUDE.md istnieje w korzeniu repo
- Zawiera wszystkie krytyczne reguły protokołu
- Zawiera komendy budowania i testowania

---

## PHASE 1: Protocol Foundation

### TASK-01: Wire Format — Header Encode/Decode

**Cel:** Implementacja kodowania i dekodowania 11-bajtowego nagłówka binarnego.

**Plik:** `src/protocol/wire_format.rs`

**Implementacja:**

1. Zdefiniuj stałe:
```rust
pub const HEADER_SIZE: usize = 11;
pub const DEFAULT_MAX_PAYLOAD_SIZE: u32 = 1_073_741_824; // 1 GB
pub const ABSOLUTE_MAX_PAYLOAD_SIZE: u32 = 2_147_483_647; // ~2 GB
pub const ABORT_METHOD_ID: u16 = 0xFFFF;
pub const HEADER_POOL_SIZE: usize = 16;
```

2. Zdefiniuj moduł Flags:
```rust
pub mod flags {
    pub const DIRECTION_TO_PARENT: u8 = 0b00000001;
    pub const IS_RESPONSE: u8         = 0b00000010;
    pub const IS_ERROR: u8            = 0b00000100;
    pub const IS_STREAM: u8           = 0b00001000;
    pub const STREAM_END: u8          = 0b00010000;
    pub const IS_ACK: u8              = 0b00100000;

    pub fn has_flag(flags: u8, flag: u8) -> bool { flags & flag != 0 }
}
```

3. Zdefiniuj `FrameHeader`:
```rust
#[derive(Debug, Clone, PartialEq)]
pub struct FrameHeader {
    pub method_id: u16,
    pub flags: u8,
    pub request_id: u32,
    pub payload_length: u32,
}
```

4. Implementuj encode/decode:
```rust
pub fn encode_header(header: &FrameHeader) -> [u8; HEADER_SIZE] { ... }
pub fn encode_header_into(buf: &mut [u8], header: &FrameHeader) { ... }
pub fn decode_header(buf: &[u8]) -> Result<FrameHeader, ProtocolError> { ... }
pub fn validate_header(header: &FrameHeader, max_payload: u32) -> Result<(), ProtocolError> { ... }
```

**Testy (w module):**
- Round-trip encode → decode
- Big Endian byte order verification
- Minimum/maximum values
- Method ID 0 rejected
- Payload size validation
- Flags bit patterns

**Kryterium akceptacji:**
- `cargo test protocol::wire_format` — PASS
- Pełna kompatybilność byte-order z Node.js implementacją

---

### TASK-02: Frame Buffer — Akumulacja Frames z Chunków

**Cel:** Bufor akumulujący bajty z socketa i wyciągający kompletne ramki.

**Plik:** `src/protocol/frame_buffer.rs`

**Implementacja:**

1. Użyj `bytes::BytesMut` jako wewnętrzny bufor
2. Stany maszyny stanowej:
   - `WaitingForHeader` — potrzeba ≥ 11 bytes
   - `WaitingForPayload { header, remaining }` — potrzeba `remaining` bytes
3. Metoda `push(data: &[u8]) -> Vec<Frame>`
4. Obsługa wielu frames w jednym push (pipeline)
5. Obsługa partial header/payload (fragmentation)

**Kluczowe:**
- NIGDY nie rób `copy` w pętli — użyj `BytesMut::extend_from_slice` raz, potem `split_to`
- Payload przechowywany jako `Bytes` (frozen, zero-copy sharing)
- Frame buffer jest restowalny — po `push()` wewnętrzny stan jest konsystentny

**Testy:**
- Single complete frame
- Multiple frames in one push
- Fragmented header (partial header, then rest)
- Fragmented payload (header complete, payload in 2 pushes)
- Large payload (1MB)
- Empty payload (0 bytes)
- Max payload validation

**Kryterium akceptacji:**
- `cargo test protocol::frame_buffer` — PASS
- Żadne alokacje w hot path (poza `BytesMut::extend_from_slice`)
- Poprawne odzyskiwanie po fragmentation

---

### TASK-03: Frame Struct i Protocol Exports

**Cel:** Frame struct i publiczny API modułu protocol.

**Pliki:** `src/protocol/frame.rs`, `src/protocol/mod.rs`

**Implementacja:**

1. `Frame` struct:
```rust
pub struct Frame {
    pub header: FrameHeader,
    payload: Bytes,  // frozen, zero-copy
}

impl Frame {
    pub fn payload(&self) -> &[u8] { &self.payload }
    pub fn payload_bytes(&self) -> Bytes { self.payload.clone() } // cheap clone
    pub fn payload_length(&self) -> usize { self.payload.len() }
}
```

2. Frame builder:
```rust
pub fn build_frame(header: FrameHeaderInput, payload: &[u8]) -> Vec<u8> { ... }
pub fn build_frame_parts(header: FrameHeaderInput, payload: &[u8]) -> ([u8; HEADER_SIZE], &[u8]) { ... }
```

3. Exporty w `src/protocol/mod.rs`

**Kryterium akceptacji:**
- `cargo test protocol` — ALL PASS
- Public API eksportuje: FrameHeader, Frame, FrameBuffer, flags, constants, encode/decode

---

## PHASE 2: Codec Layer

### TASK-04: Codec Trait + Raw Codec

**Cel:** Definiowanie interfejsu codec i implementacja prostego raw codec.

**Pliki:** `src/codec/traits.rs`, `src/codec/raw.rs`, `src/codec/mod.rs`

**Implementacja:**

1. Trait `Codec`:
```rust
pub trait Codec: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn serialize(&self, data: &dyn erased_serde::Serialize) -> Result<Vec<u8>, CodecError>;
    fn deserialize<'de, T: serde::Deserialize<'de>>(&self, data: &'de [u8]) -> Result<T, CodecError>;
}
```

   Alternatywa (prostsza, recommended):
```rust
pub trait Codec: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn serialize_value(&self, data: &rmpv::Value) -> Result<Vec<u8>, CodecError>;
    fn deserialize_value(&self, data: &[u8]) -> Result<rmpv::Value, CodecError>;
}
```

   **Rekomendacja:** Użyj generycznego podejścia z `serde`:
```rust
// Serialize: user provides &T where T: Serialize
// Deserialize: framework returns bytes, user deserializes with serde
// This avoids trait object complications

pub trait Codec: Send + Sync {
    fn name(&self) -> &str;
    fn serialize_bytes(&self, data: &[u8]) -> Bytes; // For raw
}
```

   **OSTATECZNIE:** Najlepsze podejście to NIE robić codec jako trait object, ale
   serializować na poziomie handlera (handler wie jaki typ), a codec trait służy
   tylko do rejestracji nazwy. Framework operuje na surowych `Bytes`:

   **Handler serializuje sam:**
```rust
// W handlerze:
ctx.respond(&my_struct).await  // respond() robi serialize wewnętrznie

// RequestContext wie jaki codec użyć (z konfiguracji metody)
// Używa rmp_serde::to_vec() internally
```

2. Raw codec (pass-through):
```rust
pub struct RawCodec;
impl RawCodec {
    pub fn serialize(data: &[u8]) -> Bytes { Bytes::copy_from_slice(data) }
    pub fn deserialize(data: &[u8]) -> &[u8] { data }
}
```

**Kryterium akceptacji:**
- `cargo test codec` — PASS
- Raw codec round-trip działa

---

### TASK-05: MsgPack Codec

**Cel:** MsgPack serializacja/deserializacja kompatybilna z Node.js @msgpack/msgpack.

**Plik:** `src/codec/msgpack.rs`

**Implementacja:**

1. Użyj `rmp-serde` w trybie `named` (struct fields as map keys — ważne dla interop z JS!):
```rust
pub fn serialize<T: Serialize>(data: &T) -> Result<Vec<u8>, CodecError> {
    rmp_serde::to_vec_named(data).map_err(|e| CodecError::Serialize(e.to_string()))
}

pub fn deserialize<'de, T: Deserialize<'de>>(data: &'de [u8]) -> Result<T, CodecError> {
    rmp_serde::from_slice(data).map_err(|e| CodecError::Deserialize(e.to_string()))
}
```

2. **KRYTYCZNE:** Użyj `to_vec_named` (nie `to_vec`)!
   - `to_vec` serializuje struct fields jako array (pozycyjne) → JS nie zrozumie
   - `to_vec_named` serializuje jako map (name → value) → kompatybilne z JS

3. Testuj interop z wartościami wygenerowanymi przez Node.js

**Testy:**
- Round-trip: simple object
- Round-trip: nested objects
- Round-trip: arrays
- Round-trip: null/None
- Interop: deserializacja payloadu z Node.js (hardcoded bytes)
- Interop: Buffer extension type (jeśli potrzebne)

**Kryterium akceptacji:**
- `cargo test codec::msgpack` — PASS
- Serializacja kompatybilna z Node.js `@msgpack/msgpack` encode()

---

## PHASE 3: Transport Layer

### TASK-06: Pipe Listener (Unix Socket / Named Pipe)

**Cel:** Cross-platform pipe listener — child nasłuchuje, parent się łączy.

**Plik:** `src/transport/pipe.rs`

**Implementacja:**

1. Generowanie ścieżki pipe:
```rust
pub fn generate_pipe_path() -> String {
    let pid = std::process::id();
    let rand: String = /* 8 random alphanumeric chars */;

    if cfg!(windows) {
        format!("\\\\.\\pipe\\procwire-{}-{}", pid, rand)
    } else {
        format!("/tmp/procwire-{}-{}.sock", pid, rand)
    }
}
```

2. Listener:
```rust
pub async fn listen_pipe(path: &str) -> Result<tokio::net::UnixStream, ProcwireError> {
    // Unix: tokio::net::UnixListener::bind(path)
    // Windows: tokio::net::windows::named_pipe::ServerOptions (lub net::TcpListener fallback)

    let listener = UnixListener::bind(path)?;
    let (stream, _addr) = listener.accept().await?;
    Ok(stream)
}
```

3. Cleanup: Usuń socket file po zamknięciu (Unix only)

**Platform-specific:**
- **Unix/macOS:** `tokio::net::UnixListener` + `UnixStream`
- **Windows:** `tokio::net::windows::named_pipe::ServerOptions` → `NamedPipeServer`
  - UWAGA: API jest inne niż Unix! Wymaga conditional compilation `#[cfg(windows)]`

**Testy:**
- Listener bind + accept (loopback test)
- Poprawna ścieżka na danej platformie
- Cleanup after close

**Kryterium akceptacji:**
- `cargo test transport::pipe` — PASS na Linux/macOS
- (Windows: test jeśli CI ma Windows runner)

---

### TASK-07: Stdio I/O (Control Plane)

**Cel:** Wysyłanie JSON-RPC przez stdout, (przyszłe) czytanie ze stdin.

**Plik:** `src/control/stdio.rs`

**Implementacja:**

1. Write to stdout (synchronous — jedna linia JSON + newline):
```rust
pub fn write_stdout_line(json: &str) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(json.as_bytes()).unwrap();
    handle.write_all(b"\n").unwrap();
    handle.flush().unwrap();
}
```

2. **WAŻNE:** NIE używaj `println!` — parent parsuje stdout line-by-line i `println!` dodaje
   system-dependent line endings. Użyj explicit `\n`.

3. (Przyszłość) Async stdin reader dla heartbeat/shutdown:
```rust
pub async fn read_stdin_lines() -> impl Stream<Item = String> { ... }
```

**Kryterium akceptacji:**
- Funkcja `write_stdout_line` wysyła poprawną linię JSON
- Żadne logi/debug output nie idą na stdout (muszą iść na stderr)

---

### TASK-08: $init Message Builder

**Cel:** Budowanie i wysyłanie wiadomości `$init` na stdout.

**Plik:** `src/control/init.rs`

**Implementacja:**

1. Schema builder:
```rust
pub struct InitSchema {
    pub methods: HashMap<String, MethodSchema>,
    pub events: HashMap<String, EventSchema>,
}

pub struct MethodSchema {
    pub id: u16,
    pub response: ResponseType,
}

pub struct EventSchema {
    pub id: u16,
}

pub enum ResponseType {
    Result,
    Stream,
    Ack,
    None,
}
```

2. Init message:
```rust
pub fn build_init_message(pipe_path: &str, schema: &InitSchema) -> String {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "$init",
        "params": {
            "pipe": pipe_path,
            "schema": {
                "methods": schema.methods.iter().map(|(name, m)| {
                    (name.clone(), serde_json::json!({
                        "id": m.id,
                        "response": match m.response {
                            ResponseType::Result => "result",
                            ResponseType::Stream => "stream",
                            ResponseType::Ack => "ack",
                            ResponseType::None => "none",
                        }
                    }))
                }).collect::<serde_json::Map<_, _>>(),
                "events": schema.events.iter().map(|(name, e)| {
                    (name.clone(), serde_json::json!({ "id": e.id }))
                }).collect::<serde_json::Map<_, _>>(),
            },
            "version": "2.0.0"
        }
    });
    serde_json::to_string(&msg).unwrap()
}
```

**Testy:**
- Poprawny JSON format
- Poprawne ID assignment
- Poprawne response type strings
- Version field present

**Kryterium akceptacji:**
- `cargo test control::init` — PASS
- JSON output parsowany poprawnie przez Node.js

---

## PHASE 4: Handler System

### TASK-09: Handler Registry

**Cel:** Rejestr metod i handlerów z type-safe dispatch.

**Plik:** `src/handler/registry.rs`

**Implementacja:**

1. Handler trait:
```rust
pub trait Handler: Send + Sync + 'static {
    fn call(&self, data: &[u8], ctx: RequestContext) -> Pin<Box<dyn Future<Output = Result<()>> + Send>>;
}
```

2. Typed handler wrapper (konwertuje `&[u8]` → deserializowany typ):
```rust
struct TypedHandler<F, T, Fut>
where
    F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
    T: DeserializeOwned + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    handler: F,
    _phantom: PhantomData<(T, Fut)>,
}
```

3. Registry:
```rust
pub struct HandlerRegistry {
    methods: HashMap<String, MethodEntry>,
    events: HashSet<String>,
    method_name_to_id: HashMap<String, u16>,
    method_id_to_name: HashMap<u16, String>,
    event_name_to_id: HashMap<String, u16>,
}

struct MethodEntry {
    handler: Box<dyn Handler>,
    response_type: ResponseType,
}
```

**Kryterium akceptacji:**
- `cargo test handler::registry` — PASS
- Rejestracja metod z typami generycznymi działa
- Dispatch po method_id działa

---

### TASK-10: RequestContext

**Cel:** Kontekst przekazywany do handlera — umożliwia wysyłanie odpowiedzi.

**Plik:** `src/handler/context.rs`

**Implementacja:**

```rust
pub struct RequestContext {
    request_id: u32,
    method_name: String,
    method_id: u16,
    writer: Arc<Mutex<PipeWriter>>,  // shared write access to socket
    responded: AtomicBool,
    aborted: AtomicBool,
    cancellation_token: CancellationToken,
}

impl RequestContext {
    pub async fn respond<T: Serialize>(&self, data: &T) -> Result<()> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        let payload = rmp_serde::to_vec_named(data)?;
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE;
        self.write_frame(flags, &payload).await
    }

    pub async fn ack<T: Serialize>(&self, data: &T) -> Result<()> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        let payload = rmp_serde::to_vec_named(data)?;
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE | flags::IS_ACK;
        self.write_frame(flags, &payload).await
    }

    pub async fn ack_empty(&self) -> Result<()> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE | flags::IS_ACK;
        self.write_frame(flags, &[]).await
    }

    pub async fn chunk<T: Serialize>(&self, data: &T) -> Result<()> {
        // chunk() NIE ustawia responded — można wysłać wiele chunków
        let payload = rmp_serde::to_vec_named(data)?;
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE | flags::IS_STREAM;
        self.write_frame(flags, &payload).await
    }

    pub async fn end(&self) -> Result<()> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        // STREAM_END z pustym payloadem
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE
                  | flags::IS_STREAM | flags::STREAM_END;
        self.write_frame(flags, &[]).await
    }

    pub async fn error(&self, message: &str) -> Result<()> {
        self.ensure_not_responded()?;
        self.responded.store(true, Ordering::SeqCst);

        let payload = rmp_serde::to_vec_named(&message)?;
        let flags = flags::DIRECTION_TO_PARENT | flags::IS_RESPONSE | flags::IS_ERROR;
        self.write_frame(flags, &payload).await
    }

    // Internal: write header + payload to socket
    async fn write_frame(&self, flags: u8, payload: &[u8]) -> Result<()> {
        let header = FrameHeader {
            method_id: self.method_id,
            flags,
            request_id: self.request_id,
            payload_length: payload.len() as u32,
        };

        let header_bytes = encode_header(&header);
        let mut writer = self.writer.lock().await;
        // Use write_all for header, then write_all for payload
        // TODO: use writev for single syscall
        writer.write_all(&header_bytes).await?;
        writer.write_all(payload).await?;
        writer.flush().await?;
        Ok(())
    }
}
```

**Testy:**
- respond() sets responded flag
- Double respond() returns error
- chunk() doesn't set responded
- end() sets responded
- error() sets responded
- Correct flags for each response type

**Kryterium akceptacji:**
- `cargo test handler::context` — PASS
- Poprawne flagi dla każdego typu odpowiedzi
- Backpressure propagated (write_all handles partial writes)

---

## PHASE 5: Client Assembly

### TASK-11: Client Builder

**Cel:** Fluent API do konfiguracji klienta.

**Plik:** `src/client.rs`

**Implementacja:**

```rust
pub struct ClientBuilder {
    registry: HandlerRegistry,
    default_codec: CodecType,
}

impl ClientBuilder {
    pub fn new() -> Self { ... }

    pub fn handle<F, T, Fut>(mut self, method: &str, handler: F) -> Self
    where
        F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
        T: DeserializeOwned + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        self.registry.register(method, handler, ResponseType::Result);
        self
    }

    pub fn handle_stream<F, T, Fut>(mut self, method: &str, handler: F) -> Self { ... }
    pub fn handle_ack<F, T, Fut>(mut self, method: &str, handler: F) -> Self { ... }

    pub fn event(mut self, name: &str) -> Self {
        self.registry.register_event(name);
        self
    }

    pub async fn start(self) -> Result<Client> {
        Client::start(self.registry).await
    }
}

pub struct Client {
    // Runtime state
    registry: HandlerRegistry,
    pipe_writer: Arc<Mutex<PipeWriter>>,
    shutdown_rx: oneshot::Receiver<()>,
}
```

**Kryterium akceptacji:**
- `cargo test client` — PASS
- Builder compiles z ergonomicznym API
- Method chaining działa

---

### TASK-12: Client Runtime Loop

**Cel:** Główna pętla klienta: start → init → accept → read frames → dispatch.

**Plik:** `src/client.rs` (kontynuacja)

**Implementacja:**

```rust
impl Client {
    async fn start(registry: HandlerRegistry) -> Result<Client> {
        // 1. Generate pipe path
        let pipe_path = generate_pipe_path();

        // 2. Start pipe listener
        let listener = listen_pipe(&pipe_path).await?;

        // 3. Build schema from registry
        let schema = registry.build_schema();

        // 4. Send $init to parent (stdout)
        let init_msg = build_init_message(&pipe_path, &schema);
        write_stdout_line(&init_msg);

        // 5. Accept parent connection
        let stream = accept_connection(listener).await?;

        // 6. Split stream into reader + writer
        let (reader, writer) = tokio::io::split(stream);
        let writer = Arc::new(Mutex::new(writer));

        // 7. Spawn read loop
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let registry = Arc::new(registry);
        let writer_clone = writer.clone();

        tokio::spawn(async move {
            let mut frame_buffer = FrameBuffer::new();
            let mut buf = vec![0u8; 64 * 1024]; // 64KB read buffer

            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break, // Connection closed
                    Ok(n) => {
                        let frames = frame_buffer.push(&buf[..n]);
                        for frame in frames {
                            dispatch_frame(frame, &registry, &writer_clone).await;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Pipe read error: {}", e);
                        break;
                    }
                }
            }
            let _ = shutdown_tx.send(());
        });

        Ok(Client { registry: (*registry).clone(), pipe_writer: writer, shutdown_rx })
    }

    pub async fn wait_for_shutdown(self) -> Result<()> {
        let _ = self.shutdown_rx.await;
        Ok(())
    }

    pub async fn emit<T: Serialize>(&self, event: &str, data: &T) -> Result<()> {
        let event_id = self.registry.get_event_id(event)?;
        let payload = rmp_serde::to_vec_named(data)?;

        let header = FrameHeader {
            method_id: event_id,
            flags: flags::DIRECTION_TO_PARENT,
            request_id: 0, // Events have no request ID
            payload_length: payload.len() as u32,
        };

        let header_bytes = encode_header(&header);
        let mut writer = self.pipe_writer.lock().await;
        writer.write_all(&header_bytes).await?;
        writer.write_all(&payload).await?;
        writer.flush().await?;
        Ok(())
    }
}
```

**Dispatch logic:**
```rust
async fn dispatch_frame(frame: Frame, registry: &HandlerRegistry, writer: &PipeWriter) {
    let header = &frame.header;

    // Abort handling
    if header.method_id == ABORT_METHOD_ID {
        registry.abort(header.request_id);
        return;
    }

    // Lookup method
    let method_name = match registry.get_method_name(header.method_id) {
        Some(name) => name,
        None => {
            // Send error response
            send_error_response(writer, header, "Unknown method ID").await;
            return;
        }
    };

    // Create context
    let ctx = RequestContext::new(
        header.request_id,
        method_name.to_string(),
        header.method_id,
        writer.clone(),
    );

    // Spawn handler task
    let handler = registry.get_handler(&method_name).unwrap();
    tokio::spawn(async move {
        if let Err(e) = handler.call(frame.payload(), ctx.clone()).await {
            if !ctx.is_responded() {
                let _ = ctx.error(&e.to_string()).await;
            }
        }
    });
}
```

**Kryterium akceptacji:**
- `cargo test client` — PASS
- Klient startuje, wysyła $init, akceptuje połączenie
- Frame dispatch działa
- Graceful shutdown na zamknięcie pipe

---

## PHASE 6: Integration & Testing

### TASK-13: Unit Test Suite

**Cel:** Kompletne testy jednostkowe.

**Pliki:** Testy w `#[cfg(test)]` w każdym module + `tests/`

**Zakres:**
1. Wire format: encode/decode round-trip, byte order, edge cases
2. Frame buffer: fragmentation, multi-frame, large payload
3. Codec: MsgPack interop, raw pass-through
4. Handler registry: register, lookup, dispatch
5. RequestContext: all response types, double-respond guard
6. Client builder: compile-time API correctness

**Kryterium akceptacji:**
- `cargo test` — ALL PASS
- Coverage > 80% (mierzone `cargo tarpaulin`)

---

### TASK-14: E2E Test z Node.js Parent

**Cel:** Test end-to-end: Node.js parent ↔ Rust child.

**Plik:** `tests/e2e_test.rs` + `tests/fixtures/parent.mjs`

**Implementacja:**

1. Node.js fixture (`tests/fixtures/parent.mjs`):
```javascript
// Importuje @procwire/core
// Konfiguruje Module z executable na Rust binary
// Spawnuje moduł
// Wysyła requesty
// Weryfikuje odpowiedzi
// Testuje streaming
// Testuje eventy
// Raportuje wyniki na stdout jako JSON
```

2. Rust test:
```rust
#[tokio::test]
async fn test_e2e_echo() {
    // Build Rust example binary
    // Run Node.js fixture that spawns it
    // Parse JSON results from Node.js stdout
    // Assert all tests passed
}
```

**Test cases:**
- Echo (request → result response)
- Stream (request → multiple chunks → end)
- ACK (request → ack response)
- Error (request → error response)
- Events (child emits → parent receives)
- Abort (parent sends abort → child cancels)
- Large payload (10MB+ throughput test)

**Kryterium akceptacji:**
- E2E tests pass na Linux
- (Opcjonalnie) macOS i Windows

---

### TASK-15: Examples

**Cel:** Przykładowe workery Rust.

**Pliki:** `examples/`

1. `examples/echo_worker.rs` — prosty echo
2. `examples/stream_worker.rs` — generowanie streamu
3. `examples/event_worker.rs` — emitowanie eventów progress

Każdy example musi:
- Kompilować się z `cargo build --example <name>`
- Być uruchamialny jako child process @procwire/core
- Mieć komentarze wyjaśniające API

**Kryterium akceptacji:**
- `cargo build --examples` — OK
- Każdy example działa z Node.js parent

---

### TASK-16: CI/CD (GitHub Actions)

**Cel:** Automatyczne testy na CI.

**Plik:** `.github/workflows/ci.yml`

```yaml
name: CI
on: [push, pull_request]
jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        rust: [stable, "1.75"]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: ${{ matrix.rust }}
          components: clippy, rustfmt
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test --all-features
      - run: cargo build --examples

  e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with:
          node-version: "22"
      - run: npm install -g @procwire/core @procwire/client @procwire/codecs @procwire/protocol
      - run: cargo test --test e2e_test
```

**Kryterium akceptacji:**
- CI zielony na wszystkich platformach
- Clippy zero warnings
- Fmt check pass

---

## Podsumowanie Faz i Szacowane Czasy

| Phase | Taski     | Opis                          | Szacunek  |
|-------|-----------|-------------------------------|-----------|
| 0     | 00, 00b   | Bootstrap + CLAUDE.md         | 1-2h      |
| 1     | 01-03     | Protocol (wire, frame, types) | 4-6h      |
| 2     | 04-05     | Codec (trait, raw, msgpack)   | 2-3h      |
| 3     | 06-08     | Transport (pipe, stdio, init) | 3-4h      |
| 4     | 09-10     | Handlers (registry, context)  | 3-4h      |
| 5     | 11-12     | Client (builder, runtime)     | 4-6h      |
| 6     | 13-16     | Testing + CI                  | 4-6h      |
| **Σ** |           |                               | **21-31h**|

---

## Kolejność Wykonania (Dependency Graph)

```
TASK-00 (Bootstrap) ─┐
TASK-00b (CLAUDE.md) ┘
         │
         ▼
TASK-01 (Wire Format) ──► TASK-02 (Frame Buffer) ──► TASK-03 (Frame + Exports)
                                                           │
         ┌─────────────────────────────────────────────────┘
         ▼
TASK-04 (Codec Trait + Raw) ──► TASK-05 (MsgPack)
         │
         ▼
TASK-06 (Pipe Listener) ──► TASK-07 (Stdio) ──► TASK-08 ($init)
         │                                            │
         └────────────────────┬───────────────────────┘
                              ▼
              TASK-09 (Handler Registry) ──► TASK-10 (RequestContext)
                              │                        │
                              └────────┬───────────────┘
                                       ▼
                            TASK-11 (Client Builder)
                                       │
                                       ▼
                            TASK-12 (Client Runtime)
                                       │
                              ┌────────┼────────┐
                              ▼        ▼        ▼
                        TASK-13    TASK-14   TASK-15
                        (Tests)   (E2E)     (Examples)
                              │        │        │
                              └────────┼────────┘
                                       ▼
                                   TASK-16 (CI)
```

---

## Zasady dla Agenta

1. **PRZED KAŻDYM TASKIEM** przeczytaj `CLAUDE.md` i `docs/01-PROTOCOL-SPEC.md`
2. **Każdy task kończy się** uruchomieniem `cargo test` — MUSI przejść
3. **Nigdy nie skipuj error handling** — każdy `Result` musi być obsłużony
4. **Data Plane = BINARY** — zero JSON na pipe. JSON jest TYLKO na stdout ($init)
5. **Big Endian** — wszystkie multi-byte values w nagłówku
6. **`rmp_serde::to_vec_named`** — NIGDY `to_vec` (struct-as-map, nie struct-as-array)
7. **Testy interop** — serializowane dane muszą być deserializowalne przez Node.js i odwrotnie
8. **Cross-platform** — `#[cfg(unix)]` / `#[cfg(windows)]` dla pipe ścieżek
9. **Backpressure** — ZAWSZE sprawdzaj wynik write, nie ignoruj
10. **Zero-copy** — używaj `bytes::Bytes` i `BytesMut`, nie kopiuj payloadu niepotrzebnie
