//! Backpressure handling for write operations.
//!
//! This module provides backpressure management to prevent memory exhaustion
//! when the write side is slower than the producer side. It tracks pending
//! frames and provides mechanisms to limit the number of in-flight writes.
//!
//! # Usage
//!
//! The [`BackpressureController`] is used internally by the writer task to
//! track pending writes. Handlers interact with backpressure through the
//! [`WriterHandle`](crate::writer::WriterHandle) which will automatically
//! wait or reject when backpressure is active.
//!
//! # Configuration
//!
//! - `max_pending`: Maximum number of pending frames (default: 1024)
//! - Timeout: How long to wait when backpressure is active (default: 5s)

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{ProcwireError, Result};

/// Default maximum pending frames before backpressure kicks in.
pub const DEFAULT_MAX_PENDING: usize = 1024;

/// Default backpressure timeout (how long to wait for space to become available).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Interval between backpressure checks.
const CHECK_INTERVAL: Duration = Duration::from_micros(100);

/// Backpressure controller for managing write queue pressure.
///
/// This controller uses atomic operations for lock-free tracking of
/// pending frame counts. It can be shared across multiple threads/tasks.
#[derive(Debug)]
pub struct BackpressureController {
    /// Current pending frame count.
    pending: Arc<AtomicUsize>,
    /// Maximum allowed pending frames.
    max_pending: usize,
    /// Timeout for waiting on backpressure.
    timeout: Duration,
}

impl BackpressureController {
    /// Create a new backpressure controller with specified limit.
    pub fn new(max_pending: usize) -> Self {
        Self {
            pending: Arc::new(AtomicUsize::new(0)),
            max_pending,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Create a controller with custom timeout.
    pub fn with_timeout(max_pending: usize, timeout: Duration) -> Self {
        Self {
            pending: Arc::new(AtomicUsize::new(0)),
            max_pending,
            timeout,
        }
    }

    /// Create from an existing atomic counter (for sharing with writer task).
    pub fn from_shared(pending: Arc<AtomicUsize>, max_pending: usize, timeout: Duration) -> Self {
        Self {
            pending,
            max_pending,
            timeout,
        }
    }

    /// Get a clone of the pending counter Arc (for sharing).
    pub fn pending_counter(&self) -> Arc<AtomicUsize> {
        self.pending.clone()
    }

    /// Check if we can accept more frames without blocking.
    #[inline]
    pub fn can_accept(&self) -> bool {
        self.pending.load(Ordering::Acquire) < self.max_pending
    }

    /// Check if backpressure is currently active.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.pending.load(Ordering::Acquire) >= self.max_pending
    }

    /// Get current pending count.
    #[inline]
    pub fn pending_count(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }

    /// Get maximum pending limit.
    #[inline]
    pub fn max_pending(&self) -> usize {
        self.max_pending
    }

    /// Get available capacity.
    #[inline]
    pub fn available_capacity(&self) -> usize {
        let current = self.pending.load(Ordering::Acquire);
        self.max_pending.saturating_sub(current)
    }

    /// Try to reserve a slot without blocking.
    ///
    /// Returns `Ok(())` if reserved, `Err(BackpressureTimeout)` if at capacity.
    pub fn try_reserve(&self) -> Result<()> {
        let current = self.pending.load(Ordering::Acquire);
        if current >= self.max_pending {
            return Err(ProcwireError::BackpressureTimeout);
        }

        // Use compare-and-swap for safety (though fetch_add is usually fine)
        self.pending.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Reserve a slot, waiting if necessary.
    ///
    /// Returns `Err(BackpressureTimeout)` if timeout is reached.
    pub async fn reserve(&self) -> Result<()> {
        // Fast path: try immediate reservation
        if self.pending.load(Ordering::Acquire) < self.max_pending {
            self.pending.fetch_add(1, Ordering::AcqRel);
            return Ok(());
        }

        // Slow path: wait for space
        self.wait_and_reserve().await
    }

    /// Wait for backpressure to clear and then reserve.
    async fn wait_and_reserve(&self) -> Result<()> {
        let start = Instant::now();

        loop {
            let current = self.pending.load(Ordering::Acquire);
            if current < self.max_pending {
                self.pending.fetch_add(1, Ordering::AcqRel);
                return Ok(());
            }

            if start.elapsed() > self.timeout {
                return Err(ProcwireError::BackpressureTimeout);
            }

            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    }

    /// Release a slot (called after frame is written).
    #[inline]
    pub fn release(&self) {
        self.pending.fetch_sub(1, Ordering::Release);
    }

    /// Release multiple slots at once (for batch writes).
    #[inline]
    pub fn release_many(&self, count: usize) {
        self.pending.fetch_sub(count, Ordering::Release);
    }

    /// Reset the pending count (use with caution).
    pub fn reset(&self) {
        self.pending.store(0, Ordering::Release);
    }
}

impl Default for BackpressureController {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_PENDING)
    }
}

impl Clone for BackpressureController {
    fn clone(&self) -> Self {
        Self {
            pending: self.pending.clone(),
            max_pending: self.max_pending,
            timeout: self.timeout,
        }
    }
}

/// Guard that automatically releases a backpressure slot on drop.
///
/// Useful for RAII-style backpressure management.
pub struct BackpressureGuard {
    controller: BackpressureController,
    released: bool,
}

impl BackpressureGuard {
    /// Create a guard that will release on drop.
    pub fn new(controller: BackpressureController) -> Self {
        Self {
            controller,
            released: false,
        }
    }

    /// Manually release the slot.
    pub fn release(mut self) {
        if !self.released {
            self.controller.release();
            self.released = true;
        }
    }

