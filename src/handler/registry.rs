//! Handler registry for dispatching requests by method ID.
//!
//! The registry maps method names to handlers and manages ID assignment.
//! IDs are assigned sequentially starting from 1 (0 is reserved).
//!
//! # Example
//!
//! ```ignore
//! use procwire_client::handler::{HandlerRegistry, RequestContext};
//! use procwire_client::control::ResponseType;
//!
//! let mut registry = HandlerRegistry::new();
//!
//! registry.register("echo", ResponseType::Result, |data: String, ctx| async move {
//!     ctx.respond(&data).await
//! });
//!
//! let schema = registry.build_schema();
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use serde::de::DeserializeOwned;

use super::RequestContext;
use crate::codec::MsgPackCodec;
use crate::control::{InitSchema, ResponseType};
use crate::error::{ProcwireError, Result};

/// Result type for handler functions.
pub type HandlerResult = Result<()>;

/// Boxed future for handler results.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for handler functions.
pub trait Handler: Send + Sync + 'static {
    /// Handle a request with raw payload bytes.
    fn call(&self, data: &[u8], ctx: RequestContext) -> BoxFuture<'static, HandlerResult>;
}

/// Wrapper that deserializes payload before calling the handler.
pub struct TypedHandler<F, T, Fut>
where
    F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
    T: DeserializeOwned + Send + 'static,
    Fut: Future<Output = HandlerResult> + Send + 'static,
{
    handler: F,
    _phantom: PhantomData<fn(T) -> Fut>,
}

impl<F, T, Fut> TypedHandler<F, T, Fut>
where
    F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
    T: DeserializeOwned + Send + 'static,
    Fut: Future<Output = HandlerResult> + Send + 'static,
{
    /// Create a new typed handler.
    pub fn new(handler: F) -> Self {
        Self {
            handler,
            _phantom: PhantomData,
        }
    }
}

impl<F, T, Fut> Handler for TypedHandler<F, T, Fut>
where
    F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
    T: DeserializeOwned + Send + 'static,
    Fut: Future<Output = HandlerResult> + Send + 'static,
{
    fn call(&self, data: &[u8], ctx: RequestContext) -> BoxFuture<'static, HandlerResult> {
        // Deserialize payload using MsgPack
        let parsed: T = match MsgPackCodec::decode(data) {
            Ok(v) => v,
            Err(e) => return Box::pin(async move { Err(e) }),
        };

        let fut = (self.handler)(parsed, ctx);
        Box::pin(fut)
    }
}

/// Entry for a registered method.
struct MethodEntry {
    /// The handler function.
    handler: Box<dyn Handler>,
    /// Expected response type.
    response_type: ResponseType,
    /// Assigned method ID.
    id: u16,
}

/// Registry mapping method names to handlers.
pub struct HandlerRegistry {
    /// Methods by name.
    methods: HashMap<String, MethodEntry>,
    /// Events by name (just track IDs, no handlers).
    events: HashMap<String, u16>,
    /// Next method ID to assign.
    next_method_id: u16,
    /// Next event ID to assign.
    next_event_id: u16,
    /// Method ID to name mapping (for dispatch).
    id_to_name: HashMap<u16, String>,
}

