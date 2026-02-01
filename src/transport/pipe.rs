//! Platform-specific pipe/socket implementation.
//!
//! - Unix: Unix Domain Socket
//! - Windows: Named Pipe
//!
//! # Example
//!
//! ```ignore
//! use procwire_client::transport::{generate_pipe_path, PipeListener};
//!
//! let path = generate_pipe_path();
//! let listener = PipeListener::bind(&path).await?;
//! let stream = listener.accept().await?;
//! ```

use crate::error::{ProcwireError, Result};
use tokio::io::{AsyncRead, AsyncWrite};

/// Generate a unique pipe path for this process.
///
/// Format:
/// - Unix: `/tmp/procwire-{pid}-{random}.sock`
/// - Windows: `\\.\pipe\procwire-{pid}-{random}`
pub fn generate_pipe_path() -> String {
    let pid = std::process::id();
    let rand: u64 = rand_u64();

    #[cfg(unix)]
    {
        format!("/tmp/procwire-{}-{:x}.sock", pid, rand)
    }

    #[cfg(windows)]
    {
        format!(r"\\.\pipe\procwire-{}-{:x}", pid, rand)
    }
}

/// Simple random u64 using system time and process ID.
fn rand_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    // Mix in process ID and some bit shuffling for better randomness
    let pid = std::process::id() as u64;
    nanos.wrapping_mul(0x517cc1b727220a95) ^ pid
}

// ============================================================================
// Unix Implementation
// ============================================================================

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use std::path::Path;
    use tokio::net::{UnixListener, UnixStream};

    /// Unix Domain Socket listener.
    pub struct PipeListener {
        listener: UnixListener,
        path: String,
    }

    /// Unix Domain Socket stream (connected).
    pub struct PipeStream {
        stream: UnixStream,
    }

    /// Cleanup guard that removes the socket file on drop.
    pub struct PipeCleanup {
        path: String,
    }

    impl Drop for PipeCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    impl PipeListener {
        /// Bind to a Unix socket path.
        ///
        /// Removes any existing socket file at the path before binding.
        pub async fn bind(path: &str) -> Result<Self> {
            // Remove old socket if it exists
            if Path::new(path).exists() {
                std::fs::remove_file(path)?;
            }

            let listener = UnixListener::bind(path)?;

            Ok(Self {
                listener,
                path: path.to_string(),
            })
        }

        /// Accept a single connection.
        ///
        /// Returns a connected `PipeStream`.
        pub async fn accept(&self) -> Result<PipeStream> {
            let (stream, _addr) = self.listener.accept().await?;
            Ok(PipeStream { stream })
        }

        /// Get the socket path.
        pub fn path(&self) -> &str {
            &self.path
        }

        /// Create a cleanup guard that removes the socket on drop.
        pub fn cleanup_guard(&self) -> PipeCleanup {
            PipeCleanup {
                path: self.path.clone(),
            }
        }
    }

    impl Drop for PipeListener {
        fn drop(&mut self) {
            // Clean up socket file when listener is dropped
            let _ = std::fs::remove_file(&self.path);
        }
    }

    impl PipeStream {
        /// Split into read and write halves.
        pub fn into_split(self) -> (impl AsyncRead, impl AsyncWrite) {
            self.stream.into_split()
        }

        /// Get a reference to the underlying stream.
        pub fn inner(&self) -> &UnixStream {
            &self.stream
        }

        /// Get a mutable reference to the underlying stream.
        pub fn inner_mut(&mut self) -> &mut UnixStream {
            &mut self.stream
        }
    }

    impl AsyncRead for PipeStream {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.stream).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for PipeStream {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::pin::Pin::new(&mut self.stream).poll_write(cx, buf)
        }

        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.stream).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.stream).poll_shutdown(cx)
        }
    }
}

// ============================================================================
// Windows Implementation
// ============================================================================

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    /// Windows Named Pipe listener.
    pub struct PipeListener {
        path: String,
    }

    /// Windows Named Pipe stream (connected).
    pub struct PipeStream {
        pipe: NamedPipeServer,
    }

    /// Cleanup guard (no-op on Windows, pipes are auto-cleaned).
    pub struct PipeCleanup {
        _path: String,
    }

    impl Drop for PipeCleanup {
        fn drop(&mut self) {
            // Windows Named Pipes are automatically cleaned up
        }
    }

    impl PipeListener {
        /// Create a Named Pipe server.
        pub async fn bind(path: &str) -> Result<Self> {
            // Verify we can create the pipe (will be created on first accept)
            let _ = ServerOptions::new()
                .first_pipe_instance(true)
                .create(path)
                .map_err(ProcwireError::Io)?;

            Ok(Self {
                path: path.to_string(),
            })
        }

        /// Accept a single connection.
        pub async fn accept(&self) -> Result<PipeStream> {
            let server = ServerOptions::new()
                .first_pipe_instance(false)
                .create(&self.path)
                .map_err(ProcwireError::Io)?;

            server.connect().await?;

            Ok(PipeStream { pipe: server })
        }

        /// Get the pipe path.
        pub fn path(&self) -> &str {
            &self.path
        }

        /// Create a cleanup guard (no-op on Windows).
        pub fn cleanup_guard(&self) -> PipeCleanup {
            PipeCleanup {
                _path: self.path.clone(),
            }
        }
    }

    impl PipeStream {
        /// Split into read and write halves.
        pub fn into_split(self) -> (impl AsyncRead, impl AsyncWrite) {
            tokio::io::split(self)
        }

        /// Get a reference to the underlying pipe.
        pub fn inner(&self) -> &NamedPipeServer {
            &self.pipe
        }

        /// Get a mutable reference to the underlying pipe.
        pub fn inner_mut(&mut self) -> &mut NamedPipeServer {
            &mut self.pipe
        }
    }

    impl AsyncRead for PipeStream {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.pipe).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for PipeStream {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::pin::Pin::new(&mut self.pipe).poll_write(cx, buf)
        }

        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.pipe).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.pipe).poll_shutdown(cx)
        }
    }
}

// ============================================================================
// Platform-independent re-exports
// ============================================================================

#[cfg(unix)]
pub use unix_impl::{PipeCleanup, PipeListener, PipeStream};

#[cfg(windows)]
pub use windows_impl::{PipeCleanup, PipeListener, PipeStream};

/// Create a pipe listener bound to the given path.
pub async fn create_pipe_listener(path: &str) -> Result<PipeListener> {
    PipeListener::bind(path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pipe_path_format() {
        let path = generate_pipe_path();

        #[cfg(unix)]
        {
            assert!(path.starts_with("/tmp/procwire-"));
            assert!(path.ends_with(".sock"));
        }

        #[cfg(windows)]
        {
            assert!(path.starts_with(r"\\.\pipe\procwire-"));
        }
    }

    #[test]
    fn test_generate_pipe_path_uniqueness() {
        // Generate multiple paths and check they're different
        let paths: Vec<String> = (0..10).map(|_| generate_pipe_path()).collect();

        for (i, p1) in paths.iter().enumerate() {
            for (j, p2) in paths.iter().enumerate() {
                if i != j {
                    // Note: In theory paths could collide if called at exact same nanosecond
                    // but in practice this is extremely unlikely
                    assert_ne!(p1, p2, "Paths should be unique");
                }
            }
        }
    }

    #[test]
    fn test_pipe_path_contains_pid() {
        let path = generate_pipe_path();
        let pid = std::process::id().to_string();
        assert!(path.contains(&pid), "Path should contain PID");
    }

    // Integration tests for actual bind/accept would need tokio runtime
    // and are better suited for integration tests folder
}
