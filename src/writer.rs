//! Dedicated writer task for high-throughput frame sending.
//!
//! This module replaces the `Arc<Mutex<BoxedWriter>>` pattern with a dedicated
//! writer task that receives frames via an mpsc channel. This eliminates lock
//! contention and enables batching multiple frames into single syscalls.
//!
//! # Architecture
//!
//! ```text
//! Handler 1 ─┐
//! Handler 2 ─┼─► mpsc::Sender<OutboundFrame> ─► Writer Task ─► Pipe
//! Handler N ─┘
//! ```
//!
//! # Benefits
//!
//! - **No lock contention**: Channel-based, not mutex-based
//! - **Batching**: Multiple frames can be written in a single syscall via writev
//! - **Backpressure**: Built-in pending count tracking with configurable limits

use std::io::IoSlice;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::error::{ProcwireError, Result};
use crate::protocol::{Header, HEADER_SIZE};

/// Default maximum pending frames before backpressure kicks in.
pub const DEFAULT_MAX_PENDING_FRAMES: usize = 1024;

/// Default channel capacity.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 1024;

/// Default backpressure timeout.
pub const DEFAULT_BACKPRESSURE_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum frames to batch in a single write operation.
const MAX_BATCH_SIZE: usize = 64;

/// A frame ready to be written to the pipe.
#[derive(Debug)]
pub struct OutboundFrame {
    /// Pre-encoded header (11 bytes).
    pub header: [u8; HEADER_SIZE],
    /// Payload bytes (can be empty for ACK, STREAM_END, etc.).
    pub payload: Bytes,
}

impl OutboundFrame {
    /// Create a new outbound frame.
    #[inline]
    pub fn new(header: &Header, payload: Bytes) -> Self {
        Self {
            header: header.encode(),
            payload,
        }
    }

    /// Create a new outbound frame with empty payload.
    #[inline]
    pub fn empty(header: &Header) -> Self {
        Self {
            header: header.encode(),
            payload: Bytes::new(),
        }
    }

    /// Total size of this frame (header + payload).
    #[inline]
    pub fn size(&self) -> usize {
        HEADER_SIZE + self.payload.len()
    }
}

/// Configuration for the writer task.
#[derive(Debug, Clone)]
pub struct WriterConfig {
    /// Maximum pending frames before backpressure kicks in.
    pub max_pending_frames: usize,
    /// Channel capacity for frame queue.
    pub channel_capacity: usize,
    /// Timeout when waiting for backpressure to clear.
    pub backpressure_timeout: Duration,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            max_pending_frames: DEFAULT_MAX_PENDING_FRAMES,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            backpressure_timeout: DEFAULT_BACKPRESSURE_TIMEOUT,
        }
    }
}

/// Handle for sending frames to the writer task.
///
/// This is cheaply cloneable and can be shared across multiple handlers.
#[derive(Clone)]
pub struct WriterHandle {
    /// Channel sender for frames.
    tx: mpsc::Sender<OutboundFrame>,
    /// Pending frame count (for backpressure).
    pending: Arc<AtomicUsize>,
    /// Maximum pending frames.
    max_pending: usize,
    /// Backpressure timeout.
    timeout: Duration,
}

impl WriterHandle {
    /// Create a new writer handle.
    fn new(
        tx: mpsc::Sender<OutboundFrame>,
        pending: Arc<AtomicUsize>,
        max_pending: usize,
        timeout: Duration,
    ) -> Self {
        Self {
            tx,
            pending,
            max_pending,
            timeout,
        }
    }

    /// Send a frame to the writer task.
    ///
    /// This method will wait if backpressure is active, timing out after
    /// the configured duration.
    pub async fn send(&self, frame: OutboundFrame) -> Result<()> {
        // Check backpressure
        let current = self.pending.load(Ordering::Acquire);
        if current >= self.max_pending {
            // Wait with timeout for backpressure to clear
            self.wait_for_backpressure().await?;
        }

        // Increment pending count BEFORE sending
        self.pending.fetch_add(1, Ordering::AcqRel);

        // Send to channel
        self.tx.send(frame).await.map_err(|_| {
            // Decrement on failure
            self.pending.fetch_sub(1, Ordering::Release);
            ProcwireError::ConnectionClosed
        })
    }

