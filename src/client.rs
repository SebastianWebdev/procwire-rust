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

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use bytes::Bytes;
use serde::de::DeserializeOwned;
use tokio::sync::{oneshot, RwLock, Semaphore};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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

/// Active request context holder for ABORT handling.
struct ActiveContext {
    /// Cancellation token to signal abort.
    cancellation_token: CancellationToken,
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
    /// Active request contexts for ABORT handling.
    /// Maps request_id -> ActiveContext
    _active_contexts: Arc<RwLock<HashMap<u32, ActiveContext>>>,
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

        // 9. Create active contexts map for ABORT handling
        let active_contexts = Arc::new(RwLock::new(HashMap::new()));

        // 10. Spawn read loop
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let registry = Arc::new(registry);
        let writer_clone = writer.clone();
        let registry_clone = registry.clone();
        let active_contexts_clone = active_contexts.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::read_loop(
                reader,
                registry_clone,
                writer_clone,
                handler_semaphore,
                active_contexts_clone,
            )
            .await
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
            _active_contexts: active_contexts,
        })
    }

    /// Main read loop - reads frames and dispatches to handlers.
    async fn read_loop<R: tokio::io::AsyncRead + Unpin>(
        mut reader: R,
        registry: Arc<HandlerRegistry>,
        writer: WriterHandle,
        semaphore: Arc<Semaphore>,
        active_contexts: Arc<RwLock<HashMap<u32, ActiveContext>>>,
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
                Self::dispatch_frame(&frame, &registry, &writer, &semaphore, &active_contexts)
                    .await;
            }
        }
    }

    /// Dispatch a single frame to its handler.
    async fn dispatch_frame(
        frame: &crate::protocol::Frame,
        registry: &Arc<HandlerRegistry>,
        writer: &WriterHandle,
        semaphore: &Arc<Semaphore>,
        active_contexts: &Arc<RwLock<HashMap<u32, ActiveContext>>>,
    ) {
        let header = &frame.header;

        // Handle ABORT signal
        if header.method_id == ABORT_METHOD_ID {
            tracing::debug!("Received ABORT for request {}", header.request_id);

            // Find and cancel the active context
            let contexts = active_contexts.read().await;
            if let Some(ctx) = contexts.get(&header.request_id) {
                ctx.cancellation_token.cancel();
                tracing::debug!("Cancelled request {}", header.request_id);
            } else {
                tracing::warn!(
                    "ABORT for unknown or completed request {}",
                    header.request_id
                );
            }
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

        // Create cancellation token for this request
        let cancellation_token = CancellationToken::new();

        // Register active context for abort handling
        {
            let mut contexts = active_contexts.write().await;
            contexts.insert(
                header.request_id,
                ActiveContext {
                    cancellation_token: cancellation_token.clone(),
                },
            );
        }

        // Create context for handler with the cancellation token
        let ctx = RequestContext::with_writer_and_token(
            header.method_id,
            header.request_id,
            writer.clone(),
            cancellation_token,
        );

        // Get payload
        let payload = frame.payload.clone();

        // Clone what we need for the spawned task
        let registry = registry.clone();
        let method_id = header.method_id;
        let request_id = header.request_id;
        let active_contexts = active_contexts.clone();

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

            // Remove from active contexts when handler completes
            let mut contexts = active_contexts.write().await;
            contexts.remove(&request_id);
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

    #[tokio::test]
    async fn test_abort_cancels_active_handler() {
        use crate::protocol::{Frame, Header, ABORT_METHOD_ID};

        // Setup: Create active_contexts map and add a context
        let active_contexts = Arc::new(RwLock::new(HashMap::new()));
        let cancellation_token = CancellationToken::new();

        {
            let mut contexts = active_contexts.write().await;
            contexts.insert(
                42, // request_id
                ActiveContext {
                    cancellation_token: cancellation_token.clone(),
                },
            );
        }

        // Verify not cancelled initially
        assert!(!cancellation_token.is_cancelled());

        // Create ABORT frame
        let abort_header = Header::new(ABORT_METHOD_ID, 0, 42, 0);
        let abort_frame = Frame::new(abort_header, bytes::Bytes::new());

        // Create minimal mocks for dispatch_frame
        let registry = Arc::new(HandlerRegistry::new());
        let (client, _server) = tokio::io::duplex(4096);
        let (writer, _task) =
            crate::writer::spawn_writer_task(client, crate::writer::WriterConfig::default());
        let semaphore = Arc::new(Semaphore::new(256));

        // Dispatch ABORT frame
        Client::dispatch_frame(
            &abort_frame,
            &registry,
            &writer,
            &semaphore,
            &active_contexts,
        )
        .await;

        // Verify cancellation was triggered
        assert!(cancellation_token.is_cancelled());
    }

    #[tokio::test]
    async fn test_abort_for_unknown_request_logs_warning() {
        use crate::protocol::{Frame, Header, ABORT_METHOD_ID};

        // Setup: Create empty active_contexts map
        let active_contexts = Arc::new(RwLock::new(HashMap::new()));

        // Create ABORT frame for non-existent request
        let abort_header = Header::new(ABORT_METHOD_ID, 0, 999, 0);
        let abort_frame = Frame::new(abort_header, bytes::Bytes::new());

        // Create minimal mocks
        let registry = Arc::new(HandlerRegistry::new());
        let (client, _server) = tokio::io::duplex(4096);
        let (writer, _task) =
            crate::writer::spawn_writer_task(client, crate::writer::WriterConfig::default());
        let semaphore = Arc::new(Semaphore::new(256));

        // Dispatch ABORT frame - should not panic, just log warning
        Client::dispatch_frame(
            &abort_frame,
            &registry,
            &writer,
            &semaphore,
            &active_contexts,
        )
        .await;

        // No assertion needed - we just verify it doesn't panic
    }

    #[tokio::test]
    async fn test_handler_context_is_removed_after_completion() {
        use crate::protocol::{Frame, Header};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;

        // Setup
        let active_contexts = Arc::new(RwLock::new(HashMap::new()));
        let handler_started = Arc::new(AtomicBool::new(false));
        let handler_completed = Arc::new(AtomicBool::new(false));

        let handler_started_clone = handler_started.clone();
        let handler_completed_clone = handler_completed.clone();

        // Create registry with a handler that signals when it runs
        let mut registry = HandlerRegistry::new();
        registry.register(
            "test",
            crate::control::ResponseType::Result,
            move |_: (), ctx: RequestContext| {
                let started = handler_started_clone.clone();
                let completed = handler_completed_clone.clone();
                async move {
                    started.store(true, Ordering::SeqCst);
                    // Small delay to allow test to check active_contexts
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    ctx.respond(&"done").await?;
                    completed.store(true, Ordering::SeqCst);
                    Ok(())
                }
            },
        );

        let registry = Arc::new(registry);

        // Create test writer
        let (client, _server) = tokio::io::duplex(4096);
        let (writer, _task) =
            crate::writer::spawn_writer_task(client, crate::writer::WriterConfig::default());
        let semaphore = Arc::new(Semaphore::new(256));

        // Get the method ID assigned to "test"
        let method_id = registry.get_method_id("test").unwrap();

        // Create request frame with empty MsgPack payload for ()
        let payload = crate::codec::MsgPackCodec::encode(&()).unwrap();
        let header = Header::new(method_id, 0, 123, payload.len() as u32);
        let frame = Frame::new(header, bytes::Bytes::from(payload));

        // Dispatch frame
        Client::dispatch_frame(&frame, &registry, &writer, &semaphore, &active_contexts).await;

        // Wait for handler to start
        tokio::time::timeout(Duration::from_millis(100), async {
            while !handler_started.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Handler should start");

        // Context should be in active_contexts
        {
            let contexts = active_contexts.read().await;
            assert!(
                contexts.contains_key(&123),
                "Context should be active while handler runs"
            );
        }

        // Wait for handler to complete
        tokio::time::timeout(Duration::from_millis(100), async {
            while !handler_completed.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Handler should complete");

        // Give a bit of time for cleanup
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Context should be removed from active_contexts
        {
            let contexts = active_contexts.read().await;
            assert!(
                !contexts.contains_key(&123),
                "Context should be removed after handler completes"
            );
        }
    }
}
