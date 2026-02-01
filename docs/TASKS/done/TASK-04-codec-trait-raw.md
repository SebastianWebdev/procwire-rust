# TASK-04: Codec Trait + Raw Codec

## Cel

Definiowanie interfejsu codec i implementacja prostego raw codec (pass-through).

## Pliki

- `src/codec/mod.rs`
- `src/codec/raw.rs`

## Zakres

- [ ] Definicja podejścia do codec (trait vs funkcje)
- [ ] Implementacja `RawCodec` (pass-through)
- [ ] Eksporty modułu codec
- [ ] Testy jednostkowe

## Implementacja

### Podejście do Codec

**Rekomendacja:** NIE używamy trait object dla codec. Zamiast tego:
- Handler serializuje sam (wie jaki typ danych)
- RequestContext używa konkretnego codec z konfiguracji metody
- Codec jako prosty moduł z funkcjami serialize/deserialize

### RawCodec

```rust
// src/codec/raw.rs
use bytes::Bytes;

/// Raw codec - pass-through bez transformacji.
/// Akceptuje tylko `&[u8]` lub `Bytes`.
pub struct RawCodec;

impl RawCodec {
    /// Serializuje surowe bajty (zero-copy jeśli to Bytes)
    pub fn serialize(data: &[u8]) -> Bytes {
        Bytes::copy_from_slice(data)
    }

    /// Serializuje Bytes (true zero-copy)
    pub fn serialize_bytes(data: Bytes) -> Bytes {
        data
    }

    /// Deserializuje - zwraca referencję (zero-copy)
    pub fn deserialize(data: &[u8]) -> &[u8] {
        data
    }
}
```

### Eksporty

```rust
// src/codec/mod.rs
mod raw;
mod msgpack;  // placeholder dla TASK-05

pub use raw::RawCodec;
// pub use msgpack::MsgPackCodec;  // w TASK-05
```

## Testy

- [ ] RawCodec round-trip z różnymi danymi
- [ ] Empty buffer
- [ ] Large buffer (1MB)
- [ ] Zero-copy verification

## Definition of Done

- [ ] `cargo test codec` — PASS
- [ ] RawCodec round-trip działa
- [ ] Doc comments

## Kontekst

- RawCodec jest używany gdy payload to już gotowe bajty (np. z FFI)
- Zero-copy jest kluczowe dla wydajności
- W Node.js odpowiednik to `rawCodec` z `@procwire/codecs`
