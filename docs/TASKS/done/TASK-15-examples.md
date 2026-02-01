# TASK-15: Examples

## Cel

Przykładowe workery Rust demonstrujące API.

## Pliki

- `examples/echo_worker.rs`
- `examples/stream_worker.rs`
- `examples/event_worker.rs`

## Zakres

- [ ] Echo worker (prosty request/response)
- [ ] Stream worker (generowanie streamu)
- [ ] Event worker (emitowanie eventów)
- [ ] Każdy example z komentarzami
- [ ] README z instrukcjami uruchomienia

## Implementacja

### Echo Worker (`examples/echo_worker.rs`)

```rust
//! Echo Worker - prosty przykład request/response
//!
//! Uruchom z Node.js parent:
//! ```js
//! const mod = new Module('echo')
//!     .executable('./target/debug/examples/echo_worker')
//!     .method('echo', { response: 'result' });
//! await mod.spawn();
//! const result = await mod.send('echo', { message: 'hello' });
//! console.log(result); // { echo: 'hello' }
//! ```

use procwire_client::{Client, RequestContext};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct EchoInput {
    message: String,
}

#[derive(Serialize)]
struct EchoOutput {
    echo: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Inicjalizacja logowania (opcjonalne)
    tracing_subscriber::fmt::init();

    // Budowanie klienta z fluent API
    let client = Client::builder()
        .handle("echo", |data: EchoInput, ctx: RequestContext| async move {
            // Odpowiedz tym samym co otrzymaliśmy
            ctx.respond(&EchoOutput {
                echo: data.message
            }).await
        })
        .start()
        .await?;

    // Czekaj na zamknięcie przez parent
    client.wait_for_shutdown().await?;

    Ok(())
}
```

### Stream Worker (`examples/stream_worker.rs`)

```rust
//! Stream Worker - przykład streamowania danych
//!
//! Uruchom z Node.js parent:
//! ```js
//! const mod = new Module('generator')
//!     .executable('./target/debug/examples/stream_worker')
//!     .method('generate', { response: 'stream' });
//! await mod.spawn();
//! for await (const chunk of mod.stream('generate', { count: 5 })) {
//!     console.log(chunk); // { index: 0 }, { index: 1 }, ...
//! }
//! ```

use procwire_client::{Client, RequestContext};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct GenerateInput {
    count: usize,
}

#[derive(Serialize)]
struct Chunk {
    index: usize,
    data: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .handle_stream("generate", |data: GenerateInput, ctx: RequestContext| async move {
            // Wyślij serię chunków
            for i in 0..data.count {
                // Sprawdź czy request nie został anulowany
                if ctx.is_aborted() {
                    break;
                }

                ctx.chunk(&Chunk {
                    index: i,
                    data: format!("Chunk {}", i),
                }).await?;

                // Symulacja pracy
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }

            // Zakończ stream
            ctx.end().await
        })
        .start()
        .await?;

    client.wait_for_shutdown().await?;

    Ok(())
}
```

### Event Worker (`examples/event_worker.rs`)

```rust
//! Event Worker - przykład emitowania eventów do parent
//!
//! Uruchom z Node.js parent:
//! ```js
//! const mod = new Module('progress')
//!     .executable('./target/debug/examples/event_worker')
//!     .method('start_work', { response: 'ack' })
//!     .event('progress');
//! await mod.spawn();
//!
//! mod.onEvent('progress', (data) => {
//!     console.log(`Progress: ${data.percent}%`);
//! });
//!
//! await mod.send('start_work', { duration_ms: 1000 });
//! ```

use procwire_client::{Client, RequestContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
struct WorkInput {
    duration_ms: u64,
}

#[derive(Serialize)]
struct ProgressEvent {
    percent: u32,
    message: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(
        Client::builder()
            .handle_ack("start_work", |data: WorkInput, ctx: RequestContext| {
                let client = ctx.client().clone(); // Potrzebne do emit
                async move {
                    // Potwierdź otrzymanie requestu
                    ctx.ack_empty().await?;

                    // Symulacja pracy z raportowaniem postępu
                    let steps = 10;
                    let step_duration = data.duration_ms / steps;

                    for i in 1..=steps {
                        tokio::time::sleep(
                            tokio::time::Duration::from_millis(step_duration)
                        ).await;

                        // Emituj event progress
                        client.emit("progress", &ProgressEvent {
                            percent: (i * 10) as u32,
                            message: format!("Step {} of {}", i, steps),
                        }).await?;
                    }

                    Ok(())
                }
            })
            .event("progress")
            .start()
            .await?
    );

    client.wait_for_shutdown().await?;

    Ok(())
}
```

## Budowanie i uruchamianie

```bash
# Budowanie wszystkich examples
cargo build --examples

# Lista examples
ls target/debug/examples/

# Uruchomienie (wymaga Node.js parent)
# Parent musi zaimportować @procwire/core i spawować binary
```

## Definition of Done

- [ ] `cargo build --examples` — OK
- [ ] Każdy example kompiluje się bez błędów
- [ ] Każdy example ma komentarze dokumentacyjne
- [ ] Każdy example działa z Node.js parent

## Kontekst

- Examples są najlepszą dokumentacją dla użytkowników
- Powinny pokazywać idiomatyczne użycie API
- Służą też jako test workers dla E2E testów
