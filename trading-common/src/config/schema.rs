//! Configuration schema types for the trading system.
//!
//! This module defines all configuration types that can be serialized to/from
//! both TOML (for defaults) and JSON (for user overrides).
//!
//! # Architecture
//!
//! Configuration follows a hybrid approach:
//! - TOML files in `config/` directory define defaults (git-tracked)
//! - JSON file in user config directory stores overrides (user-specific)
//! - Runtime config is the merge of defaults + overrides

use serde::{Deserialize, Serialize};

/// Root application configuration.
///
/// This represents the complete configuration for the trading application.
/// It can be loaded from TOML defaults and JSON user overrides, then merged.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    /// Trading symbols to monitor
    pub symbols: Vec<String>,

    /// Account configuration
    pub accounts: AccountsConfigSchema,

    /// Paper trading settings
    pub paper_trading: PaperTradingConfig,

    /// Built-in strategy parameters
    pub builtin_strategies: BuiltinStrategiesConfig,

    /// Python strategy configuration
    pub strategies: StrategiesConfig,

    /// Execution simulation settings (fees, latency)
    pub execution: ExecutionConfig,

    /// Database settings
    pub database: DatabaseConfig,

    /// Cache settings
    pub cache: CacheConfig,

    /// Alerting settings
    pub alerting: AlertingConfig,

    /// Service settings
    pub service: ServiceConfig,

    /// Backtest settings
    pub backtest: BacktestConfig,

    /// Validation settings
    pub validation: ValidationConfig,
}

/// Account configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountsConfigSchema {
    /// Default account (used when no account_id specified)
    pub default: DefaultAccountConfig,
    /// Additional simulation accounts
    #[serde(default)]
    pub simulation: Vec<SimulationAccountConfig>,
}

impl Default for AccountsConfigSchema {
    fn default() -> Self {
        Self {
            default: DefaultAccountConfig::default(),
            simulation: Vec::new(),
        }
    }
}

/// Default account configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultAccountConfig {
    /// Account identifier
    pub id: String,
    /// Base currency (e.g., "USDT")
    pub currency: String,
    /// Initial balance as string (to preserve decimal precision)
    pub initial_balance: String,
}

impl Default for DefaultAccountConfig {
    fn default() -> Self {
        Self {
            id: "DEFAULT".to_string(),
            currency: "USDT".to_string(),
            initial_balance: "10000".to_string(),
        }
    }
}

/// Simulation account configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationAccountConfig {
    /// Account identifier
    pub id: String,
    /// Base currency
    pub currency: String,
    /// Initial balance as string
    pub initial_balance: String,
    /// Strategy IDs that use this account
    #[serde(default)]
    pub strategies: Vec<String>,
}

/// Paper trading configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PaperTradingConfig {
    /// Enable paper trading
    pub enabled: bool,
    /// Strategy to use for paper trading
    pub strategy: String,
    /// Initial capital for paper trading
    pub initial_capital: f64,
}

impl Default for PaperTradingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: "rsi".to_string(),
            initial_capital: 10000.0,
        }
    }
}

/// Built-in strategy parameters.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BuiltinStrategiesConfig {
    /// RSI strategy parameters
    pub rsi: RsiStrategyConfig,
    /// SMA strategy parameters
    pub sma: SmaStrategyConfig,
}

/// RSI strategy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RsiStrategyConfig {
    /// RSI calculation period
    pub period: u32,
    /// Oversold threshold (buy signal)
    pub oversold: u32,
    /// Overbought threshold (sell signal)
    pub overbought: u32,
    /// Order quantity
    pub order_quantity: String,
    /// Lookback multiplier
    pub lookback_multiplier: f64,
    /// Lookback buffer
    pub lookback_buffer: u32,
}

impl Default for RsiStrategyConfig {
    fn default() -> Self {
        Self {
            period: 14,
            oversold: 30,
            overbought: 70,
            order_quantity: "100".to_string(),
            lookback_multiplier: 2.0,
            lookback_buffer: 10,
        }
    }
}

/// SMA strategy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SmaStrategyConfig {
    /// Short period moving average
    pub short_period: u32,
    /// Long period moving average
    pub long_period: u32,
    /// Order quantity
    pub order_quantity: String,
    /// Lookback multiplier
    pub lookback_multiplier: f64,
}

