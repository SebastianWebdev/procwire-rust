# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## ⚠️ FIRST: Read Agent Memory

**PRZED ROZPOCZĘCIEM PRACY** przeczytaj [`AGENT_MEMORY.md`](AGENT_MEMORY.md) - zawiera krótkie podsumowanie projektu, ostatnich decyzji, bugów i ważnych informacji między sesjami.

### Kiedy aktualizować Agent Memory

**ZAWSZE** aktualizuj `AGENT_MEMORY.md` na koniec sesji lub po zakończeniu znaczącego taska:

1. **Ukończone taski** - dodaj do sekcji "TODO / W Trakcie" z checkboxem `[x]`
2. **Nowe taski** - dodaj do sekcji "TODO / W Trakcie" z checkboxem `[ ]`
3. **Ważne decyzje architektoniczne** - dodaj do "Ostatnie Ważne Decyzje" z datą
4. **Naprawione bugi** - dodaj krótki postmortem do "Ostatnie Ważne Decyzje"
5. **Benchmark results** - aktualizuj tabelę jeśli wyniki się zmieniły
6. **Nowe notatki** - dodaj do "Notatki dla Agenta" jeśli odkryłeś coś ważnego

### Co zapisywać (przykłady)

```markdown
### 2026-02-01: Nazwa decyzji/buga

**Problem:** Krótki opis problemu
**Root cause:** Co było przyczyną
**Fix:** Jak naprawiono
**Commit:** `abc1234` (opcjonalnie)
```

### Czego NIE zapisywać

- Drobne refaktory bez wpływu na architekturę
- Poprawki literówek, formatowania
- Zmiany w dokumentacji (chyba że znaczące)
- Rzeczy już udokumentowane w `docs/`

## Task Management (docs/tasks/)

Folder `docs/tasks/` to centralne miejsce zarządzania taskami projektu.

### Struktura

```
docs/tasks/
├── todo/           # Taski do zrobienia (TASK-XX-nazwa.md)
├── done/           # Ukończone taski (przenoszone z todo/)
└── README.md       # Opis konwencji (opcjonalnie)
```

### Gdy user prosi o "napisanie taska" lub "zrobienie taska"

1. **Napisanie taska** = utworzenie pliku `docs/tasks/todo/TASK-XX-nazwa.md`
2. **Zrobienie taska** = implementacja według pliku z `todo/`, potem przeniesienie do `done/`

### Format pliku taska

```markdown
# TASK-XX: Krótki tytuł

## Cel

Co chcemy osiągnąć.

## Zakres

- [ ] Subtask 1
- [ ] Subtask 2

## Kontekst

Dlaczego to robimy, powiązane pliki, decyzje.

## Definition of Done

- Testy przechodzą
- Dokumentacja zaktualizowana
- Code review (jeśli wymagane)
```

### Po ukończeniu taska - OBOWIĄZKOWE

1. **Przenieś plik** z `todo/` do `done/`
2. **Zaktualizuj `AGENT_MEMORY.md`**:
   - Dodaj wpis `[x] TASK-XX: ...` w sekcji "TODO / W Trakcie"
   - Jeśli wystąpił bug lub ważna decyzja → dodaj do "Ostatnie Ważne Decyzje"
   - Jeśli odkryłeś coś przydatnego → dodaj do "Notatki dla Agenta"

**PAMIĘTAJ:** Memory musi zawierać wzmiankę o każdym zrealizowanym tasku. Jeśli podczas pracy wyszedł jakiś bug, edge case lub istotna informacja architektoniczna - zapisz to w memory, aby kolejne sesje mogły z tego skorzystać.

## Project Overview

This is a **Rust client SDK** for the Procwire v2.0 IPC protocol. It enables Rust workers (child processes) to communicate with a Node.js parent process running `@procwire/core`.