    /// Disarm the guard (don't release on drop).
    ///
    /// Use this when the slot will be released by another mechanism.
    pub fn disarm(&mut self) {
        self.released = true;
    }
}

impl Drop for BackpressureGuard {
    fn drop(&mut self) {
        if !self.released {
            self.controller.release();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_creation() {
        let ctrl = BackpressureController::new(100);
        assert_eq!(ctrl.max_pending(), 100);
        assert_eq!(ctrl.pending_count(), 0);
        assert!(ctrl.can_accept());
        assert!(!ctrl.is_active());
    }

    #[test]
    fn test_controller_default() {
        let ctrl = BackpressureController::default();
        assert_eq!(ctrl.max_pending(), DEFAULT_MAX_PENDING);
    }

    #[test]
    fn test_try_reserve_success() {
        let ctrl = BackpressureController::new(10);

        for _ in 0..10 {
            assert!(ctrl.try_reserve().is_ok());
        }

        assert_eq!(ctrl.pending_count(), 10);
        assert!(ctrl.is_active());
    }

    #[test]
    fn test_try_reserve_at_capacity() {
        let ctrl = BackpressureController::new(5);

        // Fill to capacity
        for _ in 0..5 {
            ctrl.try_reserve().unwrap();
        }

        // Should fail now
        let result = ctrl.try_reserve();
        assert!(matches!(result, Err(ProcwireError::BackpressureTimeout)));
    }

    #[test]
    fn test_release() {
        let ctrl = BackpressureController::new(10);

        ctrl.try_reserve().unwrap();
        ctrl.try_reserve().unwrap();
        assert_eq!(ctrl.pending_count(), 2);

        ctrl.release();
        assert_eq!(ctrl.pending_count(), 1);

        ctrl.release();
        assert_eq!(ctrl.pending_count(), 0);
    }

    #[test]
    fn test_release_many() {
        let ctrl = BackpressureController::new(100);

        for _ in 0..50 {
            ctrl.try_reserve().unwrap();
        }
        assert_eq!(ctrl.pending_count(), 50);

        ctrl.release_many(30);
        assert_eq!(ctrl.pending_count(), 20);
    }

    #[test]
    fn test_available_capacity() {
        let ctrl = BackpressureController::new(100);

        assert_eq!(ctrl.available_capacity(), 100);

        ctrl.try_reserve().unwrap();
        assert_eq!(ctrl.available_capacity(), 99);

        for _ in 0..50 {
            ctrl.try_reserve().unwrap();
        }
        assert_eq!(ctrl.available_capacity(), 49);
    }

    #[test]
    fn test_clone_shares_state() {
        let ctrl1 = BackpressureController::new(10);
        let ctrl2 = ctrl1.clone();

        ctrl1.try_reserve().unwrap();
        assert_eq!(ctrl2.pending_count(), 1);

        ctrl2.try_reserve().unwrap();
        assert_eq!(ctrl1.pending_count(), 2);
    }

    #[test]
    fn test_from_shared() {
        let pending = Arc::new(AtomicUsize::new(5));
        let ctrl = BackpressureController::from_shared(pending.clone(), 10, Duration::from_secs(1));

        assert_eq!(ctrl.pending_count(), 5);
        assert!(!ctrl.is_active());

        pending.store(10, Ordering::SeqCst);
        assert!(ctrl.is_active());
    }

    #[tokio::test]
    async fn test_reserve_immediate() {
        let ctrl = BackpressureController::new(10);

        ctrl.reserve().await.unwrap();
        assert_eq!(ctrl.pending_count(), 1);
    }

    #[tokio::test]
    async fn test_reserve_timeout() {
        let ctrl = BackpressureController::with_timeout(1, Duration::from_millis(10));

        // Fill to capacity
        ctrl.try_reserve().unwrap();

        // Should timeout
        let start = Instant::now();
        let result = ctrl.reserve().await;
        let elapsed = start.elapsed();

        assert!(matches!(result, Err(ProcwireError::BackpressureTimeout)));
        assert!(elapsed >= Duration::from_millis(10));
    }

    #[tokio::test]
    async fn test_reserve_wait_success() {
        let ctrl = BackpressureController::with_timeout(1, Duration::from_secs(1));

        // Fill to capacity
        ctrl.try_reserve().unwrap();

        // Spawn task to release after delay
        let ctrl_clone = ctrl.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            ctrl_clone.release();
        });

        // Should succeed after release
        let result = ctrl.reserve().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_reset() {
        let ctrl = BackpressureController::new(100);

        for _ in 0..50 {
            ctrl.try_reserve().unwrap();
        }
        assert_eq!(ctrl.pending_count(), 50);

        ctrl.reset();
        assert_eq!(ctrl.pending_count(), 0);
    }

    #[test]
    fn test_guard_release_on_drop() {
        let ctrl = BackpressureController::new(10);
        ctrl.try_reserve().unwrap();

        {
            let _guard = BackpressureGuard::new(ctrl.clone());
            assert_eq!(ctrl.pending_count(), 1);
        }

        // Should be released after guard drops
        assert_eq!(ctrl.pending_count(), 0);
    }

    #[test]
    fn test_guard_manual_release() {
        let ctrl = BackpressureController::new(10);
        ctrl.try_reserve().unwrap();

        let guard = BackpressureGuard::new(ctrl.clone());
        assert_eq!(ctrl.pending_count(), 1);

        guard.release();
        assert_eq!(ctrl.pending_count(), 0);
    }

    #[test]
    fn test_guard_disarm() {
        let ctrl = BackpressureController::new(10);
        ctrl.try_reserve().unwrap();

        {
            let mut guard = BackpressureGuard::new(ctrl.clone());
            guard.disarm();
        }

        // Should NOT be released (disarmed)
        assert_eq!(ctrl.pending_count(), 1);
    }
}
