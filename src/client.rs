//! Client builder and runtime loop.
//!
//! The [`ClientBuilder`] provides a fluent API for configuring handlers
//! and building the client. The [`Client`] manages the lifecycle:
//! 1. Create pipe listener
//! 2. Send `$init` via stdout
//! 3. Accept parent connection
//! 4. Read frames and dispatch to handlers
//!
//! # Example
//!
//! ```ignore
//! use procwire_client::Client;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Client::builder()
//!         .handle("echo", |data: String, ctx| async move {
//!             ctx.respond(&data).await
//!         })
//!         .handle_stream("count", |n: i32, ctx| async move {
//!             for i in 0..n {
//!                 ctx.chunk(&i).await?;
//!             }
//!             ctx.end().await
//!         })
//!         .event("progress")
//!         .start()
//!         .await?;
//!
//!     client.wait_for_shutdown().await?;
//!     Ok(())
//! }
//! ```

use std::future::Future;
use std::sync::Arc;

use bytes::Bytes;
use serde::de::DeserializeOwned;
use tokio::sync::{oneshot, Semaphore};
use tokio::task::JoinHandle;

use crate::codec::MsgPackCodec;
use crate::control::{build_init_message, write_stdout_line, ResponseType};
use crate::error::{ProcwireError, Result};
use crate::handler::{HandlerRegistry, HandlerResult, RequestContext};
use crate::protocol::{flags, FrameBuffer, Header, ABORT_METHOD_ID};
use crate::transport::{generate_pipe_path, PipeListener};
use crate::writer::{spawn_writer_task, OutboundFrame, WriterConfig, WriterHandle};

/// Default maximum concurrent handlers.
pub const DEFAULT_MAX_CONCURRENT_HANDLERS: usize = 256;

/// Builder for configuring and creating a Procwire client.
///
/// Use the fluent API to register handlers and events, then call `start()`
/// to begin the client lifecycle.
pub struct ClientBuilder {
    registry: HandlerRegistry,
    writer_config: WriterConfig,
    max_concurrent_handlers: usize,
}

impl ClientBuilder {
    /// Create a new client builder.
    pub fn new() -> Self {
        Self {
            registry: HandlerRegistry::new(),
            writer_config: WriterConfig::default(),
            max_concurrent_handlers: DEFAULT_MAX_CONCURRENT_HANDLERS,
        }
    }

