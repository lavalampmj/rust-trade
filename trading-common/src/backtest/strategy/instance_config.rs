//! Strategy instance configuration.
//!
//! Each strategy-symbol pair runs as an independent instance with its own configuration.
//! This module defines the configuration structure for strategy instances.
//!
//! # Example
//!
//! ```ignore
//! use trading_common::backtest::strategy::{StrategyInstanceConfig, StartupBehavior, FillModel};
//! use rust_decimal_macros::dec;
//!
//! let config = StrategyInstanceConfig::new("rsi_strategy", "BTCUSD")
//!     .with_account("SIM-001")
//!     .with_starting_cash(dec!(100000), "USD")
//!     .with_startup_behavior(StartupBehavior::WaitUntilFlat)
//!     .with_minimum_bars(14)
//!     .with_fill_model(FillModel::OnPenetration);
//! ```

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::data::types::{BarDataMode, BarType, Timeframe};

// =============================================================================
// Enums for Configuration Options
// =============================================================================

/// How the strategy should behave on startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupBehavior {
    /// Wait until strategy signals flat (no position) before allowing trades.
    /// Safe for live trading - prevents accidental position accumulation.
    #[default]
    WaitUntilFlat,

    /// Immediately allow order submission without waiting.
    /// Use for backtesting or when you're confident about initial state.
    ImmediateSubmit,

    /// Immediately submit orders and sync with real account positions.
    /// Queries broker for current positions before trading.
    ImmediateSubmitSyncAccount,

    /// Wait until flat AND sync with real account.
    /// Most conservative option for live trading.
    WaitUntilFlatSyncAccount,
}

impl StartupBehavior {
    pub fn requires_account_sync(&self) -> bool {
        matches!(
            self,
            StartupBehavior::ImmediateSubmitSyncAccount | StartupBehavior::WaitUntilFlatSyncAccount
        )
    }

    pub fn requires_flat(&self) -> bool {
        matches!(
            self,
            StartupBehavior::WaitUntilFlat | StartupBehavior::WaitUntilFlatSyncAccount
        )
    }
}

/// How limit orders are filled in simulation/backtest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillModel {
    /// Fill when price touches the limit price.
    /// More optimistic - assumes you get filled at your price.
    #[default]
    OnTouch,

    /// Fill only when price penetrates through the limit price.
    /// More realistic - assumes you need price to move through your level.
    OnPenetration,
}

/// Resolution for order fills in simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillResolution {
    /// Standard bar-based fill resolution.
    /// Orders filled based on OHLC of the bar.
    #[default]
    Standard,

    /// Tick-level resolution for OHLC bars.
    /// More accurate but slower - simulates tick-by-tick within bars.
    TickForOHLC,
}

/// Time in force for orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimeInForce {
    /// Good Till Cancelled - remains active until filled or cancelled.
    #[default]
    GTC,

    /// Immediate Or Cancel - fill immediately or cancel unfilled portion.
    IOC,

    /// Fill Or Kill - fill entire order immediately or cancel completely.
    FOK,

    /// Day order - cancelled at end of trading session.
    DAY,

    /// Good Till Date - remains active until specified date.
    GTD,
}

// =============================================================================
// Bar Specification
// =============================================================================

/// Specification for bar generation and historical data range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarSpec {
    /// Bar type (time-based or tick-based).
    pub bar_type: BarType,

    /// Start date for historical data (None = use all available).
    #[serde(default)]
    pub from_date: Option<DateTime<Utc>>,

    /// End date for historical data (None = up to now).
    #[serde(default)]
    pub to_date: Option<DateTime<Utc>>,
}

impl Default for BarSpec {
    fn default() -> Self {
        Self {
            bar_type: BarType::TimeBased(Timeframe::OneMinute),
            from_date: None,
            to_date: None,
        }
    }
}

impl BarSpec {
    pub fn time_based(timeframe: Timeframe) -> Self {
        Self {
            bar_type: BarType::TimeBased(timeframe),
            from_date: None,
            to_date: None,
        }
    }

    pub fn tick_based(tick_count: u32) -> Self {
        Self {
            bar_type: BarType::TickBased(tick_count),
            from_date: None,
            to_date: None,
        }
    }

    pub fn with_date_range(mut self, from: DateTime<Utc>, to: DateTime<Utc>) -> Self {
        self.from_date = Some(from);
        self.to_date = Some(to);
        self
    }
}

// =============================================================================
// Fill Configuration
// =============================================================================

