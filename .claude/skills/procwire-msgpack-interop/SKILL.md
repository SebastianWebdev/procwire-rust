---
name: procwire-msgpack-interop
description: Reference for serializing data-plane payloads with MsgPack and staying interoperable with the Node.js/Bun parent (@msgpack/msgpack). Covers the critical to_vec_named (struct-as-map) rule and why to_vec breaks Node, the Node↔Rust type mapping table, MsgPack extension types (Buffer=ext 1, Date=ext 2), null/Option handling, Buffer via serde_bytes, and the RawCodec zero-copy pass-through. Use when adding/changing handler payload types, debugging "Node can't decode this" / deserialization failures, or choosing between MsgPack and raw bytes.
---

# Procwire Codecs & Node.js Interop

Data-plane payloads are serialized by a **codec**. Default is **MessagePack**
(`MsgPackCodec`, via `rmp-serde`); `RawCodec` is a zero-copy pass-through for
already-encoded or binary data. Source: `src/codec/msgpack.rs`,
`src/codec/raw.rs`. Spec: `docs/01-PROTOCOL-SPEC.md` §9, CLAUDE.md "Interoperability".

## 🔴 THE rule: `to_vec_named`, never `to_vec`

```rust
// ✅ correct — structs become MsgPack MAPS (field name → value)
rmp_serde::to_vec_named(&value)   // MsgPackCodec::encode does exactly this

// ❌ WRONG — structs become MsgPack ARRAYS (positional)
rmp_serde::to_vec(&value)
```

**Why it matters:** Node's `@msgpack/msgpack` decodes objects from MsgPack
**maps**. `to_vec` emits **arrays**, so the parent receives a positional array
instead of `{ field: value }` and either errors or hands the user garbage.
`MsgPackCodec::encode` already uses `to_vec_named` — always go through it; never
call `rmp_serde` directly with `to_vec`.

Detecting the format from the first byte (used in tests):
- map: high nibble `0x8X` (e.g. `0x82` = 2-field fixmap)
- array: high nibble `0x9X` (e.g. `0x93` = 3-element fixarray)
- `(encoded[0] & 0xF0) == 0x80` ⇒ map (good)

## The codec API

```rust
use procwire_client::codec::MsgPackCodec;

let bytes: Vec<u8>      = MsgPackCodec::encode(&value)?;      // T: Serialize
let value: T            = MsgPackCodec::decode(&bytes)?;      // T: DeserializeOwned
```

Handlers normally never call this directly: the registry decodes the inbound
payload into your handler's `T`, and `ctx.respond(&out)` / `ctx.chunk(&c)` /
`ctx.error(msg)` encode for you. Reach for `MsgPackCodec` directly only in
tests or custom framing.

## Node.js ↔ Rust type mapping

| Node.js                 | Rust                                   | Notes                              |
|-------------------------|----------------------------------------|------------------------------------|
| `string`                | `String` / `&str`                      |                                    |
| `number` (integer)      | `i64` / `u64` (or sized `i32`/`u32`…)  | pick a type that fits the range    |
| `number` (float)        | `f64`                                  |                                    |
| `boolean`               | `bool`                                 |                                    |
| `null` / `undefined`    | `Option<T>::None`                      | encodes as MsgPack nil `0xc0`      |
| `object`                | `struct` (derive Serde) or `HashMap`   | **must be struct-as-map**          |
| `Array`                 | `Vec<T>`                               | array `0x9X` — this is correct     |
| `Buffer` / binary       | `Vec<u8>` / `bytes::Bytes`             | use `serde_bytes` for `bin` format |
| `Date`                  | MsgPack **ext type 2** (f64 ms epoch)  |                                    |

## Extension types (defined by the Node parent)

- **Ext type 1: Buffer** — raw bytes.
- **Ext type 2: Date** — milliseconds since epoch as `f64`.

If you exchange raw binary blobs as a field, use `serde_bytes` so it encodes as
MsgPack `bin` (`0xc4` bin8 …) rather than an array of integers:

```rust
#[derive(Serialize, Deserialize)]
struct Frame {
    #[serde(with = "serde_bytes")]
    pixels: Vec<u8>,
}
```

(`serde_bytes` is a dev-dependency used in tests; add it as a normal dependency
if a public payload type needs `bin` encoding.)

## null / Option

- Node `null`/`undefined` → `Option<T>::None` → MsgPack nil (`0xc0`).
- A missing optional field deserializes to `None` cleanly. Prefer `Option<T>`
  for any field the parent may omit; otherwise decode fails on absence.

## Error payload shape

`ctx.error("msg")` encodes a plain string — the parent surfaces it as the
rejection reason. `ctx.error_with(&value)` encodes any `Serialize` value; the
parent reads a string `message` field if present and preserves the whole object
on `error.data`. Prefer `{ "message": "...", "code": ... }`:

```rust
#[derive(Serialize)]
struct ApiError { message: String, code: u32 }
ctx.error_with(&ApiError { message: "not found".into(), code: 404 }).await?;
```

## RawCodec — zero-copy pass-through

`RawCodec` (`src/codec/raw.rs`) performs no serialization — use it when the
payload is already encoded or is opaque binary, and pair it with the context's
`*_raw` / `*_bytes` methods to avoid copies:

```rust
RawCodec::serialize(&data)         // &[u8]  -> Bytes (copies)
RawCodec::serialize_bytes(bytes)   // Bytes  -> Bytes (true zero-copy, same alloc)
RawCodec::deserialize(&bytes)      // &[u8]  -> &[u8]  (zero-copy view)
// on RequestContext: respond_raw / respond_bytes / chunk_raw / chunk_bytes
```

## Common mistakes

- ❌ `rmp_serde::to_vec(...)` anywhere — breaks Node decoding silently.
- ❌ Using `serde_json` on the data plane. JSON is **only** for `$init`/control.
- ❌ Encoding a `Vec<u8>` payload as a normal `Vec` (array of ints) when the
  parent expects a `Buffer` — use `serde_bytes`.
- ❌ A required field that the parent sometimes omits — make it `Option<T>`.
- ❌ Integer type too small for the parent's `number` range → decode error.

## Where to look

| Concern                       | File                                   |
|-------------------------------|----------------------------------------|
| MsgPack encode/decode + WHY   | `src/codec/msgpack.rs`                 |
| Raw / zero-copy codec         | `src/codec/raw.rs`                     |
| Interop tests (byte shapes)   | `src/codec/msgpack.rs` (`#[cfg(test)]`)|
| Type mapping reference        | `CLAUDE.md`, `docs/01-PROTOCOL-SPEC.md` §9 |
