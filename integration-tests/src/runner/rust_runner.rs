//! Rust strategy runner implementation
//!
//! This module provides a runner for native Rust strategies that implements
//! the StrategyRunner trait for use in integration tests.

use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;

use data_manager::schema::NormalizedTick;
use trading_common::backtest::strategy::Strategy;

use crate::metrics::StrategyMetrics;
use crate::strategies::TestTickCounterStrategy;

use super::StrategyRunner;

/// Rust strategy runner that wraps a Strategy implementation
pub struct RustStrategyRunner {
    /// Runner identifier
    id: String,
    /// Symbol this runner is subscribed to
    symbol: String,
    /// The underlying strategy (wrapped in parking_lot::Mutex for interior mutability)
    strategy: parking_lot::Mutex<Box<dyn Strategy>>,
    /// Metrics for this runner
    metrics: Arc<StrategyMetrics>,
}

impl RustStrategyRunner {
    /// Create a new Rust strategy runner subscribed to a specific symbol
    pub fn new(
        id: String,
        symbol: String,
        strategy: Box<dyn Strategy>,
        sample_limit: usize,
    ) -> Self {
        let metrics = Arc::new(StrategyMetrics::new(
            id.clone(),
            "rust".to_string(),
            sample_limit,
        ));

        Self {
            id,
            symbol,
            strategy: parking_lot::Mutex::new(strategy),
            metrics,
        }
    }

    /// Create a new Rust strategy runner with the default TestTickCounterStrategy
    pub fn with_tick_counter(id: String, symbol: String, sample_limit: usize) -> Self {
        Self::new(
            id,
            symbol,
            Box::new(TestTickCounterStrategy::new()),
            sample_limit,
        )
    }

    /// Get mutable access to the strategy
    pub fn strategy_mut(&self) -> parking_lot::MutexGuard<'_, Box<dyn Strategy>> {
        self.strategy.lock()
    }
}

#[async_trait]
impl StrategyRunner for RustStrategyRunner {
    fn id(&self) -> &str {
        &self.id
    }

    fn strategy_type(&self) -> &str {
        "rust"
    }

    fn subscribed_symbol(&self) -> &str {
        &self.symbol
    }

    async fn process_tick(&self, tick: &NormalizedTick) {
        // Extract latency from ts_in_delta if available
        // Note: In a real implementation, we'd need access to the raw TradeMsg
        // For now, we calculate latency from ts_recv vs current time
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

        // Record the tick
        self.metrics.record_tick(latency_us);

        // Process through strategy (simplified - not using full BarData flow)
        // In a real integration, this would go through the BacktestEngine.
        // For TestTickCounterStrategy, we can call process_tick_direct
        // but since we use trait objects, the strategy would update tick_count
        // via on_bar_data if we construct proper BarData.
        // Currently we just track metrics at the runner level.
    }

    fn metrics(&self) -> Arc<StrategyMetrics> {
        self.metrics.clone()
    }

    async fn shutdown(&self) {
        // Reset strategy state
        self.strategy.lock().reset();
    }
}

/// Extension trait for downcasting Strategy trait objects
#[allow(dead_code)]
pub trait StrategyExt {
    fn as_any_mut(&mut self) -> Option<&mut TestTickCounterStrategy>;
}

impl StrategyExt for Box<dyn Strategy> {
    fn as_any_mut(&mut self) -> Option<&mut TestTickCounterStrategy> {
        // This is a workaround since we can't use Any with trait objects easily
        // In practice, we'd use a proper downcast mechanism
        None
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
    async fn test_rust_runner() {
        let runner = RustStrategyRunner::with_tick_counter(
            "test_runner".to_string(),
            "TEST0000".to_string(),
            1000,
        );

        assert_eq!(runner.id(), "test_runner");
        assert_eq!(runner.strategy_type(), "rust");
        assert_eq!(runner.subscribed_symbol(), "TEST0000");
        assert_eq!(runner.metrics().received_count(), 0);

        let tick = create_test_tick();
        runner.process_tick(&tick).await;

        assert_eq!(runner.metrics().received_count(), 1);
    }

    #[tokio::test]
    async fn test_rust_runner_with_custom_strategy() {
        let runner = RustStrategyRunner::new(
            "test_runner".to_string(),
            "TEST0001".to_string(),
            Box::new(TestTickCounterStrategy::new()),
            1000,
        );

        assert_eq!(runner.id(), "test_runner");
        assert_eq!(runner.strategy_type(), "rust");
        assert_eq!(runner.subscribed_symbol(), "TEST0001");
    }

    #[tokio::test]
    async fn test_rust_runner_multiple_ticks() {
        let runner = RustStrategyRunner::with_tick_counter(
            "test_runner".to_string(),
            "TEST0000".to_string(),
            1000,
        );

        let tick = create_test_tick();
        for _ in 0..100 {
            runner.process_tick(&tick).await;
        }

        assert_eq!(runner.metrics().received_count(), 100);
    }

    #[tokio::test]
    async fn test_rust_runner_latency_recording() {
        let runner = RustStrategyRunner::with_tick_counter(
            "test_runner".to_string(),
            "TEST0000".to_string(),
            1000,
        );

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
        // Latency should be recorded and be small (since tick is recent)
        assert!(stats.count > 0);
        // Average should be less than 1 second (1_000_000 us)
        assert!(stats.average_us() < 1_000_000.0);
    }
}
