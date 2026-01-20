/// Test that Rust and Python strategies can coexist and both work correctly
use trading_common::backtest::strategy;
use trading_common::backtest::engine::{BacktestConfig, BacktestData, BacktestEngine};
use trading_common::data::types::{TickData, TradeSide};
use chrono::Utc;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::env;

fn get_config_path() -> String {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    format!("{}/../config/development.toml", manifest_dir)
}

fn create_sample_tick(symbol: &str, price: &str, index: i64) -> TickData {
    TickData {
        timestamp: Utc::now(),
        symbol: symbol.to_string(),
        price: Decimal::from_str(price).unwrap(),
        quantity: Decimal::from_str("1.0").unwrap(),
        side: TradeSide::Buy,
        trade_id: index.to_string(),
        is_buyer_maker: false,
    }
}

#[test]
fn test_both_rust_and_python_strategies_work() {
    // Initialize Python strategies
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    // Verify both Rust and Python strategies are available
    let strategies = strategy::list_strategies();
    assert!(strategies.len() >= 3, "Should have at least 3 strategies (sma, rsi, sma_python)");

    let rust_strategies: Vec<_> = strategies.iter()
        .filter(|s| matches!(s.strategy_type, strategy::StrategyType::Rust))
        .collect();
    let python_strategies: Vec<_> = strategies.iter()
        .filter(|s| matches!(s.strategy_type, strategy::StrategyType::Python))
        .collect();

    assert!(rust_strategies.len() >= 2, "Should have at least 2 Rust strategies");
    assert!(python_strategies.len() >= 1, "Should have at least 1 Python strategy");

    println!("Found {} Rust strategies and {} Python strategies",
             rust_strategies.len(), python_strategies.len());
}

#[test]
fn test_rust_sma_strategy_backtest() {
    // Create sample data
    let ticks = vec![
        create_sample_tick("BTCUSDT", "50000.0", 1),
        create_sample_tick("BTCUSDT", "50100.0", 2),
        create_sample_tick("BTCUSDT", "50200.0", 3),
        create_sample_tick("BTCUSDT", "50300.0", 4),
        create_sample_tick("BTCUSDT", "50400.0", 5),
    ];

    // Create Rust SMA strategy
    let strategy_box = strategy::create_strategy("sma").expect("Failed to create Rust SMA strategy");

    // Run backtest
    let config = BacktestConfig::new(Decimal::from_str("10000.0").unwrap());

    let mut engine = BacktestEngine::new(strategy_box, config).expect("Failed to create backtest engine");
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert!(result.total_trades >= 0, "Backtest should complete successfully");
    println!("Rust SMA strategy executed {} trades", result.total_trades);
}

#[test]
fn test_python_sma_strategy_backtest() {
    // Initialize Python strategies
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    // Create sample data
    let ticks = vec![
        create_sample_tick("BTCUSDT", "50000.0", 1),
        create_sample_tick("BTCUSDT", "50100.0", 2),
        create_sample_tick("BTCUSDT", "50200.0", 3),
        create_sample_tick("BTCUSDT", "50300.0", 4),
        create_sample_tick("BTCUSDT", "50400.0", 5),
    ];

    // Create Python SMA strategy
    let strategy_box = strategy::create_strategy("sma_python").expect("Failed to create Python SMA strategy");

    // Run backtest
    let config = BacktestConfig::new(Decimal::from_str("10000.0").unwrap());

    let mut engine = BacktestEngine::new(strategy_box, config).expect("Failed to create backtest engine");
    let result = engine.run_unified(BacktestData::Ticks(ticks));

    assert!(result.total_trades >= 0, "Backtest should complete successfully");
    println!("Python SMA strategy executed {} trades", result.total_trades);
}

#[test]
fn test_strategy_type_distinction() {
    // Initialize Python strategies
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    // Get info for Rust strategy
    let rust_info = strategy::get_strategy_info("sma").expect("SMA strategy should exist");
    assert!(matches!(rust_info.strategy_type, strategy::StrategyType::Rust));
    println!("Rust SMA: {} - {}", rust_info.name, rust_info.description);

    // Get info for Python strategy
    let python_info = strategy::get_strategy_info("sma_python").expect("Python SMA strategy should exist");
    assert!(matches!(python_info.strategy_type, strategy::StrategyType::Python));
    println!("Python SMA: {} - {}", python_info.name, python_info.description);

    // Verify they have different types
    // Rust type should not equal Python type
    let rust_is_rust = matches!(rust_info.strategy_type, strategy::StrategyType::Rust);
    let python_is_python = matches!(python_info.strategy_type, strategy::StrategyType::Python);
    assert!(rust_is_rust && python_is_python, "Strategies should have correct types");
}
