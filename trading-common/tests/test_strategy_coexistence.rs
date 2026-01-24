use chrono::Utc;
use rust_decimal::Decimal;
use std::env;
use std::str::FromStr;
use trading_common::backtest::engine::{BacktestConfig, BacktestData, BacktestEngine};
/// Test that Rust and Python strategies can coexist and both work correctly
use trading_common::backtest::strategy;
use trading_common::data::types::{TickData, TradeSide};

fn get_config_path() -> String {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    format!("{}/../config/development.toml", manifest_dir)
}

fn create_sample_tick(symbol: &str, price: &str, index: i64) -> TickData {
    let ts = Utc::now();
    TickData::with_details(
        ts,
        ts,
        symbol.to_string(),
        "TEST".to_string(),
        Decimal::from_str(price).unwrap(),
        Decimal::from_str("1.0").unwrap(),
        TradeSide::Buy,
        "TEST".to_string(),
        index.to_string(),
        false,
        index,
    )
}

#[test]
fn test_both_rust_and_python_strategies_work() {
    // Initialize Python strategies
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    // Verify both Rust and Python strategies are available
    let strategies = strategy::list_strategies();
    assert!(
        strategies.len() >= 3,
        "Should have at least 3 strategies (sma, rsi, sma_python)"
    );

    let rust_strategies: Vec<_> = strategies
        .iter()
        .filter(|s| matches!(s.strategy_type, strategy::StrategyType::Rust))
        .collect();
    let python_strategies: Vec<_> = strategies
        .iter()
        .filter(|s| matches!(s.strategy_type, strategy::StrategyType::Python))
        .collect();

    assert!(
        rust_strategies.len() >= 2,
        "Should have at least 2 Rust strategies"
    );
    assert!(
        python_strategies.len() >= 1,
        "Should have at least 1 Python strategy"
    );

    println!(
        "Found {} Rust strategies and {} Python strategies",
        rust_strategies.len(),
        python_strategies.len()
    );
}

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
    let strategy_box =
        strategy::create_strategy("sma").expect("Failed to create Rust SMA strategy");
    assert_eq!(
        strategy_box.name(),
        "Simple Moving Average",
        "Strategy name mismatch"
    );

    // Run backtest
    let config = BacktestConfig::new(initial_capital);
    let mut engine =
        BacktestEngine::new(strategy_box, config).expect("Failed to create backtest engine");
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
    // With only 5 ticks, SMA strategy likely won't have enough data for crossover
    // so we just verify the backtest ran without crashing
    // The key assertion is that the strategy was created and executed
}

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
    let strategy_box =
        strategy::create_strategy("sma_python").expect("Failed to create Python SMA strategy");
    assert_eq!(
        strategy_box.name(),
        "Simple Moving Average (Python)",
        "Strategy name mismatch"
    );

    // Run backtest
    let config = BacktestConfig::new(initial_capital);
    let mut engine =
        BacktestEngine::new(strategy_box, config).expect("Failed to create backtest engine");
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

#[test]
fn test_strategy_type_distinction() {
    // Initialize Python strategies
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    // Get info for Rust strategy
    let rust_info = strategy::get_strategy_info("sma").expect("SMA strategy should exist");
    assert!(matches!(
        rust_info.strategy_type,
        strategy::StrategyType::Rust
    ));
    println!("Rust SMA: {} - {}", rust_info.name, rust_info.description);

    // Get info for Python strategy
    let python_info =
        strategy::get_strategy_info("sma_python").expect("Python SMA strategy should exist");
    assert!(matches!(
        python_info.strategy_type,
        strategy::StrategyType::Python
    ));
    println!(
        "Python SMA: {} - {}",
        python_info.name, python_info.description
    );

    // Verify they have different types
    // Rust type should not equal Python type
    let rust_is_rust = matches!(rust_info.strategy_type, strategy::StrategyType::Rust);
    let python_is_python = matches!(python_info.strategy_type, strategy::StrategyType::Python);
    assert!(
        rust_is_rust && python_is_python,
        "Strategies should have correct types"
    );
}
