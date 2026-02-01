# TASK-14: E2E Tests z Node.js Parent

## Cel

Testy end-to-end: Node.js parent (@procwire/core) ↔ Rust child (procwire-client).

## Pliki

- `tests/e2e_test.rs`
- `tests/fixtures/parent.mjs` (Node.js test script)

## Zakres

- [ ] Node.js fixture script
- [ ] Rust test runner
- [ ] Test cases: echo, stream, ack, error, events, abort
- [ ] Large payload throughput test
- [ ] Cross-platform (Linux, macOS, Windows)

## Implementacja

### Node.js Fixture (`tests/fixtures/parent.mjs`)

```javascript
import { Module } from '@procwire/core';

async function main() {
    const results = {
        tests: [],
        passed: 0,
        failed: 0,
    };

    function test(name, fn) {
        results.tests.push({ name, fn });
    }

    // Test definitions
    test('echo - request/response', async (module) => {
        const result = await module.send('echo', { message: 'hello' });
        if (result.echo !== 'hello') throw new Error(`Expected 'hello', got '${result.echo}'`);
    });

    test('stream - multiple chunks', async (module) => {
        const chunks = [];
        for await (const chunk of module.stream('generate', { count: 5 })) {
            chunks.push(chunk);
        }
        if (chunks.length !== 5) throw new Error(`Expected 5 chunks, got ${chunks.length}`);
    });

    test('ack - acknowledgment', async (module) => {
        const ack = await module.send('fire', { data: 'test' });
        // ACK received = success
    });

    test('error - error response', async (module) => {
        try {
            await module.send('fail', {});
            throw new Error('Expected error');
        } catch (e) {
            if (!e.message.includes('Intentional error')) {
                throw new Error(`Wrong error: ${e.message}`);
            }
        }
    });

    test('event - child emits', async (module) => {
        return new Promise((resolve, reject) => {
            const timeout = setTimeout(() => reject(new Error('Timeout')), 5000);
            module.onEvent('progress', (data) => {
                clearTimeout(timeout);
                if (data.percent === 50) resolve();
                else reject(new Error(`Expected 50, got ${data.percent}`));
            });
            module.send('trigger_event', {});
        });
    });

    test('large payload - 10MB', async (module) => {
        const payload = Buffer.alloc(10 * 1024 * 1024, 'x');
        const start = Date.now();
        const result = await module.send('echo_raw', payload);
        const elapsed = Date.now() - start;
        const throughput = (10 / (elapsed / 1000)).toFixed(2);
        console.error(`  Throughput: ${throughput} MB/s`);
        if (result.length !== payload.length) {
            throw new Error(`Size mismatch: ${result.length} vs ${payload.length}`);
        }
    });

    // Run tests
    const rustBinary = process.argv[2] || './target/debug/examples/test_worker';

    const mod = new Module('rust-worker')
        .executable(rustBinary)
        .method('echo', { response: 'result' })
        .method('generate', { response: 'stream' })
        .method('fire', { response: 'ack' })
        .method('fail', { response: 'result' })
        .method('trigger_event', { response: 'ack' })
        .method('echo_raw', { response: 'result', codec: 'raw' })
        .event('progress');

    try {
        await mod.spawn();

        for (const { name, fn } of results.tests) {
            try {
                await fn(mod);
                results.passed++;
                console.error(`✓ ${name}`);
            } catch (e) {
                results.failed++;
                console.error(`✗ ${name}: ${e.message}`);
            }
        }
    } finally {
        await mod.kill();
    }

    // Output JSON result for Rust test runner
    console.log(JSON.stringify(results));
    process.exit(results.failed > 0 ? 1 : 0);
}

main().catch(e => {
    console.error(e);
    process.exit(1);
});
```

### Rust E2E Test

```rust
// tests/e2e_test.rs
use std::process::Command;

#[test]
fn test_e2e_with_nodejs_parent() {
    // Build the test worker example
    let build = Command::new("cargo")
        .args(["build", "--example", "test_worker"])
        .status()
        .expect("Failed to build test worker");
    assert!(build.success(), "Build failed");

    // Run Node.js test fixture
    let output = Command::new("node")
        .args(["tests/fixtures/parent.mjs", "./target/debug/examples/test_worker"])
        .output()
        .expect("Failed to run Node.js fixture");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    eprintln!("Node.js output:\n{}", stderr);

    // Parse JSON result
    let result: serde_json::Value = serde_json::from_str(&stdout)
        .expect("Failed to parse test results");

    let passed = result["passed"].as_u64().unwrap_or(0);
    let failed = result["failed"].as_u64().unwrap_or(0);

    assert_eq!(failed, 0, "E2E tests failed: {} passed, {} failed", passed, failed);
}
```

### Test Worker Example

```rust
// examples/test_worker.rs
use procwire_client::{Client, RequestContext};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct EchoInput { message: String }

#[derive(Serialize)]
struct EchoOutput { echo: String }

// ... inne handlery

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .handle("echo", |data: EchoInput, ctx: RequestContext| async move {
            ctx.respond(&EchoOutput { echo: data.message }).await
        })
        // ... inne metody
        .event("progress")
        .start()
        .await?;

    client.wait_for_shutdown().await?;
    Ok(())
}
```

## Test Cases

| Test | Typ | Opis |
|------|-----|------|
| echo | request→result | Prosty echo |
| stream | request→stream | Wiele chunków + end |
| ack | request→ack | Potwierdzenie |
| error | request→error | Error response |
| event | child→parent | Event emission |
| large_payload | throughput | 10MB+ test |
| abort | cancellation | Parent cancels request |

## Definition of Done

- [ ] E2E tests pass na Linux
- [ ] (Opcjonalnie) macOS i Windows
- [ ] Throughput test pokazuje > 100 MB/s
- [ ] Wszystkie scenariusze pokryte

## Kontekst

- E2E testy wymagają zainstalowanego Node.js
- Wymagają `@procwire/core` w PATH lub node_modules
- Są wolniejsze niż unit testy - uruchamiaj selektywnie