impl HandlerRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            methods: HashMap::new(),
            events: HashMap::new(),
            next_method_id: 1, // Start from 1, 0 is reserved
            next_event_id: 1,
            id_to_name: HashMap::new(),
        }
    }

    /// Register a method handler.
    ///
    /// # Arguments
    ///
    /// * `name` - Method name
    /// * `response_type` - Expected response type
    /// * `handler` - Handler function that takes (T, RequestContext) and returns Result<()>
    pub fn register<F, T, Fut>(&mut self, name: &str, response_type: ResponseType, handler: F)
    where
        F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
        T: DeserializeOwned + Send + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        let id = self.next_method_id;
        self.next_method_id += 1;

        let typed = TypedHandler::new(handler);
        self.methods.insert(
            name.to_string(),
            MethodEntry {
                handler: Box::new(typed),
                response_type,
                id,
            },
        );
        self.id_to_name.insert(id, name.to_string());
    }

    /// Register an event (no handler, just ID assignment).
    pub fn register_event(&mut self, name: &str) {
        let id = self.next_event_id;
        self.next_event_id += 1;
        self.events.insert(name.to_string(), id);
    }

    /// Get a handler by method name.
    pub fn get_handler(&self, name: &str) -> Option<&dyn Handler> {
        self.methods.get(name).map(|e| e.handler.as_ref())
    }

    /// Get a handler by method ID.
    pub fn get_handler_by_id(&self, id: u16) -> Option<&dyn Handler> {
        self.id_to_name
            .get(&id)
            .and_then(|name| self.methods.get(name))
            .map(|e| e.handler.as_ref())
    }

    /// Get method name by ID.
    pub fn get_method_name(&self, id: u16) -> Option<&str> {
        self.id_to_name.get(&id).map(|s| s.as_str())
    }

    /// Get method ID by name.
    pub fn get_method_id(&self, name: &str) -> Option<u16> {
        self.methods.get(name).map(|e| e.id)
    }

    /// Get event ID by name.
    pub fn get_event_id(&self, name: &str) -> Option<u16> {
        self.events.get(name).copied()
    }

    /// Get response type for a method.
    pub fn get_response_type(&self, name: &str) -> Option<ResponseType> {
        self.methods.get(name).map(|e| e.response_type)
    }

    /// Build an InitSchema from the registered methods and events.
    pub fn build_schema(&self) -> InitSchema {
        let mut schema = InitSchema::new();

        for (name, entry) in &self.methods {
            schema.add_method(name, entry.id, entry.response_type);
        }

        for (name, &id) in &self.events {
            schema.add_event(name, id);
        }

        schema
    }

    /// Dispatch a request to the appropriate handler.
    ///
    /// # Arguments
    ///
    /// * `method_id` - Method ID from frame header
    /// * `payload` - Raw payload bytes
    /// * `ctx` - Request context for responding
    pub async fn dispatch(
        &self,
        method_id: u16,
        payload: &[u8],
        ctx: RequestContext,
    ) -> Result<()> {
        let handler = self
            .get_handler_by_id(method_id)
            .ok_or(ProcwireError::HandlerNotFound(method_id))?;

        handler.call(payload, ctx).await
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_method() {
        let mut registry = HandlerRegistry::new();

        registry.register("echo", ResponseType::Result, |_data: String, _ctx| async {
            Ok(())
        });

        assert!(registry.get_handler("echo").is_some());
        assert_eq!(registry.get_method_id("echo"), Some(1));
        assert_eq!(registry.get_method_name(1), Some("echo"));
    }

    #[test]
    fn test_id_assignment_sequential() {
        let mut registry = HandlerRegistry::new();

        registry.register("method1", ResponseType::Result, |_: (), _ctx| async {
            Ok(())
        });
        registry.register("method2", ResponseType::Stream, |_: (), _ctx| async {
            Ok(())
        });
        registry.register("method3", ResponseType::Ack, |_: (), _ctx| async { Ok(()) });

        assert_eq!(registry.get_method_id("method1"), Some(1));
        assert_eq!(registry.get_method_id("method2"), Some(2));
        assert_eq!(registry.get_method_id("method3"), Some(3));
    }

    #[test]
    fn test_register_event() {
        let mut registry = HandlerRegistry::new();

        registry.register_event("progress");
        registry.register_event("status");

        assert_eq!(registry.get_event_id("progress"), Some(1));
        assert_eq!(registry.get_event_id("status"), Some(2));
    }

    #[test]
    fn test_build_schema() {
        let mut registry = HandlerRegistry::new();

        registry.register("echo", ResponseType::Result, |_: String, _ctx| async {
            Ok(())
        });
        registry.register("generate", ResponseType::Stream, |_: i32, _ctx| async {
            Ok(())
        });
        registry.register_event("progress");

        let schema = registry.build_schema();

        assert_eq!(schema.get_method("echo").unwrap().id, 1);
        assert_eq!(
            schema.get_method("echo").unwrap().response,
            ResponseType::Result
        );
        assert_eq!(schema.get_method("generate").unwrap().id, 2);
        assert_eq!(
            schema.get_method("generate").unwrap().response,
            ResponseType::Stream
        );
        assert_eq!(schema.get_event("progress").unwrap().id, 1);
    }

    #[test]
    fn test_handler_not_found() {
        let registry = HandlerRegistry::new();

        assert!(registry.get_handler("nonexistent").is_none());
        assert!(registry.get_handler_by_id(99).is_none());
    }

    #[test]
    fn test_response_type() {
        let mut registry = HandlerRegistry::new();

        registry.register("result_method", ResponseType::Result, |_: (), _ctx| async {
            Ok(())
        });
        registry.register("stream_method", ResponseType::Stream, |_: (), _ctx| async {
            Ok(())
        });

        assert_eq!(
            registry.get_response_type("result_method"),
            Some(ResponseType::Result)
        );
        assert_eq!(
            registry.get_response_type("stream_method"),
            Some(ResponseType::Stream)
        );
    }
}
