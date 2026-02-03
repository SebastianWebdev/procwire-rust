//! Header buffer pool for zero-allocation header encoding.
//!
//! Provides a pool of pre-allocated 11-byte header buffers that can be
//! reused across frame sends to avoid per-frame allocations.
//!
//! # Design
//!
//! The pool uses thread-local storage with round-robin allocation:
//! - Each thread gets its own pool of 16 buffers
//! - No locking or atomic operations on the hot path
//! - Headers are overwritten in a circular fashion
//!
//! # Usage
//!
//! ```ignore
//! use procwire_client::protocol::header_pool::HeaderPool;
//!
//! let mut pool = HeaderPool::new();
//! let buf = pool.acquire();
//! // Use buf for header encoding...
//! ```
//!
//! # Note
//!
//! Since `Header::encode()` already returns a stack-allocated `[u8; 11]`,
//! the main benefit of this pool is for scenarios where you need to hold
//! multiple headers simultaneously or pass them across async boundaries.

use super::wire_format::{Header, HEADER_POOL_SIZE, HEADER_SIZE};

/// A pool of pre-allocated header buffers.
///
/// Uses round-robin allocation with no synchronization overhead.
/// Thread-safe when used from a single thread (typical async executor usage).
pub struct HeaderPool {
    /// Pre-allocated header buffers.
    buffers: [[u8; HEADER_SIZE]; HEADER_POOL_SIZE],
    /// Current index (round-robin).
    index: usize,
}

impl HeaderPool {
    /// Create a new header pool with zeroed buffers.
    #[inline]
    pub const fn new() -> Self {
        Self {
            buffers: [[0u8; HEADER_SIZE]; HEADER_POOL_SIZE],
            index: 0,
        }
    }

    /// Acquire the next buffer from the pool (round-robin).
    ///
    /// Returns a mutable reference to a 11-byte buffer.
    /// The buffer content is NOT cleared - it may contain old data.
    #[inline]
    pub fn acquire(&mut self) -> &mut [u8; HEADER_SIZE] {
        let buf = &mut self.buffers[self.index];
        self.index = (self.index + 1) % HEADER_POOL_SIZE;
        buf
    }

    /// Acquire and encode a header into a pooled buffer.
    ///
    /// This is a convenience method that combines `acquire()` and encoding.
    #[inline]
    pub fn encode(&mut self, header: &Header) -> &[u8; HEADER_SIZE] {
        let buf = self.acquire();
        header.encode_into(buf);
        buf
    }

    /// Get the current index (for debugging).
    #[inline]
    pub fn current_index(&self) -> usize {
        self.index
    }

    /// Reset the pool index to 0.
    #[inline]
    pub fn reset(&mut self) {
        self.index = 0;
    }
}

impl Default for HeaderPool {
    fn default() -> Self {
        Self::new()
    }
}

// Thread-local pool for truly zero-contention access
thread_local! {
    static THREAD_LOCAL_POOL: std::cell::RefCell<HeaderPool> =
        const { std::cell::RefCell::new(HeaderPool::new()) };
}

/// Encode a header using the thread-local pool.
///
/// This returns a copy of the encoded header since we can't safely
/// return a reference to thread-local data across await points.
///
/// For most use cases, prefer `Header::encode()` directly since it
/// also returns a stack-allocated array with no heap allocation.
#[inline]
pub fn encode_header_pooled(header: &Header) -> [u8; HEADER_SIZE] {
    THREAD_LOCAL_POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        let buf = pool.acquire();
        header.encode_into(buf);
        *buf
    })
}