impl Default for SmaStrategyConfig {
    fn default() -> Self {
        Self {
            short_period: 5,
            long_period: 20,
            order_quantity: "100".to_string(),
            lookback_multiplier: 2.0,
        }
    }
}

/// Python strategies configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StrategiesConfig {
    /// Directory containing Python strategy files
    pub python_dir: String,
    /// Enable hot-reload for development
    pub hot_reload: bool,
    /// Hot-reload configuration
    pub hot_reload_config: HotReloadConfig,
    /// Registered Python strategies
    #[serde(default)]
    pub python: Vec<PythonStrategyConfig>,
}

/// Hot-reload configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotReloadConfig {
    /// Debounce time in milliseconds
    pub debounce_ms: u64,
    /// Skip hash verification (development only)
    pub skip_hash_verification: bool,
}

impl Default for HotReloadConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 300,
            skip_hash_verification: false,
        }
    }
}

/// Python strategy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonStrategyConfig {
    /// Strategy identifier
    pub id: String,
    /// Python file path (relative to python_dir)
    pub file: String,
    /// Python class name
    pub class_name: String,
    /// Strategy description
    pub description: String,
    /// Enable/disable strategy
    pub enabled: bool,
    /// SHA256 hash for verification (write-only from UI perspective)
    #[serde(default)]
    pub sha256: String,
    /// Execution limits
    #[serde(default)]
    pub limits: PythonStrategyLimits,
}

/// Python strategy execution limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PythonStrategyLimits {
    /// Maximum execution time per tick in milliseconds
    pub max_execution_time_ms: u64,
}

impl Default for PythonStrategyLimits {
    fn default() -> Self {
        Self {
            max_execution_time_ms: 10,
        }
    }
}

/// Execution simulation configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExecutionConfig {
    /// Fee configuration
    pub fees: FeesConfig,
    /// Latency simulation
    pub latency: LatencyConfig,
}

/// Fee configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeesConfig {
    /// Default fee model: "zero", "binance_spot", "binance_futures", "custom"
    pub default_model: String,
    /// Binance spot fees
    pub binance_spot: FeeRate,
    /// Binance spot with BNB discount
    pub binance_spot_bnb: FeeRate,
    /// Binance futures fees
    pub binance_futures: FeeRate,
    /// Custom fee rates
    pub custom: FeeRate,
}

impl Default for FeesConfig {
    fn default() -> Self {
        Self {
            default_model: "zero".to_string(),
            binance_spot: FeeRate {
                maker: "0.001".to_string(),
                taker: "0.001".to_string(),
            },
            binance_spot_bnb: FeeRate {
                maker: "0.00075".to_string(),
                taker: "0.00075".to_string(),
            },
            binance_futures: FeeRate {
                maker: "0.0002".to_string(),
                taker: "0.0004".to_string(),
            },
            custom: FeeRate {
                maker: "0.001".to_string(),
                taker: "0.001".to_string(),
            },
        }
    }
}

/// Fee rate (maker/taker).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeRate {
    /// Maker fee rate
    pub maker: String,
    /// Taker fee rate
    pub taker: String,
}

/// Latency simulation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LatencyConfig {
    /// Latency model: "none", "fixed", "variable"
    pub model: String,
    /// Order insert latency in milliseconds
    pub insert_ms: u64,
    /// Order update latency in milliseconds
    pub update_ms: u64,
    /// Order delete latency in milliseconds
    pub delete_ms: u64,
    /// Jitter for variable latency
    pub jitter_ms: u64,
    /// Random seed (0 = random each run)
    pub seed: u64,
}

impl Default for LatencyConfig {
    fn default() -> Self {
        Self {
            model: "none".to_string(),
            insert_ms: 0,
            update_ms: 0,
            delete_ms: 0,
            jitter_ms: 0,
            seed: 0,
        }
    }
}

/// Database configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    /// Maximum database connections
    pub max_connections: u32,
    /// Minimum database connections
    pub min_connections: u32,
    /// Connection max lifetime in seconds
    pub max_lifetime: u64,
    /// Connection acquire timeout in seconds
    pub acquire_timeout_secs: u64,
    /// Idle connection timeout in seconds
    pub idle_timeout_secs: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            max_connections: 8,
            min_connections: 2,
            max_lifetime: 1800,
            acquire_timeout_secs: 30,
            idle_timeout_secs: 600,
        }
    }
}

/// Cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CacheConfig {
    /// In-memory cache settings
    pub memory: MemoryCacheConfig,
    /// Redis cache settings
    pub redis: RedisCacheConfig,
}

