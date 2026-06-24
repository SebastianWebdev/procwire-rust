---
name: procwire-auth
description: Reference for the optional Procwire data-plane AUTH handshake (§11). Covers the AUTH frame format (method id 0xFFFE, raw token bytes, NO codec), the PROCWIRE_TOKEN env var vs explicit ClientBuilder::auth_token precedence, constant-time token comparison, the "first frame must be AUTH or the connection is dropped" pending-adoption flow, dispatch of frames pipelined behind AUTH, the stray-AUTH-on-adopted-connection no-op, and why the writer task starts only post-adoption. Use when working on authentication, run_auth_handshake, token handling, or interop with an auth:true parent.
---

# Procwire Data-Plane Authentication (AUTH)

Optional, off by default. When the parent runs `spawnPolicy({ auth: true })` it
authenticates the data-plane connection so a rogue local process cannot connect
to the child's socket first. Source: `src/client.rs`
(`run_auth_handshake`, `constant_time_eq`, `AuthOutcome`),
`src/protocol/wire_format.rs` (`AUTH_METHOD_ID`). Spec:
`docs/01-PROTOCOL-SPEC.md` §11.

## How the parent drives it

1. Generates a per-spawn token: `randomBytes(32).toString("hex")` → a **64-char
   lowercase-hex** string.
2. Passes it to the child via the **`PROCWIRE_TOKEN`** environment variable.
3. Sends it as the **first data-plane frame** — the AUTH frame — immediately
   after connecting, before any request/stream.

## AUTH frame format

Standard 11-byte header; payload is the **raw token bytes — NOT codec-encoded**:

```
methodId      = 0xFFFE   (AUTH_METHOD_ID)
flags         = 0x00     (to child, not a response; reserved bits 0)
requestId     = 0
payloadLength = token length in bytes
payload       = token, raw UTF-8 bytes (NO MsgPack)
```

`AUTH_METHOD_ID` (`0xFFFE`) passes D6 header validation just like
`ABORT_METHOD_ID` (`0xFFFF`); only method id `0` is rejected.

## Token source & precedence (`ClientBuilder::start`)

```
explicit ClientBuilder::auth_token(...)   ─┐ (wins if set)
                                           ├─► take first present, then
PROCWIRE_TOKEN env var                     ─┘ filter out empty → Option<String>
```

- Explicit `auth_token` **overrides** `PROCWIRE_TOKEN`.
- An **empty** value (explicit or env) is treated as *absent* → auth disabled.
- `Some(token)` ⇒ auth enabled; `None` ⇒ auth disabled (adopt on accept,
  backward-compatible with non-auth parents).

```rust
// Usually nothing to do — PROCWIRE_TOKEN is read automatically:
let client = ClientBuilder::new().handle("echo", h).start().await?;

// Or pin it explicitly (explicit wins over the env var):
let client = ClientBuilder::new().auth_token("…").handle("echo", h).start().await?;
```

## The pending-adoption handshake (`run_auth_handshake`)

When auth is enabled, an accepted connection is **pending**, not adopted. The
client reads until the first complete frame, then:

- **first frame is AUTH (`0xFFFE`) AND payload == token (constant-time)** →
  `AuthOutcome::Adopted { frame_buffer, pending_frames }`. Any frames pipelined
  after AUTH in the same packet are returned as `pending_frames` and dispatched
  first by the read loop.
- **anything else** — non-AUTH first frame, token mismatch, oversized/invalid
  frame, read error, or EOF before the AUTH frame → `AuthOutcome::Rejected`.
  The connection is **dropped** and the listener keeps waiting for the real
  parent (it does *not* fail the whole client).

So a wrong token doesn't error the worker — it just refuses that connection and
loops back to `accept()`.

## Constant-time comparison

`constant_time_eq(a, b)` returns `false` immediately on length mismatch (length
is not secret), then XOR-accumulates every byte without short-circuiting, so the
compare time doesn't leak how many leading bytes matched. This mirrors the Node
client's `crypto.timingSafeEqual`. **Never replace it with `a == b`** for the
token check — that reintroduces a timing side channel.

## Two safety invariants

- **The writer task is spawned only AFTER adoption.** No child→parent frame is
  ever written to an unauthenticated peer. Keep it that way — don't move writer
  setup before the handshake.
- **A stray AUTH frame on an already-adopted connection is a no-op**
  (`dispatch_frame` logs and ignores it). Auth is a one-time gate before
  adoption, never a regular method.

## Common mistakes

- ❌ MsgPack-encoding the token. The AUTH payload is **raw bytes**.
- ❌ Using `==` instead of `constant_time_eq`.
- ❌ Treating a rejected connection as a fatal error instead of dropping it and
  re-accepting.
- ❌ Spawning the writer / sending anything before adoption.
- ❌ Forgetting that empty `PROCWIRE_TOKEN` means *disabled*, not *empty token*.
- ❌ Assuming auth is on — it is **opt-in**; non-auth parents adopt on accept.

## Where to look

| Concern                              | File                                        |
|--------------------------------------|---------------------------------------------|
| Handshake / outcome / token precedence | `src/client.rs` (`run_auth_handshake`, `start`) |
| Constant-time compare                | `src/client.rs` (`constant_time_eq`)        |
| `AUTH_METHOD_ID` + validation        | `src/protocol/wire_format.rs`               |
| Wire-format test of an AUTH frame    | `tests/integration.rs` (`test_auth_frame_wire_format`) |
