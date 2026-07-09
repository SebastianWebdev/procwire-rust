//! Request context for handlers.
//!
//! Provides methods for responding to requests:
//! - `respond` - send a single response
//! - `ack` - send acknowledgment
//! - `chunk` - send a stream chunk
//! - `end` - end a stream (empty payload)
//! - `error` - send an error response
//!
//! # Cancellation Support
//!
//! Handlers can check for and respond to ABORT signals from the parent:
//! - `is_cancelled()` - check if request was aborted
//! - `cancelled().await` - wait for cancellation (use with `tokio::select!`)
//! - `cancellation_token()` - get token for child tasks
//!
//! # Example
//!
//! ```ignore
//! async fn echo_handler(data: String, ctx: RequestContext) -> Result<()> {
//!     ctx.respond(&data).await
//! }
//!
//! async fn stream_handler(count: i32, ctx: RequestContext) -> Result<()> {
//!     for i in 0..count {
//!         // Check for cancellation
//!         if ctx.is_cancelled() {
//!             return Ok(());
//!         }
//!         ctx.chunk(&i).await?;
//!     }
//!     ctx.end().await
//! }
//!
//! async fn long_task(data: Input, ctx: RequestContext) -> Result<()> {
//!     tokio::select! {
//!         _ = ctx.cancelled() => {
//!             // Request was aborted, clean up
//!             return Ok(());
//!         }
//!         result = do_work() => {
//!             ctx.respond(&result).await
//!         }
//!     }
//! }
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use tokio_util::sync::CancellationToken;

use crate::codec::MsgPackCodec;
use crate::control::ResponseType;
use crate::error::{ProcwireError, Result};
use crate::protocol::{flags, Header};
use crate::writer::{OutboundFrame, WriterHandle};

/// Context passed to request handlers.
///
/// Provides methods for sending responses back to the parent.
/// All response methods handle serialization and frame building internally.
///
/// # Thread Safety
///
/// `RequestContext` is `Clone` and can be safely shared across async tasks.
/// The underlying writer uses a channel-based architecture that eliminates
/// lock contention.
///
/// # Cancellation
///
/// Each context has a [`CancellationToken`] that is triggered when the parent
/// sends an ABORT signal. Handlers should check `is_cancelled()` periodically
/// or use `cancelled().await` with `tokio::select!` for immediate response.
#[derive(Clone)]
pub struct RequestContext {
    /// Method ID for this request.
    method_id: u16,
    /// Request ID for this request (0 = event).
    request_id: u32,
    /// The method's response type. Only `Stream` changes behaviour: it makes
    /// `error()` tag the error frame with `IS_STREAM` so the parent routes it
    /// to the pending stream instead of the pending-request table.
    response_type: ResponseType,
    /// Writer handle for sending responses.
    writer: Option<WriterHandle>,
    /// Cancellation token for ABORT handling.
    cancellation_token: CancellationToken,
    /// Whether a terminal response (`respond`/`ack`/`end`/`error`) was sent.
    /// Shared across clones so the single-terminal-response rule holds for the
    /// request, not per context handle.
    responded: Arc<AtomicBool>,
}

