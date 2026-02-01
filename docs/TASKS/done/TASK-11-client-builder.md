# TASK-11: Client Builder

## Cel

Fluent API do konfiguracji klienta.

## Plik

`src/client.rs`

## Zakres

- [ ] Implementacja `ClientBuilder` struct
- [ ] Metody handle/handle_stream/handle_ack/event
- [ ] Method chaining (fluent API)
- [ ] Przygotowanie do `start()` (implementacja w TASK-12)
- [ ] Testy jednostkowe

## Implementacja

### ClientBuilder

```rust
use std::future::Future;
use serde::de::DeserializeOwned;

use crate::handler::{HandlerRegistry, HandlerResult, RequestContext};
use crate::control::init::ResponseType;
use crate::error::ProcwireError;

pub struct ClientBuilder {
    registry: HandlerRegistry,
}

impl ClientBuilder {
    pub fn new() -> Self {
        Self {
            registry: HandlerRegistry::new(),
        }
    }

    /// Register a method handler with "result" response type
    pub fn handle<F, T, Fut>(mut self, method: &str, handler: F) -> Self
    where
        F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
        T: DeserializeOwned + Send + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        self.registry.register(method, handler, ResponseType::Result);
        self
    }

    /// Register a method handler with "stream" response type
    pub fn handle_stream<F, T, Fut>(mut self, method: &str, handler: F) -> Self
    where
        F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
        T: DeserializeOwned + Send + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        self.registry.register(method, handler, ResponseType::Stream);
        self
    }

    /// Register a method handler with "ack" response type
    pub fn handle_ack<F, T, Fut>(mut self, method: &str, handler: F) -> Self
    where
        F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
        T: DeserializeOwned + Send + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        self.registry.register(method, handler, ResponseType::Ack);
        self
    }

    /// Register an event that this client can emit
    pub fn event(mut self, name: &str) -> Self {
        self.registry.register_event(name);
        self
    }

    /// Build and start the client
    ///
    /// This will:
    /// 1. Generate pipe path
    /// 2. Start pipe listener
    /// 3. Send $init to parent (stdout)
    /// 4. Accept parent connection
    /// 5. Start frame processing loop
    pub async fn start(self) -> Result<Client, ProcwireError> {
        Client::start(self.registry).await
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
```

### Client struct (placeholder dla TASK-12)

```rust
pub struct Client {
    // Runtime state - implementacja w TASK-12
    _registry: HandlerRegistry,
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    async fn start(_registry: HandlerRegistry) -> Result<Self, ProcwireError> {
        // Implementacja w TASK-12
        todo!("Implemented in TASK-12")
    }
}
```

## Przykład użycia

```rust
use procwire_client::{Client, RequestContext};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct EchoInput { message: String }

#[derive(Serialize)]
struct EchoOutput { echo: String }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .handle("echo", |data: EchoInput, ctx: RequestContext| async move {
            ctx.respond(&EchoOutput { echo: data.message }).await
        })
        .handle_stream("generate", |data: GenInput, ctx: RequestContext| async move {
            for i in 0..10 {
                ctx.chunk(&Chunk { index: i }).await?;
            }
            ctx.end().await
        })
        .event("progress")
        .start()
        .await?;

    // ...
    Ok(())
}
```

## Testy

- [ ] Builder creates with empty registry
- [ ] handle() adds method with Result response type
- [ ] handle_stream() adds method with Stream response type
- [ ] handle_ack() adds method with Ack response type
- [ ] event() adds event
- [ ] Method chaining works
- [ ] Default impl works

## Definition of Done

- [ ] `cargo test client` — PASS
- [ ] Builder compiles z ergonomicznym API
- [ ] Method chaining działa
- [ ] Doc comments z przykładami

## Kontekst

- ClientBuilder jest głównym entry point dla użytkowników crate
- Fluent API pozwala na czytelną konfigurację
- `start()` jest implementowane w TASK-12