    /// Register a method handler with "result" response type.
    ///
    /// The handler receives deserialized payload and a context for responding.
    pub fn handle<F, T, Fut>(mut self, method: &str, handler: F) -> Self
    where
        F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
        T: DeserializeOwned + Send + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        self.registry
            .register(method, ResponseType::Result, handler);
        self
    }

    /// Register a method handler with "stream" response type.
    ///
    /// Use `ctx.chunk()` to send stream chunks and `ctx.end()` to finish.
    pub fn handle_stream<F, T, Fut>(mut self, method: &str, handler: F) -> Self
    where
        F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
        T: DeserializeOwned + Send + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        self.registry
            .register(method, ResponseType::Stream, handler);
        self
    }

    /// Register a method handler with "ack" response type.
    ///
    /// Use `ctx.ack()` to send the acknowledgment.
    pub fn handle_ack<F, T, Fut>(mut self, method: &str, handler: F) -> Self
    where
        F: Fn(T, RequestContext) -> Fut + Send + Sync + 'static,
        T: DeserializeOwned + Send + 'static,
        Fut: Future<Output = HandlerResult> + Send + 'static,
    {
        self.registry.register(method, ResponseType::Ack, handler);
        self
    }

    /// Register an event that this client can emit.
    ///
    /// Events are fire-and-forget messages to the parent.
    pub fn event(mut self, name: &str) -> Self {
        self.registry.register_event(name);
        self
    }

    /// Set the maximum number of concurrent handlers.
    ///
    /// When this limit is reached, new requests will be dropped with a warning.
    /// Default: 256
    pub fn max_concurrent_handlers(mut self, limit: usize) -> Self {
        self.max_concurrent_handlers = limit;
        self
    }

    /// Set the maximum pending frames for backpressure.
    ///
    /// When this limit is reached, response methods will wait until
    /// backpressure clears or timeout.
    /// Default: 1024
    pub fn max_pending_frames(mut self, limit: usize) -> Self {
        self.writer_config.max_pending_frames = limit;
        self
    }

    /// Set the writer channel capacity.
    ///
    /// Default: 1024
    pub fn channel_capacity(mut self, capacity: usize) -> Self {
        self.writer_config.channel_capacity = capacity;
        self
    }

    /// Set the backpressure timeout.
    ///
    /// Default: 5 seconds
    pub fn backpressure_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.writer_config.backpressure_timeout = timeout;
        self
    }

    /// Build and start the client.
    ///
    /// This will:
    /// 1. Generate pipe path
    /// 2. Start pipe listener
    /// 3. Send `$init` to parent (stdout)
    /// 4. Accept parent connection
    /// 5. Start frame processing loop
    pub async fn start(self) -> Result<Client> {
        Client::start(
            self.registry,
            self.writer_config,
            self.max_concurrent_handlers,
        )
        .await
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A running Procwire client.
///
/// Use `emit()` to send events to the parent.
/// Use `wait_for_shutdown()` to block until the connection closes.
pub struct Client {
    /// Registry of handlers.
    registry: Arc<HandlerRegistry>,
    /// Writer handle for sending frames.
    writer: WriterHandle,
    /// Shutdown signal receiver.
    shutdown_rx: oneshot::Receiver<()>,
    /// Writer task handle.
    _writer_task: JoinHandle<Result<()>>,
}

impl Client {
    /// Create a new client builder.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Start the client with the given registry and configuration.
    async fn start(
        registry: HandlerRegistry,
        writer_config: WriterConfig,
        max_concurrent_handlers: usize,
    ) -> Result<Self> {
        // 1. Generate pipe path
        let pipe_path = generate_pipe_path();

        // 2. Start pipe listener
        let listener = PipeListener::bind(&pipe_path).await?;

        // 3. Build schema from registry
        let schema = registry.build_schema();

        // 4. Send $init to parent (stdout)
        let init_msg = build_init_message(&pipe_path, &schema);
        write_stdout_line(&init_msg)?;

        // 5. Accept parent connection
        let stream = listener.accept().await?;

        // 6. Split stream into reader and writer
        let (reader, write_half) = stream.into_split();

        // 7. Spawn writer task (replaces Arc<Mutex<Writer>>)
        let (writer, writer_task) = spawn_writer_task(write_half, writer_config);

        // 8. Create handler semaphore
        let handler_semaphore = Arc::new(Semaphore::new(max_concurrent_handlers));

        // 9. Spawn read loop
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let registry = Arc::new(registry);
        let writer_clone = writer.clone();
        let registry_clone = registry.clone();

        tokio::spawn(async move {
            if let Err(e) =
                Self::read_loop(reader, registry_clone, writer_clone, handler_semaphore).await
            {
                tracing::error!("Read loop error: {}", e);
            }
            let _ = shutdown_tx.send(());
        });

        Ok(Client {
            registry,
            writer,
            shutdown_rx,
            _writer_task: writer_task,
        })
    }

    /// Main read loop - reads frames and dispatches to handlers.
    async fn read_loop<R: tokio::io::AsyncRead + Unpin>(
        mut reader: R,
        registry: Arc<HandlerRegistry>,
        writer: WriterHandle,
        semaphore: Arc<Semaphore>,
    ) -> Result<()> {
        use tokio::io::AsyncReadExt;

        let mut frame_buffer = FrameBuffer::new();
        let mut buf = vec![0u8; 64 * 1024]; // 64KB read buffer

        loop {
            let n = match reader.read(&mut buf).await {
                Ok(0) => return Ok(()), // Connection closed
                Ok(n) => n,
                Err(e) => return Err(ProcwireError::Io(e)),
            };

            // Parse frames
            let frames = frame_buffer.push(&buf[..n])?;

            // Dispatch each frame
            for frame in frames {
                Self::dispatch_frame(&frame, &registry, &writer, &semaphore).await;
            }
        }
    }

    /// Dispatch a single frame to its handler.
    async fn dispatch_frame(
        frame: &crate::protocol::Frame,
        registry: &Arc<HandlerRegistry>,
        writer: &WriterHandle,
        semaphore: &Arc<Semaphore>,
    ) {
        let header = &frame.header;

        // Handle ABORT signal
        if header.method_id == ABORT_METHOD_ID {
            tracing::debug!("Received ABORT for request {}", header.request_id);
            // TODO: Implement cancellation
            return;
        }

        // Skip responses (we only handle requests)
        if header.is_response() {
            tracing::warn!("Received unexpected response frame");
            return;
        }

        // Try to acquire semaphore permit
        let permit = match semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!(
                    "Handler capacity reached, dropping request {} for method {}",
                    header.request_id,
                    header.method_id
                );
                return;
            }
        };

        // Create context for handler
        let ctx = RequestContext::with_writer(header.method_id, header.request_id, writer.clone());

        // Get payload
        let payload = frame.payload.clone();

        // Clone what we need for the spawned task
        let registry = registry.clone();
        let method_id = header.method_id;

        // Spawn handler task
        tokio::spawn(async move {
            // Permit is held until this task completes
            let _permit = permit;

            match registry.dispatch(method_id, &payload, ctx).await {
                Ok(()) => {}
                Err(e) => {
                    tracing::error!("Handler error for method {}: {}", method_id, e);
                }
            }
        });
    }

    /// Emit an event to the parent (fire-and-forget).
    ///
    /// Events are one-way messages that don't expect a response.
    pub async fn emit<T: serde::Serialize>(&self, event: &str, data: &T) -> Result<()> {
        let event_id = self
            .registry
            .get_event_id(event)
            .ok_or_else(|| ProcwireError::Protocol(format!("Unknown event: {}", event)))?;

        let payload = MsgPackCodec::encode(data)?;

        let header = Header::new(
            event_id,
            flags::DIRECTION_TO_PARENT, // Event, not a response
            0,                          // Events have request_id = 0
            payload.len() as u32,
        );

        let frame = OutboundFrame::new(&header, Bytes::from(payload));
        self.writer.send(frame).await
    }

    /// Emit an event with raw bytes payload.
    pub async fn emit_raw(&self, event: &str, data: &[u8]) -> Result<()> {
        let event_id = self
            .registry
            .get_event_id(event)
            .ok_or_else(|| ProcwireError::Protocol(format!("Unknown event: {}", event)))?;

        let header = Header::new(event_id, flags::DIRECTION_TO_PARENT, 0, data.len() as u32);

        let frame = OutboundFrame::new(&header, Bytes::copy_from_slice(data));
        self.writer.send(frame).await
    }

    /// Get the current backpressure status.
    pub fn is_backpressure_active(&self) -> bool {
        self.writer.is_backpressure_active()
    }

    /// Get the current pending frame count.
    pub fn pending_frames(&self) -> usize {
        self.writer.pending_count()
    }

    /// Wait for shutdown (pipe close or parent kill).
    ///
    /// This consumes the client and blocks until the connection closes.
    pub async fn wait_for_shutdown(self) -> Result<()> {
        let _ = self.shutdown_rx.await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let builder = ClientBuilder::new();
        // Just verify it compiles and creates
        let _ = builder;
    }

    #[test]
    fn test_builder_default() {
        let builder = ClientBuilder::default();
        let _ = builder;
    }

    #[test]
    fn test_builder_method_chaining() {
        let builder = Client::builder()
            .handle("echo", |_data: String, _ctx| async { Ok(()) })
            .handle_stream("stream", |_data: i32, _ctx| async { Ok(()) })
            .handle_ack("ack", |_data: (), _ctx| async { Ok(()) })
            .event("progress");

        // Verify registry was populated
        let schema = builder.registry.build_schema();
        assert!(schema.get_method("echo").is_some());
        assert!(schema.get_method("stream").is_some());
        assert!(schema.get_method("ack").is_some());
        assert!(schema.get_event("progress").is_some());
    }

    #[test]
    fn test_builder_response_types() {
        let builder = Client::builder()
            .handle("result", |_: (), _ctx| async { Ok(()) })
            .handle_stream("stream", |_: (), _ctx| async { Ok(()) })
            .handle_ack("ack", |_: (), _ctx| async { Ok(()) });

        assert_eq!(
            builder.registry.get_response_type("result"),
            Some(ResponseType::Result)
        );
        assert_eq!(
            builder.registry.get_response_type("stream"),
            Some(ResponseType::Stream)
        );
        assert_eq!(
            builder.registry.get_response_type("ack"),
            Some(ResponseType::Ack)
        );
    }

    #[test]
    fn test_builder_configuration() {
        let builder = Client::builder()
            .max_concurrent_handlers(512)
            .max_pending_frames(2048)
            .channel_capacity(512)
            .backpressure_timeout(std::time::Duration::from_secs(10));

        assert_eq!(builder.max_concurrent_handlers, 512);
        assert_eq!(builder.writer_config.max_pending_frames, 2048);
        assert_eq!(builder.writer_config.channel_capacity, 512);
        assert_eq!(
            builder.writer_config.backpressure_timeout,
            std::time::Duration::from_secs(10)
        );
    }
}
