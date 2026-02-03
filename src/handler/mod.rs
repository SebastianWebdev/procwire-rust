//! Handler module - request handling and dispatch.
//!
//! Provides:
//! - [`HandlerRegistry`] - maps method IDs to handlers
//! - [`RequestContext`] - allows handlers to respond, stream, etc.
//!
//! # Example
//!
//! ```ignore
//! use procwire_client::handler::{HandlerRegistry, RequestContext};
//! use procwire_client::control::ResponseType;
//!
//! let mut registry = HandlerRegistry::new();
//!
//! // Register a method handler
//! registry.register("echo", ResponseType::Result, |data: String, ctx| async move {
//!     ctx.respond(&data).await
//! });
//!
//! // Register a streaming handler
//! registry.register("count", ResponseType::Stream, |n: i32, ctx| async move {
//!     for i in 0..n {
//!         ctx.chunk(&i).await?;
//!     }
//!     ctx.end().await
//! });
//! ```

mod context;
mod registry;

pub use context::{RawPayload, RequestContext};
pub use registry::{BoxFuture, Handler, HandlerRegistry, HandlerResult, TypedHandler};
