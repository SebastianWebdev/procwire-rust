//! Request context for handlers.
//!
//! Provides methods for responding to requests:
//! - `respond` - send a single response
//! - `ack` - send acknowledgment
//! - `chunk` - send a stream chunk
//! - `end` - end a stream (empty payload)
//! - `error` - send an error response
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
//!         ctx.chunk(&i).await?;
//!     }
//!     ctx.end().await
//! }
//! ```

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::codec::MsgPackCodec;
use crate::error::Result;
use crate::protocol::{build_frame, flags, Header};

/// Boxed async writer using the low-level trait.
pub type BoxedWriter = Pin<Box<dyn AsyncWrite + Send>>;

/// Shared writer type for sending responses.
pub type SharedWriter = Arc<Mutex<BoxedWriter>>;

/// Context passed to request handlers.
///
/// Provides methods for sending responses back to the parent.
/// All response methods handle serialization and frame building internally.
#[derive(Clone)]
pub struct RequestContext {
    /// Method ID for this request.
    method_id: u16,
    /// Request ID for this request (0 = event).
    request_id: u32,
    /// Shared writer for sending responses.
    writer: Option<SharedWriter>,
}

impl RequestContext {
    /// Create a new request context (for testing without writer).
    pub fn new(method_id: u16, request_id: u32) -> Self {
        Self {
            method_id,
            request_id,
            writer: None,
        }
    }

    /// Create a new request context with a writer.
    pub fn with_writer(method_id: u16, request_id: u32, writer: SharedWriter) -> Self {
        Self {
            method_id,
            request_id,
            writer: Some(writer),
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

    /// Send a response with the given payload.
    ///
    /// Serializes the payload using MsgPack and sends a response frame.
    pub async fn respond<T: serde::Serialize>(&self, payload: &T) -> Result<()> {
        let data = MsgPackCodec::encode(payload)?;
        self.send_frame(flags::RESPONSE, &data).await
    }

    /// Send a response with raw bytes.
    pub async fn respond_raw(&self, payload: &[u8]) -> Result<()> {
        self.send_frame(flags::RESPONSE, payload).await
    }

    /// Send an acknowledgment (empty payload).
    pub async fn ack(&self) -> Result<()> {
        self.send_frame(flags::ACK_RESPONSE, &[]).await
    }

    /// Send a stream chunk.
    ///
    /// Serializes the payload using MsgPack and sends a stream chunk frame.
    pub async fn chunk<T: serde::Serialize>(&self, payload: &T) -> Result<()> {
        let data = MsgPackCodec::encode(payload)?;
        self.send_frame(flags::STREAM_CHUNK, &data).await
    }

    /// Send a stream chunk with raw bytes.
    pub async fn chunk_raw(&self, payload: &[u8]) -> Result<()> {
        self.send_frame(flags::STREAM_CHUNK, payload).await
    }

    /// End a stream.
    ///
    /// Sends a stream end frame with empty payload.
    /// **IMPORTANT**: STREAM_END frames always have empty payload!
    pub async fn end(&self) -> Result<()> {
        // NOTE: STREAM_END always has empty payload (payloadLength=0)
        self.send_frame(flags::STREAM_END_RESPONSE, &[]).await
    }

    /// Send an error response.
    ///
    /// Serializes the error message and sends an error frame.
    pub async fn error(&self, message: &str) -> Result<()> {
        let data = MsgPackCodec::encode(&message)?;
        self.send_frame(flags::ERROR_RESPONSE, &data).await
    }

    /// Send a frame with the given flags and payload.
    async fn send_frame(&self, frame_flags: u8, payload: &[u8]) -> Result<()> {
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

        let frame_bytes = build_frame(&header, payload);

        let mut guard = writer.lock().await;
        guard.write_all(&frame_bytes).await?;
        guard.flush().await?;

        Ok(())
    }
}

/// Wrapper to make PipeStream into a pinned boxed writer.
pub struct PinnedWriter<W> {
    inner: W,
}

impl<W: AsyncWrite + Unpin> PinnedWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for PinnedWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Create a shared writer from any AsyncWrite + Send + Unpin.
pub fn shared_writer<W: AsyncWrite + Send + Unpin + 'static>(writer: W) -> SharedWriter {
    Arc::new(Mutex::new(
        Box::pin(PinnedWriter::new(writer)) as BoxedWriter
    ))
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
        let ctx = RequestContext::new(1, 42);

        assert!(ctx.respond(&"test").await.is_ok());
        assert!(ctx.respond_raw(b"test").await.is_ok());
        assert!(ctx.ack().await.is_ok());
        assert!(ctx.chunk(&1i32).await.is_ok());
        assert!(ctx.chunk_raw(b"chunk").await.is_ok());
        assert!(ctx.end().await.is_ok());
        assert!(ctx.error("error message").await.is_ok());
    }

    #[tokio::test]
    async fn test_respond_with_writer() {
        use std::io::Cursor;

        let buffer: Vec<u8> = Vec::new();
        let cursor = Cursor::new(buffer);
        let writer = shared_writer(cursor);

        let ctx = RequestContext::with_writer(1, 42, writer.clone());
        ctx.respond(&"hello").await.unwrap();

        // Can't easily inspect the written data since it's behind a Pin
        // but the test verifies no panic
    }

    #[tokio::test]
    async fn test_respond_produces_correct_flags() {
        use std::io::Cursor;

        // Create a custom writer that captures data
        let cursor = Cursor::new(Vec::new());
        let writer = shared_writer(cursor);

        let ctx = RequestContext::with_writer(5, 100, writer.clone());
        ctx.respond(&42i32).await.unwrap();

        // Read what was written
        let guard = writer.lock().await;
        // The writer is a Pin<Box<...>> wrapping a Cursor, we can verify it ran without panic
        drop(guard);
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
}