/// In-memory cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryCacheConfig {
    /// Maximum ticks per symbol
    pub max_ticks_per_symbol: u32,
    /// TTL in seconds
    pub ttl_seconds: u64,
}

impl Default for MemoryCacheConfig {
    fn default() -> Self {
        Self {
            max_ticks_per_symbol: 1000,
            ttl_seconds: 300,
        }
    }
}

/// Redis cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RedisCacheConfig {
    /// Connection pool size
    pub pool_size: u32,
    /// TTL in seconds
    pub ttl_seconds: u64,
    /// Maximum ticks per symbol
    pub max_ticks_per_symbol: u32,
}

impl Default for RedisCacheConfig {
    fn default() -> Self {
        Self {
            pool_size: 10,
            ttl_seconds: 3600,
            max_ticks_per_symbol: 10000,
        }
    }
}

/// Alerting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertingConfig {
    /// Enable alerting system
    pub enabled: bool,
    /// Evaluation interval in seconds
    pub interval_secs: u64,
    /// Cooldown between repeated alerts in seconds
    pub cooldown_secs: u64,
    /// Pool saturation warning threshold (0.0-1.0)
    pub pool_saturation_threshold: f64,
    /// Pool critical threshold (0.0-1.0)
    pub pool_critical_threshold: f64,
    /// Batch failure rate threshold (0.0-1.0)
    pub batch_failure_threshold: f64,
    /// Channel backpressure threshold (0.0-100.0)
    pub channel_backpressure_threshold: f64,
    /// Reconnection storm threshold
    pub reconnection_storm_threshold: u32,
}

impl Default for AlertingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: 30,
            cooldown_secs: 300,
            pool_saturation_threshold: 0.8,
            pool_critical_threshold: 0.95,
            batch_failure_threshold: 0.2,
            channel_backpressure_threshold: 80.0,
            reconnection_storm_threshold: 10,
        }
    }
}

/// Service configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServiceConfig {
    /// Channel capacity for tick processing
    pub channel_capacity: u32,
    /// Health monitor interval in seconds
    pub health_monitor_interval_secs: u64,
    /// Bar timer interval in seconds
    pub bar_timer_interval_secs: u64,
    /// Reconnection delay in seconds
    pub reconnection_delay_secs: u64,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 10000,
            health_monitor_interval_secs: 30,
            bar_timer_interval_secs: 1,
            reconnection_delay_secs: 5,
        }
    }
}

/// Backtest configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BacktestConfig {
    /// Default commission rate
    pub default_commission_rate: String,
    /// Default number of records
    pub default_records: u32,
    /// Default initial capital
    pub default_capital: String,
    /// Minimum records required
    pub minimum_records: u32,
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            default_commission_rate: "0.001".to_string(),
            default_records: 10000,
            default_capital: "10000".to_string(),
            minimum_records: 100,
        }
    }
}

/// Validation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationConfig {
    /// Enable tick validation
    pub enabled: bool,
    /// Maximum price change percentage
    pub max_price_change_pct: f64,
    /// Timestamp tolerance in minutes
    pub timestamp_tolerance_minutes: u64,
    /// Maximum past days for timestamps
    pub max_past_days: u64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_price_change_pct: 10.0,
            timestamp_tolerance_minutes: 5,
            max_past_days: 3650,
        }
    }
}

/// Sensitive fields that should never be exposed via UI/API.
///
/// These are identified by their section paths.
pub const SENSITIVE_FIELDS: &[&str] = &[
    "database.url",
    "server.host",
    "server.port",
    "metrics.bind_address",
    "transport.ipc.shm_path_prefix",
    "strategies.python.sha256", // Write-only (computed on backend)
];

/// Check if a section path contains sensitive data.
pub fn is_sensitive_section(section: &str) -> bool {
    SENSITIVE_FIELDS.iter().any(|s| section.starts_with(s))
}

impl AppConfig {
    /// Create a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a section as JSON value.
    ///
    /// Supports dot-notation paths like "accounts.default" or "strategies.python".
    pub fn get_section(&self, section: &str) -> Result<serde_json::Value, String> {
        let full = serde_json::to_value(self).map_err(|e| e.to_string())?;
        get_nested_value(&full, section)
    }

