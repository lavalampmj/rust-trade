use trading_common::backtest::strategy;
use std::env;

fn get_config_path() -> String {
    // CARGO_MANIFEST_DIR is the directory containing trading-common's Cargo.toml
    // We need to go up one level to get to the project root
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    format!("{}/../config/development.toml", manifest_dir)
}

#[test]
#[ignore] // This test can't run with others because initialize_python_strategies can only be called once
fn test_python_strategy_initialization() {
    // Initialize Python strategies from config
    let config_path = get_config_path();
    let result = strategy::initialize_python_strategies(&config_path);
    assert!(result.is_ok(), "Python strategy initialization failed: {:?}", result);
}

#[test]
fn test_list_strategies() {
    // Initialize first
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    // List all strategies
    let strategies = strategy::list_strategies();

    // Should have at least 3 strategies: sma (Rust), rsi (Rust), sma_python (Python)
    assert!(strategies.len() >= 3, "Expected at least 3 strategies, got {}", strategies.len());

    // Check that we have both Rust and Python strategies
    let rust_count = strategies.iter().filter(|s| matches!(s.strategy_type, strategy::StrategyType::Rust)).count();
    let python_count = strategies.iter().filter(|s| matches!(s.strategy_type, strategy::StrategyType::Python)).count();

    assert!(rust_count >= 2, "Expected at least 2 Rust strategies, got {}", rust_count);
    assert!(python_count >= 1, "Expected at least 1 Python strategy, got {}", python_count);

    // Verify specific strategies exist
    assert!(strategies.iter().any(|s| s.id == "sma"), "SMA Rust strategy not found");
    assert!(strategies.iter().any(|s| s.id == "rsi"), "RSI Rust strategy not found");
    assert!(strategies.iter().any(|s| s.id == "sma_python"), "SMA Python strategy not found");
}

#[test]
fn test_create_rust_strategy() {
    let result = strategy::create_strategy("sma");
    assert!(result.is_ok(), "Failed to create Rust SMA strategy");

    if let Ok(s) = result {
        assert_eq!(s.name(), "Simple Moving Average");
    }
}

#[test]
fn test_create_python_strategy() {
    // Initialize first
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    let result = strategy::create_strategy("sma_python");
    assert!(result.is_ok(), "Failed to create Python SMA strategy");

    if let Ok(s) = result {
        assert_eq!(s.name(), "Simple Moving Average (Python)");
    }
}

#[test]
fn test_get_strategy_info() {
    // Initialize first
    let config_path = get_config_path();
    let _ = strategy::initialize_python_strategies(&config_path);

    // Test Rust strategy
    let info = strategy::get_strategy_info("sma");
    assert!(info.is_some(), "Failed to get info for SMA strategy");

    if let Some(info) = info {
        assert_eq!(info.id, "sma");
        assert!(matches!(info.strategy_type, strategy::StrategyType::Rust));
    }

    // Test Python strategy
    let info = strategy::get_strategy_info("sma_python");
    assert!(info.is_some(), "Failed to get info for Python SMA strategy");

    if let Some(info) = info {
        assert_eq!(info.id, "sma_python");
        assert!(matches!(info.strategy_type, strategy::StrategyType::Python));
    }
}
