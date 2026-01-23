use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use trading_common::backtest::strategy::{Signal, Strategy};
use trading_common::backtest::{BacktestConfig, BacktestData, BacktestEngine};
use trading_common::data::types::{BarData, BarDataMode, BarType, TickData, Timeframe, TradeSide};
use trading_common::series::bars_context::BarsContext;

/// Simple test strategy that buys on first bar and sells on last
struct TestStrategy {
    bar_count: usize,
    total_bars: usize,
}

impl TestStrategy {
    fn new(total_bars: usize) -> Self {
        TestStrategy {
            bar_count: 0,
            total_bars,
        }
    }
}

impl Strategy for TestStrategy {
    fn name(&self) -> &str {
        "Test Strategy"
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        // Test strategy is always ready (no warmup needed)
        true
    }

    fn warmup_period(&self) -> usize {
        0
    }

    fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        self.bar_count += 1;

        if self.bar_count == 1 {
            // Buy on first bar
            Signal::Buy {
                symbol: bar_data.ohlc_bar.symbol.clone(),
                quantity: Decimal::from(1),
            }
        } else if self.bar_count == self.total_bars {
            // Sell on last bar
            Signal::Sell {
                symbol: bar_data.ohlc_bar.symbol.clone(),
                quantity: Decimal::from(1),
            }
        } else {
            Signal::Hold
        }
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.bar_count = 0;
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)
    }
}

fn create_test_ticks(count: usize, base_price: &str) -> Vec<TickData> {
    let mut ticks = Vec::new();
    let base_time = Utc::now();
    let base_price = Decimal::from_str(base_price).unwrap();

    for i in 0..count {
        // Small price variation
        let price_delta = Decimal::from(i % 10); // Keep price changes small
        let price = base_price + price_delta;
        let ts = base_time + Duration::seconds(i as i64);
        ticks.push(TickData::with_details(
            ts,
            ts,
            "TESTUSDT".to_string(),
            "TEST".to_string(),
            price,
            Decimal::from(10),
            TradeSide::Buy,
            "TEST".to_string(),
            format!("trade_{}", i),
            false,
            i as i64,
        ));
    }

    ticks
}

#[test]
fn test_unified_backtest_on_close_bar_mode() {
    // Create 120 ticks - use lower price that fits in capital
    let ticks = create_test_ticks(120, "100");

    // Expect at least 1 bar (could be 2-3 depending on timing alignment)
    let strategy = Box::new(TestStrategy::new(3)); // Allow up to 3 bars
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    // Verify basic results
    assert_eq!(result.strategy_name, "Test Strategy");
    assert!(result.total_trades >= 2); // At least 1 buy + 1 sell
    assert!(result.final_value > Decimal::ZERO);
}

#[test]
fn test_unified_backtest_modes_comparison() {
    // Create test data - use affordable price
    // 100 ticks at 1 second apart = 100 seconds = ~2 bars at 1-minute timeframe
    let ticks = create_test_ticks(100, "100");

    // Test OnCloseBar mode - use total_bars=2 to match the ~2 bars generated
    let strategy_close = Box::new(TestStrategy::new(2));
    let config_close = BacktestConfig::new(Decimal::from(10000));
    let mut engine_close = BacktestEngine::new(strategy_close, config_close).unwrap();
    let result_close = engine_close.run_unified(BacktestData::Ticks(ticks.clone()));

    // Verify OnCloseBar mode works - should execute trades (buy + sell)
    assert_eq!(result_close.strategy_name, "Test Strategy");
    // In OnCloseBar mode with TestStrategy, we expect exactly 2 trades (buy on bar 1, sell on bar 2)
    assert_eq!(
        result_close.total_trades, 2,
        "OnCloseBar mode should generate exactly 2 trades (buy + sell), got {}",
        result_close.total_trades
    );
    // Verify final value differs from initial (position was taken)
    assert_ne!(
        result_close.final_value, result_close.initial_capital,
        "Final value should differ from initial capital after trades"
    );
}

#[test]
fn test_backtest_data_enum_ticks() {
    // Create 120 ticks at 1 second apart = 120 seconds = 2 full 1-minute bars
    let ticks = create_test_ticks(120, "100");
    let tick_count = ticks.len();
    let strategy = Box::new(TestStrategy::new(2)); // Sell on bar 2
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    // Verify backtest processed the tick data correctly
    assert_eq!(result.initial_capital, Decimal::from(10000));
    // TestStrategy buys on bar 1 and sells on bar 2, so we expect exactly 2 trades
    assert_eq!(
        result.total_trades, 2,
        "TestStrategy should execute exactly 2 trades (buy + sell), got {}",
        result.total_trades
    );
    // Verify the strategy name was correctly set
    assert_eq!(result.strategy_name, "Test Strategy");
    // Verify backtest processed ticks
    assert_eq!(tick_count, 120, "Should have 120 ticks");
}

/// Strategy that uses OnEachTick mode with shared counter for verification
struct TickModeStrategy {
    event_counter: Arc<AtomicUsize>,
}

impl TickModeStrategy {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        TickModeStrategy { event_counter: counter }
    }
}

impl Strategy for TickModeStrategy {
    fn name(&self) -> &str {
        "Tick Mode Test Strategy"
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        true
    }

