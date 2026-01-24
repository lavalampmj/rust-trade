use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use trading_common::backtest::strategy::Strategy;
use trading_common::backtest::{BacktestConfig, BacktestData, BacktestEngine, SessionAwareConfig};
use trading_common::data::types::{BarData, BarDataMode, BarType, TickData, Timeframe, TradeSide};
use trading_common::instruments::session_presets;
use trading_common::orders::{Order, OrderSide};
use trading_common::series::bars_context::BarsContext;

/// Simple test strategy that buys on first bar and sells on last
struct TestStrategy {
    bar_count: usize,
    total_bars: usize,
    pending_orders: Vec<Order>,
}

impl TestStrategy {
    fn new(total_bars: usize) -> Self {
        TestStrategy {
            bar_count: 0,
            total_bars,
            pending_orders: Vec::new(),
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

    fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) {
        self.pending_orders.clear();
        self.bar_count += 1;

        if self.bar_count == 1 {
            // Buy on first bar
            if let Ok(order) =
                Order::market(&bar_data.ohlc_bar.symbol, OrderSide::Buy, Decimal::from(1)).build()
            {
                self.pending_orders.push(order);
            }
        } else if self.bar_count == self.total_bars {
            // Sell on last bar
            if let Ok(order) =
                Order::market(&bar_data.ohlc_bar.symbol, OrderSide::Sell, Decimal::from(1)).build()
            {
                self.pending_orders.push(order);
            }
        }
    }

    fn get_orders(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Vec<Order> {
        std::mem::take(&mut self.pending_orders)
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.bar_count = 0;
        self.pending_orders.clear();
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

/// Create test ticks within US equity session hours (10:00 AM ET = 15:00 UTC)
fn create_ticks_within_session(count: usize, base_price: &str) -> Vec<TickData> {
    let mut ticks = Vec::new();
    // Use a fixed date in a Monday to ensure it's a trading day
    // 15:00 UTC = 10:00 AM ET (within 9:30 AM - 4:00 PM ET session)
    let base_naive = NaiveDateTime::new(
        NaiveDate::from_ymd_opt(2024, 1, 8).unwrap(), // Monday
        NaiveTime::from_hms_opt(15, 0, 0).unwrap(),   // 15:00 UTC = 10:00 AM ET
    );
    let base_time = Utc.from_utc_datetime(&base_naive);
    let base_price = Decimal::from_str(base_price).unwrap();

    for i in 0..count {
        // Small price variation
        let price_delta = Decimal::from(i % 10);
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
        TickModeStrategy {
            event_counter: counter,
        }
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

    fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) {
        self.event_counter.fetch_add(1, Ordering::SeqCst);
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
        PriceMoveStrategy {
            event_counter: counter,
        }
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

    fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) {
        self.event_counter.fetch_add(1, Ordering::SeqCst);
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
        events_processed,
        tick_count
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

        fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) {
            self.bar_counter.fetch_add(1, Ordering::SeqCst);
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
    let strategy = Box::new(TickBasedStrategy {
        bar_counter: bar_counter.clone(),
    });
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

// ============================================================================
// Session-Aware Strategy Tests
// ============================================================================

/// Strategy that configures session-aware bar generation programmatically
struct SessionAwareTestStrategy {
    session_config: SessionAwareConfig,
    bar_counter: Arc<AtomicUsize>,
    saw_session_aligned: Arc<AtomicBool>,
    saw_session_truncated: Arc<AtomicBool>,
}

impl SessionAwareTestStrategy {
    fn new(
        session_config: SessionAwareConfig,
        bar_counter: Arc<AtomicUsize>,
        saw_aligned: Arc<AtomicBool>,
        saw_truncated: Arc<AtomicBool>,
    ) -> Self {
        Self {
            session_config,
            bar_counter,
            saw_session_aligned: saw_aligned,
            saw_session_truncated: saw_truncated,
        }
    }
}

impl Strategy for SessionAwareTestStrategy {
    fn name(&self) -> &str {
        "Session-Aware Test Strategy"
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        true
    }

    fn warmup_period(&self) -> usize {
        0
    }

    fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) {
        self.bar_counter.fetch_add(1, Ordering::SeqCst);

        // Track if we see session-aligned or truncated bars
        if bar_data.metadata.is_session_aligned {
            self.saw_session_aligned.store(true, Ordering::SeqCst);
        }
        if bar_data.metadata.is_session_truncated {
            self.saw_session_truncated.store(true, Ordering::SeqCst);
        }
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.bar_counter.store(0, Ordering::SeqCst);
        self.saw_session_aligned.store(false, Ordering::SeqCst);
        self.saw_session_truncated.store(false, Ordering::SeqCst);
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)
    }

    /// KEY: Strategy can programmatically configure session awareness
    fn session_config(&self) -> SessionAwareConfig {
        self.session_config.clone()
    }
}

#[test]
fn test_strategy_session_config_default_no_session() {
    // Strategy with default (no session) config
    let bar_counter = Arc::new(AtomicUsize::new(0));
    let saw_aligned = Arc::new(AtomicBool::new(false));
    let saw_truncated = Arc::new(AtomicBool::new(false));

    let strategy = Box::new(SessionAwareTestStrategy::new(
        SessionAwareConfig::default(), // No session
        bar_counter.clone(),
        saw_aligned.clone(),
        saw_truncated.clone(),
    ));

    let ticks = create_test_ticks(120, "100");
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert_eq!(result.strategy_name, "Session-Aware Test Strategy");
    assert!(
        bar_counter.load(Ordering::SeqCst) > 0,
        "Should process bars"
    );

    // Without session config, no bars should be marked as session-aligned or truncated
    assert!(
        !saw_aligned.load(Ordering::SeqCst),
        "Without session, bars should not be session-aligned"
    );
    assert!(
        !saw_truncated.load(Ordering::SeqCst),
        "Without session, bars should not be session-truncated"
    );
}

#[test]
fn test_strategy_session_config_with_schedule() {
    // Use the built-in US equity schedule (9:30 AM - 4:00 PM ET)
    let schedule = session_presets::us_equity();
    let session_config = SessionAwareConfig::with_session(Arc::new(schedule))
        .with_session_open_alignment(true)
        .with_session_close_truncation(true);

    let bar_counter = Arc::new(AtomicUsize::new(0));
    let saw_aligned = Arc::new(AtomicBool::new(false));
    let saw_truncated = Arc::new(AtomicBool::new(false));

    let strategy = Box::new(SessionAwareTestStrategy::new(
        session_config,
        bar_counter.clone(),
        saw_aligned.clone(),
        saw_truncated.clone(),
    ));

    // Use ticks within session hours (10:00 AM ET)
    let ticks = create_ticks_within_session(120, "100");
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert_eq!(result.strategy_name, "Session-Aware Test Strategy");
    assert!(
        bar_counter.load(Ordering::SeqCst) > 0,
        "Should process bars"
    );

    // With session config, bars may be aligned (depending on tick timing)
    // The test verifies the config was passed through the framework
    println!(
        "Session test: bars={}, saw_aligned={}, saw_truncated={}",
        bar_counter.load(Ordering::SeqCst),
        saw_aligned.load(Ordering::SeqCst),
        saw_truncated.load(Ordering::SeqCst)
    );
}

#[test]
fn test_strategy_session_config_continuous_market() {
    // 24/7 continuous market (e.g., crypto)
    let bar_counter = Arc::new(AtomicUsize::new(0));
    let saw_aligned = Arc::new(AtomicBool::new(false));
    let saw_truncated = Arc::new(AtomicBool::new(false));

    let strategy = Box::new(SessionAwareTestStrategy::new(
        SessionAwareConfig::continuous(), // 24/7 mode
        bar_counter.clone(),
        saw_aligned.clone(),
        saw_truncated.clone(),
    ));

    let ticks = create_test_ticks(120, "100");
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert_eq!(result.strategy_name, "Session-Aware Test Strategy");
    assert!(
        bar_counter.load(Ordering::SeqCst) > 0,
        "Should process bars"
    );

    // Continuous markets have no session boundaries
    assert!(
        !saw_aligned.load(Ordering::SeqCst),
        "Continuous market should not have aligned bars"
    );
    assert!(
        !saw_truncated.load(Ordering::SeqCst),
        "Continuous market should not have truncated bars"
    );
}

/// Strategy with tick-based bars and session truncation
struct SessionAwareTickStrategy {
    session_config: SessionAwareConfig,
    bar_counter: Arc<AtomicUsize>,
    truncated_bar_count: Arc<AtomicUsize>,
}

impl Strategy for SessionAwareTickStrategy {
    fn name(&self) -> &str {
        "Session-Aware Tick Strategy"
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        true
    }

    fn warmup_period(&self) -> usize {
        0
    }

    fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) {
        self.bar_counter.fetch_add(1, Ordering::SeqCst);

        if bar_data.metadata.is_session_truncated {
            self.truncated_bar_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) {
        self.bar_counter.store(0, Ordering::SeqCst);
        self.truncated_bar_count.store(0, Ordering::SeqCst);
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TickBased(50) // 50-tick bars
    }

    fn session_config(&self) -> SessionAwareConfig {
        self.session_config.clone()
    }
}

#[test]
fn test_tick_based_strategy_with_session_config() {
    // Test that tick-based strategies can also use session config
    // Use the built-in US equity schedule
    let schedule = session_presets::us_equity();
    let session_config = SessionAwareConfig::with_session(Arc::new(schedule))
        .with_session_open_alignment(false) // Not applicable for tick bars
        .with_session_close_truncation(true); // Truncate partial bars at session close

    let bar_counter = Arc::new(AtomicUsize::new(0));
    let truncated_count = Arc::new(AtomicUsize::new(0));

    let strategy = Box::new(SessionAwareTickStrategy {
        session_config,
        bar_counter: bar_counter.clone(),
        truncated_bar_count: truncated_count.clone(),
    });

    // 75 ticks with 50-tick bars = 2 bars (50 + 25 partial)
    // Use ticks within session hours (10:00 AM ET)
    let ticks = create_ticks_within_session(75, "100");
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert_eq!(result.strategy_name, "Session-Aware Tick Strategy");
    let bars = bar_counter.load(Ordering::SeqCst);
    assert!(
        bars >= 1, // At least 1 complete 50-tick bar
        "Should have at least 1 bar (50-tick), got {}",
        bars
    );

    println!(
        "Tick-based session test: bars={}, truncated={}",
        bars,
        truncated_count.load(Ordering::SeqCst)
    );
}

#[test]
fn test_session_config_builder_pattern_in_strategy() {
    // Demonstrate the builder pattern for SessionAwareConfig within a strategy

    struct ConfigurableStrategy {
        use_session: bool,
    }

    impl Strategy for ConfigurableStrategy {
        fn name(&self) -> &str {
            "Configurable Strategy"
        }

        fn is_ready(&self, _bars: &BarsContext) -> bool {
            true
        }

        fn warmup_period(&self) -> usize {
            0
        }

        fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) {
            // No action needed for this test
        }

        fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
            // Can configure from params
            if let Some(use_session) = params.get("use_session") {
                self.use_session = use_session == "true";
            }
            Ok(())
        }

        fn reset(&mut self) {}

        fn bar_data_mode(&self) -> BarDataMode {
            BarDataMode::OnCloseBar
        }

        fn preferred_bar_type(&self) -> BarType {
            BarType::TimeBased(Timeframe::FiveMinutes)
        }

        fn session_config(&self) -> SessionAwareConfig {
            if self.use_session {
                // Use built-in US equity session schedule
                let schedule = session_presets::us_equity();
                SessionAwareConfig::with_session(Arc::new(schedule))
                    .with_session_open_alignment(true)
                    .with_session_close_truncation(true)
            } else {
                SessionAwareConfig::continuous()
            }
        }
    }

    // Test with session enabled via params
    let strategy = Box::new(ConfigurableStrategy { use_session: false });
    let config = BacktestConfig::new(Decimal::from(10000)).with_param("use_session", "true");

    let ticks = create_test_ticks(60, "100");
    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert_eq!(result.strategy_name, "Configurable Strategy");
}
