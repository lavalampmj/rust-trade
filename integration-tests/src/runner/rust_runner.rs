//! Rust strategy runner implementation
//!
//! This module provides a runner for native Rust strategies and includes
//! a minimal test strategy for tick counting and latency measurement.

use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;

use data_manager::schema::NormalizedTick;
use trading_common::backtest::strategy::{Signal, Strategy};
use trading_common::data::types::{BarData, BarDataMode, BarType, Timeframe};
use trading_common::series::bars_context::BarsContext;
use trading_common::series::MaximumBarsLookBack;

use crate::metrics::StrategyMetrics;

use super::StrategyRunner;

/// Minimal test strategy for counting ticks and measuring latency
///
/// This strategy does not generate trading signals - it simply counts
/// every tick it receives and records latency measurements.
pub struct TestTickCounterStrategy {
    tick_count: u64,
    total_value: Decimal,
}

impl TestTickCounterStrategy {
    /// Create a new test tick counter strategy
    pub fn new() -> Self {
        Self {
            tick_count: 0,
            total_value: Decimal::ZERO,
        }
    }

    /// Get the total tick count processed by this strategy
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// Get the total value (sum of price * quantity)
    pub fn total_value(&self) -> Decimal {
        self.total_value
    }

    /// Process a tick directly (bypassing BarData for simpler testing)
    pub fn process_tick_direct(&mut self, price: Decimal, quantity: Decimal) {
        self.tick_count += 1;
        self.total_value += price * quantity;
    }
}

impl Default for TestTickCounterStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl Strategy for TestTickCounterStrategy {
    fn name(&self) -> &str {
        "Test Tick Counter"
    }

    fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        // Process tick if available
        if let Some(ref tick) = bar_data.current_tick {
            self.tick_count += 1;
            self.total_value += tick.price * tick.quantity;
        }

        // Always hold - this strategy doesn't trade
        Signal::Hold
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.tick_count = 0;
        self.total_value = Decimal::ZERO;
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        // Always ready - no warmup needed
        true
    }

    fn warmup_period(&self) -> usize {
        0
    }

    fn bar_data_mode(&self) -> BarDataMode {
        // Process on each tick
        BarDataMode::OnEachTick
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)
    }

    fn max_bars_lookback(&self) -> MaximumBarsLookBack {
        MaximumBarsLookBack::Fixed(1)
    }
}

/// Rust strategy runner that wraps a Strategy implementation
pub struct RustStrategyRunner {
    /// Runner identifier
    id: String,
    /// The underlying strategy (wrapped in parking_lot::Mutex for interior mutability)
    strategy: parking_lot::Mutex<Box<dyn Strategy>>,
    /// Metrics for this runner
    metrics: Arc<StrategyMetrics>,
}

impl RustStrategyRunner {
    /// Create a new Rust strategy runner
    pub fn new(
        id: String,
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
            strategy: parking_lot::Mutex::new(strategy),
            metrics,
        }
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

    #[test]
    fn test_tick_counter_strategy() {
        let mut strategy = TestTickCounterStrategy::new();

        assert_eq!(strategy.tick_count(), 0);

        strategy.process_tick_direct(Decimal::from(100), Decimal::from(10));
        strategy.process_tick_direct(Decimal::from(200), Decimal::from(5));

        assert_eq!(strategy.tick_count(), 2);
        assert_eq!(strategy.total_value(), Decimal::from(2000)); // 100*10 + 200*5
    }

    #[test]
    fn test_tick_counter_strategy_reset() {
        let mut strategy = TestTickCounterStrategy::new();

        strategy.process_tick_direct(Decimal::from(100), Decimal::from(10));
        assert_eq!(strategy.tick_count(), 1);

        strategy.reset();
        assert_eq!(strategy.tick_count(), 0);
        assert_eq!(strategy.total_value(), Decimal::ZERO);
    }

    #[test]
    fn test_tick_counter_strategy_trait() {
        let strategy = TestTickCounterStrategy::new();

        assert_eq!(strategy.name(), "Test Tick Counter");
        assert!(strategy.is_ready(&BarsContext::new("TEST")));
        assert_eq!(strategy.warmup_period(), 0);
        assert_eq!(strategy.bar_data_mode(), BarDataMode::OnEachTick);
    }

    #[tokio::test]
    async fn test_rust_runner() {
        let runner = RustStrategyRunner::new(
            "test_runner".to_string(),
            Box::new(TestTickCounterStrategy::new()),
            1000,
        );

        assert_eq!(runner.id(), "test_runner");
        assert_eq!(runner.strategy_type(), "rust");
        assert_eq!(runner.metrics().received_count(), 0);

        let tick = create_test_tick();
        runner.process_tick(&tick).await;

        assert_eq!(runner.metrics().received_count(), 1);
    }

    #[tokio::test]
    async fn test_rust_runner_multiple_ticks() {
        let runner = RustStrategyRunner::new(
            "test_runner".to_string(),
            Box::new(TestTickCounterStrategy::new()),
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
        let runner = RustStrategyRunner::new(
            "test_runner".to_string(),
            Box::new(TestTickCounterStrategy::new()),
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
