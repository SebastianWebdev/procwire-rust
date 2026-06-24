---
name: procwire-testing
description: The definition-of-done and conventions for procwire-client — the mandatory four-check CI gate (cargo fmt --check, clippy -D warnings, test, build --release), the test levels (unit/integration/e2e-with-Node/bench/cross-platform), how to write frame-level tests with build_frame + FrameBuffer and async tests with tokio::io::duplex, and the repo's AGENT_MEMORY.md + docs/TASKS workflow that must be updated when a task completes. Use before committing, when adding tests, when finishing any task, or when a clippy/fmt/test failure needs fixing.
---

# Procwire Testing, CI Gate & Project Conventions

Source of truth: `CLAUDE.md` (Commands, Task Completion Checklist),
`docs/TASKS/README.md`, `tests/integration.rs`, the `#[cfg(test)]` modules in
each `src/*` file, `benches/throughput.rs`.

## ✅ The four-check gate (run ALL before "done")

A task is **not complete** until every one of these passes:

```bash
cargo fmt -- --check          # formatting (rustfmt defaults, max width 100)
cargo clippy -- -D warnings   # lint: warnings are ERRORS
cargo test                    # all unit + integration tests
cargo build --release         # release build compiles
```

Fix failures before committing — do not commit red. Clippy is strict; pay
attention to `clippy::cast_possible_truncation` and `clippy::cast_sign_loss`
(this code casts `usize`→`u32` for `payloadLength` and `u16` method ids).

## Library code rules that tests enforce

- **No `unwrap()` / `expect()` in library code** — only in tests. Public fns
  return `Result<T, ProcwireError>`; propagate with `?`.
- All public items need `///` docs; modules need `//!` docs. New public API
  without docs is a (doc) smell even if it compiles.

## Test levels

1. **Unit** — wire format, frame buffer, codecs, context, registry. Co-located
   in `#[cfg(test)] mod tests` in each source file.
2. **Integration** — `tests/integration.rs`: full encode→frame→decode cycles,
   stream/ack/error/event/auth wire shapes, fragmentation.
3. **E2E** — Rust child ↔ real Node.js `@procwire/core` parent (see the
   `# Running with Node.js parent` doc-comments in `examples/*.rs`): the parent
   spawns the built example binary, sends requests, asserts responses, exercises
   streaming/events/abort, and validates throughput.
4. **Benchmark** — Criterion, `cargo bench` (`benches/throughput.rs`; currently a
   placeholder — real benches are TODO). Targets: >1 GB/s for ≥10 MB payloads,
   <100 µs RTT for 1 KB.
5. **Cross-platform** — CI runs on Linux/macOS/Windows (Unix socket vs Named
   Pipe). Keep `#[cfg(unix)]` / `#[cfg(windows)]` paths in sync.

## Patterns for writing tests

**Frame-level (no I/O):** build raw bytes, push through a `FrameBuffer`, assert.

```rust
use procwire_client::protocol::{build_frame, flags, FrameBuffer, Header};
use procwire_client::codec::MsgPackCodec;

let payload = MsgPackCodec::encode(&"hi").unwrap();
let header  = Header::new(1, flags::RESPONSE, 42, payload.len() as u32);
let bytes   = build_frame(&header, &payload);
let frames  = FrameBuffer::new().push(&bytes).unwrap();
assert_eq!(frames[0].request_id(), 42);
```

Exercise fragmentation by pushing slices (`&bytes[..5]`, then the rest) and
asserting `frames.is_empty()` until complete — readers must tolerate arbitrary
chunk boundaries.

**Async I/O (no real socket):** use `tokio::io::duplex(n)` as an in-memory pipe
and `spawn_writer_task` / `run_auth_handshake` against the two halves (see the
`#[tokio::test]` cases in `src/client.rs` and `src/writer.rs`). A
`RequestContext::new(method, req)` with no writer makes reply methods no-ops, so
handler logic can be unit-tested without a connection.

**Reply assertions:** read the raw bytes off the other duplex half and check
header flags/length (e.g. `n == HEADER_SIZE + payload_len`).

## 🔴 After completing a task — mandatory housekeeping (CLAUDE.md)

1. If working from a task file: move it `docs/TASKS/todo/` → `docs/TASKS/done/`.
2. **Update `AGENT_MEMORY.md`** (Polish, per repo convention):
   - add `[x] TASK-XX: …` under "TODO / W Trakcie",
   - record any bug/important decision under "Ostatnie Ważne Decyzje"
     (Problem / Root cause / Fix / Commit),
   - add useful discoveries to "Notatki dla Agenta".
   Memory must mention **every** completed task. Do not log trivial
   refactors/typos/formatting.

## Commits & diffs

- **Conventional commits**, scoped: `feat(protocol): …`, `fix(codec): …`,
  `test(e2e): …`, `docs: …`, `ci(release): …`, `chore: …`.
- When asked for a diff, **save it to a file**: `git diff main..HEAD > x.diff`.
- Releases are automated via release-plz → crates.io Trusted Publishing
  (`RELEASING.md`); don't hand-bump versions.

## Common mistakes

- ❌ Committing with clippy warnings ("it's just a warning") — CI uses
  `-D warnings`; warnings fail the build.
- ❌ `unwrap()`/`expect()` in non-test code.
- ❌ Skipping `cargo fmt -- --check` (CI fails on formatting).
- ❌ Finishing a task without updating `AGENT_MEMORY.md`.
- ❌ Adding a dependency without justifying it (CLAUDE.md keeps deps minimal).

## Where to look

| Concern                       | File / location                         |
|-------------------------------|-----------------------------------------|
| CI gate & conventions         | `CLAUDE.md`                             |
| Integration / wire-shape tests| `tests/integration.rs`                  |
| Async test patterns           | `src/client.rs`, `src/writer.rs` tests  |
| Benches                       | `benches/throughput.rs`                 |
| Task workflow                 | `docs/TASKS/README.md`, `AGENT_MEMORY.md` |
| Release process               | `RELEASING.md`                          |
