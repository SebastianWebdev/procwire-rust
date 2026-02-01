//! Control plane module - `$init` message and stdio I/O.
//!
//! The control plane uses JSON over stdio for the initial handshake.
//! After handshake, all communication happens on the data plane (pipe).
//!
//! # Workflow
//!
//! 1. Child creates pipe listener
//! 2. Child sends `$init` via stdout (JSON-RPC)
//! 3. Parent validates schema
//! 4. Parent connects to pipe
//! 5. Binary communication begins on data plane
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
mod stdio;

// New API
pub use init::{
    build_init_message, EventSchema, InitSchema, MethodSchema, ResponseType, PROTOCOL_VERSION,
};
pub use stdio::{write_stdout_json, write_stdout_line};

// Legacy API (for backward compatibility)
pub use init::{EventDef, InitMessage, InitParams, MethodDef, Schema};
