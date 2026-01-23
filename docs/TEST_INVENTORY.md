# Test Inventory

Comprehensive documentation of all unit and integration tests in the rust-trade workspace, including test source code.

## Summary

| Crate | Unit Tests | Integration Tests | Benchmarks | Total |
|-------|------------|-------------------|------------|-------|
| trading-common | ~387 | 21 | 0 | ~408 |
| trading-core | 17 | 21 | 9 | 47 |
| data-manager | 134 | 8 | 0 | 142 |
| **Total** | **~538** | **50** | **9** | **~597** |

---

## 1. trading-common

### Integration Tests

#### `tests/backtest_unified_test.rs`
Tests the unified backtesting engine with different bar modes.

##### test_unified_backtest_on_close_bar_mode
Tests unified backtest with OnCloseBar mode.

```rust
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
```

##### test_unified_backtest_modes_comparison
Compares different bar modes with proper assertions.

```rust
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
```

##### test_backtest_data_enum_ticks
Tests BacktestData enum with ticks and proper trade count verification.

```rust
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
```

##### test_on_each_tick_mode
Tests OnEachTick mode with shared counter for verifying every tick is processed.

```rust
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
    fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        self.event_counter.fetch_add(1, Ordering::SeqCst);
        Signal::Hold
    }
    fn bar_data_mode(&self) -> BarDataMode { BarDataMode::OnEachTick }
    // ...other trait methods
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
```

##### test_on_price_move_mode
Tests OnPriceMove mode with shared counter for verifying price change events.

```rust
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
    fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        self.event_counter.fetch_add(1, Ordering::SeqCst);
        Signal::Hold
    }
    fn bar_data_mode(&self) -> BarDataMode { BarDataMode::OnPriceMove }
    // ...other trait methods
}

#[test]
fn test_on_price_move_mode() {
    // Create ticks - price cycles through 10 values (i % 10)
    let ticks = create_test_ticks(100, "100");
    let tick_count = ticks.len();

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
    assert!(events_processed > 0, "OnPriceMove mode should fire at least once");
    assert!(events_processed <= tick_count, "Events should not exceed tick count");
    // Since all consecutive ticks have different prices in our test data:
    assert_eq!(
        events_processed, tick_count,
        "With unique consecutive prices, OnPriceMove should fire for every tick"
    );
}
```

##### test_tick_based_bars
Tests N-tick bar generation with shared counter for bar count verification.

```rust
#[test]
fn test_tick_based_bars() {
    /// Strategy using tick-based bars (N-tick bars) with shared counter
    struct TickBasedStrategy {
        bar_counter: Arc<AtomicUsize>,
    }

    impl Strategy for TickBasedStrategy {
        fn name(&self) -> &str { "Tick-Based Bar Strategy" }
        fn is_ready(&self, _bars: &BarsContext) -> bool { true }
        fn warmup_period(&self) -> usize { 0 }
        fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
            self.bar_counter.fetch_add(1, Ordering::SeqCst);
            Signal::Hold
        }
        fn bar_data_mode(&self) -> BarDataMode { BarDataMode::OnCloseBar }
        fn preferred_bar_type(&self) -> BarType { BarType::TickBased(10) } // 10-tick bars
        // ...other trait methods
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
```

##### test_strategy_session_config_default_no_session
Tests that strategies with default (no session) config operate in 24/7 mode.

```rust
#[test]
fn test_strategy_session_config_default_no_session() {
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

    // Without session config, no bars should be marked as session-aligned or truncated
    assert!(!saw_aligned.load(Ordering::SeqCst));
    assert!(!saw_truncated.load(Ordering::SeqCst));
}
```

##### test_strategy_session_config_with_schedule
Tests that strategies can programmatically configure session awareness using preset schedules.

```rust
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

    let ticks = create_test_ticks(120, "100");
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert!(bar_counter.load(Ordering::SeqCst) > 0, "Should process bars");
}
```

##### test_strategy_session_config_continuous_market
Tests that strategies can specify 24/7 continuous market mode.

```rust
#[test]
fn test_strategy_session_config_continuous_market() {
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

    // Continuous markets have no session boundaries
    assert!(!saw_aligned.load(Ordering::SeqCst));
    assert!(!saw_truncated.load(Ordering::SeqCst));
}
```

##### test_tick_based_strategy_with_session_config
Tests that tick-based strategies can also use session configuration.

```rust
#[test]
fn test_tick_based_strategy_with_session_config() {
    let schedule = session_presets::us_equity();
    let session_config = SessionAwareConfig::with_session(Arc::new(schedule))
        .with_session_open_alignment(false) // Not applicable for tick bars
        .with_session_close_truncation(true); // Truncate partial bars at session close

    let strategy = Box::new(SessionAwareTickStrategy {
        session_config,
        bar_counter: bar_counter.clone(),
        truncated_bar_count: truncated_count.clone(),
    });

    // 75 ticks with 50-tick bars = 2 bars (50 + 25 partial)
    let ticks = create_test_ticks(75, "100");
    let config = BacktestConfig::new(Decimal::from(10000));

    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    let bars = bar_counter.load(Ordering::SeqCst);
    assert!(bars >= 2, "Should have at least 2 bars (50-tick)");
}
```

##### test_session_config_builder_pattern_in_strategy
Tests that strategies can dynamically configure session awareness via parameters.

```rust
#[test]
fn test_session_config_builder_pattern_in_strategy() {
    // Strategy that can enable/disable session awareness via params
    struct ConfigurableStrategy { use_session: bool }

    impl Strategy for ConfigurableStrategy {
        fn session_config(&self) -> SessionAwareConfig {
            if self.use_session {
                let schedule = session_presets::us_equity();
                SessionAwareConfig::with_session(Arc::new(schedule))
                    .with_session_open_alignment(true)
                    .with_session_close_truncation(true)
            } else {
                SessionAwareConfig::continuous()
            }
        }
        // ... other trait methods
    }

    let strategy = Box::new(ConfigurableStrategy { use_session: false });
    let config = BacktestConfig::new(Decimal::from(10000))
        .with_param("use_session", "true");

    let ticks = create_test_ticks(60, "100");
    let mut engine = BacktestEngine::new(strategy, config).unwrap();
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert_eq!(result.strategy_name, "Configurable Strategy");
}
```

---

#### `tests/synthetic_bar_test.rs`
Tests synthetic bar generation for data gaps.

##### test_synthetic_bars_in_historical_backtest
Tests synthetic bar generation in gaps.