    /// Wait for backpressure to clear with timeout.
    async fn wait_for_backpressure(&self) -> Result<()> {
        let start = Instant::now();
        let check_interval = Duration::from_micros(100);

        loop {
            if self.pending.load(Ordering::Acquire) < self.max_pending {
                return Ok(());
            }

            if start.elapsed() > self.timeout {
                return Err(ProcwireError::BackpressureTimeout);
            }

            tokio::time::sleep(check_interval).await;
        }
    }

    /// Check if backpressure is currently active.
    #[inline]
    pub fn is_backpressure_active(&self) -> bool {
        self.pending.load(Ordering::Acquire) >= self.max_pending
    }

    /// Get current pending frame count.
    #[inline]
    pub fn pending_count(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }

    /// Try to send a frame without waiting for backpressure.
    ///
    /// Returns `Err(BackpressureTimeout)` immediately if at capacity.
    pub fn try_send(&self, frame: OutboundFrame) -> Result<()> {
        let current = self.pending.load(Ordering::Acquire);
        if current >= self.max_pending {
            return Err(ProcwireError::BackpressureTimeout);
        }

        self.pending.fetch_add(1, Ordering::AcqRel);

        self.tx.try_send(frame).map_err(|e| {
            self.pending.fetch_sub(1, Ordering::Release);
            match e {
                mpsc::error::TrySendError::Full(_) => ProcwireError::BackpressureTimeout,
                mpsc::error::TrySendError::Closed(_) => ProcwireError::ConnectionClosed,
            }
        })
    }
}

/// Spawn the writer task and return a handle for sending frames.
///
/// # Arguments
///
/// * `writer` - The async writer (pipe write half)
/// * `config` - Writer configuration
///
/// # Returns
///
/// A tuple of `(WriterHandle, JoinHandle)` where the JoinHandle can be used
/// to wait for the writer task to complete.
pub fn spawn_writer_task<W>(
    writer: W,
    config: WriterConfig,
) -> (WriterHandle, JoinHandle<Result<()>>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (tx, rx) = mpsc::channel(config.channel_capacity);
    let pending = Arc::new(AtomicUsize::new(0));

    let handle = WriterHandle::new(
        tx,
        pending.clone(),
        config.max_pending_frames,
        config.backpressure_timeout,
    );

    let task = tokio::spawn(writer_loop(rx, writer, pending));

    (handle, task)
}

/// Spawn the writer task with default configuration.
pub fn spawn_writer_task_default<W>(writer: W) -> (WriterHandle, JoinHandle<Result<()>>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    spawn_writer_task(writer, WriterConfig::default())
}

/// Main writer loop - receives frames and writes them to the pipe.
///
/// Uses batching and scatter/gather I/O (writev) for efficiency.
async fn writer_loop<W>(
    mut rx: mpsc::Receiver<OutboundFrame>,
    mut writer: W,
    pending: Arc<AtomicUsize>,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    loop {
        // Wait for first frame
        let first = match rx.recv().await {
            Some(f) => f,
            None => {
                // Channel closed, clean shutdown
                return Ok(());
            }
        };

        // Collect additional ready frames (non-blocking)
        let mut batch = Vec::with_capacity(MAX_BATCH_SIZE);
        batch.push(first);

        while batch.len() < MAX_BATCH_SIZE {
            match rx.try_recv() {
                Ok(frame) => batch.push(frame),
                Err(_) => break,
            }
        }

        // Write the batch
        let batch_size = batch.len();
        write_batch(&mut writer, &batch).await?;

        // Update pending count
        pending.fetch_sub(batch_size, Ordering::Release);
    }
}

