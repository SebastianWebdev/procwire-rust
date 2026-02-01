//! Transport module - platform-specific pipe/socket handling.
//!
//! Provides abstraction over:
//! - Unix Domain Sockets (Linux/macOS)
//! - Named Pipes (Windows)

mod pipe;

pub use pipe::{create_pipe_listener, generate_pipe_path, PipeCleanup, PipeListener, PipeStream};