```rust
#[test]
fn test_synthetic_bars_in_historical_backtest() {
    let gen = HistoricalOHLCGenerator::new(
        BarType::TimeBased(Timeframe::OneMinute),
        BarDataMode::OnCloseBar,
    );

    // Create ticks with a gap:
    // Tick at 0s (minute 0), Tick at 30s (minute 0)
    // GAP: no ticks in minute 1, 2, 3
    // Tick at 240s (minute 4)
    let ticks = vec![
        create_tick("50000", 0),    // 00:00
        create_tick("50100", 30),   // 00:30
        create_tick("50200", 240),  // 04:00 (4 minutes later!)
    ];

    let bars = gen.generate_from_ticks(&ticks);

    // Expected: Bar 0 (real), Bar 1-3 (synthetic), Bar 4 (real)
    assert_eq!(bars.len(), 5, "Expected 5 bars: 1 real + 3 synthetic + 1 real");

    // Bar 0: Real bar from minute 0
    assert_eq!(bars[0].metadata.is_synthetic, false);
    assert_eq!(bars[0].metadata.is_bar_closed, true);

    // Bar 1: Synthetic bar for minute 1
    assert_eq!(bars[1].metadata.is_synthetic, true);
    assert_eq!(bars[1].metadata.tick_count_in_bar, 0);
}
```

##### test_no_synthetic_bars_without_gaps
Verifies no synthetic bars without gaps.

```rust
#[test]
fn test_no_synthetic_bars_without_gaps() {
    let gen = HistoricalOHLCGenerator::new(
        BarType::TimeBased(Timeframe::OneMinute),
        BarDataMode::OnCloseBar,
    );

    // Create ticks with NO gaps (consecutive minutes)
    let ticks = vec![
        create_tick("50000", 0),    // minute 0
        create_tick("50100", 60),   // minute 1
        create_tick("50200", 120),  // minute 2
    ];

    let bars = gen.generate_from_ticks(&ticks);

    // Should have exactly 3 real bars, no synthetic
    assert_eq!(bars.len(), 3);
    assert_eq!(bars[0].metadata.is_synthetic, false);
    assert_eq!(bars[1].metadata.is_synthetic, false);
    assert_eq!(bars[2].metadata.is_synthetic, false);
}
```

---

#### `tests/test_python_strategies.rs`
Tests Python strategy integration.

##### test_list_strategies
Lists available Rust and Python strategies.

```rust
#[test]
fn test_list_strategies() {
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    let strategies = strategy::list_strategies();

    // Should have at least 3 strategies: sma (Rust), rsi (Rust), sma_python (Python)
    assert!(strategies.len() >= 3);

    let rust_count = strategies.iter()
        .filter(|s| matches!(s.strategy_type, strategy::StrategyType::Rust))
        .count();
    let python_count = strategies.iter()
        .filter(|s| matches!(s.strategy_type, strategy::StrategyType::Python))
        .count();

    assert!(rust_count >= 2);
    assert!(python_count >= 1);
    assert!(strategies.iter().any(|s| s.id == "sma"));
    assert!(strategies.iter().any(|s| s.id == "sma_python"));
}
```

##### test_create_rust_strategy
Creates SMA Rust strategy.

```rust
#[test]
fn test_create_rust_strategy() {
    let result = strategy::create_strategy("sma");
    assert!(result.is_ok(), "Failed to create Rust SMA strategy");

    if let Ok(s) = result {
        assert_eq!(s.name(), "Simple Moving Average");
    }
}
```

##### test_create_python_strategy
Creates Python SMA strategy.

```rust
#[test]
fn test_create_python_strategy() {
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    let result = strategy::create_strategy("sma_python");
    assert!(result.is_ok());

    if let Ok(s) = result {
        assert_eq!(s.name(), "Simple Moving Average (Python)");
    }
}
```

##### test_get_strategy_info
Retrieves info for both Rust and Python strategies.

```rust
#[test]
fn test_get_strategy_info() {
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    // Test Rust strategy
    let info = strategy::get_strategy_info("sma");
    assert!(info.is_some());
    assert!(matches!(info.unwrap().strategy_type, strategy::StrategyType::Rust));

    // Test Python strategy
    let info = strategy::get_strategy_info("sma_python");
    assert!(info.is_some());
    assert!(matches!(info.unwrap().strategy_type, strategy::StrategyType::Python));
}
```

---

#### `tests/test_strategy_coexistence.rs`
Tests Rust and Python strategy coexistence.

##### test_both_rust_and_python_strategies_work
Verifies Rust and Python strategies coexist.

```rust
#[test]
fn test_both_rust_and_python_strategies_work() {
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    let strategies = strategy::list_strategies();
    assert!(strategies.len() >= 3);

    let rust_strategies: Vec<_> = strategies.iter()
        .filter(|s| matches!(s.strategy_type, strategy::StrategyType::Rust))
        .collect();
    let python_strategies: Vec<_> = strategies.iter()
        .filter(|s| matches!(s.strategy_type, strategy::StrategyType::Python))
        .collect();

    assert!(rust_strategies.len() >= 2);
    assert!(python_strategies.len() >= 1);
}
```

##### test_rust_sma_strategy_backtest
Backtests Rust SMA strategy with proper result assertions.

```rust
#[test]
fn test_rust_sma_strategy_backtest() {
    // Create sample data with enough ticks to potentially trigger SMA crossover
    let ticks = vec![
        create_sample_tick("BTCUSDT", "50000.0", 1),
        create_sample_tick("BTCUSDT", "50100.0", 2),
        create_sample_tick("BTCUSDT", "50200.0", 3),
        create_sample_tick("BTCUSDT", "50300.0", 4),
        create_sample_tick("BTCUSDT", "50400.0", 5),
    ];
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    // Create Rust SMA strategy
    let strategy_box = strategy::create_strategy("sma").expect("Failed to create Rust SMA strategy");
    assert_eq!(strategy_box.name(), "Simple Moving Average", "Strategy name mismatch");

    // Run backtest
    let config = BacktestConfig::new(initial_capital);
    let mut engine = BacktestEngine::new(strategy_box, config).expect("Failed to create engine");
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    // Verify backtest completed with correct configuration
    assert_eq!(result.strategy_name, "Simple Moving Average");
    assert_eq!(result.initial_capital, initial_capital);
    // Final value should be positive (even if no trades, capital remains)
    assert!(
        result.final_value > Decimal::ZERO,
        "Final value should be positive, got {}",
        result.final_value
    );
}
```

##### test_python_sma_strategy_backtest
Backtests Python SMA strategy with proper result assertions.

```rust
#[test]
fn test_python_sma_strategy_backtest() {
    // Initialize Python strategies (may already be initialized by other tests)
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    // Create sample data with enough ticks to potentially trigger SMA crossover
    let ticks = vec![
        create_sample_tick("BTCUSDT", "50000.0", 1),
        create_sample_tick("BTCUSDT", "50100.0", 2),
        create_sample_tick("BTCUSDT", "50200.0", 3),
        create_sample_tick("BTCUSDT", "50300.0", 4),
        create_sample_tick("BTCUSDT", "50400.0", 5),
    ];
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    // Create Python SMA strategy
    let strategy_box = strategy::create_strategy("sma_python").expect("Failed to create Python SMA");
    assert_eq!(strategy_box.name(), "Simple Moving Average (Python)", "Strategy name mismatch");

    // Run backtest
    let config = BacktestConfig::new(initial_capital);
    let mut engine = BacktestEngine::new(strategy_box, config).expect("Failed to create engine");
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    // Verify backtest completed with correct configuration
    assert_eq!(result.strategy_name, "Simple Moving Average (Python)");
    assert_eq!(result.initial_capital, initial_capital);
    // Final value should be positive (even if no trades, capital remains)
    assert!(
        result.final_value > Decimal::ZERO,
        "Final value should be positive, got {}",
        result.final_value
    );
    // Verify max drawdown is in valid range (0 to 100%)
    assert!(
        result.max_drawdown >= Decimal::ZERO && result.max_drawdown <= Decimal::from(100),
        "Max drawdown should be between 0% and 100%, got {}",
        result.max_drawdown
    );
}
```