**Crate name:** `procwire-client`
**GitHub:** `SebastianWebdev/procwire-client-rs`
**Parent project:** `SebastianWebdev/procwire` (Node.js/TypeScript)

## Architecture

### Dual-Channel Architecture

```
Control Plane (stdio)     - JSON (only $init)  - Handshake
Data Plane (named pipe)   - BINARY PROTOCOL    - User data, high throughput
```

**CRITICAL RULE**: Data Plane = Binary Protocol = ZERO JSON

### Wire Format (Data Plane)

```
┌──────────┬───────┬──────────┬──────────┬──────────────────────┐
│ Method ID│ Flags │ Req ID   │ Length   │ Payload              │
│ 2 bytes  │ 1 byte│ 4 bytes  │ 4 bytes  │ N bytes              │
│ uint16 BE│       │ uint32 BE│ uint32 BE│ (codec output)       │
└──────────┴───────┴──────────┴──────────┴──────────────────────┘

Header: 11 bytes FIXED
All multi-byte integers: BIG ENDIAN
```

### Flags byte (bitmask)

```
bit 0 (0x01): DIRECTION_TO_PARENT  (0 = to child, 1 = to parent)
bit 1 (0x02): IS_RESPONSE          (0 = request/event, 1 = response)
bit 2 (0x04): IS_ERROR             (0 = ok, 1 = error)
bit 3 (0x08): IS_STREAM            (0 = single, 1 = stream chunk)
bit 4 (0x10): STREAM_END           (0 = more coming, 1 = final chunk)
bit 5 (0x20): IS_ACK               (0 = full response, 1 = ack only)
bit 6-7: reserved (must be 0)
```

### Reserved IDs

- Method ID `0` → reserved (never use)
- Method ID `0xFFFF` → ABORT signal
- Request ID `0` → event (fire-and-forget, no response expected)

### Transaction Patterns (exact flag values)

```
Request→Result:   request flags=0x00, response flags=0x03
Request→ACK:      request flags=0x00, response flags=0x23
Request→Stream:   request flags=0x00, chunks flags=0x0B, end flags=0x1B (empty payload!)
Error response:   flags=0x07
Event to parent:  flags=0x01, requestId=0
Abort:            methodId=0xFFFF, empty payload
```

**⚠️ STREAM_END frame ALWAYS has empty payload (payloadLength=0)**

### Lifecycle

1. Parent spawns Rust child process
2. Child creates pipe listener (platform-specific path)
3. Child sends `$init` via stdout (JSON, one line)
4. Parent validates schema, connects to pipe
5. Binary communication begins (full-duplex)
6. Shutdown: parent kills process

## ⛔ ABSOLUTE RULES

### 1. Data Plane = BINARY ONLY

```rust
// ❌ NIGDY na data plane:
serde_json::to_string(&data)   // NIGDY!
serde_json::from_str(&data)    // NIGDY!

// ✅ ZAWSZE na data plane:
rmp_serde::to_vec_named(&data)  // MsgPack codec
rmp_serde::from_slice(&bytes)   // MsgPack codec
// lub raw bytes pass-through (RawCodec)
```

### 2. MsgPack: ALWAYS `to_vec_named`, NEVER `to_vec`

```rust
// ❌ to_vec serializes structs as arrays → Node.js nie zdeserializuje!
rmp_serde::to_vec(&data)

// ✅ to_vec_named serializes structs as maps → kompatybilne z Node.js
rmp_serde::to_vec_named(&data)
```

**WHY:** Node.js `@msgpack/msgpack` expects struct-as-map format. Using `to_vec` produces struct-as-array which is incompatible.

### 3. All wire format numbers are Big Endian

```rust
// ✅ 
buf.put_u16(method_id);  // Big Endian by default in bytes crate
buf.put_u32(request_id);
buf.put_u32(length);

// ❌ NIGDY:
buf.put_u16_le(method_id);  // Little Endian = WRONG!
```

### 4. Header is EXACTLY 11 bytes