/// Write a batch of frames using scatter/gather I/O (write_vectored).
///
/// Always uses write_vectored for both single and multiple frames to minimize
/// syscalls. For a single frame with payload, this reduces from 2-3 syscalls
/// (header write, payload write, flush) to 1-2 syscalls (vectored write, flush).
async fn write_batch<W>(writer: &mut W, batch: &[OutboundFrame]) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    if batch.is_empty() {
        return Ok(());
    }

    // Build IoSlice array: each frame contributes 1-2 slices (header, optionally payload)
    // Using write_vectored even for single frame to minimize syscalls
    let mut slices: Vec<IoSlice<'_>> = Vec::with_capacity(batch.len() * 2);

    for frame in batch {
        slices.push(IoSlice::new(&frame.header));
        if !frame.payload.is_empty() {
            slices.push(IoSlice::new(&frame.payload));
        }
    }

    // Calculate total size
    let total_size: usize = batch.iter().map(|f| f.size()).sum();

    // Fast path: try single write_vectored call first
    // This is the common case when kernel buffer has enough space
    let written = writer.write_vectored(&slices).await?;

    if written == total_size {
        // All data written in one syscall - optimal case
        writer.flush().await?;
        return Ok(());
    }

    if written == 0 {
        return Err(ProcwireError::Io(std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "write_vectored returned 0",
        )));
    }

    // Slow path: partial write, need to continue with remaining data
    let mut total_written = written;

    while total_written < total_size {
        // Rebuild slices for remaining data
        let remaining_slices = build_remaining_slices(batch, total_written);
        if remaining_slices.is_empty() {
            break;
        }

        let written = writer.write_vectored(&remaining_slices).await?;
        if written == 0 {
            return Err(ProcwireError::Io(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "write_vectored returned 0",
            )));
        }

        total_written += written;
    }

    writer.flush().await?;
    Ok(())
}

