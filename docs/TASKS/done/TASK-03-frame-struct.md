# TASK-03: Frame Struct i Protocol Exports

## Cel

Definicja Frame struct i publiczny API modułu protocol.

## Pliki

- `src/protocol/frame.rs`
- `src/protocol/mod.rs`

## Zakres

- [ ] Implementacja `Frame` struct
- [ ] Frame builder functions
- [ ] Eksporty modułu protocol
- [ ] Testy jednostkowe

## Implementacja

### Frame struct

```rust
pub struct Frame {
    pub header: FrameHeader,
    payload: Bytes,  // frozen, zero-copy
}

impl Frame {
    pub fn new(header: FrameHeader, payload: Bytes) -> Self { ... }
    pub fn payload(&self) -> &[u8] { &self.payload }
    pub fn payload_bytes(&self) -> Bytes { self.payload.clone() } // cheap clone
    pub fn payload_length(&self) -> usize { self.payload.len() }

    // Convenience methods
    pub fn method_id(&self) -> u16 { self.header.method_id }
    pub fn flags(&self) -> u8 { self.header.flags }
    pub fn request_id(&self) -> u32 { self.header.request_id }

    // Flag checks
    pub fn is_response(&self) -> bool { ... }
    pub fn is_error(&self) -> bool { ... }
    pub fn is_stream(&self) -> bool { ... }
    pub fn is_stream_end(&self) -> bool { ... }
    pub fn is_ack(&self) -> bool { ... }
    pub fn is_to_parent(&self) -> bool { ... }
}
```

### Frame builder

```rust
pub fn build_frame(header: FrameHeader, payload: &[u8]) -> Vec<u8> { ... }
pub fn build_frame_parts(header: FrameHeader, payload: &[u8]) -> ([u8; HEADER_SIZE], &[u8]) { ... }
```

### Eksporty w mod.rs

```rust
// src/protocol/mod.rs
mod wire_format;
mod frame_buffer;
mod frame;

pub use wire_format::{
    FrameHeader,
    encode_header,
    encode_header_into,
    decode_header,
    validate_header,
    flags,
    HEADER_SIZE,
    DEFAULT_MAX_PAYLOAD_SIZE,
    ABSOLUTE_MAX_PAYLOAD_SIZE,
    ABORT_METHOD_ID,
    HEADER_POOL_SIZE,
};

pub use frame_buffer::FrameBuffer;
pub use frame::{Frame, build_frame, build_frame_parts};
```

## Testy

- [ ] Frame creation z różnymi payloadami
- [ ] Flag convenience methods
- [ ] build_frame round-trip (build → parse)
- [ ] Zero-copy verification (Bytes::clone nie kopiuje danych)

## Definition of Done

- [ ] `cargo test protocol` — ALL PASS
- [ ] Public API eksportuje wszystkie potrzebne typy
- [ ] Doc comments dla wszystkich publicznych elementów

## Kontekst

- Frame jest główną jednostką danych w protokole
- `Bytes` zapewnia zero-copy sharing payloadu
- Moduł protocol jest fundamentem dla całego crate