/// Configuration for order fill simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillConfig {
    /// How limit orders are filled.
    #[serde(default)]
    pub fill_model: FillModel,

    /// Resolution for order fills.
    #[serde(default)]
    pub resolution: FillResolution,

    /// Slippage in minimum price increments (ticks).
    /// 0 = no slippage, 1 = 1 tick slippage, etc.
    #[serde(default)]
    pub slippage_ticks: u32,

    /// Whether to apply slippage to market orders only.
    #[serde(default = "default_true")]
    pub slippage_market_only: bool,
}

fn default_true() -> bool {
    true
}

impl Default for FillConfig {
    fn default() -> Self {
        Self {
            fill_model: FillModel::default(),
            resolution: FillResolution::default(),
            slippage_ticks: 0,
            slippage_market_only: true,
        }
    }
}

// =============================================================================
// Order Defaults
// =============================================================================

/// Default order parameters for the strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDefaults {
    /// Default time in force for orders.
    #[serde(default)]
    pub time_in_force: TimeInForce,

    /// Default order quantity (if not specified by strategy).
    #[serde(default)]
    pub default_quantity: Option<Decimal>,

    /// Maximum position size (risk limit).
    #[serde(default)]
    pub max_position_size: Option<Decimal>,

    /// Maximum number of open orders.
    #[serde(default)]
    pub max_open_orders: Option<u32>,
}

impl Default for OrderDefaults {
    fn default() -> Self {
        Self {
            time_in_force: TimeInForce::default(),
            default_quantity: None,
            max_position_size: None,
            max_open_orders: None,
        }
    }
}

// =============================================================================
// Transform Parameters
// =============================================================================

/// Parameters for a single transform (indicator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformParams {
    /// Transform type (e.g., "SMA", "EMA", "RSI", "ATR").
    pub transform_type: String,

    /// Named parameters for the transform.
    pub params: HashMap<String, TransformValue>,
}

/// Value types for transform parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TransformValue {
    Integer(i64),
    Float(f64),
    Decimal(Decimal),
    String(String),
    Bool(bool),
}

impl TransformValue {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            TransformValue::Integer(v) => Some(*v),
            TransformValue::Float(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            TransformValue::Float(v) => Some(*v),
            TransformValue::Integer(v) => Some(*v as f64),
            TransformValue::Decimal(v) => v.to_string().parse().ok(),
            _ => None,
        }
    }

    pub fn as_usize(&self) -> Option<usize> {
        self.as_i64().and_then(|v| usize::try_from(v).ok())
    }
}

// =============================================================================
// Main Strategy Instance Configuration
// =============================================================================

/// Complete configuration for a strategy-symbol instance.
///
/// Each strategy runs as an independent instance with its own:
/// - Symbol and account binding
/// - Capital allocation
/// - Bar calculation settings
/// - Fill simulation parameters
/// - Transform/indicator parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyInstanceConfig {
    // =========================================================================
    // Core Identity
    // =========================================================================
    /// Strategy identifier (e.g., "rsi_python", "sma_crossover").
    pub strategy_id: String,

    /// Symbol in canonical DBT format (e.g., "BTCUSD", "ETHUSD").
    pub symbol: String,

    /// Account ID for this instance (for multi-account support).
    #[serde(default = "default_account")]
    pub account_id: String,

    // =========================================================================
    // Capital & Currency
    // =========================================================================
    /// Starting cash for this instance.
    #[serde(default = "default_starting_cash")]
    pub starting_cash: Decimal,

    /// Base currency for P&L calculation.
    #[serde(default = "default_currency")]
    pub currency: String,

    // =========================================================================
    // Bar Calculation
    // =========================================================================
    /// When strategy receives bar events.
    #[serde(default)]
    pub bar_data_mode: BarDataMode,

    /// Bar specification (type, size, date range).
    #[serde(default)]
    pub bar_spec: BarSpec,

    // =========================================================================
    // Warmup & Startup
    // =========================================================================
    /// Minimum bars required before strategy can trade.
    /// Strategy will receive data but signals are ignored until warmup completes.
    #[serde(default = "default_minimum_bars")]
    pub minimum_bars_required: usize,

    /// How strategy should behave on startup.
    #[serde(default)]
    pub startup_behavior: StartupBehavior,

    // =========================================================================
    // Fill Simulation
    // =========================================================================
    /// Configuration for order fill simulation.
    #[serde(default)]
    pub fill_config: FillConfig,

    // =========================================================================
    // Order Defaults
    // =========================================================================
    /// Default order parameters.
    #[serde(default)]
    pub order_defaults: OrderDefaults,

    // =========================================================================
    // Transform Parameters
    // =========================================================================
    /// Parameters for each transform/indicator used by the strategy.
    /// Key: transform name (e.g., "rsi", "sma_fast", "sma_slow").
    #[serde(default)]
    pub transforms: HashMap<String, TransformParams>,

    // =========================================================================
    // Metadata
    // =========================================================================
    /// Optional description for this instance.
    #[serde(default)]
    pub description: Option<String>,

    /// Whether this instance is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Custom key-value metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

