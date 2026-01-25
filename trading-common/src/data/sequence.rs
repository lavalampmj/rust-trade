//! Sequence number generator for message ordering.
//!
//! Provides a thread-safe, atomic sequence number generator for use
//! in data normalizers and message processors.

use std::sync::atomic::{AtomicI64, Ordering};

/// Thread-safe sequence number generator.
///
/// Provides monotonically increasing sequence numbers for ordering
/// messages within a provider/normalizer. Uses atomic operations
/// for lock-free, high-performance sequence generation.
///
/// # Example
///
/// ```
/// use trading_common::data::SequenceGenerator;
///
/// let seq = SequenceGenerator::new();
/// assert_eq!(seq.next(), 0);
/// assert_eq!(seq.next(), 1);
/// assert_eq!(seq.next(), 2);
///
/// // Starting from a specific value
/// let seq = SequenceGenerator::starting_at(100);
/// assert_eq!(seq.next(), 100);
/// assert_eq!(seq.next(), 101);
/// ```
#[derive(Debug)]
pub struct SequenceGenerator {
    counter: AtomicI64,
}

impl SequenceGenerator {
    /// Create a new sequence generator starting at 0.
    pub fn new() -> Self {
        Self {
            counter: AtomicI64::new(0),
        }
    }

    /// Create a new sequence generator starting at the given value.
    pub fn starting_at(start: i64) -> Self {
        Self {
            counter: AtomicI64::new(start),
        }
    }

    /// Get the next sequence number.
    ///
    /// This operation is atomic and thread-safe.
    pub fn next(&self) -> i64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Get the current sequence number without incrementing.
    pub fn current(&self) -> i64 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Reset the sequence counter to 0.
    pub fn reset(&self) {
        self.counter.store(0, Ordering::Relaxed);
    }

    /// Reset the sequence counter to a specific value.
    pub fn reset_to(&self, value: i64) {
        self.counter.store(value, Ordering::Relaxed);
    }
}

impl Default for SequenceGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SequenceGenerator {
    /// Clone creates a new generator with the same current value.
    ///
    /// Note: The cloned generator is independent; incrementing one
    /// does not affect the other.
    fn clone(&self) -> Self {
        Self::starting_at(self.current())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_new_starts_at_zero() {
        let seq = SequenceGenerator::new();
        assert_eq!(seq.next(), 0);
        assert_eq!(seq.next(), 1);
    }

    #[test]
    fn test_starting_at() {
        let seq = SequenceGenerator::starting_at(100);
        assert_eq!(seq.next(), 100);
        assert_eq!(seq.next(), 101);
    }

    #[test]
    fn test_current() {
        let seq = SequenceGenerator::new();
        assert_eq!(seq.current(), 0);
        seq.next();
        assert_eq!(seq.current(), 1);
    }

    #[test]
    fn test_reset() {
        let seq = SequenceGenerator::new();
        seq.next();
        seq.next();
        assert_eq!(seq.current(), 2);
        seq.reset();
        assert_eq!(seq.current(), 0);
    }

    #[test]
    fn test_reset_to() {
        let seq = SequenceGenerator::new();
        seq.next();
        seq.reset_to(50);
        assert_eq!(seq.next(), 50);
    }

    #[test]
    fn test_default() {
        let seq = SequenceGenerator::default();
        assert_eq!(seq.next(), 0);
    }

    #[test]
    fn test_clone() {
        let seq = SequenceGenerator::new();
        seq.next();
        seq.next();
        let cloned = seq.clone();
        assert_eq!(cloned.current(), 2);
        // Cloned is independent
        cloned.next();
        assert_eq!(cloned.current(), 3);
        assert_eq!(seq.current(), 2);
    }

    #[test]
    fn test_thread_safety() {
        let seq = Arc::new(SequenceGenerator::new());
        let mut handles = vec![];

        for _ in 0..10 {
            let seq_clone = Arc::clone(&seq);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    seq_clone.next();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // 10 threads * 100 increments = 1000
        assert_eq!(seq.current(), 1000);
    }
}