/// Build IoSlice array for remaining data after partial write.
fn build_remaining_slices(batch: &[OutboundFrame], skip_bytes: usize) -> Vec<IoSlice<'_>> {
    let mut slices = Vec::with_capacity(batch.len() * 2);
    let mut skipped = 0;

    for frame in batch {
        // Handle header
        let header_start = skipped;
        let header_end = skipped + HEADER_SIZE;

        if skip_bytes < header_end {
            let start_in_header = skip_bytes.saturating_sub(header_start);
            slices.push(IoSlice::new(&frame.header[start_in_header..]));
        }
        skipped = header_end;

        // Handle payload
        if !frame.payload.is_empty() {
            let payload_start = skipped;
            let payload_end = skipped + frame.payload.len();

            if skip_bytes < payload_end {
                let start_in_payload = skip_bytes.saturating_sub(payload_start);
                slices.push(IoSlice::new(&frame.payload[start_in_payload..]));
            }
            skipped = payload_end;
        }
    }

    slices
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::io::duplex;

    #[test]
    fn test_outbound_frame_creation() {
        let header = Header::new(1, 0x03, 42, 5);
        let payload = Bytes::from_static(b"hello");
        let frame = OutboundFrame::new(&header, payload);

        assert_eq!(frame.header.len(), HEADER_SIZE);
        assert_eq!(frame.payload.len(), 5);
        assert_eq!(frame.size(), HEADER_SIZE + 5);
    }

    #[test]
    fn test_outbound_frame_empty() {
        let header = Header::new(1, 0x23, 42, 0);
        let frame = OutboundFrame::empty(&header);

        assert!(frame.payload.is_empty());
        assert_eq!(frame.size(), HEADER_SIZE);
    }

    #[test]
    fn test_writer_config_default() {
        let config = WriterConfig::default();
        assert_eq!(config.max_pending_frames, DEFAULT_MAX_PENDING_FRAMES);
        assert_eq!(config.channel_capacity, DEFAULT_CHANNEL_CAPACITY);
        assert_eq!(config.backpressure_timeout, DEFAULT_BACKPRESSURE_TIMEOUT);
    }

    #[tokio::test]
    async fn test_writer_handle_send() {
        let (client, mut server) = duplex(4096);
        let (handle, _task) = spawn_writer_task_default(client);

        // Send a frame
        let header = Header::new(1, 0x03, 42, 5);
        let frame = OutboundFrame::new(&header, Bytes::from_static(b"hello"));
        handle.send(frame).await.unwrap();

        // Small delay for writer task to process
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Read from server side
        let mut buf = vec![0u8; 64];
        let n = tokio::io::AsyncReadExt::read(&mut server, &mut buf)
            .await
            .unwrap();

        assert_eq!(n, HEADER_SIZE + 5);
    }

    #[tokio::test]
    async fn test_writer_handle_pending_count() {
        let (client, _server) = duplex(4096);
        let config = WriterConfig {
            max_pending_frames: 1000,
            channel_capacity: 100,
            backpressure_timeout: Duration::from_secs(1),
        };
        let (handle, _task) = spawn_writer_task(client, config);

        assert_eq!(handle.pending_count(), 0);
        assert!(!handle.is_backpressure_active());
    }

    #[tokio::test]
    async fn test_writer_batching() {
        let (client, mut server) = duplex(4096);
        let (handle, _task) = spawn_writer_task_default(client);

        // Send multiple frames quickly
        for i in 0..10u32 {
            let header = Header::new(1, 0x03, i, 4);
            let payload = Bytes::copy_from_slice(&i.to_be_bytes());
            let frame = OutboundFrame::new(&header, payload);
            handle.send(frame).await.unwrap();
        }

        // Wait for writes to complete
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Read all data
        let mut buf = vec![0u8; 1024];
        let n = tokio::io::AsyncReadExt::read(&mut server, &mut buf)
            .await
            .unwrap();

        // Should have received all 10 frames
        let expected_size = 10 * (HEADER_SIZE + 4);
        assert_eq!(n, expected_size);
    }

    #[tokio::test]
    async fn test_try_send_at_capacity() {
        let (tx, _rx) = mpsc::channel::<OutboundFrame>(10);
        let pending = Arc::new(AtomicUsize::new(100)); // At capacity

        let handle = WriterHandle::new(tx, pending, 100, Duration::from_secs(1));

        let header = Header::new(1, 0x03, 42, 0);
        let frame = OutboundFrame::empty(&header);

        let result = handle.try_send(frame);
        assert!(matches!(result, Err(ProcwireError::BackpressureTimeout)));
    }

    #[test]
    fn test_build_remaining_slices_no_skip() {
        let header = Header::new(1, 0x03, 42, 5);
        let batch = vec![OutboundFrame::new(&header, Bytes::from_static(b"hello"))];

        let slices = build_remaining_slices(&batch, 0);
        assert_eq!(slices.len(), 2); // header + payload
    }

    #[test]
    fn test_build_remaining_slices_partial_header() {
        let header = Header::new(1, 0x03, 42, 5);
        let batch = vec![OutboundFrame::new(&header, Bytes::from_static(b"hello"))];

        let slices = build_remaining_slices(&batch, 5);
        // Should have partial header (6 bytes) + full payload
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].len(), HEADER_SIZE - 5);
        assert_eq!(slices[1].len(), 5);
    }

    #[test]
    fn test_build_remaining_slices_skip_header() {
        let header = Header::new(1, 0x03, 42, 5);
        let batch = vec![OutboundFrame::new(&header, Bytes::from_static(b"hello"))];

        let slices = build_remaining_slices(&batch, HEADER_SIZE);
        // Should have only payload
        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].len(), 5);
    }

    #[tokio::test]
    async fn test_write_batch_single() {
        let mut buf = Cursor::new(Vec::new());

        let header = Header::new(1, 0x03, 42, 5);
        let batch = vec![OutboundFrame::new(&header, Bytes::from_static(b"hello"))];

        write_batch(&mut buf, &batch).await.unwrap();

        let written = buf.into_inner();
        assert_eq!(written.len(), HEADER_SIZE + 5);
    }

    #[tokio::test]
    async fn test_write_batch_multiple() {
        let mut buf = Cursor::new(Vec::new());

        let batch: Vec<_> = (0..5)
            .map(|i| {
                let header = Header::new(1, 0x03, i, 3);
                OutboundFrame::new(&header, Bytes::from_static(b"abc"))
            })
            .collect();

        write_batch(&mut buf, &batch).await.unwrap();

        let written = buf.into_inner();
        assert_eq!(written.len(), 5 * (HEADER_SIZE + 3));
    }

    #[tokio::test]
    async fn test_writer_shutdown_on_channel_close() {
        let (client, _server) = duplex(4096);
        let (handle, task) = spawn_writer_task_default(client);

        // Drop the handle to close the channel
        drop(handle);

        // Writer task should complete cleanly
        let result = task.await.unwrap();
        assert!(result.is_ok());
    }
}
