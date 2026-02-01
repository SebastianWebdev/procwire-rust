//! # procwire-client
//!
//! Rust client SDK for Procwire v2.0 IPC protocol.
//!
//! This crate enables Rust workers (child processes) to communicate with
//! a Node.js parent process running `@procwire/core`.
//!
//! ## Architecture
//!
//! - **Control Plane** (stdio): JSON only for `$init` handshake
//! - **Data Plane** (named pipe): Binary protocol for high-throughput communication
//!
//! ## Example
//!
//! ```ignore
//! use procwire_client::ClientBuilder;
//!
//! #[tokio::main]
//! async fn main() {
//!     let client = ClientBuilder::new()
//!         .method("echo", |ctx, payload| async move {
//!             ctx.respond(payload).await
//!         })
//!         .build()
//!         .await
//!         .unwrap();
//!
//!     client.run().await.unwrap();
//! }
//! ```

pub mod codec;
pub mod control;
pub mod error;
pub mod handler;
pub mod protocol;
pub mod transport;

mod backpressure;
mod client;

pub use client::{Client, ClientBuilder};
pub use error::ProcwireError;
pub use handler::RequestContext;