    fn warmup_period(&self) -> usize {
        0
    }

    fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        self.event_counter.fetch_add(1, Ordering::SeqCst);
        Signal::Hold
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.event_counter.store(0, Ordering::SeqCst);
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnEachTick
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)
    }
}

#[test]
fn test_on_each_tick_mode() {
    let ticks = create_test_ticks(50, "100");
    let tick_count = ticks.len();

    // Use shared counter to verify on_bar_data was called for each tick
    let event_counter = Arc::new(AtomicUsize::new(0));
    let strategy = Box::new(TickModeStrategy::new(event_counter.clone()));
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    // Verify strategy name
    assert_eq!(result.strategy_name, "Tick Mode Test Strategy");

    // In OnEachTick mode, on_bar_data should be called for EVERY tick
    let events_processed = event_counter.load(Ordering::SeqCst);
    assert_eq!(
        events_processed, tick_count,
        "OnEachTick mode should call on_bar_data for every tick. Expected {}, got {}",
        tick_count, events_processed
    );
}

/// Strategy that uses OnPriceMove mode with shared counter for verification
struct PriceMoveStrategy {
    event_counter: Arc<AtomicUsize>,
}

impl PriceMoveStrategy {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        PriceMoveStrategy { event_counter: counter }
    }
}

impl Strategy for PriceMoveStrategy {
    fn name(&self) -> &str {
        "Price Move Strategy"
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        true
    }

    fn warmup_period(&self) -> usize {
        0
    }

    fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        self.event_counter.fetch_add(1, Ordering::SeqCst);
        Signal::Hold
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.event_counter.store(0, Ordering::SeqCst);
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnPriceMove
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)
    }
}

#[test]
fn test_on_price_move_mode() {
    // Create ticks - price cycles through 10 values (i % 10), so not all ticks have unique prices
    // For 100 ticks with price = base + (i % 10), we have 10 unique prices cycling
    let ticks = create_test_ticks(100, "100");
    let tick_count = ticks.len();

    // Count unique price changes (first tick always fires, subsequent only on change)
    // With i % 10 pattern: prices are 100, 101, 102, ..., 109, 100, 101, ...
    // Each cycle of 10 has 10 changes (going 109->100 is a change too)
    // So we expect approximately tick_count price move events

    let event_counter = Arc::new(AtomicUsize::new(0));
    let strategy = Box::new(PriceMoveStrategy::new(event_counter.clone()));
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    // Verify strategy name
    assert_eq!(result.strategy_name, "Price Move Strategy");

    // OnPriceMove should fire on each price change
    let events_processed = event_counter.load(Ordering::SeqCst);

    // With cycling prices (i % 10), every tick has a different price from the previous
    // so we should have approximately tick_count events
    assert!(
        events_processed > 0,
        "OnPriceMove mode should fire at least once, got 0 events"
    );
    assert!(
        events_processed <= tick_count,
        "OnPriceMove events ({}) should not exceed tick count ({})",
        events_processed, tick_count
    );
    // Since all consecutive ticks have different prices in our test data,
    // we expect events_processed to equal tick_count
    assert_eq!(
        events_processed, tick_count,
        "With unique consecutive prices, OnPriceMove should fire for every tick. Expected {}, got {}",
        tick_count, events_processed
    );
}

#[test]
fn test_tick_based_bars() {
    /// Strategy using tick-based bars (N-tick bars) with shared counter
    struct TickBasedStrategy {
        bar_counter: Arc<AtomicUsize>,
    }

    impl Strategy for TickBasedStrategy {
        fn name(&self) -> &str {
            "Tick-Based Bar Strategy"
        }

        fn is_ready(&self, _bars: &BarsContext) -> bool {
            true
        }

        fn warmup_period(&self) -> usize {
            0
        }

        fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
            self.bar_counter.fetch_add(1, Ordering::SeqCst);
            Signal::Hold
        }

        fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
            Ok(())
        }

        fn reset(&mut self) {
            self.bar_counter.store(0, Ordering::SeqCst);
        }

        fn bar_data_mode(&self) -> BarDataMode {
            BarDataMode::OnCloseBar
        }

        fn preferred_bar_type(&self) -> BarType {
            BarType::TickBased(10) // 10-tick bars
        }
    }

    // 25 ticks with 10-tick bars = 3 bars (10 + 10 + 5 ticks)
    // In OnCloseBar mode, we get an event when each bar closes
    let ticks = create_test_ticks(25, "100");
    let tick_count = ticks.len();
    let ticks_per_bar = 10;
    let expected_bars = (tick_count + ticks_per_bar - 1) / ticks_per_bar; // Ceiling division

    let bar_counter = Arc::new(AtomicUsize::new(0));
    let strategy = Box::new(TickBasedStrategy { bar_counter: bar_counter.clone() });
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    // Verify strategy name
    assert_eq!(result.strategy_name, "Tick-Based Bar Strategy");

    // Verify correct number of bars were generated
    let bars_processed = bar_counter.load(Ordering::SeqCst);
    assert_eq!(
        bars_processed, expected_bars,
        "With {} ticks and {}-tick bars, expected {} bars, got {}",
        tick_count, ticks_per_bar, expected_bars, bars_processed
    );
}
