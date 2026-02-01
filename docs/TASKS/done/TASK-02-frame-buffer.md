# TASK-02: Frame Buffer — Akumulacja Frames z Chunków

## Cel

Bufor akumulujący bajty z socketa i wyciągający kompletne ramki.

## Plik

`src/protocol/frame_buffer.rs`

## Zakres

- [ ] Implementacja `FrameBuffer` struct z `bytes::BytesMut`
- [ ] Maszyna stanowa (WaitingForHeader / WaitingForPayload)
- [ ] Metoda `push(data: &[u8]) -> Vec<Frame>`
- [ ] Obsługa wielu frames w jednym push (pipeline)
- [ ] Obsługa partial header/payload (fragmentation)
- [ ] Testy jednostkowe

## Implementacja

### Stany maszyny stanowej

```rust
enum FrameBufferState {
    WaitingForHeader,
    WaitingForPayload { header: FrameHeader, remaining: usize },
}
```

### Struktura

```rust
pub struct FrameBuffer {
    buffer: BytesMut,
    state: FrameBufferState,
    max_payload_size: u32,
}

impl FrameBuffer {
    pub fn new() -> Self { ... }
    pub fn with_max_payload(max: u32) -> Self { ... }
    pub fn push(&mut self, data: &[u8]) -> Vec<Frame> { ... }
}
```

### Kluczowe zasady

1. **NIGDY** nie rób `copy` w pętli — użyj `BytesMut::extend_from_slice` raz, potem `split_to`
2. Payload przechowywany jako `Bytes` (frozen, zero-copy sharing)
3. Frame buffer jest restowalny — po `push()` wewnętrzny stan jest konsystentny

## Testy

- [ ] Single complete frame
- [ ] Multiple frames in one push
- [ ] Fragmented header (partial header, then rest)
- [ ] Fragmented payload (header complete, payload in 2 pushes)
- [ ] Large payload (1MB)
- [ ] Empty payload (0 bytes)
- [ ] Max payload validation

## Definition of Done

- [ ] `cargo test protocol::frame_buffer` — PASS
- [ ] Żadne alokacje w hot path (poza `BytesMut::extend_from_slice`)
- [ ] Poprawne odzyskiwanie po fragmentation

## Kontekst

- FrameBuffer jest używany w głównej pętli odczytu z socketa
- Dane przychodzą w fragmentach (chunki ~64KB z OS)
- Jeden push może zawierać wiele kompletnych ramek (pipelining)