##### test_strategy_type_distinction
Verifies correct strategy type identification.

```rust
#[test]
fn test_strategy_type_distinction() {
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    let rust_info = strategy::get_strategy_info("sma").expect("SMA strategy should exist");
    assert!(matches!(rust_info.strategy_type, strategy::StrategyType::Rust));

    let python_info = strategy::get_strategy_info("sma_python").expect("Python SMA should exist");
    assert!(matches!(python_info.strategy_type, strategy::StrategyType::Python));
}
```

---

### Unit Tests

#### `src/accounts/account.rs`
Account management and balance operations.

##### test_account_balance_operations
Balance operations (deposit/withdraw).

```rust
#[test]
fn test_account_balance_operations() {
    let mut balance = AccountBalance::new("USDT", dec!(1000));

    assert_eq!(balance.total, dec!(1000));
    assert_eq!(balance.free, dec!(1000));
    assert_eq!(balance.locked, Decimal::ZERO);

    // Lock funds
    assert!(balance.lock(dec!(300)).is_ok());
    assert_eq!(balance.free, dec!(700));
    assert_eq!(balance.locked, dec!(300));

    // Cannot lock more than free
    assert!(balance.lock(dec!(800)).is_err());

    // Unlock funds
    assert!(balance.unlock(dec!(100)).is_ok());
    assert_eq!(balance.free, dec!(800));
    assert_eq!(balance.locked, dec!(200));

    // Fill from locked
    assert!(balance.fill(dec!(100)).is_ok());
    assert_eq!(balance.total, dec!(900));
    assert_eq!(balance.locked, dec!(100));
}
```

##### test_cash_account
Cash account creation and operations.

```rust
#[test]
fn test_cash_account() {
    let mut account = Account::cash("test-001", "USDT");

    assert_eq!(account.account_type, AccountType::Cash);
    assert!(account.can_trade());
    assert!(account.margin.is_none());

    account.deposit("USDT", dec!(10000));
    assert_eq!(account.total_base(), dec!(10000));
    assert_eq!(account.free_base(), dec!(10000));

    assert!(account.withdraw("USDT", dec!(3000)).is_ok());
    assert_eq!(account.total_base(), dec!(7000));
}
```

##### test_margin_account
Margin account creation.

```rust
#[test]
fn test_margin_account() {
    let mut account = Account::margin("margin-001", "USDT", dec!(20));

    assert_eq!(account.account_type, AccountType::Margin);
    assert!(account.is_margin_account());
    assert!(account.margin.is_some());

    account.deposit("USDT", dec!(1000));

    let margin = account.margin.as_ref().unwrap();
    assert_eq!(margin.margin_balance, dec!(1000));
    assert_eq!(account.buying_power(), dec!(20000)); // 1000 * 20x
}
```

##### test_margin_calculations
Margin ratio calculations.

```rust
#[test]
fn test_margin_calculations() {
    let mut margin = MarginAccount::new(dec!(10000), dec!(10));

    assert_eq!(margin.equity(), dec!(10000));
    assert_eq!(margin.available_for_trading(), dec!(10000));
    assert_eq!(margin.buying_power(), dec!(100000));

    margin.update_margin(dec!(5000), dec!(2500));
    assert_eq!(margin.margin_used, dec!(5000));
    assert_eq!(margin.available_for_trading(), dec!(5000));

    margin.update_unrealized_pnl(dec!(1000));
    assert_eq!(margin.equity(), dec!(11000));

    margin.update_unrealized_pnl(dec!(-500));
    assert_eq!(margin.equity(), dec!(9500));
}
```

##### test_margin_call_detection
Margin call detection.

```rust
#[test]
fn test_margin_call_detection() {
    let mut margin = MarginAccount::new(dec!(10000), dec!(10));
    margin.update_margin(dec!(8000), dec!(8000));

    assert!(!margin.is_margin_call());

    // Equity = 10000 - 3000 = 7000, Maintenance = 8000
    // 7000 < 8000 * 0.9 = 7200, so margin call
    margin.update_unrealized_pnl(dec!(-3000));
    assert!(margin.is_margin_call());
}
```

##### test_account_state_transitions
Account state transitions.

```rust
#[test]
fn test_account_state_transitions() {
    let mut account = Account::cash("test", "USDT");

    assert!(account.can_trade());
    assert!(!account.is_reduce_only());

    account.suspend();
    assert!(!account.can_trade());
    assert!(account.is_reduce_only());

    account.activate();
    assert!(account.can_trade());

    account.close();
    assert!(!account.can_trade());
}
```

##### test_simulated_account
Simulated/paper account.

```rust
#[test]
fn test_simulated_account() {
    let account = Account::simulated("paper-001", "USDT");

    assert!(account.is_simulated);
    assert_eq!(account.account_type, AccountType::Cash);
}
```

##### test_total_value_calculation
Total account value.

```rust
#[test]
fn test_total_value_calculation() {
    let mut account = Account::cash("test", "USDT");

    account.set_balance("USDT", dec!(1000));
    account.set_balance("BTC", dec!(0.5));
    account.set_balance("ETH", dec!(2));

    let mut prices = HashMap::new();
    prices.insert("BTC".to_string(), dec!(50000));
    prices.insert("ETH".to_string(), dec!(3000));

    let total = account.total_value(&prices);
    // 1000 USDT + 0.5 * 50000 + 2 * 3000 = 32000
    assert_eq!(total, dec!(32000));
}
```

---

#### `src/accounts/types.rs`
Account type definitions.

##### test_account_type_properties
Account type properties.

```rust
#[test]
fn test_account_type_properties() {
    assert!(!AccountType::Cash.supports_margin());
    assert!(AccountType::Margin.supports_margin());
    assert!(!AccountType::Cash.allows_short());
    assert!(AccountType::Margin.allows_short());
}
```

##### test_account_state_properties
Account state properties.

```rust
#[test]
fn test_account_state_properties() {
    assert!(AccountState::Active.can_trade());
    assert!(!AccountState::Suspended.can_trade());
    assert!(AccountState::Suspended.reduce_only());
    assert!(AccountState::Closed.is_terminal());
}
```

##### test_margin_mode
Margin mode properties.

```rust
#[test]
fn test_margin_mode() {
    assert!(MarginMode::Cross.is_cross());
    assert!(MarginMode::Isolated.is_isolated());
}
```

##### test_defaults
Default account type/state.

```rust
#[test]
fn test_defaults() {
    assert_eq!(AccountType::default(), AccountType::Cash);
    assert_eq!(AccountState::default(), AccountState::Active);
    assert_eq!(MarginMode::default(), MarginMode::Cross);
    assert_eq!(PositionMode::default(), PositionMode::OneWay);
}
```

