//! Control plane module - `$init` handshake, heartbeat, and stdio I/O.
//!
//! The control plane uses newline-delimited JSON-RPC 2.0 over stdio. It carries
//! the initial handshake plus liveness/lifecycle messages; all user data flows
//! on the data plane (pipe).
//!
//! # Workflow
//!
//! 1. Child creates pipe listener
//! 2. Child sends `$init` via stdout (JSON-RPC)
//! 3. Parent validates schema
//! 4. Parent connects to pipe
//! 5. Binary communication begins on data plane
//!
//! # Control messages (parent → child, on stdin)
//!
//! - `$ping`     → child replies with `$pong` on stdout (liveness heartbeat)
//! - `$shutdown` → child shuts down cleanly and exits
//!
//! See [`run_control_reader`] for the reader loop that handles these.
//!
//! # Example
//!
//! ```ignore
//! use procwire_client::control::{build_init_message, write_stdout_line, InitSchema, ResponseType};
//! use procwire_client::transport::generate_pipe_path;
//!
//! // Create schema
//! let mut schema = InitSchema::new();
//! schema.add_method("echo", 1, ResponseType::Result);
//!
//! // Send $init
//! let pipe_path = generate_pipe_path();
//! let init_msg = build_init_message(&pipe_path, &schema);
//! write_stdout_line(&init_msg)?;
//! ```

mod init;
mod stdin;
mod stdio;

// New API
pub use init::{
    build_init_message, EventSchema, InitSchema, MethodSchema, ResponseType, PROTOCOL_VERSION,
};
pub use stdin::{run_control_reader, PONG_MESSAGE};
pub use stdio::{write_stdout_json, write_stdout_line};

// Legacy API (for backward compatibility)
pub use init::{EventDef, InitMessage, InitParams, MethodDef, Schema};