`[methodId: 2][flags: 1][requestId: 4][payloadLength: 4]` = 11 bytes total

### 5. JSON only for $init (Control Plane)

The only JSON in this crate is the `$init` message sent via stdout. Everything else on the pipe is binary.

### 6. Backpressure - always check write results

```rust
// ✅ Handle backpressure
writer.writable().await?;
writer.write_all(&header).await?;
writer.write_all(&payload).await?;

// ❌ Ignore write result → OOM crash
let _ = writer.write_all(&data);
```

### 7. Zero-copy when possible

```rust
// ✅ Use bytes::Bytes for zero-copy
use bytes::{Bytes, BytesMut};

// ❌ Don't copy payload unnecessarily
let copy = payload.to_vec(); // Avoid!
```

## Commands

### Development

```bash
cargo build               # Build
cargo test                 # Run tests
cargo clippy               # Lint (all warnings must pass!)
cargo fmt                  # Format code
cargo fmt -- --check       # Check formatting (CI)
cargo doc --no-deps        # Generate docs
```

### Full CI Check (before commit)

```bash
cargo fmt -- --check       # Format check
cargo clippy -- -D warnings # Lint (warnings = errors)
cargo test                 # All tests
cargo build --release      # Release build
```

**All four checks MUST pass before considering a task complete.**

### Benchmarks

```bash
cargo bench                # Run all benchmarks
cargo bench -- wire_format # Run specific benchmark
```

### Cross-platform testing

```bash
# Windows (primary dev)
cargo test

# Linux/macOS (CI)
cargo test --target x86_64-unknown-linux-gnu
```

## Codebase Structure

```
src/
├── lib.rs                # Public API re-exports
├── protocol/
│   ├── mod.rs            # Protocol module exports
│   ├── wire_format.rs    # Header encode/decode, flags constants
│   ├── frame_buffer.rs   # BytesMut accumulation, frame extraction
│   └── frame.rs          # Frame struct, typed accessors
├── codec/
│   ├── mod.rs            # Codec trait + exports
│   ├── msgpack.rs        # MsgPackCodec (rmp-serde, to_vec_named!)
│   └── raw.rs            # RawCodec (pass-through, zero-copy)
├── transport/
│   ├── mod.rs            # Transport exports
│   └── pipe.rs           # Unix Socket / Named Pipe listener
├── control/
│   ├── mod.rs            # Control plane exports
│   ├── init.rs           # $init message builder
│   └── stdio.rs          # Stdout writer for control plane
├── handler/
│   ├── mod.rs            # Handler exports
│   ├── registry.rs       # Handler registry, dispatch by method_id
│   └── context.rs        # RequestContext (respond/ack/chunk/end/error)
├── client.rs             # Client builder + runtime loop
├── backpressure.rs       # Write backpressure handling
└── error.rs              # Error types (thiserror)
```

## Dependencies (justified)

| Crate | Purpose | Why |
|-------|---------|-----|
| `tokio` | async runtime, pipe/socket | Required for async I/O |
| `bytes` | BytesMut, zero-copy buffers | Frame buffer performance |
| `serde` + `serde_json` | $init message only | Control plane JSON |
| `rmp-serde` | MsgPack codec | Data plane serialization |
| `tracing` | structured logging | Debug & diagnostics |
| `thiserror` | error types | Ergonomic error handling |

**No unnecessary dependencies.** Every crate must be justified.

## Code Style (Rust)

### Naming

- Modules: `snake_case`
- Types/Traits: `PascalCase`
- Functions/Methods: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Flag constants: use `pub const` in a `flags` module

### Error Handling

- Use `thiserror` for library errors
- All public functions return `Result<T, ProcwireError>`
- Never `unwrap()` or `expect()` in library code (only in tests)
- Use `?` operator for propagation

### Documentation

- All public items must have `///` doc comments
- Include examples in doc comments for key API functions
- Module-level `//!` docs for each module

