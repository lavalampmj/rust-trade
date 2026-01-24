//! Latency modeling for realistic order execution simulation.
//!
//! This module provides latency models that simulate network and exchange delays
//! between order submission and execution. This is critical for realistic backtesting
//! as orders cannot fill on the same bar they were submitted.
//!
//! # Models
//!
//! - [`NoLatencyModel`]: Zero latency (legacy behavior)
//! - [`FixedLatencyModel`]: Constant delay for all operations
//! - [`VariableLatencyModel`]: Base delay plus random jitter
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::latency_model::*;
//!
//! // 50ms fixed latency for all operations
//! let model = FixedLatencyModel::new(50_000_000);
//! assert_eq!(model.insert_latency_nanos(), 50_000_000);
//!
//! // Variable latency: 30ms base + up to 20ms jitter
//! let model = VariableLatencyModel::new(30_000_000, 20_000_000, 42);
//! ```

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::fmt::Debug;
use std::sync::Mutex;

/// Trait for modeling network and exchange latency.
///
/// Implementations provide latency values (in nanoseconds) for different
/// order operations. These delays determine when commands "arrive" at the
/// simulated exchange after being submitted by the strategy.
pub trait LatencyModel: Send + Sync + Debug {
    /// Latency for order submission (insert) in nanoseconds.
    fn insert_latency_nanos(&self) -> u64;

    /// Latency for order modification (update) in nanoseconds.
    fn update_latency_nanos(&self) -> u64;

    /// Latency for order cancellation (delete) in nanoseconds.
    fn delete_latency_nanos(&self) -> u64;

    /// Reset any internal state (e.g., RNG seed for reproducibility).
    fn reset(&self) {}
}

/// No latency model - orders are processed immediately.
///
/// Use this for legacy behavior or when latency simulation is not needed.
/// Orders submitted on bar N can fill on bar N (look-ahead bias risk).
#[derive(Debug, Clone, Default)]
pub struct NoLatencyModel;

impl NoLatencyModel {
    pub fn new() -> Self {
        Self
    }
}

impl LatencyModel for NoLatencyModel {
    fn insert_latency_nanos(&self) -> u64 {
        0
    }

    fn update_latency_nanos(&self) -> u64 {
        0
    }

    fn delete_latency_nanos(&self) -> u64 {
        0
    }
}

/// Fixed latency model - constant delay for all operations.
///
/// All order operations experience the same configurable delay.
/// Simple and deterministic.
#[derive(Debug, Clone)]
pub struct FixedLatencyModel {
    /// Latency for order insertion in nanoseconds
    insert_latency_ns: u64,
    /// Latency for order updates in nanoseconds
    update_latency_ns: u64,
    /// Latency for order deletion in nanoseconds
    delete_latency_ns: u64,
}

impl FixedLatencyModel {
    /// Create a fixed latency model with the same delay for all operations.
    pub fn new(latency_ns: u64) -> Self {
        Self {
            insert_latency_ns: latency_ns,
            update_latency_ns: latency_ns,
            delete_latency_ns: latency_ns,
        }
    }

    /// Create a fixed latency model with different delays per operation type.
    pub fn with_separate_latencies(
        insert_latency_ns: u64,
        update_latency_ns: u64,
        delete_latency_ns: u64,
    ) -> Self {
        Self {
            insert_latency_ns,
            update_latency_ns,
            delete_latency_ns,
        }
    }

    /// Create from milliseconds (convenience method).
    pub fn from_millis(insert_ms: u64, update_ms: u64, delete_ms: u64) -> Self {
        Self {
            insert_latency_ns: insert_ms * 1_000_000,
            update_latency_ns: update_ms * 1_000_000,
            delete_latency_ns: delete_ms * 1_000_000,
        }
    }
}

impl Default for FixedLatencyModel {
    fn default() -> Self {
        // Default: 50ms for insert/update, 30ms for cancel
        Self::from_millis(50, 50, 30)
    }
}

impl LatencyModel for FixedLatencyModel {
    fn insert_latency_nanos(&self) -> u64 {
        self.insert_latency_ns
    }

    fn update_latency_nanos(&self) -> u64 {
        self.update_latency_ns
    }

    fn delete_latency_nanos(&self) -> u64 {
        self.delete_latency_ns
    }
}

/// Variable latency model - base delay plus random jitter.
///
/// Simulates real-world network conditions where latency varies.
/// Uses a seeded RNG for reproducible results in backtests.
#[derive(Debug)]
pub struct VariableLatencyModel {
    /// Base latency in nanoseconds
    base_latency_ns: u64,
    /// Maximum jitter (random variation) in nanoseconds
    jitter_ns: u64,
    /// Seeded random number generator (behind mutex for thread safety)
    rng: Mutex<StdRng>,
    /// Original seed for reset
    seed: u64,
}

impl VariableLatencyModel {
    /// Create a variable latency model.
    ///
    /// # Arguments
    /// * `base_latency_ns` - Minimum latency in nanoseconds
    /// * `jitter_ns` - Maximum additional random delay in nanoseconds
    /// * `seed` - RNG seed for reproducibility (0 = use random seed)
    pub fn new(base_latency_ns: u64, jitter_ns: u64, seed: u64) -> Self {
        let actual_seed = if seed == 0 {
            rand::thread_rng().gen()
        } else {
            seed
        };

        Self {
            base_latency_ns,
            jitter_ns,
            rng: Mutex::new(StdRng::seed_from_u64(actual_seed)),
            seed: actual_seed,
        }
    }

