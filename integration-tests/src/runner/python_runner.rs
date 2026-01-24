//! Python strategy runner implementation
//!
//! This module provides a runner for Python strategies using PyO3.
//! For simplicity in integration tests, this provides a minimal Python
//! strategy bridge focused on tick counting and latency measurement.

use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;

use data_manager::schema::NormalizedTick;

use crate::metrics::StrategyMetrics;

use super::StrategyRunner;

/// Python strategy runner
///
/// Provides a simplified Python strategy runner for integration tests.
/// In a production scenario, this would use the full PythonStrategy bridge
/// from trading-common, but for testing purposes we provide a simpler
/// implementation that focuses on tick counting and latency measurement.
pub struct PythonStrategyRunner {
    /// Runner identifier
    id: String,
    /// Symbol this runner is subscribed to
    symbol: String,
    /// Metrics for this runner
    metrics: Arc<StrategyMetrics>,
    /// Flag to indicate if Python environment is available
    python_available: bool,
}

impl PythonStrategyRunner {
    /// Create a new Python strategy runner subscribed to a specific symbol
    pub fn new(id: String, symbol: String, sample_limit: usize) -> Self {
        let metrics = Arc::new(StrategyMetrics::new(
            id.clone(),
            "python".to_string(),
            sample_limit,
        ));

        // Check if Python is available
        let python_available = Self::check_python_available();

        Self {
            id,
            symbol,
            metrics,
            python_available,
        }
    }

    /// Check if Python environment is available
    fn check_python_available() -> bool {
        // For now, we assume Python is not available in the test environment
        // In a real implementation, we'd check for PyO3 initialization
        // and the availability of the test strategy script.
        // Return false since Python support is not yet implemented.
        false
    }

    /// Process tick through Python strategy
    ///
    /// Returns latency in microseconds if available
    fn process_python_tick(&self, _tick: &NormalizedTick) -> Option<u64> {
        if !self.python_available {
            // If Python not available, just simulate processing
            // with minimal overhead
            None
        } else {
            // In real implementation:
            // 1. Convert NormalizedTick to Python dict
            // 2. Call Python strategy's on_tick method
            // 3. Extract latency from timing
            None
        }
    }
}

#[async_trait]
impl StrategyRunner for PythonStrategyRunner {
    fn id(&self) -> &str {
        &self.id
    }

    fn strategy_type(&self) -> &str {
        "python"
    }

    fn subscribed_symbol(&self) -> &str {
        &self.symbol
    }

    async fn process_tick(&self, tick: &NormalizedTick) {
        // Calculate latency from ts_recv
        let latency_us = {
            let now = Utc::now();
            let recv_time = tick.ts_recv;
            let diff = now.signed_duration_since(recv_time);
            if diff.num_microseconds().unwrap_or(0) >= 0 {
                Some(diff.num_microseconds().unwrap_or(0) as u64)
            } else {
                None
            }
        };

        // Record the tick with latency
        self.metrics.record_tick(latency_us);

        // Process through Python strategy (if available)
        // The actual Python processing latency would be measured here
        let _ = self.process_python_tick(tick);
    }

    fn metrics(&self) -> Arc<StrategyMetrics> {
        self.metrics.clone()
    }

    async fn shutdown(&self) {
        // Cleanup Python resources if needed
        // In real implementation, we'd release the GIL and cleanup
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_manager::schema::TradeSide;
    use rust_decimal::Decimal;

    fn create_test_tick() -> NormalizedTick {
        NormalizedTick::new(
            Utc::now(),
            "TEST0000".to_string(),
            "TEST".to_string(),
            Decimal::from(50000),
            Decimal::from(100),
            TradeSide::Buy,
            "test".to_string(),
            1,
        )
    }

    #[tokio::test]
    async fn test_python_runner() {
        let runner =
            PythonStrategyRunner::new("python_0".to_string(), "TEST0000".to_string(), 1000);

        assert_eq!(runner.id(), "python_0");
        assert_eq!(runner.strategy_type(), "python");
        assert_eq!(runner.subscribed_symbol(), "TEST0000");
        assert_eq!(runner.metrics().received_count(), 0);
    }

    #[tokio::test]
    async fn test_python_runner_process_tick() {
        let runner =
            PythonStrategyRunner::new("python_0".to_string(), "TEST0000".to_string(), 1000);

        let tick = create_test_tick();
        runner.process_tick(&tick).await;

        assert_eq!(runner.metrics().received_count(), 1);
    }

    #[tokio::test]
    async fn test_python_runner_multiple_ticks() {
        let runner =
            PythonStrategyRunner::new("python_0".to_string(), "TEST0000".to_string(), 1000);

        let tick = create_test_tick();
        for _ in 0..50 {
            runner.process_tick(&tick).await;
        }

        assert_eq!(runner.metrics().received_count(), 50);
    }

    #[tokio::test]
    async fn test_python_runner_latency() {
        let runner =
            PythonStrategyRunner::new("python_0".to_string(), "TEST0000".to_string(), 1000);

        // Create tick with recent timestamp
        let tick = NormalizedTick::with_details(
            Utc::now(),
            Utc::now(),
            "TEST0000".to_string(),
            "TEST".to_string(),
            Decimal::from(50000),
            Decimal::from(100),
            TradeSide::Buy,
            "test".to_string(),
            None,
            None,
            1,
        );

        runner.process_tick(&tick).await;

        let metrics = runner.metrics();
        let stats = metrics.latency_stats.lock();
        assert!(stats.count > 0);
    }

    #[tokio::test]
    async fn test_python_runner_shutdown() {
        let runner =
            PythonStrategyRunner::new("python_0".to_string(), "TEST0000".to_string(), 1000);

        let tick = create_test_tick();
        runner.process_tick(&tick).await;
        runner.shutdown().await;

        // Shutdown should not affect metrics
        assert_eq!(runner.metrics().received_count(), 1);
    }
}