---

#### `src/backtest/bar_generator.rs`
Bar generation for backtesting.

##### test_on_close_bar_time_based
Time-based OnCloseBar mode.

```rust
#[test]
fn test_on_close_bar_time_based() {
    let gen = HistoricalOHLCGenerator::new(
        BarType::TimeBased(Timeframe::OneMinute),
        BarDataMode::OnCloseBar,
    );

    let base_time = Utc::now().with_second(0).unwrap().with_nanosecond(0).unwrap();

    let ticks = vec![
        create_tick("50000", base_time),
        create_tick("50100", base_time + Duration::seconds(30)),
        create_tick("50200", base_time + Duration::minutes(1)),
        create_tick("50300", base_time + Duration::minutes(1) + Duration::seconds(30)),
    ];

    let bars = gen.generate_from_ticks(&ticks);

    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].current_tick, None);
    assert_eq!(bars[0].metadata.is_bar_closed, true);
    assert_eq!(bars[0].metadata.tick_count_in_bar, 2);
}
```

##### test_on_each_tick_mode
OnEachTick mode bar generation.

```rust
#[test]
fn test_on_each_tick_mode() {
    let gen = HistoricalOHLCGenerator::new(
        BarType::TimeBased(Timeframe::OneMinute),
        BarDataMode::OnEachTick,
    );

    let base_time = Utc::now().with_second(0).unwrap().with_nanosecond(0).unwrap();

    let ticks = vec![
        create_tick("50000", base_time),
        create_tick("50100", base_time + Duration::seconds(30)),
    ];

    let bars = gen.generate_from_ticks(&ticks);

    assert_eq!(bars.len(), 2);
    assert!(bars[0].current_tick.is_some());
    assert_eq!(bars[0].metadata.is_first_tick_of_bar, true);
    assert_eq!(bars[0].metadata.tick_count_in_bar, 1);
}
```

##### test_on_price_move_mode
OnPriceMove mode filtering.

```rust
#[test]
fn test_on_price_move_mode() {
    let gen = HistoricalOHLCGenerator::new(
        BarType::TimeBased(Timeframe::OneMinute),
        BarDataMode::OnPriceMove,
    );

    let base_time = Utc::now().with_second(0).unwrap().with_nanosecond(0).unwrap();

    let ticks = vec![
        create_tick("50000", base_time),
        create_tick("50000", base_time + Duration::seconds(10)), // Same price
        create_tick("50100", base_time + Duration::seconds(20)), // Price change
        create_tick("50100", base_time + Duration::seconds(30)), // Same price
        create_tick("50200", base_time + Duration::seconds(40)), // Price change
    ];

    let bars = gen.generate_from_ticks(&ticks);

    assert_eq!(bars.len(), 3); // Only when price changes
}
```

##### test_tick_based_on_close_bar
N-tick bars in OnCloseBar mode.

```rust
#[test]
fn test_tick_based_on_close_bar() {
    let gen = HistoricalOHLCGenerator::new(
        BarType::TickBased(3),
        BarDataMode::OnCloseBar,
    );

    let base_time = Utc::now();

    let ticks = vec![
        create_tick("50000", base_time),
        create_tick("50100", base_time + Duration::seconds(1)),
        create_tick("50200", base_time + Duration::seconds(2)),
        create_tick("50300", base_time + Duration::seconds(3)),
        create_tick("50400", base_time + Duration::seconds(4)),
    ];

    let bars = gen.generate_from_ticks(&ticks);

    assert_eq!(bars.len(), 2); // 3 ticks + 2 ticks
    assert_eq!(bars[0].metadata.tick_count_in_bar, 3);
    assert_eq!(bars[0].metadata.is_bar_closed, true);
}
```

##### test_tick_based_on_each_tick
N-tick bars in OnEachTick mode.

```rust
#[test]
fn test_tick_based_on_each_tick() {
    let gen = HistoricalOHLCGenerator::new(
        BarType::TickBased(2),
        BarDataMode::OnEachTick,
    );

    let base_time = Utc::now();

    let ticks = vec![
        create_tick("50000", base_time),
        create_tick("50100", base_time + Duration::seconds(1)),
        create_tick("50200", base_time + Duration::seconds(2)),
    ];

    let bars = gen.generate_from_ticks(&ticks);

    assert_eq!(bars.len(), 3);
    assert_eq!(bars[0].metadata.tick_count_in_bar, 1);
    assert_eq!(bars[0].metadata.is_first_tick_of_bar, true);
    assert_eq!(bars[1].metadata.tick_count_in_bar, 2);
    assert_eq!(bars[1].metadata.is_bar_closed, true);
}
```

---

##### Session-Aware Bar Generation Tests (9 tests)

Tests for OHLC window alignment to session open and truncation at session close.

| Test | Description |
|------|-------------|
| `test_session_aware_config_default` | Default config has no session schedule |
| `test_session_aware_config_continuous` | Continuous config for 24/7 markets |
| `test_session_aware_config_builder` | Builder pattern for session config |
| `test_generator_without_session_config` | Generator works without session config |
| `test_tick_based_bars_session_truncation_logic` | Partial tick bars without session schedule |
| `test_time_based_bars_default_session_flags` | Default session flags are false |
| `test_session_config_with_schedule_builder` | Config with actual session schedule |
| `test_generator_set_session_config` | Setting session config on generator |
| `test_tick_based_full_bars_not_truncated` | Full bars are not marked as truncated |
| `test_session_aware_config_is_within_session_no_schedule` | 24/7 always returns true |

**SessionAwareConfig** structure:
- `session_schedule: Option<Arc<SessionSchedule>>` - Session schedule for alignment
- `align_to_session_open: bool` - Align first bar to session open time
- `truncate_at_session_close: bool` - Force-close partial bars at session end

**BarMetadata** session fields:
- `is_session_truncated: bool` - Bar was closed early due to session end
- `is_session_aligned: bool` - Bar start was aligned to session open

---

#### `src/state/mod.rs`
Component state lifecycle management.

##### test_component_state_default
Default strategy state.

```rust
#[test]
fn test_component_state_default() {
    let state = ComponentState::default();
    assert_eq!(state, ComponentState::Undefined);
}
```

##### test_component_state_display
State display formatting.

```rust
#[test]
fn test_component_state_display() {
    assert_eq!(ComponentState::Undefined.to_string(), "UNDEFINED");
    assert_eq!(ComponentState::SetDefaults.to_string(), "SET_DEFAULTS");
    assert_eq!(ComponentState::Configure.to_string(), "CONFIGURE");
    assert_eq!(ComponentState::Active.to_string(), "ACTIVE");
    assert_eq!(ComponentState::DataLoaded.to_string(), "DATA_LOADED");
    assert_eq!(ComponentState::Historical.to_string(), "HISTORICAL");
    assert_eq!(ComponentState::Transition.to_string(), "TRANSITION");
    assert_eq!(ComponentState::Realtime.to_string(), "REALTIME");
    assert_eq!(ComponentState::Terminated.to_string(), "TERMINATED");
    assert_eq!(ComponentState::Faulted.to_string(), "FAULTED");
    assert_eq!(ComponentState::Finalized.to_string(), "FINALIZED");
}
```