    /// Create from milliseconds (convenience method).
    pub fn from_millis(base_ms: u64, jitter_ms: u64, seed: u64) -> Self {
        Self::new(base_ms * 1_000_000, jitter_ms * 1_000_000, seed)
    }

    /// Get a latency sample with jitter.
    fn sample_latency(&self) -> u64 {
        let jitter = if self.jitter_ns > 0 {
            let mut rng = self.rng.lock().unwrap();
            rng.gen_range(0..=self.jitter_ns)
        } else {
            0
        };
        self.base_latency_ns + jitter
    }
}

impl LatencyModel for VariableLatencyModel {
    fn insert_latency_nanos(&self) -> u64 {
        self.sample_latency()
    }

    fn update_latency_nanos(&self) -> u64 {
        self.sample_latency()
    }

    fn delete_latency_nanos(&self) -> u64 {
        self.sample_latency()
    }

    fn reset(&self) {
        let mut rng = self.rng.lock().unwrap();
        *rng = StdRng::seed_from_u64(self.seed);
    }
}

/// Create a default latency model based on configuration.
pub fn default_latency_model() -> Box<dyn LatencyModel> {
    Box::new(NoLatencyModel::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_latency_model() {
        let model = NoLatencyModel::new();
        assert_eq!(model.insert_latency_nanos(), 0);
        assert_eq!(model.update_latency_nanos(), 0);
        assert_eq!(model.delete_latency_nanos(), 0);
    }

    #[test]
    fn test_fixed_latency_model_uniform() {
        let model = FixedLatencyModel::new(100_000_000); // 100ms
        assert_eq!(model.insert_latency_nanos(), 100_000_000);
        assert_eq!(model.update_latency_nanos(), 100_000_000);
        assert_eq!(model.delete_latency_nanos(), 100_000_000);
    }

    #[test]
    fn test_fixed_latency_model_separate() {
        let model = FixedLatencyModel::with_separate_latencies(
            50_000_000, // 50ms insert
            50_000_000, // 50ms update
            30_000_000, // 30ms delete
        );
        assert_eq!(model.insert_latency_nanos(), 50_000_000);
        assert_eq!(model.update_latency_nanos(), 50_000_000);
        assert_eq!(model.delete_latency_nanos(), 30_000_000);
    }

    #[test]
    fn test_fixed_latency_from_millis() {
        let model = FixedLatencyModel::from_millis(50, 50, 30);
        assert_eq!(model.insert_latency_nanos(), 50_000_000);
        assert_eq!(model.update_latency_nanos(), 50_000_000);
        assert_eq!(model.delete_latency_nanos(), 30_000_000);
    }

    #[test]
    fn test_variable_latency_model_bounds() {
        let model = VariableLatencyModel::new(30_000_000, 20_000_000, 42);

        for _ in 0..100 {
            let latency = model.insert_latency_nanos();
            assert!(latency >= 30_000_000, "Latency below base");
            assert!(latency <= 50_000_000, "Latency above base + jitter");
        }
    }

    #[test]
    fn test_variable_latency_model_reproducibility() {
        let model1 = VariableLatencyModel::new(30_000_000, 20_000_000, 42);
        let model2 = VariableLatencyModel::new(30_000_000, 20_000_000, 42);

        // Same seed should produce same sequence
        let seq1: Vec<u64> = (0..10).map(|_| model1.insert_latency_nanos()).collect();
        let seq2: Vec<u64> = (0..10).map(|_| model2.insert_latency_nanos()).collect();

        assert_eq!(seq1, seq2);
    }

    #[test]
    fn test_variable_latency_model_reset() {
        let model = VariableLatencyModel::new(30_000_000, 20_000_000, 42);

        let seq1: Vec<u64> = (0..5).map(|_| model.insert_latency_nanos()).collect();

        model.reset();

        let seq2: Vec<u64> = (0..5).map(|_| model.insert_latency_nanos()).collect();

        assert_eq!(seq1, seq2);
    }

    #[test]
    fn test_variable_latency_no_jitter() {
        let model = VariableLatencyModel::new(50_000_000, 0, 42);

        for _ in 0..10 {
            assert_eq!(model.insert_latency_nanos(), 50_000_000);
        }
    }

    #[test]
    fn test_different_seeds_different_results() {
        let model1 = VariableLatencyModel::new(30_000_000, 20_000_000, 42);
        let model2 = VariableLatencyModel::new(30_000_000, 20_000_000, 123);

        let seq1: Vec<u64> = (0..10).map(|_| model1.insert_latency_nanos()).collect();
        let seq2: Vec<u64> = (0..10).map(|_| model2.insert_latency_nanos()).collect();

        assert_ne!(seq1, seq2);
    }

    #[test]
    fn test_default_fixed_latency() {
        let model = FixedLatencyModel::default();
        assert_eq!(model.insert_latency_nanos(), 50_000_000); // 50ms
        assert_eq!(model.update_latency_nanos(), 50_000_000);
        assert_eq!(model.delete_latency_nanos(), 30_000_000);
    }
}