### Formatting

- `rustfmt` defaults (enforced by CI)
- Max line width: 100 chars
- Group imports: std → external → crate

### Clippy

- All clippy warnings MUST be resolved
- CI runs `cargo clippy -- -D warnings`
- Pay special attention to: `clippy::cast_possible_truncation`, `clippy::cast_sign_loss`

## Platform Support

### Pipe Paths

```rust
#[cfg(unix)]
fn pipe_path(pid: u32, rand: &str) -> String {
    format!("/tmp/procwire-{pid}-{rand}.sock")
}

#[cfg(windows)]
fn pipe_path(pid: u32, rand: &str) -> String {
    format!(r"\\.\pipe\procwire-{pid}-{rand}")
}
```

### Conditional Compilation

- `#[cfg(unix)]` for Unix Domain Socket
- `#[cfg(windows)]` for Named Pipe
- Test on both platforms in CI (GitHub Actions)

## Git Conventions

### Diff for Review

When the user requests a git diff, **always save it to a file**:

```bash
git diff HEAD~1 > task-XX.diff
git diff main..HEAD > feature-branch.diff
```

### Commit Messages

Use conventional commits:

```
feat(protocol): implement wire format encode/decode
fix(codec): use to_vec_named for Node.js compatibility
test(e2e): add echo roundtrip test with Node.js parent
docs: update AGENT_MEMORY with TASK-01 completion
```

## Performance Targets

| Payload | Target Throughput | Target Latency |
|---------|------------------|----------------|
| 1 KB | - | < 100μs RTT |
| 10 MB | > 1 GB/s | - |
| 1 GB | > 1 GB/s | - |

### Optimization Techniques

1. **Header ring buffer** - 16 pre-allocated 11-byte buffers in round-robin
2. **writev/scatter-gather** - header+payload in single syscall
3. **Zero-copy framing** - `bytes::BytesMut`, split without copy
4. **Chunk accumulation** - `Vec<Bytes>` not concat
5. **Backpressure** - `AsyncWriteExt::writable()` before writes

## Interoperability with Node.js

### Type Mapping

| Node.js | Rust |
|---------|------|
| `string` | `String` |
| `number` (int) | `i64` / `u64` |
| `number` (float) | `f64` |
| `boolean` | `bool` |
| `null` / `undefined` | `Option<T>::None` |
| `object` | `HashMap<String, V>` or struct |
| `Array` | `Vec<T>` |
| `Buffer` | `Vec<u8>` / `Bytes` |
| `Date` | MsgPack ext type 2 |

### MsgPack Extension Types

- **Type 1:** Buffer (raw bytes)
- **Type 2:** Date (milliseconds since epoch as f64)

## Testing Strategy

### Levels

1. **Unit** - wire format, frame buffer, codecs (`cargo test`)
2. **Integration** - client ↔ mock parent (`tokio::test`)
3. **E2E** - Rust child ↔ Node.js `@procwire/core` parent
4. **Benchmark** - throughput/latency (Criterion)
5. **Cross-platform** - CI on Linux/macOS/Windows

### E2E Test Flow

```
Node.js script (imports @procwire/core)
  → spawns cargo-built Rust binary
  → sends requests, verifies responses
  → tests streaming, events, abort
  → validates throughput > 1 GB/s
```

## Task Completion Checklist

**IMPORTANT**: Before completing any coding task, ALWAYS run:

```bash
cargo fmt -- --check       # Format check
cargo clippy -- -D warnings # Lint
cargo test                 # All tests
cargo build                # Verify build
```

All checks MUST pass before considering a task complete. Fix any errors before committing.

## 🎯 Summary

> **This crate = Rust client for Procwire v2 IPC protocol. Binary data plane on pipe. JSON only for $init on stdio. MsgPack with to_vec_named. 11-byte BE header. Always handle backpressure. Zero-copy where possible.**