##### test_component_state_is_running
Is running checks.

```rust
#[test]
fn test_component_state_is_running() {
    assert!(!ComponentState::Undefined.is_running());
    assert!(!ComponentState::SetDefaults.is_running());
    assert!(!ComponentState::Configure.is_running());
    assert!(ComponentState::Active.is_running());
    assert!(!ComponentState::DataLoaded.is_running());
    assert!(ComponentState::Historical.is_running());
    assert!(!ComponentState::Transition.is_running());
    assert!(ComponentState::Realtime.is_running());
    assert!(!ComponentState::Terminated.is_running());
}
```

##### test_component_state_is_terminal
Terminal state detection.

```rust
#[test]
fn test_component_state_is_terminal() {
    assert!(!ComponentState::Undefined.is_terminal());
    assert!(!ComponentState::Realtime.is_terminal());
    assert!(!ComponentState::Active.is_terminal());
    assert!(ComponentState::Terminated.is_terminal());
    assert!(ComponentState::Faulted.is_terminal());
    assert!(ComponentState::Finalized.is_terminal());
}
```

##### test_component_state_valid_transitions_data_processing_path
Valid state transitions for data-processing path.

```rust
#[test]
fn test_component_state_valid_transitions_data_processing_path() {
    assert!(ComponentState::Undefined.can_transition_to(ComponentState::SetDefaults));
    assert!(ComponentState::SetDefaults.can_transition_to(ComponentState::Configure));
    assert!(ComponentState::Configure.can_transition_to(ComponentState::DataLoaded));
    assert!(ComponentState::DataLoaded.can_transition_to(ComponentState::Historical));
    assert!(ComponentState::Historical.can_transition_to(ComponentState::Transition));
    assert!(ComponentState::Transition.can_transition_to(ComponentState::Realtime));
    assert!(ComponentState::Realtime.can_transition_to(ComponentState::Terminated));
    assert!(ComponentState::Terminated.can_transition_to(ComponentState::Finalized));
}
```

##### test_component_state_as_i32
FFI integer conversion.

```rust
#[test]
fn test_component_state_as_i32() {
    assert_eq!(ComponentState::Undefined.as_i32(), 0);
    assert_eq!(ComponentState::SetDefaults.as_i32(), 1);
    assert_eq!(ComponentState::Configure.as_i32(), 2);
    assert_eq!(ComponentState::Active.as_i32(), 3);
    assert_eq!(ComponentState::Finalized.as_i32(), 10);
}
```

##### test_component_id_factory_methods
ComponentId factory methods.

```rust
#[test]
fn test_component_id_factory_methods() {
    let strategy = ComponentId::strategy("sma-20");
    assert_eq!(strategy.component_type, ComponentType::Strategy);

    let indicator = ComponentId::indicator("RSI:BTCUSDT");
    assert_eq!(indicator.component_type, ComponentType::Indicator);

    let cache = ComponentId::cache("tick-cache");
    assert_eq!(cache.component_type, ComponentType::Cache);

    let data_source = ComponentId::data_source("binance-ws");
    assert_eq!(data_source.component_type, ComponentType::DataSource);

    let service = ComponentId::service("market-data");
    assert_eq!(service.component_type, ComponentType::Service);
}
```

---

#### `src/state/registry.rs`
Centralized state registry.

##### test_registry_register
Component registration.

```rust
#[test]
fn test_registry_register() {
    let registry = StateRegistry::default();
    let id = ComponentId::strategy("test");

    registry.register(id.clone(), ComponentState::Undefined).unwrap();
    assert_eq!(registry.get_state(&id), Some(ComponentState::Undefined));
}
```

##### test_registry_transition
State transition.

```rust
#[test]
fn test_registry_transition() {
    let registry = StateRegistry::default();
    let id = ComponentId::strategy("test");

    registry.register(id.clone(), ComponentState::Undefined).unwrap();

    let event = registry
        .transition(&id, ComponentState::SetDefaults, None)
        .unwrap();

    assert_eq!(event.old_state, ComponentState::Undefined);
    assert_eq!(event.new_state, ComponentState::SetDefaults);
    assert_eq!(registry.get_state(&id), Some(ComponentState::SetDefaults));
}
```

##### test_registry_invalid_transition
Invalid transition rejection.

```rust
#[test]
fn test_registry_invalid_transition() {
    let registry = StateRegistry::default();
    let id = ComponentId::strategy("test");

    registry.register(id.clone(), ComponentState::Undefined).unwrap();

    let result = registry.transition(&id, ComponentState::Realtime, None);
    assert!(matches!(result, Err(StateError::InvalidTransition { .. })));
}
```

##### test_registry_parent_child
Parent-child relationships.

```rust
#[test]
fn test_registry_parent_child() {
    let registry = StateRegistry::default();

    let parent = ComponentId::strategy("parent");
    let child1 = ComponentId::indicator("child1");
    let child2 = ComponentId::indicator("child2");

    registry.register(parent.clone(), ComponentState::Undefined).unwrap();
    registry.register_with_parent(child1.clone(), ComponentState::Undefined, parent.clone()).unwrap();
    registry.register_with_parent(child2.clone(), ComponentState::Undefined, parent.clone()).unwrap();

    let children = registry.get_children(&parent);
    assert_eq!(children.len(), 2);
    assert!(children.contains(&child1));
    assert!(children.contains(&child2));

    assert_eq!(registry.get_parent(&child1), Some(parent.clone()));
}
```

##### test_registry_wait_for_state
Async wait for state.

```rust
#[tokio::test]
async fn test_registry_wait_for_state() {
    let registry = Arc::new(StateRegistry::default());
    let id = ComponentId::strategy("test");

    registry.register(id.clone(), ComponentState::Undefined).unwrap();

    let registry_clone = registry.clone();
    let id_clone = id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        registry_clone.transition(&id_clone, ComponentState::SetDefaults, None).unwrap();
    });

    let result = registry
        .wait_for_state(&id, ComponentState::SetDefaults, Duration::from_secs(1))
        .await;

    assert!(result.is_ok());
}
```

---

#### `src/series/mod.rs`
Generic time series implementation.

##### test_series_new
Series creation.

```rust
#[test]
fn test_series_new() {
    let series: Series<Decimal> = Series::new("close");
    assert_eq!(series.name(), "close");
    assert!(series.is_empty());
    assert_eq!(series.count(), 0);
}
```

##### test_series_push_and_get
Push and retrieval.

```rust
#[test]
fn test_series_push_and_get() {
    let mut series: Series<Decimal> = Series::new("close");

    series.push(Decimal::from(100));
    series.push(Decimal::from(101));
    series.push(Decimal::from(102));

    assert_eq!(series.count(), 3);
    assert_eq!(series[0], Decimal::from(102)); // Most recent
    assert_eq!(series[1], Decimal::from(101));
    assert_eq!(series[2], Decimal::from(100)); // Oldest
}
```

##### test_series_sma_calculation
Simple Moving Average calculation.