    /// Set a section from JSON value.
    ///
    /// Supports dot-notation paths.
    pub fn set_section(&mut self, section: &str, value: serde_json::Value) -> Result<(), String> {
        let mut full = serde_json::to_value(&self).map_err(|e| e.to_string())?;
        set_nested_value(&mut full, section, value)?;
        *self = serde_json::from_value(full).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Add an item to an array section.
    ///
    /// For example, add a Python strategy to "strategies.python".
    pub fn add_item(&mut self, section: &str, item: serde_json::Value) -> Result<(), String> {
        let mut full = serde_json::to_value(&self).map_err(|e| e.to_string())?;
        add_to_array(&mut full, section, item)?;
        *self = serde_json::from_value(full).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Remove an item from an array section by index.
    pub fn remove_item(&mut self, section: &str, index: usize) -> Result<(), String> {
        let mut full = serde_json::to_value(&self).map_err(|e| e.to_string())?;
        remove_from_array(&mut full, section, index)?;
        *self = serde_json::from_value(full).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Update an item in an array section by index.
    pub fn update_item(
        &mut self,
        section: &str,
        index: usize,
        item: serde_json::Value,
    ) -> Result<(), String> {
        let mut full = serde_json::to_value(&self).map_err(|e| e.to_string())?;
        update_array_item(&mut full, section, index, item)?;
        *self = serde_json::from_value(full).map_err(|e| e.to_string())?;
        Ok(())
    }
}

// Helper functions for nested JSON operations

fn get_nested_value(value: &serde_json::Value, path: &str) -> Result<serde_json::Value, String> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in parts {
        current = current
            .get(part)
            .ok_or_else(|| format!("Section '{}' not found in path '{}'", part, path))?;
    }

    Ok(current.clone())
}

fn set_nested_value(
    value: &mut serde_json::Value,
    path: &str,
    new_value: serde_json::Value,
) -> Result<(), String> {
    let parts: Vec<&str> = path.split('.').collect();

    if parts.is_empty() {
        return Err("Empty path".to_string());
    }

    if parts.len() == 1 {
        if let Some(obj) = value.as_object_mut() {
            obj.insert(parts[0].to_string(), new_value);
            return Ok(());
        }
        return Err("Root value is not an object".to_string());
    }

    let mut current = value;
    for part in &parts[..parts.len() - 1] {
        current = current
            .get_mut(*part)
            .ok_or_else(|| format!("Section '{}' not found", part))?;
    }

    if let Some(obj) = current.as_object_mut() {
        obj.insert(parts.last().unwrap().to_string(), new_value);
        Ok(())
    } else {
        Err(format!(
            "Cannot set value: parent is not an object at '{}'",
            path
        ))
    }
}

fn add_to_array(
    value: &mut serde_json::Value,
    path: &str,
    item: serde_json::Value,
) -> Result<(), String> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in &parts {
        current = current
            .get_mut(*part)
            .ok_or_else(|| format!("Section '{}' not found", part))?;
    }

    if let Some(arr) = current.as_array_mut() {
        arr.push(item);
        Ok(())
    } else {
        Err(format!("Section '{}' is not an array", path))
    }
}

fn remove_from_array(
    value: &mut serde_json::Value,
    path: &str,
    index: usize,
) -> Result<(), String> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in &parts {
        current = current
            .get_mut(*part)
            .ok_or_else(|| format!("Section '{}' not found", part))?;
    }

    if let Some(arr) = current.as_array_mut() {
        if index >= arr.len() {
            return Err(format!(
                "Index {} out of bounds for array of length {}",
                index,
                arr.len()
            ));
        }
        arr.remove(index);
        Ok(())
    } else {
        Err(format!("Section '{}' is not an array", path))
    }
}