impl RequestContext {
    /// Create a new request context (for testing without writer).
    pub fn new(method_id: u16, request_id: u32) -> Self {
        Self {
            method_id,
            request_id,
            response_type: ResponseType::Result,
            writer: None,
            cancellation_token: CancellationToken::new(),
            responded: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a new request context with a writer.
    pub fn with_writer(method_id: u16, request_id: u32, writer: WriterHandle) -> Self {
        Self {
            method_id,
            request_id,
            response_type: ResponseType::Result,
            writer: Some(writer),
            cancellation_token: CancellationToken::new(),
            responded: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set the method's response type (builder-style).
    ///
    /// The runtime sets this from the handler registration; `Stream` makes
    /// [`Self::error`] answer on the stream channel (flags `0x0F`).
    #[must_use]
    pub fn with_response_type(mut self, response_type: ResponseType) -> Self {
        self.response_type = response_type;
        self
    }

    /// Create a new request context with a writer and cancellation token.
    ///
    /// Used internally when tracking active contexts for ABORT handling.
    pub(crate) fn with_writer_and_token(
        method_id: u16,
        request_id: u32,
        response_type: ResponseType,
        writer: WriterHandle,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            method_id,
            request_id,
            response_type,
            writer: Some(writer),
            cancellation_token,
            responded: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the method ID.
    #[inline]
    pub fn method_id(&self) -> u16 {
        self.method_id
    }

    /// Get the request ID.
    #[inline]
    pub fn request_id(&self) -> u32 {
        self.request_id
    }

    /// Get the method's response type.
    #[inline]
    pub fn response_type(&self) -> ResponseType {
        self.response_type
    }

    /// Whether a terminal response (`respond`/`ack`/`end`/`error`) was already
    /// sent for this request.
    #[inline]
    pub fn has_responded(&self) -> bool {
        self.responded.load(Ordering::SeqCst)
    }

    /// Atomically mark the terminal response as sent.
    ///
    /// Mirrors the Node client's `_ensureNotResponded`: the flag is set even if
    /// the subsequent write fails, so no second terminal frame can follow.
    fn mark_responded(&self) -> Result<()> {
        if self.responded.swap(true, Ordering::SeqCst) {
            return Err(ProcwireError::ResponseAlreadySent);
        }
        Ok(())
    }

    /// Check if this request has been cancelled.
    ///
    /// Handlers should check this periodically during long operations.
    ///
    /// # Example
    ///
    /// ```ignore
    /// for i in 0..1000 {
    ///     if ctx.is_cancelled() {
    ///         tracing::info!("Request cancelled at step {}", i);
    ///         return Ok(());
    ///     }
    ///     do_step(i).await;
    /// }
    /// ```
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    /// Wait for cancellation.
    ///
    /// Use with `tokio::select!` to handle cancellation immediately:
    ///
    /// # Example
    ///
    /// ```ignore
    /// tokio::select! {
    ///     _ = ctx.cancelled() => {
    ///         // Request was cancelled, clean up
    ///         return Ok(());
    ///     }
    ///     result = do_work() => {
    ///         ctx.respond(&result).await
    ///     }
    /// }
    /// ```
    pub fn cancelled(&self) -> tokio_util::sync::WaitForCancellationFuture<'_> {
        self.cancellation_token.cancelled()
    }

    /// Get the cancellation token for advanced use cases.
    ///
    /// Useful when you need to pass the token to child tasks.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let token = ctx.cancellation_token();
    /// let handle = tokio::spawn(async move {
    ///     tokio::select! {
    ///         _ = token.cancelled() => None,
    ///         result = do_work() => Some(result),
    ///     }
    /// });
    /// ```
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Cancel this request (internal use).
    ///
    /// Called when an ABORT frame is received for this request.
    /// Currently used only in tests, but kept for potential future use.
    #[allow(dead_code)]
    pub(crate) fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    /// Send a response with the given payload.
    ///
    /// Serializes the payload using MsgPack and sends a response frame.
    /// Terminal: at most one of `respond`/`ack`/`end`/`error` per request,
    /// otherwise [`ProcwireError::ResponseAlreadySent`].
    pub async fn respond<T: serde::Serialize>(&self, payload: &T) -> Result<()> {
        self.mark_responded()?;
        let data = MsgPackCodec::encode(payload)?;
        self.send_frame(flags::RESPONSE, Bytes::from(data)).await
    }

    /// Send a response with raw bytes (zero-copy). Terminal (see [`Self::respond`]).
    pub async fn respond_raw(&self, payload: &[u8]) -> Result<()> {
        self.mark_responded()?;
        self.send_frame(flags::RESPONSE, Bytes::copy_from_slice(payload))
            .await
    }

    /// Send a response with pre-allocated Bytes (zero-copy). Terminal (see [`Self::respond`]).
    pub async fn respond_bytes(&self, payload: Bytes) -> Result<()> {
        self.mark_responded()?;
        self.send_frame(flags::RESPONSE, payload).await
    }

    /// Send an acknowledgment (empty payload). Terminal (see [`Self::respond`]).
    pub async fn ack(&self) -> Result<()> {
        self.mark_responded()?;
        self.send_frame_empty(flags::ACK_RESPONSE).await
    }

    /// Send a stream chunk.
    ///
    /// Serializes the payload using MsgPack and sends a stream chunk frame.
    pub async fn chunk<T: serde::Serialize>(&self, payload: &T) -> Result<()> {
        let data = MsgPackCodec::encode(payload)?;
        self.send_frame(flags::STREAM_CHUNK, Bytes::from(data))
            .await
    }

    /// Send a stream chunk with raw bytes.
    pub async fn chunk_raw(&self, payload: &[u8]) -> Result<()> {
        self.send_frame(flags::STREAM_CHUNK, Bytes::copy_from_slice(payload))
            .await
    }

    /// Send a stream chunk with pre-allocated Bytes (zero-copy).
    pub async fn chunk_bytes(&self, payload: Bytes) -> Result<()> {
        self.send_frame(flags::STREAM_CHUNK, payload).await
    }

    /// End a stream.
    ///
    /// Sends a stream end frame with empty payload. Terminal (see [`Self::respond`]).
    /// **IMPORTANT**: STREAM_END frames always have empty payload!
    pub async fn end(&self) -> Result<()> {
        self.mark_responded()?;
        // NOTE: STREAM_END always has empty payload (payloadLength=0)
        self.send_frame_empty(flags::STREAM_END_RESPONSE).await
    }

    /// Flags for an error frame on this method.
    ///
    /// A `stream` method must answer errors on the stream channel: without
    /// `IS_STREAM` the parent routes the frame to its pending-request table,
    /// the lookup misses, and — since streams have no timeout — the consumer's
    /// iteration hangs forever.
    fn error_flags(&self) -> u8 {
        if self.response_type == ResponseType::Stream {
            flags::STREAM_ERROR_RESPONSE
        } else {
            flags::ERROR_RESPONSE
        }
    }

    /// Send an error response with a plain string message.
    ///
    /// Serializes the error message and sends an error frame. The parent
    /// surfaces the string as the rejection reason.
    ///
    /// The error frame is TERMINAL: it ends the request/stream by itself
    /// (no `STREAM_END` after it), and no further frame may follow for this
    /// request. The payload is always MessagePack, independent of the method's
    /// data codec — the parent decodes `IS_ERROR` payloads with the same fixed
    /// codec.
    pub async fn error(&self, message: &str) -> Result<()> {
        self.mark_responded()?;
        let data = MsgPackCodec::encode(&message)?;
        self.send_frame(self.error_flags(), Bytes::from(data)).await
    }

    /// Send a structured error response.
    ///
    /// Serializes any [`serde::Serialize`] value as the error payload. The
    /// parent derives its message from a string `message` field if present and
    /// preserves the whole object on `error.data`, so prefer an object shaped
    /// like `{ "message": "...", "code": ... }`. A plain string ([`Self::error`])
    /// also remains valid. Terminal, like [`Self::error`].
    pub async fn error_with<T: serde::Serialize>(&self, error: &T) -> Result<()> {
        self.mark_responded()?;
        let data = MsgPackCodec::encode(error)?;
        self.send_frame(self.error_flags(), Bytes::from(data)).await
    }

    /// Send a frame with the given flags and payload.
    async fn send_frame(&self, frame_flags: u8, payload: Bytes) -> Result<()> {
        let writer = match &self.writer {
            Some(w) => w,
            None => {
                // No writer configured (testing mode)
                return Ok(());
            }
        };

        let header = Header::new(
            self.method_id,
            frame_flags,
            self.request_id,
            payload.len() as u32,
        );

        let frame = OutboundFrame::new(&header, payload);
        writer.send(frame).await
    }

    /// Send a frame with empty payload.
    async fn send_frame_empty(&self, frame_flags: u8) -> Result<()> {
        let writer = match &self.writer {
            Some(w) => w,
            None => {
                // No writer configured (testing mode)
                return Ok(());
            }
        };

        let header = Header::new(self.method_id, frame_flags, self.request_id, 0);

        let frame = OutboundFrame::empty(&header);
        writer.send(frame).await
    }
}

/// Wrapper for Bytes payload (zero-copy).
pub struct RawPayload(pub Bytes);

impl RawPayload {
    /// Create from bytes.
    pub fn new(bytes: Bytes) -> Self {
        Self(bytes)
    }

    /// Get the bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Into bytes.
    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let ctx = RequestContext::new(1, 42);
        assert_eq!(ctx.method_id(), 1);
        assert_eq!(ctx.request_id(), 42);
    }

    #[tokio::test]
    async fn test_respond_without_writer() {
        let ctx = RequestContext::new(1, 42);
        // Should not panic, just return Ok
        let result = ctx.respond(&"test").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_all_response_methods_without_writer() {
        // Terminal methods each need a fresh context (one terminal response
        // per request); chunks are non-terminal and may repeat.
        assert!(RequestContext::new(1, 42).respond(&"test").await.is_ok());
        assert!(RequestContext::new(1, 42)
            .respond_raw(b"test")
            .await
            .is_ok());
        assert!(RequestContext::new(1, 42)
            .respond_bytes(Bytes::from_static(b"test"))
            .await
            .is_ok());
        assert!(RequestContext::new(1, 42).ack().await.is_ok());
        assert!(RequestContext::new(1, 42)
            .error("error message")
            .await
            .is_ok());

        let ctx = RequestContext::new(1, 42);
        assert!(ctx.chunk(&1i32).await.is_ok());
        assert!(ctx.chunk_raw(b"chunk").await.is_ok());
        assert!(ctx.chunk_bytes(Bytes::from_static(b"chunk")).await.is_ok());
        assert!(ctx.end().await.is_ok());
    }

    #[tokio::test]
    async fn test_second_terminal_response_is_rejected() {
        use crate::error::ProcwireError;

        let ctx = RequestContext::new(1, 42);
        assert!(ctx.respond(&"first").await.is_ok());
        assert!(ctx.has_responded());

        // Any further terminal response must fail with ResponseAlreadySent.
        assert!(matches!(
            ctx.respond(&"second").await,
            Err(ProcwireError::ResponseAlreadySent)
        ));
        assert!(matches!(
            ctx.ack().await,
            Err(ProcwireError::ResponseAlreadySent)
        ));
        assert!(matches!(
            ctx.end().await,
            Err(ProcwireError::ResponseAlreadySent)
        ));
        assert!(matches!(
            ctx.error("boom").await,
            Err(ProcwireError::ResponseAlreadySent)
        ));
    }

    #[tokio::test]
    async fn test_error_is_terminal_for_stream() {
        use crate::error::ProcwireError;

        // A stream ends with EITHER a STREAM_END frame OR an error frame —
        // never both. end() after error() must be rejected.
        let ctx = RequestContext::new(1, 42).with_response_type(ResponseType::Stream);
        ctx.chunk(&1i32).await.unwrap();
        ctx.error("boom").await.unwrap();
        assert!(matches!(
            ctx.end().await,
            Err(ProcwireError::ResponseAlreadySent)
        ));
    }

    #[tokio::test]
    async fn test_responded_state_is_shared_across_clones() {
        let ctx = RequestContext::new(1, 42);
        let ctx2 = ctx.clone();

        ctx.respond(&"done").await.unwrap();
        assert!(ctx2.has_responded());
        assert!(ctx2.respond(&"again").await.is_err());
    }

    #[tokio::test]
    async fn test_error_with_structured_payload() {
        #[derive(serde::Serialize)]
        struct StructuredError {
            message: String,
            code: i32,
        }

        let ctx = RequestContext::new(1, 42);
        let err = StructuredError {
            message: "boom".to_string(),
            code: 500,
        };
        assert!(ctx.error_with(&err).await.is_ok());
    }

    #[tokio::test]
    async fn test_chunk_allows_multiple_calls() {
        let ctx = RequestContext::new(1, 42);

        // Multiple chunks should all succeed
        assert!(ctx.chunk(&1i32).await.is_ok());
        assert!(ctx.chunk(&2i32).await.is_ok());
        assert!(ctx.chunk(&3i32).await.is_ok());
        assert!(ctx.end().await.is_ok());
    }

    #[tokio::test]
    async fn test_end_can_be_called_after_chunks() {
        let ctx = RequestContext::new(1, 42);

        ctx.chunk(&"first").await.unwrap();
        ctx.chunk(&"second").await.unwrap();
        ctx.end().await.unwrap();
    }

    #[test]
    fn test_context_is_clone() {
        let ctx = RequestContext::new(1, 42);
        let ctx2 = ctx.clone();

        assert_eq!(ctx.method_id(), ctx2.method_id());
        assert_eq!(ctx.request_id(), ctx2.request_id());
    }

    #[test]
    fn test_raw_payload() {
        let data = Bytes::from_static(b"hello world");
        let payload = RawPayload::new(data.clone());

        assert_eq!(payload.as_bytes(), b"hello world");
        assert_eq!(payload.into_bytes(), data);
    }

    #[tokio::test]
    async fn test_context_with_writer() {
        use crate::writer::spawn_writer_task_default;
        use tokio::io::duplex;

        let (client, _server) = duplex(4096);
        let (writer_handle, _task) = spawn_writer_task_default(client);

        // A stream sequence: chunks then a single terminal end().
        let ctx = RequestContext::with_writer(1, 42, writer_handle)
            .with_response_type(ResponseType::Stream);
        assert!(ctx.chunk(&123i32).await.is_ok());
        assert!(ctx.end().await.is_ok());
    }

    /// Read frames from the far end of a duplex pipe until `count` are parsed.
    async fn read_frames<R: tokio::io::AsyncRead + Unpin>(
        reader: &mut R,
        count: usize,
    ) -> Vec<crate::protocol::Frame> {
        use crate::protocol::FrameBuffer;
        use tokio::io::AsyncReadExt;

        let mut frame_buffer = FrameBuffer::new();
        let mut frames = Vec::new();
        let mut buf = vec![0u8; 4096];
        while frames.len() < count {
            let n = reader.read(&mut buf).await.expect("read failed");
            assert!(n > 0, "connection closed before frames arrived");
            frames.extend(frame_buffer.push(&buf[..n]).expect("framing error"));
        }
        frames
    }

    #[tokio::test]
    async fn test_error_flags_for_result_method() {
        use crate::writer::spawn_writer_task_default;
        use tokio::io::duplex;

        let (client, mut server) = duplex(4096);
        let (writer_handle, _task) = spawn_writer_task_default(client);

        let ctx = RequestContext::with_writer(7, 42, writer_handle);
        ctx.error("boom").await.unwrap();

        let frames = read_frames(&mut server, 1).await;
        let header = &frames[0].header;
        assert_eq!(header.flags, flags::ERROR_RESPONSE); // 0x07
        assert_eq!(header.method_id, 7);
        assert_eq!(header.request_id, 42);
        // Error payload is always MessagePack: "boom" -> fixstr, 5 bytes.
        assert_eq!(frames[0].payload.as_ref(), b"\xA4boom");
    }

    #[tokio::test]
    async fn test_error_flags_for_stream_method_include_is_stream() {
        use crate::writer::spawn_writer_task_default;
        use tokio::io::duplex;

        let (client, mut server) = duplex(4096);
        let (writer_handle, _task) = spawn_writer_task_default(client);

        let ctx = RequestContext::with_writer(7, 42, writer_handle)
            .with_response_type(ResponseType::Stream);
        ctx.error("boom").await.unwrap();

        let frames = read_frames(&mut server, 1).await;
        let header = &frames[0].header;
        // Stream errors answer on the stream channel: 0x0F, NOT 0x07, and
        // never with STREAM_END (the error frame is terminal by itself).
        assert_eq!(header.flags, flags::STREAM_ERROR_RESPONSE); // 0x0F
        assert!(header.is_stream());
        assert!(header.is_error());
        assert!(!header.is_stream_end());
    }

    #[tokio::test]
    async fn test_error_with_structured_payload_on_stream_method() {
        use crate::writer::spawn_writer_task_default;
        use tokio::io::duplex;

        #[derive(serde::Serialize)]
        struct StructuredError {
            message: String,
            code: i32,
        }

        let (client, mut server) = duplex(4096);
        let (writer_handle, _task) = spawn_writer_task_default(client);

        let ctx = RequestContext::with_writer(7, 42, writer_handle)
            .with_response_type(ResponseType::Stream);
        ctx.error_with(&StructuredError {
            message: "boom".to_string(),
            code: 500,
        })
        .await
        .unwrap();

        let frames = read_frames(&mut server, 1).await;
        assert_eq!(frames[0].header.flags, flags::STREAM_ERROR_RESPONSE);
        // Struct-as-map (to_vec_named): fixmap(2) leading byte.
        assert_eq!(frames[0].payload[0], 0x82);
    }

    #[test]
    fn test_cancellation_token_initially_not_cancelled() {
        let ctx = RequestContext::new(1, 42);
        assert!(!ctx.is_cancelled());
    }

    #[test]
    fn test_cancellation_after_cancel() {
        let ctx = RequestContext::new(1, 42);
        assert!(!ctx.is_cancelled());

        ctx.cancel();

        assert!(ctx.is_cancelled());
    }

    #[test]
    fn test_cancellation_propagates_to_clones() {
        let ctx = RequestContext::new(1, 42);
        let ctx_clone = ctx.clone();

        assert!(!ctx.is_cancelled());
        assert!(!ctx_clone.is_cancelled());

        ctx.cancel();

        // Both should see cancellation
        assert!(ctx.is_cancelled());
        assert!(ctx_clone.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancelled_future_completes_after_cancel() {
        use std::time::Duration;

        let ctx = RequestContext::new(1, 42);
        let ctx_clone = ctx.clone();

        // Spawn task that cancels after delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            ctx_clone.cancel();
        });

        // This should complete after cancellation
        tokio::time::timeout(Duration::from_millis(100), ctx.cancelled())
            .await
            .expect("cancelled() should complete within timeout");
    }

    #[tokio::test]
    async fn test_cancellation_token_can_be_passed_to_child_task() {
        use std::time::Duration;

        let ctx = RequestContext::new(1, 42);
        let token = ctx.cancellation_token();

        // Spawn child task with token
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => "cancelled",
                _ = tokio::time::sleep(Duration::from_secs(10)) => "timeout",
            }
        });

        // Cancel the context
        tokio::time::sleep(Duration::from_millis(10)).await;
        ctx.cancel();

        // Child should see cancellation
        let result = tokio::time::timeout(Duration::from_millis(100), handle)
            .await
            .expect("task should complete")
            .expect("task should not panic");

        assert_eq!(result, "cancelled");
    }

    #[tokio::test]
    async fn test_with_writer_and_token() {
        use crate::writer::spawn_writer_task_default;
        use tokio::io::duplex;

        let (client, _server) = duplex(4096);
        let (writer_handle, _task) = spawn_writer_task_default(client);

        let token = CancellationToken::new();
        let ctx = RequestContext::with_writer_and_token(
            1,
            42,
            ResponseType::Result,
            writer_handle,
            token.clone(),
        );

        assert!(!ctx.is_cancelled());

        token.cancel();

        assert!(ctx.is_cancelled());
    }
}