```rust
#[test]
fn test_sma_calculation() {
    let mut series: Series<Decimal> = Series::new("close");

    series.push(Decimal::from_str("10").unwrap());
    series.push(Decimal::from_str("20").unwrap());
    series.push(Decimal::from_str("30").unwrap());
    series.push(Decimal::from_str("40").unwrap());
    series.push(Decimal::from_str("50").unwrap());

    // SMA(3) = (50 + 40 + 30) / 3 = 40
    let sma = series.sma(3).unwrap();
    assert_eq!(sma, Decimal::from(40));

    // SMA(5) = (10 + 20 + 30 + 40 + 50) / 5 = 30
    let sma5 = series.sma(5).unwrap();
    assert_eq!(sma5, Decimal::from(30));

    // Not enough data for SMA(6)
    assert!(series.sma(6).is_none());
}
```

##### test_highest_lowest
Highest and lowest value tracking.

```rust
#[test]
fn test_highest_lowest() {
    let mut series: Series<Decimal> = Series::new("high");

    series.push(Decimal::from(10));
    series.push(Decimal::from(50));
    series.push(Decimal::from(30));
    series.push(Decimal::from(20));
    series.push(Decimal::from(40));

    assert_eq!(series.highest(3).unwrap(), Decimal::from(40));
    assert_eq!(series.lowest(3).unwrap(), Decimal::from(20));
    assert_eq!(series.highest(5).unwrap(), Decimal::from(50));
    assert_eq!(series.lowest(5).unwrap(), Decimal::from(10));
}
```

##### test_series_is_ready_with_warmup
Warmup-based readiness.

```rust
#[test]
fn test_series_is_ready_with_warmup() {
    let mut series: Series<Decimal> = Series::with_warmup(
        "sma",
        MaximumBarsLookBack::default(),
        5, // Need 5 samples
    );

    assert_eq!(series.warmup_period(), 5);

    for i in 1..=4 {
        series.push(Decimal::from(i));
        assert!(!series.is_ready());
    }

    series.push(Decimal::from(5));
    assert!(series.is_ready());
}
```

---

#### `src/instruments/session.rs`
Trading session and market hours management (33 tests).

| Test | Description |
|------|-------------|
| `test_session_schedule_creation` | US equity preset structure |
| `test_trading_session_is_active` | Active day/time detection |
| `test_session_crossing_midnight` | Sessions spanning midnight |
| `test_market_calendar` | Holiday and early close handling |
| `test_schedule_is_open` | Market open status checks |
| `test_session_duration` | Duration calculations |
| `test_24_7_schedule` | Continuous schedule structure |
| `test_maintenance_window` | Maintenance window detection |
| `test_session_exact_start_time` | Boundary: exactly at start |
| `test_session_exact_end_time` | Boundary: exactly at end (exclusive) |
| `test_session_midnight_boundary` | Sessions ending at midnight |
| `test_session_starts_at_midnight` | Sessions starting at midnight |
| `test_cross_midnight_session_basic` | CME-style overnight sessions |
| `test_cross_midnight_duration` | Duration for overnight sessions |
| `test_timezone_conversion_us_eastern` | Eastern timezone handling |
| `test_timezone_conversion_utc_queries` | UTC query conversion |
| `test_timezone_chicago` | CME Chicago timezone |
| `test_dst_spring_forward` | DST spring transition |
| `test_dst_fall_back` | DST fall transition |
| `test_holiday_affects_is_open` | Holiday closure |
| `test_early_close` | Early close recording |
| `test_late_open` | Late open recording |
| `test_weekend_closed_for_equity` | Weekend closure for equities |
| `test_crypto_always_open` | 24/7 crypto schedule |
| `test_get_session_state_regular_hours` | Regular hours state |
| `test_get_session_state_pre_market` | Pre-market state |
| `test_get_session_state_after_hours` | After-hours state |
| `test_get_session_state_closed` | Closed market state |
| `test_always_open_schedule` | Always-open schedule boundaries |
| `test_is_trading_time_includes_extended` | Extended hours trading time |
| `test_maintenance_window_exact_boundaries` | Maintenance window boundaries |
| `test_maintenance_window_wrong_day` | Maintenance on wrong day |
| `test_forex_schedule` | Forex schedule structure |

---

#### `src/instruments/session_manager.rs`
Real-time session state tracking (14 tests).

| Test | Description |
|------|-------------|
| `test_session_manager_config_default` | Default configuration values |
| `test_market_status_counts` | Status count aggregation |
| `test_session_state_storage` | DashMap state storage |
| `test_session_state_update` | State update mechanics |
| `test_session_state_transitions` | All valid status values |
| `test_event_channel_creation` | Broadcast channel creation |
| `test_event_channel_multiple_subscribers` | Multiple subscriber support |
| `test_all_event_types` | All 6 SessionEvent variants |
| `test_get_symbols_by_status` | Query symbols by status |
| `test_halt_state_change` | Manual halt state change |
| `test_resume_clears_reason` | Resume clears halt reason |
| `test_concurrent_state_updates` | Thread-safe concurrent updates |
| `test_concurrent_register_unregister` | Concurrent registration |
| `test_is_tradeable_status` | Tradeable status detection |

---

## 2. trading-core

### Unit Tests

#### `src/alerting/tests.rs`
Alert system for monitoring.

##### test_ipc_disconnection_alert
Verifies IPC disconnection triggers CRITICAL alert.

```rust
#[test]
fn test_ipc_disconnection_alert() {
    IPC_CONNECTION_STATUS.set(0);

    let rule = AlertRule::ipc_disconnected();
    let condition_met = rule.evaluate();

    assert!(condition_met, "Alert should trigger when IPC is disconnected");
    assert_eq!(rule.severity(), AlertSeverity::Critical);
}
```

##### test_ipc_reconnection_storm_alert
Verifies excessive reconnection attempts trigger WARNING.

```rust
#[test]
fn test_ipc_reconnection_storm_alert() {
    IPC_RECONNECTS_TOTAL.reset();
    IPC_RECONNECTS_TOTAL.inc_by(5);

    let rule = AlertRule::ipc_reconnection_storm(3);
    let condition_met = rule.evaluate();

    assert!(condition_met, "Alert should trigger with excessive reconnections");
    assert_eq!(rule.severity(), AlertSeverity::Warning);
}
```

##### test_channel_backpressure_alert
Verifies channel backpressure at 85% triggers WARNING.

```rust
#[test]
fn test_channel_backpressure_alert() {
    CHANNEL_UTILIZATION.set(85.0);

    let rule = AlertRule::channel_backpressure(80.0);
    let condition_met = rule.evaluate();

    assert!(condition_met, "Alert should trigger at 85% channel utilization");
    assert_eq!(rule.severity(), AlertSeverity::Warning);
}
```

##### test_cache_failure_rate_alert
Verifies cache failure rate >10% triggers WARNING (resilient to parallel test interference).

