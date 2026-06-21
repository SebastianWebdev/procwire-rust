//! # procwire-client
//!
//! Rust client SDK for the [Procwire](https://github.com/SebastianWebdev/procwire)
//! IPC protocol (wire version `1.0.0`).
//!
//! This crate enables Rust workers (child processes) to communicate with
//! a Node.js (or Bun) parent process running `@procwire/core` using a
//! high-performance binary protocol. It is wire-compatible with the post-audit
//! ("Phase 4") Node/Bun client: it answers the `$ping`/`$pong` heartbeat, shuts
//! down gracefully on `$shutdown`, bounds incoming frame sizes, and supports the
//! optional data-plane AUTH handshake (`PROCWIRE_TOKEN`).
//!
//! ## Features
//!
//! - **High Performance**: Binary protocol with MsgPack serialization
//! - **Zero-copy**: Uses `bytes::BytesMut` for efficient buffer management
//! - **Async/await**: Built on Tokio for non-blocking I/O
//! - **Cross-platform**: Works on Linux, macOS, and Windows
//! - **Type-safe**: Strongly typed handlers with Serde integration
//! - **Streaming**: Support for chunked responses
//! - **Cancellation**: Full abort signal support with `CancellationToken`
//!
//! ## Architecture
//!
//! Procwire uses a dual-channel architecture:
//!
//! - **Control Plane** (stdio): JSON-RPC for the `$init` handshake
//! - **Data Plane** (named pipe/Unix socket): Binary protocol for high-throughput communication
//!
//! ## Quick Start
//!
//! ```ignore
//! use procwire_client::ClientBuilder;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct EchoRequest {
//!     message: String,
//! }
//!
//! #[derive(Serialize, Deserialize)]
//! struct EchoResponse {
//!     message: String,
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = ClientBuilder::new()
//!         .handle("echo", |payload: EchoRequest, ctx| async move {
//!             ctx.respond(&EchoResponse {
//!                 message: payload.message,
//!             })
//!             .await
//!         })
//!         .start()
//!         .await?;
//!
//!     client.wait_for_shutdown().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Response Types
//!
//! Handlers can respond in several ways:
//!
//! - [`RequestContext::respond`] - Send a single response
//! - [`RequestContext::ack`] - Acknowledge receipt (fire-and-forget pattern)
//! - [`RequestContext::chunk`] / [`RequestContext::end`] - Streaming responses
//! - [`RequestContext::error`] - Send an error response
//!
//! ## Cancellation
//!
//! Handlers can check for abort signals from the parent:
//!
//! ```ignore
//! .handle("long_task", |_payload: (), ctx| async move {
//!     if ctx.is_cancelled() {
//!         return Ok(());
//!     }
//!     // ... do work
//!     ctx.respond(&result).await
//! })
//! ```
//!
//! ## Modules
//!
//! - [`protocol`] - Wire format, frame buffer, and frame types
//! - [`codec`] - Serialization codecs (MsgPack, Raw)
//! - [`transport`] - Platform-specific pipe/socket transport
//! - [`control`] - Control plane ($init message)
//! - [`handler`] - Handler registry and request context
//! - [`error`] - Error types

pub mod backpressure;
pub mod codec;
pub mod control;
pub mod error;
pub mod handler;
pub mod protocol;
pub mod transport;
pub mod writer;

mod client;

pub use client::{Client, ClientBuilder};
pub use error::ProcwireError;
pub use handler::RequestContext;
