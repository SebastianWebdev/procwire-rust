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
use crate::control::{build_init_message, run_control_reader, write_stdout_line, ResponseType};
use crate::error::{ProcwireError, Result};
use crate::handler::{HandlerRegistry, HandlerResult, RequestContext};
use crate::protocol::{
    flags, Frame, FrameBuffer, Header, ABORT_METHOD_ID, ABSOLUTE_MAX_PAYLOAD_SIZE, AUTH_METHOD_ID,
    DEFAULT_MAX_PAYLOAD_SIZE,
};
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
    max_payload_size: u32,
    auth_token: Option<String>,
}

impl ClientBuilder {
    /// Create a new client builder.
    pub fn new() -> Self {
        Self {
            registry: HandlerRegistry::new(),
            writer_config: WriterConfig::default(),
            max_concurrent_handlers: DEFAULT_MAX_CONCURRENT_HANDLERS,
            max_payload_size: DEFAULT_MAX_PAYLOAD_SIZE,
            auth_token: None,
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

    /// Set the maximum size (in bytes) of an incoming frame's payload.
    ///
    /// The declared `payloadLength` of every incoming frame is checked against
    /// this limit **before** any payload buffer is allocated. A frame that
    /// exceeds the limit is rejected and the connection is torn down, instead of
    /// attempting a huge allocation (a DoS/OOM vector, since `payloadLength` is
    /// peer-controlled).
    ///
    /// Values are clamped to [`ABSOLUTE_MAX_PAYLOAD_SIZE`] (~2 GiB − 1).
    ///
    /// Default: [`DEFAULT_MAX_PAYLOAD_SIZE`] (1 GiB)
    pub fn max_payload_size(mut self, size: u32) -> Self {
        self.max_payload_size = size.min(ABSOLUTE_MAX_PAYLOAD_SIZE);
        self
    }

    /// Set the data-plane authentication token explicitly.
    ///
    /// When a token is configured, the client holds every accepted connection
    /// **pending** until its first data-plane frame is an [`AUTH_METHOD_ID`]
    /// frame whose payload equals this token (compared in constant time); only
    /// then is the connection adopted. A missing/mismatched first frame drops
    /// the connection while the listener keeps waiting for the real parent.
    ///
    /// If this is not set, the token defaults to the `PROCWIRE_TOKEN`
    /// environment variable (which an `auth: true` parent sets on the child).
    /// When neither is present, authentication is disabled and connections are
    /// adopted on accept — fully compatible with non-auth parents.
    pub fn auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    /// Build and start the client.
    ///
    /// This will:
    /// 1. Generate pipe path
    /// 2. Start pipe listener
    /// 3. Send `$init` to parent (stdout)
    /// 4. Accept (and, when a token is configured, authenticate) the parent connection
    /// 5. Start frame processing loop
    pub async fn start(self) -> Result<Client> {
        // An explicit builder token wins; otherwise fall back to PROCWIRE_TOKEN
        // (set by an auth-enabled parent). An empty value disables auth.
        let auth_token = self
            .auth_token
            .or_else(|| std::env::var("PROCWIRE_TOKEN").ok())
            .filter(|t| !t.is_empty());

        Client::start(
            self.registry,
            self.writer_config,
            self.max_concurrent_handlers,
            self.max_payload_size,
            auth_token,
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

/// Outcome of the data-plane authentication handshake for one connection.
enum AuthOutcome {
    /// The connection authenticated. `frame_buffer` carries any bytes buffered
    /// past the AUTH frame; `pending_frames` are whole frames already parsed
    /// after it (pipelined in the same packet) that must be dispatched.
    Adopted {
        frame_buffer: FrameBuffer,
        pending_frames: Vec<Frame>,
    },
    /// The connection failed authentication and must be dropped.
    Rejected,
}

/// Constant-time byte-slice equality.
///
/// Returns `false` immediately on a length mismatch (the length is not secret),
/// otherwise compares every byte without short-circuiting so the comparison
/// time does not leak how many leading bytes matched. This mirrors the Node
/// client's use of `crypto.timingSafeEqual` for the AUTH token check.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
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
    /// Shutdown signal receiver (fires when the data plane closes).
    shutdown_rx: oneshot::Receiver<()>,
    /// Control-plane shutdown token (fires when `$shutdown` is received).
    shutdown_token: CancellationToken,
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
    ///
    /// When `auth_token` is `Some`, the data-plane connection must authenticate
    /// with a matching AUTH frame as its first frame before it is adopted.
    async fn start(
        registry: HandlerRegistry,
        writer_config: WriterConfig,
        max_concurrent_handlers: usize,
        max_payload_size: u32,
        auth_token: Option<String>,
    ) -> Result<Self> {
        // 1. Generate pipe path
        let pipe_path = generate_pipe_path();

        // 2. Start pipe listener (MUST be listening before $init is announced,
        //    so the parent's connect — which is now bounded by a timeout — finds
        //    the server ready).
        let listener = PipeListener::bind(&pipe_path).await?;

        // 3. Build schema from registry
        let schema = registry.build_schema();

        // 4. Send $init to parent (stdout)
        let init_msg = build_init_message(&pipe_path, &schema);
        write_stdout_line(&init_msg)?;

        // 5. Spawn the control-plane reader (heartbeat + graceful shutdown).
        //    It runs on a dedicated OS thread so a blocking stdin read can never
        //    keep the runtime/process alive; it simply dies with the process.
        let shutdown_token = CancellationToken::new();
        let control_token = shutdown_token.clone();
        std::thread::Builder::new()
            .name("procwire-control".to_string())
            .spawn(move || run_control_reader(control_token))
            .map_err(ProcwireError::Io)?;

        // 6. Accept (and, with auth enabled, authenticate) the parent connection.
        //    Without a token the first accepted connection is adopted. With a
        //    token, the connection is held pending until its FIRST data-plane
        //    frame proves the token; a missing/mismatched first frame drops the
        //    connection and we keep listening for the real parent. The writer
        //    task is only spawned post-adoption, so no child→parent frame is
        //    ever written to an unauthenticated peer.
        let (reader, write_half, frame_buffer, initial_frames) = loop {
            let stream = listener.accept().await?;
            let (mut reader, write_half) = stream.into_split();

            match &auth_token {
                None => {
                    break (
                        reader,
                        write_half,
                        FrameBuffer::with_max_payload(max_payload_size),
                        Vec::new(),
                    );
                }
                Some(token) => {
                    match Self::run_auth_handshake(&mut reader, token, max_payload_size).await {
                        AuthOutcome::Adopted {
                            frame_buffer,
                            pending_frames,
                        } => break (reader, write_half, frame_buffer, pending_frames),
                        AuthOutcome::Rejected => {
                            tracing::warn!(
                                "Data-plane authentication failed; dropping connection \
                                 and waiting for the parent"
                            );
                            // reader + write_half drop here, closing the socket.
                            continue;
                        }
                    }
                }
            }
        };

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
                frame_buffer,
                initial_frames,
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
            shutdown_token,
            _writer_task: writer_task,
            _active_contexts: active_contexts,
        })
    }

    /// Main read loop - reads frames and dispatches to handlers.
    ///
    /// `frame_buffer` carries the configured `max_payload_size` (and, after an
    /// auth handshake, any bytes already buffered past the AUTH frame), so an
    /// oversized/invalid `payloadLength` makes `push` return an error here,
    /// ending the loop and tearing down the connection without ever allocating
    /// the giant buffer. `initial_frames` are frames that were pipelined after
    /// the AUTH frame in the same packet and must be dispatched first.
    async fn read_loop<R: tokio::io::AsyncRead + Unpin>(
        mut reader: R,
        registry: Arc<HandlerRegistry>,
        writer: WriterHandle,
        semaphore: Arc<Semaphore>,
        active_contexts: Arc<RwLock<HashMap<u32, ActiveContext>>>,
        mut frame_buffer: FrameBuffer,
        initial_frames: Vec<Frame>,
    ) -> Result<()> {
        use tokio::io::AsyncReadExt;

        // Dispatch frames that arrived pipelined behind the AUTH frame before
        // reading anything new from the socket.
        for frame in initial_frames {
            Self::dispatch_frame(&frame, &registry, &writer, &semaphore, &active_contexts).await;
        }

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

    /// Run the data-plane authentication handshake on a freshly accepted
    /// connection.
    ///
    /// Reads from `reader` until the first complete frame is available. The
    /// connection is adopted only if that first frame is an [`AUTH_METHOD_ID`]
    /// frame whose payload equals `token` (compared in constant time). Any
    /// frames pipelined after the AUTH frame in the same packet are returned for
    /// normal dispatch. Anything else — a non-auth first frame, a token
    /// mismatch, an oversized frame, a read error, or EOF — rejects the
    /// connection so the caller can drop it and keep listening.
    async fn run_auth_handshake<R: tokio::io::AsyncRead + Unpin>(
        reader: &mut R,
        token: &str,
        max_payload_size: u32,
    ) -> AuthOutcome {
        use tokio::io::AsyncReadExt;

        let mut frame_buffer = FrameBuffer::with_max_payload(max_payload_size);
        let mut buf = vec![0u8; 64 * 1024];

        loop {
            let n = match reader.read(&mut buf).await {
                Ok(0) => return AuthOutcome::Rejected, // closed before authenticating
                Ok(n) => n,
                Err(e) => {
                    tracing::debug!("Auth handshake read error: {}", e);
                    return AuthOutcome::Rejected;
                }
            };

            let mut frames = match frame_buffer.push(&buf[..n]) {
                Ok(frames) => frames,
                Err(e) => {
                    // Oversized/invalid first frame: drop without allocating.
                    tracing::debug!("Auth handshake framing error: {}", e);
                    return AuthOutcome::Rejected;
                }
            };

            if frames.is_empty() {
                continue; // need more bytes for the first complete frame
            }

            // The FIRST frame must be a matching AUTH frame.
            let first = frames.remove(0);
            if first.header.method_id == AUTH_METHOD_ID
                && constant_time_eq(&first.payload, token.as_bytes())
            {
                return AuthOutcome::Adopted {
                    frame_buffer,
                    pending_frames: frames,
                };
            }

            return AuthOutcome::Rejected;
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

        // A stray AUTH frame on an already-adopted connection is a no-op: auth
        // is a one-time gate (handled before adoption), never a regular method.
        if header.method_id == AUTH_METHOD_ID {
            tracing::debug!("Ignoring stray AUTH frame on adopted connection");
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

    /// Wait for shutdown.
    ///
    /// This consumes the client and resolves when **either**:
    /// - the data plane closes (pipe EOF / parent kill / read error), or
    /// - a `$shutdown` message is received on the control plane.
    ///
    /// Returning lets `main` exit promptly, which drops the connection and ends
    /// the process — graceful teardown rather than waiting for the parent's
    /// force-kill grace period.
    pub async fn wait_for_shutdown(self) -> Result<()> {
        let shutdown_rx = self.shutdown_rx;
        let shutdown_token = self.shutdown_token;

        tokio::select! {
            _ = shutdown_rx => {
                tracing::debug!("Data plane closed; shutting down");
            }
            _ = shutdown_token.cancelled() => {
                tracing::info!("$shutdown received; shutting down gracefully");
            }
        }

        Ok(())
    }

    /// Returns `true` once a `$shutdown` has been requested on the control plane.
    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_token.is_cancelled()
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

    #[test]
    fn test_builder_max_payload_size_default() {
        let builder = Client::builder();
        assert_eq!(builder.max_payload_size, DEFAULT_MAX_PAYLOAD_SIZE);
    }

    #[test]
    fn test_builder_max_payload_size_custom() {
        let builder = Client::builder().max_payload_size(4096);
        assert_eq!(builder.max_payload_size, 4096);
    }

    #[test]
    fn test_builder_max_payload_size_clamped_to_absolute_max() {
        // u32::MAX (~4 GiB) must be clamped to the absolute ceiling (~2 GiB - 1).
        let builder = Client::builder().max_payload_size(u32::MAX);
        assert_eq!(builder.max_payload_size, ABSOLUTE_MAX_PAYLOAD_SIZE);
    }

    #[test]
    fn test_builder_auth_token_default_none() {
        let builder = Client::builder();
        assert!(builder.auth_token.is_none());
    }

    #[test]
    fn test_builder_auth_token_set() {
        let builder = Client::builder().auth_token("secret-token");
        assert_eq!(builder.auth_token.as_deref(), Some("secret-token"));
    }

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"", b""));
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab")); // length mismatch
        assert!(!constant_time_eq(b"ab", b"abc")); // length mismatch
        let token = "a".repeat(64);
        assert!(constant_time_eq(token.as_bytes(), token.as_bytes()));
    }

    /// Build raw bytes for an AUTH frame carrying `token`.
    fn auth_frame_bytes(token: &[u8]) -> Vec<u8> {
        let header = Header::new(AUTH_METHOD_ID, 0, 0, token.len() as u32);
        let mut bytes = header.encode().to_vec();
        bytes.extend_from_slice(token);
        bytes
    }

    #[tokio::test]
    async fn test_auth_handshake_accepts_matching_token() {
        use tokio::io::AsyncWriteExt;

        let token = "deadbeef".repeat(8); // 64-char hex-like token
        let (mut parent, mut child) = tokio::io::duplex(4096);
        parent
            .write_all(&auth_frame_bytes(token.as_bytes()))
            .await
            .unwrap();

        let outcome =
            Client::run_auth_handshake(&mut child, &token, DEFAULT_MAX_PAYLOAD_SIZE).await;

        match outcome {
            AuthOutcome::Adopted { pending_frames, .. } => {
                assert!(pending_frames.is_empty());
            }
            AuthOutcome::Rejected => panic!("expected adoption with a matching token"),
        }
    }

    #[tokio::test]
    async fn test_auth_handshake_rejects_wrong_token() {
        use tokio::io::AsyncWriteExt;

        let (mut parent, mut child) = tokio::io::duplex(4096);
        parent
            .write_all(&auth_frame_bytes(b"the-wrong-token"))
            .await
            .unwrap();

        let outcome =
            Client::run_auth_handshake(&mut child, "the-expected-token", DEFAULT_MAX_PAYLOAD_SIZE)
                .await;

        assert!(matches!(outcome, AuthOutcome::Rejected));
    }

    #[tokio::test]
    async fn test_auth_handshake_rejects_non_auth_first_frame() {
        use tokio::io::AsyncWriteExt;

        // A normal request as the first frame must be rejected before adoption.
        let payload = crate::codec::MsgPackCodec::encode(&"hi").unwrap();
        let header = Header::new(1, 0, 7, payload.len() as u32);
        let mut bytes = header.encode().to_vec();
        bytes.extend_from_slice(&payload);

        let (mut parent, mut child) = tokio::io::duplex(4096);
        parent.write_all(&bytes).await.unwrap();

        let outcome =
            Client::run_auth_handshake(&mut child, "token", DEFAULT_MAX_PAYLOAD_SIZE).await;

        assert!(matches!(outcome, AuthOutcome::Rejected));
    }

    #[tokio::test]
    async fn test_auth_handshake_returns_pipelined_frames() {
        use tokio::io::AsyncWriteExt;

        let token = "token-1234";
        let mut bytes = auth_frame_bytes(token.as_bytes());

        // Pipeline a request right after the AUTH frame in the same packet.
        let payload = crate::codec::MsgPackCodec::encode(&"hi").unwrap();
        let header = Header::new(1, 0, 7, payload.len() as u32);
        bytes.extend_from_slice(&header.encode());
        bytes.extend_from_slice(&payload);

        let (mut parent, mut child) = tokio::io::duplex(4096);
        parent.write_all(&bytes).await.unwrap();

        let outcome = Client::run_auth_handshake(&mut child, token, DEFAULT_MAX_PAYLOAD_SIZE).await;

        match outcome {
            AuthOutcome::Adopted { pending_frames, .. } => {
                assert_eq!(pending_frames.len(), 1);
                assert_eq!(pending_frames[0].method_id(), 1);
                assert_eq!(pending_frames[0].request_id(), 7);
            }
            AuthOutcome::Rejected => panic!("expected adoption with pipelined request"),
        }
    }

    #[tokio::test]
    async fn test_auth_handshake_rejects_on_eof() {
        // Connection closes before sending the AUTH frame.
        let (parent, mut child) = tokio::io::duplex(4096);
        drop(parent); // EOF

        let outcome =
            Client::run_auth_handshake(&mut child, "token", DEFAULT_MAX_PAYLOAD_SIZE).await;

        assert!(matches!(outcome, AuthOutcome::Rejected));
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
