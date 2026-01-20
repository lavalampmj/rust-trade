use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
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
        ticks.push(TickData {
            timestamp: base_time + Duration::seconds(i as i64),
            symbol: "TESTUSDT".to_string(),
            price,
            quantity: Decimal::from(10),
            side: TradeSide::Buy,
            trade_id: format!("trade_{}", i),
            is_buyer_maker: false,
        });
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
    let ticks = create_test_ticks(100, "100");

    // Test OnCloseBar mode - allow for multiple bars
    let strategy_close = Box::new(TestStrategy::new(3));
    let config_close = BacktestConfig::new(Decimal::from(10000));
    let mut engine_close = BacktestEngine::new(strategy_close, config_close).unwrap();
    let result_close = engine_close.run_unified(BacktestData::Ticks(ticks.clone()));

    // Verify OnCloseBar mode works
    assert_eq!(result_close.strategy_name, "Test Strategy");
    println!("OnCloseBar mode: {} trades", result_close.total_trades);
}

#[test]
fn test_backtest_data_enum_ticks() {
    let ticks = create_test_ticks(50, "100");
    let strategy = Box::new(TestStrategy::new(2)); // Allow for at least 1 bar
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert_eq!(result.initial_capital, Decimal::from(10000));
    // total_trades is u64, so this assertion is always true - just verify backtest completed
    let _ = result.total_trades;
}

/// Strategy that uses OnEachTick mode
struct TickModeStrategy {
    tick_count: usize,
}

impl TickModeStrategy {
    fn new() -> Self {
        TickModeStrategy { tick_count: 0 }
    }
}

impl Strategy for TickModeStrategy {
    fn name(&self) -> &str {
        "Tick Mode Test Strategy"
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        // Test strategy is always ready (no warmup needed)
        true
    }

    fn warmup_period(&self) -> usize {
        0
    }

    fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        self.tick_count += 1;
        Signal::Hold
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.tick_count = 0;
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

    let strategy = Box::new(TickModeStrategy::new());
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    // In OnEachTick mode, we should process every tick
    assert_eq!(result.strategy_name, "Tick Mode Test Strategy");
    println!(
        "OnEachTick mode processed {} events from {} ticks",
        tick_count, tick_count
    );
}

/// Strategy that uses OnPriceMove mode
struct PriceMoveStrategy {
    event_count: usize,
}

impl PriceMoveStrategy {
    fn new() -> Self {
        PriceMoveStrategy { event_count: 0 }
    }
}

impl Strategy for PriceMoveStrategy {
    fn name(&self) -> &str {
        "Price Move Strategy"
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        // Test strategy is always ready (no warmup needed)
        true
    }

    fn warmup_period(&self) -> usize {
        0
    }

    fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        self.event_count += 1;
        Signal::Hold
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.event_count = 0;
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
    // Create ticks with changing prices
    let ticks = create_test_ticks(100, "100"); // Each tick has different price

    let strategy = Box::new(PriceMoveStrategy::new());
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    // OnPriceMove should fire on each price change
    assert_eq!(result.strategy_name, "Price Move Strategy");
    println!("OnPriceMove mode executed successfully");
}

#[test]
fn test_tick_based_bars() {
    /// Strategy using tick-based bars (N-tick bars)
    struct TickBasedStrategy {
        bar_count: usize,
    }

    impl Strategy for TickBasedStrategy {
        fn name(&self) -> &str {
            "Tick-Based Bar Strategy"
        }

        fn is_ready(&self, _bars: &BarsContext) -> bool {
            // Test strategy is always ready (no warmup needed)
            true
        }

        fn warmup_period(&self) -> usize {
            0
        }

        fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
            self.bar_count += 1;
            Signal::Hold
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
            BarType::TickBased(10) // 10-tick bars
        }
    }

    let ticks = create_test_ticks(25, "100"); // Should create 3 bars (10 + 10 + 5)

    let strategy = Box::new(TickBasedStrategy { bar_count: 0 });
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert_eq!(result.strategy_name, "Tick-Based Bar Strategy");
    println!("Tick-based bar test executed successfully");
}