fn default_account() -> String {
    "DEFAULT".to_string()
}

fn default_starting_cash() -> Decimal {
    Decimal::from(10000)
}

fn default_currency() -> String {
    "USD".to_string()
}

fn default_minimum_bars() -> usize {
    20
}

impl StrategyInstanceConfig {
    /// Create a new strategy instance configuration.
    pub fn new(strategy_id: impl Into<String>, symbol: impl Into<String>) -> Self {
        Self {
            strategy_id: strategy_id.into(),
            symbol: symbol.into(),
            account_id: default_account(),
            starting_cash: default_starting_cash(),
            currency: default_currency(),
            bar_data_mode: BarDataMode::default(),
            bar_spec: BarSpec::default(),
            minimum_bars_required: default_minimum_bars(),
            startup_behavior: StartupBehavior::default(),
            fill_config: FillConfig::default(),
            order_defaults: OrderDefaults::default(),
            transforms: HashMap::new(),
            description: None,
            enabled: true,
            metadata: HashMap::new(),
        }
    }

    // =========================================================================
    // Builder Methods
    // =========================================================================

    /// Set the account ID.
    pub fn with_account(mut self, account_id: impl Into<String>) -> Self {
        self.account_id = account_id.into();
        self
    }

    /// Set starting cash and currency.
    pub fn with_starting_cash(mut self, cash: Decimal, currency: impl Into<String>) -> Self {
        self.starting_cash = cash;
        self.currency = currency.into();
        self
    }

    /// Set the bar data mode.
    pub fn with_bar_data_mode(mut self, mode: BarDataMode) -> Self {
        self.bar_data_mode = mode;
        self
    }

    /// Set the bar specification.
    pub fn with_bar_spec(mut self, spec: BarSpec) -> Self {
        self.bar_spec = spec;
        self
    }

    /// Set minimum bars required for warmup.
    pub fn with_minimum_bars(mut self, bars: usize) -> Self {
        self.minimum_bars_required = bars;
        self
    }

    /// Set startup behavior.
    pub fn with_startup_behavior(mut self, behavior: StartupBehavior) -> Self {
        self.startup_behavior = behavior;
        self
    }

    /// Set fill model.
    pub fn with_fill_model(mut self, model: FillModel) -> Self {
        self.fill_config.fill_model = model;
        self
    }

    /// Set slippage in ticks.
    pub fn with_slippage(mut self, ticks: u32) -> Self {
        self.fill_config.slippage_ticks = ticks;
        self
    }

    /// Set default time in force.
    pub fn with_time_in_force(mut self, tif: TimeInForce) -> Self {
        self.order_defaults.time_in_force = tif;
        self
    }

    /// Set maximum position size.
    pub fn with_max_position(mut self, size: Decimal) -> Self {
        self.order_defaults.max_position_size = Some(size);
        self
    }

    /// Add a transform configuration.
    pub fn with_transform(
        mut self,
        name: impl Into<String>,
        transform_type: impl Into<String>,
        params: HashMap<String, TransformValue>,
    ) -> Self {
        self.transforms.insert(
            name.into(),
            TransformParams {
                transform_type: transform_type.into(),
                params,
            },
        );
        self
    }

    /// Add an integer transform parameter.
    pub fn with_transform_param_int(
        mut self,
        transform_name: impl Into<String>,
        transform_type: impl Into<String>,
        param_name: impl Into<String>,
        value: i64,
    ) -> Self {
        let name = transform_name.into();
        let entry = self.transforms.entry(name).or_insert_with(|| TransformParams {
            transform_type: transform_type.into(),
            params: HashMap::new(),
        });
        entry.params.insert(param_name.into(), TransformValue::Integer(value));
        self
    }

    /// Set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set enabled state.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Add custom metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    // =========================================================================
    // Accessors
    // =========================================================================

    /// Get a unique instance ID (strategy_id:symbol:account_id).
    pub fn instance_id(&self) -> String {
        format!("{}:{}:{}", self.strategy_id, self.symbol, self.account_id)
    }

    /// Get transform parameter as integer.
    pub fn get_transform_param_int(&self, transform: &str, param: &str) -> Option<i64> {
        self.transforms
            .get(transform)
            .and_then(|t| t.params.get(param))
            .and_then(|v| v.as_i64())
    }