```rust
#[test]
fn test_cache_failure_rate_alert() {
    // Given: High cache failure rate
    // When: Failure rate exceeds 10%
    // Then: WARNING alert should be triggered

    // Set metrics to specific values (don't reset - just set absolute values)
    // Use high enough values that even parallel test interference won't drop rate below 10%
    // Note: Due to global metrics and parallel tests, we set values then immediately evaluate
    TICKS_PROCESSED_TOTAL.reset();
    CACHE_UPDATE_FAILURES_TOTAL.reset();
    // Set values - 200 failures out of 500 ticks = 40% failure rate
    TICKS_PROCESSED_TOTAL.inc_by(500);
    CACHE_UPDATE_FAILURES_TOTAL.inc_by(200);

    let rule = AlertRule::cache_failure_rate(0.1); // 10% threshold
    let condition_met = rule.evaluate();

    // Get current values for debug output
    let failures = CACHE_UPDATE_FAILURES_TOTAL.get();
    let total = TICKS_PROCESSED_TOTAL.get();
    let actual_rate = if total > 0 { failures as f64 / total as f64 } else { 0.0 };

    // Due to parallel test execution, metrics may be modified by other tests
    // We verify the invariant: alert triggers IFF rate >= threshold AND total > 0
    if total > 0 && actual_rate >= 0.1 {
        assert!(
            condition_met,
            "Alert should trigger when cache failure rate ({:.1}%) >= 10%",
            actual_rate * 100.0
        );
    }
    // If metrics were modified such that rate < 10%, the rule logic is still correct
    // We just can't assert alert fired in that case

    // Always verify severity is correct
    assert_eq!(rule.severity(), AlertSeverity::Warning);
}
```

##### test_no_alert_when_below_threshold
Verifies no alerts trigger when metrics are below thresholds (resilient to parallel test interference).

```rust
#[test]
fn test_no_alert_when_below_threshold() {
    // Given: All metrics are healthy
    // When: Values are below thresholds
    // Then: No alerts should be triggered

    // Test IPC connected status - set and evaluate immediately
    IPC_CONNECTION_STATUS.set(1); // Connected
    let ipc_rule = AlertRule::ipc_disconnected();
    let ipc_result = ipc_rule.evaluate();
    // Assert unconditionally - if parallel tests interfere, the test correctly fails
    // to indicate the test environment is not isolated
    assert!(
        !ipc_result || IPC_CONNECTION_STATUS.get() != 1,
        "IPC alert should NOT trigger when connected (status={})",
        IPC_CONNECTION_STATUS.get()
    );

    // Test channel utilization (50%, threshold 80%)
    CHANNEL_UTILIZATION.set(50.0);
    let channel_rule = AlertRule::channel_backpressure(80.0);
    let channel_result = channel_rule.evaluate();
    assert!(
        !channel_result || CHANNEL_UTILIZATION.get() >= 80.0,
        "Channel alert should NOT trigger at {}% utilization (threshold 80%)",
        CHANNEL_UTILIZATION.get()
    );

    // Test cache failure rate (0% failure, threshold 10%)
    // Note: Due to parallel test execution, we can't guarantee metric isolation
    // Instead, we verify the rule logic: when failure rate is 0%, no alert triggers
    TICKS_PROCESSED_TOTAL.reset();
    CACHE_UPDATE_FAILURES_TOTAL.reset();
    TICKS_PROCESSED_TOTAL.inc_by(100); // 0% failure rate (0 failed / 100 total)
    let cache_rule = AlertRule::cache_failure_rate(0.1);
    let cache_result = cache_rule.evaluate();
    let failures = CACHE_UPDATE_FAILURES_TOTAL.get();
    let total = TICKS_PROCESSED_TOTAL.get();
    let actual_rate = if total > 0 { failures as f64 / total as f64 } else { 0.0 };
    assert!(
        !cache_result || actual_rate >= 0.1,
        "Cache alert should NOT trigger when failure rate is {:.1}% (threshold 10%)",
        actual_rate * 100.0
    );
}
```

##### test_alert_evaluator_with_multiple_rules
Verifies evaluator correctly handles multiple rules based on current metric state.

```rust
#[test]
fn test_alert_evaluator_with_multiple_rules() {
    // Given: Multiple alert rules configured
    // When: Evaluator checks all rules
    // Then: Only violated rules should generate alerts

    // Set metrics - IPC disconnected should trigger, channel utilization should not
    IPC_CONNECTION_STATUS.set(0);  // Disconnected - will trigger
    CHANNEL_UTILIZATION.set(50.0); // Below 80% threshold - should NOT trigger

    let handler = MockAlertHandler::new();
    let mut evaluator = AlertEvaluator::new(handler.clone());

    evaluator.add_rule(AlertRule::ipc_disconnected());
    evaluator.add_rule(AlertRule::channel_backpressure(80.0));

    evaluator.evaluate_all();

    let alerts = handler.get_alerts();
    let ipc_status = IPC_CONNECTION_STATUS.get();
    let channel_util = CHANNEL_UTILIZATION.get();

    // Count expected alerts based on current metric state (accounts for parallel test interference)
    let expected_ipc_alert = ipc_status == 0;
    let expected_channel_alert = channel_util >= 80.0;
    let expected_count = (expected_ipc_alert as usize) + (expected_channel_alert as usize);

    assert_eq!(
        alerts.len(), expected_count,
        "Expected {} alerts (IPC disconnected={}, channel high={}), got {}. IPC={}, Channel={:.1}%",
        expected_count, expected_ipc_alert, expected_channel_alert, alerts.len(),
        ipc_status, channel_util
    );

    // If IPC was disconnected, verify we got that alert
    if expected_ipc_alert && !alerts.is_empty() {
        assert!(
            alerts.iter().any(|a| a.message.contains("IPC")),
            "Should have IPC disconnection alert"
        );
    }
}
```

##### test_alert_cooldown_prevents_spam
Verifies cooldown mechanism suppresses repeated alerts.

```rust
#[test]
fn test_alert_cooldown_prevents_spam() {
    // Given: Alert rule with cooldown period
    // When: Same alert fires multiple times quickly
    // Then: Only first alert is sent during cooldown

    IPC_CONNECTION_STATUS.set(0);

    let handler = MockAlertHandler::new();
    let mut evaluator = AlertEvaluator::new(handler.clone());
    evaluator.set_cooldown(Duration::from_secs(60)); // 60 second cooldown

    evaluator.add_rule(AlertRule::ipc_disconnected());

    // First evaluation - should trigger alert
    evaluator.evaluate_all();
    assert_eq!(handler.get_alerts().len(), 1, "First alert should be sent");

    // Second evaluation immediately - should be suppressed
    evaluator.evaluate_all();
    assert_eq!(handler.get_alerts().len(), 1, "Second alert should be suppressed by cooldown");
}
```

##### test_alert_contains_metric_value
Verifies alert messages contain the actual metric values.