fn update_array_item(
    value: &mut serde_json::Value,
    path: &str,
    index: usize,
    item: serde_json::Value,
) -> Result<(), String> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in &parts {
        current = current
            .get_mut(*part)
            .ok_or_else(|| format!("Section '{}' not found", part))?;
    }

    if let Some(arr) = current.as_array_mut() {
        if index >= arr.len() {
            return Err(format!(
                "Index {} out of bounds for array of length {}",
                index,
                arr.len()
            ));
        }
        arr[index] = item;
        Ok(())
    } else {
        Err(format!("Section '{}' is not an array", path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert!(config.symbols.is_empty());
        assert!(!config.paper_trading.enabled);
        assert_eq!(config.builtin_strategies.rsi.period, 14);
    }

    #[test]
    fn test_accounts_config_default() {
        let config = AccountsConfigSchema::default();
        assert_eq!(config.default.id, "DEFAULT");
        assert_eq!(config.default.currency, "USDT");
        assert!(config.simulation.is_empty());
    }

    #[test]
    fn test_get_section() {
        let mut config = AppConfig::default();
        config.symbols = vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()];

        let symbols = config.get_section("symbols").unwrap();
        assert!(symbols.is_array());
        assert_eq!(symbols.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_get_nested_section() {
        let config = AppConfig::default();
        let default_account = config.get_section("accounts.default").unwrap();
        assert!(default_account.is_object());
        assert_eq!(default_account["id"], "DEFAULT");
    }

    #[test]
    fn test_set_section() {
        let mut config = AppConfig::default();
        let new_symbols = serde_json::json!(["BTCUSDT", "ETHUSDT", "SOLUSDT"]);
        config.set_section("symbols", new_symbols).unwrap();
        assert_eq!(config.symbols.len(), 3);
    }

    #[test]
    fn test_set_nested_section() {
        let mut config = AppConfig::default();
        let new_default = serde_json::json!({
            "id": "MAIN",
            "currency": "USDC",
            "initial_balance": "50000"
        });
        config.set_section("accounts.default", new_default).unwrap();
        assert_eq!(config.accounts.default.id, "MAIN");
        assert_eq!(config.accounts.default.currency, "USDC");
    }

    #[test]
    fn test_add_item_to_array() {
        let mut config = AppConfig::default();
        let new_account = serde_json::json!({
            "id": "SIM-001",
            "currency": "USDT",
            "initial_balance": "50000",
            "strategies": ["rsi"]
        });
        config.add_item("accounts.simulation", new_account).unwrap();
        assert_eq!(config.accounts.simulation.len(), 1);
        assert_eq!(config.accounts.simulation[0].id, "SIM-001");
    }

    #[test]
    fn test_remove_item_from_array() {
        let mut config = AppConfig::default();
        config.symbols = vec![
            "BTCUSDT".to_string(),
            "ETHUSDT".to_string(),
            "SOLUSDT".to_string(),
        ];

        config.remove_item("symbols", 1).unwrap();
        assert_eq!(config.symbols.len(), 2);
        assert_eq!(config.symbols[0], "BTCUSDT");
        assert_eq!(config.symbols[1], "SOLUSDT");
    }

    #[test]
    fn test_update_item_in_array() {
        let mut config = AppConfig::default();
        config.symbols = vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()];

        config
            .update_item("symbols", 1, serde_json::json!("SOLUSDT"))
            .unwrap();
        assert_eq!(config.symbols[1], "SOLUSDT");
    }

    #[test]
    fn test_remove_item_out_of_bounds() {
        let mut config = AppConfig::default();
        config.symbols = vec!["BTCUSDT".to_string()];

        let result = config.remove_item("symbols", 5);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of bounds"));
    }

    #[test]
    fn test_is_sensitive_section() {
        assert!(is_sensitive_section("database.url"));
        assert!(is_sensitive_section("server.host"));
        assert!(is_sensitive_section("strategies.python.sha256"));
        assert!(!is_sensitive_section("symbols"));
        assert!(!is_sensitive_section("accounts.default"));
    }

    #[test]
    fn test_json_serialization() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.accounts.default.id, config.accounts.default.id);
    }

    #[test]
    fn test_toml_serialization() {
        let config = AppConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.accounts.default.id, config.accounts.default.id);
    }

    #[test]
    fn test_python_strategy_config() {
        let strategy = PythonStrategyConfig {
            id: "my_strategy".to_string(),
            file: "examples/my_strategy.py".to_string(),
            class_name: "MyStrategy".to_string(),
            description: "A test strategy".to_string(),
            enabled: true,
            sha256: "abc123".to_string(),
            limits: PythonStrategyLimits::default(),
        };

        let json = serde_json::to_value(&strategy).unwrap();
        assert_eq!(json["id"], "my_strategy");
        assert_eq!(json["limits"]["max_execution_time_ms"], 10);
    }

    #[test]
    fn test_fee_config_defaults() {
        let config = FeesConfig::default();
        assert_eq!(config.default_model, "zero");
        assert_eq!(config.binance_spot.maker, "0.001");
        assert_eq!(config.binance_futures.taker, "0.0004");
    }
}