    /// Get transform parameter as usize (for periods).
    pub fn get_transform_param_usize(&self, transform: &str, param: &str) -> Option<usize> {
        self.transforms
            .get(transform)
            .and_then(|t| t.params.get(param))
            .and_then(|v| v.as_usize())
    }

    /// Get transform parameter as f64.
    pub fn get_transform_param_f64(&self, transform: &str, param: &str) -> Option<f64> {
        self.transforms
            .get(transform)
            .and_then(|t| t.params.get(param))
            .and_then(|v| v.as_f64())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_basic_config() {
        let config = StrategyInstanceConfig::new("rsi_strategy", "BTCUSD");

        assert_eq!(config.strategy_id, "rsi_strategy");
        assert_eq!(config.symbol, "BTCUSD");
        assert_eq!(config.account_id, "DEFAULT");
        assert_eq!(config.starting_cash, dec!(10000));
        assert_eq!(config.currency, "USD");
        assert!(config.enabled);
    }

    #[test]
    fn test_builder_pattern() {
        let config = StrategyInstanceConfig::new("sma_crossover", "ETHUSD")
            .with_account("SIM-001")
            .with_starting_cash(dec!(50000), "EUR")
            .with_minimum_bars(30)
            .with_startup_behavior(StartupBehavior::ImmediateSubmit)
            .with_fill_model(FillModel::OnPenetration)
            .with_slippage(2)
            .with_max_position(dec!(100))
            .with_description("SMA crossover strategy for ETH");

        assert_eq!(config.account_id, "SIM-001");
        assert_eq!(config.starting_cash, dec!(50000));
        assert_eq!(config.currency, "EUR");
        assert_eq!(config.minimum_bars_required, 30);
        assert_eq!(config.startup_behavior, StartupBehavior::ImmediateSubmit);
        assert_eq!(config.fill_config.fill_model, FillModel::OnPenetration);
        assert_eq!(config.fill_config.slippage_ticks, 2);
        assert_eq!(config.order_defaults.max_position_size, Some(dec!(100)));
        assert_eq!(config.description, Some("SMA crossover strategy for ETH".to_string()));
    }

    #[test]
    fn test_instance_id() {
        let config = StrategyInstanceConfig::new("rsi", "BTCUSD")
            .with_account("AGGRESSIVE");

        assert_eq!(config.instance_id(), "rsi:BTCUSD:AGGRESSIVE");
    }

    #[test]
    fn test_transform_params() {
        let mut params = HashMap::new();
        params.insert("period".to_string(), TransformValue::Integer(14));
        params.insert("overbought".to_string(), TransformValue::Float(70.0));

        let config = StrategyInstanceConfig::new("rsi", "BTCUSD")
            .with_transform("rsi", "RSI", params);

        assert_eq!(config.get_transform_param_int("rsi", "period"), Some(14));
        assert_eq!(config.get_transform_param_usize("rsi", "period"), Some(14));
        assert_eq!(config.get_transform_param_f64("rsi", "overbought"), Some(70.0));
        assert_eq!(config.get_transform_param_int("rsi", "nonexistent"), None);
    }

    #[test]
    fn test_startup_behavior_methods() {
        assert!(!StartupBehavior::WaitUntilFlat.requires_account_sync());
        assert!(StartupBehavior::WaitUntilFlat.requires_flat());

        assert!(StartupBehavior::ImmediateSubmitSyncAccount.requires_account_sync());
        assert!(!StartupBehavior::ImmediateSubmitSyncAccount.requires_flat());

        assert!(StartupBehavior::WaitUntilFlatSyncAccount.requires_account_sync());
        assert!(StartupBehavior::WaitUntilFlatSyncAccount.requires_flat());
    }

    #[test]
    fn test_bar_spec() {
        let spec = BarSpec::time_based(Timeframe::FiveMinutes);
        assert_eq!(spec.bar_type, BarType::TimeBased(Timeframe::FiveMinutes));

        let spec = BarSpec::tick_based(100);
        assert_eq!(spec.bar_type, BarType::TickBased(100));
    }

    #[test]
    fn test_serialization() {
        let config = StrategyInstanceConfig::new("test", "BTCUSD")
            .with_starting_cash(dec!(25000), "USD");

        let json = serde_json::to_string(&config).unwrap();
        let parsed: StrategyInstanceConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.strategy_id, "test");
        assert_eq!(parsed.symbol, "BTCUSD");
        assert_eq!(parsed.starting_cash, dec!(25000));
    }
}