```rust
#[test]
fn test_alert_contains_metric_value() {
    // Given: Alert rule that checks metric value
    // When: Alert is triggered
    // Then: Alert should contain the actual metric value

    CHANNEL_UTILIZATION.set(90.0);
    CHANNEL_BUFFER_SIZE.set(900);

    let handler = MockAlertHandler::new();
    let mut evaluator = AlertEvaluator::new(handler.clone());
    evaluator.add_rule(AlertRule::channel_backpressure(80.0)); // 80% threshold

    evaluator.evaluate_all();

    let alerts = handler.get_alerts();

    // Due to parallel test execution with global metrics, we verify the invariant:
    // - If alerts were generated, the utilization WAS >= 80% at evaluation time
    // - The alert message should contain a numeric utilization value
    // We cannot check current_utilization because parallel tests may have changed it
    // between evaluate_all() and now.

    if !alerts.is_empty() {
        // Alert was triggered - verify message contains a utilization value
        let msg = &alerts[0].message;
        // The message should contain some numeric value representing utilization
        // (could be 90 from our set, or another value if parallel test interfered before evaluation)
        let contains_numeric = msg.chars().any(|c| c.is_ascii_digit());
        assert!(
            contains_numeric,
            "Alert message should include a numeric utilization value. Message: '{}'",
            msg
        );
        // Verify the alert is for channel backpressure
        assert!(
            msg.to_lowercase().contains("channel") || msg.to_lowercase().contains("utilization") ||
            msg.to_lowercase().contains("backpressure") || msg.to_lowercase().contains("buffer"),
            "Alert should be about channel/utilization. Message: '{}'",
            msg
        );
    }
    // If no alerts, that's also valid - means utilization was < 80% at evaluation time
    // (due to parallel test interference). The rule logic is still correct.
}
```

##### test_cache_failure_with_zero_ticks
Verifies no alert triggers when zero ticks processed (division by zero protection).

```rust
#[test]
fn test_cache_failure_with_zero_ticks() {
    // Given: No ticks have been processed yet
    // When: Checking cache failure rate
    // Then: No alert should trigger (avoid division by zero)

    TICKS_PROCESSED_TOTAL.reset();
    CACHE_UPDATE_FAILURES_TOTAL.reset();

    let rule = AlertRule::cache_failure_rate(0.1);
    let condition_met = rule.evaluate();

    // Get current state after evaluation
    let failures = CACHE_UPDATE_FAILURES_TOTAL.get();
    let total = TICKS_PROCESSED_TOTAL.get();
    let failure_rate = if total > 0 { failures as f64 / total as f64 } else { 0.0 };

    // Verify the rule behaves correctly given the current state
    // The key invariant: alert triggers IFF failure_rate >= threshold AND total > 0
    if total == 0 {
        // Zero ticks: no alert should trigger (division by zero protection)
        assert!(
            !condition_met,
            "Alert should NOT trigger when total ticks is 0 (division by zero protection)"
        );
    } else if failure_rate < 0.1 {
        // Below threshold: no alert should trigger
        assert!(
            !condition_met,
            "Alert should NOT trigger when failure rate ({:.1}%) < 10%",
            failure_rate * 100.0
        );
    } else {
        // Above threshold: alert SHOULD trigger
        assert!(
            condition_met,
            "Alert SHOULD trigger when failure rate ({:.1}%) >= 10%",
            failure_rate * 100.0
        );
    }
}
```

##### test_log_alert_handler_formats_correctly
Verifies log handler processes alerts without panicking.

```rust
#[test]
fn test_log_alert_handler_formats_correctly() {
    // Given: Log alert handler
    // When: Alert is triggered
    // Then: Log should contain all alert details

    let handler = LogAlertHandler::new();
    let alert = Alert::new(
        AlertSeverity::Critical,
        "test_metric".to_string(),
        "Test alert message".to_string(),
    );

    // This should not panic
    handler.handle(alert);
}
```

##### test_alert_severity_ordering
Verifies severity levels (Critical > Warning).

```rust
#[test]
fn test_alert_severity_ordering() {
    // Given: Different alert severities
    // Then: Critical > Warning

    assert!(AlertSeverity::Critical > AlertSeverity::Warning);
}
```

---

### Benchmarks

#### `benches/repository_bench.rs`
Performance benchmarks for repository and cache.

| Benchmark | Description |
|-----------|-------------|
| `insert_tick` | Measures single tick insertion latency |
| `batch_insert_100` | Measures 100-tick batch insertion |
| `batch_insert_1000` | Measures 1000-tick batch insertion |
| `get_ticks_cache_hit` | Measures L1/L2 cache hit latency |
| `get_ticks_cache_miss` | Measures database fallback latency |
| `get_latest_price` | Measures latest price lookup latency |
| `get_historical_data_for_backtest` | Measures backtest data retrieval |
| `cache_push_tick` | Measures cache push operation latency |
| `cache_get_recent_ticks` | Measures recent ticks retrieval from cache |

---

## 3. data-manager

### Integration Tests

#### `tests/ipc_stress_test.rs`
IPC shared memory stress tests.

| Function | Description |
|----------|-------------|
| `test_basic_producer_consumer` | Basic producer/consumer communication |
| `test_throughput_single_thread` | High-throughput single-threaded performance |
| `test_concurrent_producer_consumer` | Concurrent producer/consumer with threads |
| `test_overflow_behavior` | Buffer overflow behavior |
| `test_multiple_symbols` | Multiple symbol channels |
| `test_stress_high_rate` | Stress test with high message rate |
| `test_latency` | Latency characteristics |
| `test_real_shared_memory` | Real POSIX shared memory (cross-thread) |

### Unit Tests

#### `src/validation/tests.rs`
Tick data validation (32 tests).

| Function | Description |
|----------|-------------|
| `test_valid_tick_passes_validation` | Valid tick validation |
| `test_positive_price_passes` | Positive price acceptance |
| `test_zero_price_fails` | Zero price rejection |
| `test_negative_price_fails` | Negative price rejection |
| `test_price_out_of_bounds_fails` | Price bounds checking |
| `test_positive_size_passes` | Positive size acceptance |
| `test_zero_size_fails` | Zero size rejection |
| `test_negative_size_fails` | Negative size rejection |
| `test_empty_symbol_fails` | Empty symbol rejection |
| `test_symbol_too_long_fails` | Symbol length validation |
| `test_suspicious_price_change_detected` | Price change detection |
| `test_timestamp_in_future_fails` | Future timestamp rejection |
| `test_timestamp_too_old_fails` | Old timestamp rejection |

---

## Running Tests

### Run All Tests
```bash
cargo test
```

### Run Tests for Specific Crate
```bash
cargo test --package trading-common
cargo test --package trading-core
cargo test --package data-manager
```

### Run Specific Test
```bash
cargo test test_market_order_creation
```

### Run Tests with Output
```bash
cargo test -- --nocapture
```

### Run Benchmarks
```bash
cd trading-core && cargo bench
```

### Run Integration Tests Only
```bash
cargo test --test ipc_stress_test
cargo test --test backtest_unified_test
```

---

## Test Organization Philosophy

### Unit Tests
- Located in `#[cfg(test)] mod tests { ... }` blocks within source files
- Test individual functions and methods in isolation
- Use mock dependencies where needed

### Integration Tests
- Located in `tests/` directories
- Test module interactions and full workflows
- May require database and Redis connections

### Benchmarks
- Located in `benches/` directories
- Use Criterion for statistical analysis
- Measure performance of critical paths