/// Access the thread-local header pool directly.
///
/// Use this when you need to encode multiple headers in sequence
/// without copying.
///
/// # Example
///
/// ```ignore
/// with_header_pool(|pool| {
///     let buf1 = pool.encode(&header1);
///     let buf2 = pool.encode(&header2);
///     // Use buf1 and buf2...
/// });
/// ```
pub fn with_header_pool<F, R>(f: F) -> R
where
    F: FnOnce(&mut HeaderPool) -> R,
{
    THREAD_LOCAL_POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        f(&mut pool)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_creation() {
        let pool = HeaderPool::new();
        assert_eq!(pool.current_index(), 0);
    }

    #[test]
    fn test_pool_acquire_round_robin() {
        let mut pool = HeaderPool::new();

        for i in 0..HEADER_POOL_SIZE {
            let _buf = pool.acquire();
            assert_eq!(pool.current_index(), (i + 1) % HEADER_POOL_SIZE);
        }

        // Should wrap around
        assert_eq!(pool.current_index(), 0);
    }

    #[test]
    fn test_pool_acquire_returns_different_buffers() {
        let mut pool = HeaderPool::new();

        let buf1_ptr = pool.acquire().as_ptr();
        let buf2_ptr = pool.acquire().as_ptr();
        let buf3_ptr = pool.acquire().as_ptr();

        assert_ne!(buf1_ptr, buf2_ptr);
        assert_ne!(buf2_ptr, buf3_ptr);
        assert_ne!(buf1_ptr, buf3_ptr);
    }

    #[test]
    fn test_pool_encode() {
        let mut pool = HeaderPool::new();
        let header = Header::new(1, 0x03, 42, 100);

        let encoded = pool.encode(&header);

        // Verify encoding
        assert_eq!(&encoded[0..2], &[0x00, 0x01]); // method_id = 1 (BE)
        assert_eq!(encoded[2], 0x03); // flags
        assert_eq!(&encoded[3..7], &[0x00, 0x00, 0x00, 0x2A]); // request_id = 42 (BE)
        assert_eq!(&encoded[7..11], &[0x00, 0x00, 0x00, 0x64]); // payload_length = 100 (BE)
    }

    #[test]
    fn test_pool_reset() {
        let mut pool = HeaderPool::new();

        pool.acquire();
        pool.acquire();
        pool.acquire();
        assert_eq!(pool.current_index(), 3);

        pool.reset();
        assert_eq!(pool.current_index(), 0);
    }

    #[test]
    fn test_encode_header_pooled() {
        let header = Header::new(5, 0x07, 1000, 50);

        let encoded = encode_header_pooled(&header);

        assert_eq!(&encoded[0..2], &[0x00, 0x05]); // method_id = 5 (BE)
        assert_eq!(encoded[2], 0x07); // flags
        assert_eq!(&encoded[3..7], &[0x00, 0x00, 0x03, 0xE8]); // request_id = 1000 (BE)
        assert_eq!(&encoded[7..11], &[0x00, 0x00, 0x00, 0x32]); // payload_length = 50 (BE)
    }

    #[test]
    fn test_with_header_pool() {
        let header1 = Header::new(1, 0x00, 1, 10);
        let header2 = Header::new(2, 0x00, 2, 20);

        let (enc1, enc2) = with_header_pool(|pool| {
            let e1 = *pool.encode(&header1);
            let e2 = *pool.encode(&header2);
            (e1, e2)
        });

        // Verify both headers encoded correctly
        assert_eq!(&enc1[0..2], &[0x00, 0x01]);
        assert_eq!(&enc2[0..2], &[0x00, 0x02]);
    }

    #[test]
    fn test_pool_default() {
        let pool = HeaderPool::default();
        assert_eq!(pool.current_index(), 0);
    }

    #[test]
    fn test_pool_buffers_are_mutable() {
        let mut pool = HeaderPool::new();

        let buf = pool.acquire();
        buf[0] = 0xFF;
        buf[10] = 0xAA;

        // Verify we can write to the buffer
        assert_eq!(buf[0], 0xFF);
        assert_eq!(buf[10], 0xAA);
    }

    #[test]
    fn test_pool_wrap_around_overwrites() {
        let mut pool = HeaderPool::new();

        // Fill first buffer with a pattern
        {
            let buf = pool.acquire();
            for b in buf.iter_mut() {
                *b = 0xAB;
            }
        }

        // Advance to wrap around
        for _ in 0..(HEADER_POOL_SIZE - 1) {
            pool.acquire();
        }

        // Now we should get the first buffer again
        let buf = pool.acquire();
        // It should still have our pattern (pool doesn't clear)
        assert_eq!(buf[0], 0xAB);
    }
}
