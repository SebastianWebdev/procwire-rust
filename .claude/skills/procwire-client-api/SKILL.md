---
name: procwire-client-api
description: How to build a Procwire Rust worker — the ClientBuilder fluent API, the three handler kinds (handle/handle_stream/handle_ack), the exact handler closure signature and trait bounds, the RequestContext response methods (respond/ack/chunk/end/error and their raw/bytes variants), emitting events, cancellation/abort via CancellationToken, builder configuration knobs, and the wait_for_shutdown lifecycle. Use when adding or modifying a method/event handler, wiring up a worker's main(), sharing state into handlers, or implementing cancellable long-running work.
---

# Procwire Client API (writing a worker)

A worker registers handlers with `ClientBuilder`, calls `.start()`, then awaits
`wait_for_shutdown()`. Source: `src/client.rs`, `src/handler/context.rs`,
`src/handler/registry.rs`. Runnable examples: `examples/echo.rs`,
`examples/stream.rs`, `examples/events.rs`.

## Minimal worker

```rust
use procwire_client::{Client, RequestContext};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)] struct In  { message: String }
#[derive(Serialize)]   struct Out { echo: String }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .handle("echo", |data: In, ctx: RequestContext| async move {
            ctx.respond(&Out { echo: data.message }).await
        })
        .start()
        .await?;
    client.wait_for_shutdown().await?;
    Ok(())
}
```

`Client::builder()` and `ClientBuilder::new()` are equivalent.

## The three handler kinds — match the registration to the response

The handler kind sets the method's `ResponseType` announced in `$init`, and the
parent expects exactly that shape. Use the matching `ctx` method to reply:

| Builder method   | ResponseType | Reply with                              |
|------------------|--------------|-----------------------------------------|
| `.handle`        | `result`     | `ctx.respond(&out)` (exactly once)      |
| `.handle_stream` | `stream`     | `ctx.chunk(&c)*` then `ctx.end()`       |
| `.handle_ack`    | `ack`        | `ctx.ack()`                             |
| `.event(name)`   | (event)      | `client.emit(name, &data)`              |

Any handler may also reply with `ctx.error(...)` instead.

## Handler closure signature (exact)

```rust
F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static
T: DeserializeOwned + Send + 'static     // payload, deserialized for you (MsgPack)
Fut: Future<Output = Result<(), ProcwireError>> + Send + 'static
```

Key points / gotchas:
- **Argument order is `(payload, ctx)`** — payload first, context second.
  (The README's cancellation snippet showing `.method(|ctx, payload|)` is stale;
  the real API is `.handle*` with `|payload, ctx|`.)
- The closure is **`Fn`** (callable many times, concurrently) and must be
  `Send + Sync + 'static`; the returned future `Send + 'static`. So you cannot
  capture and mutate plain state — clone what you need per call (e.g. an
  `mpsc::Sender`, an `Arc<...>`) **inside** the closure before `async move`.
- Use `()` as `T` for methods that take no payload (`|_: (), ctx| ...`).
- A handler that returns `Err(...)` is logged **and**, if no terminal response
  was sent yet, the runtime auto-sends an error frame with the error's message
  (parity with the Node client). `ctx.error(...)` is still the way to control
  the message; an unknown method id gets the same auto-error treatment.

Sharing state into a handler (pattern from `examples/events.rs`):

```rust
let (tx, mut rx) = tokio::sync::mpsc::channel::<u32>(10);
let client = Client::builder()
    .handle_ack("start", move |data: WorkInput, ctx| {
        let tx = tx.clone();                 // clone per registration / call
        async move { ctx.ack().await?; tx.send(data.steps).await.ok(); Ok(()) }
    })
    .event("progress")
    .start().await?;
```

## RequestContext — replying to the parent

`RequestContext` is `Clone` and cheap to share across tasks. Methods:

| Method                       | Frame flags | Purpose                                  |
|------------------------------|-------------|------------------------------------------|
| `respond(&T)`                | 0x03        | single MsgPack result                    |
| `respond_raw(&[u8])`         | 0x03        | result, raw bytes (copies)               |
| `respond_bytes(Bytes)`       | 0x03        | result, zero-copy                        |
| `ack()`                      | 0x23        | acknowledgment, empty payload            |
| `chunk(&T)`                  | 0x0B        | one stream chunk (MsgPack)               |
| `chunk_raw(&[u8])` / `chunk_bytes(Bytes)` | 0x0B | stream chunk, raw / zero-copy     |
| `end()`                      | 0x1B        | **stream terminator, empty payload**     |
| `error(&str)`                | 0x07 / 0x0F | error, string reason (stream methods get IS_STREAM → 0x0F) |
| `error_with(&T)`             | 0x07 / 0x0F | error, structured (`{message, code}`)    |

