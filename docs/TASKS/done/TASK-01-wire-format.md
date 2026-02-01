# TASK-01: Wire Format — Header Encode/Decode

## Cel

Implementacja kodowania i dekodowania 11-bajtowego nagłówka binarnego.

## Plik

`src/protocol/wire_format.rs`

## Zakres

- [ ] Zdefiniowanie stałych protokołu
- [ ] Zdefiniowanie modułu `flags` z bitmaskami
- [ ] Implementacja `FrameHeader` struct
- [ ] Implementacja `encode_header()` i `encode_header_into()`
- [ ] Implementacja `decode_header()`
- [ ] Implementacja `validate_header()`
- [ ] Testy jednostkowe

## Implementacja

### 1. Stałe

```rust
pub const HEADER_SIZE: usize = 11;
pub const DEFAULT_MAX_PAYLOAD_SIZE: u32 = 1_073_741_824; // 1 GB
pub const ABSOLUTE_MAX_PAYLOAD_SIZE: u32 = 2_147_483_647; // ~2 GB
pub const ABORT_METHOD_ID: u16 = 0xFFFF;
pub const HEADER_POOL_SIZE: usize = 16;
```

### 2. Moduł Flags

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

### 3. FrameHeader

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct FrameHeader {
    pub method_id: u16,
    pub flags: u8,
    pub request_id: u32,
    pub payload_length: u32,
}
```

### 4. Funkcje encode/decode

```rust
pub fn encode_header(header: &FrameHeader) -> [u8; HEADER_SIZE] { ... }
pub fn encode_header_into(buf: &mut [u8], header: &FrameHeader) { ... }
pub fn decode_header(buf: &[u8]) -> Result<FrameHeader, ProtocolError> { ... }
pub fn validate_header(header: &FrameHeader, max_payload: u32) -> Result<(), ProtocolError> { ... }
```

## Testy

- [ ] Round-trip encode → decode
- [ ] Big Endian byte order verification
- [ ] Minimum/maximum values
- [ ] Method ID 0 rejected
- [ ] Payload size validation
- [ ] Flags bit patterns

## Definition of Done

- [ ] `cargo test protocol::wire_format` — PASS
- [ ] Pełna kompatybilność byte-order z Node.js implementacją
- [ ] Wszystkie publiczne funkcje mają doc comments

## Kontekst

- Wire format jest fundamentem całego protokołu
- Wszystkie liczby są Big Endian (kompatybilność z Node.js)
- Header ma ZAWSZE dokładnie 11 bajtów
