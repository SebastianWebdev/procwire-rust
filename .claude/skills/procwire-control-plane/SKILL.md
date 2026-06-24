---
name: procwire-control-plane
description: Reference for the Procwire control plane (stdio JSON-RPC) and connection lifecycle — the $init handshake message and schema, the $ping/$pong heartbeat, graceful $shutdown, the strict "stdout is JSON-only, logs go to stderr" discipline, the dedicated-OS-thread stdin reader, and platform-specific pipe-path generation (XDG_RUNTIME_DIR/TMPDIR/tmp on Unix, \\.\pipe on Windows). Use when editing src/control/* or src/transport/*, debugging handshake/heartbeat/shutdown, or anything about how the parent and child establish and tear down the connection.
---

# Procwire Control Plane & Lifecycle

The **control plane** is newline-delimited **JSON-RPC 2.0 over stdio**. It is the
*only* place JSON is allowed; the data plane is binary. It carries just the
handshake and liveness/shutdown signals — never user data. Source:
`src/control/{init,stdio,stdin,mod}.rs`, `src/transport/pipe.rs`, lifecycle in
`src/client.rs`. Spec: `docs/01-PROTOCOL-SPEC.md` §2, §5, §10.

## 🔴 The stdout discipline

- **stdout carries ONLY control-plane JSON** (one compact object per line, `\n`
  terminated). The first line is `$init`; later lines are `$pong` replies.
- **All logging goes to stderr.** The parent inherits the child's stderr.
  `tracing` writes to stderr by default — keep it that way. A stray `println!`
  or non-JSON byte on stdout corrupts the control stream.
- The reader **ignores any line not starting with `{`** and ignores unknown
  methods, so stray output is *usually* tolerated — but never rely on that.

## Lifecycle (order matters)

```
1. bind the pipe listener            (MUST happen before $init is announced,
                                       so the parent's connect finds a server)
2. write $init to stdout             (announces pipe path + schema + version)
3. spawn control-plane stdin reader  (heartbeat + shutdown, on an OS thread)
4. accept (+ optionally authenticate) the parent connection
5. spawn writer task, run read loop  (binary data plane, full-duplex)
6. shutdown: data-plane EOF OR $shutdown → wait_for_shutdown() resolves → exit
```

`ClientBuilder::start()` performs steps 1–5; `wait_for_shutdown()` is step 6.

## `$init` — the handshake (child → parent, stdout, one line)

```json
{
  "jsonrpc": "2.0",
  "method": "$init",
  "params": {
    "pipe": "/run/user/1000/procwire-12345-a1b2c3d4.sock",
    "schema": {
      "methods": { "echo": { "id": 1, "response": "result" } },
      "events":  { "progress": { "id": 1 } }
    },
    "version": "1.0.0"
  }
}
```

- Built by `build_init_message(pipe_path, &schema)` from the registry's
  `build_schema()`. You normally never hand-build it.
- `params.version` is **`"1.0.0"`** (`PROTOCOL_VERSION`). Procwire dropped the
  "v2" framing; the wire protocol is de facto v1. The parent does not gate the
  handshake on this string, but keep it matching the Node/Bun client.
- `response` ∈ `result` | `stream` | `ack` | `none` (serialized lowercase via
  the `ResponseType` enum).
- Method IDs and event IDs are assigned by the **child**, sequentially from 1,
  in **separate** spaces. The parent builds name↔id lookups from this schema.
- `$error` (`{ "method": "$error", "params": { "message": ... } }`) is the
  optional "I couldn't initialize" message a child may send instead.

## `$ping` / `$pong` heartbeat (REQUIRED)

A parent with `spawnPolicy({ heartbeat: { intervalMs, timeoutMs } })` sends
`{"jsonrpc":"2.0","method":"$ping"}` on the child's **stdin** and expects
`{"jsonrpc":"2.0","method":"$pong"}` on **stdout** within `timeoutMs`. Miss it
and the parent kills the child as dead.

The client answers automatically in `run_control_reader` (`control/stdin.rs`).
`PONG_MESSAGE` is the exact reply constant. **Do not block stdin handling** with
heavy work — the heartbeat must stay responsive.

## `$shutdown` — graceful stop

`{"jsonrpc":"2.0","method":"$shutdown","params":{}}` on stdin cancels the
shutdown token, which makes `wait_for_shutdown()` resolve so `main` returns and
the process exits promptly — instead of waiting for the parent's force-kill
grace period (~5s). `client.is_shutdown_requested()` reflects this.

## The stdin reader runs on a dedicated OS thread

`run_control_reader` does **blocking** `read_line` on stdin and is spawned on a
plain `std::thread` (not the Tokio blocking pool). That guarantees the blocking
read can never keep the async runtime or the process alive: once `main` returns,
the abandoned read dies with the process. Parsing (`parse_control_line`):
- line not starting with `{` (after trim) → ignore,
- malformed JSON → ignore,
- `method == "$ping"` → reply `$pong`,
- `method == "$shutdown"` → cancel shutdown token, stop,
- anything else → ignore.

## Pipe path generation (`transport/pipe.rs`)

```
Unix:    {XDG_RUNTIME_DIR | TMPDIR | /tmp}/procwire-{pid}-{rand:x}.sock
Windows: \\.\pipe\procwire-{pid}-{rand:x}
```

- Unix base dir preference: `XDG_RUNTIME_DIR` (per-user, mode 0700 on systemd) →
  `TMPDIR` → `/tmp`. Preferring a private runtime dir over world-writable `/tmp`
  is defense-in-depth for the data plane (alongside optional AUTH).
- `rand_u64()` mixes wall-clock nanos, PID, and a process-wide atomic counter.
  The **counter** is what guarantees uniqueness even when the clock is coarse
  (macOS µs, Windows ~100ns–15ms) and two calls land in the same tick.
- Unix: the listener `bind` removes any stale socket file first, and the socket
  file is unlinked on `Drop`. Windows Named Pipes auto-clean.
- The listener accepts a **single** parent connection.

## Common mistakes

- ❌ `println!`/`print!`/`dbg!` (writes to stdout) → corrupts the control stream.
  Use `tracing::*` / `eprintln!` (stderr).
- ❌ Announcing `$init` before the pipe listener is bound (race: parent connects
  to nothing).
- ❌ Doing slow work in the stdin reader → missed heartbeat → parent kills you.
- ❌ Changing `version` away from `"1.0.0"` without matching the parent.
- ❌ Putting any non-`$init` user data through stdio. Control plane is tiny and
  rare; throughput belongs on the binary pipe.

## Where to look

| Concern                          | File                          |
|----------------------------------|-------------------------------|
| `$init` builder + schema + version | `src/control/init.rs`       |
| stdout writer                    | `src/control/stdio.rs`        |
| stdin reader (ping/pong/shutdown)| `src/control/stdin.rs`        |
| pipe path + listener (Unix/Win)  | `src/transport/pipe.rs`       |
| lifecycle wiring                 | `src/client.rs` (`start`, `wait_for_shutdown`) |
