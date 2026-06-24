---
name: procwire-wire-protocol
description: Reference for the Procwire binary data-plane wire format — the 11-byte header, flags bitmask, transaction patterns (request/response/stream/ack/error/event/abort), reserved method IDs, D6 header validation, the frame-buffer state machine, and the writev/backpressure writer. Use when encoding/decoding frames, editing src/protocol/* or src/writer.rs, debugging framing or byte-order bugs, ensuring byte-for-byte compatibility with the Node/Bun parent, or reasoning about flag values and stream termination.
---

# Procwire Wire Protocol (Data Plane)

The **data plane** runs over a Unix domain socket (Linux/macOS) or Named Pipe
(Windows). It carries a **pure binary protocol — ZERO JSON**. This skill is the
authoritative byte-level reference. Source: `src/protocol/wire_format.rs`,
`src/protocol/frame_buffer.rs`, `src/protocol/frame.rs`, `src/writer.rs`.
Cross-check against the parent: `docs/01-PROTOCOL-SPEC.md` §3–§4, §12.

## ⛔ Non-negotiable rules

1. **All multi-byte integers are BIG ENDIAN.** Use `to_be_bytes` / `put_u16` /
   `put_u32` (the `bytes` crate is BE by default). Never `*_le`.
2. **Header is EXACTLY 11 bytes.** Fixed layout, never variable.
3. **Data plane is binary only** — never `serde_json` here; payloads are codec
   output (MsgPack by default) or raw bytes.
4. **STREAM_END frames ALWAYS have an empty payload** (`payloadLength == 0`).
5. **Reserved flag bits 6–7 must be 0** on every frame, in and out.

## Header layout (11 bytes, fixed, Big Endian)

```
Offset  Size  Type        Field           Notes
0       2     uint16 BE   Method ID       1..=65533 normal; 0 reserved; 0xFFFE auth; 0xFFFF abort
2       1     uint8       Flags           bitmask, see below
3       4     uint32 BE   Request ID      0 = event/fire-and-forget
7       4     uint32 BE   Payload Length  bytes of payload that follow
11..    N     bytes       Payload         codec output or raw (0 length allowed)
```

`Header` (`wire_format.rs`) provides `encode()` / `encode_into(buf)` /
`decode(buf) -> Option<Header>` and typed accessors (`is_response()`,
`is_stream()`, `is_stream_end()`, `is_ack()`, `is_error()`, `is_to_parent()`,
`is_event()`, `is_abort()`, `is_auth()`).

## Flags byte (bitmask)

| Bit | Const                 | Hex   | Meaning                                  |
|-----|-----------------------|-------|------------------------------------------|
| 0   | `DIRECTION_TO_PARENT` | 0x01  | 0 = to child, 1 = to parent              |
| 1   | `IS_RESPONSE`         | 0x02  | 0 = request/event, 1 = response          |
| 2   | `IS_ERROR`            | 0x04  | 0 = ok, 1 = error                        |
| 3   | `IS_STREAM`           | 0x08  | 0 = single, 1 = stream chunk             |
| 4   | `STREAM_END`          | 0x10  | 0 = more coming, 1 = final chunk         |
| 5   | `IS_ACK`              | 0x20  | 0 = full response, 1 = ack only          |
| 6–7 | reserved (`0xC0`)     | —     | MUST be 0                                |

Pre-combined constants live in `protocol::flags`:

| Constant               | Value | = bits                                            |
|------------------------|-------|---------------------------------------------------|
| `RESPONSE`             | 0x03  | TO_PARENT \| IS_RESPONSE                           |
| `ERROR_RESPONSE`       | 0x07  | TO_PARENT \| IS_RESPONSE \| IS_ERROR              |
| `STREAM_CHUNK`         | 0x0B  | TO_PARENT \| IS_RESPONSE \| IS_STREAM            |
| `STREAM_END_RESPONSE`  | 0x1B  | TO_PARENT \| IS_RESPONSE \| IS_STREAM \| STREAM_END |
| `ACK_RESPONSE`         | 0x23  | TO_PARENT \| IS_RESPONSE \| IS_ACK              |

## Transaction patterns (exact flag values)

These flag values are wire-contract — they must match the parent exactly.

```
Request  → Result:   request flags=0x00          → response flags=0x03
Request  → ACK:      request flags=0x00          → response flags=0x23
Request  → Stream:   request flags=0x00          → chunks flags=0x0B,
                                                    end flags=0x1B (EMPTY payload!)
Error response:                                     flags=0x07
Event (child→parent):                               flags=0x01, requestId=0
Abort (parent→child):                methodId=0xFFFF, empty payload, requestId=target
Auth (parent→child, first frame):    methodId=0xFFFE, flags=0x00, requestId=0,
                                     payload = raw token bytes (NO codec)
```

Inbound requests from the parent carry `flags=0x00`. The client **ignores any
inbound frame with `IS_RESPONSE` set** (`Client::dispatch_frame`) — it only
handles requests.

## Reserved IDs

| ID       | Meaning                                                          |
|----------|------------------------------------------------------------------|
| method 0 | **reserved — never use; rejected on receive** (`RESERVED_METHOD_ID`) |
| 0xFFFE   | `AUTH_METHOD_ID` — data-plane auth handshake (see `procwire-auth`) |
| 0xFFFF   | `ABORT_METHOD_ID` — cancel an in-flight request                  |
| reqId 0  | event / fire-and-forget (no request/response correlation)        |

**Methods and events have SEPARATE ID spaces.** The registry assigns both
sequentially from 1, so `echo` method id=1 and `progress` event id=1 coexist;
they are disambiguated on the wire by flags (`request 0x00` vs `event 0x01`+`reqId 0`).

## Payload size limits

| Constant                    | Value         | Meaning                          |
|-----------------------------|---------------|----------------------------------|
| `DEFAULT_MAX_PAYLOAD_SIZE`  | 1_073_741_824 | 1 GiB — configurable default     |
| `ABSOLUTE_MAX_PAYLOAD_SIZE` | 2_147_483_647 | ~2 GiB−1 — hard interop cap (Node Buffer / i32) |

`ClientBuilder::max_payload_size(n)` clamps to the absolute cap.

## D6: receive-side header validation

`Header::validate(max_payload_size)` runs on **every parsed frame, before any
payload buffer is allocated** (called from `FrameBuffer::try_extract_one`).
It mirrors the TypeScript `validateHeader`. It rejects:

- `method_id == 0` (reserved) — but `0xFFFE`/`0xFFFF` are allowed,
- `payload_length > max_payload_size` (peer-controlled → DoS/OOM guard),
- `payload_length > ABSOLUTE_MAX_PAYLOAD_SIZE`,
- reserved flag bits (6–7) set.

A validation error ends the read loop and **tears the connection down** —
intentionally, so a malicious/oversized length never triggers a giant alloc.

## Frame buffer: state machine for fragmented reads

`FrameBuffer` (`frame_buffer.rs`) accumulates socket reads in one `BytesMut` and
extracts complete frames. TCP/pipe reads are arbitrary fragments — a frame may
span many reads, or one read may contain many frames. States:

- `WaitingForHeader` — need ≥ 11 bytes; peek+validate header, then consume it.
- `WaitingForPayload { header, remaining }` — need `remaining` more bytes.

API: `push(&[u8]) -> Result<Vec<Frame>>` (extend + extract all ready frames).
Empty payload (`payload_length == 0`) completes the frame immediately in the
header state. Payload extraction is **zero-copy** via `split_to(n).freeze()`.

Gotchas:
- `push` returns `Err` on a D6 violation — the read loop must propagate it
  (drops the connection), not swallow it.
- `try_extract()` is `#[deprecated]`; use `push()`.

## Building & sending frames

- Outbound: `OutboundFrame::new(&header, payload: Bytes)` /
  `OutboundFrame::empty(&header)` (`writer.rs`). The header is encoded once into
  a `[u8; 11]` at construction.
- The dedicated **writer task** (`spawn_writer_task`) owns the write half;
  handlers send via a cloneable `WriterHandle` over an mpsc channel — no
  `Arc<Mutex<Writer>>`, no lock contention.
- Writes use **`write_vectored` (writev)**: header + payload go out as scatter/
  gather slices, batching up to `MAX_BATCH_SIZE = 64` frames per syscall.
  Partial writes are handled by rebuilding remaining slices.
- Test helpers: `build_frame(&header, &payload)` and `build_frame_parts(...)`
  in `protocol::frame` produce raw frame bytes.

## Backpressure (always handle it)

The `WriterHandle` tracks a pending-frame count. `send()` waits (polling every
100µs) until pending `< max_pending_frames` or `backpressure_timeout`
(default 5s) elapses → `ProcwireError::BackpressureTimeout`. `try_send()` fails
immediately at capacity. Never ignore a write result and never buffer unbounded
data — that is an OOM crash.

## Common mistakes

- ❌ Writing `payloadLength` for a STREAM_END frame. It must be 0; use
  `ctx.end()` which sends `OutboundFrame::empty`.
- ❌ Little-endian encoding (`put_u16_le`). Always BE.
- ❌ Treating method/event IDs as a shared space.
- ❌ Sending a normal method on `0` / `0xFFFE` / `0xFFFF`.
- ❌ Allocating the payload before validating the header (re-introduces the DoS
  vector D6 closes).
- ❌ Setting any of bits 6–7 in the flags byte.

## Where to look

| Concern                         | File                              |
|---------------------------------|-----------------------------------|
| Header encode/decode/validate   | `src/protocol/wire_format.rs`     |
| Flags constants & combinations  | `src/protocol/wire_format.rs` (`flags` mod) |
| Frame struct & accessors        | `src/protocol/frame.rs`           |
| Fragment reassembly             | `src/protocol/frame_buffer.rs`    |
| Header ring buffer (zero-alloc) | `src/protocol/header_pool.rs`     |
| Writer task / writev / batching | `src/writer.rs`                   |
| Read loop & dispatch            | `src/client.rs` (`read_loop`, `dispatch_frame`) |