Rules:
- **A `stream` handler must finish with `ctx.end()`** — it sends an empty
  STREAM_END frame; without it the parent's async iterator never completes.
- Send exactly one terminal response per request (one `respond`/`ack`/`error`,
  or a chunk sequence ending in `end`). Terminal methods are guarded: a second
  `respond`/`ack`/`end`/`error` returns `ProcwireError::ResponseAlreadySent`
  (shared across `clone()`s of the context; `chunk` is not terminal).
- The error frame is terminal on its own: after `ctx.error(...)` do **not**
  call `ctx.end()` — a stream ends with either STREAM_END or an error frame,
  never both. Error payloads are always fixed MsgPack, regardless of the
  method's data codec.
- `respond_bytes` / `chunk_bytes` take `bytes::Bytes` for zero-copy hot paths.

## Events (fire-and-forget, child → parent)

```rust
client.emit("progress", &ProgressEvent { percent: 50 }).await?;  // MsgPack
client.emit_raw("progress", &raw_bytes).await?;                  // raw bytes
```

- The event **must be registered** with `.event("progress")` first, or `emit`
  returns `ProcwireError::Protocol("Unknown event: …")`.
- Events use `requestId = 0` and `flags = 0x01`; no response is expected.
- `emit` lives on `Client`, not `RequestContext`. To emit from inside a handler,
  send the data out to a task that owns the `Client` (see `examples/events.rs`),
  or share the client via `Arc`.

## Cancellation / ABORT

When the parent sends an ABORT (`methodId 0xFFFF`, target `requestId`), the
client cancels that request's `CancellationToken`. Handlers must cooperate:

```rust
.handle_stream("work", |n: u32, ctx| async move {
    for i in 0..n {
        if ctx.is_cancelled() { return Ok(()); }   // polling check
        ctx.chunk(&i).await?;
    }
    ctx.end().await
})

// or react immediately with select!:
tokio::select! {
    _ = ctx.cancelled() => Ok(()),                  // aborted: stop cleanly
    out = do_work()      => ctx.respond(&out).await,
}
```

`ctx.cancellation_token()` hands the token to child tasks. After a handler
completes, its context is removed from the active map; an ABORT for an
unknown/finished request is just logged.

## Builder configuration

| Method                              | Default | Effect                                            |
|-------------------------------------|---------|---------------------------------------------------|
| `.max_concurrent_handlers(n)`       | 256     | semaphore cap; **excess requests are DROPPED** with a warning (not queued) |
| `.max_pending_frames(n)`            | 1024    | outbound backpressure threshold                   |
| `.channel_capacity(n)`              | 1024    | writer mpsc channel size                          |
| `.backpressure_timeout(Duration)`   | 5s      | how long `send()` waits before `BackpressureTimeout` |
| `.max_payload_size(u32)`            | 1 GiB   | inbound frame cap (clamped to ~2 GiB−1)           |
| `.auth_token(impl Into<String>)`    | env     | data-plane auth (see `procwire-auth`)             |

**Concurrency gotcha:** at `max_concurrent_handlers`, new requests are dropped,
not queued. A handler that blocks (holds its permit) for a long time can starve
others. Keep handlers non-blocking; offload CPU-bound work to
`tokio::task::spawn_blocking` and bump the limit if you legitimately need more
in-flight requests.

## Lifecycle

- `.start().await` → binds the pipe, sends `$init`, spawns the control-plane
  reader, accepts (and optionally authenticates) the parent, starts the read
  loop, returns a running `Client`.
- `client.wait_for_shutdown().await` resolves when **either** the data plane
  closes (EOF / parent kill / read error) **or** a `$shutdown` arrives on the
  control plane. Returning lets `main` exit promptly (graceful teardown).
- Introspection: `is_shutdown_requested()`, `is_backpressure_active()`,
  `pending_frames()`.

## Where to look

| Concern                           | File                                |
|-----------------------------------|-------------------------------------|
| Builder + runtime + dispatch      | `src/client.rs`                     |
| RequestContext (all reply methods)| `src/handler/context.rs`            |
| Registry / ID assignment / dispatch | `src/handler/registry.rs`         |
| End-to-end examples               | `examples/{echo,stream,events}.rs`  |
