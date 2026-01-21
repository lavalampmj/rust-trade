//! Test Tick Counter Strategy
//!
//! A minimal strategy for counting ticks and measuring latency in integration tests.
//! This strategy does not generate trading signals - it simply counts every tick
//! it receives and records latency measurements.

use rust_decimal::Decimal;
use std::collections::HashMap;

use trading_common::backtest::strategy::{Signal, Strategy};
use trading_common::data::types::{BarData, BarDataMode, BarType, Timeframe};
use trading_common::series::bars_context::BarsContext;
use trading_common::series::MaximumBarsLookBack;

/// Minimal test strategy for counting ticks and measuring latency
///
/// This strategy does not generate trading signals - it simply counts
/// every tick it receives and records latency measurements.
///
/// # Example
///
/// ```rust,ignore
/// use integration_tests::strategies::TestTickCounterStrategy;
///
/// let mut strategy = TestTickCounterStrategy::new();
/// strategy.process_tick_direct(Decimal::from(100), Decimal::from(10));
/// assert_eq!(strategy.tick_count(), 1);
/// ```
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

#[cfg(test)]
mod tests {
    use super::*;
    use trading_common::series::bars_context::BarsContext;

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

    #[test]
    fn test_tick_counter_default() {
        let strategy = TestTickCounterStrategy::default();
        assert_eq!(strategy.tick_count(), 0);
        assert_eq!(strategy.total_value(), Decimal::ZERO);
    }
}
