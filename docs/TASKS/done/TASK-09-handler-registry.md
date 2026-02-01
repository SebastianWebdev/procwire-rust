# TASK-09: Handler Registry

## Cel

Rejestr metod i handlerów z type-safe dispatch.

## Plik

`src/handler/registry.rs`

## Zakres

- [ ] Definicja trait `Handler`
- [ ] Implementacja `TypedHandler` wrapper
- [ ] Implementacja `HandlerRegistry`
- [ ] Metody register/lookup/dispatch
- [ ] Testy jednostkowe

## Implementacja

### Handler trait

```rust
use std::future::Future;
use std::pin::Pin;

use crate::handler::RequestContext;
use crate::error::ProcwireError;

pub type HandlerResult = Result<(), ProcwireError>;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Handler: Send + Sync + 'static {
    fn call(&self, data: &[u8], ctx: RequestContext) -> BoxFuture<'static, HandlerResult>;
}
```

### TypedHandler wrapper

```rust
use std::marker::PhantomData;
use serde::de::DeserializeOwned;

use crate::codec::MsgPackCodec;

pub struct TypedHandler<F, T, Fut>
where
    F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
    T: DeserializeOwned + Send + 'static,
    Fut: Future<Output = HandlerResult> + Send + 'static,
{
    handler: F,
    _phantom: PhantomData<(T, Fut)>,
}

impl<F, T, Fut> TypedHandler<F, T, Fut>
where
    F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
    T: DeserializeOwned + Send + 'static,
    Fut: Future<Output = HandlerResult> + Send + 'static,
{
    pub fn new(handler: F) -> Self {
        Self { handler, _phantom: PhantomData }
    }
}

impl<F, T, Fut> Handler for TypedHandler<F, T, Fut>
where
    F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
    T: DeserializeOwned + Send + 'static,
    Fut: Future<Output = HandlerResult> + Send + 'static,
{
    fn call(&self, data: &[u8], ctx: RequestContext) -> BoxFuture<'static, HandlerResult> {
        // Deserialize data
        let parsed: T = match MsgPackCodec::deserialize(data) {
            Ok(v) => v,
            Err(e) => return Box::pin(async move { Err(e.into()) }),
        };

        let fut = (self.handler)(parsed, ctx);
        Box::pin(fut)
    }
}
```

### HandlerRegistry

```rust
use std::collections::{HashMap, HashSet};
use crate::control::init::{ResponseType, InitSchema, MethodSchema, EventSchema};

struct MethodEntry {
    handler: Box<dyn Handler>,
    response_type: ResponseType,
}

pub struct HandlerRegistry {
    methods: HashMap<String, MethodEntry>,
    events: HashSet<String>,
    next_method_id: u16,
    next_event_id: u16,
    method_name_to_id: HashMap<String, u16>,
    method_id_to_name: HashMap<u16, String>,
    event_name_to_id: HashMap<String, u16>,
}

impl HandlerRegistry {
    pub fn new() -> Self { ... }

    pub fn register<F, T, Fut>(&mut self, name: &str, handler: F, response_type: ResponseType)
    where
        F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
        T: DeserializeOwned + Send + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    { ... }

    pub fn register_event(&mut self, name: &str) { ... }

    pub fn get_handler(&self, name: &str) -> Option<&dyn Handler> { ... }
    pub fn get_handler_by_id(&self, id: u16) -> Option<&dyn Handler> { ... }
    pub fn get_method_name(&self, id: u16) -> Option<&str> { ... }
    pub fn get_method_id(&self, name: &str) -> Option<u16> { ... }
    pub fn get_event_id(&self, name: &str) -> Option<u16> { ... }

    pub fn build_schema(&self) -> InitSchema { ... }
}
```

## Testy

- [ ] Register method z typed handler
- [ ] Lookup by name
- [ ] Lookup by ID
- [ ] Event registration
- [ ] build_schema produces correct InitSchema
- [ ] ID assignment is sequential from 1

## Definition of Done

- [ ] `cargo test handler::registry` — PASS
- [ ] Rejestracja metod z typami generycznymi działa
- [ ] Dispatch po method_id działa
- [ ] Doc comments

## Kontekst

- Registry jest centrum dispatchu — mapuje method_id na handler
- IDs są przydzielane sekwencyjnie od 1 (0 jest reserved)
- TypedHandler automatycznie deserializuje dane z MsgPack
