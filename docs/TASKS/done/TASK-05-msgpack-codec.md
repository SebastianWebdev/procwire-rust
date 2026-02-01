# TASK-05: MsgPack Codec

## Cel

MsgPack serializacja/deserializacja kompatybilna z Node.js `@msgpack/msgpack`.

## Plik

`src/codec/msgpack.rs`

## Zakres

- [ ] Implementacja serialize z `rmp_serde::to_vec_named`
- [ ] Implementacja deserialize z `rmp_serde::from_slice`
- [ ] Testy jednostkowe
- [ ] Testy interoperacyjności z Node.js

## Implementacja

```rust
// src/codec/msgpack.rs
use serde::{Serialize, Deserialize};

use crate::error::CodecError;

/// MsgPack codec kompatybilny z Node.js @msgpack/msgpack.
pub struct MsgPackCodec;

impl MsgPackCodec {
    /// Serializuje dane do MsgPack.
    ///
    /// KRYTYCZNE: Używa `to_vec_named` (nie `to_vec`!)
    /// - `to_vec` serializuje struct fields jako array (pozycyjne)
    /// - `to_vec_named` serializuje jako map (name → value)
    /// Node.js oczekuje formatu map!
    pub fn serialize<T: Serialize>(data: &T) -> Result<Vec<u8>, CodecError> {
        rmp_serde::to_vec_named(data)
            .map_err(|e| CodecError::Serialize(e.to_string()))
    }

    /// Deserializuje dane z MsgPack.
    pub fn deserialize<'de, T: Deserialize<'de>>(data: &'de [u8]) -> Result<T, CodecError> {
        rmp_serde::from_slice(data)
            .map_err(|e| CodecError::Deserialize(e.to_string()))
    }
}
```

## Testy

- [ ] Round-trip: simple object `{ name: "test", value: 42 }`
- [ ] Round-trip: nested objects
- [ ] Round-trip: arrays / Vec
- [ ] Round-trip: null / Option::None
- [ ] Round-trip: numbers (i64, u64, f64)
- [ ] Interop: deserializacja payloadu wygenerowanego przez Node.js (hardcoded bytes)
- [ ] Verify: `to_vec_named` produces map format (nie array)

### Test interoperacyjności

```rust
#[test]
fn test_nodejs_interop() {
    // Payload wygenerowany przez Node.js:
    // @msgpack/msgpack encode({ name: "test", value: 42 })
    let nodejs_payload = vec![0x82, 0xa4, 0x6e, 0x61, 0x6d, 0x65, 0xa4, 0x74, 0x65, 0x73, 0x74, 0xa5, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x2a];

    #[derive(Deserialize, PartialEq, Debug)]
    struct TestData { name: String, value: i32 }

    let result: TestData = MsgPackCodec::deserialize(&nodejs_payload).unwrap();
    assert_eq!(result, TestData { name: "test".to_string(), value: 42 });
}
```

## Definition of Done

- [ ] `cargo test codec::msgpack` — PASS
- [ ] Serializacja kompatybilna z Node.js `@msgpack/msgpack encode()`
- [ ] Deserializacja kompatybilna z Node.js `@msgpack/msgpack decode()`

## Kontekst

- MsgPack jest domyślnym codec w Procwire
- **KRYTYCZNE:** `to_vec_named` NIGDY `to_vec`!
- Node.js używa `@msgpack/msgpack` który oczekuje struct-as-map
- Jeśli użyjesz `to_vec`, Node.js nie zdeserializuje danych poprawnie
